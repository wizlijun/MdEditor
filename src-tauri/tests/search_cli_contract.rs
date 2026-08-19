//! `notemd search` is an interface for agents as much as for people. Its shape —
//! grep-like output, meaningful exit codes, never failing because the index is
//! unhappy — is the contract; these tests are what stop it from drifting.

use std::path::PathBuf;
use std::process::Command;

fn vault(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (rel, body) in files {
        let p = d.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    d
}

/// A throwaway app-data home so the index lands in a scratch directory instead
/// of the developer's real one — same isolation convention as
/// `cli_builtin_integration.rs` / `cli_startup_timing.rs`. Every test gets its
/// own, so two tests never race on the same on-disk index.
fn temp_home() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "notemd-search-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `dirs::data_local_dir()` is derived from `$HOME` on macOS/most-unix and
/// from `%LOCALAPPDATA%` on Windows — neither reads `XDG_DATA_HOME` (that is
/// Linux-only). Setting both here isolates the index on every platform this
/// suite might run on.
fn isolate(cmd: &mut Command, home: &std::path::Path) {
    cmd.env("HOME", home);
    #[cfg(windows)]
    cmd.env("LOCALAPPDATA", home);
}

/// Guards the two tests below that seed an index *in this process* by
/// temporarily overriding `HOME`/`LOCALAPPDATA` (so `searchidx`'s
/// `dirs::data_local_dir()`-based path math lands on the same on-disk file
/// the isolated CLI subprocess will later open) — the technique
/// `cli_glob_stamp_matches_independently_computed_scan_options_source_globs`
/// established first. `HOME` is process-wide state; `cargo test` runs the
/// tests in one file on multiple threads by default, so two such overrides
/// racing each other (one test restores the other's temp value mid-window)
/// is exactly the shape of this suite's one known flake. This mutex doesn't
/// remove that risk against tests in *other* files (a different process
/// each, out of reach), but it does stop the two in *this* file from
/// colliding with each other, which local runs showed reliably tripping the
/// older test the moment a second HOME-mutating test appeared alongside it.
static PROCESS_HOME_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runs `f` with `HOME` (and, on Windows, `LOCALAPPDATA`) temporarily set to
/// `home` for *this* process, restoring the previous value before returning
/// — under `PROCESS_HOME_ENV_LOCK` so no other test in this file observes
/// the override.
fn with_process_home<T>(home: &std::path::Path, f: impl FnOnce() -> T) -> T {
    let _guard = PROCESS_HOME_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let saved_home = std::env::var_os("HOME");
    #[cfg(windows)]
    let saved_appdata = std::env::var_os("LOCALAPPDATA");
    std::env::set_var("HOME", home);
    #[cfg(windows)]
    std::env::set_var("LOCALAPPDATA", home);

    let out = f();

    match saved_home {
        Some(h) => std::env::set_var("HOME", h),
        None => std::env::remove_var("HOME"),
    }
    #[cfg(windows)]
    match saved_appdata {
        Some(a) => std::env::set_var("LOCALAPPDATA", a),
        None => std::env::remove_var("LOCALAPPDATA"),
    }
    out
}

fn search(vault: &std::path::Path, args: &[&str]) -> std::process::Output {
    let home = temp_home();
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    cmd.arg("--cli").arg("search");
    cmd.args(args);
    cmd.arg("--vault").arg(vault);
    isolate(&mut cmd, &home);
    let out = cmd.output().expect("spawn");
    let _ = std::fs::remove_dir_all(&home);
    out
}

#[test]
fn default_output_is_path_colon_line_colon_text() {
    let v = vault(&[("a.md", "# T\n\nthe brownfox jumped\n")]);
    let out = search(v.path(), &["brownfox"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.lines().any(|l| l.starts_with("a.md:3:")), "{text}");
    assert_eq!(out.status.code(), Some(0));
}

/// agent 靠退出码分支,所以 0/1/2 的含义不能含糊。
#[test]
fn exit_code_one_means_no_matches_not_an_error() {
    let v = vault(&[("a.md", "nothing here\n")]);
    let out = search(v.path(), &["zzzznotfound"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

#[test]
fn json_output_carries_the_full_contract() {
    let v = vault(&[("2026-01-01-a.md", "brownfox\n")]);
    let out = search(v.path(), &["brownfox", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert!(v["route"].as_str().unwrap().starts_with("t1-"));
    assert!(v["took_ms"].is_number());
    let hit = &v["hits"][0];
    for key in ["path", "line", "text", "score", "breadcrumb", "doc_date", "source_ref", "attention_minutes"] {
        assert!(!hit[key].is_null(), "missing {key} in {hit}");
    }
    assert_eq!(hit["source_ref"].as_str().unwrap(), "2026-01-01-a.md#L1");
    assert!(hit["provenance"]["human_verified"].is_boolean());
    // 2026-08-12 design (C-T2): a frontmatter-less file with no configured
    // source-glob match is `unlabeled` (rule 6′), not `source` (the retired
    // rule 6) — `ScanOptions` doesn't carry real source globs yet (C-T3),
    // so this CLI's default vault never matches rule 5′ either.
    assert_eq!(hit["origin"].as_str(), Some("unlabeled"), "no frontmatter, no glob match → rule 6′");
}

/// `json_output_carries_the_full_contract` only proves the key is present —
/// a hardcoded `"attention_minutes": 0.0` in `print_json` would satisfy it
/// too, since a freshly-scanned vault genuinely has no attention data (the
/// CLI never calls `refresh_attention` itself; see `cli::search::run`'s
/// doc comments — ingestion only happens from the GUI's `open_vault` /
/// watcher). To catch that regression this test seeds a real, non-zero
/// `doc_attention` row the same way the GUI would (via
/// `searchidx::SearchIndex::refresh_attention`, reading a real
/// `.notemd/analytics/` day file) and asserts the CLI's own `--json` output
/// actually reflects it — not just that the field exists.
///
/// Uses the same "seed the index directly through the library, in-process,
/// under a temporarily-overridden `HOME` matching what `isolate` sets on the
/// child" technique as `cli_glob_stamp_matches_independently_computed_scan_options_source_globs`
/// above, for the same reason: `SearchIndex::open`'s db path is a pure
/// function of `HOME`/`LOCALAPPDATA`, so this is the only way to land this
/// process and the CLI subprocess on the exact same on-disk index.
#[test]
fn attention_minutes_reflects_real_ingested_data_not_a_hardcoded_stand_in() {
    let v = vault(&[("a.md", "brownfox\n")]);
    let home = temp_home();

    let today = searchidx::chunk::ymd_from_unix_public(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64,
    );
    let analytics_dir = v.path().join(".notemd/analytics");
    std::fs::create_dir_all(&analytics_dir).unwrap();
    // 10 read-minutes, no edits, dated today so the 30-day decay factor is
    // exactly 1.0 — the expected `attention_minutes` is then exactly 10.0,
    // no date arithmetic needed in the assertion below.
    std::fs::write(
        analytics_dir.join(format!("{today}.DEV-1.json")),
        format!(
            // 盘上形状是 `docs: docKey -> counters`(一层),天由文件名隐含 ——
            // 权威定义是 src/lib/insights/store.svelte.ts 的 `DayFile`,不是
            // model.ts 的 `DeviceAnalytics`(那是内存里的多天结构)。v6.817.1
            // 就是把这两者搞混,导致摄取对所有用户静默失效。
            r#"{{"deviceId":"DEV-1","deviceName":"test","day":"{today}","docs":{{
                 "rel:a.md":{{"read_ms":600000,"edit_ms":0,"open_count":1,
                   "edit_sessions":0,"net_chars":0,"mark_ops":0,
                   "first_seen_at":0,"last_active_at":0}}}}}}"#
        ),
    )
    .unwrap();

    with_process_home(&home, || {
        let opts = notemd_lib::search::options::for_vault(v.path());
        let stamp = opts.source_globs.stamp();
        let mut idx = searchidx::SearchIndex::open(v.path(), &stamp).expect("seed open");
        idx.rebuild(&opts).expect("seed build");
        idx.refresh_attention(&[]).expect("seed attention");
    });

    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    cmd.arg("--cli").arg("search").arg("brownfox").arg("--json").arg("--no-sweep").arg("--vault").arg(v.path());
    isolate(&mut cmd, &home);
    let out = cmd.output().expect("spawn");
    let _ = std::fs::remove_dir_all(&home);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    let minutes = j["hits"][0]["attention_minutes"].as_f64().expect("attention_minutes must be a number");
    assert!(
        (minutes - 10.0).abs() < 1e-6,
        "seeded 600_000 read_ms (10 minutes) at zero age (decay ×1.0) must come through as 10.0, got {minutes} in {j}"
    );
}

/// 路径永远是 vault 相对 + `/` 分隔 —— 两平台给 agent 的锚必须一模一样。
#[test]
fn paths_are_vault_relative_with_forward_slashes() {
    let v = vault(&[("docs/sub/a.md", "brownfox\n")]);
    let out = search(v.path(), &["brownfox", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["hits"][0]["path"].as_str().unwrap(), "docs/sub/a.md");
}

/// 降级优于失败:被 agent 用错误搞糊涂的代价大于慢一次。
///
/// Forces `SearchIndex::open` to genuinely fail (not merely redirect to an
/// unusual-but-workable location) by pointing the platform's local-app-data
/// root at a plain FILE: `std::fs::create_dir_all` for the index's parent
/// directory then fails (a path component is not a directory), so
/// `Connection::open` can never succeed and the CLI is forced onto the
/// no-index fallback scan.
#[test]
fn an_unusable_index_degrades_to_a_direct_scan_and_still_exits_zero() {
    let v = vault(&[("a.md", "brownfox\n")]);
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    cmd.arg("--cli").arg("search").arg("brownfox").arg("--vault").arg(v.path());
    let blocker = v.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    // HOME drives `dirs::data_local_dir()` on macOS and most other unix
    // targets; LOCALAPPDATA is queried directly on Windows. (XDG_DATA_HOME,
    // the brief's original knob, is Linux-only and a no-op on macOS — verified
    // against the `dirs`/`dirs-sys` source before relying on this.)
    cmd.env("HOME", &blocker);
    #[cfg(windows)]
    cmd.env("LOCALAPPDATA", &blocker);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("a.md:1:"));
    assert!(!out.stderr.is_empty(), "a degradation must be announced on stderr");
}

/// Task 6 made `origin` observable in `--json` output for the first time —
/// before this, `fallback_scan`'s hardcoded `Origin::Derived` was inert
/// (score stays 0.0 on this path; `score_of` is never called). Left alone,
/// the no-index path would report `derived` for exactly the kind of
/// frontmatter-less file the indexed path classifies by a rule of its own.
/// (When Task 6 wrote this, that rule was rule 6 and the answer was `source`;
/// under the 2026-08-12 design's rule 6′ the same file is `unlabeled`, which
/// is what the paragraph below and the assertions actually check. Either way
/// the divergence is the point: the fallback must not answer `derived` just
/// because it skipped the classifier.) `fallback_scan` has `opts.source_globs`
/// available (plumbed for this exact purpose, as of C-T3) but no
/// `Frontmatter`, so it must parse one itself via the same `origin::derive`
/// the indexed path uses — not merely document the divergence.
///
/// C-T9 extension: `b.md` adds the case a single frontmatter-less file cannot
/// exercise — the absent-vs-empty-frontmatter distinction `origin::derive`'s
/// own doc comment calls out (`Some(&Frontmatter::default())` is NOT `None`).
/// `a.md` has no `---` block at all (`fm_raw` is `None`), so it must resolve
/// to `unlabeled` (rule 6′). `b.md` has a *present but empty* `---\n---\n`
/// block (`fm_raw` is `Some("")`, per `frontmatter::split`), so it must fall
/// through rule 6′ (which only fires on `fm.is_none()`) to rule 7 and resolve
/// to `derived`. `fallback_scan` only gets this right if it captures
/// `fm_present` from `fm_raw.is_some()` *before* `unwrap_or_default()`
/// collapses both shapes into the same `Frontmatter` value — a fallback that
/// instead did `Some(&fm_raw.map(parse).unwrap_or_default())` unconditionally
/// would misreport `a.md` as `derived` too, and a fallback that hardcoded
/// `unlabeled` for anything without a registered `type` would misreport
/// `b.md`. Both files share one query so one process invocation proves both
/// directions of the distinction at once, on the same fallback run.
#[test]
fn the_no_index_fallback_reports_the_same_origin_tier_the_index_would() {
    let v = vault(&[
        ("a.md", "brownfox\n"),
        ("b.md", "---\n---\nbrownfox\n"),
        // Only markdown has frontmatter. A `.txt` opening with `---` is
        // content, and the words inside it must not be read as a header —
        // `chunk::parse_file` gates the split on the extension and this path
        // must gate it identically. It did not: the whole-branch review
        // measured this exact file reporting `source` on the indexed path
        // and `derived` on this one, because the pseudo-frontmatter's
        // `type: Book Summary` reached rule 4 here and nowhere else.
        ("raw/c.txt", "---\ntype: Book Summary\n---\nbrownfox\n"),
        (".notemd/settings.json", r#"{"searchSourceGlobs":["raw/**"]}"#),
    ]);
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    cmd.arg("--cli").arg("search").arg("brownfox").arg("--vault").arg(v.path()).arg("--json");
    let blocker = v.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    cmd.env("HOME", &blocker);
    #[cfg(windows)]
    cmd.env("LOCALAPPDATA", &blocker);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!out.stderr.is_empty(), "a degradation must be announced on stderr (same precedent as the sibling test above)");
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    // Both paths classify a bare `a.md` as `unlabeled` (rule 6′, 2026-08-12
    // design — retired rule 6's `source` default), so the origin assertion
    // below is only meaningful if the `HOME`-blocker trick above actually
    // forced degradation to `fallback_scan` — otherwise this test silently
    // duplicates `json_output_carries_the_full_contract` and would stay
    // green even with the old hardcoded `Origin::Derived` restored. `route`
    // is already in the payload being parsed; pin it too.
    assert_eq!(
        j["route"].as_str(),
        Some("t1-scan"),
        "sanity: this test only proves anything if the no-index fallback actually ran: {j}"
    );
    let hits = j["hits"].as_array().expect("hits array");
    let origin_of = |path: &str| {
        hits.iter().find(|h| h["path"].as_str() == Some(path)).unwrap_or_else(|| panic!("{path} missing: {j}"))["origin"]
            .as_str()
            .map(str::to_string)
    };
    assert_eq!(
        origin_of("a.md").as_deref(),
        Some("unlabeled"),
        "a frontmatter-less .md must classify as unlabeled (rule 6′) on the no-index fallback, \
         the same as the indexed path — not the Derived fallback: {j}"
    );
    assert_eq!(
        origin_of("b.md").as_deref(),
        Some("derived"),
        "a .md with a PRESENT but empty frontmatter block must classify as derived (rule 7), \
         not unlabeled — the absent-vs-empty distinction `origin::derive` documents: {j}"
    );
    assert_eq!(
        origin_of("raw/c.txt").as_deref(),
        Some("source"),
        "a .txt inside a source glob must classify as source on the no-index fallback: its \
         leading `---` block is CONTENT, not frontmatter, so the `type: Book Summary` inside \
         it must never reach rule 4. Reading it as a header classifies this file `derived` \
         here while the indexed path says `source` — the divergence the whole-branch review \
         measured: {j}"
    );
}

/// Degrading is only acceptable if it degrades to *the same corpus*. The
/// index deliberately ignores `.gitignore`/`.ignore` — what belongs in the
/// index is a vault setting (`searchExcludeDirs`), not a decision delegated
/// to whatever the repository happens to keep out of git, and a `.gitignore`d
/// note is still a note. The fallback scan built its walker with `ignore`'s
/// defaults, which honour all of them; since note.md vaults are git
/// repositories, that made "found in the GUI / missing from `notemd search`"
/// a silent, reproducible disagreement. Both paths now share
/// `searchidx::scan::walk_builder`.
///
/// The `.git/HEAD` file is load-bearing: `ignore`'s `require_git` defaults to
/// true, so without it `.gitignore` would not be applied even by a
/// default-configured walker and the test would pass vacuously.
#[test]
fn the_no_index_fallback_searches_the_same_corpus_as_the_index() {
    let files: &[(&str, &str)] = &[
        (".git/HEAD", "ref: refs/heads/main\n"),
        (".gitignore", "gitignored.md\n"),
        (".ignore", "plainignored.md\n"),
        ("gitignored.md", "brownfox one\n"),
        ("plainignored.md", "brownfox two\n"),
    ];

    let v = vault(files);
    let indexed = String::from_utf8_lossy(&search(v.path(), &["brownfox"]).stdout).to_string();
    assert!(indexed.contains("gitignored.md:1:"), "index path: {indexed}");
    assert!(indexed.contains("plainignored.md:1:"), "index path: {indexed}");

    // Same query, index forced to be unusable (see the test above for why
    // pointing the app-data root at a plain file does that).
    let v2 = vault(files);
    let blocker = v2.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    cmd.arg("--cli").arg("search").arg("brownfox").arg("--vault").arg(v2.path());
    cmd.env("HOME", &blocker);
    #[cfg(windows)]
    cmd.env("LOCALAPPDATA", &blocker);
    let out = cmd.output().unwrap();
    let scanned = String::from_utf8_lossy(&out.stdout);
    assert!(scanned.contains("gitignored.md:1:"), "fallback path: {scanned}");
    assert!(scanned.contains("plainignored.md:1:"), "fallback path: {scanned}");
}

#[test]
fn missing_vault_is_the_only_hard_error() {
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    cmd.arg("--cli").arg("search").arg("x").arg("--vault").arg("/definitely/not/here");
    assert_eq!(cmd.output().unwrap().status.code(), Some(2));
}

#[test]
fn stats_reports_the_index_without_searching() {
    let v = vault(&[("a.md", "brownfox\n")]);
    let out = search(v.path(), &["--stats", "--json"]);
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(j["files"].as_i64().unwrap() >= 1);
    assert!(j["tokenizer_id"].is_string());
}

/// Design spec §5.1 gives one reason for the `--json` `origin` field on each
/// hit: "agent 可据此自行分层" — an agent can do its own tiering. That applies
/// just as much to the corpus-level distribution, and `stats()` already
/// computes both counts on every call, so `--stats --json` was throwing them
/// away. An agent deciding whether a vault is mostly its own output or mostly
/// the human's has no other way to ask.
#[test]
fn stats_json_reports_the_provenance_distribution() {
    let v = vault(&[
        ("mine.note.md", "- brownfox\n"),                                   // rule 1 -> human
        ("book.md", "---\ntype: Book\n---\nbrownfox\n"),                    // rule 4 -> source
        ("s.md", "---\ntype: Book Summary\n---\nbrownfox\n"),               // rule 4 -> derived
        ("a.md", "---\ngenerated: { by: claude/1 }\n---\nbrownfox\n"),      // rule 2 -> derived
        ("raw.md", "brownfox, no frontmatter, no matching source glob\n"),  // rule 6' -> unlabeled
    ]);
    let out = search(v.path(), &["--stats", "--json"]);
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    assert_eq!(j["origin_counts"]["human"].as_i64(), Some(1), "{j}");
    assert_eq!(j["origin_counts"]["derived"].as_i64(), Some(2), "{j}");
    assert_eq!(j["origin_counts"]["source"].as_i64(), Some(1), "{j}");
    // Review round 1: the reviewer judged the `unlabeled` key in scope for
    // this task (spec §6.3), not a bonus — and it shipped with no test at
    // all. `raw.md` above has neither frontmatter nor a matching
    // `searchSourceGlobs` pattern (this vault configures none), so it's the
    // one file that reaches rule 6' and must show up here, an agent-facing
    // payload (`notemd search --stats --json`), not just the GUI DTO.
    assert_eq!(j["origin_counts"]["unlabeled"].as_i64(), Some(1), "{j}");
    // Only `derived`'s typed files are itemized (see `searchidx::type_counts`),
    // so the `Book` above must NOT appear here even though it has a type.
    assert_eq!(j["type_counts"]["Book Summary"].as_i64(), Some(1), "{j}");
    assert!(j["type_counts"]["Book"].is_null(), "{j}");
}

#[test]
fn filters_and_limit_flags_work_as_flags_too() {
    let v = vault(&[("docs/a.md", "brownfox\n"), ("other/b.md", "brownfox\n")]);
    let out = search(v.path(), &["brownfox", "--path", "docs"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("docs/a.md"), "{text}");
    assert!(!text.contains("other/b.md"), "{text}");

    let out = search(v.path(), &["brownfox", "--limit", "1"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).lines().count(), 1);
}

/// `--all`(等价 `--limit 0`)返回全部命中——25 个文件特意超过默认的 20 条,
/// 否则「放开」与「默认」无法区分。
#[test]
fn all_flag_and_limit_zero_return_every_hit() {
    let files: Vec<(String, String)> =
        (0..25).map(|i| (format!("f{i:02}.md"), "brownfox\n".to_string())).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, b)| (p.as_str(), b.as_str())).collect();
    let v = vault(&refs);

    let out = search(v.path(), &["brownfox"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).lines().count(), 20, "默认仍是 20 条");

    for flags in [&["brownfox", "--all"][..], &["brownfox", "--limit", "0"][..]] {
        let out = search(v.path(), flags);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).lines().count(),
            25,
            "{flags:?} 必须返回全部命中"
        );
    }
}

#[test]
fn context_flag_prints_surrounding_lines() {
    let v = vault(&[("a.md", "one\ntwo\nbrownfox\nfour\nfive\n")]);
    let out = search(v.path(), &["brownfox", "--context", "1"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("two"), "{text}");
    assert!(text.contains("four"), "{text}");
}

/// Builds a real index against `v` (query `brownfox`), truncates the indexed
/// file's on-disk content to `shrink_to`, then re-queries the SAME (now
/// stale) index with `--no-sweep` plus `extra_query_args` — deterministically
/// reproducing "the file changed after the freshness sweep but before
/// printing" without racing a real sweep. Returns the second invocation's
/// output. Asserts the build call itself found the term, since every caller
/// needs that sanity check before the interesting part.
fn query_against_a_stale_index(
    v: &std::path::Path,
    shrink_to: &str,
    extra_query_args: &[&str],
) -> std::process::Output {
    let home = temp_home();

    let mut build = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    build.arg("--cli").arg("search").arg("brownfox").arg("--vault").arg(v);
    isolate(&mut build, &home);
    let built = build.output().unwrap();
    assert_eq!(
        built.status.code(), Some(0),
        "sanity: the index must find the term before truncation; stderr: {}",
        String::from_utf8_lossy(&built.stderr),
    );

    std::fs::write(v.join("a.md"), shrink_to).unwrap();

    let mut query = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    query.arg("--cli").arg("search").arg("brownfox").args(extra_query_args).arg("--no-sweep").arg("--vault").arg(v);
    isolate(&mut query, &home);
    let out = query.output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    out
}

/// Review round 1, Important #2: `--context` used to compute exit code from
/// the pre-print hit list, not from what was actually printed. If a file
/// shrinks between the (stale, `--no-sweep`-held) index and the print pass —
/// an ordinary edit landing in the same window a real freshness sweep would
/// cover in normal operation — a hit's recorded line range can point past the
/// file's current end, `context_lines` used to emit nothing for it, and the
/// command still exited 0. For a grep-shaped tool, exit 0 is a promise that
/// there is output to read; an agent that trusts it would treat silence as a
/// successful empty answer instead of retrying or falling back to `rg`.
#[test]
fn a_stale_context_hit_is_dropped_and_the_exit_code_follows_what_was_actually_printed() {
    // Blank-line-separated paragraphs, unlike `context_flag_prints_surrounding_lines`'s
    // fixture above: that one is a single soft-wrapped paragraph spanning
    // lines 1-5, so its hit's `line_end` (5) never exceeds a merely-shrunk
    // file. Here each word is its own block, so "brownfox" indexes as a hit
    // pinned to line 5 specifically — the shape needed to push it past the
    // truncated file's length below.
    let v = vault(&[("a.md", "one\n\ntwo\n\nbrownfox\n\nfour\n\nfive\n")]);
    let out = query_against_a_stale_index(v.path(), "only one line now\n", &["--context", "1"]);

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.is_empty(), "a hit whose lines no longer exist must print nothing, not a stale citation: {stdout}");
    assert_eq!(
        out.status.code(), Some(1),
        "exit code must agree with stdout being empty (no output was printed), not with what the stale index believed; stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Review round 2: the round-1 fix guarded on the *clamped window*
/// (`from <= to`) being non-empty, not on whether the hit's own line still
/// exists in the current file. `from = hit.line.saturating_sub(context).max(1)`
/// clamps down to 1 regardless of how large `context` is — so once `context`
/// is at least as big as the shrinkage, `from` lands back inside the
/// (now much shorter) file and the guard passed, printing UNRELATED lines
/// under the original hit's path/line as if they were genuine context.
/// `--context 1` (the test above) happens to fail safe on this fixture only
/// because `from` still clamps past the truncated file's single line; this
/// is the case that arithmetic misses: same shrink, bigger `context`.
#[test]
fn a_context_larger_than_the_shrinkage_still_drops_the_stale_hit_not_unrelated_lines() {
    let v = vault(&[("a.md", "one\n\ntwo\n\nbrownfox\n\nfour\n\nfive\n")]);
    let out = query_against_a_stale_index(v.path(), "only one line now\n", &["--context", "5"]);

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.is_empty(),
        "a hit whose own line no longer exists must never print — even lines that ARE still in the \
         file are not this hit's context once its line is gone: {stdout}"
    );
    assert_eq!(
        out.status.code(), Some(1),
        "exit code must agree with stdout being empty; stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Confirms the round-2 fix didn't over-correct: a normal hit near the end of
/// an UNCHANGED file, where `line_end + context` legitimately exceeds the
/// file's length, must still print — that clamp is the ordinary end-of-file
/// case, not the staleness race, and the new `hit.line` check must not touch
/// it. Deliberately a single normal invocation (no truncation, no stale
/// index): the whole file is one line, and `--context` asks for more
/// neighbors than exist.
#[test]
fn a_large_context_near_the_end_of_an_unchanged_file_still_prints() {
    let v = vault(&[("a.md", "brownfox\n")]);
    let out = search(v.path(), &["brownfox", "--context", "5"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("a.md:1:brownfox"), "{text}");
    assert_eq!(out.status.code(), Some(0));
}

/// Review round 1, Minor: the non-context path runs every line through
/// `one_line` (collapse + 200-char cap) so one huge paragraph can't flood an
/// agent's context; `--context` used to print raw trimmed lines instead,
/// losing that cap for exactly the mode most likely to pull in a long line
/// (a giant table row, a URL) as a neighbor.
#[test]
fn context_output_is_length_capped_like_the_default_output() {
    let long_line = format!("brownfox {}", "x".repeat(500));
    let v = vault(&[("a.md", &format!("{long_line}\n"))]);
    let out = search(v.path(), &["brownfox", "--context", "1"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.trim().is_empty(), "sanity: must have found the hit");
    assert!(
        text.lines().all(|l| l.chars().count() < 300),
        "a long line in --context output must be capped like the default path: {} chars: {text}",
        text.lines().map(|l| l.chars().count()).max().unwrap_or(0),
    );
}

/// C-T6 review round 1, Important #1: `cli::search::run` and
/// `search::mod::open_vault` each independently compute the `globs_stamp`
/// they pass to `SearchIndex::open`. An earlier version of this code
/// computed `SourceGlobs::default().stamp()` directly at both call sites
/// instead of reading `opts.source_globs.stamp()` off the one `ScanOptions`
/// `search::options::for_vault` (the declared single construction point)
/// already built — harmless *today*, since both independently-computed
/// values and the real one are all the empty-string stopgap, but a mutation
/// that only breaks the CLI's version of that computation (e.g. hardcoding
/// a wrong constant) is invisible to every other test in this file: each
/// invocation here is a fresh process re-deriving the *same* wrong constant
/// every time, so it never disagrees with itself, and `search_scan_options_
/// contract.rs`'s `the_cli_and_the_gui_build_options_through_one_function`
/// only pins that `ScanOptions.source_globs` (the input) is identical
/// between the two adapters, not that either adapter's `SearchIndex::open`
/// call actually reads it.
///
/// So this test seeds the index with the stamp computed *independently* —
/// in this test process, straight off `search::options::for_vault`, not
/// through the CLI binary at all — and then proves the CLI binary's own
/// internal computation agrees with it. The signal is a phantom row:
/// `ensure_built` only rebuilds an index whose `files` table is empty. If
/// `SearchIndex::open` (inside the CLI process) reports the seeded stamp as
/// current (`Opened::Ready`), the phantom row — for a file deleted from disk
/// *after* seeding — survives a `--no-sweep` query untouched. If the CLI's
/// own stamp disagrees with the seeded one (`Opened::StaleContents`),
/// `rebuild_in_place` empties `files` before `ensure_built` runs, which then
/// rebuilds from the vault as it exists now — minus the deleted file — so
/// the phantom row, and the hit for it, are gone. A plain row-count
/// comparison could not distinguish these two outcomes (both end up with the
/// same final count for an otherwise-unchanged vault); "does the query still
/// find a file that no longer exists on disk" is the part that can.
///
/// `search::mod::open_vault` (the GUI side of this same contract) has no
/// equivalent black-box test — it needs a live `AppHandle`, which nothing in
/// this test suite constructs — so its coverage rests on it using the exact
/// same `opts.source_globs.stamp()` expression as `cli::search::run` (see
/// the matching comment at both call sites) plus the `ScanOptions` equality
/// this file's sibling test already pins.
#[test]
fn cli_glob_stamp_matches_independently_computed_scan_options_source_globs() {
    let v = vault(&[("a.md", "brownfox\n")]);
    let home = temp_home();

    // Compute the db path and the expected stamp exactly the way
    // `cli::search::run` is supposed to, then seed the index directly
    // through the library — never through the CLI binary — so a CLI-side
    // bug in deriving the stamp cannot also corrupt the baseline this test
    // compares against. `searchidx::paths::index_db_path` (which
    // `SearchIndex::open` calls internally) is a pure function of
    // `dirs::data_local_dir()` (driven by `HOME`/`LOCALAPPDATA`, per
    // `isolate`'s doc comment above) and the vault path, so setting the same
    // env here that `isolate` will set on the child process below makes
    // `SearchIndex::open` in THIS process land on the exact db file the CLI
    // subprocess will later open. Goes through `with_process_home` (rather
    // than open-coding save/set/restore here) so this override and
    // `attention_minutes_reflects_real_ingested_data_not_a_hardcoded_stand_in`'s
    // never race each other for `HOME` — see that helper's doc comment.
    with_process_home(&home, || {
        let opts = notemd_lib::search::options::for_vault(v.path());
        let expected_stamp = opts.source_globs.stamp();
        let mut idx = searchidx::SearchIndex::open(v.path(), &expected_stamp).expect("seed open");
        idx.rebuild(&opts).expect("seed build");
    });

    // The phantom-row probe: delete the indexed file from disk *after*
    // seeding, so only an unwarranted rebuild can make the hit disappear.
    std::fs::remove_file(v.path().join("a.md")).unwrap();

    let mut query = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    query.arg("--cli").arg("search").arg("brownfox").arg("--no-sweep").arg("--vault").arg(v.path());
    isolate(&mut query, &home);
    let out = query.output().unwrap();
    let _ = std::fs::remove_dir_all(&home);

    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("a.md:1:"),
        "the CLI's own glob stamp must match the independently-computed \
         opts.source_globs.stamp() this index was seeded with — a mismatch \
         would have rebuilt the index against the now-file-less vault and \
         lost this phantom row: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Review round 2, item 1: the exact "resolve vs use" gap round 1 closed on
/// the GUI side (`search::mod::search_locked` calling `idx.search_with`,
/// which ranks with `Weights::default()` unconditionally, while
/// `weights_for_vault` sat unused) was still open on the CLI side —
/// `cli::search::weights_for` existed and was contract-tested for GUI/CLI
/// parity, but nothing proved `notemd search` itself (`cli::search::run`)
/// actually rendered its result order with the resolved value rather than
/// the shipped defaults. Drives the real compiled binary — `notemd search`'s
/// actual entry point, the one agents invoke — with a real
/// `.notemd/settings.json`, and asserts a configured, inverted weight
/// reorders `--json` output.
#[test]
fn a_configured_weight_changes_the_clis_own_result_order() {
    let v = vault(&[
        ("derived.md", "---\ntype: Answer\n---\n\nwidget\n"),
        ("raw/source.md", "widget\n"),
    ]);
    std::fs::create_dir_all(v.path().join(".notemd")).unwrap();
    std::fs::write(v.path().join(".notemd/settings.json"), r#"{"searchSourceGlobs": ["raw/**"]}"#).unwrap();

    // Default weights: `derived.md` (Origin::Derived, x1.0) outranks
    // `raw/source.md` (Origin::Source, x0.9).
    let default_out = search(v.path(), &["widget", "--json"]);
    let default_json: serde_json::Value = serde_json::from_slice(&default_out.stdout)
        .unwrap_or_else(|e| panic!("invalid json ({e}): {}", String::from_utf8_lossy(&default_out.stdout)));
    assert_eq!(
        default_json["hits"][0]["path"].as_str(),
        Some("derived.md"),
        "默认权重下 derived 应排在 source 前面 —— 测试前提不成立: {default_json}"
    );

    // Invert the configured weights so `source` dominates `derived`.
    std::fs::write(
        v.path().join(".notemd/settings.json"),
        r#"{"searchSourceGlobs": ["raw/**"], "searchWeights": {"source": 5.0, "derived": 0.1}}"#,
    )
    .unwrap();
    let inverted_out = search(v.path(), &["widget", "--json"]);
    let inverted_json: serde_json::Value = serde_json::from_slice(&inverted_out.stdout)
        .unwrap_or_else(|e| panic!("invalid json ({e}): {}", String::from_utf8_lossy(&inverted_out.stdout)));
    assert_eq!(
        inverted_json["hits"][0]["path"].as_str(),
        Some("raw/source.md"),
        "配置的反转权重必须真正改变 CLI 自己的排序,而不是被 Weights::default() 悄悄吃掉: {inverted_json}"
    );
}

/// Fix round 2 (regression review of the `took_ms` fix in `cli::search::run`):
/// the earlier fix's own unit test (`cli::search::tests::
/// execute_took_ms_excludes_time_spent_before_the_call`) only proves
/// `execute()`'s own timer is query-only in isolation. It cannot catch the
/// actual bug that shipped — `run()` wiring `print_json` to
/// `outcome.took_ms` instead of its own whole-pipeline `started.elapsed()` —
/// because that unit test never calls `run()` or reads what the CLI prints.
/// If someone re-wires that call site back to the query-only value, that
/// unit test still passes, and so does `json_output_carries_the_full_
/// contract` above (it only asserts `took_ms.is_number()`). Only observing
/// the actual `--json` output of the actual binary closes that gap.
///
/// Two invocations, same isolated `HOME` so the second reuses the index the
/// first built:
///  - `--rebuild` against ~300 freshly-generated files: forces a full
///    rebuild, whose cost sits INSIDE the pipeline window (between `run()`'s
///    `started` and `execute()` returning) but OUTSIDE `execute()`'s own
///    query-only window.
///  - `--no-sweep` immediately after, same vault, same warm index: pipeline
///    overhead is close to zero (`ensure_built` sees an up-to-date stamp and
///    does nothing before the query runs).
///
/// Asserts `rebuild_took > 3 * warm_took`, not just `rebuild_took >
/// warm_took` — verified by hand (temporarily re-wiring `run()` to the
/// query-only value and running this test repeatedly) that a plain `>` is
/// NOT a reliable pin: under the regression both numbers collapse to
/// noise-level query-only timings (observed ~2-3ms either side, occasionally
/// `rebuild_took` edges out `warm_took` by chance even though neither
/// measured the rebuild at all), while the correct code's real margin is an
/// order of magnitude (observed ~60ms vs ~3ms, a ~20x ratio) — a ratio
/// comfortably below that real margin but well above the regression's noise
/// ceiling is the actual structural signal, not the bare inequality. This is
/// still a *relative* comparison, not a tuned millisecond constant: it scales
/// with however fast or slow the machine is.
#[test]
fn json_took_ms_reflects_the_whole_pipeline_not_just_the_query() {
    let files: Vec<(String, String)> =
        (0..300).map(|i| (format!("f{i:04}.md"), format!("# Note {i}\n\nwidget content number {i}\n"))).collect();
    let refs: Vec<(&str, &str)> = files.iter().map(|(p, b)| (p.as_str(), b.as_str())).collect();
    let v = vault(&refs);
    let home = temp_home();

    let mut rebuild_cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    rebuild_cmd
        .arg("--cli")
        .arg("search")
        .arg("widget")
        .arg("--rebuild")
        .arg("--json")
        .arg("--vault")
        .arg(v.path());
    isolate(&mut rebuild_cmd, &home);
    let rebuild_out = rebuild_cmd.output().unwrap();
    assert_eq!(
        rebuild_out.status.code(), Some(0),
        "sanity: --rebuild must succeed; stderr: {}", String::from_utf8_lossy(&rebuild_out.stderr),
    );
    let rebuild_json: serde_json::Value = serde_json::from_slice(&rebuild_out.stdout)
        .unwrap_or_else(|e| panic!("invalid json ({e}): {}", String::from_utf8_lossy(&rebuild_out.stdout)));
    let rebuild_took = rebuild_json["took_ms"].as_u64().expect("took_ms must be a number");

    let mut warm_cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    warm_cmd.arg("--cli").arg("search").arg("widget").arg("--no-sweep").arg("--json").arg("--vault").arg(v.path());
    isolate(&mut warm_cmd, &home);
    let warm_out = warm_cmd.output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(
        warm_out.status.code(), Some(0),
        "sanity: the warm --no-sweep query must succeed; stderr: {}", String::from_utf8_lossy(&warm_out.stderr),
    );
    let warm_json: serde_json::Value = serde_json::from_slice(&warm_out.stdout)
        .unwrap_or_else(|e| panic!("invalid json ({e}): {}", String::from_utf8_lossy(&warm_out.stdout)));
    let warm_took = warm_json["took_ms"].as_u64().expect("took_ms must be a number");

    // `.max(1)`: a genuinely-zero warm_took must not make the ratio bound
    // vacuous (anything would clear `> 3 * 0`); floor it at 1ms instead.
    assert!(
        rebuild_took > warm_took.max(1) * 3,
        "a full --rebuild across {} freshly-created files ({rebuild_took}ms) must report a took_ms \
         well above (>3x) an immediately-following --no-sweep query against the same, already-built, \
         warm index ({warm_took}ms) — if took_ms ever degenerates to query-only timing the two \
         collapse to the same noise-level magnitude instead of this order-of-magnitude gap",
        files.len(),
    );
}
