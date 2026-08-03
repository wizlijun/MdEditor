//! Incremental sync: one run pulls every Roam page that changed since the
//! watermark, routes each to its place in the vault, and leaves a watermark the
//! next run can resume from. The pieces below it are all pure or single-purpose
//! (`changed` discovers, `route` decides where, `sync::sync_page` writes); this
//! module is where the order, the failure policy and the watermark live.
//!
//! Both impure edges are injected — `discover` (two datalog queries) and
//! `fetch` (one recursive pull per page), plus `today` and `now` — the same
//! shape as `sync::sync_requested_day` and `discover::discover_with`. That is
//! what lets the whole orchestration, watermark rule included, be exercised
//! against a `tempfile::tempdir()` with no clock, no network and no subprocess.
//!
//! # The watermark advances a whole timestamp at a time, not a page at a time
//!
//! The obvious rule — sort ascending, and after each page succeeds save its own
//! `edited` as the new watermark — **silently loses pages**, because `edited` is
//! not unique. Two different uids routinely carry the same millisecond: a bulk
//! edit, a scripted import, or plain coincidence between the two discovery
//! dimensions. Say pages P and Q both have `edited == T`. P succeeds and the
//! watermark becomes `T`; Q fails and the run stops. The next run asks Roam for
//! everything **strictly** after `T` (`[(> ?t ?since)]` — server-side, and it
//! must stay strict or every run would re-sync its own last page forever), so Q
//! — which was never synced — is not in the answer. Q is skipped permanently
//! and silently. Sorting deterministically does not help: the flaw is advancing
//! to a value that does not uniquely identify a position in the list.
//!
//! So pages sharing an `edited` are one **atomic group**: the watermark may
//! only move past `T` once *every* page with `edited == T` in this batch has
//! been dealt with. Equivalently — the persisted watermark is the greatest
//! `edited` strictly below the smallest `edited` among the failures, and the
//! greatest `edited` in the batch when there are none. Implemented by only
//! advancing at a group boundary (the next page's `edited` is strictly
//! greater), which is the same thing computed as we go. The cost of a failure
//! is that the rest of that timestamp's group is fetched again next run —
//! idempotent, because an unchanged page is not even written (`PageOutcome`'s
//! `wrote`).
//!
//! Sorting is by `(edited, uid)`, not `edited` alone, so a run's order — and
//! therefore which pages a mid-batch failure leaves for next time — is
//! reproducible rather than dependent on hash iteration order upstream.
//!
//! The watermark also never moves *backwards*: `--since` is a manual backfill
//! ("go re-read everything after July 1st"), and letting it rewind the ledger
//! would make the following incremental run rescan a month.
use crate::changed::Changed;
use crate::convert::iso_ms;
use crate::ledger::Ledger;
use crate::roam_page::RoamPage;
use crate::route::route_page;
use crate::sync::{is_safe_rel_dir, sync_page, TitlePolicy};
use chrono::{Duration, Local, NaiveDate, TimeZone, Utc};
use serde::Serialize;
use std::path::Path;

/// One page whose Roam title changed since the last sync, and the file move
/// that followed. Reported per page because a rename is the one thing this
/// sync does that the user did not ask for file-by-file — `[[wikilink]]`s
/// elsewhere in the vault still point at the old name (§0: no full-graph
/// relink), so they need to be told which names moved.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Renamed {
    pub uid: String,
    pub from: String,
    pub to: String,
}

/// What one incremental run did. Shared by the window and the CLI's `--json`.
#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    /// The watermark this run queried from, ISO-8601.
    pub from: Option<String>,
    /// The watermark on disk when this run finished, ISO-8601: the one it
    /// persisted, or — when it advanced nothing — the one that was already
    /// there. `None` only when there is none to report: no ledger, a recorded
    /// value that is not a timestamp, a save that failed, or a dry run (which
    /// persists nothing at all, so it has nothing to report here).
    pub to: Option<String>,
    /// Pages discovered as changed. It may exceed `synced + skipped + failed`:
    /// a failure stops the run, and the pages after it are left for next time.
    pub scanned: usize,
    /// Pages this run changed on disk — written, moved, or both.
    pub synced: usize,
    /// Pages that needed no change: gone from Roam, blockless (a bare tag page
    /// — Roam creates one for every `#tag`, and an empty file is not a note),
    /// or already byte-for-byte what Roam holds.
    pub skipped: usize,
    /// Pages that failed. At most 1 — the run stops at the first one.
    pub failed: usize,
    pub renamed: Vec<Renamed>,
    /// Human-readable, one per problem. Not always the same count as `failed`:
    /// an unreadable ledger, and a rename this sync refused to make because a
    /// file was already at the destination, are both reported here without any
    /// page having failed. A run with `failed == 0` and a non-empty `errors` is
    /// not a clean run and must not be shown as one.
    pub errors: Vec<String>,
    pub dry_run: bool,
}

/// Where a first run starts when the ledger has no watermark: local midnight
/// at the start of yesterday. `today` is the caller's local calendar date
/// (injected — "yesterday" is a question about the user's calendar, not the
/// process's clock), and the offset comes from `chrono::Local`.
///
/// Yesterday, not today, so an early-morning first run still picks up what was
/// written last night; not "the beginning of time", because a first run must
/// not drag the entire graph in (§8 acceptance 1) — a full import is the other
/// feature's job.
pub fn default_since(today: NaiveDate) -> i64 {
    local_midnight_ms(today - Duration::days(1))
}

/// The instant a calendar day begins **where the user is**, in epoch
/// milliseconds. Every other day boundary in this system is local — the daily
/// note's own calendar, `dates::resolve_date`'s today/yesterday — and a
/// watermark is compared against Roam's `:edit/time`, which records when the
/// user typed. Reading a day as UTC instead would, east of Greenwich, start
/// the scan hours into that morning and silently drop the edits before it:
/// the under-scanning direction, i.e. exactly the failure this feature exists
/// to prevent.
fn local_midnight_ms(day: NaiveDate) -> i64 {
    let midnight = day.and_hms_opt(0, 0, 0).expect("00:00:00 is a valid time of day");
    Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|t| t.timestamp_millis())
        // A local midnight that a DST jump skipped over does not exist. Fall
        // back to the earliest instant that midnight could carry anywhere on
        // earth (UTC+14): scanning too far back is idempotent, scanning from
        // too late silently skips edits.
        .unwrap_or_else(|| Utc.from_utc_datetime(&midnight).timestamp_millis() - 14 * 3_600_000)
}

/// A `--since yyyy-MM-dd` backfill point → epoch milliseconds, read as **local**
/// midnight: the user typing `--since 2026-07-01` means their own July 1st, the
/// same day boundary [`default_since`] uses. Strict about the format for the
/// same reason `sync_day` is: this value is user input, and a silently-misread
/// date backfills the wrong month.
fn since_from_override(raw: &str) -> Result<i64, String> {
    let raw = raw.trim();
    if !crate::dates::is_iso_date(raw) {
        return Err(format!("invalid --since '{raw}': expected yyyy-MM-dd"));
    }
    let day = NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .map_err(|_| format!("invalid --since '{raw}': expected yyyy-MM-dd"))?;
    Ok(local_midnight_ms(day))
}

/// The ledger's ISO watermark → epoch milliseconds, or `None` if it is not a
/// timestamp at all (hand-edited, or mangled by a git merge conflict — the
/// ledger is a vault file that travels through git).
fn watermark_ms(iso: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(iso).ok().map(|t| t.timestamp_millis())
}

/// Move the watermark to `edited`, unless that would move it backwards.
/// Called only at a group boundary — see this module's header for why that
/// matters more than anything else here.
fn advance_watermark(ledger: &mut Ledger, floor_ms: Option<i64>, edited: i64) {
    if matches!(floor_ms, Some(floor) if edited <= floor) {
        return;
    }
    // A timestamp chrono cannot represent (garbage out of the graph) leaves the
    // watermark where it was: re-reading a page is free, losing one is not.
    if let Some(iso) = iso_ms(Some(edited)) {
        ledger.last_synced_at = Some(iso);
    }
}

/// Route, move and write one page. Returns whether anything on disk changed.
/// The ledger is claimed here (in memory; the caller persists it) so that the
/// *next* page in the same run routes against where this one actually landed —
/// that is what keeps two same-titled pages in one batch from fighting over
/// the same file name.
#[allow(clippy::too_many_arguments)]
fn sync_one(
    vault: &Path,
    dirs: (&str, &str),
    uid: &str,
    page: &RoamPage,
    now: &str,
    dry_run: bool,
    ledger: &mut Ledger,
    renamed: &mut Vec<Renamed>,
    errors: &mut Vec<String>,
) -> Result<bool, String> {
    let target = route_page(uid, &page.title, dirs, ledger);
    let mut moved = false;

    if let Some(from_rel) = target.rename_from.clone() {
        let from_abs = vault.join(&from_rel);
        let to_abs = vault.join(&target.rel);
        // Three shapes, and only the first is a move. The old file may simply
        // be gone (deleted or moved by hand) — then this is a fresh write, not
        // a rename. And a file already sitting at the destination is NOT ours
        // to overwrite: `fs::rename` would unlink it, and it may hold blocks
        // the user wrote. Leaving it alone costs an orphan at the old path,
        // which is recoverable; clobbering it is not.
        if !from_abs.exists() {
            // Nothing to move. `sync_page` will write the new path from scratch.
        } else if to_abs.exists() {
            // Refusing to move is right; doing it silently is not. The user now
            // has two files for one page: the destination, which this sync is
            // about to merge Roam's half into, and an orphan at the old path
            // holding local-only blocks — the half that cannot be fetched
            // again. Design §8 acceptance 3 promises those blocks travel with
            // the rename; here they do not, so the run has to say so.
            errors.push(format!(
                "{uid}: Roam renamed this page to '{}', but {} already exists — {from_rel} was \
                 left where it is rather than overwriting it. Anything you wrote in {from_rel} \
                 is still there; move it into {} by hand.",
                page.title, target.rel, target.rel
            ));
        } else {
            if !dry_run {
                if let Some(parent) = to_abs.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
                }
                std::fs::rename(&from_abs, &to_abs)
                    .map_err(|e| format!("cannot move {from_rel} to {}: {e}", target.rel))?;
            }
            moved = true;
            renamed.push(Renamed {
                uid: uid.to_string(),
                from: from_rel,
                to: target.rel.clone(),
            });
            // Claimed the moment the file moves, before the write that may
            // still fail: the ledger must never point at a path the file is no
            // longer at.
            ledger.claim(uid, &target.rel, &page.title);
        }
    }

    if dry_run {
        ledger.claim(uid, &target.rel, &page.title);
        return Ok(true);
    }

    // The one case where a `title:` already on disk is not the user's to keep:
    // Roam renamed the page, so the sync *knows* the old one is wrong. Without
    // this the file name, the front-matter and the ledger disagree forever —
    // `touch_frontmatter` only fills a *missing* title, and a post-rename sync
    // usually has nothing else to write, so no later run repairs it. Scoped to
    // exactly this branch: everywhere else, an existing title stays.
    let title_policy = match target.rename_from {
        Some(_) => TitlePolicy::Refresh,
        None => TitlePolicy::KeepExisting,
    };
    let outcome = sync_page(
        vault, &target.rel, Some(page), &target.title, target.concept_type, now, title_policy,
    )?;
    ledger.claim(uid, &target.rel, &page.title);
    Ok(outcome.wrote || moved)
}

/// Pull everything Roam changed since the watermark into the vault, then leave
/// a watermark the next run resumes from.
///
/// `dirs` is `(wiki_dir, daily_dir)`, the host's configured folder names.
/// `since_override` (the CLI's `--since`) beats the ledger, which beats
/// [`default_since`]. `dry_run` reports what a real run would do — the same
/// routing, the same renames, computed against the same in-memory ledger — and
/// writes nothing at all: no note, no move, no ledger, no watermark.
///
/// The failure policy is "stop at the first one, keep what got done": errors
/// here are overwhelmingly connectivity or a page the host has open, both of
/// which will hit the next page too, and marching on would spend a minute
/// failing once per page. Everything already synced is persisted (the ledger is
/// saved after each page, so a killed process loses no progress) and the
/// watermark is left where it is safe to resume from.
#[allow(clippy::too_many_arguments)]
pub fn sync_since<D, F>(
    vault: &Path,
    dirs: (&str, &str),
    since_override: Option<&str>,
    today: NaiveDate,
    now: &str,
    dry_run: bool,
    discover: D,
    mut fetch: F,
) -> Result<SyncReport, String>
where
    D: FnOnce(i64) -> Result<Vec<Changed>, String>,
    F: FnMut(&str) -> Result<Option<RoamPage>, String>,
{
    // Both folder names are host-supplied and both become the first segment of
    // a path this function joins onto the vault root. `Path::join` *replaces*
    // the base when the component is absolute, so an unchecked value does not
    // merely land the note in the wrong folder — it lands it outside the vault.
    // Checked before anything is fetched, let alone written.
    for (label, dir) in [("wiki folder", dirs.0), ("daily folder", dirs.1)] {
        if !is_safe_rel_dir(dir) {
            return Err(format!(
                "invalid {label} '{dir}': expected a relative path inside the vault"
            ));
        }
    }

    let loaded = Ledger::load(vault);
    let mut ledger = loaded.ledger;
    let mut errors: Vec<String> = Vec::new();
    // A ledger that is there and unreadable is not a first sync, and the run
    // must not look clean: `since` is about to fall back to yesterday, which
    // abandons everything edited before that — permanently, because the
    // watermark only ever moves forward. Reported, not fatal, for the same
    // reason `Ledger::load` degrades rather than erroring: refusing to sync
    // until the user hand-repairs JSON is worse than syncing loudly.
    errors.extend(loaded.problem);

    let recorded = ledger.last_synced_at.clone();
    let floor_ms = recorded.as_deref().and_then(watermark_ms);
    if let Some(iso) = &recorded {
        if floor_ms.is_none() {
            // Same story one level down: the file parsed, but its watermark is
            // not a timestamp.
            errors.push(format!(
                "unreadable last sync time '{iso}' in the ledger — this vault's sync history \
                 is not available, so pages edited before the fallback start time will not \
                 be fetched"
            ));
        }
    }

    let since = match since_override {
        Some(raw) => since_from_override(raw)?,
        None => floor_ms.unwrap_or_else(|| default_since(today)),
    };

    let mut changed = discover(since)?;
    changed.sort_by(|a, b| a.edited.cmp(&b.edited).then_with(|| a.uid.cmp(&b.uid)));

    let mut report = SyncReport {
        from: iso_ms(Some(since)),
        to: None,
        scanned: changed.len(),
        synced: 0,
        skipped: 0,
        failed: 0,
        renamed: Vec::new(),
        errors,
        dry_run,
    };
    // What is actually on disk, so `to` never claims a watermark a failed save
    // never persisted — and never echoes back a value that is not a timestamp.
    let mut persisted = recorded.filter(|_| floor_ms.is_some());

    for i in 0..changed.len() {
        let uid = changed[i].uid.clone();
        let edited = changed[i].edited;

        let step = match fetch(&uid) {
            Err(e) => Err(e),
            // No page at that uid (deleted between discovery and now), or a
            // page with no blocks at all: nothing to write, and the page is
            // nonetheless fully dealt with, so the watermark moves past it.
            Ok(None) => Ok(false),
            Ok(Some(page)) if page.children.is_empty() => Ok(false),
            Ok(Some(page)) => sync_one(
                vault,
                dirs,
                &uid,
                &page,
                now,
                dry_run,
                &mut ledger,
                &mut report.renamed,
                &mut report.errors,
            ),
        };

        let failed = step.is_err();
        match step {
            Ok(changed_on_disk) => {
                if changed_on_disk {
                    report.synced += 1;
                } else {
                    report.skipped += 1;
                }
                // The group boundary: only once no later page shares this
                // `edited` may the watermark move past it. See the header.
                let group_done = changed.get(i + 1).map(|next| next.edited > edited).unwrap_or(true);
                if group_done {
                    advance_watermark(&mut ledger, floor_ms, edited);
                }
            }
            Err(e) => {
                report.failed += 1;
                report.errors.push(format!("{uid}: {e}"));
            }
        }

        if !dry_run {
            // Saved after every page — including the failing one, which may
            // have moved a file before it failed — so a process killed
            // mid-batch loses no progress and never leaves the ledger claiming
            // a path the file is not at.
            match ledger.save(vault) {
                Ok(()) => persisted = ledger.last_synced_at.clone(),
                Err(e) => {
                    report.errors.push(format!("cannot record progress: {e}"));
                    break;
                }
            }
        }
        if failed {
            break;
        }
    }

    report.to = if dry_run { None } else { persisted };
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changed::Changed;
    use crate::roam_page::{RoamBlock, RoamPage};

    const NOW: &str = "2026-08-03T09:00:00.000Z";
    fn today() -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 8, 3).unwrap() }
    const DIRS: (&str, &str) = ("wikipage", "dailynote");

    fn page(uid: &str, title: &str, body: &str) -> RoamPage {
        RoamPage {
            title: title.into(), uid: Some(uid.into()),
            create_time: Some(1785600005019), edit_time: None,
            children: vec![RoamBlock {
                uid: Some(format!("{uid}-b1")), string: body.into(), order: 0, heading: None,
                create_time: None, edit_time: None, children: vec![],
            }],
        }
    }

    #[test]
    fn syncs_a_daily_and_a_wiki_page_in_one_run() {
        let dir = tempfile::tempdir().unwrap();
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![
                Changed { uid: "08-02-2026".into(), edited: 1000 },
                Changed { uid: "8IFJWtnad".into(), edited: 2000 },
            ]),
            |uid| Ok(Some(match uid {
                "08-02-2026" => page("08-02-2026", "August 2nd, 2026", "日记内容"),
                _ => page("8IFJWtnad", "回顾系统", "概念内容"),
            })),
        ).unwrap();
        assert_eq!((r.scanned, r.synced, r.failed), (2, 2, 0));
        let daily = std::fs::read_to_string(dir.path().join("dailynote/2026/2026-08-02.note.md")).unwrap();
        assert!(daily.contains("type: Daily Note"));
        let wiki = std::fs::read_to_string(dir.path().join("wikipage/回顾系统.note.md")).unwrap();
        assert!(wiki.contains("type: Wiki Page"));
        let l = crate::ledger::Ledger::load(dir.path()).ledger;
        // The watermark is the later page's own `edited`, and `edited` is epoch
        // milliseconds: 2000 ms after the epoch is 1970-01-01T00:00:02.000Z.
        assert_eq!(l.last_synced_at.as_deref(), Some("1970-01-01T00:00:02.000Z"));
    }

    #[test]
    fn the_watermark_stops_at_the_first_failure_so_nothing_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![
                Changed { uid: "a".into(), edited: 1000 },
                Changed { uid: "b".into(), edited: 2000 },
                Changed { uid: "c".into(), edited: 3000 },
            ]),
            |uid| if uid == "b" { Err("network went away".into()) } else { Ok(Some(page(uid, uid, "x"))) },
        ).unwrap();
        assert_eq!((r.synced, r.failed), (1, 1));
        let l = crate::ledger::Ledger::load(dir.path()).ledger;
        assert_eq!(l.last_synced_at.as_deref(), Some("1970-01-01T00:00:01.000Z"),
                   "the watermark must stay at `a`, so `b` and `c` are retried next run");
    }

    #[test]
    fn a_page_with_no_blocks_is_skipped_and_creates_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![Changed { uid: "tag".into(), edited: 1000 }]),
            |_| Ok(Some(RoamPage { title: "PKM".into(), uid: Some("tag".into()),
                                   create_time: None, edit_time: None, children: vec![] })),
        ).unwrap();
        assert_eq!((r.synced, r.skipped), (0, 1));
        assert!(!dir.path().join("wikipage/PKM.note.md").exists());
    }

    #[test]
    fn a_renamed_page_moves_its_file_and_keeps_the_local_blocks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("wikipage")).unwrap();
        std::fs::write(dir.path().join("wikipage/旧名.note.md"),
            "---\ntype: Wiki Page\ntitle: 旧名\n---\n- from roam\n  id:: u-b1\n- 我自己写的\n").unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.claim("u", "wikipage/旧名.note.md", "旧名");
        l.save(dir.path()).unwrap();

        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![Changed { uid: "u".into(), edited: 1000 }]),
            |_| Ok(Some(page("u", "新名", "from roam"))),
        ).unwrap();

        assert_eq!(r.renamed.len(), 1);
        assert_eq!(r.renamed[0].from, "wikipage/旧名.note.md");
        assert_eq!(r.renamed[0].to, "wikipage/新名.note.md");
        assert!(!dir.path().join("wikipage/旧名.note.md").exists(), "the old file must be moved, not left behind");
        let moved = std::fs::read_to_string(dir.path().join("wikipage/新名.note.md")).unwrap();
        assert!(moved.contains("我自己写的"), "a rename must not lose what the user wrote");
        // I3. The file name says 新名, the ledger says 新名 — and the
        // front-matter has to as well. `touch_frontmatter` only fills a
        // *missing* title, and the sync after a rename usually has nothing
        // else to write (`wrote == false`), so if this one does not fix it,
        // nothing ever does.
        assert!(moved.contains("title: 新名"), "the front-matter title is still stale:\n{moved}");
        assert!(!moved.contains("旧名"), "the old title survived the rename:\n{moved}");
        assert_eq!(crate::ledger::Ledger::load(dir.path()).ledger.path_of("u"),
                   Some("wikipage/新名.note.md"));
    }

    /// The half of I3 that makes it worth doing at all: a rename whose *only*
    /// change is the name. Roam sent byte-identical blocks, so the merge has
    /// nothing to say and the note's body does not move — which is exactly
    /// when the old code left the title stale forever. The file still has to
    /// come out with the new title, and the run has to report it as synced
    /// rather than skipped.
    #[test]
    fn a_rename_with_no_content_change_still_fixes_the_title() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("wikipage")).unwrap();
        // Exactly what an earlier sync of this page would have left behind.
        let before = "---\ntype: Wiki Page\ntitle: 旧名\ncreated: 2026-08-01T16:00:05.019Z\n\
                      updated: 2026-08-02T10:00:00.000Z\n---\n- from roam\n  id:: u-b1\n";
        std::fs::write(dir.path().join("wikipage/旧名.note.md"), before).unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.claim("u", "wikipage/旧名.note.md", "旧名");
        l.save(dir.path()).unwrap();

        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![Changed { uid: "u".into(), edited: 1000 }]),
            |_| Ok(Some(page("u", "新名", "from roam"))),
        ).unwrap();

        assert_eq!(r.synced, 1, "the file moved and its title changed — that is a sync");
        let moved = std::fs::read_to_string(dir.path().join("wikipage/新名.note.md")).unwrap();
        assert!(moved.contains("title: 新名"), "{moved}");
        assert!(moved.contains("- from roam"), "{moved}");
    }

    /// The tension I3 has to respect: `touch_frontmatter` never overwriting a
    /// title is a rule the shared fixture pins, and it stays true for every
    /// sync that is not a rename — including one where the user retitled the
    /// note themselves.
    #[test]
    fn a_sync_that_is_not_a_rename_leaves_a_hand_written_title_alone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("wikipage")).unwrap();
        std::fs::write(dir.path().join("wikipage/回顾系统.note.md"),
            "---\ntype: Wiki Page\ntitle: 我给它起的名字\n---\n- 我自己写的\n").unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.claim("u", "wikipage/回顾系统.note.md", "回顾系统");
        l.save(dir.path()).unwrap();

        sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![Changed { uid: "u".into(), edited: 1000 }]),
            |_| Ok(Some(page("u", "回顾系统", "from roam"))),
        ).unwrap();

        let text = std::fs::read_to_string(dir.path().join("wikipage/回顾系统.note.md")).unwrap();
        assert!(text.contains("title: 我给它起的名字"), "the user's own title was overwritten:\n{text}");
    }

    #[test]
    fn a_dry_run_writes_nothing_and_leaves_the_watermark_alone() {
        let dir = tempfile::tempdir().unwrap();
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, true,
            |_| Ok(vec![Changed { uid: "8IFJWtnad".into(), edited: 1000 }]),
            |_| Ok(Some(page("8IFJWtnad", "回顾系统", "x"))),
        ).unwrap();
        assert!(r.dry_run && r.scanned == 1);
        assert!(!dir.path().join("wikipage/回顾系统.note.md").exists());
        assert!(crate::ledger::Ledger::load(dir.path()).ledger.last_synced_at.is_none());
    }

    #[test]
    fn nothing_changed_means_nothing_written_and_the_watermark_holds() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.last_synced_at = Some("2026-08-01T00:00:00.000Z".into());
        l.save(dir.path()).unwrap();
        let r = sync_since(dir.path(), DIRS, None, today(), NOW, false,
                           |_| Ok(vec![]), |_| panic!("must not fetch anything")).unwrap();
        assert_eq!((r.scanned, r.synced), (0, 0));
        assert_eq!(crate::ledger::Ledger::load(dir.path()).ledger.last_synced_at.as_deref(),
                   Some("2026-08-01T00:00:00.000Z"));
    }

    #[test]
    fn since_override_beats_the_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.last_synced_at = Some("2026-08-01T00:00:00.000Z".into());
        l.save(dir.path()).unwrap();
        let seen = std::cell::Cell::new(0i64);
        sync_since(dir.path(), DIRS, Some("2026-07-01"), today(), NOW, true,
                   |since| { seen.set(since); Ok(vec![]) }, |_| unreachable!()).unwrap();
        // The user's own July 1st, not UTC's. Asserted against `default_since`
        // rather than a hard-coded epoch literal, because a literal is only
        // right in one timezone — and because these two entry points reaching
        // the same instant for the same day IS the property: a run on July 2nd
        // defaults to the start of July 1st, and `--since 2026-07-01` must mean
        // exactly that same moment.
        let july_2nd = chrono::NaiveDate::from_ymd_opt(2026, 7, 2).unwrap();
        assert_eq!(seen.get(), default_since(july_2nd),
                   "--since is local midnight, the same day boundary the default uses");
    }

    /// East of Greenwich, reading `--since` as UTC midnight starts the scan
    /// hours into the user's morning and drops the edits before it — the
    /// under-scanning direction, i.e. the exact failure this feature exists to
    /// prevent. This pins the direction without depending on the test machine's
    /// zone: local midnight is never *later* than the same day's UTC midnight
    /// anywhere east of UTC, and the two coincide only in UTC itself.
    #[test]
    fn since_is_read_in_the_users_own_timezone_not_utc() {
        let dir = tempfile::tempdir().unwrap();
        let seen = std::cell::Cell::new(0i64);
        sync_since(dir.path(), DIRS, Some("2026-07-01"), today(), NOW, true,
                   |since| { seen.set(since); Ok(vec![]) }, |_| unreachable!()).unwrap();

        let utc_midnight = 1_782_864_000_000i64; // 2026-07-01T00:00:00Z, `date -u … +%s` × 1000
        let offset_ms = Local.timestamp_millis_opt(utc_midnight).single().unwrap()
            .offset().local_minus_utc() as i64 * 1000;
        assert_eq!(seen.get(), utc_midnight - offset_ms,
                   "local midnight is UTC midnight shifted back by the zone's offset");
        if offset_ms > 0 {
            assert!(seen.get() < utc_midnight,
                    "east of UTC, reading it as UTC would skip that morning's edits");
        }
    }

    #[test]
    fn no_vault_and_no_ledger_starts_at_local_yesterday_midnight() {
        let dir = tempfile::tempdir().unwrap();
        let seen = std::cell::Cell::new(0i64);
        sync_since(dir.path(), DIRS, None, today(), NOW, true,
                   |since| { seen.set(since); Ok(vec![]) }, |_| unreachable!()).unwrap();
        assert_eq!(seen.get(), default_since(today()));
    }

    #[test]
    fn default_since_is_local_midnight_at_the_start_of_yesterday() {
        let d = Local.timestamp_millis_opt(default_since(today())).single().unwrap();
        assert_eq!(
            d.naive_local(),
            (today() - Duration::days(1)).and_hms_opt(0, 0, 0).unwrap(),
            "not UTC midnight, and not the start of today"
        );
    }

    /// A vault with a watermark old enough that the toy `edited` values below
    /// are all in the future of it — so `since` behaves the way it does against
    /// a real graph instead of being swamped by `default_since`.
    fn seeded(dir: &Path, watermark_ms: i64) {
        let l = crate::ledger::Ledger {
            last_synced_at: iso_ms(Some(watermark_ms)),
            ..Default::default()
        };
        l.save(dir).unwrap();
    }

    /// Roam filters server-side and **strictly** (`[(> ?t ?since)]`), so a run
    /// that resumes from T never sees a page whose `edited` *is* T. That is the
    /// whole reason a shared timestamp cannot be crossed page by page.
    fn as_roam_would(all: Vec<Changed>) -> impl FnOnce(i64) -> Result<Vec<Changed>, String> {
        move |since| Ok(all.into_iter().filter(|c| c.edited > since).collect())
    }

    #[test]
    fn a_failure_inside_a_shared_timestamp_holds_the_watermark_below_that_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        seeded(dir.path(), 0);
        let batch = vec![
            Changed { uid: "a".into(), edited: 1000 },
            Changed { uid: "b".into(), edited: 2000 },
            Changed { uid: "c".into(), edited: 2000 },
        ];

        let first = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            as_roam_would(batch.clone()),
            |uid| if uid == "c" { Err("network went away".into()) } else { Ok(Some(page(uid, uid, "x"))) },
        ).unwrap();

        assert_eq!((first.synced, first.failed), (2, 1));
        let l = crate::ledger::Ledger::load(dir.path()).ledger;
        assert_eq!(
            l.last_synced_at.as_deref(), Some("1970-01-01T00:00:01.000Z"),
            "`a` is alone at 1000 so the watermark reaches it, but `b` succeeded at 2000 \
             with `c` sharing that millisecond — advancing to 2000 would make the next \
             run's strict `> 2000` skip `c` forever"
        );
        assert_eq!(l.path_of("b"), Some("wikipage/b.note.md"),
                   "work that finished must survive the failure that stopped the run");

        // Second run: the group comes back whole, because the watermark stayed
        // below it. Both members succeed, and only now does it move past 2000.
        let mut fetched: Vec<String> = vec![];
        let second = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            as_roam_would(batch),
            |uid| { fetched.push(uid.to_string()); Ok(Some(page(uid, uid, "x"))) },
        ).unwrap();
        assert_eq!(fetched, vec!["b", "c"], "`c` — never synced — must be seen again, and so is `b`");
        assert_eq!(second.failed, 0);
        assert_eq!(crate::ledger::Ledger::load(dir.path()).ledger.last_synced_at.as_deref(),
                   Some("1970-01-01T00:00:02.000Z"),
                   "the whole group finished, so the watermark may finally cross it");
    }

    #[test]
    fn a_batch_is_walked_in_edited_then_uid_order() {
        let dir = tempfile::tempdir().unwrap();
        seeded(dir.path(), 0);
        let mut fetched: Vec<String> = vec![];
        sync_since(
            dir.path(), DIRS, None, today(), NOW, true,
            |_| Ok(vec![
                Changed { uid: "c".into(), edited: 2000 },
                Changed { uid: "a".into(), edited: 2000 },
                Changed { uid: "b".into(), edited: 1000 },
            ]),
            |uid| { fetched.push(uid.to_string()); Ok(Some(page(uid, uid, "x"))) },
        ).unwrap();
        assert_eq!(fetched, vec!["b", "a", "c"], "ties break on uid, so a run is reproducible");
    }

    #[test]
    fn a_failure_on_the_first_page_leaves_the_watermark_exactly_where_it_was() {
        let dir = tempfile::tempdir().unwrap();
        seeded(dir.path(), 500);
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![
                Changed { uid: "a".into(), edited: 1000 },
                Changed { uid: "b".into(), edited: 2000 },
            ]),
            |_| Err("roam is not running".into()),
        ).unwrap();
        assert_eq!((r.scanned, r.synced, r.failed), (2, 0, 1));
        assert_eq!(r.to.as_deref(), Some("1970-01-01T00:00:00.500Z"));
        assert_eq!(crate::ledger::Ledger::load(dir.path()).ledger.last_synced_at.as_deref(),
                   Some("1970-01-01T00:00:00.500Z"));
    }

    #[test]
    fn every_page_failing_costs_exactly_one_attempt_and_no_watermark() {
        let dir = tempfile::tempdir().unwrap();
        let mut attempts = 0usize;
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![
                Changed { uid: "a".into(), edited: 1000 },
                Changed { uid: "b".into(), edited: 2000 },
                Changed { uid: "c".into(), edited: 3000 },
            ]),
            |_| { attempts += 1; Err("roam is not running".into()) },
        ).unwrap();
        assert_eq!(attempts, 1, "a dead connection must not be retried once per page");
        assert_eq!((r.synced, r.failed, r.errors.len()), (0, 1, 1));
        assert!(r.to.is_none());
        assert!(crate::ledger::Ledger::load(dir.path()).ledger.last_synced_at.is_none());
    }

    #[test]
    fn a_blockless_page_still_moves_the_watermark_past_itself() {
        let dir = tempfile::tempdir().unwrap();
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![Changed { uid: "tag".into(), edited: 1000 }]),
            |_| Ok(Some(RoamPage { title: "PKM".into(), uid: Some("tag".into()),
                                   create_time: None, edit_time: None, children: vec![] })),
        ).unwrap();
        assert_eq!(r.skipped, 1);
        assert_eq!(r.to.as_deref(), Some("1970-01-01T00:00:01.000Z"),
                   "it was dealt with — leaving the watermark behind would re-fetch it forever");
    }

    #[test]
    fn a_page_roam_no_longer_has_is_skipped_rather_than_failing_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![Changed { uid: "gone".into(), edited: 1000 }]),
            |_| Ok(None),
        ).unwrap();
        assert_eq!((r.skipped, r.failed), (1, 0));
        assert_eq!(r.to.as_deref(), Some("1970-01-01T00:00:01.000Z"));
    }

    #[test]
    fn a_rename_whose_old_file_is_gone_is_written_as_a_fresh_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.claim("u", "wikipage/旧名.note.md", "旧名");
        l.save(dir.path()).unwrap();

        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![Changed { uid: "u".into(), edited: 1000 }]),
            |_| Ok(Some(page("u", "新名", "from roam"))),
        ).unwrap();

        assert!(r.renamed.is_empty(), "nothing was moved, because there was nothing to move");
        assert_eq!(r.synced, 1);
        let fresh = std::fs::read_to_string(dir.path().join("wikipage/新名.note.md")).unwrap();
        assert!(fresh.contains("from roam"));
        assert_eq!(crate::ledger::Ledger::load(dir.path()).ledger.path_of("u"),
                   Some("wikipage/新名.note.md"));
    }

    #[test]
    fn a_rename_never_overwrites_a_note_already_sitting_at_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("wikipage")).unwrap();
        std::fs::write(dir.path().join("wikipage/旧名.note.md"),
            "---\ntype: Wiki Page\ntitle: 旧名\n---\n- from roam\n  id:: u-b1\n").unwrap();
        // Not in the ledger — the user made it themselves, and it is the only
        // copy of what they wrote in it.
        std::fs::write(dir.path().join("wikipage/新名.note.md"),
            "---\ntype: Wiki Page\ntitle: 新名\n---\n- 我自己建的\n").unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.claim("u", "wikipage/旧名.note.md", "旧名");
        l.save(dir.path()).unwrap();

        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![Changed { uid: "u".into(), edited: 1000 }]),
            |_| Ok(Some(page("u", "新名", "from roam"))),
        ).unwrap();

        assert!(r.renamed.is_empty(), "nothing moved, so nothing to list as moved");
        assert!(dir.path().join("wikipage/旧名.note.md").exists(), "the old file is left, not deleted");
        let dest = std::fs::read_to_string(dir.path().join("wikipage/新名.note.md")).unwrap();
        assert!(dest.contains("我自己建的"), "the destination's own blocks must survive");
        assert!(dest.contains("from roam"), "and Roam's half merges into it as usual");

        // Refusing to move is right, but doing it silently is not: the user now
        // has two files for one page, and the local-only blocks — the half that
        // cannot be re-fetched — are in the orphan, not in the file this sync
        // just wrote. §8 acceptance 3 promises otherwise, so the run must say so.
        assert_eq!(r.failed, 0, "not a failed page — the sync did its work");
        let told = r.errors.iter().find(|e| e.contains("旧名")).unwrap_or_else(
            || panic!("the blocked rename was never reported: {:?}", r.errors));
        assert!(told.contains("wikipage/新名.note.md") && told.contains("already exists"),
                "the message must name both files: {told}");
    }

    #[test]
    fn a_dry_run_reports_a_rename_without_moving_anything() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("wikipage")).unwrap();
        std::fs::write(dir.path().join("wikipage/旧名.note.md"),
            "---\ntype: Wiki Page\ntitle: 旧名\n---\n- from roam\n  id:: u-b1\n").unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.claim("u", "wikipage/旧名.note.md", "旧名");
        l.save(dir.path()).unwrap();

        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, true,
            |_| Ok(vec![
                Changed { uid: "u".into(), edited: 1000 },
                Changed { uid: "8IFJWtnad".into(), edited: 2000 },
            ]),
            |uid| Ok(Some(match uid {
                "u" => page("u", "新名", "from roam"),
                _ => page("8IFJWtnad", "回顾系统", "x"),
            })),
        ).unwrap();

        assert_eq!(r.renamed, vec![Renamed {
            uid: "u".into(), from: "wikipage/旧名.note.md".into(), to: "wikipage/新名.note.md".into(),
        }]);
        assert!(r.to.is_none(), "a dry run persists nothing, so it reports no new watermark");
        assert!(dir.path().join("wikipage/旧名.note.md").exists(), "not moved");
        assert!(!dir.path().join("wikipage/新名.note.md").exists(), "not written");
        assert!(!dir.path().join("wikipage/回顾系统.note.md").exists(), "not written");
        let after = crate::ledger::Ledger::load(dir.path()).ledger;
        assert_eq!(after.path_of("u"), Some("wikipage/旧名.note.md"), "the ledger on disk is untouched");
        assert!(after.last_synced_at.is_none());
    }

    /// `route_page` builds a path out of these two host-supplied names, and
    /// `Path::join` *replaces* the base when the component is absolute — so an
    /// unchecked folder name does not merely misfile a note, it writes outside
    /// the vault. Refused before a single query is even asked.
    #[test]
    fn a_folder_that_could_escape_the_vault_is_refused_before_anything_is_fetched() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["", "/etc", "../elsewhere", "wiki/../..", ".."] {
            for dirs in [(bad, "dailynote"), ("wikipage", bad)] {
                let err = sync_since(dir.path(), dirs, None, today(), NOW, false,
                                     |_| panic!("must not query"), |_| unreachable!()).unwrap_err();
                assert!(err.contains("expected a relative path inside the vault"),
                        "{dirs:?} was accepted: {err}");
            }
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "nothing was written");
    }

    #[test]
    fn an_unreadable_watermark_falls_back_to_yesterday_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let l = crate::ledger::Ledger {
            last_synced_at: Some("<<<<<<< HEAD".into()),
            ..Default::default()
        };
        l.save(dir.path()).unwrap();
        let seen = std::cell::Cell::new(0i64);
        let r = sync_since(dir.path(), DIRS, None, today(), NOW, false,
                           |since| { seen.set(since); Ok(vec![]) }, |_| unreachable!()).unwrap();
        assert_eq!(seen.get(), default_since(today()));
        assert_eq!(r.failed, 0, "a mangled ledger is not a failed page");
        assert!(r.errors[0].contains("unreadable last sync time"), "{:?}", r.errors);
    }

    /// The whole no-skip guarantee rests on this one file. A ledger truncated
    /// by a kill mid-save, or carrying conflict markers from a git merge across
    /// two devices, reads back as an empty one — and if that were reported as a
    /// clean first run, `since` would silently fall back to yesterday and every
    /// page edited earlier would be abandoned, with no later run ever looking
    /// that far back again.
    #[test]
    fn a_ledger_that_is_there_but_unreadable_makes_the_run_visibly_not_clean() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(dir.path().join(crate::ledger::LEDGER_REL),
                       "<<<<<<< HEAD\n{\"lastSyncedAt\":\"2026-07-01T00:00:00.000Z\"}").unwrap();
        let seen = std::cell::Cell::new(0i64);
        let r = sync_since(dir.path(), DIRS, None, today(), NOW, false,
                           |since| { seen.set(since); Ok(vec![]) }, |_| unreachable!()).unwrap();

        assert_eq!(seen.get(), default_since(today()), "there is nothing better to start from");
        assert_eq!(r.failed, 0, "no page failed");
        assert!(!r.errors.is_empty(), "but this run is NOT clean and must not look like one");
        assert!(r.errors[0].contains("unreadable"), "{:?}", r.errors);
        assert!(r.to.is_none(), "and there is no watermark to report, garbage least of all");
    }

    #[test]
    fn an_unreadable_watermark_is_never_echoed_back_as_the_run_result() {
        let dir = tempfile::tempdir().unwrap();
        let l = crate::ledger::Ledger {
            last_synced_at: Some("<<<<<<< HEAD".into()),
            ..Default::default()
        };
        l.save(dir.path()).unwrap();
        let r = sync_since(dir.path(), DIRS, None, today(), NOW, false,
                           |_| Ok(vec![]), |_| unreachable!()).unwrap();
        assert!(r.to.is_none(), "`to` is a timestamp or nothing — never a passed-through string");
    }

    #[test]
    fn a_backfill_does_not_rewind_the_watermark() {
        let dir = tempfile::tempdir().unwrap();
        seeded(dir.path(), 10_000);
        let r = sync_since(
            dir.path(), DIRS, Some("1970-01-01"), today(), NOW, false,
            |since| { assert_eq!(since, default_since(chrono::NaiveDate::from_ymd_opt(1970, 1, 2).unwrap()),
                                 "--since wins over the ledger for the query, as local midnight");
                      Ok(vec![Changed { uid: "old".into(), edited: 1000 }]) },
            |uid| Ok(Some(page(uid, uid, "x"))),
        ).unwrap();
        assert_eq!(r.synced, 1, "the old page is re-synced, which is the point of a backfill");
        assert_eq!(crate::ledger::Ledger::load(dir.path()).ledger.last_synced_at.as_deref(),
                   Some("1970-01-01T00:00:10.000Z"),
                   "but the frontier does not move back to 1000, or the next run rescans it all");
    }

    #[test]
    fn a_second_identical_run_writes_nothing_at_all() {
        let dir = tempfile::tempdir().unwrap();
        seeded(dir.path(), 0);
        let batch = vec![Changed { uid: "8IFJWtnad".into(), edited: 1000 }];
        let run = |b: Vec<Changed>| sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(b), |uid| Ok(Some(page(uid, "回顾系统", "概念内容"))),
        ).unwrap();

        assert_eq!(run(batch.clone()).synced, 1);
        let note = dir.path().join("wikipage/回顾系统.note.md");
        let first = std::fs::read_to_string(&note).unwrap();
        // Same batch again — as if discovery over-reported. Nothing may change.
        let second = run(batch);
        assert_eq!((second.synced, second.skipped), (0, 1), "an unchanged page is not a write");
        assert_eq!(std::fs::read_to_string(&note).unwrap(), first, "not one byte");
    }

    #[test]
    fn an_invalid_since_is_refused_rather_than_silently_misread() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["2026-13-40", "07/01/2026", "yesterday", "2026-7-1"] {
            let err = sync_since(dir.path(), DIRS, Some(bad), today(), NOW, true,
                                 |_| panic!("must not query"), |_| unreachable!()).unwrap_err();
            assert!(err.contains("invalid --since"), "{bad} was accepted: {err}");
        }
    }
}
