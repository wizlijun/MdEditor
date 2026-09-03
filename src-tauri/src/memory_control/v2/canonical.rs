use super::model::*;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

/// A schema-specific semantic payload that can be normalized before JCS.
///
/// YAML formatting and the root `payload_sha256` field are deliberately not
/// part of the semantic hash.  The raw file bytes have a second, independent
/// hash used by the immutable filename.
pub trait CanonicalPayload: Clone + Serialize {
    fn normalized(&self) -> Result<Self, String>;
    fn declared_payload_sha256(&self) -> &str;
    fn set_declared_payload_sha256(&mut self, value: String);
}

pub fn raw_sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn canonical_bytes<T: CanonicalPayload>(payload: &T) -> Result<Vec<u8>, String> {
    let normalized = payload.normalized()?;
    let value = serde_json::to_value(normalized)
        .map_err(|error| format!("memory v2: canonical JSON value: {error}"))?;
    let mut value = normalize_unicode(value)?;
    let object = value
        .as_object_mut()
        .ok_or("memory v2: canonical payload root must be an object")?;
    object.remove("payload_sha256");
    let mut out = String::new();
    write_jcs(&value, &mut out)?;
    Ok(out.into_bytes())
}

fn normalize_unicode(value: Value) -> Result<Value, String> {
    match value {
        Value::String(value) => Ok(Value::String(value.nfc().collect())),
        Value::Array(values) => values
            .into_iter()
            .map(normalize_unicode)
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(values) => {
            let mut normalized = serde_json::Map::new();
            for (key, value) in values {
                let key = key.nfc().collect::<String>();
                if normalized.contains_key(&key) {
                    return Err(format!(
                        "memory v2: Unicode normalization produces duplicate key {key}"
                    ));
                }
                normalized.insert(key, normalize_unicode(value)?);
            }
            Ok(Value::Object(normalized))
        }
        other => Ok(other),
    }
}

pub fn payload_sha256<T: CanonicalPayload>(payload: &T) -> Result<String, String> {
    Ok(raw_sha256(&canonical_bytes(payload)?))
}

fn write_jcs(value: &Value, out: &mut String) -> Result<(), String> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => {
            if !value.is_i64() && !value.is_u64() {
                return Err("memory v2: floating-point values are not canonical".into());
            }
            out.push_str(&value.to_string());
        }
        Value::String(value) => out.push_str(
            &serde_json::to_string(value)
                .map_err(|error| format!("memory v2: canonical JSON string: {error}"))?,
        ),
        Value::Array(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_jcs(value, out)?;
            }
            out.push(']');
        }
        Value::Object(values) => {
            out.push('{');
            let sorted = values.iter().collect::<BTreeMap<_, _>>();
            for (index, (key, value)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(
                    &serde_json::to_string(key)
                        .map_err(|error| format!("memory v2: canonical JSON key: {error}"))?,
                );
                out.push(':');
                write_jcs(value, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

fn normalize_text(value: &str) -> String {
    let line_endings = value.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = line_endings.lines().map(str::trim_end).collect::<Vec<_>>();
    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

fn sort_dedup<T: Serialize>(values: &mut Vec<T>) -> Result<(), String> {
    let mut decorated = values
        .drain(..)
        .map(|value| {
            let json = serde_json::to_value(&value)
                .map_err(|error| format!("memory v2: normalize set: {error}"))?;
            let json = normalize_unicode(json)?;
            let mut key = String::new();
            write_jcs(&json, &mut key)?;
            Ok::<(String, T), String>((key, value))
        })
        .collect::<Result<Vec<_>, _>>()?;
    decorated.sort_by(|left, right| left.0.cmp(&right.0));
    decorated.dedup_by(|left, right| left.0 == right.0);
    values.extend(decorated.into_iter().map(|(_, value)| value));
    Ok(())
}

fn normalize_refs(values: &mut Vec<RevisionRef>) {
    values.sort();
    values.dedup();
}

fn normalize_record_refs(values: &mut Vec<RecordRef>) {
    values.sort();
    values.dedup();
}

fn normalize_claim_context(context: &mut ClaimContext) {
    context.roles.sort();
    context.roles.dedup();
    context.spaces.sort();
    context.spaces.dedup();
    context.applies_when.sort();
    context.applies_when.dedup();
    context.excludes_when.sort();
    context.excludes_when.dedup();
}

impl CanonicalPayload for ProtocolRevision {
    fn normalized(&self) -> Result<Self, String> {
        let mut next = self.clone();
        normalize_refs(&mut next.base_heads);
        normalize_record_refs(&mut next.causal_context.parents);
        normalize_refs(&mut next.decision.authority_context.heads);
        for categories in next.category_registry.values_mut() {
            categories.dedup();
        }
        Ok(next)
    }

    fn declared_payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    fn set_declared_payload_sha256(&mut self, value: String) {
        self.payload_sha256 = value;
    }
}

impl CanonicalPayload for AuthorityRevision {
    fn normalized(&self) -> Result<Self, String> {
        let mut next = self.clone();
        normalize_refs(&mut next.base_heads);
        normalize_record_refs(&mut next.causal_context.parents);
        normalize_refs(&mut next.decision.authority_context.heads);
        for principal in &mut next.principals {
            principal.capabilities.sort();
            principal.capabilities.dedup();
        }
        next.principals
            .sort_by(|left, right| left.actor_id.cmp(&right.actor_id));
        if let Recovery::Quorum { principals, .. } = &mut next.recovery {
            principals.sort();
            principals.dedup();
        }
        Ok(next)
    }

    fn declared_payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    fn set_declared_payload_sha256(&mut self, value: String) {
        self.payload_sha256 = value;
    }
}

impl CanonicalPayload for ContextRegistryRevision {
    fn normalized(&self) -> Result<Self, String> {
        let mut next = self.clone();
        normalize_refs(&mut next.base_heads);
        normalize_record_refs(&mut next.causal_context.parents);
        normalize_refs(&mut next.decision.protocol_context.heads);
        normalize_refs(&mut next.decision.authority_context.heads);
        for role in &mut next.roles {
            role.description = normalize_text(&role.description);
            role.aliases.sort();
            role.aliases.dedup();
            role.agent_use.guidance = normalize_text(&role.agent_use.guidance);
            role.agent_use.avoid_error = normalize_text(&role.agent_use.avoid_error);
        }
        next.roles
            .sort_by(|left, right| left.role_id.cmp(&right.role_id));
        for scope in &mut next.scopes {
            scope.description = normalize_text(&scope.description);
            scope.aliases.sort();
            scope.aliases.dedup();
            scope.agent_use.guidance = normalize_text(&scope.agent_use.guidance);
            scope.agent_use.avoid_error = normalize_text(&scope.agent_use.avoid_error);
        }
        next.scopes
            .sort_by(|left, right| left.scope_id.cmp(&right.scope_id));
        Ok(next)
    }

    fn declared_payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    fn set_declared_payload_sha256(&mut self, value: String) {
        self.payload_sha256 = value;
    }
}

impl CanonicalPayload for MemoryClaimRevision {
    fn normalized(&self) -> Result<Self, String> {
        let mut next = self.clone();
        normalize_refs(&mut next.parents);
        normalize_record_refs(&mut next.causal_context.parents);
        if let Some(decision) = &mut next.decision {
            normalize_refs(&mut decision.protocol_context.heads);
            normalize_refs(&mut decision.authority_context.heads);
        }
        next.text = normalize_text(&next.text);
        next.agent_use.guidance = normalize_text(&next.agent_use.guidance);
        next.agent_use.avoid_error = normalize_text(&next.agent_use.avoid_error);
        normalize_claim_context(&mut next.context);
        next.consent.allowed_purposes.sort();
        next.consent.allowed_purposes.dedup();
        sort_dedup(&mut next.asserted_by)?;
        sort_dedup(&mut next.evidence)?;
        sort_dedup(&mut next.lineage.derived_from)?;
        if let KindData::Boundary(boundary) = &mut next.kind_data {
            boundary.behavior_policy.actions.sort();
            boundary.behavior_policy.actions.dedup();
            boundary.behavior_policy.resources.sort();
            boundary.behavior_policy.resources.dedup();
            boundary.behavior_policy.conditions.sort();
            boundary.behavior_policy.conditions.dedup();
        }
        Ok(next)
    }

    fn declared_payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    fn set_declared_payload_sha256(&mut self, value: String) {
        self.payload_sha256 = value;
    }
}

impl CanonicalPayload for MemoryOperation {
    fn normalized(&self) -> Result<Self, String> {
        let mut next = self.clone();
        normalize_record_refs(&mut next.causal_context.parents);
        if !next.merge_inputs.is_empty() {
            normalize_refs(&mut next.merge_inputs.target.base_heads);
            for source in &mut next.merge_inputs.sources {
                normalize_refs(&mut source.base_heads);
            }
            next.merge_inputs
                .sources
                .sort_by(|left, right| left.claim_id.cmp(&right.claim_id));
        }
        next.effects
            .sort_by(|left, right| left.claim_id.cmp(&right.claim_id));
        if let Some(reassign) = &mut next.reassign_context {
            reassign
                .changes
                .sort_by(|left, right| left.claim_id.cmp(&right.claim_id));
            for change in &mut reassign.changes {
                normalize_claim_context(&mut change.from_context);
                normalize_claim_context(&mut change.to_context);
            }
        }
        sort_dedup(&mut next.lineage)?;
        normalize_refs(&mut next.decision.protocol_context.heads);
        normalize_refs(&mut next.decision.authority_context.heads);
        Ok(next)
    }

    fn declared_payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    fn set_declared_payload_sha256(&mut self, value: String) {
        self.payload_sha256 = value;
    }
}

impl CanonicalPayload for ContextManifest {
    fn normalized(&self) -> Result<Self, String> {
        let mut next = self.clone();
        normalize_record_refs(&mut next.causal_context.parents);
        normalize_refs(&mut next.protocol_context.heads);
        normalize_refs(&mut next.authority_context.heads);
        next.request.tools.sort();
        next.request.tools.dedup();
        next.selected.sort_by(|left, right| {
            (&left.claim_id, &left.revision_id).cmp(&(&right.claim_id, &right.revision_id))
        });
        for selected in &mut next.selected {
            selected.reasons.sort();
            selected.reasons.dedup();
        }
        next.conflicts
            .sort_by(|left, right| left.conflict_id.cmp(&right.conflict_id));
        Ok(next)
    }

    fn declared_payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    fn set_declared_payload_sha256(&mut self, value: String) {
        self.payload_sha256 = value;
    }
}

pub fn canonical_yaml<T: CanonicalPayload>(payload: &T) -> Result<(T, Vec<u8>), String> {
    let mut normalized = payload.normalized()?;
    let digest = payload_sha256(&normalized)?;
    normalized.set_declared_payload_sha256(digest);
    let yaml = serde_yaml::to_string(&normalized)
        .map_err(|error| format!("memory v2: canonical YAML: {error}"))?
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    Ok((normalized, yaml.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn protocol() -> ProtocolRevision {
        ProtocolRevision {
            schema: "notemd.memory/protocol-revision/v2".into(),
            revision_id: "01900000-0000-7000-8000-000000000001".into(),
            base_heads: vec![],
            causal_context: CausalContext::default(),
            protocol_major: 2,
            protocol_minor: 0,
            renderer_version: "notemd.memory.projector/2".into(),
            claim_schema: "notemd.memory/claim-revision/v2".into(),
            category_registry: BTreeMap::from([
                (
                    "memory".into(),
                    vec!["decisions".into(), "constraints".into()],
                ),
                ("user".into(), vec!["owner".into(), "preferences".into()]),
            ]),
            decision: ControlDecision {
                verdict: Verdict::Approve,
                actor_id: "human:bruce".into(),
                authority_context: AuthorityContext {
                    heads: vec![],
                    capability: "memory.protocol.modify".into(),
                },
            },
            transition: ControlTransition {
                operation: ControlOperation::Initialize,
            },
            payload_sha256: "ignored".into(),
        }
    }

    #[test]
    fn jcs_is_stable_and_excludes_declared_hash() {
        let first = protocol();
        let mut second = first.clone();
        second.payload_sha256 = "different".into();
        assert_eq!(
            canonical_bytes(&first).unwrap(),
            canonical_bytes(&second).unwrap()
        );
        assert_eq!(
            payload_sha256(&first).unwrap(),
            payload_sha256(&second).unwrap()
        );
        let text = String::from_utf8(canonical_bytes(&first).unwrap()).unwrap();
        assert!(text.starts_with("{\"base_heads\":"), "{text}");
        assert!(!text.contains("payload_sha256"));
        assert!(!text.ends_with('\n'));
    }

    #[test]
    fn set_like_refs_are_order_independent() {
        let mut first = protocol();
        first.base_heads = vec![
            RevisionRef {
                revision_id: "b".into(),
                payload_sha256: "2".into(),
            },
            RevisionRef {
                revision_id: "a".into(),
                payload_sha256: "1".into(),
            },
        ];
        let mut second = first.clone();
        second.base_heads.reverse();
        assert_eq!(
            payload_sha256(&first).unwrap(),
            payload_sha256(&second).unwrap()
        );
    }

    #[test]
    fn empty_roles_preserve_the_frozen_claim_canonical_bytes() {
        let claim: MemoryClaimRevision = serde_yaml::from_str(include_str!(
            "../../../tests/fixtures/memory-v2/canonical/claim-payload.yaml"
        ))
        .unwrap();
        assert!(claim.context.roles.is_empty());
        let expected = include_str!(
            "../../../tests/fixtures/memory-v2/canonical/claim-payload.canonical.json"
        )
        .trim();
        assert_eq!(
            String::from_utf8(canonical_bytes(&claim).unwrap()).unwrap(),
            expected
        );
    }

    #[test]
    fn absent_request_role_stays_absent_on_the_wire() {
        let request: ContextRequest = serde_json::from_value(serde_json::json!({
            "space": "global",
            "purpose": "planning",
            "caller": "test",
            "provider": "local",
            "model": "test",
            "tools": [],
            "external_transfer": false,
            "as_of_valid_time": "2026-09-03T00:00:00Z"
        }))
        .unwrap();
        assert_eq!(request.role, None);
        assert!(serde_json::to_value(request).unwrap().get("role").is_none());
    }
}
