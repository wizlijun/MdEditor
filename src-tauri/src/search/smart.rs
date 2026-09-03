//! Deterministic multi-query retrieval for the global search window.
//!
//! The ordinary `notemd_search` command is intentionally left alone: its raw
//! query semantics are shared by the sidebar, CLI and MCP.  Long dictated
//! questions need a different adapter because the core query language ANDs
//! every term.  This module compiles one strict arm plus a small relaxation
//! ladder, executes the whole batch under one window ticket/index lock, and
//! fuses duplicate blocks with weighted reciprocal-rank fusion.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

use searchidx::{Hit, Limits, Query, SortMode};
use serde::Serialize;
use tauri::{AppHandle, Manager};

use super::{
    handle, hit_to_dto, lock, require_index, superseded, HitDto, IndexHandle, SearchGen, CANCELLED,
};

const DEFAULT_LIMIT: usize = 50;
const MAX_EXTRACTED_TERMS: usize = 12;
const RRF_K: f64 = 60.0;
const STRICT_WEIGHT: f64 = 1.5;
const RELAXED_WEIGHT: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CandidateKind {
    Ordinary,
    ProperNoun,
    Code,
    File,
    Date,
    Phrase,
}

impl CandidateKind {
    fn weight(self) -> i32 {
        match self {
            Self::Ordinary => 0,
            Self::ProperNoun => 30,
            Self::Code => 45,
            Self::File => 60,
            Self::Date => 70,
            Self::Phrase => 80,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    text: String,
    normalized: String,
    kind: CandidateKind,
    occurrences: usize,
    first_seen: usize,
}

impl Candidate {
    fn score(&self) -> i32 {
        self.kind.weight()
            + (self.occurrences.saturating_sub(1).min(4) as i32 * 18)
            + self.text.chars().count().min(16) as i32
    }

    fn query_text(&self) -> String {
        if self.kind == CandidateKind::Phrase || self.text.chars().any(char::is_whitespace) {
            format!("\"{}\"", self.text.replace('"', ""))
        } else {
            self.text.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct QueryArm {
    id: String,
    kind: &'static str,
    query: String,
    typed_query: Option<Query>,
    terms: Vec<String>,
    weight: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct SmartPlan {
    extracted: Vec<Candidate>,
    arms: Vec<QueryArm>,
    sort: SortMode,
}

/// One host-validated physical arm. Unlike the legacy smart preview, this
/// carries a typed query and is never reparsed from its display string.
pub(super) struct PlannedQueryArm {
    pub id: String,
    pub kind: &'static str,
    pub query: Query,
    pub terms: Vec<String>,
    pub weight: f64,
}

/// One executed (or deadline-skipped) arm in the deterministic query plan.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartQueryDto {
    pub id: String,
    pub kind: String,
    pub query: String,
    pub terms: Vec<String>,
    pub executed: bool,
    pub route: Option<String>,
    pub hit_count: usize,
    pub deep_used: bool,
    pub truncated: bool,
}

impl SmartQueryDto {
    fn pending(arm: &QueryArm) -> Self {
        Self {
            id: arm.id.clone(),
            kind: arm.kind.to_string(),
            query: arm.query.clone(),
            terms: arm.terms.clone(),
            executed: false,
            route: None,
            hit_count: 0,
            deep_used: false,
            truncated: false,
        }
    }
}

/// A normal search hit plus the evidence for its position in the fused list.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartHitDto {
    #[serde(flatten)]
    pub hit: HitDto,
    /// Weighted reciprocal-rank score. It is only comparable inside this one
    /// response and must never be presented as factual confidence.
    pub fused_score: f64,
    pub relevance_reasons: Vec<String>,
    pub matched_queries: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_id: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartSearchResponse {
    pub route: String,
    pub took_ms: u64,
    pub total: usize,
    pub hits: Vec<SmartHitDto>,
    pub truncated: bool,
    pub deep_available: bool,
    pub extracted_terms: Vec<String>,
    pub subqueries: Vec<SmartQueryDto>,
}

/// Smart-search counterpart to `notemd_search`.  The ticket is minted exactly
/// once here; every strict/relaxed arm below shares it.
pub(super) fn run_smart_search_command(
    app: AppHandle,
    window: tauri::Window,
    query: String,
    limit: Option<usize>,
    deep: Option<bool>,
    timeout_ms: Option<u64>,
) -> Result<SmartSearchResponse, String> {
    let started = Instant::now();
    let (ticket, counter) = app.state::<SearchGen>().next(window.label());
    let idx_handle = handle(&app);
    smart_search_locked(
        &idx_handle,
        started,
        &query,
        limit,
        deep,
        timeout_ms,
        &counter,
        ticket,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HitKey {
    path: String,
    line: u32,
    line_end: u32,
}

struct FusedHit {
    hit: Hit,
    fused_score: f64,
    matched_arm_indices: Vec<usize>,
}

#[allow(clippy::too_many_arguments)]
fn smart_search_locked(
    idx_handle: &IndexHandle,
    started: Instant,
    raw_query: &str,
    limit: Option<usize>,
    deep: Option<bool>,
    timeout_ms: Option<u64>,
    counter: &Arc<AtomicU64>,
    ticket: u64,
) -> Result<SmartSearchResponse, String> {
    execute_smart_plan_locked(
        idx_handle,
        started,
        compile_query(raw_query),
        raw_query,
        limit,
        deep,
        timeout_ms,
        counter,
        ticket,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn planned_search_locked(
    idx_handle: &IndexHandle,
    started: Instant,
    arms: Vec<PlannedQueryArm>,
    limit: Option<usize>,
    deep: Option<bool>,
    timeout_ms: Option<u64>,
    counter: &Arc<AtomicU64>,
    ticket: u64,
    sort: SortMode,
) -> Result<SmartSearchResponse, String> {
    let mut extracted = Vec::new();
    let mut positions = HashMap::new();
    let mut order = 0;
    let arms = arms
        .into_iter()
        .map(|mut arm| {
            arm.query.sort = sort;
            for candidate in collect_candidates(&arm.query) {
                add_candidate(
                    &mut extracted,
                    &mut positions,
                    &candidate.text,
                    candidate.kind,
                    &mut order,
                );
            }
            QueryArm {
                id: arm.id,
                kind: arm.kind,
                query: render_typed_query(&arm.query),
                typed_query: Some(arm.query),
                terms: arm.terms,
                weight: arm.weight,
            }
        })
        .collect();
    let plan = SmartPlan { extracted, arms, sort };
    execute_smart_plan_locked(
        idx_handle,
        started,
        plan,
        "<validated SearchPlanV1>",
        limit,
        deep,
        timeout_ms,
        counter,
        ticket,
    )
}

#[allow(clippy::too_many_arguments)]
fn execute_smart_plan_locked(
    idx_handle: &IndexHandle,
    started: Instant,
    plan: SmartPlan,
    log_query: &str,
    limit: Option<usize>,
    deep: Option<bool>,
    timeout_ms: Option<u64>,
    counter: &Arc<AtomicU64>,
    ticket: u64,
) -> Result<SmartSearchResponse, String> {
    let guard = lock(idx_handle);
    // Match `notemd_search`: waiting for a rebuild must not consume the query's
    // own budget.  All arms share this one post-lock deadline.
    let deadline = timeout_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
    if superseded(counter, ticket) {
        return Err(CANCELLED.to_string());
    }
    let idx = require_index(&guard)?;
    let final_limit = match limit.unwrap_or(DEFAULT_LIMIT) {
        0 => searchidx::NO_LIMIT,
        n => n,
    };
    let arm_limit = per_arm_limit(final_limit);
    let weights = super::options::weights_for_vault(idx.vault_root());
    let conventions = super::options::conventions_for_vault(idx.vault_root());
    let abort_counter = counter.clone();
    let abort = Arc::new(move || {
        superseded(&abort_counter, ticket) || deadline.is_some_and(|end| Instant::now() >= end)
    });
    let shallow_limits = Limits {
        deep: false,
        abort: Some(abort.clone()),
    };

    let mut query_dtos: Vec<SmartQueryDto> = plan.arms.iter().map(SmartQueryDto::pending).collect();
    let mut fused: HashMap<HitKey, FusedHit> = HashMap::new();
    let mut deep_candidates = Vec::new();
    let mut truncated = false;
    let mut used_scan = false;

    // Always finish the cheap FTS ladder before considering a scan.  Letting a
    // long Han strict arm scan first can spend the entire budget proving that
    // every filler word does not occur, never reaching the useful relaxations.
    for (arm_index, arm) in plan.arms.iter().enumerate() {
        if arm_index + 1 < plan.arms.len() && deadline_reached(deadline) {
            truncated = true;
            break;
        }
        let answer = search_arm(idx, arm, arm_limit, &shallow_limits, &weights, &conventions)?;
        if superseded(counter, ticket) {
            return Err(CANCELLED.to_string());
        }
        let dto = &mut query_dtos[arm_index];
        dto.executed = true;
        dto.route = Some(answer.route.as_str().to_string());
        dto.hit_count = answer.hits.len();
        dto.truncated = answer.truncated;
        truncated |= answer.truncated;
        if answer.deep_available {
            deep_candidates.push(arm_index);
        }
        merge_arm(&mut fused, arm_index, arm, answer.hits);
        if deadline_reached(deadline) {
            truncated = true;
            break;
        }
    }

    // Deep search is a collective fallback: only pay for one scan when the
    // complete FTS ladder found nothing.  The last candidate is the most
    // relaxed arm and therefore the least likely to repeat the strict miss.
    if deep.unwrap_or(true) && fused.is_empty() && !truncated && !deadline_reached(deadline) {
        if let Some(&arm_index) = deep_candidates.last() {
            let arm = &plan.arms[arm_index];
            let deep_limits = Limits {
                deep: true,
                abort: Some(abort),
            };
            let answer = search_arm(idx, arm, arm_limit, &deep_limits, &weights, &conventions)?;
            if superseded(counter, ticket) {
                return Err(CANCELLED.to_string());
            }
            let dto = &mut query_dtos[arm_index];
            dto.route = Some(answer.route.as_str().to_string());
            dto.hit_count = answer.hits.len();
            dto.deep_used = true;
            dto.truncated |= answer.truncated;
            used_scan = answer.route.as_str() == "t1-scan";
            truncated |= answer.truncated;
            merge_arm(&mut fused, arm_index, arm, answer.hits);
        }
    }

    if superseded(counter, ticket) {
        return Err(CANCELLED.to_string());
    }

    let deep_available =
        !deep.unwrap_or(true) && fused.is_empty() && !truncated && !deep_candidates.is_empty();
    let root = idx.vault_root().to_path_buf();
    let mut hits = finish_fusion(fused, &plan, &root);
    if final_limit != searchidx::NO_LIMIT {
        hits.truncate(final_limit);
    }
    let route = if used_scan { "smart-scan" } else { "smart-fts" }.to_string();

    crate::log_cat!(
        "search",
        "debug",
        "smart query={log_query:?} arms={} route={} hits={} {}ms deep={} truncated={}",
        plan.arms.len(),
        route,
        hits.len(),
        started.elapsed().as_millis(),
        deep.unwrap_or(true),
        truncated
    );

    Ok(SmartSearchResponse {
        route,
        took_ms: started.elapsed().as_millis() as u64,
        total: hits.len(),
        hits,
        truncated,
        deep_available,
        extracted_terms: plan
            .extracted
            .iter()
            .map(|term| term.text.clone())
            .collect(),
        subqueries: query_dtos,
    })
}

fn search_arm(
    idx: &searchidx::SearchIndex,
    arm: &QueryArm,
    limit: usize,
    limits: &Limits,
    weights: &searchidx::query::Weights,
    conventions: &searchidx::query::Conventions,
) -> Result<searchidx::Answer, String> {
    match &arm.typed_query {
        Some(query) => idx.search_query_ranked(query, limit, limits, weights, conventions),
        None => idx.search_ranked(&arm.query, limit, limits, weights, conventions),
    }
}

fn render_typed_query(query: &Query) -> String {
    let mut parts = query.terms.iter().map(|value| json_string(value)).collect::<Vec<_>>();
    parts.extend(query.phrases.iter().map(|value| format!("phrase={}", json_string(value))));
    for (name, values) in [
        ("tag", &query.tags),
        ("type", &query.types),
        ("path", &query.paths),
        ("page", &query.pages),
        ("ext", &query.exts),
        ("origin", &query.origins),
    ] {
        parts.extend(values.iter().map(|value| format!("{name}={}", json_string(value))));
    }
    if let Some(value) = &query.after {
        parts.push(format!("after={}", json_string(value)));
    }
    if let Some(value) = &query.before {
        parts.push(format!("before={}", json_string(value)));
    }
    parts.push(format!("sort={}", query.sort.as_str()));
    parts.join(" ")
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a Rust string cannot fail")
}

fn deadline_reached(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|end| Instant::now() >= end)
}

fn per_arm_limit(final_limit: usize) -> usize {
    if final_limit == searchidx::NO_LIMIT {
        return final_limit;
    }
    // A modest over-fetch gives consensus hits just below an individual arm's
    // first page a chance to surface without turning five arms into an
    // unbounded query.
    if final_limit <= 500 {
        final_limit.saturating_mul(3).min(500)
    } else {
        final_limit
    }
}

fn merge_arm(
    fused: &mut HashMap<HitKey, FusedHit>,
    arm_index: usize,
    arm: &QueryArm,
    hits: Vec<Hit>,
) {
    for (offset, hit) in hits.into_iter().enumerate() {
        let key = HitKey {
            path: hit.path.clone(),
            line: hit.line,
            line_end: hit.line_end,
        };
        let contribution = arm.weight / (RRF_K + offset as f64 + 1.0);
        match fused.get_mut(&key) {
            Some(existing) => {
                let pinned = existing.hit.pinned || hit.pinned;
                if hit.score.total_cmp(&existing.hit.score).is_gt() {
                    existing.hit = hit;
                }
                existing.hit.pinned = pinned;
                if !existing.matched_arm_indices.contains(&arm_index) {
                    existing.fused_score += contribution;
                    existing.matched_arm_indices.push(arm_index);
                }
            }
            None => {
                fused.insert(
                    key,
                    FusedHit {
                        hit,
                        fused_score: contribution,
                        matched_arm_indices: vec![arm_index],
                    },
                );
            }
        }
    }
}

fn finish_fusion(
    fused: HashMap<HitKey, FusedHit>,
    plan: &SmartPlan,
    vault_root: &std::path::Path,
) -> Vec<SmartHitDto> {
    let mut values: Vec<FusedHit> = fused.into_values().collect();
    values.sort_by(|a, b| {
        let common_ties = || {
            b.hit.pinned.cmp(&a.hit.pinned)
                .then_with(|| b.fused_score.total_cmp(&a.fused_score))
                .then_with(|| b.hit.score.total_cmp(&a.hit.score))
                .then_with(|| a.hit.path.cmp(&b.hit.path))
                .then_with(|| a.hit.line.cmp(&b.hit.line))
                .then_with(|| a.hit.line_end.cmp(&b.hit.line_end))
        };
        if plan.sort == SortMode::Relevance {
            return common_ties();
        }
        let date = match (a.hit.doc_date.as_deref(), b.hit.doc_date.as_deref()) {
            (Some(a), Some(b)) => match plan.sort {
                SortMode::DocDateAsc => a.cmp(b),
                SortMode::DocDateDesc => b.cmp(a),
                SortMode::Relevance => unreachable!(),
            },
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        date.then_with(common_ties)
    });
    values
        .into_iter()
        .map(|mut value| {
            value.matched_arm_indices.sort_unstable();
            value.matched_arm_indices.dedup();
            let matched_queries = value
                .matched_arm_indices
                .iter()
                .map(|&index| plan.arms[index].id.clone())
                .collect();
            let relevance_reasons = relevance_reasons(&value, plan);
            SmartHitDto {
                hit: hit_to_dto(value.hit, vault_root),
                fused_score: value.fused_score,
                relevance_reasons,
                matched_queries,
                result_id: None,
            }
        })
        .collect()
}

fn relevance_reasons(hit: &FusedHit, plan: &SmartPlan) -> Vec<String> {
    let mut reasons = Vec::new();
    if hit.hit.pinned {
        reasons.push("exact_page".to_string());
    }
    if hit.matched_arm_indices.contains(&0) {
        reasons.push("strict_query".to_string());
    }
    let text = format!("{}\n{}", hit.hit.breadcrumb, hit.hit.text).to_lowercase();
    if plan
        .extracted
        .iter()
        .any(|term| term.kind == CandidateKind::Phrase && text.contains(&term.normalized))
    {
        reasons.push("exact_phrase".to_string());
    }
    let path = hit.hit.path.to_lowercase();
    if plan
        .extracted
        .iter()
        .any(|term| term.kind == CandidateKind::File && path.contains(&term.normalized))
    {
        reasons.push("filename_match".to_string());
    }
    let breadcrumb = hit.hit.breadcrumb.to_lowercase();
    if plan
        .extracted
        .iter()
        .take(6)
        .any(|term| breadcrumb.contains(&term.normalized))
    {
        reasons.push("breadcrumb_match".to_string());
    }
    if hit.matched_arm_indices.len() > 1 {
        reasons.push("multiple_queries".to_string());
    }
    if !hit.matched_arm_indices.contains(&0) {
        reasons.push("relaxed_query".to_string());
    }
    reasons
}

fn compile_query(raw: &str) -> SmartPlan {
    let strict = raw.trim().to_string();
    let parsed = searchidx::query::parse(&strict);
    let filters = render_filters(&parsed);
    let mut candidates = collect_candidates(&parsed);
    candidates.sort_by(|a, b| {
        b.score()
            .cmp(&a.score())
            .then_with(|| b.occurrences.cmp(&a.occurrences))
            .then_with(|| a.first_seen.cmp(&b.first_seen))
            .then_with(|| a.normalized.cmp(&b.normalized))
    });
    candidates.truncate(MAX_EXTRACTED_TERMS);

    let strict_terms = parsed
        .phrases
        .iter()
        .chain(parsed.terms.iter())
        .cloned()
        .collect();
    let mut arms = vec![QueryArm {
        id: "strict".to_string(),
        kind: "strict",
        query: strict,
        typed_query: None,
        terms: strict_terms,
        weight: STRICT_WEIGHT,
    }];
    let mut seen = HashSet::new();
    seen.insert(normalize_query(&arms[0].query));
    for size in relaxation_sizes(candidates.len()) {
        let selected = &candidates[..size];
        let mut parts: Vec<String> = selected.iter().map(Candidate::query_text).collect();
        parts.extend(filters.iter().cloned());
        let query = parts.join(" ");
        let normalized = normalize_query(&query);
        if query.is_empty() || !seen.insert(normalized) {
            continue;
        }
        let number = arms.len();
        arms.push(QueryArm {
            id: format!("relaxed-{number}"),
            kind: "relaxed",
            query,
            typed_query: None,
            terms: selected.iter().map(|term| term.text.clone()).collect(),
            weight: RELAXED_WEIGHT,
        });
    }
    SmartPlan {
        extracted: candidates,
        arms,
        sort: SortMode::Relevance,
    }
}

fn normalize_query(query: &str) -> String {
    query
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn relaxation_sizes(count: usize) -> Vec<usize> {
    let proposed: &[usize] = match count {
        0 => &[],
        1 => &[1],
        2 => &[2, 1],
        3..=4 => &[4, 2],
        5..=6 => &[6, 4, 2],
        _ => &[8, 6, 4, 2],
    };
    let mut out = Vec::new();
    for size in proposed {
        let size = (*size).min(count);
        if size > 0 && !out.contains(&size) {
            out.push(size);
        }
    }
    out
}

fn render_filters(query: &Query) -> Vec<String> {
    let mut out = Vec::new();
    extend_filters(&mut out, "tag", &query.tags);
    extend_filters(&mut out, "type", &query.types);
    extend_filters(&mut out, "path", &query.paths);
    extend_filters(&mut out, "page", &query.pages);
    extend_filters(&mut out, "ext", &query.exts);
    extend_filters(&mut out, "origin", &query.origins);
    if let Some(value) = &query.after {
        out.push(format!("after:{value}"));
    }
    if let Some(value) = &query.before {
        out.push(format!("before:{value}"));
    }
    out
}

fn extend_filters(out: &mut Vec<String>, name: &str, values: &[String]) {
    out.extend(values.iter().map(|value| format!("{name}:{value}")));
}

fn collect_candidates(query: &Query) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    let mut order = 0usize;

    for phrase in &query.phrases {
        add_candidate(
            &mut out,
            &mut positions,
            phrase,
            CandidateKind::Phrase,
            &mut order,
        );
        collect_from_text(phrase, &mut out, &mut positions, &mut order);
    }
    for term in &query.terms {
        collect_from_text(term, &mut out, &mut positions, &mut order);
    }
    out
}

fn collect_from_text(
    text: &str,
    out: &mut Vec<Candidate>,
    positions: &mut HashMap<String, usize>,
    order: &mut usize,
) {
    for chunk in special_chunks(text) {
        if let Some(kind) = classify_special(&chunk) {
            add_candidate(out, positions, &chunk, kind, order);
        }
    }
    for token in searchidx::tokenize::tokens(text) {
        if useful_token(&token) {
            add_candidate(out, positions, &token, CandidateKind::Ordinary, order);
        }
    }
}

fn add_candidate(
    out: &mut Vec<Candidate>,
    positions: &mut HashMap<String, usize>,
    raw: &str,
    kind: CandidateKind,
    order: &mut usize,
) {
    let text = raw.trim().trim_matches('"');
    if text.is_empty() {
        return;
    }
    let normalized = text.to_lowercase();
    if is_stopword(&normalized) {
        return;
    }
    if let Some(&index) = positions.get(&normalized) {
        let candidate = &mut out[index];
        candidate.occurrences += 1;
        candidate.kind = candidate.kind.max(kind);
        if kind > CandidateKind::Ordinary {
            candidate.text = text.to_string();
        }
        return;
    }
    positions.insert(normalized.clone(), out.len());
    out.push(Candidate {
        text: text.to_string(),
        normalized,
        kind,
        occurrences: 1,
        first_seen: *order,
    });
    *order += 1;
}

fn special_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        let keep = ch.is_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/' | '\\');
        if keep {
            current.push(ch);
        } else if !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn classify_special(token: &str) -> Option<CandidateKind> {
    if looks_like_date(token) {
        Some(CandidateKind::Date)
    } else if looks_like_file(token) {
        Some(CandidateKind::File)
    } else if looks_like_code(token) {
        Some(CandidateKind::Code)
    } else if looks_like_proper_noun(token) {
        Some(CandidateKind::ProperNoun)
    } else {
        None
    }
}

fn looks_like_date(token: &str) -> bool {
    let separator = if token.contains('-') {
        '-'
    } else if token.contains('/') {
        '/'
    } else {
        return false;
    };
    let parts: Vec<&str> = token.split(separator).collect();
    if parts.len() != 3
        || parts[0].len() != 4
        || !(1..=2).contains(&parts[1].len())
        || !(1..=2).contains(&parts[2].len())
        || !parts
            .iter()
            .all(|part| part.chars().all(|ch| ch.is_ascii_digit()))
    {
        return false;
    }
    let month = parts[1].parse::<u8>().unwrap_or(0);
    let day = parts[2].parse::<u8>().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn looks_like_file(token: &str) -> bool {
    let name = token.rsplit(['/', '\\']).next().unwrap_or(token);
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && (1..=8).contains(&extension.len())
        && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
}

fn looks_like_code(token: &str) -> bool {
    if !token.is_ascii() || token.len() < 2 {
        return false;
    }
    let has_letter = token.chars().any(|ch| ch.is_ascii_alphabetic());
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    let has_lower = token.chars().any(|ch| ch.is_ascii_lowercase());
    let has_upper = token.chars().any(|ch| ch.is_ascii_uppercase());
    has_letter
        && (token.contains('_')
            || (token.contains('-') && !token.starts_with('-') && !token.ends_with('-'))
            || has_digit
            || (has_lower && has_upper && !looks_like_proper_noun(token))
            || (has_upper && !has_lower && token.len() >= 2))
}

fn looks_like_proper_noun(token: &str) -> bool {
    if !token.is_ascii() || token.len() < 2 || !token.chars().all(char::is_alphabetic) {
        return false;
    }
    let mut chars = token.chars();
    chars.next().is_some_and(|first| first.is_ascii_uppercase())
        && chars.all(|ch| ch.is_ascii_lowercase())
}

fn useful_token(token: &str) -> bool {
    let normalized = token.trim().to_lowercase();
    if normalized.is_empty() || is_stopword(&normalized) {
        return false;
    }
    if normalized.chars().any(|ch| ch.is_ascii_digit()) {
        return true;
    }
    normalized.chars().count() >= 2
}

fn is_stopword(token: &str) -> bool {
    const STOPWORDS: &[&str] = &[
        "a",
        "an",
        "and",
        "are",
        "can",
        "could",
        "document",
        "documents",
        "find",
        "for",
        "give",
        "help",
        "i",
        "in",
        "information",
        "is",
        "it",
        "look",
        "looking",
        "md",
        "me",
        "my",
        "need",
        "note",
        "notes",
        "of",
        "on",
        "or",
        "please",
        "related",
        "search",
        "show",
        "something",
        "tell",
        "the",
        "to",
        "want",
        "would",
        "一下",
        "什么",
        "关于",
        "内容",
        "可以",
        "哪",
        "哪些",
        "帮",
        "帮我",
        "我",
        "我想",
        "我想要",
        "找",
        "找到",
        "搜索",
        "文档",
        "有关",
        "查看",
        "查找",
        "看看",
        "相关",
        "相关资料",
        "笔记",
        "能否",
        "请",
        "请帮我",
        "请问",
        "资料",
        "这个",
        "这些",
        "里面",
    ];
    STOPWORDS.contains(&token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use searchidx::SearchIndex;
    use std::sync::atomic::Ordering;

    fn hit(path: &str, score: f64) -> Hit {
        Hit {
            path: path.to_string(),
            line: 1,
            line_end: 1,
            text: "Q3Budget launch budget release risk details".to_string(),
            breadcrumb: path.to_string(),
            level: "line".to_string(),
            score,
            doc_date: None,
            agent_by: None,
            human_verified: false,
            origin: searchidx::Origin::Unlabeled,
            concept_type: None,
            pinned: false,
            attention_minutes: 0.0,
        }
    }

    #[test]
    fn long_natural_language_gets_a_strict_arm_and_two_to_four_relaxations() {
        let raw = "请帮我找一下 \"发布风险\" tag:roadmap origin:human 2026-09-03 launch-plan.md Q3Budget APIClient 的相关资料";
        let plan = compile_query(raw);
        assert_eq!(plan.arms[0].id, "strict");
        assert_eq!(plan.arms[0].query, raw);
        assert!(
            (3..=5).contains(&plan.arms.len()),
            "strict + 2..=4 relaxed arms, got {:?}",
            plan.arms.iter().map(|arm| &arm.query).collect::<Vec<_>>()
        );
        for arm in plan.arms.iter().skip(1) {
            assert!(arm.query.contains("tag:roadmap"), "{}", arm.query);
            assert!(arm.query.contains("origin:human"), "{}", arm.query);
        }
        let extracted: Vec<&str> = plan
            .extracted
            .iter()
            .map(|term| term.text.as_str())
            .collect();
        assert!(extracted.contains(&"发布风险"), "{extracted:?}");
        assert!(extracted.contains(&"2026-09-03"), "{extracted:?}");
        assert!(extracted.contains(&"launch-plan.md"), "{extracted:?}");
        assert!(extracted.contains(&"Q3Budget"), "{extracted:?}");
        assert!(extracted.contains(&"APIClient"), "{extracted:?}");
        assert_eq!(
            plan,
            compile_query(raw),
            "the compiler must be deterministic"
        );
    }

    #[test]
    fn explicit_filters_are_never_dropped_by_relaxation() {
        let plan = compile_query(
            "please find launch budget tag:work type:Decision path:projects page:Roadmap ext:md origin:human after:2026-01-01 before:2026-12-31",
        );
        let expected = [
            "tag:work",
            "type:Decision",
            "path:projects",
            "page:Roadmap",
            "ext:md",
            "origin:human",
            "after:2026-01-01",
            "before:2026-12-31",
        ];
        assert!(plan.arms.len() >= 3);
        for arm in plan.arms.iter().skip(1) {
            for filter in expected {
                assert!(
                    arm.query.contains(filter),
                    "{filter} missing from {}",
                    arm.query
                );
            }
        }
    }

    #[test]
    fn repeated_terms_outrank_otherwise_similar_ordinary_terms() {
        let plan = compile_query("alpha beta beta beta gamma");
        assert_eq!(plan.extracted[0].normalized, "beta");
        assert_eq!(plan.extracted[0].occurrences, 3);
    }

    #[test]
    fn special_candidates_keep_their_explainable_kinds() {
        let plan = compile_query("Notion APIClient Q3Budget 2026-09-03 launch-plan.md risk");
        let kind = |needle: &str| {
            plan.extracted
                .iter()
                .find(|candidate| candidate.text == needle)
                .map(|candidate| candidate.kind)
        };
        assert_eq!(kind("Notion"), Some(CandidateKind::ProperNoun));
        assert_eq!(kind("APIClient"), Some(CandidateKind::Code));
        assert_eq!(kind("Q3Budget"), Some(CandidateKind::Code));
        assert_eq!(kind("2026-09-03"), Some(CandidateKind::Date));
        assert_eq!(kind("launch-plan.md"), Some(CandidateKind::File));
    }

    #[test]
    fn fusion_deduplicates_blocks_and_rewards_consensus_with_stable_ties() {
        let plan = SmartPlan {
            extracted: Vec::new(),
            arms: vec![
                QueryArm {
                    id: "strict".into(),
                    kind: "strict",
                    query: "strict".into(),
                    typed_query: None,
                    terms: vec![],
                    weight: STRICT_WEIGHT,
                },
                QueryArm {
                    id: "relaxed-1".into(),
                    kind: "relaxed",
                    query: "relaxed".into(),
                    typed_query: None,
                    terms: vec![],
                    weight: RELAXED_WEIGHT,
                },
            ],
            sort: SortMode::Relevance,
        };
        let mut fused = HashMap::new();
        merge_arm(
            &mut fused,
            0,
            &plan.arms[0],
            vec![hit("z.md", 3.0), hit("a.md", 1.0)],
        );
        merge_arm(
            &mut fused,
            1,
            &plan.arms[1],
            vec![hit("a.md", 2.0), hit("b.md", 2.0)],
        );
        let result = finish_fusion(fused, &plan, std::path::Path::new("/vault"));

        assert_eq!(result.len(), 3, "a.md must be emitted only once");
        assert_eq!(
            result[0].hit.path, "a.md",
            "consensus must outrank a single arm"
        );
        assert_eq!(result[0].matched_queries, ["strict", "relaxed-1"]);
        assert!(result[0]
            .relevance_reasons
            .contains(&"multiple_queries".to_string()));
    }

    #[test]
    fn fusion_uses_path_and_line_as_a_stable_final_tie_break() {
        let plan = SmartPlan {
            extracted: Vec::new(),
            arms: vec![
                QueryArm {
                    id: "strict".into(),
                    kind: "strict",
                    query: "strict".into(),
                    typed_query: None,
                    terms: vec![],
                    weight: STRICT_WEIGHT,
                },
                QueryArm {
                    id: "relaxed-1".into(),
                    kind: "relaxed",
                    query: "one".into(),
                    typed_query: None,
                    terms: vec![],
                    weight: RELAXED_WEIGHT,
                },
                QueryArm {
                    id: "relaxed-2".into(),
                    kind: "relaxed",
                    query: "two".into(),
                    typed_query: None,
                    terms: vec![],
                    weight: RELAXED_WEIGHT,
                },
            ],
            sort: SortMode::Relevance,
        };
        let mut fused = HashMap::new();
        merge_arm(&mut fused, 1, &plan.arms[1], vec![hit("b.md", 1.0)]);
        merge_arm(&mut fused, 2, &plan.arms[2], vec![hit("a.md", 1.0)]);

        let result = finish_fusion(fused, &plan, std::path::Path::new("/vault"));
        assert_eq!(
            result
                .iter()
                .map(|item| item.hit.path.as_str())
                .collect::<Vec<_>>(),
            ["a.md", "b.md"]
        );
    }

    #[test]
    fn fusion_honours_date_sort_with_missing_dates_last() {
        let plan = SmartPlan {
            extracted: Vec::new(),
            arms: vec![QueryArm {
                id: "q1".into(),
                kind: "precision",
                query: "typed".into(),
                typed_query: None,
                terms: vec![],
                weight: 1.0,
            }],
            sort: SortMode::DocDateDesc,
        };
        let mut old = hit("old.md", 99.0);
        old.doc_date = Some("2025-01-01".into());
        let mut recent = hit("recent.md", 1.0);
        recent.doc_date = Some("2026-08-01".into());
        let missing = hit("missing.md", 100.0);
        let mut fused = HashMap::new();
        merge_arm(&mut fused, 0, &plan.arms[0], vec![old, missing, recent]);

        let result = finish_fusion(fused, &plan, std::path::Path::new("/vault"));
        assert_eq!(
            result.iter().map(|item| item.hit.path.as_str()).collect::<Vec<_>>(),
            ["recent.md", "old.md", "missing.md"]
        );
    }

    #[test]
    fn planned_executor_uses_typed_multiword_filters() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(
            vault.path().join("book.md"),
            "---\ntype: Book Summary\n---\n# Book\nrelease risk\n",
        )
        .unwrap();
        std::fs::write(
            vault.path().join("decision.md"),
            "---\ntype: Decision\n---\n# Decision\nrelease risk\n",
        )
        .unwrap();
        let data = tempfile::tempdir().unwrap();
        let mut idx = SearchIndex::open_at(vault.path(), &data.path().join("i.db"), "sync").unwrap();
        idx.sweep(&searchidx::ScanOptions::default(), None).unwrap();
        let handle: IndexHandle = Arc::new(std::sync::Mutex::new(Some(idx)));
        let counter = Arc::new(AtomicU64::new(1));
        let response = planned_search_locked(
            &handle,
            Instant::now(),
            vec![PlannedQueryArm {
                id: "q1".into(),
                kind: "precision",
                query: Query { types: vec!["Book Summary".into()], ..Default::default() },
                terms: vec![],
                weight: 1.0,
            }],
            Some(20),
            Some(false),
            None,
            &counter,
            1,
            SortMode::Relevance,
        )
        .unwrap();

        assert!(!response.hits.is_empty());
        assert!(response.hits.iter().all(|hit| hit.hit.path == "book.md"));
        assert!(response.subqueries[0].query.contains("type=\"Book Summary\""));
    }

    #[test]
    fn a_long_question_recalls_a_document_that_the_strict_and_query_misses() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(
            vault.path().join("launch.md"),
            "# Launch\nQ3Budget launch budget release risk details\n",
        )
        .unwrap();
        std::fs::write(vault.path().join("decoy.md"), "Q3Budget only\n").unwrap();
        let data = tempfile::tempdir().unwrap();
        let mut idx =
            SearchIndex::open_at(vault.path(), &data.path().join("i.db"), "sync").unwrap();
        idx.sweep(&searchidx::ScanOptions::default(), None).unwrap();
        let handle: IndexHandle = Arc::new(std::sync::Mutex::new(Some(idx)));
        let counter = Arc::new(AtomicU64::new(1));
        let response = smart_search_locked(
            &handle,
            Instant::now(),
            "Please help me find Q3Budget launch budget release risk details",
            Some(20),
            Some(false),
            None,
            &counter,
            1,
        )
        .unwrap();

        assert_eq!(response.subqueries[0].id, "strict");
        assert_eq!(
            response.subqueries[0].hit_count, 0,
            "filler words must make strict miss"
        );
        assert_eq!(response.hits[0].hit.path, "launch.md");
        assert!(response.hits[0]
            .relevance_reasons
            .contains(&"relaxed_query".to_string()));
        assert!(response.hits[0]
            .relevance_reasons
            .contains(&"multiple_queries".to_string()));

        let wire = serde_json::to_value(&response).unwrap();
        assert!(wire.get("extractedTerms").is_some());
        assert!(wire.get("subqueries").is_some());
        let first = &wire["hits"][0];
        for key in [
            "path",
            "score",
            "fusedScore",
            "relevanceReasons",
            "matchedQueries",
        ] {
            assert!(
                first.get(key).is_some(),
                "missing smart hit wire key {key}: {first}"
            );
        }
    }

    #[test]
    fn deep_mode_scans_only_the_most_relaxed_candidate_after_the_fts_ladder_misses() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("other.md"), "nothing here\n").unwrap();
        let data = tempfile::tempdir().unwrap();
        let mut idx =
            SearchIndex::open_at(vault.path(), &data.path().join("i.db"), "sync").unwrap();
        idx.sweep(&searchidx::ScanOptions::default(), None).unwrap();
        let handle: IndexHandle = Arc::new(std::sync::Mutex::new(Some(idx)));
        let counter = Arc::new(AtomicU64::new(1));

        let shallow = smart_search_locked(
            &handle,
            Instant::now(),
            "发布 风险 预算 会议",
            Some(20),
            Some(false),
            Some(4_000),
            &counter,
            1,
        )
        .unwrap();
        assert!(shallow.deep_available);
        assert!(shallow.subqueries.iter().all(|query| !query.deep_used));

        let response = smart_search_locked(
            &handle,
            Instant::now(),
            "发布 风险 预算 会议",
            Some(20),
            Some(true),
            Some(4_000),
            &counter,
            1,
        )
        .unwrap();

        assert_eq!(response.route, "smart-scan");
        assert_eq!(
            response
                .subqueries
                .iter()
                .filter(|query| query.deep_used)
                .count(),
            1,
            "one smart request must never fan out into several full-vault scans"
        );
        assert!(
            !response.deep_available,
            "the one available deep scan was already spent"
        );
    }

    #[test]
    fn one_zero_budget_marks_the_unexecuted_batch_truncated() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("a.md"), "alpha beta\n").unwrap();
        let data = tempfile::tempdir().unwrap();
        let mut idx =
            SearchIndex::open_at(vault.path(), &data.path().join("i.db"), "sync").unwrap();
        idx.sweep(&searchidx::ScanOptions::default(), None).unwrap();
        let handle: IndexHandle = Arc::new(std::sync::Mutex::new(Some(idx)));
        let counter = Arc::new(AtomicU64::new(1));

        let response = smart_search_locked(
            &handle,
            Instant::now(),
            "alpha beta gamma delta",
            Some(20),
            Some(false),
            Some(0),
            &counter,
            1,
        )
        .unwrap();

        assert!(response.truncated);
        assert!(response.subqueries.iter().all(|query| !query.executed));
        assert!(response.hits.is_empty());
    }

    #[test]
    fn a_superseded_smart_batch_is_cancelled_before_touching_the_index() {
        let handle: IndexHandle = Arc::new(std::sync::Mutex::new(None));
        let counter = Arc::new(AtomicU64::new(2));
        let result = smart_search_locked(
            &handle,
            Instant::now(),
            "alpha beta gamma",
            None,
            Some(false),
            None,
            &counter,
            1,
        );
        let error = match result {
            Ok(_) => panic!("a superseded batch must not return a response"),
            Err(error) => error,
        };
        assert_eq!(error, CANCELLED);
    }

    #[test]
    fn every_arm_observes_the_same_ticket_counter() {
        let counter = Arc::new(AtomicU64::new(7));
        let ticket = 7;
        let seen = counter.clone();
        let abort = move || superseded(&seen, ticket);
        assert!(!abort());
        counter.fetch_add(1, Ordering::AcqRel);
        assert!(
            abort(),
            "one newer window query must cancel the whole batch"
        );
    }
}
