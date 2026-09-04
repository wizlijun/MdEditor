//! Read-only, root-confined resource access for JSON Canvas surfaces.
//!
//! Canvas documents may contain paths controlled by the document itself.  The
//! webview therefore never receives a filesystem URL or unrestricted read
//! capability: every request is canonicalized here and checked after symlink
//! resolution against the explicitly selected Canvas resource root.

use serde::Serialize;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Take, Write};
use std::path::{Path, PathBuf};

const MAX_CANVAS_RESOURCE_BYTES: u64 = 12 * 1024 * 1024;
const MAX_CANVAS_IMPORT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasResourceResult {
    pub canonical_path: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasResourceImportResult {
    pub relative_path: String,
    pub canonical_path: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasResourceResolveResult {
    pub canonical_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CanvasResourceError {
    InvalidRoot { message: String },
    InvalidPath { message: String },
    NotFound { message: String },
    OutsideRoot { message: String },
    NotFile { message: String },
    UnsupportedType { message: String },
    InvalidContent { message: String },
    TooLarge {
        message: String,
        limit_bytes: u64,
        actual_bytes: u64,
    },
    Io { message: String },
}

impl CanvasResourceError {
    fn io(action: &str, error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound {
                message: format!("{action}: not found"),
            },
            std::io::ErrorKind::PermissionDenied => Self::Io {
                message: format!("{action}: permission denied"),
            },
            _ => Self::Io {
                message: format!("{action}: I/O error"),
            },
        }
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn canonical_root(root: &str) -> Result<PathBuf, CanvasResourceError> {
    let requested = Path::new(root);
    if !requested.is_absolute() {
        return Err(CanvasResourceError::InvalidRoot {
            message: "canvas resource root must be absolute".to_string(),
        });
    }
    let canonical = fs::canonicalize(requested)
        .map_err(|error| CanvasResourceError::io("resolve canvas resource root", error))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| CanvasResourceError::io("inspect canvas resource root", error))?;
    if !metadata.is_dir() {
        return Err(CanvasResourceError::InvalidRoot {
            message: "canvas resource root is not a directory".to_string(),
        });
    }
    Ok(canonical)
}

fn canonical_target(root: &Path, target: &str) -> Result<PathBuf, CanvasResourceError> {
    if target.is_empty() || target.contains('\0') {
        return Err(CanvasResourceError::InvalidPath {
            message: "canvas resource path is empty or invalid".to_string(),
        });
    }
    let requested = Path::new(target);
    let joined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
    let canonical = fs::canonicalize(&joined)
        .map_err(|error| CanvasResourceError::io("resolve canvas resource", error))?;
    if !canonical.starts_with(root) {
        return Err(CanvasResourceError::OutsideRoot {
            message: "canvas resource resolves outside its allowed root".to_string(),
        });
    }
    let metadata = fs::metadata(&canonical)
        .map_err(|error| CanvasResourceError::io("inspect canvas resource", error))?;
    if !metadata.is_file() {
        return Err(CanvasResourceError::NotFile {
            message: "canvas resource is not a regular file".to_string(),
        });
    }
    Ok(canonical)
}

fn allowed_mime(path: &Path) -> Result<&'static str, CanvasResourceError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "webp" => Ok("image/webp"),
        "bmp" => Ok("image/bmp"),
        "ico" => Ok("image/x-icon"),
        "avif" => Ok("image/avif"),
        "heic" => Ok("image/heic"),
        "heif" => Ok("image/heif"),
        // SVG can contain active content and external subresources.  It is
        // deliberately outside the first-version Canvas resource profile.
        _ => Err(CanvasResourceError::UnsupportedType {
            message: "canvas resource type is not allowed".to_string(),
        }),
    }
}

fn has_ftyp_brand(bytes: &[u8], brands: &[&[u8; 4]]) -> bool {
    bytes.len() >= 12
        && &bytes[4..8] == b"ftyp"
        && bytes[8..].chunks_exact(4).any(|brand| {
            brands
                .iter()
                .any(|allowed| brand == allowed.as_slice())
        })
}

fn content_matches_mime(bytes: &[u8], mime: &str) -> bool {
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => {
            bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP"
        }
        "image/bmp" => bytes.starts_with(b"BM"),
        "image/x-icon" => bytes.starts_with(&[0, 0, 1, 0]),
        "image/avif" => has_ftyp_brand(bytes, &[b"avif", b"avis"]),
        "image/heic" => has_ftyp_brand(bytes, &[b"heic", b"heix", b"hevc", b"hevx"]),
        "image/heif" => has_ftyp_brand(bytes, &[b"mif1", b"msf1", b"heic", b"heix"]),
        _ => false,
    }
}

fn read_limited(file: File, expected_size: u64) -> Result<Vec<u8>, CanvasResourceError> {
    if expected_size > MAX_CANVAS_RESOURCE_BYTES {
        return Err(CanvasResourceError::TooLarge {
            message: "canvas resource exceeds the size limit".to_string(),
            limit_bytes: MAX_CANVAS_RESOURCE_BYTES,
            actual_bytes: expected_size,
        });
    }
    let mut bytes = Vec::with_capacity(expected_size as usize);
    let mut limited: Take<File> = file.take(MAX_CANVAS_RESOURCE_BYTES + 1);
    limited
        .read_to_end(&mut bytes)
        .map_err(|error| CanvasResourceError::io("read canvas resource", error))?;
    if bytes.len() as u64 > MAX_CANVAS_RESOURCE_BYTES {
        return Err(CanvasResourceError::TooLarge {
            message: "canvas resource exceeds the size limit".to_string(),
            limit_bytes: MAX_CANVAS_RESOURCE_BYTES,
            actual_bytes: bytes.len() as u64,
        });
    }
    Ok(bytes)
}

fn read_inner(root: String, target: String) -> Result<CanvasResourceResult, CanvasResourceError> {
    let root = canonical_root(&root)?;
    let target = canonical_target(&root, &target)?;
    let mime = allowed_mime(&target)?;
    let file = File::open(&target)
        .map_err(|error| CanvasResourceError::io("open canvas resource", error))?;
    let metadata = file
        .metadata()
        .map_err(|error| CanvasResourceError::io("inspect canvas resource", error))?;
    if !metadata.is_file() {
        return Err(CanvasResourceError::NotFile {
            message: "canvas resource is not a regular file".to_string(),
        });
    }
    let bytes = read_limited(file, metadata.len())?;
    if !content_matches_mime(&bytes, mime) {
        return Err(CanvasResourceError::InvalidContent {
            message: "canvas resource content does not match its allowed image type".to_string(),
        });
    }
    Ok(CanvasResourceResult {
        canonical_path: path_string(&target),
        mime: mime.to_string(),
        bytes,
    })
}

fn resolve_inner(
    root: String,
    target: String,
) -> Result<CanvasResourceResolveResult, CanvasResourceError> {
    let root = canonical_root(&root)?;
    let target = canonical_target(&root, &target)?;
    Ok(CanvasResourceResolveResult {
        canonical_path: path_string(&target),
    })
}

fn canonical_canvas_in_root(root: &Path, canvas_path: &str) -> Result<PathBuf, CanvasResourceError> {
    let requested = Path::new(canvas_path);
    if !requested.is_absolute() {
        return Err(CanvasResourceError::InvalidPath {
            message: "canvas path must be absolute".to_string(),
        });
    }
    let canonical = fs::canonicalize(requested)
        .map_err(|error| CanvasResourceError::io("resolve canvas document", error))?;
    if !canonical.starts_with(root) {
        return Err(CanvasResourceError::OutsideRoot {
            message: "canvas document resolves outside its resource root".to_string(),
        });
    }
    if !canonical
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("canvas"))
    {
        return Err(CanvasResourceError::InvalidPath {
            message: "canvas document must use the .canvas extension".to_string(),
        });
    }
    if !fs::metadata(&canonical)
        .map_err(|error| CanvasResourceError::io("inspect canvas document", error))?
        .is_file()
    {
        return Err(CanvasResourceError::NotFile {
            message: "canvas document is not a regular file".to_string(),
        });
    }
    Ok(canonical)
}

fn import_directory(root: &Path, canvas: &Path) -> Result<PathBuf, CanvasResourceError> {
    let stem = canvas
        .file_stem()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CanvasResourceError::InvalidPath {
            message: "canvas document has no valid filename".to_string(),
        })?;
    let mut directory_name = OsString::from(stem);
    directory_name.push("_files");
    let requested = canvas
        .parent()
        .ok_or_else(|| CanvasResourceError::InvalidPath {
            message: "canvas document has no parent directory".to_string(),
        })?
        .join(directory_name);
    match fs::create_dir(&requested) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(CanvasResourceError::io(
                "create canvas resource directory",
                error,
            ));
        }
    }
    let canonical = fs::canonicalize(&requested)
        .map_err(|error| CanvasResourceError::io("resolve canvas resource directory", error))?;
    if !canonical.starts_with(root) {
        return Err(CanvasResourceError::OutsideRoot {
            message: "canvas resource directory resolves outside its allowed root".to_string(),
        });
    }
    if !fs::metadata(&canonical)
        .map_err(|error| CanvasResourceError::io("inspect canvas resource directory", error))?
        .is_dir()
    {
        return Err(CanvasResourceError::InvalidPath {
            message: "canvas resource directory is not a directory".to_string(),
        });
    }
    Ok(canonical)
}

fn numbered_filename(source: &Path, suffix: u32) -> Result<OsString, CanvasResourceError> {
    let filename = source
        .file_name()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CanvasResourceError::InvalidPath {
            message: "import source has no valid filename".to_string(),
        })?;
    if suffix == 1 {
        return Ok(filename.to_os_string());
    }
    let stem = source.file_stem().unwrap_or(filename);
    let mut numbered = OsString::from(stem);
    numbered.push(format!("-{suffix}"));
    if let Some(extension) = source.extension().filter(|value| !value.is_empty()) {
        numbered.push(".");
        numbered.push(extension);
    }
    Ok(numbered)
}

fn copy_limited(mut source: File, mut destination: File) -> Result<u64, CanvasResourceError> {
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|error| CanvasResourceError::io("read import source", error))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_CANVAS_IMPORT_BYTES {
            return Err(CanvasResourceError::TooLarge {
                message: "canvas import exceeds the size limit".to_string(),
                limit_bytes: MAX_CANVAS_IMPORT_BYTES,
                actual_bytes: total,
            });
        }
        destination
            .write_all(&buffer[..read])
            .map_err(|error| CanvasResourceError::io("write imported canvas resource", error))?;
    }
    destination
        .sync_all()
        .map_err(|error| CanvasResourceError::io("sync imported canvas resource", error))?;
    Ok(total)
}

fn root_relative_path(root: &Path, target: &Path) -> Result<String, CanvasResourceError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| CanvasResourceError::OutsideRoot {
            message: "import destination is outside its allowed root".to_string(),
        })?;
    let value = relative.to_string_lossy().replace('\\', "/");
    if value.is_empty() {
        return Err(CanvasResourceError::InvalidPath {
            message: "import destination has no relative path".to_string(),
        });
    }
    Ok(value)
}

fn import_inner(
    root: String,
    canvas_path: String,
    source_path: String,
) -> Result<CanvasResourceImportResult, CanvasResourceError> {
    let root = canonical_root(&root)?;
    let canvas = canonical_canvas_in_root(&root, &canvas_path)?;
    let requested_source = Path::new(&source_path);
    if !requested_source.is_absolute() {
        return Err(CanvasResourceError::InvalidPath {
            message: "import source path must be absolute".to_string(),
        });
    }
    let source = fs::canonicalize(requested_source)
        .map_err(|error| CanvasResourceError::io("resolve import source", error))?;
    let source_metadata = fs::metadata(&source)
        .map_err(|error| CanvasResourceError::io("inspect import source", error))?;
    if !source_metadata.is_file() {
        return Err(CanvasResourceError::NotFile {
            message: "import source is not a regular file".to_string(),
        });
    }
    if source_metadata.len() > MAX_CANVAS_IMPORT_BYTES {
        return Err(CanvasResourceError::TooLarge {
            message: "canvas import exceeds the size limit".to_string(),
            limit_bytes: MAX_CANVAS_IMPORT_BYTES,
            actual_bytes: source_metadata.len(),
        });
    }
    if source.starts_with(&root) {
        return Ok(CanvasResourceImportResult {
            relative_path: root_relative_path(&root, &source)?,
            canonical_path: path_string(&source),
            size: source_metadata.len(),
        });
    }

    let directory = import_directory(&root, &canvas)?;
    let mut source_file = Some(
        File::open(&source)
            .map_err(|error| CanvasResourceError::io("open import source", error))?,
    );
    for suffix in 1..=10_000 {
        let destination = directory.join(numbered_filename(&source, suffix)?);
        let destination_file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(CanvasResourceError::io(
                    "create imported canvas resource",
                    error,
                ));
            }
        };
        let copy_result = copy_limited(source_file.take().unwrap(), destination_file);
        let size = match copy_result {
            Ok(size) => size,
            Err(error) => {
                let _ = fs::remove_file(&destination);
                return Err(error);
            }
        };
        let canonical = match fs::canonicalize(&destination) {
            Ok(path) if path.starts_with(&root) => path,
            Ok(_) => {
                let _ = fs::remove_file(&destination);
                return Err(CanvasResourceError::OutsideRoot {
                    message: "import destination resolved outside its allowed root".to_string(),
                });
            }
            Err(error) => {
                let _ = fs::remove_file(&destination);
                return Err(CanvasResourceError::io(
                    "resolve imported canvas resource",
                    error,
                ));
            }
        };
        return Ok(CanvasResourceImportResult {
            relative_path: root_relative_path(&root, &canonical)?,
            canonical_path: path_string(&canonical),
            size,
        });
    }
    Err(CanvasResourceError::Io {
        message: "could not allocate a unique imported resource filename".to_string(),
    })
}

#[tauri::command]
pub async fn canvas_resource_read(
    root: String,
    target: String,
) -> Result<tauri::ipc::Response, CanvasResourceError> {
    let result = tauri::async_runtime::spawn_blocking(move || read_inner(root, target))
        .await
        .map_err(|_| CanvasResourceError::Io {
            message: "canvas resource worker stopped unexpectedly".to_string(),
        })??;
    let mime_id = match result.mime.as_str() {
        "image/png" => 1,
        "image/jpeg" => 2,
        "image/gif" => 3,
        "image/webp" => 4,
        "image/bmp" => 5,
        "image/x-icon" => 6,
        "image/avif" => 7,
        "image/heic" => 8,
        "image/heif" => 9,
        _ => {
            return Err(CanvasResourceError::UnsupportedType {
                message: "canvas resource type is not allowed".to_string(),
            });
        }
    };
    // Binary response: one-byte MIME discriminator followed by file bytes.
    // A serde Vec<u8> would expand into a large JSON number array in the IPC.
    let mut payload = Vec::with_capacity(result.bytes.len() + 1);
    payload.push(mime_id);
    payload.extend(result.bytes);
    Ok(tauri::ipc::Response::new(payload))
}

#[tauri::command]
pub async fn canvas_resource_resolve(
    root: String,
    target: String,
) -> Result<CanvasResourceResolveResult, CanvasResourceError> {
    tauri::async_runtime::spawn_blocking(move || resolve_inner(root, target))
        .await
        .map_err(|_| CanvasResourceError::Io {
            message: "canvas resource resolver stopped unexpectedly".to_string(),
        })?
}

#[tauri::command]
pub async fn canvas_resource_import(
    root: String,
    canvas_path: String,
    source_path: String,
) -> Result<CanvasResourceImportResult, CanvasResourceError> {
    tauri::async_runtime::spawn_blocking(move || import_inner(root, canvas_path, source_path))
        .await
        .map_err(|_| CanvasResourceError::Io {
            message: "canvas resource import worker stopped unexpectedly".to_string(),
        })?
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nminimal";

    #[test]
    fn reads_a_valid_image_inside_the_canonical_root() {
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join("image.png");
        fs::write(&image, PNG).unwrap();

        let result = read_inner(path_string(directory.path()), path_string(&image)).unwrap();

        assert_eq!(result.mime, "image/png");
        assert_eq!(result.bytes, PNG);
        assert_eq!(result.canonical_path, path_string(&fs::canonicalize(image).unwrap()));
    }

    #[test]
    fn rejects_outside_paths_and_non_files() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let image = outside.path().join("outside.png");
        fs::write(&image, PNG).unwrap();

        assert!(matches!(
            read_inner(path_string(root.path()), path_string(&image)),
            Err(CanvasResourceError::OutsideRoot { .. })
        ));
        assert!(matches!(
            read_inner(path_string(root.path()), path_string(root.path())),
            Err(CanvasResourceError::NotFile { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_that_resolves_outside_the_root() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let image = outside.path().join("outside.png");
        let alias = root.path().join("alias.png");
        fs::write(&image, PNG).unwrap();
        symlink(&image, &alias).unwrap();

        assert!(matches!(
            read_inner(path_string(root.path()), path_string(&alias)),
            Err(CanvasResourceError::OutsideRoot { .. })
        ));
    }

    #[test]
    fn rejects_svg_and_mismatched_image_content() {
        let directory = tempfile::tempdir().unwrap();
        let svg = directory.path().join("active.svg");
        let fake_png = directory.path().join("fake.png");
        fs::write(&svg, b"<svg><script>alert(1)</script></svg>").unwrap();
        fs::write(&fake_png, b"<html>not an image</html>").unwrap();

        assert!(matches!(
            read_inner(path_string(directory.path()), path_string(&svg)),
            Err(CanvasResourceError::UnsupportedType { .. })
        ));
        assert!(matches!(
            read_inner(path_string(directory.path()), path_string(&fake_png)),
            Err(CanvasResourceError::InvalidContent { .. })
        ));
    }

    #[test]
    fn enforces_the_resource_size_limit_before_reading() {
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join("large.png");
        let file = File::create(&image).unwrap();
        file.set_len(MAX_CANVAS_RESOURCE_BYTES + 1).unwrap();

        assert!(matches!(
            read_inner(path_string(directory.path()), path_string(&image)),
            Err(CanvasResourceError::TooLarge { .. })
        ));
    }

    #[test]
    fn command_payload_uses_camel_case_fields() {
        let value = serde_json::to_value(CanvasResourceResult {
            canonical_path: "/vault/image.png".to_string(),
            mime: "image/png".to_string(),
            bytes: vec![1, 2, 3],
        })
        .unwrap();
        assert_eq!(value["canonicalPath"], "/vault/image.png");
        assert_eq!(value["mime"], "image/png");
        assert_eq!(value["bytes"], serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn resolves_any_regular_file_inside_root_without_reading_it() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("archive.zip");
        fs::write(&file, b"not inspected").unwrap();

        let result = resolve_inner(path_string(directory.path()), path_string(&file)).unwrap();

        assert_eq!(result.canonical_path, path_string(&fs::canonicalize(file).unwrap()));
    }

    #[test]
    fn imports_outside_files_without_overwriting_existing_names() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let canvas = root.path().join("board.canvas");
        let source = outside.path().join("photo.png");
        fs::write(&canvas, b"{}").unwrap();
        fs::write(&source, PNG).unwrap();

        let first = import_inner(
            path_string(root.path()),
            path_string(&canvas),
            path_string(&source),
        )
        .unwrap();
        let second = import_inner(
            path_string(root.path()),
            path_string(&canvas),
            path_string(&source),
        )
        .unwrap();

        assert_eq!(first.relative_path, "board_files/photo.png");
        assert_eq!(second.relative_path, "board_files/photo-2.png");
        assert_eq!(fs::read(first.canonical_path).unwrap(), PNG);
        assert_eq!(fs::read(second.canonical_path).unwrap(), PNG);
    }

    #[test]
    fn keeps_an_existing_inside_file_without_copying_it() {
        let root = tempfile::tempdir().unwrap();
        let canvas = root.path().join("board.canvas");
        let assets = root.path().join("assets");
        let source = assets.join("photo.png");
        fs::create_dir(&assets).unwrap();
        fs::write(&canvas, b"{}").unwrap();
        fs::write(&source, PNG).unwrap();

        let imported = import_inner(
            path_string(root.path()),
            path_string(&canvas),
            path_string(&source),
        )
        .unwrap();

        assert_eq!(imported.relative_path, "assets/photo.png");
        assert!(!root.path().join("board_files").exists());
    }

    #[cfg(unix)]
    #[test]
    fn import_rejects_a_symlinked_resource_directory_outside_root() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source_dir = tempfile::tempdir().unwrap();
        let canvas = root.path().join("board.canvas");
        let source = source_dir.path().join("note.canvas");
        fs::write(&canvas, b"{}").unwrap();
        fs::write(&source, b"{}").unwrap();
        symlink(outside.path(), root.path().join("board_files")).unwrap();

        assert!(matches!(
            import_inner(
                path_string(root.path()),
                path_string(&canvas),
                path_string(&source)
            ),
            Err(CanvasResourceError::OutsideRoot { .. })
        ));
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }
}
