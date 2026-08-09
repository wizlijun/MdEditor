//! Platform port layer — the single funnel for spawning child processes.
//!
//! Two things must be true on Windows that are free on unix, and both are easy
//! to forget at each individual call site (which is why they live here):
//!
//! 1. **No console flash.** A GUI process spawning a console subprocess (`git`)
//!    pops a black `conhost` window for the life of the child. `git` runs on
//!    every vault-sync tick, so without `CREATE_NO_WINDOW` the app strobes.
//! 2. **A usable child environment.** `env_clear()` on Windows is far more
//!    destructive than on unix: without `SystemRoot` the loader cannot
//!    initialise winsock/crypto and most binaries die before `main`.
//!
//! Migration discipline (docs/2026-08-08-pc-port-refactor-plan.md §1.1): new
//! code MUST NOT call `std::process::Command::new` / `tokio::process::Command::new`
//! directly outside this module.

use std::ffi::OsStr;

/// `CREATE_NO_WINDOW` (winbase.h). Suppresses the console window that a GUI
/// parent would otherwise pop for a console child.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `std::process::Command`, windowless on Windows.
pub fn command(program: impl AsRef<OsStr>) -> std::process::Command {
    let cmd = std::process::Command::new(program);
    #[cfg(windows)]
    let mut cmd = cmd;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `tokio::process::Command`, windowless on Windows.
#[cfg(not(target_os = "ios"))]
pub fn tokio_command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    let cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    let mut cmd = cmd;
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Environment variables a plugin subprocess may inherit after `env_clear()`.
///
/// The unix set is the original allowlist from `plugin_runtime::process`: enough
/// for a plugin to find `$HOME`/`$PATH` (openclaw resolves its UDS socket path
/// from them) and nothing that carries a secret.
///
/// The Windows set is the equivalent, and it is not optional the way the unix
/// one is: `SystemRoot` in particular is required by the loader itself — clear
/// it and a child dies before reaching `main` with an opaque failure. `PATHEXT`
/// and `COMSPEC` are needed for command resolution; `APPDATA`/`LOCALAPPDATA`/
/// `USERPROFILE` are the Windows analogue of `$HOME`; `TEMP`/`TMP` of `TMPDIR`.
pub fn plugin_env_allowlist() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &[
            "SystemRoot", "windir", "SystemDrive", "COMSPEC", "PATHEXT", "PATH",
            "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "APPDATA", "LOCALAPPDATA",
            "PROGRAMDATA", "TEMP", "TMP", "NUMBER_OF_PROCESSORS",
            "PROCESSOR_ARCHITECTURE", "LANG", "LC_ALL", "USERNAME",
        ]
    }
    #[cfg(not(windows))]
    {
        &["HOME", "PATH", "LANG", "LC_ALL", "TERM", "USER", "TMPDIR"]
    }
}

/// Point `link` at the directory `target`.
///
/// unix: an ordinary symlink (the caller decides relative vs absolute).
///
/// Windows: `symlink_dir` first — that is the pre-existing behaviour and what
/// you get with Developer Mode on — and on failure a **directory junction**,
/// which needs no privilege whatsoever. Without the fallback, installing any
/// plugin failed with `os error 1314` (ERROR_PRIVILEGE_NOT_HELD) on a stock
/// Windows account, because the plugin tree's `current` pointer is a directory
/// link.
///
/// A junction is indistinguishable from a symlink to the callers here:
/// `read_link` returns the target, `symlink_metadata().file_type().is_symlink()`
/// is true, and traversal works. The one difference is that a junction must
/// name an absolute local path, which is what the Windows branch already passed.
#[cfg(windows)]
pub fn link_dir(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    let first = match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    // `mklink` is a cmd.exe builtin, so it has to go through the shell. Args are
    // passed as separate values, letting std quote paths containing spaces
    // (`C:\Users\Some Name\...` is entirely ordinary).
    let status = command("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() && link.exists() {
        Ok(())
    } else {
        Err(first)
    }
}

/// unix counterpart of the Windows [`link_dir`]: a plain symlink.
#[cfg(unix)]
pub fn link_dir(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Create a symlink in a test, of whichever kind the platform needs.
///
/// Windows distinguishes file from directory links and requires Developer Mode
/// or elevation, so an `Err` here means "this machine cannot make symlinks" —
/// callers must skip rather than fail (docs/2026-08-08-pc-port-refactor-plan.md
/// §9.1). Before this existed, the affected tests called `std::os::unix`
/// directly and the whole lib-test crate failed to compile on Windows.
#[cfg(test)]
pub fn test_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_carries_no_secrets() {
        // A regression guard on the intent of the list, not its exact contents:
        // nothing key/token/secret-shaped may be inherited by a plugin.
        for k in plugin_env_allowlist() {
            let lower = k.to_ascii_lowercase();
            assert!(
                !lower.contains("key")
                    && !lower.contains("token")
                    && !lower.contains("secret")
                    && !lower.contains("password"),
                "secret-shaped var in plugin env allowlist: {k}"
            );
        }
    }

    /// `SystemRoot` is load-bearing on Windows — a child without it fails to
    /// start at all, so this is not a "nice to have" entry.
    #[cfg(windows)]
    #[test]
    fn windows_allowlist_has_systemroot() {
        assert!(plugin_env_allowlist().contains(&"SystemRoot"));
    }

    #[test]
    fn command_builds() {
        // Smoke: the builder must compile and be usable on every platform.
        let c = command("git");
        assert_eq!(c.get_program(), std::ffi::OsStr::new("git"));
    }
}
