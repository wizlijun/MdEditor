//! Short-lived trusted state for Smart Lookup summaries.
//!
//! Normal lookup never reads source files here. Only the explicit Summary
//! action resolves opaque result ids back to canonical planned-search hits,
//! re-reads the current line block and rejects stale content.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::Manager;

const RUN_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_RUNS_PER_WINDOW: usize = 20;
const MAX_SOURCES: usize = 6;
const MAX_CHARS: usize = 6_000;
const MAX_SOURCE_CHARS: usize = 3_000;
const MAX_FILE_BYTES: u64 = 52_428_800;
const MAX_SELECTED_RESULT_IDS: usize = 100;

#[derive(Clone)]
struct CanonicalHit {
    result_id: String,
    path: String,
    line: u32,
    line_end: u32,
    level: String,
    indexed_text: String,
}

#[derive(Clone)]
struct LookupRun {
    id: String,
    created_at: Instant,
    window_label: String,
    root: PathBuf,
    question: String,
    hits: Vec<CanonicalHit>,
}

#[derive(Default)]
struct LookupRuns {
    runs: HashMap<String, LookupRun>,
    order: VecDeque<String>,
}

static LOOKUP_RUNS: OnceLock<Mutex<LookupRuns>> = OnceLock::new();

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummarySource {
    id: String,
    path: String,
    line: u32,
    line_end: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryPreparation {
    prompt: String,
    sources: Vec<SummarySource>,
    stale_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskStart {
    run_id: String,
    resolved_model: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryTaskStart {
    run_id: String,
    resolved_model: Option<String>,
    sources: Vec<SummarySource>,
    stale_count: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandoffRef {
    path: String,
    line: u32,
    line_end: u32,
}

fn contains_forbidden_filter_path(value: &Value) -> bool {
    match value {
        Value::String(text) => {
            text.contains('\0')
                || text.contains('\\')
                || Path::new(text).is_absolute()
                || (text.len() >= 3
                    && text.as_bytes()[1] == b':'
                    && text.as_bytes()[0].is_ascii_alphabetic()
                    && matches!(text.as_bytes()[2], b'/' | b'\\'))
        }
        Value::Array(values) => values.iter().any(contains_forbidden_filter_path),
        Value::Object(values) => values.values().any(contains_forbidden_filter_path),
        _ => false,
    }
}

fn prepare_handoff_in(
    root: &Path,
    question: &str,
    resolved_filters: Value,
    query_terms: Vec<String>,
    selected_refs: Vec<HandoffRef>,
) -> Result<String, String> {
    let root = fs::canonicalize(root).map_err(|error| format!("resolve vault: {error}"))?;
    if question.trim().is_empty()
        || question.chars().count() > 2_000
        || question.len() > 8 * 1_024
        || !resolved_filters.is_object()
        || contains_forbidden_filter_path(&resolved_filters)
        || query_terms.len() > 24
        || selected_refs.len() > 20
        || query_terms.iter().any(|term| {
            term.is_empty()
                || term.chars().count() > 256
                || term.contains('\0')
                || term.contains('\\')
                || Path::new(term).is_absolute()
                || (term.len() >= 3
                    && term.as_bytes()[1] == b':'
                    && term.as_bytes()[0].is_ascii_alphabetic()
                    && matches!(term.as_bytes()[2], b'/' | b'\\'))
        })
    {
        return Err("invalid handoff packet".to_string());
    }
    for reference in &selected_refs {
        if reference.line == 0 || reference.line_end < reference.line {
            return Err("invalid handoff reference".to_string());
        }
        canonical_source_path(&root, &reference.path)?;
    }
    let packet = json!({
        "version": 1,
        "question": question.trim(),
        "resolvedFilters": resolved_filters,
        "queryTerms": query_terms,
        "selectedRefs": selected_refs,
        "limitations": ["lookup_results_are_not_complete_evidence"],
    });
    if packet.to_string().len() > 16 * 1_024 {
        return Err("handoff packet exceeds the size limit".to_string());
    }
    Ok(format!(
        "回答用户问题。先根据根 AGENTS.md 确认 Vault 约定；使用 notemd search 验证并扩展候选来源。\n需要个人或项目长期上下文时，按当前 Agent 身份、Role、Scope 和 purpose=information-answer 调用 notemd memory context；不要使用未经 context broker 允许的 Memory。\n下面的 refs 只是检索起点，不是完整证据；请自行重搜、重读并限定结论。\n\nHANDOFF_PACKET_JSON\n{}",
        packet
    ))
}

fn active_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let root = crate::sotvault::resolve_vault_root(app)
        .ok_or_else(|| "Vault not configured".to_string())?;
    fs::canonicalize(root).map_err(|error| format!("resolve vault: {error}"))
}

#[tauri::command]
pub fn smart_lookup_agent_default(app: tauri::AppHandle) -> String {
    let installed = crate::plugin_runtime::agent_provider::providers(
        &crate::plugin_runtime::commands::installed_manifests(),
    );
    let vault = crate::sotvault::resolve_vault_root(&app);
    let configured = crate::plugin_runtime::agent_provider::configured_default(vault.as_deref());
    crate::plugin_runtime::agent_provider::resolve(None, configured.as_deref(), &installed)
}

fn prune(runs: &mut LookupRuns, window_label: &str) {
    let now = Instant::now();
    runs.runs
        .retain(|_, run| now.duration_since(run.created_at) <= RUN_TTL);
    runs.order.retain(|id| runs.runs.contains_key(id));
    while runs
        .order
        .iter()
        .filter_map(|id| runs.runs.get(id))
        .filter(|run| run.window_label == window_label)
        .count()
        >= MAX_RUNS_PER_WINDOW
    {
        let Some(position) = runs.order.iter().position(|id| {
            runs.runs
                .get(id)
                .is_some_and(|run| run.window_label == window_label)
        }) else {
            break;
        };
        if let Some(id) = runs.order.remove(position) {
            runs.runs.remove(&id);
        }
    }
}

pub(crate) fn retain_planned_search(
    window: &tauri::Window,
    original_query: &str,
    response: &mut crate::search::plan::PlannedSearchResponse,
) -> Result<(), String> {
    let root = crate::sotvault::resolve_vault_root(window.app_handle())
        .ok_or_else(|| "Vault not configured".to_string())?;
    retain_in(window.label(), &root, original_query, response)
}

fn retain_in(
    window_label: &str,
    root: &Path,
    original_query: &str,
    response: &mut crate::search::plan::PlannedSearchResponse,
) -> Result<(), String> {
    let root = fs::canonicalize(root).map_err(|error| format!("resolve vault: {error}"))?;
    let run_id = uuid::Uuid::now_v7().to_string();
    let mut hits = Vec::with_capacity(response.search.hits.len());
    for hit in &mut response.search.hits {
        let result_id = uuid::Uuid::now_v7().to_string();
        hit.result_id = Some(result_id.clone());
        hits.push(CanonicalHit {
            result_id,
            path: hit.hit.path.clone(),
            line: hit.hit.line,
            line_end: hit.hit.line_end,
            level: hit.hit.level.clone(),
            indexed_text: if hit.hit.level == "line"
                && !hit.hit.text.trim().is_empty()
                && hit.hit.text.chars().count() <= MAX_SOURCE_CHARS
            {
                hit.hit.text.clone()
            } else {
                String::new()
            },
        });
    }
    let run = LookupRun {
        id: run_id.clone(),
        created_at: Instant::now(),
        window_label: window_label.to_string(),
        root,
        question: original_query.chars().take(2_000).collect(),
        hits,
    };
    let mut registry = LOOKUP_RUNS
        .get_or_init(|| Mutex::new(LookupRuns::default()))
        .lock()
        .map_err(|_| "lookup run registry is unavailable".to_string())?;
    prune(&mut registry, window_label);
    registry.order.push_back(run_id.clone());
    registry.runs.insert(run_id.clone(), run);
    response.lookup_run_id = Some(run_id);
    Ok(())
}

fn canonical_source_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative.len() > 2_048
        || relative.contains('\0')
        || relative.contains('\\')
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("invalid lookup source path".to_string());
    }
    let mut current = root.to_path_buf();
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err("invalid lookup source path".to_string());
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("inspect lookup source: {error}"))?;
        if metadata.file_type().is_symlink() {
            return Err("refusing symlink lookup source".to_string());
        }
    }
    let metadata = fs::symlink_metadata(&current)
        .map_err(|error| format!("inspect lookup source: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return Err("lookup source is not a supported file".to_string());
    }
    let canonical =
        fs::canonicalize(&current).map_err(|error| format!("resolve lookup source: {error}"))?;
    if !canonical.starts_with(root) {
        return Err("lookup source escaped the Vault".to_string());
    }
    Ok(canonical)
}

fn current_block(root: &Path, hit: &CanonicalHit) -> Result<String, String> {
    if hit.level != "line"
        || hit.indexed_text.is_empty()
        || hit.line == 0
        || hit.line_end < hit.line
    {
        return Err("lookup result is not a complete line block".to_string());
    }
    let path = canonical_source_path(root, &hit.path)?;
    let body = fs::read_to_string(path).map_err(|error| format!("read lookup source: {error}"))?;
    let start = usize::try_from(hit.line - 1).map_err(|_| "invalid lookup line".to_string())?;
    let count = usize::try_from(hit.line_end - hit.line + 1)
        .map_err(|_| "invalid lookup line range".to_string())?;
    let lines = body.lines().skip(start).take(count).collect::<Vec<_>>();
    if lines.len() != count {
        return Err("lookup source changed after indexing".to_string());
    }
    let text = lines.join("\n");
    if text.is_empty() || text != hit.indexed_text {
        return Err("lookup source changed after indexing".to_string());
    }
    Ok(text)
}

fn prepare_summary_in(
    run: &LookupRun,
    selected_result_ids: &[String],
    source_limit: usize,
    char_limit: usize,
    style: &str,
) -> Result<SummaryPreparation, String> {
    if !(1..=MAX_SOURCES).contains(&source_limit)
        || !(1_000..=MAX_CHARS).contains(&char_limit)
        || !matches!(style, "sentence" | "bullets")
    {
        return Err("invalid quick summary settings".to_string());
    }
    if selected_result_ids.len() > MAX_SELECTED_RESULT_IDS
        || selected_result_ids
            .iter()
            .any(|id| id.is_empty() || id.len() > 128)
        || selected_result_ids.iter().collect::<HashSet<_>>().len() != selected_result_ids.len()
    {
        return Err("invalid selected lookup result ids".to_string());
    }
    let known = run
        .hits
        .iter()
        .map(|hit| (hit.result_id.as_str(), hit))
        .collect::<HashMap<_, _>>();
    if selected_result_ids
        .iter()
        .any(|id| !known.contains_key(id.as_str()))
    {
        return Err("unknown lookup result id".to_string());
    }
    let candidates = if selected_result_ids.is_empty() {
        run.hits.iter().collect::<Vec<_>>()
    } else {
        selected_result_ids
            .iter()
            .filter_map(|id| known.get(id.as_str()).copied())
            .collect::<Vec<_>>()
    };
    let prefer_distinct_files = selected_result_ids.is_empty();
    let mut paths = HashSet::new();
    let mut accepted = Vec::new();
    let mut stale_count = 0;
    let mut chars = 0;
    for hit in candidates {
        if accepted.len() >= source_limit || (prefer_distinct_files && paths.contains(&hit.path)) {
            continue;
        }
        if hit.level != "line" || hit.indexed_text.is_empty() {
            continue;
        }
        let Ok(text) = current_block(&run.root, hit) else {
            stale_count += 1;
            continue;
        };
        let source_chars = text.chars().count();
        if source_chars > MAX_SOURCE_CHARS || chars + source_chars > char_limit {
            continue;
        }
        chars += source_chars;
        if prefer_distinct_files {
            paths.insert(hit.path.clone());
        }
        accepted.push((hit, text));
    }
    if accepted.is_empty() {
        return Err("当前结果适合打开阅读，不能安全生成短摘要".to_string());
    }
    let sources = accepted
        .iter()
        .enumerate()
        .map(|(index, (hit, _))| SummarySource {
            id: format!("S{}", index + 1),
            path: hit.path.clone(),
            line: hit.line,
            line_end: hit.line_end,
        })
        .collect::<Vec<_>>();
    let evidence = accepted
        .iter()
        .enumerate()
        .map(|(index, (hit, text))| {
            format!(
                "[S{}] {}:{}-{}\n{}",
                index + 1,
                hit.path,
                hit.line,
                hit.line_end,
                text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let format_rule = if style == "sentence" {
        "输出一个短段落。"
    } else {
        "输出最多三个简短要点。"
    };
    let prompt = format!(
        "QUESTION\n{}\n\nLIMITATION\n这些只是当前匹配结果，不是完整 corpus；不能据此声称全部、不存在、从未或精确总数。\n\nRULES\n{} 每个实质段落或要点至少引用一个已知 [Sx]。只输出简答，不使用工具、不读取文件或 Memory。\n\nSOURCES\n{}",
        run.question, format_rule, evidence
    );
    Ok(SummaryPreparation {
        prompt,
        sources,
        stale_count,
    })
}

fn checked_invocation_id(value: &str) -> Result<&str, String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Err("invalid invocation id".to_string());
    }
    Ok(value)
}

fn checked_model_selector(
    model_profile: Option<String>,
    model: Option<String>,
) -> Result<Value, String> {
    match (model_profile, model) {
        (Some(_), Some(_)) => {
            Err("model profile and exact model are mutually exclusive".to_string())
        }
        (Some(profile), None) if matches!(profile.as_str(), "fast" | "default") => {
            Ok(json!({ "model_profile": profile }))
        }
        (Some(_), None) => Err("unknown model profile".to_string()),
        (None, Some(model)) if !model.trim().is_empty() && model.len() <= 256 => {
            Ok(json!({ "model": model }))
        }
        (None, Some(_)) => Err("invalid exact model".to_string()),
        (None, None) => Ok(json!({})),
    }
}

async fn start_agent_task(
    app: tauri::AppHandle,
    provider: String,
    task: &str,
    prompt: String,
    model_profile: Option<String>,
    model: Option<String>,
    usage_display: &str,
    invocation_id: String,
) -> Result<AgentTaskStart, String> {
    checked_invocation_id(&invocation_id)?;
    let selector = checked_model_selector(model_profile, model)?;
    let fingerprint = json!({ "task": task, "prompt": prompt, "selector": selector });
    let input_hash = format!("{:x}", Sha256::digest(fingerprint.to_string().as_bytes()));
    let mut context = json!({
        "task": task,
        "prompt": fingerprint["prompt"],
        "usage_display": usage_display,
        "invocation_id": invocation_id,
        "input_hash": input_hash,
    });
    if let Some(profile) = selector.get("model_profile") {
        context["model_profile"] = profile.clone();
    }
    if let Some(model) = selector.get("model") {
        context["model"] = model.clone();
    }
    let response = crate::plugin_runtime::commands::plugin_v2_execute(
        app,
        provider,
        "run-task".to_string(),
        context,
    )
    .await?;
    let run_id = response
        .get("run_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Agent provider returned no run id".to_string())?
        .to_string();
    let resolved_model = response
        .get("resolved_model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok(AgentTaskStart {
        run_id,
        resolved_model,
    })
}

#[tauri::command]
pub async fn smart_lookup_start_summary(
    app: tauri::AppHandle,
    window: tauri::Window,
    lookup_run_id: String,
    selected_result_ids: Vec<String>,
    source_limit: usize,
    char_limit: usize,
    style: String,
    provider: String,
    model_profile: Option<String>,
    model: Option<String>,
    invocation_id: String,
) -> Result<SummaryTaskStart, String> {
    let root = active_root(&app)?;
    let run = {
        let mut registry = LOOKUP_RUNS
            .get_or_init(|| Mutex::new(LookupRuns::default()))
            .lock()
            .map_err(|_| "lookup run registry is unavailable".to_string())?;
        prune(&mut registry, window.label());
        let run = registry
            .runs
            .get(&lookup_run_id)
            .filter(|run| run.id == lookup_run_id && run.window_label == window.label())
            .cloned()
            .ok_or_else(|| "lookup run expired; run Smart Lookup again".to_string())?;
        registry.order.retain(|id| id != &lookup_run_id);
        registry.order.push_back(lookup_run_id.clone());
        run
    };
    if run.root != root {
        return Err("lookup run belongs to another Vault".to_string());
    }
    let prepared =
        prepare_summary_in(&run, &selected_result_ids, source_limit, char_limit, &style)?;
    let start = start_agent_task(
        app,
        provider,
        "search-summary",
        prepared.prompt,
        model_profile,
        model,
        "result",
        invocation_id,
    )
    .await?;
    Ok(SummaryTaskStart {
        run_id: start.run_id,
        resolved_model: start.resolved_model,
        sources: prepared.sources,
        stale_count: prepared.stale_count,
    })
}

#[tauri::command]
pub async fn smart_lookup_start_handoff(
    app: tauri::AppHandle,
    question: String,
    resolved_filters: Value,
    query_terms: Vec<String>,
    selected_refs: Vec<HandoffRef>,
    provider: String,
    invocation_id: String,
) -> Result<AgentTaskStart, String> {
    let root = active_root(&app)?;
    let prompt = prepare_handoff_in(
        &root,
        &question,
        resolved_filters,
        query_terms,
        selected_refs,
    )?;
    start_agent_task(
        app,
        provider,
        "vault-research",
        prompt,
        Some("default".to_string()),
        None,
        "tip",
        invocation_id,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(root: &Path, hits: Vec<CanonicalHit>) -> LookupRun {
        LookupRun {
            id: "run".into(),
            created_at: Instant::now(),
            window_label: "smart-search".into(),
            root: fs::canonicalize(root).unwrap(),
            question: "为什么延期？".into(),
            hits,
        }
    }

    #[test]
    fn summary_uses_only_current_complete_line_blocks() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.md"), "one\ntwo\nthree\n").unwrap();
        let hit = CanonicalHit {
            result_id: "r1".into(),
            path: "a.md".into(),
            line: 2,
            line_end: 3,
            level: "line".into(),
            indexed_text: "two\nthree".into(),
        };
        let prepared =
            prepare_summary_in(&run(root.path(), vec![hit]), &[], 4, 4_000, "bullets").unwrap();
        assert!(prepared.prompt.contains("two\nthree"));
        assert!(prepared.prompt.contains("不是完整 corpus"));
        assert_eq!(prepared.sources.len(), 1);
    }

    #[test]
    fn long_section_ranges_are_harmless_and_never_enter_summary() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("long.md"), "heading\n").unwrap();
        let hit = CanonicalHit {
            result_id: "r1".into(),
            path: "long.md".into(),
            line: 1,
            line_end: 100_000,
            level: "section".into(),
            indexed_text: "heading".into(),
        };
        let error =
            prepare_summary_in(&run(root.path(), vec![hit]), &[], 4, 4_000, "bullets").unwrap_err();
        assert!(error.contains("不能安全生成短摘要"));
        assert!(!error.contains("invalid source line range"));
    }

    #[test]
    fn handoff_contains_only_valid_relative_hints_and_no_file_body() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.md"), "PRIVATE_FILE_BODY\n").unwrap();
        let prompt = prepare_handoff_in(
            root.path(),
            "为什么延期？",
            json!({ "after": "2026-08-01" }),
            vec!["延期".to_string()],
            vec![HandoffRef {
                path: "a.md".to_string(),
                line: 1,
                line_end: 100_000,
            }],
        )
        .unwrap();
        assert!(prompt.contains("\"path\":\"a.md\""));
        assert!(prompt.contains("notemd search"));
        assert!(!prompt.contains("PRIVATE_FILE_BODY"));
        assert!(!prompt.contains(&root.path().display().to_string()));
    }

    #[test]
    fn handoff_rejects_traversal_and_unknown_files() {
        let root = tempfile::tempdir().unwrap();
        for path in ["../outside.md", "missing.md", "nested\\file.md"] {
            let error = prepare_handoff_in(
                root.path(),
                "question",
                json!({}),
                vec![],
                vec![HandoffRef {
                    path: path.to_string(),
                    line: 1,
                    line_end: 1,
                }],
            )
            .unwrap_err();
            assert!(!error.is_empty(), "{path}");
        }
        assert!(prepare_handoff_in(
            root.path(),
            "question",
            json!({ "path": "/Users/private/vault" }),
            vec![],
            vec![],
        )
        .is_err());
        assert!(prepare_handoff_in(
            root.path(),
            "question",
            json!({}),
            vec!["/Users/private/topic".to_string()],
            vec![],
        )
        .is_err());
    }

    #[test]
    fn explicit_summary_selection_may_keep_two_blocks_from_one_file() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.md"), "one\ntwo\n").unwrap();
        let hits = vec![
            CanonicalHit {
                result_id: "r1".into(),
                path: "a.md".into(),
                line: 1,
                line_end: 1,
                level: "line".into(),
                indexed_text: "one".into(),
            },
            CanonicalHit {
                result_id: "r2".into(),
                path: "a.md".into(),
                line: 2,
                line_end: 2,
                level: "line".into(),
                indexed_text: "two".into(),
            },
        ];
        let run = run(root.path(), hits);
        let prepared = prepare_summary_in(
            &run,
            &["r1".to_string(), "r2".to_string()],
            4,
            4_000,
            "bullets",
        )
        .unwrap();
        assert_eq!(prepared.sources.len(), 2);
        assert!(prepare_summary_in(
            &run,
            &["r1".to_string(), "r1".to_string()],
            4,
            4_000,
            "bullets",
        )
        .is_err());
    }
}
