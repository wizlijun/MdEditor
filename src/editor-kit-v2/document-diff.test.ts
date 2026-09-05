import { describe, expect, it } from 'vitest'
import { applyDocumentChange, validateDocumentChange, type DocumentRevision } from '../lib/cdr/core'
import { operationContentWrites, type OperationBatch } from '../lib/cdr/operation'
import { mergeAcknowledgedDraft, operationsForDraft, sameDocument } from './document-diff'
import type { LayoutBlock } from './block-layout'

const block = (blockId: string, markdown = blockId.toUpperCase()) => ({
  blockId, markdown, blockRevision: `hash:${markdown}`,
})
const base: DocumentRevision = {
  documentId: 'document', revisionId: 'base', blocks: [block('a'), block('b'), block('c')],
}
const history: DocumentRevision[] = [{
  documentId: base.documentId, revisionId: 'before-delete', blocks: [...base.blocks, block('retired')],
}, base]

function batchFor(draft: readonly LayoutBlock[], source = base, known = history): OperationBatch {
  let id = 0
  return {
    requestId: 'request', documentId: source.documentId, baseRevisionId: source.revisionId,
    operations: operationsForDraft(source, draft, known, () => `operation-${++id}`),
  }
}

function apply(batch: OperationBatch, source = base, known = history): DocumentRevision {
  const blockRevisions = Object.fromEntries(batch.operations.flatMap(operationContentWrites)
    .map((write) => [write.blockId, `hash:${write.content}`]))
  return applyDocumentChange(source, batch, { revisionId: 'next', blockRevisions }, { historicalRevisions: known })
}

describe('governed draft normalization', () => {
  it('conforms to Core for every nonempty order/subset of existing, inserted and restored identities', () => {
    const visit = (prefix: string[], remaining: string[]): void => {
      if (prefix.length) {
        const draft = prefix.map((id) => block(id, `Edited ${id}`))
        const batch = batchFor(draft)
        expect(validateDocumentChange(base, batch, { historicalRevisions: history })).toBeNull()
        expect(apply(batch).blocks).toEqual(draft)
      }
      for (const id of remaining) visit([...prefix, id], remaining.filter((item) => item !== id))
    }
    visit([], ['a', 'b', 'c', 'new', 'retired'])
    expect(base.blocks.map((item) => item.markdown)).toEqual(['A', 'B', 'C'])
  })

  it('normalizes full selection clearing into a single atomic revision with one empty stable block', () => {
    const draft = [block('a', '')]
    const batch = batchFor(draft)
    expect(batch.operations.map((op) => op.kind)).toEqual(['block.delete', 'block.delete', 'block.replace'])
    expect(apply(batch).blocks).toEqual(draft)
  })

  it('uses exact historical content for restoration before a later draft replacement', () => {
    const draft = [...base.blocks, block('retired', 'Restored and edited')]
    const batch = batchFor(draft)
    expect(batch.operations).toMatchObject([
      { kind: 'block.insert', payload: { candidateBlockId: 'retired', content: 'RETIRED',
        restoreFrom: { revisionId: 'before-delete', blockId: 'retired' } } },
      { kind: 'block.replace', target: { blockId: 'retired', expectedBlockRevision: 'hash:RETIRED' },
        payload: { content: 'Restored and edited' } },
    ])
    expect(apply(batch).blocks).toEqual(draft)
    // An editor recovery/cache entry can suggest provenance, but never proves it.
    expect(validateDocumentChange(base, batch, { historicalRevisions: [base] }))
      .toMatchObject({ code: 'invalid-operation' })
  })

  it('rebases disjoint remote content but does not overwrite a concurrently edited selection target', () => {
    const draft = [block('a', 'Merged A and B'), base.blocks[2]]
    const batch = batchFor(draft)
    const unrelated = { ...base, revisionId: 'remote', blocks: [base.blocks[0], base.blocks[1], block('c', 'Remote C')] }
    expect(apply(batch, unrelated).blocks).toEqual([draft[0], unrelated.blocks[2]])
    const editedTarget = { ...base, revisionId: 'remote-target', blocks: [base.blocks[0], block('b', 'Remote B'), base.blocks[2]] }
    expect(validateDocumentChange(editedTarget, batch, { historicalRevisions: history }))
      .toMatchObject({ code: 'stale-base', blockId: 'b' })
  })

  it('merges later local input with disjoint authoritative content and identity changes', () => {
    const draft = [block('a', 'Local A'), base.blocks[1], base.blocks[2]]
    const remote = [base.blocks[0], block('b', 'Remote B'), base.blocks[2], block('new')]
    expect(mergeAcknowledgedDraft(base.blocks, draft, remote))
      .toEqual([draft[0], remote[1], base.blocks[2], remote[3]])
    expect(mergeAcknowledgedDraft(base.blocks, [base.blocks[0]], [base.blocks[0], base.blocks[1], base.blocks[2]]))
      .toEqual([base.blocks[0]])
    expect(sameDocument(base.blocks, [...base.blocks])).toBe(true)
  })

  it('retains conflict when one side edits a block the other side deleted', () => {
    expect(() => mergeAcknowledgedDraft(base.blocks, [block('a', 'Local A'), base.blocks[1], base.blocks[2]], base.blocks.slice(1)))
      .toThrow('deleted block was edited')
    expect(() => mergeAcknowledgedDraft(base.blocks, base.blocks.slice(1), [block('a', 'Remote A'), base.blocks[1], base.blocks[2]]))
      .toThrow('deleted block was edited')
    expect(() => mergeAcknowledgedDraft(base.blocks, [base.blocks[1], base.blocks[0], base.blocks[2]], [base.blocks[0], base.blocks[2], base.blocks[1]]))
      .toThrow('structure changed on both sides')
  })

  it('does not silently pick local text when both sides independently restore the same retired ID', () => {
    const local = [...base.blocks, block('retired', 'Local restored text')]
    const remote = [...base.blocks, block('retired', 'Remote restored text')]
    expect(() => mergeAcknowledgedDraft(base.blocks, local, remote)).toThrow()
    expect(mergeAcknowledgedDraft(base.blocks, local, local)).toEqual(local)
  })
})
