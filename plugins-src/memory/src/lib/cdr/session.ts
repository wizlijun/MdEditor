/**
 * Domain-neutral Stage 0 application slice.
 *
 * This stays inside the first consumer until a second real profile proves the
 * extraction boundary. It intentionally knows nothing about Memory claims,
 * vault paths, ProseMirror or Yjs.
 */

export interface DocumentBlock {
  blockId: string
  blockRevision: string
  markdown: string
}

export interface DocumentRevision {
  documentId: string
  revisionId: string
  blocks: readonly DocumentBlock[]
}

export interface ReplaceBlockOperation {
  kind: 'block.replace'
  operationId: string
  blockId: string
  expectedBlockRevision: string
  markdown: string
}

export interface OperationBatch {
  requestId: string
  baseRevisionId: string
  operations: readonly ReplaceBlockOperation[]
}

export interface AppliedChange {
  changeId: string
  originRequestId?: string
  revisionId: string
  blockRevisions: Readonly<Record<string, string>>
  operations: readonly ReplaceBlockOperation[]
}

export interface Conflict {
  code: 'stale-base' | 'invalid-operation'
  message: string
  blockId?: string
}

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

export interface SessionIds {
  revisionId(): string
  blockRevision(): string
  changeId(): string
  changeSetId(): string
  assessmentId(): string
  eventId(): string
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

function signature(batch: OperationBatch): string {
  return JSON.stringify(batch)
}

export class InMemoryDocumentSession {
  #head: DocumentRevision
  #receipts = new Map<string, { signature: string; result: SubmitResult }>()
  #proposals: Proposal[] = []
  #assessments: Assessment[] = []
  #audit: AuditEvent[] = []

  constructor(initial: DocumentRevision, private readonly ids: SessionIds) {
    this.#head = cloneRevision(initial)
  }

  snapshot(): DocumentRevision {
    return cloneRevision(this.#head)
  }

  proposals(): readonly Proposal[] {
    return this.#proposals.map((proposal) => ({ ...proposal, batch: cloneBatch(proposal.batch) }))
  }

  assessments(): readonly Assessment[] {
    return this.#assessments.map((assessment) => ({ ...assessment }))
  }

  audit(): readonly AuditEvent[] {
    return this.#audit.map((event) => ({ ...event }))
  }

  submit(batch: OperationBatch, actorId: string): SubmitResult {
    const payload = signature(batch)
    const existing = this.#receipts.get(batch.requestId)
    if (existing) {
      if (existing.signature !== payload) throw new Error('CDR_IDEMPOTENCY_KEY_REUSED')
      if (existing.result.kind === 'applied') {
        return { ...existing.result, snapshot: this.snapshot(), duplicate: true }
      }
      return { ...existing.result, snapshot: this.snapshot() }
    }

    const conflict = this.#validate(batch)
    if (conflict) {
      const result: SubmitResult = { kind: 'conflicted', conflict, snapshot: this.snapshot() }
      this.#receipts.set(batch.requestId, { signature: payload, result })
      return result
    }

    const changed = new Map<string, string>()
    const nextBlocks = this.#head.blocks.map((block) => {
      const operation = batch.operations.find((item) => item.blockId === block.blockId)
      if (!operation) return block
      const blockRevision = this.ids.blockRevision()
      changed.set(block.blockId, blockRevision)
      return { ...block, markdown: operation.markdown, blockRevision }
    })
    const revisionId = this.ids.revisionId()
    this.#head = { ...this.#head, revisionId, blocks: nextBlocks }
    const change: AppliedChange = {
      changeId: this.ids.changeId(),
      originRequestId: batch.requestId,
      revisionId,
      blockRevisions: Object.fromEntries(changed),
      operations: batch.operations.map((operation) => ({ ...operation })),
    }
    const result: SubmitResult = { kind: 'applied', change, snapshot: this.snapshot(), duplicate: false }
    this.#receipts.set(batch.requestId, { signature: payload, result })
    this.#audit.push({ eventId: this.ids.eventId(), actorId, action: 'applied', targetId: change.changeId })
    return result
  }

  propose(batch: OperationBatch, actorId: string): Proposal {
    const existing = this.#proposals.find((proposal) => proposal.batch.requestId === batch.requestId)
    if (existing) {
      if (signature(existing.batch) !== signature(batch)) throw new Error('CDR_IDEMPOTENCY_KEY_REUSED')
      return { ...existing, batch: cloneBatch(existing.batch) }
    }
    const proposal: Proposal = {
      changeSetId: this.ids.changeSetId(),
      actorId,
      status: 'pending',
      batch: cloneBatch(batch),
    }
    this.#proposals = [...this.#proposals, proposal]
    this.#audit.push({ eventId: this.ids.eventId(), actorId, action: 'proposed', targetId: proposal.changeSetId })
    return { ...proposal, batch: cloneBatch(proposal.batch) }
  }

  decideProposal(changeSetId: string, decision: 'accept' | 'reject', actorId: string): SubmitResult | null {
    const proposal = this.#proposals.find((item) => item.changeSetId === changeSetId)
    if (!proposal) throw new Error('CDR_PROPOSAL_NOT_FOUND')
    if (proposal.status !== 'pending') return null
    if (decision === 'reject') {
      proposal.status = 'rejected'
      this.#audit.push({ eventId: this.ids.eventId(), actorId, action: 'proposal-rejected', targetId: changeSetId })
      return null
    }

    // Acceptance is a new decision request but retains the exact block bases
    // from the proposal. This makes a target changed since proposal creation
    // conflict instead of overwriting it.
    const result = this.submit({
      ...proposal.batch,
      requestId: `decision/${changeSetId}`,
    }, actorId)
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

  assess(blockId: string, actorId: string, conclusion: Assessment['conclusion']): Assessment {
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

  #validate(batch: OperationBatch): Conflict | null {
    if (!batch.operations.length) return { code: 'invalid-operation', message: '操作批不能为空。' }
    const seen = new Set<string>()
    for (const operation of batch.operations) {
      if (!operation.markdown.trim()) return { code: 'invalid-operation', message: '块内容不能为空。', blockId: operation.blockId }
      if (seen.has(operation.blockId)) return { code: 'invalid-operation', message: '同一批操作不能两次修改同一块。', blockId: operation.blockId }
      seen.add(operation.blockId)
      const block = this.#head.blocks.find((item) => item.blockId === operation.blockId)
      if (!block) return { code: 'invalid-operation', message: '目标块不存在。', blockId: operation.blockId }
      if (block.blockRevision !== operation.expectedBlockRevision) {
        return { code: 'stale-base', message: '目标块已被修改，本次操作未覆盖新版本。', blockId: operation.blockId }
      }
    }
    return null
  }
}

export function sequentialIds(prefix = 'spike'): SessionIds & { requestId(): string; operationId(): string } {
  let value = 0
  const next = (kind: string) => `${prefix}/${kind}-${++value}`
  return {
    revisionId: () => next('revision'),
    blockRevision: () => next('block-revision'),
    changeId: () => next('change'),
    changeSetId: () => next('change-set'),
    assessmentId: () => next('assessment'),
    eventId: () => next('event'),
    requestId: () => next('request'),
    operationId: () => next('operation'),
  }
}
