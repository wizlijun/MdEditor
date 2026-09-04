import { describe, expect, it } from 'vitest'
import {
  DOCUMENT_SESSION_STATE_SCHEMA,
  InMemoryDocumentSession,
  InvalidSessionStateError,
  sequentialIds,
  uuidIds,
  type DocumentRevision,
  type OperationBatch,
  type ReplaceBlockOperation,
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

function replace(
  requestId: string,
  blockId: string,
  expectedBlockRevision: string,
  markdown: string,
): OperationBatch & { operations: readonly [ReplaceBlockOperation] } {
  return {
    requestId,
    documentId: 'document-1',
    baseRevisionId: 'revision-1',
    operations: [{
      kind: 'block.replace',
      operationId: `${requestId}/op`,
      target: { blockId, expectedBlockRevision },
      payload: { content: markdown },
    }],
  }
}

function insert(
  snapshot: DocumentRevision,
  requestId: string,
  candidateBlockId: string,
  leftBlockId: string | null,
  rightBlockId: string | null,
  content = 'Inserted block.',
): OperationBatch {
  return {
    requestId,
    documentId: snapshot.documentId,
    baseRevisionId: snapshot.revisionId,
    operations: [{
      kind: 'block.insert',
      operationId: `${requestId}/op`,
      target: { leftBlockId, rightBlockId },
      payload: { candidateBlockId, content },
    }],
  }
}

function remove(snapshot: DocumentRevision, requestId: string, blockId: string): OperationBatch {
  const block = snapshot.blocks.find((item) => item.blockId === blockId)
  if (!block) throw new Error(`missing fixture block ${blockId}`)
  return {
    requestId,
    documentId: snapshot.documentId,
    baseRevisionId: snapshot.revisionId,
    operations: [{
      kind: 'block.delete',
      operationId: `${requestId}/op`,
      target: { blockId, expectedBlockRevision: block.blockRevision },
      payload: {},
    }],
  }
}

function legacyBatch(batch: OperationBatch) {
  return {
    requestId: batch.requestId,
    baseRevisionId: batch.baseRevisionId,
    operations: batch.operations.map((operation) => {
      if (operation.kind !== 'block.replace') throw new Error('legacy fixture only supports replace')
      return {
        kind: operation.kind,
        operationId: operation.operationId,
        blockId: operation.target.blockId,
        expectedBlockRevision: operation.target.expectedBlockRevision,
        markdown: operation.payload.content,
      }
    }),
  }
}

function legacyState(
  state: ReturnType<InMemoryDocumentSession['exportState']>,
  schema: 'notemd.cdr/document-session/v2' | 'notemd.cdr/document-session/v3',
) {
  return {
    ...structuredClone(state),
    schema,
    receipts: state.receipts.map((receipt) => {
      const signed = JSON.parse(receipt.batchSignature) as OperationBatch
      const outcome = receipt.outcome.kind === 'applied'
        ? {
            kind: 'applied',
            change: {
              ...receipt.outcome.change,
              operations: legacyBatch({ ...signed, operations: receipt.outcome.change.operations }).operations,
            },
          }
        : structuredClone(receipt.outcome)
      const migrated = {
        ...receipt,
        batchSignature: JSON.stringify(legacyBatch(signed)),
        outcome,
      }
      if (schema === 'notemd.cdr/document-session/v3') return migrated
      const { actorId: _actorId, ...withoutActor } = migrated
      return withoutActor
    }),
    proposals: state.proposals.map((proposal) => ({
      ...structuredClone(proposal),
      batch: legacyBatch(proposal.batch),
    })),
  }
}

describe('InMemoryDocumentSession', () => {
  it('persists insert/delete atomically and never reuses a retired block identity', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('structure'))
    const inserted = await session.submit(
      insert(session.snapshot(), 'insert-c', 'block-c', 'block-a', 'block-b'),
      'human',
    )
    expect(inserted.kind).toBe('applied')
    expect(session.snapshot().blocks.map((block) => block.blockId)).toEqual(['block-a', 'block-c', 'block-b'])

    const deleted = await session.submit(remove(session.snapshot(), 'delete-c', 'block-c'), 'human')
    expect(deleted.kind).toBe('applied')
    if (deleted.kind === 'applied') expect(deleted.change.blockRevisions).toEqual({})
    expect(session.snapshot().blocks.map((block) => block.blockId)).toEqual(['block-a', 'block-b'])
    expect(session.revisionHistory()).toHaveLength(2)

    const reopened = InMemoryDocumentSession.fromState(session.exportState(), sequentialIds('reopened'))
    const reused = await reopened.submit(
      insert(reopened.snapshot(), 'reuse-c', 'block-c', 'block-a', 'block-b'),
      'human',
    )
    expect(reused).toMatchObject({
      kind: 'conflicted',
      conflict: { code: 'invalid-operation', blockId: 'block-c' },
    })

    await reopened.submit(replace('after-delete', 'block-a', 'block-a/1', '# After delete'), 'human')
    const exported = reopened.exportState()
    const damaged = { ...exported, head: { ...exported.head, blocks: [
      exported.head.blocks[0],
      { blockId: 'block-c', blockRevision: 'block-c/reused', markdown: 'Reused.' },
      exported.head.blocks[1],
    ] } }
    expect(() => InMemoryDocumentSession.fromState(damaged, sequentialIds('damaged')))
      .toThrow('reuses a retired block ID')
  })

  it('accepts a structural proposal through the same atomic decision transition', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('proposal-structure'))
    const proposal = await session.propose(
      insert(session.snapshot(), 'proposal-insert', 'block-c', 'block-a', 'block-b'),
      'agent',
    )
    expect(session.snapshot().blocks.map((block) => block.blockId)).toEqual(['block-a', 'block-b'])
    const result = await session.decideProposal(proposal.changeSetId, 'accept', 'human')
    expect(result?.kind).toBe('applied')
    expect(session.snapshot().blocks.map((block) => block.blockId)).toEqual(['block-a', 'block-c', 'block-b'])
    expect(session.proposals()[0].status).toBe('applied')
  })

  it('classifies concurrent deletion as stale while allowing a different-block rebase', async () => {
    const concurrent = new InMemoryDocumentSession(fixture(), sequentialIds('concurrent-delete'))
    const base = concurrent.snapshot()
    const firstDelete = remove(base, 'delete-a-first', 'block-a')
    const secondDelete = {
      ...remove(base, 'delete-a-second', 'block-a'),
      operations: [{ ...firstDelete.operations[0], operationId: 'delete-a-second/op' }],
    }
    expect((await concurrent.submit(firstDelete, 'human:first')).kind).toBe('applied')
    expect(await concurrent.submit(secondDelete, 'human:second')).toMatchObject({
      kind: 'conflicted',
      conflict: { code: 'stale-base', blockId: 'block-a' },
    })

    const rebased = new InMemoryDocumentSession(fixture(), sequentialIds('rebased-delete'))
    const deleteA = remove(rebased.snapshot(), 'delete-a-after-b-change', 'block-a')
    expect((await rebased.submit(replace('change-b', 'block-b', 'block-b/1', 'Changed B.'), 'human')).kind)
      .toBe('applied')
    expect((await rebased.submit(deleteA, 'human')).kind).toBe('applied')
  })

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

  it('requires every live request and proposal to name a valid operation base', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('declared-base'))
    const unknownBase = { ...replace('unknown-base', 'block-a', 'block-a/1', '# Invalid base'), baseRevisionId: 'missing' }
    expect(await session.submit(unknownBase, 'human')).toMatchObject({
      kind: 'conflicted', conflict: { code: 'stale-base' },
    })
    await expect(session.propose({ ...unknownBase, requestId: 'unknown-proposal' }, 'agent'))
      .rejects.toThrow('CDR_PROPOSAL_CONFLICT: stale-base')
    expect(session.snapshot()).toEqual(fixture())
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
    await expect(session.submit({
      ...batch,
      operations: [{ ...batch.operations[0], payload: { content: '# Twice' } }],
    }, 'human'))
      .rejects.toThrow('CDR_IDEMPOTENCY_KEY_REUSED')
    await expect(session.submit(batch, 'another-human')).rejects.toThrow('CDR_IDEMPOTENCY_KEY_REUSED')
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

  it('does not reuse one actor\'s proposal for another actor', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const proposed = replace('shared-request', 'block-a', 'block-a/1', '# Suggested')
    await session.propose(proposed, 'agent:a')
    await expect(session.propose(proposed, 'agent:b')).rejects.toThrow('CDR_IDEMPOTENCY_KEY_REUSED')
    expect(session.proposals()).toHaveLength(1)
    expect(session.proposals()[0].actorId).toBe('agent:a')
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

  it('limits Stage 1A proposals to one visible block replacement', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const one = replace('proposal', 'block-a', 'block-a/1', '# Suggested')
    await expect(session.propose({ ...one, operations: [] }, 'agent')).rejects.toThrow('CDR_PROPOSAL_OPERATION_COUNT')
    await expect(session.propose({
      ...one,
      operations: [
        one.operations[0],
        {
          ...one.operations[0],
          operationId: 'proposal/op-2',
          target: { blockId: 'block-b', expectedBlockRevision: 'block-b/1' },
        },
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

  it('records the revision an asynchronous verifier actually inspected', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    await session.submit(replace('update', 'block-b', 'block-b/1', 'Changed constraint.'), 'human')

    const assessment = await session.assessRevision(
      'block-b', 'block-b/1', 'agent:verifier', 'verified', undefined, '输入材料支持原措辞。',
    )

    expect(assessment).toMatchObject({
      blockId: 'block-b', blockRevision: 'block-b/1', rationale: '输入材料支持原措辞。',
    })
    expect(session.assessmentIsOutdated(assessment)).toBe(true)
    await expect(session.assessRevision('block-b', 'missing', 'agent:verifier', 'verified'))
      .rejects.toThrow('CDR_ASSESSMENT_TARGET_NOT_FOUND')
  })

  it('exports one strict aggregate and restores compact receipts without snapshots', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('first'))
    const batch = replace('durable-request', 'block-a', 'block-a/1', '# Durable')
    await session.submit(batch, 'human')
    await session.propose(
      replace('proposal-request', 'block-b', 'block-b/1', 'Suggested.'),
      'agent', undefined, '更容易阅读。',
    )
    await session.assess('block-b', 'verifier', 'verified', undefined, '检查依据。')

    const state = session.exportState()
    expect(state.schema).toBe(DOCUMENT_SESSION_STATE_SCHEMA)
    expect(state.revisionHistory).toEqual([fixture()])
    expect(state.proposals[0].rationale).toBe('更容易阅读。')
    expect(state.assessments[0].rationale).toBe('检查依据。')
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

  it('migrates v4 proposals and assessments without rationale fields', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('v4'))
    await session.propose(
      replace('v4-proposal', 'block-b', 'block-b/1', 'Legacy proposal.'),
      'agent:legacy', undefined, 'v5-only proposal rationale',
    )
    await session.assess('block-b', 'agent:legacy', 'verified', undefined, 'v5-only assessment rationale')
    const current = structuredClone(session.exportState()) as any
    current.schema = 'notemd.cdr/document-session/v4'
    delete current.proposals[0].rationale
    delete current.assessments[0].rationale
    const restored = InMemoryDocumentSession.fromState(
      current,
      sequentialIds('restored-v4'),
    )
    expect(restored.exportState().schema).toBe(DOCUMENT_SESSION_STATE_SCHEMA)
    expect(restored.proposals()[0]).not.toHaveProperty('rationale')
    expect(restored.assessments()[0]).not.toHaveProperty('rationale')
  })

  it('migrates a v2 receipt actor from its durable audit event', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('legacy'))
    await session.submit(replace('legacy-request', 'block-a', 'block-a/1', '# Legacy'), 'human:legacy')
    const current = session.exportState()
    const legacy = legacyState(current, 'notemd.cdr/document-session/v2')

    const restored = InMemoryDocumentSession.fromState(legacy, sequentialIds('restored'))
    expect(restored.exportState().schema).toBe(DOCUMENT_SESSION_STATE_SCHEMA)
    expect(restored.exportState().receipts[0].actorId).toBe('human:legacy')
    await expect(restored.submit(replace('legacy-request', 'block-a', 'block-a/1', '# Legacy'), 'other'))
      .rejects.toThrow('CDR_IDEMPOTENCY_KEY_REUSED')
  })

  it('migrates v3 applied, conflicted, and proposal batches to current canonical signatures', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('legacy-v3'))
    const applied = replace('legacy-v3-applied', 'block-a', 'block-a/1', '# Legacy v3')
    const conflicted = replace('legacy-v3-conflict', 'block-b', 'missing', 'Never applied.')
    const proposal = replace('legacy-v3-proposal', 'block-b', 'block-b/1', 'Proposed in v3.')
    await session.submit(applied, 'human:legacy')
    await session.submit(conflicted, 'agent:legacy')
    await session.propose(proposal, 'agent:legacy')

    const restored = InMemoryDocumentSession.fromState(
      legacyState(session.exportState(), 'notemd.cdr/document-session/v3'),
      sequentialIds('restored'),
    )
    const migrated = restored.exportState()
    expect(migrated.schema).toBe(DOCUMENT_SESSION_STATE_SCHEMA)
    expect(JSON.parse(migrated.receipts[0].batchSignature)).toMatchObject({
      documentId: 'document-1',
      operations: [{ target: { blockId: 'block-a' }, payload: { content: '# Legacy v3' } }],
    })
    await expect(restored.submit(applied, 'human:legacy'))
      .resolves.toMatchObject({ kind: 'applied', duplicate: true })
    await expect(restored.submit(conflicted, 'agent:legacy'))
      .resolves.toMatchObject({ kind: 'conflicted' })
    const accepted = await restored.decideProposal(restored.proposals()[0].changeSetId, 'accept', 'human')
    expect(accepted?.kind).toBe('applied')
  })

  it('fails closed when a legacy receipt signature is not canonical JSON', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('legacy-invalid'))
    await session.submit(replace('legacy-invalid', 'block-a', 'block-a/1', '# Legacy'), 'human')
    const legacy = legacyState(session.exportState(), 'notemd.cdr/document-session/v3')
    legacy.receipts[0].batchSignature = ` ${legacy.receipts[0].batchSignature}`
    expect(() => InMemoryDocumentSession.fromState(legacy, sequentialIds('restored')))
      .toThrow('must be canonical')
  })

  it('does not let a new actor claim an unresolvable v2 receipt', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('legacy-conflict'))
    const stale = replace('legacy-conflict-request', 'block-a', 'missing-revision', '# Never applied')
    expect((await session.submit(stale, 'agent:legacy')).kind).toBe('conflicted')
    const current = session.exportState()
    const legacy = legacyState(current, 'notemd.cdr/document-session/v2')

    const restored = InMemoryDocumentSession.fromState(legacy, sequentialIds('restored'))
    expect(restored.exportState().receipts[0].actorId).toBe('legacy:unknown')
    await expect(restored.submit(stale, 'agent:new')).rejects.toThrow('CDR_IDEMPOTENCY_KEY_REUSED')
  })

  it('requires a current applied receipt actor to match exactly one audit event', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('audit-binding'))
    await session.submit(replace('audit-request', 'block-a', 'block-a/1', '# Applied'), 'human:author')
    const state = session.exportState()

    expect(() => InMemoryDocumentSession.fromState({
      ...state,
      audit: state.audit.map((event) => event.action === 'applied' ? { ...event, actorId: 'human:other' } : event),
    }, sequentialIds('restored'))).toThrow('must match exactly one applied audit actor')
  })

  it('migrates an ambiguous v2 applied actor to an unclaimable sentinel', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('ambiguous-audit'))
    await session.submit(replace('ambiguous-request', 'block-a', 'block-a/1', '# Applied'), 'human:author')
    const current = session.exportState()
    const applied = current.audit.find((event) => event.action === 'applied')!
    const legacy = {
      ...legacyState(current, 'notemd.cdr/document-session/v2'),
      audit: [...current.audit, { ...applied, eventId: 'legacy/duplicate-audit', actorId: 'human:other' }],
    }

    const restored = InMemoryDocumentSession.fromState(legacy, sequentialIds('restored'))
    expect(restored.exportState().receipts[0].actorId).toBe('legacy:unknown')
    expect(() => InMemoryDocumentSession.fromState(restored.exportState(), sequentialIds('reopened'))).not.toThrow()
    await expect(restored.submit(replace('ambiguous-request', 'block-a', 'block-a/1', '# Applied'), 'human:author'))
      .rejects.toThrow('CDR_IDEMPOTENCY_KEY_REUSED')
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

  it('requires applied receipts to prove one linear revision chain', async () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('linear-history'))
    await session.submit(replace('linear-a', 'block-a', 'block-a/1', '# Linear A'), 'human')
    await session.submit(replace('linear-b', 'block-b', 'block-b/1', 'Linear B.'), 'human')
    const state = session.exportState()

    expect(() => InMemoryDocumentSession.fromState({
      ...state,
      revisionHistory: [...state.revisionHistory].reverse(),
    }, sequentialIds('reordered'))).toThrow('must describe every adjacent document revision exactly once')
    expect(() => InMemoryDocumentSession.fromState({
      ...state,
      receipts: state.receipts.slice(1),
    }, sequentialIds('missing-receipt'))).toThrow('must describe every adjacent document revision exactly once')
    expect(() => InMemoryDocumentSession.fromState({
      ...state,
      receipts: [state.receipts[0], {
        ...state.receipts[1],
        outcome: state.receipts[1].outcome.kind === 'applied'
          ? {
              kind: 'applied' as const,
              change: { ...state.receipts[1].outcome.change, changeId: state.receipts[0].outcome.kind === 'applied'
                ? state.receipts[0].outcome.change.changeId
                : 'unreachable' },
            }
          : state.receipts[1].outcome,
      }],
    }, sequentialIds('duplicate-change'))).toThrow('contains duplicate')

    const signed = JSON.parse(state.receipts[0].batchSignature)
    expect(() => InMemoryDocumentSession.fromState({
      ...state,
      receipts: [{
        ...state.receipts[0],
        submittedBaseRevisionId: 'missing',
        batchSignature: JSON.stringify({ ...signed, baseRevisionId: 'missing' }),
      }, state.receipts[1]],
    }, sequentialIds('unknown-base'))).toThrow('does not describe a valid operation base')
  })

  it('provides UUID-backed production ids while retaining deterministic test ids', () => {
    const ids = uuidIds()
    expect(ids.revisionId()).toMatch(/^revision\/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
    expect(sequentialIds('fixture').revisionId()).toBe('fixture/revision-1')
  })
})
