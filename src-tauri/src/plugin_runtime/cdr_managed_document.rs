//! Durable local adapter for one managed Markdown representation plus an
//! opaque collaborative-document aggregate.
//!
//! The Vault file and app-data envelope live in different directories, so no
//! filesystem primitive can replace both atomically. This adapter provides a
//! single CDR-visible commit by writing a durable prepared journal first and
//! deterministically recovering it on every inspect/load/commit.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use super::cdr_repository::{
    aggregate_filename, exact_object, plugin_repository_directory, required_document_id,
    validate_aggregate_size, validate_plugin_id, MAX_AGGREGATE_BYTES, MAX_SAFE_GENERATION,
    REPOSITORY_LOCK,
};

const SCHEMA_VERSION: u32 = 1;
const MAX_VAULT_PATH_BYTES: usize = 4 * 1024;
const MAX_MARKDOWN_BYTES: usize = 16 * 1024 * 1024;
const MAX_STORED_BYTES: u64 = (MAX_AGGREGATE_BYTES + MAX_MARKDOWN_BYTES + 128 * 1024) as u64;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ManagedEnvelope {
    schema_version: u32,
    plugin_id: String,
    document_id: String,
    generation: u64,
    vault_path: String,
    representation_sha256: String,
    aggregate: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct LocatorBinding {
    schema_version: u32,
    plugin_id: String,
    document_id: String,
    vault_path: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PreparedTransaction {
    schema_version: u32,
    plugin_id: String,
    document_id: String,
    vault_path: String,
    expected_generation: u64,
    expected_representation_sha256: Option<String>,
    next: ManagedEnvelope,
    markdown: String,
}

#[derive(Clone, Debug, PartialEq)]
enum ExpectedRepresentation {
    Missing,
    Present(String),
}

#[derive(Clone, Debug)]
enum DiskState {
    Missing,
    Present { markdown: String, sha256: String },
}

#[derive(Debug)]
struct FrontmatterIdentity {
    document_id: String,
    profile_type: String,
}

#[derive(Debug)]
enum ReplaceMarkdownError {
    NotWritten(String),
    OutcomeUnknown(String),
}

/// Resolve a fixed managed-document slot without trusting a client-supplied
/// document id. A durable locator binding lets deletion be reported as drift
/// instead of being mistaken for a never-created document.
pub fn inspect(
    app_data_root: &Path,
    vault_root: &Path,
    plugin_id: &str,
    params: &Value,
) -> Result<Value, String> {
    validate_plugin_id(plugin_id)?;
    let object = exact_object(params, &["vault_path"])?;
    let vault_path = required_vault_path(object.get("vault_path"))?;
    let document_path = resolve_managed_path(vault_root, vault_path, false)?;
    let _guard = REPOSITORY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    recover_for_path(app_data_root, vault_root, plugin_id, vault_path)?;

    let directory = repository_directory(app_data_root, vault_root, plugin_id, false)?;
    let binding = match &directory {
        Some(directory) => read_binding(directory, plugin_id, vault_path)?,
        None => None,
    };
    let disk = read_disk(&document_path)?;
    let disk_document_id = match &disk {
        DiskState::Missing => None,
        DiskState::Present { markdown, .. } => Some(document_id_from_markdown(markdown)?),
    };

    match (binding, disk_document_id) {
        (None, None) => Ok(json!({ "kind": "missing" })),
        (Some(binding), None) => Ok(json!({
            "kind": "located",
            "document_id": binding.document_id,
        })),
        (None, Some(_)) => Err(
            "identity_conflict: managed Markdown exists without a repository locator binding"
                .to_owned(),
        ),
        (Some(binding), Some(document_id)) if binding.document_id == document_id => Ok(json!({
            "kind": "located",
            "document_id": document_id,
        })),
        (Some(_), Some(_)) => Err(
            "identity_conflict: frontmatter document id does not match the locator binding"
                .to_owned(),
        ),
    }
}

/// Load a managed aggregate and report whether its committed Markdown bytes
/// are still present. Drift is returned as data so the caller can render the
/// committed head read-only while preserving the external bytes on disk.
pub fn load(
    app_data_root: &Path,
    vault_root: &Path,
    plugin_id: &str,
    params: &Value,
) -> Result<Value, String> {
    validate_plugin_id(plugin_id)?;
    let object = exact_object(params, &["document_id", "vault_path"])?;
    let document_id = required_document_id(object.get("document_id"))?;
    let vault_path = required_vault_path(object.get("vault_path"))?;
    let document_path = resolve_managed_path(vault_root, vault_path, false)?;
    let _guard = REPOSITORY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    recover_for_path(app_data_root, vault_root, plugin_id, vault_path)?;

    let Some(directory) = repository_directory(app_data_root, vault_root, plugin_id, false)? else {
        return Ok(json!({ "kind": "missing" }));
    };
    let binding = read_binding(&directory, plugin_id, vault_path)?
        .ok_or_else(|| "identity_conflict: locator binding is missing".to_owned())?;
    if binding.document_id != document_id {
        return Err(
            "identity_conflict: requested document id does not match locator binding".into(),
        );
    }
    let envelope = read_managed_envelope(&directory, plugin_id, document_id)?.ok_or_else(|| {
        "identity_conflict: locator binding points to a missing aggregate".to_owned()
    })?;
    if envelope.vault_path != vault_path {
        return Err("identity_conflict: aggregate locator does not match requested path".into());
    }

    let representation = representation_payload(
        &read_disk(&document_path)?,
        document_id,
        vault_path,
        &envelope.representation_sha256,
    )?;
    Ok(json!({
        "kind": "loaded",
        "generation": envelope.generation,
        "aggregate": envelope.aggregate,
        "representation": representation,
    }))
}

/// Compare-and-swap an opaque aggregate with an optimistic Markdown precondition.
/// A stale aggregate or drift detected before replacement performs no write.
/// Stage 0 assumes cooperative writers: a process that ignores this repository
/// can still race the final existing-file rename.
pub fn commit(
    app_data_root: &Path,
    vault_root: &Path,
    plugin_id: &str,
    params: &Value,
) -> Result<Value, String> {
    validate_plugin_id(plugin_id)?;
    let object = exact_object(
        params,
        &[
            "document_id",
            "expected_generation",
            "aggregate",
            "representation",
        ],
    )?;
    let document_id = required_document_id(object.get("document_id"))?;
    let expected_generation = object
        .get("expected_generation")
        .and_then(Value::as_u64)
        .filter(|value| *value <= MAX_SAFE_GENERATION)
        .ok_or_else(|| "invalid_params: expected_generation is invalid".to_owned())?;
    let aggregate = object
        .get("aggregate")
        .filter(|value| value.is_object())
        .ok_or_else(|| "invalid_params: aggregate must be an object".to_owned())?;
    validate_aggregate_size(aggregate)?;
    let representation = exact_object(
        object
            .get("representation")
            .ok_or_else(|| "invalid_params: representation is required".to_owned())?,
        &["vault_path", "expected", "markdown"],
    )?;
    let vault_path = required_vault_path(representation.get("vault_path"))?;
    let expected = parse_expected(representation.get("expected"))?;
    let markdown = representation
        .get("markdown")
        .and_then(Value::as_str)
        .ok_or_else(|| "invalid_params: representation.markdown must be a string".to_owned())?;
    if markdown.len() > MAX_MARKDOWN_BYTES {
        return Err(format!(
            "too_large: Markdown exceeds {MAX_MARKDOWN_BYTES} bytes"
        ));
    }
    if document_id_from_markdown(markdown)? != document_id {
        return Err(
            "identity_conflict: representation frontmatter does not match document id".into(),
        );
    }

    let _guard = REPOSITORY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    recover_for_path(app_data_root, vault_root, plugin_id, vault_path)?;
    let directory = repository_directory(app_data_root, vault_root, plugin_id, true)?
        .ok_or_else(|| "io: managed repository disappeared after creation".to_owned())?;
    let document_path = resolve_managed_path(vault_root, vault_path, true)?;
    let current = read_managed_envelope(&directory, plugin_id, document_id)?;
    let binding = read_binding(&directory, plugin_id, vault_path)?;

    if expected_generation == 0 {
        if current.is_some() || binding.is_some() {
            return Ok(aggregate_conflict_payload(
                current.as_ref(),
                binding.as_ref(),
            ));
        }
        if !matches!(expected, ExpectedRepresentation::Missing) {
            return Err(
                "invalid_params: initial commit must expect a missing representation".into(),
            );
        }
        if !matches!(read_disk(&document_path)?, DiskState::Missing) {
            return Ok(external_drift_payload(
                None,
                &read_disk(&document_path)?,
                vault_path,
            ));
        }
    } else {
        let Some(current) = current.as_ref() else {
            return Err("corrupt: expected aggregate is missing".into());
        };
        if current.vault_path != vault_path {
            return Err("identity_conflict: aggregate locator cannot change during commit".into());
        }
        let Some(binding) = binding.as_ref() else {
            return Err("identity_conflict: locator binding is missing".into());
        };
        if binding.document_id != document_id {
            return Err("identity_conflict: locator binding points to another document".into());
        }
        if current.generation != expected_generation {
            return Ok(aggregate_conflict_payload(Some(current), Some(binding)));
        }
        let expected_hash = match &expected {
            ExpectedRepresentation::Present(hash) => hash,
            ExpectedRepresentation::Missing => {
                return Err(
                    "invalid_params: existing commit must expect a present representation".into(),
                )
            }
        };
        if expected_hash != &current.representation_sha256 {
            return Ok(aggregate_conflict_payload(Some(current), Some(binding)));
        }
        let disk = read_disk(&document_path)?;
        if !disk_matches_hash(&disk, expected_hash) {
            return Ok(external_drift_payload(Some(current), &disk, vault_path));
        }
    }

    let generation = expected_generation
        .checked_add(1)
        .filter(|value| *value <= MAX_SAFE_GENERATION)
        .ok_or_else(|| "generation_overflow: aggregate generation is exhausted".to_owned())?;
    let representation_sha256 = sha256(markdown.as_bytes());
    let next = ManagedEnvelope {
        schema_version: SCHEMA_VERSION,
        plugin_id: plugin_id.to_owned(),
        document_id: document_id.to_owned(),
        generation,
        vault_path: vault_path.to_owned(),
        representation_sha256: representation_sha256.clone(),
        aggregate: aggregate.clone(),
    };
    let transaction = PreparedTransaction {
        schema_version: SCHEMA_VERSION,
        plugin_id: plugin_id.to_owned(),
        document_id: document_id.to_owned(),
        vault_path: vault_path.to_owned(),
        expected_generation,
        expected_representation_sha256: match expected {
            ExpectedRepresentation::Missing => None,
            ExpectedRepresentation::Present(hash) => Some(hash),
        },
        next: next.clone(),
        markdown: markdown.to_owned(),
    };

    write_json_atomic(
        &directory,
        &journal_path(&directory, vault_path),
        &transaction,
    )?;
    match replace_markdown(
        &document_path,
        transaction.expected_representation_sha256.as_deref(),
        markdown,
    ) {
        Ok(()) => {}
        Err(ReplaceMarkdownError::NotWritten(message)) => {
            remove_journal(&directory, vault_path)?;
            if message.starts_with("external_drift:") {
                let disk = read_disk(&document_path)?;
                return Ok(external_drift_payload(current.as_ref(), &disk, vault_path));
            }
            return Err(message);
        }
        Err(ReplaceMarkdownError::OutcomeUnknown(message)) => return Err(message),
    }
    write_json_atomic(&directory, &envelope_path(&directory, document_id), &next)?;
    write_binding(&directory, plugin_id, document_id, vault_path)?;
    remove_journal(&directory, vault_path)?;

    Ok(json!({
        "kind": "committed",
        "generation": generation,
        "representation_sha256": representation_sha256,
    }))
}

fn repository_directory(
    root: &Path,
    vault_root: &Path,
    plugin_id: &str,
    create: bool,
) -> Result<Option<PathBuf>, String> {
    let canonical_vault = vault_root
        .canonicalize()
        .map_err(|error| format!("vault_required: vault root is unavailable: {error}"))?;
    let namespace = vault_namespace(&canonical_vault);
    plugin_repository_directory(
        root,
        plugin_id,
        &["cdr-managed-documents", "v1", namespace.as_str()],
        create,
    )
}

#[cfg(unix)]
fn vault_namespace(canonical_vault: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut digest = Sha256::new();
    digest.update(b"unix\0");
    digest.update(canonical_vault.as_os_str().as_bytes());
    format!("vault-{}", hex::encode(digest.finalize()))
}

#[cfg(windows)]
fn vault_namespace(canonical_vault: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    let mut digest = Sha256::new();
    digest.update(b"windows-utf16le\0");
    for unit in canonical_vault.as_os_str().encode_wide() {
        digest.update(unit.to_le_bytes());
    }
    format!("vault-{}", hex::encode(digest.finalize()))
}

#[cfg(not(any(unix, windows)))]
fn vault_namespace(canonical_vault: &Path) -> String {
    format!(
        "vault-{}",
        sha256(canonical_vault.to_string_lossy().as_bytes())
    )
}

fn required_vault_path(value: Option<&Value>) -> Result<&str, String> {
    let raw = value
        .and_then(Value::as_str)
        .ok_or_else(|| "invalid_params: vault_path must be a string".to_owned())?;
    if raw.is_empty() || raw.len() > MAX_VAULT_PATH_BYTES || raw.chars().any(char::is_control) {
        return Err(format!(
            "invalid_params: vault_path must contain 1..={MAX_VAULT_PATH_BYTES} safe UTF-8 bytes"
        ));
    }
    if raw.starts_with('/') || raw.contains('\\') || raw.contains(':') {
        return Err("unsafe_path: vault_path must be a forward-slash relative path".into());
    }
    let parts: Vec<&str> = raw.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err("unsafe_path: vault_path contains an invalid component".into());
    }
    if !raw.to_ascii_lowercase().ends_with(".md") {
        return Err("invalid_params: managed document must use a .md path".into());
    }
    let lowered = raw.to_ascii_lowercase();
    if lowered == "memory.md"
        || lowered == "user.md"
        || lowered == ".notemd/memory"
        || lowered.starts_with(".notemd/memory/")
    {
        return Err("reserved_path: Memory v2 authority and root projections are not writable CDR documents".into());
    }
    Ok(raw)
}

fn parse_expected(value: Option<&Value>) -> Result<ExpectedRepresentation, String> {
    let value =
        value.ok_or_else(|| "invalid_params: representation.expected is required".to_owned())?;
    let object = value
        .as_object()
        .ok_or_else(|| "invalid_params: representation.expected must be an object".to_owned())?;
    match object.get("kind").and_then(Value::as_str) {
        Some("missing") => {
            if object.len() != 1 {
                return Err("invalid_params: missing expectation only accepts kind".into());
            }
            Ok(ExpectedRepresentation::Missing)
        }
        Some("present") => {
            if object.len() != 2 {
                return Err("invalid_params: present expectation requires kind and sha256".into());
            }
            let hash = object
                .get("sha256")
                .and_then(Value::as_str)
                .filter(|value| valid_sha256(value))
                .ok_or_else(|| {
                    "invalid_params: expected sha256 must be 64 lowercase hex".to_owned()
                })?;
            Ok(ExpectedRepresentation::Present(hash.to_owned()))
        }
        _ => Err("invalid_params: expected kind must be missing or present".into()),
    }
}

fn resolve_managed_path(
    root: &Path,
    vault_path: &str,
    create_parents: bool,
) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|error| format!("vault_required: vault root is unavailable: {error}"))?;
    let parts: Vec<&str> = vault_path.split('/').collect();
    let mut current = root.clone();
    for component in &parts[..parts.len() - 1] {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("unsafe_path: managed document parent is a symbolic link".into())
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err("unsafe_path: managed document parent is not a directory".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && create_parents => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(format!("io: create managed document directory: {error}"))
                    }
                }
                let metadata = fs::symlink_metadata(&current)
                    .map_err(|error| format!("io: inspect created document directory: {error}"))?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(
                        "unsafe_path: created document parent is not a real directory".into(),
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(root.join(vault_path));
            }
            Err(error) => return Err(format!("io: inspect managed document parent: {error}")),
        }
    }
    current.push(parts.last().expect("validated non-empty path"));
    match fs::symlink_metadata(&current) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err("unsafe_path: managed document is a symbolic link".into())
        }
        Ok(metadata) if !metadata.is_file() => {
            Err("unsafe_path: managed document path is not a regular file".into())
        }
        Ok(_) => Ok(current),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(current),
        Err(error) => Err(format!("io: inspect managed document: {error}")),
    }
}

fn read_disk(path: &Path) -> Result<DiskState, String> {
    let before = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DiskState::Missing)
        }
        Err(error) => return Err(format!("io: inspect managed Markdown: {error}")),
    };
    if before.file_type().is_symlink() || !before.is_file() {
        return Err("unsafe_path: managed Markdown is not a regular file".into());
    }
    if before.len() > MAX_MARKDOWN_BYTES as u64 {
        return Err(format!(
            "too_large: managed Markdown exceeds {MAX_MARKDOWN_BYTES} bytes"
        ));
    }
    let bytes = fs::read(path).map_err(|error| format!("io: read managed Markdown: {error}"))?;
    let after = fs::symlink_metadata(path)
        .map_err(|error| format!("io: re-inspect managed Markdown: {error}"))?;
    if after.file_type().is_symlink() || !after.is_file() || before.len() != after.len() {
        return Err("external_drift: managed Markdown changed while it was being read".into());
    }
    let markdown = String::from_utf8(bytes)
        .map_err(|_| "corrupt: managed Markdown is not valid UTF-8".to_owned())?;
    Ok(DiskState::Present {
        sha256: sha256(markdown.as_bytes()),
        markdown,
    })
}

fn document_id_from_markdown(markdown: &str) -> Result<String, String> {
    Ok(frontmatter_identity(markdown)?.document_id)
}

fn frontmatter_identity(markdown: &str) -> Result<FrontmatterIdentity, String> {
    let (_, yaml) = leading_frontmatter(markdown)?;
    let parsed: serde_yaml::Value = serde_yaml::from_str(yaml)
        .map_err(|error| format!("identity_conflict: invalid YAML frontmatter: {error}"))?;
    let mapping = parsed
        .as_mapping()
        .ok_or_else(|| "identity_conflict: frontmatter must be a mapping".to_owned())?;
    let profile_type = mapping
        .get(serde_yaml::Value::String("type".into()))
        .and_then(serde_yaml::Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| "identity_conflict: frontmatter type is required".to_owned())?;
    let cdr = mapping
        .get(serde_yaml::Value::String("cdr".into()))
        .and_then(serde_yaml::Value::as_mapping)
        .ok_or_else(|| "identity_conflict: frontmatter cdr mapping is required".to_owned())?;
    let document_id = cdr
        .get(serde_yaml::Value::String("document_id".into()))
        .and_then(serde_yaml::Value::as_str)
        .ok_or_else(|| "identity_conflict: frontmatter cdr.document_id is required".to_owned())?;
    required_document_id(Some(&Value::String(document_id.to_owned())))?;
    Ok(FrontmatterIdentity {
        document_id: document_id.to_owned(),
        profile_type: profile_type.to_owned(),
    })
}

fn leading_frontmatter(markdown: &str) -> Result<(&str, &str), String> {
    let mut offset = 0usize;
    let mut first_end = None;
    for (index, line) in markdown.split_inclusive('\n').enumerate() {
        let normalized = line.trim_end_matches(['\r', '\n']);
        let start = offset;
        offset += line.len();
        if index == 0 {
            if normalized != "---" {
                return Err(
                    "identity_conflict: managed Markdown requires leading frontmatter".into(),
                );
            }
            first_end = Some(offset);
            continue;
        }
        if normalized == "---" {
            let yaml_start = first_end.expect("first line recorded");
            return Ok((&markdown[..offset], &markdown[yaml_start..start]));
        }
    }
    Err("identity_conflict: managed Markdown frontmatter is not closed".into())
}

fn representation_payload(
    disk: &DiskState,
    document_id: &str,
    vault_path: &str,
    committed_sha256: &str,
) -> Result<Value, String> {
    Ok(match disk {
        DiskState::Missing => json!({
            "vault_path": vault_path,
            "committed_sha256": committed_sha256,
            "status": "missing",
        }),
        DiskState::Present { markdown, sha256 } => {
            let identity = frontmatter_identity(markdown)?;
            if identity.document_id != document_id {
                return Err("identity_conflict: frontmatter document id changed on disk".into());
            }
            json!({
                "vault_path": vault_path,
                "committed_sha256": committed_sha256,
                "status": if sha256 == committed_sha256 { "in-sync" } else { "external-drift" },
                "disk_sha256": sha256,
                "markdown": markdown,
                "profile_type": identity.profile_type,
            })
        }
    })
}

fn disk_matches_hash(disk: &DiskState, expected: &str) -> bool {
    matches!(disk, DiskState::Present { sha256, .. } if sha256 == expected)
}

fn aggregate_conflict_payload(
    current: Option<&ManagedEnvelope>,
    binding: Option<&LocatorBinding>,
) -> Value {
    match current {
        Some(current) => json!({
            "kind": "aggregate-conflict",
            "current": {
                "generation": current.generation,
                "aggregate": current.aggregate,
                "representation_sha256": current.representation_sha256,
            }
        }),
        None => json!({
            "kind": "aggregate-conflict",
            "current": {
                "generation": 0,
                "aggregate": {},
                "representation_sha256": null,
                "binding_document_id": binding.map(|value| value.document_id.as_str()),
            }
        }),
    }
}

fn external_drift_payload(
    current: Option<&ManagedEnvelope>,
    disk: &DiskState,
    vault_path: &str,
) -> Value {
    let disk = match disk {
        DiskState::Missing => json!({ "status": "missing" }),
        DiskState::Present { markdown, sha256 } => json!({
            "status": "external-drift",
            "disk_sha256": sha256,
            "markdown": markdown,
        }),
    };
    json!({
        "kind": "external-drift",
        "current": current.map(|value| json!({
            "generation": value.generation,
            "aggregate": value.aggregate,
            "representation_sha256": value.representation_sha256,
        })),
        "representation": {
            "vault_path": vault_path,
            "disk": disk,
        },
    })
}

fn recover_for_path(
    app_data_root: &Path,
    vault_root: &Path,
    plugin_id: &str,
    vault_path: &str,
) -> Result<(), String> {
    let Some(directory) = repository_directory(app_data_root, vault_root, plugin_id, false)? else {
        return Ok(());
    };
    let Some(transaction) = read_json_file::<PreparedTransaction>(
        &journal_path(&directory, vault_path),
        "prepared transaction",
    )?
    else {
        return Ok(());
    };
    validate_transaction(&transaction, plugin_id, vault_path)?;
    let document_path = resolve_managed_path(vault_root, vault_path, false)?;
    let disk = read_disk(&document_path)?;
    let current = read_managed_envelope(&directory, plugin_id, &transaction.document_id)?;
    let old_envelope = current.as_ref().is_some_and(|value| {
        value.generation == transaction.expected_generation
            && value.vault_path == transaction.vault_path
            && Some(value.representation_sha256.as_str())
                == transaction.expected_representation_sha256.as_deref()
    }) || (transaction.expected_generation == 0 && current.is_none());
    let next_envelope = current.as_ref() == Some(&transaction.next);
    let old_disk = match &transaction.expected_representation_sha256 {
        None => matches!(disk, DiskState::Missing),
        Some(hash) => disk_matches_hash(&disk, hash),
    };
    let next_disk = disk_matches_hash(&disk, &transaction.next.representation_sha256);

    if old_envelope && old_disk {
        return remove_journal(&directory, vault_path);
    }
    if old_envelope && next_disk {
        write_json_atomic(
            &directory,
            &envelope_path(&directory, &transaction.document_id),
            &transaction.next,
        )?;
        write_binding(&directory, plugin_id, &transaction.document_id, vault_path)?;
        return remove_journal(&directory, vault_path);
    }
    if next_envelope {
        write_binding(&directory, plugin_id, &transaction.document_id, vault_path)?;
        // Once the aggregate is committed, a different disk value is an
        // external drift event rather than an ambiguous prepared commit.
        return remove_journal(&directory, vault_path);
    }
    Err("recovery_conflict: prepared managed-document commit cannot be resolved".into())
}

fn validate_transaction(
    transaction: &PreparedTransaction,
    plugin_id: &str,
    vault_path: &str,
) -> Result<(), String> {
    let next_generation = transaction
        .expected_generation
        .checked_add(1)
        .filter(|value| *value <= MAX_SAFE_GENERATION)
        .ok_or_else(|| "corrupt: prepared transaction generation is invalid".to_owned())?;
    let expected_hash_is_valid = match &transaction.expected_representation_sha256 {
        None => transaction.expected_generation == 0,
        Some(hash) => transaction.expected_generation > 0 && valid_sha256(hash),
    };
    if transaction.schema_version != SCHEMA_VERSION
        || transaction.plugin_id != plugin_id
        || transaction.vault_path != vault_path
        || transaction.expected_generation > MAX_SAFE_GENERATION
        || !expected_hash_is_valid
        || transaction.next.schema_version != SCHEMA_VERSION
        || transaction.next.plugin_id != plugin_id
        || transaction.next.document_id != transaction.document_id
        || transaction.next.vault_path != vault_path
        || transaction.next.generation != next_generation
        || !valid_sha256(&transaction.next.representation_sha256)
        || !transaction.next.aggregate.is_object()
        || transaction.markdown.len() > MAX_MARKDOWN_BYTES
        || transaction.next.representation_sha256 != sha256(transaction.markdown.as_bytes())
        || document_id_from_markdown(&transaction.markdown)? != transaction.document_id
    {
        return Err("corrupt: prepared transaction fields are inconsistent".into());
    }
    validate_plugin_id(&transaction.plugin_id)?;
    required_document_id(Some(&Value::String(transaction.document_id.clone())))?;
    required_vault_path(Some(&Value::String(transaction.vault_path.clone())))?;
    validate_aggregate_size(&transaction.next.aggregate)?;
    Ok(())
}

fn read_managed_envelope(
    directory: &Path,
    plugin_id: &str,
    document_id: &str,
) -> Result<Option<ManagedEnvelope>, String> {
    let envelope = read_json_file::<ManagedEnvelope>(
        &envelope_path(directory, document_id),
        "managed aggregate",
    )?;
    let Some(envelope) = envelope else {
        return Ok(None);
    };
    if envelope.schema_version != SCHEMA_VERSION
        || envelope.plugin_id != plugin_id
        || envelope.document_id != document_id
        || envelope.generation == 0
        || envelope.generation > MAX_SAFE_GENERATION
        || !valid_sha256(&envelope.representation_sha256)
        || !envelope.aggregate.is_object()
    {
        return Err("corrupt: managed aggregate envelope is inconsistent".into());
    }
    required_vault_path(Some(&Value::String(envelope.vault_path.clone())))?;
    validate_aggregate_size(&envelope.aggregate)?;
    Ok(Some(envelope))
}

fn read_binding(
    directory: &Path,
    plugin_id: &str,
    vault_path: &str,
) -> Result<Option<LocatorBinding>, String> {
    let binding =
        read_json_file::<LocatorBinding>(&binding_path(directory, vault_path), "locator binding")?;
    let Some(binding) = binding else {
        return Ok(None);
    };
    if binding.schema_version != SCHEMA_VERSION
        || binding.plugin_id != plugin_id
        || binding.vault_path != vault_path
    {
        return Err("corrupt: locator binding is inconsistent".into());
    }
    required_document_id(Some(&Value::String(binding.document_id.clone())))?;
    Ok(Some(binding))
}

fn write_binding(
    directory: &Path,
    plugin_id: &str,
    document_id: &str,
    vault_path: &str,
) -> Result<(), String> {
    let binding = LocatorBinding {
        schema_version: SCHEMA_VERSION,
        plugin_id: plugin_id.to_owned(),
        document_id: document_id.to_owned(),
        vault_path: vault_path.to_owned(),
    };
    write_json_atomic(directory, &binding_path(directory, vault_path), &binding)
}

fn read_json_file<T: for<'de> Deserialize<'de>>(
    path: &Path,
    label: &str,
) -> Result<Option<T>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("io: inspect {label}: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("corrupt: {label} is not a regular file"));
    }
    if metadata.len() > MAX_STORED_BYTES {
        return Err(format!("too_large: stored {label} exceeds its limit"));
    }
    let bytes = fs::read(path).map_err(|error| format!("io: read {label}: {error}"))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("corrupt: invalid {label}: {error}"))
}

fn write_json_atomic<T: Serialize>(directory: &Path, path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| format!("io: serialize managed repository value: {error}"))?;
    if bytes.len() as u64 > MAX_STORED_BYTES {
        return Err("too_large: managed repository value exceeds its limit".into());
    }
    let mut temp = tempfile::NamedTempFile::new_in(directory)
        .map_err(|error| format!("io: create managed repository temporary file: {error}"))?;
    temp.write_all(&bytes)
        .map_err(|error| format!("io: write managed repository temporary file: {error}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|error| format!("io: sync managed repository temporary file: {error}"))?;
    temp.persist(path)
        .map_err(|error| format!("io: replace managed repository value: {}", error.error))?;
    sync_directory(directory)
}

fn replace_markdown(
    path: &Path,
    expected_sha256: Option<&str>,
    markdown: &str,
) -> Result<(), ReplaceMarkdownError> {
    let parent = path.parent().ok_or_else(|| {
        ReplaceMarkdownError::NotWritten("unsafe_path: managed Markdown has no parent".to_owned())
    })?;
    let current = read_disk(path).map_err(ReplaceMarkdownError::NotWritten)?;
    let matches = match expected_sha256 {
        None => matches!(current, DiskState::Missing),
        Some(hash) => disk_matches_hash(&current, hash),
    };
    if !matches {
        return Err(ReplaceMarkdownError::NotWritten(
            "external_drift: managed Markdown changed before replacement".into(),
        ));
    }
    let next_hash = sha256(markdown.as_bytes());
    if disk_matches_hash(&current, &next_hash) {
        return Ok(());
    }
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        ReplaceMarkdownError::NotWritten(format!("io: create Markdown temporary file: {error}"))
    })?;
    temp.write_all(markdown.as_bytes()).map_err(|error| {
        ReplaceMarkdownError::NotWritten(format!("io: write Markdown temporary file: {error}"))
    })?;
    temp.as_file().sync_all().map_err(|error| {
        ReplaceMarkdownError::NotWritten(format!("io: sync Markdown temporary file: {error}"))
    })?;
    let immediately_before = read_disk(path).map_err(ReplaceMarkdownError::NotWritten)?;
    let still_matches = match expected_sha256 {
        None => matches!(immediately_before, DiskState::Missing),
        Some(hash) => disk_matches_hash(&immediately_before, hash),
    };
    if !still_matches {
        return Err(ReplaceMarkdownError::NotWritten(
            "external_drift: managed Markdown changed during commit".into(),
        ));
    }
    persist_markdown_temp(temp, path, expected_sha256.is_none())?;
    sync_directory(parent).map_err(ReplaceMarkdownError::OutcomeUnknown)?;
    let persisted = read_disk(path).map_err(ReplaceMarkdownError::OutcomeUnknown)?;
    if !disk_matches_hash(&persisted, &next_hash) {
        return Err(ReplaceMarkdownError::OutcomeUnknown(
            "external_drift: managed Markdown changed after replacement".into(),
        ));
    }
    Ok(())
}

fn persist_markdown_temp(
    temp: tempfile::NamedTempFile,
    path: &Path,
    create: bool,
) -> Result<(), ReplaceMarkdownError> {
    if create {
        temp.persist_noclobber(path).map_err(|error| {
            if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                ReplaceMarkdownError::NotWritten(
                    "external_drift: managed Markdown appeared during initial commit".into(),
                )
            } else {
                ReplaceMarkdownError::NotWritten(format!(
                    "io: create managed Markdown: {}",
                    error.error
                ))
            }
        })?;
    } else {
        temp.persist(path).map_err(|error| {
            ReplaceMarkdownError::NotWritten(format!(
                "io: replace managed Markdown: {}",
                error.error
            ))
        })?;
    }
    Ok(())
}

fn remove_journal(directory: &Path, vault_path: &str) -> Result<(), String> {
    let path = journal_path(directory, vault_path);
    match fs::remove_file(path) {
        Ok(()) => sync_directory(directory),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("io: remove prepared transaction: {error}")),
    }
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| format!("io: sync directory: {error}"))
    }
    #[cfg(not(unix))]
    {
        if let Ok(directory) = File::open(path) {
            let _ = directory.sync_all();
        }
        Ok(())
    }
}

fn envelope_path(directory: &Path, document_id: &str) -> PathBuf {
    directory.join(format!("document-{}", aggregate_filename(document_id)))
}

fn binding_path(directory: &Path, vault_path: &str) -> PathBuf {
    directory.join(format!("locator-{}.json", sha256(vault_path.as_bytes())))
}

fn journal_path(directory: &Path, vault_path: &str) -> PathBuf {
    directory.join(format!("prepared-{}.json", sha256(vault_path.as_bytes())))
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLUGIN: &str = "notemd.test";
    const DOCUMENT_ID: &str = "01900000-0000-7000-8000-000000000001";
    const VAULT_PATH: &str = "wikipage/Memory Workspace.note.md";

    fn markdown(body: &str) -> String {
        format!("---\ntype: Memory\ncdr:\n  document_id: {DOCUMENT_ID}\n---\n{body}\n")
    }

    fn inspect_params() -> Value {
        json!({ "vault_path": VAULT_PATH })
    }

    fn load_params() -> Value {
        json!({ "document_id": DOCUMENT_ID, "vault_path": VAULT_PATH })
    }

    fn commit_params(
        generation: u64,
        expected_sha256: Option<&str>,
        aggregate: Value,
        body: &str,
    ) -> Value {
        json!({
            "document_id": DOCUMENT_ID,
            "expected_generation": generation,
            "aggregate": aggregate,
            "representation": {
                "vault_path": VAULT_PATH,
                "expected": match expected_sha256 {
                    Some(hash) => json!({ "kind": "present", "sha256": hash }),
                    None => json!({ "kind": "missing" }),
                },
                "markdown": markdown(body),
            },
        })
    }

    #[test]
    fn create_load_update_and_metadata_only_commit_round_trip() {
        let app = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        assert_eq!(
            inspect(app.path(), vault.path(), PLUGIN, &inspect_params()).unwrap(),
            json!({ "kind": "missing" })
        );
        let created = commit(
            app.path(),
            vault.path(),
            PLUGIN,
            &commit_params(0, None, json!({ "head": 1 }), "# First"),
        )
        .unwrap();
        assert_eq!(created["kind"], "committed");
        assert_eq!(created["generation"], 1);
        let first_hash = created["representation_sha256"].as_str().unwrap();
        assert_eq!(
            inspect(app.path(), vault.path(), PLUGIN, &inspect_params()).unwrap(),
            json!({ "kind": "located", "document_id": DOCUMENT_ID })
        );
        let loaded = load(app.path(), vault.path(), PLUGIN, &load_params()).unwrap();
        assert_eq!(loaded["representation"]["status"], "in-sync");
        assert_eq!(loaded["aggregate"], json!({ "head": 1 }));

        let updated = commit(
            app.path(),
            vault.path(),
            PLUGIN,
            &commit_params(1, Some(first_hash), json!({ "head": 2 }), "# Second"),
        )
        .unwrap();
        let second_hash = updated["representation_sha256"].as_str().unwrap();
        assert_ne!(first_hash, second_hash);
        let before_metadata = fs::metadata(vault.path().join(VAULT_PATH))
            .unwrap()
            .modified()
            .unwrap();
        let metadata = commit(
            app.path(),
            vault.path(),
            PLUGIN,
            &commit_params(
                2,
                Some(second_hash),
                json!({ "head": 2, "audit": 3 }),
                "# Second",
            ),
        )
        .unwrap();
        assert_eq!(metadata["generation"], 3);
        assert_eq!(
            fs::metadata(vault.path().join(VAULT_PATH))
                .unwrap()
                .modified()
                .unwrap(),
            before_metadata
        );
    }

    #[test]
    fn aggregate_conflict_and_external_drift_do_not_overwrite() {
        let app = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let created = commit(
            app.path(),
            vault.path(),
            PLUGIN,
            &commit_params(0, None, json!({ "value": 1 }), "Original"),
        )
        .unwrap();
        let hash = created["representation_sha256"].as_str().unwrap();
        let stale = commit(
            app.path(),
            vault.path(),
            PLUGIN,
            &commit_params(0, None, json!({ "value": 2 }), "Stale"),
        )
        .unwrap();
        assert_eq!(stale["kind"], "aggregate-conflict");

        fs::write(vault.path().join(VAULT_PATH), markdown("External")).unwrap();
        let drift = commit(
            app.path(),
            vault.path(),
            PLUGIN,
            &commit_params(1, Some(hash), json!({ "value": 3 }), "Overwrite"),
        )
        .unwrap();
        assert_eq!(drift["kind"], "external-drift");
        assert_eq!(
            fs::read_to_string(vault.path().join(VAULT_PATH)).unwrap(),
            markdown("External")
        );
        assert_eq!(
            load(app.path(), vault.path(), PLUGIN, &load_params()).unwrap()["aggregate"],
            json!({ "value": 1 })
        );
    }

    #[test]
    fn deleted_representation_is_located_and_reported_as_missing() {
        let app = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        commit(
            app.path(),
            vault.path(),
            PLUGIN,
            &commit_params(0, None, json!({ "value": 1 }), "Original"),
        )
        .unwrap();
        fs::remove_file(vault.path().join(VAULT_PATH)).unwrap();
        assert_eq!(
            inspect(app.path(), vault.path(), PLUGIN, &inspect_params()).unwrap()["document_id"],
            DOCUMENT_ID
        );
        assert_eq!(
            load(app.path(), vault.path(), PLUGIN, &load_params()).unwrap()["representation"]
                ["status"],
            "missing"
        );
    }

    #[test]
    fn prepared_new_markdown_with_old_aggregate_rolls_forward() {
        let app = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let created = commit(
            app.path(),
            vault.path(),
            PLUGIN,
            &commit_params(0, None, json!({ "value": 1 }), "Old"),
        )
        .unwrap();
        let old_hash = created["representation_sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        let directory = repository_directory(app.path(), vault.path(), PLUGIN, true)
            .unwrap()
            .unwrap();
        let next_markdown = markdown("New");
        let next = ManagedEnvelope {
            schema_version: SCHEMA_VERSION,
            plugin_id: PLUGIN.into(),
            document_id: DOCUMENT_ID.into(),
            generation: 2,
            vault_path: VAULT_PATH.into(),
            representation_sha256: sha256(next_markdown.as_bytes()),
            aggregate: json!({ "value": 2 }),
        };
        let transaction = PreparedTransaction {
            schema_version: SCHEMA_VERSION,
            plugin_id: PLUGIN.into(),
            document_id: DOCUMENT_ID.into(),
            vault_path: VAULT_PATH.into(),
            expected_generation: 1,
            expected_representation_sha256: Some(old_hash),
            next,
            markdown: next_markdown.clone(),
        };
        write_json_atomic(
            &directory,
            &journal_path(&directory, VAULT_PATH),
            &transaction,
        )
        .unwrap();
        fs::write(vault.path().join(VAULT_PATH), &next_markdown).unwrap();

        let loaded = load(app.path(), vault.path(), PLUGIN, &load_params()).unwrap();
        assert_eq!(loaded["generation"], 2);
        assert_eq!(loaded["aggregate"], json!({ "value": 2 }));
        assert!(!journal_path(&directory, VAULT_PATH).exists());
    }

    #[test]
    fn ambiguous_recovery_keeps_journal_and_fails_closed() {
        let app = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let created = commit(
            app.path(),
            vault.path(),
            PLUGIN,
            &commit_params(0, None, json!({ "value": 1 }), "Old"),
        )
        .unwrap();
        let old_hash = created["representation_sha256"]
            .as_str()
            .unwrap()
            .to_owned();
        let directory = repository_directory(app.path(), vault.path(), PLUGIN, true)
            .unwrap()
            .unwrap();
        let next_markdown = markdown("New");
        let next = ManagedEnvelope {
            schema_version: SCHEMA_VERSION,
            plugin_id: PLUGIN.into(),
            document_id: DOCUMENT_ID.into(),
            generation: 2,
            vault_path: VAULT_PATH.into(),
            representation_sha256: sha256(next_markdown.as_bytes()),
            aggregate: json!({ "value": 2 }),
        };
        let transaction = PreparedTransaction {
            schema_version: SCHEMA_VERSION,
            plugin_id: PLUGIN.into(),
            document_id: DOCUMENT_ID.into(),
            vault_path: VAULT_PATH.into(),
            expected_generation: 1,
            expected_representation_sha256: Some(old_hash),
            next,
            markdown: next_markdown,
        };
        let journal = journal_path(&directory, VAULT_PATH);
        write_json_atomic(&directory, &journal, &transaction).unwrap();
        fs::write(vault.path().join(VAULT_PATH), markdown("Third party")).unwrap();

        let error = load(app.path(), vault.path(), PLUGIN, &load_params()).unwrap_err();
        assert!(error.starts_with("recovery_conflict:"), "{error}");
        assert!(journal.exists());
    }

    #[test]
    fn rejects_reserved_paths_existing_files_bad_identity_and_symlinks() {
        let app = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        for path in [
            "MEMORY.md",
            "USER.md",
            ".notemd/memory/owned.md",
            "../escape.md",
            "C:/escape.md",
            "folder/C:/escape.md",
        ] {
            let mut params = inspect_params();
            params["vault_path"] = json!(path);
            assert!(
                inspect(app.path(), vault.path(), PLUGIN, &params).is_err(),
                "{path}"
            );
        }

        let target = vault.path().join(VAULT_PATH);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "---\ntype: Memory\n---\nExisting\n").unwrap();
        assert!(inspect(app.path(), vault.path(), PLUGIN, &inspect_params())
            .unwrap_err()
            .starts_with("identity_conflict:"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            fs::remove_file(&target).unwrap();
            let outside = tempfile::tempdir().unwrap();
            symlink(outside.path(), target.parent().unwrap()).unwrap_err();
            let linked = vault.path().join("linked");
            symlink(outside.path(), &linked).unwrap();
            let mut params = inspect_params();
            params["vault_path"] = json!("linked/doc.md");
            assert!(inspect(app.path(), vault.path(), PLUGIN, &params)
                .unwrap_err()
                .contains("symbolic link"));
        }
    }

    #[test]
    fn identical_slots_in_different_vaults_have_independent_identity_and_state() {
        let app = tempfile::tempdir().unwrap();
        let vault_a = tempfile::tempdir().unwrap();
        let vault_b = tempfile::tempdir().unwrap();
        let mut params_b = commit_params(0, None, json!({ "vault": "b" }), "Vault B");
        let document_b = "01900000-0000-7000-8000-000000000002";
        params_b["document_id"] = json!(document_b);
        params_b["representation"]["markdown"] = json!(format!(
            "---\ntype: Memory\ncdr:\n  document_id: {document_b}\n---\nVault B\n"
        ));

        commit(
            app.path(),
            vault_a.path(),
            PLUGIN,
            &commit_params(0, None, json!({ "vault": "a" }), "Vault A"),
        )
        .unwrap();
        commit(app.path(), vault_b.path(), PLUGIN, &params_b).unwrap();

        assert_eq!(
            inspect(app.path(), vault_a.path(), PLUGIN, &inspect_params()).unwrap()["document_id"],
            DOCUMENT_ID
        );
        assert_eq!(
            inspect(app.path(), vault_b.path(), PLUGIN, &inspect_params()).unwrap()["document_id"],
            document_b
        );
        assert_eq!(
            load(app.path(), vault_a.path(), PLUGIN, &load_params()).unwrap()["aggregate"],
            json!({ "vault": "a" })
        );
        let load_b = json!({ "document_id": document_b, "vault_path": VAULT_PATH });
        assert_eq!(
            load(app.path(), vault_b.path(), PLUGIN, &load_b).unwrap()["aggregate"],
            json!({ "vault": "b" })
        );

        let directory_a = repository_directory(app.path(), vault_a.path(), PLUGIN, false)
            .unwrap()
            .unwrap();
        let directory_b = repository_directory(app.path(), vault_b.path(), PLUGIN, false)
            .unwrap()
            .unwrap();
        assert_ne!(directory_a, directory_b);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_use_distinct_lossless_namespaces() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path_a = PathBuf::from(OsString::from_vec(vec![b'/', b'v', 0x80]));
        let path_b = PathBuf::from(OsString::from_vec(vec![b'/', b'v', 0x81]));
        assert_eq!(path_a.to_string_lossy(), path_b.to_string_lossy());
        assert_ne!(vault_namespace(&path_a), vault_namespace(&path_b));
    }

    #[test]
    fn corrupted_generation_in_prepared_transaction_fails_closed_without_panicking() {
        let app = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let directory = repository_directory(app.path(), vault.path(), PLUGIN, true)
            .unwrap()
            .unwrap();
        let body = markdown("Invalid");
        let transaction = PreparedTransaction {
            schema_version: SCHEMA_VERSION,
            plugin_id: PLUGIN.into(),
            document_id: DOCUMENT_ID.into(),
            vault_path: VAULT_PATH.into(),
            expected_generation: u64::MAX,
            expected_representation_sha256: Some(sha256(body.as_bytes())),
            next: ManagedEnvelope {
                schema_version: SCHEMA_VERSION,
                plugin_id: PLUGIN.into(),
                document_id: DOCUMENT_ID.into(),
                generation: 0,
                vault_path: VAULT_PATH.into(),
                representation_sha256: sha256(body.as_bytes()),
                aggregate: json!({}),
            },
            markdown: body,
        };
        write_json_atomic(
            &directory,
            &journal_path(&directory, VAULT_PATH),
            &transaction,
        )
        .unwrap();

        let error = inspect(app.path(), vault.path(), PLUGIN, &inspect_params()).unwrap_err();
        assert!(error.starts_with("corrupt:"), "{error}");
        assert!(journal_path(&directory, VAULT_PATH).exists());
    }

    #[test]
    fn initial_markdown_persist_never_clobbers_a_file_that_appeared() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("managed.md");
        fs::write(&target, "external").unwrap();
        let mut temp = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
        temp.write_all(b"ours").unwrap();

        let error = persist_markdown_temp(temp, &target, true).unwrap_err();
        assert!(matches!(error, ReplaceMarkdownError::NotWritten(_)));
        assert_eq!(fs::read_to_string(target).unwrap(), "external");
    }
}
