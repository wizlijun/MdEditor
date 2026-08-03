//! Locate the `roam` executable (@roam-research/roam-cli). A GUI-spawned
//! process inherits a lean PATH with none of the user's shell additions, so a
//! plain `Command::new("roam")` fails for most installs. Three tiers:
//! explicit override → login-shell lookup → well-known install locations.
use std::path::{Path, PathBuf};
use std::process::Command;

/// Well-known install locations, in priority order.
pub fn candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin/roam"),
        PathBuf::from("/usr/local/bin/roam"),
        home.join(".local/bin/roam"),
        home.join(".npm-global/bin/roam"),
        home.join(".volta/bin/roam"),
    ]
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

fn shell_lookup() -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = Command::new(shell).args(["-l", "-i", "-c", "command -v roam"]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
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
}
