import { describe, expect, it } from 'vitest'
import { applyDocumentChange, validateDocumentChange, type DocumentRevision } from './core'
import type { OperationBatch, MoveBlockOperation } from './operation'

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

  it('rejects another document and invalid prepared results atomically', () => {
    expect(validateDocumentChange(snapshot, { ...replace('b', 'b/1', 'B2'), documentId: 'other' }))
      .toMatchObject({ code: 'invalid-operation' })
    expect(validateDocumentChange(snapshot, {
      ...insert('c', 'a', 'b'),
      operations: [...insert('c', 'a', 'b').operations, ...replace('a', 'a/1', 'A2').operations],
    })).toBeNull()
    expect(() => applyDocumentChange(snapshot, replace('b', 'b/1', 'B2'), {
      revisionId: 'revision-3', blockRevisions: {},
    })).toThrow('CDR_CORE_PREPARED_BLOCK_REVISIONS')
    expect(() => applyDocumentChange(snapshot, remove('a', 'a/1'), {
      revisionId: 'revision-3', blockRevisions: { a: 'invented' },
    })).toThrow('CDR_CORE_PREPARED_BLOCK_REVISIONS')
    expect(snapshot.blocks.map((block) => block.markdown)).toEqual(['A', 'B'])
  })

  it('applies a cross-block replacement and multiple deletes as one revision, including a full clear', () => {
    const initial = { ...snapshot, blocks: [...snapshot.blocks, { blockId: 'c', blockRevision: 'c/1', markdown: 'C' }] }
    const batch = { ...replace('a', 'a/1', ''), operations: [
      ...replace('a', 'a/1', '').operations,
      ...remove('b', 'b/1').operations,
      ...remove('c', 'c/1').operations,
    ] }
    expect(applyDocumentChange(initial, batch, { revisionId: 'clear', blockRevisions: { a: 'empty' } }).blocks)
      .toEqual([{ blockId: 'a', blockRevision: 'empty', markdown: '' }])
    const failing = { ...batch, operations: [...batch.operations.slice(0, 2), ...remove('c', 'stale').operations] }
    expect(validateDocumentChange(initial, failing)).toMatchObject({ code: 'stale-base', blockId: 'c' })
    expect(() => applyDocumentChange(initial, failing, { revisionId: 'bad', blockRevisions: { a: 'empty' } }))
      .toThrow('CDR_CORE_CONFLICT')
    expect(initial.blocks.map((block) => block.markdown)).toEqual(['A', 'B', 'C'])
  })

  it('interprets each insert gap after preceding operations and validates the final nonempty document', () => {
    const batch = { ...insert('c', 'a', 'b'), operations: [
      ...insert('c', 'a', 'b').operations,
      ...insert('d', 'c', 'b').operations,
      ...remove('b', 'b/1').operations,
    ] }
    expect(applyDocumentChange(snapshot, batch, { revisionId: 'many', blockRevisions: { c: 'c/1', d: 'd/1' } })
      .blocks.map((block) => block.blockId)).toEqual(['a', 'c', 'd'])
    const empty = { ...batch, operations: [...remove('a', 'a/1').operations, ...remove('b', 'b/1').operations] }
    expect(validateDocumentChange(snapshot, empty)).toMatchObject({ code: 'invalid-operation' })
    expect(applyDocumentChange(snapshot, { ...empty, operations: [...empty.operations, ...insert('c', null, null).operations] }, {
      revisionId: 'new-only', blockRevisions: { c: 'c/1' },
    }).blocks.map((block) => block.blockId)).toEqual(['c'])
    expect(validateDocumentChange(snapshot, insert('c', null, null))).toMatchObject({ code: 'stale-base' })
  })

  it('moves with exact source/destination anchors, preserves hashes, and permits replace plus move', () => {
    const move: MoveBlockOperation = {
      kind: 'block.move', operationId: 'move-a', target: { blockId: 'a', expectedBlockRevision: 'a/1' },
      payload: {
        source: { leftBlockId: null, rightBlockId: 'b' },
        destination: { leftBlockId: 'b', rightBlockId: null },
      },
    }
    const batch = { ...replace('a', 'a/1', 'Edited A'), operations: [move] }
    expect(applyDocumentChange(snapshot, batch, { revisionId: 'moved', blockRevisions: {} }).blocks)
      .toEqual([snapshot.blocks[1], snapshot.blocks[0]])
    expect(applyDocumentChange(snapshot, { ...batch, operations: [...replace('a', 'a/1', 'Edited A').operations, move] }, {
      revisionId: 'edit-and-move', blockRevisions: { a: 'a/2' },
    }).blocks[1]).toEqual({ blockId: 'a', blockRevision: 'a/2', markdown: 'Edited A' })
    const inserted = applyDocumentChange(snapshot, insert('c', 'a', 'b'), { revisionId: 'inserted', blockRevisions: { c: 'c/1' } })
    expect(validateDocumentChange(inserted, batch)).toMatchObject({ code: 'stale-base' })
    expect(validateDocumentChange(snapshot, { ...batch, operations: [{ ...move, payload: {
      ...move.payload, destination: { leftBlockId: 'a', rightBlockId: null },
    } }] })).toMatchObject({ code: 'invalid-operation' })
  })

  it('restores only exact historical content and permits an explicit subsequent edit against that history', () => {
    const deleted = applyDocumentChange(snapshot, remove('a', 'a/1'), { revisionId: 'deleted', blockRevisions: {} })
    const operation = {
      kind: 'block.insert' as const, operationId: 'restore-a', target: { leftBlockId: null, rightBlockId: 'b' },
      payload: { candidateBlockId: 'a', content: 'A', restoreFrom: { revisionId: snapshot.revisionId, blockId: 'a' } },
    }
    const batch = { ...replace('a', 'a/1', 'A2'), baseRevisionId: 'deleted', operations: [operation, ...replace('a', 'a/1', 'A2').operations] }
    const context = { historicalRevisions: [snapshot] }
    expect(applyDocumentChange(deleted, batch, { revisionId: 'restored', blockRevisions: { a: 'a/2' } }, context).blocks[0])
      .toEqual({ blockId: 'a', blockRevision: 'a/2', markdown: 'A2' })
    expect(validateDocumentChange(deleted, batch)).toMatchObject({ code: 'invalid-operation' })
    expect(validateDocumentChange(deleted, { ...batch, operations: [{ ...operation, payload: { ...operation.payload, content: 'Forged' } }] }, context))
      .toMatchObject({ code: 'invalid-operation' })
    expect(validateDocumentChange(deleted, { ...batch, operations: [{ ...operation, payload: { ...operation.payload, candidateBlockId: 'other' } }] }, context))
      .toMatchObject({ code: 'invalid-operation' })
  })

  it('rebases move across neighbour text edits but rejects a changed destination gap or target revision', () => {
    const initial: DocumentRevision = { ...snapshot, blocks: [...snapshot.blocks,
      { blockId: 'c', blockRevision: 'c/1', markdown: 'C' },
      { blockId: 'd', blockRevision: 'd/1', markdown: 'D' },
    ] }
    const batch: OperationBatch = { ...replace('a', 'a/1', 'A'), operations: [{
      kind: 'block.move', operationId: 'move', target: { blockId: 'a', expectedBlockRevision: 'a/1' },
      payload: {
        source: { leftBlockId: null, rightBlockId: 'b' },
        destination: { leftBlockId: 'c', rightBlockId: 'd' },
      },
    }] }
    const neighbourEdit = applyDocumentChange(initial, replace('c', 'c/1', 'New C'), {
      revisionId: 'neighbour-edited', blockRevisions: { c: 'c/2' },
    })
    expect(validateDocumentChange(neighbourEdit, batch)).toBeNull()
    const changedGap = applyDocumentChange(initial, insert('x', 'c', 'd'), {
      revisionId: 'gap-edited', blockRevisions: { x: 'x/1' },
    })
    expect(validateDocumentChange(changedGap, batch)).toMatchObject({ code: 'stale-base', message: '移动目标位置已发生变化。' })
    const targetEdit = applyDocumentChange(initial, replace('a', 'a/1', 'New A'), {
      revisionId: 'target-edited', blockRevisions: { a: 'a/2' },
    })
    expect(validateDocumentChange(targetEdit, batch)).toMatchObject({ code: 'stale-base', blockId: 'a' })
  })
})
