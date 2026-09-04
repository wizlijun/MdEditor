import type { OperationBatch } from './operation'

export interface DocumentBlock {
  blockId: string
  blockRevision: string
  markdown: string
}

export interface DocumentRevision {
  documentId: string
  revisionId: string
  blocks: readonly DocumentBlock[]
}

export interface CoreConflict {
  code: 'stale-base' | 'invalid-operation'
  message: string
  blockId?: string
}

export interface PreparedDocumentChange {
  revisionId: string
  blockRevisions: Readonly<Record<string, string>>
}

/** Validate against the current head; an older document base may safely rebase when every target version still matches. */
export function validateDocumentChange(
  snapshot: DocumentRevision,
  batch: OperationBatch,
): CoreConflict | null {
  if (!batch.operations.length) return { code: 'invalid-operation', message: '操作批不能为空。' }
  for (const operation of batch.operations) {
    if (!operation.markdown.trim()) {
      return { code: 'invalid-operation', message: '块内容不能为空。', blockId: operation.blockId }
    }
    const block = snapshot.blocks.find((item) => item.blockId === operation.blockId)
    if (!block) return { code: 'invalid-operation', message: '目标块不存在。', blockId: operation.blockId }
    if (block.blockRevision !== operation.expectedBlockRevision) {
      return { code: 'stale-base', message: '目标块已被修改，本次操作未覆盖新版本。', blockId: operation.blockId }
    }
  }
  return null
}

/** Apply a pre-validated and pre-hashed batch without performing I/O or allocating IDs. */
export function applyDocumentChange(
  snapshot: DocumentRevision,
  batch: OperationBatch,
  prepared: PreparedDocumentChange,
): DocumentRevision {
  const conflict = validateDocumentChange(snapshot, batch)
  if (conflict) throw new Error(`CDR_CORE_CONFLICT: ${conflict.code}`)
  if (!prepared.revisionId) throw new Error('CDR_CORE_PREPARED_REVISION_REQUIRED')
  const expectedKeys = batch.operations.map((operation) => operation.blockId).sort()
  const actualKeys = Object.keys(prepared.blockRevisions).sort()
  if (JSON.stringify(expectedKeys) !== JSON.stringify(actualKeys)
    || actualKeys.some((blockId) => !prepared.blockRevisions[blockId])) {
    throw new Error('CDR_CORE_PREPARED_BLOCK_REVISIONS')
  }

  return {
    ...snapshot,
    revisionId: prepared.revisionId,
    blocks: snapshot.blocks.map((block) => {
      const operation = batch.operations.find((item) => item.blockId === block.blockId)
      return operation
        ? { ...block, markdown: operation.markdown, blockRevision: prepared.blockRevisions[block.blockId] }
        : { ...block }
    }),
  }
}
