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

function replace(blockId: string, expectedBlockRevision: string, markdown: string): OperationBatch {
  return {
    requestId: `request-${blockId}`,
    baseRevisionId: 'revision-1',
    operations: [{ kind: 'block.replace', operationId: `operation-${blockId}`, blockId, expectedBlockRevision, markdown }],
  }
}

describe('DocumentCore', () => {
  it('safely rebases an unchanged target and preserves block identity', () => {
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

  it('rejects a stale target and invalid prepared results', () => {
    expect(validateDocumentChange(snapshot, replace('b', 'b/0', 'B2'))).toMatchObject({
      code: 'stale-base', blockId: 'b',
    })
    expect(() => applyDocumentChange(snapshot, replace('b', 'b/1', 'B2'), {
      revisionId: 'revision-3', blockRevisions: {},
    })).toThrow('CDR_CORE_PREPARED_BLOCK_REVISIONS')
  })
})
