//! Install / uninstall / repair the `notemd` symlink.
//!
//! macOS-only in v1. Uses `osascript -e 'do shell script ... with
//! administrator privileges'` for paths that require elevation.
//!
//! The command was named `mdedit` before the note.md rename. Installing
//! refreshes a legacy `mdedit` link when one exists (keeping old scripts
//! working, and repairing it if the install moved), and uninstalling removes
//! both names.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

const LINK_NAME: &str = "notemd";
const LEGACY_LINK_NAME: &str = "mdedit";

#[derive(Debug, Serialize)]
pub struct InstallStatus {
    pub installed: bool,
    pub path: Option<String>,
    pub target_valid: bool,
}

pub fn current_app_binary() -> PathBuf {
    std::env::current_exe()
        // CFBundleExecutable / MacOS binary is `notemd` (mainBinaryName), not
        // the product name `note.md`.
        .unwrap_or_else(|_| PathBuf::from("/Applications/note.md.app/Contents/MacOS/notemd"))
}

pub fn candidate_dirs() -> Vec<PathBuf> {
    let mut out = vec![
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        out.push(Path::new(&home).join(".local/bin"));
    }
    out
}

/// POSIX-escape a string so it can be embedded inside a single-quoted shell
/// argument. Replaces `'` with `'\''` (close-quote, escaped quote, open-quote).
fn sh_single_quote_escape(s: &str) -> String {
    s.replace('\'', "'\\''")
}

fn assert_candidate(dir: &str) -> Result<(), String> {
    let candidates = candidate_dirs();
    let path = std::path::Path::new(dir);
    if candidates.iter().any(|c| c == path) {
        Ok(())
    } else {
        Err(format!(
            "notemd: refusing to install into '{dir}' — only candidate dirs are allowed"
        ))
    }
}

/// Create the `notemd` launcher link. Split by platform the same way
/// `agents_sync::make_symlink` is: unix gets a real symlink, everything else
/// reports the capability as missing rather than failing to compile.
#[cfg(unix)]
fn make_link(target: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())
}

/// Windows has no unprivileged symlink; the plan is a PATH shim written by the
/// NSIS installer instead (see docs/2026-08-08-pc-port-refactor-plan.md §5.1).
#[cfg(not(unix))]
fn make_link(_target: &Path, _link: &Path) -> Result<(), String> {
    Err("CLI symlink install is not supported on this platform".to_string())
}

pub fn install(dir: &Path) -> Result<bool, String> {
    let target = current_app_binary();
    if !target.exists() {
        return Err(format!("source binary missing: {}", target.display()));
    }
    let link = dir.join(LINK_NAME);
    // Refresh a pre-rename `mdedit` link too so old scripts keep working.
    let legacy_link = dir.join(LEGACY_LINK_NAME);
    let refresh_legacy = legacy_link.symlink_metadata().is_ok();

    let need_sudo = matches!(dir.to_str(), Some("/usr/local/bin") | Some("/opt/homebrew/bin"));

    if !need_sudo {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        for l in std::iter::once(&link).chain(refresh_legacy.then_some(&legacy_link)) {
            if l.symlink_metadata().is_ok() {
                std::fs::remove_file(l).map_err(|e| e.to_string())?;
            }
            make_link(&target, l)?;
        }
        return Ok(true);
    }

    let mut script = format!(
        "mkdir -p '{dir}' && ln -sfn '{target}' '{link}'",
        dir = sh_single_quote_escape(&dir.display().to_string()),
        target = sh_single_quote_escape(&target.display().to_string()),
        link = sh_single_quote_escape(&link.display().to_string()),
    );
    if refresh_legacy {
        script.push_str(&format!(
            " && ln -sfn '{target}' '{legacy}'",
            target = sh_single_quote_escape(&target.display().to_string()),
            legacy = sh_single_quote_escape(&legacy_link.display().to_string()),
        ));
    }
    let status = Command::new("osascript")
        .args(["-e", &format!("do shell script \"{}\" with administrator privileges",
            script.replace('"', "\\\""))])
        .status()
        .map_err(|e| format!("osascript failed: {e}"))?;
    if !status.success() {
        return Err(format!("osascript exited {}", status.code().unwrap_or(-1)));
    }
    Ok(true)
}

pub fn uninstall(dir: &Path) -> Result<(), String> {
    let links: Vec<PathBuf> = [LINK_NAME, LEGACY_LINK_NAME]
        .iter()
        .map(|n| dir.join(n))
        .filter(|l| l.symlink_metadata().is_ok())
        .collect();
    if links.is_empty() {
        return Ok(());
    }
    let need_sudo = matches!(dir.to_str(), Some("/usr/local/bin") | Some("/opt/homebrew/bin"));
    if !need_sudo {
        for link in &links {
            std::fs::remove_file(link).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let script = links
        .iter()
        .map(|l| format!("rm -f '{}'", sh_single_quote_escape(&l.display().to_string())))
        .collect::<Vec<_>>()
        .join(" && ");
    let status = Command::new("osascript")
        .args(["-e", &format!("do shell script \"{}\" with administrator privileges",
            script.replace('"', "\\\""))])
        .status()
        .map_err(|e| format!("osascript failed: {e}"))?;
    if !status.success() {
        return Err(format!("osascript exited {}", status.code().unwrap_or(-1)));
    }
    Ok(())
}

pub fn status(installed_path: Option<&Path>) -> InstallStatus {
    if let Some(p) = installed_path {
        if p.exists() {
            let resolved = std::fs::read_link(p).ok();
            let current = current_app_binary();
            let target_valid = resolved.as_deref().map(|r| r == current).unwrap_or(false);
            return InstallStatus {
                installed: true,
                path: Some(p.display().to_string()),
                target_valid,
            };
        }
    }
    for dir in candidate_dirs() {
        for name in [LINK_NAME, LEGACY_LINK_NAME] {
            let link = dir.join(name);
            // symlink_metadata (not exists) so a dangling legacy link still
            // surfaces as installed-but-broken instead of "not installed".
            if link.symlink_metadata().is_ok() {
                let resolved = std::fs::read_link(&link).ok();
                let current = current_app_binary();
                let target_valid = resolved.as_deref().map(|r| r == current).unwrap_or(false);
                return InstallStatus {
                    installed: true,
                    path: Some(link.display().to_string()),
                    target_valid,
                };
            }
        }
    }
    InstallStatus { installed: false, path: None, target_valid: false }
}

#[tauri::command]
pub fn cli_install_status() -> InstallStatus {
    status(None)
}

#[tauri::command]
pub fn cli_install(dir: String) -> Result<(), String> {
    assert_candidate(&dir)?;
    install(Path::new(&dir)).map(|_| ())
}

#[tauri::command]
pub fn cli_uninstall(dir: String) -> Result<(), String> {
    assert_candidate(&dir)?;
    uninstall(Path::new(&dir))
}

#[tauri::command]
pub fn cli_install_candidates() -> Vec<String> {
    candidate_dirs().into_iter().map(|p| p.display().to_string()).collect()
}

// The suite below exercises the unix install path end to end: real symlinks,
// `/usr/local/bin`-shaped candidate dirs, and `sh` quoting for the osascript
// elevation script. None of that has a Windows counterpart — `install()`
// reports the capability as missing there (see `make_link`) — so the module is
// unix-gated rather than contorted, and Windows gets its own assertion below.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::platform::test_symlink as unix_symlink;

    #[test]
    fn status_reports_installed_when_link_present() {
        let dir = std::env::temp_dir().join(format!("notemd-status-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join("notemd");
        let target = std::env::current_exe().unwrap();
        unix_symlink(&target, &link).unwrap();

        let st = status(Some(&link));
        assert!(st.installed);
        assert_eq!(st.path.as_deref(), Some(link.display().to_string().as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // (Removed: `status_reports_not_installed_when_no_link` depended on no
    // `mdedit` symlink existing in any of the hardcoded candidate dirs, which
    // isn't a property the test can enforce. `status(Some(stale_path))`
    // legitimately falls through to candidate probing, so on a developer
    // machine with a real install the test reports "installed" — by design.)

    #[test]
    fn install_creates_symlink_in_writable_dir() {
        let dir = std::env::temp_dir().join(format!("notemd-install-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = install(&dir).unwrap();
        assert!(ok);
        let link = dir.join("notemd");
        assert!(link.symlink_metadata().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_refreshes_dangling_legacy_mdedit_link() {
        let dir = std::env::temp_dir().join(format!("notemd-legacy-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let legacy = dir.join("mdedit");
        unix_symlink(dir.join("no-such-binary"), &legacy).unwrap();

        install(&dir).unwrap();
        assert_eq!(std::fs::read_link(&legacy).unwrap(), current_app_binary());
        assert_eq!(std::fs::read_link(dir.join("notemd")).unwrap(), current_app_binary());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uninstall_removes_legacy_link_too() {
        let dir = std::env::temp_dir().join(format!("notemd-uninst-legacy-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        unix_symlink(std::env::current_exe().unwrap(), dir.join("mdedit")).unwrap();
        install(&dir).unwrap();
        uninstall(&dir).unwrap();
        assert!(dir.join("notemd").symlink_metadata().is_err());
        assert!(dir.join("mdedit").symlink_metadata().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uninstall_removes_existing_symlink() {
        let dir = std::env::temp_dir().join(format!("notemd-uninst-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        install(&dir).unwrap();
        uninstall(&dir).unwrap();
        assert!(dir.join("notemd").symlink_metadata().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cli_install_rejects_arbitrary_dir() {
        // Calling assert_candidate directly since cli_install is a Tauri command.
        let r = assert_candidate("/tmp/whatever");
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("only candidate dirs"));
    }

    #[test]
    fn cli_install_accepts_candidate_dirs() {
        let cands = candidate_dirs();
        // First candidate is /usr/local/bin (or /opt/homebrew/bin on Apple Silicon)
        let r = assert_candidate(&cands[0].display().to_string());
        assert!(r.is_ok());
    }

    #[test]
    fn sh_single_quote_escape_handles_quote() {
        assert_eq!(sh_single_quote_escape("a'b"), "a'\\''b");
        assert_eq!(sh_single_quote_escape("no quotes"), "no quotes");
        assert_eq!(sh_single_quote_escape("'"), "'\\''");
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    /// Windows has no unprivileged symlink, so CLI install must fail loudly
    /// rather than half-succeed. The NSIS installer writes a PATH shim instead
    /// (docs/2026-08-08-pc-port-refactor-plan.md §5.1), and the settings UI is
    /// expected to hide the affordance.
    #[test]
    fn install_reports_unsupported() {
        let dir = std::env::temp_dir().join(format!(
            "notemd-install-win-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let err = install(&dir).unwrap_err();
        assert!(err.contains("not supported"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
