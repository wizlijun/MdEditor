//! Format-drift guard. The SAME fixture is asserted from the Rust side (this
//! file) and the TypeScript side (`src/lib/outline/roam-golden.test.ts`). If
//! either side's `.note.md` format moves, one of them goes red.
//!
//! This runs the real write path — `sync::sync_day` against a tempdir seeded
//! with `local-before.note.md` — rather than re-assembling the pipeline by
//! hand, so the fixture pins the bytes the plugin actually puts in the user's
//! vault, not a test-only approximation of them.
use notemd_roam_import::{outline, roam_page, sync};

const ROAM_DAY: &str = include_str!("fixtures/roam-day.json");
const LOCAL_BEFORE: &str = include_str!("fixtures/local-before.note.md");
const GOLDEN: &str = include_str!("fixtures/daily.note.md");
const FM_TOUCH: &str = include_str!("fixtures/frontmatter-touch.json");

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
    // a grandchild, an evening block, a shopping list and a code block
    // (created), edited the TODO's wording (updated), the user's question,
    // their reply under the TODO and the whole annotation → question → answer
    // subtree are untouched (kept_local), and the block Roam has since deleted
    // is still there (roam_gone_kept).
    assert_eq!(
        (out.created, out.updated, out.kept_local, out.roam_gone_kept),
        (4, 1, 5, 1)
    );
    assert!(out.found);
    assert_eq!(out.path, REL);
}

/// The sync runs unattended and repeatedly. Running it twice against an
/// unchanged Roam page must not move a block, duplicate one, or drop the
/// user's writing — on the real fixture, not just the toy trees in `merge`'s
/// unit tests. A third run because C2/C3 (blocks that forge outline structure)
/// compounded rather than settling: 1 → 2 → 3 copies.
///
/// The clock moves between runs, exactly as it does under cron. A no-op sync
/// must not rewrite the note at all — asserted on the mtime, since equal bytes
/// alone would not prove no write happened.
#[test]
fn syncing_the_same_day_again_does_not_touch_the_file() {
    let dir = vault_with_local_before();
    let path = dir.path().join(REL);
    sync::sync_day(dir.path(), DAILY_DIR, Some(&page()), DATE, NOW).unwrap();
    let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));

    for (run, now) in [(2, "2026-08-04T11:11:11.111Z"), (3, "2026-08-05T22:22:22.222Z")] {
        let out = sync::sync_day(dir.path(), DAILY_DIR, Some(&page()), DATE, now).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), GOLDEN, "run {run} moved the bytes");
        assert_eq!(
            std::fs::metadata(&path).unwrap().modified().unwrap(),
            mtime,
            "run {run} rewrote the file even though nothing changed"
        );
        assert_eq!((out.created, out.updated), (0, 0), "run {run} thought something was new");
    }
}

/// Front-matter drift guard, Rust half. The golden `.note.md` above cannot
/// catch this: its TS half round-trips `tree.frontmatter` verbatim and never
/// calls the host's `touchFrontmatter`, so `outline::touch_frontmatter` — a
/// hand-rolled line-based reimplementation of a function the host builds on
/// the `yaml` package — had no counterpart assertion at all. Both sides now
/// assert the same cases; see `src/lib/outline/roam-golden.test.ts`.
#[test]
fn frontmatter_touch_matches_the_shared_fixture() {
    let fixture: serde_json::Value = serde_json::from_str(FM_TOUCH).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let got = outline::touch_frontmatter(
            c["raw"].as_str(),
            c["title"].as_str().unwrap(),
            c["created"].as_str().unwrap(),
            c["now"].as_str().unwrap(),
        );
        assert_eq!(got, c["expected"].as_str().unwrap(), "{name}");
    }
}
