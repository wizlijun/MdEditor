use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SharedConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sotvault: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rawvault: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibre_path: Option<String>,
}

fn default_version() -> u32 { 1 }

pub fn config_path() -> std::io::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not found")
    })?;
    Ok(home.join("Library/Application Support/com.laobu.mdeditor-shared/config.json"))
}

pub fn read(path: &Path) -> std::io::Result<SharedConfig> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(serde_json::from_str(&s).unwrap_or_else(|_| SharedConfig {
            version: 1, ..Default::default()
        })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SharedConfig {
            version: 1, ..Default::default()
        }),
        Err(e) => Err(e),
    }
}

pub fn write(path: &Path, cfg: &SharedConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Migrate the legacy `vault_sync.repo_path` value from a JSON store file into the
/// shared config's `sotvault` field. Idempotent: a non-empty `sotvault` short-circuits.
///
/// `legacy_store_path` points to the Tauri Store JSON (typically
/// `~/Library/Application Support/net.notemd.app/settings.json`).
pub fn migrate_vault_sync_repo_to_shared(
    shared_path: &Path,
    legacy_store_path: &Path,
) -> std::io::Result<bool> {
    let mut cfg = read(shared_path)?;
    if cfg.sotvault.as_ref().is_some_and(|s| !s.is_empty()) {
        return Ok(false);
    }
    let legacy_raw = match std::fs::read_to_string(legacy_store_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let v: serde_json::Value = match serde_json::from_str(&legacy_raw) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };
    let repo = v.pointer("/vault_sync.repo_path")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    if let Some(repo) = repo {
        if !repo.is_empty() {
            cfg.sotvault = Some(repo);
            write(shared_path, &cfg)?;
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn read_missing_returns_default() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("config.json");
        let cfg = read(&p).unwrap();
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.sotvault, None);
    }

    #[test]
    fn write_then_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("config.json");
        let cfg = SharedConfig {
            version: 1,
            sotvault: Some("/tmp/sot".into()),
            rawvault: Some("/tmp/raw".into()),
            calibre_path: Some("/Applications/calibre.app/Contents/MacOS".into()),
        };
        write(&p, &cfg).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back, cfg);
    }

    #[test]
    fn write_uses_atomic_tmp_rename() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("config.json");
        let cfg = SharedConfig::default();
        write(&p, &cfg).unwrap();
        assert!(p.exists());
        assert!(!p.with_extension("json.tmp").exists());
    }

    #[test]
    fn corrupted_file_falls_back_to_default() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("config.json");
        std::fs::write(&p, "{ not valid json").unwrap();
        let cfg = read(&p).unwrap();
        assert_eq!(cfg.version, 1);
    }

    #[test]
    fn migration_copies_gitsync_repo_when_shared_empty() {
        let tmp = TempDir::new().unwrap();
        let shared = tmp.path().join("shared.json");
        let legacy = tmp.path().join("legacy.json");
        std::fs::write(&legacy, r#"{"vault_sync.repo_path":"/Users/me/notes"}"#).unwrap();

        let migrated = migrate_vault_sync_repo_to_shared(&shared, &legacy).unwrap();
        assert!(migrated);

        let cfg = read(&shared).unwrap();
        assert_eq!(cfg.sotvault.as_deref(), Some("/Users/me/notes"));
    }

    #[test]
    fn migration_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let shared = tmp.path().join("shared.json");
        let legacy = tmp.path().join("legacy.json");
        std::fs::write(&legacy, r#"{"vault_sync.repo_path":"/Users/me/notes"}"#).unwrap();

        let first = migrate_vault_sync_repo_to_shared(&shared, &legacy).unwrap();
        let second = migrate_vault_sync_repo_to_shared(&shared, &legacy).unwrap();
        assert!(first);
        assert!(!second);
    }

    #[test]
    fn migration_noop_when_legacy_missing() {
        let tmp = TempDir::new().unwrap();
        let shared = tmp.path().join("shared.json");
        let legacy = tmp.path().join("legacy.json");
        let migrated = migrate_vault_sync_repo_to_shared(&shared, &legacy).unwrap();
        assert!(!migrated);
    }

    #[test]
    fn migration_noop_when_shared_already_has_sotvault() {
        let tmp = TempDir::new().unwrap();
        let shared = tmp.path().join("shared.json");
        let legacy = tmp.path().join("legacy.json");
        write(&shared, &SharedConfig {
            version: 1,
            sotvault: Some("/preset".into()),
            ..Default::default()
        }).unwrap();
        std::fs::write(&legacy, r#"{"vault_sync.repo_path":"/Users/me/notes"}"#).unwrap();

        let migrated = migrate_vault_sync_repo_to_shared(&shared, &legacy).unwrap();
        assert!(!migrated);

        let cfg = read(&shared).unwrap();
        assert_eq!(cfg.sotvault.as_deref(), Some("/preset"));
    }
}
