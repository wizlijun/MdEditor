//! Thin wrapper over the `roam` CLI (@roam-research/roam-cli). Every argument
//! is program-constructed — nothing is ever handed to a shell.
use serde::Serialize;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    /// No `roam` executable anywhere.
    Missing,
    /// Executable found, but `roam connect` has never been run on this machine.
    NotConnected,
    /// Executable found and at least one graph is configured.
    Ready,
}

#[derive(Debug, Clone, Serialize)]
pub struct Probe {
    pub state: ProbeState,
    /// Absolute path of the executable we found, when we found one.
    pub found: Option<String>,
    pub version: Option<String>,
    pub graphs: Vec<String>,
}

/// First semver-looking token in `roam --version` output.
pub fn parse_version(stdout: &str) -> Option<String> {
    regex::Regex::new(r"\d+\.\d+\.\d+")
        .ok()?
        .find(stdout)
        .map(|m| m.as_str().to_string())
}

/// Graph names from `roam list-graphs`. The CLI answers with an error envelope
/// (not a non-zero exit) when no graph is connected, so the JSON must be read.
pub fn graphs_from_list(stdout: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value =
        serde_json::from_str(stdout).map_err(|e| format!("unreadable list-graphs output: {e}"))?;
    if let Some(code) = v.pointer("/error/code").and_then(|c| c.as_str()) {
        return Err(code.to_string());
    }
    let arr = v
        .as_array()
        .or_else(|| v.get("graphs").and_then(|g| g.as_array()))
        .ok_or_else(|| "list-graphs did not return an array".to_string())?;
    Ok(arr
        .iter()
        .filter_map(|g| {
            g.get("graph")
                .or_else(|| g.get("nickname"))
                .and_then(|s| s.as_str())
                .or_else(|| g.as_str())
        })
        .map(|s| s.to_string())
        .collect())
}

/// Run `roam <args>` with a hard timeout, returning stdout. stderr is folded
/// into the error so an authorization failure is visible to the user.
pub fn run(exe: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(exe)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                return Err(format!("roam timed out after {}s", timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { stdout } else { err });
    }
    Ok(stdout)
}

/// Full status for the UI's three-state banner.
pub fn probe(explicit: Option<&str>) -> Probe {
    let Some(exe) = crate::discover::discover(explicit) else {
        return Probe { state: ProbeState::Missing, found: None, version: None, graphs: vec![] };
    };
    let version = run(&exe, &["--version"], Duration::from_secs(20))
        .ok()
        .and_then(|s| parse_version(&s));
    let graphs = run(&exe, &["list-graphs"], Duration::from_secs(20))
        .ok()
        .and_then(|s| graphs_from_list(&s).ok())
        .unwrap_or_default();
    let state = if graphs.is_empty() { ProbeState::NotConnected } else { ProbeState::Ready };
    Probe { state, found: Some(exe.display().to_string()), version, graphs }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_version() {
        assert_eq!(parse_version("0.9.2\n"), Some("0.9.2".to_string()));
    }

    #[test]
    fn parses_version_with_prefix() {
        assert_eq!(parse_version("roam-cli 1.0.0\n"), Some("1.0.0".to_string()));
    }

    #[test]
    fn version_of_garbage_is_none() {
        assert_eq!(parse_version("command not found"), None);
    }

    #[test]
    fn config_not_found_is_an_error() {
        let out = r#"{"error":{"code":"CONFIG_NOT_FOUND","message":"No graphs configured."}}"#;
        assert_eq!(graphs_from_list(out), Err("CONFIG_NOT_FOUND".to_string()));
    }

    #[test]
    fn reads_graph_names_from_array() {
        let out = r#"[{"graph":"bruce","nickname":"bruce"},{"graph":"work","nickname":"w"}]"#;
        assert_eq!(graphs_from_list(out), Ok(vec!["bruce".to_string(), "work".to_string()]));
    }

    #[test]
    fn reads_graph_names_from_wrapped_object() {
        let out = r#"{"graphs":[{"graph":"bruce"}]}"#;
        assert_eq!(graphs_from_list(out), Ok(vec!["bruce".to_string()]));
    }
}
