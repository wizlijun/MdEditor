use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_MEETINGS_ROOT: &str = "ssot/meetings";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct VaultSettings {
    #[serde(default = "default_meetings_root")]
    pub meetings_root: String,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            meetings_root: default_meetings_root(),
        }
    }
}

fn default_meetings_root() -> String {
    DEFAULT_MEETINGS_ROOT.to_string()
}

fn settings_path(vault_root: &Path) -> PathBuf {
    vault_root.join(".notemd").join("meetings.json")
}

pub fn load(vault_root: &Path) -> VaultSettings {
    std::fs::read_to_string(settings_path(vault_root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(|settings: &VaultSettings| validate_meetings_root(&settings.meetings_root).is_ok())
        .unwrap_or_default()
}

pub fn save(vault_root: &Path, settings: &VaultSettings) -> Result<(), String> {
    validate_meetings_root(&settings.meetings_root)?;
    let path = settings_path(vault_root);
    let parent = path.parent().expect("settings path has parent");
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create meetings settings directory: {error}"))?;
    let text = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("serialize meetings settings: {error}"))?;
    std::fs::write(&path, format!("{text}\n"))
        .map_err(|error| format!("write {}: {error}", path.display()))
}

pub fn validate_meetings_root(value: &str) -> Result<(), String> {
    let path = Path::new(value);
    let normalized = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if value.is_empty()
        || value != value.trim()
        || normalized != value
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("meetings_root must be a non-empty Vault-relative directory".into());
    }
    if !path
        .components()
        .any(|component| matches!(component, Component::Normal(_)))
    {
        return Err("meetings_root must be a non-empty Vault-relative directory".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_partial_and_bad_settings_fall_back_safely() {
        let vault = tempfile::tempdir().unwrap();
        assert_eq!(load(vault.path()).meetings_root, DEFAULT_MEETINGS_ROOT);
        std::fs::create_dir_all(vault.path().join(".notemd")).unwrap();
        std::fs::write(vault.path().join(".notemd/meetings.json"), "{}").unwrap();
        assert_eq!(load(vault.path()).meetings_root, DEFAULT_MEETINGS_ROOT);
        std::fs::write(vault.path().join(".notemd/meetings.json"), "{bad").unwrap();
        assert_eq!(load(vault.path()).meetings_root, DEFAULT_MEETINGS_ROOT);
        std::fs::write(
            vault.path().join(".notemd/meetings.json"),
            r#"{"meetings_root":"../../outside"}"#,
        )
        .unwrap();
        assert_eq!(load(vault.path()).meetings_root, DEFAULT_MEETINGS_ROOT);
    }

    #[test]
    fn settings_round_trip() {
        let vault = tempfile::tempdir().unwrap();
        let settings = VaultSettings {
            meetings_root: "archive/transcripts".into(),
        };
        save(vault.path(), &settings).unwrap();
        assert_eq!(load(vault.path()), settings);
    }

    #[test]
    fn root_validation_rejects_escape_and_accepts_nested_relative_paths() {
        for invalid in [
            "",
            "/tmp/meetings",
            "../meetings",
            "ssot/../meetings",
            ".",
            "ssot/./meetings",
            " ssot/meetings ",
        ] {
            assert!(
                validate_meetings_root(invalid).is_err(),
                "accepted {invalid}"
            );
        }
        for valid in ["ssot/meetings", "meetings", "archive/transcripts"] {
            assert!(validate_meetings_root(valid).is_ok(), "rejected {valid}");
        }
    }
}
