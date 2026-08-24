//! CLI mode: argv parsing, routing, and execution.
//!
//! Entered from `main.rs` when argv contains `--cli`, or `argv[0]` is a bare
//! `notemd` / `mdedit` symlink invocation (not the GUI binary launched from
//! inside the `.app` bundle or `target/`). Returns a `std::process::ExitCode`
//! that main propagates. See [`is_cli_mode`] for the exact discrimination.

use std::path::PathBuf;
use std::process::ExitCode;

pub mod args;
pub mod router;
pub mod builtin;
pub mod runner;
pub mod install;
pub mod doctor;
pub mod search;
pub mod state;
pub mod open;

use crate::app_dirs::BUNDLE_ID as APP_BUNDLE_ID;

/// Whether a manifest produced by the CLI scan is active.
///
/// Everything the scan yields is enabled by construction — the injected core
/// stubs are always on, and v2 discovery only returns plugins that the runtime's
/// `state.json` marks enabled — so the map is consulted purely as an explicit
/// override and a missing entry means "on".
pub fn is_enabled(
    m: &crate::plugin_host::PluginManifest,
    enabled: &std::collections::HashMap<String, bool>,
) -> bool {
    enabled.get(&m.id).copied().unwrap_or(true)
}

/// Resolve the app config directory (where settings.json lives).
///
/// `dirs::config_dir()` IS `~/Library/Application Support` on macOS, so this is
/// byte-identical to the previous hand-rolled `$HOME/Library/Application
/// Support` there — and it is the only form that works elsewhere. The old code
/// keyed off `$HOME`, which Windows does not set: it fell through to `"."` and
/// scattered settings.json into whatever directory the app happened to be
/// launched from. Matches `shared_config::config_path` and `runner.rs`, which
/// were already on `dirs::`.
pub fn resolve_config_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join(APP_BUNDLE_ID))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Detect whether the current process should run in CLI mode.
///
/// The GUI executable is itself named `notemd` (mainBinaryName), the same as
/// the CLI symlink — so a bare basename check would misfire and drop the GUI
/// into CLI mode (printing help and exiting instead of opening a window).
/// Disambiguate by launch path: a GUI launch runs the *real* binary, which
/// lives inside the `.app` bundle in production or under `target/` in dev /
/// `cargo run`; a CLI invocation comes through a bin-dir symlink (e.g.
/// `/usr/local/bin/notemd`) or a bare `notemd` argv[0], neither of which
/// contains those path segments.
pub fn is_cli_mode(argv: &[String]) -> bool {
    // `notemd <path>` re-launches this same binary to show the window; the
    // marker it passes has to win over every heuristic below, or a `notemd`
    // that lives outside a bundle would relaunch itself forever.
    if argv.iter().any(|a| a == open::GUI_FLAG) { return false; }
    if argv.iter().any(|a| a == "--cli") { return true; }
    if let Some(arg0) = argv.first() {
        // `cargo run` (tauri dev) launches with a *relative* arg0
        // (`target/debug/notemd`), so match "target/" without a leading
        // slash too — otherwise dev GUI drops into CLI help and exits.
        if arg0.contains(".app/Contents/MacOS/") || arg0.contains("/target/")
            || arg0.starts_with("target/") {
            return false;
        }
        let name = std::path::Path::new(arg0)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        // `mdedit` is the pre-rename command name; old symlinks keep working.
        if name == "notemd" || name == "mdedit" { return true; }
    }
    false
}

pub fn run_cli(argv: Vec<String>) -> ExitCode {
    let parsed = args::parse(&argv);
    let route = router::resolve(&parsed);
    match route {
        router::Route::Builtin(b) => builtin::run(b, &parsed),
        router::Route::Plugin(p) => runner::run(p, parsed),
        router::Route::Disabled { plugin_id, subcommand } => {
            eprintln!("notemd: command '{subcommand}' is provided by the '{plugin_id}' plugin, which is disabled.");
            eprintln!("Enable it in Preferences → Plugins, or run:");
            eprintln!("  notemd plugin enable {plugin_id}");
            ExitCode::from(3)
        }
        router::Route::Unknown(name) => {
            eprintln!("notemd: unknown command '{name}'. Run 'notemd help' to see available commands.");
            ExitCode::from(127)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_cli_mode;

    fn argv(a0: &str) -> Vec<String> {
        vec![a0.to_string(), "help".to_string()]
    }

    #[test]
    fn gui_launch_from_app_bundle_is_not_cli() {
        // Regression: the GUI binary is named `notemd`; launching it from the
        // .app must open a window, not drop into CLI help + exit.
        assert!(!is_cli_mode(&argv(
            "/Applications/note.md.app/Contents/MacOS/notemd"
        )));
    }

    #[test]
    fn gui_launch_from_target_dir_is_not_cli() {
        assert!(!is_cli_mode(&argv(
            "/Users/x/src-tauri/target/debug/notemd"
        )));
        assert!(!is_cli_mode(&argv(
            "/Users/x/src-tauri/target/aarch64-apple-darwin/release/notemd"
        )));
        // `cargo run` / `tauri dev` uses a relative arg0.
        assert!(!is_cli_mode(&argv("target/debug/notemd")));
    }

    #[test]
    fn bare_symlink_name_is_cli() {
        assert!(is_cli_mode(&argv("notemd")));
        assert!(is_cli_mode(&argv("/usr/local/bin/notemd")));
        assert!(is_cli_mode(&argv("mdedit")));
        assert!(is_cli_mode(&argv("/opt/homebrew/bin/mdedit")));
    }

    /// The re-launch marker beats both the `--cli` flag and a bare `notemd`
    /// argv[0] — otherwise `notemd .` would spawn CLI processes in a loop.
    #[test]
    fn gui_flag_beats_every_cli_signal() {
        assert!(!is_cli_mode(&vec![
            "/usr/local/bin/notemd".to_string(),
            "--gui".to_string(),
            "/Users/x/notes".to_string(),
        ]));
        assert!(!is_cli_mode(&vec![
            "notemd".to_string(),
            "--cli".to_string(),
            "--gui".to_string(),
        ]));
    }

    #[test]
    fn explicit_cli_flag_always_wins() {
        assert!(is_cli_mode(&vec![
            "/Applications/note.md.app/Contents/MacOS/notemd".to_string(),
            "--cli".to_string(),
        ]));
    }

    #[test]
    fn unrelated_name_is_not_cli() {
        assert!(!is_cli_mode(&argv("/usr/local/bin/something-else")));
    }
}
