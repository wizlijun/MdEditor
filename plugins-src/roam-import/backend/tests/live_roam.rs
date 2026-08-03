//! End-to-end against the REAL local Roam graph, into a tempdir — never the
//! user's vault. Ignored by default: it needs the Roam desktop app running and
//! `roam connect` already done, so it must not fail a normal `cargo test`.
//!
//!     cargo test --test live_roam -- --ignored --nocapture
use notemd_roam_import::{discover, roam_cli, roam_page, sync};

#[test]
#[ignore = "needs a connected roam CLI and the Roam desktop app running"]
fn syncs_a_real_day_three_times_without_moving_a_byte() {
    let date = std::env::var("ROAM_DAY").unwrap_or_else(|_| "2026-08-02".to_string());
    let uid = notemd_roam_import::dates::to_roam_uid(&date).expect("yyyy-MM-dd");
    let exe = discover::discover(None).expect("roam CLI not found");
    let raw = roam_cli::fetch_day(&exe, None, &uid).expect("datalog-query failed");
    let Some(page) = roam_page::parse_day_result(&raw).expect("unreadable datalog output") else {
        println!("Roam has no daily page for {date} — nothing to check");
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let rel = sync::daily_rel_path("dailynote", &date);
    let path = dir.path().join(&rel);

    let mut prev: Option<String> = None;
    for run in 1..=3 {
        // A different clock each run, exactly as cron would.
        let now = format!("2026-08-0{run}T09:00:00.000Z");
        let out = sync::sync_day(dir.path(), "dailynote", Some(&page), &date, &now).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        println!(
            "run {run}: created={} updated={} kept_local={} roam_gone_kept={} bytes={}",
            out.created, out.updated, out.kept_local, out.roam_gone_kept, text.len()
        );
        if let Some(p) = &prev {
            assert_eq!(&text, p, "run {run} moved bytes on an unchanged Roam page");
            assert_eq!((out.created, out.updated), (0, 0), "run {run} saw new material");
        } else {
            println!("--- first write ---\n{text}\n--- end ---");
        }
        prev = Some(text);
    }

    // And the file the plugin wrote must read back as the tree it serialized.
    let text = prev.unwrap();
    let back = notemd_roam_import::outline::parse_outline(&text);
    assert_eq!(
        notemd_roam_import::outline::serialize_outline(&back),
        text,
        "the real day's note does not round-trip through our own parser"
    );
}
