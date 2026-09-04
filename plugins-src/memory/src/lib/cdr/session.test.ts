import { describe, expect, it } from 'vitest'
import {
  DOCUMENT_SESSION_STATE_SCHEMA,
  InMemoryDocumentSession,
  InvalidSessionStateError,
  sequentialIds,
  uuidIds,
  type DocumentRevision,
  type OperationBatch,
} from './session'

function fixture(): DocumentRevision {
  return {
    documentId: 'document-1',
    revisionId: 'revision-1',
    blocks: [
      { blockId: 'block-a', blockRevision: 'block-a/1', markdown: '# Background' },
      { blockId: 'block-b', blockRevision: 'block-b/1', markdown: 'Keep this constraint.' },
    ],
  }
}

function replace(requestId: string, blockId: string, expectedBlockRevision: string, markdown: string): OperationBatch {
  return {
    requestId,
    baseRevisionId: 'revision-1',
    operations: [{ kind: 'block.replace', operationId: `${requestId}/op`, blockId, expectedBlockRevision, markdown }],
  }
}

describe('InMemoryDocumentSession', () => {
  it('safely rebases a different-block edit while rejecting a stale same-block edit', () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const first = session.submit(replace('r1', 'block-a', 'block-a/1', '# New background'), 'human')
    expect(first.kind).toBe('applied')
    if (first.kind === 'applied') expect(first.change.baseRevisionId).toBe('revision-1')
    expect(session.submit(replace('r2', 'block-b', 'block-b/1', 'New constraint.'), 'agent-b').kind).toBe('applied')

    const stale = session.submit(replace('r3', 'block-a', 'block-a/1', '# Stale overwrite'), 'agent-a')
    expect(stale.kind).toBe('conflicted')
    expect(session.snapshot().blocks.find((block) => block.blockId === 'block-a')?.markdown).toBe('# New background')
  })

  it('returns the first receipt for an identical request and rejects key reuse with another payload', () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const batch = replace('same', 'block-a', 'block-a/1', '# Once')
    const first = session.submit(batch, 'human')
    const second = session.submit(batch, 'human')
    expect(first.kind).toBe('applied')
    expect(second).toMatchObject({ kind: 'applied', duplicate: true })
    expect(session.audit().filter((event) => event.action === 'applied')).toHaveLength(1)
    expect(() => session.submit({ ...batch, operations: [{ ...batch.operations[0], markdown: '# Twice' }] }, 'human'))
      .toThrow('CDR_IDEMPOTENCY_KEY_REUSED')
  })

  it('keeps a proposal out of the document and conflicts when its exact target changes before acceptance', () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const proposal = session.propose(replace('proposal', 'block-a', 'block-a/1', '# Suggested'), 'agent-a')
    expect(session.snapshot().blocks[0].markdown).toBe('# Background')
    session.submit(replace('human', 'block-a', 'block-a/1', '# Human edit'), 'human')

    const decision = session.decideProposal(proposal.changeSetId, 'accept', 'human')
    expect(decision?.kind).toBe('conflicted')
    expect(session.proposals()[0].status).toBe('conflicted')
    expect(session.snapshot().blocks[0].markdown).toBe('# Human edit')
  })

  it('limits Stage 0 proposals to one visible block replacement', () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const one = replace('proposal', 'block-a', 'block-a/1', '# Suggested')
    expect(() => session.propose({ ...one, operations: [] }, 'agent')).toThrow('CDR_PROPOSAL_OPERATION_COUNT')
    expect(() => session.propose({
      ...one,
      operations: [
        one.operations[0],
        { ...one.operations[0], operationId: 'proposal/op-2', blockId: 'block-b', expectedBlockRevision: 'block-b/1' },
      ],
    }, 'agent')).toThrow('CDR_PROPOSAL_OPERATION_COUNT')

    const state = session.exportState()
    expect(() => InMemoryDocumentSession.fromState({
      ...state,
      proposals: [{
        changeSetId: 'damaged', actorId: 'agent', status: 'pending', batch: { ...one, operations: [] },
      }],
    }, sequentialIds('restore'))).toThrow('CDR_STATE_INVALID')
  })

  it('binds an assessment to the exact block revision', () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const assessment = session.assess('block-b', 'verifier', 'verified')
    expect(session.assessmentIsOutdated(assessment)).toBe(false)
    session.submit(replace('update', 'block-b', 'block-b/1', 'Changed constraint.'), 'human')
    expect(session.assessmentIsOutdated(assessment)).toBe(true)
  })

  it('exports one strict aggregate and restores compact receipts without snapshots', () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('first'))
    const batch = replace('durable-request', 'block-a', 'block-a/1', '# Durable')
    session.submit(batch, 'human')
    session.propose(replace('proposal-request', 'block-b', 'block-b/1', 'Suggested.'), 'agent')
    session.assess('block-b', 'verifier', 'verified')

    const state = session.exportState()
    expect(state.schema).toBe(DOCUMENT_SESSION_STATE_SCHEMA)
    expect(state.receipts).toHaveLength(1)
    expect(JSON.stringify(state.receipts)).not.toContain('snapshot')

    const restored = InMemoryDocumentSession.fromState(state, sequentialIds('restored'))
    expect(restored.snapshot()).toEqual(session.snapshot())
    expect(restored.proposals()).toEqual(session.proposals())
    expect(restored.assessments()).toEqual(session.assessments())
    expect(restored.audit()).toEqual(session.audit())
    expect(restored.submit(batch, 'human')).toMatchObject({ kind: 'applied', duplicate: true })
    expect(restored.audit()).toHaveLength(session.audit().length)
  })

  it('rejects unknown fields and inconsistent receipt payloads while restoring', () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    session.submit(replace('request', 'block-a', 'block-a/1', '# Durable'), 'human')
    const state = session.exportState()

    expect(() => InMemoryDocumentSession.fromState({ ...state, future: true }, sequentialIds('restore')))
      .toThrow(InvalidSessionStateError)
    expect(() => InMemoryDocumentSession.fromState({
      ...state,
      receipts: [{ ...state.receipts[0], batchSignature: 'tampered' }],
    }, sequentialIds('restore'))).toThrow('CDR_STATE_INVALID')
    expect(() => InMemoryDocumentSession.fromState({
      ...state,
      head: { ...state.head, blocks: [] },
    }, sequentialIds('restore'))).toThrow('CDR_STATE_INVALID')
  })

  it('provides UUID-backed production ids while retaining deterministic test ids', () => {
    const ids = uuidIds()
    expect(ids.revisionId()).toMatch(/^revision\/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    expect(sequentialIds('fixture').revisionId()).toBe('fixture/revision-1')
  })
})
