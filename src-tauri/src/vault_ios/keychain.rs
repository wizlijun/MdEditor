use super::VaultError;

/// Read PAT from secure storage.
///
/// On iOS: invokes the Swift Keychain plugin (`plugin:keychain|get`).
/// On other targets (cargo test on macOS): reads from a JSON file under
/// `$MDEDITOR_KEYCHAIN_STUB_DIR`, so unit tests work without real Keychain.
pub fn get_pat() -> Result<String, VaultError> {
    #[cfg(target_os = "ios")]
    { return ios::get("pat")?.ok_or(VaultError::NotConfigured); }

    #[cfg(not(target_os = "ios"))]
    { return stub::get("pat")?.ok_or(VaultError::NotConfigured); }
}

#[cfg(target_os = "ios")]
pub mod ios {
    use super::VaultError;

    pub fn set(_account: &str, _value: &str) -> Result<(), VaultError> {
        Err(VaultError::FsError("keychain set must go via plugin:keychain|set from JS".into()))
    }

    pub fn get(_account: &str) -> Result<Option<String>, VaultError> {
        Err(VaultError::NotConfigured)
    }

    pub fn delete(_account: &str) -> Result<(), VaultError> {
        Err(VaultError::FsError("keychain delete must go via plugin:keychain|delete from JS".into()))
    }
}

#[cfg(not(target_os = "ios"))]
pub mod stub {
    use super::VaultError;
    use std::path::PathBuf;

    fn dir() -> PathBuf {
        std::env::var("MDEDITOR_KEYCHAIN_STUB_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("notemd-keychain-stub"))
    }

    fn path(account: &str) -> PathBuf {
        dir().join(format!("{account}.txt"))
    }

    pub fn set(account: &str, value: &str) -> Result<(), VaultError> {
        std::fs::create_dir_all(dir())?;
        std::fs::write(path(account), value)?;
        Ok(())
    }

    pub fn get(account: &str) -> Result<Option<String>, VaultError> {
        let p = path(account);
        if !p.exists() { return Ok(None); }
        Ok(Some(std::fs::read_to_string(p)?))
    }

    pub fn delete(account: &str) -> Result<(), VaultError> {
        let p = path(account);
        if p.exists() { std::fs::remove_file(p)?; }
        Ok(())
    }
}
