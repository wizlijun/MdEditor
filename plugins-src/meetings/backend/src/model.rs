use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MigrationMode {
    #[default]
    Incremental,
    Full,
}

#[derive(Clone, Debug)]
pub struct MigrationOptions {
    pub source: PathBuf,
    pub user: Option<String>,
    pub timezone: Option<String>,
    pub mode: MigrationMode,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceDetection {
    pub source: String,
    pub users: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_user: Option<String>,
    pub needs_timezone: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct OutputHashes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationAction {
    Create,
    Update,
    Skip,
    Conflict,
    Blocked,
    SourceMissing,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MigrationItem {
    pub conversation_id: String,
    pub source_relative_path: String,
    pub source_schema: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_transcript: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_fingerprint: Option<String>,
    pub target_relative_path: String,
    pub action: MigrationAction,
    pub reason: String,
    pub output_hashes: OutputHashes,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MigrationReport {
    pub schema_version: u32,
    pub mode: MigrationMode,
    pub dry_run: bool,
    pub planned_at: String,
    pub source_user: String,
    pub scanned: usize,
    pub eligible: usize,
    pub create: usize,
    pub update: usize,
    pub skip: usize,
    pub conflict: usize,
    pub blocked: usize,
    pub excluded_audio: usize,
    pub committed: usize,
    pub source_missing: usize,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub items: Vec<MigrationItem>,
}

impl MigrationReport {
    pub fn new(mode: MigrationMode, dry_run: bool, source_user: String) -> Self {
        Self {
            schema_version: 1,
            mode,
            dry_run,
            planned_at: String::new(),
            source_user,
            scanned: 0,
            eligible: 0,
            create: 0,
            update: 0,
            skip: 0,
            conflict: 0,
            blocked: 0,
            excluded_audio: 0,
            committed: 0,
            source_missing: 0,
            warnings: Vec::new(),
            errors: Vec::new(),
            items: Vec::new(),
        }
    }

    pub fn recount(&mut self) {
        self.create = 0;
        self.update = 0;
        self.skip = 0;
        self.conflict = 0;
        self.blocked = 0;
        self.source_missing = 0;
        self.eligible = 0;
        for item in &self.items {
            match item.action {
                MigrationAction::Create => self.create += 1,
                MigrationAction::Update => self.update += 1,
                MigrationAction::Skip => self.skip += 1,
                MigrationAction::Conflict => self.conflict += 1,
                MigrationAction::Blocked => self.blocked += 1,
                MigrationAction::SourceMissing => self.source_missing += 1,
            }
            if matches!(
                item.action,
                MigrationAction::Create | MigrationAction::Update | MigrationAction::Skip
            ) {
                self.eligible += 1;
            }
        }
    }

    pub fn is_clean(&self) -> bool {
        self.errors.is_empty() && self.conflict == 0 && self.blocked == 0
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SpeakerMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub name_source: String,
    pub source_labels: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct NormalizedMeeting {
    pub source_abs: PathBuf,
    pub source_relative_path: String,
    pub source_schema: String,
    pub conversation_id: String,
    pub original_conversation_id: String,
    pub original_directory: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_ms: Option<u64>,
    pub title: Option<String>,
    pub language: Option<String>,
    pub category: Option<Value>,
    pub topics: Vec<String>,
    pub source: String,
    pub source_updated_at: Option<String>,
    pub speakers: BTreeMap<String, SpeakerMeta>,
    pub transcript_bytes: Vec<u8>,
    pub transcript_source_kind: String,
    pub transcript_relative_path: String,
    pub transcript_output_file: String,
    pub transcript_format: String,
    pub transcript_sha256: String,
    pub summary_bytes: Option<Vec<u8>>,
    pub summary_sha256: Option<String>,
    pub fingerprint: String,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MeetingSummary {
    pub conversation_id: String,
    pub title: String,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub speaker_count: usize,
    pub source: String,
    pub target_relative_path: String,
    pub transcript_relative_path: String,
}
