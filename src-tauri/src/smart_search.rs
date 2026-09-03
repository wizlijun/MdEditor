//! Trusted host boundary for Smart Search memory context, feedback and files.
//!
//! Agent providers remain read-only. Only these narrow commands may persist an
//! answer or detailed document, and every path is derived under the active
//! vault rather than accepted from a webview.

use chrono::{Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;

const MAX_ANSWER_BYTES: usize = 1_048_576;
const MAX_SOURCES: usize = 12;
const MAX_SOURCE_FILE_BYTES: u64 = 52_428_800;
const MAX_SOURCE_LINES: u32 = 500;
// Memory Protocol v2's established consent purpose for answering factual
// questions. Using a new ad-hoc string would make every existing USER.md and
// MEMORY.md projection ineligible even though the files visibly contain it.
const MEMORY_PURPOSE: &str = "information-answer";

static ARCHIVE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static FEEDBACK_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchSourcePayload {
    id: String,
    hit: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerArchivePayload {
    answer_id: String,
    query: String,
    answer: String,
    provider: String,
    model: Option<String>,
    run_id: String,
    memory_manifest_id: Option<String>,
    sources: Vec<SearchSourcePayload>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentWritePayload {
    title: String,
    query: String,
    content: String,
    provider: String,
    model: Option<String>,
    run_id: String,
    memory_manifest_id: Option<String>,
    sources: Vec<SearchSourcePayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveReceipt {
    path: String,
    created: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySelectionDto {
    claim_id: String,
    revision_id: String,
    text: String,
    target: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryContextDto {
    available: bool,
    selected: Vec<MemorySelectionDto>,
    excluded_summary: BTreeMap<String, u64>,
    manifest_id: Option<String>,
    error: Option<String>,
}

fn active_root(app: &AppHandle) -> Result<PathBuf, String> {
    crate::sotvault::resolve_vault_root(app).ok_or_else(|| "Vault not configured".into())
}

fn canonical_source_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty() || relative.len() > 2_048 || relative.contains('\0') {
        return Err("invalid source path".into());
    }
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("source path must stay inside the Vault".into());
    }
    let canonical_root = fs::canonicalize(root).map_err(|e| format!("resolve vault: {e}"))?;
    let mut current = canonical_root.clone();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("source path must stay inside the Vault".into());
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|e| format!("inspect source {}: {e}", relative.display()))?;
        if metadata.file_type().is_symlink() {
            return Err("refusing symlink source".into());
        }
    }
    let metadata = fs::symlink_metadata(&current)
        .map_err(|e| format!("inspect source {}: {e}", relative.display()))?;
    if !metadata.is_file() || metadata.len() > MAX_SOURCE_FILE_BYTES {
        return Err("source is not a readable indexed file".into());
    }
    let canonical = fs::canonicalize(&current)
        .map_err(|e| format!("resolve source {}: {e}", relative.display()))?;
    if !canonical.starts_with(&canonical_root) {
        return Err("source escaped the Vault".into());
    }
    Ok(canonical)
}

fn freeze_sources_in(
    root: &Path,
    mut sources: Vec<SearchSourcePayload>,
) -> Result<Vec<SearchSourcePayload>, String> {
    if sources.is_empty() || sources.len() > MAX_SOURCES {
        return Err("source count is outside the supported range".into());
    }
    for (index, source) in sources.iter_mut().enumerate() {
        if source.id != format!("S{}", index + 1) {
            return Err("invalid source id".into());
        }
        let hit = source.hit.as_object_mut().ok_or("invalid source hit")?;
        let relative = hit
            .get("path")
            .and_then(Value::as_str)
            .ok_or("source is missing path")?
            .to_string();
        let line = hit
            .get("line")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or("invalid source line")?;
        let line_end = hit
            .get("lineEnd")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .ok_or("invalid source lineEnd")?;
        if line_end < line || line_end - line >= MAX_SOURCE_LINES {
            return Err("invalid source line range".into());
        }
        let expected_ref = format!("{relative}#L{line}");
        if hit.get("sourceRef").and_then(Value::as_str) != Some(expected_ref.as_str()) {
            return Err("sourceRef does not match the source range".into());
        }
        let absolute = canonical_source_path(root, &relative)?;
        let body =
            fs::read_to_string(&absolute).map_err(|e| format!("read source {relative}: {e}"))?;
        let start = usize::try_from(line - 1).map_err(|_| "invalid source line")?;
        let count = usize::try_from(line_end - line + 1).map_err(|_| "invalid source range")?;
        let text = body
            .lines()
            .skip(start)
            .take(count)
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            return Err("source range no longer exists".into());
        }
        hit.insert("text".into(), Value::String(text));
        hit.insert(
            "absPath".into(),
            Value::String(absolute.to_string_lossy().to_string()),
        );
        hit.insert("sourceRef".into(), Value::String(expected_ref));
    }
    Ok(sources)
}

#[tauri::command]
pub fn smart_search_freeze_sources(
    app: AppHandle,
    sources: Vec<SearchSourcePayload>,
) -> Result<Vec<SearchSourcePayload>, String> {
    freeze_sources_in(&active_root(&app)?, sources)
}

fn ensure_fixed_dir(root: &Path, components: &[&str]) -> Result<PathBuf, String> {
    let canonical_root = fs::canonicalize(root).map_err(|e| format!("resolve vault: {e}"))?;
    let mut current = canonical_root.clone();
    for component in components {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!("refusing symlink directory: {component}"));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(format!("expected directory: {component}"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|e| format!("create {component}: {e}"))?;
            }
            Err(error) => return Err(format!("inspect {component}: {error}")),
        }
        let canonical =
            fs::canonicalize(&current).map_err(|e| format!("resolve {component}: {e}"))?;
        if !canonical.starts_with(&canonical_root) {
            return Err(format!("directory escaped vault: {component}"));
        }
        current = canonical;
    }
    Ok(current)
}

fn source_refs(sources: &[SearchSourcePayload]) -> Result<Vec<String>, String> {
    if sources.is_empty() {
        return Err("at least one source is required".into());
    }
    if sources.len() > MAX_SOURCES {
        return Err(format!("too many sources: {}", sources.len()));
    }
    let mut refs = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        if source.id != format!("S{}", index + 1) {
            return Err("invalid source id".into());
        }
        let hit = source.hit.as_object().ok_or("invalid source hit")?;
        let source_ref = hit
            .get("sourceRef")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .or_else(|| {
                let path = hit.get("path")?.as_str()?;
                let line = hit.get("line")?.as_u64()?;
                Some(format!("{path}#L{line}"))
            })
            .ok_or("source is missing sourceRef")?;
        if source_ref.contains('\0') || source_ref.len() > 2_048 {
            return Err("invalid sourceRef".into());
        }
        refs.push(source_ref);
    }
    Ok(refs)
}

fn slug(input: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for ch in input.trim().chars() {
        if ch.is_alphanumeric() {
            if separator && !result.is_empty() {
                result.push('-');
            }
            separator = false;
            for lower in ch.to_lowercase() {
                result.push(lower);
            }
        } else {
            separator = true;
        }
        if result.chars().count() >= 56 {
            break;
        }
    }
    let value = result.trim_matches('-');
    if value.is_empty() {
        "answer".into()
    } else {
        value.into()
    }
}

fn title_from_query(query: &str) -> String {
    let line = query.lines().next().unwrap_or_default().trim();
    let mut title = line.chars().take(96).collect::<String>();
    if line.chars().count() > 96 {
        title.push('…');
    }
    if title.is_empty() {
        "Answer".into()
    } else {
        title
    }
}

fn validate_generated_markdown(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_ANSWER_BYTES || value.contains('\0') {
        return Err(format!("invalid {label}"));
    }
    if value.contains('<') || value.contains('>') {
        return Err(format!("{label} contains raw HTML"));
    }
    if value.trim_start().starts_with("---") {
        return Err(format!("{label} contains frontmatter"));
    }
    let compact = value
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let inline_targets = compact.match_indices("](").filter_map(|(index, _)| {
        let target = &compact[index + 2..];
        Some(target.split(')').next().unwrap_or(target))
    });
    let reference_targets = value.lines().filter_map(|line| {
        let compact_line = line
            .chars()
            .filter(|ch| !ch.is_ascii_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        compact_line
            .find("]:")
            .map(|index| compact_line[index + 2..].to_string())
    });
    let unsafe_target = |target: &str| {
        ["javascript:", "vbscript:", "data:"]
            .iter()
            .any(|scheme| target.starts_with(scheme))
            || target.contains("&#")
            || target.contains("&colon;")
            || target.contains("%3a")
    };
    if inline_targets.into_iter().any(unsafe_target)
        || reference_targets
            .into_iter()
            .any(|target| unsafe_target(&target))
    {
        return Err(format!("{label} contains an unsafe URI"));
    }
    Ok(())
}

fn validate_user_text(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_ANSWER_BYTES || value.contains('\0') {
        return Err(format!("invalid {label}"));
    }
    Ok(())
}

fn escape_markdown_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn write_unique_markdown(dir: &Path, stem: &str, body: &str) -> Result<PathBuf, String> {
    for suffix in 1..=10_000 {
        let name = if suffix == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem}-{suffix}.md")
        };
        let target = dir.join(name);
        let mut temp = tempfile::NamedTempFile::new_in(dir)
            .map_err(|e| format!("create temporary answer: {e}"))?;
        temp.write_all(body.as_bytes())
            .and_then(|_| temp.as_file().sync_all())
            .map_err(|e| format!("write answer: {e}"))?;
        match temp.persist_noclobber(&target) {
            Ok(_) => return Ok(target),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("publish answer: {}", error.error)),
        }
    }
    Err("could not allocate an answer filename".into())
}

fn receipt_key(answer_id: &str) -> String {
    hex::encode(Sha256::digest(answer_id.as_bytes()))
}

fn payload_digest<T: Serialize>(payload: &T) -> Result<String, String> {
    let bytes = serde_json::to_vec(payload).map_err(|e| format!("encode archive payload: {e}"))?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn read_receipt(
    path: &Path,
    root: &Path,
    expected_digest: &str,
) -> Result<Option<ArchiveReceipt>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("inspect archive receipt: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("invalid archive receipt".into());
    }
    let raw = fs::read_to_string(path).map_err(|e| format!("read archive receipt: {e}"))?;
    let receipt: Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse archive receipt: {e}"))?;
    let relative = receipt
        .get("relativePath")
        .and_then(Value::as_str)
        .ok_or("archive receipt is missing relativePath")?;
    let digest = receipt
        .get("payloadSha256")
        .and_then(Value::as_str)
        .ok_or("archive receipt is missing payloadSha256")?;
    if digest != expected_digest {
        return Err("answer id is already archived with different content".into());
    }
    let absolute = root.join(relative);
    let metadata =
        fs::symlink_metadata(&absolute).map_err(|e| format!("inspect archived answer: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("invalid archived answer".into());
    }
    let canonical =
        fs::canonicalize(absolute).map_err(|e| format!("resolve archived answer: {e}"))?;
    if !canonical.starts_with(root) {
        return Err("archived answer escaped vault".into());
    }
    Ok(Some(ArchiveReceipt {
        path: canonical.to_string_lossy().to_string(),
        created: false,
    }))
}

fn write_receipt(
    path: &Path,
    root: &Path,
    answer: &Path,
    payload_sha256: &str,
) -> Result<(), String> {
    let relative = answer
        .strip_prefix(root)
        .map_err(|_| "answer escaped vault")?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::now_v7()));
    fs::write(
        &temp,
        serde_json::to_vec(&json!({
            "relativePath": relative,
            "payloadSha256": payload_sha256,
        }))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("write archive receipt: {e}"))?;
    fs::rename(&temp, path).map_err(|e| format!("publish archive receipt: {e}"))
}

fn answer_markdown(payload: &AnswerArchivePayload) -> Result<String, String> {
    validate_user_text(&payload.query, "query")?;
    validate_generated_markdown(&payload.answer, "answer")?;
    let sources = source_refs(&payload.sources)?;
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let by = match payload.model.as_deref() {
        Some(model) if !model.is_empty() => format!("{}/{}", payload.provider, model),
        _ => payload.provider.clone(),
    };
    let frontmatter = json!({
        "type": "Answer",
        "title": escape_markdown_html(&title_from_query(&payload.query)),
        "generated": { "by": by, "at": now },
        "feedback": { "value": "helpful", "by": "human", "at": now },
        "answer_run": payload.run_id,
        "answer_id": payload.answer_id,
        "memory_manifest": payload.memory_manifest_id,
        "sources": sources,
    });
    let yaml =
        serde_yaml::to_string(&frontmatter).map_err(|e| format!("encode answer metadata: {e}"))?;
    Ok(format!(
        "---\n{yaml}---\n\n## Question\n\n{}\n\n## Answer\n\n{}\n",
        escape_markdown_html(payload.query.trim()),
        payload.answer.trim()
    ))
}

fn archive_answer_in(root: &Path, payload: AnswerArchivePayload) -> Result<ArchiveReceipt, String> {
    if payload.answer_id.trim().is_empty() || payload.answer_id.len() > 256 {
        return Err("invalid answer id".into());
    }
    let _guard = ARCHIVE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "archive lock poisoned")?;
    // macOS commonly exposes /var as a symlink to /private/var. The fixed
    // directories below are canonical paths, so the root used for receipts
    // must be canonical too or strip_prefix would reject a path that is
    // actually inside the vault.
    let root = fs::canonicalize(root).map_err(|e| format!("resolve vault: {e}"))?;
    let answers = ensure_fixed_dir(&root, &["answers"])?;
    let receipts = ensure_fixed_dir(&root, &[".notemd", "smart-search", "archives"])?;
    let receipt_path = receipts.join(format!("{}.json", receipt_key(&payload.answer_id)));
    let digest = payload_digest(&payload)?;
    if let Some(receipt) = read_receipt(&receipt_path, &root, &digest)? {
        return Ok(receipt);
    }
    let markdown = answer_markdown(&payload)?;
    let date = Local::now().format("%Y-%m-%d");
    let stem = format!("{date}-answer-{}", slug(&payload.query));
    let path = write_unique_markdown(&answers, &stem, &markdown)?;
    write_receipt(&receipt_path, &root, &path, &digest)?;
    Ok(ArchiveReceipt {
        path: path.to_string_lossy().to_string(),
        created: true,
    })
}

#[tauri::command]
pub fn smart_search_archive_answer(
    app: AppHandle,
    payload: AnswerArchivePayload,
) -> Result<ArchiveReceipt, String> {
    let root = active_root(&app)?;
    archive_answer_in(&root, payload)
}

#[tauri::command]
pub fn smart_search_record_feedback(
    app: AppHandle,
    answer_id: String,
    value: String,
    reason: Option<String>,
) -> Result<(), String> {
    if answer_id.trim().is_empty()
        || answer_id.len() > 256
        || !matches!(value.as_str(), "helpful" | "unhelpful")
        || reason
            .as_deref()
            .is_some_and(|item| item.len() > 256 || item.contains('\0'))
    {
        return Err("invalid feedback".into());
    }
    let root = active_root(&app)?;
    let _guard = FEEDBACK_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "feedback lock poisoned")?;
    let dir = ensure_fixed_dir(&root, &[".notemd", "agent-runs", "search-answer"])?;
    let line = serde_json::to_string(&json!({
        "answerId": answer_id,
        "value": value,
        "reason": reason,
        "at": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    }))
    .map_err(|e| e.to_string())?;
    let feedback_path = dir.join("feedback.jsonl");
    if fs::symlink_metadata(&feedback_path).is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err("refusing symlink feedback file".into());
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(feedback_path)
        .map_err(|e| format!("open feedback log: {e}"))?;
    writeln!(file, "{line}")
        .and_then(|_| file.sync_data())
        .map_err(|e| format!("write feedback: {e}"))
}

fn document_markdown(payload: &DocumentWritePayload) -> Result<String, String> {
    validate_user_text(&payload.query, "query")?;
    validate_generated_markdown(&payload.content, "document")?;
    let sources = source_refs(&payload.sources)?;
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
    let by = match payload.model.as_deref() {
        Some(model) if !model.is_empty() => format!("{}/{}", payload.provider, model),
        _ => payload.provider.clone(),
    };
    let frontmatter = json!({
        "type": "Answer",
        "title": escape_markdown_html(payload.title.trim()),
        "generated": { "by": by, "at": now },
        "answer_run": payload.run_id,
        "memory_manifest": payload.memory_manifest_id,
        "query": escape_markdown_html(&payload.query),
        "sources": sources,
    });
    let yaml = serde_yaml::to_string(&frontmatter)
        .map_err(|e| format!("encode document metadata: {e}"))?;
    Ok(format!("---\n{yaml}---\n\n{}\n", payload.content.trim()))
}

fn write_document_in(root: &Path, payload: DocumentWritePayload) -> Result<ArchiveReceipt, String> {
    if payload.title.trim().is_empty() || payload.title.len() > 256 {
        return Err("invalid document title".into());
    }
    let root = fs::canonicalize(root).map_err(|e| format!("resolve vault: {e}"))?;
    let answers = ensure_fixed_dir(&root, &["answers"])?;
    let markdown = document_markdown(&payload)?;
    let date = Local::now().format("%Y-%m-%d");
    let stem = format!("{date}-{}", slug(&payload.title));
    let path = write_unique_markdown(&answers, &stem, &markdown)?;
    Ok(ArchiveReceipt {
        path: path.to_string_lossy().to_string(),
        created: true,
    })
}

#[tauri::command]
pub fn smart_search_write_document(
    app: AppHandle,
    payload: DocumentWritePayload,
) -> Result<ArchiveReceipt, String> {
    let root = active_root(&app)?;
    write_document_in(&root, payload)
}

#[tauri::command]
pub fn smart_search_memory_context(
    app: AppHandle,
    provider: String,
    model: Option<String>,
) -> Result<MemoryContextDto, String> {
    let root = active_root(&app)?;
    let model = model.unwrap_or_else(|| "default".into());
    let mut request = json!({
        "space": "global",
        "purpose": MEMORY_PURPOSE,
        "caller": "core:global-search",
        "provider": provider,
        "model": model,
        "tools": [],
        "external_transfer": true,
        "as_of_valid_time": Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    });
    let preview =
        match crate::memory_control::v2::dispatch_rpc(&root, "host.memory.v2.context", &request) {
            Ok(value) => value,
            Err(error) => {
                return Ok(MemoryContextDto {
                    available: false,
                    selected: Vec::new(),
                    excluded_summary: BTreeMap::new(),
                    manifest_id: None,
                    error: Some(error),
                })
            }
        };
    let excluded_summary = serde_json::from_value(
        preview
            .get("excluded_summary")
            .cloned()
            .unwrap_or_else(|| json!({})),
    )
    .unwrap_or_default();
    let allowed = preview
        .pointer("/policy_result/external_action_allowed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !allowed {
        return Ok(MemoryContextDto {
            available: true,
            selected: Vec::new(),
            excluded_summary,
            manifest_id: None,
            error: Some("Memory policy did not allow this external transfer".into()),
        });
    }

    let repository = crate::memory_control::v2::V2Repository::new(&root)
        .load()
        .map_err(|e| e.to_string())?;
    let targets = repository
        .claims
        .iter()
        .map(|loaded| {
            let target = match loaded.value.projection.target {
                crate::memory_control::v2::ProjectionTarget::User => "user",
                crate::memory_control::v2::ProjectionTarget::Memory => "memory",
            };
            (loaded.value.revision_id.as_str(), target)
        })
        .collect::<BTreeMap<_, _>>();
    let mut selected = preview
        .get("selected")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let claim_id = item.get("claim_id")?.as_str()?.to_string();
            let revision_id = item.get("revision_id")?.as_str()?.to_string();
            let text = item.get("text")?.as_str()?.to_string();
            let target = targets
                .get(revision_id.as_str())
                .copied()
                .unwrap_or("memory")
                .to_string();
            Some(MemorySelectionDto {
                claim_id,
                revision_id,
                text,
                target,
            })
        })
        .collect::<Vec<_>>();
    selected.sort_by_key(|item| (item.target != "user", item.claim_id.clone()));

    request["preview_sha256"] = preview
        .get("preview_sha256")
        .cloned()
        .unwrap_or(Value::Null);
    let manifest =
        crate::memory_control::v2::dispatch_rpc(&root, "host.memory.v2.contextManifest", &request)
            .map_err(|e| format!("publish memory context manifest: {e}"))?;
    Ok(MemoryContextDto {
        available: true,
        selected,
        excluded_summary,
        manifest_id: manifest
            .get("manifest_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> SearchSourcePayload {
        SearchSourcePayload {
            id: "S1".into(),
            hit: json!({ "path": "notes/a.md", "line": 3, "sourceRef": "notes/a.md#L3" }),
        }
    }

    fn answer(id: &str) -> AnswerArchivePayload {
        AnswerArchivePayload {
            answer_id: id.into(),
            query: "Release risks?".into(),
            answer: "Fix architecture first. [S1]".into(),
            provider: "notemd.test-agent".into(),
            model: Some("test".into()),
            run_id: "run-1".into(),
            memory_manifest_id: None,
            sources: vec![source()],
        }
    }

    #[test]
    fn archive_is_idempotent_and_never_marks_helpful_as_verified() {
        let dir = tempfile::tempdir().unwrap();
        let first = archive_answer_in(dir.path(), answer("answer-1")).unwrap();
        let second = archive_answer_in(dir.path(), answer("answer-1")).unwrap();
        assert!(first.created);
        assert!(!second.created);
        assert_eq!(first.path, second.path);
        let body = fs::read_to_string(first.path).unwrap();
        assert!(body.contains("value: helpful"));
        assert!(!body.contains("verified"));
        assert!(body.contains("notes/a.md#L3"));
    }

    #[test]
    fn detailed_documents_do_not_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let payload = DocumentWritePayload {
            title: "Release plan".into(),
            query: "what changed".into(),
            content: "# Release plan\n\nBody [S1]".into(),
            provider: "agent".into(),
            model: None,
            run_id: "r1".into(),
            memory_manifest_id: None,
            sources: vec![source()],
        };
        let first = write_document_in(dir.path(), payload.clone()).unwrap();
        let second = write_document_in(dir.path(), payload).unwrap();
        assert_ne!(first.path, second.path);
        assert!(Path::new(&first.path).is_file());
        assert!(Path::new(&second.path).is_file());
    }

    #[test]
    fn rejects_an_answers_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), dir.path().join("answers")).unwrap();
        #[cfg(unix)]
        assert!(archive_answer_in(dir.path(), answer("answer-2"))
            .unwrap_err()
            .contains("symlink"));
    }

    #[test]
    fn archive_escapes_html_in_the_question() {
        let dir = tempfile::tempdir().unwrap();
        let mut payload = answer("answer-unsafe");
        payload.query = "<script>alert(1)</script>".into();
        let receipt = archive_answer_in(dir.path(), payload).unwrap();
        let body = fs::read_to_string(receipt.path).unwrap();
        assert!(body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(!body.contains("<script>"));
    }

    #[test]
    fn rejects_raw_html_and_dangerous_links_in_generated_markdown() {
        for unsafe_answer in [
            "<img src=x onerror=alert(1)>",
            "<svg onload=alert(1)>",
            "<meta http-equiv=refresh content=x>",
            "[open](javascript:alert(1))",
            "[open](java&#x73;cript&#58;alert(1))",
            "[open]: data:text/html;base64,WA==",
        ] {
            let mut payload = answer("unused");
            payload.answer = unsafe_answer.into();
            assert!(
                answer_markdown(&payload).is_err(),
                "accepted {unsafe_answer}"
            );
        }
    }

    #[test]
    fn rejects_frontmatter_in_generated_documents() {
        let payload = DocumentWritePayload {
            title: "Release plan".into(),
            query: "what changed".into(),
            content: "---\ntype: injected\n---\nBody".into(),
            provider: "agent".into(),
            model: None,
            run_id: "r1".into(),
            memory_manifest_id: None,
            sources: vec![source()],
        };
        assert!(document_markdown(&payload)
            .unwrap_err()
            .contains("frontmatter"));
    }

    #[test]
    fn archive_receipt_rejects_reused_id_with_different_payload() {
        let dir = tempfile::tempdir().unwrap();
        archive_answer_in(dir.path(), answer("answer-conflict")).unwrap();
        let mut changed = answer("answer-conflict");
        changed.answer = "A different answer. [S1]".into();
        assert!(archive_answer_in(dir.path(), changed)
            .unwrap_err()
            .contains("different content"));
    }

    #[test]
    fn sources_must_use_sequential_unique_ids() {
        let mut payload = answer("unused");
        payload.sources[0].id = "S2".into();
        assert!(answer_markdown(&payload).unwrap_err().contains("source id"));
    }

    #[test]
    fn frozen_sources_are_reread_from_the_vault_instead_of_trusting_webview_text() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("notes")).unwrap();
        fs::write(dir.path().join("notes/a.md"), "one\nauthoritative\nthree\n").unwrap();
        let source = SearchSourcePayload {
            id: "S1".into(),
            hit: json!({
                "path": "notes/a.md",
                "absPath": "/tampered/a.md",
                "line": 2,
                "lineEnd": 2,
                "text": "tampered",
                "sourceRef": "notes/a.md#L2"
            }),
        };

        let frozen = freeze_sources_in(dir.path(), vec![source]).unwrap();
        assert_eq!(frozen[0].hit["text"], "authoritative");
        assert_eq!(
            frozen[0].hit["absPath"],
            fs::canonicalize(dir.path().join("notes/a.md"))
                .unwrap()
                .to_string_lossy()
                .as_ref()
        );
    }

    #[cfg(unix)]
    #[test]
    fn frozen_sources_reject_traversal_and_symlink_leaves() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::create_dir(dir.path().join("notes")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("notes/link.md")).unwrap();
        for path in ["../outside.md", "notes/link.md"] {
            let source = SearchSourcePayload {
                id: "S1".into(),
                hit: json!({
                    "path": path,
                    "line": 1,
                    "lineEnd": 1,
                    "sourceRef": format!("{path}#L1")
                }),
            };
            assert!(freeze_sources_in(dir.path(), vec![source]).is_err());
        }
    }
}
