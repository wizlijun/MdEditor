import { describe, expect, it } from 'vitest'
import { canonicalOperationBatch, OperationProtocolError, parseOperationBatch } from './operation'

const batch = {
  requestId: 'request-1',
  documentId: 'document-1',
  baseRevisionId: 'revision-1',
  operations: [{
    kind: 'block.replace' as const,
    operationId: 'operation-1',
    target: { blockId: 'block-1', expectedBlockRevision: 'block-revision-1' },
    payload: { content: 'New text.' },
  }],
}

describe('CDR operation protocol', () => {
  it('strictly parses and canonically signs the target + payload wire shape', () => {
    expect(parseOperationBatch(batch)).toEqual(batch)
    expect(canonicalOperationBatch(batch)).toBe(JSON.stringify(batch))
  })

  it('strictly parses isolated insert and delete operations', () => {
    const inserted = {
      ...batch,
      operations: [{
        kind: 'block.insert' as const,
        operationId: 'insert-1',
        target: { leftBlockId: 'block-1', rightBlockId: null },
        payload: { candidateBlockId: 'block-2', content: 'Second block.' },
      }],
    }
    const deleted = {
      ...batch,
      operations: [{
        kind: 'block.delete' as const,
        operationId: 'delete-1',
        target: { blockId: 'block-1', expectedBlockRevision: 'block-revision-1' },
        payload: {},
      }],
    }
    expect(parseOperationBatch(inserted)).toEqual(inserted)
    expect(parseOperationBatch(deleted)).toEqual(deleted)
    expect(() => parseOperationBatch({ ...batch, operations: [...inserted.operations, ...deleted.operations] }))
      .toThrow('must isolate structural operations')
  })

  it('rejects old flat fields, unknown fields, invalid gaps, and duplicate targets', () => {
    expect(() => parseOperationBatch({ ...batch, actorId: 'self-reported' }))
      .toThrow(OperationProtocolError)
    expect(() => parseOperationBatch({
      ...batch,
      operations: [batch.operations[0], { ...batch.operations[0], operationId: 'operation-2' }],
    })).toThrow('contains duplicate block-1')
    expect(() => parseOperationBatch({
      ...batch,
      operations: [{
        kind: 'block.insert',
        operationId: 'insert-1',
        target: { leftBlockId: null, rightBlockId: null },
        payload: { candidateBlockId: 'block-2', content: 'Second block.' },
      }],
    })).toThrow('must identify at least one neighbouring block')
    expect(() => parseOperationBatch({
      requestId: 'legacy',
      documentId: 'document-1',
      baseRevisionId: 'revision-1',
      operations: [{ ...batch.operations[0], blockId: 'legacy-flat' }],
    })).toThrow('blockId is not allowed')
  })
})
