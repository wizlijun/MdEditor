//! Reliable document I/O for first-class JSON Canvas files.
//!
//! This module deliberately knows nothing about the JSON Canvas data model. Its
//! contract is byte-oriented: reads return a revision computed from the same
//! bytes, while writes compare an expected disk revision immediately before an
//! atomic same-directory replacement. The comparison is optimistic protection
//! against other processes, not a filesystem-wide compare-and-swap primitive.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, Metadata};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::UNIX_EPOCH;

#[cfg(any(target_os = "ios", test))]
const IOS_MAX_CANVAS_BYTES: u64 = 4 * 1024 * 1024;
#[cfg(target_os = "ios")]
const MAX_CANVAS_BYTES: u64 = IOS_MAX_CANVAS_BYTES;
#[cfg(not(target_os = "ios"))]
const MAX_CANVAS_BYTES: u64 = 32 * 1024 * 1024;

const TEMP_PREFIX: &str = ".notemd-canvas-";
#[cfg(any(target_os = "ios", test))]
const IOS_IMPORTED_CANVASES_DIR: &str = "Imported Canvases";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskRevision {
    /// Decimal nanoseconds since the Unix epoch. A string avoids JS precision
    /// loss and matches the frontend document-session contract.
    pub mtime_ns: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ExpectedDiskState {
    Missing,
    Present { revision: DiskRevision },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenCanvasResult {
    pub text: String,
    pub revision: DiskRevision,
    pub requested_path: String,
    pub canonical_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProbeCanvasResult {
    Missing {
        requested_path: String,
        canonical_path: String,
    },
    Present {
        revision: DiskRevision,
        requested_path: String,
        canonical_path: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCanvasResult {
    pub revision: DiskRevision,
    pub canonical_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CanvasDocumentError {
    InvalidPath {
        message: String,
    },
    NotFound {
        message: String,
    },
    TooLarge {
        message: String,
        limit_bytes: u64,
        actual_bytes: u64,
    },
    InvalidUtf8 {
        message: String,
    },
    UnstableRead {
        message: String,
    },
    Conflict {
        message: String,
        expected: ExpectedDiskState,
        actual: ExpectedDiskState,
        canonical_path: String,
    },
    Io {
        message: String,
    },
}

impl CanvasDocumentError {
    fn invalid_path(message: impl Into<String>) -> Self {
        Self::InvalidPath {
            message: message.into(),
        }
    }

    fn io(action: &str, error: std::io::Error) -> Self {
        let reason = match error.kind() {
            std::io::ErrorKind::NotFound => "not found",
            std::io::ErrorKind::PermissionDenied => "permission denied",
            std::io::ErrorKind::AlreadyExists => "already exists",
            std::io::ErrorKind::InvalidData => "invalid data",
            _ => "I/O error",
        };
        Self::Io {
            message: format!("{action}: {reason}"),
        }
    }
}

#[derive(Debug)]
struct Snapshot {
    bytes: Vec<u8>,
    revision: DiskRevision,
}

static PATH_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

fn lock_for_path(path: &Path) -> Arc<Mutex<()>> {
    let locks = PATH_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
    lock
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(crate) fn is_canvas_path(path: &Path) -> bool {
    path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("canvas"))
}

fn require_canvas_extension(path: &Path) -> Result<(), CanvasDocumentError> {
    if is_canvas_path(path) {
        Ok(())
    } else {
        Err(CanvasDocumentError::invalid_path(
            "document path must use the .canvas extension",
        ))
    }
}

/// Resolve an existing file itself, but resolve only the parent for a missing
/// target. This prevents aliases from getting separate save queues and avoids
/// trusting frontend string-prefix checks for authorization.
fn resolve_canvas_path(requested: &str) -> Result<PathBuf, CanvasDocumentError> {
    let path = Path::new(requested);
    if !path.is_absolute() {
        return Err(CanvasDocumentError::invalid_path("path must be absolute"));
    }
    require_canvas_extension(path)?;

    match fs::symlink_metadata(path) {
        Ok(_) => {
            let canonical = fs::canonicalize(path)
                .map_err(|error| CanvasDocumentError::io("resolve canvas path", error))?;
            require_canvas_extension(&canonical)?;
            let checked = super::safe_path(&path_string(&canonical))
                .map_err(CanvasDocumentError::invalid_path)?;
            let metadata = fs::metadata(&checked)
                .map_err(|error| CanvasDocumentError::io("inspect canvas", error))?;
            if !metadata.is_file() {
                return Err(CanvasDocumentError::invalid_path(
                    "canvas target is not a regular file",
                ));
            }
            Ok(checked)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| {
                CanvasDocumentError::invalid_path("canvas path has no parent directory")
            })?;
            let file_name = path
                .file_name()
                .ok_or_else(|| CanvasDocumentError::invalid_path("canvas path has no file name"))?;
            let canonical_parent = fs::canonicalize(parent).map_err(|error| {
                CanvasDocumentError::io("resolve canvas parent directory", error)
            })?;
            if !canonical_parent.is_dir() {
                return Err(CanvasDocumentError::invalid_path(
                    "canvas parent is not a directory",
                ));
            }
            let candidate = canonical_parent.join(file_name);
            super::safe_path(&path_string(&candidate)).map_err(CanvasDocumentError::invalid_path)
        }
        Err(error) => Err(CanvasDocumentError::io("inspect canvas path", error)),
    }
}

fn mtime_ns(metadata: &Metadata) -> Result<u128, CanvasDocumentError> {
    metadata
        .modified()
        .map_err(|error| CanvasDocumentError::io("read canvas modification time", error))?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| CanvasDocumentError::Io {
            message: "read canvas modification time: time is before Unix epoch".to_string(),
        })
}

fn metadata_signature(metadata: &Metadata) -> Result<(u64, u128), CanvasDocumentError> {
    Ok((metadata.len(), mtime_ns(metadata)?))
}

fn revision_for(bytes: &[u8], metadata: &Metadata) -> Result<DiskRevision, CanvasDocumentError> {
    Ok(DiskRevision {
        mtime_ns: mtime_ns(metadata)?.to_string(),
        size: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
    })
}

/// Read through one file handle and require its before/after metadata to stay
/// stable. A replacement may leave this handle on the old inode, but the bytes,
/// hash, size and mtime returned here still describe one coherent snapshot; a
/// later conditional save will compare against the then-current path target.
fn read_snapshot_with_limit(
    path: &Path,
    max_bytes: u64,
) -> Result<Snapshot, CanvasDocumentError> {
    for _ in 0..3 {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(CanvasDocumentError::NotFound {
                    message: "canvas document was not found".to_string(),
                });
            }
            Err(error) => return Err(CanvasDocumentError::io("open canvas", error)),
        };
        let before = file
            .metadata()
            .map_err(|error| CanvasDocumentError::io("inspect canvas before read", error))?;
        if before.len() > max_bytes {
            return Err(CanvasDocumentError::TooLarge {
                message: "canvas document exceeds the editable size limit".to_string(),
                limit_bytes: max_bytes,
                actual_bytes: before.len(),
            });
        }

        let mut bytes = Vec::with_capacity(before.len() as usize);
        Read::by_ref(&mut file)
            .take(max_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| CanvasDocumentError::io("read canvas", error))?;
        if bytes.len() as u64 > max_bytes {
            return Err(CanvasDocumentError::TooLarge {
                message: "canvas document exceeds the editable size limit".to_string(),
                limit_bytes: max_bytes,
                actual_bytes: bytes.len() as u64,
            });
        }
        let after = file
            .metadata()
            .map_err(|error| CanvasDocumentError::io("inspect canvas after read", error))?;
        if metadata_signature(&before)? == metadata_signature(&after)?
            && after.len() == bytes.len() as u64
        {
            let revision = revision_for(&bytes, &after)?;
            return Ok(Snapshot { bytes, revision });
        }
    }
    Err(CanvasDocumentError::UnstableRead {
        message: "canvas changed repeatedly while it was being read".to_string(),
    })
}

fn read_snapshot(path: &Path) -> Result<Snapshot, CanvasDocumentError> {
    read_snapshot_with_limit(path, MAX_CANVAS_BYTES)
}

/// Make an iOS Files/Open In canvas durable while the URL callback still owns
/// access to the provider-backed file. The helper is platform-neutral so the
/// containment, size-limit and no-clobber behavior can be tested on desktop.
///
/// Files already inside the app's Documents directory are returned in place.
/// External files are read once with the iOS limit, then atomically persisted
/// under Documents/Imported Canvases using a collision-free display name.
#[cfg(any(target_os = "ios", test))]
pub(crate) fn prepare_ios_opened_canvas(
    source: &Path,
    documents_dir: &Path,
) -> Result<PathBuf, CanvasDocumentError> {
    if !source.is_absolute() || !documents_dir.is_absolute() {
        return Err(CanvasDocumentError::invalid_path(
            "opened canvas and Documents paths must be absolute",
        ));
    }
    require_canvas_extension(source)?;

    let documents = fs::canonicalize(documents_dir)
        .map_err(|error| CanvasDocumentError::io("resolve app Documents directory", error))?;
    if !documents.is_dir() {
        return Err(CanvasDocumentError::invalid_path(
            "app Documents path is not a directory",
        ));
    }

    // Do not run externally granted paths through safe_path: the Opened event is
    // the OS authorization boundary, and a Files provider may live outside the
    // app's normal roots. Canonicalization plus the regular-file check still
    // rejects broken aliases and directories.
    let source = fs::canonicalize(source)
        .map_err(|error| CanvasDocumentError::io("resolve opened canvas", error))?;
    require_canvas_extension(&source)?;
    let metadata = fs::metadata(&source)
        .map_err(|error| CanvasDocumentError::io("inspect opened canvas", error))?;
    if !metadata.is_file() {
        return Err(CanvasDocumentError::invalid_path(
            "opened canvas is not a regular file",
        ));
    }

    let snapshot = read_snapshot_with_limit(&source, IOS_MAX_CANVAS_BYTES)?;
    if source.starts_with(&documents) {
        return Ok(source);
    }

    let imported = documents.join(IOS_IMPORTED_CANVASES_DIR);
    fs::create_dir_all(&imported)
        .map_err(|error| CanvasDocumentError::io("create imported canvases directory", error))?;
    let imported = fs::canonicalize(&imported)
        .map_err(|error| CanvasDocumentError::io("resolve imported canvases directory", error))?;
    if !imported.starts_with(&documents) || !imported.is_dir() {
        return Err(CanvasDocumentError::invalid_path(
            "imported canvases directory is outside app Documents",
        ));
    }

    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("Imported Canvas");
    let mut temp = tempfile::Builder::new()
        .prefix(TEMP_PREFIX)
        .suffix(".tmp")
        .tempfile_in(&imported)
        .map_err(|error| CanvasDocumentError::io("create imported canvas temporary file", error))?;
    temp.write_all(&snapshot.bytes)
        .map_err(|error| CanvasDocumentError::io("write imported canvas", error))?;
    temp.as_file()
        .sync_all()
        .map_err(|error| CanvasDocumentError::io("sync imported canvas", error))?;

    for collision in 0..u32::MAX {
        let file_name = if collision == 0 {
            format!("{stem}.canvas")
        } else {
            format!("{stem} ({}).canvas", collision + 1)
        };
        let destination = imported.join(file_name);
        match temp.persist_noclobber(&destination) {
            Ok(_) => {
                if let Ok(directory) = File::open(&imported) {
                    let _ = directory.sync_all();
                }
                return fs::canonicalize(&destination).map_err(|error| {
                    CanvasDocumentError::io("resolve imported canvas", error)
                });
            }
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                temp = error.file;
            }
            Err(error) => {
                return Err(CanvasDocumentError::io(
                    "persist imported canvas",
                    error.error,
                ));
            }
        }
    }

    Err(CanvasDocumentError::Io {
        message: "could not allocate a unique imported canvas name".to_string(),
    })
}

fn disk_state(path: &Path) -> Result<ExpectedDiskState, CanvasDocumentError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(CanvasDocumentError::invalid_path(
                "canvas target changed to a symbolic link; retry the operation",
            ))
        }
        Ok(metadata) if !metadata.is_file() => Err(CanvasDocumentError::invalid_path(
            "canvas target is not a regular file",
        )),
        Ok(_) => Ok(ExpectedDiskState::Present {
            revision: read_snapshot(path)?.revision,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ExpectedDiskState::Missing)
        }
        Err(error) => Err(CanvasDocumentError::io("inspect canvas", error)),
    }
}

fn conflict(
    expected: ExpectedDiskState,
    actual: ExpectedDiskState,
    path: &Path,
    message: &str,
) -> CanvasDocumentError {
    CanvasDocumentError::Conflict {
        message: message.to_string(),
        expected,
        actual,
        canonical_path: path_string(path),
    }
}

fn open_inner(requested_path: String) -> Result<OpenCanvasResult, CanvasDocumentError> {
    let canonical_path = resolve_canvas_path(&requested_path)?;
    let snapshot = read_snapshot(&canonical_path)?;
    let text = String::from_utf8(snapshot.bytes).map_err(|_| CanvasDocumentError::InvalidUtf8 {
        message: "canvas document is not valid UTF-8".to_string(),
    })?;
    Ok(OpenCanvasResult {
        text,
        revision: snapshot.revision,
        requested_path,
        canonical_path: path_string(&canonical_path),
    })
}

fn probe_inner(requested_path: String) -> Result<ProbeCanvasResult, CanvasDocumentError> {
    let canonical_path = resolve_canvas_path(&requested_path)?;
    match disk_state(&canonical_path)? {
        ExpectedDiskState::Missing => Ok(ProbeCanvasResult::Missing {
            requested_path,
            canonical_path: path_string(&canonical_path),
        }),
        ExpectedDiskState::Present { revision } => Ok(ProbeCanvasResult::Present {
            revision,
            requested_path,
            canonical_path: path_string(&canonical_path),
        }),
    }
}

fn replace_inner(
    requested_path: String,
    text: String,
    expected: ExpectedDiskState,
    force: bool,
) -> Result<SaveCanvasResult, CanvasDocumentError> {
    if text.len() as u64 > MAX_CANVAS_BYTES {
        return Err(CanvasDocumentError::TooLarge {
            message: "canvas document exceeds the editable size limit".to_string(),
            limit_bytes: MAX_CANVAS_BYTES,
            actual_bytes: text.len() as u64,
        });
    }

    let initial_path = resolve_canvas_path(&requested_path)?;
    let path_lock = lock_for_path(&initial_path);
    let _guard = path_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // Resolve again under the path lock. If an alias was retargeted while this
    // command waited, retrying will acquire the lock for its new canonical key.
    let canonical_path = resolve_canvas_path(&requested_path)?;
    if canonical_path != initial_path {
        return Err(CanvasDocumentError::invalid_path(
            "canvas path target changed while waiting to save; retry the operation",
        ));
    }
    let parent = canonical_path
        .parent()
        .ok_or_else(|| CanvasDocumentError::invalid_path("canvas path has no parent directory"))?;

    let mut temp = tempfile::Builder::new()
        .prefix(TEMP_PREFIX)
        .suffix(".tmp")
        .tempfile_in(parent)
        .map_err(|error| CanvasDocumentError::io("create canvas temporary file", error))?;
    temp.write_all(text.as_bytes())
        .map_err(|error| CanvasDocumentError::io("write canvas temporary file", error))?;

    // Preserve permissions when replacing an existing document. New documents
    // intentionally retain tempfile's private default permissions.
    #[cfg(unix)]
    if let Ok(metadata) = fs::metadata(&canonical_path) {
        fs::set_permissions(temp.path(), metadata.permissions())
            .map_err(|error| CanvasDocumentError::io("copy canvas permissions", error))?;
    }

    temp.as_file()
        .sync_all()
        .map_err(|error| CanvasDocumentError::io("sync canvas temporary file", error))?;

    // Keep this check immediately adjacent to replacement. There remains an
    // unavoidable window in which a non-cooperating process can write.
    let actual = disk_state(&canonical_path)?;
    if !force && actual != expected {
        return Err(conflict(
            expected,
            actual,
            &canonical_path,
            "canvas changed on disk",
        ));
    }
    if force && matches!(actual, ExpectedDiskState::Missing) {
        return Err(conflict(
            expected,
            actual,
            &canonical_path,
            "explicit overwrite requires an existing canvas",
        ));
    }

    // Creation gets the stronger no-clobber primitive as a second guard after
    // the revision check. Existing-document saves use the platform overwrite
    // rename (MoveFileExW(REPLACE_EXISTING) on Windows in `tempfile`, rename on
    // Unix/iOS); neither path deletes the destination first.
    let persist_result = if matches!(expected, ExpectedDiskState::Missing) {
        temp.persist_noclobber(&canonical_path)
    } else {
        temp.persist(&canonical_path)
    };
    if let Err(error) = persist_result {
        if matches!(expected, ExpectedDiskState::Missing)
            && error.error.kind() == std::io::ErrorKind::AlreadyExists
        {
            let actual = disk_state(&canonical_path)?;
            return Err(conflict(
                expected,
                actual,
                &canonical_path,
                "canvas appeared on disk while it was being created",
            ));
        }
        return Err(CanvasDocumentError::io(
            "replace canvas document",
            error.error,
        ));
    }
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }

    let saved = read_snapshot(&canonical_path)?;
    let intended_hash = hex::encode(Sha256::digest(text.as_bytes()));
    if saved.revision.sha256 != intended_hash || saved.revision.size != text.len() as u64 {
        return Err(conflict(
            expected,
            ExpectedDiskState::Present {
                revision: saved.revision,
            },
            &canonical_path,
            "canvas was changed by another process immediately after save",
        ));
    }

    Ok(SaveCanvasResult {
        revision: saved.revision,
        canonical_path: path_string(&canonical_path),
    })
}

fn create_inner(
    requested_path: String,
    text: String,
) -> Result<SaveCanvasResult, CanvasDocumentError> {
    replace_inner(requested_path, text, ExpectedDiskState::Missing, false)
}

fn save_inner(
    requested_path: String,
    text: String,
    expected: ExpectedDiskState,
    force: bool,
) -> Result<SaveCanvasResult, CanvasDocumentError> {
    if !matches!(expected, ExpectedDiskState::Present { .. }) {
        return Err(CanvasDocumentError::invalid_path(
            "save requires an expected present revision; use create for a missing target",
        ));
    }
    replace_inner(requested_path, text, expected, force)
}

async fn run_blocking<T, F>(operation: F) -> Result<T, CanvasDocumentError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CanvasDocumentError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|_| CanvasDocumentError::Io {
            message: "canvas document worker stopped unexpectedly".to_string(),
        })?
}

#[tauri::command]
pub async fn canvas_document_open(path: String) -> Result<OpenCanvasResult, CanvasDocumentError> {
    run_blocking(move || open_inner(path)).await
}

#[tauri::command]
pub async fn canvas_document_probe(path: String) -> Result<ProbeCanvasResult, CanvasDocumentError> {
    run_blocking(move || probe_inner(path)).await
}

#[tauri::command]
pub async fn canvas_document_create(
    path: String,
    text: String,
) -> Result<SaveCanvasResult, CanvasDocumentError> {
    run_blocking(move || create_inner(path, text)).await
}

#[tauri::command]
pub async fn canvas_document_save(
    path: String,
    text: String,
    expected: ExpectedDiskState,
    force: bool,
) -> Result<SaveCanvasResult, CanvasDocumentError> {
    run_blocking(move || save_inner(path, text, expected, force)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    fn revision(path: &Path) -> DiskRevision {
        read_snapshot(path).unwrap().revision
    }

    #[test]
    fn ios_open_in_copies_an_external_canvas_into_documents() {
        let directory = tempfile::tempdir().unwrap();
        let documents = directory.path().join("Documents");
        let external = directory.path().join("Provider");
        fs::create_dir_all(&documents).unwrap();
        fs::create_dir_all(&external).unwrap();
        let source = external.join("board.canvas");
        fs::write(&source, r#"{"nodes":[],"edges":[]}"#).unwrap();

        let imported = prepare_ios_opened_canvas(&source, &documents).unwrap();

        assert_eq!(
            imported.parent(),
            Some(
                fs::canonicalize(documents.join(IOS_IMPORTED_CANVASES_DIR))
                    .unwrap()
                    .as_path()
            )
        );
        assert_eq!(imported.file_name().unwrap(), "board.canvas");
        assert_eq!(fs::read_to_string(&imported).unwrap(), fs::read_to_string(&source).unwrap());
        assert!(source.exists(), "import must not move or delete the provider file");
    }

    #[test]
    fn ios_open_in_allocates_a_unique_name_without_overwriting() {
        let directory = tempfile::tempdir().unwrap();
        let documents = directory.path().join("Documents");
        let imported_dir = documents.join(IOS_IMPORTED_CANVASES_DIR);
        let external = directory.path().join("Provider");
        fs::create_dir_all(&imported_dir).unwrap();
        fs::create_dir_all(&external).unwrap();
        let existing = imported_dir.join("board.canvas");
        fs::write(&existing, "existing").unwrap();
        let source = external.join("board.canvas");
        fs::write(&source, "incoming").unwrap();

        let imported = prepare_ios_opened_canvas(&source, &documents).unwrap();

        assert_eq!(imported.file_name().unwrap(), "board (2).canvas");
        assert_eq!(fs::read_to_string(existing).unwrap(), "existing");
        assert_eq!(fs::read_to_string(imported).unwrap(), "incoming");
    }

    #[test]
    fn ios_open_in_keeps_a_canvas_already_inside_documents_in_place() {
        let directory = tempfile::tempdir().unwrap();
        let documents = directory.path().join("Documents");
        let nested = documents.join("Boards");
        fs::create_dir_all(&nested).unwrap();
        let source = nested.join("board.canvas");
        fs::write(&source, "{}").unwrap();

        let opened = prepare_ios_opened_canvas(&source, &documents).unwrap();

        assert_eq!(opened, fs::canonicalize(&source).unwrap());
        assert!(
            !documents.join(IOS_IMPORTED_CANVASES_DIR).exists(),
            "an app-owned document must not create an imported duplicate"
        );
    }

    #[test]
    fn ios_open_in_rejects_an_oversized_external_canvas_before_copying() {
        let directory = tempfile::tempdir().unwrap();
        let documents = directory.path().join("Documents");
        let external = directory.path().join("Provider");
        fs::create_dir_all(&documents).unwrap();
        fs::create_dir_all(&external).unwrap();
        let source = external.join("large.canvas");
        File::create(&source)
            .unwrap()
            .set_len(IOS_MAX_CANVAS_BYTES + 1)
            .unwrap();

        let error = prepare_ios_opened_canvas(&source, &documents).unwrap_err();

        assert!(matches!(
            error,
            CanvasDocumentError::TooLarge {
                limit_bytes: IOS_MAX_CANVAS_BYTES,
                actual_bytes,
                ..
            } if actual_bytes == IOS_MAX_CANVAS_BYTES + 1
        ));
        assert!(!documents.join(IOS_IMPORTED_CANVASES_DIR).exists());
    }

    #[test]
    fn ios_open_in_rejects_non_canvas_files_without_creating_an_import() {
        let directory = tempfile::tempdir().unwrap();
        let documents = directory.path().join("Documents");
        fs::create_dir_all(&documents).unwrap();
        let source = directory.path().join("note.md");
        fs::write(&source, "# note").unwrap();

        let error = prepare_ios_opened_canvas(&source, &documents).unwrap_err();

        assert!(matches!(error, CanvasDocumentError::InvalidPath { .. }));
        assert!(!documents.join(IOS_IMPORTED_CANVASES_DIR).exists());
    }

    #[test]
    fn open_returns_text_and_revision_from_the_same_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("board.canvas");
        let text = r#"{"nodes":[],"edges":[]}"#;
        fs::write(&path, text).unwrap();

        let opened = open_inner(path_string(&path)).unwrap();

        assert_eq!(opened.text, text);
        assert_eq!(opened.revision.size, text.len() as u64);
        assert_eq!(
            opened.revision.sha256,
            hex::encode(Sha256::digest(text.as_bytes()))
        );
        assert_eq!(
            opened.canonical_path,
            path_string(&fs::canonicalize(path).unwrap())
        );
    }

    #[test]
    fn probe_reports_a_canonical_missing_target() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("new.canvas");

        let probed = probe_inner(path_string(&path)).unwrap();

        assert_eq!(
            probed,
            ProbeCanvasResult::Missing {
                requested_path: path_string(&path),
                canonical_path: path_string(
                    &fs::canonicalize(directory.path())
                        .unwrap()
                        .join("new.canvas")
                ),
            }
        );
    }

    #[test]
    fn create_refuses_to_replace_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("board.canvas");
        fs::write(&path, "old").unwrap();

        let error = create_inner(path_string(&path), "new".to_string()).unwrap_err();

        assert!(matches!(
            error,
            CanvasDocumentError::Conflict {
                expected: ExpectedDiskState::Missing,
                actual: ExpectedDiskState::Present { .. },
                ..
            }
        ));
        assert_eq!(fs::read_to_string(path).unwrap(), "old");
    }

    #[test]
    fn create_writes_a_new_file_and_returns_its_exact_revision() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("board.canvas");
        let text = r#"{"nodes":[]}"#.to_string();

        let saved = create_inner(path_string(&path), text.clone()).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert_eq!(saved.revision, revision(&path));
        assert_eq!(
            saved.canonical_path,
            path_string(&fs::canonicalize(path).unwrap())
        );
        assert!(fs::read_dir(directory.path()).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(TEMP_PREFIX)));
    }

    #[test]
    fn conditional_save_rejects_a_stale_revision_without_touching_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("board.canvas");
        fs::write(&path, "first").unwrap();
        let stale = revision(&path);
        fs::write(&path, "external").unwrap();

        let error = save_inner(
            path_string(&path),
            "local".to_string(),
            ExpectedDiskState::Present { revision: stale },
            false,
        )
        .unwrap_err();

        assert!(matches!(error, CanvasDocumentError::Conflict { .. }));
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        assert!(fs::read_dir(directory.path()).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(TEMP_PREFIX)));
    }

    #[test]
    fn force_overwrites_an_existing_file_but_does_not_recreate_a_missing_one() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("board.canvas");
        fs::write(&path, "first").unwrap();
        let stale = revision(&path);
        fs::write(&path, "external").unwrap();

        let saved = save_inner(
            path_string(&path),
            "local".to_string(),
            ExpectedDiskState::Present {
                revision: stale.clone(),
            },
            true,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "local");
        assert_eq!(saved.revision, revision(&path));

        fs::remove_file(&path).unwrap();
        let error = save_inner(
            path_string(&path),
            "recreate".to_string(),
            ExpectedDiskState::Present { revision: stale },
            true,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            CanvasDocumentError::Conflict {
                actual: ExpectedDiskState::Missing,
                ..
            }
        ));
        assert!(!path.exists());
    }

    #[test]
    fn simultaneous_saves_with_one_revision_cannot_both_win() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("board.canvas");
        fs::write(&path, "base").unwrap();
        let expected = ExpectedDiskState::Present {
            revision: revision(&path),
        };
        let barrier = Arc::new(Barrier::new(3));

        let handles: Vec<_> = ["one", "two"]
            .into_iter()
            .map(|text| {
                let barrier = Arc::clone(&barrier);
                let expected = expected.clone();
                let path = path_string(&path);
                std::thread::spawn(move || {
                    barrier.wait();
                    save_inner(path, text.to_string(), expected, false)
                })
            })
            .collect();
        barrier.wait();
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(CanvasDocumentError::Conflict { .. })))
                .count(),
            1
        );
        assert!(matches!(
            fs::read_to_string(path).unwrap().as_str(),
            "one" | "two"
        ));
    }

    #[test]
    fn missing_parent_and_non_canvas_extension_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let missing_parent = directory.path().join("missing").join("board.canvas");
        assert!(matches!(
            create_inner(path_string(&missing_parent), "{}".to_string()),
            Err(CanvasDocumentError::Io { .. })
        ));
        let wrong_extension = directory.path().join("board.json");
        assert!(matches!(
            create_inner(path_string(&wrong_extension), "{}".to_string()),
            Err(CanvasDocumentError::InvalidPath { .. })
        ));
    }

    #[test]
    fn command_payloads_use_the_frontend_camel_case_contract() {
        let revision = DiskRevision {
            mtime_ns: "123".to_string(),
            size: 2,
            sha256: "abcd".to_string(),
        };
        let probe = serde_json::to_value(ProbeCanvasResult::Present {
            revision: revision.clone(),
            requested_path: "/tmp/a.canvas".to_string(),
            canonical_path: "/tmp/a.canvas".to_string(),
        })
        .unwrap();
        assert_eq!(probe["kind"], "present");
        assert_eq!(probe["requestedPath"], "/tmp/a.canvas");
        assert_eq!(probe["canonicalPath"], "/tmp/a.canvas");
        assert_eq!(probe["revision"]["mtimeNs"], "123");

        let error = serde_json::to_value(CanvasDocumentError::Conflict {
            message: "changed".to_string(),
            expected: ExpectedDiskState::Present {
                revision: revision.clone(),
            },
            actual: ExpectedDiskState::Present { revision },
            canonical_path: "/tmp/a.canvas".to_string(),
        })
        .unwrap();
        assert_eq!(error["kind"], "conflict");
        assert_eq!(error["canonicalPath"], "/tmp/a.canvas");
        assert_eq!(error["expected"]["kind"], "present");
    }

    #[cfg(unix)]
    #[test]
    fn existing_symlink_is_saved_through_its_canonical_target() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target.canvas");
        let alias = directory.path().join("alias.canvas");
        fs::write(&target, "old").unwrap();
        symlink(&target, &alias).unwrap();
        let expected = ExpectedDiskState::Present {
            revision: revision(&target),
        };

        let saved = save_inner(path_string(&alias), "new".to_string(), expected, false).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert!(fs::symlink_metadata(&alias)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            saved.canonical_path,
            path_string(&fs::canonicalize(target).unwrap())
        );
    }
}
