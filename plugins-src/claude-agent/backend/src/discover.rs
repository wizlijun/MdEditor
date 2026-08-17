//! Locate the `claude` executable. The mechanism (explicit path → login-shell
//! lookup → well-known locations, plus the cached login `PATH`) is shared and
//! lives in `agent-run-core`; only the binary name and this harness's install
//! locations are claude's own.
use agent_run_core::discover as core;
use std::path::{Path, PathBuf};

const BIN: &str = "claude";

/// Well-known install locations, in priority order.
pub fn candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".claude/local/claude"),
        home.join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
}

pub fn discover(explicit: Option<&str>) -> Option<PathBuf> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    core::discover_with(
        explicit,
        &candidates(&home),
        || core::probe(BIN).0,
        core::is_executable,
    )
}

/// The `PATH` a spawned claude should see. claude itself pulls up stdio MCP
/// servers (`npx …`), and a GUI-launched host inherits a PATH that has none.
pub fn runtime_path() -> String {
    core::runtime_path(BIN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_claude_local_install_over_homebrew() {
        let home = Path::new("/home/u");
        let got = core::discover_with(None, &candidates(home), || None, |_| true);
        assert_eq!(got, Some(home.join(".claude/local/claude")));
    }

    #[test]
    fn every_candidate_is_an_absolute_claude_path() {
        for c in candidates(Path::new("/home/u")) {
            assert!(c.is_absolute(), "{c:?}");
            assert_eq!(c.file_name().unwrap(), "claude");
        }
    }
}
