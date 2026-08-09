use std::path::{Path, PathBuf};

/// Append "Vault" subdir to the given base. Pure function for testability.
pub fn resolve_vault_path(base: &Path) -> PathBuf {
    base.join("Vault")
}

/// Production helper: read iOS document directory from the app handle.
///
/// `test` is in the gate for the same reason `macos` is: `vault_ios` is
/// `#![cfg(any(target_os = "ios", test))]` so its logic stays unit-testable off
/// device, and the module's callers need this function to exist in that build.
/// `macos` alone covered a Mac dev box; on a Windows one the whole test crate
/// failed to compile. The body is platform-neutral Tauri API.
#[cfg(any(target_os = "ios", target_os = "macos", test))]
pub fn vault_path(app: &tauri::AppHandle) -> Result<PathBuf, super::VaultError> {
    use tauri::Manager;
    let doc = app.path().document_dir()
        .map_err(|e| super::VaultError::FsError(format!("document_dir: {e}")))?;
    Ok(resolve_vault_path(&doc))
}
