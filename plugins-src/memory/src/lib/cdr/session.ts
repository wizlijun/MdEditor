/**
 * Domain-neutral collaborative-document state machine.
 *
 * Persistence is deliberately outside this module. The session owns validation
 * and deterministic state transitions; repository.ts owns durable CAS.
 * Content hashes are asynchronous because production uses Web Crypto.
 */

import { sha256Hex } from '../../../../../src/lib/hash'
import {
  applyDocumentChange,
  validateDocumentChange,
  type CoreConflict,
  type DocumentBlock,
  type DocumentRevision,
} from '../../../../../src/lib/cdr/core'
import {
  canonicalOperationBatch,
  parseOperation,
  parseOperationBatch,
  type AppliedChange,
  type OperationBatch,
  type ReplaceBlockOperation,
} from '../../../../../src/lib/cdr/operation'
import { GovernedRevisionChangedError } from '../../../../../src/lib/cdr/governance'

export { sha256Hex } from '../../../../../src/lib/hash'
export type { DocumentBlock, DocumentRevision } from '../../../../../src/lib/cdr/core'
export type { AppliedChange, OperationBatch, ReplaceBlockOperation } from '../../../../../src/lib/cdr/operation'

export type Conflict = CoreConflict

export type SubmitResult =
  | { kind: 'applied'; change: AppliedChange; snapshot: DocumentRevision; duplicate: boolean }
  | { kind: 'conflicted'; conflict: Conflict; snapshot: DocumentRevision }

export interface Proposal {
  changeSetId: string
  actorId: string
  status: 'pending' | 'applied' | 'conflicted' | 'rejected'
  batch: OperationBatch
  conflict?: Conflict
}

export interface Assessment {
  assessmentId: string
  actorId: string
  blockId: string
  blockRevision: string
  conclusion: 'verified' | 'needs-review'
}

export interface AuditEvent {
  eventId: string
  actorId: string
  action: 'applied' | 'proposed' | 'proposal-applied' | 'proposal-conflicted' | 'proposal-rejected' | 'assessed'
  targetId: string
}

export interface IdProvider {
  revisionId(): string
  blockRevision(markdown: string): Promise<string>
  changeId(): string
  changeSetId(): string
  assessmentId(): string
  eventId(): string
}

const LEGACY_DOCUMENT_SESSION_STATE_SCHEMA = 'notemd.cdr/document-session/v2' as const
export const DOCUMENT_SESSION_STATE_SCHEMA = 'notemd.cdr/document-session/v3' as const

export type StoredSubmitOutcome =
  | { kind: 'applied'; change: AppliedChange }
  | { kind: 'conflicted'; conflict: Conflict }

export interface StoredSubmitReceipt {
  requestId: string
  actorId: string
  submittedBaseRevisionId: string
  batchSignature: string
  outcome: StoredSubmitOutcome
}

/** The complete durable aggregate. A receipt never repeats the current head. */
export interface DocumentSessionState {
  schema: typeof DOCUMENT_SESSION_STATE_SCHEMA
  head: DocumentRevision
  revisionHistory: readonly DocumentRevision[]
  receipts: readonly StoredSubmitReceipt[]
  proposals: readonly Proposal[]
  assessments: readonly Assessment[]
  audit: readonly AuditEvent[]
}

export class InvalidSessionStateError extends Error {
  readonly code = 'CDR_STATE_INVALID'

  constructor(message: string) {
    super(`CDR_STATE_INVALID: ${message}`)
    this.name = 'InvalidSessionStateError'
  }
}

type UnknownRecord = Record<string, unknown>

function invalid(path: string, message: string): never {
  throw new InvalidSessionStateError(`${path} ${message}`)
}

function record(value: unknown, path: string): UnknownRecord {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) invalid(path, 'must be an object')
  return value as UnknownRecord
}

function exactKeys(value: UnknownRecord, path: string, required: readonly string[], optional: readonly string[] = []): void {
  const allowed = new Set([...required, ...optional])
  for (const key of required) {
    if (!Object.prototype.hasOwnProperty.call(value, key)) invalid(`${path}.${key}`, 'is required')
  }
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) invalid(`${path}.${key}`, 'is not allowed by this schema')
  }
}

function stringValue(value: unknown, path: string, allowEmpty = false): string {
  if (typeof value !== 'string' || (!allowEmpty && value.length === 0)) invalid(path, 'must be a non-empty string')
  return value
}

function arrayValue(value: unknown, path: string): unknown[] {
  if (!Array.isArray(value)) invalid(path, 'must be an array')
  return value
}

function enumValue<T extends string>(value: unknown, path: string, allowed: readonly T[]): T {
  if (typeof value !== 'string' || !allowed.includes(value as T)) invalid(path, `must be one of ${allowed.join(', ')}`)
  return value as T
}

function unique(values: readonly string[], path: string): void {
  const seen = new Set<string>()
  for (const value of values) {
    if (seen.has(value)) invalid(path, `contains duplicate ${value}`)
    seen.add(value)
  }
}

function parseBlock(value: unknown, path: string): DocumentBlock {
  const item = record(value, path)
  exactKeys(item, path, ['blockId', 'blockRevision', 'markdown'])
  return {
    blockId: stringValue(item.blockId, `${path}.blockId`),
    blockRevision: stringValue(item.blockRevision, `${path}.blockRevision`),
    markdown: stringValue(item.markdown, `${path}.markdown`, true),
  }
}

function parseRevision(value: unknown, path: string): DocumentRevision {
  const item = record(value, path)
  exactKeys(item, path, ['documentId', 'revisionId', 'blocks'])
  const blocks = arrayValue(item.blocks, `${path}.blocks`).map((block, index) => parseBlock(block, `${path}.blocks[${index}]`))
  if (blocks.length === 0) invalid(`${path}.blocks`, 'must contain at least one block')
  for (const [index, block] of blocks.entries()) {
    if (!block.markdown.trim()) invalid(`${path}.blocks[${index}].markdown`, 'must contain visible content')
  }
  unique(blocks.map((block) => block.blockId), `${path}.blocks`)
  return {
    documentId: stringValue(item.documentId, `${path}.documentId`),
    revisionId: stringValue(item.revisionId, `${path}.revisionId`),
    blocks,
  }
}

function parseBatch(value: unknown, path: string): OperationBatch {
  return parseOperationBatch(value, path, invalid)
}

function parseConflict(value: unknown, path: string): Conflict {
  const item = record(value, path)
  exactKeys(item, path, ['code', 'message'], ['blockId'])
  return {
    code: enumValue(item.code, `${path}.code`, ['stale-base', 'invalid-operation']),
    message: stringValue(item.message, `${path}.message`),
    ...(item.blockId === undefined ? {} : { blockId: stringValue(item.blockId, `${path}.blockId`) }),
  }
}

function parseChange(value: unknown, path: string): AppliedChange {
  const item = record(value, path)
  exactKeys(item, path, ['changeId', 'baseRevisionId', 'revisionId', 'blockRevisions', 'operations'], ['originRequestId'])
  const blockRevisions = record(item.blockRevisions, `${path}.blockRevisions`)
  const parsedBlockRevisions: Record<string, string> = {}
  for (const [blockId, revision] of Object.entries(blockRevisions)) {
    if (!blockId) invalid(`${path}.blockRevisions`, 'contains an empty block id')
    parsedBlockRevisions[blockId] = stringValue(revision, `${path}.blockRevisions.${blockId}`)
  }
  const operations = arrayValue(item.operations, `${path}.operations`)
    .map((operation, index) => parseOperation(operation, `${path}.operations[${index}]`))
  unique(operations.map((operation) => operation.operationId), `${path}.operations operationIds`)
  unique(operations.map((operation) => operation.blockId), `${path}.operations blockIds`)
  const operationBlocks = operations.map((operation) => operation.blockId).sort()
  if (JSON.stringify(operationBlocks) !== JSON.stringify(Object.keys(parsedBlockRevisions).sort())) {
    invalid(`${path}.blockRevisions`, 'must contain exactly the changed operation blocks')
  }
  return {
    changeId: stringValue(item.changeId, `${path}.changeId`),
    ...(item.originRequestId === undefined ? {} : { originRequestId: stringValue(item.originRequestId, `${path}.originRequestId`) }),
    baseRevisionId: stringValue(item.baseRevisionId, `${path}.baseRevisionId`),
    revisionId: stringValue(item.revisionId, `${path}.revisionId`),
    blockRevisions: parsedBlockRevisions,
    operations,
  }
}

function parseStoredOutcome(value: unknown, path: string): StoredSubmitOutcome {
  const item = record(value, path)
  const kind = enumValue(item.kind, `${path}.kind`, ['applied', 'conflicted'])
  if (kind === 'applied') {
    exactKeys(item, path, ['kind', 'change'])
    return { kind, change: parseChange(item.change, `${path}.change`) }
  }
  exactKeys(item, path, ['kind', 'conflict'])
  return { kind, conflict: parseConflict(item.conflict, `${path}.conflict`) }
}

function signature(batch: OperationBatch): string {
  return canonicalOperationBatch(batch)
}

type ParsedReceipt = Omit<StoredSubmitReceipt, 'actorId'> & { actorId?: string }

function parseReceipt(value: unknown, path: string, legacy: boolean): ParsedReceipt {
  const item = record(value, path)
  exactKeys(
    item,
    path,
    legacy
      ? ['requestId', 'submittedBaseRevisionId', 'batchSignature', 'outcome']
      : ['requestId', 'actorId', 'submittedBaseRevisionId', 'batchSignature', 'outcome'],
  )
  const requestId = stringValue(item.requestId, `${path}.requestId`)
  const actorId = legacy ? undefined : stringValue(item.actorId, `${path}.actorId`)
  const submittedBaseRevisionId = stringValue(item.submittedBaseRevisionId, `${path}.submittedBaseRevisionId`)
  const batchSignature = stringValue(item.batchSignature, `${path}.batchSignature`)
  const outcome = parseStoredOutcome(item.outcome, `${path}.outcome`)
  if (outcome.kind === 'applied') {
    if (outcome.change.originRequestId !== requestId) invalid(`${path}.outcome.change.originRequestId`, 'must match receipt requestId')
    const expected = signature({ requestId, baseRevisionId: submittedBaseRevisionId, operations: outcome.change.operations })
    if (batchSignature !== expected) invalid(`${path}.batchSignature`, 'does not match the applied change')
  }
  return { requestId, ...(actorId ? { actorId } : {}), submittedBaseRevisionId, batchSignature, outcome }
}

function parseProposal(value: unknown, path: string): Proposal {
  const item = record(value, path)
  exactKeys(item, path, ['changeSetId', 'actorId', 'status', 'batch'], ['conflict'])
  const status = enumValue(item.status, `${path}.status`, ['pending', 'applied', 'conflicted', 'rejected'])
  const conflict = item.conflict === undefined ? undefined : parseConflict(item.conflict, `${path}.conflict`)
  if ((status === 'conflicted') !== (conflict !== undefined)) invalid(`${path}.conflict`, 'must exist only for a conflicted proposal')
  const batch = parseBatch(item.batch, `${path}.batch`)
  if (batch.operations.length !== 1) invalid(`${path}.batch.operations`, 'must contain exactly one operation in Stage 1A')
  return {
    changeSetId: stringValue(item.changeSetId, `${path}.changeSetId`),
    actorId: stringValue(item.actorId, `${path}.actorId`),
    status,
    batch,
    ...(conflict ? { conflict } : {}),
  }
}

function parseAssessment(value: unknown, path: string): Assessment {
  const item = record(value, path)
  exactKeys(item, path, ['assessmentId', 'actorId', 'blockId', 'blockRevision', 'conclusion'])
  return {
    assessmentId: stringValue(item.assessmentId, `${path}.assessmentId`),
    actorId: stringValue(item.actorId, `${path}.actorId`),
    blockId: stringValue(item.blockId, `${path}.blockId`),
    blockRevision: stringValue(item.blockRevision, `${path}.blockRevision`),
    conclusion: enumValue(item.conclusion, `${path}.conclusion`, ['verified', 'needs-review']),
  }
}

function parseAudit(value: unknown, path: string): AuditEvent {
  const item = record(value, path)
  exactKeys(item, path, ['eventId', 'actorId', 'action', 'targetId'])
  return {
    eventId: stringValue(item.eventId, `${path}.eventId`),
    actorId: stringValue(item.actorId, `${path}.actorId`),
    action: enumValue(item.action, `${path}.action`, [
      'applied', 'proposed', 'proposal-applied', 'proposal-conflicted', 'proposal-rejected', 'assessed',
    ]),
    targetId: stringValue(item.targetId, `${path}.targetId`),
  }
}

/** Validate untrusted repository data and return a detached canonical value. */
export function parseDocumentSessionState(value: unknown): DocumentSessionState {
  const item = record(value, 'state')
  exactKeys(item, 'state', ['schema', 'head', 'revisionHistory', 'receipts', 'proposals', 'assessments', 'audit'])
  const legacy = item.schema === LEGACY_DOCUMENT_SESSION_STATE_SCHEMA
  if (!legacy && item.schema !== DOCUMENT_SESSION_STATE_SCHEMA) {
    invalid('state.schema', `must be ${DOCUMENT_SESSION_STATE_SCHEMA}`)
  }
  const head = parseRevision(item.head, 'state.head')
  const revisionHistory = arrayValue(item.revisionHistory, 'state.revisionHistory')
    .map((revision, index) => parseRevision(revision, `state.revisionHistory[${index}]`))
  const proposals = arrayValue(item.proposals, 'state.proposals').map((proposal, index) => parseProposal(proposal, `state.proposals[${index}]`))
  const assessments = arrayValue(item.assessments, 'state.assessments').map((assessment, index) => parseAssessment(assessment, `state.assessments[${index}]`))
  const audit = arrayValue(item.audit, 'state.audit').map((event, index) => parseAudit(event, `state.audit[${index}]`))
  const receipts = arrayValue(item.receipts, 'state.receipts').map((receipt, index): StoredSubmitReceipt => {
    const parsed = parseReceipt(receipt, `state.receipts[${index}]`, legacy)
    if (parsed.actorId) return parsed as StoredSubmitReceipt
    if (parsed.outcome.kind === 'applied') {
      const changeId = parsed.outcome.change.changeId
      const candidates = audit.filter((event) => (
        event.action === 'applied' && event.targetId === changeId
      ))
      return { ...parsed, actorId: candidates.length === 1 ? candidates[0].actorId : 'legacy:unknown' }
    }
    const candidates = proposals.filter((proposal) => proposal.batch.requestId === parsed.requestId)
    return { ...parsed, actorId: candidates.length === 1 ? candidates[0].actorId : 'legacy:unknown' }
  })
  unique(receipts.map((receipt) => receipt.requestId), 'state.receipts requestIds')
  unique(proposals.map((proposal) => proposal.changeSetId), 'state.proposals changeSetIds')
  unique(proposals.map((proposal) => proposal.batch.requestId), 'state.proposals requestIds')
  unique(assessments.map((assessment) => assessment.assessmentId), 'state.assessments assessmentIds')
  unique(audit.map((event) => event.eventId), 'state.audit eventIds')
  unique(revisionHistory.map((revision) => revision.revisionId), 'state.revisionHistory revisionIds')
  for (const revision of revisionHistory) {
    if (revision.documentId !== head.documentId) invalid('state.revisionHistory', 'must belong to the head document')
    if (revision.revisionId === head.revisionId) invalid('state.revisionHistory', 'must not repeat the head revision')
  }
  for (const [index, receipt] of receipts.entries()) {
    if (receipt.outcome.kind !== 'applied') continue
    const changeId = receipt.outcome.change.changeId
    const candidates = audit.filter((event) => (
      event.action === 'applied' && event.targetId === changeId
    ))
    if (receipt.actorId === 'legacy:unknown') {
      if (candidates.length === 1) {
        invalid(`state.receipts[${index}].actorId`, 'must match the unique applied audit actor')
      }
    } else if (candidates.length !== 1 || candidates[0].actorId !== receipt.actorId) {
      invalid(`state.receipts[${index}].actorId`, 'must match exactly one applied audit actor')
    }
  }
  const receiptRequests = new Set(receipts.map((receipt) => receipt.requestId))
  for (const proposal of proposals) {
    if (!receiptRequests.has(proposal.batch.requestId)) continue
    const receipt = receipts.find((item) => item.requestId === proposal.batch.requestId)
    if (receipt?.outcome.kind !== 'conflicted'
      || receipt.batchSignature !== signature(proposal.batch)
      || receipt.actorId !== proposal.actorId) {
      invalid('state', `requestId ${proposal.batch.requestId} is reused inconsistently by a proposal and a receipt`)
    }
  }
  return { schema: DOCUMENT_SESSION_STATE_SCHEMA, head, revisionHistory, receipts, proposals, assessments, audit }
}

function cloneRevision(revision: DocumentRevision): DocumentRevision {
  return {
    documentId: revision.documentId,
    revisionId: revision.revisionId,
    blocks: revision.blocks.map((block) => ({ ...block })),
  }
}

function cloneBatch(batch: OperationBatch): OperationBatch {
  return { ...batch, operations: batch.operations.map((operation) => ({ ...operation })) }
}

function cloneChange(change: AppliedChange): AppliedChange {
  return {
    ...change,
    blockRevisions: { ...change.blockRevisions },
    operations: change.operations.map((operation) => ({ ...operation })),
  }
}

function cloneConflict(conflict: Conflict): Conflict {
  return { ...conflict }
}

function cloneOutcome(outcome: StoredSubmitOutcome): StoredSubmitOutcome {
  return outcome.kind === 'applied'
    ? { kind: 'applied', change: cloneChange(outcome.change) }
    : { kind: 'conflicted', conflict: cloneConflict(outcome.conflict) }
}

export class InMemoryDocumentSession {
  #head: DocumentRevision
  #revisionHistory: DocumentRevision[] = []
  #receipts = new Map<string, { actorId: string; submittedBaseRevisionId: string; batchSignature: string; outcome: StoredSubmitOutcome }>()
  #mutationTail: Promise<void> = Promise.resolve()
  #proposals: Proposal[] = []
  #assessments: Assessment[] = []
  #audit: AuditEvent[] = []

  constructor(initial: DocumentRevision, private readonly ids: IdProvider) {
    this.#head = parseRevision(initial, 'initial')
  }

  static fromState(state: unknown, ids: IdProvider): InMemoryDocumentSession {
    const parsed = parseDocumentSessionState(state)
    const session = new InMemoryDocumentSession(parsed.head, ids)
    session.#revisionHistory = parsed.revisionHistory.map(cloneRevision)
    session.#receipts = new Map(parsed.receipts.map((receipt) => [receipt.requestId, {
      actorId: receipt.actorId,
      submittedBaseRevisionId: receipt.submittedBaseRevisionId,
      batchSignature: receipt.batchSignature,
      outcome: cloneOutcome(receipt.outcome),
    }]))
    session.#proposals = parsed.proposals.map((proposal) => ({
      ...proposal,
      batch: cloneBatch(proposal.batch),
      ...(proposal.conflict ? { conflict: cloneConflict(proposal.conflict) } : {}),
    }))
    session.#assessments = parsed.assessments.map((assessment) => ({ ...assessment }))
    session.#audit = parsed.audit.map((event) => ({ ...event }))
    return session
  }

  exportState(): DocumentSessionState {
    return {
      schema: DOCUMENT_SESSION_STATE_SCHEMA,
      head: this.snapshot(),
      revisionHistory: this.revisionHistory(),
      receipts: [...this.#receipts.entries()].map(([requestId, receipt]) => ({
        requestId,
        actorId: receipt.actorId,
        submittedBaseRevisionId: receipt.submittedBaseRevisionId,
        batchSignature: receipt.batchSignature,
        outcome: cloneOutcome(receipt.outcome),
      })),
      proposals: this.proposals(),
      assessments: this.assessments(),
      audit: this.audit(),
    }
  }

  snapshot(): DocumentRevision {
    return cloneRevision(this.#head)
  }

  revisionHistory(): readonly DocumentRevision[] {
    return this.#revisionHistory.map(cloneRevision)
  }

  proposals(): readonly Proposal[] {
    return this.#proposals.map((proposal) => ({
      ...proposal,
      batch: cloneBatch(proposal.batch),
      ...(proposal.conflict ? { conflict: cloneConflict(proposal.conflict) } : {}),
    }))
  }

  proposal(changeSetId: string): Proposal | undefined {
    const proposal = this.#proposals.find((item) => item.changeSetId === changeSetId)
    return proposal
      ? {
          ...proposal,
          batch: cloneBatch(proposal.batch),
          ...(proposal.conflict ? { conflict: cloneConflict(proposal.conflict) } : {}),
        }
      : undefined
  }

  assessments(): readonly Assessment[] {
    return this.#assessments.map((assessment) => ({ ...assessment }))
  }

  audit(): readonly AuditEvent[] {
    return this.#audit.map((event) => ({ ...event }))
  }

  async submit(batch: OperationBatch, actorId: string, governedRevisionId?: string): Promise<SubmitResult> {
    return this.#enqueue(() => this.#submit(batch, actorId, governedRevisionId))
  }

  async #submit(batch: OperationBatch, actorId: string, governedRevisionId?: string): Promise<SubmitResult> {
    const normalizedBatch = parseBatch(batch, 'batch')
    stringValue(actorId, 'actorId')
    const payload = signature(normalizedBatch)
    const existing = this.#receipts.get(normalizedBatch.requestId)
    if (existing) {
      if (existing.actorId !== actorId || existing.batchSignature !== payload) throw new Error('CDR_IDEMPOTENCY_KEY_REUSED')
      return existing.outcome.kind === 'applied'
        ? { kind: 'applied', change: cloneChange(existing.outcome.change), snapshot: this.snapshot(), duplicate: true }
        : { kind: 'conflicted', conflict: cloneConflict(existing.outcome.conflict), snapshot: this.snapshot() }
    }
    const existingProposal = this.#proposals.find((proposal) => proposal.batch.requestId === normalizedBatch.requestId)
    if (existingProposal) {
      if (existingProposal.actorId !== actorId || signature(existingProposal.batch) !== payload) {
        throw new Error('CDR_IDEMPOTENCY_KEY_REUSED')
      }
      throw new Error('CDR_IDEMPOTENCY_KEY_REUSED')
    }
    this.#assertGovernedRevision(governedRevisionId)

    const conflict = this.#validate(normalizedBatch)
    if (conflict) {
      this.#receipts.set(normalizedBatch.requestId, {
        actorId,
        submittedBaseRevisionId: normalizedBatch.baseRevisionId,
        batchSignature: payload,
        outcome: { kind: 'conflicted', conflict: cloneConflict(conflict) },
      })
      return { kind: 'conflicted', conflict, snapshot: this.snapshot() }
    }

    const previousHead = this.snapshot()
    const baseRevisionId = previousHead.revisionId
    const changedRevisions = new Map(await Promise.all(normalizedBatch.operations.map(async (operation) => [
      operation.blockId,
      await this.ids.blockRevision(operation.markdown),
    ] as const)))
    const revisionId = this.ids.revisionId()
    const blockRevisions = Object.fromEntries(changedRevisions)
    this.#head = applyDocumentChange(previousHead, normalizedBatch, { revisionId, blockRevisions })
    this.#revisionHistory = [...this.#revisionHistory, previousHead]
    const change: AppliedChange = {
      changeId: this.ids.changeId(),
      originRequestId: normalizedBatch.requestId,
      baseRevisionId,
      revisionId,
      blockRevisions,
      operations: normalizedBatch.operations.map((operation) => ({ ...operation })),
    }
    const outcome: StoredSubmitOutcome = { kind: 'applied', change: cloneChange(change) }
    this.#receipts.set(normalizedBatch.requestId, {
      actorId,
      submittedBaseRevisionId: normalizedBatch.baseRevisionId,
      batchSignature: payload,
      outcome,
    })
    this.#audit.push({ eventId: this.ids.eventId(), actorId, action: 'applied', targetId: change.changeId })
    return { kind: 'applied', change: cloneChange(change), snapshot: this.snapshot(), duplicate: false }
  }

  async propose(batch: OperationBatch, actorId: string, governedRevisionId?: string): Promise<Proposal> {
    return this.#enqueue(() => this.#propose(batch, actorId, governedRevisionId))
  }

  #propose(batch: OperationBatch, actorId: string, governedRevisionId?: string): Proposal {
    const normalizedBatch = parseBatch(batch, 'batch')
    stringValue(actorId, 'actorId')
    if (normalizedBatch.operations.length !== 1) throw new Error('CDR_PROPOSAL_OPERATION_COUNT')
    const existing = this.#proposals.find((proposal) => proposal.batch.requestId === normalizedBatch.requestId)
    if (existing) {
      if (existing.actorId !== actorId || signature(existing.batch) !== signature(normalizedBatch)) {
        throw new Error('CDR_IDEMPOTENCY_KEY_REUSED')
      }
      return {
        ...existing,
        batch: cloneBatch(existing.batch),
        ...(existing.conflict ? { conflict: cloneConflict(existing.conflict) } : {}),
      }
    }
    const receipt = this.#receipts.get(normalizedBatch.requestId)
    if (receipt && (receipt.actorId !== actorId
      || receipt.outcome.kind !== 'conflicted'
      || receipt.batchSignature !== signature(normalizedBatch))) {
      throw new Error('CDR_IDEMPOTENCY_KEY_REUSED')
    }
    this.#assertGovernedRevision(governedRevisionId)
    const proposal: Proposal = {
      changeSetId: this.ids.changeSetId(),
      actorId,
      status: 'pending',
      batch: cloneBatch(normalizedBatch),
    }
    this.#proposals = [...this.#proposals, proposal]
    this.#audit.push({ eventId: this.ids.eventId(), actorId, action: 'proposed', targetId: proposal.changeSetId })
    return { ...proposal, batch: cloneBatch(proposal.batch) }
  }

  async decideProposal(
    changeSetId: string,
    decision: 'accept' | 'reject',
    actorId: string,
    governedRevisionId?: string,
  ): Promise<SubmitResult | null> {
    return this.#enqueue(() => this.#decideProposal(changeSetId, decision, actorId, governedRevisionId))
  }

  async #decideProposal(
    changeSetId: string,
    decision: 'accept' | 'reject',
    actorId: string,
    governedRevisionId?: string,
  ): Promise<SubmitResult | null> {
    stringValue(changeSetId, 'changeSetId')
    enumValue(decision, 'decision', ['accept', 'reject'])
    stringValue(actorId, 'actorId')
    this.#assertGovernedRevision(governedRevisionId)
    const proposal = this.#proposals.find((item) => item.changeSetId === changeSetId)
    if (!proposal) throw new Error('CDR_PROPOSAL_NOT_FOUND')
    if (proposal.status !== 'pending') return null
    if (decision === 'reject') {
      proposal.status = 'rejected'
      this.#audit.push({ eventId: this.ids.eventId(), actorId, action: 'proposal-rejected', targetId: changeSetId })
      return null
    }

    const result = await this.#submit({ ...proposal.batch, requestId: `decision/${changeSetId}` }, actorId, governedRevisionId)
    if (result.kind === 'conflicted') {
      proposal.status = 'conflicted'
      proposal.conflict = result.conflict
      this.#audit.push({ eventId: this.ids.eventId(), actorId, action: 'proposal-conflicted', targetId: changeSetId })
      return result
    }
    proposal.status = 'applied'
    this.#audit.push({ eventId: this.ids.eventId(), actorId, action: 'proposal-applied', targetId: changeSetId })
    return result
  }

  async assess(
    blockId: string,
    actorId: string,
    conclusion: Assessment['conclusion'],
    governedRevisionId?: string,
  ): Promise<Assessment> {
    return this.#enqueue(() => this.#assess(blockId, actorId, conclusion, governedRevisionId))
  }

  #assess(
    blockId: string,
    actorId: string,
    conclusion: Assessment['conclusion'],
    governedRevisionId?: string,
  ): Assessment {
    stringValue(blockId, 'blockId')
    stringValue(actorId, 'actorId')
    enumValue(conclusion, 'conclusion', ['verified', 'needs-review'])
    this.#assertGovernedRevision(governedRevisionId)
    const block = this.#head.blocks.find((item) => item.blockId === blockId)
    if (!block) throw new Error('CDR_BLOCK_NOT_FOUND')
    const assessment: Assessment = {
      assessmentId: this.ids.assessmentId(),
      actorId,
      blockId,
      blockRevision: block.blockRevision,
      conclusion,
    }
    this.#assessments = [...this.#assessments, assessment]
    this.#audit.push({ eventId: this.ids.eventId(), actorId, action: 'assessed', targetId: assessment.assessmentId })
    return { ...assessment }
  }

  assessmentIsOutdated(assessment: Assessment): boolean {
    return this.#head.blocks.find((block) => block.blockId === assessment.blockId)?.blockRevision !== assessment.blockRevision
  }

  #enqueue<T>(transition: () => T | Promise<T>): Promise<T> {
    const run = this.#mutationTail.then(transition)
    this.#mutationTail = run.then(() => undefined, () => undefined)
    return run
  }

  #assertGovernedRevision(expected: string | undefined): void {
    if (expected !== undefined && expected !== this.#head.revisionId) throw new GovernedRevisionChangedError()
  }

  #validate(batch: OperationBatch): Conflict | null {
    return validateDocumentChange(this.#head, batch)
  }
}

function cryptoUuid(): string {
  if (typeof globalThis.crypto?.randomUUID === 'function') return globalThis.crypto.randomUUID()
  if (typeof globalThis.crypto?.getRandomValues !== 'function') throw new Error('CDR_UUID_UNAVAILABLE')
  const bytes = globalThis.crypto.getRandomValues(new Uint8Array(16))
  bytes[6] = (bytes[6] & 0x0f) | 0x40
  bytes[8] = (bytes[8] & 0x3f) | 0x80
  const hex = [...bytes].map((byte) => byte.toString(16).padStart(2, '0'))
  return `${hex.slice(0, 4).join('')}-${hex.slice(4, 6).join('')}-${hex.slice(6, 8).join('')}-${hex.slice(8, 10).join('')}-${hex.slice(10).join('')}`
}

export function documentId(): string {
  return cryptoUuid()
}

export function uuidIds(): IdProvider & { requestId(): string; operationId(): string } {
  const next = (kind: string) => `${kind}/${cryptoUuid()}`
  return {
    revisionId: () => next('revision'),
    blockRevision: sha256Hex,
    changeId: () => next('change'),
    changeSetId: () => next('change-set'),
    assessmentId: () => next('assessment'),
    eventId: () => next('event'),
    requestId: () => next('request'),
    operationId: () => next('operation'),
  }
}

export function sequentialIds(prefix = 'spike'): IdProvider & { requestId(): string; operationId(): string } {
  let value = 0
  const next = (kind: string) => `${prefix}/${kind}-${++value}`
  return {
    revisionId: () => next('revision'),
    blockRevision: async () => next('block-revision'),
    changeId: () => next('change'),
    changeSetId: () => next('change-set'),
    assessmentId: () => next('assessment'),
    eventId: () => next('event'),
    requestId: () => next('request'),
    operationId: () => next('operation'),
  }
}
