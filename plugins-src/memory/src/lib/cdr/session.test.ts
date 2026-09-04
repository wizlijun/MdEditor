import { describe, expect, it } from 'vitest'
import { InMemoryDocumentSession, sequentialIds, type DocumentRevision, type OperationBatch } from './session'

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
    expect(session.submit(replace('r1', 'block-a', 'block-a/1', '# New background'), 'human').kind).toBe('applied')
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

  it('binds an assessment to the exact block revision', () => {
    const session = new InMemoryDocumentSession(fixture(), sequentialIds('test'))
    const assessment = session.assess('block-b', 'verifier', 'verified')
    expect(session.assessmentIsOutdated(assessment)).toBe(false)
    session.submit(replace('update', 'block-b', 'block-b/1', 'Changed constraint.'), 'human')
    expect(session.assessmentIsOutdated(assessment)).toBe(true)
  })
})
