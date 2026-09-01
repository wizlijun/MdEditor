//! Trusted Host RPC adapter for Memory Protocol v2.
//!
//! The official Memory plugin is the human gesture boundary. Request bodies
//! never carry an actor or capability: both are derived from the current,
//! uniquely reduced authority revision and bound into the immutable child.

use super::*;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const CLAIM_APPROVE: &str = "memory.claim.approve";
const CLAIM_RESOLVE: &str = "memory.claim.resolve";
const AUTHORITY_SCOPE: &str = "personal-assistant";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotInput {
    #[serde(default = "now")]
    as_of_valid_time: String,
    #[serde(default)]
    space: Option<String>,
    #[serde(default)]
    purpose: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerSubjectInput {
    kind: SubjectKind,
    id: String,
    relation_to_owner: OwnerRelation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddInput {
    request_id: String,
    expected_protocol: RevisionRef,
    target: ProjectionTarget,
    category: String,
    text: String,
    claim_kind: ClaimKind,
    subject: OwnerSubjectInput,
    approval_kind: ApprovalKind,
    trust_tier: TrustTier,
    risk_class: RiskClass,
    salience: Salience,
    polarity: Polarity,
    sensitivity: Sensitivity,
    context: ClaimContext,
    consent: Consent,
    agent_use: AgentUse,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingDecisionInput {
    request_id: String,
    expected_protocol: RevisionRef,
    #[serde(default)]
    expected_heads: Vec<RevisionRef>,
    revision_id: String,
    expected_sha256: String,
    gesture_intent: GestureIntent,
    #[serde(default)]
    salience_override: Option<Salience>,
    #[serde(default)]
    delete_kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClaimMutationInput {
    request_id: String,
    expected_protocol: RevisionRef,
    claim_id: String,
    #[serde(default)]
    expected_heads: Vec<RevisionRef>,
    #[serde(default)]
    salience: Option<Salience>,
    #[serde(default)]
    delete_kind: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ResolveStrategy {
    KeepHead,
    Merge,
    RevokeAll,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolveInput {
    request_id: String,
    expected_protocol: RevisionRef,
    conflict_id: String,
    claim_id: String,
    #[serde(default)]
    expected_heads: Vec<RevisionRef>,
    strategy: ResolveStrategy,
    #[serde(default)]
    selected_revision_id: Option<String>,
    #[serde(default)]
    merged_text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MigrationInput {
    mode: String,
}

#[derive(Debug, Serialize)]
struct WriteReceipt {
    claim_id: String,
    revision_id: String,
    payload_sha256: String,
    effective_status: String,
    conflict: bool,
    projection_rebuilt: bool,
}

#[derive(Debug, Clone)]
pub struct PendingProposalInput {
    pub request_id: String,
    pub claim_id: Option<String>,
    pub text: String,
    pub claim_kind: ClaimKind,
    pub kind_data: KindData,
    pub subject: Subject,
    pub asserted_by: Vec<Assertion>,
    pub recorded_by: Recorder,
    pub projection: Projection,
    pub lifecycle: LifecycleState,
    pub temporal: Temporal,
    pub epistemic: Epistemic,
    pub trust_tier: TrustTier,
    pub risk_class: RiskClass,
    pub salience: Salience,
    pub polarity: Polarity,
    pub sensitivity: Sensitivity,
    pub context: ClaimContext,
    pub consent: Consent,
    pub agent_use: AgentUse,
    pub evidence: Vec<Evidence>,
    pub dedupe_key: String,
}

pub fn dispatch(root: &Path, method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "host.memory.v2.snapshot" => {
            let input: SnapshotInput = parse(params, "snapshot")?;
            snapshot_view(root, input)
        }
        "host.memory.v2.add" => add(root, parse(params, "add")?),
        "host.memory.v2.approve" => {
            decide_pending(root, parse(params, "approve")?, GestureIntent::Approve)
        }
        "host.memory.v2.reject" => {
            decide_pending(root, parse(params, "reject")?, GestureIntent::Reject)
        }
        "host.memory.v2.ignore" => {
            decide_pending(root, parse(params, "ignore")?, GestureIntent::Ignore)
        }
        "host.memory.v2.delete" => delete(root, params),
        "host.memory.v2.setSalience" => set_salience(root, parse(params, "setSalience")?),
        "host.memory.v2.resolve" => resolve(root, parse(params, "resolve")?),
        "host.memory.v2.context" => context_preview(root, parse(params, "context")?),
        "host.memory.v2.contextManifest" => {
            context_manifest(root, parse(params, "contextManifest")?)
        }
        "host.memory.v2.check" => check(root),
        "host.memory.v2.migrate" => migration_dry_run(root, parse(params, "migrate")?),
        _ => Err(format!("memory: unknown method {method}")),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(params: &Value, operation: &str) -> Result<T, String> {
    serde_json::from_value(params.clone())
        .map_err(|error| format!("MEMORY_INVALID_REQUEST: {operation}: {error}"))
}

fn snapshot_view(root: &Path, input: SnapshotInput) -> Result<Value, String> {
    let repository = match V2Repository::new(root).load() {
        Ok(repository) => repository,
        Err(error) => return Ok(recovery_view(error.to_string())),
    };
    match repository.mode {
        RepositoryMode::Absent => Ok(non_v2_view(
            "legacy",
            "尚未初始化 Memory Protocol v2",
            false,
        )),
        RepositoryMode::LegacyV1 => Ok(non_v2_view(
            "legacy",
            "发现旧版记忆资产；请先查看迁移 dry-run",
            true,
        )),
        RepositoryMode::V2Incomplete => Ok(recovery_view(
            repository
                .diagnostics
                .join("；")
                .trim()
                .to_string()
                .if_empty("v2 控制资产不完整"),
        )),
        RepositoryMode::V2Active => rich_v2_view(root, &repository, input),
    }
}

trait EmptyFallback {
    fn if_empty(self, fallback: &str) -> String;
}

impl EmptyFallback for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() {
            fallback.to_string()
        } else {
            self
        }
    }
}

fn non_v2_view(mode: &str, message: &str, migration_required: bool) -> Value {
    json!({
        "mode": mode,
        "migration_required": migration_required,
        "read_only_reason": message,
        "claims": [], "pending": [], "conflicts": [], "history": [],
        "health": {
            "status": "attention", "message": message,
            "pending_count": 0, "conflict_count": 0, "integrity_errors": []
        }
    })
}

fn recovery_view(message: String) -> Value {
    json!({
        "mode": "recovery", "read_only_reason": message,
        "claims": [], "pending": [], "conflicts": [], "history": [],
        "health": {
            "status": "damaged", "message": "Memory 控制资产需要恢复",
            "pending_count": 0, "conflict_count": 0, "integrity_errors": [message]
        }
    })
}

fn rich_v2_view(
    root: &Path,
    repository: &RepositorySnapshot,
    input: SnapshotInput,
) -> Result<Value, String> {
    let snapshot = reduce(
        repository,
        &SnapshotRequest {
            as_of_valid_time: input.as_of_valid_time,
            space: input.space,
            purpose: input.purpose,
        },
    )
    .map_err(|error| error.to_string())?;
    let protocol = unique_protocol(&snapshot)?;
    let owner = snapshot
        .authority
        .owner
        .as_ref()
        .ok_or("MEMORY_UNAUTHORIZED: authority owner is not unique")?;
    let by_revision = repository
        .claims
        .iter()
        .map(|item| (item.value.revision_id.as_str(), &item.value))
        .collect::<BTreeMap<_, _>>();

    let mut claims = Vec::new();
    let mut conflicts = Vec::new();
    for view in &snapshot.claims {
        if let Some(conflict) = &view.conflict {
            let heads = conflict
                .heads
                .iter()
                .filter_map(|head| by_revision.get(head.revision_id.as_str()).copied())
                .collect::<Vec<_>>();
            conflicts.push(json!({
                "conflict_id": conflict.conflict_id,
                "claim_id": view.claim_id,
                "risk_class": conflict.risk_class,
                "action_allowed": conflict.action_allowed,
                "heads": heads,
                "reasons": ["concurrent-approved-heads", "do-not-rely"]
            }));
        }
        if view.current_heads.len() == 1 {
            if let Some(revision) = by_revision.get(view.current_heads[0].revision_id.as_str()) {
                claims.push(json!({
                    "claim": revision,
                    "application_state": view.application_state,
                    "do_not_rely": view.do_not_rely,
                    "reasons": if view.do_not_rely { vec!["do-not-rely"] } else { Vec::<&str>::new() }
                }));
            }
        }
    }

    let decided_pending = repository
        .claims
        .iter()
        .filter_map(|item| item.value.transition.approves_revision_id.as_deref())
        .collect::<BTreeSet<_>>();
    let pending = repository
        .claims
        .iter()
        .filter(|item| {
            item.value.workflow.state == WorkflowState::Pending
                && !decided_pending.contains(item.value.revision_id.as_str())
        })
        .map(|item| {
            json!({
                "revision": item.value,
                "expected_sha256": item.value.payload_sha256,
                "expected_heads": item.value.parents,
                "required_approval_kind": approval_kind_for(item.value.claim_kind),
                "source_summary": item.value.evidence.first().map(|source| source.resource.clone())
            })
        })
        .collect::<Vec<_>>();

    let history = repository
        .claims
        .iter()
        .map(|item| {
            json!({
                "id": item.value.request_id,
                "claim_id": item.value.claim_id,
                "revision_id": item.value.revision_id,
                "operation": item.value.transition.operation,
                "workflow_state": item.value.workflow.state,
                "lifecycle_state": item.value.lifecycle.state,
                "actor_id": item.value.decision.as_ref().map(|decision| decision.actor_id.clone()),
                "approval_kind": item.value.decision.as_ref().map(|decision| decision.approval_kind),
                "recorded_at": item.value.recorded_at,
                "summary": item.value.text.lines().next().unwrap_or("")
            })
        })
        .collect::<Vec<_>>();
    let projection_edited = projection_is_edited(repository, &snapshot, root);
    let integrity_errors = snapshot.diagnostics.clone();
    let status = if !conflicts.is_empty() {
        "conflict"
    } else if !integrity_errors.is_empty() {
        "damaged"
    } else if !pending.is_empty() || projection_edited {
        "attention"
    } else {
        "healthy"
    };
    let message = if !conflicts.is_empty() {
        "存在未解决的并发主张冲突"
    } else if !pending.is_empty() {
        "存在待确认的记忆建议"
    } else if projection_edited {
        "纯文本投影与权威资产不一致"
    } else {
        "Memory Protocol v2 状态正常"
    };
    Ok(json!({
        "mode": if snapshot.protocol.writable && snapshot.authority.action_allowed { "v2" } else { "read-only" },
        "read_only_reason": if snapshot.action_allowed { Value::Null } else { json!("协议、authority 或安全冲突已暂停行动") },
        "protocol": protocol,
        "owner": {
            "actor_id": owner.actor_id,
            "subject": { "kind": "vault-owner", "id": owner.owner_id, "relation_to_owner": "self", "label": owner.owner_id }
        },
        "claims": claims, "pending": pending, "conflicts": conflicts, "history": history,
        "health": {
            "status": status, "message": message,
            "pending_count": pending.len(), "conflict_count": conflicts.len(),
            "integrity_errors": integrity_errors, "projection_edited": projection_edited
        },
        "context_options": context_options(repository)
    }))
}

fn projection_is_edited(
    repository: &RepositorySnapshot,
    snapshot: &MemorySnapshotV2,
    root: &Path,
) -> bool {
    let Ok(expected) = project(repository, snapshot) else {
        return true;
    };
    fs::read_to_string(root.join("USER.md")).ok().as_deref() != Some(expected.user.as_str())
        || fs::read_to_string(root.join("MEMORY.md")).ok().as_deref()
            != Some(expected.memory.as_str())
}

fn context_options(repository: &RepositorySnapshot) -> Value {
    let mut spaces = BTreeSet::new();
    let mut purposes = BTreeSet::new();
    for revision in &repository.claims {
        spaces.extend(revision.value.context.spaces.iter().cloned());
        purposes.extend(revision.value.consent.allowed_purposes.iter().cloned());
    }
    if spaces.is_empty() {
        spaces.insert("global".into());
    }
    if purposes.is_empty() {
        purposes.extend(["planning".into(), "writing".into()]);
    }
    json!({
        "spaces": spaces.into_iter().map(|id| json!({"id": id, "label": id})).collect::<Vec<_>>(),
        "purposes": purposes.into_iter().map(|id| json!({"id": id, "label": id})).collect::<Vec<_>>(),
        "providers": [{"id": "local", "label": "本机"}, {"id": "openai", "label": "OpenAI"}],
        "models": [{"id": "local", "label": "本机", "provider_id": "local"}, {"id": "gpt-5", "label": "GPT-5", "provider_id": "openai"}]
    })
}

fn add(root: &Path, input: AddInput) -> Result<Value, String> {
    validate_request_id(&input.request_id)?;
    let (repository, snapshot) = active(root, &input.expected_protocol)?;
    if let Some(existing) = idempotent(&repository, &input.request_id) {
        if existing.value.transition.operation != ClaimOperation::CreateApproved
            || existing.value.text != input.text.trim()
            || existing.value.claim_kind != input.claim_kind
            || existing.value.projection.target != input.target
            || existing.value.projection.category != input.category
        {
            return Err(
                "MEMORY_IDEMPOTENCY_CONFLICT: request_id was reused for different add input".into(),
            );
        }
        return receipt_for(existing, &snapshot, true);
    }
    let controls = controls(&repository, &snapshot, CLAIM_APPROVE)?;
    validate_add(&input, &controls.owner)?;
    let recorded_at = now();
    let kind_data = kind_data_for(
        input.claim_kind,
        &input.text,
        &input.category,
        &recorded_at,
        input.polarity,
    );
    let temporal = temporal_for(input.claim_kind, &recorded_at);
    let claim_id = uuid_v7();
    let revision = MemoryClaimRevision {
        schema: "notemd.memory/claim-revision/v2".into(),
        claim_id: claim_id.clone(),
        revision_id: uuid_v7(),
        request_id: input.request_id,
        parents: vec![],
        causal_context: controls.causal_context.clone(),
        claim_kind: input.claim_kind,
        kind_data,
        subject: Subject {
            kind: SubjectKind::VaultOwner,
            id: controls.owner.owner_id.clone(),
            relation_to_owner: OwnerRelation::Self_,
        },
        asserted_by: vec![Assertion {
            kind: "owner".into(),
            id: controls.owner.owner_id.clone(),
            basis: "direct-ui-entry".into(),
        }],
        recorded_by: Recorder {
            kind: "host".into(),
            id: "notemd.memory-ui".into(),
            device_id: "device:official-memory-ui".into(),
        },
        recorded_at: recorded_at.clone(),
        text: input.text.trim().to_string(),
        projection: Projection {
            target: input.target,
            category: input.category,
            visibility: Visibility::Projection,
        },
        workflow: Workflow {
            state: WorkflowState::Approved,
        },
        lifecycle: Lifecycle {
            state: LifecycleState::Active,
        },
        temporal,
        epistemic: epistemic_for(input.approval_kind),
        trust_tier: input.trust_tier,
        risk_class: input.risk_class,
        salience: input.salience,
        polarity: input.polarity,
        sensitivity: input.sensitivity,
        context: input.context,
        consent: input.consent,
        agent_use: input.agent_use,
        decision: Some(decision(
            &controls,
            input.approval_kind,
            CLAIM_APPROVE,
            recorded_at,
        )),
        transition: ClaimTransition {
            operation: ClaimOperation::CreateApproved,
            approves_revision_id: None,
            approves_payload_sha256: None,
        },
        evidence: vec![],
        lineage: Lineage::default(),
        dedupe_key: format!("human:{}", digest(input.text.trim().as_bytes())),
        payload_sha256: String::new(),
    };
    publish_and_verify(root, revision)
}

fn validate_add(input: &AddInput, owner: &AuthorityOwner) -> Result<(), String> {
    if input.text.trim().is_empty() || input.text.len() > 32_768 {
        return Err("MEMORY_INVALID_CLAIM: text must be non-empty and at most 32768 bytes".into());
    }
    if input.subject.kind != SubjectKind::VaultOwner
        || input.subject.relation_to_owner != OwnerRelation::Self_
        || input.subject.id != owner.owner_id
    {
        return Err(
            "MEMORY_UNAUTHORIZED: claims written by this UI must concern the vault owner".into(),
        );
    }
    if input.sensitivity == Sensitivity::Restricted {
        return Err(
            "MEMORY_RESTRICTED_PERSISTENCE_DENIED: restricted content cannot enter Git".into(),
        );
    }
    if input.approval_kind != approval_kind_for(input.claim_kind) {
        return Err("MEMORY_INVALID_CLAIM: approval kind does not match claim kind".into());
    }
    if input.risk_class != risk_for(input.claim_kind) {
        return Err("MEMORY_INVALID_CLAIM: risk class does not match claim kind".into());
    }
    if input.context.spaces.is_empty() || input.consent.allowed_purposes.is_empty() {
        return Err("MEMORY_CONTEXT_INCOMPLETE: space and purpose are required".into());
    }
    Ok(())
}

fn decide_pending(
    root: &Path,
    input: PendingDecisionInput,
    expected_intent: GestureIntent,
) -> Result<Value, String> {
    validate_request_id(&input.request_id)?;
    if input.gesture_intent != expected_intent {
        return Err("MEMORY_UNAUTHORIZED: gesture intent does not match RPC".into());
    }
    if input.delete_kind.is_some() {
        return Err("MEMORY_INVALID_REQUEST: delete_kind is not valid for this gesture".into());
    }
    let (repository, snapshot) = active(root, &input.expected_protocol)?;
    if let Some(existing) = idempotent(&repository, &input.request_id) {
        let operation_matches = matches!(
            (expected_intent, existing.value.transition.operation),
            (GestureIntent::Approve, ClaimOperation::Approve)
                | (GestureIntent::Reject, ClaimOperation::Reject)
                | (GestureIntent::Ignore, ClaimOperation::Ignore)
        );
        if !operation_matches
            || existing.value.transition.approves_revision_id.as_deref()
                != Some(input.revision_id.as_str())
        {
            return Err(
                "MEMORY_IDEMPOTENCY_CONFLICT: request_id was reused for another decision".into(),
            );
        }
        return receipt_for(existing, &snapshot, true);
    }
    let proposed = repository
        .claims
        .iter()
        .find(|item| item.value.revision_id == input.revision_id)
        .ok_or("MEMORY_STALE_BASE: pending revision no longer exists")?;
    if proposed.value.payload_sha256 != input.expected_sha256 {
        return Err("MEMORY_REVISION_HASH_CHANGED: pending payload hash differs".into());
    }
    if proposed.value.workflow.state != WorkflowState::Pending
        || repository.claims.iter().any(|item| {
            item.value.transition.approves_revision_id.as_deref()
                == Some(proposed.value.revision_id.as_str())
        })
    {
        return Err("MEMORY_STALE_BASE: pending revision was already decided".into());
    }
    exact_heads(&input.expected_heads, &proposed.value.parents)?;
    let controls = controls(&repository, &snapshot, CLAIM_APPROVE)?;
    let (workflow, operation, lifecycle, verdict) = match expected_intent {
        GestureIntent::Approve => (
            WorkflowState::Approved,
            ClaimOperation::Approve,
            proposed.value.lifecycle.state,
            Verdict::Approve,
        ),
        GestureIntent::Reject => (
            WorkflowState::Rejected,
            ClaimOperation::Reject,
            proposed.value.lifecycle.state,
            Verdict::Reject,
        ),
        GestureIntent::Ignore => (
            WorkflowState::Ignored,
            ClaimOperation::Ignore,
            proposed.value.lifecycle.state,
            Verdict::Reject,
        ),
        _ => return Err("MEMORY_UNAUTHORIZED: unsupported pending gesture".into()),
    };
    let mut child = proposed.value.clone();
    child.revision_id = uuid_v7();
    child.request_id = input.request_id;
    child.parents = vec![revision_ref(&proposed.value)];
    child.causal_context = causal_context(&repository, &snapshot, &[proposed]);
    child.recorded_by = host_recorder();
    child.recorded_at = now();
    child.workflow.state = workflow;
    child.lifecycle.state = lifecycle;
    if let Some(salience) = input.salience_override {
        child.salience = salience;
    }
    child.decision = Some(ClaimDecision {
        verdict,
        approval_kind: approval_kind_for(child.claim_kind),
        authority_scope: AUTHORITY_SCOPE.into(),
        actor_id: controls.owner.actor_id,
        decided_at: child.recorded_at.clone(),
        protocol_context: ContextHeads {
            heads: snapshot.protocol.heads.clone(),
        },
        authority_context: AuthorityContext {
            heads: snapshot.authority.heads.clone(),
            capability: CLAIM_APPROVE.into(),
        },
    });
    child.transition = ClaimTransition {
        operation,
        approves_revision_id: Some(proposed.value.revision_id.clone()),
        approves_payload_sha256: Some(proposed.value.payload_sha256.clone()),
    };
    child.payload_sha256.clear();
    publish_and_verify(root, child)
}

fn delete(root: &Path, params: &Value) -> Result<Value, String> {
    if params.get("revision_id").is_some() {
        let input: PendingDecisionInput = parse(params, "delete pending")?;
        if input.gesture_intent != GestureIntent::Delete || input.delete_kind.is_some() {
            return Err("MEMORY_UNAUTHORIZED: invalid pending delete gesture".into());
        }
        return delete_pending(root, input);
    }
    let input: ClaimMutationInput = parse(params, "delete claim")?;
    if input.delete_kind.as_deref() != Some("claim-tombstone") || input.salience.is_some() {
        return Err("MEMORY_UNAUTHORIZED: explicit claim tombstone gesture required".into());
    }
    mutate_current(root, input, ClaimOperation::Delete, |child| {
        child.lifecycle.state = LifecycleState::Deleted;
    })
}

fn delete_pending(root: &Path, input: PendingDecisionInput) -> Result<Value, String> {
    validate_request_id(&input.request_id)?;
    let (repository, snapshot) = active(root, &input.expected_protocol)?;
    if let Some(existing) = idempotent(&repository, &input.request_id) {
        if existing.value.transition.operation != ClaimOperation::Ignore
            || existing.value.lifecycle.state != LifecycleState::Deleted
            || existing.value.transition.approves_revision_id.as_deref()
                != Some(input.revision_id.as_str())
        {
            return Err(
                "MEMORY_IDEMPOTENCY_CONFLICT: request_id was reused for another pending delete"
                    .into(),
            );
        }
        return receipt_for(existing, &snapshot, true);
    }
    let proposed = repository
        .claims
        .iter()
        .find(|item| item.value.revision_id == input.revision_id)
        .ok_or("MEMORY_STALE_BASE: pending revision no longer exists")?;
    if proposed.value.payload_sha256 != input.expected_sha256 {
        return Err("MEMORY_REVISION_HASH_CHANGED: pending payload hash differs".into());
    }
    exact_heads(&input.expected_heads, &proposed.value.parents)?;
    let controls = controls(&repository, &snapshot, CLAIM_APPROVE)?;
    let mut child = proposed.value.clone();
    child.revision_id = uuid_v7();
    child.request_id = input.request_id;
    child.parents = vec![revision_ref(&proposed.value)];
    child.causal_context = causal_context(&repository, &snapshot, &[proposed]);
    child.recorded_by = host_recorder();
    child.recorded_at = now();
    child.workflow.state = WorkflowState::Ignored;
    child.lifecycle.state = LifecycleState::Deleted;
    child.decision = Some(decision(
        &controls,
        approval_kind_for(child.claim_kind),
        CLAIM_APPROVE,
        child.recorded_at.clone(),
    ));
    child.transition = ClaimTransition {
        operation: ClaimOperation::Ignore,
        approves_revision_id: Some(proposed.value.revision_id.clone()),
        approves_payload_sha256: Some(proposed.value.payload_sha256.clone()),
    };
    child.payload_sha256.clear();
    publish_and_verify(root, child)
}

fn set_salience(root: &Path, input: ClaimMutationInput) -> Result<Value, String> {
    if input.delete_kind.is_some() {
        return Err("MEMORY_INVALID_REQUEST: delete_kind is invalid for salience".into());
    }
    let salience = input
        .salience
        .ok_or("MEMORY_INVALID_REQUEST: salience is required")?;
    mutate_current(root, input, ClaimOperation::SetSalience, move |child| {
        child.salience = salience;
    })
}

fn mutate_current<F>(
    root: &Path,
    input: ClaimMutationInput,
    operation: ClaimOperation,
    change: F,
) -> Result<Value, String>
where
    F: FnOnce(&mut MemoryClaimRevision),
{
    validate_request_id(&input.request_id)?;
    let (repository, snapshot) = active(root, &input.expected_protocol)?;
    if let Some(existing) = idempotent(&repository, &input.request_id) {
        if existing.value.transition.operation != operation
            || existing.value.claim_id != input.claim_id
        {
            return Err(
                "MEMORY_IDEMPOTENCY_CONFLICT: request_id was reused for another mutation".into(),
            );
        }
        return receipt_for(existing, &snapshot, true);
    }
    let view = snapshot
        .claims
        .iter()
        .find(|view| view.claim_id == input.claim_id)
        .ok_or("MEMORY_STALE_BASE: claim does not exist")?;
    exact_heads(&input.expected_heads, &view.current_heads)?;
    if view.current_heads.len() != 1 || view.application_state != ApplicationState::Current {
        return Err("MEMORY_STALE_BASE: claim does not have one current active head".into());
    }
    let parent = loaded_claim(&repository, &view.current_heads[0])?;
    let controls = controls(&repository, &snapshot, CLAIM_APPROVE)?;
    let mut child = parent.value.clone();
    child.revision_id = uuid_v7();
    child.request_id = input.request_id;
    child.parents = view.current_heads.clone();
    child.causal_context = causal_context(&repository, &snapshot, &[parent]);
    child.recorded_by = host_recorder();
    child.recorded_at = now();
    child.workflow.state = WorkflowState::Approved;
    child.decision = Some(decision(
        &controls,
        approval_kind_for(child.claim_kind),
        CLAIM_APPROVE,
        child.recorded_at.clone(),
    ));
    child.transition = ClaimTransition {
        operation,
        approves_revision_id: None,
        approves_payload_sha256: None,
    };
    change(&mut child);
    child.payload_sha256.clear();
    publish_and_verify(root, child)
}

fn resolve(root: &Path, input: ResolveInput) -> Result<Value, String> {
    validate_request_id(&input.request_id)?;
    let (repository, snapshot) = active(root, &input.expected_protocol)?;
    if let Some(existing) = idempotent(&repository, &input.request_id) {
        if existing.value.transition.operation != ClaimOperation::Resolve
            || existing.value.claim_id != input.claim_id
            || existing.value.parents != input.expected_heads
        {
            return Err("MEMORY_IDEMPOTENCY_CONFLICT: request_id was reused for another conflict resolution".into());
        }
        return receipt_for(existing, &snapshot, true);
    }
    let view = snapshot
        .claims
        .iter()
        .find(|view| view.claim_id == input.claim_id)
        .ok_or("MEMORY_STALE_BASE: claim does not exist")?;
    let conflict = view
        .conflict
        .as_ref()
        .ok_or("MEMORY_STALE_BASE: claim is no longer conflicted")?;
    if conflict.conflict_id != input.conflict_id {
        return Err("MEMORY_STALE_BASE: conflict identity changed".into());
    }
    exact_heads(&input.expected_heads, &conflict.heads)?;
    let parents = conflict
        .heads
        .iter()
        .map(|head| loaded_claim(&repository, head))
        .collect::<Result<Vec<_>, _>>()?;
    let base = match input.strategy {
        ResolveStrategy::KeepHead => {
            let selected = input
                .selected_revision_id
                .as_deref()
                .ok_or("MEMORY_INVALID_REQUEST: selected_revision_id is required")?;
            parents
                .iter()
                .find(|item| item.value.revision_id == selected)
                .copied()
                .ok_or("MEMORY_STALE_BASE: selected head is not current")?
        }
        _ => parents[0],
    };
    let controls = controls(&repository, &snapshot, CLAIM_RESOLVE)?;
    let mut child = base.value.clone();
    child.revision_id = uuid_v7();
    child.request_id = input.request_id;
    child.parents = conflict.heads.clone();
    child.causal_context = causal_context(&repository, &snapshot, &parents);
    child.recorded_by = host_recorder();
    child.recorded_at = now();
    child.workflow.state = WorkflowState::Approved;
    match input.strategy {
        ResolveStrategy::KeepHead => {}
        ResolveStrategy::Merge => {
            child.text = input
                .merged_text
                .filter(|text| !text.trim().is_empty())
                .ok_or("MEMORY_INVALID_REQUEST: merged_text is required")?;
        }
        ResolveStrategy::RevokeAll => child.lifecycle.state = LifecycleState::Revoked,
    }
    child.decision = Some(decision(
        &controls,
        approval_kind_for(child.claim_kind),
        CLAIM_RESOLVE,
        child.recorded_at.clone(),
    ));
    child.transition = ClaimTransition {
        operation: ClaimOperation::Resolve,
        approves_revision_id: None,
        approves_payload_sha256: None,
    };
    child.payload_sha256.clear();
    publish_and_verify(root, child)
}

fn context_preview(root: &Path, request: ContextRequest) -> Result<Value, String> {
    validate_context_request(&request)?;
    let repository = require_active(root)?;
    context_value(&repository, request)
}

fn context_value(
    repository: &RepositorySnapshot,
    request: ContextRequest,
) -> Result<Value, String> {
    let selected =
        select_context(repository, request.clone()).map_err(|error| error.to_string())?;
    let selected_ids = selected
        .claims
        .iter()
        .map(|claim| claim.revision_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut excluded = BTreeMap::<String, u64>::new();
    for revision in &repository.claims {
        if selected_ids.contains(revision.value.revision_id.as_str()) {
            continue;
        }
        let reason = if request.external_transfer
            && revision.value.consent.external_provider_policy != ExternalProviderPolicy::Allow
        {
            "provider-policy"
        } else if !revision.value.context.spaces.contains(&request.space)
            || !revision
                .value
                .consent
                .allowed_purposes
                .contains(&request.purpose)
        {
            "scope-or-purpose"
        } else {
            "not-current"
        };
        *excluded.entry(reason.into()).or_default() += 1;
    }
    let policy_allowed = selected.action_allowed
        && (!request.external_transfer
            || excluded.get("provider-policy").copied().unwrap_or(0) == 0);
    Ok(json!({
        "request": request,
        "selected": selected.claims.iter().map(|claim| json!({
            "claim_id": claim.claim_id, "revision_id": claim.revision_id,
            "payload_sha256": claim.payload_sha256,
            "reasons": ["current", "space-match", "purpose-match"], "text": claim.text
        })).collect::<Vec<_>>(),
        "excluded_summary": excluded,
        "conflicts": selected.conflicts.iter().map(|conflict| json!({
            "conflict_id": conflict.conflict_id, "action_allowed": conflict.action_allowed
        })).collect::<Vec<_>>(),
        "redactions": 0,
        "policy_result": { "external_action_allowed": policy_allowed }
    }))
}

fn context_manifest(root: &Path, request: ContextRequest) -> Result<Value, String> {
    validate_context_request(&request)?;
    let repository = require_active(root)?;
    let snapshot = reduce(
        &repository,
        &SnapshotRequest {
            as_of_valid_time: request.as_of_valid_time.clone(),
            space: Some(request.space.clone()),
            purpose: Some(request.purpose.clone()),
        },
    )
    .map_err(|error| error.to_string())?;
    let preview = context_value(&repository, request.clone())?;
    let selected_values = preview["selected"].as_array().cloned().unwrap_or_default();
    let selected = selected_values
        .iter()
        .map(|item| ContextSelection {
            claim_id: item["claim_id"].as_str().unwrap_or_default().into(),
            revision_id: item["revision_id"].as_str().unwrap_or_default().into(),
            payload_sha256: item["payload_sha256"].as_str().unwrap_or_default().into(),
            reasons: item["reasons"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
        })
        .collect::<Vec<_>>();
    let excluded_summary = serde_json::from_value(preview["excluded_summary"].clone())
        .map_err(|error| format!("MEMORY_INVALID_PAYLOAD: {error}"))?;
    let policy_allowed = preview["policy_result"]["external_action_allowed"]
        .as_bool()
        .unwrap_or(false);
    let selected_loaded = selected
        .iter()
        .filter_map(|selected| {
            repository
                .claims
                .iter()
                .find(|item| item.value.revision_id == selected.revision_id)
        })
        .collect::<Vec<_>>();
    let manifest = ContextManifest {
        schema: "notemd.memory/context-manifest/v2".into(),
        manifest_id: uuid_v7(),
        request,
        selected,
        excluded_summary,
        conflicts: snapshot
            .claims
            .iter()
            .filter_map(|view| view.conflict.clone())
            .collect(),
        policy_result: ContextPolicyResult {
            external_action_allowed: policy_allowed,
        },
        protocol_context: ContextHeads {
            heads: snapshot.protocol.heads.clone(),
        },
        authority_context: ContextHeads {
            heads: snapshot.authority.heads.clone(),
        },
        causal_context: causal_context(&repository, &snapshot, &selected_loaded),
        payload_sha256: String::new(),
    };
    let published = RepositoryWriter::new(root)
        .publish_context_manifest(manifest)
        .map_err(|error| error.to_string())?;
    // Reloading verifies filename, semantic hash and the complete causal DAG.
    let reloaded = require_active(root)?;
    reduce(
        &reloaded,
        &SnapshotRequest {
            as_of_valid_time: now(),
            space: None,
            purpose: None,
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(json!({
        "manifest_id": published.value.manifest_id,
        "payload_sha256": published.value.payload_sha256,
        "selected_count": published.value.selected.len()
    }))
}

fn validate_context_request(request: &ContextRequest) -> Result<(), String> {
    if [
        request.space.as_str(),
        request.purpose.as_str(),
        request.caller.as_str(),
        request.provider.as_str(),
        request.model.as_str(),
        request.as_of_valid_time.as_str(),
    ]
    .iter()
    .any(|value| value.trim().is_empty())
    {
        return Err("MEMORY_CONTEXT_INCOMPLETE: space, purpose, caller, provider, model and as-of time are required".into());
    }
    chrono::DateTime::parse_from_rfc3339(&request.as_of_valid_time)
        .map_err(|error| format!("MEMORY_INVALID_TIME: {error}"))?;
    Ok(())
}

fn check(root: &Path) -> Result<Value, String> {
    let snapshot = snapshot_view(
        root,
        SnapshotInput {
            as_of_valid_time: now(),
            space: None,
            purpose: None,
        },
    )?;
    Ok(snapshot["health"].clone())
}

fn migration_dry_run(root: &Path, input: MigrationInput) -> Result<Value, String> {
    if input.mode != "dry-run" {
        return Err("MEMORY_UNAUTHORIZED: UI migration endpoint only permits dry-run".into());
    }
    let source_manifest_sha256 = source_manifest(root)?;
    let mut warnings = Vec::new();
    let mut blockers = Vec::new();
    let (claims, pending, approved, rejected, legacy_unclassified) =
        match crate::memory_control::list(root) {
            Ok(snapshot) => {
                if snapshot.integrity.drift {
                    blockers.extend(snapshot.integrity.errors);
                }
                let pending = snapshot
                    .proposals
                    .iter()
                    .filter(|proposal| {
                        proposal.decision == crate::memory_control::model::ProposalDecision::Pending
                    })
                    .count();
                let rejected = snapshot
                    .proposals
                    .iter()
                    .filter(|proposal| {
                        proposal.decision
                            == crate::memory_control::model::ProposalDecision::Rejected
                    })
                    .count();
                let approved = snapshot.entries.len();
                let claims = approved + pending;
                if claims > 0 {
                    warnings.push(format!(
                        "{claims} 条旧记录缺少 v2 完整语义，迁移后需逐条复核"
                    ));
                }
                (claims, pending, approved, rejected, claims)
            }
            Err(error) => {
                blockers.push(error);
                (0, 0, 0, 0, 0)
            }
        };
    let plan_material = format!("memory-v2-migration/1\n{source_manifest_sha256}\n{claims}\n{pending}\n{approved}\n{rejected}");
    let plan_sha256 = digest(plan_material.as_bytes());
    let projection_preview = json!({
        "user": fs::read_to_string(root.join("USER.md")).unwrap_or_else(|_| "# USER\n".into()),
        "memory": fs::read_to_string(root.join("MEMORY.md")).unwrap_or_else(|_| "# MEMORY\n".into())
    });
    Ok(json!({
        "migration_id": format!("migration-{}", &plan_sha256[..16]),
        "plan_sha256": plan_sha256,
        "source_manifest_sha256": source_manifest_sha256,
        "counts": {
            "claims": claims, "pending": pending, "approved": approved,
            "rejected": rejected, "legacy_unclassified": legacy_unclassified
        },
        "projection_preview": projection_preview,
        "warnings": warnings, "blockers": blockers, "writes_performed": false
    }))
}

fn source_manifest(root: &Path) -> Result<String, String> {
    let mut paths = Vec::new();
    for relative in [
        "USER.md",
        "MEMORY.md",
        "inbox/memory-candidates",
        "memory/events",
    ] {
        collect_files(root, &root.join(relative), &mut paths)?;
    }
    paths.sort();
    let mut hasher = Sha256::new();
    for path in paths {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let bytes = fs::read(&path).map_err(|error| format!("MEMORY_IO: {error}"))?;
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(digest(&bytes).as_bytes());
        hasher.update([b'\n']);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect_files(root: &Path, path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let _ = root;
    if !path.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| format!("MEMORY_IO: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("MEMORY_TAMPERED_ASSET: symlink {}", path.display()));
    }
    if metadata.is_file() {
        out.push(path.to_path_buf());
        return Ok(());
    }
    let mut children = fs::read_dir(path)
        .map_err(|error| format!("MEMORY_IO: {error}"))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| format!("MEMORY_IO: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    children.sort();
    for child in children {
        collect_files(root, &child, out)?;
    }
    Ok(())
}

/// Agent-facing proposal primitive. It deliberately cannot produce an
/// approved revision and refuses non-owner subjects and restricted content.
pub fn propose_pending(
    root: &Path,
    input: PendingProposalInput,
) -> Result<MemoryClaimRevision, String> {
    validate_request_id(&input.request_id)?;
    let repository = require_active(root)?;
    let snapshot = reduce(
        &repository,
        &SnapshotRequest {
            as_of_valid_time: now(),
            space: None,
            purpose: None,
        },
    )
    .map_err(|error| error.to_string())?;
    if let Some(existing) = idempotent(&repository, &input.request_id) {
        return Ok(existing.value.clone());
    }
    let owner = snapshot
        .authority
        .owner
        .as_ref()
        .ok_or("MEMORY_UNAUTHORIZED: owner is not unique")?;
    if input.subject.kind != SubjectKind::VaultOwner
        || input.subject.relation_to_owner != OwnerRelation::Self_
        || input.subject.id != owner.owner_id
    {
        return Err("MEMORY_UNAUTHORIZED: agents may only propose owner-related memories".into());
    }
    if input.sensitivity == Sensitivity::Restricted {
        return Err(
            "MEMORY_RESTRICTED_PERSISTENCE_DENIED: restricted content cannot enter Git".into(),
        );
    }
    if input.claim_kind != input.kind_data.kind() || input.asserted_by.is_empty() {
        return Err("MEMORY_INVALID_CLAIM: incomplete proposal semantics".into());
    }
    if input.claim_id.is_none() && input.lifecycle != LifecycleState::Active {
        return Err("MEMORY_INVALID_TRANSITION: a create proposal must be active".into());
    }
    let claim_id = input.claim_id.unwrap_or_else(uuid_v7);
    let parents = snapshot
        .claims
        .iter()
        .find(|view| view.claim_id == claim_id)
        .map(|view| view.current_heads.clone())
        .unwrap_or_default();
    let parent_loaded = parents
        .iter()
        .map(|parent| loaded_claim(&repository, parent))
        .collect::<Result<Vec<_>, _>>()?;
    let revision = MemoryClaimRevision {
        schema: "notemd.memory/claim-revision/v2".into(),
        claim_id,
        revision_id: uuid_v7(),
        request_id: input.request_id,
        parents: parents.clone(),
        causal_context: causal_context(&repository, &snapshot, &parent_loaded),
        claim_kind: input.claim_kind,
        kind_data: input.kind_data,
        subject: input.subject,
        asserted_by: input.asserted_by,
        recorded_by: input.recorded_by,
        recorded_at: now(),
        text: input.text,
        projection: input.projection,
        workflow: Workflow {
            state: WorkflowState::Pending,
        },
        lifecycle: Lifecycle {
            state: input.lifecycle,
        },
        temporal: input.temporal,
        epistemic: input.epistemic,
        trust_tier: input.trust_tier,
        risk_class: input.risk_class,
        salience: input.salience,
        polarity: input.polarity,
        sensitivity: input.sensitivity,
        context: input.context,
        consent: input.consent,
        agent_use: input.agent_use,
        decision: None,
        transition: ClaimTransition {
            operation: if parents.is_empty() {
                ClaimOperation::ProposeCreate
            } else {
                ClaimOperation::ProposeReplace
            },
            approves_revision_id: None,
            approves_payload_sha256: None,
        },
        evidence: input.evidence,
        lineage: Lineage::default(),
        dedupe_key: input.dedupe_key,
        payload_sha256: String::new(),
    };
    let published = RepositoryWriter::new(root)
        .publish_claim(revision)
        .map_err(|error| error.to_string())?;
    let reloaded = require_active(root)?;
    reduce(
        &reloaded,
        &SnapshotRequest {
            as_of_valid_time: now(),
            space: None,
            purpose: None,
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(published.value)
}

fn active(
    root: &Path,
    expected_protocol: &RevisionRef,
) -> Result<(RepositorySnapshot, MemorySnapshotV2), String> {
    let repository = require_active(root)?;
    let snapshot = reduce(
        &repository,
        &SnapshotRequest {
            as_of_valid_time: now(),
            space: None,
            purpose: None,
        },
    )
    .map_err(|error| error.to_string())?;
    let actual = unique_protocol(&snapshot)?;
    if &actual != expected_protocol {
        return Err("MEMORY_STALE_BASE: protocol head changed".into());
    }
    if !snapshot.protocol.writable || !snapshot.authority.action_allowed {
        return Err("MEMORY_UNAUTHORIZED: protocol or authority is conflicted".into());
    }
    Ok((repository, snapshot))
}

fn require_active(root: &Path) -> Result<RepositorySnapshot, String> {
    let repository = V2Repository::new(root)
        .load()
        .map_err(|error| error.to_string())?;
    if repository.mode != RepositoryMode::V2Active {
        return Err(format!("MEMORY_PROTOCOL_NOT_ACTIVE: {:?}", repository.mode));
    }
    Ok(repository)
}

struct Controls {
    owner: AuthorityOwner,
    causal_context: CausalContext,
    protocol_heads: Vec<RevisionRef>,
    authority_heads: Vec<RevisionRef>,
}

fn controls(
    repository: &RepositorySnapshot,
    snapshot: &MemorySnapshotV2,
    capability: &str,
) -> Result<Controls, String> {
    let owner = snapshot
        .authority
        .owner
        .clone()
        .ok_or("MEMORY_UNAUTHORIZED: authority owner is not unique")?;
    let granted = snapshot
        .authority
        .effective_capabilities
        .get(&owner.actor_id)
        .is_some_and(|capabilities| capabilities.iter().any(|item| item == capability));
    if !granted {
        return Err(format!("MEMORY_UNAUTHORIZED: owner lacks {capability}"));
    }
    Ok(Controls {
        owner,
        causal_context: causal_context(repository, snapshot, &[]),
        protocol_heads: snapshot.protocol.heads.clone(),
        authority_heads: snapshot.authority.heads.clone(),
    })
}

fn decision(
    controls: &Controls,
    kind: ApprovalKind,
    capability: &str,
    decided_at: String,
) -> ClaimDecision {
    ClaimDecision {
        verdict: Verdict::Approve,
        approval_kind: kind,
        authority_scope: AUTHORITY_SCOPE.into(),
        actor_id: controls.owner.actor_id.clone(),
        decided_at,
        protocol_context: ContextHeads {
            heads: controls.protocol_heads.clone(),
        },
        authority_context: AuthorityContext {
            heads: controls.authority_heads.clone(),
            capability: capability.into(),
        },
    }
}

fn causal_context<'a>(
    repository: &'a RepositorySnapshot,
    snapshot: &MemorySnapshotV2,
    claims: &[&'a Loaded<MemoryClaimRevision>],
) -> CausalContext {
    let mut parents = Vec::new();
    for head in &snapshot.protocol.heads {
        if let Some(item) = repository
            .protocols
            .iter()
            .find(|item| item.value.revision_id == head.revision_id)
        {
            parents.push(RecordRef {
                record_id: item.value.revision_id.clone(),
                raw_sha256: item.raw_sha256.clone(),
            });
        }
    }
    for head in &snapshot.authority.heads {
        if let Some(item) = repository
            .authorities
            .iter()
            .find(|item| item.value.revision_id == head.revision_id)
        {
            parents.push(RecordRef {
                record_id: item.value.revision_id.clone(),
                raw_sha256: item.raw_sha256.clone(),
            });
        }
    }
    parents.extend(claims.iter().map(|item| RecordRef {
        record_id: item.value.revision_id.clone(),
        raw_sha256: item.raw_sha256.clone(),
    }));
    parents.sort();
    parents.dedup();
    CausalContext { parents }
}

fn publish_and_verify(root: &Path, revision: MemoryClaimRevision) -> Result<Value, String> {
    let published = RepositoryWriter::new(root)
        .publish_claim(revision)
        .map_err(|error| error.to_string())?;
    let repository = require_active(root)?;
    let snapshot = reduce(
        &repository,
        &SnapshotRequest {
            as_of_valid_time: now(),
            space: None,
            purpose: None,
        },
    )
    .map_err(|error| error.to_string())?;
    let projection_rebuilt = rebuild_projections(root).is_ok();
    receipt_for_published(&published.value, &snapshot, projection_rebuilt)
}

fn receipt_for(
    existing: &Loaded<MemoryClaimRevision>,
    snapshot: &MemorySnapshotV2,
    projection_rebuilt: bool,
) -> Result<Value, String> {
    receipt_for_published(&existing.value, snapshot, projection_rebuilt)
}

fn receipt_for_published(
    revision: &MemoryClaimRevision,
    snapshot: &MemorySnapshotV2,
    projection_rebuilt: bool,
) -> Result<Value, String> {
    let view = snapshot
        .claims
        .iter()
        .find(|view| view.claim_id == revision.claim_id);
    let conflict = view.and_then(|view| view.conflict.as_ref()).is_some();
    let status = match revision.workflow.state {
        WorkflowState::Pending => "pending",
        WorkflowState::Rejected => "rejected",
        WorkflowState::Ignored => "ignored",
        WorkflowState::Approved => match revision.lifecycle.state {
            LifecycleState::Active => "active",
            LifecycleState::Revoked => "revoked",
            LifecycleState::Deleted => "deleted",
            LifecycleState::Merged => "merged",
        },
    };
    serde_json::to_value(WriteReceipt {
        claim_id: revision.claim_id.clone(),
        revision_id: revision.revision_id.clone(),
        payload_sha256: revision.payload_sha256.clone(),
        effective_status: status.into(),
        conflict,
        projection_rebuilt,
    })
    .map_err(|error| error.to_string())
}

fn unique_protocol(snapshot: &MemorySnapshotV2) -> Result<RevisionRef, String> {
    if snapshot.protocol.heads.len() != 1 || snapshot.protocol.conflict {
        return Err("MEMORY_PROTOCOL_CONFLICT: protocol does not have one head".into());
    }
    Ok(snapshot.protocol.heads[0].clone())
}

fn loaded_claim<'a>(
    repository: &'a RepositorySnapshot,
    reference: &RevisionRef,
) -> Result<&'a Loaded<MemoryClaimRevision>, String> {
    repository
        .claims
        .iter()
        .find(|item| {
            item.value.revision_id == reference.revision_id
                && item.value.payload_sha256 == reference.payload_sha256
        })
        .ok_or_else(|| {
            format!(
                "MEMORY_STALE_BASE: missing claim head {}",
                reference.revision_id
            )
        })
}

fn exact_heads(expected: &[RevisionRef], actual: &[RevisionRef]) -> Result<(), String> {
    let mut expected = expected.to_vec();
    let mut actual = actual.to_vec();
    expected.sort();
    actual.sort();
    if expected != actual {
        Err("MEMORY_STALE_BASE: exact claim heads changed".into())
    } else {
        Ok(())
    }
}

fn idempotent<'a>(
    repository: &'a RepositorySnapshot,
    request_id: &str,
) -> Option<&'a Loaded<MemoryClaimRevision>> {
    repository
        .claims
        .iter()
        .find(|item| item.value.request_id == request_id)
}

fn revision_ref(revision: &MemoryClaimRevision) -> RevisionRef {
    RevisionRef {
        revision_id: revision.revision_id.clone(),
        payload_sha256: revision.payload_sha256.clone(),
    }
}

fn approval_kind_for(kind: ClaimKind) -> ApprovalKind {
    match kind {
        ClaimKind::Boundary | ClaimKind::Practice => ApprovalKind::BehavioralAuthorization,
        ClaimKind::MaterialFact => ApprovalKind::FactualVerification,
        _ => ApprovalKind::SelfRepresentation,
    }
}

fn risk_for(kind: ClaimKind) -> RiskClass {
    match kind {
        ClaimKind::Boundary => RiskClass::ActionSensitive,
        ClaimKind::Decision | ClaimKind::Commitment | ClaimKind::Practice => RiskClass::Behavioral,
        _ => RiskClass::Informational,
    }
}

fn kind_data_for(
    kind: ClaimKind,
    text: &str,
    category: &str,
    at: &str,
    polarity: Polarity,
) -> KindData {
    match kind {
        ClaimKind::Identity => KindData::Identity(IdentityData {
            identity_type: IdentityType::Person,
            value: text.into(),
        }),
        ClaimKind::Preference => KindData::Preference(PreferenceData {
            dimension: category.into(),
        }),
        ClaimKind::Boundary => KindData::Boundary(BoundaryData {
            behavior_policy: BehaviorPolicy {
                effect: if polarity == Polarity::Negative {
                    PolicyEffect::Deny
                } else {
                    PolicyEffect::Prompt
                },
                actions: vec!["unspecified-external-action".into()],
                resources: vec!["unspecified".into()],
                conditions: vec![
                    "Natural-language boundary requires conservative interpretation".into(),
                ],
            },
        }),
        ClaimKind::Decision => KindData::Decision(DecisionData {
            made_by: "vault-owner".into(),
            decided_at: at.into(),
            decision_scope: category.into(),
        }),
        ClaimKind::Belief => KindData::Belief(BeliefData {
            proposition: text.into(),
        }),
        ClaimKind::Observation => KindData::Observation(ObservationData {
            observer: "vault-owner".into(),
        }),
        ClaimKind::Commitment => KindData::Commitment(CommitmentData {
            committed_by: "vault-owner".into(),
            beneficiary: "unspecified".into(),
        }),
        ClaimKind::Practice => KindData::Practice(PracticeData {
            practice_scope: category.into(),
        }),
        ClaimKind::MaterialFact => KindData::MaterialFact(MaterialFactData {
            proposition: text.into(),
        }),
        ClaimKind::Quotation => KindData::Quotation(QuotationData {
            speaker: "vault-owner".into(),
        }),
        ClaimKind::LegacyUnclassified => KindData::LegacyUnclassified(LegacyData {
            missing_semantics: vec!["claim-kind".into()],
        }),
    }
}

fn temporal_for(kind: ClaimKind, at: &str) -> Temporal {
    let mut temporal = Temporal::default();
    match kind {
        ClaimKind::Observation => temporal.observed_at = Some(at.into()),
        ClaimKind::Quotation => temporal.uttered_at = Some(at.into()),
        ClaimKind::Preference
        | ClaimKind::Boundary
        | ClaimKind::Decision
        | ClaimKind::Commitment
        | ClaimKind::Practice => temporal.valid_from = Some(at.into()),
        _ => {}
    }
    temporal
}

fn epistemic_for(kind: ApprovalKind) -> Epistemic {
    match kind {
        ApprovalKind::SelfRepresentation | ApprovalKind::BehavioralAuthorization => Epistemic {
            basis: "owner-stated".into(),
            representation_certainty: "high".into(),
            truth_status: "not-assessed".into(),
            truth_confidence: "unknown".into(),
        },
        ApprovalKind::FactualVerification => Epistemic {
            basis: "owner-stated".into(),
            representation_certainty: "high".into(),
            truth_status: "verified".into(),
            truth_confidence: "high".into(),
        },
    }
}

fn host_recorder() -> Recorder {
    Recorder {
        kind: "host".into(),
        id: "notemd.memory-ui".into(),
        device_id: "device:official-memory-ui".into(),
    }
}

fn validate_request_id(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 256 {
        Err("MEMORY_INVALID_REQUEST: request_id is required and must be at most 256 bytes".into())
    } else {
        Ok(())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn uuid_v7() -> String {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let mut bytes = *Uuid::new_v4().as_bytes();
    let timestamp = milliseconds.to_be_bytes();
    bytes[..6].copy_from_slice(&timestamp[2..]);
    bytes[6] = (bytes[6] & 0x0f) | 0x70;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes).to_string()
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed with {status}");
    }

    fn configure_git(root: &Path, email: &str) {
        git(root, &["config", "user.email", email]);
        git(root, &["config", "user.name", "Memory v2 test"]);
        git(root, &["config", "core.autocrlf", "false"]);
    }

    fn initialize(root: &Path) -> RevisionRef {
        let actor = "human:test".to_string();
        let authority_id = uuid_v7();
        let protocol = ProtocolRevision {
            schema: "notemd.memory/protocol-revision/v2".into(),
            revision_id: uuid_v7(),
            base_heads: vec![],
            causal_context: CausalContext::default(),
            protocol_major: 2,
            protocol_minor: 0,
            renderer_version: "notemd.memory.projector/2".into(),
            claim_schema: "notemd.memory/claim-revision/v2".into(),
            category_registry: BTreeMap::from([
                (
                    "user".into(),
                    vec!["preferences".into(), "boundaries".into()],
                ),
                ("memory".into(), vec!["context".into()]),
            ]),
            decision: ControlDecision {
                verdict: Verdict::Approve,
                actor_id: actor.clone(),
                authority_context: AuthorityContext {
                    heads: vec![],
                    capability: "bootstrap".into(),
                },
            },
            transition: ControlTransition {
                operation: ControlOperation::Initialize,
            },
            payload_sha256: String::new(),
        };
        let authority = AuthorityRevision {
            schema: "notemd.memory/authority-revision/v2".into(),
            revision_id: authority_id,
            base_heads: vec![],
            causal_context: CausalContext::default(),
            owner: AuthorityOwner {
                owner_id: "owner:test".into(),
                actor_id: actor.clone(),
            },
            principals: vec![Principal {
                actor_id: actor.clone(),
                capabilities: vec![CLAIM_APPROVE.into(), CLAIM_RESOLVE.into()],
            }],
            recovery: Recovery::LocalOwnerSetup,
            decision: ControlDecision {
                verdict: Verdict::Approve,
                actor_id: actor,
                authority_context: AuthorityContext {
                    heads: vec![],
                    capability: "bootstrap".into(),
                },
            },
            transition: ControlTransition {
                operation: ControlOperation::Initialize,
            },
            payload_sha256: String::new(),
        };
        RepositoryWriter::new(root)
            .initialize("vault:test".into(), protocol, authority)
            .unwrap();
        let repository = V2Repository::new(root).load().unwrap();
        let reduced = reduce(
            &repository,
            &SnapshotRequest {
                as_of_valid_time: now(),
                space: None,
                purpose: None,
            },
        )
        .unwrap();
        unique_protocol(&reduced).unwrap()
    }

    fn add_params(protocol: &RevisionRef) -> Value {
        json!({
            "request_id": "memory-ui/add/test", "expected_protocol": protocol,
            "target": "user", "category": "preferences", "text": "回答先给出结论。", "claim_kind": "preference",
            "subject": {"kind": "vault-owner", "id": "owner:test", "relation_to_owner": "self"},
            "approval_kind": "self-representation", "trust_tier": "stable-preference",
            "risk_class": "informational", "salience": "normal", "polarity": "positive", "sensitivity": "normal",
            "context": {"spaces": ["global"], "applies_when": [], "excludes_when": []},
            "consent": {"scope": "personal-assistant-only", "allowed_purposes": ["planning"], "external_provider_policy": "deny"},
            "agent_use": {"guidance": "先给结论", "avoid_error": "不要扩张为外部行动授权"}
        })
    }

    fn pending_proposal(subject_id: &str) -> PendingProposalInput {
        PendingProposalInput {
            request_id: "agent/propose/test".into(),
            claim_id: None,
            text: "用户偏好先给出结论。".into(),
            claim_kind: ClaimKind::Preference,
            kind_data: KindData::Preference(PreferenceData {
                dimension: "response-style".into(),
            }),
            subject: Subject {
                kind: SubjectKind::VaultOwner,
                id: subject_id.into(),
                relation_to_owner: OwnerRelation::Self_,
            },
            asserted_by: vec![Assertion {
                kind: "human".into(),
                id: subject_id.into(),
                basis: "owner-stated".into(),
            }],
            recorded_by: Recorder {
                kind: "agent".into(),
                id: "agent:test".into(),
                device_id: "device:test".into(),
            },
            projection: Projection {
                target: ProjectionTarget::User,
                category: "preferences".into(),
                visibility: Visibility::Projection,
            },
            lifecycle: LifecycleState::Active,
            temporal: Temporal {
                valid_from: Some("2026-09-01T00:00:00Z".into()),
                ..Temporal::default()
            },
            epistemic: Epistemic {
                basis: "owner-stated".into(),
                representation_certainty: "high".into(),
                truth_status: "not-assessed".into(),
                truth_confidence: "unknown".into(),
            },
            trust_tier: TrustTier::StablePreference,
            risk_class: RiskClass::Informational,
            salience: Salience::Normal,
            polarity: Polarity::Positive,
            sensitivity: Sensitivity::Normal,
            context: ClaimContext {
                spaces: vec!["global".into()],
                applies_when: vec![],
                excludes_when: vec![],
            },
            consent: Consent {
                scope: "personal-assistant-only".into(),
                allowed_purposes: vec!["planning".into()],
                external_provider_policy: ExternalProviderPolicy::Deny,
            },
            agent_use: AgentUse {
                guidance: "先给出结论".into(),
                avoid_error: "不要提升为外部事实".into(),
            },
            evidence: vec![],
            dedupe_key: "agent:test:response-style".into(),
        }
    }

    #[test]
    fn uuid_generator_emits_v7_variant() {
        let id = Uuid::parse_str(&uuid_v7()).unwrap();
        assert_eq!(id.get_version_num(), 7);
    }

    #[test]
    fn absent_snapshot_is_read_only_and_migration_dry_run_writes_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let snapshot = dispatch(
            dir.path(),
            "host.memory.v2.snapshot",
            &json!({"as_of_valid_time": "2026-09-01T00:00:00Z"}),
        )
        .unwrap();
        assert_eq!(snapshot["mode"], "legacy");
        let migration = dispatch(
            dir.path(),
            "host.memory.v2.migrate",
            &json!({"mode": "dry-run"}),
        )
        .unwrap();
        assert_eq!(migration["writes_performed"], false);
        assert!(!dir.path().join(".notemd/memory/bootstrap.yaml").exists());
    }

    #[test]
    fn human_rpc_payload_cannot_forge_actor() {
        let parsed = serde_json::from_value::<AddInput>(json!({
            "request_id": "r", "expected_protocol": {"revision_id": "p", "payload_sha256": "h"},
            "target": "user", "category": "preferences", "text": "text", "claim_kind": "preference",
            "subject": {"kind": "vault-owner", "id": "owner", "relation_to_owner": "self"},
            "approval_kind": "self-representation", "trust_tier": "stable-preference",
            "risk_class": "informational", "salience": "normal", "polarity": "neutral", "sensitivity": "normal",
            "context": {"spaces": ["global"], "applies_when": [], "excludes_when": []},
            "consent": {"scope": "personal-assistant-only", "allowed_purposes": ["planning"], "external_provider_policy": "deny"},
            "agent_use": {"guidance": "", "avoid_error": ""}, "actor": "human:attacker"
        }));
        assert!(parsed.is_err());
    }

    #[test]
    fn add_is_one_approved_revision_bound_to_the_authority_owner() {
        let dir = tempfile::TempDir::new().unwrap();
        let protocol = initialize(dir.path());
        let receipt = dispatch(dir.path(), "host.memory.v2.add", &add_params(&protocol)).unwrap();
        assert_eq!(receipt["effective_status"], "active");
        assert_eq!(receipt["projection_rebuilt"], true);

        let repository = V2Repository::new(dir.path()).load().unwrap();
        assert_eq!(repository.claims.len(), 1);
        let claim = &repository.claims[0].value;
        assert_eq!(claim.workflow.state, WorkflowState::Approved);
        assert_eq!(claim.transition.operation, ClaimOperation::CreateApproved);
        assert_eq!(claim.subject.id, "owner:test");
        assert_eq!(claim.decision.as_ref().unwrap().actor_id, "human:test");
        assert_eq!(
            claim.decision.as_ref().unwrap().approval_kind,
            ApprovalKind::SelfRepresentation
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("USER.md")).unwrap(),
            "# USER\n\n## preferences\n\n- 回答先给出结论。\n"
        );
    }

    #[test]
    fn add_is_idempotent_and_restricted_or_stale_requests_fail_closed() {
        let dir = tempfile::TempDir::new().unwrap();
        let protocol = initialize(dir.path());
        let params = add_params(&protocol);
        let first = dispatch(dir.path(), "host.memory.v2.add", &params).unwrap();
        let repeated = dispatch(dir.path(), "host.memory.v2.add", &params).unwrap();
        assert_eq!(first["revision_id"], repeated["revision_id"]);
        assert_eq!(
            V2Repository::new(dir.path()).load().unwrap().claims.len(),
            1
        );

        let mut reused = params.clone();
        reused["text"] = json!("同一个 request_id 的不同内容。");
        let error = dispatch(dir.path(), "host.memory.v2.add", &reused).unwrap_err();
        assert!(error.contains("MEMORY_IDEMPOTENCY_CONFLICT"));

        let mut restricted = add_params(&protocol);
        restricted["request_id"] = json!("memory-ui/add/restricted");
        restricted["sensitivity"] = json!("restricted");
        let error = dispatch(dir.path(), "host.memory.v2.add", &restricted).unwrap_err();
        assert!(error.contains("MEMORY_RESTRICTED_PERSISTENCE_DENIED"));

        let mut stale = add_params(&protocol);
        stale["request_id"] = json!("memory-ui/add/stale");
        stale["expected_protocol"]["payload_sha256"] = json!("changed");
        let error = dispatch(dir.path(), "host.memory.v2.add", &stale).unwrap_err();
        assert!(error.contains("MEMORY_STALE_BASE"));
        assert_eq!(
            V2Repository::new(dir.path()).load().unwrap().claims.len(),
            1
        );
    }

    #[test]
    fn active_v2_repository_fences_every_legacy_rpc() {
        let dir = tempfile::TempDir::new().unwrap();
        initialize(dir.path());

        let error = crate::memory_control::dispatch(dir.path(), "host.memory.list", &json!({}))
            .unwrap_err();
        assert!(error.contains("MEMORY_PROTOCOL_V2_WRITE_FENCE"));
    }

    #[test]
    fn agent_proposal_is_owner_only_pending_until_one_click_approval() {
        let dir = tempfile::TempDir::new().unwrap();
        let protocol = initialize(dir.path());

        let error =
            propose_pending(dir.path(), pending_proposal("owner:someone-else")).unwrap_err();
        assert!(error.contains("MEMORY_UNAUTHORIZED"));

        let proposed = propose_pending(dir.path(), pending_proposal("owner:test")).unwrap();
        assert_eq!(proposed.workflow.state, WorkflowState::Pending);
        assert!(proposed.decision.is_none());
        assert!(!dir.path().join("USER.md").exists());

        let receipt = dispatch(
            dir.path(),
            "host.memory.v2.approve",
            &json!({
                "request_id": "memory-ui/approve/test",
                "expected_protocol": protocol,
                "expected_heads": [],
                "revision_id": proposed.revision_id,
                "expected_sha256": proposed.payload_sha256,
                "gesture_intent": "approve"
            }),
        )
        .unwrap();
        assert_eq!(receipt["effective_status"], "active");
        assert_eq!(
            fs::read_to_string(dir.path().join("USER.md")).unwrap(),
            "# USER\n\n## preferences\n\n- 用户偏好先给出结论。\n"
        );
    }

    #[test]
    fn context_preview_is_read_only_and_manifest_persists_selection_proof() {
        let dir = tempfile::TempDir::new().unwrap();
        let protocol = initialize(dir.path());
        dispatch(dir.path(), "host.memory.v2.add", &add_params(&protocol)).unwrap();
        let request = json!({
            "space": "global",
            "purpose": "planning",
            "caller": "agent:test",
            "provider": "local-test-provider",
            "model": "test-model",
            "tools": ["read"],
            "external_transfer": false,
            "as_of_valid_time": now()
        });

        let preview = dispatch(dir.path(), "host.memory.v2.context", &request).unwrap();
        assert_eq!(preview["selected"].as_array().unwrap().len(), 1);
        let manifest_dir = dir.path().join(".notemd/memory/context-manifests");
        assert!(!manifest_dir.exists());

        let receipt = dispatch(dir.path(), "host.memory.v2.contextManifest", &request).unwrap();
        assert_eq!(receipt["selected_count"], 1);
        assert_eq!(fs::read_dir(manifest_dir).unwrap().count(), 1);
    }

    #[test]
    fn two_git_clones_union_independent_claims_without_last_write_wins() {
        let root = tempfile::TempDir::new().unwrap();
        let bare = root.path().join("remote.git");
        git(
            root.path(),
            &["init", "--bare", "-q", "-b", "main", "remote.git"],
        );
        let first = root.path().join("first");
        fs::create_dir(&first).unwrap();
        git(&first, &["init", "-q", "-b", "main"]);
        configure_git(&first, "first@example.test");
        git(&first, &["remote", "add", "origin", bare.to_str().unwrap()]);
        initialize(&first);
        git(&first, &["add", "-A"]);
        git(&first, &["commit", "-q", "-m", "initialize memory v2"]);
        git(&first, &["push", "-q", "origin", "main"]);

        let second = root.path().join("second");
        git(root.path(), &["clone", "-q", "remote.git", "second"]);
        configure_git(&second, "second@example.test");

        let mut first_proposal = pending_proposal("owner:test");
        first_proposal.request_id = "agent/propose/first".into();
        first_proposal.dedupe_key = "device:first".into();
        first_proposal.text = "第一台设备记录的偏好。".into();
        propose_pending(&first, first_proposal).unwrap();

        let mut second_proposal = pending_proposal("owner:test");
        second_proposal.request_id = "agent/propose/second".into();
        second_proposal.dedupe_key = "device:second".into();
        second_proposal.text = "第二台设备记录的偏好。".into();
        propose_pending(&second, second_proposal).unwrap();

        crate::vault_sync::git_ops::sync(&second, "origin", "main").unwrap();
        crate::vault_sync::git_ops::sync(&first, "origin", "main").unwrap();

        let third = root.path().join("third");
        git(root.path(), &["clone", "-q", "remote.git", "third"]);
        let repository = V2Repository::new(&third).load().unwrap();
        assert_eq!(repository.mode, RepositoryMode::V2Active);
        assert_eq!(repository.claims.len(), 2);
        let texts = repository
            .claims
            .iter()
            .map(|item| item.value.text.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            texts,
            BTreeSet::from(["第一台设备记录的偏好。", "第二台设备记录的偏好。"])
        );
    }

    #[test]
    fn two_git_clones_expose_same_claim_siblings_as_a_conflict() {
        let root = tempfile::TempDir::new().unwrap();
        let bare = root.path().join("remote.git");
        git(
            root.path(),
            &["init", "--bare", "-q", "-b", "main", "remote.git"],
        );
        let first = root.path().join("first");
        fs::create_dir(&first).unwrap();
        git(&first, &["init", "-q", "-b", "main"]);
        configure_git(&first, "first@example.test");
        git(&first, &["remote", "add", "origin", bare.to_str().unwrap()]);
        let protocol = initialize(&first);
        dispatch(&first, "host.memory.v2.add", &add_params(&protocol)).unwrap();
        crate::vault_sync::git_ops::sync(&first, "origin", "main").unwrap();

        let repository = V2Repository::new(&first).load().unwrap();
        let base = repository.claims[0].value.clone();
        let base_ref = revision_ref(&base);
        let second = root.path().join("second");
        git(root.path(), &["clone", "-q", "remote.git", "second"]);
        configure_git(&second, "second@example.test");

        let mut first_proposal = pending_proposal("owner:test");
        first_proposal.request_id = "agent/replace/first".into();
        first_proposal.claim_id = Some(base.claim_id.clone());
        first_proposal.dedupe_key = "replace:first".into();
        first_proposal.text = "第一台设备修改后的偏好。".into();
        let first_pending = propose_pending(&first, first_proposal).unwrap();
        dispatch(
            &first,
            "host.memory.v2.approve",
            &json!({
                "request_id": "memory-ui/approve/first",
                "expected_protocol": protocol,
                "expected_heads": [base_ref],
                "revision_id": first_pending.revision_id,
                "expected_sha256": first_pending.payload_sha256,
                "gesture_intent": "approve"
            }),
        )
        .unwrap();

        let mut second_proposal = pending_proposal("owner:test");
        second_proposal.request_id = "agent/replace/second".into();
        second_proposal.claim_id = Some(base.claim_id.clone());
        second_proposal.dedupe_key = "replace:second".into();
        second_proposal.text = "第二台设备修改后的偏好。".into();
        let second_pending = propose_pending(&second, second_proposal).unwrap();
        dispatch(
            &second,
            "host.memory.v2.approve",
            &json!({
                "request_id": "memory-ui/approve/second",
                "expected_protocol": protocol,
                "expected_heads": [base_ref],
                "revision_id": second_pending.revision_id,
                "expected_sha256": second_pending.payload_sha256,
                "gesture_intent": "approve"
            }),
        )
        .unwrap();

        crate::vault_sync::git_ops::sync(&second, "origin", "main").unwrap();
        crate::vault_sync::git_ops::sync(&first, "origin", "main").unwrap();

        let third = root.path().join("third");
        git(root.path(), &["clone", "-q", "remote.git", "third"]);
        let repository = V2Repository::new(&third).load().unwrap();
        let snapshot = reduce(
            &repository,
            &SnapshotRequest {
                as_of_valid_time: now(),
                space: None,
                purpose: None,
            },
        )
        .unwrap();
        let claim = snapshot
            .claims
            .iter()
            .find(|claim| claim.claim_id == base.claim_id)
            .unwrap();
        assert!(claim.conflict.is_some());
        assert_eq!(claim.current_heads.len(), 2);
        assert!(!claim.projection_eligible);
        assert_eq!(
            fs::read_to_string(third.join("USER.md")).unwrap(),
            "# USER\n"
        );
    }
}
