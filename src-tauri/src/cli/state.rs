//! Shared state between the Rust CLI runner and the frontend CliRunner.
//!
//! The runner builds a CliPayload, pushes it into CliState before showing
//! the hidden window, and waits on a oneshot channel for the frontend's
//! cli_finish call. The frontend's CliRunner pulls the payload via the
//! cli_payload command, performs the work, and reports completion through
//! cli_finish.
//!
//! cli_finish is the single exit point for every route that reaches the
//! headless Tauri instance (plugin subcommands, and the core-ised `share` /
//! `reading-insights report` paths — see CliRunner.svelte's `finish()`, the
//! only caller of the `cli_finish` command). It ends the process itself via
//! `std::process::exit`, deliberately not via `AppHandle::exit` — see the
//! comment on `cli_finish` for why that path silently drops the exit code.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::sync::Mutex;
use tokio::sync::oneshot;

#[derive(Debug, Clone, Serialize)]
pub struct CliPayload {
    pub subcommand: String,
    pub plugin_id: String,
    pub plugin_command: String,
    /// Positional arguments keyed by the names declared in the manifest's
    /// `contributes.cli[].args` array. Path values have already been resolved
    /// by the Rust runner; string/integer values are preserved as typed.
    pub args: serde_json::Map<String, serde_json::Value>,
    pub flags: serde_json::Map<String, serde_json::Value>,
    pub global: GlobalFlags,
}

#[derive(Debug, Clone, Serialize)]
pub struct GlobalFlags {
    pub json: bool,
    pub quiet: bool,
    pub clipboard: bool,
}

#[derive(Debug, Deserialize)]
pub struct CliResult {
    pub exit_code: i32,
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Vec<String>,
}

pub struct CliState {
    pub payload: Mutex<Option<CliPayload>>,
    pub result_tx: Mutex<Option<oneshot::Sender<CliResult>>>,
}

impl CliState {
    pub fn new(payload: CliPayload, tx: oneshot::Sender<CliResult>) -> Self {
        Self {
            payload: Mutex::new(Some(payload)),
            result_tx: Mutex::new(Some(tx)),
        }
    }
}

#[tauri::command]
pub fn cli_payload(state: tauri::State<'_, CliState>) -> Result<CliPayload, String> {
    let p = state.payload.lock().unwrap().clone();
    p.ok_or_else(|| "cli payload missing".to_string())
}

/// What `cli_finish` must do with a `CliResult`, split out from the
/// tauri::command so the code-preservation contract is unit-testable —
/// `cli_finish` itself ends in `std::process::exit`, which a test cannot
/// safely invoke (it would kill the test process).
pub struct FinishEffects {
    pub exit_code: i32,
    pub stdout_line: Option<String>,
    pub stderr_lines: Vec<String>,
}

pub fn finish_effects(result: &CliResult) -> FinishEffects {
    FinishEffects {
        exit_code: result.exit_code,
        stdout_line: result.stdout.as_ref().filter(|s| !s.is_empty()).cloned(),
        stderr_lines: result.stderr.clone(),
    }
}

#[tauri::command]
pub fn cli_finish(result: CliResult, state: tauri::State<'_, CliState>) -> Result<(), String> {
    if let Some(tx) = state.result_tx.lock().unwrap().take() {
        let effects = finish_effects(&result);
        if let Some(s) = &effects.stdout_line {
            println!("{s}");
        }
        for line in &effects.stderr_lines {
            eprintln!("{line}");
        }
        // `process::exit` does not run destructors. Flush explicitly so JSON
        // envelopes and human diagnostics are never lost when stdout/stderr
        // are redirected rather than attached to a terminal.
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        let exit_code = effects.exit_code;
        // Kept for the (currently unreachable on macOS) fallback path in
        // launch_tauri_headless, in case app.run() ever does return.
        let _ = tx.send(result);
        // Do NOT route this through `AppHandle::exit` / `app.exit(code)`.
        // That funnels through tao's event loop: tauri-runtime-wry's
        // `Message::RequestExit(code)` handler unconditionally sets
        // `*control_flow = ControlFlow::Exit`, which tao defines as an alias
        // for `ExitWithCode(0)` — the caller-supplied code is discarded right
        // there. tao's own `EventLoop::run` (macOS, and structurally similar
        // elsewhere) then calls `std::process::exit(0)` itself, bypassing
        // Rust's normal call-stack return entirely, so main()'s ExitCode
        // never gets a chance to matter. This is why every `notemd
        // <plugin-subcommand>` invocation used to exit 0 regardless of
        // outcome. Exit directly with the real code instead.
        std::process::exit(exit_code);
    } else {
        Err("cli_finish called twice or without state".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;
    use tokio::sync::oneshot;

    #[test]
    fn cli_state_holds_payload_once() {
        let (tx, _rx) = oneshot::channel();
        let payload = CliPayload {
            subcommand: "share".into(),
            plugin_id: "share".into(),
            plugin_command: "publish".into(),
            args: serde_json::Map::from_iter([(
                "file".into(),
                serde_json::Value::String("/tmp/x.md".into()),
            )]),
            flags: Map::new(),
            global: GlobalFlags {
                json: false,
                quiet: false,
                clipboard: true,
            },
        };
        let state = CliState::new(payload.clone(), tx);
        let first = state.payload.lock().unwrap().clone().unwrap();
        assert_eq!(first.subcommand, "share");
    }

    /// The defect this guards: cli_finish must exit with the *real* code
    /// (0 success, 2 bad flags, 3 plugin disabled/missing, 4 plugin failure,
    /// …) instead of always 0. finish_effects is the pure slice of
    /// cli_finish's behavior that a test can observe without invoking
    /// std::process::exit — every exit_code value must survive unchanged.
    #[test]
    fn finish_effects_preserves_success_code() {
        let result = CliResult {
            exit_code: 0,
            stdout: Some("wrote /tmp/x".into()),
            stderr: vec![],
        };
        let effects = finish_effects(&result);
        assert_eq!(effects.exit_code, 0);
        assert_eq!(effects.stdout_line.as_deref(), Some("wrote /tmp/x"));
        assert!(effects.stderr_lines.is_empty());
    }

    #[test]
    fn finish_effects_preserves_plugin_failure_code() {
        // The exact --json shape CliRunner.svelte sends on a caught plugin error.
        let result = CliResult {
            exit_code: 4,
            stdout: Some(
                r#"{"ok":false,"error":{"code":"plugin_failed","message":"boom"}}"#.into(),
            ),
            stderr: vec![],
        };
        let effects = finish_effects(&result);
        assert_eq!(effects.exit_code, 4);
        assert!(effects.stderr_lines.is_empty());
    }

    #[test]
    fn finish_effects_preserves_bad_flag_code() {
        let result = CliResult {
            exit_code: 2,
            stdout: None,
            stderr: vec!["notemd: missing file argument".into()],
        };
        let effects = finish_effects(&result);
        assert_eq!(effects.exit_code, 2);
        assert_eq!(effects.stdout_line, None);
    }

    #[test]
    fn finish_effects_treats_empty_stdout_as_absent() {
        let result = CliResult {
            exit_code: 0,
            stdout: Some(String::new()),
            stderr: vec![],
        };
        let effects = finish_effects(&result);
        assert_eq!(effects.stdout_line, None);
    }
}
