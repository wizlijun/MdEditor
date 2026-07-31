use serde::{Deserialize, Serialize};
use std::path::{Component, Path};

fn default_ebooks_root() -> String {
    "ssot/ebooks".to_string()
}
fn default_provider() -> String {
    "wechat".to_string()
}
fn default_wechat_url() -> String {
    "http://10.17.0.123:8092/ocr".to_string()
}

/// Lives at `<vault_root>/.notemd/ebook-import.json`. Every field carries
/// `#[serde(default)]` because external agents (or humans) may hand-edit
/// this file; a partially-specified or entirely malformed file must still
/// load usable settings rather than fail the plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VaultSettings {
    #[serde(default = "default_ebooks_root")]
    pub ebooks_root: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_wechat_url")]
    pub wechat_url: String,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            ebooks_root: default_ebooks_root(),
            provider: default_provider(),
            wechat_url: default_wechat_url(),
        }
    }
}

/// Lives at `<data_dir>/device.json`, outside the vault (and thus outside
/// git sync) because it holds machine-local paths and OCR provider
/// credentials that must never be committed alongside vault content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DeviceSettings {
    #[serde(default)]
    pub calibre_path: Option<String>,
    #[serde(default)]
    pub baidu_api_key: String,
    #[serde(default)]
    pub baidu_secret_key: String,
}

fn vault_settings_path(vault_root: &Path) -> std::path::PathBuf {
    vault_root.join(".notemd").join("ebook-import.json")
}

fn device_settings_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join("device.json")
}

fn load_json_tolerant<T: Default + for<'de> Deserialize<'de>>(path: &Path) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn load_vault(vault_root: &Path) -> VaultSettings {
    load_json_tolerant(&vault_settings_path(vault_root))
}

pub fn save_vault(vault_root: &Path, settings: &VaultSettings) -> std::io::Result<()> {
    let path = vault_settings_path(vault_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(settings)?)
}

/// Rejects an `ebooks_root` that could escape the vault. `pipeline.rs`'s
/// `run_import` joins it straight onto the vault root
/// (`vault_root.join(ebooks_root)`); `Path::join` replaces the base
/// entirely when the joined path is absolute, and a leading `..` component
/// climbs out of it -- either way the book would land outside the vault
/// instead of archived inside it. An empty string is also rejected: joined
/// as-is it resolves to the vault root itself, silently dumping every
/// import loose into the vault root rather than under a books subfolder.
///
/// Called both where `save_settings` merges a fresh value in (`plugin.rs`'s
/// `apply_vault_patch`, so a bad value from the UI or an external agent is
/// rejected before it's ever persisted) and again by `run_import` itself
/// (defense in depth against a bad value that reached
/// `.notemd/ebook-import.json` some other way, e.g. hand-edited).
pub fn validate_ebooks_root(root: &str) -> Result<(), String> {
    if root.is_empty() {
        return Err("ebooks_root must be a vault-relative path".to_string());
    }
    let path = Path::new(root);
    if path.is_absolute() {
        return Err("ebooks_root must be a vault-relative path".to_string());
    }
    if path.components().any(|c| c == Component::ParentDir) {
        return Err("ebooks_root must be a vault-relative path".to_string());
    }
    Ok(())
}

pub fn load_device(data_dir: &Path) -> DeviceSettings {
    load_json_tolerant(&device_settings_path(data_dir))
}

pub fn save_device(data_dir: &Path, settings: &DeviceSettings) -> std::io::Result<()> {
    let path = device_settings_path(data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(settings)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_defaults_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_vault(tmp.path());
        assert_eq!(s.ebooks_root, "ssot/ebooks");
        assert_eq!(s.provider, "wechat");
        assert_eq!(s.wechat_url, "http://10.17.0.123:8092/ocr");
    }

    #[test]
    fn vault_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let s = VaultSettings {
            ebooks_root: "custom/ebooks".into(),
            provider: "baidu".into(),
            wechat_url: "http://example.com/ocr".into(),
        };
        save_vault(tmp.path(), &s).unwrap();
        let loaded = load_vault(tmp.path());
        assert_eq!(loaded.ebooks_root, "custom/ebooks");
        assert_eq!(loaded.provider, "baidu");
        assert_eq!(loaded.wechat_url, "http://example.com/ocr");
    }

    #[test]
    fn vault_bad_json_falls_back_to_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".notemd");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ebook-import.json"), "{ not json").unwrap();
        let s = load_vault(tmp.path());
        assert_eq!(s.ebooks_root, "ssot/ebooks");
        assert_eq!(s.provider, "wechat");
        assert_eq!(s.wechat_url, "http://10.17.0.123:8092/ocr");
    }

    #[test]
    fn vault_partial_json_uses_defaults_for_missing_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".notemd");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ebook-import.json"), r#"{"provider":"baidu"}"#).unwrap();
        let s = load_vault(tmp.path());
        assert_eq!(s.ebooks_root, "ssot/ebooks");
        assert_eq!(s.provider, "baidu");
        assert_eq!(s.wechat_url, "http://10.17.0.123:8092/ocr");
    }

    #[test]
    fn device_defaults_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let s = load_device(tmp.path());
        assert_eq!(s.calibre_path, None);
        assert_eq!(s.baidu_api_key, "");
        assert_eq!(s.baidu_secret_key, "");
    }

    #[test]
    fn device_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let s = DeviceSettings {
            calibre_path: Some("/usr/bin/ebook-convert".into()),
            baidu_api_key: "key".into(),
            baidu_secret_key: "secret".into(),
        };
        save_device(tmp.path(), &s).unwrap();
        let loaded = load_device(tmp.path());
        assert_eq!(loaded.calibre_path, Some("/usr/bin/ebook-convert".into()));
        assert_eq!(loaded.baidu_api_key, "key");
        assert_eq!(loaded.baidu_secret_key, "secret");
    }

    // ── validate_ebooks_root (Finding 4: keep imports inside the vault) ────

    #[test]
    fn validate_ebooks_root_rejects_absolute_paths() {
        assert!(validate_ebooks_root("/etc").is_err());
        assert!(validate_ebooks_root("/ssot/ebooks").is_err());
    }

    #[test]
    fn validate_ebooks_root_rejects_parent_dir_components() {
        assert!(validate_ebooks_root("../x").is_err());
        assert!(validate_ebooks_root("a/../..").is_err());
    }

    #[test]
    fn validate_ebooks_root_rejects_empty() {
        assert!(validate_ebooks_root("").is_err());
    }

    #[test]
    fn validate_ebooks_root_accepts_vault_relative_paths() {
        assert!(validate_ebooks_root("ssot/ebooks").is_ok());
        assert!(validate_ebooks_root("books/sub").is_ok());
    }

    #[test]
    fn device_bad_json_falls_back_to_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("device.json"), "not json at all").unwrap();
        let s = load_device(tmp.path());
        assert_eq!(s.calibre_path, None);
        assert_eq!(s.baidu_api_key, "");
        assert_eq!(s.baidu_secret_key, "");
    }
}
