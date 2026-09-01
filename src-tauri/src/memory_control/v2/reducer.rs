use super::canonical::raw_sha256;
use super::model::*;
use super::repository::{Loaded, RepositorySnapshot};
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReducerError {
    pub code: &'static str,
    pub message: String,
}

impl ReducerError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ReducerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ReducerError {}

pub fn reduce(
    repository: &RepositorySnapshot,
    request: &SnapshotRequest,
) -> Result<MemorySnapshotV2, ReducerError> {
    if repository.mode != RepositoryMode::V2Active {
        return Err(ReducerError::new(
            "MEMORY_PROTOCOL_NOT_ACTIVE",
            format!("repository mode is {:?}", repository.mode),
        ));
    }
    let as_of = parse_time(&request.as_of_valid_time, "as_of_valid_time")?;
    let global = GlobalDag::build(repository)?;
    let protocol = reduce_protocol(&repository.protocols)?;
    let authority = reduce_authority(&repository.authorities)?;
    let operation_activation = validate_operations(repository)?;
    let mut by_claim = BTreeMap::<String, Vec<&Loaded<MemoryClaimRevision>>>::new();
    for revision in &repository.claims {
        if let Some(operation_id) = &revision.value.lineage.produced_by_operation {
            if !operation_activation.active.contains(operation_id) {
                continue;
            }
        }
        by_claim
            .entry(revision.value.claim_id.clone())
            .or_default()
            .push(revision);
    }
    let mut claims = Vec::with_capacity(by_claim.len());
    for (claim_id, revisions) in by_claim {
        claims.push(reduce_claim(
            &claim_id,
            &revisions,
            &repository.authorities,
            &authority,
            &global,
            as_of,
            request,
        )?);
    }
    let action_sensitive_conflict = claims.iter().any(|claim| {
        claim
            .conflict
            .as_ref()
            .is_some_and(|conflict| conflict.risk_class == RiskClass::ActionSensitive)
    });
    let action_allowed =
        protocol.writable && authority.action_allowed && !action_sensitive_conflict;
    Ok(MemorySnapshotV2 {
        as_of_valid_time: request.as_of_valid_time.clone(),
        protocol,
        authority,
        claims,
        action_allowed,
        diagnostics: operation_activation.diagnostics,
    })
}

#[derive(Default)]
struct OperationActivation {
    active: BTreeSet<String>,
    diagnostics: Vec<String>,
}

fn validate_operations(
    repository: &RepositorySnapshot,
) -> Result<OperationActivation, ReducerError> {
    let mut out = OperationActivation::default();
    let claims_by_revision = repository
        .claims
        .iter()
        .map(|revision| (revision.value.revision_id.as_str(), &revision.value))
        .collect::<HashMap<_, _>>();
    for loaded in &repository.operations {
        let operation = &loaded.value;
        let mut reasons = Vec::new();
        if operation.schema != "notemd.memory/operation/v2"
            || operation.operation_kind != OperationKind::MergeClaims
            || operation.state != OperationState::Complete
            || operation.decision.verdict != Verdict::Approve
        {
            reasons.push("unsupported or incomplete operation envelope".to_string());
        }
        if operation.merge_inputs.sources.is_empty() {
            reasons.push("merge requires at least one source in addition to target".into());
        }
        let mut participants = BTreeSet::from([operation.merge_inputs.target.claim_id.clone()]);
        for source in &operation.merge_inputs.sources {
            if !participants.insert(source.claim_id.clone()) {
                reasons.push(format!("duplicate merge participant {}", source.claim_id));
            }
        }
        validate_operation_ref(
            operation,
            &operation.result,
            LifecycleState::Active,
            &claims_by_revision,
            &mut reasons,
        );
        for effect in &operation.effects {
            validate_operation_ref(
                operation,
                &OperationRevisionRef {
                    claim_id: effect.claim_id.clone(),
                    revision_id: effect.revision_id.clone(),
                    payload_sha256: effect.payload_sha256.clone(),
                },
                LifecycleState::Merged,
                &claims_by_revision,
                &mut reasons,
            );
            if effect.merged_into != operation.result.claim_id {
                reasons.push(format!(
                    "effect {} points to another merge target",
                    effect.claim_id
                ));
            }
        }
        let effect_claims = operation
            .effects
            .iter()
            .map(|effect| effect.claim_id.as_str())
            .collect::<BTreeSet<_>>();
        let source_claims = operation
            .merge_inputs
            .sources
            .iter()
            .map(|source| source.claim_id.as_str())
            .collect::<BTreeSet<_>>();
        if effect_claims != source_claims {
            reasons.push("merge effects do not exactly cover source claims".into());
        }
        for participant in
            std::iter::once(&operation.merge_inputs.target).chain(&operation.merge_inputs.sources)
        {
            for head in &participant.base_heads {
                let Some(revision) = claims_by_revision.get(head.revision_id.as_str()) else {
                    reasons.push(format!("missing merge input head {}", head.revision_id));
                    continue;
                };
                if revision.claim_id != participant.claim_id
                    || revision.payload_sha256 != head.payload_sha256
                {
                    reasons.push(format!("merge input head mismatch {}", head.revision_id));
                }
            }
        }
        if reasons.is_empty() {
            out.active.insert(operation.operation_id.clone());
        } else {
            out.diagnostics.push(format!(
                "MEMORY_PARTIAL_OPERATION {}: {}",
                operation.operation_id,
                reasons.join("; ")
            ));
        }
    }
    for revision in &repository.claims {
        if let Some(operation_id) = &revision.value.lineage.produced_by_operation {
            if !repository
                .operations
                .iter()
                .any(|operation| operation.value.operation_id == *operation_id)
            {
                out.diagnostics.push(format!(
                    "MEMORY_PARTIAL_OPERATION {}: revision {} has no complete manifest",
                    operation_id, revision.value.revision_id
                ));
            }
        }
    }
    out.diagnostics.sort();
    out.diagnostics.dedup();
    Ok(out)
}

fn validate_operation_ref(
    operation: &MemoryOperation,
    reference: &OperationRevisionRef,
    expected_lifecycle: LifecycleState,
    claims: &HashMap<&str, &MemoryClaimRevision>,
    reasons: &mut Vec<String>,
) {
    let Some(revision) = claims.get(reference.revision_id.as_str()) else {
        reasons.push(format!(
            "missing operation revision {}",
            reference.revision_id
        ));
        return;
    };
    if revision.claim_id != reference.claim_id
        || revision.payload_sha256 != reference.payload_sha256
        || revision.lifecycle.state != expected_lifecycle
        || revision.lineage.produced_by_operation.as_deref()
            != Some(operation.operation_id.as_str())
        || revision.lineage.produced_by_run.as_deref() != Some(operation.run_id.as_str())
    {
        reasons.push(format!(
            "operation revision mismatch {}",
            reference.revision_id
        ));
    }
}

fn reduce_protocol(revisions: &[Loaded<ProtocolRevision>]) -> Result<ProtocolView, ReducerError> {
    let nodes = revisions
        .iter()
        .map(|revision| Node {
            id: revision.value.revision_id.as_str(),
            hash: revision.value.payload_sha256.as_str(),
            parents: &revision.value.base_heads,
        })
        .collect::<Vec<_>>();
    validate_dag("protocol", &nodes)?;
    for revision in revisions {
        if revision.value.schema != "notemd.memory/protocol-revision/v2"
            || revision.value.protocol_major != 2
            || revision.value.decision.verdict != Verdict::Approve
        {
            return Err(ReducerError::new(
                "MEMORY_PROTOCOL_UNSUPPORTED",
                format!("invalid protocol revision {}", revision.value.revision_id),
            ));
        }
    }
    let heads = maximal_heads(&nodes, nodes.iter().map(|node| node.id))?;
    let refs = head_refs(&nodes, &heads);
    Ok(ProtocolView {
        conflict: refs.len() != 1,
        writable: refs.len() == 1,
        heads: refs,
    })
}

fn reduce_authority(
    revisions: &[Loaded<AuthorityRevision>],
) -> Result<AuthorityView, ReducerError> {
    let nodes = revisions
        .iter()
        .map(|revision| Node {
            id: revision.value.revision_id.as_str(),
            hash: revision.value.payload_sha256.as_str(),
            parents: &revision.value.base_heads,
        })
        .collect::<Vec<_>>();
    validate_dag("authority", &nodes)?;
    for revision in revisions {
        if revision.value.schema != "notemd.memory/authority-revision/v2"
            || revision.value.decision.verdict != Verdict::Approve
        {
            return Err(ReducerError::new(
                "MEMORY_AUTHORITY_INVALID",
                format!("invalid authority revision {}", revision.value.revision_id),
            ));
        }
    }
    let head_ids = maximal_heads(&nodes, nodes.iter().map(|node| node.id))?;
    let head_values = head_ids
        .iter()
        .map(|id| {
            revisions
                .iter()
                .find(|revision| &revision.value.revision_id == id)
                .map(|revision| &revision.value)
                .ok_or_else(|| {
                    ReducerError::new("MEMORY_AUTHORITY_INVALID", "missing authority head")
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut effective = head_values
        .first()
        .map(|head| principal_map(head))
        .unwrap_or_default();
    for head in head_values.iter().skip(1) {
        let next = principal_map(head);
        effective.retain(|actor, capabilities| {
            let Some(other) = next.get(actor) else {
                return false;
            };
            capabilities.retain(|capability| other.contains(capability));
            !capabilities.is_empty()
        });
    }
    let owner = head_values.first().and_then(|first| {
        head_values
            .iter()
            .all(|head| head.owner == first.owner)
            .then(|| first.owner.clone())
    });
    let heads = head_refs(&nodes, &head_ids);
    Ok(AuthorityView {
        conflict: heads.len() != 1,
        action_allowed: heads.len() == 1,
        heads,
        owner,
        effective_capabilities: effective
            .into_iter()
            .map(|(actor, capabilities)| (actor, capabilities.into_iter().collect()))
            .collect(),
    })
}

fn principal_map(revision: &AuthorityRevision) -> BTreeMap<String, BTreeSet<String>> {
    revision
        .principals
        .iter()
        .map(|principal| {
            (
                principal.actor_id.clone(),
                principal.capabilities.iter().cloned().collect(),
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn reduce_claim(
    claim_id: &str,
    revisions: &[&Loaded<MemoryClaimRevision>],
    authorities: &[Loaded<AuthorityRevision>],
    current_authority: &AuthorityView,
    global: &GlobalDag<'_>,
    as_of: DateTime<Utc>,
    request: &SnapshotRequest,
) -> Result<ClaimView, ReducerError> {
    let nodes = revisions
        .iter()
        .map(|revision| Node {
            id: revision.value.revision_id.as_str(),
            hash: revision.value.payload_sha256.as_str(),
            parents: &revision.value.parents,
        })
        .collect::<Vec<_>>();
    validate_dag(claim_id, &nodes)?;
    for revision in revisions {
        validate_claim_shape(&revision.value, revisions)?;
        validate_decision_authority(&revision.value, authorities, global)?;
    }

    let mut approved = Vec::new();
    for revision in revisions {
        if revision.value.workflow.state == WorkflowState::Approved
            && validity_contains(&revision.value.temporal, as_of)?
        {
            approved.push(revision.value.revision_id.as_str());
        }
    }
    let heads = maximal_heads(&nodes, approved.iter().copied())?;
    let head_values = heads
        .iter()
        .map(|id| {
            revisions
                .iter()
                .find(|revision| &revision.value.revision_id == id)
                .copied()
                .ok_or_else(|| ReducerError::new("MEMORY_INVALID_DAG", "missing claim head"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let current_heads = head_refs(&nodes, &heads);

    if head_values.len() > 1 {
        let risk = highest_risk(head_values.iter().map(|revision| revision.value.risk_class));
        let overlay = (risk == RiskClass::ActionSensitive)
            .then(|| safety_overlay(&head_values))
            .transpose()?;
        let conflict_id = conflict_id(&current_heads);
        return Ok(ClaimView {
            claim_id: claim_id.into(),
            as_of_valid_time: request.as_of_valid_time.clone(),
            workflow_state: WorkflowState::Approved,
            lifecycle_state: None,
            application_state: ApplicationState::ClaimConflict,
            current_heads: current_heads.clone(),
            projection_eligible: false,
            context_eligible: false,
            do_not_rely: true,
            conflict: Some(ConflictView {
                conflict_id,
                heads: current_heads,
                risk_class: risk,
                do_not_rely: true,
                action_allowed: false,
                safety_overlay: overlay,
            }),
            text: None,
            projection: None,
            claim_kind: None,
            salience: None,
        });
    }

    let Some(head) = head_values.first() else {
        let state = temporal_absence_state(revisions, as_of)?;
        return Ok(ClaimView {
            claim_id: claim_id.into(),
            as_of_valid_time: request.as_of_valid_time.clone(),
            workflow_state: revisions
                .iter()
                .map(|revision| revision.value.workflow.state)
                .find(|state| *state == WorkflowState::Pending)
                .unwrap_or(WorkflowState::Rejected),
            lifecycle_state: None,
            application_state: state,
            current_heads: Vec::new(),
            projection_eligible: false,
            context_eligible: false,
            do_not_rely: true,
            conflict: None,
            text: None,
            projection: None,
            claim_kind: None,
            salience: None,
        });
    };

    if authorization_concurrent_with_revoke(&head.value, authorities, current_authority, global)? {
        return Ok(ClaimView {
            claim_id: claim_id.into(),
            as_of_valid_time: request.as_of_valid_time.clone(),
            workflow_state: head.value.workflow.state,
            lifecycle_state: Some(head.value.lifecycle.state),
            application_state: ApplicationState::Quarantined,
            current_heads,
            projection_eligible: false,
            context_eligible: false,
            do_not_rely: true,
            conflict: None,
            text: None,
            projection: None,
            claim_kind: None,
            salience: None,
        });
    }

    let stale = head
        .value
        .temporal
        .review_after
        .as_deref()
        .map(|value| parse_time(value, "review_after").map(|time| time <= as_of))
        .transpose()?
        .unwrap_or(false);
    let active = head.value.lifecycle.state == LifecycleState::Active;
    let projection_eligible = active
        && !stale
        && head.value.projection.visibility == Visibility::Projection
        && head.value.sensitivity != Sensitivity::Restricted;
    let context_eligible = active && !stale && context_matches(&head.value, request);
    Ok(ClaimView {
        claim_id: claim_id.into(),
        as_of_valid_time: request.as_of_valid_time.clone(),
        workflow_state: head.value.workflow.state,
        lifecycle_state: Some(head.value.lifecycle.state),
        application_state: if stale {
            ApplicationState::Stale
        } else {
            ApplicationState::Current
        },
        current_heads,
        projection_eligible,
        context_eligible,
        do_not_rely: stale,
        conflict: None,
        text: (projection_eligible || context_eligible).then(|| head.value.text.clone()),
        projection: Some(head.value.projection.clone()),
        claim_kind: Some(head.value.claim_kind),
        salience: Some(head.value.salience),
    })
}

fn validate_claim_shape(
    revision: &MemoryClaimRevision,
    revisions: &[&Loaded<MemoryClaimRevision>],
) -> Result<(), ReducerError> {
    if revision.schema != "notemd.memory/claim-revision/v2" {
        return Err(ReducerError::new(
            "MEMORY_PROTOCOL_UNSUPPORTED",
            format!("unsupported claim schema in {}", revision.revision_id),
        ));
    }
    if revision.claim_kind != revision.kind_data.kind() {
        return Err(ReducerError::new(
            "MEMORY_INVALID_CLAIM",
            format!("claim_kind/kind_data mismatch in {}", revision.revision_id),
        ));
    }
    if revision.sensitivity == Sensitivity::Restricted {
        return Err(ReducerError::new(
            "MEMORY_RESTRICTED_PERSISTENCE_DENIED",
            format!("restricted claim {} is stored in Git", revision.revision_id),
        ));
    }
    if revision.asserted_by.is_empty()
        || revision.context.spaces.is_empty()
        || revision.consent.allowed_purposes.is_empty()
    {
        return Err(ReducerError::new(
            "MEMORY_INVALID_CLAIM",
            format!(
                "required semantic fields are empty in {}",
                revision.revision_id
            ),
        ));
    }
    match revision.workflow.state {
        WorkflowState::Pending if revision.decision.is_some() => {
            return Err(ReducerError::new(
                "MEMORY_INVALID_CLAIM",
                format!("pending revision {} has a decision", revision.revision_id),
            ))
        }
        WorkflowState::Approved
            if revision.decision.as_ref().map(|decision| decision.verdict)
                != Some(Verdict::Approve) =>
        {
            return Err(ReducerError::new(
                "MEMORY_INVALID_CLAIM",
                format!(
                    "approved revision {} lacks approve decision",
                    revision.revision_id
                ),
            ))
        }
        WorkflowState::Rejected | WorkflowState::Ignored if revision.decision.is_none() => {
            return Err(ReducerError::new(
                "MEMORY_INVALID_CLAIM",
                format!("decided revision {} lacks decision", revision.revision_id),
            ))
        }
        _ => {}
    }
    validate_transition(revision, revisions)
}

fn validate_transition(
    revision: &MemoryClaimRevision,
    revisions: &[&Loaded<MemoryClaimRevision>],
) -> Result<(), ReducerError> {
    let parent_values = revision
        .parents
        .iter()
        .map(|parent| {
            revisions
                .iter()
                .find(|candidate| candidate.value.revision_id == parent.revision_id)
                .map(|candidate| &candidate.value)
                .ok_or_else(|| ReducerError::new("MEMORY_INVALID_DAG", "missing transition parent"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let operation = revision.transition.operation;
    let invalid = |message: &str| {
        ReducerError::new(
            "MEMORY_INVALID_TRANSITION",
            format!("{}: {message}", revision.revision_id),
        )
    };
    match operation {
        ClaimOperation::ProposeCreate | ClaimOperation::CreateApproved => {
            if !parent_values.is_empty() {
                return Err(invalid("create must have no parents"));
            }
        }
        ClaimOperation::Resolve => {
            if parent_values.len() < 2 || revision.workflow.state != WorkflowState::Approved {
                return Err(invalid(
                    "resolve requires at least two parents and approval",
                ));
            }
        }
        ClaimOperation::MergeResult | ClaimOperation::MergeEffect => {
            if parent_values.is_empty()
                || revision.workflow.state != WorkflowState::Approved
                || revision.lineage.produced_by_operation.is_none()
                || revision.lineage.produced_by_run.is_none()
            {
                return Err(invalid(
                    "merge revision requires parent, approval and operation lineage",
                ));
            }
        }
        ClaimOperation::Approve | ClaimOperation::Reject | ClaimOperation::Ignore => {
            let proposed_id = revision
                .transition
                .approves_revision_id
                .as_deref()
                .ok_or_else(|| invalid("decision transition requires approves_revision_id"))?;
            let proposed = revisions
                .iter()
                .find(|candidate| candidate.value.revision_id == proposed_id)
                .ok_or_else(|| invalid("approved pending revision does not exist"))?;
            if proposed.value.workflow.state != WorkflowState::Pending
                || revision.transition.approves_payload_sha256.as_deref()
                    != Some(proposed.value.payload_sha256.as_str())
            {
                return Err(invalid("decision does not bind the exact pending payload"));
            }
        }
        _ if parent_values.len() != 1 => return Err(invalid("operation requires one parent")),
        _ => {}
    }
    if matches!(
        operation,
        ClaimOperation::Replace | ClaimOperation::SetSalience
    ) && parent_values
        .first()
        .is_some_and(|parent| !matches!(parent.lifecycle.state, LifecycleState::Active))
    {
        return Err(invalid("ordinary mutation cannot restore a closed claim"));
    }
    match operation {
        ClaimOperation::Revoke if revision.lifecycle.state != LifecycleState::Revoked => {
            Err(invalid("revoke must produce revoked lifecycle"))
        }
        ClaimOperation::Delete if revision.lifecycle.state != LifecycleState::Deleted => {
            Err(invalid("delete must produce deleted lifecycle"))
        }
        ClaimOperation::Reinstate | ClaimOperation::RestoreDeleted
            if revision.lifecycle.state != LifecycleState::Active =>
        {
            Err(invalid("restore must produce active lifecycle"))
        }
        ClaimOperation::MergeResult if revision.lifecycle.state != LifecycleState::Active => {
            Err(invalid("merge result must be active"))
        }
        ClaimOperation::MergeEffect if revision.lifecycle.state != LifecycleState::Merged => {
            Err(invalid("merge effect must be merged"))
        }
        _ => Ok(()),
    }
}

fn validate_decision_authority(
    revision: &MemoryClaimRevision,
    authorities: &[Loaded<AuthorityRevision>],
    global: &GlobalDag<'_>,
) -> Result<(), ReducerError> {
    let Some(decision) = &revision.decision else {
        return Ok(());
    };
    if decision.authority_context.heads.is_empty() || decision.protocol_context.heads.is_empty() {
        return Err(ReducerError::new(
            "MEMORY_UNAUTHORIZED",
            format!(
                "decision {} has incomplete control context",
                revision.revision_id
            ),
        ));
    }
    let mut capability_sets = Vec::new();
    for head in &decision.authority_context.heads {
        let authority = authorities
            .iter()
            .find(|authority| {
                authority.value.revision_id == head.revision_id
                    && authority.value.payload_sha256 == head.payload_sha256
            })
            .ok_or_else(|| {
                ReducerError::new(
                    "MEMORY_UNAUTHORIZED",
                    format!("unknown authority head {}", head.revision_id),
                )
            })?;
        if !global.is_ancestor(&authority.value.revision_id, &revision.revision_id)? {
            return Err(ReducerError::new(
                "MEMORY_UNAUTHORIZED",
                format!(
                    "authority head {} is not in decision causal history",
                    head.revision_id
                ),
            ));
        }
        capability_sets.push(principal_map(&authority.value));
    }
    let allowed = capability_sets.iter().all(|principals| {
        principals
            .get(&decision.actor_id)
            .is_some_and(|capabilities| {
                capabilities.contains(&decision.authority_context.capability)
            })
    });
    if !allowed {
        return Err(ReducerError::new(
            "MEMORY_UNAUTHORIZED",
            format!(
                "{} lacks {}",
                decision.actor_id, decision.authority_context.capability
            ),
        ));
    }
    Ok(())
}

fn authorization_concurrent_with_revoke(
    revision: &MemoryClaimRevision,
    authorities: &[Loaded<AuthorityRevision>],
    current: &AuthorityView,
    global: &GlobalDag<'_>,
) -> Result<bool, ReducerError> {
    let Some(decision) = &revision.decision else {
        return Ok(false);
    };
    for head in &current.heads {
        let authority = authorities
            .iter()
            .find(|authority| authority.value.revision_id == head.revision_id)
            .ok_or_else(|| ReducerError::new("MEMORY_AUTHORITY_INVALID", "missing current head"))?;
        let decision_before =
            global.is_ancestor(&revision.revision_id, &authority.value.revision_id)?;
        let authority_before =
            global.is_ancestor(&authority.value.revision_id, &revision.revision_id)?;
        if !decision_before && !authority_before {
            let grants = authority
                .value
                .principals
                .iter()
                .find(|principal| principal.actor_id == decision.actor_id)
                .is_some_and(|principal| {
                    principal
                        .capabilities
                        .contains(&decision.authority_context.capability)
                });
            if !grants {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn context_matches(revision: &MemoryClaimRevision, request: &SnapshotRequest) -> bool {
    if let Some(space) = &request.space {
        if !revision.context.spaces.contains(space) {
            return false;
        }
    }
    if let Some(purpose) = &request.purpose {
        if !revision.consent.allowed_purposes.contains(purpose) {
            return false;
        }
    }
    true
}

fn parse_time(value: &str, field: &str) -> Result<DateTime<Utc>, ReducerError> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|error| ReducerError::new("MEMORY_INVALID_TIME", format!("{field}: {error}")))
}

fn validity_contains(temporal: &Temporal, as_of: DateTime<Utc>) -> Result<bool, ReducerError> {
    let from = temporal
        .valid_from
        .as_deref()
        .map(|value| parse_time(value, "valid_from"))
        .transpose()?;
    let until = temporal
        .valid_until
        .as_deref()
        .map(|value| parse_time(value, "valid_until"))
        .transpose()?;
    if from.zip(until).is_some_and(|(from, until)| from >= until) {
        return Err(ReducerError::new(
            "MEMORY_INVALID_TIME",
            "validity interval must be half-open with from < until",
        ));
    }
    Ok(from.is_none_or(|from| from <= as_of) && until.is_none_or(|until| as_of < until))
}

fn temporal_absence_state(
    revisions: &[&Loaded<MemoryClaimRevision>],
    as_of: DateTime<Utc>,
) -> Result<ApplicationState, ReducerError> {
    let approved = revisions
        .iter()
        .filter(|revision| revision.value.workflow.state == WorkflowState::Approved)
        .collect::<Vec<_>>();
    if approved.is_empty() {
        return Ok(ApplicationState::NoCurrent);
    }
    let mut future = false;
    for revision in approved {
        if let Some(value) = revision.value.temporal.valid_from.as_deref() {
            future |= parse_time(value, "valid_from")? > as_of;
        }
    }
    Ok(if future {
        ApplicationState::Future
    } else {
        ApplicationState::Expired
    })
}

fn highest_risk(risks: impl Iterator<Item = RiskClass>) -> RiskClass {
    risks.fold(RiskClass::Informational, |current, risk| {
        match (current, risk) {
            (RiskClass::ActionSensitive, _) | (_, RiskClass::ActionSensitive) => {
                RiskClass::ActionSensitive
            }
            (RiskClass::Behavioral, _) | (_, RiskClass::Behavioral) => RiskClass::Behavioral,
            _ => RiskClass::Informational,
        }
    })
}

fn safety_overlay(heads: &[&Loaded<MemoryClaimRevision>]) -> Result<SafetyOverlay, ReducerError> {
    let mut deny = BTreeSet::new();
    let mut prompt = BTreeSet::new();
    let mut allow_intersection: Option<BTreeSet<PolicyTuple>> = None;
    for head in heads {
        let Some(policy) = head.value.kind_data.behavior_policy() else {
            allow_intersection = Some(BTreeSet::new());
            continue;
        };
        let tuples = policy
            .actions
            .iter()
            .flat_map(|action| {
                policy.resources.iter().map(move |resource| PolicyTuple {
                    action: action.clone(),
                    resource: resource.clone(),
                })
            })
            .collect::<BTreeSet<_>>();
        match policy.effect {
            PolicyEffect::Deny => {
                deny.extend(tuples);
                allow_intersection = Some(BTreeSet::new());
            }
            PolicyEffect::Prompt => {
                prompt.extend(tuples);
                allow_intersection = Some(BTreeSet::new());
            }
            PolicyEffect::Allow => {
                if let Some(current) = &mut allow_intersection {
                    current.retain(|tuple| tuples.contains(tuple));
                } else {
                    allow_intersection = Some(tuples);
                }
            }
        }
    }
    Ok(SafetyOverlay {
        deny: deny.into_iter().collect(),
        allow: allow_intersection.unwrap_or_default().into_iter().collect(),
        prompt: prompt.into_iter().collect(),
    })
}

fn conflict_id(heads: &[RevisionRef]) -> String {
    let mut values = heads
        .iter()
        .map(|head| format!("{}:{}", head.revision_id, head.payload_sha256))
        .collect::<Vec<_>>();
    values.sort();
    format!("sha256:{}", raw_sha256(values.join("\n").as_bytes()))
}

#[derive(Clone, Copy)]
struct Node<'a> {
    id: &'a str,
    hash: &'a str,
    parents: &'a [RevisionRef],
}

fn validate_dag(label: &str, nodes: &[Node<'_>]) -> Result<(), ReducerError> {
    let by_id = nodes
        .iter()
        .map(|node| (node.id, *node))
        .collect::<HashMap<_, _>>();
    if by_id.len() != nodes.len() {
        return Err(ReducerError::new(
            "MEMORY_INVALID_DAG",
            format!("duplicate node in {label}"),
        ));
    }
    for node in nodes {
        for parent in node.parents {
            let actual = by_id.get(parent.revision_id.as_str()).ok_or_else(|| {
                ReducerError::new(
                    "MEMORY_INVALID_DAG",
                    format!(
                        "{} references missing parent {}",
                        node.id, parent.revision_id
                    ),
                )
            })?;
            if actual.hash != parent.payload_sha256 {
                return Err(ReducerError::new(
                    "MEMORY_INVALID_DAG",
                    format!("{} parent hash mismatch {}", node.id, parent.revision_id),
                ));
            }
        }
    }
    for node in nodes {
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        visit(node.id, &by_id, &mut visiting, &mut visited)?;
    }
    Ok(())
}

fn visit<'a>(
    id: &'a str,
    nodes: &HashMap<&'a str, Node<'a>>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> Result<(), ReducerError> {
    if visited.contains(id) {
        return Ok(());
    }
    if !visiting.insert(id) {
        return Err(ReducerError::new(
            "MEMORY_INVALID_DAG",
            format!("cycle at {id}"),
        ));
    }
    if let Some(node) = nodes.get(id) {
        for parent in node.parents {
            visit(parent.revision_id.as_str(), nodes, visiting, visited)?;
        }
    }
    visiting.remove(id);
    visited.insert(id);
    Ok(())
}

fn maximal_heads<'a>(
    nodes: &[Node<'a>],
    candidates: impl Iterator<Item = &'a str>,
) -> Result<Vec<String>, ReducerError> {
    let by_id = nodes
        .iter()
        .map(|node| (node.id, *node))
        .collect::<HashMap<_, _>>();
    let candidates = candidates.map(str::to_owned).collect::<Vec<_>>();
    let mut heads = candidates
        .iter()
        .filter(|candidate| {
            !candidates.iter().any(|other| {
                candidate != &other
                    && is_ancestor_in(candidate.as_str(), other.as_str(), &by_id).unwrap_or(false)
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    heads.sort();
    Ok(heads)
}

fn is_ancestor_in<'a>(
    ancestor: &str,
    descendant: &str,
    nodes: &HashMap<&'a str, Node<'a>>,
) -> Result<bool, ReducerError> {
    if ancestor == descendant {
        return Ok(true);
    }
    let mut stack = vec![descendant];
    let mut seen = HashSet::new();
    while let Some(id) = stack.pop() {
        if !seen.insert(id.to_string()) {
            continue;
        }
        let node = nodes
            .get(id)
            .ok_or_else(|| ReducerError::new("MEMORY_INVALID_DAG", format!("missing node {id}")))?;
        for parent in node.parents {
            if parent.revision_id == ancestor {
                return Ok(true);
            }
            stack.push(parent.revision_id.as_str());
        }
    }
    Ok(false)
}

fn head_refs(nodes: &[Node<'_>], heads: &[String]) -> Vec<RevisionRef> {
    let by_id = nodes
        .iter()
        .map(|node| (node.id, node.hash))
        .collect::<HashMap<_, _>>();
    heads
        .iter()
        .map(|id| RevisionRef {
            revision_id: id.clone(),
            payload_sha256: by_id.get(id.as_str()).copied().unwrap_or_default().into(),
        })
        .collect()
}

struct GlobalDag<'a> {
    parents: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> GlobalDag<'a> {
    fn build(repository: &'a RepositorySnapshot) -> Result<Self, ReducerError> {
        let mut raw = HashMap::<&str, &str>::new();
        let mut parents = HashMap::<&str, Vec<&str>>::new();
        for revision in &repository.protocols {
            insert_global(
                &mut raw,
                &mut parents,
                &revision.value.revision_id,
                &revision.raw_sha256,
                &revision.value.causal_context,
            )?;
        }
        for revision in &repository.authorities {
            insert_global(
                &mut raw,
                &mut parents,
                &revision.value.revision_id,
                &revision.raw_sha256,
                &revision.value.causal_context,
            )?;
        }
        for revision in &repository.claims {
            insert_global(
                &mut raw,
                &mut parents,
                &revision.value.revision_id,
                &revision.raw_sha256,
                &revision.value.causal_context,
            )?;
        }
        for operation in &repository.operations {
            insert_global(
                &mut raw,
                &mut parents,
                &operation.value.operation_id,
                &operation.raw_sha256,
                &operation.value.causal_context,
            )?;
        }
        for manifest in &repository.context_manifests {
            insert_global(
                &mut raw,
                &mut parents,
                &manifest.value.manifest_id,
                &manifest.raw_sha256,
                &manifest.value.causal_context,
            )?;
        }
        for (id, direct) in &parents {
            for parent in direct {
                if !parents.contains_key(parent) {
                    return Err(ReducerError::new(
                        "MEMORY_INVALID_DAG",
                        format!("global record {id} references missing {parent}"),
                    ));
                }
            }
        }
        for revision in
            repository
                .protocols
                .iter()
                .map(|revision| (&revision.value.revision_id, &revision.value.causal_context))
                .chain(
                    repository.authorities.iter().map(|revision| {
                        (&revision.value.revision_id, &revision.value.causal_context)
                    }),
                )
                .chain(
                    repository.claims.iter().map(|revision| {
                        (&revision.value.revision_id, &revision.value.causal_context)
                    }),
                )
                .chain(repository.operations.iter().map(|operation| {
                    (
                        &operation.value.operation_id,
                        &operation.value.causal_context,
                    )
                }))
                .chain(
                    repository.context_manifests.iter().map(|manifest| {
                        (&manifest.value.manifest_id, &manifest.value.causal_context)
                    }),
                )
        {
            for parent in &revision.1.parents {
                if raw.get(parent.record_id.as_str()).copied() != Some(parent.raw_sha256.as_str()) {
                    return Err(ReducerError::new(
                        "MEMORY_INVALID_DAG",
                        format!("global parent raw hash mismatch in {}", revision.0),
                    ));
                }
            }
        }
        let dag = Self { parents };
        for id in dag.parents.keys() {
            if dag.is_ancestor(id, id)? && dag.has_strict_cycle(id)? {
                return Err(ReducerError::new(
                    "MEMORY_INVALID_DAG",
                    format!("global cycle at {id}"),
                ));
            }
        }
        Ok(dag)
    }

    fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, ReducerError> {
        if ancestor == descendant {
            return Ok(true);
        }
        let mut stack = vec![descendant];
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id.to_string()) {
                continue;
            }
            let direct = self.parents.get(id).ok_or_else(|| {
                ReducerError::new("MEMORY_INVALID_DAG", format!("unknown global record {id}"))
            })?;
            for parent in direct {
                if *parent == ancestor {
                    return Ok(true);
                }
                stack.push(parent);
            }
        }
        Ok(false)
    }

    fn has_strict_cycle(&self, start: &str) -> Result<bool, ReducerError> {
        let mut stack = self.parents.get(start).cloned().unwrap_or_default();
        let mut seen = HashSet::new();
        while let Some(id) = stack.pop() {
            if id == start {
                return Ok(true);
            }
            if seen.insert(id.to_string()) {
                stack.extend(self.parents.get(id).cloned().unwrap_or_default());
            }
        }
        Ok(false)
    }
}

fn insert_global<'a>(
    raw: &mut HashMap<&'a str, &'a str>,
    parents: &mut HashMap<&'a str, Vec<&'a str>>,
    id: &'a str,
    raw_hash: &'a str,
    causal: &'a CausalContext,
) -> Result<(), ReducerError> {
    if raw.insert(id, raw_hash).is_some() {
        return Err(ReducerError::new(
            "MEMORY_TAMPERED_ASSET",
            format!("duplicate global record id {id}"),
        ));
    }
    parents.insert(
        id,
        causal
            .parents
            .iter()
            .map(|parent| parent.record_id.as_str())
            .collect(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const P_ID: &str = "01900000-0000-7000-8000-000000000001";
    const A_ID: &str = "01900000-0000-7000-8000-000000000002";
    const P_HASH: &str = "pppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppp";
    const A_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const P_RAW: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const A_RAW: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn loaded<T>(id: &str, raw: &str, value: T) -> Loaded<T> {
        Loaded {
            path: PathBuf::from(format!("{id}.yaml")),
            raw_sha256: raw.into(),
            value,
        }
    }

    fn protocol() -> Loaded<ProtocolRevision> {
        loaded(
            P_ID,
            P_RAW,
            ProtocolRevision {
                schema: "notemd.memory/protocol-revision/v2".into(),
                revision_id: P_ID.into(),
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
                    actor_id: "human:bruce".into(),
                    authority_context: AuthorityContext {
                        heads: vec![],
                        capability: "bootstrap".into(),
                    },
                },
                transition: ControlTransition {
                    operation: ControlOperation::Initialize,
                },
                payload_sha256: P_HASH.into(),
            },
        )
    }

    fn authority_with(id: &str, raw: &str, capabilities: &[&str]) -> Loaded<AuthorityRevision> {
        loaded(
            id,
            raw,
            AuthorityRevision {
                schema: "notemd.memory/authority-revision/v2".into(),
                revision_id: id.into(),
                base_heads: vec![],
                causal_context: CausalContext {
                    parents: vec![RecordRef {
                        record_id: P_ID.into(),
                        raw_sha256: P_RAW.into(),
                    }],
                },
                owner: AuthorityOwner {
                    owner_id: "owner:bruce".into(),
                    actor_id: "human:bruce".into(),
                },
                principals: vec![Principal {
                    actor_id: "human:bruce".into(),
                    capabilities: capabilities.iter().map(|value| (*value).into()).collect(),
                }],
                recovery: Recovery::LocalOwnerSetup,
                decision: ControlDecision {
                    verdict: Verdict::Approve,
                    actor_id: "human:bruce".into(),
                    authority_context: AuthorityContext {
                        heads: vec![],
                        capability: "bootstrap".into(),
                    },
                },
                transition: ControlTransition {
                    operation: ControlOperation::Initialize,
                },
                payload_sha256: if id == A_ID {
                    A_HASH.into()
                } else {
                    format!("{:0<64}", &id[id.len() - 4..])
                },
            },
        )
    }

    fn authority() -> Loaded<AuthorityRevision> {
        authority_with(
            A_ID,
            A_RAW,
            &["memory.claim.approve", "memory.claim.resolve"],
        )
    }

    fn claim(
        id: &str,
        claim_id: &str,
        from: &str,
        until: Option<&str>,
        policy: Option<PolicyEffect>,
    ) -> Loaded<MemoryClaimRevision> {
        let (kind, data, risk, category) = if let Some(effect) = policy {
            (
                ClaimKind::Boundary,
                KindData::Boundary(BoundaryData {
                    behavior_policy: BehaviorPolicy {
                        effect,
                        actions: vec!["send-message".into()],
                        resources: vec!["external-recipient".into()],
                        conditions: vec![],
                    },
                }),
                RiskClass::ActionSensitive,
                "boundaries",
            )
        } else {
            (
                ClaimKind::Preference,
                KindData::Preference(PreferenceData {
                    dimension: "response-style".into(),
                }),
                RiskClass::Informational,
                "preferences",
            )
        };
        let hash = format!(
            "{:0<64}",
            id.chars()
                .filter(|c| c.is_ascii_hexdigit())
                .collect::<String>()
        );
        loaded(
            id,
            &format!("{:1<64}", id),
            MemoryClaimRevision {
                schema: "notemd.memory/claim-revision/v2".into(),
                claim_id: claim_id.into(),
                revision_id: id.into(),
                request_id: format!("test/{id}"),
                parents: vec![],
                causal_context: CausalContext {
                    parents: vec![
                        RecordRef {
                            record_id: P_ID.into(),
                            raw_sha256: P_RAW.into(),
                        },
                        RecordRef {
                            record_id: A_ID.into(),
                            raw_sha256: A_RAW.into(),
                        },
                    ],
                },
                claim_kind: kind,
                kind_data: data,
                subject: Subject {
                    kind: SubjectKind::VaultOwner,
                    id: "owner:bruce".into(),
                    relation_to_owner: OwnerRelation::Self_,
                },
                asserted_by: vec![Assertion {
                    kind: "owner".into(),
                    id: "owner:bruce".into(),
                    basis: "direct-input".into(),
                }],
                recorded_by: Recorder {
                    kind: "host".into(),
                    id: "notemd.memory-ui".into(),
                    device_id: "device:test".into(),
                },
                recorded_at: "2026-01-01T00:00:00Z".into(),
                text: format!("claim {id}"),
                projection: Projection {
                    target: ProjectionTarget::User,
                    category: category.into(),
                    visibility: Visibility::Projection,
                },
                workflow: Workflow {
                    state: WorkflowState::Approved,
                },
                lifecycle: Lifecycle {
                    state: LifecycleState::Active,
                },
                temporal: Temporal {
                    valid_from: Some(from.into()),
                    valid_until: until.map(str::to_owned),
                    ..Temporal::default()
                },
                epistemic: Epistemic {
                    basis: "owner-stated".into(),
                    representation_certainty: "high".into(),
                    truth_status: "not-assessed".into(),
                    truth_confidence: "unknown".into(),
                },
                trust_tier: TrustTier::StablePreference,
                risk_class: risk,
                salience: Salience::Normal,
                polarity: Polarity::Neutral,
                sensitivity: Sensitivity::Normal,
                context: ClaimContext {
                    spaces: vec!["global".into()],
                    applies_when: vec![],
                    excludes_when: vec![],
                },
                consent: Consent {
                    scope: "personal-assistant-only".into(),
                    allowed_purposes: vec!["information-answer".into()],
                    external_provider_policy: ExternalProviderPolicy::Allow,
                },
                agent_use: AgentUse {
                    guidance: "use carefully".into(),
                    avoid_error: "do not infer more".into(),
                },
                decision: Some(ClaimDecision {
                    verdict: Verdict::Approve,
                    approval_kind: if policy.is_some() {
                        ApprovalKind::BehavioralAuthorization
                    } else {
                        ApprovalKind::SelfRepresentation
                    },
                    authority_scope: "personal-assistant".into(),
                    actor_id: "human:bruce".into(),
                    decided_at: "2026-01-01T00:00:00Z".into(),
                    protocol_context: ContextHeads {
                        heads: vec![RevisionRef {
                            revision_id: P_ID.into(),
                            payload_sha256: P_HASH.into(),
                        }],
                    },
                    authority_context: AuthorityContext {
                        heads: vec![RevisionRef {
                            revision_id: A_ID.into(),
                            payload_sha256: A_HASH.into(),
                        }],
                        capability: "memory.claim.approve".into(),
                    },
                }),
                transition: ClaimTransition {
                    operation: ClaimOperation::CreateApproved,
                    approves_revision_id: None,
                    approves_payload_sha256: None,
                },
                evidence: vec![],
                lineage: Lineage::default(),
                dedupe_key: format!("claim/{id}"),
                payload_sha256: hash,
            },
        )
    }

    fn child(
        mut value: Loaded<MemoryClaimRevision>,
        id: &str,
        parents: &[&Loaded<MemoryClaimRevision>],
        operation: ClaimOperation,
    ) -> Loaded<MemoryClaimRevision> {
        value.value.revision_id = id.into();
        value.value.request_id = format!("test/{id}");
        value.value.parents = parents
            .iter()
            .map(|parent| RevisionRef {
                revision_id: parent.value.revision_id.clone(),
                payload_sha256: parent.value.payload_sha256.clone(),
            })
            .collect();
        value.value.transition.operation = operation;
        value.value.payload_sha256 = format!("{:0<64}", id);
        value.path = PathBuf::from(format!("{id}.yaml"));
        value.raw_sha256 = format!("{:9<64}", id);
        value
    }

    fn repository(claims: Vec<Loaded<MemoryClaimRevision>>) -> RepositorySnapshot {
        RepositorySnapshot {
            mode: RepositoryMode::V2Active,
            bootstrap: None,
            protocols: vec![protocol()],
            authorities: vec![authority()],
            claims,
            operations: vec![],
            context_manifests: vec![],
            diagnostics: vec![],
        }
    }

    fn request(at: &str) -> SnapshotRequest {
        SnapshotRequest {
            as_of_valid_time: at.into(),
            space: Some("global".into()),
            purpose: Some("information-answer".into()),
        }
    }

    #[test]
    fn unique_approved_head_is_current_and_projectable() {
        let snapshot = reduce(
            &repository(vec![claim(
                "c1",
                "claim-1",
                "2026-01-01T00:00:00Z",
                None,
                None,
            )]),
            &request("2026-09-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(
            snapshot.claims[0].application_state,
            ApplicationState::Current
        );
        assert!(snapshot.claims[0].projection_eligible);
        assert!(snapshot.action_allowed);
    }

    #[test]
    fn concurrent_non_overlapping_validity_is_not_a_current_conflict() {
        let repo = repository(vec![
            claim(
                "c1",
                "claim-1",
                "2025-01-01T00:00:00Z",
                Some("2026-01-01T00:00:00Z"),
                None,
            ),
            claim("c2", "claim-1", "2026-01-01T00:00:00Z", None, None),
        ]);
        let old = reduce(&repo, &request("2025-06-01T00:00:00Z")).unwrap();
        let new = reduce(&repo, &request("2026-06-01T00:00:00Z")).unwrap();
        assert_eq!(old.claims[0].current_heads[0].revision_id, "c1");
        assert_eq!(new.claims[0].current_heads[0].revision_id, "c2");
        assert!(old.claims[0].conflict.is_none());
        assert!(new.claims[0].conflict.is_none());
    }

    #[test]
    fn action_sensitive_siblings_deny_union_and_never_restore_allow() {
        let repo = repository(vec![
            claim(
                "c1",
                "claim-1",
                "2026-01-01T00:00:00Z",
                None,
                Some(PolicyEffect::Deny),
            ),
            claim(
                "c2",
                "claim-1",
                "2026-01-01T00:00:00Z",
                None,
                Some(PolicyEffect::Allow),
            ),
        ]);
        let snapshot = reduce(&repo, &request("2026-09-01T00:00:00Z")).unwrap();
        let conflict = snapshot.claims[0].conflict.as_ref().unwrap();
        assert_eq!(conflict.risk_class, RiskClass::ActionSensitive);
        assert_eq!(conflict.safety_overlay.as_ref().unwrap().deny.len(), 1);
        assert!(conflict.safety_overlay.as_ref().unwrap().allow.is_empty());
        assert!(!snapshot.action_allowed);
    }

    #[test]
    fn concurrent_authority_heads_use_capability_intersection_and_close_actions() {
        let mut repo = repository(vec![]);
        repo.authorities.push(authority_with(
            "01900000-0000-7000-8000-000000000003",
            "3333333333333333333333333333333333333333333333333333333333333333",
            &["memory.claim.resolve"],
        ));
        let snapshot = reduce(&repo, &request("2026-09-01T00:00:00Z")).unwrap();
        assert!(snapshot.authority.conflict);
        assert!(!snapshot.authority.action_allowed);
        assert!(!snapshot.authority.effective_capabilities["human:bruce"]
            .contains(&"memory.claim.approve".to_string()));
        assert!(snapshot.authority.effective_capabilities["human:bruce"]
            .contains(&"memory.claim.resolve".to_string()));
    }

    #[test]
    fn pending_never_overrides_an_approved_head() {
        let active = claim("c1", "claim-1", "2026-01-01T00:00:00Z", None, None);
        let mut pending = child(
            active.clone(),
            "c2",
            &[&active],
            ClaimOperation::ProposeReplace,
        );
        pending.value.workflow.state = WorkflowState::Pending;
        pending.value.decision = None;
        let snapshot = reduce(
            &repository(vec![pending, active]),
            &request("2026-09-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(snapshot.claims[0].current_heads[0].revision_id, "c1");
        assert_eq!(
            snapshot.claims[0].application_state,
            ApplicationState::Current
        );
    }

    #[test]
    fn approved_descendant_supersedes_its_parent_regardless_of_input_order() {
        let parent = claim("c1", "claim-1", "2026-01-01T00:00:00Z", None, None);
        let child = child(parent.clone(), "c2", &[&parent], ClaimOperation::Replace);
        let first = reduce(
            &repository(vec![parent.clone(), child.clone()]),
            &request("2026-09-01T00:00:00Z"),
        )
        .unwrap();
        let second = reduce(
            &repository(vec![child, parent]),
            &request("2026-09-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(first.claims[0].current_heads[0].revision_id, "c2");
    }

    #[test]
    fn duplicate_revision_id_is_an_integrity_error() {
        let first = claim("c1", "claim-1", "2026-01-01T00:00:00Z", None, None);
        let mut duplicate = first.clone();
        duplicate.value.text = "different".into();
        let error = reduce(
            &repository(vec![first, duplicate]),
            &request("2026-09-01T00:00:00Z"),
        )
        .unwrap_err();
        assert_eq!(error.code, "MEMORY_TAMPERED_ASSET");
    }

    #[test]
    fn late_sibling_reopens_a_resolved_conflict() {
        let first = claim("c1", "claim-1", "2026-01-01T00:00:00Z", None, None);
        let second = claim("c2", "claim-1", "2026-01-01T00:00:00Z", None, None);
        let resolved = child(
            first.clone(),
            "c3",
            &[&first, &second],
            ClaimOperation::Resolve,
        );
        let settled = reduce(
            &repository(vec![first.clone(), second.clone(), resolved.clone()]),
            &request("2026-09-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(settled.claims[0].current_heads[0].revision_id, "c3");

        let late = claim("c4", "claim-1", "2026-01-01T00:00:00Z", None, None);
        let reopened = reduce(
            &repository(vec![first, second, resolved, late]),
            &request("2026-09-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(
            reopened.claims[0].application_state,
            ApplicationState::ClaimConflict
        );
        assert_eq!(reopened.claims[0].current_heads.len(), 2);
    }

    #[test]
    fn revision_from_incomplete_operation_is_not_activated() {
        let base = claim("c1", "claim-1", "2026-01-01T00:00:00Z", None, None);
        let mut result = child(base.clone(), "c2", &[&base], ClaimOperation::MergeResult);
        result.value.lineage.produced_by_operation = Some("missing-operation".into());
        result.value.lineage.produced_by_run = Some("merge/missing".into());
        let snapshot = reduce(
            &repository(vec![base, result]),
            &request("2026-09-01T00:00:00Z"),
        )
        .unwrap();
        assert_eq!(snapshot.claims[0].current_heads[0].revision_id, "c1");
        assert!(snapshot.diagnostics[0].contains("MEMORY_PARTIAL_OPERATION"));
    }

    #[test]
    fn invalid_valid_time_fails_closed_instead_of_disappearing() {
        let mut invalid = claim("c1", "claim-1", "2026-01-01T00:00:00Z", None, None);
        invalid.value.temporal.valid_from = Some("not-a-time".into());
        let error =
            reduce(&repository(vec![invalid]), &request("2026-09-01T00:00:00Z")).unwrap_err();
        assert_eq!(error.code, "MEMORY_INVALID_TIME");
    }
}
