//! `notemd <path>` — hand a file or a directory to the desktop app.
//!
//! Not a subcommand of its own: this is the fallback the router takes when the
//! first token matched no command but names something on disk — `notemd .`
//! (open the current directory in the folder view) and `notemd xxx.md` (open
//! that file in a tab).
//!
//! Execution re-launches the GUI binary with the resolved *absolute* paths.
//! When the app is already running, `tauri-plugin-single-instance` hands that
//! argv to the live instance and the second process exits — so no second
//! window ever appears, and the CLI returns to the prompt immediately either
//! way.

use super::args::Parsed;
use super::router::{Builtin, Route};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

/// Marker argv entry that forces GUI mode in the re-launched process.
///
/// Without it, a `notemd` binary living outside a `.app` bundle or a `target/`
/// dir (see [`super::is_cli_mode`]) would recognize its own relaunch as a CLI
/// invocation and re-launch itself forever. The GUI's argv scan skips it for
/// free — it starts with `-`, and flags are never paths to open.
pub const GUI_FLAG: &str = "--gui";

/// Whether a token can only have been meant as a filesystem path, judged on
/// shape alone (no disk access, so this stays testable and cheap).
///
/// `.`/`..`, anything with a separator or a `~` prefix, and anything carrying a
/// dot (i.e. an extension: `xxx.md`) qualify. Flags never do.
pub fn looks_like_path(token: &str) -> bool {
    if token.starts_with('-') {
        return false;
    }
    if token == "." || token == ".." || token.starts_with('~') {
        return true;
    }
    token.chars().any(|c| std::path::is_separator(c) || c == '.')
}

/// Last routing step: turn tokens that matched no command into an open request.
///
/// Shape is not the only signal — an extension-less directory (`notemd notes`)
/// is a legitimate target too, so a token that exists on disk counts as well.
/// That probe is why this lives here and not inside the deliberately pure
/// `router::resolve_with`.
///
/// **The first token decides**, and the rest are then this command's problem:
/// `notemd a.md --bogus` is an open request with a bad option, and saying so
/// beats the alternative reading ("unknown command 'a.md'"), which points at
/// the one argument that was fine.
pub fn route_unmatched(rest: &[String]) -> Option<Route> {
    let first = rest.first()?;
    if looks_like_path(first) || Path::new(first).exists() {
        Some(Route::Builtin(Builtin::Open(rest.to_vec())))
    } else {
        None
    }
}

/// `~` / `~/x` → the home directory. Interactive shells expand this before we
/// ever see it; a quoted `notemd '~/notes'` (or a call from a non-shell parent)
/// does not.
fn expand_tilde(token: &str) -> PathBuf {
    if token == "~" || token.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return if token == "~" { home } else { home.join(&token[2..]) };
        }
    }
    PathBuf::from(token)
}

/// Resolve one token against the CLI's cwd. Absolute is mandatory, not
/// cosmetic: the GUI process has a different working directory (and the
/// frontend has no notion of one at all), so a relative path that survived to
/// the webview would resolve against the wrong root.
pub fn resolve_target(token: &str) -> Result<PathBuf, String> {
    expand_tilde(token)
        .canonicalize()
        .map_err(|_| format!("notemd: cannot open '{token}': No such file or directory"))
}

/// Resolve every token, or name the first one that isn't openable. There are no
/// flags of its own here — the globals are stripped in `args::parse`, so a
/// leftover `-x` is a mistake, not a path.
pub fn resolve_targets(tokens: &[String]) -> Result<Vec<PathBuf>, String> {
    // Usage errors first: a stray option means the command line was misread,
    // and reporting a missing file before it would send the user hunting for
    // the wrong problem.
    if let Some(flag) = tokens.iter().find(|t| t.starts_with('-')) {
        return Err(format!("notemd: unknown option '{flag}'"));
    }
    tokens.iter().map(|t| resolve_target(t)).collect()
}

pub fn run(tokens: &[String], parsed: &Parsed) -> ExitCode {
    let targets = match resolve_targets(tokens) {
        Ok(t) => t,
        Err(msg) => {
            if parsed.globals.json {
                println!("{}", json!({
                    "ok": false,
                    "error": { "code": "invalid_path", "message": msg.strip_prefix("notemd: ").unwrap_or(&msg) }
                }));
            } else {
                eprintln!("{msg}");
            }
            return ExitCode::from(2);
        }
    };

    if let Err(msg) = launch(&targets) {
        if parsed.globals.json {
            println!("{}", json!({
                "ok": false,
                "error": { "code": "launch_failed", "message": msg }
            }));
        } else {
            eprintln!("notemd: {msg}");
        }
        return ExitCode::from(1);
    }

    let shown: Vec<String> = targets.iter().map(|p| p.display().to_string()).collect();
    if parsed.globals.json {
        println!("{}", json!({ "ok": true, "data": { "opened": shown } }));
    } else if !parsed.globals.quiet {
        for p in &shown {
            eprintln!("→ opening {p}");
        }
    }
    ExitCode::from(0)
}

/// Spawn the GUI with the targets on its argv and return without waiting.
///
/// Deliberately a plain spawn of our own executable rather than macOS `open
/// -a`: `open --args` silently drops the arguments when the app is *already*
/// running, which is the common case here. A direct launch works in both
/// states because single-instance forwarding handles the running one.
fn launch(targets: &[PathBuf]) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot locate the note.md binary: {e}"))?;
    let mut cmd = Command::new(&exe);
    cmd.arg(GUI_FLAG);
    cmd.args(targets);
    // The GUI's stdio is not this terminal's business — and inheriting it would
    // dribble app logs into the user's shell long after `notemd .` returned.
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // Its own process group, so a Ctrl-C in the launching terminal doesn't
    // reach the window we just opened.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch {}: {e}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn dot_and_dotdot_are_paths() {
        assert!(looks_like_path("."));
        assert!(looks_like_path(".."));
    }

    #[test]
    fn extension_and_separator_are_paths() {
        assert!(looks_like_path("xxx.md"));
        assert!(looks_like_path("notes/a.md"));
        assert!(looks_like_path("/abs/dir"));
        assert!(looks_like_path("./x"));
        assert!(looks_like_path("~/vault"));
    }

    #[test]
    fn bare_words_and_flags_are_not_paths() {
        // A bare word is a (mistyped) command until the disk says otherwise —
        // `route_unmatched` is what upgrades an existing one to a target.
        assert!(!looks_like_path("notes"));
        assert!(!looks_like_path("share"));
        assert!(!looks_like_path("--json"));
        assert!(!looks_like_path("-s"));
    }

    #[test]
    fn route_unmatched_takes_dot() {
        let r = route_unmatched(&s(&["."])).expect("`.` must route to open");
        let Route::Builtin(Builtin::Open(t)) = r else { panic!("expected Open") };
        assert_eq!(t, s(&["."]));
    }

    #[test]
    fn route_unmatched_takes_several_files() {
        let r = route_unmatched(&s(&["a.md", "b.md"])).expect("both are paths");
        let Route::Builtin(Builtin::Open(t)) = r else { panic!("expected Open") };
        assert_eq!(t, s(&["a.md", "b.md"]));
    }

    #[test]
    fn route_unmatched_takes_an_existing_extensionless_dir() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("notes");
        std::fs::create_dir(&sub).unwrap();
        let token = sub.display().to_string();
        assert!(route_unmatched(&[token]).is_some());
    }

    #[test]
    fn route_unmatched_declines_a_bare_word() {
        // Stays Unknown → "unknown command 'nosuchthing'", not a file error.
        assert!(route_unmatched(&s(&["nosuchthing"])).is_none());
    }

    #[test]
    fn route_unmatched_declines_nothing_at_all() {
        assert!(route_unmatched(&[]).is_none());
    }

    /// A stray flag after a path is still an open request — so the error can
    /// name the flag instead of blaming the file that was fine.
    #[test]
    fn mixed_invocation_routes_to_open_and_then_rejects_the_flag() {
        assert!(route_unmatched(&s(&["a.md", "--bogus"])).is_some());
        let err = resolve_targets(&s(&["a.md", "--bogus"])).unwrap_err();
        assert!(err.contains("unknown option '--bogus'"), "{err}");
    }

    #[test]
    fn resolve_targets_stops_at_the_first_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("a.md");
        std::fs::write(&ok, b"a").unwrap();
        let err = resolve_targets(&s(&[
            &ok.display().to_string(),
            &dir.path().join("gone.md").display().to_string(),
        ]))
        .unwrap_err();
        assert!(err.contains("gone.md"), "{err}");
    }

    #[test]
    fn resolve_target_makes_paths_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.md");
        std::fs::write(&file, b"# hi\n").unwrap();
        let resolved = resolve_target(&file.display().to_string()).unwrap();
        assert!(resolved.is_absolute());
        assert!(resolved.ends_with("x.md"));

        // A directory resolves the same way — `notemd .` is the whole point.
        let d = resolve_target(&dir.path().display().to_string()).unwrap();
        assert!(d.is_dir());
    }

    #[test]
    fn resolve_target_reports_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.md");
        let err = resolve_target(&missing.display().to_string()).unwrap_err();
        assert!(err.contains("cannot open"), "{err}");
        assert!(err.contains("No such file"), "{err}");
    }

    #[test]
    fn expand_tilde_uses_home() {
        let home = dirs::home_dir().expect("home dir");
        assert_eq!(expand_tilde("~"), home);
        assert_eq!(expand_tilde("~/notes"), home.join("notes"));
        // Only a leading `~/` is special; a file literally named `~x` is not.
        assert_eq!(expand_tilde("plain.md"), PathBuf::from("plain.md"));
    }
}
