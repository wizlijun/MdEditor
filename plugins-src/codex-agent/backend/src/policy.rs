//! A task's Codex sandbox policy. `permission_mode` maps directly to
//! `codex exec --sandbox`; the process runs with approval policy `never`.
//!
//! `on_permission_request` stays in the shared file format so the same task can
//! be used by DeepSeek Agent. Codex emits no permission prompt in this
//! non-interactive mode, so that field is compatibility metadata here.
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The sandbox preset passed through verbatim to Codex CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    /// Reads only. Nothing on disk changes.
    ReadOnly,
    /// Writes within the Vault used as Codex's primary workspace.
    #[default]
    WorkspaceWrite,
    /// No fence at all. Never a default; a task has to ask for it in writing.
    DangerFullAccess,
}

impl PermissionMode {
    /// The value accepted by `codex exec --sandbox`.
    pub fn as_env(self) -> &'static str {
        match self {
            PermissionMode::ReadOnly => "read-only",
            PermissionMode::WorkspaceWrite => "workspace-write",
            PermissionMode::DangerFullAccess => "danger-full-access",
        }
    }
}

/// Shared policy compatibility; Codex runs themselves never ask interactively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnRequest {
    /// Approve. Correct for an unattended task already fenced by its mode — the
    /// sandbox has decided what is reachable; the prompt is a formality.
    Allow,
    /// Refuse. The run continues; the tool call gets a denial and the model has
    /// to work around it.
    #[default]
    Reject,
    /// Ask the person, if a window is open to ask in. With no window there is
    /// nobody to answer, so this degrades to `Reject` — never to `Allow`.
    Ask,
}

/// A task's `policy.json`. Every field optional: a task that ships no policy
/// gets the defaults, which are the conservative pair
/// (`workspace-write` + `reject`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub permission_mode: PermissionMode,
    pub on_permission_request: OnRequest,
    /// Free text shown in the window and written into the run log, so a person
    /// reading a record can see WHY this task runs as loosely as it does.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub rationale: String,
}

impl Policy {
    /// Read `<task_dir>/policy.json`.
    ///
    /// A malformed policy is an ERROR, not a fallback to defaults. Silently
    /// running a task under different permissions than its author wrote down is
    /// the one failure mode this file exists to prevent — and `deny_unknown_fields`
    /// means a typo like `"permision_mode"` is caught here rather than ignored.
    pub fn load(task_dir: &Path) -> Result<Policy, String> {
        let p = task_dir.join("policy.json");
        let Ok(body) = std::fs::read_to_string(&p) else {
            return Ok(Policy::default());
        };
        serde_json::from_str(&body)
            .map_err(|e| format!("{} is not a valid policy: {e}", p.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, body: &str) {
        std::fs::write(dir.join("policy.json"), body).unwrap();
    }

    /// A task that ships no policy must still be safe to run.
    #[test]
    fn a_task_without_a_policy_gets_the_conservative_defaults() {
        let d = tempfile::tempdir().unwrap();
        let p = Policy::load(d.path()).unwrap();
        assert_eq!(p.permission_mode, PermissionMode::WorkspaceWrite);
        assert_eq!(p.on_permission_request, OnRequest::Reject);
        assert_eq!(p, Policy::default());
    }

    #[test]
    fn reads_both_knobs_and_the_rationale() {
        let d = tempfile::tempdir().unwrap();
        write(
            d.path(),
            r#"{"permission_mode":"read-only","on_permission_request":"ask","rationale":"只读一篇笔记"}"#,
        );
        let p = Policy::load(d.path()).unwrap();
        assert_eq!(p.permission_mode, PermissionMode::ReadOnly);
        assert_eq!(p.on_permission_request, OnRequest::Ask);
        assert_eq!(p.rationale, "只读一篇笔记");
    }

    #[test]
    fn a_partial_policy_keeps_the_defaults_for_what_it_omits() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), r#"{"permission_mode":"danger-full-access"}"#);
        let p = Policy::load(d.path()).unwrap();
        assert_eq!(p.permission_mode, PermissionMode::DangerFullAccess);
        assert_eq!(p.on_permission_request, OnRequest::Reject);
    }

    /// Running under permissions the author did not write is exactly what this
    /// file exists to prevent, so a broken one stops the run.
    #[test]
    fn a_malformed_policy_is_an_error_rather_than_a_silent_default() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "{not json");
        let e = Policy::load(d.path()).unwrap_err();
        assert!(e.contains("not a valid policy"), "{e}");

        write(d.path(), r#"{"permission_mode":"wide-open"}"#);
        assert!(
            Policy::load(d.path()).is_err(),
            "an unknown mode must not pass"
        );
    }

    /// A typo would otherwise read as "the author said nothing", i.e. as the
    /// defaults — quietly ignoring what they actually asked for.
    #[test]
    fn a_misspelled_key_is_caught_rather_than_ignored() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), r#"{"permision_mode":"read-only"}"#);
        let e = Policy::load(d.path()).unwrap_err();
        assert!(e.contains("not a valid policy"), "{e}");
    }

    #[test]
    fn the_modes_serialize_as_the_sandboxs_own_vocabulary() {
        assert_eq!(PermissionMode::ReadOnly.as_env(), "read-only");
        assert_eq!(PermissionMode::WorkspaceWrite.as_env(), "workspace-write");
        assert_eq!(
            PermissionMode::DangerFullAccess.as_env(),
            "danger-full-access"
        );
    }

    #[test]
    fn a_policy_round_trips_through_json() {
        let p = Policy {
            permission_mode: PermissionMode::DangerFullAccess,
            on_permission_request: OnRequest::Allow,
            rationale: "全盘扫描".into(),
        };
        let back: Policy = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }
}
