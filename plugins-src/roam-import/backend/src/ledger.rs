//! The incremental sync ledger: persistent memory of which Roam pages we've
//! already synced and where they landed in the vault.
//!
//! `Ledger::load` gracefully degrades to an empty ledger on any failure
//! (missing file, corrupt JSON, missing fields). The reasoning: a vault file can
//! be hand-edited, partially written, or mangled by a git merge conflict. The
//! consequence of an empty ledger is only that the next sync rescans more than
//! it needed to, which is harmless because syncing is idempotent. The
//! consequence of returning an error would be a plugin that refuses to sync
//! until the user hand-repairs a JSON file — unacceptable.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const LEDGER_REL: &str = ".notemd/roam-sync.json";

/// A single Roam page's record in the ledger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageRecord {
    pub path: String,
    pub title: String,
}

/// The incremental sync ledger, tracking which Roam pages we've synced and
/// where they landed in the vault.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Ledger {
    /// The Roam graph name (e.g. "bruce").
    #[serde(default)]
    pub graph: Option<String>,
    /// Last sync timestamp (ISO 8601).
    #[serde(default, rename = "lastSyncedAt")]
    pub last_synced_at: Option<String>,
    /// Map from Roam page UID to its vault file path and title.
    #[serde(default)]
    pub pages: BTreeMap<String, PageRecord>,
}

impl Ledger {
    /// Load the ledger from disk, or return an empty ledger if the file is
    /// missing, unreadable, or contains invalid JSON. Never returns an error.
    pub fn load(vault: &Path) -> Ledger {
        let path = vault.join(LEDGER_REL);
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Ledger::default();
        };
        serde_json::from_str(&content).unwrap_or_default()
    }

    /// Save the ledger to disk, creating `.notemd/` if needed.
    pub fn save(&self, vault: &Path) -> Result<(), String> {
        let notemd_dir = vault.join(".notemd");
        std::fs::create_dir_all(&notemd_dir)
            .map_err(|e| format!("failed to create .notemd directory: {}", e))?;

        let path = vault.join(LEDGER_REL);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize ledger: {}", e))?;
        std::fs::write(&path, format!("{}\n", json))
            .map_err(|e| format!("failed to write ledger: {}", e))?;

        Ok(())
    }

    /// Look up the vault file path for a given Roam page UID.
    pub fn path_of(&self, uid: &str) -> Option<&str> {
        self.pages.get(uid).map(|r| r.path.as_str())
    }

    /// Look up the Roam page UID for a given vault file path.
    pub fn uid_at(&self, path: &str) -> Option<&str> {
        self.pages
            .iter()
            .find(|(_, record)| record.path == path)
            .map(|(uid, _)| uid.as_str())
    }

    /// Record that a Roam page UID has been synced to a vault file. If the UID
    /// was already recorded at a different path, that old record is removed.
    pub fn claim(&mut self, uid: &str, path: &str, title: &str) {
        // Remove any existing record pointing to this UID
        self.pages.remove(uid);
        // Insert the new record
        self.pages.insert(
            uid.to_string(),
            PageRecord {
                path: path.to_string(),
                title: title.to_string(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_loads_as_an_empty_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let l = Ledger::load(dir.path());
        assert!(l.last_synced_at.is_none());
        assert!(l.pages.is_empty());
    }

    #[test]
    fn a_corrupt_file_loads_as_an_empty_ledger_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(dir.path().join(LEDGER_REL), "{ this is not json").unwrap();
        let l = Ledger::load(dir.path());
        assert!(l.pages.is_empty(), "a hand-mangled ledger must degrade to a full rescan, not a crash");
    }

    #[test]
    fn a_partial_file_keeps_what_it_has() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(dir.path().join(LEDGER_REL), r#"{"lastSyncedAt":"2026-08-01T00:00:00.000Z"}"#).unwrap();
        let l = Ledger::load(dir.path());
        assert_eq!(l.last_synced_at.as_deref(), Some("2026-08-01T00:00:00.000Z"));
        assert!(l.pages.is_empty());
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = Ledger::default();
        l.graph = Some("bruce".into());
        l.last_synced_at = Some("2026-08-03T11:58:41.185Z".into());
        l.claim("8IFJWtnad", "wikipage/回顾系统.note.md", "回顾系统");
        l.save(dir.path()).unwrap();
        let back = Ledger::load(dir.path());
        assert_eq!(back.last_synced_at, l.last_synced_at);
        assert_eq!(back.path_of("8IFJWtnad"), Some("wikipage/回顾系统.note.md"));
        assert_eq!(back.uid_at("wikipage/回顾系统.note.md"), Some("8IFJWtnad"));
    }

    #[test]
    fn claiming_an_existing_uid_replaces_its_record() {
        let mut l = Ledger::default();
        l.claim("u", "wikipage/旧名.note.md", "旧名");
        l.claim("u", "wikipage/新名.note.md", "新名");
        assert_eq!(l.path_of("u"), Some("wikipage/新名.note.md"));
        assert_eq!(l.uid_at("wikipage/旧名.note.md"), None);
        assert_eq!(l.pages.len(), 1);
    }

    #[test]
    fn save_creates_the_notemd_folder() {
        let dir = tempfile::tempdir().unwrap();
        Ledger::default().save(dir.path()).unwrap();
        assert!(dir.path().join(LEDGER_REL).exists());
    }
}
