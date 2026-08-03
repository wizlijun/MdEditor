//! Format-drift guard. The SAME fixture is asserted from the Rust side (this
//! file) and the TypeScript side (`src/lib/outline/roam-golden.test.ts`). If
//! either side's `.note.md` format moves, one of them goes red.
//!
//! This runs the real write path — `sync::sync_day` against a tempdir seeded
//! with `local-before.note.md` — rather than re-assembling the pipeline by
//! hand, so the fixture pins the bytes the plugin actually puts in the user's
//! vault, not a test-only approximation of them.
use notemd_roam_import::{roam_page, sync};

const ROAM_DAY: &str = include_str!("fixtures/roam-day.json");
const LOCAL_BEFORE: &str = include_str!("fixtures/local-before.note.md");
const GOLDEN: &str = include_str!("fixtures/daily.note.md");

const DATE: &str = "2026-08-02";
const DAILY_DIR: &str = "dailynote";
const NOW: &str = "2026-08-03T09:00:00.000Z";
const REL: &str = "dailynote/2026/2026-08-02.note.md";

/// A vault holding yesterday's note exactly as an earlier sync left it, plus
/// the two things the user added afterwards.
fn vault_with_local_before() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(REL);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, LOCAL_BEFORE).unwrap();
    dir
}

fn page() -> roam_page::RoamPage {
    let raw: serde_json::Value = serde_json::from_str(ROAM_DAY).unwrap();
    roam_page::parse_day_result(&raw).unwrap().unwrap()
}

#[test]
fn merged_output_matches_the_golden_fixture() {
    let dir = vault_with_local_before();
    let out = sync::sync_day(dir.path(), DAILY_DIR, Some(&page()), DATE, NOW).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join(REL)).unwrap(), GOLDEN);

    // The stats the UI reports, pinned against the same fixture: Roam gained
    // an evening block and a grandchild (created), edited the TODO's wording
    // (updated), the user's question and their reply under the TODO are
    // untouched (kept_local), and the block Roam has since deleted is still
    // there (roam_gone_kept).
    assert_eq!(
        (out.created, out.updated, out.kept_local, out.roam_gone_kept),
        (2, 1, 2, 1)
    );
    assert!(out.found);
    assert_eq!(out.path, REL);
}

/// The sync runs unattended and repeatedly. Running it twice against an
/// unchanged Roam page must not move a block, duplicate one, or drop the
/// user's writing — on the real fixture, not just the toy trees in
/// `merge`'s unit tests.
#[test]
fn syncing_the_same_day_twice_rewrites_the_same_bytes() {
    let dir = vault_with_local_before();
    sync::sync_day(dir.path(), DAILY_DIR, Some(&page()), DATE, NOW).unwrap();
    let out = sync::sync_day(dir.path(), DAILY_DIR, Some(&page()), DATE, NOW).unwrap();
    assert_eq!(std::fs::read_to_string(dir.path().join(REL)).unwrap(), GOLDEN);
    assert_eq!((out.created, out.updated), (0, 0), "nothing is new the second time");
}
