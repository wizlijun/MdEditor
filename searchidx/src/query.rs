//! Query parsing, retrieval and ranking. The UI and the CLI both call `parse`
//! and `search`, so a filter that works in one works in the other by
//! construction.

use std::sync::Arc;

use rusqlite::{params_from_iter, Connection};

use crate::origin::Origin;
use crate::tokenize::{has_han, tokens};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Query {
    pub terms: Vec<String>,
    pub phrases: Vec<String>,
    pub tags: Vec<String>,
    pub types: Vec<String>,
    pub paths: Vec<String>,
    pub pages: Vec<String>,
    pub exts: Vec<String>,
    pub origins: Vec<String>,
    pub after: Option<String>,
    pub before: Option<String>,
    pub raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Fts,
    Scan,
}

impl Route {
    pub fn as_str(self) -> &'static str {
        match self {
            Route::Fts => "t1-fts",
            Route::Scan => "t1-scan",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub path: String,
    pub line: u32,
    pub line_end: u32,
    pub text: String,
    pub breadcrumb: String,
    pub level: String,
    pub score: f64,
    pub doc_date: Option<String>,
    pub agent_by: Option<String>,
    pub human_verified: bool,
    pub origin: Origin,
    /// `files.concept_type` verbatim (frontmatter `type`), e.g. `"Book
    /// Summary"`. `None` when the file has no `type` at all. Carried through
    /// purely for UI grouping (task B-T7, design spec §5) — ranking never
    /// reads it, `origin` already encodes what ranking needs.
    pub concept_type: Option<String>,
}

impl Hit {
    /// The back-to-source anchor handed to agents, e.g. `docs/a.md#L120`.
    pub fn source_ref(&self) -> String {
        format!("{}#L{}", self.path, self.line)
    }
}

pub fn parse(raw: &str) -> Query {
    let mut q = Query { raw: raw.to_string(), ..Default::default() };
    for token in split_respecting_quotes(raw) {
        if let Some(rest) = token.strip_prefix('"') {
            // Only a *closed* quote makes a phrase; an unterminated one is far
            // more likely a typo than an intent, so it degrades to a term.
            if let Some(inner) = rest.strip_suffix('"') {
                if !inner.trim().is_empty() {
                    q.phrases.push(inner.trim().to_string());
                    continue;
                }
            }
            push_terms(&mut q, rest.trim_matches('"'));
            continue;
        }
        match token.split_once(':') {
            Some(("tag", v)) if !v.is_empty() => q.tags.push(v.to_string()),
            Some(("type", v)) if !v.is_empty() => q.types.push(v.to_string()),
            Some(("path", v)) if !v.is_empty() => q.paths.push(v.to_string()),
            Some(("ext", v)) if !v.is_empty() => q.exts.push(v.trim_start_matches('.').to_string()),
            // Unvalidated on purpose, like every other filter value here — an
            // unrecognized tier (`origin:bogus`) is bound literally into the
            // SQL by `push_filters` below and fails closed (matches zero
            // rows) rather than erroring or being silently dropped. See the
            // `an_unrecognized_origin_value_matches_nothing_not_everything`
            // test for the reasoning.
            Some(("origin", v)) if !v.is_empty() => q.origins.push(v.to_string()),
            Some(("after", v)) if !v.is_empty() => q.after = Some(v.to_string()),
            Some(("before", v)) if !v.is_empty() => q.before = Some(v.to_string()),
            Some(("page", v)) if !v.is_empty() => {
                q.pages.push(v.trim_start_matches("[[").trim_end_matches("]]").to_string())
            }
            _ => push_terms(&mut q, &token),
        }
    }
    q
}

fn push_terms(q: &mut Query, raw: &str) {
    let t = raw.trim();
    if !t.is_empty() {
        q.terms.push(t.to_string());
    }
}

fn split_respecting_quotes(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in raw.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(c);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Cooperative abort, consulted from SQLite's own progress callback.
///
/// It has to be checked *inside* a running statement, not between rows: the
/// expensive path here is the LIKE fallback, and on a miss it spends its
/// entire runtime inside a single `step()` over every block in the vault
/// without ever handing a row back to us. A between-rows check would never
/// run. Measured on a real vault (8.9k files / 1.3M blocks): 14.3s in one
/// `step()`.
pub type Abort = Arc<dyn Fn() -> bool + Send + Sync>;

/// What a caller is willing to spend on one query.
#[derive(Clone, Default)]
pub struct Limits {
    /// Allow the bounded LIKE fallback when FTS misses. `false` keeps the
    /// query on the fast path only — what live typing wants, since the
    /// fallback costs seconds on a large vault and a half-typed word misses
    /// FTS almost by definition.
    pub deep: bool,
    /// Cut the query short when this returns true. `None` = run to completion.
    pub abort: Option<Abort>,
}

impl Limits {
    /// Every route, no abort — what the CLI and every non-interactive caller
    /// want, and what `search` has always done.
    pub fn full() -> Self {
        Limits { deep: true, abort: None }
    }
}

/// One query's answer. Carries *why* it looks the way it does, so the caller
/// can tell "nothing matches" from "we did not look everywhere yet" — two
/// states that must never render as the same "no results".
#[derive(Debug, Clone)]
pub struct Answer {
    pub hits: Vec<Hit>,
    pub route: Route,
    /// The retrieval was cut short by [`Limits::abort`]. `hits` is whatever
    /// had been collected by then, not the whole answer.
    pub truncated: bool,
    /// FTS found nothing and a LIKE fallback *would* have been tried, but
    /// [`Limits::deep`] was false. The one honest way to offer "search harder"
    /// without paying for it on every keystroke.
    pub deep_available: bool,
}

/// Back-compatible entry point: every route, no abort.
pub fn search(
    conn: &Connection,
    q: &Query,
    limit: usize,
    today: &str,
) -> rusqlite::Result<(Vec<Hit>, Route)> {
    let a = search_with(conn, q, limit, today, &Limits::full())?;
    Ok((a.hits, a.route))
}

pub fn search_with(
    conn: &Connection,
    q: &Query,
    limit: usize,
    today: &str,
    limits: &Limits,
) -> rusqlite::Result<Answer> {
    // Installed for the whole call and removed on every exit path (including
    // `?`) by the guard's Drop — a progress handler left behind on this
    // connection would abort the *next* caller's work, and the next caller is
    // usually the watcher's sweep or a rebuild.
    let _guard = ProgressGuard::install(conn, limits.abort.clone())?;

    let (hits, truncated) = fts_search(conn, q, limit, today)?;
    if !hits.is_empty() || truncated {
        return Ok(Answer { hits, route: Route::Fts, truncated, deep_available: false });
    }
    // The dictionary has blind spots — new coinages, personal names, single
    // characters. A miss there would be invisible to the user, so we pay for a
    // bounded LIKE scan rather than report "no results". The corpus is bounded
    // and the scan is capped, which is why the usual "never full-scan" rule is
    // suspended here on purpose.
    if needs_scan_fallback(q) {
        if !limits.deep {
            return Ok(Answer {
                hits: Vec::new(),
                route: Route::Fts,
                truncated: false,
                deep_available: true,
            });
        }
        // Report `Route::Scan` regardless of whether the scan itself found
        // anything: the route records which retrieval path actually ran, not
        // whether it succeeded. Collapsing an attempted-but-empty scan back
        // into `Route::Fts` would tell a caller (the CLI's `--json` output,
        // read by agents deciding whether a query was exhaustively tried)
        // that no fallback was attempted when one was.
        let (hits, truncated) = like_search(conn, q, limit, today)?;
        return Ok(Answer { hits, route: Route::Scan, truncated, deep_available: false });
    }
    Ok(Answer { hits: Vec::new(), route: Route::Fts, truncated: false, deep_available: false })
}

/// How many SQLite VM instructions between abort checks. Small enough that a
/// superseded scan dies in well under a frame, large enough that the callback
/// is noise next to the scan itself (~65k calls across a 14s full scan).
const PROGRESS_OPS: std::os::raw::c_int = 1000;

struct ProgressGuard<'c> {
    conn: &'c Connection,
    installed: bool,
}

impl<'c> ProgressGuard<'c> {
    fn install(conn: &'c Connection, abort: Option<Abort>) -> rusqlite::Result<Self> {
        let Some(abort) = abort else { return Ok(ProgressGuard { conn, installed: false }) };
        conn.progress_handler(PROGRESS_OPS, Some(move || abort()))?;
        Ok(ProgressGuard { conn, installed: true })
    }
}

impl Drop for ProgressGuard<'_> {
    fn drop(&mut self) {
        if self.installed {
            let _ = self.conn.progress_handler(0, None::<fn() -> bool>);
        }
    }
}

/// An abort is not a failure: SQLite reports it as `SQLITE_INTERRUPT`, and the
/// rows gathered before it are perfectly good partial results.
fn is_abort(e: &rusqlite::Error) -> bool {
    matches!(e, rusqlite::Error::SqliteFailure(err, _)
        if err.code == rusqlite::ErrorCode::OperationInterrupted)
}

fn needs_scan_fallback(q: &Query) -> bool {
    q.terms.iter().chain(q.phrases.iter()).any(|t| has_han(t) || t.chars().count() <= 2)
}

/// Build the FTS5 MATCH expression. Every term is emitted as a quoted string
/// literal, which is what neutralizes FTS5 operators (`OR`, `NEAR`, `*`, `^`)
/// arriving inside user input — an agent will hand us its query verbatim and a
/// syntax error would look like "no results".
fn match_expr(q: &Query) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for t in q.terms.iter().chain(q.phrases.iter()) {
        for tok in tokens(t) {
            parts.push(format!("\"{}\"", tok.replace('"', "\"\"")));
        }
    }
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

// Column order shared by `fts_search` and `like_search`; `row_to_hit` reads
// these back by POSITION (0-11). If this list changes, every index in
// `row_to_hit` and the two `r.get::<_, _>(N)` calls for `rank`/`is_annotation`
// below must change with it — see the module-level hazard note in the task
// brief. THIS IS THE SHARPEST EDGE IN THIS FILE: every column here is TEXT
// or INTEGER-ish, so a shifted index resolves via a nearby column's value
// with NO type error and NO test failure unless something explicitly pins
// that column's value round-tripping through a real index (see
// `a_hits_origin_round_trips_through_the_real_index` and
// `a_hits_concept_type_round_trips_through_the_real_index` below — both
// exist specifically to catch that).
//
// `f.origin` (index 10) is deliberately last of the "stable" columns, and
// `f.concept_type` (index 11) is appended AFTER it — not inserted in the
// middle — so neither this column's position nor any earlier one moves
// again if a future column is added. Both callers append their own `rank`
// column after everything here: index 12 in `fts_search`. `is_annotation`
// (index 9) is unchanged from before `origin`/`concept_type` were added.
const SELECT_COLS: &str = "f.path, b.line_start, b.line_end, b.text, b.breadcrumb, b.level, \
                           f.doc_date, b.agent_by, f.human_verified, b.is_annotation, f.origin, \
                           f.concept_type";

/// Drain a row iterator, treating an abort as "stop here" rather than an
/// error. Returns the rows collected and whether the drain was cut short.
fn drain<T>(
    rows: impl Iterator<Item = rusqlite::Result<T>>,
) -> rusqlite::Result<(Vec<T>, bool)> {
    let mut out = Vec::new();
    for r in rows {
        match r {
            Ok(v) => out.push(v),
            Err(e) if is_abort(&e) => return Ok((out, true)),
            Err(e) => return Err(e),
        }
    }
    Ok((out, false))
}

fn fts_search(
    conn: &Connection,
    q: &Query,
    limit: usize,
    today: &str,
) -> rusqlite::Result<(Vec<Hit>, bool)> {
    let Some(expr) = match_expr(q) else { return Ok((Vec::new(), false)) };
    let mut sql = format!(
        "SELECT {SELECT_COLS}, bm25(blocks_fts, 1.0, 2.0) AS rank
         FROM blocks_fts
         JOIN blocks b ON b.id = blocks_fts.rowid
         JOIN files f ON f.id = b.file_id
         WHERE blocks_fts MATCH ?1"
    );
    let mut args: Vec<String> = vec![expr];
    push_filters(q, &mut sql, &mut args);
    // Over-fetch: business boosts reorder, and a phrase recheck removes rows.
    sql.push_str(&format!(" ORDER BY rank ASC LIMIT {}", (limit * 8).max(64)));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok((row_to_hit(r)?, r.get::<_, f64>(12)?, r.get::<_, i64>(9)? != 0))
    })?;
    let (rows, truncated) = drain(rows)?;
    Ok((finish(rows, q, limit, today)?, truncated))
}

fn like_search(
    conn: &Connection,
    q: &Query,
    limit: usize,
    today: &str,
) -> rusqlite::Result<(Vec<Hit>, bool)> {
    // Every term and phrase must constrain the scan, ANDed — the same
    // contract the FTS path gives via `match_expr`. Binding only the first
    // needle (the original bug here) silently drops every other term, so a
    // two-term query like `target 慕` would return any file containing
    // EITHER word instead of both: a confident false positive, which is
    // worse than the miss this fallback exists to prevent (a miss at least
    // looks like "no results"; this looked like an answer).
    let needles: Vec<&str> = q.terms.iter().chain(q.phrases.iter()).map(String::as_str).collect();
    if needles.is_empty() {
        return Ok((Vec::new(), false));
    }
    let mut args: Vec<String> = Vec::new();
    let clauses: Vec<String> = needles
        .iter()
        .map(|n| {
            args.push(format!("%{}%", escape_like(n)));
            format!("b.text LIKE ?{} ESCAPE '\\'", args.len())
        })
        .collect();
    let mut sql = format!(
        "SELECT {SELECT_COLS}, 0.0 AS rank
         FROM blocks b JOIN files f ON f.id = b.file_id
         WHERE {}",
        clauses.join(" AND ")
    );
    push_filters(q, &mut sql, &mut args);
    // Hard cap: the fallback is a safety net, not a query plan.
    sql.push_str(" LIMIT 500");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok((row_to_hit(r)?, -1.0f64, r.get::<_, i64>(9)? != 0))
    })?;
    let (rows, truncated) = drain(rows)?;
    Ok((finish(rows, q, limit, today)?, truncated))
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

// Filters are appended to the WHERE clause AFTER the MATCH expression (in
// `fts_search`) or the LIKE needle (in `like_search`), both of which already
// occupy `?1`. `next` returns `args.len()` AFTER pushing, so the first filter
// value bound here lands at `?2`, and each subsequent one numbers
// consecutively — verified by hand against both call sites (see task report).
//
// Repetition semantics are decided PER FILTER, not by one blanket rule, and
// the choice is silent unless documented at each site (task 6 review round
// 1, Minor #3) — so: `tags`/`types`/`paths`/`exts` each emit one `AND ... =
// ?N` clause per pushed value, which ANDs repeats together (`type:Note
// type:Idea` matches nothing, same for `ext:`/`path:`). That is correct for
// `tags` (a file can carry several tags at once, so "must have tag A AND tag
// B" is a real, satisfiable query) and merely unexploited-but-harmless for
// the rest — nobody has asked for `type:x OR type:y`. `origin` breaks that
// pattern on purpose: see its own comment below for why ANDing repeats there
// would silently zero out every result instead of widening the match. If you
// add a new single-valued-per-file filter, default to the `tags`/`types`
// AND idiom UNLESS repeats are a plausible query shape (like `origin:`'s
// tier), in which case use the `IN (...)` idiom instead and say so.
fn push_filters(q: &Query, sql: &mut String, args: &mut Vec<String>) {
    let next = |args: &mut Vec<String>, v: String| {
        args.push(v);
        args.len()
    };
    for t in &q.tags {
        // tags_json is a JSON array; matching the quoted value avoids `a` also
        // matching `alpha`. The tag value itself must go through
        // `escape_like` + `ESCAPE '\'` just like `path:` below — an
        // unescaped `_` is a single-character SQL wildcard, so
        // `tag:in_progress` would otherwise also match a file tagged
        // `inXprogress`.
        let i = next(args, format!("%\"{}\"%", escape_like(t)));
        sql.push_str(&format!(" AND f.tags_json LIKE ?{i} ESCAPE '\\'"));
    }
    for t in &q.types {
        let i = next(args, t.clone());
        sql.push_str(&format!(" AND f.concept_type = ?{i}"));
    }
    for p in &q.paths {
        let i = next(args, format!("%{}%", escape_like(p)));
        sql.push_str(&format!(" AND f.path LIKE ?{i} ESCAPE '\\'"));
    }
    for e in &q.exts {
        let i = next(args, e.clone());
        sql.push_str(&format!(" AND f.ext = ?{i}"));
    }
    if !q.origins.is_empty() {
        // Unlike `types`/`exts`/`paths` above (one `AND ... = ?N` clause per
        // pushed value, which only makes sense when a filter can plausibly
        // be satisfied by more than one stored value at once, e.g. tags),
        // `origin` is a single-valued per-file column: a file cannot equal
        // two different origins simultaneously. ANDing multiple `origin:`
        // filters the way `types` does would make a second `origin:` filter
        // silently zero out every result instead of widening the match — see
        // `multiple_origin_filters_are_ored_not_anded`. `IN (...)` gives the
        // OR semantics repeating a single-valued filter should have, and
        // costs nothing when there is only one value (`IN (?N)` behaves like
        // `= ?N`).
        let placeholders: Vec<String> =
            q.origins.iter().map(|o| format!("?{}", next(args, o.clone()))).collect();
        sql.push_str(&format!(" AND f.origin IN ({})", placeholders.join(", ")));
    }
    for p in &q.pages {
        let i = next(args, p.clone());
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM links l WHERE l.file_id = f.id AND l.target = ?{i})"
        ));
    }
    if let Some(a) = &q.after {
        let i = next(args, a.clone());
        sql.push_str(&format!(" AND f.doc_date >= ?{i}"));
    }
    if let Some(b) = &q.before {
        let i = next(args, b.clone());
        sql.push_str(&format!(" AND f.doc_date <= ?{i}"));
    }
}

fn row_to_hit(r: &rusqlite::Row) -> rusqlite::Result<Hit> {
    // `Option<String>`, not `String`: the column is `NOT NULL` today (v1
    // databases with no `origin` column are wiped on open, see `store::open`),
    // but `store::origin_of`'s own doc comment promises a graceful `Derived`
    // fallback for a NULL row, not a hard `InvalidColumnType` error that
    // would fail the whole query. Reading it as non-nullable would make that
    // promise unreachable in practice and contradict fail-toward-neutral
    // with a fail-loud crash instead (review round 1, Minor #4).
    let origin_raw: Option<String> = r.get(10)?;
    // `f.concept_type` (index 11, appended after `f.origin` — see
    // `SELECT_COLS`'s comment). Genuinely nullable: a `.md` with no
    // frontmatter `type` at all stores `NULL` here, distinct from an empty
    // string, and that distinction matters to the grouping consumer (task
    // B-T7): a typeless `derived` hit falls into the UI's catch-all group.
    let concept_type: Option<String> = r.get(11)?;
    Ok(Hit {
        path: r.get(0)?,
        line: r.get::<_, i64>(1)? as u32,
        line_end: r.get::<_, i64>(2)? as u32,
        text: r.get(3)?,
        breadcrumb: r.get(4)?,
        level: r.get(5)?,
        doc_date: r.get(6)?,
        agent_by: r.get(7)?,
        human_verified: r.get::<_, i64>(8)? != 0,
        origin: crate::store::origin_of(origin_raw.as_deref()),
        concept_type,
        score: 0.0,
    })
}

fn finish(
    rows: Vec<(Hit, f64, bool)>,
    q: &Query,
    limit: usize,
    today: &str,
) -> rusqlite::Result<Vec<Hit>> {
    let mut out: Vec<Hit> = Vec::new();
    for (mut hit, rank, is_annotation) in rows {
        // A quoted phrase means "these words, in this order". The index stores
        // OVERLAPPING tokens, so FTS can only tell us the words are all present
        // — adjacency has to be rechecked against the stored text.
        let mut phrase_exact = false;
        if !q.phrases.is_empty() {
            let hay = hit.text.to_lowercase();
            if !q.phrases.iter().all(|p| hay.contains(&p.to_lowercase())) {
                continue;
            }
            phrase_exact = true;
        }
        hit.score = score_of(rank, &hit, is_annotation, phrase_exact, today);
        out.push(hit);
    }
    let mut out = drop_redundant_rollups(out);
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    Ok(out)
}

/// Multi-granularity indexing (design spec §3.3) means the same words can
/// match at Line, Section AND File resolution in the same file at once — a
/// one-paragraph note's Line block and its File-level rollup cover the exact
/// same lines, and a Section's rollup always engulfs every Line nested in
/// it. Surfacing all of them is not "more results", it is the same evidence
/// shown two or three times with the least specific copy often ranking
/// highest (the file/section business boost in `score_of` outweighs bm25's
/// length normalization on tiny documents). So: within one file, a hit whose
/// line range is fully covered by another hit's range is dropped in favor of
/// the covering hit — unless the ranges are identical, in which case the
/// FINER level (Line over Section over File) wins the tie. Hits over
/// disjoint or partially-overlapping ranges (e.g. two different paragraphs,
/// or two sibling sections) are untouched — this only collapses genuine
/// containment, never merges distinct evidence.
fn drop_redundant_rollups(hits: Vec<Hit>) -> Vec<Hit> {
    let n = hits.len();
    let mut removed = vec![false; n];
    for i in 0..n {
        for j in 0..n {
            if i == j || hits[i].path != hits[j].path {
                continue;
            }
            // Does i's range fully cover j's range?
            let engulfs = hits[i].line <= hits[j].line && hits[i].line_end >= hits[j].line_end;
            if !engulfs {
                continue;
            }
            let same_range = hits[i].line == hits[j].line && hits[i].line_end == hits[j].line_end;
            let i_is_coarser_level = level_rank(&hits[i].level) > level_rank(&hits[j].level);
            if !same_range || i_is_coarser_level {
                // Either i strictly contains j (more territory, less
                // specific), or the ranges tie and i is the coarser level —
                // either way j is the more specific match to keep.
                removed[i] = true;
            }
        }
    }
    hits.into_iter().zip(removed).filter_map(|(h, r)| (!r).then_some(h)).collect()
}

/// Coarseness order for the containment tie-break above: Line is the finest
/// resolution, File the coarsest (design spec §3.3's own ordering).
fn level_rank(level: &str) -> u8 {
    match level {
        "line" => 0,
        "section" => 1,
        _ => 2, // "file"
    }
}

/// FTS5's `bm25()` returns NEGATIVE values, more negative meaning more relevant.
/// The design spec's literal `1/(1+rank)` is non-monotonic there (and can go
/// negative), so we work with `r = -bm25` — non-negative, larger is better —
/// apply the multiplicative business boosts to `r`, and only then squash into
/// (0,1) with `r/(1+r)`. Squashing last keeps the ordering the boosts produced.
///
/// The boost constants are STARTING VALUES. Changing them means re-running the
/// retrievability regression set — they encode a product claim (§4): content you
/// have judged outranks content a model produced.
fn score_of(rank: f64, hit: &Hit, is_annotation: bool, phrase_exact: bool, today: &str) -> f64 {
    let mut r = if rank < 0.0 { -rank } else { 0.001 };
    if phrase_exact {
        r *= 1.3;
    }
    if hit.level == "file" || hit.level == "section" {
        r *= 1.2;
    }
    if is_annotation {
        r *= 1.2;
    }
    if hit.human_verified {
        r *= 1.1;
    }
    // Provenance tiering (spec `docs/superpowers/specs/
    // 2026-08-11-md-origin-tiering-design.md` §3, CLAUDE.md belief 1): what
    // you wrote outranks what an agent generated, which outranks raw source
    // material an agent still has to read. `Derived` gets the identity
    // multiplier — it is the middle tier, and also the fallback both
    // `origin::derive` (rule 7) and `store::origin_of` resolve to when the
    // signal is absent or unreadable, so leaving it at ×1.0 keeps that
    // fallback genuinely neutral rather than accidentally penalizing it.
    //
    // Stacking this with the `human_verified` ×1.1 boost above is
    // INTENTIONAL, not double-counting: they are different signals about
    // different things. `human_verified` says "a human signed off on THIS
    // document" (a per-document fact from `verified.by`). This says
    // "documents classified into THIS ORIGIN TIER are usually
    // human-written" (a category-level prior from `origin::derive`). A
    // hand-written, verified note legitimately earns both — one human
    // signal from the specific sign-off, one from the general shape of the
    // document — the same way `is_annotation` and `phrase_exact` above each
    // apply independently even though both can fire on the same hit.
    r *= match hit.origin {
        Origin::Human => 1.25,
        Origin::Derived => 1.0,
        Origin::Source => 0.9,
        // TODO(C-T7): hardcoded at spec §3.1's default rather than sourced
        // from a configurable `Weights` struct — C-T7 replaces all four of
        // these literals with `Weights`-driven multipliers. Using the real
        // spec default here (not an arbitrary placeholder) so ranking is
        // already correct in the interim, not merely "compiles".
        Origin::Unlabeled => 0.3,
    };
    // The first line of defense against memory self-propagation: AI-authored
    // material is findable but never outranks the primary source it summarized.
    if hit.agent_by.is_some() {
        r *= 0.85;
    }
    if let Some(age) = hit.doc_date.as_deref().and_then(|d| days_between(d, today)) {
        r *= 1.0 + 0.2 * (-(age as f64) / 180.0).exp();
    }
    r / (1.0 + r)
}

/// Whole days from `from` to `to`, both `YYYY-MM-DD`. `None` on unparseable input.
fn days_between(from: &str, to: &str) -> Option<i64> {
    Some((days_from_civil(to)? - days_from_civil(from)?).max(0))
}

fn days_from_civil(ymd: &str) -> Option<i64> {
    let mut it = ymd.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.get(..2).unwrap_or("").parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_words_are_and_terms() {
        let q = parse("alpha beta");
        assert_eq!(q.terms, vec!["alpha", "beta"]);
        assert!(q.phrases.is_empty());
    }

    #[test]
    fn quoted_text_is_a_phrase() {
        let q = parse(r#"alpha "exact phrase" beta"#);
        assert_eq!(q.phrases, vec!["exact phrase"]);
        assert_eq!(q.terms, vec!["alpha", "beta"]);
    }

    #[test]
    fn every_filter_prefix_is_recognized() {
        let q = parse("tag:x type:concept path:docs ext:note.md after:2026-01-01 before:2026-12-31 page:[[Home]] rest");
        assert_eq!(q.tags, vec!["x"]);
        assert_eq!(q.types, vec!["concept"]);
        assert_eq!(q.paths, vec!["docs"]);
        assert_eq!(q.exts, vec!["note.md"]);
        assert_eq!(q.after.as_deref(), Some("2026-01-01"));
        assert_eq!(q.before.as_deref(), Some("2026-12-31"));
        assert_eq!(q.pages, vec!["Home"]);
        assert_eq!(q.terms, vec!["rest"]);
    }

    #[test]
    fn an_unterminated_quote_degrades_to_a_plain_term() {
        let q = parse(r#"alpha "unterminated"#);
        assert!(q.phrases.is_empty());
        assert!(q.terms.contains(&"unterminated".to_string()));
    }

    // ---- search over a real index -------------------------------------------

    fn indexed(files: &[(&str, &str)]) -> (tempfile::TempDir, Connection) {
        let d = tempfile::tempdir().unwrap();
        for (rel, body) in files {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        let mut c = crate::store::open(&d.path().join(".idx.db"), "v", "sync").unwrap();
        crate::scan::build_full(&mut c, d.path(), &crate::scan::ScanOptions::default(), None).unwrap();
        (d, c)
    }

    /// Review round 1, Important #1: the read path this task exists to add —
    /// `row_to_hit` pulling `f.origin` back out of the real `files` table —
    /// was entirely unpinned. `each_origin_tier_moves_the_score_on_its_own`
    /// builds `Hit`s by hand via `hit_with` and never touches SQL, so it
    /// cannot catch `row_to_hit` ignoring the column (always `Derived`) or
    /// reading the wrong index (e.g. index 5 = `b.level`, also `TEXT`, so no
    /// type error either) — both reduced the whole tiering feature to a
    /// silent no-op while every existing test stayed green. This goes
    /// through the real index: `a.note.md` classifies `Human` (rule 1,
    /// blind to frontmatter), a bare `.md` with none classifies `Unlabeled`
    /// (rule 6′ — `indexed`'s `ScanOptions::default()` carries no source
    /// globs, so rule 5′ never fires here; see the C-T2 task report) — two
    /// tiers, neither the `Derived` fallback both mutations above collapse
    /// onto, so either mutation is caught here.
    #[test]
    fn a_hits_origin_round_trips_through_the_real_index() {
        let (_d, c) = indexed(&[("a.note.md", "target\n"), ("b.md", "target\n")]);
        let hits = search(&c, &parse("target"), 20, "2026-08-10").unwrap().0;
        let human = hits.iter().find(|h| h.path == "a.note.md").expect("a.note.md must be found");
        let unlabeled = hits.iter().find(|h| h.path == "b.md").expect("b.md must be found");
        assert_eq!(human.origin, Origin::Human, "{human:?}");
        assert_eq!(unlabeled.origin, Origin::Unlabeled, "{unlabeled:?}");
    }

    /// Same hazard as the `origin` round-trip above, for the column task
    /// B-T7 appended: `f.concept_type` (index 11 of `SELECT_COLS`, the new
    /// last column before `rank`). `b.md` carries a registered `type` in its
    /// frontmatter; `a.md` carries none. Both are TEXT-shaped like every
    /// neighboring column, so a mutation that reads the wrong index (off by
    /// one either direction) would silently substitute another column's
    /// value here with no type error — this must fail on that mutation, not
    /// just on `concept_type` never being read at all.
    #[test]
    fn a_hits_concept_type_round_trips_through_the_real_index() {
        let (_d, c) = indexed(&[
            ("a.md", "target\n"),
            ("b.md", "---\ntype: Book Summary\n---\ntarget\n"),
        ]);
        let hits = search(&c, &parse("target"), 20, "2026-08-10").unwrap().0;
        let untyped = hits.iter().find(|h| h.path == "a.md").expect("a.md must be found");
        let typed = hits.iter().find(|h| h.path == "b.md").expect("b.md must be found");
        assert_eq!(untyped.concept_type, None, "{untyped:?}");
        assert_eq!(typed.concept_type.as_deref(), Some("Book Summary"), "{typed:?}");
    }

    #[test]
    fn finds_an_ascii_term_and_returns_a_source_anchor() {
        let (_d, c) = indexed(&[("2026-01-01-a.md", "# T\n\nthe quick brownfox\n")]);
        let (hits, route) = search(&c, &parse("brownfox"), 20, "2026-08-10").unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].path, "2026-01-01-a.md");
        assert_eq!(hits[0].line, 3);
        assert_eq!(route.as_str(), "t1-fts");
    }

    /// spec §3.2 的招牌用例:查「增量」必须命中只写了「增量索引」的文档。
    #[test]
    fn a_cjk_sub_word_query_hits_the_longer_word() {
        let (_d, c) = indexed(&[("a.md", "本节讲增量索引的设计\n")]);
        let (hits, _) = search(&c, &parse("增量"), 20, "2026-08-10").unwrap();
        assert!(!hits.is_empty(), "cut_for_search overlap must make this hit");
    }

    /// 词典盲区(未登录词/人名/单字):FTS 零命中就降级有界扫描,并如实标注路由。
    ///
    /// Deviation from the task brief's literal fixture, recorded in the task
    /// report: the brief queried the full name `李慕白`, but jieba's bundled
    /// dictionary actually recognizes that (real, well-known) name as one
    /// token on both the index and query side (verified with a standalone
    /// probe), so it is not actually a dictionary blind spot and the query
    /// resolves via `t1-fts`, not `t1-scan`. `李慕白` IS a genuine blind spot
    /// for a single-character query, though: `cut_for_search` keeps a
    /// recognized name as one atomic token and does not also emit its
    /// individual characters as sub-words (unlike the overlap it gives
    /// `增量索引`, which is dictionary-unknown as a whole and so gets cut into
    /// `增量`+`索引`+itself). Querying `慕` in isolation therefore shares zero
    /// tokens with `李慕白`'s indexed form, giving a real FTS miss.
    #[test]
    fn an_out_of_vocabulary_cjk_query_falls_back_to_a_bounded_scan() {
        let (_d, c) = indexed(&[("a.md", "会见了李慕白同志\n")]);
        let (hits, route) = search(&c, &parse("慕"), 20, "2026-08-10").unwrap();
        assert!(!hits.is_empty(), "the dictionary blind spot must not become a miss");
        assert_eq!(route.as_str(), "t1-scan");
    }

    /// Fix round 1, Critical: `like_search` used to bind only
    /// `q.phrases.first().or_else(|| q.terms.first())`, silently dropping
    /// every other term — a query with an ordinary term plus a dictionary
    /// blind-spot term (this fallback's central use case) would return any
    /// file containing EITHER, not both. Reproduced live by the reviewer on
    /// exactly this shape. `会见了李慕白同志` forces the fallback (see the
    /// OOV test above: `慕` shares no FTS token with the recognized
    /// compound `李慕白`), so this also proves the fix constrains via LIKE,
    /// not by accidentally resolving through FTS.
    #[test]
    fn the_bounded_scan_fallback_constrains_on_every_term_not_just_the_first() {
        let (_d, c) = indexed(&[("both.md", "target 会见了李慕白同志\n"), ("only_target.md", "target only\n")]);
        let (hits, route) = search(&c, &parse("target 慕"), 20, "2026-08-10").unwrap();
        assert_eq!(route.as_str(), "t1-scan", "must genuinely exercise the fallback, not the FTS path");
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].path, "both.md");
    }

    /// 逐字输入(尤其中文)几乎必然 FTS 未命中,而回退是全表扫描 —— 在真实
    /// vault(8933 文件 / 1_306_104 blocks)上实测 14.3 秒。所以非 deep 的
    /// 查询必须**根本不跑**回退,只汇报「深搜可用」,由调用方决定何时付这笔钱。
    #[test]
    fn a_shallow_query_never_pays_for_the_scan_fallback_but_says_it_is_available() {
        let (_d, c) = indexed(&[("both.md", "target 会见了李慕白同志\n")]);
        let shallow = Limits { deep: false, abort: None };
        let a = search_with(&c, &parse("target 慕"), 20, "2026-08-10", &shallow).unwrap();
        assert_eq!(a.route.as_str(), "t1-fts", "the fallback must not have run");
        assert!(a.hits.is_empty());
        assert!(a.deep_available, "the caller has to be able to offer the deep search");

        let deep = search_with(&c, &parse("target 慕"), 20, "2026-08-10", &Limits::full()).unwrap();
        assert_eq!(deep.route.as_str(), "t1-scan");
        assert_eq!(deep.hits.len(), 1, "{:?}", deep.hits);
        assert!(!deep.deep_available, "it already ran; there is nothing left to offer");
    }

    /// 上一轮搜索必须真的停下来,而不只是「结果被丢弃」:它占着索引锁,后面
    /// 每一次击键都排在它后面。中止点必须在语句**内部** —— 一次未命中的
    /// LIKE 扫描会在单个 `step()` 里跑满全程,一行都不返回。
    #[test]
    fn an_aborted_scan_stops_inside_the_statement_instead_of_running_to_completion() {
        let (_d, c) = indexed(&[("a.md", "target 李慕白\n")]);
        // Enough rows that the scan is guaranteed to cross a progress-handler
        // checkpoint; inserted directly, since what is under test is the
        // scan's abort, not indexing. `origin` has to be supplied explicitly:
        // schema v2 declares it `NOT NULL` with no default on purpose (see
        // `store::tests::the_origin_column_rejects_a_null_insert`), so the
        // pre-tiering spelling of this insert would fail the constraint
        // rather than index anything.
        c.execute(
            "INSERT INTO files(id, path, ext, origin) VALUES (999, 'bulk.md', 'md', 'derived')",
            [],
        )
        .unwrap();
        let tx = c.unchecked_transaction().unwrap();
        for i in 0..20_000 {
            tx.execute(
                "INSERT INTO blocks(file_id, line_start, line_end, breadcrumb, text, level)
                 VALUES (999, ?1, ?1, '', ?2, 'line')",
                rusqlite::params![i, format!("filler row {i} with no needle in it")],
            )
            .unwrap();
        }
        tx.commit().unwrap();

        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = calls.clone();
        let limits = Limits {
            deep: true,
            abort: Some(Arc::new(move || {
                seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                true
            })),
        };
        let a = search_with(&c, &parse("慕"), 20, "2026-08-10", &limits).unwrap();
        assert!(calls.load(std::sync::atomic::Ordering::Relaxed) > 0, "abort was never consulted");
        assert!(a.truncated, "an aborted retrieval must say so, not pose as a complete answer");

        // And the handler must not outlive the call: the next caller through
        // this connection is usually the watcher's sweep or a rebuild, and an
        // always-true abort left installed would kill it.
        let after = search_with(&c, &parse("慕"), 20, "2026-08-10", &Limits::full()).unwrap();
        assert!(!after.truncated, "the progress handler leaked into the next query");
        assert_eq!(after.hits.len(), 1, "{:?}", after.hits);
    }

    /// 引号短语必须做精确子串复核:分词是重叠的,FTS 的 AND 不保证顺序。
    #[test]
    fn a_phrase_query_rejects_hits_where_the_words_are_not_adjacent() {
        let (_d, c) = indexed(&[("a.md", "alpha then beta\n"), ("b.md", "alpha beta\n")]);
        let (hits, _) = search(&c, &parse(r#""alpha beta""#), 20, "2026-08-10").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "b.md");
    }

    #[test]
    fn filters_narrow_the_result_set() {
        let (_d, c) = indexed(&[
            ("docs/a.md", "---\ntype: concept\ntags: [x]\n---\ntarget\n"),
            ("other/b.md", "target\n"),
        ]);
        let only = |q: &str| search(&c, &parse(q), 20, "2026-08-10").unwrap().0.len();
        assert_eq!(only("target path:docs"), 1);
        assert_eq!(only("target type:concept"), 1);
        assert_eq!(only("target tag:x"), 1);
        assert_eq!(only("target"), 2);
    }

    /// `path:` filters go through `LIKE ... ESCAPE '\'`; a literal `%`/`_` in
    /// the filter value must not act as a SQL wildcard, or `path:100%` would
    /// also match an unrelated `100X/a.md` — exercises hazard 6 from the task
    /// brief, which no other test in this module covers.
    #[test]
    fn a_percent_sign_in_a_path_filter_is_matched_literally_not_as_a_wildcard() {
        let (_d, c) = indexed(&[("100%/a.md", "target\n"), ("100X/a.md", "target\n")]);
        let hits = search(&c, &parse("target path:100%"), 20, "2026-08-10").unwrap().0;
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].path, "100%/a.md");
    }

    /// Fix round 1, Important: the `tag:` filter built its LIKE pattern from
    /// the raw tag value with no `escape_like()`/`ESCAPE` clause, unlike
    /// `path:` right above — reproduced live: `tag:in_progress` also matched
    /// a file tagged `inXprogress`, because an unescaped `_` is a
    /// single-character SQL wildcard. Snake_case tags are ordinary; this
    /// pins the same literal-match treatment `path:` already gets.
    #[test]
    fn an_underscore_in_a_tag_filter_is_matched_literally_not_as_a_wildcard() {
        let (_d, c) = indexed(&[
            ("a.md", "---\ntags: [in_progress]\n---\ntarget\n"),
            ("b.md", "---\ntags: [inXprogress]\n---\ntarget\n"),
        ]);
        let hits = search(&c, &parse("target tag:in_progress"), 20, "2026-08-10").unwrap().0;
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].path, "a.md");
    }

    #[test]
    fn origin_prefix_is_recognized_by_parse() {
        let q = parse("target origin:human");
        assert_eq!(q.origins, vec!["human"]);
        assert_eq!(q.terms, vec!["target"]);
    }

    /// Task 6: `origin:` narrows to one tier. Three files, three tiers, via the
    /// same `origin::derive` rules the real index uses — `.note.md` is rule 1
    /// (Human), `type: Book Summary` maps to Derived, `type: Book` maps to
    /// Source (see `origin::mapped_type_origin`).
    #[test]
    fn the_origin_filter_narrows_to_a_single_tier() {
        let (_d, c) = indexed(&[
            ("a.note.md", "target\n"),
            ("b.md", "---\ntype: Book Summary\n---\ntarget\n"),
            ("c.md", "---\ntype: Book\n---\ntarget\n"),
        ]);
        let only = |q: &str| {
            let mut paths: Vec<String> =
                search(&c, &parse(q), 20, "2026-08-10").unwrap().0.into_iter().map(|h| h.path).collect();
            paths.sort();
            paths
        };
        assert_eq!(only("target origin:human"), vec!["a.note.md"]);
        assert_eq!(only("target origin:derived"), vec!["b.md"]);
        assert_eq!(only("target origin:source"), vec!["c.md"]);
        assert_eq!(only("target"), vec!["a.note.md", "b.md", "c.md"], "sanity: all three recall without a filter");
    }

    /// `origin:` is a per-file singular attribute (like `type:`/`ext:`), so
    /// repeating it must OR across tiers, not AND to an impossible
    /// simultaneous match — the task brief flags this explicitly: following
    /// the literal `f.origin = ?N` per value would AND multiple origin:
    /// filters together via `push_filters`'s existing per-item-clause idiom,
    /// and no file can equal two different origins at once, so a second
    /// `origin:` filter would silently zero out every result. `IN (...)`
    /// avoids that.
    #[test]
    fn multiple_origin_filters_are_ored_not_anded() {
        let (_d, c) = indexed(&[
            ("a.note.md", "target\n"),
            ("b.md", "---\ntype: Book Summary\n---\ntarget\n"),
            ("c.md", "---\ntype: Book\n---\ntarget\n"),
        ]);
        let mut hits = search(&c, &parse("target origin:human origin:source"), 20, "2026-08-10").unwrap().0;
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        let paths: Vec<_> = hits.iter().map(|h| h.path.clone()).collect();
        assert_eq!(paths, vec!["a.note.md", "c.md"], "{hits:?}");
    }

    /// Retrieval must never fail because of the caller (task brief). Two
    /// candidate semantics for `origin:bogus`: drop the filter (return every
    /// tier, as if no `origin:` were given) or match nothing. Dropping the
    /// filter is the more dangerous of the two — a caller who explicitly
    /// asked to filter by origin would silently get back an UNFILTERED set
    /// with no signal that filtering never happened: a confident false
    /// positive, exactly the failure mode `like_search`'s own doc comment
    /// above calls worse than a miss ("a miss at least looks like 'no
    /// results'; this looked like an answer"). So an unrecognized origin
    /// literal fails closed — it is bound into the SQL literally, like every
    /// other filter value in this function, and the `files.origin` column
    /// never stores anything but `human`/`derived`/`source`, so it naturally
    /// matches zero rows without any special-cased validation.
    #[test]
    fn an_unrecognized_origin_value_matches_nothing_not_everything() {
        let (_d, c) = indexed(&[("a.md", "target\n")]);
        let hits = search(&c, &parse("target origin:bogus"), 20, "2026-08-10").unwrap().0;
        assert!(hits.is_empty(), "an invalid origin: value must fail closed, not silently return every tier: {hits:?}");
    }

    #[test]
    fn date_filters_use_doc_date() {
        let (_d, c) = indexed(&[("2020-01-01-old.md", "target\n"), ("2026-08-01-new.md", "target\n")]);
        let hits = search(&c, &parse("target after:2026-01-01"), 20, "2026-08-10").unwrap().0;
        assert!(hits.iter().all(|h| h.path.starts_with("2026")), "{hits:?}");
    }

    /// Hand-verified against independently computed day counts — hazard 8
    /// from the task brief: `days_from_civil` must read the day out of the
    /// first 2 chars of the third `-`-separated segment, which is what makes
    /// it tolerant of a full ISO datetime (`...T10:00:00Z`) landing in the
    /// day position (frontmatter `created`/`date` values are often full
    /// timestamps, not bare dates).
    #[test]
    fn days_between_handles_plain_dates_and_an_iso_datetime_in_the_day_position() {
        assert_eq!(days_between("2026-08-01", "2026-08-10"), Some(9));
        assert_eq!(days_between("2026-01-31", "2026-02-01"), Some(1), "cross-month");
        assert_eq!(days_between("2024-02-28", "2024-03-01"), Some(2), "leap day");
        assert_eq!(
            days_between("2026-08-01T10:00:00Z", "2026-08-10"),
            Some(9),
            "a full ISO datetime in the day position must parse the same as a bare date"
        );
        assert_eq!(days_between("2026-08-10", "2026-08-01"), Some(0), "future 'from' clamps to 0, never negative");
    }

    /// spec §4 的产品主张:你留过判断的内容优先,AI 生成物降权。
    ///
    /// This end-to-end test exercises the full pipeline (outline chunking →
    /// `is_annotation`/`agent_by` extraction → indexing → ranking), but on
    /// this particular fixture the two blocks' raw bm25 scores tie exactly
    /// (both are "target <word>", same token count), so `Vec::sort_by`'s
    /// stability — not the boost math — is what keeps them in insertion
    /// order. Mutation-verified: this test still passes with BOTH the
    /// `is_annotation` boost and the `agent_by` penalty in `score_of`
    /// hard-coded to a no-op. `score_of_boosts_annotations_and_penalizes_agent_authored_content`
    /// below is the test that actually isolates and pins the boost math;
    /// this one stays as the pipeline-wiring smoke test its name promises.
    #[test]
    fn annotations_outrank_agent_authored_blocks() {
        let (_d, c) = indexed(&[("a.note.md", "- target one\n  type:: annotation\n- target two\n  by:: claude/1\n")]);
        let hits = search(&c, &parse("target"), 20, "2026-08-10").unwrap().0;
        let anno = hits.iter().position(|h| h.text.contains("one")).unwrap();
        let agent = hits.iter().position(|h| h.text.contains("two")).unwrap();
        assert!(anno < agent, "human-marked content must rank above agent output: {hits:?}");
    }

    /// Shared fixture for the pure `score_of` tests below. `origin` defaults
    /// to whatever the caller passes; callers that don't care about it use
    /// `Origin::Derived` — the identity multiplier (see `score_of`'s
    /// origin-tier comment) — so it never contaminates a test pinning a
    /// different boost.
    fn hit_with(origin: Origin) -> Hit {
        Hit {
            path: "a.md".into(),
            line: 1,
            line_end: 1,
            text: String::new(),
            breadcrumb: String::new(),
            level: "line".into(),
            score: 0.0,
            doc_date: None,
            agent_by: None,
            human_verified: false,
            origin,
            concept_type: None,
        }
    }

    /// spec §4 的产品主张,与 origin tiering 设计 §3 的落点:你写的 > agent 生成的
    /// > agent 要读的原始材料。**三档必须各自独立可验** —— 前置项目里出现过「两个
    /// 乘数一起推同一方向、任一个单独失效测试仍通过」的假阴性(见 mutation check
    /// in the task report),所以这里逐档断言 `score_of` 本身,而不是端到端排序,
    /// 且断言两条不等式而非一条,让 mutation check 能分别单独抓到每一档。
    #[test]
    fn each_origin_tier_moves_the_score_on_its_own() {
        let human = score_of(-1.0, &hit_with(Origin::Human), false, false, "2026-08-10");
        let derived = score_of(-1.0, &hit_with(Origin::Derived), false, false, "2026-08-10");
        let source = score_of(-1.0, &hit_with(Origin::Source), false, false, "2026-08-10");
        let unlabeled = score_of(-1.0, &hit_with(Origin::Unlabeled), false, false, "2026-08-10");
        assert!(human > derived, "human 必须高于 derived: {human} vs {derived}");
        assert!(derived > source, "derived 必须高于 source: {derived} vs {source}");
        // Task 2 review round 2, Important #1: without this inequality, the
        // suite had ZERO coverage distinguishing `Unlabeled` from `Source` —
        // `Unlabeled => 0.9` (exactly `Source`'s multiplier, i.e. the retired
        // rule 6 behavior this whole task exists to end) passed all 194+9+1
        // tests, because nothing asserted `source` and `unlabeled` differ at
        // all. `source > unlabeled` is the ordering half of that fix.
        assert!(source > unlabeled, "source 必须高于 unlabeled: {source} vs {unlabeled}");
        // Review round 1, Important #2: the two inequalities above only pin
        // `Derived` to the open interval (0.9, 1.25) — a later "unclassified
        // deserves a small nudge" change (e.g. `Derived => 1.1`) would still
        // satisfy both and slip through green. `Derived` being EXACTLY the
        // identity multiplier is the real invariant: it's what
        // `store::origin_of` falls back to for an unreadable/unknown/NULL
        // origin, so "fail toward neutral" only holds if neutral means
        // literally no change. Pin the exact value against a tier-bypassed
        // reference: with `rank = -1.0` and every other boost off, `r/(1+r)`
        // computes to exactly `0.5` before any origin multiplier is applied
        // — `Derived`'s score must equal that reference exactly, not just
        // sit somewhere between `source` and `human`.
        assert_eq!(derived, 0.5, "Derived must be the exact identity multiplier, not merely `< human` and `> source`");
        // Task 2 review round 2, Important #1 (the exact-value half): the
        // `source > unlabeled` inequality above still only pins `Unlabeled`
        // to the open interval `(0.0, 0.9)` — it would not catch, say,
        // `Unlabeled => 0.5` slipping in from a future `Weights` refactor
        // (C-T7) that transcribes spec §3.1's ×0.3 wrong. Pin the exact
        // value the same way `derived` is pinned above: `r = 1.0 * 0.3`
        // before the final `r / (1 + r)` normalization, computed from first
        // principles (not a decimal literal that could itself be a
        // typo-in-the-test) so this is falsifiable by inspection.
        assert_eq!(
            unlabeled,
            0.3_f64 / 1.3_f64,
            "Unlabeled must be exactly ×0.3 (spec §3.1), not merely `< source`"
        );
    }

    /// The test above can pass on stable-sort tie-breaking alone (see its
    /// comment) — this one calls the pure ranking function directly so the
    /// two boosts spec §4 cares about (`is_annotation` ×1.2, `agent_by`
    /// ×0.85) are pinned independent of bm25/SQLite behavior.
    #[test]
    fn score_of_boosts_annotations_and_penalizes_agent_authored_content() {
        let base = hit_with(Origin::Derived);
        let plain = score_of(-1.0, &base, false, false, "2026-08-10");
        let annotated = score_of(-1.0, &base, true, false, "2026-08-10");
        let mut agent_hit = base.clone();
        agent_hit.agent_by = Some("claude/1".to_string());
        let agent = score_of(-1.0, &agent_hit, false, false, "2026-08-10");
        assert!(annotated > plain, "annotation boost must raise the score: {annotated} vs {plain}");
        assert!(agent < plain, "agent-authored content must be penalized: {agent} vs {plain}");
        assert!(annotated > agent, "human-marked content must outrank agent output: {annotated} vs {agent}");
    }

    /// spec §4's third boost — `human_verified` ×1.1 — had no pure-function pin
    /// until task-11 review round 1 flagged that its only coverage was an
    /// end-to-end acceptance test with an uncontrolled bm25 length confound
    /// between the two fixtures. This mirrors
    /// `score_of_boosts_annotations_and_penalizes_agent_authored_content`
    /// immediately above: call `score_of` directly so the ×1.1 multiplier is
    /// pinned independent of bm25/SQLite/fixture-length behavior entirely.
    #[test]
    fn score_of_boosts_human_verified_content() {
        let base = hit_with(Origin::Derived);
        let unverified = score_of(-1.0, &base, false, false, "2026-08-10");
        let mut verified_hit = base.clone();
        verified_hit.human_verified = true;
        let verified = score_of(-1.0, &verified_hit, false, false, "2026-08-10");
        assert!(verified > unverified, "human_verified boost must raise the score: {verified} vs {unverified}");
    }

    #[test]
    fn scores_are_finite_positive_and_descending() {
        let (_d, c) = indexed(&[("a.md", "target target target\n"), ("b.md", "target\n")]);
        let hits = search(&c, &parse("target"), 20, "2026-08-10").unwrap().0;
        assert!(hits.iter().all(|h| h.score > 0.0 && h.score < 1.0 && h.score.is_finite()), "{hits:?}");
        assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn limit_is_respected() {
        let (_d, c) = indexed(&[("a.md", "target\n"), ("b.md", "target\n"), ("c.md", "target\n")]);
        assert_eq!(search(&c, &parse("target"), 2, "2026-08-10").unwrap().0.len(), 2);
    }

    /// Fix round 1, minor: when FTS misses AND the attempted LIKE fallback
    /// also misses, `search()` used to report `Route::Fts` — the record
    /// would say no fallback was tried when one was. The CLI surfaces
    /// `route` in `--json`, and agents read it to decide whether a query was
    /// exhaustively tried, so the route must name whichever path actually
    /// ran, not just the one that found something.
    #[test]
    fn a_double_miss_still_reports_the_route_that_actually_ran() {
        let (_d, c) = indexed(&[("a.md", "target\n")]);
        let (hits, route) = search(&c, &parse("慕"), 20, "2026-08-10").unwrap();
        assert!(hits.is_empty(), "{hits:?}");
        assert_eq!(route.as_str(), "t1-scan", "the fallback was attempted and found nothing; the route must say so");
    }

    /// FTS5 的语法字符不能把查询打成语法错误 —— agent 会原样传用户输入进来。
    #[test]
    fn fts_syntax_characters_in_a_query_do_not_error() {
        let (_d, c) = indexed(&[("a.md", "target\n")]);
        for q in ["a OR b", "NEAR(", "*", "\"", "^x", "a-b"] {
            assert!(search(&c, &parse(q), 5, "2026-08-10").is_ok(), "query {q:?} must not error");
        }
    }
}
