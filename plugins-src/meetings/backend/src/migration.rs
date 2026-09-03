use crate::hemory::{self, DiscoveredConversation};
use crate::model::{
    MeetingSummary, MigrationAction, MigrationItem, MigrationOptions, MigrationReport,
    NormalizedMeeting, OutputHashes,
};
use crate::settings::{validate_meetings_root, DEFAULT_MEETINGS_ROOT};
use crate::srt::{validate_content_markdown, validate_srt, SpeakerLookup};
use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

const LEDGER_REL: &str = ".notemd/meetings/hemory-import-v1.json";
const LOCK_REL: &str = ".notemd/meetings/hemory-import-v1.lock";
const TRANSACTION_JOURNAL_REL: &str = ".notemd/meetings/hemory-transaction-journal-v1.json";
#[cfg(test)]
const MEETINGS_REL: &str = DEFAULT_MEETINGS_ROOT;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Ledger {
    schema_version: u32,
    #[serde(default)]
    entries: BTreeMap<String, LedgerEntry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct LedgerEntry {
    instance_id: String,
    user_id: String,
    conversation_id: String,
    original_conversation_id: String,
    original_directory: String,
    source_schema: String,
    transcript_source_kind: String,
    transcript_file: String,
    #[serde(default)]
    speaker_mapping: BTreeMap<String, String>,
    source_relative_path: String,
    source_fingerprint: String,
    target_relative_path: String,
    output_hashes: OutputHashes,
    committed_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct SourceBindings {
    schema_version: u32,
    #[serde(default)]
    roots: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TransactionJournal {
    schema_version: u32,
    nonce: String,
    kind: TransactionKind,
    phase: TransactionPhase,
    source_key: String,
    target_name: String,
    backup_name: Option<String>,
    stage_name: String,
    old_hashes: Option<OutputHashes>,
    new_entry: LedgerEntry,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TransactionKind {
    Create,
    Update,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TransactionPhase {
    Active,
    CreateRollbackPrepared,
    CreateNoClobberPrepared,
}

#[derive(Serialize)]
struct MeetingMeta<'a> {
    conversation_id: &'a str,
    created_at: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_at: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    category: &'a Option<Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    key_topics: &'a Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: &'a Option<String>,
    source: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: &'a Option<u64>,
    speaker_count: usize,
    transcript_file: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary_file: Option<&'static str>,
    imported_from: &'static str,
    updated_at: &'a str,
}

struct PlannedMeeting {
    meeting: NormalizedMeeting,
    meta_bytes: Vec<u8>,
}

struct PlanBundle {
    report: MigrationReport,
    meetings: BTreeMap<String, PlannedMeeting>,
    ledger: Ledger,
    ledger_corrupt: bool,
    instance_id: String,
    source_root: PathBuf,
}

pub struct MigrationService {
    vault_root: PathBuf,
    data_dir: PathBuf,
    meetings_root: String,
}

impl MigrationService {
    pub fn new(vault_root: impl Into<PathBuf>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            vault_root: vault_root.into(),
            data_dir: data_dir.into(),
            meetings_root: DEFAULT_MEETINGS_ROOT.to_string(),
        }
    }

    pub fn with_meetings_root(
        vault_root: impl Into<PathBuf>,
        data_dir: impl Into<PathBuf>,
        meetings_root: impl Into<String>,
    ) -> Result<Self, String> {
        let meetings_root = meetings_root.into();
        validate_meetings_root(&meetings_root)?;
        Ok(Self {
            vault_root: vault_root.into(),
            data_dir: data_dir.into(),
            meetings_root,
        })
    }

    pub fn detect(&self, source: &Path) -> Result<crate::model::SourceDetection, String> {
        hemory::detect(source)
    }

    pub fn plan(&self, options: &MigrationOptions) -> Result<MigrationReport, String> {
        Ok(self.plan_inner(options, true, None)?.report)
    }

    pub fn apply<F>(
        &self,
        options: &MigrationOptions,
        expected_plan: Option<&MigrationReport>,
        cancelled: &AtomicBool,
        mut progress: F,
    ) -> Result<MigrationReport, String>
    where
        F: FnMut(usize, usize, &MigrationItem),
    {
        self.validate_vault()?;
        let lock_path = self.vault_root.join(LOCK_REL);
        let lock_parent = lock_path.parent().expect("lock path has parent");
        ensure_no_symlink_from(&self.vault_root, lock_parent)?;
        fs::create_dir_all(lock_parent)
            .map_err(|error| format!("create migration state directory: {error}"))?;
        ensure_no_symlink_from(&self.vault_root, lock_parent)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|error| format!("open migration lock: {error}"))?;
        FileExt::try_lock_exclusive(&lock)
            .map_err(|_| "another Hemory migration is already running".to_string())?;

        self.recover_transaction_journal()?;
        let mut bundle = self.plan_inner(
            options,
            false,
            expected_plan.map(|report| report.planned_at.as_str()),
        )?;
        if bundle.ledger_corrupt {
            return Err(
                "migration ledger is corrupt; apply is disabled until it is audited".into(),
            );
        }
        if let Some(expected) = expected_plan {
            verify_expected_plan(expected, &bundle.report)?;
        }
        let keys: Vec<String> = bundle
            .report
            .items
            .iter()
            .filter(|item| {
                matches!(
                    item.action,
                    MigrationAction::Create | MigrationAction::Update
                )
            })
            .filter_map(|item| {
                bundle
                    .meetings
                    .iter()
                    .find(|(_, planned)| {
                        planned.meeting.conversation_id == item.conversation_id
                            && planned.meeting.source_relative_path == item.source_relative_path
                    })
                    .map(|(key, _)| key.clone())
            })
            .collect();

        let total = keys.len();
        for key in keys {
            if cancelled.load(Ordering::Relaxed) {
                bundle
                    .report
                    .warnings
                    .push("migration cancelled at meeting boundary".into());
                break;
            }
            let planned = bundle.meetings.get(&key).expect("planned key exists");
            let latest = match self.renormalize(&bundle.source_root, &planned.meeting, options) {
                Ok(latest) => latest,
                Err(error) => {
                    self.mark_runtime_conflict(
                        &mut bundle.report,
                        &planned.meeting,
                        &format!("source became invalid after planning: {error}"),
                    );
                    continue;
                }
            };
            if latest.fingerprint != planned.meeting.fingerprint
                || latest.transcript_sha256 != planned.meeting.transcript_sha256
            {
                self.mark_runtime_conflict(
                    &mut bundle.report,
                    &planned.meeting,
                    "source changed after planning",
                );
                continue;
            }
            let action = bundle
                .report
                .items
                .iter()
                .find(|item| {
                    item.conversation_id == planned.meeting.conversation_id
                        && item.source_relative_path == planned.meeting.source_relative_path
                })
                .map(|item| item.action.clone())
                .expect("report item exists");
            let hashes = desired_hashes(&planned.meeting, &planned.meta_bytes);
            let new_entry = ledger_entry_at(
                &bundle.instance_id,
                &bundle.report.source_user,
                &planned.meeting,
                &self.target_relative(&planned.meeting.conversation_id),
                hashes,
            );
            if let Err(error) = self.commit_meeting(
                planned,
                &action,
                bundle.ledger.entries.get(&key),
                &key,
                &new_entry,
            ) {
                if self.update_journal_path().exists() {
                    if let Err(recovery_error) = self.recover_transaction_journal() {
                        return Err(format!(
                            "meeting commit failed: {error}; transaction recovery failed: {recovery_error}"
                        ));
                    }
                    let (recovered_ledger, corrupt) = self.read_ledger(false)?;
                    if corrupt {
                        return Err("transaction recovered but ledger is corrupt".into());
                    }
                    bundle.ledger = recovered_ledger;
                }
                self.mark_runtime_conflict(&mut bundle.report, &planned.meeting, &error);
                continue;
            }
            bundle.ledger.entries.insert(key, new_entry);
            self.write_ledger(&bundle.ledger)?;
            self.recover_transaction_journal()?;
            bundle.report.committed += 1;
            if let Some(item) = bundle.report.items.iter().find(|item| {
                item.conversation_id == planned.meeting.conversation_id
                    && item.source_relative_path == planned.meeting.source_relative_path
            }) {
                progress(bundle.report.committed, total, item);
            }
        }

        bundle.report.recount();
        Ok(bundle.report)
    }

    pub fn list_meetings(&self) -> Result<Vec<MeetingSummary>, String> {
        self.validate_vault()?;
        let root = self.meetings_root_path();
        if !root.exists() {
            return Ok(Vec::new());
        }
        ensure_no_symlink_from(&self.vault_root, &root)?;
        let (ledger, ledger_corrupt) = self.read_ledger(true)?;
        let mut meetings = Vec::new();
        for entry in fs::read_dir(&root).map_err(|error| format!("read meetings: {error}"))? {
            let entry = entry.map_err(|error| format!("read meetings: {error}"))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| format!("inspect {}: {error}", path.display()))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            let Some(id) = entry.file_name().to_str().map(ToOwned::to_owned) else {
                continue;
            };
            if !is_conversation_id(&id) {
                continue;
            }
            let srt = path.join("transcript.srt");
            let markdown = path.join("transcript.md");
            let has_srt = fs::symlink_metadata(&srt)
                .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                .unwrap_or(false);
            let has_markdown = fs::symlink_metadata(&markdown)
                .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                .unwrap_or(false);
            if has_srt == has_markdown {
                continue;
            }
            let meta_path = path.join("meta.yml");
            let meta = if meta_path.exists() {
                match read_yaml(&meta_path) {
                    Ok(value) => Some(value),
                    Err(_) => continue,
                }
            } else {
                None
            };
            let checkpoint = if meta
                .as_ref()
                .and_then(|value| value.get("imported_from"))
                .and_then(Value::as_str)
                == Some("hemory")
            {
                if ledger_corrupt || !meta.as_ref().is_some_and(has_only_flat_meta_fields) {
                    continue;
                }
                let relative = self.target_relative(&id);
                let Some(checkpoint) = ledger
                    .entries
                    .values()
                    .find(|entry| entry.target_relative_path == relative)
                else {
                    continue;
                };
                Some(checkpoint)
            } else {
                None
            };
            let (transcript_name, _bytes, inferred_speakers) = if has_srt {
                let bytes = match read_regular(&srt) {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                };
                let mut lookup = SpeakerLookup::default();
                if let Some(checkpoint) = checkpoint {
                    lookup.canonical = checkpoint.speaker_mapping.clone();
                }
                let validation = match validate_srt(&bytes, &lookup) {
                    Ok(validation) => validation,
                    Err(_) => continue,
                };
                ("transcript.srt", bytes, validation.canonical_labels.len())
            } else {
                let bytes = match read_regular(&markdown) {
                    Ok(bytes) => bytes,
                    Err(_) => continue,
                };
                let validation = match validate_content_markdown(&bytes) {
                    Ok(validation) => validation,
                    Err(_) => continue,
                };
                ("transcript.md", bytes, validation.speakers.len())
            };
            if let Some(meta) = &meta {
                if meta.get("transcript_file").and_then(Value::as_str) != Some(transcript_name) {
                    continue;
                }
                let summary_exists = fs::symlink_metadata(path.join("summary.md"))
                    .map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
                    .unwrap_or(false);
                let declared_summary = meta.get("summary_file").and_then(Value::as_str);
                if declared_summary.is_some_and(|file| file != "summary.md")
                    || summary_exists != (declared_summary == Some("summary.md"))
                {
                    continue;
                }
                if let Some(checkpoint) = checkpoint {
                    let Ok(actual) = actual_target_hashes(&path) else {
                        continue;
                    };
                    if !hashes_equal(&actual, &checkpoint.output_hashes) {
                        continue;
                    }
                }
            }
            let empty = Value::Null;
            let meta = meta.as_ref().unwrap_or(&empty);
            meetings.push(MeetingSummary {
                conversation_id: id.clone(),
                title: meta
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string(),
                started_at: meta
                    .get("created_at")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                duration_ms: meta.get("duration_ms").and_then(Value::as_u64),
                speaker_count: meta
                    .get("speaker_count")
                    .and_then(Value::as_u64)
                    .map_or(inferred_speakers, |count| count as usize),
                source: meta
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or("native")
                    .to_string(),
                target_relative_path: self.target_relative(&id),
                transcript_relative_path: format!(
                    "{}/{transcript_name}",
                    self.target_relative(&id)
                ),
            });
        }
        meetings.sort_by(|left, right| {
            right
                .started_at
                .cmp(&left.started_at)
                .then_with(|| left.conversation_id.cmp(&right.conversation_id))
        });
        Ok(meetings)
    }

    pub fn library_list(&self) -> Result<Vec<MeetingSummary>, String> {
        self.list_meetings()
    }

    fn plan_inner(
        &self,
        options: &MigrationOptions,
        dry_run: bool,
        planned_at_override: Option<&str>,
    ) -> Result<PlanBundle, String> {
        self.validate_vault()?;
        let (source_root, users, mut detection_warnings) = hemory::users_at(&options.source)?;
        let user = match (&options.user, users.as_slice()) {
            (Some(selected), _) => users
                .iter()
                .find(|user| &user.user_id == selected)
                .ok_or_else(|| format!("Hemory user '{selected}' was not found"))?,
            (None, [only]) => only,
            (None, _) => return Err("multiple Hemory users found; --user is required".into()),
        };
        let instance_id = self.instance_id(&source_root, !dry_run)?;
        let (candidates, discover_warnings) = hemory::discover_conversations(&source_root, user)?;
        let discovered_paths: BTreeSet<String> = candidates
            .iter()
            .map(|candidate| candidate.relative_path.clone())
            .collect();
        detection_warnings.extend(discover_warnings);
        let excluded_audio = hemory::count_audio_files(&candidates);
        let (ledger, ledger_corrupt) = self.read_ledger(dry_run)?;
        let imported_at = planned_at_override
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        let mut report = MigrationReport::new(options.mode, dry_run, user.user_id.clone());
        report.planned_at = imported_at.clone();
        report.scanned = candidates.len();
        report.excluded_audio = excluded_audio;
        report.warnings = detection_warnings;
        if ledger_corrupt {
            report
                .errors
                .push("migration ledger is corrupt; existing targets are fail-closed".into());
        }

        let mut normalized = Vec::new();
        for candidate in candidates {
            match hemory::normalize(&source_root, &candidate, options.timezone.as_deref()) {
                Ok(meeting) => normalized.push(meeting),
                Err(failure) => report.items.push(MigrationItem {
                    conversation_id: failure.conversation_id,
                    source_relative_path: failure.source_relative_path,
                    source_schema: failure.source_schema,
                    selected_transcript: None,
                    source_fingerprint: None,
                    target_relative_path: String::new(),
                    action: MigrationAction::Blocked,
                    reason: failure.reason,
                    output_hashes: OutputHashes::default(),
                    warnings: failure.warnings,
                }),
            }
        }
        let duplicate_ids: BTreeSet<String> = normalized
            .iter()
            .fold(BTreeMap::<String, usize>::new(), |mut counts, meeting| {
                *counts.entry(meeting.conversation_id.clone()).or_default() += 1;
                counts
            })
            .into_iter()
            .filter_map(|(id, count)| (count > 1).then_some(id))
            .collect();

        let mut meetings = BTreeMap::new();
        let mut current_keys = BTreeSet::new();
        for meeting in normalized {
            let source_key = source_key(&instance_id, &user.user_id, &meeting.conversation_id);
            current_keys.insert(source_key.clone());
            let meta_bytes = render_meta(&meeting, &imported_at)?;
            let mut output_hashes = desired_hashes(&meeting, &meta_bytes);
            let action_and_reason = if duplicate_ids.contains(&meeting.conversation_id) {
                (
                    MigrationAction::Conflict,
                    "duplicate source conversation ID".into(),
                )
            } else {
                self.classify(&meeting, &source_key, &ledger, ledger_corrupt)?
            };
            if action_and_reason.0 == MigrationAction::Skip {
                output_hashes = actual_target_hashes(&self.target_path(&meeting.conversation_id))?;
            }
            report.items.push(MigrationItem {
                conversation_id: meeting.conversation_id.clone(),
                source_relative_path: meeting.source_relative_path.clone(),
                source_schema: meeting.source_schema.clone(),
                selected_transcript: Some(meeting.transcript_relative_path.clone()),
                source_fingerprint: Some(meeting.fingerprint.clone()),
                target_relative_path: self.target_relative(&meeting.conversation_id),
                action: action_and_reason.0,
                reason: action_and_reason.1,
                output_hashes,
                warnings: meeting.warnings.clone(),
            });
            meetings.insert(
                source_key.clone(),
                PlannedMeeting {
                    meeting,
                    meta_bytes,
                },
            );
        }

        let prefix = format!("hemory:{instance_id}:{}:", user.user_id);
        for (key, entry) in &ledger.entries {
            if key.starts_with(&prefix)
                && !current_keys.contains(key)
                && !discovered_paths.contains(&entry.source_relative_path)
            {
                let id = key.rsplit(':').next().unwrap_or("unknown").to_string();
                report.items.push(MigrationItem {
                    conversation_id: id,
                    source_relative_path: entry.source_relative_path.clone(),
                    source_schema: "ledger".into(),
                    selected_transcript: None,
                    source_fingerprint: Some(entry.source_fingerprint.clone()),
                    target_relative_path: entry.target_relative_path.clone(),
                    action: MigrationAction::SourceMissing,
                    reason: "source conversation is missing; target retained".into(),
                    output_hashes: entry.output_hashes.clone(),
                    warnings: Vec::new(),
                });
            }
        }
        report.items.sort_by(|left, right| {
            left.conversation_id
                .cmp(&right.conversation_id)
                .then_with(|| left.source_relative_path.cmp(&right.source_relative_path))
        });
        report.recount();
        Ok(PlanBundle {
            report,
            meetings,
            ledger,
            ledger_corrupt,
            instance_id,
            source_root,
        })
    }

    fn classify(
        &self,
        meeting: &NormalizedMeeting,
        key: &str,
        ledger: &Ledger,
        ledger_corrupt: bool,
    ) -> Result<(MigrationAction, String), String> {
        let target = self.target_path(&meeting.conversation_id);
        let desired_relative = self.target_relative(&meeting.conversation_id);
        ensure_no_symlink_from(&self.vault_root, &target)?;
        if let Some(previous) = ledger.entries.get(key) {
            if previous.target_relative_path != desired_relative {
                return Ok((
                    MigrationAction::Conflict,
                    format!(
                        "meeting was previously imported at {}; changing meetings_root does not move existing meetings",
                        previous.target_relative_path
                    ),
                ));
            }
        }
        if fs::symlink_metadata(&target)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            return Ok((MigrationAction::Create, "target does not exist".into()));
        }
        if ledger_corrupt {
            return Ok((MigrationAction::Conflict, "ledger is corrupt".into()));
        }
        if !fs::metadata(&target)
            .map(|meta| meta.is_dir())
            .unwrap_or(false)
        {
            return Ok((
                MigrationAction::Conflict,
                "target is not a directory".into(),
            ));
        }
        let Some(previous) = ledger.entries.get(key) else {
            return Ok((
                MigrationAction::Conflict,
                "target exists but no trusted checkpoint exists".into(),
            ));
        };
        if previous.instance_id.is_empty()
            || previous.user_id.is_empty()
            || previous.conversation_id != meeting.conversation_id
            || previous.original_conversation_id != meeting.original_conversation_id
            || previous.target_relative_path != desired_relative
        {
            return Ok((
                MigrationAction::Conflict,
                "checkpoint provenance does not match this source".into(),
            ));
        }
        let actual = match actual_target_hashes(&target) {
            Ok(hashes) => hashes,
            Err(error) => return Ok((MigrationAction::Conflict, error)),
        };
        if !hashes_equal(&actual, &previous.output_hashes) {
            return Ok((
                MigrationAction::Conflict,
                "target was modified after the last import".into(),
            ));
        }
        if previous.source_fingerprint == meeting.fingerprint {
            return Ok((
                MigrationAction::Skip,
                "source and target are unchanged".into(),
            ));
        }
        Ok((
            MigrationAction::Update,
            "source changed and target still matches the checkpoint".into(),
        ))
    }

    fn commit_meeting(
        &self,
        planned: &PlannedMeeting,
        action: &MigrationAction,
        previous: Option<&LedgerEntry>,
        source_key: &str,
        new_entry: &LedgerEntry,
    ) -> Result<(), String> {
        let meetings_root = self.meetings_root_path();
        ensure_no_symlink_from(&self.vault_root, &meetings_root)?;
        fs::create_dir_all(&meetings_root)
            .map_err(|error| format!("create meetings directory: {error}"))?;
        ensure_no_symlink_from(&self.vault_root, &meetings_root)?;
        let target = self.target_path(&planned.meeting.conversation_id);
        let nonce = Uuid::new_v4().simple().to_string();
        let stage_name = format!(".txn-{nonce}-stage");
        let stage = meetings_root.join(&stage_name);
        fs::create_dir(&stage).map_err(|error| format!("create staging directory: {error}"))?;
        let result = (|| {
            match (
                planned.meeting.transcript_output_file.as_str(),
                planned.meeting.transcript_format.as_str(),
            ) {
                ("transcript.md", "markdown") | ("transcript.srt", "srt") => {}
                _ => return Err("unsupported transcript output file/format pair".into()),
            }
            let staged_transcript_path = stage.join(&planned.meeting.transcript_output_file);
            write_synced(&staged_transcript_path, &planned.meeting.transcript_bytes)?;
            let staged_transcript = read_regular(&staged_transcript_path)?;
            match planned.meeting.transcript_format.as_str() {
                "markdown" => {
                    validate_content_markdown(&staged_transcript).map_err(|error| {
                        format!("staged Markdown transcript validation failed: {error}")
                    })?;
                }
                "srt" => {
                    let mut lookup = SpeakerLookup::default();
                    for (canonical, speaker) in &planned.meeting.speakers {
                        for label in &speaker.source_labels {
                            lookup.canonical.insert(label.clone(), canonical.clone());
                        }
                    }
                    validate_srt(&staged_transcript, &lookup).map_err(|error| {
                        format!("staged SRT transcript validation failed: {error}")
                    })?;
                }
                format => return Err(format!("unsupported transcript format '{format}'")),
            }
            if let Some(summary) = &planned.meeting.summary_bytes {
                write_synced(&stage.join("summary.md"), summary)?;
                if hemory::sha256(&read_regular(&stage.join("summary.md"))?)
                    != planned.meeting.summary_sha256.clone().unwrap_or_default()
                {
                    return Err("staged summary hash mismatch".into());
                }
            }
            write_synced(&stage.join("meta.yml"), &planned.meta_bytes)?;
            sync_dir(&stage)?;

            match action {
                MigrationAction::Create => {
                    self.write_transaction_journal_new(&TransactionJournal {
                        schema_version: 1,
                        nonce: nonce.clone(),
                        kind: TransactionKind::Create,
                        phase: TransactionPhase::Active,
                        source_key: source_key.to_string(),
                        target_name: planned.meeting.conversation_id.clone(),
                        backup_name: None,
                        stage_name: stage_name.clone(),
                        old_hashes: None,
                        new_entry: new_entry.clone(),
                    })?;
                    rename_directory_noreplace(&stage, &target).map_err(|error| {
                        format!("activate new meeting without clobber: {error}")
                    })?;
                    sync_dir(&meetings_root)?;
                    inject_update_crash(3)?;
                }
                MigrationAction::Update => {
                    let previous = previous.ok_or("missing checkpoint for update")?;
                    if actual_target_hashes(&target)
                        .map(|actual| !hashes_equal(&actual, &previous.output_hashes))
                        .unwrap_or(true)
                    {
                        return Err("target changed after planning".into());
                    }
                    let backup_name =
                        format!(".txn-{nonce}-backup-{}", planned.meeting.conversation_id);
                    let backup = meetings_root.join(&backup_name);
                    let journal = TransactionJournal {
                        schema_version: 1,
                        nonce: nonce.clone(),
                        kind: TransactionKind::Update,
                        phase: TransactionPhase::Active,
                        source_key: source_key.to_string(),
                        target_name: planned.meeting.conversation_id.clone(),
                        backup_name: Some(backup_name),
                        stage_name: stage_name.clone(),
                        old_hashes: Some(previous.output_hashes.clone()),
                        new_entry: new_entry.clone(),
                    };
                    self.write_transaction_journal_new(&journal)?;
                    fs::rename(&target, &backup)
                        .map_err(|error| format!("stage existing meeting: {error}"))?;
                    sync_dir(&meetings_root)?;
                    inject_update_crash(1)?;
                    fs::rename(&stage, &target)
                        .map_err(|error| format!("activate updated meeting: {error}"))?;
                    sync_dir(&meetings_root)?;
                    inject_update_crash(2)?;
                }
                _ => return Err("internal error: non-writable migration action".into()),
            }
            sync_dir(&meetings_root)
        })();
        if result.is_err() && stage.exists() {
            let journal_path = self.update_journal_path();
            let may_remove_own_stage = if !journal_path.exists() {
                true
            } else {
                fs::read(&journal_path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<TransactionJournal>(&bytes).ok())
                    .is_some_and(|journal| {
                        journal.nonce != nonce || journal.stage_name != stage_name
                    })
            };
            if may_remove_own_stage {
                let _ = fs::remove_dir_all(&stage);
            }
        }
        result
    }

    fn renormalize(
        &self,
        root: &Path,
        meeting: &NormalizedMeeting,
        options: &MigrationOptions,
    ) -> Result<NormalizedMeeting, String> {
        let candidate = DiscoveredConversation {
            path: meeting.source_abs.clone(),
            relative_path: meeting.source_relative_path.clone(),
            schema: meeting.source_schema.clone(),
        };
        hemory::normalize(root, &candidate, options.timezone.as_deref())
            .map_err(|failure| failure.reason)
    }

    fn mark_runtime_conflict(
        &self,
        report: &mut MigrationReport,
        meeting: &NormalizedMeeting,
        reason: &str,
    ) {
        if let Some(item) = report.items.iter_mut().find(|item| {
            item.conversation_id == meeting.conversation_id
                && item.source_relative_path == meeting.source_relative_path
        }) {
            item.action = MigrationAction::Conflict;
            item.reason = reason.to_string();
        }
        report
            .errors
            .push(format!("{}: {reason}", meeting.source_relative_path));
        report.recount();
    }

    fn update_journal_path(&self) -> PathBuf {
        self.vault_root.join(TRANSACTION_JOURNAL_REL)
    }

    fn write_transaction_journal_new(&self, journal: &TransactionJournal) -> Result<(), String> {
        validate_transaction_journal(journal)?;
        let path = self.update_journal_path();
        let parent = path.parent().expect("journal has parent");
        let temporary = parent.join(journal_temp_name(&journal.nonce));
        ensure_no_symlink_from(&self.vault_root, &path)?;
        ensure_no_symlink_from(&self.vault_root, &temporary)?;
        let bytes = serde_json::to_vec_pretty(journal)
            .map_err(|error| format!("serialize transaction journal: {error}"))?;
        let mut owns_temporary = false;
        let result = (|| {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(|error| format!("create temporary transaction journal: {error}"))?;
            owns_temporary = true;
            if inject_update_crash(4).is_err() {
                file.write_all(&bytes[..bytes.len() / 2])
                    .map_err(|error| format!("write partial transaction journal: {error}"))?;
                file.sync_all()
                    .map_err(|error| format!("sync partial transaction journal: {error}"))?;
                return Err("injected transaction journal partial-write crash".into());
            }
            file.write_all(&bytes)
                .map_err(|error| format!("write temporary transaction journal: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("sync temporary transaction journal: {error}"))?;
            drop(file);
            inject_update_crash(5)?;
            rename_directory_noreplace(&temporary, &path).map_err(|error| {
                format!("publish transaction journal without overwrite: {error}")
            })?;
            owns_temporary = false;
            inject_update_crash(6)?;
            sync_dir(parent)
        })();
        if let Err(error) = result {
            if owns_temporary {
                if let Err(cleanup_error) = cleanup_owned_journal_temp(&temporary, journal) {
                    return Err(format!(
                        "{error}; temporary journal preserved: {cleanup_error}"
                    ));
                }
            }
            return Err(error);
        }
        Ok(())
    }

    fn clear_transaction_journal(&self) -> Result<(), String> {
        let path = self.update_journal_path();
        ensure_no_symlink_from(&self.vault_root, &path)?;
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|error| format!("remove transaction journal: {error}"))?;
            sync_dir(path.parent().expect("journal has parent"))?;
        }
        Ok(())
    }

    fn replace_transaction_journal(
        &self,
        current: &TransactionJournal,
        replacement: &TransactionJournal,
    ) -> Result<(), String> {
        validate_transaction_journal(current)?;
        validate_transaction_journal(replacement)?;
        if !matches!(
            (
                current.kind,
                current.phase,
                replacement.kind,
                replacement.phase,
            ),
            (
                TransactionKind::Create,
                TransactionPhase::Active,
                TransactionKind::Create,
                TransactionPhase::CreateRollbackPrepared
                    | TransactionPhase::CreateNoClobberPrepared,
            )
        ) {
            return Err("transaction journal phase transition is invalid".into());
        }
        let path = self.update_journal_path();
        let parent = path.parent().expect("journal has parent");
        // Phase transitions can be retried for the same transaction nonce.
        // Use a fresh publish nonce so a partial temp from an interrupted
        // attempt cannot make every later recovery fail with AlreadyExists.
        let temporary = parent.join(journal_phase_temp_name(
            &replacement.nonce,
            &Uuid::new_v4().simple().to_string(),
        ));
        ensure_no_symlink_from(&self.vault_root, &path)?;
        ensure_no_symlink_from(&self.vault_root, &temporary)?;
        let current_bytes = serde_json::to_vec_pretty(current)
            .map_err(|error| format!("serialize current transaction journal: {error}"))?;
        if read_regular(&path)? != current_bytes {
            return Err("transaction journal changed before phase transition".into());
        }
        let replacement_bytes = serde_json::to_vec_pretty(replacement)
            .map_err(|error| format!("serialize replacement transaction journal: {error}"))?;
        let mut expected_replacement = current.clone();
        expected_replacement.phase = replacement.phase;
        if replacement_bytes
            != serde_json::to_vec_pretty(&expected_replacement)
                .map_err(|error| format!("serialize expected journal phase: {error}"))?
        {
            return Err("transaction journal phase transition changes protected data".into());
        }
        if let Err(error) = write_synced(&temporary, &replacement_bytes) {
            return Err(format!("write transaction journal phase: {error}"));
        }
        let result = (|| {
            if read_regular(&path)? != current_bytes {
                return Err("transaction journal changed during phase transition".into());
            }
            fs::rename(&temporary, &path)
                .map_err(|error| format!("publish transaction journal phase: {error}"))?;
            sync_dir(parent)
        })();
        if let Err(error) = result {
            if temporary.exists() {
                if let Err(cleanup_error) = cleanup_owned_journal_temp(&temporary, replacement) {
                    return Err(format!(
                        "{error}; replacement journal preserved: {cleanup_error}"
                    ));
                }
            }
            return Err(error);
        }
        Ok(())
    }

    fn recover_transaction_journal(&self) -> Result<(), String> {
        self.cleanup_unpublished_journal_temps()?;
        let journal_path = self.update_journal_path();
        if !journal_path.exists() {
            return Ok(());
        }
        ensure_no_symlink_from(&self.vault_root, &journal_path)?;
        let journal: TransactionJournal = serde_json::from_slice(&read_regular(&journal_path)?)
            .map_err(|error| format!("transaction journal is corrupt: {error}"))?;
        validate_transaction_journal(&journal)?;
        let root_relative = journal_target_root(&journal)?;
        let root = self.vault_root.join(root_relative);
        ensure_no_symlink_from(&self.vault_root, &root)?;
        let target = root.join(&journal.target_name);
        let stage = root.join(&journal.stage_name);
        for path in [&target, &stage] {
            ensure_no_symlink_from(&self.vault_root, path)?;
        }
        match (journal.kind, journal.phase) {
            (TransactionKind::Create, TransactionPhase::Active) => {
                match (target.exists(), stage.exists()) {
                    (true, false) => {
                        require_hashes(
                            &target,
                            &journal.new_entry.output_hashes,
                            "created target",
                        )?;
                        self.checkpoint_recovered_transaction(&journal)?;
                    }
                    (false, true) => {
                        require_hashes(
                            &stage,
                            &journal.new_entry.output_hashes,
                            "create staging directory",
                        )?;
                        let mut prepared = journal.clone();
                        prepared.phase = TransactionPhase::CreateRollbackPrepared;
                        self.replace_transaction_journal(&journal, &prepared)?;
                        require_hashes(
                            &stage,
                            &journal.new_entry.output_hashes,
                            "prepared create staging directory",
                        )?;
                        fs::remove_dir_all(&stage)
                            .map_err(|error| format!("remove abandoned create staging: {error}"))?;
                        sync_dir(&root)?;
                        inject_update_crash(7)?;
                    }
                    (true, true) => {
                        // Atomic no-clobber failed: the target belongs to
                        // somebody else. Validate only our nonce-bound stage,
                        // then remove it without touching the target.
                        require_hashes(
                            &stage,
                            &journal.new_entry.output_hashes,
                            "create staging directory",
                        )?;
                        let mut prepared = journal.clone();
                        prepared.phase = TransactionPhase::CreateNoClobberPrepared;
                        self.replace_transaction_journal(&journal, &prepared)?;
                        require_hashes(
                            &stage,
                            &journal.new_entry.output_hashes,
                            "prepared create staging directory",
                        )?;
                        fs::remove_dir_all(&stage)
                            .map_err(|error| format!("remove refused create staging: {error}"))?;
                        sync_dir(&root)?;
                        inject_update_crash(8)?;
                    }
                    (false, false) => {
                        return Err(
                            "active create journal has neither target nor staging directory".into(),
                        )
                    }
                }
            }
            (
                TransactionKind::Create,
                TransactionPhase::CreateRollbackPrepared
                | TransactionPhase::CreateNoClobberPrepared,
            ) => {
                if stage.exists() {
                    require_hashes(
                        &stage,
                        &journal.new_entry.output_hashes,
                        "prepared create staging directory",
                    )?;
                    fs::remove_dir_all(&stage)
                        .map_err(|error| format!("remove prepared create staging: {error}"))?;
                    sync_dir(&root)?;
                }
            }
            (TransactionKind::Update, TransactionPhase::Active) => {
                let backup_name = journal.backup_name.as_ref().expect("validated backup");
                let backup = root.join(backup_name);
                ensure_no_symlink_from(&self.vault_root, &backup)?;
                let old = journal.old_hashes.as_ref().expect("validated old hashes");
                match (target.exists(), backup.exists()) {
                    (false, true) => {
                        require_hashes(&backup, old, "update backup")?;
                        if stage.exists() {
                            require_hashes(
                                &stage,
                                &journal.new_entry.output_hashes,
                                "update staging directory",
                            )?;
                        }
                        fs::rename(&backup, &target).map_err(|error| {
                            format!("restore interrupted update backup: {error}")
                        })?;
                    }
                    (true, true) => {
                        require_hashes(
                            &target,
                            &journal.new_entry.output_hashes,
                            "updated target",
                        )?;
                        require_hashes(&backup, old, "update backup")?;
                        if stage.exists() {
                            return Err(
                                "activated update unexpectedly still has staging data".into()
                            );
                        }
                        self.checkpoint_recovered_transaction(&journal)?;
                        fs::remove_dir_all(&backup)
                            .map_err(|error| format!("remove recovered update backup: {error}"))?;
                    }
                    (true, false) => {
                        let actual = actual_target_hashes(&target)?;
                        if hashes_equal(&actual, &journal.new_entry.output_hashes) {
                            if stage.exists() {
                                return Err(
                                    "activated update unexpectedly still has staging data".into()
                                );
                            }
                            self.checkpoint_recovered_transaction(&journal)?;
                        } else if hashes_equal(&actual, old) {
                            if stage.exists() {
                                require_hashes(
                                    &stage,
                                    &journal.new_entry.output_hashes,
                                    "update staging directory",
                                )?;
                            }
                        } else {
                            return Err(
                                "update target matches neither old nor new journal bytes".into()
                            );
                        }
                    }
                    (false, false) => {
                        return Err("update journal has neither target nor backup".into())
                    }
                }
                if stage.exists() {
                    fs::remove_dir_all(&stage)
                        .map_err(|error| format!("remove recovered update staging: {error}"))?;
                }
            }
            (TransactionKind::Update, _) => {
                return Err("update journal has an invalid create-only phase".into())
            }
        }
        sync_dir(&root)?;
        self.clear_transaction_journal()
    }

    fn cleanup_unpublished_journal_temps(&self) -> Result<(), String> {
        let journal_path = self.update_journal_path();
        let parent = journal_path.parent().expect("journal has parent");
        if !parent.exists() {
            return Ok(());
        }
        ensure_no_symlink_from(&self.vault_root, parent)?;
        let mut temporary_paths = Vec::new();
        for entry in fs::read_dir(parent)
            .map_err(|error| format!("read transaction state directory: {error}"))?
        {
            let entry = entry.map_err(|error| format!("read transaction state entry: {error}"))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if is_journal_temp_name(&name) {
                temporary_paths.push(entry.path());
            }
        }
        for temporary in temporary_paths {
            // A temporary journal was never authoritative: a crash may leave
            // it empty or truncated before the atomic publish. Remove only a
            // complete, self-consistent temp file. Unknown bytes are preserved
            // for audit but must not permanently block the formal journal.
            let _ = cleanup_verified_journal_temp(&temporary);
        }
        Ok(())
    }

    fn checkpoint_recovered_transaction(&self, journal: &TransactionJournal) -> Result<(), String> {
        let (mut ledger, corrupt) = self.read_ledger(false)?;
        if corrupt {
            return Err("cannot recover transaction while ledger is corrupt".into());
        }
        ledger
            .entries
            .insert(journal.source_key.clone(), journal.new_entry.clone());
        self.write_ledger(&ledger)
    }

    fn validate_vault(&self) -> Result<(), String> {
        let metadata = fs::symlink_metadata(&self.vault_root).map_err(|error| {
            format!(
                "vault {} is unavailable: {error}",
                self.vault_root.display()
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err("vault root must be a real directory, not a symlink".into());
        }
        Ok(())
    }

    fn target_path(&self, id: &str) -> PathBuf {
        self.meetings_root_path().join(id)
    }

    fn meetings_root_path(&self) -> PathBuf {
        self.vault_root.join(&self.meetings_root)
    }

    fn target_relative(&self, id: &str) -> String {
        format!("{}/{id}", self.meetings_root.trim_end_matches('/'))
    }

    fn read_ledger(&self, dry_run: bool) -> Result<(Ledger, bool), String> {
        let path = self.vault_root.join(LEDGER_REL);
        if !path.exists() {
            return Ok((
                Ledger {
                    schema_version: 1,
                    entries: BTreeMap::new(),
                },
                false,
            ));
        }
        ensure_no_symlink_from(&self.vault_root, &path)?;
        let bytes = read_regular(&path)?;
        match serde_json::from_slice::<Ledger>(&bytes) {
            Ok(ledger) if ledger.schema_version == 1 => Ok((ledger, false)),
            Ok(_) | Err(_) if dry_run => Ok((Ledger::default(), true)),
            Ok(_) | Err(_) => Ok((Ledger::default(), true)),
        }
    }

    fn write_ledger(&self, ledger: &Ledger) -> Result<(), String> {
        let path = self.vault_root.join(LEDGER_REL);
        let parent = path.parent().expect("ledger path has parent");
        ensure_no_symlink_from(&self.vault_root, parent)?;
        fs::create_dir_all(parent).map_err(|error| format!("create ledger directory: {error}"))?;
        ensure_no_symlink_from(&self.vault_root, parent)?;
        let bytes = serde_json::to_vec_pretty(ledger).map_err(|error| error.to_string())?;
        atomic_write(&path, &bytes)
    }

    fn bindings_path(&self) -> PathBuf {
        self.data_dir.join("hemory-source-instances-v1.json")
    }

    fn instance_id(&self, source_root: &Path, persist: bool) -> Result<String, String> {
        let root = source_root.to_string_lossy().into_owned();
        let path = self.bindings_path();
        let mut bindings = if path.exists() {
            ensure_no_symlink_leaf(&path)?;
            serde_json::from_slice::<SourceBindings>(&read_regular(&path)?)
                .map_err(|error| format!("source instance registry is corrupt: {error}"))?
        } else {
            SourceBindings {
                schema_version: 1,
                roots: BTreeMap::new(),
            }
        };
        if let Some(id) = bindings.roots.get(&root) {
            return Ok(id.clone());
        }
        if !persist {
            return Ok("pending-unbound-source".into());
        }
        fs::create_dir_all(&self.data_dir)
            .map_err(|error| format!("create plugin data directory: {error}"))?;
        let instance_id = format!("src-{}", Uuid::new_v4().simple());
        bindings.schema_version = 1;
        bindings.roots.insert(root, instance_id.clone());
        let bytes = serde_json::to_vec_pretty(&bindings).map_err(|error| error.to_string())?;
        atomic_write(&path, &bytes)?;
        Ok(instance_id)
    }
}

fn render_meta(meeting: &NormalizedMeeting, imported_at: &str) -> Result<Vec<u8>, String> {
    let updated_at = meeting.source_updated_at.as_deref().unwrap_or(imported_at);
    let meta = MeetingMeta {
        conversation_id: &meeting.conversation_id,
        created_at: &meeting.started_at,
        end_at: &meeting.ended_at,
        title: &meeting.title,
        category: &meeting.category,
        key_topics: &meeting.topics,
        language: &meeting.language,
        source: &meeting.source,
        duration_ms: &meeting.duration_ms,
        speaker_count: meeting.speakers.len(),
        transcript_file: &meeting.transcript_output_file,
        summary_file: meeting.summary_bytes.as_ref().map(|_| "summary.md"),
        imported_from: "hemory",
        updated_at,
    };
    serde_yaml::to_string(&meta)
        .map(|text| text.into_bytes())
        .map_err(|error| format!("serialize meeting metadata: {error}"))
}

#[cfg(test)]
fn ledger_entry(
    instance_id: &str,
    user_id: &str,
    meeting: &NormalizedMeeting,
    output_hashes: OutputHashes,
) -> LedgerEntry {
    ledger_entry_at(
        instance_id,
        user_id,
        meeting,
        &format!("{DEFAULT_MEETINGS_ROOT}/{}", meeting.conversation_id),
        output_hashes,
    )
}

fn ledger_entry_at(
    instance_id: &str,
    user_id: &str,
    meeting: &NormalizedMeeting,
    target_relative_path: &str,
    output_hashes: OutputHashes,
) -> LedgerEntry {
    LedgerEntry {
        instance_id: instance_id.to_string(),
        user_id: user_id.to_string(),
        conversation_id: meeting.conversation_id.clone(),
        original_conversation_id: meeting.original_conversation_id.clone(),
        original_directory: meeting.original_directory.clone(),
        source_schema: meeting.source_schema.clone(),
        transcript_source_kind: meeting.transcript_source_kind.clone(),
        transcript_file: meeting.transcript_output_file.clone(),
        speaker_mapping: meeting
            .speakers
            .iter()
            .flat_map(|(canonical, speaker)| {
                speaker
                    .source_labels
                    .iter()
                    .map(move |label| (label.clone(), canonical.clone()))
            })
            .collect(),
        source_relative_path: meeting.source_relative_path.clone(),
        source_fingerprint: meeting.fingerprint.clone(),
        target_relative_path: target_relative_path.to_string(),
        output_hashes,
        committed_at: Utc::now().to_rfc3339(),
    }
}

fn has_only_flat_meta_fields(meta: &Value) -> bool {
    const ALLOWED: &[&str] = &[
        "conversation_id",
        "created_at",
        "end_at",
        "title",
        "category",
        "key_topics",
        "language",
        "source",
        "duration_ms",
        "speaker_count",
        "transcript_file",
        "summary_file",
        "imported_from",
        "updated_at",
    ];
    meta.as_object()
        .map(|map| map.keys().all(|key| ALLOWED.contains(&key.as_str())))
        .unwrap_or(false)
}

fn desired_hashes(meeting: &NormalizedMeeting, meta_bytes: &[u8]) -> OutputHashes {
    OutputHashes {
        transcript: Some(meeting.transcript_sha256.clone()),
        summary: meeting.summary_sha256.clone(),
        meta: Some(hemory::sha256(meta_bytes)),
    }
}

fn actual_target_hashes(target: &Path) -> Result<OutputHashes, String> {
    let meta_bytes = read_regular(&target.join("meta.yml"))?;
    let meta: Value = serde_yaml::from_slice(&meta_bytes)
        .map_err(|error| format!("parse {}/meta.yml: {error}", target.display()))?;
    if !has_only_flat_meta_fields(&meta) {
        return Err("target metadata contains unsupported fields".into());
    }
    let transcript_file = meta
        .get("transcript_file")
        .and_then(Value::as_str)
        .ok_or("target metadata has no transcript file")?;
    if !matches!(transcript_file, "transcript.md" | "transcript.srt") {
        return Err("target metadata names an unsupported transcript file".into());
    }
    let summary_file = meta.get("summary_file").and_then(Value::as_str);
    if summary_file.is_some_and(|file| file != "summary.md") {
        return Err("target metadata names an unsupported summary file".into());
    }
    let mut allowed = BTreeSet::from(["meta.yml", transcript_file]);
    if summary_file.is_some() {
        allowed.insert("summary.md");
    }
    let mut actual_names = BTreeSet::new();
    for entry in fs::read_dir(target)
        .map_err(|error| format!("read target directory {}: {error}", target.display()))?
    {
        let entry = entry.map_err(|error| format!("read target directory: {error}"))?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or("target contains a non-UTF-8 entry name")?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| format!("inspect target entry {name}: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!("target entry is not a regular file: {name}"));
        }
        if !allowed.contains(name) {
            return Err(format!("target contains an unrecognized entry: {name}"));
        }
        actual_names.insert(name.to_string());
    }
    if actual_names.len() != allowed.len()
        || !allowed.iter().all(|name| actual_names.contains(*name))
    {
        return Err("target files do not match meta.yml declarations".into());
    }
    let transcript = read_regular(&target.join(transcript_file))?;
    let summary = summary_file
        .map(|file| read_regular(&target.join(file)).map(|bytes| hemory::sha256(&bytes)))
        .transpose()?;
    Ok(OutputHashes {
        transcript: Some(hemory::sha256(&transcript)),
        summary,
        meta: Some(hemory::sha256(&meta_bytes)),
    })
}

fn hashes_equal(left: &OutputHashes, right: &OutputHashes) -> bool {
    left.transcript == right.transcript && left.summary == right.summary && left.meta == right.meta
}

fn source_key(instance_id: &str, user_id: &str, conversation_id: &str) -> String {
    format!("hemory:{instance_id}:{user_id}:{conversation_id}")
}

fn verify_expected_plan(
    expected: &MigrationReport,
    actual: &MigrationReport,
) -> Result<(), String> {
    if expected.mode != actual.mode
        || expected.source_user != actual.source_user
        || expected.planned_at != actual.planned_at
    {
        return Err("migration options changed after preflight; run plan again".into());
    }
    let signature = |report: &MigrationReport| {
        report
            .items
            .iter()
            .map(|item| {
                (
                    item.conversation_id.clone(),
                    item.source_relative_path.clone(),
                    item.source_fingerprint.clone(),
                    item.target_relative_path.clone(),
                    item.action.clone(),
                    item.output_hashes.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    if signature(expected) != signature(actual) {
        return Err("source or target changed after preflight; run plan again".into());
    }
    Ok(())
}

fn is_conversation_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 15
        && bytes.get(8) == Some(&b'_')
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && (bytes.len() == 15
            || (bytes.get(15) == Some(&b'_')
                && bytes[16..].iter().all(u8::is_ascii_digit)
                && bytes.len() > 16))
}

fn ensure_no_symlink_from(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("path escapes vault: {}", path.display()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(format!("unsafe vault path: {}", path.display()));
        };
        current.push(part);
        if !current.exists() {
            continue;
        }
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("inspect {}: {error}", current.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "target symlink is not allowed: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn ensure_no_symlink_leaf(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("symlink is not allowed: {}", path.display()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("inspect {}: {error}", path.display())),
    }
}

fn validate_transaction_journal(journal: &TransactionJournal) -> Result<(), String> {
    let nonce =
        Uuid::parse_str(&journal.nonce).map_err(|_| "transaction journal nonce is not a UUID")?;
    let nonce = nonce.simple().to_string();
    let expected_stage = format!(".txn-{nonce}-stage");
    if journal.schema_version != 1
        || journal.nonce != nonce
        || !is_conversation_id(&journal.target_name)
        || journal.stage_name != expected_stage
        || journal.new_entry.conversation_id != journal.target_name
        || journal_target_root(journal).is_err()
        || journal.new_entry.transcript_file != "transcript.md"
            && journal.new_entry.transcript_file != "transcript.srt"
        || journal.source_key
            != source_key(
                &journal.new_entry.instance_id,
                &journal.new_entry.user_id,
                &journal.target_name,
            )
        || journal.new_entry.output_hashes.transcript.is_none()
        || journal.new_entry.output_hashes.meta.is_none()
    {
        return Err("transaction journal identity or hashes are invalid".into());
    }
    match (journal.kind, journal.phase) {
        (
            TransactionKind::Create,
            TransactionPhase::Active
            | TransactionPhase::CreateRollbackPrepared
            | TransactionPhase::CreateNoClobberPrepared,
        ) if journal.backup_name.is_none() && journal.old_hashes.is_none() => {}
        (TransactionKind::Update, TransactionPhase::Active) => {
            let expected_backup = format!(".txn-{nonce}-backup-{}", journal.target_name);
            if journal.backup_name.as_deref() != Some(expected_backup.as_str())
                || journal.old_hashes.is_none()
            {
                return Err("update journal backup is not bound to its nonce and target".into());
            }
        }
        _ => return Err("transaction journal kind does not match its old/backup state".into()),
    }
    Ok(())
}

fn journal_target_root(journal: &TransactionJournal) -> Result<&Path, String> {
    let relative = Path::new(&journal.new_entry.target_relative_path);
    let Some(file_name) = relative.file_name().and_then(|name| name.to_str()) else {
        return Err("transaction journal target path is invalid".into());
    };
    let Some(parent) = relative.parent() else {
        return Err("transaction journal target path has no meetings root".into());
    };
    let parent = parent
        .to_str()
        .ok_or("transaction journal target path is not UTF-8")?;
    validate_meetings_root(parent)?;
    if file_name != journal.target_name {
        return Err("transaction journal target path does not match its identity".into());
    }
    Ok(Path::new(parent))
}

fn journal_temp_name(nonce: &str) -> String {
    format!(".hemory-transaction-journal-v1.{nonce}.tmp")
}

fn journal_phase_temp_name(transaction_nonce: &str, publish_nonce: &str) -> String {
    format!(".hemory-transaction-journal-v1.{transaction_nonce}.{publish_nonce}.tmp")
}

fn journal_temp_transaction_nonce(name: &str) -> Option<&str> {
    let body = name
        .strip_prefix(".hemory-transaction-journal-v1.")?
        .strip_suffix(".tmp")?;
    let mut parts = body.split('.');
    let transaction_nonce = parts.next()?;
    let publish_nonce = parts.next();
    if parts.next().is_some()
        || !Uuid::parse_str(transaction_nonce)
            .map(|uuid| uuid.simple().to_string() == transaction_nonce)
            .unwrap_or(false)
        || publish_nonce.is_some_and(|nonce| {
            !Uuid::parse_str(nonce)
                .map(|uuid| uuid.simple().to_string() == nonce)
                .unwrap_or(false)
        })
    {
        return None;
    }
    Some(transaction_nonce)
}

fn is_journal_temp_name(name: &str) -> bool {
    journal_temp_transaction_nonce(name).is_some()
}

fn read_verified_journal_temp(path: &Path) -> Result<(TransactionJournal, Vec<u8>), String> {
    ensure_no_symlink_leaf(path)?;
    let bytes = read_regular(path)?;
    let journal: TransactionJournal = serde_json::from_slice(&bytes)
        .map_err(|error| format!("temporary transaction journal is corrupt: {error}"))?;
    validate_transaction_journal(&journal)?;
    let actual_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("temporary transaction journal has an invalid filename")?;
    if journal_temp_transaction_nonce(actual_name) != Some(journal.nonce.as_str()) {
        return Err("temporary transaction journal is not bound to its filename nonce".into());
    }
    Ok((journal, bytes))
}

fn cleanup_verified_journal_temp(path: &Path) -> Result<(), String> {
    read_verified_journal_temp(path)?;
    fs::remove_file(path)
        .map_err(|error| format!("remove unpublished transaction journal: {error}"))?;
    sync_dir(path.parent().expect("temporary journal has parent"))
}

fn cleanup_owned_journal_temp(path: &Path, expected: &TransactionJournal) -> Result<(), String> {
    let (actual, bytes) = read_verified_journal_temp(path)?;
    let expected_bytes = serde_json::to_vec_pretty(expected)
        .map_err(|error| format!("serialize expected transaction journal: {error}"))?;
    if actual.nonce != expected.nonce || bytes != expected_bytes {
        return Err("temporary transaction journal no longer matches the owned bytes".into());
    }
    fs::remove_file(path)
        .map_err(|error| format!("remove owned temporary transaction journal: {error}"))?;
    sync_dir(path.parent().expect("temporary journal has parent"))
}

fn require_hashes(target: &Path, expected: &OutputHashes, label: &str) -> Result<(), String> {
    let actual = actual_target_hashes(target)?;
    if hashes_equal(&actual, expected) {
        Ok(())
    } else {
        Err(format!("{label} does not match its journal hashes"))
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rename_directory_noreplace(from: &Path, to: &Path) -> Result<(), String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let from =
        CString::new(from.as_os_str().as_bytes()).map_err(|_| "source path contains a NUL byte")?;
    let to =
        CString::new(to.as_os_str().as_bytes()).map_err(|_| "target path contains a NUL byte")?;
    #[cfg(target_os = "macos")]
    let result = unsafe {
        extern "C" {
            fn renamex_np(
                old: *const std::os::raw::c_char,
                new: *const std::os::raw::c_char,
                flags: u32,
            ) -> i32;
        }
        const RENAME_EXCL: u32 = 0x0000_0004;
        renamex_np(from.as_ptr(), to.as_ptr(), RENAME_EXCL)
    };
    #[cfg(target_os = "linux")]
    let result = unsafe {
        extern "C" {
            fn renameat2(
                olddirfd: i32,
                oldpath: *const std::os::raw::c_char,
                newdirfd: i32,
                newpath: *const std::os::raw::c_char,
                flags: u32,
            ) -> i32;
        }
        const AT_FDCWD: i32 = -100;
        const RENAME_NOREPLACE: u32 = 1;
        renameat2(
            AT_FDCWD,
            from.as_ptr(),
            AT_FDCWD,
            to.as_ptr(),
            RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn rename_directory_noreplace(_from: &Path, _to: &Path) -> Result<(), String> {
    Err("atomic no-clobber directory publish is unsupported on this platform".into())
}

#[cfg(test)]
thread_local! {
    static UPDATE_CRASH_POINT: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn inject_update_crash(point: u8) -> Result<(), String> {
    UPDATE_CRASH_POINT.with(|configured| {
        if configured.get() == point {
            configured.set(0);
            Err(format!("injected update crash at point {point}"))
        } else {
            Ok(())
        }
    })
}

#[cfg(not(test))]
fn inject_update_crash(_point: u8) -> Result<(), String> {
    Ok(())
}

fn read_regular(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("expected a regular file: {}", path.display()));
    }
    fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))
}

fn read_yaml(path: &Path) -> Result<Value, String> {
    let bytes = read_regular(path)?;
    serde_yaml::from_slice(&bytes).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|error| format!("create {}: {error}", path.display()))?;
    file.write_all(bytes)
        .map_err(|error| format!("write {}: {error}", path.display()))?;
    file.sync_all()
        .map_err(|error| format!("sync {}: {error}", path.display()))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).map_err(|error| format!("create {}: {error}", parent.display()))?;
    ensure_no_symlink_leaf(path)?;
    let temporary = parent.join(format!(".tmp-{}", Uuid::new_v4()));
    write_synced(&temporary, bytes)?;
    fs::rename(&temporary, path).map_err(|error| format!("replace {}: {error}", path.display()))?;
    sync_dir(parent)
}

fn sync_dir(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("sync directory {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, MigrationOptions) {
        let dir = tempdir().unwrap();
        let source = dir
            .path()
            .join("source/alice/conversation/202604/20260403_173300");
        let vault = dir.path().join("vault");
        let data = dir.path().join("data");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&vault).unwrap();
        fs::write(
            source.join("meta.json"),
            r#"{"created_at":"2026-04-03T17:33:00+08:00","updated_at":"2026-04-04T08:00:00+08:00","title":"Weekly","source":"mac","category":"meeting","key_topics":["release"],"language":"zh-CN","audio":{"path":"secret.wav","duration_ms":1000}}"#,
        )
        .unwrap();
        fs::write(
            source.join("content.md"),
            "# Weekly\n---\nSummary: ready\n---\n00:00:00  Alice: hello\n",
        )
        .unwrap();
        fs::write(source.join("recording.wav"), b"audio").unwrap();
        fs::write(source.join("summary.md"), b"# Summary\r\n").unwrap();
        let options = MigrationOptions {
            source: dir.path().join("source"),
            user: Some("alice".into()),
            timezone: None,
            mode: crate::model::MigrationMode::Incremental,
        };
        (dir, vault, data, options)
    }

    #[test]
    fn dry_run_is_zero_write_and_reports_excluded_audio() {
        let (_dir, vault, data, options) = fixture();
        let service = MigrationService::new(&vault, &data);
        let report = service.plan(&options).unwrap();
        assert_eq!(report.create, 1);
        assert_eq!(report.excluded_audio, 1);
        assert!(!vault.join(".notemd").exists());
        assert!(!vault.join(MEETINGS_REL).exists());
        assert!(!data.exists());
    }

    #[test]
    fn apply_copies_only_transcript_metadata_and_optional_summary() {
        let (_dir, vault, data, options) = fixture();
        let service = MigrationService::new(&vault, &data);
        let report = service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        assert_eq!(report.committed, 1);
        let target = vault.join(MEETINGS_REL).join("20260403_173300");
        let names: BTreeSet<String> = fs::read_dir(&target)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            BTreeSet::from([
                "meta.yml".into(),
                "summary.md".into(),
                "transcript.md".into(),
            ])
        );
        assert_eq!(
            fs::read(target.join("transcript.md")).unwrap(),
            b"# Weekly\n---\nSummary: ready\n---\n00:00:00  Alice: hello\n"
        );
        assert_eq!(
            fs::read(target.join("summary.md")).unwrap(),
            b"# Summary\r\n"
        );
        let meta_text = fs::read_to_string(target.join("meta.yml")).unwrap();
        let meta: Value = serde_yaml::from_str(&meta_text).unwrap();
        let keys: BTreeSet<&str> = meta
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "category",
                "conversation_id",
                "created_at",
                "duration_ms",
                "imported_from",
                "key_topics",
                "language",
                "source",
                "speaker_count",
                "summary_file",
                "title",
                "transcript_file",
                "updated_at",
            ])
        );
        assert_eq!(meta["transcript_file"], "transcript.md");
        assert_eq!(meta["summary_file"], "summary.md");
        assert_eq!(meta["source"], "hemory_v1.0:mac");
        assert_eq!(meta["updated_at"], "2026-04-04T08:00:00+08:00");
        assert!(!meta_text.contains("secret.wav"));
        assert!(!meta_text.to_ascii_lowercase().contains("audio"));
    }

    #[test]
    fn custom_root_controls_plan_apply_and_library_paths() {
        let (_dir, vault, data, options) = fixture();
        let service =
            MigrationService::with_meetings_root(&vault, &data, "archive/transcripts").unwrap();
        let plan = service.plan(&options).unwrap();
        assert_eq!(
            plan.items[0].target_relative_path,
            "archive/transcripts/20260403_173300"
        );
        service
            .apply(&options, Some(&plan), &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        assert!(vault
            .join("archive/transcripts/20260403_173300/transcript.md")
            .is_file());
        assert!(!vault.join(MEETINGS_REL).exists());
        let meetings = service.library_list().unwrap();
        assert_eq!(meetings.len(), 1);
        assert_eq!(
            meetings[0].transcript_relative_path,
            "archive/transcripts/20260403_173300/transcript.md"
        );
    }

    #[test]
    fn changing_root_conflicts_with_an_existing_checkpoint_instead_of_copying_it() {
        let (_dir, vault, data, options) = fixture();
        let original = MigrationService::new(&vault, &data);
        original
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();

        let moved = MigrationService::with_meetings_root(&vault, &data, "other/meetings").unwrap();
        let report = moved.plan(&options).unwrap();
        assert_eq!(report.conflict, 1);
        assert_eq!(report.create, 0);
        assert!(report.items[0]
            .reason
            .contains("does not move existing meetings"));
        assert!(!vault.join("other/meetings/20260403_173300").exists());
        assert!(vault.join(MEETINGS_REL).join("20260403_173300").exists());
    }

    #[test]
    fn reviewed_plan_is_bound_to_the_configured_target_root() {
        let (_dir, vault, data, options) = fixture();
        let original = MigrationService::new(&vault, &data);
        let plan = original.plan(&options).unwrap();
        let moved = MigrationService::with_meetings_root(&vault, &data, "other/meetings").unwrap();
        let error = moved
            .apply(&options, Some(&plan), &AtomicBool::new(false), |_, _, _| {})
            .unwrap_err();
        assert!(
            error.contains("source or target changed after preflight"),
            "{error}"
        );
        assert!(!vault.join("other/meetings/20260403_173300").exists());
    }

    #[test]
    fn journal_recovery_uses_its_recorded_root_after_the_setting_changes() {
        let (_dir, vault, data, options) = fixture();
        let custom =
            MigrationService::with_meetings_root(&vault, &data, "archive/transcripts").unwrap();
        let bundle = custom.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let entry = ledger_entry_at(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            "archive/transcripts/20260403_173300",
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );
        fs::create_dir_all(vault.join(".notemd/meetings")).unwrap();
        UPDATE_CRASH_POINT.with(|configured| configured.set(3));
        let error = custom
            .commit_meeting(planned, &MigrationAction::Create, None, key, &entry)
            .unwrap_err();
        assert!(
            error.contains("injected update crash at point 3"),
            "{error}"
        );
        assert!(vault.join(TRANSACTION_JOURNAL_REL).exists(), "{error}");

        let service_after_setting_change = MigrationService::new(&vault, &data);
        service_after_setting_change
            .recover_transaction_journal()
            .unwrap();
        assert!(vault
            .join("archive/transcripts/20260403_173300/transcript.md")
            .is_file());
        assert!(!vault.join(TRANSACTION_JOURNAL_REL).exists());
        let (ledger, corrupt) = service_after_setting_change.read_ledger(false).unwrap();
        assert!(!corrupt);
        assert_eq!(
            ledger.entries.get(key).unwrap().target_relative_path,
            "archive/transcripts/20260403_173300"
        );
    }

    #[test]
    fn repeated_incremental_and_full_are_byte_and_mtime_idempotent() {
        let (_dir, vault, data, mut options) = fixture();
        let service = MigrationService::new(&vault, &data);
        service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        let transcript = vault
            .join(MEETINGS_REL)
            .join("20260403_173300/transcript.md");
        let before = fs::metadata(&transcript).unwrap().modified().unwrap();
        let bytes = fs::read(&transcript).unwrap();
        thread::sleep(Duration::from_millis(20));
        let incremental = service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        assert_eq!(incremental.skip, 1);
        options.mode = crate::model::MigrationMode::Full;
        let full = service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        assert_eq!(full.skip, 1);
        assert_eq!(fs::read(&transcript).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&transcript).unwrap().modified().unwrap(),
            before
        );
    }

    #[test]
    fn source_change_updates_but_local_target_edit_conflicts() {
        let (dir, vault, data, options) = fixture();
        let service = MigrationService::new(&vault, &data);
        service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        let source = dir
            .path()
            .join("source/alice/conversation/202604/20260403_173300/content.md");
        fs::write(&source, "00:00:00  Alice: changed\n").unwrap();
        assert_eq!(service.plan(&options).unwrap().update, 1);
        let target = vault
            .join(MEETINGS_REL)
            .join("20260403_173300/transcript.md");
        fs::write(&target, "00:00:00  Alice: local\n").unwrap();
        let report = service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        assert_eq!(report.conflict, 1);
        assert!(fs::read_to_string(target).unwrap().contains("local"));
    }

    #[test]
    fn extra_target_entry_blocks_update_and_is_never_deleted() {
        let (dir, vault, data, options) = fixture();
        let service = MigrationService::new(&vault, &data);
        service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        let target = vault.join(MEETINGS_REL).join("20260403_173300");
        fs::write(target.join("user-notes.md"), b"do not delete").unwrap();
        fs::write(
            dir.path()
                .join("source/alice/conversation/202604/20260403_173300/content.md"),
            "00:00:00  Alice: source changed\n",
        )
        .unwrap();
        let report = service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        assert_eq!(report.conflict, 1);
        assert_eq!(
            fs::read(target.join("user-notes.md")).unwrap(),
            b"do not delete"
        );
    }

    #[test]
    fn update_journal_recovers_both_directory_swap_crash_windows() {
        for point in [1_u8, 2_u8] {
            let (dir, vault, data, options) = fixture();
            let service = MigrationService::new(&vault, &data);
            service
                .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
                .unwrap();
            fs::write(
                dir.path()
                    .join("source/alice/conversation/202604/20260403_173300/content.md"),
                "00:00:00  Alice: recovered update\n",
            )
            .unwrap();
            UPDATE_CRASH_POINT.with(|configured| configured.set(point));
            let interrupted = service
                .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
                .unwrap();
            assert_eq!(interrupted.conflict, 1);
            assert!(
                !vault.join(TRANSACTION_JOURNAL_REL).exists(),
                "commit errors must recover their journal before returning"
            );

            let recovered = service
                .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
                .unwrap();
            assert_eq!(recovered.conflict, 0);
            assert_eq!(
                fs::read_to_string(
                    vault
                        .join(MEETINGS_REL)
                        .join("20260403_173300/transcript.md")
                )
                .unwrap(),
                "00:00:00  Alice: recovered update\n"
            );
            assert!(!vault.join(TRANSACTION_JOURNAL_REL).exists());
            assert!(!fs::read_dir(vault.join(MEETINGS_REL))
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| entry.file_name().to_string_lossy().starts_with(".txn-")));
        }
    }

    #[test]
    fn create_journal_recovers_activation_before_ledger_checkpoint() {
        let (_dir, vault, data, options) = fixture();
        let service = MigrationService::new(&vault, &data);
        UPDATE_CRASH_POINT.with(|configured| configured.set(3));
        let interrupted = service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        assert_eq!(interrupted.conflict, 1);
        assert!(!vault.join(TRANSACTION_JOURNAL_REL).exists());
        let target = vault.join(MEETINGS_REL).join("20260403_173300");
        assert!(target.join("meta.yml").is_file());
        assert_eq!(service.plan(&options).unwrap().skip, 1);
    }

    #[test]
    fn partial_journal_publish_never_exposes_a_formal_journal_or_deletes_unknown_bytes() {
        let (_dir, vault, data, options) = fixture();
        fs::create_dir_all(vault.join(".notemd/meetings")).unwrap();
        let service = MigrationService::new(&vault, &data);
        let bundle = service.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let entry = ledger_entry(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );
        let nonce = Uuid::new_v4().simple().to_string();
        let journal = TransactionJournal {
            schema_version: 1,
            nonce: nonce.clone(),
            kind: TransactionKind::Create,
            phase: TransactionPhase::Active,
            source_key: key.clone(),
            target_name: planned.meeting.conversation_id.clone(),
            backup_name: None,
            stage_name: format!(".txn-{nonce}-stage"),
            old_hashes: None,
            new_entry: entry,
        };

        UPDATE_CRASH_POINT.with(|configured| configured.set(4));
        let error = service.write_transaction_journal_new(&journal).unwrap_err();
        assert!(error.contains("partial-write"), "{error}");
        assert!(!vault.join(TRANSACTION_JOURNAL_REL).exists());
        let temporary = vault
            .join(".notemd/meetings")
            .join(journal_temp_name(&nonce));
        let partial = fs::read(&temporary).unwrap();
        assert!(!partial.is_empty());
        assert!(partial.len() < serde_json::to_vec_pretty(&journal).unwrap().len());
        service.recover_transaction_journal().unwrap();
        assert_eq!(fs::read(&temporary).unwrap(), partial);
    }

    #[test]
    fn verified_unpublished_journal_is_cleaned_after_pre_publish_failure() {
        let (_dir, vault, data, options) = fixture();
        fs::create_dir_all(vault.join(".notemd/meetings")).unwrap();
        let service = MigrationService::new(&vault, &data);
        let bundle = service.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let entry = ledger_entry(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );
        let nonce = Uuid::new_v4().simple().to_string();
        let journal = TransactionJournal {
            schema_version: 1,
            nonce: nonce.clone(),
            kind: TransactionKind::Create,
            phase: TransactionPhase::Active,
            source_key: key.clone(),
            target_name: planned.meeting.conversation_id.clone(),
            backup_name: None,
            stage_name: format!(".txn-{nonce}-stage"),
            old_hashes: None,
            new_entry: entry,
        };

        UPDATE_CRASH_POINT.with(|configured| configured.set(5));
        service.write_transaction_journal_new(&journal).unwrap_err();
        assert!(!vault.join(TRANSACTION_JOURNAL_REL).exists());
        assert!(!vault
            .join(".notemd/meetings")
            .join(journal_temp_name(&nonce))
            .exists());
    }

    #[test]
    fn create_rollback_recovery_is_idempotent_after_stage_removal() {
        let (_dir, vault, data, options) = fixture();
        fs::create_dir_all(vault.join(".notemd/meetings")).unwrap();
        let service = MigrationService::new(&vault, &data);
        let bundle = service.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let entry = ledger_entry(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );

        UPDATE_CRASH_POINT.with(|configured| configured.set(6));
        service
            .commit_meeting(planned, &MigrationAction::Create, None, key, &entry)
            .unwrap_err();
        let journal_path = vault.join(TRANSACTION_JOURNAL_REL);
        let journal: TransactionJournal =
            serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
        let stage = vault.join(MEETINGS_REL).join(&journal.stage_name);
        assert!(stage.exists());

        UPDATE_CRASH_POINT.with(|configured| configured.set(7));
        assert!(service.recover_transaction_journal().is_err());
        assert!(!stage.exists());
        assert!(journal_path.exists());
        let prepared: TransactionJournal =
            serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
        assert_eq!(prepared.phase, TransactionPhase::CreateRollbackPrepared);
        service.recover_transaction_journal().unwrap();
        assert!(!journal_path.exists());
        assert!(!vault.join(MEETINGS_REL).join(&journal.target_name).exists());
    }

    #[test]
    fn partial_phase_temp_does_not_block_retrying_the_formal_journal() {
        let (_dir, vault, data, options) = fixture();
        fs::create_dir_all(vault.join(".notemd/meetings")).unwrap();
        let service = MigrationService::new(&vault, &data);
        let bundle = service.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let entry = ledger_entry(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );

        UPDATE_CRASH_POINT.with(|configured| configured.set(6));
        service
            .commit_meeting(planned, &MigrationAction::Create, None, key, &entry)
            .unwrap_err();
        let journal_path = vault.join(TRANSACTION_JOURNAL_REL);
        let journal: TransactionJournal =
            serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
        let partial = vault
            .join(".notemd/meetings")
            .join(journal_temp_name(&journal.nonce));
        fs::write(&partial, b"{partial phase journal").unwrap();

        service.recover_transaction_journal().unwrap();
        assert!(!journal_path.exists());
        assert_eq!(fs::read(&partial).unwrap(), b"{partial phase journal");
        assert!(!vault.join(MEETINGS_REL).join(&journal.stage_name).exists());
    }

    #[test]
    fn create_no_clobber_recovery_is_idempotent_after_stage_removal() {
        let (_dir, vault, data, options) = fixture();
        fs::create_dir_all(vault.join(".notemd/meetings")).unwrap();
        let service = MigrationService::new(&vault, &data);
        let bundle = service.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let entry = ledger_entry(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );
        let target = vault.join(MEETINGS_REL).join("20260403_173300");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("sentinel.txt"), b"external owner").unwrap();
        service
            .commit_meeting(planned, &MigrationAction::Create, None, key, &entry)
            .unwrap_err();
        let journal_path = vault.join(TRANSACTION_JOURNAL_REL);
        let journal: TransactionJournal =
            serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
        let stage = vault.join(MEETINGS_REL).join(&journal.stage_name);
        assert!(stage.exists());

        UPDATE_CRASH_POINT.with(|configured| configured.set(8));
        assert!(service.recover_transaction_journal().is_err());
        assert!(!stage.exists());
        assert!(journal_path.exists());
        let prepared: TransactionJournal =
            serde_json::from_slice(&fs::read(&journal_path).unwrap()).unwrap();
        assert_eq!(prepared.phase, TransactionPhase::CreateNoClobberPrepared);
        assert_eq!(
            fs::read(target.join("sentinel.txt")).unwrap(),
            b"external owner"
        );
        service.recover_transaction_journal().unwrap();
        assert!(!journal_path.exists());
        assert_eq!(
            fs::read(target.join("sentinel.txt")).unwrap(),
            b"external owner"
        );
        assert_eq!(service.plan(&options).unwrap().conflict, 1);
    }

    #[test]
    fn recovered_first_create_is_not_overwritten_by_the_next_meeting_checkpoint() {
        let (dir, vault, data, options) = fixture();
        let second = dir
            .path()
            .join("source/alice/conversation/202604/20260403_173301");
        fs::create_dir_all(&second).unwrap();
        fs::write(
            second.join("meta.json"),
            r#"{"created_at":"2026-04-03T17:33:01+08:00"}"#,
        )
        .unwrap();
        fs::write(second.join("content.md"), "00:00:00  Bob: second\n").unwrap();
        let service = MigrationService::new(&vault, &data);
        UPDATE_CRASH_POINT.with(|configured| configured.set(3));
        let report = service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        assert_eq!(report.committed, 1);
        assert_eq!(report.conflict, 1);
        assert!(!vault.join(TRANSACTION_JOURNAL_REL).exists());
        let ledger: Ledger =
            serde_json::from_slice(&fs::read(vault.join(LEDGER_REL)).expect("ledger must exist"))
                .unwrap();
        assert_eq!(ledger.entries.len(), 2);
    }

    #[test]
    fn atomic_create_no_clobber_preserves_a_target_that_appears_after_plan() {
        let (_dir, vault, data, options) = fixture();
        fs::create_dir_all(vault.join(".notemd/meetings")).unwrap();
        let service = MigrationService::new(&vault, &data);
        let bundle = service.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let entry = ledger_entry(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );
        let target = vault.join(MEETINGS_REL).join("20260403_173300");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("sentinel.txt"), b"external owner").unwrap();
        let error = service
            .commit_meeting(planned, &MigrationAction::Create, None, key, &entry)
            .unwrap_err();
        assert!(error.contains("without clobber"), "{error}");
        assert_eq!(
            fs::read(target.join("sentinel.txt")).unwrap(),
            b"external owner"
        );
        service.recover_transaction_journal().unwrap();
        assert_eq!(
            fs::read(target.join("sentinel.txt")).unwrap(),
            b"external owner"
        );
        assert!(!vault.join(TRANSACTION_JOURNAL_REL).exists());
    }

    #[test]
    fn polluted_stage_and_existing_journal_fail_closed_without_deletion_or_overwrite() {
        let (_dir, vault, data, options) = fixture();
        fs::create_dir_all(vault.join(".notemd/meetings")).unwrap();
        let service = MigrationService::new(&vault, &data);
        let bundle = service.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let entry = ledger_entry(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );
        let target = vault.join(MEETINGS_REL).join("20260403_173300");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("sentinel.txt"), b"external owner").unwrap();
        service
            .commit_meeting(planned, &MigrationAction::Create, None, key, &entry)
            .unwrap_err();
        let stage = fs::read_dir(vault.join(MEETINGS_REL))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .ends_with("-stage")
            })
            .unwrap();
        fs::write(stage.join("unexpected.bin"), b"pollution").unwrap();
        assert!(service.recover_transaction_journal().is_err());
        assert!(stage.join("unexpected.bin").exists());
        assert!(vault.join(TRANSACTION_JOURNAL_REL).exists());
        assert_eq!(
            fs::read(target.join("sentinel.txt")).unwrap(),
            b"external owner"
        );

        let original = fs::read(vault.join(TRANSACTION_JOURNAL_REL)).unwrap();
        let existing: TransactionJournal = serde_json::from_slice(&original).unwrap();
        assert!(service.write_transaction_journal_new(&existing).is_err());
        assert_eq!(
            fs::read(vault.join(TRANSACTION_JOURNAL_REL)).unwrap(),
            original
        );
    }

    #[test]
    fn journal_nonce_and_target_binding_are_strict() {
        let (_dir, vault, data, options) = fixture();
        fs::create_dir_all(vault.join(".notemd/meetings")).unwrap();
        let service = MigrationService::new(&vault, &data);
        let bundle = service.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let entry = ledger_entry(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );
        let nonce = Uuid::new_v4().simple().to_string();
        let journal = TransactionJournal {
            schema_version: 1,
            nonce: nonce.clone(),
            kind: TransactionKind::Create,
            phase: TransactionPhase::Active,
            source_key: key.clone(),
            target_name: planned.meeting.conversation_id.clone(),
            backup_name: None,
            stage_name: format!(".txn-{nonce}-stage-for-another-target"),
            old_hashes: None,
            new_entry: entry,
        };
        assert!(service.write_transaction_journal_new(&journal).is_err());
        assert!(!vault.join(TRANSACTION_JOURNAL_REL).exists());
        fs::write(
            vault.join(TRANSACTION_JOURNAL_REL),
            serde_json::to_vec_pretty(&journal).unwrap(),
        )
        .unwrap();
        assert!(service
            .recover_transaction_journal()
            .unwrap_err()
            .contains("identity"));
        assert!(vault.join(TRANSACTION_JOURNAL_REL).exists());
    }

    #[test]
    fn active_create_with_no_target_or_stage_fails_closed() {
        let (_dir, vault, data, options) = fixture();
        fs::create_dir_all(vault.join(".notemd/meetings")).unwrap();
        let service = MigrationService::new(&vault, &data);
        let bundle = service.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let entry = ledger_entry(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );
        let nonce = Uuid::new_v4().simple().to_string();
        let journal = TransactionJournal {
            schema_version: 1,
            nonce: nonce.clone(),
            kind: TransactionKind::Create,
            phase: TransactionPhase::Active,
            source_key: key.clone(),
            target_name: planned.meeting.conversation_id.clone(),
            backup_name: None,
            stage_name: format!(".txn-{nonce}-stage"),
            old_hashes: None,
            new_entry: entry,
        };
        service.write_transaction_journal_new(&journal).unwrap();

        let error = service.recover_transaction_journal().unwrap_err();
        assert!(error.contains("active create journal"), "{error}");
        assert!(vault.join(TRANSACTION_JOURNAL_REL).exists());
    }

    #[test]
    fn recovery_never_deletes_a_polluted_update_backup() {
        let (dir, vault, data, options) = fixture();
        let service = MigrationService::new(&vault, &data);
        service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        fs::write(
            dir.path()
                .join("source/alice/conversation/202604/20260403_173300/content.md"),
            "00:00:00  Alice: new bytes\n",
        )
        .unwrap();
        let bundle = service.plan_inner(&options, false, None).unwrap();
        let (key, planned) = bundle.meetings.iter().next().unwrap();
        let previous = bundle.ledger.entries.get(key).unwrap();
        let entry = ledger_entry(
            &bundle.instance_id,
            &bundle.report.source_user,
            &planned.meeting,
            desired_hashes(&planned.meeting, &planned.meta_bytes),
        );
        UPDATE_CRASH_POINT.with(|configured| configured.set(2));
        service
            .commit_meeting(
                planned,
                &MigrationAction::Update,
                Some(previous),
                key,
                &entry,
            )
            .unwrap_err();
        let backup = fs::read_dir(vault.join(MEETINGS_REL))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains("-backup-")
            })
            .unwrap();
        fs::write(backup.join("unexpected.txt"), b"must survive audit").unwrap();
        assert!(service.recover_transaction_journal().is_err());
        assert_eq!(
            fs::read(backup.join("unexpected.txt")).unwrap(),
            b"must survive audit"
        );
        assert!(vault.join(TRANSACTION_JOURNAL_REL).exists());
    }

    #[test]
    fn library_uses_checkpointed_speaker_mapping_for_imported_srt() {
        let dir = tempdir().unwrap();
        let source = dir
            .path()
            .join("source/alice/conversation/202604/20260403_173300");
        let vault = dir.path().join("vault");
        let data = dir.path().join("data");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&vault).unwrap();
        fs::write(
            source.join("meta.json"),
            r#"{"created_at":"2026-04-03T17:33:00+08:00"}"#,
        )
        .unwrap();
        fs::write(
            source.join("speakers.json"),
            r#"{"speakers":{"Speaker A":{"cluster_id":"spk_01","name":"Alice"}}}"#,
        )
        .unwrap();
        fs::write(
            source.join("pro_asr.srt"),
            b"1\n00:00:00,000 --> 00:00:01,000\n[Speaker A] hello\n",
        )
        .unwrap();
        let options = MigrationOptions {
            source: dir.path().join("source"),
            user: Some("alice".into()),
            timezone: None,
            mode: crate::model::MigrationMode::Incremental,
        };
        let service = MigrationService::new(&vault, &data);
        service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        let meetings = service.library_list().unwrap();
        assert_eq!(meetings.len(), 1);
        assert_eq!(meetings[0].speaker_count, 1);
        assert!(meetings[0]
            .transcript_relative_path
            .ends_with("transcript.srt"));
    }

    #[test]
    fn library_requires_transcript_and_summary_declarations_to_match_disk() {
        let (_dir, vault, data, options) = fixture();
        let service = MigrationService::new(&vault, &data);
        service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        assert_eq!(service.library_list().unwrap().len(), 1);
        let target = vault.join(MEETINGS_REL).join("20260403_173300");
        fs::remove_file(target.join("summary.md")).unwrap();
        assert!(service.library_list().unwrap().is_empty());

        fs::write(target.join("summary.md"), b"# Summary\r\n").unwrap();
        let meta_path = target.join("meta.yml");
        let mut meta: Value = serde_yaml::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
        meta["transcript_file"] = Value::String("transcript.srt".into());
        fs::write(&meta_path, serde_yaml::to_string(&meta).unwrap()).unwrap();
        assert!(service.library_list().unwrap().is_empty());
    }

    #[test]
    fn reviewed_plan_pins_fallback_updated_at_and_meta_hash() {
        let (_dir, vault, data, options) = fixture();
        let service = MigrationService::new(&vault, &data);
        // Remove the source timestamp so updated_at must use the reviewed
        // plan's stable prospective commit time.
        let meta_path = options
            .source
            .join("alice/conversation/202604/20260403_173300/meta.json");
        let mut meta: Value = serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
        meta.as_object_mut().unwrap().remove("updated_at");
        fs::write(&meta_path, serde_json::to_vec(&meta).unwrap()).unwrap();
        let plan = service.plan(&options).unwrap();
        assert!(!data.exists(), "dry-run must not bind a source instance");
        service
            .apply(&options, Some(&plan), &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        let target = vault.join(MEETINGS_REL).join("20260403_173300");
        let target_meta = fs::read(target.join("meta.yml")).unwrap();
        assert_eq!(
            hemory::sha256(&target_meta),
            plan.items[0].output_hashes.meta.clone().unwrap()
        );
        let yaml: Value = serde_yaml::from_slice(&target_meta).unwrap();
        assert_eq!(yaml["updated_at"], plan.planned_at);
        let bindings: SourceBindings =
            serde_json::from_slice(&fs::read(service.bindings_path()).unwrap()).unwrap();
        let id = bindings.roots.values().next().unwrap();
        assert!(id.starts_with("src-"));
        assert!(Uuid::parse_str(id.trim_start_matches("src-")).is_ok());
    }

    #[test]
    fn discovered_but_newly_invalid_source_is_blocked_not_source_missing() {
        let (dir, vault, data, options) = fixture();
        let service = MigrationService::new(&vault, &data);
        service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        fs::write(
            dir.path()
                .join("source/alice/conversation/202604/20260403_173300/content.md"),
            "invalid",
        )
        .unwrap();
        let report = service.plan(&options).unwrap();
        assert_eq!(report.blocked, 1);
        assert_eq!(report.source_missing, 0);
    }

    #[test]
    fn renormalize_failure_stays_in_the_partial_report() {
        let (dir, vault, data, options) = fixture();
        let second = dir
            .path()
            .join("source/alice/conversation/202604/20260403_173301");
        fs::create_dir_all(&second).unwrap();
        fs::write(
            second.join("meta.json"),
            r#"{"created_at":"2026-04-03T17:33:01+08:00"}"#,
        )
        .unwrap();
        fs::write(
            second.join("content.md"),
            "00:00:00  Bob: initially valid\n",
        )
        .unwrap();
        let second_content = second.join("content.md");
        let service = MigrationService::new(&vault, &data);
        let report = service
            .apply(
                &options,
                None,
                &AtomicBool::new(false),
                |committed, _, _| {
                    if committed == 1 {
                        fs::write(&second_content, "became invalid").unwrap();
                    }
                },
            )
            .unwrap();
        assert_eq!(report.committed, 1);
        assert_eq!(report.conflict, 1);
        assert!(report
            .errors
            .iter()
            .any(|error| error.contains("source became invalid after planning")));
    }

    #[test]
    fn source_missing_keeps_target() {
        let (dir, vault, data, options) = fixture();
        let service = MigrationService::new(&vault, &data);
        service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        fs::remove_dir_all(
            dir.path()
                .join("source/alice/conversation/202604/20260403_173300"),
        )
        .unwrap();
        let report = service.plan(&options).unwrap();
        assert_eq!(report.source_missing, 1);
        assert!(vault.join(MEETINGS_REL).join("20260403_173300").exists());
    }

    #[test]
    fn a_source_moved_to_deleted_is_not_reimported_or_deleted_from_the_archive() {
        let (dir, vault, data, mut options) = fixture();
        let service = MigrationService::new(&vault, &data);
        service
            .apply(&options, None, &AtomicBool::new(false), |_, _, _| {})
            .unwrap();
        let source = dir
            .path()
            .join("source/alice/conversation/202604/20260403_173300");
        let deleted = dir
            .path()
            .join("source/alice/conversation/_deleted/20260403_173300");
        fs::create_dir_all(deleted.parent().unwrap()).unwrap();
        fs::rename(source, deleted).unwrap();

        let incremental = service.plan(&options).unwrap();
        assert_eq!(incremental.source_missing, 1);
        assert_eq!(incremental.create + incremental.update, 0);
        options.mode = crate::model::MigrationMode::Full;
        let full = service.plan(&options).unwrap();
        assert_eq!(full.source_missing, 1);
        assert_eq!(full.create + full.update, 0);
        assert!(vault.join(MEETINGS_REL).join("20260403_173300").exists());
    }

    #[cfg(unix)]
    #[test]
    fn target_symlink_is_never_followed() {
        use std::os::unix::fs::symlink;
        let (dir, vault, data, options) = fixture();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(vault.join(MEETINGS_REL)).unwrap();
        symlink(&outside, vault.join(MEETINGS_REL).join("20260403_173300")).unwrap();
        let service = MigrationService::new(&vault, &data);
        assert!(service.plan(&options).unwrap_err().contains("symlink"));
        assert!(fs::read_dir(outside).unwrap().next().is_none());
    }
}
