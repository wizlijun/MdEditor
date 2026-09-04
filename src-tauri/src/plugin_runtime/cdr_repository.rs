//! Atomic, opaque local aggregate storage for collaborative-document runtimes.
//!
//! This module is deliberately independent of Tauri and of any document
//! profile.  The authenticated UI bridge supplies the app-data root and plugin
//! id; the repository only validates the request, performs generation CAS, and
//! atomically replaces one JSON envelope.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SCHEMA_VERSION: u32 = 1;
const MAX_DOCUMENT_ID_BYTES: usize = 4 * 1024;
const MAX_AGGREGATE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENVELOPE_BYTES: u64 = (MAX_AGGREGATE_BYTES + 64 * 1024) as u64;
// RPC clients are JavaScript; keep every generation exactly representable.
const MAX_SAFE_GENERATION: u64 = 9_007_199_254_740_991;

// RPC dispatches run concurrently.  A single process-wide lock is sufficient
// for Stage 0 and, unlike a per-path lock registry, cannot grow without bound.
// The temporary-file replace keeps readers in other processes from observing a
// partial envelope; cross-process writers are outside this local-store contract.
static REPOSITORY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    schema_version: u32,
    plugin_id: String,
    document_id: String,
    generation: u64,
    aggregate: Value,
}

/// Load one plugin-owned aggregate.
///
/// Returns `{ "kind": "missing" }` when no envelope exists.  Any malformed,
/// oversized, or mismatched envelope is an error: corrupt stored state is
/// never interpreted as an empty document.
pub fn load(root: &Path, plugin_id: &str, params: &Value) -> Result<Value, String> {
    validate_plugin_id(plugin_id)?;
    let document_id = load_document_id(params)?;
    let _guard = REPOSITORY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(directory) = repository_directory(root, plugin_id, false)? else {
        return Ok(json!({ "kind": "missing" }));
    };
    let path = directory.join(aggregate_filename(document_id));
    let loaded = read_envelope(&path, plugin_id, document_id)?;
    // Do not return bytes read through a parent that was replaced while the
    // file was being opened. This is also a fail-closed guard for accidental
    // repository-directory mutation by other host code.
    require_same_repository_directory(root, plugin_id, &directory)?;

    match loaded {
        None => Ok(json!({ "kind": "missing" })),
        Some(envelope) => Ok(json!({
            "kind": "loaded",
            "generation": envelope.generation,
            "aggregate": envelope.aggregate,
        })),
    }
}

/// Atomically compare-and-swap one plugin-owned aggregate.
///
/// Generation zero denotes a missing aggregate, so creation uses
/// `expected_generation: 0`.  A stale generation returns the current snapshot
/// and never changes disk state.
pub fn commit(root: &Path, plugin_id: &str, params: &Value) -> Result<Value, String> {
    validate_plugin_id(plugin_id)?;
    let object = exact_object(params, &["document_id", "expected_generation", "aggregate"])?;
    let document_id = required_document_id(object.get("document_id"))?;
    let expected_generation = object
        .get("expected_generation")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            "invalid_params: expected_generation must be a non-negative integer".to_owned()
        })?;
    if expected_generation > MAX_SAFE_GENERATION {
        return Err(format!(
            "invalid_params: expected_generation must be <= {MAX_SAFE_GENERATION}"
        ));
    }
    let aggregate = object
        .get("aggregate")
        .filter(|value| value.is_object())
        .ok_or_else(|| "invalid_params: aggregate must be an object".to_owned())?;
    validate_aggregate_size(aggregate)?;

    let _guard = REPOSITORY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let existing_directory = repository_directory(root, plugin_id, false)?;
    let current = match &existing_directory {
        Some(directory) => {
            let path = directory.join(aggregate_filename(document_id));
            let loaded = read_envelope(&path, plugin_id, document_id)?;
            require_same_repository_directory(root, plugin_id, directory)?;
            loaded
        }
        None => None,
    };
    let current_generation = current
        .as_ref()
        .map(|envelope| envelope.generation)
        .unwrap_or(0);

    if current_generation != expected_generation {
        let current_aggregate = current
            .map(|envelope| envelope.aggregate)
            .unwrap_or_else(|| json!({}));
        return Ok(json!({
            "kind": "conflict",
            "current": {
                "generation": current_generation,
                "aggregate": current_aggregate,
            },
        }));
    }

    let generation = current_generation
        .checked_add(1)
        .filter(|value| *value <= MAX_SAFE_GENERATION)
        .ok_or_else(|| "generation_overflow: aggregate generation is exhausted".to_owned())?;
    let envelope = Envelope {
        schema_version: SCHEMA_VERSION,
        plugin_id: plugin_id.to_owned(),
        document_id: document_id.to_owned(),
        generation,
        aggregate: aggregate.clone(),
    };
    let directory = repository_directory(root, plugin_id, true)?
        .ok_or_else(|| "io: repository directory disappeared after creation".to_owned())?;
    let path = directory.join(aggregate_filename(document_id));
    write_envelope(root, plugin_id, &path, &envelope)?;

    Ok(json!({ "kind": "committed", "generation": generation }))
}

fn load_document_id(params: &Value) -> Result<&str, String> {
    let object = exact_object(params, &["document_id"])?;
    required_document_id(object.get("document_id"))
}

fn exact_object<'a>(
    value: &'a Value,
    allowed_fields: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "invalid_params: params must be an object".to_owned())?;
    if object.len() != allowed_fields.len()
        || object
            .keys()
            .any(|key| !allowed_fields.contains(&key.as_str()))
    {
        return Err(format!(
            "invalid_params: expected exactly fields {}",
            allowed_fields.join(", ")
        ));
    }
    Ok(object)
}

fn required_document_id(value: Option<&Value>) -> Result<&str, String> {
    let document_id = value
        .and_then(Value::as_str)
        .ok_or_else(|| "invalid_params: document_id must be a string".to_owned())?;
    if document_id.is_empty() || document_id.len() > MAX_DOCUMENT_ID_BYTES {
        return Err(format!(
            "invalid_params: document_id must contain 1..={MAX_DOCUMENT_ID_BYTES} UTF-8 bytes"
        ));
    }
    if document_id.chars().any(char::is_control) {
        return Err("invalid_params: document_id must not contain control characters".to_owned());
    }
    Ok(document_id)
}

fn validate_plugin_id(plugin_id: &str) -> Result<(), String> {
    let parts: Vec<&str> = plugin_id.split('.').collect();
    let valid = parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
    if !valid {
        return Err("invalid_plugin_id: expected publisher.name ([a-z0-9-])".to_owned());
    }
    Ok(())
}

fn validate_aggregate_size(aggregate: &Value) -> Result<(), String> {
    let size = serde_json::to_vec(aggregate)
        .map_err(|error| format!("invalid_params: aggregate is not serializable: {error}"))?
        .len();
    if size > MAX_AGGREGATE_BYTES {
        return Err(format!(
            "too_large: aggregate exceeds {MAX_AGGREGATE_BYTES} bytes"
        ));
    }
    Ok(())
}

fn aggregate_filename(document_id: &str) -> String {
    let digest = hex::encode(Sha256::digest(document_id.as_bytes()));
    format!("{digest}.json")
}

#[cfg(test)]
fn aggregate_path(root: &Path, plugin_id: &str, document_id: &str) -> PathBuf {
    root.join("plugin_data")
        .join(plugin_id)
        .join("cdr-repository")
        .join("v1")
        .join(aggregate_filename(document_id))
}

/// Resolve the plugin repository without ever following a symlink below the
/// host-supplied root. With `create = false`, a missing layer means the
/// repository is absent. With `create = true`, every layer is created
/// individually and revalidated, including the `AlreadyExists` race.
fn repository_directory(
    root: &Path,
    plugin_id: &str,
    create: bool,
) -> Result<Option<PathBuf>, String> {
    if !ensure_root(root, create)? {
        return Ok(None);
    }

    let mut current = root.to_path_buf();
    let mut layers = Vec::with_capacity(4);
    for component in ["plugin_data", plugin_id, "cdr-repository", "v1"] {
        current.push(component);
        if !ensure_child_directory(&current, create)? {
            return Ok(None);
        }
        layers.push(current.clone());
    }

    // Repeat the checks after the full chain exists. A component replaced
    // during creation is rejected before any aggregate path is opened.
    for layer in &layers {
        require_child_directory(layer)?;
    }
    Ok(Some(current))
}

fn ensure_root(root: &Path, create: bool) -> Result<bool, String> {
    match fs::metadata(root) {
        Ok(metadata) if metadata.is_dir() => Ok(true),
        Ok(_) => Err("unsafe_path: app-data root is not a directory".to_owned()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(root)
                .map_err(|error| format!("io: create app-data root: {error}"))?;
            match fs::metadata(root) {
                Ok(metadata) if metadata.is_dir() => Ok(true),
                Ok(_) => Err("unsafe_path: app-data root is not a directory".to_owned()),
                Err(error) => Err(format!("io: inspect app-data root after creation: {error}")),
            }
        }
        Err(error) => Err(format!("io: inspect app-data root: {error}")),
    }
}

fn ensure_child_directory(path: &Path, create: bool) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            require_child_directory(path)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::create_dir(path) {
                Ok(()) => {}
                // Another creator may have won after symlink_metadata. It is
                // accepted only after the same no-symlink validation below.
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(format!(
                        "io: create repository directory '{}': {error}",
                        path.display()
                    ));
                }
            }
            require_child_directory(path)?;
            Ok(true)
        }
        Err(error) => Err(format!(
            "io: inspect repository directory '{}': {error}",
            path.display()
        )),
    }
}

fn require_child_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "unsafe_path: repository directory '{}' is unavailable: {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!(
            "unsafe_path: repository directory '{}' is a symbolic link",
            path.display()
        ));
    }
    if !metadata.is_dir() {
        return Err(format!(
            "unsafe_path: repository directory '{}' is not a directory",
            path.display()
        ));
    }
    Ok(())
}

fn require_same_repository_directory(
    root: &Path,
    plugin_id: &str,
    expected: &Path,
) -> Result<(), String> {
    let actual = repository_directory(root, plugin_id, false)?
        .ok_or_else(|| "unsafe_path: repository directory disappeared".to_owned())?;
    if actual != expected {
        return Err("unsafe_path: repository directory changed during operation".to_owned());
    }
    Ok(())
}

fn read_envelope(
    path: &Path,
    expected_plugin_id: &str,
    expected_document_id: &str,
) -> Result<Option<Envelope>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("io: inspect aggregate: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("corrupt: aggregate path is not a regular file".to_owned());
    }
    if metadata.len() > MAX_ENVELOPE_BYTES {
        return Err(format!(
            "too_large: stored envelope exceeds {MAX_ENVELOPE_BYTES} bytes"
        ));
    }

    let bytes = fs::read(path).map_err(|error| format!("io: read aggregate: {error}"))?;
    let envelope: Envelope = serde_json::from_slice(&bytes)
        .map_err(|error| format!("corrupt: invalid aggregate envelope: {error}"))?;
    if envelope.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "corrupt: unsupported schema_version {}",
            envelope.schema_version
        ));
    }
    if envelope.plugin_id != expected_plugin_id {
        return Err("corrupt: stored plugin_id does not match repository scope".to_owned());
    }
    if envelope.document_id != expected_document_id {
        return Err("corrupt: stored document_id does not match filename".to_owned());
    }
    if envelope.generation == 0 {
        return Err("corrupt: stored generation must be positive".to_owned());
    }
    if envelope.generation > MAX_SAFE_GENERATION {
        return Err(
            "corrupt: stored generation exceeds the JavaScript safe integer range".to_owned(),
        );
    }
    if !envelope.aggregate.is_object() {
        return Err("corrupt: stored aggregate must be an object".to_owned());
    }
    validate_aggregate_size(&envelope.aggregate)
        .map_err(|error| format!("corrupt: stored {error}"))?;
    Ok(Some(envelope))
}

fn write_envelope(
    root: &Path,
    plugin_id: &str,
    path: &Path,
    envelope: &Envelope,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "io: aggregate path has no parent".to_owned())?;
    require_same_repository_directory(root, plugin_id, parent)?;
    let bytes = serde_json::to_vec(envelope)
        .map_err(|error| format!("io: serialize aggregate envelope: {error}"))?;
    if bytes.len() as u64 > MAX_ENVELOPE_BYTES {
        return Err(format!(
            "too_large: envelope exceeds {MAX_ENVELOPE_BYTES} bytes"
        ));
    }

    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("io: create aggregate temporary file: {error}"))?;
    temp.write_all(&bytes)
        .map_err(|error| format!("io: write aggregate temporary file: {error}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|error| format!("io: sync aggregate temporary file: {error}"))?;
    require_same_repository_directory(root, plugin_id, parent)?;
    temp.persist(path)
        .map_err(|error| format!("io: replace aggregate envelope: {}", error.error))?;

    // A synced file plus a synced containing directory makes the rename itself
    // durable. Windows' std::fs::File cannot portably open directory handles,
    // so the already-synced replace is the strongest primitive available there.
    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("io: sync repository directory: {error}"))?;
    #[cfg(not(unix))]
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    const PLUGIN: &str = "notemd.test";

    fn load_params(document_id: &str) -> Value {
        json!({ "document_id": document_id })
    }

    fn commit_params(document_id: &str, expected_generation: u64, aggregate: Value) -> Value {
        json!({
            "document_id": document_id,
            "expected_generation": expected_generation,
            "aggregate": aggregate,
        })
    }

    #[test]
    fn missing_create_load_and_update_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(dir.path(), PLUGIN, &load_params("doc:a")).unwrap(),
            json!({ "kind": "missing" })
        );
        assert_eq!(
            commit(
                dir.path(),
                PLUGIN,
                &commit_params("doc:a", 0, json!({ "title": "one" }))
            )
            .unwrap(),
            json!({ "kind": "committed", "generation": 1 })
        );
        let stored: Value =
            serde_json::from_slice(&fs::read(aggregate_path(dir.path(), PLUGIN, "doc:a")).unwrap())
                .unwrap();
        assert_eq!(
            stored,
            json!({
                "schema_version": 1,
                "plugin_id": PLUGIN,
                "document_id": "doc:a",
                "generation": 1,
                "aggregate": { "title": "one" },
            })
        );
        assert_eq!(
            load(dir.path(), PLUGIN, &load_params("doc:a")).unwrap(),
            json!({
                "kind": "loaded",
                "generation": 1,
                "aggregate": { "title": "one" },
            })
        );
        assert_eq!(
            commit(
                dir.path(),
                PLUGIN,
                &commit_params("doc:a", 1, json!({ "title": "two" }))
            )
            .unwrap(),
            json!({ "kind": "committed", "generation": 2 })
        );
    }

    #[test]
    fn stale_commit_returns_current_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        commit(
            dir.path(),
            PLUGIN,
            &commit_params("doc:a", 0, json!({ "value": 1 })),
        )
        .unwrap();

        let result = commit(
            dir.path(),
            PLUGIN,
            &commit_params("doc:a", 0, json!({ "value": 99 })),
        )
        .unwrap();
        assert_eq!(
            result,
            json!({
                "kind": "conflict",
                "current": { "generation": 1, "aggregate": { "value": 1 } },
            })
        );
        assert_eq!(
            load(dir.path(), PLUGIN, &load_params("doc:a")).unwrap()["aggregate"],
            json!({ "value": 1 })
        );
    }

    #[test]
    fn concurrent_same_generation_has_one_winner() {
        let dir = tempfile::tempdir().unwrap();
        let root = Arc::new(dir.path().to_path_buf());
        let barrier = Arc::new(Barrier::new(3));
        let mut handles = Vec::new();
        for value in [1, 2] {
            let root = Arc::clone(&root);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                commit(
                    &root,
                    PLUGIN,
                    &commit_params("doc:race", 0, json!({ "value": value })),
                )
                .unwrap()
            }));
        }
        barrier.wait();
        let results: Vec<Value> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(
            results
                .iter()
                .filter(|result| result["kind"] == "committed")
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| result["kind"] == "conflict")
                .count(),
            1
        );
    }

    #[test]
    fn corruption_fails_closed_for_load_and_commit() {
        let dir = tempfile::tempdir().unwrap();
        let path = aggregate_path(dir.path(), PLUGIN, "doc:bad");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not json").unwrap();

        let load_error = load(dir.path(), PLUGIN, &load_params("doc:bad")).unwrap_err();
        assert!(load_error.starts_with("corrupt:"), "{load_error}");
        let commit_error = commit(
            dir.path(),
            PLUGIN,
            &commit_params("doc:bad", 0, json!({ "replacement": true })),
        )
        .unwrap_err();
        assert!(commit_error.starts_with("corrupt:"), "{commit_error}");
        assert_eq!(fs::read(path).unwrap(), b"not json");
    }

    #[test]
    fn path_is_scoped_by_plugin_and_hashes_document_id() {
        let dir = tempfile::tempdir().unwrap();
        commit(
            dir.path(),
            PLUGIN,
            &commit_params("../../escape", 0, json!({ "ok": true })),
        )
        .unwrap();
        let path = aggregate_path(dir.path(), PLUGIN, "../../escape");
        assert!(path.is_file());
        assert!(path.starts_with(dir.path().join("plugin_data").join(PLUGIN)));
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("json")
        );
        assert_eq!(path.file_stem().unwrap().to_string_lossy().len(), 64);
        assert_eq!(
            load(dir.path(), "other.plugin", &load_params("../../escape")).unwrap(),
            json!({ "kind": "missing" })
        );
    }

    #[cfg(unix)]
    #[test]
    fn parent_directory_symlinks_fail_closed_for_load_and_commit() {
        use std::os::unix::fs::symlink;

        for symlinked_layer in 0..4 {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let layers = ["plugin_data", PLUGIN, "cdr-repository", "v1"];
            let mut parent = root.path().to_path_buf();
            for component in &layers[..symlinked_layer] {
                parent.push(component);
                fs::create_dir(&parent).unwrap();
            }
            let link = parent.join(layers[symlinked_layer]);
            let target = outside.path().join("target");
            fs::create_dir(&target).unwrap();
            symlink(&target, &link).unwrap();

            let mut external_plugin = target;
            for component in &layers[symlinked_layer + 1..] {
                external_plugin.push(component);
                fs::create_dir(&external_plugin).unwrap();
            }

            let external_path = external_plugin.join(aggregate_filename("doc:outside"));
            let external_bytes = serde_json::to_vec(&Envelope {
                schema_version: SCHEMA_VERSION,
                plugin_id: PLUGIN.to_owned(),
                document_id: "doc:outside".to_owned(),
                generation: 1,
                aggregate: json!({ "owner": "outside" }),
            })
            .unwrap();
            fs::write(&external_path, &external_bytes).unwrap();

            let load_error = load(root.path(), PLUGIN, &load_params("doc:outside")).unwrap_err();
            assert!(load_error.contains("symbolic link"), "{load_error}");
            let commit_error = commit(
                root.path(),
                PLUGIN,
                &commit_params("doc:outside", 1, json!({ "owner": "repository" })),
            )
            .unwrap_err();
            assert!(commit_error.contains("symbolic link"), "{commit_error}");
            assert_eq!(fs::read(&external_path).unwrap(), external_bytes);
        }
    }

    #[test]
    fn rejects_non_object_aggregate_unknown_fields_and_invalid_ids() {
        let dir = tempfile::tempdir().unwrap();
        assert!(commit(
            dir.path(),
            PLUGIN,
            &commit_params("doc", 0, json!([1, 2, 3]))
        )
        .unwrap_err()
        .contains("aggregate must be an object"));
        assert!(load(
            dir.path(),
            PLUGIN,
            &json!({ "document_id": "doc", "extra": true })
        )
        .unwrap_err()
        .contains("expected exactly fields"));
        assert!(load(dir.path(), PLUGIN, &load_params("")).is_err());
        assert!(load(dir.path(), "../escape", &load_params("doc")).is_err());
        assert!(commit(
            dir.path(),
            PLUGIN,
            &commit_params("doc", MAX_SAFE_GENERATION + 1, json!({ "ok": true }))
        )
        .unwrap_err()
        .contains("expected_generation"));
    }

    #[test]
    fn rejects_oversized_aggregate_before_creating_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let oversized = "x".repeat(MAX_AGGREGATE_BYTES + 1);
        let error = commit(
            dir.path(),
            PLUGIN,
            &commit_params("large", 0, json!({ "text": oversized })),
        )
        .unwrap_err();
        assert!(error.starts_with("too_large:"), "{error}");
        assert!(!aggregate_path(dir.path(), PLUGIN, "large").exists());
    }
}
