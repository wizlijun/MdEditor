//! `notemd search` — the zero-token retrieval surface.
//!
//! Shaped like grep on purpose (design spec §5): `path:line:text`, exit code 0
//! for hits / 1 for none / 2 for a real error. Claude Code, Codex and friends
//! internalized Unix conventions from their training data, so the friendliest
//! interface is the one that already looks like a tool they know. We are
//! accelerating the loop they already run, not asking them to learn ours.
//!
//! Nothing here decides *what* matches or *how it ranks* — that all lives in
//! `searchidx`, so the CLI and the UI cannot disagree.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use searchidx::{Limits, ScanOptions, SearchIndex, SkippedFile};

/// The CLI's freshness sweep is bounded: retrieval must never block its caller.
const SWEEP_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default)]
pub struct SearchArgs {
    pub query: Vec<String>,
    pub vault: Option<String>,
    pub limit: usize,
    pub json: bool,
    pub context: usize,
    pub no_sweep: bool,
    pub rebuild: bool,
    pub stats: bool,
}

impl SearchArgs {
    pub fn with_global_json(mut self, global: bool) -> Self {
        self.json = self.json || global;
        self
    }
}

/// Flags map onto the same filter syntax the UI uses: `--tag x` is sugar for
/// `tag:x`, so there is one grammar to learn and one parser to maintain
/// (`searchidx::query::parse` is the only place that interprets it).
///
/// Written as a plain `match` + `if let Some(v) = rest.get(i + 1) { …; i += 1; }`
/// per flag rather than a shared closure: a closure taking `&mut SearchArgs`
/// would need to simultaneously borrow `a` (to mutate) and `i` (to advance),
/// which does not borrow-check.
pub fn parse_args(rest: &[String], json_global: bool) -> SearchArgs {
    let mut a = SearchArgs { limit: 20, json: json_global, ..Default::default() };
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "--json" => a.json = true,
            "--no-sweep" => a.no_sweep = true,
            "--rebuild" => a.rebuild = true,
            "--stats" => a.stats = true,
            "--vault" => {
                if let Some(v) = rest.get(i + 1) {
                    a.vault = Some(v.clone());
                    i += 1;
                }
            }
            "--limit" => {
                if let Some(v) = rest.get(i + 1) {
                    a.limit = v.parse().unwrap_or(20);
                    i += 1;
                }
            }
            "--context" => {
                if let Some(v) = rest.get(i + 1) {
                    a.context = v.parse().unwrap_or(0);
                    i += 1;
                }
            }
            "--tag" => {
                if let Some(v) = rest.get(i + 1) {
                    a.query.push(format!("tag:{v}"));
                    i += 1;
                }
            }
            "--type" => {
                if let Some(v) = rest.get(i + 1) {
                    a.query.push(format!("type:{v}"));
                    i += 1;
                }
            }
            "--path" => {
                if let Some(v) = rest.get(i + 1) {
                    a.query.push(format!("path:{v}"));
                    i += 1;
                }
            }
            "--ext" => {
                if let Some(v) = rest.get(i + 1) {
                    a.query.push(format!("ext:{v}"));
                    i += 1;
                }
            }
            "--after" => {
                if let Some(v) = rest.get(i + 1) {
                    a.query.push(format!("after:{v}"));
                    i += 1;
                }
            }
            "--before" => {
                if let Some(v) = rest.get(i + 1) {
                    a.query.push(format!("before:{v}"));
                    i += 1;
                }
            }
            other => a.query.push(other.to_string()),
        }
        i += 1;
    }
    a
}

/// Headless vault-root resolution.
///
/// `sotvault::resolve_vault_root` needs an `AppHandle`, which the CLI does not
/// have, so this reads the shared config directly — the same file the GUI
/// writes. `--vault` is a first-class flag rather than a debugging aid: it is
/// the escape hatch when config resolution is wrong on a given machine.
pub fn resolve_vault_root(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(v) = explicit {
        return Some(PathBuf::from(v));
    }
    let cfg_path = crate::shared_config::config_path().ok()?;
    let cfg = crate::shared_config::read(&cfg_path).ok()?;
    cfg.sotvault.filter(|s| !s.is_empty()).map(PathBuf::from)
}

pub fn run(args: SearchArgs) -> ExitCode {
    let Some(root) = resolve_vault_root(args.vault.as_deref()) else {
        eprintln!("notemd: no vault configured. Set one in Preferences, or pass --vault PATH.");
        return ExitCode::from(2);
    };
    if !root.is_dir() {
        eprintln!("notemd: vault not found: {}", root.display());
        return ExitCode::from(2);
    }

    let started = std::time::Instant::now();
    let opts = scan_options_for(&root);
    let mut skipped_large: Vec<SkippedFile> = Vec::new();

    // The store's staleness stamp is `SourceGlobs::stamp()` (C-T6 repointed
    // it away from `sync_dir` — see `store::open`'s doc comment). Derived
    // from `opts.source_globs` — the field `scan_options_for` (a thin
    // delegate to `search::options::for_vault`, the declared single
    // construction point for `ScanOptions`) already populated above —
    // rather than computed independently. Review round 1: an earlier
    // version of this call independently recomputed `SourceGlobs::default()`
    // here (and in `search::mod::open_vault`), which would have gone
    // permanently inert the moment `for_vault` starts returning real
    // patterns (C-T8) unless *both* call sites were remembered and
    // repointed together — see `search::mod::open_vault`'s matching comment
    // for the full failure mode. Reading it off `opts` makes that drift
    // structurally impossible.
    let globs_stamp = opts.source_globs.stamp();
    // Every failure below degrades. The only hard error is "no vault".
    let mut index = match SearchIndex::open(&root, &globs_stamp) {
        Ok(i) => Some(i),
        Err(e) => {
            eprintln!("notemd: search index unavailable ({e}); scanning files directly");
            None
        }
    };

    if let Some(idx) = index.as_mut() {
        let outcome = if args.rebuild {
            idx.rebuild(&opts)
        } else if args.no_sweep {
            idx.ensure_built(&opts)
        } else {
            idx.ensure_built(&opts).and_then(|_| idx.sweep(&opts, Some(SWEEP_DEADLINE)))
        };
        match outcome {
            Ok(stats) => {
                skipped_large = stats.files_skipped_large.clone();
                if stats.timed_out {
                    eprintln!("notemd: freshness scan exceeded 2s; answering from the existing index");
                }
            }
            Err(e) => {
                eprintln!("notemd: index update failed ({e}); scanning files directly");
                index = None;
            }
        }
    }

    if args.stats {
        return report_stats(index.as_ref(), args.json, &skipped_large);
    }

    let query = args.query.join(" ");
    if query.trim().is_empty() {
        eprintln!("notemd: usage: notemd search <query...> [--vault PATH] [--limit N] [--json]");
        return ExitCode::from(2);
    }

    // Review round 1, Important 2: this used to call `i.search(...)`, which
    // ranks with `Weights::default()` unconditionally — the CLI's own
    // `weights_for` (added for the GUI/CLI parity contract test) had no
    // production caller, so a configured `searchWeights` never actually
    // reached a `notemd search` query. `weights_for` is the single
    // construction point (task C-T8) both adapters must go through.
    let weights = weights_for(&root);
    let conventions = conventions_for(&root);
    let (hits, route) = match index
        .as_ref()
        .map(|i| i.search_ranked(&query, args.limit, &Limits::full(), &weights, &conventions))
    {
        Some(Ok(a)) => (a.hits, a.route),
        Some(Err(e)) => {
            eprintln!("notemd: query failed ({e}); scanning files directly");
            (fallback_scan(&root, &query, args.limit, &opts), searchidx::Route::Scan)
        }
        None => (fallback_scan(&root, &query, args.limit, &opts), searchidx::Route::Scan),
    };

    let took = started.elapsed().as_millis();
    // Exit code must reflect what actually reached stdout, not what the index
    // believes exists — see `print_plain`: a `--context` hit whose recorded
    // line range no longer resolves against the on-disk file (e.g. the file
    // shrank between the freshness sweep and printing) is dropped rather than
    // printed, and a caller reading exit 0 as "there is output" must not be
    // lied to.
    let printed = if args.json {
        print_json(&query, route, took, &hits);
        hits.len()
    } else {
        print_plain(&root, &hits, args.context)
    };
    if printed == 0 { ExitCode::from(1) } else { ExitCode::from(0) }
}

/// Thin delegation to the single shared constructor
/// (`crate::search::options::for_vault`) — public, rather than inlined at the
/// one call site above, so `tests/search_scan_options_contract.rs` can call
/// the CLI's path directly and assert it is byte-for-byte the same
/// `ScanOptions` the GUI builds for the same vault.
pub fn scan_options_for(root: &Path) -> ScanOptions {
    crate::search::options::for_vault(root)
}

/// Same rationale as [`scan_options_for`], for the other single construction
/// point: `tests/search_scan_options_contract.rs` calls this to assert the
/// CLI resolves the identical `Weights` the GUI does for the same vault
/// (task C-T8). `run` above calls this too — its live query goes through
/// `SearchIndex::search_with_weights` with this value, not the
/// `Weights::default()`-only `SearchIndex::search`/`search_with` facades.
pub fn weights_for(root: &Path) -> searchidx::query::Weights {
    crate::search::options::weights_for_vault(root)
}

/// The third single construction point, same rationale again: `wikipageDir`
/// decides which note (if any) `run` above pins to the top, and the CLI must
/// resolve it through the GUI's function rather than reading the setting
/// itself. `tests/search_scan_options_contract.rs` asserts the two agree.
pub fn conventions_for(root: &Path) -> searchidx::query::Conventions {
    crate::search::options::conventions_for_vault(root)
}

/// Last-ditch retrieval with no index at all: walk the vault and substring-match.
/// Slower and unranked, but the caller gets an answer instead of an excuse.
///
/// The walker comes from `searchidx::scan::walk_builder` — the same one the
/// index itself uses — rather than being configured here. Built locally with
/// `ignore`'s defaults it honoured `.gitignore`/`.ignore`/global excludes,
/// and since note.md vaults are git repositories that meant the fallback
/// searched a *different corpus* than the index: a `.gitignore`d note was
/// found by one and invisible to the other, with nothing in the output to
/// say so. Same walker, same `is_indexable`, one corpus.
fn fallback_scan(root: &Path, query: &str, limit: usize, opts: &ScanOptions) -> Vec<searchidx::Hit> {
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    for entry in searchidx::scan::walk_builder(root).build().flatten() {
        if out.len() >= limit {
            break;
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Some(rel) = searchidx::norm::rel_path(root, entry.path()) else { continue };
        if !searchidx::scan::is_indexable(&rel, opts) {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(entry.path()) else { continue };
        let text = searchidx::norm::strip_cr(&raw);
        // Task 6 made `origin` observable in `--json` for the first time on
        // this path (score stays 0.0 here, so `score_of` never reads it, but
        // the CLI now prints it directly). A hardcoded `Origin::Derived` used
        // to be silently fine because of that — it stopped being fine the
        // moment this became visible output: it would report `derived` for
        // exactly the frontmatter-less files the indexed path reports
        // `unlabeled` for (rule 6′). Derive it for real, with the same inputs
        // `chunk::parse_file` uses on the indexed path, and `fm_present` must
        // be captured before `unwrap_or_default()` collapses "no frontmatter"
        // and "empty frontmatter" into the same value (see `origin::derive`'s
        // own doc comment on why `Some(&Frontmatter::default())` is not `None`).
        //
        // Forwards `&opts.source_globs` — the same field `is_indexable`
        // above already consulted to decide this file was in scope at all —
        // so this fallback path classifies identically to the indexed path
        // (`searchidx::scan::index_into`). As of C-T8, `opts.source_globs`
        // is the vault's real configured patterns (resolved once by
        // `for_vault`/`scan_options_for` above, not read again here), so
        // this agreement is no longer a "both stopgaps happen to match"
        // coincidence — it is the actual, current classification.
        // Only markdown has frontmatter. A `.txt`/`.srt`/`.vtt` that happens
        // to open with `---` is content, not a header — `chunk::parse_file`
        // gates the split the same way, and skipping this gate here made the
        // two paths disagree: a `.txt` whose pseudo-frontmatter said
        // `type: Book Summary` classified `derived` on this path and `source`
        // on the indexed one.
        let is_markdown = !(rel.to_lowercase().ends_with(".srt")
            || rel.to_lowercase().ends_with(".vtt")
            || rel.to_lowercase().ends_with(".txt"));
        let fm_raw = if is_markdown { searchidx::frontmatter::split(&text).0 } else { None };
        let fm_present = fm_raw.is_some();
        let fm = fm_raw.map(searchidx::frontmatter::parse).unwrap_or_default();
        let origin = searchidx::origin::derive(&rel, fm_present.then_some(&fm), &opts.source_globs);
        for (i, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                out.push(searchidx::Hit {
                    path: rel.clone(),
                    line: i as u32 + 1,
                    line_end: i as u32 + 1,
                    text: line.trim().to_string(),
                    breadcrumb: String::new(),
                    level: "line".into(),
                    score: 0.0,
                    doc_date: None,
                    agent_by: None,
                    human_verified: false,
                    origin,
                    concept_type: fm.concept_type.clone(),
                    // No index, no pinning: this path exists for the case
                    // where the index is unavailable, and it has no
                    // `files.title` to compare a name against. Reporting
                    // `false` is the honest answer — inventing a pin from
                    // the path alone would make the no-index fallback rank
                    // *differently* from the indexed path it stands in for.
                    pinned: false,
                    // 同理:没有索引就没有 `doc_attention` 表可查。0.0 是诚实
                    // 的答案(`attention::boost` 对 0 严格返回 1.0,即不加成),
                    // 而不是把上一次索引里的陈旧分钟数搬过来。
                    attention_minutes: 0.0,
                });
                break;
            }
        }
    }
    out
}

/// Prints hits grep-style and returns how many *lines* actually reached
/// stdout — the caller's exit code follows this, not `hits.len()`, precisely
/// because a `--context` hit can resolve to nothing (see `context_lines`).
fn print_plain(root: &Path, hits: &[searchidx::Hit], context: usize) -> usize {
    let mut printed = 0usize;
    for h in hits {
        if context > 0 {
            let Some(lines) = context_lines(root, h, context) else {
                // The file no longer has this hit's recorded line range (it
                // shrank, or vanished, since the hit was indexed). A stale
                // citation is worse than a dropped one — see `context_lines`.
                continue;
            };
            for (n, text) in lines {
                println!("{}:{}:{}", h.path, n, one_line(&text));
                printed += 1;
            }
        } else {
            println!("{}:{}:{}", h.path, h.line, one_line(&h.text));
            printed += 1;
        }
    }
    printed
}

/// `None` when the hit's line range can no longer be honestly resolved
/// against the *current* on-disk file — unreadable/gone, or (the case a
/// `--context` request can hit that a plain hit never does, since only this
/// path re-reads the file) the file has shrunk since the hit was indexed, so
/// `hit.line`/`hit.line_end` point past its current end. That happens for real
/// in the ordinary window between a freshness sweep and printing: an edit can
/// land in between. Printing stale line numbers there would be a wrong
/// citation, which is worse than silently having fewer results, so the
/// caller drops the hit instead — see `print_plain`.
fn context_lines(root: &Path, hit: &searchidx::Hit, context: usize) -> Option<Vec<(u32, String)>> {
    let raw = std::fs::read_to_string(root.join(&hit.path)).ok()?;
    let text = searchidx::norm::strip_cr(&raw);
    let lines: Vec<&str> = text.lines().collect();
    // Review round 2: a non-empty *clamped window* is not the same claim as
    // "this hit still exists". `from` is clamped down by `.max(1)`, so once
    // `context` is large enough it can land on a line that genuinely exists
    // even though `hit.line` itself is long gone — that used to print
    // unrelated lines under the original hit's path/line as if they were its
    // context. The hit's own start line must still be inside the current
    // file; only *that* line has to be checked this way — clamping
    // `hit.line_end` down to the current file length below is the ordinary
    // end-of-file case (a hit near the end of an unchanged, or merely
    // trimmed-at-the-end, file) and must keep working.
    if hit.line as usize > lines.len() {
        return None;
    }
    let from = hit.line.saturating_sub(context as u32).max(1);
    let to = (hit.line_end as usize + context).min(lines.len()) as u32;
    if from > to {
        return None;
    }
    let out: Vec<(u32, String)> =
        (from..=to).filter_map(|n| lines.get(n as usize - 1).map(|l| (n, l.trim().to_string()))).collect();
    (!out.is_empty()).then_some(out)
}

/// Collapse a multi-line block to one grep-shaped line, capped so a long
/// paragraph cannot flood an agent's context.
fn one_line(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= 200 {
        joined
    } else {
        joined.chars().take(200).collect::<String>() + "…"
    }
}

fn print_json(query: &str, route: searchidx::Route, took_ms: u128, hits: &[searchidx::Hit]) {
    let arr: Vec<serde_json::Value> = hits
        .iter()
        .map(|h| {
            serde_json::json!({
                "path": h.path,
                "line": h.line,
                "line_end": h.line_end,
                "text": h.text,
                "score": h.score,
                "breadcrumb": h.breadcrumb,
                "level": h.level,
                "doc_date": h.doc_date,
                "source_ref": h.source_ref(),
                // Surfaced so an agent can prefer primary sources over
                // AI-authored summaries of them (design spec §5-T3).
                "provenance": { "agent_by": h.agent_by, "human_verified": h.human_verified },
                // Task 6: the tier `origin::derive` classified this file into
                // (`"human"`/`"derived"`/`"source"`), alongside — not inside —
                // `provenance`: `provenance` is per-document signals read from
                // this file's own frontmatter, `origin` is the category-level
                // tier `score_of` actually ranks on (see its doc comment on
                // why the two are independent, not double-counted).
                "origin": h.origin.as_str(),
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({
            "query": query, "route": route.as_str(), "took_ms": took_ms,
            "total": hits.len(), "hits": arr
        })
    );
}

fn report_stats(index: Option<&SearchIndex>, json: bool, skipped: &[SkippedFile]) -> ExitCode {
    let Some(idx) = index else {
        eprintln!("notemd: no index available");
        return ExitCode::from(1);
    };
    match idx.stats() {
        Ok(s) if json => {
            println!(
                "{}",
                serde_json::json!({
                    "files": s.files, "blocks": s.blocks, "db_bytes": s.db_bytes,
                    "built_at": s.built_at, "tokenizer_id": s.tokenizer_id,
                    // Design spec §5.1's rationale for the per-hit `origin`
                    // field ("agent 可据此自行分层") applies to the corpus as a
                    // whole too, and `stats()` computes both of these on every
                    // call regardless — dropping them here was pure loss.
                    // snake_case like every other key in this payload, not the
                    // GUI DTO's camelCase.
                    "origin_counts": {
                        "human": s.origin_counts.human,
                        "derived": s.origin_counts.derived,
                        "source": s.origin_counts.source,
                        // C-T11: `unlabeled` used to be silently absent from
                        // this payload (the same known undercount GUI stats
                        // carried — see `searchidx::origin_counts`'s doc
                        // comment); now real, so an agent scripting off
                        // `notemd search --stats --json` sees the same four
                        // tiers the settings page does.
                        "unlabeled": s.origin_counts.unlabeled,
                    },
                    "type_counts": s.type_counts,
                })
            );
            ExitCode::from(0)
        }
        Ok(s) => {
            println!("files      {}", s.files);
            println!("blocks     {}", s.blocks);
            println!("db size    {:.1} MB", s.db_bytes as f64 / 1_048_576.0);
            println!("tokenizer  {}", s.tokenizer_id);
            // Spec §3.7/§9: a file skipped by the size guardrail is invisible to
            // search, so `--stats` has to say so — an unexplained miss is worse
            // than a slow query. Size is included (not just the path) so the
            // user can judge at a glance whether raising the threshold is
            // reasonable.
            for f in skipped {
                println!(
                    "skipped    {} ({:.1} MB, over the size threshold; rg still finds it)",
                    f.path,
                    f.size as f64 / 1_048_576.0
                );
            }
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("notemd: {e}");
            ExitCode::from(1)
        }
    }
}
