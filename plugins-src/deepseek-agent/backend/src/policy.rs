//! What a run is allowed to touch.
//!
//! ## Why this is not a path allowlist
//!
//! The obvious design — `"allow": ["write:${VAULT}/**/*.note.md"]`, matched
//! against each request — cannot be built on this protocol. The ACP server sends
//! `session/request_permission` with `toolCall: { toolCallId }` and **nothing
//! else** (`packages/acp/acp/src/index.ts`): no tool name, no path, no
//! arguments. A client that matched paths would be matching on data it never
//! receives.
//!
//! So the real enforcement lives where it can actually see the filesystem: the
//! harness's own sandbox. `dsh-sandbox-policy` fences bash AND the fs tools by
//! mode, and `dsh-user-approval` decides whether a fenced action asks first.
//! Both read their mode from `DSH_PERMISSION_MODE` in the composition we ship
//! (`templates/_dsh/cordis.yml`), so a task setting that variable is setting a
//! real, kernel-adjacent boundary rather than an honour-system rule.
//!
//! That leaves exactly one decision for us: when the sandbox does ask, what do we
//! answer? `on_permission_request` says so, and it is fail-closed.
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The sandbox preset a run is confined to. These are `dsh-sandbox-policy`'s own
/// three modes, passed through verbatim — inventing our own vocabulary here
/// would only add a translation layer that can drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PermissionMode {
    /// Reads only. Nothing on disk changes.
    ReadOnly,
    /// Reads anywhere, writes confined to the session workspace (the task
    /// directory) plus temp. The default: an agent that cannot write cannot
    /// answer a question into a `.note.md`.
    #[default]
    WorkspaceWrite,
    /// No fence at all. Never a default; a task has to ask for it in writing.
    DangerFullAccess,
}

impl PermissionMode {
    /// The value `cordis.yml` reads out of the environment.
    pub fn as_env(self) -> &'static str {
        match self {
            PermissionMode::ReadOnly => "read-only",
            PermissionMode::WorkspaceWrite => "workspace-write",
            PermissionMode::DangerFullAccess => "danger-full-access",
        }
    }
}

/// What to answer when the harness asks for approval mid-run.
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
        serde_json::from_str(&body).map_err(|e| format!("{} is not a valid policy: {e}", p.display()))
    }

    /// How a permission request should be answered right now. `window` says
    /// whether there is a person to ask.
    pub fn decide(&self, window_open: bool) -> Outcome {
        match self.on_permission_request {
            OnRequest::Allow => Outcome::Allow,
            OnRequest::Reject => Outcome::Reject,
            OnRequest::Ask if window_open => Outcome::Ask,
            // Nobody to ask. Fail closed: an unattended run must not be able to
            // widen its own permissions by asking a question no one hears.
            OnRequest::Ask => Outcome::Reject,
        }
    }
}

/// The resolved answer for one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Allow,
    Reject,
    /// Put it to the person in the plugin window.
    Ask,
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
        assert!(Policy::load(d.path()).is_err(), "an unknown mode must not pass");
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
    fn allow_and_reject_do_not_depend_on_a_window() {
        let mut p = Policy::default();
        p.on_permission_request = OnRequest::Allow;
        assert_eq!(p.decide(true), Outcome::Allow);
        assert_eq!(p.decide(false), Outcome::Allow);
        p.on_permission_request = OnRequest::Reject;
        assert_eq!(p.decide(true), Outcome::Reject);
        assert_eq!(p.decide(false), Outcome::Reject);
    }

    /// The fail-closed rule: an unattended run cannot widen its own permissions
    /// by asking a question nobody is there to hear.
    #[test]
    fn ask_without_a_window_rejects_rather_than_allowing() {
        let mut p = Policy::default();
        p.on_permission_request = OnRequest::Ask;
        assert_eq!(p.decide(true), Outcome::Ask);
        assert_eq!(p.decide(false), Outcome::Reject);
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
