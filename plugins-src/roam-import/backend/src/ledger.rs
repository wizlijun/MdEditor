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
//!
//! **But degrading silently is not harmless**, which is why `load` returns a
//! [`Loaded`] rather than a bare `Ledger`. "No ledger yet" and "a ledger is
//! there and unreadable" are opposite facts: the first honestly means *this is
//! a first run, start from yesterday*; the second means **we do not know how
//! far back this vault is behind**, and quietly starting from yesterday there
//! silently abandons every page edited earlier — with no future run ever
//! looking that far back again, because the watermark only moves forward. The
//! caller must be able to tell the run apart from a clean one, so the fact
//! travels with the data instead of being thrown away here.
//!
//! `save` is atomic for the same reason [`crate::sync`]'s note writer is: this
//! is a plugin process the host kills on deactivate or quit, and a plain
//! truncating write leaves a half-written ledger behind if it lands in that
//! window. The whole no-page-is-ever-skipped guarantee rests on this one small
//! file, and it is now written once per synced page rather than once per run.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
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

/// A ledger read from disk, and whether reading it was clean.
///
/// `problem` is `None` both for a healthy ledger and for a vault that has
/// never been synced — those two are genuinely the same situation. It is
/// `Some` only when a ledger **is** there and could not be understood: then
/// `ledger` is empty for safety, and the message says so out loud rather than
/// letting the run look pristine. See this module's header.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub ledger: Ledger,
    pub problem: Option<String>,
}

impl Ledger {
    /// Load the ledger from disk. An empty ledger stands in for a file that is
    /// missing, unreadable or not valid JSON — never an error, because a
    /// hand-mangled JSON file must not be able to stop the user syncing. What
    /// went wrong (if anything) comes back in [`Loaded::problem`]; callers are
    /// expected to surface it, not drop it.
    pub fn load(vault: &Path) -> Loaded {
        let clean = |ledger| Loaded { ledger, problem: None };
        let broken = |problem: String| Loaded { ledger: Ledger::default(), problem: Some(problem) };

        let path = vault.join(LEDGER_REL);
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            // The one benign failure: no ledger yet, i.e. a first sync.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return clean(Ledger::default()),
            Err(e) => return broken(format!("cannot read {LEDGER_REL}: {e}")),
        };
        match serde_json::from_str(&content) {
            Ok(ledger) => clean(ledger),
            Err(e) => broken(format!(
                "{LEDGER_REL} is unreadable ({e}) — this vault's sync history is not \
                 available, so pages edited before the fallback start time will not be \
                 fetched. Restore it from git, or run once with an explicit start date."
            )),
        }
    }

    /// Save the ledger to disk, creating `.notemd/` if needed.
    ///
    /// Atomic, exactly like the note writer (`sync::write_atomically`, whose
    /// doc comment carries the full reasoning): a uniquely-named temporary in
    /// the same directory, flushed to disk, then renamed into place — so a
    /// process killed mid-save leaves either the whole old ledger or the whole
    /// new one, never a truncated file. `std::fs::write` truncates first, and
    /// a truncated ledger reads back as "no sync history", which silently
    /// abandons every page edited before the fallback start time.
    ///
    /// No lost-update check here, unlike the note writer: the ledger is this
    /// plugin's own bookkeeping (a note is the user's own text, and losing an
    /// edit to it is unrecoverable), and design §0 deliberately does not lock
    /// against a second concurrent sync — the worst case is one run's claims
    /// losing to another's, which the next run repairs.
    pub fn save(&self, vault: &Path) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        let notemd_dir = vault.join(".notemd");
        std::fs::create_dir_all(&notemd_dir)
            .map_err(|e| format!("failed to create .notemd directory: {}", e))?;

        let path = vault.join(LEDGER_REL);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize ledger: {}", e))?;
        let fail = |e: std::io::Error| format!("failed to write ledger: {e}");

        // 0666 and let the kernel apply the umask, matching what
        // `std::fs::write` would have produced for this file; a fresh
        // `NamedTempFile` is 0600.
        let mut tmp = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o666))
            .tempfile_in(&notemd_dir)
            .map_err(fail)?;
        tmp.write_all(format!("{json}\n").as_bytes())
            .and_then(|()| tmp.as_file().sync_all())
            .map_err(fail)?;
        if let Ok(meta) = std::fs::metadata(&path) {
            let mode = meta.permissions().mode() & 0o7777;
            std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(mode))
                .map_err(fail)?;
        }
        tmp.persist(&path).map_err(|e| fail(e.error))?;

        Ok(())
    }

    /// Look up the vault file path for a given Roam page UID.
    pub fn path_of(&self, uid: &str) -> Option<&str> {
        self.pages.get(uid).map(|r| r.path.as_str())
    }

    /// The Roam title this uid carried at the end of the last sync — the only
    /// record of what Roam used to call the page. Comparing it against the
    /// title Roam hands us now is how [`crate::incremental`] tells a genuine
    /// Roam rename from a file that merely moved for some other reason (the
    /// host's folder setting changed, say), which is not a licence to rewrite
    /// the user's `title:`.
    pub fn title_of(&self, uid: &str) -> Option<&str> {
        self.pages.get(uid).map(|r| r.title.as_str())
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
        assert!(l.ledger.last_synced_at.is_none());
        assert!(l.ledger.pages.is_empty());
        assert_eq!(l.problem, None, "a first sync is not a problem to report");
    }

    #[test]
    fn a_corrupt_file_loads_as_an_empty_ledger_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(dir.path().join(LEDGER_REL), "{ this is not json").unwrap();
        let l = Ledger::load(dir.path());
        assert!(l.ledger.pages.is_empty(), "a hand-mangled ledger must degrade to a full rescan, not a crash");
    }

    /// The difference the whole no-skip guarantee hangs on. A truncated ledger
    /// (killed mid-save) or one carrying `<<<<<<<` from a git merge across two
    /// devices must NOT be indistinguishable from a vault that has never been
    /// synced: that would start from the fallback time and silently abandon
    /// everything edited before it, forever.
    #[test]
    fn a_ledger_that_is_there_but_unreadable_is_not_the_same_as_no_ledger() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        for mangled in [
            "{ this is not json",
            "",
            "<<<<<<< HEAD\n{\"lastSyncedAt\":\"2026-08-01T00:00:00.000Z\"}\n=======",
            r#"{"lastSyncedAt":"2026-08-01T00:00:00.000Z""#, // truncated mid-write
        ] {
            std::fs::write(dir.path().join(LEDGER_REL), mangled).unwrap();
            let l = Ledger::load(dir.path());
            let problem = l.problem.unwrap_or_else(|| panic!("{mangled:?} passed as a clean ledger"));
            assert!(problem.contains("unreadable"), "{problem}");
        }
    }

    #[test]
    fn a_partial_file_keeps_what_it_has() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(dir.path().join(LEDGER_REL), r#"{"lastSyncedAt":"2026-08-01T00:00:00.000Z"}"#).unwrap();
        let l = Ledger::load(dir.path());
        assert_eq!(l.ledger.last_synced_at.as_deref(), Some("2026-08-01T00:00:00.000Z"));
        assert!(l.ledger.pages.is_empty());
        assert_eq!(l.problem, None, "missing optional fields are not a corrupt file");
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
        assert_eq!(back.problem, None);
        assert_eq!(back.ledger.last_synced_at, l.last_synced_at);
        assert_eq!(back.ledger.path_of("8IFJWtnad"), Some("wikipage/回顾系统.note.md"));
        assert_eq!(back.ledger.uid_at("wikipage/回顾系统.note.md"), Some("8IFJWtnad"));
    }

    /// The save that cannot finish must leave the previous ledger whole. It is
    /// written once per synced page now, so the window a truncating write
    /// leaves open is not a rare one — and a truncated ledger reads back as
    /// "never synced", which is the one state that loses pages silently.
    #[test]
    fn a_save_that_cannot_finish_leaves_the_previous_ledger_intact() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mut before = Ledger {
            last_synced_at: Some("2026-08-01T00:00:00.000Z".into()),
            ..Default::default()
        };
        before.claim("u", "wikipage/名.note.md", "名");
        before.save(dir.path()).unwrap();
        let bytes = std::fs::read_to_string(dir.path().join(LEDGER_REL)).unwrap();

        let folder = dir.path().join(".notemd");
        let lock = |mode| std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode));
        lock(0o500).unwrap();
        if std::fs::File::create(folder.join(".probe")).is_ok() {
            // Running as root, where permissions do not bind. Nothing to assert.
            let _ = std::fs::remove_file(folder.join(".probe"));
            lock(0o700).unwrap();
            return;
        }

        let mut next = before.clone();
        next.last_synced_at = Some("2026-08-02T00:00:00.000Z".into());
        assert!(next.save(dir.path()).is_err());
        lock(0o700).unwrap();

        assert_eq!(std::fs::read_to_string(dir.path().join(LEDGER_REL)).unwrap(), bytes,
                   "not one byte of the old ledger may be lost to a failed save");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "and no temporary left behind");
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
