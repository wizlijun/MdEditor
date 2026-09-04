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
  it('safely rebases a different-block edit while rejecting a stale same-block edit', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const first = await session.submit(replace('r1', 'block-a', 'block-a/1', '# New background'), 'human')
    expect(first.kind).toBe('applied')
    if (first.kind === 'applied') expect(first.change.baseRevisionId).toBe('revision-1')
    expect((await session.submit(replace('r2', 'block-b', 'block-b/1', 'New constraint.'), 'agent-b')).kind).toBe('applied')
    const restored = InMemoryDocumentSession.fromState(session.exportState(), sequentialIds('restored'))
    await expect(restored.submit(replace('r2', 'block-b', 'block-b/1', 'New constraint.'), 'agent-b'))
      .resolves.toMatchObject({ kind: 'applied', duplicate: true })

    const stale = await session.submit(replace('r3', 'block-a', 'block-a/1', '# Stale overwrite'), 'agent-a')
    expect(stale.kind).toBe('conflicted')
    expect(session.snapshot().blocks.find((block) => block.blockId === 'block-a')?.markdown).toBe('# New background')
  })

  it('serializes concurrent submissions on the directly exported session', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const [first, second] = await Promise.all([
      session.submit(replace('parallel-a', 'block-a', 'block-a/1', '# Parallel A'), 'human-a'),
      session.submit(replace('parallel-b', 'block-b', 'block-b/1', 'Parallel B'), 'human-b'),
    ])

    expect(first.kind).toBe('applied')
    expect(second.kind).toBe('applied')
    expect(session.snapshot().blocks.map((block) => block.markdown)).toEqual(['# Parallel A', 'Parallel B'])
    expect(session.revisionHistory()).toHaveLength(2)
  })

  it('returns the first receipt for an identical request and rejects key reuse with another payload', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const batch = replace('same', 'block-a', 'block-a/1', '# Once')
    const first = await session.submit(batch, 'human')
    const second = await session.submit(batch, 'human')
    expect(first.kind).toBe('applied')
    expect(second).toMatchObject({ kind: 'applied', duplicate: true })
    expect(session.audit().filter((event) => event.action === 'applied')).toHaveLength(1)
    await expect(session.submit({ ...batch, operations: [{ ...batch.operations[0], markdown: '# Twice' }] }, 'human'))
      .rejects.toThrow('CDR_IDEMPOTENCY_KEY_REUSED')
  })

  it('keeps a proposal out of the document and conflicts when its exact target changes before acceptance', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const proposal = await session.propose(replace('proposal', 'block-a', 'block-a/1', '# Suggested'), 'agent-a')
    expect(session.snapshot().blocks[0].markdown).toBe('# Background')
    await session.submit(replace('human', 'block-a', 'block-a/1', '# Human edit'), 'human')

    const decision = await session.decideProposal(proposal.changeSetId, 'accept', 'human')
    expect(decision?.kind).toBe('conflicted')
    expect(session.proposals()[0].status).toBe('conflicted')
    expect(session.snapshot().blocks[0].markdown).toBe('# Human edit')
  })

  it('serializes concurrent proposal decisions into one final decision and one audit event', async () => {
    const accepted = new InMemoryDocumentSession(fixture(), sequentialIds('accepted'))
    const proposal = await accepted.propose(
      replace('proposal-accepted', 'block-a', 'block-a/1', '# Accepted once'),
      'agent',
    )
    const [firstAccept, secondAccept, lateReject] = await Promise.all([
      accepted.decideProposal(proposal.changeSetId, 'accept', 'human-a'),
      accepted.decideProposal(proposal.changeSetId, 'accept', 'human-b'),
      accepted.decideProposal(proposal.changeSetId, 'reject', 'human-c'),
    ])
    expect(firstAccept?.kind).toBe('applied')
    expect(secondAccept).toBeNull()
    expect(lateReject).toBeNull()
    expect(accepted.proposals()[0].status).toBe('applied')
    expect(accepted.audit().filter((event) => event.action === 'proposal-applied')).toHaveLength(1)
    expect(accepted.audit().filter((event) => event.action === 'proposal-rejected')).toHaveLength(0)

    const rejected = new InMemoryDocumentSession(fixture(), sequentialIds('rejected'))
    const rejectedProposal = await rejected.propose(
      replace('proposal-rejected', 'block-a', 'block-a/1', '# Never applied'),
      'agent',
    )
    await Promise.all([
      rejected.decideProposal(rejectedProposal.changeSetId, 'reject', 'human-a'),
      rejected.decideProposal(rejectedProposal.changeSetId, 'accept', 'human-b'),
    ])
    expect(rejected.proposals()[0].status).toBe('rejected')
    expect(rejected.snapshot()).toEqual(fixture())
    expect(rejected.audit().filter((event) => event.action === 'proposal-rejected')).toHaveLength(1)
    expect(rejected.audit().filter((event) => event.action === 'proposal-applied')).toHaveLength(0)
  })

  it('limits Stage 0 proposals to one visible block replacement', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const one = replace('proposal', 'block-a', 'block-a/1', '# Suggested')
    await expect(session.propose({ ...one, operations: [] }, 'agent')).rejects.toThrow('CDR_PROPOSAL_OPERATION_COUNT')
    await expect(session.propose({
      ...one,
      operations: [
        one.operations[0],
        { ...one.operations[0], operationId: 'proposal/op-2', blockId: 'block-b', expectedBlockRevision: 'block-b/1' },
      ],
    }, 'agent')).rejects.toThrow('CDR_PROPOSAL_OPERATION_COUNT')

    const state = session.exportState()
    expect(() => InMemoryDocumentSession.fromState({
      ...state,
      proposals: [{
        changeSetId: 'damaged', actorId: 'agent', status: 'pending', batch: { ...one, operations: [] },
      }],
    }, sequentialIds('restore'))).toThrow('CDR_STATE_INVALID')
  })

  it('binds an assessment to the exact block revision', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const assessment = await session.assess('block-b', 'verifier', 'verified')
    expect(session.assessmentIsOutdated(assessment)).toBe(false)
    await session.submit(replace('update', 'block-b', 'block-b/1', 'Changed constraint.'), 'human')
    expect(session.assessmentIsOutdated(assessment)).toBe(true)
  })

  it('exports one strict aggregate and restores compact receipts without snapshots', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('first'))
    const batch = replace('durable-request', 'block-a', 'block-a/1', '# Durable')
    await session.submit(batch, 'human')
    await session.propose(replace('proposal-request', 'block-b', 'block-b/1', 'Suggested.'), 'agent')
    await session.assess('block-b', 'verifier', 'verified')

    const state = session.exportState()
    expect(state.schema).toBe(DOCUMENT_SESSION_STATE_SCHEMA)
    expect(state.revisionHistory).toEqual([fixture()])
    expect(state.receipts).toHaveLength(1)
    expect(JSON.stringify(state.receipts)).not.toContain('snapshot')

    const restored = InMemoryDocumentSession.fromState(state, sequentialIds('restored'))
    expect(restored.snapshot()).toEqual(session.snapshot())
    expect(restored.revisionHistory()).toEqual(session.revisionHistory())
    expect(restored.proposals()).toEqual(session.proposals())
    expect(restored.assessments()).toEqual(session.assessments())
    expect(restored.audit()).toEqual(session.audit())
    await expect(restored.submit(batch, 'human')).resolves.toMatchObject({ kind: 'applied', duplicate: true })
    expect(restored.audit()).toHaveLength(session.audit().length)
  })

  it('rejects unknown fields and inconsistent receipt payloads while restoring', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    await session.submit(replace('request', 'block-a', 'block-a/1', '# Durable'), 'human')
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
