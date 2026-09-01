export type ClaimKind =
  | 'identity'
  | 'preference'
  | 'boundary'
  | 'decision'
  | 'belief'
  | 'observation'
  | 'commitment'
  | 'practice'
  | 'material-fact'
  | 'quotation'
  | 'legacy-unclassified'

export type ProjectionTarget = 'user' | 'memory'
export type WorkflowState = 'pending' | 'approved' | 'rejected' | 'ignored'
export type LifecycleState = 'active' | 'revoked' | 'deleted' | 'merged'
export type ApprovalKind = 'self-representation' | 'behavioral-authorization' | 'factual-verification'
export type TrustTier = 'identity' | 'stable-preference' | 'contextual'
export type RiskClass = 'action-sensitive' | 'behavioral' | 'informational'
export type Salience = 'pinned' | 'normal'
export type Polarity = 'positive' | 'negative' | 'neutral'
export type Sensitivity = 'normal' | 'private' | 'restricted'
export type ProviderPolicy = 'deny' | 'prompt' | 'allow'

export interface RevisionRef {
  revision_id: string
  payload_sha256: string
}

export interface ActorRef {
  kind: string
  id: string
  basis?: string
  device_id?: string
}

export interface SubjectRef {
  kind: string
  id: string
  relation_to_owner: 'self' | 'direct' | 'shared-context' | 'external'
  label?: string
}

export interface MemoryClaimRevision {
  schema: 'notemd.memory/claim-revision/v2'
  claim_id: string
  revision_id: string
  request_id?: string
  parents: RevisionRef[]
  claim_kind: ClaimKind
  kind_data?: Record<string, unknown>
  subject: SubjectRef
  asserted_by: ActorRef[]
  recorded_by: ActorRef
  recorded_at: string
  text: string
  projection: {
    target: ProjectionTarget
    category: string
    visibility: 'projection' | 'trusted-agent' | 'ui-only'
  }
  workflow: { state: WorkflowState }
  lifecycle: { state: LifecycleState }
  temporal: {
    uttered_at?: string | null
    observed_at?: string | null
    valid_from?: string | null
    valid_until?: string | null
    planned_for?: string | null
    due_at?: string | null
    review_after?: string | null
  }
  epistemic: {
    basis: 'owner-stated' | 'source-supported' | 'inferred' | 'contested' | 'unknown'
    representation_certainty: 'high' | 'medium' | 'low' | 'unknown'
    truth_status: 'verified' | 'not-assessed' | 'contested' | 'unknown'
    truth_confidence: 'high' | 'medium' | 'low' | 'unknown'
  }
  trust_tier: TrustTier
  risk_class: RiskClass
  salience: Salience
  polarity: Polarity
  sensitivity: Sensitivity
  context: {
    spaces: string[]
    applies_when: string[]
    excludes_when: string[]
  }
  consent: {
    scope: string
    allowed_purposes: string[]
    external_provider_policy: ProviderPolicy
  }
  agent_use: { guidance: string; avoid_error: string }
  decision?: {
    verdict: 'approve' | 'reject' | 'ignore'
    approval_kind?: ApprovalKind
    authority_scope?: string
    actor_id: string
    decided_at: string
  }
  evidence?: Array<{
    relation: 'evidence-of-speech' | 'evidence-of-observation' | 'evidence-of-truth' | 'derived-from'
    resource: string
    content_sha256?: string
    title?: string
  }>
  payload_sha256: string
}

export interface EffectiveClaim {
  claim: MemoryClaimRevision
  application_state: 'current' | 'stale' | 'expired' | 'superseded' | 'quarantined'
  do_not_rely?: boolean
  reasons?: string[]
}

export interface PendingClaim {
  revision: MemoryClaimRevision
  expected_sha256: string
  expected_heads: RevisionRef[]
  required_approval_kind?: ApprovalKind
  base_text?: string
  source_summary?: string
}

export interface MemoryConflict {
  conflict_id: string
  claim_id: string
  risk_class: RiskClass
  action_allowed: boolean
  common_ancestor?: MemoryClaimRevision
  heads: MemoryClaimRevision[]
  reasons: string[]
}

export interface HistoryItem {
  id: string
  claim_id: string
  revision_id: string
  operation: string
  workflow_state: WorkflowState
  lifecycle_state: LifecycleState
  actor_id?: string
  approval_kind?: ApprovalKind
  recorded_at: string
  summary: string
}

export interface ProtocolRef {
  revision_id: string
  payload_sha256: string
}

export interface ContextOption {
  id: string
  label: string
  provider_id?: string
}

export interface MemoryHealth {
  status: 'healthy' | 'attention' | 'conflict' | 'damaged' | 'unsupported'
  message: string
  pending_count: number
  conflict_count: number
  integrity_errors: string[]
  projection_edited?: boolean
}

export interface MemorySnapshotV2 {
  mode: 'v2' | 'legacy' | 'recovery' | 'read-only'
  migration_required?: boolean
  read_only_reason?: string
  protocol?: ProtocolRef
  owner?: { actor_id: string; subject: SubjectRef }
  claims: EffectiveClaim[]
  pending: PendingClaim[]
  conflicts: MemoryConflict[]
  history: HistoryItem[]
  health: MemoryHealth
  context_options?: {
    spaces: ContextOption[]
    purposes: ContextOption[]
    providers: ContextOption[]
    models: ContextOption[]
  }
}

export interface MigrationDryRun {
  migration_id: string
  plan_sha256: string
  source_manifest_sha256: string
  counts: {
    claims: number
    pending: number
    approved: number
    rejected: number
    legacy_unclassified: number
  }
  projection_preview?: { user: string; memory: string }
  warnings: string[]
  blockers: string[]
  writes_performed: false
}

export interface MutationBase {
  request_id: string
  expected_protocol: ProtocolRef
}

export interface AddClaimInput extends MutationBase {
  target: ProjectionTarget
  category: string
  text: string
  claim_kind: ClaimKind
  subject: { kind: 'vault-owner'; id: string; relation_to_owner: 'self' }
  approval_kind: ApprovalKind
  trust_tier: TrustTier
  risk_class: RiskClass
  salience: Salience
  polarity: Polarity
  sensitivity: Exclude<Sensitivity, 'restricted'>
  context: MemoryClaimRevision['context']
  consent: MemoryClaimRevision['consent']
  agent_use: MemoryClaimRevision['agent_use']
}

export interface PendingDecisionInput extends MutationBase {
  expected_heads: RevisionRef[]
  revision_id: string
  expected_sha256: string
  gesture_intent: 'approve' | 'reject' | 'ignore' | 'delete'
  salience_override?: Salience
}

export interface ClaimMutationInput extends MutationBase {
  claim_id: string
  expected_heads: RevisionRef[]
}

export interface ResolveConflictInput extends MutationBase {
  conflict_id: string
  claim_id: string
  expected_heads: RevisionRef[]
  strategy: 'keep-head' | 'merge' | 'revoke-all'
  selected_revision_id?: string
  merged_text?: string
}

export interface WriteReceipt {
  claim_id: string
  revision_id: string
  payload_sha256: string
  effective_status: string
  conflict: boolean
  projection_rebuilt: boolean
  transaction_id?: string
  error_code?: string
}

export interface ContextRequest {
  space: string
  purpose: string
  caller: string
  provider: string
  model: string
  tools: string[]
  external_transfer: boolean
  as_of_valid_time: string
}

export interface ContextPreview {
  request: ContextRequest
  manifest_id?: string
  selected: Array<{ claim_id: string; revision_id: string; reasons: string[]; text?: string }>
  excluded_summary: Record<string, number>
  conflicts: Array<{ conflict_id: string; action_allowed: boolean }>
  redactions: number
  policy_result: { external_action_allowed: boolean }
}

export interface ContextManifestReceipt {
  manifest_id: string
  payload_sha256: string
  selected_count: number
}
