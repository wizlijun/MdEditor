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
use crate::outline::{frontmatter_value, parse_outline, serialize_outline, touch_frontmatter};
use crate::roam_page::RoamPage;
use chrono::NaiveDate;
use serde::Serialize;
use std::io::Write;
use std::path::Path;

/// What the host calls its daily folder when it has not said otherwise —
/// `outlineDirs.dailynote` on the TS side.
const DEFAULT_DAILY_DIR: &str = "dailynote";

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

/// A vault-relative folder and nothing else: not empty, not absolute, and
/// with no `..` segment. `Path::join` *replaces* the base path when handed an
/// absolute component, so an unchecked value here does not merely escape the
/// daily folder — it escapes the vault.
fn is_safe_rel_dir(dir: &str) -> bool {
    !dir.is_empty()
        && !Path::new(dir).is_absolute()
        && !dir.starts_with('/')
        && dir.split('/').all(|seg| seg != "..")
}

/// Write `text` to `path` without ever leaving it half-written, and without
/// ever publishing over an edit this sync did not see.
///
/// A temporary sibling first, flushed to disk, then renamed into place — a
/// same-directory rename is atomic on macOS and Linux, so any reader sees
/// either the whole old file or the whole new one. `std::fs::write` truncates
/// first, and this is a plugin process the host can kill on deactivate or
/// quit; the file it truncates holds the only copy of the user's local-only
/// blocks, which — unlike Roam's half — cannot be fetched again.
///
/// The temporary's name is **unique**, not merely dot-prefixed. The window's
/// sync button and a cron `notemd roam-day` run in *different processes*, and
/// under a deterministic name writer B's `File::create` truncates the
/// temporary writer A has already filled — after which A's rename publishes
/// B's partial bytes. `NamedTempFile` also removes the temporary on every
/// failure path, and that unconditional cleanup — not the leading dot — is
/// what keeps a stray file out of the vault: git tracks dotfiles like any
/// other file, so a leftover `.tmp` would be committed and synced, not
/// ignored.
///
/// `expected` is the file's content as this sync read it. It is re-read as
/// late as possible — a couple of syscalls before the rename — and a
/// difference aborts: the host may have the same note open in its outline pane
/// with its own in-memory tree, and this is a read-modify-write on that file.
/// The repo's own `.note.md` write path hash-checks before writing for exactly
/// this reason.
fn write_atomically(path: &Path, text: &str, expected: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let parent = path
        .parent()
        .ok_or_else(|| format!("cannot write {}: not a file path", path.display()))?;
    let fail = |e: std::io::Error| format!("cannot write {}: {e}", path.display());

    // A fresh `NamedTempFile` is 0600, which is not what a note should be born
    // as: every other file the host writes goes through `std::fs::write`, i.e.
    // `0o666 & !umask`. Ask for 0666 and let the *kernel* apply the umask
    // (tempfile documents `permissions & !umask` for files) — a hardcoded
    // fallback mode cannot: 0644 hands a world-readable note to someone whose
    // umask is 077, and 0600 hands an unreadable-by-anything-else one to
    // everybody else.
    let mut tmp = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o666))
        .tempfile_in(parent)
        .map_err(fail)?;
    tmp.write_all(text.as_bytes()).and_then(|()| tmp.as_file().sync_all()).map_err(fail)?;
    // A note that already exists keeps exactly the mode it had — the user (or
    // another tool) may have chmod'd it, and a rename must not quietly reset
    // that. Only an existing file has a mode to preserve; for a new one the
    // umask above is the whole answer.
    if let Ok(meta) = std::fs::metadata(path) {
        let mode = meta.permissions().mode() & 0o7777;
        std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(mode)).map_err(fail)?;
    }

    if read_existing(path)? != expected {
        // `tmp` is dropped here, which deletes it.
        return Err(format!(
            "{} changed while this sync was reading it — nothing was written. \
             Close the day's note (or let the other sync finish) and run it again.",
            path.display()
        ));
    }
    tmp.persist(path).map_err(|e| fail(e.error))?;
    Ok(())
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
    // Both halves of the path are checked here, at the function that writes,
    // rather than trusted to have been validated upstream: `date` reaches
    // callers from a CLI flag or a UI field, and `daily_dir` from the host
    // (and, from Task 10, possibly not from the host at all).
    if !crate::dates::is_iso_date(date) {
        return Err(format!("invalid date '{date}': expected yyyy-MM-dd"));
    }
    if !is_safe_rel_dir(daily_dir) {
        return Err(format!(
            "invalid daily folder '{daily_dir}': expected a relative path inside the vault"
        ));
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
    let existing = read_existing(&abs)?;
    let local = parse_outline(&existing);
    let roam = convert_page(page, date);
    let (mut merged, stats) = merge(&local, &roam);
    // Filled in before the early returns below rather than at each of them:
    // what the merge decided is the same story whether or not anything reached
    // the disk.
    outcome.created = stats.created;
    outcome.updated = stats.updated;
    outcome.kept_local = stats.kept_local;
    outcome.roam_gone_kept = stats.roam_gone_kept;

    // The page's own creation time is the day's `created:`; a page Roam
    // reports without one falls back to now rather than inventing a date.
    // Either way `touch_frontmatter` only uses it when the file has no
    // `created:` yet, so an existing note keeps the value it was born with.
    let created = iso_ms(page.create_time).unwrap_or_else(|| now.to_string());
    // `merge` passes the local front-matter through untouched (rule 8), so
    // this is the file's own block — the base both serializations start from.
    let base_fm = merged.frontmatter.clone();

    // A no-op sync must not touch the file AT ALL. `updated:` is refreshed
    // from a live clock, so serializing with `now` would rewrite the note on
    // every cron run: dirtying it for vaultgitsync, re-triggering the host's
    // file watcher, and re-opening the concurrency window above for nothing.
    // So serialize once with the `updated:` the file already carries, and if
    // that reproduces the file byte-for-byte, there is nothing to say.
    if let Some(prev) = frontmatter_value(base_fm.as_deref(), "updated") {
        merged.frontmatter = Some(touch_frontmatter(base_fm.as_deref(), date, &created, &prev));
        if serialize_outline(&merged) == existing {
            return Ok(outcome);
        }
    }
    merged.frontmatter = Some(touch_frontmatter(base_fm.as_deref(), date, &created, now));
    let text = serialize_outline(&merged);
    // The same "say nothing when there is nothing to say" rule once more, for a
    // note whose front-matter carries no `updated:` for the branch above to
    // reuse. A block `touch_frontmatter` deliberately leaves alone (one that is
    // not a YAML mapping — see its doc comment) never grows one, so without
    // this it would be rewritten on every single run.
    if text == existing {
        return Ok(outcome);
    }

    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    write_atomically(&abs, &text, &existing)?;

    Ok(outcome)
}

/// The whole operation as the UI (and Task 10's CLI) asks for it: resolve the
/// day, ask Roam for that day's page, merge it into the vault.
///
/// The two impure edges are injected — `today`, because "yesterday" is a
/// question about the user's local calendar, and `fetch_page`, which in
/// production discovers and runs the `roam` CLI. That keeps the orchestration
/// itself (no vault, no daily folder, date → Roam's `MM-DD-YYYY` page uid, no
/// page that day) testable without a clock or a subprocess; `plugin.rs` is
/// left as a thin adapter that supplies the real ones. Same shape as
/// `discover::discover_with` and `plugin::resolve_vault_info`.
pub fn sync_requested_day<Fetch>(
    vault: Option<&Path>,
    daily_dir: &str,
    date_input: Option<&str>,
    today: NaiveDate,
    now: &str,
    fetch_page: Fetch,
) -> Result<SyncOutcome, String>
where
    Fetch: FnOnce(&str) -> Result<Option<RoamPage>, String>,
{
    // First, and before anything is fetched: without a vault there is nowhere
    // to put the answer, and writing a day's notes somewhere else would be
    // worse than asking the user to try again.
    let vault = vault.ok_or("no vault configured")?;
    let daily_dir = if daily_dir.is_empty() { DEFAULT_DAILY_DIR } else { daily_dir };

    let date = crate::dates::resolve_date(date_input, today)?;
    let uid = crate::dates::to_roam_uid(&date)
        .ok_or_else(|| format!("invalid date '{date}': expected yyyy-MM-dd"))?;

    let page = fetch_page(&uid)?;
    sync_day(vault, daily_dir, page.as_ref(), &date, now)
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

    fn a_day(today: (i32, u32, u32)) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(today.0, today.1, today.2).unwrap()
    }

    #[test]
    fn path_is_daily_dir_year_date() {
        assert_eq!(daily_rel_path("dailynote", "2026-08-02"), "dailynote/2026/2026-08-02.note.md");
    }

    /// The daily folder comes from the host's vault info today, but Task 10
    /// adds a caller that need not go through it. It is joined into a path
    /// exactly like `date` is — and `Path::join` *replaces* the base when the
    /// component is absolute, so an unchecked value does not merely escape
    /// the daily folder, it escapes the vault.
    #[test]
    fn a_daily_dir_that_could_escape_the_vault_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["", "/etc", "../elsewhere", "daily/../..", ".."] {
            let err = sync_day(dir.path(), bad, Some(&page()), "2026-08-02", NOW)
                .unwrap_err();
            assert!(err.contains("invalid daily folder"), "{bad:?} was accepted: {err}");
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "nothing was written");
    }

    /// The file being written holds the only copy of the user's local-only
    /// blocks — Roam's half can always be fetched again, their margin notes
    /// cannot. So the write must never truncate the old file until the new
    /// one is complete on disk, and a write that cannot finish must not
    /// litter the vault with the half-written temporary either.
    #[test]
    fn a_write_that_cannot_finish_leaves_the_old_note_and_no_debris() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("dailynote/2026");
        let note = folder.join("2026-08-02.note.md");
        std::fs::create_dir_all(&folder).unwrap();
        let before = "---\ntitle: 2026-08-02\n---\n- the only copy of this\n";
        std::fs::write(&note, before).unwrap();

        // An unwritable *folder*: the note itself can still be opened for
        // writing (file mode, not folder mode, governs that — so a truncating
        // write would still destroy it), but no new file can be created
        // beside it. That is the difference this test is here to hold.
        let lock = |mode| std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(mode));
        lock(0o500).unwrap();
        if std::fs::File::create(folder.join(".probe")).is_ok() {
            // Running as root, where permissions do not bind. Nothing to assert.
            let _ = std::fs::remove_file(folder.join(".probe"));
            lock(0o700).unwrap();
            return;
        }

        let err = sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap_err();
        lock(0o700).unwrap();

        assert!(err.contains("cannot write"), "unexpected error: {err}");
        assert_eq!(
            std::fs::read_to_string(&note).unwrap(),
            before,
            "the user's only copy must survive a write that could not finish"
        );
        let left: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["2026-08-02.note.md".to_string()], "debris left behind: {left:?}");
    }

    #[test]
    fn a_completed_write_leaves_no_temporary_behind() {
        let dir = tempfile::tempdir().unwrap();
        sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap();
        let left: Vec<String> = std::fs::read_dir(dir.path().join("dailynote/2026"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["2026-08-02.note.md".to_string()]);
    }

    // `sync_requested_day` is the whole operation as the UI asks for it. It
    // lives here, not in plugin.rs, precisely so these paths are testable:
    // the bin crate cannot be reached from a test.

    #[test]
    fn without_a_vault_nothing_is_attempted() {
        let err = sync_requested_day(None, "dailynote", None, a_day((2026, 8, 3)), NOW, |_| {
            panic!("Roam must not be contacted before the vault is known")
        })
        .unwrap_err();
        assert_eq!(err, "no vault configured");
    }

    /// The host answers with its configured daily folder; an empty string
    /// means it has not told us one, and the default has to match the host's
    /// own (`outlineDirs.dailynote`).
    #[test]
    fn an_empty_daily_dir_falls_back_to_the_hosts_default() {
        let dir = tempfile::tempdir().unwrap();
        let out = sync_requested_day(
            Some(dir.path()), "", Some("2026-08-02"), a_day((2026, 8, 3)), NOW,
            |_| Ok(Some(page())),
        )
        .unwrap();
        assert_eq!(out.path, "dailynote/2026/2026-08-02.note.md");
        assert!(dir.path().join(&out.path).exists());
    }

    /// The date the user asked for (or, by default, yesterday) has to reach
    /// Roam as *its* daily-page uid, `MM-DD-YYYY` — asking for the wrong uid
    /// silently syncs the wrong day.
    #[test]
    fn roam_is_asked_for_the_resolved_dates_page_uid() {
        let dir = tempfile::tempdir().unwrap();
        let asked = std::cell::RefCell::new(String::new());
        let out = sync_requested_day(
            Some(dir.path()), "dailynote", None, a_day((2026, 8, 3)), NOW,
            |uid| { asked.borrow_mut().push_str(uid); Ok(Some(page())) },
        )
        .unwrap();
        assert_eq!(asked.into_inner(), "08-02-2026", "default is yesterday, as MM-DD-YYYY");
        assert_eq!(out.date, "2026-08-02");
    }

    #[test]
    fn a_day_roam_has_no_page_for_is_reported_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let out = sync_requested_day(
            Some(dir.path()), "dailynote", Some("2026-08-02"), a_day((2026, 8, 3)), NOW,
            |_| Ok(None),
        )
        .unwrap();
        assert!(!out.found);
        assert!(!dir.path().join(&out.path).exists());
    }

    #[test]
    fn a_fetch_failure_is_reported_and_touches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let err = sync_requested_day(
            Some(dir.path()), "dailynote", Some("2026-08-02"), a_day((2026, 8, 3)), NOW,
            |_| Err("roam: not authorized".to_string()),
        )
        .unwrap_err();
        assert_eq!(err, "roam: not authorized");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_date_that_is_not_a_date_never_reaches_roam() {
        let err = sync_requested_day(
            None::<&std::path::Path>, "dailynote", Some("last tuesday"),
            a_day((2026, 8, 3)), NOW, |_| panic!("must not be reached"),
        )
        .unwrap_err();
        // The vault check comes first, so use a configured vault to reach the date check.
        assert_eq!(err, "no vault configured");

        let dir = tempfile::tempdir().unwrap();
        let err = sync_requested_day(
            Some(dir.path()), "dailynote", Some("last tuesday"),
            a_day((2026, 8, 3)), NOW, |_| panic!("Roam must not be asked for a nonsense date"),
        )
        .unwrap_err();
        assert!(err.contains("expected yyyy-MM-dd"), "unexpected error: {err}");
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

    /// R6, at the file level. The plugin does not have to have *produced* a
    /// note to be handed one: it writes into whatever daily note is already
    /// there, and a `.note.md` is hand-edited and agent-edited. A front-matter
    /// block that is not a YAML mapping must come through the sync verbatim —
    /// appending `title:`/`created:`/`updated:` under the sentence leaves
    /// behind a block the host's own `yaml`-backed reader then refuses as a
    /// map, i.e. this sync corrupting front-matter it did not write.
    #[test]
    fn a_hand_written_non_mapping_frontmatter_is_not_corrupted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dailynote/2026/2026-08-02.note.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "---\njust a sentence\n---\n- what I actually thought\n").unwrap();

        let out = sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("---\njust a sentence\n---\n"),
            "the front-matter was rewritten:\n{text}"
        );
        assert!(text.contains("- from roam"), "Roam's block still lands:\n{text}");
        assert!(text.contains("- what I actually thought"), "the user's block survives:\n{text}");
        assert_eq!((out.created, out.kept_local), (1, 1));

        // …and, having no `updated:` for the fast path to reuse, a second sync
        // must still be a no-op rather than rewriting the note on every run.
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let again =
            sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", "2026-08-04T17:45:12.345Z")
                .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "the bytes moved");
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            mtime,
            "the file was rewritten even though nothing changed"
        );
        assert_eq!((again.created, again.updated), (0, 0));
    }

    /// Regression, C2/C3. `parse_outline` reads three shapes as structure and
    /// `convert::block_content` escapes all three; when it escaped only the
    /// `key:: value` one, a Roam block containing a shift-enter list or an
    /// unterminated fence was re-read as a *different tree* than the one just
    /// written — the block lost the `id::` that is its identity, merge saw new
    /// Roam material, and three syncs against an unchanged Roam page produced
    /// three copies of the user's note (verified: 1 → 2 → 3 copies).
    ///
    /// Three runs, not two: the first sync writes, the second is where the
    /// misparse first bites, and the third proves it compounds rather than
    /// settling.
    #[test]
    fn a_block_that_looks_like_outline_structure_does_not_multiply() {
        fn stable_across_three_syncs(label: &str, children: Vec<RoamBlock>) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("dailynote/2026/2026-08-02.note.md");
            let mut p = page();
            p.children = children;
            sync_day(dir.path(), "dailynote", Some(&p), "2026-08-02", NOW).unwrap();
            let first = std::fs::read_to_string(&path).unwrap();
            for run in 2..=3 {
                let out = sync_day(dir.path(), "dailynote", Some(&p), "2026-08-02", NOW).unwrap();
                let text = std::fs::read_to_string(&path).unwrap();
                assert_eq!(text, first, "{label}: sync {run} rewrote the file:\n{text}");
                assert_eq!(
                    (out.created, out.updated, out.kept_local, out.roam_gone_kept),
                    (0, 0, 0, 0),
                    "{label}: sync {run} thought something had changed"
                );
            }
        }

        let b = |uid: &str, s: &str| RoamBlock {
            uid: Some(uid.into()), string: s.into(), order: 0, heading: None,
            create_time: None, edit_time: None, children: vec![],
        };
        stable_across_three_syncs("shift-enter list", vec![b("u1", "shopping\n- milk\n- eggs")]);
        stable_across_three_syncs(
            "unterminated fence",
            vec![b("u1", "```js\nconst x = 1"), b("u2", "the block after the fence")],
        );
        stable_across_three_syncs("property line", vec![b("u1", "meeting\nid:: not-a-property")]);
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

    /// The idempotence tests above all freeze `now`, so they prove the MERGE
    /// is idempotent — not the file. In production `now` is a live clock, so a
    /// serialization that always refreshes `updated:` rewrites the note on
    /// every cron run: it dirties the file for vaultgitsync, re-triggers the
    /// host's watcher, and re-opens the concurrency window for nothing. A
    /// second sync with a *different* clock must not touch the file at all —
    /// asserted on the mtime, because equal bytes alone would not prove that
    /// no write happened.
    #[test]
    fn a_second_sync_with_a_different_clock_does_not_touch_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dailynote/2026/2026-08-02.note.md");
        sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        // Coarse filesystem timestamps would hide a rewrite that lands in the
        // same tick; make sure a real write could be seen.
        std::thread::sleep(std::time::Duration::from_millis(20));

        let out = sync_day(
            dir.path(), "dailynote", Some(&page()), "2026-08-02",
            "2026-08-04T17:45:12.345Z", // a whole day later
        )
        .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), first, "the bytes moved");
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            mtime,
            "the file was rewritten even though nothing changed"
        );
        assert!(first.contains(&format!("updated: {NOW}")), "the first sync's updated: is kept");
        assert_eq!((out.created, out.updated), (0, 0));
    }

    /// …and a sync that DOES have something to say still refreshes `updated:`.
    /// (The skip above must not turn into "never touch the front-matter".)
    #[test]
    fn a_sync_that_changes_something_refreshes_updated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dailynote/2026/2026-08-02.note.md");
        sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap();

        let mut edited = page();
        edited.children[0].string = "from roam, edited".into();
        let later = "2026-08-04T17:45:12.345Z";
        let out = sync_day(dir.path(), "dailynote", Some(&edited), "2026-08-02", later).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(out.updated, 1);
        assert!(text.contains("- from roam, edited"));
        assert!(text.contains(&format!("updated: {later}")), "front-matter not refreshed:\n{text}");
    }

    /// A read-modify-write on a file the host may have open in its outline
    /// pane with its own in-memory tree. If it changed under us between the
    /// read and the rename, publishing would silently destroy whatever landed
    /// there — abort with something the user can act on instead.
    #[test]
    fn a_file_that_changed_since_it_was_read_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "what the host just wrote\n").unwrap();

        let err = write_atomically(&path, "our merge\n", "what this sync read\n").unwrap_err();
        assert!(err.contains("changed while this sync was reading it"), "unexpected: {err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "what the host just wrote\n");
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["note.md".to_string()], "temporary left behind: {left:?}");
    }

    #[test]
    fn a_file_that_did_not_change_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "before\n").unwrap();
        write_atomically(&path, "after\n", "before\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "after\n");
        // A file that does not exist yet reads as "" — the first-sync case.
        let fresh = dir.path().join("fresh.md");
        write_atomically(&fresh, "new\n", "").unwrap();
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), "new\n");
    }

    /// `NamedTempFile` creates at 0600. A note must not come out of a sync
    /// readable differently from every other file in the vault, and one the
    /// user (or another tool) already chmod'd must keep the mode it had.
    ///
    /// "Like every other file in the vault" is asserted against an actual
    /// `std::fs::write` — the call the host itself writes notes with — rather
    /// than against a hardcoded 0644, because the answer depends on the
    /// process umask: 0644 under the usual 022, but 0600 under 077, where a
    /// literal 0644 would publish the user's day to every account on the
    /// machine. Comparing to the reference file makes the assertion true under
    /// any umask, including the reviewer's.
    #[test]
    fn the_notes_permissions_survive_the_temp_file_rename() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");

        let reference = dir.path().join("reference.md");
        std::fs::write(&reference, "what the host would have written\n").unwrap();
        let expected = std::fs::metadata(&reference).unwrap().permissions().mode() & 0o777;

        write_atomically(&path, "new\n", "").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, expected,
            "a note this sync created is 0{mode:o}, not the 0{expected:o} the umask asks for"
        );

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_atomically(&path, "newer\n", "new\n").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600,
            "a mode the user set was reset by the rename"
        );
    }
}
