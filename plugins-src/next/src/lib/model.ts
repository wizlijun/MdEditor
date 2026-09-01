export const NEXT_ACTIONS = ['commit', 'wait', 'park', 'settle', 'reopen', 'relink'] as const
export type NextAction = (typeof NEXT_ACTIONS)[number]

export const ITEM_KINDS = ['idea', 'task'] as const
export type ItemKind = (typeof ITEM_KINDS)[number]

export function itemKey(kind: ItemKind, id: string): string {
  return JSON.stringify([kind, id])
}

export const IDEA_STATES = ['capture', 'wip', 'waiting', 'dormant', 'closed', 'unsupported'] as const
export type IdeaState = (typeof IDEA_STATES)[number]

export const DEFAULT_WIP_LIMIT = 5
export const DEFAULT_WAITING_WARNING = 5

/** A source locator. Legacy/agent-authored ideas may not have a creation marker. */
export interface SourceRef {
  path: string
  created?: string
  [key: string]: unknown
}

export interface EventBase {
  at: string
  event_id: string
  /** v2 identity. New events in a v2 ledger use this pair exclusively. */
  item_id?: string
  item_kind?: ItemKind
  /** v1 persisted identity. Readers normalize it to item_kind=idea in memory. */
  idea_id?: string
  action: NextAction
  source?: SourceRef
  /** Canonical tag set. Omitted inherits, an array replaces, and null clears. */
  projects?: string[] | null
  /** Compatibility shadow for readers before 1.4; mirrors the first project tag. */
  project?: string | null
  [key: string]: unknown
}

export interface CommitEvent extends EventBase {
  action: 'commit'
  commitment: string
  next_action: string
  close_condition: string
}

export interface WaitEvent extends EventBase {
  action: 'wait'
  waiting_for: string
  review_at: string
}

export interface ParkEvent extends EventBase {
  action: 'park'
  wake_trigger: string
  next_action?: string
}

export type DoneExit = {
  kind: 'done'
  via?: 'delegate'
  /** Compatibility-safe delivery subtype; older readers preserve this unknown field. */
  delivery?: 'article'
  [key: string]: unknown
}
export type StoppedExit = {
  kind: 'stopped'
  via: 'drop' | 'disproved' | 'ignore'
  [key: string]: unknown
}
export type TransferredExit = {
  kind: 'transferred'
  via: 'merge' | 'project' | 'delegate' | 'buy' | 'publish'
  [key: string]: unknown
}
export type CompressedExit = {
  kind: 'compressed'
  via: 'principle' | 'automate'
  [key: string]: unknown
}
export type SettlementExit = DoneExit | StoppedExit | TransferredExit | CompressedExit

export interface SettleEvent extends EventBase {
  action: 'settle'
  exit: SettlementExit
  reason?: string
  target?: string
  result?: string
}

export interface ReopenEvent extends EventBase {
  action: 'reopen'
}

export interface RelinkEvent extends EventBase {
  action: 'relink'
  source: SourceRef
}

export type NextEvent =
  | CommitEvent
  | WaitEvent
  | ParkEvent
  | SettleEvent
  | ReopenEvent
  | RelinkEvent

/**
 * A structurally valid v1 envelope whose action this version does not know.
 * The full record remains available so a newer writer can recover it losslessly.
 */
export interface UnsupportedEvent {
  at: string
  event_id: string
  item_id: string
  item_kind: ItemKind
  idea_id?: string
  action: string
  source?: SourceRef
  [key: string]: unknown
}

export interface IdeaProjectionBase {
  item_id?: string
  item_kind?: ItemKind
  /** Deprecated compatibility alias. Generic consumers should use item_id. */
  idea_id: string
  source?: SourceRef
  last_event_id: string
  last_at: string
  projects?: readonly string[]
  /** Derived first tag retained for compatibility with pre-1.4 views. */
  project?: string
}

export interface CaptureIdea extends IdeaProjectionBase {
  state: 'capture'
}

export interface WipIdea extends IdeaProjectionBase {
  state: 'wip'
  commitment: string
  next_action: string
  close_condition: string
}

export interface WaitingIdea extends IdeaProjectionBase {
  state: 'waiting'
  waiting_for: string
  review_at: string
}

export interface DormantIdea extends IdeaProjectionBase {
  state: 'dormant'
  wake_trigger: string
  next_action?: string
}

export interface ClosedIdea extends IdeaProjectionBase {
  state: 'closed'
  exit: SettlementExit
  reason?: string
  target?: string
  result?: string
}

export interface UnsupportedIdea extends IdeaProjectionBase {
  state: 'unsupported'
  /** Last state understood before an unknown or contradictory event, if any. */
  last_known_state?: Exclude<IdeaState, 'unsupported'>
  unsupported_actions: readonly string[]
}

export type IdeaProjection =
  | CaptureIdea
  | WipIdea
  | WaitingIdea
  | DormantIdea
  | ClosedIdea
  | UnsupportedIdea

/** Canonical in-memory projection after v1 identities have been normalized. */
export type ItemProjection = IdeaProjection & {
  item_id: string
  item_kind: ItemKind
}

export type NormalizedNextEvent = NextEvent & {
  item_id: string
  item_kind: ItemKind
}

export function projectTagsOf(projection: IdeaProjection | undefined): readonly string[] {
  if (projection?.projects?.length) return projection.projects
  return projection?.project ? [projection.project] : []
}

export function normalizeProjectTag(value: string): string {
  return value.normalize('NFKC').trim()
}

export function projectTagKey(value: string): string {
  return normalizeProjectTag(value).toLocaleLowerCase()
}

export function uniqueProjectTags(values: readonly string[]): string[] {
  const projects: string[] = []
  const keys = new Set<string>()
  for (const value of values) {
    const project = normalizeProjectTag(value)
    const key = projectTagKey(project)
    if (!project || keys.has(key)) continue
    projects.push(project)
    keys.add(key)
  }
  return projects
}

export type DomainIssueCode =
  | 'invalid_event'
  | 'unsupported_action'
  | 'event_id_conflict'
  | 'source_identity_conflict'
  | 'source_mismatch'
  | 'missing_initial_source'
  | 'invalid_transition'
  | 'idea_unsupported'
  | 'wip_limit_exceeded'
  | 'wip_limit_uncertain'

export interface DomainIssue {
  code: DomainIssueCode
  severity: 'warning' | 'blocking'
  message: string
  event_id?: string
  item_id?: string
  item_kind?: ItemKind
  /** Deprecated v1 compatibility field for Idea issues. */
  idea_id?: string
  event_index?: number
  fields?: readonly string[]
  related_idea_ids?: readonly string[]
  related_item_ids?: readonly string[]
}

export interface LedgerProjection {
  /** Canonical map keyed by itemKey(item_kind, item_id). */
  items: ReadonlyMap<string, ItemProjection>
  /** Deprecated v1 compatibility view, containing Idea items keyed by idea_id. */
  ideas: ReadonlyMap<string, IdeaProjection>
  /** Unique event payloads, including unsupported events, keyed by event_id. */
  eventById: ReadonlyMap<string, unknown>
  /** Historical source claims. A relink adds a claim; it does not erase the old one. */
  sourceToIdeaId: ReadonlyMap<string, string>
  /** Canonical source claims whose values are itemKey(item_kind, item_id). */
  sourceToItemKey: ReadonlyMap<string, string>
  issues: readonly DomainIssue[]
  idempotentEventIds: readonly string[]
  wipCount: number
  waitingCount: number
  wipLimit: number
  waitingWarning: number
  wipAtLimit: boolean
  wipExceeded: boolean
  waitingExceeded: boolean
  hasUnsupported: boolean
  hasBlockingIssues: boolean
}

export interface FieldValidationError {
  field: string
  message: string
}

export type EventValidation =
  | { ok: true; known: true; event: NormalizedNextEvent }
  | { ok: true; known: false; event: UnsupportedEvent }
  | { ok: false; errors: readonly FieldValidationError[] }

export interface AppendOptions {
  /** Optional hard guard used by selected atomic write paths. */
  hardLimit?: boolean
  wipLimit?: number
  /** When supplied, require the persisted identity envelope for that ledger. */
  ledgerVersion?: 1 | 2
}

export type AppendValidation =
  | { ok: true; idempotent: boolean; event: NormalizedNextEvent }
  | { ok: false; issues: readonly DomainIssue[] }
