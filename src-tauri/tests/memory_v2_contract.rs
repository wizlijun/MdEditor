use notemd_lib::memory_control::v2::{
    canonical_bytes as runtime_canonical_bytes, MemoryClaimRevision,
};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/memory-v2")
        .join(relative)
}

fn read_json(relative: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(fixture(relative)).unwrap()).unwrap()
}

fn read_yaml(relative: &str) -> Value {
    serde_yaml::from_str(&fs::read_to_string(fixture(relative)).unwrap()).unwrap()
}

fn assert_top_level_schema(schema: &Value, document: &Value, schema_name: &str) {
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(document["schema"], schema["properties"]["schema"]["const"]);

    let object = document.as_object().expect("fixture must be a mapping");
    let properties = schema["properties"].as_object().unwrap();
    for required in schema["required"].as_array().unwrap() {
        let required = required.as_str().unwrap();
        assert!(
            object.contains_key(required),
            "{schema_name} misses required `{required}`"
        );
    }
    for key in object.keys() {
        assert!(
            properties.contains_key(key),
            "{schema_name} has unknown top-level key `{key}`"
        );
    }
}

fn value_at_mut<'a>(root: &'a mut Value, path: &str) -> &'a mut Value {
    let mut current = root;
    for segment in path.split('.') {
        current = current
            .as_object_mut()
            .and_then(|object| object.get_mut(segment))
            .unwrap_or_else(|| panic!("fixture patch path does not exist: {path}"));
    }
    current
}

fn apply_fixture_patch(target: &mut Value, patch: &Value) {
    for operation in patch["operations"].as_array().unwrap() {
        if let Some(path) = operation.get("reverse").and_then(Value::as_str) {
            value_at_mut(target, path).as_array_mut().unwrap().reverse();
        } else if let Some(change) = operation.get("replace_text") {
            let path = change["path"].as_str().unwrap();
            *value_at_mut(target, path) = change["value"].clone();
        } else if let Some(change) = operation.get("replace") {
            let path = change["path"].as_str().unwrap();
            *value_at_mut(target, path) = change["value"].clone();
        } else {
            panic!("unknown canonical fixture patch operation: {operation}");
        }
    }
}

fn is_set_array(path: &[String]) -> bool {
    matches!(
        path.join(".").as_str(),
        "parents"
            | "causal_context.parents"
            | "asserted_by"
            | "context.spaces"
            | "context.applies_when"
            | "context.excludes_when"
            | "consent.allowed_purposes"
            | "evidence"
            | "lineage.derived_from"
            | "decision.protocol_context.heads"
            | "decision.authority_context.heads"
    )
}

fn is_text_field(path: &[String]) -> bool {
    matches!(
        path.last().map(String::as_str),
        Some("text" | "guidance" | "avoid_error" | "title")
    )
}

fn normalize_fixture_string(input: &str, text: bool) -> String {
    // The golden vector deliberately contains the decomposed e + acute form.
    // Production code must use full Unicode NFC; this dependency-free contract
    // helper composes the code-point pair exercised by the published vector.
    let mut value = input.replace("e\u{301}", "é");
    if text {
        value = value.replace("\r\n", "\n").replace('\r', "\n");
        value = value
            .split('\n')
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n")
            .trim_matches('\n')
            .to_owned();
    }
    value
}

fn canonicalize(value: Value, path: &mut Vec<String>) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().cloned().collect();
            keys.sort();
            let mut result = Map::new();
            for key in keys {
                if path.is_empty() && key == "payload_sha256" {
                    continue;
                }
                let mut child_path = path.clone();
                child_path.push(key.clone());
                result.insert(
                    key.clone(),
                    canonicalize(object[&key].clone(), &mut child_path),
                );
            }
            Value::Object(result)
        }
        Value::Array(array) => {
            let mut normalized: Vec<_> = array
                .into_iter()
                .map(|item| canonicalize(item, &mut path.clone()))
                .collect();
            if is_set_array(path) {
                normalized.sort_by_key(|item| serde_json::to_string(item).unwrap());
            }
            Value::Array(normalized)
        }
        Value::String(value) => {
            Value::String(normalize_fixture_string(&value, is_text_field(path)))
        }
        other => other,
    }
}

fn canonical_bytes(value: Value) -> Vec<u8> {
    serde_json::to_vec(&canonicalize(value, &mut Vec::new())).unwrap()
}

#[test]
fn v2_schemas_and_yaml_fixtures_share_strict_top_level_contracts() {
    for (schema_path, fixture_path, name) in [
        (
            "schemas/bootstrap.schema.json",
            "valid/bootstrap.yaml",
            "bootstrap",
        ),
        (
            "schemas/protocol-revision.schema.json",
            "valid/protocol-revision.yaml",
            "protocol revision",
        ),
        (
            "schemas/authority-revision.schema.json",
            "valid/authority-revision.yaml",
            "authority revision",
        ),
        (
            "schemas/claim-revision.schema.json",
            "canonical/claim-payload.yaml",
            "Claim revision",
        ),
    ] {
        assert_top_level_schema(&read_json(schema_path), &read_yaml(fixture_path), name);
    }

    let claim_schema = read_json("schemas/claim-revision.schema.json");
    assert!(
        !claim_schema.to_string().contains("migration"),
        "pure v2 Claim schema must not retain migration-only identities or evidence bases"
    );
    let definitions = claim_schema["$defs"].as_object().unwrap();
    for definition in ["kindData", "decision", "transition", "evidence"] {
        assert!(
            definitions.contains_key(definition),
            "Claim schema misses definition `{definition}`"
        );
    }
    assert_eq!(
        definitions["kindData"]["properties"]
            .as_object()
            .unwrap()
            .len(),
        10,
        "every RFC 0.10 Claim kind must have a tagged payload schema"
    );

    let claim = read_yaml("canonical/claim-payload.yaml");
    let kind_data = claim["kind_data"].as_object().unwrap();
    assert_eq!(
        kind_data.len(),
        1,
        "kind_data must be a one-member tagged union"
    );
    assert!(kind_data.contains_key(claim["claim_kind"].as_str().unwrap()));
    assert_ne!(
        claim["sensitivity"], "restricted",
        "restricted plaintext must not be Git-backed"
    );
    assert!(claim["context"]["spaces"]
        .as_array()
        .is_some_and(|spaces| !spaces.is_empty()));

    let mut pending = claim;
    apply_fixture_patch(&mut pending, &read_yaml("valid/claim-pending.patch.yaml"));
    assert_eq!(pending["workflow"]["state"], "pending");
    assert!(pending["decision"].is_null());
    assert_eq!(pending["transition"]["operation"], "propose-create");
    assert!(pending["transition"]["approves_revision_id"].is_null());
    assert!(pending["transition"]["approves_payload_sha256"].is_null());
}

#[test]
fn canonical_claim_golden_vector_is_stable_and_transport_independent() {
    let source = read_yaml("canonical/claim-payload.yaml");
    let canonical = canonical_bytes(source.clone());
    let expected_canonical = fs::read_to_string(fixture("canonical/claim-payload.canonical.json"))
        .unwrap()
        .trim_end_matches('\n')
        .as_bytes()
        .to_vec();
    let expected = read_json("canonical/claim-payload.expected.json");

    assert_eq!(canonical, expected_canonical);
    assert_eq!(
        canonical.len() as u64,
        expected["canonical_bytes"].as_u64().unwrap()
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&canonical)),
        expected["payload_sha256"]
    );

    let mut equivalent = source.clone();
    apply_fixture_patch(
        &mut equivalent,
        &read_yaml("canonical/claim-payload-equivalent.patch.yaml"),
    );
    assert_eq!(
        canonical_bytes(equivalent),
        canonical,
        "set order, CRLF, trailing whitespace and NFC must not affect the hash"
    );

    let mut changed = source;
    apply_fixture_patch(
        &mut changed,
        &read_yaml("canonical/claim-payload-changed.patch.yaml"),
    );
    assert_ne!(
        canonical_bytes(changed),
        canonical,
        "a semantic field change must affect the hash"
    );
}

#[test]
fn runtime_canonicalizer_matches_the_published_claim_vector() {
    let claim: MemoryClaimRevision =
        serde_yaml::from_str(&fs::read_to_string(fixture("canonical/claim-payload.yaml")).unwrap())
            .unwrap();
    let actual = runtime_canonical_bytes(&claim).unwrap();
    let expected = fs::read_to_string(fixture("canonical/claim-payload.canonical.json"))
        .unwrap()
        .trim_end_matches('\n')
        .as_bytes()
        .to_vec();
    if actual != expected {
        let offset = actual
            .iter()
            .zip(&expected)
            .position(|(left, right)| left != right)
            .unwrap_or(actual.len().min(expected.len()));
        let start = offset.saturating_sub(80);
        let actual_end = (offset + 160).min(actual.len());
        let expected_end = (offset + 160).min(expected.len());
        panic!(
            "runtime vector differs at byte {offset}\nactual: {}\nexpected: {}",
            String::from_utf8_lossy(&actual[start..actual_end]),
            String::from_utf8_lossy(&expected[start..expected_end])
        );
    }
}

#[test]
fn projection_templates_are_plain_text_and_agents_template_points_to_v2_authority() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
    let user = fs::read_to_string(root.join("USER.md")).unwrap();
    let memory = fs::read_to_string(root.join("MEMORY.md")).unwrap();
    let agents = fs::read_to_string(root.join("AGENTS.md")).unwrap();

    assert_eq!(user, "# USER\n");
    assert_eq!(memory, "# MEMORY\n");
    for projection in [&user, &memory] {
        assert!(!projection.starts_with("---"));
        assert!(!projection.contains("::"));
        assert!(!projection.contains("[^"));
    }
    for rule in [
        "only authoritative memory data",
        "notemd memory owner --json",
        "Never parse owner identity from `/USER.md`",
        "notemd memory context --space",
        "notemd memory propose",
        "no YAML\n  frontmatter",
        "Do not store facts whose subject is another person",
        "do not ask for a second confirmation",
        "Delete creates a tombstone",
        "Tasks and reminders remain one file per Task under `/inbox/tasks/`",
    ] {
        assert!(agents.contains(rule), "AGENTS.md misses v2 rule: {rule}");
    }
    assert!(!agents.contains("`owner.actor`"));
    assert!(!agents.contains("`owner.names`"));
    assert!(!agents.contains("/inbox/memory-candidates/"));
    assert!(!agents.contains("/memory/events/"));
}
