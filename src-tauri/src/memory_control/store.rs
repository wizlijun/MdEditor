use super::document::{self, EntryUpdate};
use super::model::*;
use chrono::{SecondsFormat, Utc};
use fs2::FileExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use uuid::Uuid;

const STATE_PATH: &str = ".notemd/memory/state.json";
const LOCK_PATH: &str = ".notemd/memory/control.lock";
const CANDIDATE_DIR: &str = "inbox/memory-candidates";
const EVENT_DIR: &str = "memory/events";

struct MemoryLock(File);
impl Drop for MemoryLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn lock(root: &Path) -> Result<MemoryLock, String> {
    let path = root.join(LOCK_PATH);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("memory: create lock dir: {e}"))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("memory: open lock: {e}"))?;
    file.lock_exclusive()
        .map_err(|e| format!("memory: lock: {e}"))?;
    Ok(MemoryLock(file))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn read_required(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("memory: read {}: {e}", path.display()))
}

fn atomic_write(path: &Path, content: &str) -> Result<(), String> {
    let parent = path.parent().ok_or("memory: target has no parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("memory: create {}: {e}", parent.display()))?;
    let mode = fs::metadata(path).ok().map(|m| m.permissions());
    let mut temp = NamedTempFile::new_in(parent).map_err(|e| format!("memory: temp file: {e}"))?;
    temp.write_all(content.as_bytes())
        .map_err(|e| format!("memory: temp write: {e}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| format!("memory: temp sync: {e}"))?;
    if let Some(permissions) = mode {
        let _ = temp.as_file().set_permissions(permissions);
    }
    temp.persist(path)
        .map_err(|e| format!("memory: publish {}: {}", path.display(), e.error))?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn create_new(path: &Path, content: &str) -> Result<(), String> {
    let parent = path.parent().ok_or("memory: target has no parent")?;
    fs::create_dir_all(parent).map_err(|e| format!("memory: create {}: {e}", parent.display()))?;
    let mut temp = NamedTempFile::new_in(parent).map_err(|e| format!("memory: temp file: {e}"))?;
    temp.write_all(content.as_bytes())
        .map_err(|e| format!("memory: temp write: {e}"))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| format!("memory: temp sync: {e}"))?;
    temp.persist_noclobber(path)
        .map_err(|e| format!("memory: no-clobber publish {}: {}", path.display(), e.error))?;
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn state(root: &Path) -> Result<Option<ManagedState>, String> {
    let path = root.join(STATE_PATH);
    if !path.exists() {
        return Ok(None);
    }
    let raw = read_required(&path)?;
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|e| format!("memory: invalid state: {e}"))
}

fn write_state(root: &Path, state: &ManagedState) -> Result<(), String> {
    let raw =
        serde_json::to_string_pretty(state).map_err(|e| format!("memory: state JSON: {e}"))? + "\n";
    atomic_write(&root.join(STATE_PATH), &raw)
}

fn document_texts(root: &Path) -> Result<(String, String), String> {
    Ok((
        read_required(&root.join("USER.md"))?,
        read_required(&root.join("MEMORY.md"))?,
    ))
}

fn integrity(root: &Path, user: &str, memory: &str) -> Result<Integrity, String> {
    let Some(state) = state(root)? else {
        return Ok(Integrity {
            managed: false,
            drift: false,
            errors: Vec::new(),
        });
    };
    let mut errors = Vec::new();
    if state.protocol != PROTOCOL_VERSION {
        errors.push(format!("unsupported state protocol {}", state.protocol));
    }
    if !document::is_managed(user) {
        errors.push("USER.md is not a managed projection".into());
    }
    if !document::is_managed(memory) {
        errors.push("MEMORY.md is not a managed projection".into());
    }
    let user_hash = document::projection_hash(user);
    let memory_hash = document::projection_hash(memory);
    if document::stored_projection_hash(user).as_deref() != Some(user_hash.as_str())
        || state.user_hash != user_hash
    {
        errors.push("USER.md projection drift".into());
    }
    if document::stored_projection_hash(memory).as_deref() != Some(memory_hash.as_str())
        || state.memory_hash != memory_hash
    {
        errors.push("MEMORY.md projection drift".into());
    }
    errors.extend(document::classification_errors(user));
    errors.extend(document::classification_errors(memory));
    Ok(Integrity {
        managed: true,
        drift: !errors.is_empty(),
        errors,
    })
}

fn split_markdown(raw: &str) -> Result<(String, String), String> {
    let rest = raw
        .strip_prefix("---\n")
        .ok_or("memory: missing proposal frontmatter")?;
    let (yaml, body) = rest
        .split_once("\n---\n")
        .ok_or("memory: unterminated proposal frontmatter")?;
    Ok((yaml.to_string(), body.to_string()))
}

fn proposal_markdown(fm: &ProposalFrontmatter, text: &str, reason: &str) -> Result<String, String> {
    let yaml = serde_yaml::to_string(fm).map_err(|e| format!("memory: proposal YAML: {e}"))?;
    let mut out = format!("---\n{}---\n\n{}\n", yaml, text.trim());
    if !reason.trim().is_empty() {
        out.push_str(&format!("\n## Reason\n\n{}\n", reason.trim()));
    }
    Ok(out)
}

fn parse_proposal(
    path: &Path,
    root: &Path,
    decisions: &HashMap<String, ProposalDecision>,
) -> Result<Proposal, String> {
    let raw = read_required(path)?;
    let (yaml, body) = split_markdown(&raw)?;
    let fm: ProposalFrontmatter = serde_yaml::from_str(&yaml)
        .map_err(|e| format!("memory: proposal YAML {}: {e}", path.display()))?;
    if fm.kind != "Memory Proposal" || fm.proposal.version != PROTOCOL_VERSION {
        return Err(format!("memory: unsupported proposal {}", path.display()));
    }
    let (text, reason) = match body.split_once("\n## Reason\n") {
        Some((text, reason)) => (text.trim().to_string(), reason.trim().to_string()),
        None => (body.trim().to_string(), String::new()),
    };
    Ok(Proposal {
        path: path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/"),
        sha256: sha256(raw.as_bytes()),
        decision: decisions
            .get(&fm.proposal.id)
            .copied()
            .unwrap_or(ProposalDecision::Pending),
        frontmatter: fm,
        text,
        reason,
    })
}

fn event_markdown(fm: &EventFrontmatter) -> Result<String, String> {
    let yaml = serde_yaml::to_string(fm).map_err(|e| format!("memory: event YAML: {e}"))?;
    Ok(format!("---\n{}---\n\nThis event is immutable.\n", yaml))
}

fn collect_files(dir: &Path, suffix: &str, recursive: bool) -> Result<Vec<PathBuf>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("memory: list {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("memory: list entry: {e}"))?;
        let path = entry.path();
        if path.is_dir() && recursive {
            out.extend(collect_files(&path, suffix, true)?);
        } else if path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.ends_with(suffix))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn events(root: &Path) -> Result<Vec<EventFrontmatter>, String> {
    collect_files(&root.join(EVENT_DIR), ".memory-event.md", true)?
        .into_iter()
        .map(|path| {
            let raw = read_required(&path)?;
            let (yaml, _) = split_markdown(&raw)?;
            let event: EventFrontmatter = serde_yaml::from_str(&yaml)
                .map_err(|e| format!("memory: event YAML {}: {e}", path.display()))?;
            if event.kind != "Memory Decision" || event.event.version != PROTOCOL_VERSION {
                return Err(format!("memory: unsupported event {}", path.display()));
            }
            Ok(event)
        })
        .collect()
}

fn decision_map(events: &[EventFrontmatter]) -> HashMap<String, ProposalDecision> {
    events
        .iter()
        .map(|event| {
            let decision = match event.event.action {
                DecisionAction::Approve => ProposalDecision::Approved,
                DecisionAction::Reject => ProposalDecision::Rejected,
            };
            (event.event.proposal_id.clone(), decision)
        })
        .collect()
}

pub fn list(root: &Path) -> Result<Snapshot, String> {
    let (user, memory) = document_texts(root)?;
    let event_list = events(root)?;
    let decisions = decision_map(&event_list);
    let mut proposals = Vec::new();
    for path in collect_files(&root.join(CANDIDATE_DIR), ".memory-candidate.md", false)? {
        proposals.push(parse_proposal(&path, root, &decisions)?);
    }
    let mut entries = document::parse_blocks("USER.md", &user, Scope::UserProfile)
        .into_iter()
        .map(|b| b.entry)
        .collect::<Vec<_>>();
    if let Some(owner) = document::owner_entry(&user)? {
        entries.insert(0, owner);
    }
    entries.extend(
        document::parse_blocks("MEMORY.md", &memory, Scope::Memory)
            .into_iter()
            .map(|b| b.entry),
    );
    let mut integrity = integrity(root, &user, &memory)?;
    let mut ids = HashSet::new();
    for entry in &entries {
        if !entry.id.is_empty() && !ids.insert(entry.id.clone()) {
            integrity
                .errors
                .push(format!("duplicate entry id {}", entry.id));
        }
    }
    let mut proposal_ids = HashSet::new();
    for proposal in &proposals {
        if !proposal_ids.insert(proposal.frontmatter.proposal.id.clone()) {
            integrity.errors.push(format!(
                "duplicate proposal id {}",
                proposal.frontmatter.proposal.id
            ));
        }
    }
    let mut event_ids = HashSet::new();
    let mut decided_proposals = HashSet::new();
    for event in &event_list {
        if !event_ids.insert(event.event.id.clone()) {
            integrity
                .errors
                .push(format!("duplicate decision event id {}", event.event.id));
        }
        if !decided_proposals.insert(event.event.proposal_id.clone()) {
            integrity.errors.push(format!(
                "multiple decisions for proposal {}",
                event.event.proposal_id
            ));
        }
        match proposals
            .iter()
            .find(|proposal| proposal.frontmatter.proposal.id == event.event.proposal_id)
        {
            None => integrity.errors.push(format!(
                "decision references missing proposal {}",
                event.event.proposal_id
            )),
            Some(proposal) if proposal.sha256 != event.event.proposal_sha256 => {
                integrity.errors.push(format!(
                    "decided proposal hash mismatch {}",
                    event.event.proposal_id
                ));
            }
            Some(_) => {}
        }
        if !event.event.decided_by.starts_with("human:") {
            integrity.errors.push(format!(
                "decision actor is not human for proposal {}",
                event.event.proposal_id
            ));
        }
    }
    integrity.drift = integrity.drift || !integrity.errors.is_empty();
    let owner_actor = document::owner_actor(&user)?;
    Ok(Snapshot {
        entries,
        proposals,
        integrity,
        owner_actor,
    })
}

fn validate_propose(input: &ProposeInput) -> Result<(), String> {
    if input.text.trim().is_empty()
        && !matches!(
            input.operation,
            Operation::Revoke | Operation::Delete | Operation::SetPriority
        )
    {
        return Err("memory: proposal text is required".into());
    }
    if input.source.trim().is_empty() {
        return Err("memory: proposal source is required".into());
    }
    if input.by.trim().is_empty() || input.by.starts_with("human:") {
        return Err("memory: Agent proposal by must be a non-human producer id".into());
    }
    if input.dedupe_key.trim().is_empty() {
        return Err("memory: dedupe_key is required".into());
    }
    if !matches!(input.operation, Operation::Create)
        && (input
            .target_id
            .as_deref()
            .is_none_or(|target_id| target_id.trim().is_empty())
            || input.base_revision.is_none())
    {
        return Err("memory: target_id and base_revision are required for this operation".into());
    }
    if matches!(input.operation, Operation::SetPriority) && input.priority.is_none() {
        return Err("memory: priority is required".into());
    }
    if input.scope == Scope::UserOwner
        && !matches!(input.operation, Operation::Create | Operation::Replace)
    {
        return Err("memory: owner scope supports only create/replace".into());
    }
    if input.scope != Scope::UserOwner
        && matches!(
            input.operation,
            Operation::Create | Operation::Replace | Operation::Merge
        )
    {
        let polarity = input
            .polarity
            .ok_or("memory: polarity is required for create/replace/merge")?;
        let epistemic = input
            .epistemic_status
            .ok_or("memory: epistemic_status is required for create/replace/merge")?;
        let certainty = input
            .certainty
            .ok_or("memory: certainty is required for create/replace/merge")?;
        if input
            .agent_guidance
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return Err("memory: agent_guidance is required for create/replace/merge".into());
        }
        let needs_avoid = polarity == Polarity::Negative
            || matches!(
                epistemic,
                EpistemicStatus::Inferred | EpistemicStatus::Contested
            )
            || matches!(certainty, Certainty::Low | Certainty::Unknown);
        if needs_avoid && input.avoid_error.as_deref().unwrap_or("").trim().is_empty() {
            return Err("memory: avoid_error is required for negative, inferred, contested, low or unknown entries".into());
        }
        if epistemic == EpistemicStatus::Inferred && certainty == Certainty::High {
            return Err("memory: inferred entries cannot have high certainty".into());
        }
    }
    if input.operation == Operation::Merge {
        let merge_sources = parse_merge_sources(&input.merge_from)?;
        if merge_sources.is_empty() {
            return Err("memory: merge requires --merge-from id@revision".into());
        }
        let target = input.target_id.as_deref().unwrap_or_default();
        let mut ids = HashSet::new();
        for (id, _) in merge_sources {
            if id == target {
                return Err("memory: merge source cannot also be the target".into());
            }
            if !ids.insert(id.clone()) {
                return Err(format!("memory: duplicate merge source: {id}"));
            }
        }
    } else if !input.merge_from.is_empty() {
        return Err("memory: merge_from is valid only for merge".into());
    }
    Ok(())
}

fn parse_merge_sources(values: &[String]) -> Result<Vec<(String, u64)>, String> {
    values
        .iter()
        .map(|value| {
            let (id, revision) = value
                .rsplit_once('@')
                .ok_or_else(|| format!("memory: merge source must be id@revision: {value}"))?;
            if id.trim().is_empty() {
                return Err(format!("memory: merge source id is empty: {value}"));
            }
            let revision = revision
                .parse::<u64>()
                .map_err(|_| format!("memory: invalid merge source revision: {value}"))?;
            Ok((id.to_string(), revision))
        })
        .collect()
}

pub fn propose(root: &Path, input: ProposeInput) -> Result<Proposal, String> {
    validate_propose(&input)?;
    let _guard = lock(root)?;
    let snapshot = list(root)?;
    if snapshot.integrity.managed && snapshot.integrity.drift {
        return Err(format!(
            "memory: projection drift: {}",
            snapshot.integrity.errors.join("; ")
        ));
    }
    if let Some(existing) = snapshot
        .proposals
        .iter()
        .find(|p| p.frontmatter.proposal.dedupe_key == input.dedupe_key)
    {
        let same = existing.frontmatter.proposal.scope == input.scope
            && existing.frontmatter.proposal.operation == input.operation
            && existing.frontmatter.proposal.target_id == input.target_id
            && existing.frontmatter.proposal.base_revision == input.base_revision
            && existing.frontmatter.proposal.section == input.section
            && existing.frontmatter.proposal.merge_from == input.merge_from
            && existing.text.trim() == input.text.trim()
            && existing.frontmatter.proposal.suggested_priority == input.priority
            && existing.frontmatter.proposal.suggested_polarity == input.polarity
            && existing.frontmatter.proposal.suggested_epistemic_status == input.epistemic_status
            && existing.frontmatter.proposal.suggested_certainty == input.certainty
            && existing.frontmatter.proposal.suggested_agent_guidance == input.agent_guidance
            && existing.frontmatter.proposal.suggested_avoid_error == input.avoid_error
            && existing
                .frontmatter
                .sources
                .first()
                .map(|s| s.resource.as_str())
                == Some(input.source.trim());
        return if same {
            Ok(existing.clone())
        } else {
            Err(format!("memory: dedupe_key conflict: {}", input.dedupe_key))
        };
    }
    if let Some(id) = &input.target_id {
        let entry = snapshot
            .entries
            .iter()
            .find(|entry| &entry.id == id)
            .ok_or_else(|| format!("memory: target entry not found: {id}"))?;
        if input.base_revision != Some(entry.revision) {
            return Err(format!(
                "memory: stale base revision for {id}: expected {}, got {:?}",
                entry.revision, input.base_revision
            ));
        }
    }
    if input.operation == Operation::Merge {
        for (id, revision) in parse_merge_sources(&input.merge_from)? {
            let entry = snapshot
                .entries
                .iter()
                .find(|entry| entry.id == id && entry.scope == input.scope)
                .ok_or_else(|| format!("memory: merge source not found in scope: {id}"))?;
            if entry.status != "active" || entry.revision != revision {
                return Err(format!(
                    "memory: stale merge source {id}: expected active revision {revision}, got {} revision {}",
                    entry.status, entry.revision
                ));
            }
        }
    }
    let id = Uuid::new_v4().to_string();
    let created = now();
    let action_sensitive = input.scope == Scope::UserOwner
        || input.priority == Some(Priority::Critical)
        || input.polarity == Some(Polarity::Negative)
        || input.operation == Operation::Delete;
    let fallback_title = format!(
        "{:?} {}",
        input.operation,
        input.target_id.as_deref().unwrap_or("")
    );
    let title_source = if input.text.trim().is_empty() {
        fallback_title.as_str()
    } else {
        input.text.as_str()
    };
    let fm = ProposalFrontmatter {
        kind: "Memory Proposal".into(),
        title: document::title_for(title_source),
        created: created.clone(),
        proposal: ProposalSpec {
            version: PROTOCOL_VERSION,
            id: id.clone(),
            scope: input.scope,
            operation: input.operation,
            target_id: input.target_id,
            base_revision: input.base_revision,
            section: input.section,
            suggested_priority: input.priority,
            suggested_polarity: input.polarity,
            suggested_epistemic_status: input.epistemic_status,
            suggested_certainty: input.certainty,
            suggested_agent_guidance: input.agent_guidance,
            suggested_avoid_error: input.avoid_error,
            dedupe_key: input.dedupe_key,
            action_sensitive,
            merge_from: input.merge_from,
        },
        generated: Generated {
            by: input.by,
            at: created.clone(),
        },
        sources: vec![Source {
            id: "source".into(),
            resource: input.source,
            title: None,
        }],
    };
    let raw = proposal_markdown(&fm, &input.text, &input.reason)?;
    let stamp = Utc::now().format("%Y-%m-%d-%H%M");
    let path = root
        .join(CANDIDATE_DIR)
        .join(format!("{stamp}-{id}.memory-candidate.md"));
    create_new(&path, &raw)?;
    parse_proposal(&path, root, &HashMap::new())
}

fn ensure_owner(user: &str, actor: &str) -> Result<(), String> {
    if !actor.starts_with("human:") {
        return Err("memory: decision actor must be human:<id>".into());
    }
    let configured =
        document::owner_actor(user)?.ok_or("memory: USER.md has no confirmed owner actor")?;
    if configured != actor {
        return Err(format!(
            "memory: actor {actor} is not current owner {configured}"
        ));
    }
    Ok(())
}

fn proposal_by_id(
    root: &Path,
    id: &str,
    decisions: &HashMap<String, ProposalDecision>,
) -> Result<Proposal, String> {
    for path in collect_files(&root.join(CANDIDATE_DIR), ".memory-candidate.md", false)? {
        let proposal = parse_proposal(&path, root, decisions)?;
        if proposal.frontmatter.proposal.id == id {
            return Ok(proposal);
        }
    }
    Err(format!("memory: proposal not found: {id}"))
}

pub fn decide(root: &Path, input: DecideInput) -> Result<serde_json::Value, String> {
    if !input.human_confirmed {
        return Err("memory: explicit human confirmation is required".into());
    }
    let _guard = lock(root)?;
    let (user, memory) = document_texts(root)?;
    let current_integrity = integrity(root, &user, &memory)?;
    if current_integrity.managed && current_integrity.drift {
        return Err(format!(
            "memory: projection drift: {}",
            current_integrity.errors.join("; ")
        ));
    }
    let event_list = events(root)?;
    let decisions = decision_map(&event_list);
    if decisions.contains_key(&input.proposal_id) {
        return Err(format!(
            "memory: proposal already decided: {}",
            input.proposal_id
        ));
    }
    let proposal = proposal_by_id(root, &input.proposal_id, &decisions)?;
    if input.expected_sha256 != proposal.sha256 {
        return Err(format!(
            "memory: proposal content changed: expected {}, current {}",
            input.expected_sha256, proposal.sha256
        ));
    }
    let spec = &proposal.frontmatter.proposal;
    if input.action == DecisionAction::Approve
        && !matches!(spec.operation, Operation::Create)
        && (spec
            .target_id
            .as_deref()
            .is_none_or(|target_id| target_id.trim().is_empty())
            || spec.base_revision.is_none())
    {
        return Err(
            "memory: candidate target_id and base_revision are required for this operation"
                .into(),
        );
    }
    if spec.scope == Scope::UserOwner {
        match document::owner_actor(&user)? {
            Some(current) if current != input.actor => {
                return Err(format!(
                    "memory: actor {} is not current owner {}",
                    input.actor, current
                ))
            }
            Some(_) => {}
            None => {
                let (proposed, _) = document::proposed_owner(&proposal.text)?;
                if proposed != input.actor {
                    return Err(
                        "memory: initial owner claim must be approved by the proposed actor".into(),
                    );
                }
            }
        }
    } else {
        ensure_owner(&user, &input.actor)?;
    }
    let document_name = spec.scope.document();
    let original = if document_name == "USER.md" {
        user
    } else {
        memory
    };
    let mut next = original.clone();
    let entry_id = spec.target_id.clone().unwrap_or_else(|| spec.id.clone());
    let blocks = document::parse_blocks(document_name, &original, spec.scope);
    let target = blocks.iter().find(|block| block.entry.id == entry_id);
    let current_revision = if spec.scope == Scope::UserOwner {
        document::owner_revision(&original)
    } else {
        target.map(|block| block.entry.revision).unwrap_or(0)
    };
    if let Some(base) = spec.base_revision {
        if base != current_revision {
            return Err(format!(
                "memory: proposal conflict for {entry_id}: base {base}, current {current_revision}"
            ));
        }
    }
    let next_revision = current_revision + 1;
    let decided_at = now();

    if input.action == DecisionAction::Approve {
        let priority = spec
            .suggested_priority
            .or_else(|| target.map(|block| block.entry.priority))
            .unwrap_or_default();
        let polarity = spec
            .suggested_polarity
            .or_else(|| target.map(|block| block.entry.polarity))
            .unwrap_or_default();
        let epistemic_status = spec
            .suggested_epistemic_status
            .or_else(|| target.map(|block| block.entry.epistemic_status))
            .unwrap_or_default();
        let certainty = spec
            .suggested_certainty
            .or_else(|| target.map(|block| block.entry.certainty))
            .unwrap_or_default();
        let agent_guidance = spec
            .suggested_agent_guidance
            .as_deref()
            .or_else(|| target.and_then(|block| block.entry.agent_guidance.as_deref()))
            .unwrap_or("Verify the source and ask the owner before relying on this entry.");
        let avoid_error = spec
            .suggested_avoid_error
            .as_deref()
            .or_else(|| target.and_then(|block| block.entry.avoid_error.as_deref()));
        let source = proposal
            .frontmatter
            .sources
            .first()
            .map(|s| s.resource.as_str())
            .unwrap_or("");
        if spec.scope == Scope::UserOwner {
            if !matches!(spec.operation, Operation::Create | Operation::Replace) {
                return Err("memory: owner scope supports only create/replace".into());
            }
            next = document::update_owner(
                &next,
                &entry_id,
                &proposal.text,
                next_revision,
                &spec.id,
                &input.actor,
                &decided_at,
            )?;
        } else {
            match spec.operation {
                Operation::Create if target.is_none() => {
                    next = document::append_entry(
                        &next,
                        spec.section.as_deref().unwrap_or("Active memory"),
                        &entry_id,
                        &proposal.text,
                        next_revision,
                        priority,
                        polarity,
                        epistemic_status,
                        certainty,
                        agent_guidance,
                        avoid_error,
                        &spec.id,
                        &input.actor,
                        &decided_at,
                        source,
                    );
                }
                Operation::Create | Operation::Replace | Operation::Merge => {
                    next = document::update_entry(
                        &next,
                        &entry_id,
                        EntryUpdate {
                            text: Some(&proposal.text),
                            revision: next_revision,
                            status: "active",
                            priority,
                            polarity,
                            epistemic_status,
                            certainty,
                            agent_guidance,
                            avoid_error,
                            proposal: &spec.id,
                            approved_by: &input.actor,
                            approved_at: &decided_at,
                            source: Some(source),
                        },
                    )?;
                }
                Operation::Revoke => {
                    let text = target
                        .ok_or_else(|| format!("memory: target entry not found: {entry_id}"))?
                        .entry
                        .text
                        .clone();
                    next = document::update_entry(
                        &next,
                        &entry_id,
                        EntryUpdate {
                            text: Some(&text),
                            revision: next_revision,
                            status: "revoked",
                            priority,
                            polarity,
                            epistemic_status,
                            certainty,
                            agent_guidance,
                            avoid_error,
                            proposal: &spec.id,
                            approved_by: &input.actor,
                            approved_at: &decided_at,
                            source: Some(source),
                        },
                    )?;
                }
                Operation::Delete => {
                    target.ok_or_else(|| {
                        format!("memory: target entry not found: {entry_id}")
                    })?;
                    next = document::remove_entry(&next, &entry_id)?;
                }
                Operation::SetPriority => {
                    let text = target
                        .ok_or_else(|| format!("memory: target entry not found: {entry_id}"))?
                        .entry
                        .text
                        .clone();
                    next = document::update_entry(
                        &next,
                        &entry_id,
                        EntryUpdate {
                            text: Some(&text),
                            revision: next_revision,
                            status: "active",
                            priority,
                            polarity,
                            epistemic_status,
                            certainty,
                            agent_guidance,
                            avoid_error,
                            proposal: &spec.id,
                            approved_by: &input.actor,
                            approved_at: &decided_at,
                            source: Some(source),
                        },
                    )?;
                }
            }
        }
        if spec.scope != Scope::UserOwner && spec.operation == Operation::Merge {
            for (merged_id, reviewed_revision) in parse_merge_sources(&spec.merge_from)? {
                let merged = document::parse_blocks(document_name, &next, spec.scope)
                    .into_iter()
                    .find(|block| block.entry.id == merged_id)
                    .ok_or_else(|| format!("memory: merge source not found: {merged_id}"))?;
                if merged.entry.status != "active" || merged.entry.revision != reviewed_revision {
                    return Err(format!(
                        "memory: merge source conflict for {merged_id}: reviewed active revision {reviewed_revision}, current {} revision {}",
                        merged.entry.status, merged.entry.revision
                    ));
                }
                next = document::update_entry(
                    &next,
                    &merged_id,
                    EntryUpdate {
                        text: Some(&merged.entry.text),
                        revision: merged.entry.revision + 1,
                        status: "revoked",
                        priority: merged.entry.priority,
                        polarity: merged.entry.polarity,
                        epistemic_status: merged.entry.epistemic_status,
                        certainty: merged.entry.certainty,
                        agent_guidance: merged
                            .entry
                            .agent_guidance
                            .as_deref()
                            .unwrap_or("Do not rely on this revoked merge source."),
                        avoid_error: merged.entry.avoid_error.as_deref(),
                        proposal: &spec.id,
                        approved_by: &input.actor,
                        approved_at: &decided_at,
                        source: merged.entry.source.as_deref(),
                    },
                )?;
            }
        }
    } else if target.map(|block| block.entry.status.as_str()) == Some("pending") {
        next = document::update_entry(
            &next,
            &entry_id,
            EntryUpdate {
                text: target.map(|block| block.entry.text.as_str()),
                revision: 0,
                status: "rejected",
                priority: target.map(|block| block.entry.priority).unwrap_or_default(),
                polarity: target.map(|block| block.entry.polarity).unwrap_or_default(),
                epistemic_status: target
                    .map(|block| block.entry.epistemic_status)
                    .unwrap_or_default(),
                certainty: target
                    .map(|block| block.entry.certainty)
                    .unwrap_or_default(),
                agent_guidance: target
                    .and_then(|block| block.entry.agent_guidance.as_deref())
                    .unwrap_or("Do not rely on this rejected candidate."),
                avoid_error: target.and_then(|block| block.entry.avoid_error.as_deref()),
                proposal: &spec.id,
                approved_by: &input.actor,
                approved_at: &decided_at,
                source: target.and_then(|block| block.entry.source.as_deref()),
            },
        )?;
    }

    // Keep the epistemic contract adjacent to the claim for search snippets and
    // direct readers, regardless of the property's position in an older entry.
    if next != original && spec.scope != Scope::UserOwner {
        next = document::upgrade_classification_defaults(&next);
    }
    let current_state = state(root)?.unwrap_or_default();
    let projection_revision = current_state.revision + 1;
    next = document::stamp_managed(&next, projection_revision)?;
    if next != original {
        atomic_write(&root.join(document_name), &next)?;
    }

    let event_id = Uuid::new_v4().to_string();
    let event = EventFrontmatter {
        kind: "Memory Decision".into(),
        title: format!(
            "{} memory proposal {}",
            if input.action == DecisionAction::Approve {
                "Approve"
            } else {
                "Reject"
            },
            &spec.id[..8]
        ),
        created: decided_at.clone(),
        event: EventSpec {
            version: PROTOCOL_VERSION,
            id: event_id.clone(),
            action: input.action,
            proposal_id: spec.id.clone(),
            proposal_sha256: proposal.sha256.clone(),
            entry_id: entry_id.clone(),
            prior_revision: (current_revision > 0).then_some(current_revision),
            revision: if input.action == DecisionAction::Approve {
                next_revision
            } else {
                current_revision
            },
            decided_by: input.actor.clone(),
            decided_at: decided_at.clone(),
            reason: input.reason,
        },
    };
    let month = Utc::now().format("%Y-%m").to_string();
    let stamp = Utc::now().format("%Y-%m-%d-%H%M%S");
    let event_path = root
        .join(EVENT_DIR)
        .join(month)
        .join(format!("{stamp}-{event_id}.memory-event.md"));
    create_new(&event_path, &event_markdown(&event)?)?;

    let final_user = read_required(&root.join("USER.md"))?;
    let final_memory = read_required(&root.join("MEMORY.md"))?;
    write_state(
        root,
        &ManagedState {
            protocol: PROTOCOL_VERSION,
            revision: projection_revision,
            user_hash: document::projection_hash(&final_user),
            memory_hash: document::projection_hash(&final_memory),
        },
    )?;
    Ok(
        json!({ "proposal_id": spec.id, "event_id": event_id, "entry_id": entry_id, "action": input.action, "revision": next_revision }),
    )
}

pub fn suggest(root: &Path) -> Result<serde_json::Value, String> {
    let snapshot = list(root)?;
    let mut suggestions = Vec::new();
    for entry in &snapshot.entries {
        if entry.legacy {
            suggestions
                .push(json!({"kind":"migrate", "entry_id":entry.id, "document":entry.document}));
        }
        if entry.source.as_deref().unwrap_or("").trim().is_empty() {
            suggestions.push(json!({"kind":"missing-source", "entry_id":entry.id}));
        }
        if entry.text.chars().count() > 500 {
            suggestions.push(json!({"kind":"too-long", "entry_id":entry.id, "characters":entry.text.chars().count()}));
        }
        if !entry.classification_complete {
            suggestions.push(json!({
                "kind":"missing-classification",
                "entry_id":entry.id,
                "document":entry.document,
                "safe_defaults": {
                    "polarity":"neutral",
                    "epistemic_status":"unknown",
                    "certainty":"unknown"
                }
            }));
        }
        if entry.epistemic_status == EpistemicStatus::Inferred && entry.certainty == Certainty::High
        {
            suggestions.push(json!({"kind":"inferred-high", "entry_id":entry.id}));
        }
        let needs_avoid = entry.polarity == Polarity::Negative
            || matches!(
                entry.epistemic_status,
                EpistemicStatus::Inferred | EpistemicStatus::Contested
            )
            || matches!(entry.certainty, Certainty::Low | Certainty::Unknown);
        if needs_avoid && entry.avoid_error.as_deref().unwrap_or("").trim().is_empty() {
            suggestions.push(json!({"kind":"missing-avoid-error", "entry_id":entry.id}));
        }
    }
    for i in 0..snapshot.entries.len() {
        for j in i + 1..snapshot.entries.len() {
            let a = snapshot.entries[i].text.to_lowercase();
            let b = snapshot.entries[j].text.to_lowercase();
            if !a.is_empty() && a == b {
                suggestions.push(json!({"kind":"duplicate", "entry_ids":[snapshot.entries[i].id, snapshot.entries[j].id]}));
            }
        }
    }
    Ok(json!({"suggestions": suggestions, "integrity": snapshot.integrity}))
}

pub fn migrate(root: &Path) -> Result<serde_json::Value, String> {
    let _guard = lock(root)?;
    if let Some(current_state) = state(root)? {
        let (user, memory) = document_texts(root)?;
        let current_integrity = integrity(root, &user, &memory)?;
        if current_integrity.drift {
            return Err(format!(
                "memory: projection drift: {}",
                current_integrity.errors.join("; ")
            ));
        }
        let next_user =
            document::upgrade_classification_defaults(&document::ensure_control_notice(&user));
        let next_memory =
            document::upgrade_classification_defaults(&document::ensure_control_notice(&memory));
        if next_user == user && next_memory == memory {
            return Ok(
                json!({"ok":true,"migrated":0,"already_managed":true,"upgraded":false,"schema":"v2"}),
            );
        }
        let revision = current_state.revision + 1;
        let next_user = document::stamp_managed(&next_user, revision)?;
        let next_memory = document::stamp_managed(&next_memory, revision)?;
        atomic_write(&root.join("USER.md"), &next_user)?;
        atomic_write(&root.join("MEMORY.md"), &next_memory)?;
        write_state(
            root,
            &ManagedState {
                protocol: PROTOCOL_VERSION,
                revision,
                user_hash: document::projection_hash(&next_user),
                memory_hash: document::projection_hash(&next_memory),
            },
        )?;
        return Ok(
            json!({"ok":true,"migrated":0,"already_managed":true,"upgraded":true,"schema":"v2"}),
        );
    }
    let (mut user, mut memory) = document_texts(root)?;
    let mut created = 0usize;

    // The compatibility owner block remains usable until its dedicated,
    // action-sensitive proposal is approved. Migration never fabricates
    // confirmed_by/confirmed_at for an old `confirmed: true` value.
    if let Some(owner_text) = document::owner_proposal_text(&user)? {
        if document::owner_revision(&user) == 0 {
            let proposal_id = Uuid::new_v4().to_string();
            let entry_id = Uuid::new_v4().to_string();
            let created_at = now();
            let fm = ProposalFrontmatter {
                kind: "Memory Proposal".into(),
                title: "Confirm vault owner".into(),
                created: created_at.clone(),
                proposal: ProposalSpec {
                    version: PROTOCOL_VERSION,
                    id: proposal_id.clone(),
                    scope: Scope::UserOwner,
                    operation: Operation::Create,
                    target_id: Some(entry_id),
                    base_revision: Some(0),
                    section: Some("Owner".into()),
                    suggested_priority: Some(Priority::High),
                    suggested_polarity: None,
                    suggested_epistemic_status: None,
                    suggested_certainty: None,
                    suggested_agent_guidance: None,
                    suggested_avoid_error: None,
                    dedupe_key: "memory-migrate/v1/USER.md/owner".into(),
                    action_sensitive: true,
                    merge_from: Vec::new(),
                },
                generated: Generated {
                    by: "migration/notemd-memory-1".into(),
                    at: created_at,
                },
                sources: vec![Source {
                    id: "legacy-owner".into(),
                    resource: "/USER.md#owner".into(),
                    title: Some("v1 owner block".into()),
                }],
            };
            let raw = proposal_markdown(&fm, &owner_text, "The legacy owner remains compatible, but this exact owner block needs a dedicated confirmation event.")?;
            let stamp = Utc::now().format("%Y-%m-%d-%H%M");
            create_new(
                &root
                    .join(CANDIDATE_DIR)
                    .join(format!("{stamp}-{proposal_id}.memory-candidate.md")),
                &raw,
            )?;
            created += 1;
        }
    }

    for (name, scope, text) in [
        ("USER.md", Scope::UserProfile, &mut user),
        ("MEMORY.md", Scope::Memory, &mut memory),
    ] {
        let blocks = document::parse_blocks(name, text, scope);
        for block in blocks.into_iter().rev() {
            if !block.entry.legacy {
                continue;
            }
            let entry_id = if block.entry.id.is_empty() {
                Uuid::new_v4().to_string()
            } else {
                block.entry.id.clone()
            };
            let proposal_id = Uuid::new_v4().to_string();
            let created_at = now();
            let source = block
                .entry
                .source
                .clone()
                .unwrap_or_else(|| format!("/{name}"));
            let fm = ProposalFrontmatter {
                kind: "Memory Proposal".into(),
                title: document::title_for(&block.entry.text),
                created: created_at.clone(),
                proposal: ProposalSpec {
                    version: PROTOCOL_VERSION,
                    id: proposal_id.clone(),
                    scope,
                    operation: Operation::Create,
                    target_id: Some(entry_id.clone()),
                    base_revision: Some(0),
                    section: Some(block.entry.section.clone()),
                    suggested_priority: Some(block.entry.priority),
                    suggested_polarity: None,
                    suggested_epistemic_status: None,
                    suggested_certainty: None,
                    suggested_agent_guidance: None,
                    suggested_avoid_error: None,
                    dedupe_key: format!(
                        "memory-migrate/v1/{name}/{}",
                        document::entry_hash(&block.entry)
                    ),
                    action_sensitive: false,
                    merge_from: Vec::new(),
                },
                generated: Generated {
                    by: "migration/notemd-memory-1".into(),
                    at: created_at,
                },
                sources: vec![Source {
                    id: "legacy-source".into(),
                    resource: source,
                    title: Some(format!("v1 {name} entry")),
                }],
            };
            let raw = proposal_markdown(
                &fm,
                &block.entry.text,
                "Imported from the unreviewed v1 document; human approval is still required.",
            )?;
            let stamp = Utc::now().format("%Y-%m-%d-%H%M");
            let path = root
                .join(CANDIDATE_DIR)
                .join(format!("{stamp}-{proposal_id}.memory-candidate.md"));
            create_new(&path, &raw)?;
            *text = document::mark_pending(text, block.start, block.end, &entry_id, &proposal_id);
            created += 1;
        }
    }
    user = document::stamp_managed(
        &document::upgrade_classification_defaults(&document::ensure_control_notice(&user)),
        1,
    )?;
    memory = document::stamp_managed(
        &document::upgrade_classification_defaults(&document::ensure_control_notice(&memory)),
        1,
    )?;
    atomic_write(&root.join("USER.md"), &user)?;
    atomic_write(&root.join("MEMORY.md"), &memory)?;
    write_state(
        root,
        &ManagedState {
            protocol: PROTOCOL_VERSION,
            revision: 1,
            user_hash: document::projection_hash(&user),
            memory_hash: document::projection_hash(&memory),
        },
    )?;
    Ok(json!({"ok":true,"migrated":created,"already_managed":false,"schema":"v2"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn vault() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("USER.md"), "---\ntype: User Profile\nowner:\n  actor: human:bruce\n  names: [Bruce]\n  confirmed: true\n---\n\n## Stable profile\n\n- Likes precise sources\n  source:: /daily.md#L1\n  updated:: 2026-09-01T00:00:00Z\n  by:: codex/x\n").unwrap();
        fs::write(dir.path().join("MEMORY.md"), "---\ntype: Memory\n---\n\n## Active memory\n\n- A durable fact\n  id:: 11111111-1111-4111-8111-111111111111\n  source:: /a.md#L1\n  recorded:: 2026-01-01T00:00:00Z\n  by:: codex/x\n").unwrap();
        dir
    }

    #[test]
    fn migration_is_idempotent_and_keeps_entries_pending() {
        let v = vault();
        let first = migrate(v.path()).unwrap();
        assert_eq!(first["migrated"], 3);
        let second = migrate(v.path()).unwrap();
        assert_eq!(second["migrated"], 0);
        assert_eq!(second["upgraded"], false);
        let snapshot = list(v.path()).unwrap();
        assert_eq!(snapshot.entries.len(), 3);
        assert!(snapshot
            .entries
            .iter()
            .filter(|e| e.scope != Scope::UserOwner)
            .all(|e| e.status == "pending"));
        assert_eq!(snapshot.proposals.len(), 3);
        assert!(!snapshot.integrity.drift, "{:?}", snapshot.integrity.errors);
        for name in ["USER.md", "MEMORY.md"] {
            let content = fs::read_to_string(v.path().join(name)).unwrap();
            assert!(content.contains(document::CONTROL_NOTICE_MARKER));
            assert_eq!(content.matches(document::CONTROL_NOTICE_MARKER).count(), 1);
        }
    }

    #[test]
    fn agent_proposal_requires_human_decision_before_projection_changes() {
        let v = vault();
        migrate(v.path()).unwrap();
        let before = fs::read_to_string(v.path().join("MEMORY.md")).unwrap();
        let proposal = propose(
            v.path(),
            ProposeInput {
                scope: Scope::Memory,
                operation: Operation::Create,
                text: "A new fact".into(),
                source: "/b.md#L2".into(),
                by: "codex/gpt-5".into(),
                dedupe_key: "test/new-fact".into(),
                reason: "test".into(),
                target_id: None,
                base_revision: None,
                section: Some("Active memory".into()),
                priority: Some(Priority::High),
                polarity: Some(Polarity::Positive),
                epistemic_status: Some(EpistemicStatus::SourceSupported),
                certainty: Some(Certainty::High),
                agent_guidance: Some("Use this durable fact when relevant.".into()),
                avoid_error: None,
                merge_from: vec![],
            },
        )
        .unwrap();
        let candidate_path = v.path().join(&proposal.path);
        assert_eq!(
            before,
            fs::read_to_string(v.path().join("MEMORY.md")).unwrap()
        );
        decide(
            v.path(),
            DecideInput {
                proposal_id: proposal.frontmatter.proposal.id,
                expected_sha256: proposal.sha256,
                action: DecisionAction::Approve,
                actor: "human:bruce".into(),
                human_confirmed: true,
                reason: None,
            },
        )
        .unwrap();
        let after = fs::read_to_string(v.path().join("MEMORY.md")).unwrap();
        assert!(after.contains("A new fact"));
        assert!(after.contains("priority:: high"));
        let mut tampered = fs::read_to_string(&candidate_path).unwrap();
        tampered.push_str("\nmodified after decision\n");
        fs::write(candidate_path, tampered).unwrap();
        let snapshot = list(v.path()).unwrap();
        assert!(snapshot.integrity.drift);
        assert!(snapshot
            .integrity
            .errors
            .iter()
            .any(|error| error.contains("decided proposal hash mismatch")));
    }

    #[test]
    fn decision_rejects_a_candidate_changed_after_review() {
        let v = vault();
        migrate(v.path()).unwrap();
        let proposal = propose(
            v.path(),
            ProposeInput {
                scope: Scope::Memory,
                operation: Operation::Create,
                text: "Reviewed text".into(),
                source: "/b.md#L2".into(),
                by: "codex/gpt-5".into(),
                dedupe_key: "test/review-hash".into(),
                reason: "test".into(),
                target_id: None,
                base_revision: None,
                section: Some("Active memory".into()),
                priority: None,
                polarity: Some(Polarity::Neutral),
                epistemic_status: Some(EpistemicStatus::SourceSupported),
                certainty: Some(Certainty::Medium),
                agent_guidance: Some("Use only in the source context.".into()),
                avoid_error: None,
                merge_from: vec![],
            },
        )
        .unwrap();
        let candidate_path = v.path().join(&proposal.path);
        let mut changed = fs::read_to_string(&candidate_path).unwrap();
        changed.push_str("\nchanged after display\n");
        fs::write(candidate_path, changed).unwrap();
        let error = decide(
            v.path(),
            DecideInput {
                proposal_id: proposal.frontmatter.proposal.id,
                expected_sha256: proposal.sha256,
                action: DecisionAction::Approve,
                actor: "human:bruce".into(),
                human_confirmed: true,
                reason: None,
            },
        )
        .unwrap_err();
        assert!(error.contains("proposal content changed"), "{error}");
        assert!(!fs::read_to_string(v.path().join("MEMORY.md"))
            .unwrap()
            .contains("Reviewed text"));
    }

    #[test]
    fn initial_owner_claim_must_be_approved_by_the_proposed_actor() {
        let v = vault();
        fs::write(
            v.path().join("USER.md"),
            "---\ntype: User Profile\nowner:\n  actor: null\n  names: []\n  confirmed: false\n---\n\n# User\n",
        )
        .unwrap();
        migrate(v.path()).unwrap();
        let proposal = propose(
            v.path(),
            ProposeInput {
                scope: Scope::UserOwner,
                operation: Operation::Create,
                text: "{\"actor\":\"human:bruce\",\"names\":[\"Bruce\"]}".into(),
                source: "human-input://owner-claim".into(),
                by: "notemd-memory/human-ui".into(),
                dedupe_key: "test/owner-claim".into(),
                reason: "claim".into(),
                target_id: None,
                base_revision: None,
                section: Some("Owner".into()),
                priority: Some(Priority::High),
                polarity: None,
                epistemic_status: None,
                certainty: None,
                agent_guidance: None,
                avoid_error: None,
                merge_from: vec![],
            },
        )
        .unwrap();
        decide(
            v.path(),
            DecideInput {
                proposal_id: proposal.frontmatter.proposal.id,
                expected_sha256: proposal.sha256,
                action: DecisionAction::Approve,
                actor: "human:bruce".into(),
                human_confirmed: true,
                reason: None,
            },
        )
        .unwrap();
        let user = fs::read_to_string(v.path().join("USER.md")).unwrap();
        assert_eq!(
            document::owner_actor(&user).unwrap().as_deref(),
            Some("human:bruce")
        );
        assert!(user.contains("confirmed_by: human:bruce"));
        assert_eq!(document::owner_revision(&user), 1);
    }

    #[test]
    fn direct_edit_causes_drift_and_blocks_propose() {
        let v = vault();
        migrate(v.path()).unwrap();
        let path = v.path().join("MEMORY.md");
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"\nmanual edit\n")
            .unwrap();
        let snapshot = list(v.path()).unwrap();
        assert!(snapshot.integrity.drift);
        let err = propose(
            v.path(),
            ProposeInput {
                scope: Scope::Memory,
                operation: Operation::Create,
                text: "x".into(),
                source: "/x".into(),
                by: "agent/x".into(),
                dedupe_key: "x".into(),
                reason: String::new(),
                target_id: None,
                base_revision: None,
                section: None,
                priority: None,
                polarity: Some(Polarity::Neutral),
                epistemic_status: Some(EpistemicStatus::Unknown),
                certainty: Some(Certainty::Unknown),
                agent_guidance: Some("Verify before use.".into()),
                avoid_error: Some("Do not treat as confirmed.".into()),
                merge_from: vec![],
            },
        )
        .unwrap_err();
        assert!(err.contains("drift"));
    }

    #[test]
    fn update_operations_reject_an_empty_target_id() {
        let v = vault();
        migrate(v.path()).unwrap();
        let error = propose(
            v.path(),
            ProposeInput {
                scope: Scope::Memory,
                operation: Operation::SetPriority,
                text: String::new(),
                source: "/MEMORY.md".into(),
                by: "agent/x".into(),
                dedupe_key: "test/empty-target".into(),
                reason: "test".into(),
                target_id: Some("   ".into()),
                base_revision: Some(1),
                section: None,
                priority: Some(Priority::High),
                polarity: None,
                epistemic_status: None,
                certainty: None,
                agent_guidance: None,
                avoid_error: None,
                merge_from: vec![],
            },
        )
        .unwrap_err();

        assert!(error.contains("target_id and base_revision are required"));
    }

    #[test]
    fn approved_delete_removes_projection_but_preserves_audit_history() {
        let v = vault();
        migrate(v.path()).unwrap();
        let migrated = list(v.path())
            .unwrap()
            .proposals
            .into_iter()
            .find(|proposal| {
                proposal.frontmatter.proposal.scope == Scope::Memory
                    && proposal.frontmatter.proposal.target_id.as_deref()
                        == Some("11111111-1111-4111-8111-111111111111")
            })
            .unwrap();
        decide(
            v.path(),
            DecideInput {
                proposal_id: migrated.frontmatter.proposal.id,
                expected_sha256: migrated.sha256,
                action: DecisionAction::Approve,
                actor: "human:bruce".into(),
                human_confirmed: true,
                reason: None,
            },
        )
        .unwrap();

        let entry = list(v.path())
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.id == "11111111-1111-4111-8111-111111111111")
            .unwrap();
        let before = fs::read_to_string(v.path().join("MEMORY.md")).unwrap();
        let proposal = propose(
            v.path(),
            ProposeInput {
                scope: Scope::Memory,
                operation: Operation::Delete,
                text: String::new(),
                source: "human-input://human:bruce/delete".into(),
                by: "notemd-memory/human-ui".into(),
                dedupe_key: "test/delete-entry".into(),
                reason: "owner requests projection removal".into(),
                target_id: Some(entry.id.clone()),
                base_revision: Some(entry.revision),
                section: None,
                priority: None,
                polarity: None,
                epistemic_status: None,
                certainty: None,
                agent_guidance: None,
                avoid_error: None,
                merge_from: vec![],
            },
        )
        .unwrap();
        let candidate_path = v.path().join(&proposal.path);
        let candidate_before = fs::read_to_string(&candidate_path).unwrap();
        assert!(proposal.frontmatter.proposal.action_sensitive);
        assert_eq!(before, fs::read_to_string(v.path().join("MEMORY.md")).unwrap());

        decide(
            v.path(),
            DecideInput {
                proposal_id: proposal.frontmatter.proposal.id.clone(),
                expected_sha256: proposal.sha256.clone(),
                action: DecisionAction::Approve,
                actor: "human:bruce".into(),
                human_confirmed: true,
                reason: Some("confirmed projection removal".into()),
            },
        )
        .unwrap();

        let after = fs::read_to_string(v.path().join("MEMORY.md")).unwrap();
        assert!(!after.contains(&entry.id));
        assert_eq!(candidate_before, fs::read_to_string(candidate_path).unwrap());
        let snapshot = list(v.path()).unwrap();
        assert!(!snapshot.entries.iter().any(|item| item.id == entry.id));
        assert_eq!(
            snapshot
                .proposals
                .iter()
                .find(|item| item.frontmatter.proposal.id == proposal.frontmatter.proposal.id)
                .unwrap()
                .decision,
            ProposalDecision::Approved
        );
        assert!(!snapshot.integrity.drift, "{:?}", snapshot.integrity.errors);
        let event = events(v.path())
            .unwrap()
            .into_iter()
            .find(|event| event.event.proposal_id == proposal.frontmatter.proposal.id)
            .unwrap();
        assert_eq!(event.event.entry_id, entry.id);
        assert_eq!(event.event.prior_revision, Some(entry.revision));
        assert_eq!(event.event.decided_by, "human:bruce");
    }

    #[test]
    fn delete_is_forbidden_for_owner_identity_scope() {
        let v = vault();
        migrate(v.path()).unwrap();
        let error = propose(
            v.path(),
            ProposeInput {
                scope: Scope::UserOwner,
                operation: Operation::Delete,
                text: String::new(),
                source: "human-input://human:bruce/delete".into(),
                by: "notemd-memory/human-ui".into(),
                dedupe_key: "test/delete-owner".into(),
                reason: "must not delete owner via fact shortcut".into(),
                target_id: Some("owner".into()),
                base_revision: Some(0),
                section: None,
                priority: None,
                polarity: None,
                epistemic_status: None,
                certainty: None,
                agent_guidance: None,
                avoid_error: None,
                merge_from: vec![],
            },
        )
        .unwrap_err();

        assert!(error.contains("owner scope supports only create/replace"));
    }

    #[test]
    fn decision_rejects_a_handwritten_delete_without_base_revision() {
        let v = vault();
        migrate(v.path()).unwrap();
        let entry = list(v.path())
            .unwrap()
            .entries
            .into_iter()
            .find(|entry| entry.scope == Scope::Memory)
            .unwrap();
        let proposal = propose(
            v.path(),
            ProposeInput {
                scope: Scope::Memory,
                operation: Operation::Delete,
                text: String::new(),
                source: "human-input://human:bruce/delete".into(),
                by: "notemd-memory/human-ui".into(),
                dedupe_key: "test/handwritten-delete".into(),
                reason: "test missing base defense".into(),
                target_id: Some(entry.id.clone()),
                base_revision: Some(entry.revision),
                section: None,
                priority: None,
                polarity: None,
                epistemic_status: None,
                certainty: None,
                agent_guidance: None,
                avoid_error: None,
                merge_from: vec![],
            },
        )
        .unwrap();
        let path = v.path().join(&proposal.path);
        let raw = fs::read_to_string(&path).unwrap();
        let base_line = format!("  base_revision: {}\n", entry.revision);
        assert!(raw.contains(&base_line));
        fs::write(&path, raw.replacen(&base_line, "", 1)).unwrap();
        let changed = list(v.path())
            .unwrap()
            .proposals
            .into_iter()
            .find(|item| item.frontmatter.proposal.id == proposal.frontmatter.proposal.id)
            .unwrap();
        let before = fs::read_to_string(v.path().join("MEMORY.md")).unwrap();

        let error = decide(
            v.path(),
            DecideInput {
                proposal_id: changed.frontmatter.proposal.id,
                expected_sha256: changed.sha256,
                action: DecisionAction::Approve,
                actor: "human:bruce".into(),
                human_confirmed: true,
                reason: None,
            },
        )
        .unwrap_err();

        assert!(error.contains("candidate target_id and base_revision are required"));
        assert_eq!(before, fs::read_to_string(v.path().join("MEMORY.md")).unwrap());
    }

    #[test]
    fn merge_sources_bind_ids_to_exact_revisions() {
        assert_eq!(
            parse_merge_sources(&["entry-a@3".into(), "entry-b@7".into()]).unwrap(),
            vec![("entry-a".into(), 3), ("entry-b".into(), 7)]
        );
        assert!(parse_merge_sources(&["entry-a".into()])
            .unwrap_err()
            .contains("id@revision"));
        assert!(parse_merge_sources(&["entry-a@bad".into()])
            .unwrap_err()
            .contains("invalid merge source revision"));
    }
}
