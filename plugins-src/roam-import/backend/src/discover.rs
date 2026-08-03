//! Locate the `roam` executable (@roam-research/roam-cli). A GUI-spawned
//! process inherits a lean PATH with none of the user's shell additions, so a
//! plain `Command::new("roam")` fails for most installs. Three tiers:
//! explicit override → login-shell lookup → well-known install locations.
//!
//! Finding the executable is only half the problem: `roam` itself is a Node
//! script behind `#!/usr/bin/env node`, so *running* it needs `node` on
//! `$PATH` too — the same lean GUI PATH that defeats `Command::new("roam")`
//! also defeats `env node`. `roam_cli::run` asks this module for an
//! augmented PATH to spawn with, built from the same three tiers.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

/// Well-known install directories, in priority order.
pub fn well_known_dirs(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        home.join(".local/bin"),
        home.join(".npm-global/bin"),
        home.join(".volta/bin"),
    ]
}

/// Well-known install locations, in priority order.
pub fn candidates(home: &Path) -> Vec<PathBuf> {
    well_known_dirs(home).into_iter().map(|d| d.join("roam")).collect()
}

/// Pure core, injectable for tests.
pub fn discover_with(
    explicit: Option<&str>,
    home: &Path,
    shell_lookup: impl Fn() -> Option<PathBuf>,
    is_exec: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if let Some(p) = explicit.filter(|s| !s.is_empty()) {
        let p = PathBuf::from(p);
        if is_exec(&p) {
            return Some(p);
        }
    }
    if let Some(p) = shell_lookup() {
        if is_exec(&p) {
            return Some(p);
        }
    }
    candidates(home).into_iter().find(|c| is_exec(c))
}

/// Production entry. `-l -i` are both needed: a login shell alone misses rc-file
/// PATH additions (nvm/volta live there).
pub fn discover(explicit: Option<&str>) -> Option<PathBuf> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    discover_with(explicit, &home, shell_lookup, is_executable)
}

/// A wedged/slow login shell must not hang the caller — 5s is plenty for
/// `command -v roam`. Bounded via `procutil::run_with_timeout`, the same
/// spawn/poll/kill loop `roam_cli::run` uses, so there's exactly one
/// implementation of "run a child process with a hard deadline" in the crate.
fn shell_lookup() -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = Command::new(shell);
    cmd.args(["-l", "-i", "-c", "command -v roam"]);
    let out = crate::procutil::run_with_timeout(cmd, Duration::from_secs(5)).ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// Pure core: the PATH to spawn `roam` with. `discovered` is the login
/// shell's `$PATH` (colon-separated), when the probe succeeded; `fallback`
/// is a list of directories to use instead when it didn't; `inherited` is
/// this process's own `$PATH`. Whichever source wins is prepended to
/// `inherited`, deduplicated, first occurrence wins, order preserved.
pub fn compose_path(discovered: Option<&str>, fallback: &[PathBuf], inherited: Option<&str>) -> String {
    fn entries(s: &str) -> impl Iterator<Item = &str> {
        s.split(':').map(str::trim).filter(|s| !s.is_empty())
    }
    fn push(s: &str, seen: &mut HashSet<String>, parts: &mut Vec<String>) {
        if seen.insert(s.to_string()) {
            parts.push(s.to_string());
        }
    }

    let discovered_entries: Vec<&str> = discovered.map(entries).into_iter().flatten().collect();

    let mut seen = HashSet::new();
    let mut parts: Vec<String> = Vec::new();

    if discovered_entries.is_empty() {
        for dir in fallback {
            push(&dir.to_string_lossy(), &mut seen, &mut parts);
        }
    } else {
        for e in discovered_entries {
            push(e, &mut seen, &mut parts);
        }
    }

    for e in inherited.map(entries).into_iter().flatten() {
        push(e, &mut seen, &mut parts);
    }

    parts.join(":")
}

/// Login shell's `$PATH`, probed once and cached for the process lifetime —
/// a login shell is expensive to spawn and the value does not change under
/// us. `None` if the probe failed or the shell reported nothing usable.
pub fn cached_login_shell_path() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE.get_or_init(login_shell_path).clone()
}

fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = Command::new(shell);
    cmd.args(["-l", "-i", "-c", "echo $PATH"]);
    let out = crate::procutil::run_with_timeout(cmd, Duration::from_secs(5)).ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// Production entry for `roam_cli::run`: the PATH to spawn `roam` with,
/// combining the cached login-shell probe (or the well-known fallback
/// directories) with whatever PATH this process already inherited.
pub fn augmented_path(home: &Path) -> String {
    let discovered = cached_login_shell_path();
    let inherited = std::env::var("PATH").ok();
    compose_path(discovered.as_deref(), &well_known_dirs(home), inherited.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn home() -> PathBuf { PathBuf::from("/Users/x") }

    #[test]
    fn explicit_path_wins_when_executable() {
        let got = discover_with(Some("/opt/roam"), &home(), || None, |p| p == Path::new("/opt/roam"));
        assert_eq!(got, Some(PathBuf::from("/opt/roam")));
    }

    #[test]
    fn explicit_path_ignored_when_not_executable() {
        let got = discover_with(
            Some("/opt/roam"), &home(),
            || Some(PathBuf::from("/usr/local/bin/roam")),
            |p| p == Path::new("/usr/local/bin/roam"),
        );
        assert_eq!(got, Some(PathBuf::from("/usr/local/bin/roam")));
    }

    #[test]
    fn falls_back_to_well_known_locations() {
        let got = discover_with(None, &home(), || None, |p| p == Path::new("/opt/homebrew/bin/roam"));
        assert_eq!(got, Some(PathBuf::from("/opt/homebrew/bin/roam")));
    }

    #[test]
    fn returns_none_when_nothing_is_executable() {
        assert_eq!(discover_with(None, &home(), || None, |_| false), None);
    }

    fn fallback() -> Vec<PathBuf> {
        well_known_dirs(&home())
    }

    #[test]
    fn compose_path_prepends_discovered_and_keeps_inherited() {
        let got = compose_path(Some("/discovered/bin"), &fallback(), Some("/usr/bin:/bin"));
        assert_eq!(got, "/discovered/bin:/usr/bin:/bin");
    }

    #[test]
    fn compose_path_falls_back_to_well_known_locations_when_discovered_is_empty() {
        let got = compose_path(Some(""), &fallback(), Some("/usr/bin"));
        assert!(got.starts_with("/opt/homebrew/bin:/usr/local/bin:"));
        assert!(got.ends_with(":/usr/bin"));
    }

    #[test]
    fn compose_path_falls_back_to_well_known_locations_when_discovered_is_whitespace_only() {
        let got = compose_path(Some("   "), &fallback(), Some("/usr/bin"));
        assert!(got.starts_with("/opt/homebrew/bin:/usr/local/bin:"));
    }

    #[test]
    fn compose_path_falls_back_to_well_known_locations_when_discovered_is_none() {
        let got = compose_path(None, &fallback(), Some("/usr/bin"));
        assert!(got.starts_with("/opt/homebrew/bin:/usr/local/bin:"));
    }

    #[test]
    fn compose_path_handles_missing_inherited_path() {
        let got = compose_path(Some("/discovered/bin"), &fallback(), None);
        assert_eq!(got, "/discovered/bin");
    }

    #[test]
    fn compose_path_handles_both_missing() {
        let got = compose_path(None, &[], None);
        assert_eq!(got, "");
    }

    #[test]
    fn compose_path_deduplicates_overlap_between_discovered_and_inherited() {
        let got = compose_path(Some("/opt/homebrew/bin:/usr/bin"), &fallback(), Some("/usr/bin:/bin"));
        assert_eq!(got, "/opt/homebrew/bin:/usr/bin:/bin");
    }

    #[test]
    fn compose_path_deduplicates_within_discovered_itself() {
        let got = compose_path(Some("/usr/bin:/usr/bin"), &fallback(), Some("/usr/bin"));
        assert_eq!(got, "/usr/bin");
    }

    #[test]
    fn fallback_list_reaches_homebrew_node() {
        assert!(fallback().contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[test]
    fn candidates_still_point_at_roam_inside_well_known_dirs() {
        assert_eq!(candidates(&home())[0], PathBuf::from("/opt/homebrew/bin/roam"));
    }
}
