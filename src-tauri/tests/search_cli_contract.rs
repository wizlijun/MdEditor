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
    for key in ["path", "line", "text", "score", "breadcrumb", "doc_date", "source_ref"] {
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
/// frontmatter-less file the indexed path reports `source` for (rule 6:
/// "a bare `.md` with no frontmatter... judges source, not derived" —
/// `origin.rs`'s own doc comment). `fallback_scan` has `opts.sync_dir`
/// available (plumbed for this exact purpose) but no `Frontmatter`, so it
/// must parse one itself via the same `origin::derive` the indexed path uses
/// — not merely document the divergence.
#[test]
fn the_no_index_fallback_reports_the_same_origin_tier_the_index_would() {
    let v = vault(&[("a.md", "brownfox\n")]);
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
    assert_eq!(
        j["hits"][0]["origin"].as_str(),
        Some("unlabeled"),
        "a frontmatter-less .md must classify as unlabeled (rule 6′) on the no-index fallback, \
         the same as the indexed path — not the Derived fallback: {j}"
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
    ]);
    let out = search(v.path(), &["--stats", "--json"]);
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();

    assert_eq!(j["origin_counts"]["human"].as_i64(), Some(1), "{j}");
    assert_eq!(j["origin_counts"]["derived"].as_i64(), Some(2), "{j}");
    assert_eq!(j["origin_counts"]["source"].as_i64(), Some(1), "{j}");
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
