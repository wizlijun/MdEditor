//! Trusted execution boundary for LLM-produced search plans.
//!
//! The planner returns data, never a query string or command. This module
//! strictly parses that data, re-applies filters found in the user's original
//! input, resolves relative dates against one frozen instant/timezone, expands
//! bounded OR constraints, and only then constructs `searchidx::Query` values.

use std::collections::HashSet;
use std::str::FromStr;
use std::time::Instant;

use chrono::{DateTime, Datelike, Days, Duration, FixedOffset, Months, NaiveDate};
use chrono_tz::Tz;
use searchidx::{Query, SortMode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use super::smart::{self, PlannedQueryArm, SmartSearchResponse};
use super::{handle, SearchGen};

const SCHEMA_VERSION: u32 = 1;
const MAX_LOGICAL_QUERIES: usize = 2;
const MAX_PHYSICAL_QUERIES: usize = 8;
const MAX_TERMS: usize = 6;
const MAX_PHRASES: usize = 2;
const MAX_CONSTRAINT_VALUES: usize = 8;
const MAX_TEXT: usize = 256;
const MAX_RATIONALE: usize = 512;
const MAX_QUESTION_CHARS: usize = 2_000;
const MAX_QUESTION_BYTES: usize = 8 * 1_024;
const MAX_PLAN_BYTES: usize = 16 * 1_024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchPlanV1 {
    pub schema_version: u32,
    pub intent: SearchIntent,
    pub time: Option<PlanTime>,
    pub constraints: PlanConstraints,
    pub queries: Vec<PlanQuery>,
    pub sort: PlanSort,
    pub unsupported_constraints: Vec<String>,
    pub ambiguities: Vec<String>,
    pub confidence: PlanConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchIntent {
    pub kind: IntentKind,
    pub focus: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    Answer,
    Locate,
    List,
    Summarize,
    Compare,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanTime {
    pub applies_to: TimeAppliesTo,
    pub source_text: String,
    pub expression: Option<TimeExpression>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeAppliesTo {
    DocumentDate,
    ContentDate,
    ActivityTime,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TimeExpression {
    CalendarMonth { offset: i32 },
    CalendarWeek { offset: i32 },
    Quarter { year: i32, quarter: u8 },
    Year { offset: i32 },
    RollingWindow { value: u32, unit: RollingUnit },
    AbsoluteRange { after: String, before: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollingUnit {
    Days,
    Weeks,
    Months,
    Years,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanConstraints {
    pub paths: AnyAllConstraint,
    pub tags: AnyAllConstraint,
    pub types: AnyOfConstraint,
    pub extensions: AnyOfConstraint,
    pub origins: AnyOfConstraint,
    pub linked_pages: AllOfConstraint,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnyAllConstraint {
    pub any_of: Vec<String>,
    pub all_of: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AnyOfConstraint {
    pub any_of: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AllOfConstraint {
    pub all_of: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanQuery {
    pub id: String,
    pub purpose: QueryPurpose,
    pub terms: Vec<String>,
    pub phrases: Vec<String>,
    pub weight: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryPurpose {
    Precision,
    Recall,
}

impl QueryPurpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::Precision => "precision",
            Self::Recall => "recall",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanSort {
    Relevance,
    DocDateDesc,
    DocDateAsc,
}

impl From<PlanSort> for SortMode {
    fn from(value: PlanSort) -> Self {
        match value {
            PlanSort::Relevance => SortMode::Relevance,
            PlanSort::DocDateDesc => SortMode::DocDateDesc,
            PlanSort::DocDateAsc => SortMode::DocDateAsc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanConfidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedFilters {
    pub paths: Vec<String>,
    pub tags: Vec<String>,
    pub types: Vec<String>,
    pub extensions: Vec<String>,
    pub origins: Vec<String>,
    pub linked_pages: Vec<String>,
    pub after: Option<String>,
    pub before: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTime {
    pub applies_to: TimeAppliesTo,
    pub source_text: String,
    pub after: Option<String>,
    pub before: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSearchPlan {
    pub schema_version: u32,
    pub intent: SearchIntent,
    pub reference_time: String,
    pub reference_date: String,
    pub timezone: String,
    pub time: Option<ResolvedTime>,
    pub constraints: PlanConstraints,
    pub locked_filters: LockedFilters,
    pub queries: Vec<ResolvedQuery>,
    pub sort: PlanSort,
    pub unsupported_constraints: Vec<String>,
    pub ambiguities: Vec<String>,
    pub confidence: PlanConfidence,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedQuery {
    pub id: String,
    pub logical_id: String,
    pub purpose: QueryPurpose,
    pub terms: Vec<String>,
    pub phrases: Vec<String>,
    pub weight: f64,
    pub rationale: String,
    pub filters: ConcreteFilters,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcreteFilters {
    pub paths: Vec<String>,
    pub tags: Vec<String>,
    pub types: Vec<String>,
    pub extensions: Vec<String>,
    pub origins: Vec<String>,
    pub linked_pages: Vec<String>,
    pub after: Option<String>,
    pub before: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedSearchResponse {
    pub resolved_plan: ResolvedSearchPlan,
    pub search: SmartSearchResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookup_run_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPlanContext {
    pub locked_filters: LockedFilters,
    pub reference_date: String,
    pub time_anchors: TimeAnchors,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeAnchors {
    pub today: AnchorRange,
    pub yesterday: AnchorRange,
    pub tomorrow: AnchorRange,
    pub this_week: AnchorRange,
    pub last_week: AnchorRange,
    pub next_week: AnchorRange,
    pub this_month: AnchorRange,
    pub last_month: AnchorRange,
    pub next_month: AnchorRange,
    pub this_quarter: AnchorRange,
    pub last_quarter: AnchorRange,
    pub next_quarter: AnchorRange,
    pub this_year: AnchorRange,
    pub last_year: AnchorRange,
    pub next_year: AnchorRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorRange {
    pub after: String,
    pub before: String,
}

pub(super) fn search_plan_context(
    original_query: &str,
    reference_time: &str,
    timezone: &str,
) -> Result<SearchPlanContext, String> {
    validate_question(original_query)?;
    let (_, _, reference_date) = temporal_context(reference_time, timezone)?;
    Ok(SearchPlanContext {
        locked_filters: explicit_filters(original_query)?,
        reference_date: reference_date.to_string(),
        time_anchors: time_anchors(reference_date)?,
    })
}

pub(super) fn run_planned_search_command(
    app: AppHandle,
    window: tauri::Window,
    original_query: String,
    plan: Value,
    baseline_plan: Option<Value>,
    reference_time: String,
    timezone: String,
    limit: Option<usize>,
    deep: Option<bool>,
    timeout_ms: Option<u64>,
) -> Result<PlannedSearchResponse, String> {
    validate_question(&original_query)?;
    validate_plan_size(&plan)?;
    if let Some(baseline) = &baseline_plan {
        validate_plan_size(baseline)?;
    }
    let resolved_plan = parse_and_resolve_with_baseline(
        &plan,
        baseline_plan.as_ref(),
        &original_query,
        &reference_time,
        &timezone,
    )?;
    let sort = resolved_plan.sort.into();
    let arms = resolved_plan
        .queries
        .iter()
        .map(|query| resolved_to_arm(query, sort))
        .collect::<Vec<_>>();
    let (ticket, counter) = app.state::<SearchGen>().next(window.label());
    let search = smart::planned_search_locked(
        &handle(&app),
        Instant::now(),
        arms,
        limit,
        deep,
        timeout_ms,
        &counter,
        ticket,
        sort,
    )?;
    Ok(PlannedSearchResponse {
        resolved_plan,
        search,
        lookup_run_id: None,
    })
}

#[cfg(test)]
fn parse_and_resolve(
    value: &Value,
    original_query: &str,
    reference_time: &str,
    timezone: &str,
) -> Result<ResolvedSearchPlan, String> {
    parse_and_resolve_with_baseline(value, None, original_query, reference_time, timezone)
}

fn parse_and_resolve_with_baseline(
    value: &Value,
    baseline: Option<&Value>,
    original_query: &str,
    reference_time: &str,
    timezone: &str,
) -> Result<ResolvedSearchPlan, String> {
    let plan = parse_plan(value)?;
    if let Some(baseline) = baseline {
        enforce_tune_baseline(&plan, &parse_plan(baseline)?)?;
    }
    resolve(plan, original_query, reference_time, timezone)
}

fn parse_plan(value: &Value) -> Result<SearchPlanV1, String> {
    let plan: SearchPlanV1 = serde_json::from_value(value.clone())
        .map_err(|error| invalid(format!("JSON does not match SearchPlanV1: {error}")))?;
    validate(&plan)?;
    Ok(plan)
}

fn validate_question(value: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.chars().count() > MAX_QUESTION_CHARS
        || value.len() > MAX_QUESTION_BYTES
    {
        return Err(invalid(format!(
            "question must be non-empty and at most {MAX_QUESTION_CHARS} characters/{MAX_QUESTION_BYTES} bytes"
        )));
    }
    Ok(())
}

fn validate_plan_size(value: &Value) -> Result<(), String> {
    let size = serde_json::to_vec(value)
        .map_err(|error| invalid(format!("plan cannot be encoded: {error}")))?
        .len();
    if size > MAX_PLAN_BYTES {
        return Err(invalid(format!(
            "plan exceeds the {MAX_PLAN_BYTES}-byte limit"
        )));
    }
    Ok(())
}

fn enforce_tune_baseline(tuned: &SearchPlanV1, baseline: &SearchPlanV1) -> Result<(), String> {
    if tuned.schema_version != baseline.schema_version {
        return Err(invalid(
            "tuned plan changed baseline schemaVersion".to_string(),
        ));
    }
    if tuned.intent != baseline.intent {
        return Err(invalid("tuned plan changed baseline intent".to_string()));
    }
    if tuned.time != baseline.time {
        return Err(invalid("tuned plan changed baseline time".to_string()));
    }
    if tuned.constraints != baseline.constraints {
        return Err(invalid(
            "tuned plan changed baseline constraints".to_string(),
        ));
    }
    if tuned.sort != baseline.sort {
        return Err(invalid("tuned plan changed baseline sort".to_string()));
    }
    Ok(())
}

fn validate(plan: &SearchPlanV1) -> Result<(), String> {
    if plan.schema_version != SCHEMA_VERSION {
        return Err(invalid(format!(
            "unsupported schemaVersion {}; expected {SCHEMA_VERSION}",
            plan.schema_version
        )));
    }
    validate_text("intent.focus", &plan.intent.focus, MAX_TEXT)?;
    if plan.queries.is_empty() || plan.queries.len() > MAX_LOGICAL_QUERIES {
        return Err(invalid(format!(
            "queries must contain 1..={MAX_LOGICAL_QUERIES} arms"
        )));
    }
    let mut ids = HashSet::new();
    for query in &plan.queries {
        if query.id.is_empty()
            || query.id.len() > 32
            || !query
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(invalid(format!("query id {:?} is not a safe id", query.id)));
        }
        if !ids.insert(&query.id) {
            return Err(invalid(format!("duplicate query id {:?}", query.id)));
        }
        if query.terms.len() > MAX_TERMS {
            return Err(invalid(format!(
                "query {:?} has more than {MAX_TERMS} terms",
                query.id
            )));
        }
        if query.phrases.len() > MAX_PHRASES {
            return Err(invalid(format!(
                "query {:?} has more than {MAX_PHRASES} phrases",
                query.id
            )));
        }
        validate_values("query.terms", &query.terms, MAX_TERMS)?;
        validate_values("query.phrases", &query.phrases, MAX_PHRASES)?;
        if !query.weight.is_finite() || !(0.0..=10.0).contains(&query.weight) || query.weight == 0.0
        {
            return Err(invalid(format!(
                "query {:?} weight must be finite and in (0, 10]",
                query.id
            )));
        }
        validate_text("query.rationale", &query.rationale, MAX_RATIONALE)?;
    }
    validate_constraints(&plan.constraints)?;
    validate_values("unsupportedConstraints", &plan.unsupported_constraints, 16)?;
    validate_values("ambiguities", &plan.ambiguities, 16)?;
    if let Some(time) = &plan.time {
        validate_text("time.sourceText", &time.source_text, MAX_TEXT)?;
        if time.applies_to == TimeAppliesTo::DocumentDate && time.expression.is_none() {
            return Err(invalid(
                "document_date time requires a structured expression".to_string(),
            ));
        }
        if let Some(expression) = &time.expression {
            validate_time_expression(expression)?;
        }
    }
    Ok(())
}

fn validate_time_expression(expression: &TimeExpression) -> Result<(), String> {
    match expression {
        TimeExpression::CalendarMonth { offset }
        | TimeExpression::CalendarWeek { offset }
        | TimeExpression::Year { offset }
            if !(-120..=120).contains(offset) =>
        {
            Err(invalid("calendar offset is outside -120..=120".to_string()))
        }
        TimeExpression::Quarter { year, quarter }
            if !(1..=9999).contains(year) || !(1..=4).contains(quarter) =>
        {
            Err(invalid(
                "quarter requires year 1..=9999 and quarter 1..=4".to_string(),
            ))
        }
        TimeExpression::RollingWindow { value, .. } if *value == 0 || *value > 3650 => Err(
            invalid("rolling_window value must be in 1..=3650".to_string()),
        ),
        TimeExpression::AbsoluteRange { after, before } => {
            let after = parse_date("absolute_range.after", after)?;
            let before = parse_date("absolute_range.before", before)?;
            if after > before {
                Err(invalid(
                    "absolute_range after is later than before".to_string(),
                ))
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn validate_constraints(constraints: &PlanConstraints) -> Result<(), String> {
    for (name, values) in [
        ("paths.anyOf", &constraints.paths.any_of),
        ("paths.allOf", &constraints.paths.all_of),
        ("tags.anyOf", &constraints.tags.any_of),
        ("tags.allOf", &constraints.tags.all_of),
        ("types.anyOf", &constraints.types.any_of),
        ("extensions.anyOf", &constraints.extensions.any_of),
        ("origins.anyOf", &constraints.origins.any_of),
        ("linkedPages.allOf", &constraints.linked_pages.all_of),
    ] {
        validate_values(name, values, MAX_CONSTRAINT_VALUES)?;
    }
    for origin in &constraints.origins.any_of {
        if !matches!(
            origin.as_str(),
            "human" | "derived" | "source" | "unlabeled"
        ) {
            return Err(invalid(format!("unknown origin {origin:?}")));
        }
    }
    Ok(())
}

fn validate_values(name: &str, values: &[String], max: usize) -> Result<(), String> {
    if values.len() > max {
        return Err(invalid(format!("{name} has more than {max} values")));
    }
    let mut seen = HashSet::new();
    for value in values {
        validate_text(name, value, MAX_TEXT)?;
        if !seen.insert(value) {
            return Err(invalid(format!(
                "{name} contains duplicate value {value:?}"
            )));
        }
    }
    Ok(())
}

fn validate_text(name: &str, value: &str, max: usize) -> Result<(), String> {
    let length = value.chars().count();
    if value.trim().is_empty() || length > max || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{name} must be non-empty, at most {max} characters, and contain no control characters"
        )));
    }
    Ok(())
}

fn resolve(
    mut plan: SearchPlanV1,
    original_query: &str,
    reference_time: &str,
    timezone: &str,
) -> Result<ResolvedSearchPlan, String> {
    let (instant, timezone, reference_date) = temporal_context(reference_time, timezone)?;
    let locked_filters = explicit_filters(original_query)?;
    if let Some(time) = &plan.time {
        if time.applies_to == TimeAppliesTo::DocumentDate {
            if time.source_text != time.source_text.trim() {
                return Err(invalid(
                    "document_date time.sourceText must not include surrounding whitespace"
                        .to_string(),
                ));
            }
            if !original_query.contains(&time.source_text) {
                return Err(invalid(
                    "document_date time.sourceText must be an exact span from the original question"
                        .to_string(),
                ));
            }
        }
    }
    let planned_range = match plan.time.as_ref() {
        Some(time) if time.applies_to == TimeAppliesTo::DocumentDate => {
            let (after, before) =
                resolve_expression(time.expression.as_ref().unwrap(), reference_date)?;
            let anchors = time_anchors(reference_date)?;
            let expected = anchor_for_source(&time.source_text, &anchors);
            let mut source_is_recognized = false;
            if let Some(expected) = expected {
                source_is_recognized = true;
                if expected.after != after.to_string() || expected.before != before.to_string() {
                    return Err(invalid(format!(
                        "document_date expression does not match the trusted anchor for {:?}",
                        time.source_text
                    )));
                }
            }
            if let Some((value, unit)) = parse_rolling_source(&time.source_text) {
                source_is_recognized = true;
                if !matches!(
                    time.expression.as_ref(),
                    Some(TimeExpression::RollingWindow {
                        value: expression_value,
                        unit: expression_unit,
                    }) if *expression_value == value && *expression_unit == unit
                ) {
                    return Err(invalid(format!(
                        "rolling_window does not match the value and unit in {:?}",
                        time.source_text
                    )));
                }
            }
            if let Some((expected_after, expected_before)) =
                parse_iso_date_source(&time.source_text)?
            {
                source_is_recognized = true;
                if after != expected_after || before != expected_before {
                    return Err(invalid(format!(
                        "absolute_range does not match the explicit date text {:?}",
                        time.source_text
                    )));
                }
            }
            if let Some((expected_after, expected_before)) =
                parse_explicit_year_source(&time.source_text)
            {
                source_is_recognized = true;
                if after != expected_after || before != expected_before {
                    return Err(invalid(format!(
                        "document_date expression does not match the explicit year {:?}",
                        time.source_text
                    )));
                }
            }
            if let Some((expected_after, expected_before)) =
                parse_explicit_quarter_source(&time.source_text)?
            {
                source_is_recognized = true;
                if after != expected_after || before != expected_before {
                    return Err(invalid(format!(
                        "document_date expression does not match the explicit quarter {:?}",
                        time.source_text
                    )));
                }
            }
            if !source_is_recognized {
                return Err(invalid(
                    "document_date sourceText is not a supported exact temporal expression"
                        .to_string(),
                ));
            }
            Some(DateRange {
                after: Some(after),
                before: Some(before),
            })
        }
        Some(time) if time.applies_to == TimeAppliesTo::ActivityTime => {
            let warning = format!("activity_time is not supported: {}", time.source_text);
            if !plan.unsupported_constraints.contains(&warning) {
                plan.unsupported_constraints.push(warning);
            }
            None
        }
        Some(time) if time.applies_to == TimeAppliesTo::Ambiguous => {
            if !plan.ambiguities.contains(&time.source_text) {
                plan.ambiguities.push(time.source_text.clone());
            }
            None
        }
        _ => None,
    };
    let range = intersect_range(planned_range.clone(), &locked_filters)?;
    if let Some(time) = &plan.time {
        if time.applies_to == TimeAppliesTo::DocumentDate {
            strip_time_source(&mut plan.queries, &time.source_text)?;
        }
    }

    let choices = expand_choices(&plan.constraints)?;
    let physical_count = choices
        .len()
        .checked_mul(plan.queries.len())
        .ok_or_else(|| invalid("physical query count overflowed".to_string()))?;
    if physical_count > MAX_PHYSICAL_QUERIES {
        return Err(invalid(format!(
            "plan expands to {physical_count} physical queries; maximum is {MAX_PHYSICAL_QUERIES}"
        )));
    }
    let origins = resolved_origins(&plan.constraints.origins.any_of, &locked_filters.origins)?;
    let mut queries = Vec::with_capacity(physical_count);
    for logical in &plan.queries {
        for (choice_index, choice) in choices.iter().enumerate() {
            let id = if choices.len() == 1 {
                logical.id.clone()
            } else {
                format!("{}.{}", logical.id, choice_index + 1)
            };
            let mut filters = ConcreteFilters {
                paths: plan.constraints.paths.all_of.clone(),
                tags: plan.constraints.tags.all_of.clone(),
                types: locked_filters.types.clone(),
                extensions: locked_filters.extensions.clone(),
                origins: origins.clone(),
                linked_pages: plan.constraints.linked_pages.all_of.clone(),
                after: range
                    .as_ref()
                    .and_then(|range| range.after.map(|date| date.to_string())),
                before: range
                    .as_ref()
                    .and_then(|range| range.before.map(|date| date.to_string())),
            };
            append_unique(&mut filters.paths, &locked_filters.paths);
            append_unique(&mut filters.tags, &locked_filters.tags);
            append_unique(&mut filters.linked_pages, &locked_filters.linked_pages);
            if let Some(value) = &choice.path {
                append_unique(&mut filters.paths, std::slice::from_ref(value));
            }
            if let Some(value) = &choice.tag {
                append_unique(&mut filters.tags, std::slice::from_ref(value));
            }
            if let Some(value) = &choice.concept_type {
                append_unique(&mut filters.types, std::slice::from_ref(value));
            }
            if let Some(value) = &choice.extension {
                append_unique(&mut filters.extensions, std::slice::from_ref(value));
            }
            queries.push(ResolvedQuery {
                id,
                logical_id: logical.id.clone(),
                purpose: logical.purpose,
                terms: logical.terms.clone(),
                phrases: logical.phrases.clone(),
                weight: logical.weight,
                rationale: logical.rationale.clone(),
                filters,
            });
        }
    }

    if let Some(query) = queries.iter().find(|query| !query_has_scope(query)) {
        return Err(invalid(format!(
            "query {:?} has no search terms, phrases, or constraints",
            query.logical_id
        )));
    }

    let resolved_time = plan.time.as_ref().map(|time| ResolvedTime {
        applies_to: time.applies_to,
        source_text: time.source_text.clone(),
        after: range
            .as_ref()
            .and_then(|range| range.after.map(|date| date.to_string())),
        before: range
            .as_ref()
            .and_then(|range| range.before.map(|date| date.to_string())),
    });
    Ok(ResolvedSearchPlan {
        schema_version: plan.schema_version,
        intent: plan.intent,
        reference_time: instant.to_rfc3339(),
        reference_date: reference_date.to_string(),
        timezone: timezone.name().to_string(),
        time: resolved_time,
        constraints: plan.constraints,
        locked_filters,
        queries,
        sort: plan.sort,
        unsupported_constraints: plan.unsupported_constraints,
        ambiguities: plan.ambiguities,
        confidence: plan.confidence,
    })
}

fn explicit_filters(raw: &str) -> Result<LockedFilters, String> {
    let parsed = searchidx::query::parse(raw);
    let after = parsed
        .after
        .as_deref()
        .map(|value| {
            parse_date("explicit after", normalize_explicit_value(value))
                .map(|date| date.to_string())
        })
        .transpose()?;
    let before = parsed
        .before
        .as_deref()
        .map(|value| {
            parse_date("explicit before", normalize_explicit_value(value))
                .map(|date| date.to_string())
        })
        .transpose()?;
    if after
        .as_deref()
        .zip(before.as_deref())
        .is_some_and(|(a, b)| a > b)
    {
        return Err(invalid("explicit after is later than before".to_string()));
    }
    let paths = normalize_explicit_values(parsed.paths);
    let tags = normalize_explicit_values(parsed.tags);
    let types = normalize_explicit_values(parsed.types);
    let extensions = normalize_extensions(parsed.exts);
    let origins = normalize_explicit_values(parsed.origins);
    let linked_pages = normalize_explicit_values(parsed.pages);
    for (name, values) in [
        ("explicit paths", &paths),
        ("explicit tags", &tags),
        ("explicit types", &types),
        ("explicit extensions", &extensions),
        ("explicit origins", &origins),
        ("explicit linked pages", &linked_pages),
    ] {
        validate_values(name, values, MAX_CONSTRAINT_VALUES)?;
    }
    for origin in &origins {
        if !matches!(
            origin.as_str(),
            "human" | "derived" | "source" | "unlabeled"
        ) {
            return Err(invalid(format!("unknown explicit origin {origin:?}")));
        }
    }
    Ok(LockedFilters {
        paths,
        tags,
        types,
        extensions,
        origins,
        linked_pages,
        after,
        before,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DateRange {
    after: Option<NaiveDate>,
    before: Option<NaiveDate>,
}

fn intersect_range(
    range: Option<DateRange>,
    locked: &LockedFilters,
) -> Result<Option<DateRange>, String> {
    let locked_after = locked
        .after
        .as_deref()
        .map(|value| parse_date("explicit after", value))
        .transpose()?;
    let locked_before = locked
        .before
        .as_deref()
        .map(|value| parse_date("explicit before", value))
        .transpose()?;
    let after = match (range.as_ref().and_then(|value| value.after), locked_after) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    let before = match (range.as_ref().and_then(|value| value.before), locked_before) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    if after
        .zip(before)
        .is_some_and(|(after, before)| after > before)
    {
        Err(invalid(
            "planner time does not overlap the user's explicit date filters".to_string(),
        ))
    } else if after.is_some() || before.is_some() {
        Ok(Some(DateRange { after, before }))
    } else {
        Ok(None)
    }
}

fn resolve_expression(
    expression: &TimeExpression,
    reference: NaiveDate,
) -> Result<(NaiveDate, NaiveDate), String> {
    match expression {
        TimeExpression::CalendarMonth { offset } => {
            let start = calendar_month(reference, *offset)?;
            Ok((start, end_of_month(start)?))
        }
        TimeExpression::CalendarWeek { offset } => {
            let monday = reference
                .checked_sub_signed(Duration::days(
                    reference.weekday().num_days_from_monday().into(),
                ))
                .ok_or_else(|| invalid("calendar week is outside supported dates".to_string()))?;
            let start = monday
                .checked_add_signed(Duration::weeks((*offset).into()))
                .ok_or_else(|| invalid("calendar week is outside supported dates".to_string()))?;
            let end = start
                .checked_add_days(Days::new(6))
                .ok_or_else(|| invalid("calendar week is outside supported dates".to_string()))?;
            Ok((start, end))
        }
        TimeExpression::Quarter { year, quarter } => {
            let month = ((*quarter as u32 - 1) * 3) + 1;
            let start = NaiveDate::from_ymd_opt(*year, month, 1)
                .ok_or_else(|| invalid("quarter date is outside supported dates".to_string()))?;
            let next = start
                .checked_add_months(Months::new(3))
                .ok_or_else(|| invalid("quarter date is outside supported dates".to_string()))?;
            let end = next
                .checked_sub_days(Days::new(1))
                .ok_or_else(|| invalid("quarter date is outside supported dates".to_string()))?;
            Ok((start, end))
        }
        TimeExpression::Year { offset } => {
            let year = reference
                .year()
                .checked_add(*offset)
                .ok_or_else(|| invalid("year is outside supported dates".to_string()))?;
            let start = NaiveDate::from_ymd_opt(year, 1, 1)
                .ok_or_else(|| invalid("year is outside supported dates".to_string()))?;
            let end = NaiveDate::from_ymd_opt(year, 12, 31)
                .ok_or_else(|| invalid("year is outside supported dates".to_string()))?;
            Ok((start, end))
        }
        TimeExpression::RollingWindow { value, unit } => {
            let start = match unit {
                RollingUnit::Days => reference.checked_sub_days(Days::new(u64::from(*value - 1))),
                RollingUnit::Weeks => reference.checked_sub_days(Days::new(
                    u64::from(*value)
                        .checked_mul(7)
                        .and_then(|days| days.checked_sub(1))
                        .ok_or_else(|| invalid("rolling week count overflowed".to_string()))?,
                )),
                RollingUnit::Months => reference
                    .checked_sub_months(Months::new(*value))
                    .and_then(|date| date.checked_add_days(Days::new(1))),
                RollingUnit::Years => reference
                    .checked_sub_months(Months::new(
                        value
                            .checked_mul(12)
                            .ok_or_else(|| invalid("rolling year count overflowed".to_string()))?,
                    ))
                    .and_then(|date| date.checked_add_days(Days::new(1))),
            }
            .ok_or_else(|| invalid("rolling window is outside supported dates".to_string()))?;
            Ok((start, reference))
        }
        TimeExpression::AbsoluteRange { after, before } => Ok((
            parse_date("absolute_range.after", after)?,
            parse_date("absolute_range.before", before)?,
        )),
    }
}

fn temporal_context(
    reference_time: &str,
    timezone: &str,
) -> Result<(DateTime<FixedOffset>, Tz, NaiveDate), String> {
    let timezone = Tz::from_str(timezone)
        .map_err(|_| invalid(format!("unknown IANA timezone {timezone:?}")))?;
    let instant = DateTime::parse_from_rfc3339(reference_time)
        .map_err(|error| invalid(format!("referenceTime is not RFC3339: {error}")))?;
    let reference_date = instant.with_timezone(&timezone).date_naive();
    Ok((instant, timezone, reference_date))
}

fn time_anchors(reference: NaiveDate) -> Result<TimeAnchors, String> {
    let day = |offset: i64| {
        reference
            .checked_add_signed(Duration::days(offset))
            .map(|date| AnchorRange::new(date, date))
            .ok_or_else(|| invalid("calendar day is outside supported dates".to_string()))
    };
    let range = |expression: TimeExpression| {
        resolve_expression(&expression, reference)
            .map(|(after, before)| AnchorRange::new(after, before))
    };

    Ok(TimeAnchors {
        today: day(0)?,
        yesterday: day(-1)?,
        tomorrow: day(1)?,
        this_week: range(TimeExpression::CalendarWeek { offset: 0 })?,
        last_week: range(TimeExpression::CalendarWeek { offset: -1 })?,
        next_week: range(TimeExpression::CalendarWeek { offset: 1 })?,
        this_month: range(TimeExpression::CalendarMonth { offset: 0 })?,
        last_month: range(TimeExpression::CalendarMonth { offset: -1 })?,
        next_month: range(TimeExpression::CalendarMonth { offset: 1 })?,
        this_quarter: AnchorRange::from_tuple(calendar_quarter(reference, 0)?),
        last_quarter: AnchorRange::from_tuple(calendar_quarter(reference, -1)?),
        next_quarter: AnchorRange::from_tuple(calendar_quarter(reference, 1)?),
        this_year: range(TimeExpression::Year { offset: 0 })?,
        last_year: range(TimeExpression::Year { offset: -1 })?,
        next_year: range(TimeExpression::Year { offset: 1 })?,
    })
}

impl AnchorRange {
    fn new(after: NaiveDate, before: NaiveDate) -> Self {
        Self {
            after: after.to_string(),
            before: before.to_string(),
        }
    }

    fn from_tuple((after, before): (NaiveDate, NaiveDate)) -> Self {
        Self::new(after, before)
    }
}

fn calendar_quarter(reference: NaiveDate, offset: i32) -> Result<(NaiveDate, NaiveDate), String> {
    let quarter_index = reference
        .year()
        .checked_mul(4)
        .and_then(|value| value.checked_add((reference.month0() / 3) as i32))
        .and_then(|value| value.checked_add(offset))
        .ok_or_else(|| invalid("calendar quarter is outside supported dates".to_string()))?;
    let year = quarter_index.div_euclid(4);
    let month = quarter_index.rem_euclid(4) as u32 * 3 + 1;
    let start = NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| invalid("calendar quarter is outside supported dates".to_string()))?;
    let end = start
        .checked_add_months(Months::new(3))
        .and_then(|date| date.checked_sub_days(Days::new(1)))
        .ok_or_else(|| invalid("calendar quarter is outside supported dates".to_string()))?;
    Ok((start, end))
}

fn anchor_for_source<'a>(source: &str, anchors: &'a TimeAnchors) -> Option<&'a AnchorRange> {
    match source.to_lowercase().as_str() {
        "今天" | "today" => Some(&anchors.today),
        "昨天" | "yesterday" => Some(&anchors.yesterday),
        "明天" | "tomorrow" => Some(&anchors.tomorrow),
        "本周" | "这周" | "这一周" | "this week" | "current week" => {
            Some(&anchors.this_week)
        }
        "上周" | "上一周" | "last week" | "previous week" => Some(&anchors.last_week),
        "下周" | "下一周" | "next week" => Some(&anchors.next_week),
        "本月" | "这个月" | "this month" | "current month" => Some(&anchors.this_month),
        "上月" | "上个月" | "last month" | "previous month" => Some(&anchors.last_month),
        "下月" | "下个月" | "next month" => Some(&anchors.next_month),
        "本季度" | "这个季度" | "this quarter" | "current quarter" => {
            Some(&anchors.this_quarter)
        }
        "上季度" | "上一季度" | "last quarter" | "previous quarter" => {
            Some(&anchors.last_quarter)
        }
        "下季度" | "下一季度" | "next quarter" => Some(&anchors.next_quarter),
        "今年" | "this year" | "current year" => Some(&anchors.this_year),
        "去年" | "上一年" | "last year" | "previous year" => Some(&anchors.last_year),
        "明年" | "下一年" | "next year" => Some(&anchors.next_year),
        _ => None,
    }
}

fn parse_rolling_source(source: &str) -> Option<(u32, RollingUnit)> {
    let source = source.trim().to_lowercase();
    for prefix in ["最近", "过去", "近"] {
        if let Some(tail) = source.strip_prefix(prefix) {
            let tail = tail.trim_start();
            let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                continue;
            }
            let value = tail[..digits].parse().ok()?;
            let unit = match tail[digits..].trim_start() {
                "天" => RollingUnit::Days,
                "周" => RollingUnit::Weeks,
                "个月" | "月" => RollingUnit::Months,
                "年" => RollingUnit::Years,
                _ => continue,
            };
            return Some((value, unit));
        }
    }
    let words = source.split_whitespace().collect::<Vec<_>>();
    if words.len() != 3 || !matches!(words[0], "last" | "past" | "recent") {
        return None;
    }
    let value = words[1].parse().ok()?;
    let unit = match words[2] {
        "day" | "days" => RollingUnit::Days,
        "week" | "weeks" => RollingUnit::Weeks,
        "month" | "months" => RollingUnit::Months,
        "year" | "years" => RollingUnit::Years,
        _ => return None,
    };
    Some((value, unit))
}

fn parse_iso_date_source(source: &str) -> Result<Option<(NaiveDate, NaiveDate)>, String> {
    let text = source.trim();
    let bytes = text.as_bytes();
    if bytes.len() < 10 {
        return Ok(None);
    }
    let mut dates = Vec::new();
    for start in 0..=bytes.len() - 10 {
        let candidate = &bytes[start..start + 10];
        if candidate[4] != b'-'
            || candidate[7] != b'-'
            || candidate
                .iter()
                .enumerate()
                .any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
        {
            continue;
        }
        if let Ok(date) = NaiveDate::parse_from_str(
            std::str::from_utf8(candidate).expect("an ASCII date candidate is valid UTF-8"),
            "%Y-%m-%d",
        ) {
            dates.push((start, start + 10, date));
        }
    }
    if dates.is_empty() {
        return Ok(None);
    }
    if dates.len() > 2 {
        return Err(invalid(
            "document_date sourceText contains more than two explicit dates".to_string(),
        ));
    }
    match dates.as_slice() {
        [(start, end, _)] => {
            if !text[..*start].trim().is_empty() || !text[*end..].trim().is_empty() {
                return Ok(None);
            }
        }
        [(first_start, first_end, _), (second_start, second_end, _)] => {
            let separator = text[*first_end..*second_start].trim();
            if !text[..*first_start].trim().is_empty()
                || !text[*second_end..].trim().is_empty()
                || separator.is_empty()
                || separator
                    .chars()
                    .any(|ch| !matches!(ch, '到' | '至' | '-' | '–' | '—' | '~' | '～'))
            {
                return Ok(None);
            }
        }
        _ => unreachable!("date count was bounded above"),
    }
    let after = dates[0].2;
    let before = dates.get(1).map(|date| date.2).unwrap_or(after);
    Ok(Some((after, before)))
}

fn parse_explicit_year_source(source: &str) -> Option<(NaiveDate, NaiveDate)> {
    let value = source.trim().strip_suffix('年').unwrap_or(source.trim());
    if value.len() != 4 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let year = value.parse().ok()?;
    Some((
        NaiveDate::from_ymd_opt(year, 1, 1)?,
        NaiveDate::from_ymd_opt(year, 12, 31)?,
    ))
}

fn parse_explicit_quarter_source(source: &str) -> Result<Option<(NaiveDate, NaiveDate)>, String> {
    let source = source.trim().to_lowercase();
    let bytes = source.as_bytes();
    if bytes.len() < 6 || !bytes[..4].iter().all(u8::is_ascii_digit) {
        return Ok(None);
    }
    let year = &source[..4];
    let mut suffix = source[4..].trim_start();
    if let Some(value) = suffix.strip_prefix('-') {
        suffix = value.trim_start();
    }
    let Some(quarter) = suffix.strip_prefix('q') else {
        return Ok(None);
    };
    if quarter.len() != 1 || !quarter.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    let year = year.parse().ok();
    let quarter = quarter.parse().ok();
    match (year, quarter) {
        (Some(year), Some(quarter @ 1..=4)) => {
            resolve_expression(&TimeExpression::Quarter { year, quarter }, NaiveDate::MIN).map(Some)
        }
        _ => Ok(None),
    }
}

fn strip_time_source(queries: &mut [PlanQuery], source: &str) -> Result<(), String> {
    for query in queries {
        let had_text = !query.terms.is_empty() || !query.phrases.is_empty();
        strip_time_source_from_values(&mut query.terms, source);
        strip_time_source_from_values(&mut query.phrases, source);
        if had_text && query.terms.is_empty() && query.phrases.is_empty() {
            return Err(invalid(format!(
                "query {:?} has no topical text after removing document_date sourceText",
                query.id
            )));
        }
    }
    Ok(())
}

fn strip_time_source_from_values(values: &mut Vec<String>, source: &str) {
    values.retain(|value| !value.contains(source));
}

fn calendar_month(reference: NaiveDate, offset: i32) -> Result<NaiveDate, String> {
    let month_index = reference
        .year()
        .checked_mul(12)
        .and_then(|value| value.checked_add(reference.month0() as i32))
        .and_then(|value| value.checked_add(offset))
        .ok_or_else(|| invalid("calendar month is outside supported dates".to_string()))?;
    let year = month_index.div_euclid(12);
    let month = month_index.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(year, month, 1)
        .ok_or_else(|| invalid("calendar month is outside supported dates".to_string()))
}

fn end_of_month(start: NaiveDate) -> Result<NaiveDate, String> {
    start
        .checked_add_months(Months::new(1))
        .and_then(|date| date.checked_sub_days(Days::new(1)))
        .ok_or_else(|| invalid("calendar month is outside supported dates".to_string()))
}

fn parse_date(name: &str, value: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|error| invalid(format!("{name} is not YYYY-MM-DD: {error}")))
}

#[derive(Debug, Clone, Default)]
struct ConstraintChoice {
    path: Option<String>,
    tag: Option<String>,
    concept_type: Option<String>,
    extension: Option<String>,
}

fn expand_choices(constraints: &PlanConstraints) -> Result<Vec<ConstraintChoice>, String> {
    let mut choices = vec![ConstraintChoice::default()];
    expand_dimension(&mut choices, &constraints.paths.any_of, |choice, value| {
        choice.path = Some(value)
    })?;
    expand_dimension(&mut choices, &constraints.tags.any_of, |choice, value| {
        choice.tag = Some(value)
    })?;
    expand_dimension(&mut choices, &constraints.types.any_of, |choice, value| {
        choice.concept_type = Some(value)
    })?;
    expand_dimension(
        &mut choices,
        &constraints.extensions.any_of,
        |choice, value| choice.extension = Some(value),
    )?;
    Ok(choices)
}

fn expand_dimension(
    choices: &mut Vec<ConstraintChoice>,
    values: &[String],
    set: impl Fn(&mut ConstraintChoice, String),
) -> Result<(), String> {
    if values.is_empty() {
        return Ok(());
    }
    let projected = choices
        .len()
        .checked_mul(values.len())
        .ok_or_else(|| invalid("constraint expansion overflowed".to_string()))?;
    if projected > MAX_PHYSICAL_QUERIES {
        return Err(invalid(format!(
            "constraints expand to more than {MAX_PHYSICAL_QUERIES} physical variants"
        )));
    }
    let previous = std::mem::take(choices);
    for choice in previous {
        for value in values {
            let mut next = choice.clone();
            set(&mut next, value.clone());
            choices.push(next);
        }
    }
    Ok(())
}

fn resolved_origins(planned: &[String], locked: &[String]) -> Result<Vec<String>, String> {
    if planned.is_empty() {
        return Ok(locked.to_vec());
    }
    if locked.is_empty() {
        return Ok(planned.to_vec());
    }
    let intersection = planned
        .iter()
        .filter(|origin| locked.contains(origin))
        .cloned()
        .collect::<Vec<_>>();
    if intersection.is_empty() {
        Err(invalid(
            "planner origins conflict with the user's explicit origin filter".to_string(),
        ))
    } else {
        Ok(intersection)
    }
}

fn append_unique(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}

fn normalize_explicit_values(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| normalize_explicit_value(&value).to_string())
        .collect()
}

fn normalize_extensions(values: Vec<String>) -> Vec<String> {
    normalize_explicit_values(values)
        .into_iter()
        .map(|value| value.trim_start_matches('.').to_string())
        .collect()
}

fn normalize_explicit_value(value: &str) -> &str {
    let value = value.trim();
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

fn query_has_scope(query: &ResolvedQuery) -> bool {
    !query.terms.is_empty()
        || !query.phrases.is_empty()
        || !query.filters.paths.is_empty()
        || !query.filters.tags.is_empty()
        || !query.filters.types.is_empty()
        || !query.filters.extensions.is_empty()
        || !query.filters.origins.is_empty()
        || !query.filters.linked_pages.is_empty()
        || query.filters.after.is_some()
        || query.filters.before.is_some()
}

fn resolved_to_arm(query: &ResolvedQuery, sort: SortMode) -> PlannedQueryArm {
    let typed = Query {
        terms: query.terms.clone(),
        phrases: query.phrases.clone(),
        tags: query.filters.tags.clone(),
        types: query.filters.types.clone(),
        paths: query.filters.paths.clone(),
        pages: query.filters.linked_pages.clone(),
        exts: query
            .filters
            .extensions
            .iter()
            .map(|value| value.trim_start_matches('.').to_string())
            .collect(),
        origins: query.filters.origins.clone(),
        after: query.filters.after.clone(),
        before: query.filters.before.clone(),
        sort,
        raw: String::new(),
    };
    PlannedQueryArm {
        id: query.id.clone(),
        kind: query.purpose.as_str(),
        query: typed,
        terms: query.terms.clone(),
        weight: query.weight,
    }
}

fn invalid(message: String) -> String {
    format!("invalid search plan: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_plan() -> Value {
        json!({
            "schemaVersion": 1,
            "intent": { "kind": "answer", "focus": "发布风险" },
            "time": null,
            "constraints": {
                "paths": { "anyOf": [], "allOf": [] },
                "tags": { "anyOf": [], "allOf": [] },
                "types": { "anyOf": [] },
                "extensions": { "anyOf": [] },
                "origins": { "anyOf": [] },
                "linkedPages": { "allOf": [] }
            },
            "queries": [{
                "id": "q1",
                "purpose": "precision",
                "terms": ["发布", "风险"],
                "phrases": [],
                "weight": 1.5,
                "rationale": "核心主题"
            }],
            "sort": "relevance",
            "unsupportedConstraints": [],
            "ambiguities": [],
            "confidence": "high"
        })
    }

    #[test]
    fn rejects_unknown_fields_and_out_of_range_enums() {
        let mut unknown = base_plan();
        unknown["command"] = json!("notemd search --all");
        let error = parse_and_resolve(&unknown, "x", "2026-09-03T00:00:00Z", "UTC").unwrap_err();
        assert!(error.contains("unknown field"), "{error}");

        let mut bad_enum = base_plan();
        bad_enum["intent"]["kind"] = json!("delete");
        assert!(
            parse_and_resolve(&bad_enum, "x", "2026-09-03T00:00:00Z", "UTC")
                .unwrap_err()
                .contains("unknown variant")
        );

        let mut missing = base_plan();
        missing.as_object_mut().unwrap().remove("constraints");
        assert!(
            parse_and_resolve(&missing, "x", "2026-09-03T00:00:00Z", "UTC")
                .unwrap_err()
                .contains("missing field")
        );

        let mut missing_time = base_plan();
        missing_time.as_object_mut().unwrap().remove("time");
        assert!(
            parse_and_resolve(&missing_time, "x", "2026-09-03T00:00:00Z", "UTC").is_ok(),
            "SearchPlanV1 compatibility allows an omitted optional time field"
        );

        let mut too_many = base_plan();
        for index in 2..=3 {
            too_many["queries"].as_array_mut().unwrap().push(json!({
                "id": format!("q{index}"), "purpose": "recall", "terms": ["风险"],
                "phrases": [], "weight": 1.0, "rationale": "扩展"
            }));
        }
        assert!(
            parse_and_resolve(&too_many, "x", "2026-09-03T00:00:00Z", "UTC")
                .unwrap_err()
                .contains("1..=2 arms")
        );
    }

    #[test]
    fn last_month_and_last_week_use_the_named_iana_timezone() {
        let mut month = base_plan();
        month["time"] = json!({
            "appliesTo": "document_date",
            "sourceText": "上个月",
            "expression": { "kind": "calendar_month", "offset": -1 }
        });
        // Still Sep 3 UTC, but already Sep 4 in Taipei. The named timezone,
        // not the timestamp's `Z`, chooses the local reference date.
        let resolved = parse_and_resolve(
            &month,
            "上个月的发布风险",
            "2026-09-03T18:30:00Z",
            "Asia/Taipei",
        )
        .unwrap();
        assert_eq!(resolved.reference_date, "2026-09-04");
        let time = resolved.time.unwrap();
        assert_eq!(time.after.as_deref(), Some("2026-08-01"));
        assert_eq!(time.before.as_deref(), Some("2026-08-31"));

        let mut week = base_plan();
        week["time"] = json!({
            "appliesTo": "document_date",
            "sourceText": "上周",
            "expression": { "kind": "calendar_week", "offset": -1 }
        });
        let resolved =
            parse_and_resolve(&week, "上周发布风险", "2026-09-03T00:00:00Z", "Asia/Taipei")
                .unwrap();
        let time = resolved.time.unwrap();
        assert_eq!(time.after.as_deref(), Some("2026-08-24"));
        assert_eq!(time.before.as_deref(), Some("2026-08-30"));
    }

    #[test]
    fn rolling_windows_have_exact_inclusive_boundaries() {
        for (source, expression, expected_after, expected_before) in [
            (
                "最近1天",
                json!({ "kind": "rolling_window", "value": 1, "unit": "days" }),
                "2026-09-05",
                "2026-09-05",
            ),
            (
                "最近7天",
                json!({ "kind": "rolling_window", "value": 7, "unit": "days" }),
                "2026-08-30",
                "2026-09-05",
            ),
            (
                "最近1周",
                json!({ "kind": "rolling_window", "value": 1, "unit": "weeks" }),
                "2026-08-30",
                "2026-09-05",
            ),
        ] {
            let mut plan = base_plan();
            plan["time"] = json!({
                "appliesTo": "document_date",
                "sourceText": source,
                "expression": expression,
            });
            let query = format!("{source}的发布风险");
            let resolved =
                parse_and_resolve(&plan, &query, "2026-09-04T16:30:00Z", "Asia/Taipei").unwrap();
            let time = resolved.time.unwrap();
            assert_eq!(time.after.as_deref(), Some(expected_after), "{source}");
            assert_eq!(time.before.as_deref(), Some(expected_before), "{source}");
        }
    }

    #[test]
    fn rolling_months_and_years_use_half_open_calendar_boundaries() {
        let month_end = NaiveDate::from_ymd_opt(2024, 3, 31).unwrap();
        assert_eq!(
            resolve_expression(
                &TimeExpression::RollingWindow {
                    value: 1,
                    unit: RollingUnit::Months,
                },
                month_end,
            )
            .unwrap(),
            (NaiveDate::from_ymd_opt(2024, 3, 1).unwrap(), month_end)
        );

        let leap_day = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        assert_eq!(
            resolve_expression(
                &TimeExpression::RollingWindow {
                    value: 1,
                    unit: RollingUnit::Years,
                },
                leap_day,
            )
            .unwrap(),
            (NaiveDate::from_ymd_opt(2023, 3, 1).unwrap(), leap_day)
        );
    }

    #[test]
    fn host_time_anchors_cover_local_days_and_year_boundaries() {
        let context = search_plan_context(
            "找昨天和上季度的发布",
            "2026-01-15T00:30:00Z",
            "Asia/Taipei",
        )
        .unwrap();
        assert_eq!(context.reference_date, "2026-01-15");
        assert_eq!(context.time_anchors.today.after, "2026-01-15");
        assert_eq!(context.time_anchors.yesterday.after, "2026-01-14");
        assert_eq!(context.time_anchors.last_quarter.after, "2025-10-01");
        assert_eq!(context.time_anchors.last_quarter.before, "2025-12-31");
        assert_eq!(context.time_anchors.next_quarter.after, "2026-04-01");
        assert_eq!(context.time_anchors.next_quarter.before, "2026-06-30");
    }

    #[test]
    fn recognized_relative_cues_must_match_their_trusted_anchor() {
        let mut today = base_plan();
        today["time"] = json!({
            "appliesTo": "document_date", "sourceText": "今天",
            "expression": {
                "kind": "absolute_range", "after": "2026-09-05", "before": "2026-09-05"
            }
        });
        let resolved =
            parse_and_resolve(&today, "今天发布", "2026-09-04T16:30:00Z", "Asia/Taipei").unwrap();
        assert_eq!(resolved.time.unwrap().after.as_deref(), Some("2026-09-05"));

        today["time"]["expression"] = json!({
            "kind": "absolute_range", "after": "1900-01-01", "before": "1900-01-02"
        });
        let error = parse_and_resolve(&today, "今天发布", "2026-09-04T16:30:00Z", "Asia/Taipei")
            .unwrap_err();
        assert!(error.contains("trusted anchor"), "{error}");

        today["time"]["sourceText"] = json!("截至今天");
        today["time"]["expression"] = json!({
            "kind": "absolute_range", "after": "2026-09-05", "before": "2026-09-05"
        });
        let error = parse_and_resolve(
            &today,
            "截至今天发布",
            "2026-09-04T16:30:00Z",
            "Asia/Taipei",
        )
        .unwrap_err();
        assert!(
            error.contains("supported exact temporal expression"),
            "{error}"
        );

        let mut last_week = base_plan();
        last_week["time"] = json!({
            "appliesTo": "document_date", "sourceText": "上周",
            "expression": { "kind": "calendar_week", "offset": 1 }
        });
        let error =
            parse_and_resolve(&last_week, "上周发布", "2026-09-03T00:00:00Z", "UTC").unwrap_err();
        assert!(error.contains("trusted anchor"), "{error}");
    }

    #[test]
    fn rolling_and_explicit_date_text_must_match_the_resolved_expression() {
        let mut rolling = base_plan();
        rolling["time"] = json!({
            "appliesTo": "document_date", "sourceText": "最近7天",
            "expression": { "kind": "rolling_window", "value": 3, "unit": "days" }
        });
        let error =
            parse_and_resolve(&rolling, "最近7天发布", "2026-09-05T00:00:00Z", "UTC").unwrap_err();
        assert!(error.contains("value and unit"), "{error}");
        rolling["time"]["expression"]["value"] = json!(7);
        let resolved =
            parse_and_resolve(&rolling, "最近7天发布", "2026-09-05T00:00:00Z", "UTC").unwrap();
        assert_eq!(resolved.time.unwrap().after.as_deref(), Some("2026-08-30"));

        rolling["time"]["sourceText"] = json!("最近 7 天");
        let resolved =
            parse_and_resolve(&rolling, "最近 7 天发布", "2026-09-05T00:00:00Z", "UTC").unwrap();
        assert_eq!(resolved.time.unwrap().after.as_deref(), Some("2026-08-30"));

        let mut exact_day = base_plan();
        exact_day["time"] = json!({
            "appliesTo": "document_date", "sourceText": "2026-09-05",
            "expression": {
                "kind": "absolute_range", "after": "1900-01-01", "before": "1900-01-02"
            }
        });
        let error = parse_and_resolve(&exact_day, "2026-09-05 发布", "2026-09-05T00:00:00Z", "UTC")
            .unwrap_err();
        assert!(error.contains("explicit date text"), "{error}");
        exact_day["time"]["expression"] = json!({
            "kind": "absolute_range", "after": "2026-09-05", "before": "2026-09-05"
        });
        let resolved =
            parse_and_resolve(&exact_day, "2026-09-05 发布", "2026-09-05T00:00:00Z", "UTC")
                .unwrap();
        let time = resolved.time.unwrap();
        assert_eq!(time.after.as_deref(), Some("2026-09-05"));
        assert_eq!(time.before.as_deref(), Some("2026-09-05"));

        let mut quarter = base_plan();
        quarter["time"] = json!({
            "appliesTo": "document_date", "sourceText": "2026 Q3",
            "expression": { "kind": "quarter", "year": 2026, "quarter": 3 }
        });
        let resolved =
            parse_and_resolve(&quarter, "2026 Q3 发布", "2026-09-05T00:00:00Z", "UTC").unwrap();
        let time = resolved.time.unwrap();
        assert_eq!(time.after.as_deref(), Some("2026-07-01"));
        assert_eq!(time.before.as_deref(), Some("2026-09-30"));
    }

    #[test]
    fn time_null_does_not_add_filters_for_names_or_dsl_values() {
        for question in [
            "日本年号",
            "path:today 发布",
            "last yearbook notes",
            "当今年轻人",
        ] {
            let resolved = parse_and_resolve(&base_plan(), question, "2026-09-05T00:00:00Z", "UTC")
                .unwrap_or_else(|error| panic!("{question}: {error}"));
            assert!(resolved.time.is_none(), "{question}");
            assert!(resolved.queries[0].filters.after.is_none(), "{question}");
        }
    }

    #[test]
    fn multiple_time_mentions_are_classified_by_the_planner_without_lexical_false_rejection() {
        let mut plan = base_plan();
        plan["time"] = json!({
            "appliesTo": "document_date", "sourceText": "今天",
            "expression": {
                "kind": "absolute_range", "after": "2026-09-05", "before": "2026-09-05"
            }
        });
        plan["queries"][0]["terms"] = json!(["明天会议", "笔记"]);
        let resolved = parse_and_resolve(
            &plan,
            "找今天关于明天会议的笔记",
            "2026-09-05T00:00:00Z",
            "UTC",
        )
        .unwrap();
        assert_eq!(resolved.time.unwrap().after.as_deref(), Some("2026-09-05"));
        assert_eq!(resolved.queries[0].terms, ["明天会议", "笔记"]);

        plan["time"] = json!({
            "appliesTo": "ambiguous", "sourceText": "上周和本周", "expression": null
        });
        let resolved =
            parse_and_resolve(&plan, "比较上周和本周发布", "2026-09-05T00:00:00Z", "UTC").unwrap();
        assert!(resolved.queries[0].filters.after.is_none());
    }

    #[test]
    fn document_date_source_accepts_only_one_supported_expression() {
        let mut plan = base_plan();
        plan["time"] = json!({
            "appliesTo": "document_date", "sourceText": "最近7天和最近30天",
            "expression": { "kind": "rolling_window", "value": 7, "unit": "days" }
        });
        let error = parse_and_resolve(
            &plan,
            "最近7天和最近30天发布",
            "2026-09-05T00:00:00Z",
            "UTC",
        )
        .unwrap_err();
        assert!(
            error.contains("supported exact temporal expression"),
            "{error}"
        );

        plan["time"] = json!({
            "appliesTo": "document_date",
            "sourceText": "2026-09-01 到 2026-09-10 和 2026-09-20",
            "expression": {
                "kind": "absolute_range", "after": "2026-09-01", "before": "2026-09-10"
            }
        });
        let error = parse_and_resolve(
            &plan,
            "2026-09-01 到 2026-09-10 和 2026-09-20 发布",
            "2026-09-05T00:00:00Z",
            "UTC",
        )
        .unwrap_err();
        assert!(error.contains("more than two explicit dates"), "{error}");

        for source in ["2026-09-01 和 2026-09-10", "2026-09-01 2026-09-10"] {
            plan["time"] = json!({
                "appliesTo": "document_date", "sourceText": source,
                "expression": {
                    "kind": "absolute_range", "after": "2026-09-01", "before": "2026-09-10"
                }
            });
            let question = format!("{source} 发布");
            let error =
                parse_and_resolve(&plan, &question, "2026-09-05T00:00:00Z", "UTC").unwrap_err();
            assert!(
                error.contains("supported exact temporal expression"),
                "{source}: {error}"
            );
        }

        plan["time"] = json!({
            "appliesTo": "document_date", "sourceText": "release_2026-09-05_beta",
            "expression": {
                "kind": "absolute_range", "after": "2026-09-05", "before": "2026-09-05"
            }
        });
        let error = parse_and_resolve(
            &plan,
            "release_2026-09-05_beta 发布",
            "2026-09-05T00:00:00Z",
            "UTC",
        )
        .unwrap_err();
        assert!(
            error.contains("supported exact temporal expression"),
            "{error}"
        );

        for source in ["20-26-Q3", "2-0-2-6 Q3", "2026---Q3"] {
            plan["time"] = json!({
                "appliesTo": "document_date", "sourceText": source,
                "expression": { "kind": "quarter", "year": 2026, "quarter": 3 }
            });
            let question = format!("{source} 发布");
            let error =
                parse_and_resolve(&plan, &question, "2026-09-05T00:00:00Z", "UTC").unwrap_err();
            assert!(
                error.contains("supported exact temporal expression"),
                "{source}: {error}"
            );
        }
    }

    #[test]
    fn document_time_must_come_from_the_question_and_is_removed_from_query_text() {
        let mut plan = base_plan();
        plan["time"] = json!({
            "appliesTo": "document_date",
            "sourceText": "上周",
            "expression": { "kind": "calendar_week", "offset": -1 }
        });
        plan["queries"][0]["terms"] = json!(["发布", "风险", "上周", "上周的发布"]);
        plan["queries"][0]["phrases"] = json!(["发布决定", "上周 发布决定"]);
        plan["queries"].as_array_mut().unwrap().push(json!({
            "id": "q2", "purpose": "recall",
            "terms": ["回滚", "上周回滚"], "phrases": ["上周 回滚计划"],
            "weight": 1.0, "rationale": "补充主题"
        }));

        let resolved =
            parse_and_resolve(&plan, "查找上周的发布风险", "2026-09-05T00:00:00Z", "UTC").unwrap();
        assert_eq!(resolved.queries[0].terms, ["发布", "风险"]);
        assert_eq!(resolved.queries[0].phrases, ["发布决定"]);
        assert_eq!(resolved.queries[1].terms, ["回滚"]);
        assert!(resolved.queries[1].phrases.is_empty());
        assert_eq!(
            resolved.queries[0].filters.after.as_deref(),
            Some("2026-08-24")
        );
        assert_eq!(
            resolved.queries[0].filters.before.as_deref(),
            Some("2026-08-30")
        );
        let arm = resolved_to_arm(&resolved.queries[0], resolved.sort.into());
        assert_eq!(arm.query.terms, ["发布", "风险"]);
        assert_eq!(arm.query.phrases, ["发布决定"]);
        assert_eq!(arm.query.after.as_deref(), Some("2026-08-24"));
        assert_eq!(arm.query.before.as_deref(), Some("2026-08-30"));

        plan["time"]["sourceText"] = json!("模型猜测的近期");
        let error = parse_and_resolve(&plan, "查找上周的发布风险", "2026-09-05T00:00:00Z", "UTC")
            .unwrap_err();
        assert!(error.contains("exact span"), "{error}");

        let mut no_topic = base_plan();
        no_topic["time"] = json!({
            "appliesTo": "document_date", "sourceText": "上周",
            "expression": { "kind": "calendar_week", "offset": -1 }
        });
        no_topic["queries"][0]["terms"] = json!(["上周的发布"]);
        no_topic["queries"][0]["phrases"] = json!([]);
        let error =
            parse_and_resolve(&no_topic, "上周的发布", "2026-09-05T00:00:00Z", "UTC").unwrap_err();
        assert!(error.contains("no topical text"), "{error}");
    }

    #[test]
    fn resolved_time_range_excludes_matching_documents_outside_the_index_window() {
        let vault = tempfile::tempdir().unwrap();
        for (name, body) in [
            ("2026-08-23-before.md", "# Before\n发布 风险\n"),
            ("2026-08-24-inside.md", "# Inside\n发布 风险\n"),
            ("2026-08-30-inside.md", "# Inside upper\n发布 风险\n"),
            ("2026-08-31-after.md", "# After\n发布 风险\n"),
        ] {
            std::fs::write(vault.path().join(name), body).unwrap();
        }
        let data = tempfile::tempdir().unwrap();
        let mut index =
            searchidx::SearchIndex::open_at(vault.path(), &data.path().join("index.db"), "sync")
                .unwrap();
        index
            .sweep(&searchidx::ScanOptions::default(), None)
            .unwrap();

        let mut plan = base_plan();
        plan["time"] = json!({
            "appliesTo": "document_date", "sourceText": "上周",
            "expression": { "kind": "calendar_week", "offset": -1 }
        });
        let resolved =
            parse_and_resolve(&plan, "上周发布风险", "2026-09-03T00:00:00Z", "UTC").unwrap();
        let arms = resolved
            .queries
            .iter()
            .map(|query| resolved_to_arm(query, resolved.sort.into()))
            .collect();
        let handle: super::super::IndexHandle =
            std::sync::Arc::new(std::sync::Mutex::new(Some(index)));
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1));
        let response = smart::planned_search_locked(
            &handle,
            Instant::now(),
            arms,
            Some(20),
            Some(false),
            None,
            &counter,
            1,
            resolved.sort.into(),
        )
        .unwrap();

        let mut paths = response
            .hits
            .iter()
            .map(|hit| hit.hit.path.as_str())
            .collect::<Vec<_>>();
        paths.sort_unstable();
        assert_eq!(paths, ["2026-08-24-inside.md", "2026-08-30-inside.md"]);
        assert!(response.subqueries[0]
            .query
            .contains("after=\"2026-08-24\""));
        assert!(response.subqueries[0]
            .query
            .contains("before=\"2026-08-30\""));
    }

    #[test]
    fn timezone_conversion_observes_dst_across_a_utc_date_boundary() {
        let mut plan = base_plan();
        plan["time"] = json!({
            "appliesTo": "document_date",
            "sourceText": "本周",
            "expression": { "kind": "calendar_week", "offset": 0 }
        });
        // 04:30Z is Mar 7 in New York before the DST transition, while a
        // fixed +00 interpretation would call it Mar 8.
        let before = parse_and_resolve(
            &plan,
            "本周发布",
            "2026-03-08T04:30:00Z",
            "America/New_York",
        )
        .unwrap();
        assert_eq!(before.reference_date, "2026-03-07");
        assert_eq!(
            before.time.as_ref().unwrap().after.as_deref(),
            Some("2026-03-02")
        );
        assert_eq!(
            before.time.as_ref().unwrap().before.as_deref(),
            Some("2026-03-08")
        );
        // The same wall-clock-adjacent UTC time after autumn fallback resolves
        // under the zone's new offset without the caller supplying it.
        let after = parse_and_resolve(
            &plan,
            "本周发布",
            "2026-11-01T06:30:00Z",
            "America/New_York",
        )
        .unwrap();
        assert_eq!(after.reference_date, "2026-11-01");
        assert_eq!(
            after.time.as_ref().unwrap().after.as_deref(),
            Some("2026-10-26")
        );
        assert_eq!(
            after.time.as_ref().unwrap().before.as_deref(),
            Some("2026-11-01")
        );
    }

    #[test]
    fn document_time_becomes_filters_but_content_time_does_not() {
        let expression =
            json!({ "kind": "absolute_range", "after": "2025-01-01", "before": "2025-12-31" });
        let mut document = base_plan();
        document["time"] = json!({
            "appliesTo": "document_date", "sourceText": "2025", "expression": expression
        });
        let resolved =
            parse_and_resolve(&document, "2025 年预算", "2026-09-03T00:00:00Z", "UTC").unwrap();
        assert_eq!(
            resolved.queries[0].filters.after.as_deref(),
            Some("2025-01-01")
        );

        let mut content = base_plan();
        content["time"] = json!({
            "appliesTo": "content_date", "sourceText": "2025", "expression": expression
        });
        let resolved =
            parse_and_resolve(&content, "2025 年预算", "2026-09-03T00:00:00Z", "UTC").unwrap();
        assert_eq!(resolved.queries[0].filters.after, None);
        assert_eq!(resolved.queries[0].filters.before, None);
    }

    #[test]
    fn explicit_filters_are_locked_and_date_ranges_are_intersected() {
        let mut plan = base_plan();
        plan["time"] = json!({
            "appliesTo": "document_date",
            "sourceText": "今年",
            "expression": { "kind": "year", "offset": 0 }
        });
        plan["constraints"]["tags"]["allOf"] = json!(["planner"]);
        let resolved = parse_and_resolve(
            &plan,
            "找今年发布 tag:human path:projects origin:human after:2026-07-01 before:2026-08-31",
            "2026-09-03T00:00:00Z",
            "Asia/Taipei",
        )
        .unwrap();
        let filters = &resolved.queries[0].filters;
        assert_eq!(filters.tags, ["planner", "human"]);
        assert_eq!(filters.paths, ["projects"]);
        assert_eq!(filters.origins, ["human"]);
        assert_eq!(filters.after.as_deref(), Some("2026-07-01"));
        assert_eq!(filters.before.as_deref(), Some("2026-08-31"));
        let effective_time = resolved.time.as_ref().unwrap();
        assert_eq!(effective_time.after.as_deref(), Some("2026-07-01"));
        assert_eq!(effective_time.before.as_deref(), Some("2026-08-31"));
        assert_eq!(resolved.locked_filters.tags, ["human"]);
    }

    #[test]
    fn expands_any_of_within_the_eight_physical_query_budget() {
        let mut plan = base_plan();
        plan["time"] = json!({
            "appliesTo": "document_date", "sourceText": "今年",
            "expression": { "kind": "year", "offset": 0 }
        });
        plan["queries"].as_array_mut().unwrap().push(json!({
            "id": "q2", "purpose": "recall", "terms": ["发布"], "phrases": [],
            "weight": 1.0, "rationale": "召回"
        }));
        plan["constraints"]["paths"]["anyOf"] = json!(["a/", "b/"]);
        plan["constraints"]["types"]["anyOf"] = json!(["Decision", "Decision Archive"]);
        let resolved = parse_and_resolve(&plan, "今年发布", "2026-09-03T00:00:00Z", "UTC").unwrap();
        assert_eq!(resolved.queries.len(), 8);
        assert_eq!(resolved.queries[0].id, "q1.1");
        assert_eq!(resolved.queries[7].id, "q2.4");
        assert!(resolved.queries.iter().all(|query| {
            query.filters.after.as_deref() == Some("2026-01-01")
                && query.filters.before.as_deref() == Some("2026-12-31")
        }));

        plan["constraints"]["tags"]["anyOf"] = json!(["x", "y"]);
        let error =
            parse_and_resolve(&plan, "今年发布", "2026-09-03T00:00:00Z", "UTC").unwrap_err();
        assert!(error.contains("maximum is 8"), "{error}");
    }

    #[test]
    fn invalid_dates_and_unknown_timezones_fail_closed_but_unsupported_time_does_not() {
        let mut invalid_date = base_plan();
        invalid_date["time"] = json!({
            "appliesTo": "document_date", "sourceText": "坏日期",
            "expression": { "kind": "absolute_range", "after": "2026-02-30", "before": "2026-03-01" }
        });
        assert!(
            parse_and_resolve(&invalid_date, "坏日期发布", "2026-09-03T00:00:00Z", "UTC").is_err()
        );
        assert!(
            parse_and_resolve(&base_plan(), "发布", "2026-09-03T00:00:00Z", "Mars/Olympus")
                .is_err()
        );

        let mut activity = base_plan();
        activity["time"] = json!({
            "appliesTo": "activity_time", "sourceText": "最近修改",
            "expression": { "kind": "rolling_window", "value": 7, "unit": "days" }
        });
        let resolved =
            parse_and_resolve(&activity, "最近修改的发布", "2026-09-03T00:00:00Z", "UTC").unwrap();
        assert_eq!(resolved.queries[0].filters.after, None);
        assert!(resolved
            .unsupported_constraints
            .iter()
            .any(|warning| warning.contains("activity_time")));

        let mut ambiguous = base_plan();
        ambiguous["time"] = json!({
            "appliesTo": "ambiguous", "sourceText": "最近", "expression": null
        });
        let resolved =
            parse_and_resolve(&ambiguous, "最近发布", "2026-09-03T00:00:00Z", "UTC").unwrap();
        assert_eq!(resolved.queries[0].filters.after, None);
        assert!(resolved.ambiguities.iter().any(|warning| warning == "最近"));
    }

    #[test]
    fn planner_inputs_obey_the_host_hard_limits() {
        let reference = "2026-09-05T00:00:00Z";
        assert!(search_plan_context("", reference, "UTC").is_err());
        assert!(
            search_plan_context(&"问".repeat(MAX_QUESTION_CHARS + 1), reference, "UTC").is_err()
        );
        assert!(search_plan_context(&"😀".repeat(2_049), reference, "UTC").is_err());
        assert!(search_plan_context("八月发布", reference, "UTC").is_ok());
        assert!(search_plan_context("八月发布", reference, "Mars/Olympus").is_err());

        let oversized = json!({ "payload": "x".repeat(MAX_PLAN_BYTES) });
        assert!(validate_plan_size(&oversized).is_err());
        assert!(validate_plan_size(&base_plan()).is_ok());
    }

    #[test]
    fn tune_can_change_queries_but_not_time_constraints_or_sort() {
        let mut baseline = base_plan();
        baseline["time"] = json!({
            "appliesTo": "document_date", "sourceText": "今年",
            "expression": { "kind": "year", "offset": 0 }
        });
        baseline["constraints"]["tags"]["allOf"] = json!(["roadmap"]);

        let mut tuned = baseline.clone();
        tuned["queries"][0]["terms"] = json!(["上线"]);
        tuned["queries"][0]["weight"] = json!(1.0);
        assert!(parse_and_resolve_with_baseline(
            &tuned,
            Some(&baseline),
            "今年发布风险",
            "2026-09-03T00:00:00Z",
            "UTC"
        )
        .is_ok());

        for (name, changed) in [
            (
                "time",
                json!({
                    "appliesTo": "document_date", "sourceText": "去年",
                    "expression": { "kind": "year", "offset": -1 }
                }),
            ),
            (
                "constraints",
                json!({
                    "paths": { "anyOf": [], "allOf": [] },
                    "tags": { "anyOf": [], "allOf": [] },
                    "types": { "anyOf": [] }, "extensions": { "anyOf": [] },
                    "origins": { "anyOf": [] }, "linkedPages": { "allOf": [] }
                }),
            ),
            ("sort", json!("doc_date_desc")),
        ] {
            let mut changed_plan = tuned.clone();
            changed_plan[name] = changed;
            let error = parse_and_resolve_with_baseline(
                &changed_plan,
                Some(&baseline),
                "发布风险",
                "2026-09-03T00:00:00Z",
                "UTC",
            )
            .unwrap_err();
            assert!(error.contains("changed baseline"), "{name}: {error}");
        }
    }

    #[test]
    fn one_sided_explicit_date_stays_one_sided_and_filter_only_is_valid() {
        let mut plan = base_plan();
        plan["queries"][0]["terms"] = json!([]);
        let resolved =
            parse_and_resolve(&plan, "after:2026-07-01", "2026-09-03T00:00:00Z", "UTC").unwrap();
        assert_eq!(
            resolved.queries[0].filters.after.as_deref(),
            Some("2026-07-01")
        );
        assert_eq!(resolved.queries[0].filters.before, None);
    }

    #[test]
    fn resolved_queries_preserve_multiword_filters_in_the_typed_api() {
        let mut plan = base_plan();
        plan["constraints"]["types"]["anyOf"] = json!(["Book Summary"]);
        plan["sort"] = json!("doc_date_desc");
        let resolved = parse_and_resolve(
            &plan,
            "发布 type:\"Book Summary\" ext:.md",
            "2026-09-03T00:00:00Z",
            "UTC",
        )
        .unwrap();
        assert_eq!(resolved.locked_filters.types, ["Book Summary"]);
        assert_eq!(resolved.locked_filters.extensions, ["md"]);
        let arm = resolved_to_arm(&resolved.queries[0], resolved.sort.into());
        assert_eq!(arm.query.types, ["Book Summary"]);
        assert_eq!(arm.query.exts, ["md"]);
        assert_eq!(arm.query.sort, SortMode::DocDateDesc);
        assert!(
            arm.query.raw.is_empty(),
            "typed plans must not rely on raw DSL"
        );
    }

    #[test]
    fn plan_context_has_the_same_locked_filter_wire_shape() {
        let context = search_plan_context(
            "tag:work type:\"Book Summary\" page:[[Roadmap]] after:2026-01-01",
            "2026-09-05T00:00:00Z",
            "UTC",
        )
        .unwrap();
        let wire = serde_json::to_value(context).unwrap();
        assert_eq!(wire["lockedFilters"]["tags"], json!(["work"]));
        assert_eq!(wire["lockedFilters"]["types"], json!(["Book Summary"]));
        assert_eq!(wire["lockedFilters"]["linkedPages"], json!(["Roadmap"]));
        assert_eq!(wire["lockedFilters"]["after"], json!("2026-01-01"));
        assert_eq!(wire["referenceDate"], json!("2026-09-05"));
        assert_eq!(wire["timeAnchors"]["today"]["after"], json!("2026-09-05"));
    }
}
