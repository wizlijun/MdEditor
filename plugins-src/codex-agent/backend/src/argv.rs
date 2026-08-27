//! Build one `codex exec` invocation.
//!
//! The prompt is intentionally represented by the final `-` and written to
//! stdin by the engine. Besides avoiding shell quoting entirely, this keeps a
//! large task prompt out of the OS argv size limit and process listings.
use std::path::Path;

pub fn build(model: Option<&str>, vault: &Path, sandbox: &str) -> Vec<String> {
    let mut out = vec![
        "-a".into(),
        "never".into(),
        "exec".into(),
        "--json".into(),
        "--ephemeral".into(),
        "--skip-git-repo-check".into(),
        "-C".into(),
        vault.to_string_lossy().into_owned(),
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
}
