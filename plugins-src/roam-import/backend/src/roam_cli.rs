//! Thin wrapper over the `roam` CLI (@roam-research/roam-cli). Every argument
//! is program-constructed — nothing is ever handed to a shell.
use serde::Serialize;
use std::path::{Path, PathBuf};
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
/// into the error so an authorization failure is visible to the user. The
/// spawn/poll/kill mechanics live in `procutil::run_with_timeout`, shared
/// with `discover::shell_lookup` so there's exactly one wait loop.
///
/// `roam` itself is a Node script behind `#!/usr/bin/env node` — a GUI-spawned
/// process's lean PATH finds the executable (via `discover`) but not `node`,
/// so `env` fails to resolve the interpreter. We spawn with an augmented PATH
/// (login-shell PATH, or the well-known fallback dirs, prepended to whatever
/// this process inherited) so `node` is reachable the same way `roam` was.
pub fn run(exe: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    cmd.env("PATH", crate::discover::augmented_path(&home));
    let out = crate::procutil::run_with_timeout(cmd, timeout)?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { stdout } else { err });
    }
    Ok(stdout)
}

/// Full status for the UI's three-state banner. `--version`/`list-graphs`
/// are local operations, so 10s each is generous; both calls chained plus
/// the (separately 5s-bounded) shell lookup inside `discover::discover`
/// must stay comfortably under the host's default 30s `ui.request` timeout
/// (`on_ui_request` runs this synchronously — see plugin.rs) and under this
/// plugin's own `request_timeout_seconds: 120` manifest override.
pub fn probe(explicit: Option<&str>) -> Probe {
    let Some(exe) = crate::discover::discover(explicit) else {
        return Probe { state: ProbeState::Missing, found: None, version: None, graphs: vec![] };
    };
    let version = run(&exe, &["--version"], Duration::from_secs(10))
        .ok()
        .and_then(|s| parse_version(&s));
    let graphs = run(&exe, &["list-graphs"], Duration::from_secs(10))
        .ok()
        .and_then(|s| graphs_from_list(&s).ok())
        .unwrap_or_default();
    let state = if graphs.is_empty() { ProbeState::NotConnected } else { ProbeState::Ready };
    Probe { state, found: Some(exe.display().to_string()), version, graphs }
}

/// The recursive pull that returns a whole daily page in Roam-export shape.
pub fn day_query(uid: &str) -> String {
    format!(
        r#"[:find (pull ?e [:node/title :block/uid :block/string :block/order :block/heading [:create/time :as "create-time"] [:edit/time :as "edit-time"] {{:block/children ...}}]) :where [?e :block/uid "{uid}"]]"#
    )
}

/// Fetch one daily page. `graph` is optional — the CLI auto-selects when only
/// one graph is configured.
pub fn fetch_day(exe: &Path, graph: Option<&str>, uid: &str) -> Result<serde_json::Value, String> {
    let query = day_query(uid);
    let mut args: Vec<&str> = vec!["datalog-query", "--query", &query];
    if let Some(g) = graph.filter(|g| !g.is_empty()) {
        args.push("--graph");
        args.push(g);
    }
    let out = run(exe, &args, Duration::from_secs(60))?;
    serde_json::from_str(&out).map_err(|e| format!("unreadable datalog output: {e}"))
}

/// Block dimension: max `:edit/time` across a page's blocks, filtered
/// server-side to strictly-after `?since`. Catches content edits; misses a
/// page renamed or created without its blocks changing.
pub fn changed_blocks_query() -> String {
    r#"[:find ?uid (max ?t) :keys uid edited :in $ ?since
    :where [?p :block/uid ?uid] [?p :node/title _]
           [?b :block/page ?p] [?b :edit/time ?t] [(> ?t ?since)]]"#
        .to_string()
}

/// Page-entity dimension: the page's own `:edit/time`, filtered server-side
/// to strictly-after `?since`. Catches renames and page creation; for a
/// daily note this timestamp is the moment of creation, so it misses almost
/// every content edit.
pub fn changed_pages_query() -> String {
    r#"[:find ?uid ?t :keys uid edited :in $ ?since
    :where [?p :node/title _] [?p :block/uid ?uid]
           [?p :edit/time ?t] [(> ?t ?since)]]"#
        .to_string()
}

/// Run both change-discovery queries against the graph, `?since` bound via
/// `--inputs` so the filtering happens server-side and the result sets stay
/// small. `graph` is optional — the CLI auto-selects when only one graph is
/// configured, same as `fetch_day`.
pub fn fetch_changed(
    exe: &Path,
    graph: Option<&str>,
    since_ms: i64,
) -> Result<(serde_json::Value, serde_json::Value), String> {
    let inputs = format!("[{since_ms}]");
    let run_one = |query: &str| -> Result<serde_json::Value, String> {
        let mut args: Vec<&str> = vec!["datalog-query", "--query", query, "--inputs", &inputs];
        if let Some(g) = graph.filter(|g| !g.is_empty()) {
            args.push("--graph");
            args.push(g);
        }
        let out = run(exe, &args, Duration::from_secs(60))?;
        serde_json::from_str(&out).map_err(|e| format!("unreadable datalog output: {e}"))
    };
    let blocks_query = changed_blocks_query();
    let pages_query = changed_pages_query();
    let blocks = run_one(&blocks_query)?;
    let pages = run_one(&pages_query)?;
    Ok((blocks, pages))
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

    #[test]
    fn day_query_embeds_the_uid_and_aliases_both_timestamps() {
        let q = day_query("08-02-2026");
        assert!(q.contains(r#"[?e :block/uid "08-02-2026"]"#));
        // Without :as both :create/time and :edit/time collapse onto one "time" key.
        assert!(q.contains(r#"[:create/time :as "create-time"]"#));
        assert!(q.contains(r#"[:edit/time :as "edit-time"]"#));
        // Unbounded recursion: a fixed-depth pattern silently truncates deep outlines.
        assert!(q.contains("{:block/children ...}"));
    }

    #[test]
    fn both_changed_queries_filter_server_side_and_target_the_right_attribute() {
        let b = changed_blocks_query();
        assert!(b.contains(":in $ ?since"));
        assert!(b.contains("[(> ?t ?since)]"));
        assert!(b.contains("[?b :block/page ?p]"), "the block dimension must join through :block/page");
        assert!(b.contains("(max ?t)"));

        let p = changed_pages_query();
        assert!(p.contains(":in $ ?since"));
        assert!(p.contains("[?p :edit/time ?t]"));
        assert!(!p.contains(":block/page"), "the page dimension must NOT join through blocks");
    }
}
