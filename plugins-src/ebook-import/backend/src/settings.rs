use serde::{Deserialize, Serialize};
use std::path::Path;

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
