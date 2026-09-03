use crate::model::{NormalizedMeeting, SourceDetection, SpeakerMeta};
use crate::srt::{canonical_builtin, validate_srt, SpeakerLookup};
use chrono::{DateTime, FixedOffset, LocalResult, NaiveDateTime, TimeZone};
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "m4a", "aac"];

#[derive(Clone, Debug)]
pub struct UserSource {
    pub user_id: String,
    pub conversation_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct DiscoveredConversation {
    pub path: PathBuf,
    pub relative_path: String,
    pub schema: String,
}

#[derive(Clone, Debug)]
pub struct NormalizeFailure {
    pub conversation_id: String,
    pub source_relative_path: String,
    pub source_schema: String,
    pub reason: String,
    pub warnings: Vec<String>,
}

pub fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn path_string(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn sorted_dirs(path: &Path, warnings: &mut Vec<String>) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    for entry in fs::read_dir(path).map_err(|error| format!("read {}: {error}", path.display()))? {
        let entry = entry.map_err(|error| format!("read {}: {error}", path.display()))?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)
            .map_err(|error| format!("inspect {}: {error}", child.display()))?;
        if metadata.file_type().is_symlink() {
            warnings.push(format!("ignored symlink: {}", child.display()));
        } else if metadata.is_dir() {
            result.push(child);
        }
    }
    result.sort();
    Ok(result)
}

fn validate_selected_root(source: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("source {} is unavailable: {error}", source.display()))?;
    if metadata.file_type().is_symlink() {
        return Err("selected source root must not be a symlink".into());
    }
    if !metadata.is_dir() {
        return Err("selected source must be a directory".into());
    }
    source
        .canonicalize()
        .map_err(|error| format!("canonicalize source {}: {error}", source.display()))
}

fn is_conversation_root(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("conversation" | "conversations")
    )
}

fn direct_conversation_roots(path: &Path) -> Vec<PathBuf> {
    [path.join("conversation"), path.join("conversations")]
        .into_iter()
        .filter(|candidate| {
            fs::symlink_metadata(candidate)
                .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
                .unwrap_or(false)
        })
        .collect()
}

pub fn users_at(source: &Path) -> Result<(PathBuf, Vec<UserSource>, Vec<String>), String> {
    let root = validate_selected_root(source)?;
    let mut warnings = Vec::new();
    let mut users: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

    if is_conversation_root(&root) {
        let user = root
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("default")
            .to_string();
        users.entry(user).or_default().push(root.clone());
    } else {
        let direct = direct_conversation_roots(&root);
        if !direct.is_empty() {
            let user = root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("default")
                .to_string();
            users.entry(user).or_default().extend(direct);
        } else {
            let mut possible_user_roots = sorted_dirs(&root, &mut warnings)?;
            let users_dir = root.join("users");
            if fs::symlink_metadata(&users_dir)
                .map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
                .unwrap_or(false)
            {
                possible_user_roots.extend(sorted_dirs(&users_dir, &mut warnings)?);
            }
            possible_user_roots.sort();
            possible_user_roots.dedup();
            for user_root in possible_user_roots {
                let roots = direct_conversation_roots(&user_root);
                if roots.is_empty() {
                    continue;
                }
                let user = user_root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("default")
                    .to_string();
                users.entry(user).or_default().extend(roots);
            }
        }
    }
    if users.is_empty() {
        return Err("no Hemory conversation layout found under selected source".into());
    }
    let users = users
        .into_iter()
        .map(|(user_id, mut conversation_roots)| {
            conversation_roots.sort();
            conversation_roots.dedup();
            UserSource {
                user_id,
                conversation_roots,
            }
        })
        .collect();
    Ok((root, users, warnings))
}

pub fn detect(source: &Path) -> Result<SourceDetection, String> {
    let (root, users, mut warnings) = users_at(source)?;
    let selected_user = (users.len() == 1).then(|| users[0].user_id.clone());
    let mut needs_timezone = false;
    for user in &users {
        let (candidates, _) = discover_conversations(&root, user)?;
        for candidate in candidates {
            let meta_path = candidate.path.join("meta.json");
            if let Ok(Some(bytes)) = safe_read_optional(&candidate.path, &meta_path) {
                if let Ok(meta) = serde_json::from_slice::<Value>(&bytes) {
                    if let Some(created) = first_str(&meta, &["created_at", "createdAt"]) {
                        if !has_explicit_offset(created) {
                            needs_timezone = true;
                        }
                    }
                }
            }
            if !meta_path.exists() {
                needs_timezone = true;
            }
        }
    }
    if users.len() > 1 {
        warnings.push("multiple Hemory users detected; select exactly one user".into());
    }
    Ok(SourceDetection {
        source: root.to_string_lossy().into_owned(),
        users: users.into_iter().map(|user| user.user_id).collect(),
        selected_user,
        needs_timezone,
        warnings,
    })
}

fn is_tombstone(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name == "_deleted" || name.starts_with("_deleted_") || name.starts_with(".deleted_")
    })
}

fn looks_like_conversation(path: &Path) -> bool {
    [
        "meta.json",
        "content.md",
        "pro_asr.srt",
        "speakers.json",
        "summary.md",
    ]
    .iter()
    .any(|name| path.join(name).exists())
}

fn schema_for(root: &Path, candidate: &Path) -> String {
    let name = candidate
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if root.file_name().and_then(|name| name.to_str()) == Some("conversations")
        || name.starts_with("conv_")
    {
        "legacy-vault-conversation".into()
    } else if name.starts_with("session_")
        || Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}$")
            .unwrap()
            .is_match(name)
    {
        "legacy-ios-session".into()
    } else {
        "current-conversation-v1".into()
    }
}

fn discover_in(
    selected_root: &Path,
    conversation_root: &Path,
    current: &Path,
    depth: usize,
    found: &mut Vec<DiscoveredConversation>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    if depth > 5 || is_tombstone(current) {
        return Ok(());
    }
    if current != conversation_root && looks_like_conversation(current) {
        let relative_path = current
            .strip_prefix(selected_root)
            .map(path_string)
            .unwrap_or_else(|_| path_string(current));
        found.push(DiscoveredConversation {
            path: current.to_path_buf(),
            relative_path,
            schema: schema_for(conversation_root, current),
        });
        return Ok(());
    }
    for child in sorted_dirs(current, warnings)? {
        if child.file_name().and_then(|name| name.to_str()) == Some("audio") {
            continue;
        }
        discover_in(
            selected_root,
            conversation_root,
            &child,
            depth + 1,
            found,
            warnings,
        )?;
    }
    Ok(())
}

pub fn discover_conversations(
    selected_root: &Path,
    user: &UserSource,
) -> Result<(Vec<DiscoveredConversation>, Vec<String>), String> {
    let mut found = Vec::new();
    let mut warnings = Vec::new();
    for conversation_root in &user.conversation_roots {
        discover_in(
            selected_root,
            conversation_root,
            conversation_root,
            0,
            &mut found,
            &mut warnings,
        )?;
    }
    found.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    found.dedup_by(|left, right| left.path == right.path);
    Ok((found, warnings))
}

pub fn count_audio_files(candidates: &[DiscoveredConversation]) -> usize {
    fn count(path: &Path) -> usize {
        let Ok(entries) = fs::read_dir(path) else {
            return 0;
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| {
                let child = entry.path();
                let Ok(metadata) = fs::symlink_metadata(&child) else {
                    return 0;
                };
                if metadata.file_type().is_symlink() {
                    0
                } else if metadata.is_dir() {
                    count(&child)
                } else {
                    child
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(|ext| AUDIO_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
                        .unwrap_or(false) as usize
                }
            })
            .sum()
    }
    candidates
        .iter()
        .map(|candidate| count(&candidate.path))
        .sum()
}

fn safe_read_optional(base: &Path, path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("inspect {}: {error}", path.display())),
    }
    let relative = path
        .strip_prefix(base)
        .map_err(|_| format!("path escapes conversation: {}", path.display()))?;
    let mut current = base.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(format!("unsafe relative path: {}", relative.display()));
        };
        current.push(part);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("inspect {}: {error}", current.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "source symlink is not allowed: {}",
                current.display()
            ));
        }
    }
    let metadata =
        fs::metadata(path).map_err(|error| format!("inspect {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("expected a regular file: {}", path.display()));
    }
    fs::read(path)
        .map(Some)
        .map_err(|error| format!("read {}: {error}", path.display()))
}

fn first_str<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(*key)?.as_str())
}

fn first_nonempty_trimmed<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| value.get(*key)?.as_str())
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn has_explicit_offset(value: &str) -> bool {
    DateTime::parse_from_rfc3339(value).is_ok()
}

fn parse_datetime(value: &str, timezone: Option<&str>) -> Result<DateTime<FixedOffset>, String> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed);
    }
    let patterns = [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y%m%d_%H%M%S",
        "%Y-%m-%dT%H-%M-%S",
    ];
    let naive = patterns
        .iter()
        .find_map(|pattern| NaiveDateTime::parse_from_str(value, pattern).ok())
        .ok_or_else(|| format!("invalid meeting timestamp '{value}'"))?;
    let timezone =
        timezone.ok_or_else(|| "needs_timezone: timestamp has no UTC offset".to_string())?;
    let zone: chrono_tz::Tz = timezone
        .parse()
        .map_err(|_| format!("unknown IANA timezone '{timezone}'"))?;
    let zoned = match zone.from_local_datetime(&naive) {
        LocalResult::Single(value) => value,
        LocalResult::Ambiguous(_, _) => {
            return Err(format!("ambiguous local timestamp '{value}' in {timezone}"))
        }
        LocalResult::None => {
            return Err(format!(
                "nonexistent local timestamp '{value}' in {timezone}"
            ))
        }
    };
    Ok(zoned.fixed_offset())
}

fn format_datetime(value: DateTime<FixedOffset>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Millis, false)
}

fn normalized_id(name: &str) -> Option<String> {
    let stripped = name.strip_prefix("conv_").unwrap_or(name);
    Regex::new(r"^\d{8}_\d{6}(?:_\d+)?$")
        .unwrap()
        .is_match(stripped)
        .then(|| stripped.to_string())
}

fn time_from_directory(name: &str) -> Option<String> {
    if let Some(rest) = name.strip_prefix("session_") {
        let value = rest.get(..15)?;
        if NaiveDateTime::parse_from_str(value, "%Y%m%d_%H%M%S").is_ok() {
            return Some(value.to_string());
        }
    }
    if Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}-\d{2}-\d{2}$")
        .unwrap()
        .is_match(name)
    {
        return Some(name.to_string());
    }
    normalized_id(name).map(|id| id[..15].to_string())
}

fn topics_from(meta: &Value) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut topics = Vec::new();
    for key in ["key_topics", "tags"] {
        if let Some(values) = meta.get(key).and_then(Value::as_array) {
            for value in values.iter().filter_map(Value::as_str) {
                if seen.insert(value.to_string()) {
                    topics.push(value.to_string());
                }
            }
        }
    }
    topics
}

fn canonical_for_entry(label: &str, entry: &Value) -> String {
    canonical_builtin(label)
        .or_else(|| {
            entry
                .get("cluster_id")
                .and_then(Value::as_str)
                .and_then(canonical_builtin)
        })
        .unwrap_or_else(|| label.to_string())
}

fn add_speaker(lookup: &mut SpeakerLookup, label: String, entry: &Value) {
    let canonical = canonical_for_entry(&label, entry);
    lookup.canonical.insert(label.clone(), canonical.clone());
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .map(ToOwned::to_owned);
    let voiceprint = entry
        .get("voiceprint_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .is_some();
    let speaker = lookup.metadata.entry(canonical).or_insert(SpeakerMeta {
        name: None,
        name_source: if voiceprint { "voiceprint" } else { "hemory" }.into(),
        source_labels: Vec::new(),
    });
    if speaker.name.is_none() {
        speaker.name = name;
    }
    if voiceprint {
        speaker.name_source = "voiceprint".into();
    }
    if !speaker.source_labels.contains(&label) {
        speaker.source_labels.push(label);
    }
}

fn speaker_lookup(meta: &Value, speakers_json: Option<&Value>) -> SpeakerLookup {
    let mut lookup = SpeakerLookup::default();
    if let Some(entries) = speakers_json
        .and_then(|value| value.get("speakers"))
        .and_then(Value::as_object)
    {
        for (label, entry) in entries {
            add_speaker(&mut lookup, label.clone(), entry);
        }
    }
    if let Some(entries) = meta.get("speakers").and_then(Value::as_array) {
        for (index, entry) in entries.iter().enumerate() {
            let default_label = format!("spk_{:02}", index + 1);
            match entry {
                Value::String(name) => {
                    add_speaker(&mut lookup, default_label, &json!({"name": name}))
                }
                Value::Object(_) => {
                    let label = first_str(entry, &["id", "speaker_id", "label"])
                        .unwrap_or(&default_label)
                        .to_string();
                    add_speaker(&mut lookup, label, entry);
                }
                _ => {}
            }
        }
    }
    lookup
}

fn fail(
    candidate: &DiscoveredConversation,
    id: String,
    reason: impl Into<String>,
) -> NormalizeFailure {
    NormalizeFailure {
        conversation_id: id,
        source_relative_path: candidate.relative_path.clone(),
        source_schema: candidate.schema.clone(),
        reason: reason.into(),
        warnings: Vec::new(),
    }
}

pub fn normalize(
    selected_root: &Path,
    candidate: &DiscoveredConversation,
    timezone: Option<&str>,
) -> Result<NormalizedMeeting, NormalizeFailure> {
    let dir_name = candidate
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    let guessed_id = normalized_id(&dir_name).unwrap_or_else(|| dir_name.clone());
    ensure_source_directory(selected_root, &candidate.path)
        .map_err(|error| fail(candidate, guessed_id.clone(), error))?;
    let meta_bytes = safe_read_optional(&candidate.path, &candidate.path.join("meta.json"))
        .map_err(|error| fail(candidate, guessed_id.clone(), error))?;
    let meta: Value = match meta_bytes {
        Some(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            fail(
                candidate,
                guessed_id.clone(),
                format!("invalid meta.json: {error}"),
            )
        })?,
        None => json!({}),
    };

    let created_raw = first_str(&meta, &["created_at", "createdAt"])
        .map(ToOwned::to_owned)
        .or_else(|| time_from_directory(&dir_name))
        .ok_or_else(|| {
            fail(
                candidate,
                guessed_id.clone(),
                "missing created_at and directory timestamp",
            )
        })?;
    let started = parse_datetime(&created_raw, timezone)
        .map_err(|error| fail(candidate, guessed_id.clone(), error))?;
    let directory_id = normalized_id(&dir_name);
    let raw_meta_id = first_str(&meta, &["conv_id", "session_id", "sessionId"]);
    let normalized_meta_id = raw_meta_id.and_then(normalized_id);
    if let (Some(directory_id), Some(meta_id)) = (&directory_id, &normalized_meta_id) {
        if directory_id != meta_id {
            return Err(fail(
                candidate,
                directory_id.clone(),
                format!("directory ID '{directory_id}' conflicts with metadata ID '{meta_id}'"),
            ));
        }
    }
    let conversation_id = directory_id
        .or(normalized_meta_id)
        .unwrap_or_else(|| started.format("%Y%m%d_%H%M%S").to_string());
    let original_conversation_id = first_str(&meta, &["conv_id", "session_id", "sessionId"])
        .unwrap_or(&dir_name)
        .to_string();

    let speakers_bytes = safe_read_optional(&candidate.path, &candidate.path.join("speakers.json"))
        .map_err(|error| fail(candidate, conversation_id.clone(), error))?;
    let speakers_json = speakers_bytes
        .as_ref()
        .map(|bytes| {
            serde_json::from_slice::<Value>(bytes).map_err(|error| {
                fail(
                    candidate,
                    conversation_id.clone(),
                    format!("invalid speakers.json: {error}"),
                )
            })
        })
        .transpose()?;
    let lookup = speaker_lookup(&meta, speakers_json.as_ref());

    let content_path = candidate.path.join("content.md");
    let content_bytes = safe_read_optional(&candidate.path, &content_path)
        .map_err(|error| fail(candidate, conversation_id.clone(), error))?;
    let (
        transcript_source_kind,
        transcript_relative_path,
        transcript_output_file,
        transcript_format,
        transcript_bytes,
        content_speakers,
        srt_validation,
    ) = if let Some(bytes) = content_bytes {
        let validation = crate::srt::validate_content_markdown(&bytes).map_err(|error| {
            fail(
                candidate,
                conversation_id.clone(),
                format!("no_valid_transcript: content.md: {error}"),
            )
        })?;
        (
            "content_md".to_string(),
            "content.md".to_string(),
            "transcript.md".to_string(),
            "markdown".to_string(),
            bytes,
            Some(validation.speakers),
            None,
        )
    } else {
        let bytes = safe_read_optional(&candidate.path, &candidate.path.join("pro_asr.srt"))
            .map_err(|error| fail(candidate, conversation_id.clone(), error))?
            .ok_or_else(|| {
                fail(
                    candidate,
                    conversation_id.clone(),
                    "no_valid_transcript: content.md and pro_asr.srt are missing",
                )
            })?;
        let validation = validate_srt(&bytes, &lookup).map_err(|error| {
            fail(
                candidate,
                conversation_id.clone(),
                format!("no_valid_transcript: pro_asr.srt: {error}"),
            )
        })?;
        (
            "pro_asr".to_string(),
            "pro_asr.srt".to_string(),
            "transcript.srt".to_string(),
            "srt".to_string(),
            bytes,
            None,
            Some(validation),
        )
    };

    let summary_bytes = safe_read_optional(&candidate.path, &candidate.path.join("summary.md"))
        .map_err(|error| fail(candidate, conversation_id.clone(), error))?;
    if let Some(summary) = &summary_bytes {
        std::str::from_utf8(summary).map_err(|_| {
            fail(
                candidate,
                conversation_id.clone(),
                "summary.md is not UTF-8",
            )
        })?;
    }

    let mut warnings = Vec::new();
    if raw_meta_id.is_some() && normalized_id(raw_meta_id.unwrap()).is_none() {
        warnings.push(
            "metadata conversation ID is not a canonical Hemory ID; used the next ID source".into(),
        );
    }
    let category = match meta.get("category") {
        Some(Value::String(value)) => Some(Value::String(value.clone())),
        Some(Value::Array(values)) => Some(Value::Array(values.clone())),
        Some(Value::Null) | None => None,
        Some(other) => {
            warnings.push(format!("ignored unsupported category value: {other}"));
            None
        }
    };
    let duration_ms = meta
        .pointer("/audio/duration_ms")
        .and_then(Value::as_u64)
        .or_else(|| meta.get("duration_ms").and_then(Value::as_u64))
        .or_else(|| {
            meta.get("duration")
                .and_then(Value::as_u64)
                .map(|seconds| seconds * 1000)
        });
    let ended_at = first_str(&meta, &["end_at", "ended_at", "endAt"])
        .map(|value| parse_datetime(value, timezone).map(format_datetime))
        .transpose()
        .map_err(|error| fail(candidate, conversation_id.clone(), error))?;
    let source_value =
        first_nonempty_trimmed(&meta, &["source", "device_source"]).unwrap_or("unknown");
    let source = format!("hemory_v1.0:{source_value}");
    let source_updated_at = first_str(&meta, &["updated_at", "updatedAt"]).and_then(|value| {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|_| value.to_string())
    });

    let mut speakers = BTreeMap::new();
    if let Some(labels) = content_speakers {
        for label in labels {
            let canonical = lookup
                .canonical
                .get(&label)
                .cloned()
                .unwrap_or_else(|| label.clone());
            let mut speaker = lookup
                .metadata
                .get(&canonical)
                .cloned()
                .unwrap_or(SpeakerMeta {
                    name: Some(label.clone()),
                    name_source: "content".into(),
                    source_labels: Vec::new(),
                });
            if !speaker.source_labels.contains(&label) {
                speaker.source_labels.push(label);
            }
            speaker.source_labels.sort();
            speakers.insert(canonical, speaker);
        }
    } else if let Some(validation) = srt_validation {
        for canonical in &validation.canonical_labels {
            let mut speaker = lookup
                .metadata
                .get(canonical)
                .cloned()
                .unwrap_or(SpeakerMeta {
                    name: None,
                    name_source: "hemory".into(),
                    source_labels: Vec::new(),
                });
            for label in &validation.source_labels {
                let mapped =
                    canonical_builtin(label).or_else(|| lookup.canonical.get(label).cloned());
                if mapped.as_deref() == Some(canonical) && !speaker.source_labels.contains(label) {
                    speaker.source_labels.push(label.clone());
                }
            }
            speaker.source_labels.sort();
            speakers.insert(canonical.clone(), speaker);
        }
    }

    let transcript_sha256 = sha256(&transcript_bytes);
    let summary_sha256 = summary_bytes.as_ref().map(|bytes| sha256(bytes));
    let fingerprint_value = json!({
        "conversation_id": conversation_id,
        "started_at": format_datetime(started),
        "ended_at": ended_at,
        "duration_ms": duration_ms,
        "title": first_str(&meta, &["title"]),
        "language": first_str(&meta, &["language"]),
        "category": category,
        "topics": topics_from(&meta),
        "source": source,
        "source_updated_at": source_updated_at,
        "speakers": speakers,
        "transcript_kind": transcript_source_kind,
        "transcript_sha256": transcript_sha256,
        "summary_sha256": summary_sha256,
    });
    let fingerprint = sha256(
        &serde_json::to_vec(&fingerprint_value)
            .map_err(|error| fail(candidate, conversation_id.clone(), error.to_string()))?,
    );

    Ok(NormalizedMeeting {
        source_abs: candidate.path.clone(),
        source_relative_path: candidate.relative_path.clone(),
        source_schema: candidate.schema.clone(),
        conversation_id,
        original_conversation_id,
        original_directory: dir_name,
        started_at: fingerprint_value["started_at"]
            .as_str()
            .unwrap()
            .to_string(),
        ended_at,
        duration_ms,
        title: first_str(&meta, &["title"]).map(ToOwned::to_owned),
        language: first_str(&meta, &["language"]).map(ToOwned::to_owned),
        category,
        topics: topics_from(&meta),
        source,
        source_updated_at,
        speakers,
        transcript_bytes,
        transcript_source_kind,
        transcript_relative_path,
        transcript_output_file,
        transcript_format,
        transcript_sha256,
        summary_bytes,
        summary_sha256,
        fingerprint,
        warnings,
    })
}

fn ensure_source_directory(selected_root: &Path, directory: &Path) -> Result<(), String> {
    let selected_root = selected_root
        .canonicalize()
        .map_err(|error| format!("canonicalize {}: {error}", selected_root.display()))?;
    let relative = directory.strip_prefix(&selected_root).map_err(|_| {
        format!(
            "conversation escapes selected source: {}",
            directory.display()
        )
    })?;
    let mut current = selected_root.clone();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(format!("unsafe conversation path: {}", directory.display()));
        };
        current.push(part);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("inspect {}: {error}", current.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "source symlink is not allowed: {}",
                current.display()
            ));
        }
        if !metadata.is_dir() {
            return Err(format!(
                "conversation is not a directory: {}",
                current.display()
            ));
        }
    }
    let canonical = directory
        .canonicalize()
        .map_err(|error| format!("canonicalize {}: {error}", directory.display()))?;
    if !canonical.starts_with(&selected_root) {
        return Err(format!(
            "conversation escapes selected source: {}",
            directory.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn good_srt(label: &str) -> String {
        format!("1\n00:00:00,000 --> 00:00:01,000\n[{label}] hello\n")
    }

    #[test]
    fn discovers_current_plural_and_ios_layouts() {
        let dir = tempdir().unwrap();
        for path in [
            "alice/conversation/202604/20260403_173300",
            "bob/conversations/202604/conv_20260403_173301_01",
            "carol/conversation/2026/04/03/session_20260403_173302_x",
        ] {
            let target = dir.path().join(path);
            fs::create_dir_all(&target).unwrap();
            fs::write(target.join("pro_asr.srt"), good_srt("spk_01")).unwrap();
        }
        let (_, users, _) = users_at(dir.path()).unwrap();
        assert_eq!(
            users.iter().map(|u| u.user_id.as_str()).collect::<Vec<_>>(),
            vec!["alice", "bob", "carol"]
        );
        for user in users {
            assert_eq!(
                discover_conversations(dir.path(), &user).unwrap().0.len(),
                1
            );
        }
    }

    #[test]
    fn content_is_copied_byte_for_byte_and_wins_over_pro() {
        let dir = tempdir().unwrap();
        let meeting = dir.path().join("alice/conversation/202604/20260403_173300");
        fs::create_dir_all(&meeting).unwrap();
        fs::write(
            meeting.join("meta.json"),
            r#"{"created_at":"2026-04-03T17:33:00+08:00"}"#,
        )
        .unwrap();
        let bytes =
            b"\xef\xbb\xbf# Weekly\r\n---\r\nSummary: ready\r\n---\r\n00:00:00  Alice: hello\r\n";
        fs::write(meeting.join("content.md"), bytes).unwrap();
        fs::write(meeting.join("pro_asr.srt"), good_srt("spk_01")).unwrap();
        let (_, users, _) = users_at(dir.path()).unwrap();
        let candidate = discover_conversations(dir.path(), &users[0])
            .unwrap()
            .0
            .remove(0);
        let normalized = normalize(dir.path(), &candidate, None).unwrap();
        assert_eq!(normalized.transcript_source_kind, "content_md");
        assert_eq!(normalized.transcript_output_file, "transcript.md");
        assert_eq!(normalized.transcript_format, "markdown");
        assert_eq!(normalized.transcript_bytes, bytes);
        assert_eq!(normalized.transcript_sha256, sha256(bytes));
        assert_eq!(normalized.source, "hemory_v1.0:unknown");
    }

    #[test]
    fn invalid_content_blocks_without_falling_back_to_good_pro() {
        let dir = tempdir().unwrap();
        let meeting = dir.path().join("conversation/202604/20260403_173300");
        fs::create_dir_all(&meeting).unwrap();
        fs::write(
            meeting.join("meta.json"),
            r#"{"created_at":"2026-04-03T17:33:00+08:00"}"#,
        )
        .unwrap();
        fs::write(
            meeting.join("content.md"),
            "plain text without time or speaker",
        )
        .unwrap();
        fs::write(meeting.join("pro_asr.srt"), good_srt("spk_01")).unwrap();
        let (_, users, _) = users_at(dir.path()).unwrap();
        let candidate = discover_conversations(dir.path(), &users[0])
            .unwrap()
            .0
            .remove(0);
        let error = normalize(dir.path(), &candidate, None).unwrap_err();
        assert!(error.reason.contains("content.md"));
        assert!(error.reason.contains("no_valid_transcript"));
    }

    #[test]
    fn pro_is_copied_byte_for_byte_only_when_content_is_absent() {
        let dir = tempdir().unwrap();
        let meeting = dir.path().join("conversation/202604/20260403_173300");
        fs::create_dir_all(&meeting).unwrap();
        fs::write(
            meeting.join("meta.json"),
            r#"{"created_at":"2026-04-03T17:33:00+08:00"}"#,
        )
        .unwrap();
        let bytes = b"\xef\xbb\xbf1\r\n00:00:00,000 --> 00:00:01,000\r\n[00_spk_01] hello\r\n";
        fs::write(meeting.join("pro_asr.srt"), bytes).unwrap();
        let (_, users, _) = users_at(dir.path()).unwrap();
        let candidate = discover_conversations(dir.path(), &users[0])
            .unwrap()
            .0
            .remove(0);
        let normalized = normalize(dir.path(), &candidate, None).unwrap();
        assert_eq!(normalized.transcript_source_kind, "pro_asr");
        assert_eq!(normalized.transcript_output_file, "transcript.srt");
        assert_eq!(normalized.transcript_format, "srt");
        assert_eq!(normalized.transcript_bytes, bytes);
    }

    #[test]
    fn invalid_pro_is_blocked_and_legacy_srt_is_not_a_fallback() {
        let dir = tempdir().unwrap();
        let meeting = dir.path().join("conversation/202604/20260403_173300");
        fs::create_dir_all(&meeting).unwrap();
        fs::write(
            meeting.join("meta.json"),
            r#"{"created_at":"2026-04-03T17:33:00+08:00"}"#,
        )
        .unwrap();
        fs::write(meeting.join("pro_asr.srt"), "bad").unwrap();
        fs::write(meeting.join("conv.srt"), good_srt("spk_01")).unwrap();
        let (_, users, _) = users_at(dir.path()).unwrap();
        let candidate = discover_conversations(dir.path(), &users[0])
            .unwrap()
            .0
            .remove(0);
        let error = normalize(dir.path(), &candidate, None).unwrap_err();
        assert!(error.reason.contains("pro_asr.srt"));
        assert!(error.reason.contains("no_valid_transcript"));
    }

    #[test]
    fn naive_time_requires_explicit_timezone() {
        let dir = tempdir().unwrap();
        let meeting = dir.path().join("conversation/202604/20260403_173300");
        fs::create_dir_all(&meeting).unwrap();
        fs::write(
            meeting.join("meta.json"),
            r#"{"created_at":"2026-04-03T17:33:00"}"#,
        )
        .unwrap();
        fs::write(meeting.join("pro_asr.srt"), good_srt("spk_01")).unwrap();
        let (_, users, _) = users_at(dir.path()).unwrap();
        let candidate = discover_conversations(dir.path(), &users[0])
            .unwrap()
            .0
            .remove(0);
        assert!(normalize(dir.path(), &candidate, None)
            .unwrap_err()
            .reason
            .contains("needs_timezone"));
        assert!(normalize(dir.path(), &candidate, Some("Asia/Taipei")).is_ok());
    }

    #[test]
    fn id_priority_and_trimmed_legacy_source_follow_the_frozen_contract() {
        let dir = tempdir().unwrap();
        let meeting = dir
            .path()
            .join("conversation/202604/session_custom_identifier");
        fs::create_dir_all(&meeting).unwrap();
        fs::write(
            meeting.join("meta.json"),
            r#"{"session_id":"20260403_173301","created_at":"2026-04-03T17:33:02+08:00","source":"   ","device_source":"  ios  "}"#,
        )
        .unwrap();
        fs::write(meeting.join("pro_asr.srt"), good_srt("spk_01")).unwrap();
        let (_, users, _) = users_at(dir.path()).unwrap();
        let candidate = discover_conversations(dir.path(), &users[0])
            .unwrap()
            .0
            .remove(0);
        let normalized = normalize(dir.path(), &candidate, None).unwrap();
        assert_eq!(normalized.conversation_id, "20260403_173301");
        assert_eq!(normalized.source, "hemory_v1.0:ios");

        let current = dir.path().join("conversation/202604/20260403_173303");
        fs::create_dir_all(&current).unwrap();
        fs::write(
            current.join("meta.json"),
            r#"{"conv_id":"20260403_173304","created_at":"2026-04-03T17:33:03+08:00"}"#,
        )
        .unwrap();
        fs::write(current.join("pro_asr.srt"), good_srt("spk_01")).unwrap();
        let candidate = DiscoveredConversation {
            path: current.canonicalize().unwrap(),
            relative_path: "conversation/202604/20260403_173303".into(),
            schema: "current-conversation-v1".into(),
        };
        let error = normalize(dir.path(), &candidate, None).unwrap_err();
        assert!(error.reason.contains("conflicts"), "{}", error.reason);
    }

    #[cfg(unix)]
    #[test]
    fn transcript_symlink_is_rejected() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let meeting = dir.path().join("conversation/202604/20260403_173300");
        fs::create_dir_all(&meeting).unwrap();
        fs::write(
            meeting.join("meta.json"),
            r#"{"created_at":"2026-04-03T17:33:00+08:00"}"#,
        )
        .unwrap();
        let outside = dir.path().join("outside.srt");
        fs::write(&outside, good_srt("spk_01")).unwrap();
        symlink(&outside, meeting.join("pro_asr.srt")).unwrap();
        let (_, users, _) = users_at(dir.path()).unwrap();
        let candidate = discover_conversations(dir.path(), &users[0])
            .unwrap()
            .0
            .remove(0);
        assert!(normalize(dir.path(), &candidate, None)
            .unwrap_err()
            .reason
            .contains("symlink"));
    }
}
