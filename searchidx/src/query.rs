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
    /// This hit's file is the wikilink page the query names exactly, so it
    /// sorts ahead of every other hit regardless of score (wikipage priority
    /// spec §4). Computed per query from [`Conventions`], never stored — the
    /// directory it depends on is a setting the user can rename at any time.
    ///
    /// Deliberately a property of the FILE, not of the block: if the page's
    /// body also matches, `drop_redundant_rollups` keeps a Line-level hit and
    /// discards the File-level one, and a pin attached only to the File block
    /// would vanish with it.
    pub pinned: bool,
    /// 已衰减到今天的注意力分钟数(`doc_attention.minutes` 再按表的 `as_of`
    /// 到今天二次衰减)。0 = 没有数据,不是「读了 0 分钟」—— 两者对排序的
    /// 影响相同,所以不用 `Option` 徒增调用方的分支。
    ///
    /// 与 `pinned` 一样由 `finish` 填,不在 `row_to_hit` 里:二次衰减需要
    /// `today`,而一行数据自己不知道今天是几号。
    pub attention_minutes: f64,
}

/// What the ranking needs to know about this vault's *conventions* — as
/// opposed to [`Limits`] (what a caller will spend) and [`Weights`] (ranking
/// arithmetic).
///
/// Passed per query rather than stored in the index on purpose: every field
/// here is a user-editable setting, and baking one into the index would mean
/// renaming a directory silently produced wrong answers until the next full
/// rebuild. `renaming_the_wikipage_dir_takes_effect_without_reindexing` in
/// `tests/acceptance.rs` is the nail in that decision.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conventions {
    /// The vault's `wikipageDir` setting — the directory `[[…]]` links create
    /// pages in. `None` disables pinning entirely, which is what every
    /// caller that has no vault settings in hand (tests, `search`) gets.
    pub wikipage_dir: Option<String>,
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

/// The per-origin-tier ranking multipliers (spec §3.1's default values, now
/// user-tunable rather than hardcoded — task C-T7). `search`/`search_with` are
/// how a caller supplies its own; every other retrieval function in this
/// module threads `&Weights` through rather than reading a global, so the
/// GUI, the `notemd search` CLI and the watcher can each carry a different
/// (or identical) value without any shared mutable state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Weights {
    pub human: f64,
    pub derived: f64,
    pub source: f64,
    pub unlabeled: f64,
    /// 注意力加成的上限增量(规格里的 `k`)。**语义与上面四个不同**:
    /// 那四个是乘数,这个是「最多再乘 (1+k)」里的 k,所以 0 是合法的
    /// 「关掉」而不是坏值 —— 见 `sanitized`。
    pub attention: f64,
}

/// The shipped constants — same four numbers `score_of` hardcoded before this
/// task, so a caller who never touches `Weights` sees byte-identical ranking.
/// Failing toward these values (not toward some "neutral" ×1.0 for every
/// tier) is deliberate, the same lesson `Limits::default()`'s `deep: false`
/// got wrong: a future `..Default::default()` must land on the safe,
/// already-shipped behavior, not silently reset every tier to a no-op.
impl Default for Weights {
    fn default() -> Self {
        Weights { human: 1.25, derived: 1.0, source: 0.9, unlabeled: 0.3, attention: 0.4 }
    }
}

impl Weights {
    /// Reject non-finite, non-positive, and above-5.0 components, falling
    /// back to the default *for that specific component* — a bad `human`
    /// value must not also clobber an otherwise-fine `source` value. Zero is
    /// rejected on purpose: it would collapse an entire tier's scores to
    /// exactly 0, making ordering *within* that tier undefined rather than
    /// merely "unweighted". A deliberate inversion (the user weighting
    /// `source` above `human`) is left untouched — that is their call about
    /// their own vault, not something this function is allowed to judge.
    pub fn sanitized(self) -> Weights {
        let fallback = Weights::default();
        let clean = |v: f64, default: f64| {
            if v.is_finite() && v > 0.0 && v <= 5.0 { v } else { default }
        };
        // `attention` 走自己的闸门,**不能**复用上面的 `clean`:那条规则
        // 拒绝 0(对乘数而言 0 会让整层塌成 0 分,层内顺序未定义),而对
        // 这个加数而言 0 正是用户表达「关掉这个功能」的唯一方式。上限 2.0
        // 而非 5.0:k=2 已经是 ×3 封顶,再高就不是加权而是覆盖排序了。
        let attention = if self.attention.is_finite() && (0.0..=2.0).contains(&self.attention) {
            self.attention
        } else {
            fallback.attention
        };
        Weights {
            human: clean(self.human, fallback.human),
            derived: clean(self.derived, fallback.derived),
            source: clean(self.source, fallback.source),
            unlabeled: clean(self.unlabeled, fallback.unlabeled),
            attention,
        }
    }
}

/// Back-compatible entry point: every route, no abort, the shipped ranking
/// weights.
pub fn search(
    conn: &Connection,
    q: &Query,
    limit: usize,
    today: &str,
) -> rusqlite::Result<(Vec<Hit>, Route)> {
    let a =
        search_with(conn, q, limit, today, &Limits::full(), &Weights::default(), &Conventions::default())?;
    Ok((a.hits, a.route))
}

#[allow(clippy::too_many_arguments)]
pub fn search_with(
    conn: &Connection,
    q: &Query,
    limit: usize,
    today: &str,
    limits: &Limits,
    weights: &Weights,
    conventions: &Conventions,
) -> rusqlite::Result<Answer> {
    // Installed for the whole call and removed on every exit path (including
    // `?`) by the guard's Drop — a progress handler left behind on this
    // connection would abort the *next* caller's work, and the next caller is
    // usually the watcher's sweep or a rebuild.
    let _guard = ProgressGuard::install(conn, limits.abort.clone())?;

    let (hits, truncated) = fts_search(conn, q, limit, today, weights, conventions)?;
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
        let (hits, truncated) = like_search(conn, q, limit, today, weights, conventions)?;
        return Ok(Answer { hits, route: Route::Scan, truncated, deep_available: false });
    }
    // A query with at least one filter (`origin:`, `type:`, `tag:`, …) but no
    // terms/phrases — `origin:unlabeled` alone, for instance — is NOT the
    // same as an empty query. `match_expr` returns `None` for it (nothing to
    // MATCH), so `fts_search` above never even calls `push_filters`, and
    // `needs_scan_fallback` only looks at terms/phrases too, so it's `false`
    // here regardless of filters. Left unhandled, the documented grammar
    // (AGENTS.md/`--help`'s `origin:unlabeled`, design spec §6.3, and the
    // settings page's "Unlabeled" statistics row, which runs exactly this
    // query) silently returns nothing for every caller — GUI, CLI, and any
    // agent that follows the advertised syntax literally.
    //
    // Order matters: this runs AFTER the scan-fallback branch above, so a
    // query that also has terms/phrases keeps its normal FTS→scan behavior
    // untouched; this only fires when there was nothing to MATCH in the
    // first place. And it's gated on `has_filters`, so a truly empty query
    // (no terms, no phrases, no filters) still falls through to the empty
    // `Answer` below, unchanged.
    if q.terms.is_empty() && q.phrases.is_empty() && has_filters(q) {
        let (hits, truncated) = filter_only_search(conn, q, limit, today, weights, conventions)?;
        return Ok(Answer { hits, route: Route::Fts, truncated, deep_available: false });
    }
    Ok(Answer { hits: Vec::new(), route: Route::Fts, truncated: false, deep_available: false })
}

/// Whether `q` carries at least one filter clause (`push_filters` would emit
/// at least one `AND ...`). Deliberately excludes `terms`/`phrases` — those
/// are what makes a query a *keyword* search; this only asks "is there a
/// filter to apply on its own".
fn has_filters(q: &Query) -> bool {
    !q.tags.is_empty()
        || !q.types.is_empty()
        || !q.paths.is_empty()
        || !q.exts.is_empty()
        || !q.origins.is_empty()
        || !q.pages.is_empty()
        || q.after.is_some()
        || q.before.is_some()
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
// again if a future column is added. `f.title` (index 12) follows the same
// rule, appended after `concept_type` rather than slotted next to the other
// `files` columns. Every caller appends its own `rank` column after
// everything here — index 15 since C-T7 widened this list (it was 13 before;
// see the C-T7 paragraph below). `is_annotation` (index 9) is unchanged from
// before `origin`/`concept_type`/`title` were added.
//
// `f.title` is read into `finish`'s row tuple, NOT into `Hit` — pinning is
// the only consumer and it has no business widening the public hit shape
// (same treatment `is_annotation` already gets).
//
// C-T7 appended two more, again AFTER everything above rather than beside
// their thematic neighbors: `COALESCE(att.minutes, 0.0)` (index 13) and
// `att.as_of` (index 14), both from the `LEFT JOIN doc_attention` each
// caller adds. They shifted every caller's own `rank` column from 13 to 15 —
// the one hardcoded index in this file that had to move with them.
//
// If you add another column here, know exactly how forgetting to move `rank`
// fails, because the obvious guess is WRONG: it does NOT raise
// `InvalidColumnType`. Index 13 is `COALESCE(att.minutes, 0.0)` — a REAL,
// same as `rank` — so a stale `r.get::<_, f64>(13)` reads back a perfectly
// valid number and every hit's bm25 is silently replaced by its attention
// minutes (0.0 for most files). `score_of` then floors that to `r = 0.001`
// for every hit, all scores collapse to one value, and relevance ordering
// stops existing — with no error, no panic, and (measured, C-T7) not one red
// test out of the whole suite INCLUDING `tests/acceptance.rs`'s
// retrievability regression set. `the_bm25_rank_column_actually_reaches_the_
// score` below was added for exactly this: it is the one test that dies when
// `rank`'s index is wrong.
const SELECT_COLS: &str = "f.path, b.line_start, b.line_end, b.text, b.breadcrumb, b.level, \
                           f.doc_date, b.agent_by, f.human_verified, b.is_annotation, f.origin, \
                           f.concept_type, f.title, \
                           COALESCE(att.minutes, 0.0), att.as_of";

/// Attention is optional data ABOUT a file, never a condition ON it. `LEFT`
/// is load-bearing, not style: an `INNER JOIN` here would delete every file
/// the user has never opened — most of the vault, and exactly the
/// freshly-generated material search exists to find — from every result set,
/// silently and on all three query paths at once.
const ATTENTION_JOIN: &str = " LEFT JOIN doc_attention att ON att.path = f.path";

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
    weights: &Weights,
    conventions: &Conventions,
) -> rusqlite::Result<(Vec<Hit>, bool)> {
    let Some(expr) = match_expr(q) else { return Ok((Vec::new(), false)) };
    let ((sql, args), attention_arm) = fts_arms(q, expr, limit, weights.attention);
    let (main_rows, main_truncated) = run_fts_arm(conn, &sql, &args)?;
    // `k = 0` 关掉的是**整条链路**,不只是打分:没有第二条臂,候选集就与接入
    // 本功能之前逐条相同(见 `fts_arms` 的「`k = 0` 必须连臂一起关」一节)。
    let (arm_rows, arm_truncated) = match &attention_arm {
        Some((arm, arm_args)) => run_fts_arm(conn, arm, arm_args)?,
        None => (Vec::new(), false),
    };

    // 去重键用 `(path, line, line_end)`,而它**不是唯一键** —— 早先这里的注释
    // 说「这个三元组在 `blocks` 里已经唯一」,那句话是错的:实测任何单块文件
    // 都同时有一条 `(1, 1) line` 和一条 `(1, 1) file` 的整篇 rollup,每个文件
    // 至少撞一对。之所以仍然够用,是因为真正兜住这件事的是下游的
    // `drop_redundant_rollups`(同一文件里被更细的块覆盖的 rollup 一律丢掉),
    // 于是撞键的那两条本来就只有一条能活到输出。
    //
    // 明知不唯一还用它,是权衡:方向上它只会**多丢**、不会**漏留**。同一个块
    // 两条臂读出的三元组逐字相同,所以重复绝不会漏网(会显示两遍的那种坏);
    // 撞键的代价只是保底臂少收一条候选,而那条候选是个 rollup,下游本来也要
    // 丢。反过来若把 block id 选出来做键,就得动 `SELECT_COLS` 的列序,把
    // `rank` 从 15 推到 16 —— 那是 C-T7 的地雷(漏改不报类型错,只让 bm25
    // 静默失效),代价方向是「无声地毁掉相关性排序」。两害相权,取会多丢一条
    // rollup 的那个。
    let mut rows = main_rows;
    let seen: std::collections::HashSet<_> =
        rows.iter().map(|(h, ..): &RawRow| (h.path.clone(), h.line, h.line_end)).collect();
    rows.extend(
        arm_rows
            .into_iter()
            .filter(|(h, ..): &RawRow| !seen.contains(&(h.path.clone(), h.line, h.line_end))),
    );
    // 两条臂各自可能被 abort 打断,如实反映:任一条被截断,结果集就是不完整的。
    let truncated = main_truncated || arm_truncated;
    Ok((finish(rows, q, limit, today, weights, conventions)?, truncated))
}

/// FTS 路径的候选臂的 SQL 与参数:`(主臂, 保底臂)` —— 保底臂是 `Option`,
/// `k = 0` 时**根本不构造**(见下)。
///
/// # 为什么是两条
///
/// 主臂先按 bm25 `ORDER BY rank ASC LIMIT (limit*8).max(64)` 取候选、**再**跑
/// `score_of`。所以纯排序加权救不了排在窗口外的文档:一份「你读了三小时、查询
/// 词只出现一次」的长文在打分之前就被 `LIMIT` 砍掉了,再大的系数也没机会作用
/// 在它身上(规格 §5)。保底臂只做一件事:换个排序取候选,给这类文档一个进入
/// 评分阶段的名额。
///
/// # 它保证的是「进入评分」,不是「出现在结果里」——最容易被误解的地方
///
/// 保底臂捞回来的行照样要过 `finish`:`score_of` 打分、排序、`truncate(limit)`。
/// 注意力加成的天花板是 `1 + k`(默认 k=0.4 即 ×1.4,`attention::boost` 里
/// `frac` 已封顶),所以当一份文档的 bm25 比窗口内的对手差得超过这个倍数、而
/// 其它加成又不站在它这边时,**用户看到的结果不会有任何变化**。这是规格定的
/// (「`limit*2` 刻意取小:它是保底,不是主力 —— 主排序仍然是相关度」),不是
/// 缺陷;但「优先召回」四个字很容易被读成「一定会出现」,对外文案要当心。
/// 实测参见 acceptance 里 `a_high_attention_document_is_recalled_past_the_bm25_
/// window` 的注释:简报原稿那份 500 行长文差了 44.9 倍,加满注意力仍差 32.1
/// 倍,任何候选阶段的改动都救不了它。
///
/// # 性能:这条路径现在跑**两条 SQL**
///
/// 实测(2000 文件合成 vault):主臂 6147µs、保底臂 3789µs —— FTS 路径的 SQL
/// 成本约 **+62%**。这是功能本身的代价,不是 bug,但要知道两件事:
///
/// 1. `warm_queries_are_fast` 那条守卫对此**完全失明**:它在小 corpus 上 p50
///    只有 49µs、阈值 10000µs,204 倍余量,再翻一倍也照样绿。
/// 2. 真正兜住形状的是 `the_attention_arm_stays_fts_driven`(下方单测):它用
///    `EXPLAIN QUERY PLAN` 钉住保底臂仍由 `blocks_fts` 驱动。会让成本量级跳变
///    的改动(丢掉 MATCH、改成扫 `blocks`、加相关子查询)都会先在那里变红。
///    但它管的是**计划形状**,不是绝对耗时 —— 动这段 SQL 请人工量一次。
///
/// `doc_attention` 上曾经有一条 `doc_attention_minutes(minutes DESC)` 索引,
/// 就是为这条臂建的。实测它**用不上**(见 `store.rs` v6 注释):查询由 FTS 驱
/// 动、`doc_attention` 是被 join 的内表,`ORDER BY att.minutes DESC` 只能靠
/// TEMP B-TREE,换 `INNER JOIN` 也一样。索引已删。
///
/// # `k = 0` 必须连臂一起关(最终评审 I-1)
///
/// 规格 §4.2/§7 把 `k = 0` 定义成**回滚开关**:「全链路输出与接入前逐位相同」。
/// 只让 `attention::boost` 返回 1.0 是**不够**的 —— 保底臂捞回来的行照样要过
/// `finish`,origin 四档乘数、`agent_by ×0.85`、`doc_date` 新鲜度加成对它们
/// 照常作用。于是一份 bm25 在窗口外、但 `origin=human`(×1.25)的文档,完全
/// 可以压过窗口内 `origin=unlabeled`(×0.3)的文档,而它能进入评分阶段这件事
/// **只由注意力臂促成**。实测(80 篇噪声 + 1 篇读了 10 小时的 human 文档,
/// `limit=10`、`attention: 0.0`):摄取前目标落榜,摄取后升到第 1 名并挤掉了
/// 第 10 名 —— 功能「关着」,结果却变了。所以闸门在**构造**这一步:`k <= 0`
/// 时压根不生成第二条臂,`fts_search` 也就没有第二条 SQL 可跑。
///
/// 附带好处:关掉功能的用户不再为一条必然为空的结果集付 +46%~+62% 的 SQL 成本。
/// 端到端的等价性由 acceptance 的 `k_zero_keeps_the_whole_pipeline_identical_
/// to_having_no_attention_data` 钉住(它比 `score_of` 层那两条更强:比的是最终
/// 可见的 hit 序列)。
fn fts_arms(
    q: &Query,
    expr: String,
    limit: usize,
    k: f64,
) -> ((String, Vec<String>), Option<(String, Vec<String>)>) {
    let base = format!(
        // Column weights, in `blocks_fts`'s declared order: `tok_text`,
        // `tok_breadcrumb`, `tok_title`. The title's 4.0 is the point of
        // giving it its own column at all — appended to `tok_text` instead it
        // would share that column's length normalization with the whole
        // document body, and a File-level block's body is the entire file, so
        // a name match would be diluted to near-nothing on exactly the long
        // documents where finding a file by its name matters most.
        "SELECT {SELECT_COLS}, bm25(blocks_fts, 1.0, 2.0, 4.0) AS rank
         FROM blocks_fts
         JOIN blocks b ON b.id = blocks_fts.rowid
         JOIN files f ON f.id = b.file_id{ATTENTION_JOIN}
         WHERE blocks_fts MATCH ?1"
    );

    // 主臂:相关度。
    let mut sql = base.clone();
    let mut args: Vec<String> = vec![expr.clone()];
    push_filters(q, &mut sql, &mut args);
    // Over-fetch: business boosts reorder, and a phrase recheck removes rows.
    sql.push_str(&format!(" ORDER BY rank ASC LIMIT {}", (limit * 8).max(64)));

    // `k <= 0` = 用户把功能关了(`Weights::sanitized` / `validate_search_weights`
    // 都刻意放行 0,这是唯一的关闭途径)。此时保底臂**不构造** —— 见上面
    // 「`k = 0` 必须连臂一起关」。NaN 单独判一次也走关闭分支(`k <= 0.0` 对
    // NaN 是 false):读取侧 `sanitized` 已经把 NaN 换成了默认值,这里不依赖它。
    if k.is_nan() || k <= 0.0 {
        return ((sql, args), None);
    }

    // 保底臂:注意力。两条臂**共用同一个 `base`**,也就是同一个 MATCH 和同一
    // 套 `push_filters` 过滤器 —— 这是硬约束,不是巧合:「我读得最多的文档」
    // 不是「我搜的东西」,这条臂一条不匹配的结果都不许引入。所以 `base` 只构
    // 造一次再 clone,而不是抄两份 SQL:抄的那份迟早跟主臂漂移,而漂移的症状
    // 正是「搜什么都出现我常读的那几篇」。
    let mut arm = base;
    let mut arm_args: Vec<String> = vec![expr];
    push_filters(q, &mut arm, &mut arm_args);
    arm.push_str(&format!(
        // `att.minutes` 全表共用一个 `as_of`,`finish` 的二次衰减对所有行是
        // 同一个单调乘数,所以按存量排序 == 按衰减后排序,SQL 里不需要算
        // 指数。`IS NOT NULL` 而不是 `> 0`:`LEFT JOIN` 未命中的行(绝大多数
        // 文件)才是要排除的那批,读过但折算成 0 分钟的文件留着无害。
        //
        // `(limit * 2).max(8)` 刻意取小:保底,不是主力。
        " AND att.minutes IS NOT NULL ORDER BY att.minutes DESC, rank ASC LIMIT {}",
        (limit * 2).max(8)
    ));

    ((sql, args), Some((arm, arm_args)))
}

/// 跑一条 FTS 候选臂,读出 `finish` 要的原始行。
///
/// 存在的理由是**列索引只写一遍**:两条臂 SELECT 的列完全相同,各自抄一份
/// `r.get(...)` 就等于把 C-T7 的地雷(`rank` 在索引 15,读错不报类型错、只让
/// bm25 静默失效)复制成两颗。
fn run_fts_arm(
    conn: &Connection,
    sql: &str,
    args: &[String],
) -> rusqlite::Result<(Vec<RawRow>, bool)> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok((
            row_to_hit(r)?,
            r.get::<_, f64>(15)?, // ← rank,因 SELECT_COLS 加了两列而位移
            r.get::<_, i64>(9)? != 0,
            r.get(12)?,
            r.get::<_, f64>(13)?, // att.minutes(已 COALESCE)
            r.get::<_, Option<String>>(14)?, // att.as_of
        ))
    })?;
    drain(rows)
}

/// **这条路径与 `filter_only_search` 都不加注意力候选臂(规格 §5)。** 别
/// 「顺手补齐」—— 这是决定,不是遗漏。理由有二:一,截断压力不在这里,两条
/// 路径都是 `LIMIT 500` / `(limit*8).max(64)` 的宽扫,能匹配上的东西本来就
/// 基本都在候选集里了,而第二条臂存在的唯一意义就是对抗 `fts_search` 那个
/// 按 bm25 排序的窄窗口把长文提前砍掉;二,LIKE 是全表 `%needle%` 扫描,本
/// 来就是最慢的兜底路径,再挂一条同样形状的查询是把最贵的路径的成本翻倍去
/// 换一个它并不存在的问题。注意力仍然照常参与这两条路径的**排序**
/// (`SELECT_COLS` 读出 `att.minutes`,`score_of` 加成),只是不额外召回。
fn like_search(
    conn: &Connection,
    q: &Query,
    limit: usize,
    today: &str,
    weights: &Weights,
    conventions: &Conventions,
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
         FROM blocks b JOIN files f ON f.id = b.file_id{ATTENTION_JOIN}
         WHERE {}",
        clauses.join(" AND ")
    );
    push_filters(q, &mut sql, &mut args);
    // Hard cap: the fallback is a safety net, not a query plan.
    sql.push_str(" LIMIT 500");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok((
            row_to_hit(r)?,
            -1.0f64,
            r.get::<_, i64>(9)? != 0,
            r.get(12)?,
            r.get::<_, f64>(13)?, // att.minutes(已 COALESCE)
            r.get::<_, Option<String>>(14)?, // att.as_of
        ))
    })?;
    let (rows, truncated) = drain(rows)?;
    Ok((finish(rows, q, limit, today, weights, conventions)?, truncated))
}

/// `search_with`'s handling for a query that has filters (`origin:`, `type:`,
/// `tag:`, …) but no terms/phrases to `MATCH` — see the call site's doc
/// comment for why this exists (it used to silently fall through to an
/// empty result, breaking `origin:unlabeled` and every other bare-filter
/// query the documented grammar advertises).
///
/// **One hit per file, not one hit per matching block (review round 2).**
/// A query with no terms isn't asking "where in the text" — it's asking
/// "which FILES". Design spec §7.4's settings-page row promises "列出所有
/// 该补 frontmatter 的文件" (list every file that needs frontmatter), one
/// entry per file. The first version of this function selected every
/// matching BLOCK (`FROM blocks b JOIN files f`, `ORDER BY f.path,
/// b.line_start LIMIT (limit*8).max(64)`) — for any file with more
/// paragraphs than that cap (an ordinary transcript: dozens of paragraphs
/// against a `limit 20` cap), that ONE file consumed the entire budget and
/// every other matching file was invisible, with `truncated` staying
/// `false` — a confident lie, since the query looked complete. Fixed by
/// picking exactly one block per file (that file's own first block, via a
/// correlated subquery ordered by `line_start` then `id`), so the query now
/// runs `FROM files f JOIN blocks b ON b.id = (...)` — filters are all on
/// `f.*` (`push_filters` never touches `b.*`), so reordering the join
/// changes nothing else — and the cap bounds DISTINCT FILES, not blocks.
/// Passage-level results (potentially several hits per file) are untouched
/// for any query that still has terms/phrases — this only applies here, to
/// the bare-filter path.
///
/// The `, id ASC` tiebreak is load-bearing, not decoration: `prose.rs`'s
/// whole-document rollup block (`BlockLevel::File`) always starts at
/// `line_start = 1`, the same value a document's very first paragraph
/// starts at when there's no leading blank line — a real, common tie. That
/// rollup is always the LAST block `chunk()` appends for a file (pushed
/// after every Line/Section block — see `prose.rs`'s `chunk` — so it always
/// gets the highest `id` among a file's blocks), so `id ASC` deterministically
/// resolves the tie toward the finer, more specific first paragraph rather
/// than a whole-file text dump, and — just as important — makes which block
/// gets picked reproducible instead of whatever order SQLite's query plan
/// happens to visit ties in.
///
/// Cost note for C-T12 (index volume): the correlated subquery runs once
/// per file that survives `push_filters`'s `WHERE` clause — cheap via the
/// existing `blocks_file` index to seek straight to that file's rows, then
/// an UNINDEXED scan bounded by that ONE file's own block count to find the
/// minimum `line_start` (there is no `(file_id, line_start)` index).
/// Ordinary files cost nothing measurable; a single file with an unusually
/// large block count would make only ITS OWN subquery relatively more
/// expensive, never the whole corpus the way the pre-fix `ORDER BY` over
/// every matching block was.
///
/// Reuses `SELECT_COLS`/`push_filters`/`finish`, the same idiom `like_search`
/// above uses. `rank` has no bm25 to read (nothing was MATCHed), so every
/// row gets the same hardcoded neutral base (`-1.0`, same as `like_search`'s)
/// — `finish`/`score_of` still apply their other boosts (origin tier,
/// recency, verification, annotation) on top of it. Capped at the same
/// over-fetch idiom `fts_search` uses (`(limit * 8).max(64)`), not
/// `like_search`'s flat 500 — a filter-only query is a first-class,
/// documented route now, not a last-resort safety net.
fn filter_only_search(
    conn: &Connection,
    q: &Query,
    limit: usize,
    today: &str,
    weights: &Weights,
    // Threaded for uniformity with the other two retrieval paths, never
    // load-bearing here: a filter-only query has no term or phrase to be
    // "the page for", so `pin_keyword` refuses it twice over.
    conventions: &Conventions,
) -> rusqlite::Result<(Vec<Hit>, bool)> {
    let mut sql = format!(
        "SELECT {SELECT_COLS}
         FROM files f
         JOIN blocks b ON b.id = (
             SELECT b2.id FROM blocks b2 WHERE b2.file_id = f.id ORDER BY b2.line_start ASC, b2.id ASC LIMIT 1
         ){ATTENTION_JOIN}
         WHERE 1 = 1"
    );
    let mut args: Vec<String> = Vec::new();
    push_filters(q, &mut sql, &mut args);
    sql.push_str(&format!(" ORDER BY f.path ASC LIMIT {}", (limit * 8).max(64)));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok((
            row_to_hit(r)?,
            -1.0f64,
            r.get::<_, i64>(9)? != 0,
            r.get(12)?,
            r.get::<_, f64>(13)?, // att.minutes(已 COALESCE)
            r.get::<_, Option<String>>(14)?, // att.as_of
        ))
    })?;
    let (rows, truncated) = drain(rows)?;
    Ok((finish(rows, q, limit, today, weights, conventions)?, truncated))
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
        // Set by `finish`, which is where the query and the vault's
        // conventions are both in scope; a row on its own cannot know.
        pinned: false,
        attention_minutes: 0.0, // `finish` 用 `today` 二次衰减后覆盖
    })
}

/// 一行原始结果:`Hit` 本体,加上三条查询路径各自读出、但**不属于 `Hit`
/// 公开形状**的四样东西 —— `rank`、`is_annotation`、`f.title`(只有置顶判定
/// 用),以及 `doc_attention` 的 `minutes` / `as_of`(要用 `today` 二次衰减
/// 后才成为 `Hit::attention_minutes`)。命名而不是裸 6 元组,纯为可读。
type RawRow = (Hit, f64, bool, Option<String>, f64, Option<String>);

fn finish(
    rows: Vec<RawRow>,
    q: &Query,
    limit: usize,
    today: &str,
    weights: &Weights,
    conventions: &Conventions,
) -> rusqlite::Result<Vec<Hit>> {
    // Resolved once for the whole result set, not per row: both halves depend
    // only on the query and the settings.
    let pin = pin_keyword(q).zip(conventions.wikipage_dir.as_deref());
    let mut out: Vec<Hit> = Vec::new();
    for (mut hit, rank, is_annotation, title, minutes, as_of) in rows {
        // 表每天重算,但 app 开着不动时存量会冻住;按 as_of 到今天的天数
        // 再衰减一次,让陈旧的表优雅退化而不是发出过期的高分。`as_of` 为
        // NULL(该文件根本没有注意力行)时 minutes 也是 0,衰减多少都是 0。
        let age = as_of.as_deref().and_then(|d| days_between(d, today)).unwrap_or(0);
        hit.attention_minutes = minutes * crate::attention::decay(age);
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
        let mention = linked_mention(&hit.text, q);
        hit.score = score_of(rank, &hit, is_annotation, phrase_exact, mention, today, weights);
        hit.pinned =
            pin.is_some_and(|(kw, dir)| is_the_named_wikipage(&hit.path, title.as_deref(), kw, dir));
        out.push(hit);
    }
    let mut out = drop_redundant_rollups(out);
    // Pinned first, then by score. A comparison, not a huge multiplier: the
    // whole point of "硬置顶" is that no combination of bm25 and boosts can
    // outrun it, which a multiplier — however large — cannot promise.
    out.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then_with(|| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal))
    });
    out.truncate(limit);
    Ok(out)
}

/// The single keyword this query is "the page for", if it is one at all.
///
/// `None` for anything with a filter or more than one term/phrase (wikipage
/// priority spec §4): 「张三 电话」 is a search, not a request to open 张三's
/// page, and a pin that jumped the queue there would be插队 rather than help.
/// A quoted single phrase counts — `"张三"` is the same intent typed more
/// carefully.
fn pin_keyword(q: &Query) -> Option<&str> {
    let filtered = !q.tags.is_empty()
        || !q.types.is_empty()
        || !q.paths.is_empty()
        || !q.exts.is_empty()
        || !q.origins.is_empty()
        || !q.pages.is_empty()
        || q.after.is_some()
        || q.before.is_some();
    if filtered {
        return None;
    }
    let kw = match (q.terms.as_slice(), q.phrases.as_slice()) {
        ([t], []) | ([], [t]) => t.trim(),
        _ => return None,
    };
    (!kw.is_empty()).then_some(kw)
}

/// Is `path` the wikilink page named exactly `keyword`?
///
/// Matches on the filename stem OR the frontmatter title, because a page
/// created by clicking a `[[…]]` keeps the display name in `title` and a
/// slugged version in the filename (`src/lib/outline/create.ts`) — checking
/// only the filename would leave that entire class of pages unpinnable.
///
/// Equality, never containment: `[[张三]]` and `[[张三的项目]]` are two
/// different pages, and only one of them is "the page for 张三". (The ×1.5
/// mention boost in `linked_mention` deliberately goes the other way — that
/// one ranks, this one jumps the queue, and a queue-jump has to be exact.)
fn is_the_named_wikipage(path: &str, title: Option<&str>, keyword: &str, dir: &str) -> bool {
    // A `dir` of `""` would make `starts_with` true for every path in the
    // vault, pinning any file whose name matches — fail closed instead.
    if dir.is_empty() || !path.starts_with(dir) || path.as_bytes().get(dir.len()) != Some(&b'/') {
        return false;
    }
    // `to_lowercase` rather than `eq_ignore_ascii_case`: vault names are
    // routinely non-ASCII, and a case fold that silently stops working
    // outside ASCII is the kind of half-measure that looks fine in tests
    // written in English.
    let want = keyword.trim().to_lowercase();
    let same = |s: &str| s.trim().to_lowercase() == want;
    crate::chunk::stem(path).is_some_and(|s| same(&s)) || title.is_some_and(same)
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

/// True when any of the query's terms or phrases appears inside a `[[…]]`
/// wikilink's target in `text` (wikipage priority spec §5).
///
/// **Substring, not equality, and deliberately so** — this口径 was the
/// explicitly chosen half of a two-way decision at design time: searching
/// 「张三」 must also reward a block linking `[[张三的项目]]`. Do not "tighten"
/// this to an equality check; `a_wikilink_target_merely_containing_the_term_
/// still_counts` exists to make that change go red.
///
/// Only the target half of `[[target|display]]` counts: `[[项目|张三]]` points
/// at 项目, and treating its display text as a link to 张三 would credit a
/// link that does not exist.
///
/// Parsing goes through `links::extract` rather than a second `[[`-scanner
/// written here, so this and the `links` table can never disagree about what
/// a wikilink is. `extract` also collects `[](…)` markdown links, which are
/// filtered out below — a little wasted work in exchange for one definition
/// of the syntax.
fn linked_mention(text: &str, q: &Query) -> bool {
    let needles: Vec<String> = q
        .terms
        .iter()
        .chain(q.phrases.iter())
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if needles.is_empty() {
        return false;
    }
    crate::links::extract(text, 1).iter().any(|l| {
        l.kind == "wiki" && {
            let target = l.target.to_lowercase();
            needles.iter().any(|n| target.contains(n.as_str()))
        }
    })
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
///
/// `weights` carries the one boost group that is no longer a fixed constant —
/// the four origin-tier multipliers below — because C-T7 makes them
/// user-tunable. Every other boost here stays a hardcoded literal on purpose;
/// only provenance tiering is a settings-page knob (spec §7.1).
fn score_of(
    rank: f64,
    hit: &Hit,
    is_annotation: bool,
    phrase_exact: bool,
    linked_mention: bool,
    today: &str,
    weights: &Weights,
) -> f64 {
    let mut r = if rank < 0.0 { -rank } else { 0.001 };
    if phrase_exact {
        r *= 1.3;
    }
    // The wikipage priority spec's §5. A product claim, not a tuned number:
    // a block where someone bothered to write `[[张三]]` is about 张三 in a
    // way a block that merely says the words is not. Deliberately NOT gated
    // on the query being a single keyword (unlike the pin in `pinned_by`) —
    // "find me 张三's phone number" is exactly when the linked mention should
    // win. Fires at most once per hit, however many links match: this
    // rewards "was it linked", and multiplying per matched term would let a
    // long query run the boost away.
    if linked_mention {
        r *= 1.5;
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
        Origin::Human => weights.human,
        Origin::Derived => weights.derived,
        Origin::Source => weights.source,
        // The ×0.3 default is a deliberate strong penalty, not a token
        // nudge: stacked with the default top-20 limit, it is usually enough
        // to push an unlabeled file out of the visible result set entirely.
        // That is the accepted, confirmed design (spec §3.1) — unlabeled
        // material is real and searchable via `origin:unlabeled`, it just
        // does not compete for the front page by default.
        Origin::Unlabeled => weights.unlabeled,
    };
    // 注意力加权(规格 §4.2)。与上面所有档一样是乘性的,但只向上:
    // `attention::boost` 在 0 分钟时严格返回 1.0,所以从没打开过的文档
    // ——包括 agent 昨天刚生成、你还没来得及读的那些——原地不动。
    // 这与 `doc_date` 那档的时间衰减是两件事:那个衰减「文档写于何时」,
    // 这个衰减「你何时在它身上花过时间」。
    //
    // `hit.attention_minutes` 已由 `finish` 衰减到今天,所以这里传 0。
    r *= crate::attention::boost(hit.attention_minutes, 0, weights.attention);
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
///
/// `pub(crate)` since the attention ingest needs the same civil-day arithmetic
/// and this crate's house rule is that a utility stays where it was born and
/// gets exported (same as `chunk::ymd_from_unix_public`) rather than being
/// moved into a new "utils" module nobody owns.
pub(crate) fn days_between(from: &str, to: &str) -> Option<i64> {
    Some((days_from_civil(to)? - days_from_civil(from)?).max(0))
}

pub(crate) fn days_from_civil(ymd: &str) -> Option<i64> {
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

    /// Shared "today" for the pure `score_of` tests below, so the date-decay
    /// boost never activates by accident (every fixture `Hit` has `doc_date:
    /// None`, but reusing one literal instead of repeating `"2026-08-10"`
    /// keeps the pure-function tests independent of the SQL-backed ones,
    /// which still use their own literal per-test dates).
    const TODAY: &str = "2026-08-10";

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

    /// C-T5, spec §5.1: `files.ext` must carry a transcript's real
    /// extension, not always `"md"` — and the whole point of storing it is
    /// that it's observable through the query language's own `ext:` filter
    /// (see `every_filter_prefix_is_recognized` above), not just readable by
    /// a raw `SELECT`. Goes through `build_full` (not `chunk::parse_file`
    /// directly), so a bug in `store::replace_file`'s `ext` argument is
    /// caught too, not just a chunk-time computation that never reaches SQL.
    /// `indexed()` above can't be reused because its `ScanOptions::default()`
    /// carries no source globs, and `is_indexable` would reject the `.srt`
    /// file outright.
    #[test]
    fn ext_filter_finds_a_transcript_by_its_real_extension_not_md() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("media")).unwrap();
        std::fs::write(d.path().join("media/talk.srt"), "1\n00:00:01,000 --> 00:00:02,000\nhello world\n").unwrap();
        let opts = crate::scan::ScanOptions {
            source_globs: crate::globs::parse(&["media/**".to_string()]),
            ..Default::default()
        };
        let mut c = crate::store::open(&d.path().join(".idx.db"), "v", "sync").unwrap();
        crate::scan::build_full(&mut c, d.path(), &opts, None).unwrap();

        let srt_hits = search(&c, &parse("ext:srt hello"), 20, "2026-08-10").unwrap().0;
        assert!(srt_hits.iter().any(|h| h.path == "media/talk.srt"), "{srt_hits:?}");

        let md_hits = search(&c, &parse("ext:md hello"), 20, "2026-08-10").unwrap().0;
        assert!(md_hits.is_empty(), "a subtitle file must not masquerade as ext:md: {md_hits:?}");
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
        let a = search_with(&c, &parse("target 慕"), 20, "2026-08-10", &shallow, &Weights::default(), &Conventions::default()).unwrap();
        assert_eq!(a.route.as_str(), "t1-fts", "the fallback must not have run");
        assert!(a.hits.is_empty());
        assert!(a.deep_available, "the caller has to be able to offer the deep search");

        let deep = search_with(&c, &parse("target 慕"), 20, "2026-08-10", &Limits::full(), &Weights::default(), &Conventions::default()).unwrap();
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
        let a = search_with(&c, &parse("慕"), 20, "2026-08-10", &limits, &Weights::default(), &Conventions::default()).unwrap();
        assert!(calls.load(std::sync::atomic::Ordering::Relaxed) > 0, "abort was never consulted");
        assert!(a.truncated, "an aborted retrieval must say so, not pose as a complete answer");

        // And the handler must not outlive the call: the next caller through
        // this connection is usually the watcher's sweep or a rebuild, and an
        // always-true abort left installed would kill it.
        let after = search_with(&c, &parse("慕"), 20, "2026-08-10", &Limits::full(), &Weights::default(), &Conventions::default()).unwrap();
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

    /// C-T9: the fourth tier parses the same way the first three do — `parse`
    /// never validates the value (see the comment on the `origin:` arm above),
    /// so this is really pinning that `origin:unlabeled` is not somehow
    /// special-cased or rejected, the way an allowlist implementation might
    /// have been tempted to.
    #[test]
    fn origin_unlabeled_is_recognized_by_parse() {
        let q = parse("target origin:unlabeled");
        assert_eq!(q.origins, vec!["unlabeled"]);
        assert_eq!(q.terms, vec!["target"]);
    }

    /// Task 6: `origin:` narrows to one tier. Three files, three tiers, via the
    /// same `origin::derive` rules the real index uses — `.note.md` is rule 1
    /// (Human), `type: Book Summary` maps to Derived, `type: Book` maps to
    /// Source (see `origin::mapped_type_origin`).
    ///
    /// C-T9 adds a fourth file, `d.md`, with no frontmatter and no configured
    /// source glob (`indexed()`'s `ScanOptions::default()` carries none) — the
    /// same rule 6′ shape `a_hits_origin_round_trips_through_the_real_index`
    /// above exercises — so `origin:unlabeled` is proven to narrow the same
    /// way the other three tiers already do, not merely to parse.
    #[test]
    fn the_origin_filter_narrows_to_a_single_tier() {
        let (_d, c) = indexed(&[
            ("a.note.md", "target\n"),
            ("b.md", "---\ntype: Book Summary\n---\ntarget\n"),
            ("c.md", "---\ntype: Book\n---\ntarget\n"),
            ("d.md", "target\n"),
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
        assert_eq!(only("target origin:unlabeled"), vec!["d.md"]);
        assert_eq!(
            only("target"),
            vec!["a.note.md", "b.md", "c.md", "d.md"],
            "sanity: all four recall without a filter"
        );
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
    /// never stores anything but `human`/`derived`/`source`/`unlabeled`, so it
    /// naturally matches zero rows without any special-cased validation.
    /// `a.md` carries no frontmatter, so this fixture is itself an `Unlabeled`
    /// file (rule 6′) — proving `origin:bogus` fails closed even against the
    /// tier added by C-T9, not just the three that predate it.
    #[test]
    fn an_unrecognized_origin_value_matches_nothing_not_everything() {
        let (_d, c) = indexed(&[("a.md", "target\n")]);
        let hits = search(&c, &parse("target origin:bogus"), 20, "2026-08-10").unwrap().0;
        assert!(hits.is_empty(), "an invalid origin: value must fail closed, not silently return every tier: {hits:?}");
    }

    /// Review round 1, Critical 1: a bare filter query — no terms, no
    /// phrases, just `origin:unlabeled` — used to structurally return
    /// nothing. `match_expr` returns `None` when there are no terms/phrases,
    /// so `fts_search` short-circuited before ever calling `push_filters`;
    /// `needs_scan_fallback` only inspects terms/phrases too, so it never
    /// tripped the LIKE fallback either. The filters were simply never
    /// applied. This is the one documented exit from the ×0.3 unlabeled
    /// demotion (design spec §3.1/§6.3, AGENTS.md's `origin:unlabeled`
    /// example, and the settings page's clickable "Unlabeled" statistics
    /// row) — it has to actually retrieve something. `a.note.md` (Human) and
    /// `c.md` (a registered `type:`, so Derived) prove the filter narrows,
    /// not just that SOMETHING comes back.
    #[test]
    fn a_bare_filter_query_with_no_terms_or_phrases_still_retrieves() {
        let (_d, c) = indexed(&[
            ("d.md", "no frontmatter at all\n"),
            ("e.md", "also no frontmatter\n"),
            ("a.note.md", "written by a human\n"),
            ("c.md", "---\ntype: Book Summary\n---\nderived content\n"),
        ]);
        let mut hits = search(&c, &parse("origin:unlabeled"), 20, "2026-08-10").unwrap().0;
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        let paths: Vec<_> = hits.iter().map(|h| h.path.clone()).collect();
        assert_eq!(paths, vec!["d.md", "e.md"], "{hits:?}");
    }

    /// Review round 2: a filter-only query is asking "which FILES", not
    /// "where in the text" — the fix above returned real rows, but a
    /// multi-paragraph file still dominated the whole result set (one hit
    /// per BLOCK, not per file), which for design spec §7.4's settings-page
    /// row means a real vault's `origin:unlabeled` click lists one file
    /// twenty times instead of twenty files once. `f1.md` has five
    /// paragraphs (five separate `Line` blocks) plus the two ordinary
    /// single-paragraph files — a correct implementation returns exactly
    /// one hit per file, not seven.
    #[test]
    fn a_filter_only_query_returns_one_hit_per_file_not_one_per_block() {
        let (_d, c) = indexed(&[
            (
                "f1.md",
                "paragraph one, no frontmatter\n\n\
                 paragraph two, no frontmatter\n\n\
                 paragraph three, no frontmatter\n\n\
                 paragraph four, no frontmatter\n\n\
                 paragraph five, no frontmatter\n",
            ),
            ("f2.md", "a second unlabeled file, one paragraph\n"),
            ("f3.md", "a third unlabeled file, one paragraph\n"),
        ]);
        let mut hits = search(&c, &parse("origin:unlabeled"), 20, "2026-08-10").unwrap().0;
        hits.sort_by(|a, b| a.path.cmp(&b.path));
        let paths: Vec<_> = hits.iter().map(|h| h.path.clone()).collect();
        // Exactly one entry per file — if `f1.md`'s five paragraphs each
        // produced their own hit, this would be `["f1.md", "f1.md", "f1.md",
        // "f1.md", "f1.md", "f2.md", "f3.md"]` instead.
        assert_eq!(paths, vec!["f1.md", "f2.md", "f3.md"], "{hits:?}");
    }

    /// The other half of the fix above: a query that is entirely empty — no
    /// terms, no phrases, and (unlike the test above) no filters either —
    /// must still return nothing. The new filter-only path is gated on
    /// `has_filters` specifically so this doesn't regress into "no query at
    /// all silently returns the whole vault".
    #[test]
    fn a_fully_empty_query_with_no_filters_either_still_returns_nothing() {
        let (_d, c) = indexed(&[("a.md", "hello world\n")]);
        let hits = search(&c, &parse(""), 20, "2026-08-10").unwrap().0;
        assert!(hits.is_empty(), "{hits:?}");
    }

    /// Review round 2 nit, upgraded to a real pin: `has_filters` (gates
    /// `search_with`'s filter-only branch) and `push_filters` (does the
    /// actual filtering) are two hand-written lists of the same eight
    /// fields with nothing forcing them to agree. A filter added to
    /// `push_filters` — a new `Query` field that a future task teaches the
    /// grammar to parse — but forgotten in `has_filters` would silently
    /// take a bare (no terms/phrases) query for THAT filter straight back
    /// to "return nothing", the exact class of bug Critical 1 (round 1) was.
    /// Calls both real, private functions directly (this test lives inside
    /// `query.rs`, so it isn't reimplementing either one) with each filter
    /// field set one at a time, and asserts `has_filters` agrees with
    /// whether `push_filters` actually emitted a clause.
    #[test]
    fn has_filters_agrees_with_push_filters_for_every_filter_field() {
        let one_field_set: Vec<Query> = vec![
            Query { tags: vec!["x".into()], ..Default::default() },
            Query { types: vec!["x".into()], ..Default::default() },
            Query { paths: vec!["x".into()], ..Default::default() },
            Query { exts: vec!["x".into()], ..Default::default() },
            Query { origins: vec!["x".into()], ..Default::default() },
            Query { pages: vec!["x".into()], ..Default::default() },
            Query { after: Some("2026-01-01".into()), ..Default::default() },
            Query { before: Some("2026-01-01".into()), ..Default::default() },
        ];
        for q in one_field_set {
            let mut sql = String::new();
            let mut args: Vec<String> = Vec::new();
            push_filters(&q, &mut sql, &mut args);
            let push_filters_emitted_a_clause = !sql.is_empty();
            assert_eq!(
                has_filters(&q), push_filters_emitted_a_clause,
                "has_filters/push_filters disagree for {q:?} (sql: {sql:?})"
            );
        }

        // The converse, over the SAME two functions: nothing set, nothing
        // emitted, `has_filters` says so.
        let empty = Query::default();
        let mut sql = String::new();
        let mut args: Vec<String> = Vec::new();
        push_filters(&empty, &mut sql, &mut args);
        assert!(sql.is_empty(), "{sql:?}");
        assert!(!has_filters(&empty));
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
            pinned: false,
            attention_minutes: 0.0,
        }
    }

    // --- 硬置顶(wikipage 检索优先级 spec §4)---------------------------------

    /// §4:置顶要求**精确相等**,与 `linked_mention` 的子串口径**刻意相反**。
    /// `[[张三]]` 和 `[[张三的项目]]` 是两个不同的页,只有一个是「张三这个词
    /// 的页」;插队必须精确,加权可以宽。这条与
    /// `a_wikilink_target_merely_containing_the_term_still_counts` 是一对,
    /// 谁被顺手改成跟另一个一致,都会有一条红。
    #[test]
    fn pinning_requires_an_exact_name_not_merely_a_containing_one() {
        assert!(is_the_named_wikipage("wikipage/张三.md", None, "张三", "wikipage"));
        assert!(!is_the_named_wikipage("wikipage/张三的项目.md", None, "张三", "wikipage"));
    }

    /// §4:文件名 stem **或** frontmatter title 任一精确匹配即可 —— 建页时
    /// 文件名 slug 化、title 存原文,只认一边会漏掉一整类页面。
    #[test]
    fn either_the_filename_stem_or_the_title_can_carry_the_name() {
        assert!(is_the_named_wikipage("wikipage/zhang-san.md", Some("张三"), "张三", "wikipage"));
        assert!(is_the_named_wikipage("wikipage/张三.md", Some("别的标题"), "张三", "wikipage"));
    }

    /// 目录前缀必须整段匹配,不能是「前缀字符串」—— 否则配置成 `wiki` 时
    /// `wikipage/` 下的文件会跟着一起被置顶。
    #[test]
    fn the_directory_must_match_a_whole_path_segment() {
        assert!(!is_the_named_wikipage("wikipage/张三.md", None, "张三", "wiki"));
    }

    /// 空目录名(设置被清空/读失败)必须失败关闭。否则 `starts_with("")` 对
    /// vault 里每个路径都为真,任何同名文件都会被置顶。
    #[test]
    fn an_empty_directory_setting_pins_nothing() {
        assert!(!is_the_named_wikipage("张三.md", None, "张三", ""));
    }

    /// §4 的触发条件:单一关键词、且不带任何过滤器。
    #[test]
    fn only_a_single_unfiltered_keyword_is_a_pin_candidate() {
        assert_eq!(pin_keyword(&parse("张三")), Some("张三"));
        assert_eq!(pin_keyword(&parse("\"张三\"")), Some("张三"), "引号短语是同一个意图");
        assert_eq!(pin_keyword(&parse("张三 电话")), None, "多词查询不是「某个词的页」");
        assert_eq!(pin_keyword(&parse("张三 ext:md")), None, "带过滤器时用户在做精确检索");
        assert_eq!(pin_keyword(&parse("ext:md")), None, "没有关键词就没有可置顶的名字");
    }

    // --- [[提及]] ×1.5(wikipage 检索优先级 spec §5)------------------------

    /// §5:命中块里以 `[[…]]` 形式出现的关键词要加权。裸写的关键词不加 ——
    /// 这一档奖励的是「有人主动把它连成了链接」,不是又一次词频。
    #[test]
    fn a_wikilink_to_the_term_is_a_mention_but_bare_text_is_not() {
        let q = parse("张三");
        assert!(linked_mention("昨天见了 [[张三]]", &q));
        assert!(!linked_mention("昨天见了张三", &q));
    }

    /// §5 已确认的**放宽**口径:target 子串包含关键词即算,不要求精确相等。
    /// 搜「张三」时 `[[张三的项目]]` 同样加权。这条专门钉住这个放宽,防止
    /// 后来者「顺手」改回精确相等 —— 那是设计阶段被明确否掉的方案。
    #[test]
    fn a_wikilink_target_merely_containing_the_term_still_counts() {
        assert!(linked_mention("见 [[张三的项目]]", &parse("张三")));
    }

    /// §5:不限定单一关键词。多词查询里任一词被连成链接即加权 ——
    /// 「找张三相关的电话」正是该优先的场景。
    #[test]
    fn a_multi_term_query_still_gets_the_mention_boost_from_one_of_its_terms() {
        assert!(linked_mention("[[张三]] 的电话是 123", &parse("张三 电话")));
    }

    /// `[[target|display]]`:连的是 target,显示名不算。否则 `[[项目|张三]]`
    /// 会被当成指向张三的链接,而它指向的是项目。
    #[test]
    fn only_the_target_half_of_a_piped_wikilink_counts_not_the_display_text() {
        assert!(linked_mention("见 [[张三|老张]]", &parse("张三")));
        assert!(!linked_mention("见 [[项目|张三]]", &parse("张三")));
    }

    /// 引号短语与裸词一视同仁 —— 两者都是「用户要找的东西」,
    /// `finish` 在别处也是把 terms 和 phrases 连起来一起处理的。
    #[test]
    fn a_quoted_phrase_also_earns_the_mention_boost() {
        assert!(linked_mention("见 [[张三]]", &parse("\"张三\"")));
    }

    /// 纯函数断言,不走 bm25 —— 照 `score_of_boosts_human_verified_content`
    /// 的既有做法,让 1.5 这个常数独立于 SQLite / fixture 长度被钉住。
    #[test]
    fn score_of_boosts_a_linked_mention() {
        let w = Weights::default();
        let base = hit_with(Origin::Derived);
        let plain = score_of(-1.0, &base, false, false, false, TODAY, &w);
        let mentioned = score_of(-1.0, &base, false, false, true, TODAY, &w);
        assert!(mentioned > plain, "[[提及]] 必须抬高分数: {mentioned} vs {plain}");
    }

    /// spec §4 的产品主张,与 origin tiering 设计 §3 的落点:你写的 > agent 生成的
    /// > agent 要读的原始材料。**三档必须各自独立可验** —— 前置项目里出现过「两个
    /// 乘数一起推同一方向、任一个单独失效测试仍通过」的假阴性(见 mutation check
    /// in the task report),所以这里逐档断言 `score_of` 本身,而不是端到端排序,
    /// 且断言两条不等式而非一条,让 mutation check 能分别单独抓到每一档。
    ///
    /// C-T7: now goes through `Weights::default()` explicitly rather than a
    /// baked-in literal inside `score_of` — the four assertions below are
    /// what pin the shipped constants against a regression, not `score_of`
    /// itself (which now just reads whatever `Weights` it is handed).
    #[test]
    fn each_origin_tier_moves_the_score_on_its_own() {
        let w = Weights::default();
        let s = |o| score_of(-1.0, &hit_with(o), false, false, false, TODAY, &w);
        let human = s(Origin::Human);
        let derived = s(Origin::Derived);
        let source = s(Origin::Source);
        let unlabeled = s(Origin::Unlabeled);
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
        let w = Weights::default();
        let base = hit_with(Origin::Derived);
        let plain = score_of(-1.0, &base, false, false, false, TODAY, &w);
        let annotated = score_of(-1.0, &base, true, false, false, TODAY, &w);
        let mut agent_hit = base.clone();
        agent_hit.agent_by = Some("claude/1".to_string());
        let agent = score_of(-1.0, &agent_hit, false, false, false, TODAY, &w);
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
        let w = Weights::default();
        let base = hit_with(Origin::Derived);
        let unverified = score_of(-1.0, &base, false, false, false, TODAY, &w);
        let mut verified_hit = base.clone();
        verified_hit.human_verified = true;
        let verified = score_of(-1.0, &verified_hit, false, false, false, TODAY, &w);
        assert!(verified > unverified, "human_verified boost must raise the score: {verified} vs {unverified}");
    }

    /// 默认值就是已发布的四个常量。前车之鉴:`Limits::default()` 的
    /// `deep: false` 与该类型自己的向后兼容承诺相反,一个未来的
    /// `..Default::default()` 会静默拿到快路径 —— `Weights::default()` 必须
    /// 失败也失败在安全一侧:已发布的行为,而不是某个"中性"占位值。
    #[test]
    fn the_default_weights_are_the_shipped_constants() {
        let w = Weights::default();
        assert_eq!((w.human, w.derived, w.source, w.unlabeled), (1.25, 1.0, 0.9, 0.3));
    }

    /// 非有限/非正/超 5.0 的分量必须回落到默认值,且回落是逐分量的——一个
    /// 分量非法不能连累其他分量。
    #[test]
    fn an_invalid_weight_falls_back_to_the_default() {
        for bad in [f64::NAN, -1.0, 0.0, 6.0] {
            let w = Weights { human: bad, ..Default::default() }.sanitized();
            assert_eq!(w.human, 1.25, "非法值 {bad} 必须回落");
            // The rest of the struct must be untouched by a bad `human`.
            assert_eq!((w.derived, w.source, w.unlabeled), (1.0, 0.9, 0.3));
        }
    }

    /// 用户可以把原始资料调得比你写的还高 —— 那是他自己的 vault。
    #[test]
    fn a_deliberate_inversion_is_allowed() {
        let w = Weights { human: 0.5, source: 2.0, ..Default::default() }.sanitized();
        assert_eq!((w.human, w.source), (0.5, 2.0));
    }

    /// attention 的 sanitize 规则与 origin 四档**相反**:那四档是乘数,0 会让
    /// 整层塌成 0 分、层内顺序变未定义,所以拒绝 0;attention 是加数,k=0
    /// 恰好是「关掉这个功能」的正确表达,必须放行。写成同一条规则就等于
    /// 剥夺了用户关掉它的能力。
    #[test]
    fn attention_weight_allows_zero_but_rejects_garbage() {
        let d = Weights::default();
        assert_eq!(d.attention, 0.4);

        let zero = Weights { attention: 0.0, ..Weights::default() }.sanitized();
        assert_eq!(zero.attention, 0.0, "k=0 必须原样保留 —— 它是关闭开关");

        for bad in [-1.0, f64::NAN, f64::INFINITY, 2.5] {
            let w = Weights { attention: bad, ..Weights::default() }.sanitized();
            assert_eq!(w.attention, d.attention, "非法值 {bad} 必须回落默认");
        }

        let ok = Weights { attention: 1.5, ..Weights::default() }.sanitized();
        assert_eq!(ok.attention, 1.5);
    }

    /// 一个坏的 attention 不得连累其它四档(既有约定,逐档独立回落)。
    #[test]
    fn a_bad_attention_does_not_clobber_the_origin_tiers() {
        let w = Weights { attention: f64::NAN, human: 2.0, ..Weights::default() }.sanitized();
        assert_eq!(w.human, 2.0);
        assert_eq!(w.attention, Weights::default().attention);
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

    // --- 注意力加权(规格 §4.2)------------------------------------------------

    /// 规格 §4.2:注意力只加分。零注意力的命中必须与接入前**逐位相同**。
    #[test]
    fn zero_attention_leaves_the_score_bit_identical() {
        let w = Weights::default();
        let h = hit_with(Origin::Derived);
        assert_eq!(h.attention_minutes, 0.0, "fixture 默认无注意力");
        let with_k = score_of(-1.0, &h, false, false, false, TODAY, &w);
        let no_k = score_of(-1.0, &h, false, false, false, TODAY, &Weights { attention: 0.0, ..w });
        assert_eq!(with_k, no_k, "注意力为 0 时,k 取任何值都不能改变分数");
    }

    /// 注意力必须**单独**能推动分数 —— 逐档隔离断言,不靠端到端排序。
    /// 前置项目(origin tiering)出现过「两个乘数一起推同一方向、任一个
    /// 单独失效测试仍通过」的假阴性,所以这里断言的是 `score_of` 本身。
    #[test]
    fn attention_alone_moves_the_score() {
        let w = Weights::default();
        let cold = hit_with(Origin::Derived);
        let mut warm = hit_with(Origin::Derived);
        warm.attention_minutes = 60.0;
        let a = score_of(-1.0, &cold, false, false, false, TODAY, &w);
        let b = score_of(-1.0, &warm, false, false, false, TODAY, &w);
        assert!(b > a, "读过 60 分钟的必须高于没读过的: {b} vs {a}");
    }

    /// 单调:更多注意力不得让分数下降。`score_of` 末尾的 `r/(1+r)` 压缩
    /// 保序,所以这条在压缩之后依然成立 —— 值得钉住,因为压缩很容易被
    /// 误读成「加成被吃掉了」。
    #[test]
    fn more_attention_never_lowers_the_score() {
        let w = Weights::default();
        let mut last = f64::MIN;
        for m in [0.0, 1.0, 5.0, 30.0, 120.0, 10_000.0] {
            let mut h = hit_with(Origin::Derived);
            h.attention_minutes = m;
            let s = score_of(-1.0, &h, false, false, false, TODAY, &w);
            assert!(s >= last, "m={m} 让分数掉了: {s} < {last}");
            last = s;
        }
    }

    /// k=0 关掉功能后,连高注意力命中也不动分。
    #[test]
    fn k_zero_disables_the_boost_in_score_of() {
        let off = Weights { attention: 0.0, ..Weights::default() };
        let cold = hit_with(Origin::Derived);
        let mut warm = hit_with(Origin::Derived);
        warm.attention_minutes = 10_000.0;
        assert_eq!(
            score_of(-1.0, &cold, false, false, false, TODAY, &off),
            score_of(-1.0, &warm, false, false, false, TODAY, &off)
        );
    }

    /// 与 `a_hits_origin_round_trips_through_the_real_index` 同源的读路径
    /// 保护:上面四条纯函数测试全部手搓 `Hit`,**碰不到 SQL** —— 它们对
    /// 「`SELECT_COLS` 加的两列读错索引」「`finish` 忘了填」都是瞎的。
    ///
    /// 更要紧的是 `LEFT JOIN`:写成 `INNER JOIN`,整个 vault 里没有注意力
    /// 数据的文件会从结果里**整体消失**。`b.md` 在这里就是没有 `doc_attention`
    /// 行的那一半,它必须照常命中且加成为 ×1.0(`attention_minutes == 0`)。
    #[test]
    fn attention_minutes_round_trip_through_the_real_index_without_dropping_unread_files() {
        let (_d, c) = indexed(&[("a.md", "target\n"), ("b.md", "target\n")]);
        crate::store::replace_attention(
            &c,
            "2026-08-10",
            &std::collections::BTreeMap::from([("a.md".to_string(), 60.0)]),
        )
        .unwrap();

        let hits = search(&c, &parse("target"), 20, "2026-08-10").unwrap().0;
        let read = hits.iter().find(|h| h.path == "a.md").expect("a.md must be found");
        let unread = hits.iter().find(|h| h.path == "b.md").expect("没有注意力数据的文件必须照常命中");
        assert!((read.attention_minutes - 60.0).abs() < 1e-9, "{read:?}");
        assert_eq!(unread.attention_minutes, 0.0, "{unread:?}");
    }

    /// 「二次衰减只做一次」这条硬约束的**唯一**护栏。
    ///
    /// `finish` 已经把 `as_of → today` 的衰减做过一遍并写进
    /// `Hit::attention_minutes` 了,所以 `score_of` 里 `attention::boost` 的
    /// `age_days` 必须传 `0`;传任何非零值都是第二遍衰减 —— 分数悄悄偏低,
    /// 不报错、不改变命中集合、也不改变任何相对顺序(每条命中都被同样地
    /// 削弱),没有任何症状。评审实测把那个 `0` 改成 `30` 时,当时的
    /// 305 + 25 条测试**一条都没红**。
    ///
    /// 隔壁的 `a_stale_attention_table_decays_once_not_twice` 挡不住它:那条
    /// 断言的是 `Hit::attention_minutes`,而那个字段是 `finish` 的产物,
    /// `score_of` 的参数再怎么改都不会动它。所以这里必须直接断言 `score_of`
    /// 的**返回值**等于「按 0 天算」的加成。
    #[test]
    fn score_of_does_not_decay_the_already_decayed_minutes_a_second_time() {
        let w = Weights::default();
        // `Origin::Derived` = ×1.0,`doc_date`/`agent_by`/`human_verified`/
        // `level: "line"` 全部不触发 —— rank=-1 给出 r=1,于是注意力加成是
        // 压缩前**唯一**的乘数,等式可以精确成立。
        let mut h = hit_with(Origin::Derived);
        h.attention_minutes = 60.0;
        let s = score_of(-1.0, &h, false, false, false, TODAY, &w);

        let once = crate::attention::boost(60.0, 0, w.attention);
        assert_eq!(s, once / (1.0 + once), "score_of 必须按 0 天算加成");

        // 断言这条测试**有区分力**:半衰期确实让加成变小,所以上面的等式
        // 不是碰巧对任何 `age_days` 都成立。
        let twice = crate::attention::boost(60.0, 30, w.attention);
        assert!(twice < once, "半衰期必须让加成变小,否则上面那条断言杀不死任何变异");
    }

    /// 表的 `as_of` 到今天的二次衰减由 `finish` 做,而且**只做一次**:
    /// 60 分钟、一个半衰期前的表,今天应当值 30 分钟,不是 15(做了两遍)。
    #[test]
    fn a_stale_attention_table_decays_once_not_twice() {
        let (_d, c) = indexed(&[("a.md", "target\n")]);
        crate::store::replace_attention(
            &c,
            "2026-07-11", // 2026-08-10 前 30 天 = 一个半衰期
            &std::collections::BTreeMap::from([("a.md".to_string(), 60.0)]),
        )
        .unwrap();

        let hits = search(&c, &parse("target"), 20, "2026-08-10").unwrap().0;
        let h = hits.iter().find(|h| h.path == "a.md").expect("a.md must be found");
        assert!((h.attention_minutes - 30.0).abs() < 1e-9, "半衰期整一次: {h:?}");
    }

    /// `rank` 列位置的显式护栏。C-T7 给 `SELECT_COLS` 追加两列后,`fts_search`
    /// 里硬编码的 `rank` 索引从 13 挪到 15 —— 而挪错回 13 读到的是
    /// `COALESCE(att.minutes, 0.0)`:同为 REAL,**不报类型错**,只是把每条
    /// 命中的 bm25 悄悄换成 0,于是所有分数塌成同一个值,相关性排序整体失效。
    /// 实测这个变异对当时的整套测试(含 `tests/acceptance.rs` 的检索回归集)
    /// 全绿 —— bm25 真正抵达 `score_of` 这件事此前没有任何测试钉住。
    #[test]
    fn the_bm25_rank_column_actually_reaches_the_score() {
        let (_d, c) = indexed(&[
            ("a.md", "target target target target\n"),
            ("b.md", "target padding padding padding padding padding padding\n"),
        ]);
        let hits = search(&c, &parse("target"), 20, "2026-08-10").unwrap().0;
        let a = hits.iter().find(|h| h.path == "a.md").expect("a.md must be found");
        let b = hits.iter().find(|h| h.path == "b.md").expect("b.md must be found");
        assert!(a.score > b.score, "词频高的必须拿到严格更高的分,否则 rank 根本没进来: {hits:?}");
    }

    /// 另外两条查询路径(`t1-scan` 的 LIKE 回退、无词的 filter-only)与 FTS
    /// 路径共用 `SELECT_COLS`,但各自硬编码列索引 —— 漏改一条**不报编译错,
    /// 也不报 `InvalidColumnType`**:`SELECT_COLS` 追加的两列里,索引 13 是
    /// `COALESCE(att.minutes, 0.0)`,与 `rank` 同为 REAL,读错只是把 bm25 悄悄
    /// 换成注意力分钟数,分数全塌成一个值、相关性排序静默失效(实测,见
    /// `SELECT_COLS` 的注释与 `the_bm25_rank_column_actually_reaches_the_score`)。
    /// 两条路径都必须真跑到并读出注意力。
    #[test]
    fn every_query_path_reads_attention_not_just_the_fts_one() {
        // `会见了李慕白同志` + 查 `慕`:与 `an_out_of_vocabulary_cjk_query_
        // falls_back_to_a_bounded_scan` 同一个词典盲区,FTS 零命中才会真的
        // 走 LIKE 回退。
        let (_d, c) = indexed(&[("a.md", "会见了李慕白同志\n")]);
        crate::store::replace_attention(
            &c,
            "2026-08-10",
            &std::collections::BTreeMap::from([("a.md".to_string(), 60.0)]),
        )
        .unwrap();

        // LIKE 回退:单字「慕」是词典盲区,FTS 零命中后走 t1-scan。
        let (scan_hits, route) = search(&c, &parse("慕"), 20, "2026-08-10").unwrap();
        assert_eq!(route.as_str(), "t1-scan", "必须真的走回退路径");
        assert!(
            scan_hits.iter().any(|h| h.path == "a.md" && (h.attention_minutes - 60.0).abs() < 1e-9),
            "{scan_hits:?}"
        );

        // filter-only:没有词、只有过滤器。
        let filter_hits = search(&c, &parse("origin:unlabeled"), 20, "2026-08-10").unwrap().0;
        assert!(
            filter_hits.iter().any(|h| h.path == "a.md" && (h.attention_minutes - 60.0).abs() < 1e-9),
            "{filter_hits:?}"
        );
    }
    /// 保底臂的**计划形状**守卫,不是耗时阈值。
    ///
    /// 背景:这条路径现在每次查询跑两条 SQL,实测(2000 文件合成 vault)保底臂
    /// 给 FTS 路径的 SQL 成本加了约 +46%~+62%(主臂 4493µs / 保底臂 2046µs,本
    /// 机;评审在同规模上量到 6147µs / 3789µs)。而 `warm_queries_are_fast` 对
    /// 这个量级的变化**完全失明** —— 它在小 corpus 上 p50 只有 49µs、阈值
    /// 10000µs,204 倍余量。
    ///
    /// 为什么钉计划形状而不是补一条耗时阈值:能让这条臂的成本**跳量级**的改动
    /// (丢掉共用的 MATCH、改成扫 `blocks`、塞进相关子查询)全都表现为查询计划
    /// 从「FTS 驱动 + 主键回表」退化成全表扫描,而计划是确定性的,不受 CI 机器
    /// 方差影响;一条耗时阈值要么松到测不出东西(现有那条就是反面教材),要么
    /// 紧到在别人的机器上偶发红。**但要说清它管不到什么**:它管形状,不管绝对
    /// 耗时,常数级的变慢它一律看不见 —— 动这段 SQL 请人工量一次。
    #[test]
    fn the_attention_arm_stays_fts_driven() {
        let (_d, c) = indexed(&[
            ("a.md", "银河 的 观测 记录\n"),
            ("b.md", "银河 的 另一 份 记录\n"),
            ("c.md", "完全 无关 的 内容\n"),
        ]);
        crate::store::replace_attention(
            &c,
            "2026-08-10",
            &std::collections::BTreeMap::from([("a.md".to_string(), 600.0)]),
        )
        .unwrap();

        let q = parse("银河");
        let expr = match_expr(&q).expect("查询必须有 MATCH 表达式");
        // 断言的是**真正会跑的那条 SQL**(`fts_arms` 的产物),不是测试里另抄
        // 一份 —— 抄一份的守卫只能证明抄件的计划,那是自说自话。
        let (main, arm) = fts_arms(&q, expr, 20, Weights::default().attention);
        let arm = arm.expect("默认 k > 0 必须有保底臂,否则这条守卫在空转");
        for (tag, (sql, args)) in ["主臂", "保底臂"].iter().zip([main, arm]) {
            let mut st = c.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
            let plan: Vec<String> = st
                .query_map(params_from_iter(args.iter()), |r| r.get::<_, String>(3))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            assert!(
                plan.first().is_some_and(|l| l.contains("blocks_fts VIRTUAL TABLE INDEX")),
                "{tag}必须由 FTS 驱动,而不是从别的表开扫:{plan:?}"
            );
            for line in &plan {
                assert!(
                    line.starts_with("SEARCH")
                        || line.contains("blocks_fts VIRTUAL TABLE INDEX")
                        || line.contains("USE TEMP B-TREE"),
                    "{tag}的计划里出现了全表扫描一类的步骤「{line}」:{plan:?}"
                );
            }
        }
    }

    /// `k = 0` 时**第二条臂根本不该被构造出来**(最终评审 I-1)。
    ///
    /// 这条钉的是构造这一步(单元级);「关掉之后最终结果与接入前逐位相同」
    /// 那件事由 acceptance 的 `k_zero_keeps_the_whole_pipeline_identical_to_
    /// having_no_attention_data` 端到端钉住。两条都要:少了这条,回归时只能
    /// 从「结果变了」倒推;少了那条,这条只能证明「没造 SQL」,证明不了「用户
    /// 看到的东西没变」。
    #[test]
    fn k_zero_builds_no_attention_arm() {
        let q = parse("银河");
        let expr = match_expr(&q).expect("查询必须有 MATCH 表达式");
        let (main, arm) = fts_arms(&q, expr.clone(), 20, 0.0);
        assert!(arm.is_none(), "k=0 时不该构造保底臂");
        assert!(!main.0.contains("att.minutes IS NOT NULL"), "主臂不该被注意力条件污染:{}", main.0);
        // 反向区分力:k>0 时它必须回来 —— 否则一个「永远不造臂」的实现也能
        // 让上面那条通过。
        assert!(fts_arms(&q, expr, 20, 0.4).1.is_some(), "k>0 时保底臂必须在");
    }

    /// `doc_attention_minutes` 索引已删除(见 `store.rs` 的 v6 注释与
    /// `fts_arms` 的文档)。这条守卫钉住「删了就别再凭直觉加回来」:它建于
    /// T4、专为保底臂的 `ORDER BY att.minutes DESC`,而实测**结构上用不到**
    /// —— 查询由 FTS 驱动,`doc_attention` 是被 join 的内表,SQLite 只能
    /// `SEARCH att USING INDEX sqlite_autoindex_doc_attention_1 (path=?)` 再
    /// 拿 TEMP B-TREE 排序;改成 `INNER JOIN` 也一样。全量重算
    /// (`replace_attention` 是 DELETE + 重插)每轮都要连它一起重建,所以它
    /// 不是白拿的。要加回来,先用 `EXPLAIN QUERY PLAN` 证明它真的进了计划。
    #[test]
    fn no_unused_index_on_doc_attention() {
        let (_d, c) = indexed(&[("a.md", "银河\n")]);
        let n: i64 = c
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND tbl_name = 'doc_attention' AND name NOT LIKE 'sqlite_autoindex%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "doc_attention 上不该有显式索引 —— 保底臂用不到它");
    }

}
