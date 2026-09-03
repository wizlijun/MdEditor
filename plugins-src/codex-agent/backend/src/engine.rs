//! One Codex CLI run: spawn `codex exec --json`, map JSONL events, and land the
//! shared note.md run record.
use crate::{argv, discover, stream};
use agent_run_core::event::{Event, RunResult, Step};
use agent_run_core::record;
use agent_run_core::scaffold::{self, Blocked, ProgressTracker, RunMeta};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

pub struct RunSpec {
    pub meta: RunMeta,
    pub codex: PathBuf,
    /// `PATH` for the child. `None` uses the login-shell enriched runtime PATH.
    pub env_path: Option<String>,
    pub prompt: String,
    /// One of Codex CLI's native sandbox names.
    pub sandbox: String,
    /// The model resolved from the task override or Codex's effective Vault
    /// configuration. Passed explicitly so execution and OKF provenance agree.
    pub model: String,
    /// Optional invocation-scoped credential. Stored CLI auth remains the
    /// default and no credential is ever written into the vault.
    pub api_key: Option<String>,
}

/// The OKF producer for files this run creates. Callers fail closed when Codex
/// cannot resolve a model, so provenance never needs a placeholder.
pub fn actor(model: &str) -> String {
    format!("codex/{}", model.trim())
}

/// Any message on `cancel` terminates the whole Codex process group.
pub async fn run(
    spec: RunSpec,
    tx: mpsc::UnboundedSender<Step>,
    mut cancel: mpsc::Receiver<()>,
) -> Result<(), record::RunRecord> {
    let started = match scaffold::preflight(&spec.meta).await {
        Ok(s) => s,
        Err(Blocked::Busy(who)) => {
            return Err(record::RunRecord {
                run_id: who.run_id,
                task: spec.meta.task.id.clone(),
                trigger: spec.meta.trigger.clone(),
                started_at: who.started_at,
                ended_at: String::new(),
                status: record::Status::Error,
                exit_code: None,
                num_turns: None,
                session_id: None,
                result: String::new(),
                stderr_tail: String::new(),
                artifacts: Vec::new(),
                harness: Some(crate::SELF_PLUGIN_ID.into()),
                usage: None,
            });
        }
        Err(Blocked::Skip(reason)) => {
            let rec = scaffold::finalize_without_run(
                &spec.meta,
                chrono::Utc::now(),
                record::Status::Skipped,
                reason,
            );
            let _ = tx.send(Step::Done(rec));
            return Ok(());
        }
    };

    let args = argv::build(Some(&spec.model), &spec.meta.vault, &spec.sandbox);
    let mut cmd = tokio::process::Command::new(&spec.codex);
    cmd.args(&args)
        .current_dir(&spec.meta.vault)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env(
            "PATH",
            spec.env_path.clone().unwrap_or_else(discover::runtime_path),
        );
    if let Some(key) = &spec.api_key {
        cmd.env("CODEX_API_KEY", key);
    }
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            let rec = scaffold::finalize_scoped(
                &spec.meta,
                &started,
                record::Status::Error,
                None,
                None,
                format!("spawn failed ({}): {e}", spec.codex.display()),
                String::new(),
                &actor(&spec.model),
                false,
            );
            let _ = tx.send(Step::Done(rec));
            return Ok(());
        }
    };
    let pgid = child.id().unwrap_or(0) as i32;

    // Codex reads the prompt until EOF. Write it once, then close stdin so the
    // turn can start; never interpolate prompt text into a shell command.
    let mut stdin = child.stdin.take().expect("piped stdin");
    let quiet_limit = std::time::Duration::from_secs(spec.meta.task.timeout_seconds);
    let input = tokio::select! {
        result = stdin.write_all(spec.prompt.as_bytes()) => result.map_err(|e| e.to_string()),
        _ = tokio::time::sleep(quiet_limit) => Err("timed out while sending the prompt to Codex".into()),
        _ = cancel.recv() => Err("cancelled while sending the prompt to Codex".into()),
    };
    if let Err(e) = input {
        kill_group(pgid, libc::SIGTERM);
        if tokio::time::timeout(std::time::Duration::from_secs(1), child.wait())
            .await
            .is_err()
        {
            force_kill(&mut child, pgid).await;
        }
        let status = if e.starts_with("cancelled") {
            record::Status::Cancelled
        } else if e.starts_with("timed out") {
            record::Status::Timeout
        } else {
            record::Status::Error
        };
        let rec = scaffold::finalize_scoped(
            &spec.meta,
            &started,
            status,
            None,
            None,
            format!("could not send prompt to Codex: {e}"),
            String::new(),
            &actor(&spec.model),
            false,
        );
        let _ = tx.send(Step::Done(rec));
        return Ok(());
    }
    let _ = stdin.shutdown().await;
    drop(stdin);

    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let mut lines = BufReader::new(stdout).lines();
    let err_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let eb = err_buf.clone();
    let err_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut buf = eb.lock().unwrap();
            buf.push_str(&line);
            buf.push('\n');
            *buf = record::tail(&buf, record::STDERR_LIMIT * 2);
        }
    });

    let mut progress =
        ProgressTracker::start(&spec.meta.task_run_dir, &spec.meta.run_id, started.started);
    let mut parser = stream::StreamState::new(&spec.model);
    let mut noise = String::new();
    let deadline = tokio::time::sleep(quiet_limit);
    tokio::pin!(deadline);

    let forced = loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    deadline.as_mut().reset(tokio::time::Instant::now() + quiet_limit);
                    let events = parser.accept(&line);
                    if events.is_empty() {
                        noise.push_str(&line);
                        noise.push('\n');
                        noise = record::tail(&noise, record::STDERR_LIMIT * 2);
                    }
                    for event in events {
                        match &event {
                            Event::Text { text } => progress.step(text, text),
                            Event::ToolUse { name, brief } => {
                                let label = if brief.is_empty() { name.clone() } else { format!("{name} {brief}") };
                                progress.step(&label, &label);
                            }
                            Event::System { .. } | Event::Permission { .. } | Event::Result(_) => {}
                        }
                        let _ = tx.send(Step::Event(event));
                    }
                    if parser.is_terminal() {
                        break None;
                    }
                }
                _ => break None,
            },
            _ = &mut deadline => {
                kill_group(pgid, libc::SIGTERM);
                break Some(record::Status::Timeout);
            }
            _ = cancel.recv() => {
                kill_group(pgid, libc::SIGTERM);
                break Some(record::Status::Cancelled);
            }
        }
    };

    let (exit, exit_wait_timed_out) =
        match tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await {
            Ok(Ok(status)) => (status.code(), false),
            Ok(Err(_)) => (None, false),
            Err(_) => {
                force_kill(&mut child, pgid).await;
                (None, true)
            }
        };
    if forced.is_some() {
        // The group leader may have honored TERM while a descendant kept the
        // inherited stderr fd open. Escalate the entire session, not just the
        // child handle, before draining diagnostics.
        kill_group(pgid, libc::SIGKILL);
    }
    let mut err_task = err_task;
    if tokio::time::timeout(std::time::Duration::from_secs(1), &mut err_task)
        .await
        .is_err()
    {
        kill_group(pgid, libc::SIGKILL);
        err_task.abort();
        let _ = err_task.await;
    }
    let mut stderr_tail = record::tail(&err_buf.lock().unwrap(), record::STDERR_LIMIT);
    if stderr_tail.is_empty() && parser.result().is_none() {
        stderr_tail = record::tail(&noise, record::STDERR_LIMIT);
    }

    let parsed = parser.result();
    let missing_terminal = parsed.is_none() && forced.is_none();
    let result = parsed.or_else(|| {
        missing_terminal.then(|| RunResult {
            is_error: true,
            result: format!(
                "Codex exited{} without a terminal JSONL event",
                exit.map(|c| format!(" with code {c}")).unwrap_or_default()
            ),
            session_id: parser.thread_id().map(str::to_string),
            num_turns: None,
            usage: None,
        })
    });
    let status = forced.unwrap_or(match (&result, exit) {
        (Some(r), Some(0)) if !r.is_error => record::Status::Success,
        (Some(r), None) if !r.is_error && exit_wait_timed_out => record::Status::Success,
        _ => record::Status::Error,
    });
    let rec = scaffold::finalize_scoped(
        &spec.meta,
        &started,
        status,
        exit,
        result,
        String::new(),
        stderr_tail,
        &actor(&spec.model),
        false,
    );
    let _ = tx.send(Step::Done(rec));
    Ok(())
}

fn kill_group(pgid: i32, signal: i32) {
    if pgid > 0 {
        unsafe {
            libc::killpg(pgid, signal);
        }
    }
}

/// Reap a process after the protocol has already reached a terminal state.
/// Neither Tokio's kill nor wait is allowed to become an unbounded second hang.
async fn force_kill(child: &mut tokio::process::Child, pgid: i32) {
    kill_group(pgid, libc::SIGKILL);
    let _ = child.start_kill();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), child.wait()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_run_core::task::TaskDef;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn fake(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("codex");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn spec(root: &Path, codex: PathBuf, timeout: u64) -> RunSpec {
        let task_dir = root.join(".notemd/agent-tasks/t");
        std::fs::create_dir_all(&task_dir).unwrap();
        RunSpec {
            meta: RunMeta {
                vault: root.into(),
                task: TaskDef {
                    id: "t".into(),
                    name: "T".into(),
                    description: String::new(),
                    prompt: "p".into(),
                    max_turns: None,
                    timeout_seconds: timeout,
                    model: Some("gpt-test".into()),
                    precheck: None,
                    okf_type: None,
                    directive: Vec::new(),
                    source_plugin: None,
                },
                task_dir,
                task_run_dir: root.join(".notemd/agent-runs/t"),
                run_id: "r1".into(),
                trigger: "window".into(),
                harness: crate::SELF_PLUGIN_ID.into(),
                target: None,
                deliverable: None,
            },
            codex,
            env_path: Some("/usr/bin:/bin".into()),
            prompt: "PROMPT SENT OVER STDIN".into(),
            sandbox: "workspace-write".into(),
            model: "gpt-test".into(),
            api_key: None,
        }
    }

    async fn record_from(spec: RunSpec, cancel: mpsc::Receiver<()>) -> record::RunRecord {
        let (tx, mut rx) = mpsc::unbounded_channel();
        run(spec, tx, cancel).await.unwrap();
        let mut last = None;
        while let Ok(step) = rx.try_recv() {
            if let Step::Done(rec) = step {
                last = Some(rec);
            }
        }
        last.expect("terminal record")
    }

    #[tokio::test]
    async fn fake_codex_runs_end_to_end_and_records_the_thread() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(
            d.path(),
            "prompt=$(cat); [ \"$prompt\" = 'PROMPT SENT OVER STDIN' ] || exit 9\n\
             [ -d .notemd/agent-tasks/t ] || exit 8\n\
             echo '{\"type\":\"thread.started\",\"thread_id\":\"thr-1\"}'\n\
             echo '{\"type\":\"turn.started\"}'\n\
             echo '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"done\"}}'\n\
             echo '{\"type\":\"turn.completed\",\"usage\":{}}'",
        );
        let (_cancel_tx, cancel_rx) = mpsc::channel(1);
        let rec = record_from(spec(d.path(), bin, 5), cancel_rx).await;
        assert_eq!(rec.status, record::Status::Success);
        assert_eq!(rec.result, "done");
        assert_eq!(rec.session_id.as_deref(), Some("thr-1"));
        assert_eq!(rec.harness.as_deref(), Some(crate::SELF_PLUGIN_ID));
    }

    #[tokio::test]
    async fn failed_turn_becomes_an_error_record() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(
            d.path(),
            "cat >/dev/null\n\
             echo '{\"type\":\"thread.started\",\"thread_id\":\"thr-2\"}'\n\
             echo '{\"type\":\"turn.failed\",\"error\":{\"message\":\"401 Unauthorized\"}}'\n\
             exit 1",
        );
        let (_cancel_tx, cancel_rx) = mpsc::channel(1);
        let rec = record_from(spec(d.path(), bin, 5), cancel_rx).await;
        assert_eq!(rec.status, record::Status::Error);
        assert!(rec.result.contains("Unauthorized"));
    }

    #[tokio::test]
    async fn cancellation_kills_a_silent_run() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(d.path(), "cat >/dev/null; sleep 30");
        let (cancel_tx, cancel_rx) = mpsc::channel(1);
        let spec = spec(d.path(), bin, 60);
        let task = tokio::spawn(record_from(spec, cancel_rx));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        cancel_tx.send(()).await.unwrap();
        let rec = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rec.status, record::Status::Cancelled);
    }

    #[tokio::test]
    async fn cancellation_also_interrupts_a_blocked_prompt_write() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(d.path(), "sleep 30");
        let (cancel_tx, cancel_rx) = mpsc::channel(1);
        let mut spec = spec(d.path(), bin, 60);
        spec.prompt = "x".repeat(2 * 1024 * 1024);
        let task = tokio::spawn(record_from(spec, cancel_rx));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        cancel_tx.send(()).await.unwrap();
        let rec = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(rec.status, record::Status::Cancelled);
    }

    #[tokio::test]
    async fn an_inherited_stderr_fd_cannot_wedge_finalization() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(
            d.path(),
            "cat >/dev/null\n\
             (trap '' TERM; sleep 30) &\n\
             echo '{\"type\":\"thread.started\",\"thread_id\":\"thr-bg\"}'\n\
             echo '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"done\"}}'\n\
             echo '{\"type\":\"turn.completed\",\"usage\":{}}'",
        );
        let (_cancel_tx, cancel_rx) = mpsc::channel(1);
        let rec = tokio::time::timeout(
            std::time::Duration::from_secs(7),
            record_from(spec(d.path(), bin, 10), cancel_rx),
        )
        .await
        .expect("stderr drain must be bounded");
        assert_eq!(rec.status, record::Status::Success);
    }

    #[tokio::test]
    async fn a_completed_turn_stays_successful_when_codex_lingers_during_shutdown() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(
            d.path(),
            "cat >/dev/null\n\
             echo '{\"type\":\"thread.started\",\"thread_id\":\"thr-slow-exit\"}'\n\
             echo '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"delivered\"}}'\n\
             echo '{\"type\":\"turn.completed\",\"usage\":{}}'\n\
             trap '' TERM
             kill -STOP $$",
        );
        let (_cancel_tx, cancel_rx) = mpsc::channel(1);
        let rec = tokio::time::timeout(
            std::time::Duration::from_secs(7),
            record_from(spec(d.path(), bin, 10), cancel_rx),
        )
        .await
        .expect("terminal completion must bound process cleanup");
        assert_eq!(rec.status, record::Status::Success);
        assert_eq!(rec.result, "delivered");
        assert_eq!(rec.exit_code, None);
    }

    #[tokio::test]
    async fn a_completed_turn_with_an_explicit_nonzero_exit_is_still_an_error() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(
            d.path(),
            "cat >/dev/null\n\
             echo '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"claimed done\"}}'\n\
             echo '{\"type\":\"turn.completed\",\"usage\":{}}'\n\
             exit 7",
        );
        let (_cancel_tx, cancel_rx) = mpsc::channel(1);
        let rec = record_from(spec(d.path(), bin, 5), cancel_rx).await;
        assert_eq!(rec.status, record::Status::Error);
        assert_eq!(rec.exit_code, Some(7));
    }

    #[tokio::test]
    async fn a_silent_run_hits_the_quiet_timeout() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(d.path(), "cat >/dev/null; sleep 30");
        let (_cancel_tx, cancel_rx) = mpsc::channel(1);
        let rec = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            record_from(spec(d.path(), bin, 1), cancel_rx),
        )
        .await
        .unwrap();
        assert_eq!(rec.status, record::Status::Timeout);
    }

    #[test]
    fn actor_uses_the_effective_model() {
        assert_eq!(actor("gpt-5.6-sol"), "codex/gpt-5.6-sol");
    }
}
