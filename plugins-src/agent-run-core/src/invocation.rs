//! Process-local idempotency for host-started Agent tasks.
//!
//! A plugin process remains alive while the host polls a run, so this closes
//! the only ambiguity window that can otherwise double-charge a model call:
//! the start response being lost and retried with the same invocation id.

use serde_json::Value;
use std::collections::{HashMap, VecDeque};

const MAX_INVOCATIONS: usize = 1_024;

#[derive(Debug, Clone)]
pub struct InvocationIdentity {
    pub id: String,
    pub input_hash: String,
    input_fingerprint: String,
}

impl InvocationIdentity {
    pub fn from_context(context: &Value) -> Result<Option<Self>, String> {
        let id = context.get("invocation_id").and_then(Value::as_str);
        let input_hash = context.get("input_hash").and_then(Value::as_str);
        match (id, input_hash) {
            (None, None) => Ok(None),
            (Some(id), Some(input_hash))
                if !id.is_empty()
                    && id.len() <= 128
                    && id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
                    && input_hash.len() == 64
                    && input_hash.chars().all(|ch| ch.is_ascii_hexdigit()) =>
            {
                Ok(Some(Self {
                    id: id.to_string(),
                    input_hash: input_hash.to_ascii_lowercase(),
                    input_fingerprint: serde_json::json!({
                        "task": context.get("task"),
                        "prompt": context.get("prompt"),
                        "model_profile": context.get("model_profile"),
                        "model": context.get("model"),
                    })
                    .to_string(),
                }))
            }
            (Some(_), Some(_)) => Err("invalid invocation_id or input_hash".to_string()),
            _ => Err("invocation_id and input_hash must be supplied together".to_string()),
        }
    }
}

#[derive(Default)]
pub struct InvocationRegistry {
    entries: HashMap<String, (String, String, String, String)>,
    order: VecDeque<String>,
}

impl InvocationRegistry {
    /// Return the existing run, or atomically reserve this id for `run_id`.
    pub fn reuse_or_insert(
        &mut self,
        identity: Option<&InvocationIdentity>,
        task: &str,
        run_id: &str,
    ) -> Result<Option<String>, String> {
        let Some(identity) = identity else {
            return Ok(None);
        };
        if let Some((known_hash, known_fingerprint, known_task, known_run)) =
            self.entries.get(&identity.id)
        {
            if known_hash != &identity.input_hash
                || known_fingerprint != &identity.input_fingerprint
                || known_task != task
            {
                return Err("invocation_id was already used with different input".to_string());
            }
            return Ok(Some(known_run.clone()));
        }
        self.entries.insert(
            identity.id.clone(),
            (
                identity.input_hash.clone(),
                identity.input_fingerprint.clone(),
                task.to_string(),
                run_id.to_string(),
            ),
        );
        self.order.push_back(identity.id.clone());
        while self.entries.len() > MAX_INVOCATIONS {
            if let Some(expired) = self.order.pop_front() {
                self.entries.remove(&expired);
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn same_identity_reuses_one_run_and_changed_input_fails() {
        let first = InvocationIdentity::from_context(&json!({
            "invocation_id": "lookup-1", "input_hash": "a".repeat(64),
            "task": "search-plan", "prompt": "first", "model_profile": "fast",
        }))
        .unwrap()
        .unwrap();
        let changed = InvocationIdentity::from_context(&json!({
            "invocation_id": "lookup-1", "input_hash": "b".repeat(64),
            "task": "search-plan", "prompt": "first", "model_profile": "fast",
        }))
        .unwrap()
        .unwrap();
        let spoofed_hash = InvocationIdentity::from_context(&json!({
            "invocation_id": "lookup-1", "input_hash": "a".repeat(64),
            "task": "search-plan", "prompt": "different", "model_profile": "fast",
        }))
        .unwrap()
        .unwrap();
        let mut registry = InvocationRegistry::default();
        assert_eq!(
            registry
                .reuse_or_insert(Some(&first), "search-plan", "run-1")
                .unwrap(),
            None
        );
        assert_eq!(
            registry
                .reuse_or_insert(Some(&first), "search-plan", "run-2")
                .unwrap(),
            Some("run-1".to_string()),
        );
        assert!(registry
            .reuse_or_insert(Some(&changed), "search-plan", "run-3")
            .is_err());
        assert!(registry
            .reuse_or_insert(Some(&spoofed_hash), "search-plan", "run-4")
            .is_err());
    }

    #[test]
    fn registry_is_bounded_without_evicting_the_newest_invocation() {
        let mut registry = InvocationRegistry::default();
        for index in 0..=MAX_INVOCATIONS {
            let identity = InvocationIdentity {
                id: format!("lookup-{index}"),
                input_hash: format!("{index:064x}"),
                input_fingerprint: format!("fingerprint-{index}"),
            };
            registry
                .reuse_or_insert(Some(&identity), "search-plan", &format!("run-{index}"))
                .unwrap();
        }
        assert_eq!(registry.entries.len(), MAX_INVOCATIONS);
        assert!(!registry.entries.contains_key("lookup-0"));
        assert!(registry
            .entries
            .contains_key(&format!("lookup-{MAX_INVOCATIONS}")));
    }
}
