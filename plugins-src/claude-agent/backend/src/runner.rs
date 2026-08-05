//! The CLI detach path. A CLI subcommand runs inside a throwaway headless app
//! instance (src-tauri/src/cli/runner.rs:82): a single invoke is capped at 300
//! seconds, and the instance exiting takes the child process with it — neither
//! works for a sweep that runs for ten minutes.
//!
//! So by default the work is handed to this binary's own runner mode: setsid'd
//! into its own session, it outlives the headless instance, and the plugin can
//! return a run id immediately.
use crate::{discover, engine, prompt, record, task};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub vault: PathBuf,
    pub task_id: String,
    pub prompt: String,
    pub run_id: String,
}

/// Start a detached runner. Returns the `{run_id, status}` the CLI prints.
pub fn spawn_detached(
    vault: &Path,
    task_id: &str,
    user_prompt: &str,
) -> Result<serde_json::Value, String> {
    let run_id = record::new_run_id(chrono::Utc::now(), std::process::id());
    let run_dir = task::runs_root(vault)
        .join(task_id)
        .join("pending")
        .join(&run_id);
    std::fs::create_dir_all(&run_dir).map_err(|e| e.to_string())?;
    let req = Request {
        vault: vault.to_path_buf(),
        task_id: task_id.into(),
        prompt: user_prompt.into(),
        run_id: run_id.clone(),
    };
    std::fs::write(
        run_dir.join("request.json"),
        serde_json::to_string(&req).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--runner")
        .arg(&run_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn()
        .map_err(|e| format!("failed to start runner: {e}"))?;
    Ok(serde_json::json!({ "run_id": run_id, "status": "started" }))
}

/// The runner process body. Returns the process exit code.
pub async fn run(run_dir: PathBuf) -> i32 {
    let Ok(body) = std::fs::read_to_string(run_dir.join("request.json")) else {
        return 2;
    };
    let Ok(req) = serde_json::from_str::<Request>(&body) else {
        return 2;
    };

    let task_dir = task::task_dir(&req.vault, &req.task_id);
    let Some(mut def) = task::read_task(&task_dir) else {
        return 2;
    };
    def.id = req.task_id.clone();
    let Some(claude) = discover::discover(std::env::var("NOTEMD_CLAUDE_BIN").ok().as_deref())
    else {
        return 3;
    };

    let spec = engine::RunSpec {
        vault: req.vault.clone(),
        prompt: prompt::compose(&def.prompt, &req.prompt, None),
        task: def,
        task_dir,
        task_run_dir: task::runs_root(&req.vault).join(&req.task_id),
        claude,
        env_path: None,
        trigger: "cli".into(),
        run_id: req.run_id.clone(),
        oauth_token: std::env::var("CLAUDE_CODE_OAUTH_TOKEN").ok(),
        // A CLI run is a whole-vault pass; the precheck decides from the vault.
        target: None,
        // The CLI names no single output file (no reminder to earn), so there
        // is nothing to declare; artifacts still cover output/ and answers/.
        deliverable: None,
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_cancel_tx, cancel_rx) = mpsc::channel(1);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let code = match engine::run(spec, tx, cancel_rx).await {
        Ok(()) => 0,
        // The same task was already running.
        Err(_busy) => 4,
    };
    let _ = drain.await;
    let _ = std::fs::remove_dir_all(&run_dir);
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_detached_writes_a_request_the_runner_can_read_back() {
        let v = tempfile::tempdir().unwrap();
        let out = spawn_detached(v.path(), "selfcheck", "extra").unwrap();
        let run_id = out["run_id"].as_str().unwrap();
        assert_eq!(out["status"], "started");
        let p = task::runs_root(v.path())
            .join("selfcheck")
            .join("pending")
            .join(run_id)
            .join("request.json");
        let req: Request = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        assert_eq!(req.task_id, "selfcheck");
        assert_eq!(req.prompt, "extra");
        assert_eq!(req.run_id, run_id);
    }

    #[tokio::test]
    async fn runner_exits_with_a_code_when_the_request_is_unreadable() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(run(d.path().to_path_buf()).await, 2);
    }

    #[tokio::test]
    async fn runner_refuses_an_unknown_task() {
        let d = tempfile::tempdir().unwrap();
        let v = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("request.json"),
            serde_json::to_string(&Request {
                vault: v.path().to_path_buf(),
                task_id: "nope".into(),
                prompt: String::new(),
                run_id: "r".into(),
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(run(d.path().to_path_buf()).await, 2);
    }
}
