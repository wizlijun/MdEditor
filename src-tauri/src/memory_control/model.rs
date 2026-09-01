use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scope {
    UserOwner,
    UserProfile,
    Memory,
}

impl Scope {
    pub fn document(self) -> &'static str {
        match self {
            Self::UserOwner | Self::UserProfile => "USER.md",
            Self::Memory => "MEMORY.md",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    Create,
    Replace,
    Merge,
    Revoke,
    Delete,
    SetPriority,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Critical,
    High,
    Normal,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Polarity {
    Positive,
    Negative,
    Neutral,
}

impl Default for Polarity {
    fn default() -> Self {
        Self::Neutral
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EpistemicStatus {
    OwnerStated,
    SourceSupported,
    Inferred,
    Contested,
    Unknown,
}

impl Default for EpistemicStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Certainty {
    High,
    Medium,
    Low,
    Unknown,
}

impl Default for Certainty {
    fn default() -> Self {
        Self::Unknown
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalSpec {
    pub version: u32,
    pub id: String,
    pub scope: Scope,
    pub operation: Operation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_polarity: Option<Polarity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_epistemic_status: Option<EpistemicStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_certainty: Option<Certainty>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_agent_guidance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_avoid_error: Option<String>,
    pub dedupe_key: String,
    #[serde(default)]
    pub action_sensitive: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merge_from: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Generated {
    pub by: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub resource: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalFrontmatter {
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub created: String,
    pub proposal: ProposalSpec,
    pub generated: Generated,
    pub sources: Vec<Source>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    #[serde(flatten)]
    pub frontmatter: ProposalFrontmatter,
    pub text: String,
    #[serde(default)]
    pub reason: String,
    pub path: String,
    pub sha256: String,
    pub decision: ProposalDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalDecision {
    Pending,
    Approved,
    Rejected,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSpec {
    pub version: u32,
    pub id: String,
    pub action: DecisionAction,
    pub proposal_id: String,
    pub proposal_sha256: String,
    pub entry_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_revision: Option<u64>,
    pub revision: u64,
    pub decided_by: String,
    pub decided_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecisionAction {
    Approve,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFrontmatter {
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub created: String,
    pub event: EventSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub scope: Scope,
    pub section: String,
    pub text: String,
    pub revision: u64,
    pub status: String,
    pub priority: Priority,
    #[serde(default)]
    pub polarity: Polarity,
    #[serde(default)]
    pub epistemic_status: EpistemicStatus,
    #[serde(default)]
    pub certainty: Certainty,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_guidance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avoid_error: Option<String>,
    #[serde(default)]
    pub classification_complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub document: String,
    pub legacy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Integrity {
    pub managed: bool,
    pub drift: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub entries: Vec<MemoryEntry>,
    pub proposals: Vec<Proposal>,
    pub integrity: Integrity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_actor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposeInput {
    pub scope: Scope,
    pub operation: Operation,
    pub text: String,
    pub source: String,
    pub by: String,
    pub dedupe_key: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polarity: Option<Polarity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epistemic_status: Option<EpistemicStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certainty: Option<Certainty>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_guidance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avoid_error: Option<String>,
    #[serde(default)]
    pub merge_from: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecideInput {
    pub proposal_id: String,
    pub expected_sha256: String,
    pub action: DecisionAction,
    pub actor: String,
    #[serde(default)]
    pub human_confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManagedState {
    pub protocol: u32,
    pub revision: u64,
    #[serde(default)]
    pub user_hash: String,
    #[serde(default)]
    pub memory_hash: String,
}
