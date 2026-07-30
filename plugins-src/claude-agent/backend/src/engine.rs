//! The run engine: start claude, pump its stream-json into events, handle
//! timeout and cancellation. The window path and the detached runner share it —
//! the only difference is who holds the child process.
use crate::{artifacts, lock, prompt, record, settings, stream, task::TaskDef};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::SystemTime;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

pub struct RunSpec {
    pub vault: PathBuf,
    pub task: TaskDef,
    pub task_dir: PathBuf,
    pub task_run_dir: PathBuf,
    pub claude: PathBuf,
    pub prompt: String,
    pub trigger: String,
    pub run_id: String,
    pub oauth_token: Option<String>,
}

/// Every step the engine emits. The window path turns these into
/// `host.ui.post`; the runner only cares about the terminal one.
#[derive(Debug)]
pub enum Step {
    Event(stream::Event),
    Done(record::RunRecord),
}

/// Run once. Any message on `cancel` terminates the child process group.
/// The task lock is acquired here and held until the run ends, so callers
/// never have to think about releasing it.
pub async fn run(
    spec: RunSpec,
    tx: mpsc::UnboundedSender<Step>,
    mut cancel: mpsc::Receiver<()>,
) -> Result<(), lock::Busy> {
    let started = chrono::Utc::now();
    // Wall-clock start, for telling this run's output/ files from an older
    // run's. Taken before the lock so a file written the instant claude starts
    // still counts.
    let started_at = SystemTime::now();
    let _guard = lock::acquire(
        &spec.task_run_dir,
        lock::LockInfo {
            pid: std::process::id() as i32,
            run_id: spec.run_id.clone(),
            started_at: started.to_rfc3339(),
        },
    )?;

    let _ = settings::materialize(&spec.task_dir, &spec.vault);
    let argv = prompt::build_argv(&spec.task, &spec.prompt);

    let mut cmd = tokio::process::Command::new(&spec.claude);
    cmd.args(&argv)
        // cwd = the task template dir. Claude Code walks UP for CLAUDE.md, so
        // both the vault's conventions and the task's instructions load, and
        // .claude/skills + .mcp.json are discovered relative to it.
        .current_dir(&spec.task_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(t) = &spec.oauth_token {
        cmd.env("CLAUDE_CODE_OAUTH_TOKEN", t);
    }
    // Own process group, so a timeout/cancel can take down claude AND every
    // process it spawned in one signal.
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let rec = finish(
                &spec,
                started,
                record::Status::Error,
                None,
                None,
                format!("spawn failed: {e}"),
                String::new(),
                Vec::new(),
            );
            let _ = record::write(&spec.task_run_dir, &rec);
            let _ = tx.send(Step::Done(rec));
            return Ok(());
        }
    };
    let pgid = child.id().unwrap_or(0) as i32;

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let mut lines = BufReader::new(stdout).lines();

    // stderr is claude's diagnostic noise, not something to show the user —
    // keep only a tail for the failure record.
    let err_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let eb = err_buf.clone();
    let err_task = tokio::spawn(async move {
        let mut el = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = el.next_line().await {
            let mut g = eb.lock().unwrap();
            g.push_str(&l);
            g.push('\n');
            let t = record::tail(&g, record::STDERR_LIMIT * 2);
            *g = t;
        }
    });

    let mut final_result: Option<stream::RunResult> = None;
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(spec.task.timeout_seconds));
    tokio::pin!(deadline);
    let forced = loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(l)) => {
                    if let Some(ev) = stream::parse_line(&l) {
                        if let stream::Event::Result(r) = &ev {
                            final_result = Some(r.clone());
                        }
                        let _ = tx.send(Step::Event(ev));
                    }
                }
                // EOF or a read error: fall through and collect the exit code.
                _ => break None,
            },
            _ = &mut deadline => { kill_group(pgid); break Some(record::Status::Timeout) }
            _ = cancel.recv() => { kill_group(pgid); break Some(record::Status::Cancelled) }
        }
    };

    let exit = child.wait().await.ok().and_then(|s| s.code());
    let _ = err_task.await;
    let stderr_tail = record::tail(&err_buf.lock().unwrap(), record::STDERR_LIMIT);
    let status = forced.unwrap_or_else(|| match (&final_result, exit) {
        (Some(r), _) if r.is_error => record::Status::Error,
        (_, Some(0)) => record::Status::Success,
        _ => record::Status::Error,
    });
    let result_text = final_result.as_ref().map(|r| r.result.clone()).unwrap_or_default();
    let found = artifacts::collect(&spec.vault, &spec.task_dir, &result_text, started_at);
    let rec = finish(
        &spec,
        started,
        status,
        exit,
        final_result,
        String::new(),
        stderr_tail,
        found,
    );
    let _ = record::write(&spec.task_run_dir, &rec);
    let _ = tx.send(Step::Done(rec));
    Ok(())
}

fn kill_group(pgid: i32) {
    if pgid > 0 {
        unsafe {
            libc::killpg(pgid, libc::SIGTERM);
        }
    }
}

fn finish(
    spec: &RunSpec,
    started: chrono::DateTime<chrono::Utc>,
    status: record::Status,
    exit_code: Option<i32>,
    result: Option<stream::RunResult>,
    fallback_err: String,
    stderr_tail: String,
    artifacts: Vec<String>,
) -> record::RunRecord {
    // A spawn failure has no stream and no stderr — carry its message in both
    // fields so neither the window nor the record comes up blank.
    let stderr_tail = if stderr_tail.is_empty() && !fallback_err.is_empty() {
        fallback_err.clone()
    } else {
        stderr_tail
    };
    record::RunRecord {
        run_id: spec.run_id.clone(),
        task: spec.task.id.clone(),
        trigger: spec.trigger.clone(),
        started_at: started.to_rfc3339(),
        ended_at: chrono::Utc::now().to_rfc3339(),
        status,
        exit_code,
        num_turns: result.as_ref().and_then(|r| r.num_turns),
        session_id: result.as_ref().and_then(|r| r.session_id.clone()),
        result: record::tail(
            &result.map(|r| r.result).unwrap_or(fallback_err),
            record::RESULT_LIMIT,
        ),
        stderr_tail,
        artifacts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskDef;
    use std::path::Path;

    /// An executable stand-in for claude. `body` is shell script.
    fn fake_claude(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn spec(dir: &Path, claude: PathBuf, timeout: u64) -> RunSpec {
        let task_dir = dir.join("task");
        std::fs::create_dir_all(&task_dir).unwrap();
        RunSpec {
            vault: dir.to_path_buf(),
            task: TaskDef {
                id: "t".into(),
                name: "T".into(),
                description: String::new(),
                prompt: "p".into(),
                max_turns: None,
                timeout_seconds: timeout,
                model: None,
            },
            task_dir,
            task_run_dir: dir.join("runs-t"),
            claude,
            prompt: "hi".into(),
            trigger: "window".into(),
            run_id: "20260730T000000Z-000001".into(),
            oauth_token: None,
        }
    }

    async fn drive(s: RunSpec) -> (Vec<stream::Event>, record::RunRecord) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_ctx, crx) = mpsc::channel(1);
        run(s, tx, crx).await.unwrap();
        let (mut evs, mut done) = (Vec::new(), None);
        while let Ok(step) = rx.try_recv() {
            match step {
                Step::Event(e) => evs.push(e),
                Step::Done(r) => done = Some(r),
            }
        }
        (evs, done.expect("engine must always emit Done"))
    }

    #[tokio::test]
    async fn streams_events_and_records_success() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(
            d.path(),
            "fake-ok",
            concat!(
                r#"echo '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"a.md"}}]}}'"#,
                "\n",
                r#"echo '{"type":"result","subtype":"success","result":"done","session_id":"s1","num_turns":2,"is_error":false}'"#
            ),
        );
        let (evs, rec) = drive(spec(d.path(), c, 30)).await;
        assert!(matches!(evs[0], stream::Event::ToolUse { .. }));
        assert_eq!(rec.status, record::Status::Success);
        assert_eq!(rec.result, "done");
        assert_eq!(rec.session_id.as_deref(), Some("s1"));
        assert_eq!(rec.num_turns, Some(2));
    }

    #[tokio::test]
    async fn writes_a_record_to_disk_for_every_run() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(d.path(), "fake-rec", r#"echo '{"type":"result","result":"x"}'"#);
        let s = spec(d.path(), c, 30);
        let run_dir = s.task_run_dir.clone();
        drive(s).await;
        assert_eq!(record::recent(&run_dir, 5).len(), 1);
    }

    #[tokio::test]
    async fn a_run_reports_the_markdown_it_produced() {
        let d = tempfile::tempdir().unwrap();
        // The fake writes a report into output/ and names another file in its
        // answer — both are things the window should be able to open.
        let c = fake_claude(
            d.path(),
            "fake-artifacts",
            concat!(
                "mkdir -p output && echo '# report' > output/report.md\n",
                r#"echo '{"type":"result","result":"wrote output/report.md and answers/deep.md","is_error":false}'"#
            ),
        );
        let s = spec(d.path(), c, 30);
        std::fs::create_dir_all(d.path().join("answers")).unwrap();
        std::fs::write(d.path().join("answers/deep.md"), "# deep").unwrap();
        let task_rel = s
            .task_dir
            .strip_prefix(d.path())
            .unwrap()
            .to_string_lossy()
            .to_string();

        let (_e, rec) = drive(s).await;
        assert_eq!(rec.status, record::Status::Success);
        assert_eq!(
            rec.artifacts,
            vec![
                "answers/deep.md".to_string(),
                format!("{task_rel}/output/report.md"),
            ]
        );
    }

    #[tokio::test]
    async fn is_error_true_records_a_failure() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(
            d.path(),
            "fake-err",
            r#"echo '{"type":"result","result":"nope","is_error":true}'"#,
        );
        let (_e, rec) = drive(spec(d.path(), c, 30)).await;
        assert_eq!(rec.status, record::Status::Error);
        assert_eq!(rec.result, "nope");
    }

    #[tokio::test]
    async fn a_nonzero_exit_without_a_result_is_a_failure() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(d.path(), "fake-exit", "echo oops >&2\nexit 7");
        let (_e, rec) = drive(spec(d.path(), c, 30)).await;
        assert_eq!(rec.status, record::Status::Error);
        assert_eq!(rec.exit_code, Some(7));
        assert!(rec.stderr_tail.contains("oops"));
    }

    #[tokio::test]
    async fn a_hung_claude_hits_the_timeout_and_is_killed() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(d.path(), "fake-hang", "sleep 30");
        let (_e, rec) = drive(spec(d.path(), c, 1)).await;
        assert_eq!(rec.status, record::Status::Timeout);
    }

    #[tokio::test]
    async fn cancel_stops_a_running_task() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(d.path(), "fake-slow", "sleep 30");
        let s = spec(d.path(), c, 60);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (ctx, crx) = mpsc::channel(1);
        let h = tokio::spawn(run(s, tx, crx));
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        ctx.send(()).await.unwrap();
        h.await.unwrap().unwrap();
        let mut done = None;
        while let Ok(step) = rx.try_recv() {
            if let Step::Done(r) = step {
                done = Some(r)
            }
        }
        assert_eq!(done.unwrap().status, record::Status::Cancelled);
    }

    #[tokio::test]
    async fn a_missing_claude_binary_records_an_error_instead_of_panicking() {
        let d = tempfile::tempdir().unwrap();
        let (_e, rec) = drive(spec(d.path(), d.path().join("nope"), 30)).await;
        assert_eq!(rec.status, record::Status::Error);
        assert!(rec.stderr_tail.contains("spawn failed"));
    }

    #[tokio::test]
    async fn runs_claude_with_the_task_dir_as_cwd() {
        let d = tempfile::tempdir().unwrap();
        // The fake prints its cwd as the result text.
        let c = fake_claude(
            d.path(),
            "fake-cwd",
            r#"printf '{"type":"result","result":"%s"}\n' "$(pwd)""#,
        );
        let s = spec(d.path(), c, 30);
        let want = s.task_dir.canonicalize().unwrap();
        let (_e, rec) = drive(s).await;
        assert_eq!(PathBuf::from(&rec.result).canonicalize().unwrap(), want);
    }

    #[tokio::test]
    async fn the_same_task_cannot_run_twice_at_once() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(d.path(), "fake-busy", "sleep 5");
        let s1 = spec(d.path(), c.clone(), 60);
        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (_c1, cr1) = mpsc::channel(1);
        let h = tokio::spawn(run(s1, tx1, cr1));
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let (tx2, _rx2) = mpsc::unbounded_channel();
        let (_c2, cr2) = mpsc::channel(1);
        assert!(run(spec(d.path(), c, 60), tx2, cr2).await.is_err());
        h.abort();
    }
}
