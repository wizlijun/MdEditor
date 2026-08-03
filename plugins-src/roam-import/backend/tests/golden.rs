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
    // a grandchild, an evening block, a shopping list, a code block and an
    // empty block (created), edited the TODO's wording (updated), the user's
    // question, their reply under the TODO, their empty bullet and the whole
    // annotation → question → answer subtree are untouched (kept_local), and
    // the block Roam has since deleted is still there (roam_gone_kept).
    assert_eq!(
        (out.created, out.updated, out.kept_local, out.roam_gone_kept),
        (5, 1, 6, 1)
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

/// The four line shapes `parse_outline` reads as structure, asserted from this
/// side by reading the golden bytes back — not just by writing them. Byte
/// equality above already pins what the plugin *emits*; this pins what the
/// plugin's own parser then makes of those bytes, which is the half that has to
/// agree with the host (`src/lib/outline/roam-golden.test.ts` asserts the same
/// four against `parseOutline`).
///
/// Each shape, left unhandled, has cost a block the `id::` that is its
/// identity — after which `merge` sees a brand-new Roam block and re-creates
/// it on every single sync, multiplying the user's note without bound. All
/// four have been real bugs in this plugin.
#[test]
fn the_structural_shapes_read_back_as_the_blocks_they_are() {
    let tree = outline::parse_outline(GOLDEN);
    let by_id = |id: &str| tree.nodes.iter().find(|n| n.id == id).unwrap_or_else(|| panic!("no {id}"));

    // 1. `key:: value` on a continuation line is text, not a property.
    assert_eq!(by_id("Nb7sT1uEv").content, "meeting notes\n id:: not-a-property");

    // 2. A Roam shift-enter list — `shopping\n- milk\n-\n- eggs` — is ONE
    //    block, not a block with three child bullets. Its third line is the
    //    *empty* bullet shape (a line of nothing but `-`), which the escaper
    //    had to learn separately from `- milk`.
    let shopping = by_id("RmQ2xL8vC");
    assert_eq!(shopping.content, "shopping\n - milk\n -\n - eggs");
    assert!(shopping.persist_id, "the block must keep the id:: that is its identity");
    assert!(
        tree.nodes.iter().all(|n| n.parent.as_deref() != Some("RmQ2xL8vC")),
        "a shift-enter line was read back as a child bullet"
    );

    // 3. A fence the Roam block never closed is closed for it, so the blocks
    //    after it survive as their own nodes instead of being eaten into the
    //    fence body.
    assert_eq!(by_id("Fp3nH6wDs").content, "```js\nconst x = 1\n```");
    assert!(tree.nodes.iter().any(|n| n.id == "Ez6yV4rTn"), "the tail was swallowed by the fence");

    // 4. An empty Roam block is written `- ` — dash, space, nothing — so the
    //    trailing space would otherwise carry the whole meaning of "this
    //    bullet exists". It is a node with empty content, and the property
    //    lines under it belong to IT, not to the block above.
    let empty = by_id("Ez6yV4rTn");
    assert_eq!(empty.content, "");
    assert_eq!(empty.created_at.as_deref(), Some("2026-08-02T14:20:00.000Z"));
    assert_eq!(empty.updated_at.as_deref(), Some("2026-08-02T14:21:00.000Z"));
    assert_eq!(
        by_id("Fp3nH6wDs").updated_at.as_deref(),
        Some("2026-08-02T14:16:40.000Z"),
        "the empty bullet's properties leaked into the block above it"
    );

    // …and the user's own empty bullet: `local-before.note.md` carries it in
    // the whitespace-stripped spelling (a bare `-`, which is what editors,
    // formatters and git hooks leave behind), and the merge keeps it as a
    // local block — with no `id::`, so it is theirs, not Roam's.
    let mine: Vec<_> = tree
        .nodes
        .iter()
        .filter(|n| n.content.is_empty() && !n.persist_id)
        .collect();
    assert_eq!(mine.len(), 1, "the user's stripped empty bullet did not survive as a node");
    assert_eq!(mine[0].parent, None);
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
            // Explicit in the fixture, not defaulted on either side: the value
            // that has to match is the one that lands on disk (OKF §4.1).
            c["type"].as_str().unwrap(),
            c["title"].as_str().unwrap(),
            c["created"].as_str().unwrap(),
            c["now"].as_str().unwrap(),
        );
        assert_eq!(got, c["expected"].as_str().unwrap(), "{name}");
    }
}

/// End-to-end, the check the branch's own end-to-end missed. That run only
/// ever synced daily notes — the real ledger holds two entries, both
/// `MM-DD-YYYY` — so the front-matter writer was never handed anything but a
/// `yyyy-MM-dd` date, and a `title:` written raw looked perfectly safe.
///
/// This drives the real `incremental::sync_since` (routing, `sync_page`,
/// `touch_frontmatter`, the atomic write) over the wiki-page titles that
/// break raw YAML, and asserts the bytes that reach the vault. The
/// TypeScript half runs the repo's own `pnpm okf:lint` implementation over
/// the same blocks (`src/lib/outline/roam-golden.test.ts`); before the fix,
/// `pnpm okf:lint` reported `frontmatter-unparsable` on four of these nine
/// files.
#[test]
fn a_wiki_page_sync_writes_front_matter_a_yaml_reader_can_read() {
    use notemd_roam_import::{changed::Changed, incremental, roam_page::{RoamBlock, RoamPage}};

    // (uid, Roam title, the file it must land in, the `title:` line it must
    // carry). Every title here is one raw YAML gets wrong: unparsable
    // (`: `, a leading indicator), or — quieter and worse — parsable as
    // something else (`PKM #2` truncates to `PKM`, `2026` becomes a number,
    // `[[nested]]` becomes a nested list).
    let cases: Vec<(&str, &str, &str, &str)> = vec![
        ("u1", "Book: Thinking Fast and Slow",
         "wikipage/Book- Thinking Fast and Slow.note.md",
         "title: \"Book: Thinking Fast and Slow\""),
        ("u2", "PKM #2", "wikipage/PKM #2.note.md", "title: \"PKM #2\""),
        ("u3", "*star", "wikipage/star.note.md", "title: \"*star\""),
        ("u4", "@home", "wikipage/@home.note.md", "title: \"@home\""),
        ("u5", "[[nested]]", "wikipage/[[nested]].note.md", "title: \"[[nested]]\""),
        ("u6", "Review: \"Dune\"", "wikipage/Review- -Dune.note.md",
         "title: 'Review: \"Dune\"'"),
        ("u7", "2026", "wikipage/2026.note.md", "title: \"2026\""),
        // …and an ordinary one, which must NOT grow quotes it does not need.
        ("u8", "回顾/系统", "wikipage/回顾-系统.note.md", "title: 回顾/系统"),
        // The shape the branch's end-to-end covered, unchanged.
        ("08-02-2026", "August 2nd, 2026", "dailynote/2026/2026-08-02.note.md",
         "title: 2026-08-02"),
    ];

    let dir = tempfile::tempdir().unwrap();
    let pages = cases.clone();
    let batch: Vec<Changed> = cases
        .iter()
        .enumerate()
        .map(|(i, (uid, ..))| Changed { uid: (*uid).into(), edited: 1000 + i as i64 })
        .collect();

    let report = incremental::sync_since(
        dir.path(), ("wikipage", "dailynote"), None, Some("e2e"),
        chrono::NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(), NOW, false,
        move |_| Ok(batch),
        move |uid| Ok(pages.iter().find(|(u, ..)| *u == uid).map(|(u, title, ..)| RoamPage {
            title: (*title).into(), uid: Some((*u).into()),
            create_time: Some(1785600005019), edit_time: None,
            children: vec![RoamBlock {
                uid: Some(format!("{u}-b1")), string: "第一条".into(), order: 0,
                heading: None, create_time: None, edit_time: None, children: vec![],
            }],
        })),
    )
    .unwrap();

    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.synced, cases.len());

    for (uid, roam_title, rel, title_line) in &cases {
        let text = std::fs::read_to_string(dir.path().join(rel))
            .unwrap_or_else(|e| panic!("{uid} ({roam_title}) is not at {rel}: {e}"));
        let fm = text
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---\n"))
            .unwrap_or_else(|| panic!("{rel} has no front-matter block:\n{text}"))
            .0;
        // OKF §4.1's one REQUIRED key, and the title, spelled the way the
        // host's `yaml` package spells it.
        assert!(
            fm.lines().any(|l| l == "type: Wiki Page" || l == "type: Daily Note"),
            "{rel} carries no OKF type:\n{fm}",
        );
        assert!(fm.lines().any(|l| l == *title_line), "{rel}:\nwant {title_line}\ngot:\n{fm}");
    }

    // …and the report names every one of them, which is what a `--dry-run`
    // over the same graph would have printed.
    let listed: Vec<&str> = report.pages.iter().map(|p| p.rel.as_str()).collect();
    for (_, _, rel, _) in &cases {
        assert!(listed.contains(rel), "{rel} was written but not reported: {listed:?}");
    }
}
