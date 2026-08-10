use std::path::Path;
use serde::Serialize;

use super::{VaultError, path::vault_path};

const ALLOWED_EXTS: &[&str] = &[
    "md", "markdown", "mdown", "mkd", "mdx",
    "html", "htm",
    "txt", "log", "csv", "tsv", "env",
    "jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "heic", "heif", "avif",
];

#[derive(Debug, Clone, Serialize)]
pub struct ListEntry {
    pub name: String,
    pub kind: String,       // "file" | "dir"
    pub size: Option<u64>,
    pub mtime: Option<u64>, // epoch ms
    pub ext: Option<String>,
}

fn is_whitelisted_file(name: &str) -> bool {
    if let Some(idx) = name.rfind('.') {
        let ext = name[idx + 1..].to_ascii_lowercase();
        ALLOWED_EXTS.contains(&ext.as_str())
    } else {
        false
    }
}

fn is_hidden(name: &str) -> bool {
    name.starts_with('.') || name == ".DS_Store"
}

pub fn list(root: &Path, rel_path: &str) -> Result<Vec<ListEntry>, VaultError> {
    if rel_path.contains("..") || rel_path.starts_with('/') {
        return Err(VaultError::FsError(format!("invalid rel_path: {rel_path}")));
    }

    let target = if rel_path.is_empty() { root.to_path_buf() } else { root.join(rel_path) };
    if !target.starts_with(root) {
        return Err(VaultError::FsError("path traversal".into()));
    }
    if !target.is_dir() {
        return Err(VaultError::FsError(format!("not a directory: {}", target.display())));
    }

    let mut out = Vec::new();
    for entry in std::fs::read_dir(&target)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_hidden(&name) { continue; }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        let kind = if metadata.is_dir() { "dir" } else { "file" };
        if kind == "file" && !is_whitelisted_file(&name) { continue; }

        let size = if metadata.is_file() { Some(metadata.len()) } else { None };
        let mtime = metadata.modified().ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64);
        let ext = name.rfind('.').map(|i| name[i + 1..].to_ascii_lowercase());

        out.push(ListEntry {
            name,
            kind: kind.into(),
            size,
            mtime,
            ext,
        });
    }

    out.sort_by(|a, b| match (a.kind.as_str(), b.kind.as_str()) {
        ("dir", "file") => std::cmp::Ordering::Less,
        ("file", "dir") => std::cmp::Ordering::Greater,
        _ => a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()),
    });

    Ok(out)
}

#[tauri::command]
pub fn vault_list_dir(app: tauri::AppHandle, rel_path: String) -> Result<Vec<ListEntry>, String> {
    let root = vault_path(&app).map_err(|e| e.to_string())?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    list(&root, &rel_path).map_err(|e| e.to_string())
}
