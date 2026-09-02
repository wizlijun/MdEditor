use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct RevisionRef {
    pub revision_id: String,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct RecordRef {
    pub record_id: String,
    pub raw_sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CausalContext {
    #[serde(default)]
    pub parents: Vec<RecordRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bootstrap {
    pub schema: String,
    pub vault_id: String,
    pub protocol_family: String,
    pub initial_protocol_revision: RevisionRef,
    pub initial_authority_revision: RevisionRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Approve,
    Reject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextHeads {
    #[serde(default)]
    pub heads: Vec<RevisionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityContext {
    #[serde(default)]
    pub heads: Vec<RevisionRef>,
    pub capability: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlDecision {
    pub verdict: Verdict,
    pub actor_id: String,
    pub authority_context: AuthorityContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlOperation {
    Initialize,
    Replace,
    Resolve,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlTransition {
    pub operation: ControlOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolRevision {
    pub schema: String,
    pub revision_id: String,
    #[serde(default)]
    pub base_heads: Vec<RevisionRef>,
    #[serde(default)]
    pub causal_context: CausalContext,
    pub protocol_major: u32,
    pub protocol_minor: u32,
    pub renderer_version: String,
    pub claim_schema: String,
    pub category_registry: BTreeMap<String, Vec<String>>,
    pub decision: ControlDecision,
    pub transition: ControlTransition,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityOwner {
    pub owner_id: String,
    pub actor_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub actor_id: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "mode")]
pub enum Recovery {
    LocalOwnerSetup,
    Quorum {
        principals: Vec<String>,
        threshold: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityRevision {
    pub schema: String,
    pub revision_id: String,
    #[serde(default)]
    pub base_heads: Vec<RevisionRef>,
    #[serde(default)]
    pub causal_context: CausalContext,
    pub owner: AuthorityOwner,
    #[serde(default)]
    pub principals: Vec<Principal>,
    pub recovery: Recovery,
    pub decision: ControlDecision,
    pub transition: ControlTransition,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimKind {
    Identity,
    Preference,
    Boundary,
    Decision,
    Belief,
    Observation,
    Commitment,
    Practice,
    MaterialFact,
    Quotation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdentityType {
    Person,
    Role,
    Account,
    Relationship,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityData {
    pub identity_type: IdentityType,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceData {
    pub dimension: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyEffect {
    Deny,
    Allow,
    Prompt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BehaviorPolicy {
    pub effect: PolicyEffect,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub resources: Vec<String>,
    #[serde(default)]
    pub conditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryData {
    pub behavior_policy: BehaviorPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionData {
    pub made_by: String,
    pub decided_at: String,
    pub decision_scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeliefData {
    pub proposition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationData {
    pub observer: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitmentData {
    pub committed_by: String,
    pub beneficiary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PracticeData {
    pub practice_scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialFactData {
    pub proposition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotationData {
    pub speaker: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KindData {
    Identity(IdentityData),
    Preference(PreferenceData),
    Boundary(BoundaryData),
    Decision(DecisionData),
    Belief(BeliefData),
    Observation(ObservationData),
    Commitment(CommitmentData),
    Practice(PracticeData),
    MaterialFact(MaterialFactData),
    Quotation(QuotationData),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KindDataWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity: Option<IdentityData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preference: Option<PreferenceData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boundary: Option<BoundaryData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    decision: Option<DecisionData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    belief: Option<BeliefData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    observation: Option<ObservationData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commitment: Option<CommitmentData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    practice: Option<PracticeData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material_fact: Option<MaterialFactData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    quotation: Option<QuotationData>,
}

impl KindDataWire {
    fn empty() -> Self {
        Self {
            identity: None,
            preference: None,
            boundary: None,
            decision: None,
            belief: None,
            observation: None,
            commitment: None,
            practice: None,
            material_fact: None,
            quotation: None,
        }
    }
}

impl Serialize for KindData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut wire = KindDataWire::empty();
        match self {
            Self::Identity(value) => wire.identity = Some(value.clone()),
            Self::Preference(value) => wire.preference = Some(value.clone()),
            Self::Boundary(value) => wire.boundary = Some(value.clone()),
            Self::Decision(value) => wire.decision = Some(value.clone()),
            Self::Belief(value) => wire.belief = Some(value.clone()),
            Self::Observation(value) => wire.observation = Some(value.clone()),
            Self::Commitment(value) => wire.commitment = Some(value.clone()),
            Self::Practice(value) => wire.practice = Some(value.clone()),
            Self::MaterialFact(value) => wire.material_fact = Some(value.clone()),
            Self::Quotation(value) => wire.quotation = Some(value.clone()),
        }
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for KindData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = KindDataWire::deserialize(deserializer)?;
        let mut values = Vec::new();
        if let Some(value) = wire.identity {
            values.push(Self::Identity(value));
        }
        if let Some(value) = wire.preference {
            values.push(Self::Preference(value));
        }
        if let Some(value) = wire.boundary {
            values.push(Self::Boundary(value));
        }
        if let Some(value) = wire.decision {
            values.push(Self::Decision(value));
        }
        if let Some(value) = wire.belief {
            values.push(Self::Belief(value));
        }
        if let Some(value) = wire.observation {
            values.push(Self::Observation(value));
        }
        if let Some(value) = wire.commitment {
            values.push(Self::Commitment(value));
        }
        if let Some(value) = wire.practice {
            values.push(Self::Practice(value));
        }
        if let Some(value) = wire.material_fact {
            values.push(Self::MaterialFact(value));
        }
        if let Some(value) = wire.quotation {
            values.push(Self::Quotation(value));
        }
        if values.len() != 1 {
            return Err(serde::de::Error::custom(
                "kind_data must contain exactly one claim-kind member",
            ));
        }
        Ok(values.remove(0))
    }
}

impl KindData {
    pub fn kind(&self) -> ClaimKind {
        match self {
            Self::Identity(_) => ClaimKind::Identity,
            Self::Preference(_) => ClaimKind::Preference,
            Self::Boundary(_) => ClaimKind::Boundary,
            Self::Decision(_) => ClaimKind::Decision,
            Self::Belief(_) => ClaimKind::Belief,
            Self::Observation(_) => ClaimKind::Observation,
            Self::Commitment(_) => ClaimKind::Commitment,
            Self::Practice(_) => ClaimKind::Practice,
            Self::MaterialFact(_) => ClaimKind::MaterialFact,
            Self::Quotation(_) => ClaimKind::Quotation,
        }
    }

    pub fn behavior_policy(&self) -> Option<&BehaviorPolicy> {
        match self {
            Self::Boundary(data) => Some(&data.behavior_policy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SubjectKind {
    VaultOwner,
    Person,
    Project,
    Organization,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerRelation {
    #[serde(rename = "self")]
    Self_,
    Direct,
    SharedContext,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RepositoryMode {
    Absent,
    V2Active,
    V2Incomplete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subject {
    pub kind: SubjectKind,
    pub id: String,
    pub relation_to_owner: OwnerRelation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub kind: String,
    pub id: String,
    pub basis: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recorder {
    pub kind: String,
    pub id: String,
    pub device_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectionTarget {
    User,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    Projection,
    TrustedAgent,
    UiOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub target: ProjectionTarget,
    pub category: String,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowState {
    Pending,
    Approved,
    Rejected,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    pub state: WorkflowState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleState {
    Active,
    Revoked,
    Deleted,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lifecycle {
    pub state: LifecycleState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Temporal {
    pub uttered_at: Option<String>,
    pub observed_at: Option<String>,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    pub planned_for: Option<String>,
    pub due_at: Option<String>,
    pub review_after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Epistemic {
    pub basis: String,
    pub representation_certainty: String,
    pub truth_status: String,
    pub truth_confidence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum TrustTier {
    Identity,
    StablePreference,
    Contextual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum RiskClass {
    ActionSensitive,
    Behavioral,
    Informational,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Salience {
    Pinned,
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Polarity {
    Positive,
    Negative,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sensitivity {
    Normal,
    Private,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimContext {
    #[serde(default)]
    pub spaces: Vec<String>,
    #[serde(default)]
    pub applies_when: Vec<String>,
    #[serde(default)]
    pub excludes_when: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalProviderPolicy {
    Deny,
    Prompt,
    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consent {
    pub scope: String,
    #[serde(default)]
    pub allowed_purposes: Vec<String>,
    pub external_provider_policy: ExternalProviderPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentUse {
    pub guidance: String,
    pub avoid_error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalKind {
    SelfRepresentation,
    BehavioralAuthorization,
    FactualVerification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimDecision {
    pub verdict: Verdict,
    pub approval_kind: ApprovalKind,
    pub authority_scope: String,
    pub actor_id: String,
    pub decided_at: String,
    pub protocol_context: ContextHeads,
    pub authority_context: AuthorityContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimOperation {
    ProposeCreate,
    CreateApproved,
    ProposeReplace,
    Approve,
    Reject,
    Ignore,
    Replace,
    SetSalience,
    ReclassifyTrust,
    ReclassifyRisk,
    ChangeContextConsent,
    Revoke,
    Delete,
    Reinstate,
    RestoreDeleted,
    Resolve,
    MergeResult,
    MergeEffect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimTransition {
    pub operation: ClaimOperation,
    pub approves_revision_id: Option<String>,
    pub approves_payload_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceRelation {
    EvidenceOfSpeech,
    EvidenceOfObservation,
    EvidenceOfTruth,
    DerivedFrom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub relation: EvidenceRelation,
    pub resource: String,
    pub content_sha256: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineageRef {
    pub claim_id: String,
    pub revision_id: String,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lineage {
    #[serde(default)]
    pub derived_from: Vec<LineageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produced_by_operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produced_by_run: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryClaimRevision {
    pub schema: String,
    pub claim_id: String,
    pub revision_id: String,
    pub request_id: String,
    #[serde(default)]
    pub parents: Vec<RevisionRef>,
    #[serde(default)]
    pub causal_context: CausalContext,
    pub claim_kind: ClaimKind,
    pub kind_data: KindData,
    pub subject: Subject,
    #[serde(default)]
    pub asserted_by: Vec<Assertion>,
    pub recorded_by: Recorder,
    pub recorded_at: String,
    pub text: String,
    pub projection: Projection,
    pub workflow: Workflow,
    pub lifecycle: Lifecycle,
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
    pub decision: Option<ClaimDecision>,
    pub transition: ClaimTransition,
    #[serde(default)]
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub lineage: Lineage,
    pub dedupe_key: String,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplicationState {
    Current,
    Superseded,
    Quarantined,
    ClaimConflict,
    Expired,
    Future,
    Stale,
    Invalid,
    NoCurrent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct PolicyTuple {
    pub action: String,
    pub resource: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyOverlay {
    #[serde(default)]
    pub deny: Vec<PolicyTuple>,
    #[serde(default)]
    pub allow: Vec<PolicyTuple>,
    #[serde(default)]
    pub prompt: Vec<PolicyTuple>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConflictView {
    pub conflict_id: String,
    pub heads: Vec<RevisionRef>,
    pub risk_class: RiskClass,
    pub do_not_rely: bool,
    pub action_allowed: bool,
    pub safety_overlay: Option<SafetyOverlay>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimView {
    pub claim_id: String,
    pub as_of_valid_time: String,
    pub workflow_state: WorkflowState,
    pub lifecycle_state: Option<LifecycleState>,
    pub application_state: ApplicationState,
    pub current_heads: Vec<RevisionRef>,
    pub projection_eligible: bool,
    pub context_eligible: bool,
    pub do_not_rely: bool,
    pub conflict: Option<ConflictView>,
    pub text: Option<String>,
    pub projection: Option<Projection>,
    pub claim_kind: Option<ClaimKind>,
    pub salience: Option<Salience>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextClaim {
    pub claim_id: String,
    pub revision_id: String,
    pub payload_sha256: String,
    pub text: String,
    pub claim_kind: ClaimKind,
    pub risk_class: RiskClass,
    pub do_not_rely: bool,
    pub guidance: String,
    pub avoid_error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryContextV2 {
    pub request: ContextRequest,
    pub claims: Vec<ContextClaim>,
    pub conflicts: Vec<ConflictView>,
    pub action_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolView {
    pub heads: Vec<RevisionRef>,
    pub conflict: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityView {
    pub heads: Vec<RevisionRef>,
    pub conflict: bool,
    pub owner: Option<AuthorityOwner>,
    pub effective_capabilities: BTreeMap<String, Vec<String>>,
    pub action_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRequest {
    pub as_of_valid_time: String,
    pub space: Option<String>,
    pub purpose: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRequest {
    pub space: String,
    pub purpose: String,
    pub caller: String,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub tools: Vec<String>,
    pub external_transfer: bool,
    pub as_of_valid_time: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySnapshotV2 {
    pub as_of_valid_time: String,
    pub protocol: ProtocolView,
    pub authority: AuthorityView,
    pub claims: Vec<ClaimView>,
    pub action_allowed: bool,
    #[serde(default)]
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationGuard {
    pub request_id: String,
    pub expected_protocol: RevisionRef,
    #[serde(default)]
    pub expected_heads: Vec<RevisionRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GestureIntent {
    Approve,
    Reject,
    Ignore,
    Delete,
    Replace,
    SetSalience,
    Resolve,
    ResetAll,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingGestureRequest {
    pub request_id: String,
    pub expected_protocol: RevisionRef,
    #[serde(default)]
    pub expected_heads: Vec<RevisionRef>,
    pub revision_id: String,
    pub expected_sha256: String,
    pub gesture_intent: GestureIntent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextSelection {
    pub claim_id: String,
    pub revision_id: String,
    pub payload_sha256: String,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextPolicyResult {
    pub external_action_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextManifest {
    pub schema: String,
    pub manifest_id: String,
    pub request: ContextRequest,
    #[serde(default)]
    pub selected: Vec<ContextSelection>,
    pub excluded_summary: BTreeMap<String, u64>,
    #[serde(default)]
    pub conflicts: Vec<ConflictView>,
    pub policy_result: ContextPolicyResult,
    pub protocol_context: ContextHeads,
    pub authority_context: ContextHeads,
    #[serde(default)]
    pub causal_context: CausalContext,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeParticipant {
    pub claim_id: String,
    #[serde(default)]
    pub base_heads: Vec<RevisionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeInputs {
    pub target: MergeParticipant,
    #[serde(default)]
    pub sources: Vec<MergeParticipant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRevisionRef {
    pub claim_id: String,
    pub revision_id: String,
    pub payload_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeEffect {
    pub claim_id: String,
    pub revision_id: String,
    pub payload_sha256: String,
    pub lifecycle: LifecycleState,
    pub merged_into: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationKind {
    MergeClaims,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OperationState {
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationDecision {
    pub verdict: Verdict,
    pub actor_id: String,
    pub protocol_context: ContextHeads,
    pub authority_context: AuthorityContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryOperation {
    pub schema: String,
    pub operation_id: String,
    pub operation_kind: OperationKind,
    pub run_id: String,
    #[serde(default)]
    pub causal_context: CausalContext,
    pub merge_inputs: MergeInputs,
    pub result: OperationRevisionRef,
    #[serde(default)]
    pub effects: Vec<MergeEffect>,
    #[serde(default)]
    pub lineage: Vec<LineageRef>,
    pub decision: OperationDecision,
    pub state: OperationState,
    pub payload_sha256: String,
}
