import { describe, expect, it } from 'vitest'
import { canonicalOperationBatch, OperationProtocolError, parseOperationBatch } from './operation'

const batch = {
  requestId: 'request-1',
  baseRevisionId: 'revision-1',
  operations: [{
    kind: 'block.replace' as const,
    operationId: 'operation-1',
    blockId: 'block-1',
    expectedBlockRevision: 'block-revision-1',
    markdown: 'New text.',
  }],
}

describe('CDR operation protocol', () => {
  it('strictly parses and canonically signs the shared wire shape', () => {
    expect(parseOperationBatch(batch)).toEqual(batch)
    expect(canonicalOperationBatch(batch)).toBe(JSON.stringify(batch))
  })

  it('rejects unknown fields and duplicate operation targets', () => {
    expect(() => parseOperationBatch({ ...batch, actorId: 'self-reported' }))
      .toThrow(OperationProtocolError)
    expect(() => parseOperationBatch({
      ...batch,
      operations: [batch.operations[0], { ...batch.operations[0], operationId: 'operation-2' }],
    })).toThrow('contains duplicate block-1')
  })
})
