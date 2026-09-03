//! Build one `codex exec` invocation.
//!
//! The prompt is intentionally represented by the final `-` and written to
//! stdin by the engine. Besides avoiding shell quoting entirely, this keeps a
//! large task prompt out of the OS argv size limit and process listings.
use std::path::Path;

pub fn build(model: Option<&str>, workspace: &Path, sandbox: &str) -> Vec<String> {
    let mut out = vec![
        "-a".into(),
        "never".into(),
        "exec".into(),
        "--json".into(),
        "--ephemeral".into(),
        "--skip-git-repo-check".into(),
        "-C".into(),
        workspace.to_string_lossy().into_owned(),
        "--sandbox".into(),
        sandbox.into(),
    ];
    if let Some(model) = model.filter(|m| !m.trim().is_empty()) {
        out.push("--model".into());
        out.push(model.to_string());
    }
    // A literal dash tells `codex exec` to read the complete prompt from stdin.
    out.push("-".into());
    out
}

/// A search protocol run receives its complete packet in stdin. Keep Codex
/// authenticated, but disable every local/discoverable information path so a
/// prompt-injected question or source cannot browse beyond that frozen packet.
pub fn build_input_only(model: Option<&str>, workspace: &Path) -> Vec<String> {
    let mut out = build(model, workspace, "read-only");
    let stdin_marker = out.pop().expect("build always ends in stdin marker");
    out.extend([
        "--strict-config".into(),
        "--ignore-user-config".into(),
        "--ignore-rules".into(),
        "--disable".into(),
        "shell_tool".into(),
        "--disable".into(),
        "unified_exec".into(),
        "-c".into(),
        "web_search=\"disabled\"".into(),
        "-c".into(),
        "agents.enabled=false".into(),
        "-c".into(),
        "apps._default.enabled=false".into(),
        "-c".into(),
        "tools.view_image=false".into(),
        "-c".into(),
        "features.skill_mcp_dependency_install=false".into(),
    ]);
    out.push(stdin_marker);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_headless_jsonl_run_with_explicit_boundaries() {
        assert_eq!(
            build(Some("gpt-5.6-terra"), Path::new("/v"), "workspace-write",),
            vec![
                "-a",
                "never",
                "exec",
                "--json",
                "--ephemeral",
                "--skip-git-repo-check",
                "-C",
                "/v",
                "--sandbox",
                "workspace-write",
                "--model",
                "gpt-5.6-terra",
                "-",
            ]
        );
    }

    #[test]
    fn leaves_model_selection_to_codex_when_the_task_does_not_pin_one() {
        let got = build(None, Path::new("/vault"), "read-only");
        assert!(!got.iter().any(|a| a == "--model"));
        assert_eq!(got.last().map(String::as_str), Some("-"));
    }

    #[test]
    fn blank_model_is_the_same_as_no_model() {
        let got = build(Some("  "), Path::new("/vault"), "danger-full-access");
        assert!(!got.iter().any(|a| a == "--model"));
    }

    #[test]
    fn input_only_disables_local_and_discoverable_tools() {
        let got = build_input_only(Some("gpt-5.6-terra"), Path::new("/isolated"));
        for pair in [
            ["--disable", "shell_tool"],
            ["--disable", "unified_exec"],
            ["-c", "web_search=\"disabled\""],
            ["-c", "agents.enabled=false"],
            ["-c", "apps._default.enabled=false"],
            ["-c", "tools.view_image=false"],
        ] {
            assert!(
                got.windows(2).any(|window| window == pair),
                "missing {pair:?}: {got:?}"
            );
        }
        assert!(got.iter().any(|arg| arg == "--ignore-user-config"));
        assert!(got.iter().any(|arg| arg == "--ignore-rules"));
        assert!(got.iter().any(|arg| arg == "--strict-config"));
        assert_eq!(got.last().map(String::as_str), Some("-"));
    }
}
