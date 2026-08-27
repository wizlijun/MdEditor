//! The run engine: start claude, pump its stream-json into events, handle
//! timeout and cancellation. The window path and the detached runner share it —
//! the only difference is who holds the child process.
use crate::{artifacts, lock, okf, prompt, record, settings, stream, task::TaskDef};
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
    /// `PATH` for the child. `None` asks the login shell — which is what
    /// production wants, and what a test wants to stay out of.
    pub env_path: Option<String>,
    pub prompt: String,
    pub trigger: String,
    /// The plugin id performing this run. Lands in the record so a shared runs
    /// root cannot make this harness's failure look like the other's.
    pub harness: String,
    pub run_id: String,
    pub oauth_token: Option<String>,
    /// The one file this run is about, if any — handed to the precheck script
    /// as NOTEMD_NOTE so it can answer "is there anything to do?" locally.
    pub target: Option<String>,
    /// The file this run was asked to PRODUCE (absolute), if the caller named
    /// one. It usually lives outside `output/`/`answers/`, so it has to be
    /// declared to be collected as an artifact and OKF-stamped at all.
    pub deliverable: Option<PathBuf>,
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
    let _guard = lock::acquire_for_run(
        &spec.task_run_dir,
        &spec.task.id,
        &spec.vault,
        spec.target.as_deref(),
        lock::LockInfo {
            pid: std::process::id() as i32,
            run_id: spec.run_id.clone(),
            started_at: started.to_rfc3339(),
        },
    )?;

    // Ask the task, locally, whether this is worth starting at all.
    if let crate::precheck::Outcome::Skip(reason) = crate::precheck::run(
        &spec.task_dir,
        spec.task.precheck.as_deref(),
        &spec.vault,
        spec.target.as_deref(),
    )
    .await
    {
        let rec = finish(
            &spec,
            started,
            record::Status::Skipped,
            None,
            None,
            reason,
            String::new(),
            Vec::new(),
        );
        let _ = record::write(&spec.task_run_dir, &rec);
        record::clear_progress_for(&spec.task_run_dir, &spec.run_id);
        let _ = tx.send(Step::Done(rec));
        return Ok(());
    }

    // A run aimed at one note gets a policy that only lets it touch that note —
    // the prompt asked nicely and the model grepped the vault anyway. The metas
    // put the run on the ORIGINAL document's directory rather than the vault's
    // snapshot of it, for whichever notes this run can reach.
    let metas = crate::mirror::read_metas(&spec.vault);
    let scope = spec
        .target
        .as_deref()
        .map(|t| settings::Scope::for_note(&spec.vault, std::path::Path::new(t), &metas));
    // Which MCP servers exist is a property of the machine: they are granted in
    // the policy AND named in the prompt, because a tool the model was never
    // told about is a tool it never reaches for.
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let servers = settings::mcp_server_names(&home, &[spec.task_dir.clone(), spec.vault.clone()]);
    let private_settings = spec
        .task_run_dir
        .join("settings")
        .join(format!("{}.json", spec.run_id));
    let has_private_settings = match settings::materialize_to(
        &spec.task_dir,
        &private_settings,
        &spec.vault,
        scope.as_ref(),
        &metas,
        &servers,
    ) {
        Ok(wrote) => wrote,
        Err(e) => {
            let _ = std::fs::remove_file(&private_settings);
            let rec = finish(
                &spec,
                started,
                record::Status::Error,
                None,
                None,
                format!("could not prepare per-run Claude settings: {e}"),
                String::new(),
                Vec::new(),
            );
            let _ = record::write(&spec.task_run_dir, &rec);
            let _ = tx.send(Step::Done(rec));
            return Ok(());
        }
    };
    struct PrivateSettings(Option<PathBuf>);
    impl Drop for PrivateSettings {
        fn drop(&mut self) {
            if let Some(path) = self.0.as_deref() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let _private_settings = PrivateSettings(has_private_settings.then(|| private_settings.clone()));
    let full_prompt = prompt::with_toolbelt(
        &prompt::with_source_context(&spec.prompt, &spec.vault, scope.as_ref()),
        &servers,
    );
    let argv = prompt::build_argv_with_settings(
        &spec.task,
        &full_prompt,
        has_private_settings.then_some(private_settings.as_path()),
    );

    let mut cmd = tokio::process::Command::new(&spec.claude);
    cmd.args(&argv)
        // cwd = the task template dir. Claude Code walks UP for CLAUDE.md, so
        // both the vault's conventions and the task's instructions load, and
        // .claude/skills + .mcp.json are discovered relative to it.
        .current_dir(&spec.task_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // claude spawns stdio MCP servers (`npx …`) itself, and a GUI-launched
        // host inherits a PATH that has none of them.
        .env(
            "PATH",
            spec.env_path
                .clone()
                .unwrap_or_else(crate::discover::runtime_path),
        );
    if let Some(t) = &spec.oauth_token {
        cmd.env("CLAUDE_CODE_OAUTH_TOKEN", t);
    }
    // Own process group, so a timeout/cancel can take down claude AND every
    // process it spawned in one signal.
    unsafe {
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
    let mut progress = record::Progress {
        run_id: spec.run_id.clone(),
        steps: 0,
        last: String::new(),
        updated_at: started.to_rfc3339(),
    };
    record::write_progress(&spec.task_run_dir, &progress);
    // `timeout_seconds` 是**静默上限**,不是总时长上限:每收到一个事件就重新
    // 起算。一本大部头读 40 分钟不是"卡住",而卡住的表现恰恰是不再有输出 ——
    // 拿总时长砍活着的 run,只会在最后一刻把已经快写完的摘要扔掉。
    let quiet_limit = std::time::Duration::from_secs(spec.task.timeout_seconds);
    let deadline = tokio::time::sleep(quiet_limit);
    tokio::pin!(deadline);
    let forced = loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(l)) => {
                    deadline.as_mut().reset(tokio::time::Instant::now() + quiet_limit);
                    if let Some(ev) = stream::parse_line(&l) {
                        match &ev {
                            stream::Event::Result(r) => final_result = Some(r.clone()),
                            // Anyone polling from another process learns what
                            // this run is doing only through this file.
                            stream::Event::ToolUse { name, brief } => {
                                progress.steps += 1;
                                progress.last = if brief.is_empty() {
                                    name.clone()
                                } else {
                                    format!("{name} {brief}")
                                };
                                record::append_log(
                                    &spec.task_run_dir,
                                    &spec.run_id,
                                    &progress.last,
                                );
                                progress.updated_at = chrono::Utc::now().to_rfc3339();
                                record::write_progress(&spec.task_run_dir, &progress);
                            }
                            stream::Event::Text { text } => {
                                progress.steps += 1;
                                progress.last = text.chars().take(80).collect();
                                record::append_log(&spec.task_run_dir, &spec.run_id, text);
                                progress.updated_at = chrono::Utc::now().to_rfc3339();
                                record::write_progress(&spec.task_run_dir, &progress);
                            }
                            // claude has nobody to ask at runtime: it
                            // pre-approves in settings.local.json instead, so
                            // Permission is an ACP-only variant here.
                            stream::Event::System { .. } | stream::Event::Permission { .. } => {}
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
    let mut found = artifacts::collect(
        &spec.vault,
        &spec.task_dir,
        started_at,
        spec.deliverable.as_deref(),
    );
    if lock::scoped_target(&spec.task.id, &spec.vault, spec.target.as_deref()).is_some() {
        let declared = spec
            .deliverable
            .as_deref()
            .and_then(|path| artifacts::vault_relative(&spec.vault, path));
        found.retain(|path| Some(path) == declared.as_ref());
    }
    // 提示词要求 agent 自己写 OKF 头,但那是约束不是保证:漏写就地补上,
    // 免得 vault 里多一份没有 `type` 的文档(§4.1)。已有 frontmatter 的不碰。
    // 声明的目标文件用任务自报的 type(如 Book Summary),其余走默认 Answer ——
    // 摘要写在 <vault>/ssot/ebooks/… 而不是 answers/,以前这道闸够不着它。
    let by = format!("claude-agent/{}", env!("CARGO_PKG_VERSION"));
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let target_rel = spec
        .deliverable
        .as_deref()
        .and_then(|d| artifacts::vault_relative(&spec.vault, d));
    let mut stamped = 0;
    if let Some(rel) = &target_rel {
        stamped += okf::stamp_vault_docs(
            &spec.vault,
            std::slice::from_ref(rel),
            spec.task.okf_type.as_deref().unwrap_or(okf::DEFAULT_TYPE),
            &by,
            &at,
        );
    }
    let rest: Vec<String> = found
        .iter()
        .filter(|r| Some(*r) != target_rel.as_ref())
        .cloned()
        .collect();
    stamped += okf::stamp_vault_docs(&spec.vault, &rest, okf::DEFAULT_TYPE, &by, &at);
    if stamped > 0 {
        eprintln!("[claude-agent] stamped OKF front-matter on {stamped} file(s)");
    }
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
    // The record is the answer from here on; a leftover snapshot would read as
    // a run still in flight.
    record::clear_progress_for(&spec.task_run_dir, &spec.run_id);
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
        harness: Some(spec.harness.clone()),
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
                precheck: None,
                okf_type: None,
                directive: Vec::new(),
            },
            task_dir,
            task_run_dir: dir.join("runs-t"),
            claude,
            env_path: None,
            prompt: "hi".into(),
            trigger: "window".into(),
            harness: "notemd.claude-agent".into(),
            run_id: "20260730T000000Z-000001".into(),
            oauth_token: None,
            target: None,
            deliverable: None,
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
    async fn the_child_runs_with_the_login_shells_path() {
        // Without this, a GUI-launched host hands claude a PATH with no node —
        // and every stdio MCP server silently fails to start.
        let d = tempfile::tempdir().unwrap();
        let seen = d.path().join("path.txt");
        let c = fake_claude(
            d.path(),
            "fake-path",
            &format!(
                "printf '%s' \"$PATH\" > {}\necho '{{\"type\":\"result\",\"result\":\"x\"}}'",
                seen.display()
            ),
        );
        let mut s = spec(d.path(), c, 30);
        s.env_path = Some("/opt/homebrew/bin:/usr/bin".into());
        drive(s).await;
        assert_eq!(
            std::fs::read_to_string(&seen).unwrap(),
            "/opt/homebrew/bin:/usr/bin"
        );
    }

    #[tokio::test]
    async fn writes_a_record_to_disk_for_every_run() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(
            d.path(),
            "fake-rec",
            r#"echo '{"type":"result","result":"x"}'"#,
        );
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
        let d2 = d.path().to_path_buf();
        let c = fake_claude(
            d.path(),
            "fake-artifacts",
            &format!(
                concat!(
                    "mkdir -p output && echo '# report' > output/report.md\n",
                    "mkdir -p {v}/answers && echo '# long' > {v}/answers/long.md\n",
                    r#"echo '{{"type":"result","result":"see docs/prior.md too","is_error":false}}'"#
                ),
                v = d2.display()
            ),
        );
        let s = spec(d.path(), c, 30);
        // Written before the run and merely NAMED in the answer: not a result.
        std::fs::create_dir_all(d.path().join("docs")).unwrap();
        std::fs::write(d.path().join("docs/prior.md"), "# prior").unwrap();
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
                "answers/long.md".to_string(),
                format!("{task_rel}/output/report.md"),
            ]
        );
    }

    /// The declared target is written wherever the caller wants (an ebook
    /// digest sits beside its book, not under answers/), so both the artifact
    /// list and the OKF fallback stamp have to reach it — and the stamp must
    /// use the TASK's type, not the generic Answer.
    #[tokio::test]
    async fn a_declared_deliverable_is_reported_and_okf_stamped_with_the_task_type() {
        let d = tempfile::tempdir().unwrap();
        let summary = d.path().join("ssot/ebooks/b/2026-08-04-summary.md");
        let c = fake_claude(
            d.path(),
            "fake-digest",
            &format!(
                concat!(
                    "mkdir -p {dir} && printf '# 深度工作 — 摘要\\n' > {p}\n",
                    r#"echo '{{"type":"result","result":"done","is_error":false}}'"#
                ),
                dir = summary.parent().unwrap().display(),
                p = summary.display(),
            ),
        );
        let mut s = spec(d.path(), c, 30);
        s.task.okf_type = Some("Book Summary".into());
        s.deliverable = Some(summary.clone());

        let (_e, rec) = drive(s).await;
        assert_eq!(rec.status, record::Status::Success);
        assert_eq!(rec.artifacts, vec!["ssot/ebooks/b/2026-08-04-summary.md"]);
        let got = std::fs::read_to_string(&summary).unwrap();
        assert!(
            got.starts_with("---\ntype: Book Summary\ntitle: \"深度工作 — 摘要\"\ngenerated: { by: claude-agent/"),
            "the model forgot its frontmatter and the fallback did not cover it: {got}"
        );
    }

    #[tokio::test]
    async fn a_scoped_ai_read_does_not_claim_another_runs_shared_output() {
        let d = tempfile::tempdir().unwrap();
        let book = d.path().join("books/b/book.md");
        let summary = d.path().join("books/b/summary.md");
        std::fs::create_dir_all(book.parent().unwrap()).unwrap();
        std::fs::write(&book, "book").unwrap();
        let c = fake_claude(
            d.path(),
            "fake-scoped-artifacts",
            &format!(
                "mkdir -p output && echo '# other' > output/other.md\necho '# mine' > {}\necho '{{\"type\":\"result\",\"result\":\"done\",\"is_error\":false}}'",
                summary.display()
            ),
        );
        let mut s = spec(d.path(), c, 30);
        s.task.id = "ai-read-ebook".into();
        s.target = Some(book.to_string_lossy().into_owned());
        s.deliverable = Some(summary);

        let (_events, rec) = drive(s).await;
        assert_eq!(rec.artifacts, vec!["books/b/summary.md"]);
    }

    /// The model normally writes its own header — the fallback must not
    /// double-stamp it.
    #[tokio::test]
    async fn a_deliverable_that_already_has_front_matter_is_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let summary = d.path().join("ssot/b-summary.md");
        let body = "---\ntype: Book Summary\ntitle: \"x\"\n---\n# x\n";
        let c = fake_claude(
            d.path(),
            "fake-digest-ok",
            &format!(
                concat!(
                    "mkdir -p {dir} && printf '%s' '{body}' > {p}\n",
                    r#"echo '{{"type":"result","result":"done","is_error":false}}'"#
                ),
                dir = summary.parent().unwrap().display(),
                p = summary.display(),
                body = body,
            ),
        );
        let mut s = spec(d.path(), c, 30);
        s.task.okf_type = Some("Book Summary".into());
        s.deliverable = Some(summary.clone());
        drive(s).await;
        assert_eq!(std::fs::read_to_string(&summary).unwrap(), body);
    }

    /// The whole point of a precheck is spending no tokens. If claude still
    /// starts, the feature is decorative.
    #[tokio::test]
    async fn a_failing_precheck_skips_the_run_without_starting_claude() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let ran = d.path().join("claude-ran");
        let c = fake_claude(
            d.path(),
            "fake-should-not-run",
            &format!("touch {}\nexit 0", ran.display()),
        );
        let mut s = spec(d.path(), c, 30);

        let check = s.task_dir.join("precheck.sh");
        std::fs::write(
            &check,
            "#!/bin/sh\necho '这篇手记里没有待答的问题'\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&check, std::fs::Permissions::from_mode(0o755)).unwrap();
        s.task.precheck = Some("precheck.sh".into());

        let (_e, rec) = drive(s).await;
        assert_eq!(rec.status, record::Status::Skipped);
        assert_eq!(rec.result, "这篇手记里没有待答的问题");
        assert!(!ran.exists(), "claude must not have been started");
    }

    #[tokio::test]
    async fn a_passing_precheck_lets_the_run_through() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(
            d.path(),
            "fake-after-check",
            r#"echo '{"type":"result","result":"done","is_error":false}'"#,
        );
        let mut s = spec(d.path(), c, 30);
        let check = s.task_dir.join("precheck.sh");
        std::fs::write(&check, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&check, std::fs::Permissions::from_mode(0o755)).unwrap();
        s.task.precheck = Some("precheck.sh".into());

        let (_e, rec) = drive(s).await;
        assert_eq!(rec.status, record::Status::Success);
    }

    /// Scope has to be enforced where the model cannot argue with it. This
    /// pins that a note-scoped run gets the narrow policy written out before
    /// claude starts.
    #[tokio::test]
    async fn a_note_scoped_run_gets_a_policy_confined_to_that_note() {
        let d = tempfile::tempdir().unwrap();
        let captured = d.path().join("captured-settings.json");
        let c = fake_claude(
            d.path(),
            "fake-scoped",
            &format!(
                "while [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--settings\" ]; then cp \"$2\" \"{}\"; fi\n  shift\ndone\necho '{{\"type\":\"result\",\"result\":\"done\",\"is_error\":false}}'",
                captured.display()
            ),
        );
        let mut s = spec(d.path(), c, 30);
        std::fs::create_dir_all(s.task_dir.join(".claude")).unwrap();
        std::fs::write(
            s.task_dir.join(".claude/settings.json"),
            r#"{"permissions":{"allow":["Read(${VAULT}/**)"]}}"#,
        )
        .unwrap();
        std::fs::write(
            s.task_dir.join(".claude/settings.scoped.json"),
            r#"{"permissions":{"allow":["Read(${NOTE})","Read(${SOURCE})"],"deny":["Grep","Bash"]}}"#,
        )
        .unwrap();
        s.target = Some(format!("{}/docs/a.note.md", d.path().display()));
        let local = s.task_dir.join(".claude/settings.local.json");

        drive(s).await;
        let got = std::fs::read_to_string(&captured).unwrap();
        assert!(
            !local.exists(),
            "the shared local policy must stay untouched"
        );
        assert!(got.contains("docs/a.note.md"), "note not in policy: {got}");
        assert!(got.contains("docs/a.md"), "source not in policy: {got}");
        assert!(
            got.contains("Grep") && got.contains("Bash"),
            "no deny list: {got}"
        );
        // Narrow means "not the whole vault" — the source's own directory IS
        // granted, since that's where the document actually lives.
        assert!(
            !got.contains(&format!("Read({}/**)", d.path().display())),
            "still vault-wide: {got}"
        );
        assert!(
            got.contains(&format!("Read({}/docs/**)", d.path().display())),
            "the source's directory must be readable: {got}"
        );
    }

    #[tokio::test]
    async fn a_run_keeps_a_log_of_what_it_did() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(
            d.path(),
            "fake-log",
            concat!(
                r#"echo '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"a.note.md"}}]}}'"#,
                "\n",
                r#"echo '{"type":"assistant","message":{"content":[{"type":"text","text":"answered it"}]}}'"#,
                "\n",
                r#"echo '{"type":"result","result":"done","is_error":false}'"#
            ),
        );
        let s = spec(d.path(), c, 30);
        let (run_dir, run_id) = (s.task_run_dir.clone(), s.run_id.clone());
        drive(s).await;
        let log = record::read_log(&run_dir, &run_id).expect("the run left a log");
        assert_eq!(log, "Read a.note.md\nanswered it\n");
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

    /// 静默上限 ≠ 总时长上限:只要还在出声,跑多久都不该被腰斩。这里的假 claude
    /// 总共跑 3.5s(超过 3s 的 timeout_seconds),但每 250ms 出一行 —— 必须成功。
    /// 上限给到 3s 是留给并行跑测试时的进程启动抖动:间隔与上限差 12 倍。
    #[tokio::test]
    async fn a_talkative_claude_outlives_the_quiet_limit() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(
            d.path(),
            "fake-chatty",
            "i=0\nwhile [ $i -lt 14 ]; do \
               printf '{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"tick\"}]}}\\n'; \
               sleep 0.25; i=$((i+1)); \
             done\n\
             printf '{\"type\":\"result\",\"result\":\"done\"}\\n'",
        );
        // 整套测试并行跑时,fork+exec(pre_exec 关掉了 posix_spawn 快路)偶尔会
        // 卡在 exec 之前 —— 那种 run 一个字都没吐过,判它超时是对的,不是本例要
        // 测的东西。所以只在"确实出过声"的那次上断言,一次没出声就重试。
        let mut last = None;
        for _ in 0..3 {
            let (evs, rec) = drive(spec(d.path(), c.clone(), 3)).await;
            if evs.is_empty() {
                last = Some(rec);
                continue;
            }
            assert_eq!(
                rec.status,
                record::Status::Success,
                "a streaming run must not be timed out ({} events, {} → {})",
                evs.len(),
                rec.started_at,
                rec.ended_at
            );
            assert_eq!(rec.result, "done");
            return;
        }
        panic!("the fake claude never produced a single event: {last:?}");
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
