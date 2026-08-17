//! The CLI detach handoff. A CLI subcommand runs inside a throwaway headless app
//! instance (`src-tauri/src/cli/runner.rs`): a single invoke is capped at 300
//! seconds, and the instance exiting takes the child process with it — neither
//! works for a sweep that runs for ten minutes.
//!
//! So by default the work is handed to the plugin binary's OWN runner mode:
//! setsid'd into its own session, it outlives the headless instance, and the
//! plugin can return a run id immediately.
//!
//! Only the handoff lives here. What the runner then DOES with the request —
//! which executable to find, which transport to speak — is the plugin's.
use crate::{record, task};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub vault: PathBuf,
    pub task_id: String,
    pub prompt: String,
    pub run_id: String,
}

/// Where one pending handoff parks until its runner picks it up.
pub fn pending_dir(vault: &Path, task_id: &str, run_id: &str) -> PathBuf {
    task::runs_root(vault)
        .join(task_id)
        .join("pending")
        .join(run_id)
}

/// Start a detached runner of `exe`. Returns the `{run_id, status}` the CLI prints.
///
/// `exe` is the plugin's own binary — passed in rather than read from
/// `current_exe()` here so a test can point it at a stand-in.
pub fn spawn_detached_with(
    exe: &Path,
    vault: &Path,
    task_id: &str,
    user_prompt: &str,
) -> Result<serde_json::Value, String> {
    task::check_task_id(task_id)?;
    let run_id = record::new_run_id(chrono::Utc::now(), std::process::id());
    let run_dir = pending_dir(vault, task_id, &run_id);
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

/// Production entry: hand off to this very binary.
pub fn spawn_detached(
    vault: &Path,
    task_id: &str,
    user_prompt: &str,
) -> Result<serde_json::Value, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    spawn_detached_with(&exe, vault, task_id, user_prompt)
}

/// Read back what `spawn_detached` parked. `None` = unreadable or malformed,
/// which the runner reports as a non-zero exit rather than guessing.
pub fn read_request(run_dir: &Path) -> Option<Request> {
    serde_json::from_str(&std::fs::read_to_string(run_dir.join("request.json")).ok()?).ok()
}

/// Drop the handoff directory once the run has ended. The RECORD is the durable
/// artifact; leaving the request behind would look like a run still pending.
pub fn cleanup(run_dir: &Path) {
    let _ = std::fs::remove_dir_all(run_dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_detached_writes_a_request_the_runner_can_read_back() {
        let v = tempfile::tempdir().unwrap();
        // `true(1)` stands in for the plugin binary: spawned and immediately done.
        let out = spawn_detached_with(Path::new("/usr/bin/true"), v.path(), "selfcheck", "extra")
            .unwrap();
        let run_id = out["run_id"].as_str().unwrap();
        assert_eq!(out["status"], "started");

        let req = read_request(&pending_dir(v.path(), "selfcheck", run_id)).unwrap();
        assert_eq!(req.task_id, "selfcheck");
        assert_eq!(req.prompt, "extra");
        assert_eq!(req.run_id, run_id);
        assert_eq!(req.vault, v.path());
    }

    /// Same fence as every other entry point: the id is joined onto the runs
    /// root, so a traversing one would park a request outside the vault.
    #[test]
    fn a_traversing_task_id_is_refused_before_anything_is_written() {
        let v = tempfile::tempdir().unwrap();
        let e = spawn_detached_with(Path::new("/usr/bin/true"), v.path(), "../../evil", "")
            .unwrap_err();
        assert!(e.contains("invalid task id"), "err: {e}");
        assert!(!task::runs_root(v.path()).exists());
    }

    #[test]
    fn a_missing_or_broken_request_reads_as_none() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read_request(d.path()), None);
        std::fs::write(d.path().join("request.json"), "{not json").unwrap();
        assert_eq!(read_request(d.path()), None);
    }

    #[test]
    fn cleanup_removes_the_handoff_and_is_safe_to_repeat() {
        let v = tempfile::tempdir().unwrap();
        let out =
            spawn_detached_with(Path::new("/usr/bin/true"), v.path(), "t", "p").unwrap();
        let dir = pending_dir(v.path(), "t", out["run_id"].as_str().unwrap());
        assert!(dir.exists());
        cleanup(&dir);
        assert!(!dir.exists());
        cleanup(&dir);
    }

    #[test]
    fn a_failure_to_spawn_is_reported_rather_than_panicking() {
        let v = tempfile::tempdir().unwrap();
        let e = spawn_detached_with(Path::new("/nonexistent/runner"), v.path(), "t", "")
            .unwrap_err();
        assert!(e.contains("failed to start runner"), "err: {e}");
    }
}
