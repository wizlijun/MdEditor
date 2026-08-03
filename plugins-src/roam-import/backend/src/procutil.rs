//! Shared child-process helper: spawn, poll `try_wait` until exit or a
//! deadline, kill on timeout. Every subprocess this crate spawns — `roam
//! --version`/`roam list-graphs` (`roam_cli::run`), and the login-shell
//! `command -v roam` lookup (`discover::shell_lookup`) — goes through this,
//! so none of them can block the plugin's protocol read loop past its
//! budget: `on_ui_request` runs `roam_cli::probe` synchronously on that
//! loop (see plugin.rs), and the host's default `ui.request` timeout is 30s.
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<Output, String> {
    let program = cmd.get_program().to_string_lossy().to_string();
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start {program}: {e}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} timed out after {}s", timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    child.wait_with_output().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_output_of_a_fast_command() {
        let mut cmd = Command::new("/bin/echo");
        cmd.arg("hi");
        let out = run_with_timeout(cmd, Duration::from_secs(5)).unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hi");
    }

    #[test]
    fn kills_and_errors_on_timeout() {
        let mut cmd = Command::new("/bin/sleep");
        cmd.arg("5");
        let started = Instant::now();
        let err = run_with_timeout(cmd, Duration::from_millis(150)).unwrap_err();
        assert!(err.contains("timed out"), "unexpected error: {err}");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "must not block for anywhere near the child's own 5s sleep"
        );
    }
}
