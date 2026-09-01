//! Where a vault is allowed to live.
//!
//! Picking a vault root is not a neutral choice: `vault_sync` runs
//! `git add -A` + commit over the whole tree on every cycle, `agents_sync`
//! can write the default collaboration files into it, and the folder view
//! walks it. Point that at a
//! drive root and note.md starts version-controlling an entire disk; point it
//! at the home directory and the auto-commit sweeps up `.ssh`, `.aws` and every
//! other secret that happens to live there — and then pushes them to the
//! configured remote.
//!
//! So the picker gates on this before persisting anything. The rules are
//! deliberately few and absolute: each rejected location is one where *no*
//! user intent makes the outcome acceptable. Anything merely unusual (a second
//! drive, a network share below its root, a cloud-sync folder) is allowed —
//! this is a guard rail, not a taste filter.

use std::path::{Component, Path, PathBuf};

/// Why a directory may not be used as a vault root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    /// Missing, or not a directory.
    NotADirectory,
    /// `/`, `C:\`, or a bare UNC share (`\\server\share`).
    FilesystemRoot,
    /// The user's home directory itself. (Anything *below* it is fine.)
    HomeDirectory,
    /// The OS's own directories, or anything inside them.
    SystemDirectory,
}

impl Reject {
    /// Key for `crate::menu_label`, so the message follows the app locale.
    pub fn i18n_key(self) -> &'static str {
        match self {
            Reject::NotADirectory => "vault.reject.notADirectory",
            Reject::FilesystemRoot => "vault.reject.filesystemRoot",
            Reject::HomeDirectory => "vault.reject.homeDirectory",
            Reject::SystemDirectory => "vault.reject.systemDirectory",
        }
    }
}

/// True when `p` has no `Normal` component — i.e. it is nothing but a prefix
/// and/or a root.
///
/// Covers every spelling in one rule: `/`, `C:\`, a bare `C:`, and `\\server\share`
/// (a UNC share root is exactly as bad a vault as a drive root). `D:\vault`,
/// `D:\vault\` and `\\server\share\vault` all have a `Normal` component and pass.
fn is_filesystem_root(p: &Path) -> bool {
    !p.components().any(|c| matches!(c, Component::Normal(_)))
}

/// Resolve for comparison. `canonicalize` is best-effort: it fails on paths
/// that do not exist, and on Windows it returns verbatim (`\\?\C:\…`) form —
/// which is exactly why both sides of every comparison go through here.
fn resolved(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Directories the OS owns. Rejecting these (and their contents) is about
/// preventing an obviously-wrong choice, not about enforcing permissions —
/// the filesystem already does that.
fn system_roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        ["SystemRoot", "ProgramFiles", "ProgramFiles(x86)", "ProgramData"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect()
    }
    #[cfg(not(windows))]
    {
        ["/System", "/Library", "/usr", "/bin", "/sbin", "/etc", "/var", "/Applications"]
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect()
    }
}

/// Testable core. `is_dir` is passed in so the rules can be exercised against
/// paths that do not exist on the test machine.
pub fn check_with(
    path: &Path,
    is_dir: bool,
    home: Option<&Path>,
    system_roots: &[PathBuf],
) -> Result<(), Reject> {
    if !is_dir {
        return Err(Reject::NotADirectory);
    }
    // Checked on the path as given: `is_filesystem_root` is a structural
    // question, and canonicalizing first would not change the answer.
    if is_filesystem_root(path) {
        return Err(Reject::FilesystemRoot);
    }
    let target = resolved(path);
    if let Some(home) = home {
        if target == resolved(home) {
            return Err(Reject::HomeDirectory);
        }
    }
    for root in system_roots {
        if target.starts_with(resolved(root)) {
            return Err(Reject::SystemDirectory);
        }
    }
    Ok(())
}

/// Gate a candidate vault root against the real environment.
pub fn check(path: &Path) -> Result<(), Reject> {
    check_with(
        path,
        path.is_dir(),
        dirs::home_dir().as_deref(),
        &system_roots(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(path: &str, is_dir: bool) -> Result<(), Reject> {
        check_with(Path::new(path), is_dir, None, &[])
    }

    #[test]
    fn rejects_a_missing_or_non_directory_path() {
        assert_eq!(no_env("/some/file.md", false), Err(Reject::NotADirectory));
    }

    // `is_filesystem_root` asks `std::path` to parse the path, and that parse is
    // itself platform-dependent: on Unix a backslash is an ordinary character, so
    // `C:\` is ONE `Normal` component and is correctly not a root *there*. Only
    // the Windows build can judge Windows spellings, hence the split — asserting
    // them on macOS was testing the host's path parser, not our rule.
    #[test]
    fn rejects_the_posix_filesystem_root() {
        assert_eq!(no_env("/", true), Err(Reject::FilesystemRoot));
    }

    #[cfg(windows)]
    #[test]
    fn rejects_every_windows_spelling_of_a_filesystem_root() {
        for p in ["C:\\", "C:", "D:/", "\\\\server\\share"] {
            assert_eq!(no_env(p, true), Err(Reject::FilesystemRoot), "{p} should be rejected");
        }
    }

    #[test]
    fn accepts_an_ordinary_directory_on_any_volume() {
        for p in [
            "/Users/me/vault",
            "D:\\vault",
            "D:\\vault\\",
            "\\\\server\\share\\vault",
        ] {
            assert_eq!(no_env(p, true), Ok(()), "{p} should be accepted");
        }
    }

    #[test]
    fn rejects_the_home_directory_but_not_a_child_of_it() {
        let home = Path::new("/Users/me");
        assert_eq!(
            check_with(home, true, Some(home), &[]),
            Err(Reject::HomeDirectory)
        );
        assert_eq!(
            check_with(Path::new("/Users/me/notes"), true, Some(home), &[]),
            Ok(())
        );
    }

    #[test]
    fn rejects_system_directories_and_their_contents() {
        let roots = [PathBuf::from("/usr")];
        assert_eq!(
            check_with(Path::new("/usr"), true, None, &roots),
            Err(Reject::SystemDirectory)
        );
        assert_eq!(
            check_with(Path::new("/usr/local/vault"), true, None, &roots),
            Err(Reject::SystemDirectory)
        );
    }

    /// Same rule, Windows spelling. Split for the reason given above: on Unix
    /// `C:\Windows\System32` is a single `Normal` component, so the component-wise
    /// `starts_with` can't match and the assertion would fail for a reason that
    /// has nothing to do with the rule under test.
    #[cfg(windows)]
    #[test]
    fn rejects_windows_system_directories_and_their_contents() {
        let roots = [PathBuf::from("C:\\Windows")];
        assert_eq!(
            check_with(Path::new("C:\\Windows\\System32"), true, None, &roots),
            Err(Reject::SystemDirectory)
        );
    }

    /// A sibling that merely shares a prefix string is not "inside".
    #[test]
    fn prefix_match_is_by_component_not_by_string() {
        let roots = [PathBuf::from("/usr")];
        assert_eq!(check_with(Path::new("/usr-local/vault"), true, None, &roots), Ok(()));
    }

    /// The live check must accept a directory where a vault actually lives —
    /// i.e. somewhere under the user's home.
    ///
    /// Deliberately NOT `tempfile::tempdir()`: on macOS that lands in
    /// `/var/folders/…`, and `/var` is in `system_roots()`, so the guard rejects
    /// it — correctly. Using the system temp dir here tested the guard against a
    /// location no user would ever pick, and failed for the right reason.
    #[test]
    fn a_real_directory_passes_the_live_check() {
        let Some(home) = dirs::home_dir() else { return };
        let dir = tempfile::tempdir_in(&home).unwrap();
        assert_eq!(check(dir.path()), Ok(()));
    }

    #[test]
    fn every_reason_has_a_distinct_i18n_key() {
        let keys = [
            Reject::NotADirectory.i18n_key(),
            Reject::FilesystemRoot.i18n_key(),
            Reject::HomeDirectory.i18n_key(),
            Reject::SystemDirectory.i18n_key(),
        ];
        let unique: std::collections::HashSet<_> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
    }
}
