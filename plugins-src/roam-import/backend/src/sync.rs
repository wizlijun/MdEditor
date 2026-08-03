//! Orchestration: the one place in this crate that writes to the user's vault.
//!
//! Everything upstream of here is pure (fetch → parse → convert → merge), so
//! this module stays deliberately thin — read the day's file if it exists,
//! merge, write it back — and takes the already-fetched `RoamPage` as an
//! argument rather than shelling out to `roam` itself. That is what lets the
//! whole write path be exercised against a `tempfile::tempdir()` with no CLI,
//! no network and no clock: `now` is a parameter too.
//!
//! Three rules this module owns, because they are about the *file*, not the
//! outline: a day Roam has no page for is not written at all (never create or
//! touch a note for a day Roam knows nothing about); the front-matter is
//! refreshed here, since `merge` deliberately passes it through untouched and
//! only the caller knows the date and the clock; and the date is validated
//! before it becomes a path.
use crate::convert::{convert_page, iso_ms};
use crate::merge::merge;
use crate::outline::{parse_outline, serialize_outline, touch_frontmatter};
use crate::roam_page::RoamPage;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct SyncOutcome {
    pub date: String,
    /// Vault-relative, always filled in — even when nothing was written, so
    /// the UI can say *which* note a "no page that day" answer refers to.
    pub path: String,
    pub created: usize,
    pub updated: usize,
    pub kept_local: usize,
    pub roam_gone_kept: usize,
    /// Roam had a daily page for this date. `false` means the file was left
    /// exactly as it was (or never created).
    pub found: bool,
}

/// `<daily_dir>/<yyyy>/<date>.note.md` — the host's own daily-note layout
/// (`dailyNotePath` in `src/lib/outline/daily.ts`). The year comes from the
/// date string itself rather than a re-parse: callers pass a `yyyy-MM-dd`
/// already validated by `dates::resolve_date`.
pub fn daily_rel_path(daily_dir: &str, date: &str) -> String {
    let year = date.split('-').next().unwrap_or(date);
    format!("{daily_dir}/{year}/{date}.note.md")
}

/// Read the day's `.note.md` if it exists. A missing file is the normal
/// first-sync case and reads as an empty outline; any other IO error is
/// surfaced, because silently treating "permission denied" as "empty" would
/// merge into nothing and then overwrite the user's file with Roam alone.
fn read_existing(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// Sync one day of Roam into the vault. `page` is `None` when Roam has no
/// daily page for `date`; `now` is an ISO-8601 instant (injected, not read
/// from the clock, so the write path is testable).
pub fn sync_day(
    vault: &Path,
    daily_dir: &str,
    page: Option<&RoamPage>,
    date: &str,
    now: &str,
) -> Result<SyncOutcome, String> {
    // `date` is joined into a path below. Callers reach this through
    // `dates::resolve_date`, which already refuses anything else — but this
    // is the function that writes, and a `..` slipping through would put the
    // write outside the daily folder (indeed outside the vault) entirely.
    if !crate::dates::is_iso_date(date) {
        return Err(format!("invalid date '{date}': expected yyyy-MM-dd"));
    }
    let rel = daily_rel_path(daily_dir, date);
    let mut outcome = SyncOutcome {
        date: date.to_string(),
        path: rel.clone(),
        created: 0,
        updated: 0,
        kept_local: 0,
        roam_gone_kept: 0,
        found: false,
    };
    let Some(page) = page else { return Ok(outcome) };
    outcome.found = true;

    let abs = vault.join(&rel);
    let local = parse_outline(&read_existing(&abs)?);
    let roam = convert_page(page, date);
    let (mut merged, stats) = merge(&local, &roam);

    // The page's own creation time is the day's `created:`; a page Roam
    // reports without one falls back to now rather than inventing a date.
    // Either way `touch_frontmatter` only uses it when the file has no
    // `created:` yet, so an existing note keeps the value it was born with.
    let created = iso_ms(page.create_time).unwrap_or_else(|| now.to_string());
    merged.frontmatter =
        Some(touch_frontmatter(merged.frontmatter.as_deref(), date, &created, now));

    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(&abs, serialize_outline(&merged))
        .map_err(|e| format!("cannot write {}: {e}", abs.display()))?;

    outcome.created = stats.created;
    outcome.updated = stats.updated;
    outcome.kept_local = stats.kept_local;
    outcome.roam_gone_kept = stats.roam_gone_kept;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roam_page::{RoamBlock, RoamPage};

    const NOW: &str = "2026-08-03T09:00:00.000Z";

    fn page() -> RoamPage {
        RoamPage {
            title: "August 2nd, 2026".into(),
            uid: Some("08-02-2026".into()),
            create_time: Some(1785600005019),
            edit_time: None,
            children: vec![RoamBlock {
                uid: Some("u1".into()),
                string: "from roam".into(),
                order: 0,
                heading: None,
                create_time: None,
                edit_time: None,
                children: vec![],
            }],
        }
    }

    #[test]
    fn path_is_daily_dir_year_date() {
        assert_eq!(daily_rel_path("dailynote", "2026-08-02"), "dailynote/2026/2026-08-02.note.md");
    }

    #[test]
    fn writes_a_new_note_when_none_exists() {
        let dir = tempfile::tempdir().unwrap();
        let out = sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap();
        assert!(out.found);
        assert_eq!(out.created, 1);
        let text = std::fs::read_to_string(dir.path().join(&out.path)).unwrap();
        assert!(text.contains("- from roam"));
        assert!(text.contains("id:: u1"));
        assert!(text.contains("title: 2026-08-02"));
    }

    /// `date` arrives from a CLI flag or a UI field and is joined straight
    /// into a vault path. Anything that is not a plain calendar date is
    /// refused *here*, at the only function in the plugin that writes to the
    /// vault, rather than trusted to have been validated upstream.
    #[test]
    fn a_date_that_is_not_a_plain_calendar_date_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let err = sync_day(dir.path(), "dailynote", Some(&page()), "../../etc/passwd", NOW)
            .unwrap_err();
        assert!(err.contains("expected yyyy-MM-dd"), "unexpected error: {err}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "nothing was written");
    }

    #[test]
    fn no_roam_page_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let out = sync_day(dir.path(), "dailynote", None, "2026-08-02", NOW).unwrap();
        assert!(!out.found);
        assert!(!dir.path().join(&out.path).exists());
    }

    /// Roam still has the day's page but the user emptied it (or the pull
    /// came back thin). The merge preserves local blocks, but this is the
    /// layer that decides what actually lands on disk, so the guarantee is
    /// pinned here too: an empty Roam page never blanks the user's file.
    #[test]
    fn an_empty_roam_page_does_not_erase_what_the_user_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dailynote/2026/2026-08-02.note.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "---\ntitle: 2026-08-02\n---\n- what I actually thought\n").unwrap();

        let mut empty = page();
        empty.children.clear();
        let out = sync_day(dir.path(), "dailynote", Some(&empty), "2026-08-02", NOW).unwrap();

        assert_eq!((out.created, out.kept_local), (0, 1));
        assert!(std::fs::read_to_string(&path).unwrap().contains("- what I actually thought"));
    }

    #[test]
    fn a_second_sync_leaves_the_file_byte_identical() {
        let dir = tempfile::tempdir().unwrap();
        sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap();
        let first =
            std::fs::read_to_string(dir.path().join("dailynote/2026/2026-08-02.note.md")).unwrap();
        sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap();
        let second =
            std::fs::read_to_string(dir.path().join("dailynote/2026/2026-08-02.note.md")).unwrap();
        assert_eq!(first, second);
    }
}
