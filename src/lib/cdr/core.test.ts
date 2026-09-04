import { describe, expect, it } from 'vitest'
import { applyDocumentChange, validateDocumentChange, type DocumentRevision } from './core'
import type { OperationBatch } from './operation'

const snapshot: DocumentRevision = {
  documentId: 'document-1',
  revisionId: 'revision-2',
  blocks: [
    { blockId: 'a', blockRevision: 'a/1', markdown: 'A' },
    { blockId: 'b', blockRevision: 'b/1', markdown: 'B' },
  ],
}

function replace(blockId: string, expectedBlockRevision: string, content: string): OperationBatch {
  return {
    requestId: `request-${blockId}`,
    documentId: snapshot.documentId,
    baseRevisionId: 'revision-1',
    operations: [{
      kind: 'block.replace',
      operationId: `operation-${blockId}`,
      target: { blockId, expectedBlockRevision },
      payload: { content },
    }],
  }
}

function insert(
  candidateBlockId: string,
  leftBlockId: string | null,
  rightBlockId: string | null,
): OperationBatch {
  return {
    requestId: `request-${candidateBlockId}`,
    documentId: snapshot.documentId,
    baseRevisionId: snapshot.revisionId,
    operations: [{
      kind: 'block.insert',
      operationId: `operation-${candidateBlockId}`,
      target: { leftBlockId, rightBlockId },
      payload: { candidateBlockId, content: candidateBlockId.toUpperCase() },
    }],
  }
}

function remove(blockId: string, expectedBlockRevision: string): OperationBatch {
  return {
    requestId: `delete-${blockId}`,
    documentId: snapshot.documentId,
    baseRevisionId: snapshot.revisionId,
    operations: [{
      kind: 'block.delete',
      operationId: `operation-delete-${blockId}`,
      target: { blockId, expectedBlockRevision },
      payload: {},
    }],
  }
}

describe('DocumentCore', () => {
  it('safely rebases an unchanged replacement target and preserves block identity', () => {
    const batch = replace('b', 'b/1', 'B2')
    expect(validateDocumentChange(snapshot, batch)).toBeNull()
    expect(applyDocumentChange(snapshot, batch, {
      revisionId: 'revision-3',
      blockRevisions: { b: 'b/2' },
    })).toEqual({
      ...snapshot,
      revisionId: 'revision-3',
      blocks: [snapshot.blocks[0], { blockId: 'b', blockRevision: 'b/2', markdown: 'B2' }],
    })
    expect(snapshot.blocks[1].markdown).toBe('B')
  })

  it('inserts at the beginning, middle, and end only while the exact gap remains', () => {
    const cases = [
      { batch: insert('head', null, 'a'), expected: ['head', 'a', 'b'] },
      { batch: insert('middle', 'a', 'b'), expected: ['a', 'middle', 'b'] },
      { batch: insert('tail', 'b', null), expected: ['a', 'b', 'tail'] },
    ]
    for (const { batch, expected } of cases) {
      const candidate = batch.operations[0].kind === 'block.insert'
        ? batch.operations[0].payload.candidateBlockId
        : ''
      expect(validateDocumentChange(snapshot, batch)).toBeNull()
      expect(applyDocumentChange(snapshot, batch, {
        revisionId: `revision-${candidate}`,
        blockRevisions: { [candidate]: `${candidate}/1` },
      }).blocks.map((block) => block.blockId)).toEqual(expected)
    }
    const changedGap = applyDocumentChange(snapshot, insert('first', 'a', 'b'), {
      revisionId: 'revision-3', blockRevisions: { first: 'first/1' },
    })
    expect(validateDocumentChange(changedGap, insert('second', 'a', 'b'))).toMatchObject({ code: 'stale-base' })
  })

  it('allows an anchor content update but rejects a retired candidate identity', () => {
    const replaced = applyDocumentChange(snapshot, replace('a', 'a/1', 'A2'), {
      revisionId: 'revision-3', blockRevisions: { a: 'a/2' },
    })
    expect(validateDocumentChange(replaced, insert('c', 'a', 'b'), {
      knownBlockIds: new Set(['a', 'b']),
    })).toBeNull()
    expect(validateDocumentChange(snapshot, insert('retired', 'a', 'b'), {
      knownBlockIds: new Set(['a', 'b', 'retired']),
    })).toMatchObject({ code: 'invalid-operation', blockId: 'retired' })
  })

  it('deletes an exact target without inventing a resulting block revision', () => {
    const batch = remove('a', 'a/1')
    expect(applyDocumentChange(snapshot, batch, {
      revisionId: 'revision-3', blockRevisions: {},
    })).toEqual({
      ...snapshot,
      revisionId: 'revision-3',
      blocks: [snapshot.blocks[1]],
    })
    expect(validateDocumentChange(snapshot, remove('a', 'a/0'))).toMatchObject({ code: 'stale-base' })
    expect(validateDocumentChange({ ...snapshot, blocks: [snapshot.blocks[1]] }, batch))
      .toMatchObject({ code: 'stale-base', blockId: 'a' })
    expect(validateDocumentChange({ ...snapshot, blocks: [snapshot.blocks[0]] }, batch))
      .toMatchObject({ code: 'invalid-operation' })
  })

  it('rejects another document, mixed structural batches, and invalid prepared results atomically', () => {
    expect(validateDocumentChange(snapshot, { ...replace('b', 'b/1', 'B2'), documentId: 'other' }))
      .toMatchObject({ code: 'invalid-operation' })
    expect(validateDocumentChange(snapshot, {
      ...insert('c', 'a', 'b'),
      operations: [...insert('c', 'a', 'b').operations, ...replace('a', 'a/1', 'A2').operations],
    })).toMatchObject({ code: 'invalid-operation' })
    expect(() => applyDocumentChange(snapshot, replace('b', 'b/1', 'B2'), {
      revisionId: 'revision-3', blockRevisions: {},
    })).toThrow('CDR_CORE_PREPARED_BLOCK_REVISIONS')
    expect(() => applyDocumentChange(snapshot, remove('a', 'a/1'), {
      revisionId: 'revision-3', blockRevisions: { a: 'invented' },
    })).toThrow('CDR_CORE_PREPARED_BLOCK_REVISIONS')
    expect(snapshot.blocks.map((block) => block.markdown)).toEqual(['A', 'B'])
  })
})
