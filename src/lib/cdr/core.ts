import {
  hasMixedStructuralOperations,
  operationContentWrites,
  type BlockGapTarget,
  type OperationBatch,
} from './operation'

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

export interface DocumentValidationContext {
  /** Every block identity ever allocated in this document, including deleted blocks. */
  knownBlockIds?: ReadonlySet<string>
}

export interface PreparedDocumentChange {
  revisionId: string
  /** Resulting revisions for replace/insert outputs; delete has no entry. */
  blockRevisions: Readonly<Record<string, string>>
}

function gapIndex(snapshot: DocumentRevision, target: BlockGapTarget): number | null {
  const ids = snapshot.blocks.map((block) => block.blockId)
  if (target.leftBlockId === null) return ids[0] === target.rightBlockId ? 0 : null
  if (target.rightBlockId === null) return ids.at(-1) === target.leftBlockId ? ids.length : null
  const left = ids.indexOf(target.leftBlockId)
  return left >= 0 && ids[left + 1] === target.rightBlockId ? left + 1 : null
}

/** Validate against one immutable head; older bases may rebase only while every referenced identity is unchanged. */
export function validateDocumentChange(
  snapshot: DocumentRevision,
  batch: OperationBatch,
  context: DocumentValidationContext = {},
): CoreConflict | null {
  if (batch.documentId !== snapshot.documentId) {
    return { code: 'invalid-operation', message: '操作批属于另一份文档。' }
  }
  if (!batch.operations.length) return { code: 'invalid-operation', message: '操作批不能为空。' }
  if (hasMixedStructuralOperations(batch.operations)) {
    return { code: 'invalid-operation', message: '当前协议要求结构操作独立成批。' }
  }

  const knownBlockIds = context.knownBlockIds ?? new Set(snapshot.blocks.map((block) => block.blockId))
  for (const operation of batch.operations) {
    const write = operationContentWrites(operation)[0]
    if (write && !write.content.trim()) {
      return { code: 'invalid-operation', message: '块内容不能为空。', blockId: write.blockId }
    }

    if (operation.kind === 'block.insert') {
      const candidate = operation.payload.candidateBlockId
      if (knownBlockIds.has(candidate)) {
        return { code: 'invalid-operation', message: '新块 ID 已在文档生命周期中使用。', blockId: candidate }
      }
      if (gapIndex(snapshot, operation.target) === null) {
        return { code: 'stale-base', message: '插入位置已发生变化。', blockId: candidate }
      }
      continue
    }

    const block = snapshot.blocks.find((item) => item.blockId === operation.target.blockId)
    if (!block) {
      return { code: 'stale-base', message: '目标块已不存在。', blockId: operation.target.blockId }
    }
    if (block.blockRevision !== operation.target.expectedBlockRevision) {
      return { code: 'stale-base', message: '目标块已被修改，本次操作未覆盖新版本。', blockId: operation.target.blockId }
    }
    if (operation.kind === 'block.delete' && snapshot.blocks.length === 1) {
      return { code: 'invalid-operation', message: '文档必须至少保留一个块。', blockId: operation.target.blockId }
    }
  }
  return null
}

/** Apply a pre-validated and pre-hashed batch without performing I/O or allocating IDs. */
export function applyDocumentChange(
  snapshot: DocumentRevision,
  batch: OperationBatch,
  prepared: PreparedDocumentChange,
  context: DocumentValidationContext = {},
): DocumentRevision {
  const conflict = validateDocumentChange(snapshot, batch, context)
  if (conflict) throw new Error(`CDR_CORE_CONFLICT: ${conflict.code}`)
  if (!prepared.revisionId) throw new Error('CDR_CORE_PREPARED_REVISION_REQUIRED')
  const expectedKeys = batch.operations
    .flatMap((operation) => operationContentWrites(operation).map((write) => write.blockId))
    .sort()
  const actualKeys = Object.keys(prepared.blockRevisions).sort()
  if (JSON.stringify(expectedKeys) !== JSON.stringify(actualKeys)
    || actualKeys.some((blockId) => !prepared.blockRevisions[blockId])) {
    throw new Error('CDR_CORE_PREPARED_BLOCK_REVISIONS')
  }

  const replacements = new Map(batch.operations.flatMap((operation) => (
    operation.kind === 'block.replace' ? [[operation.target.blockId, operation] as const] : []
  )))
  const deletion = batch.operations.find((operation) => operation.kind === 'block.delete')
  const insertion = batch.operations.find((operation) => operation.kind === 'block.insert')
  let blocks = snapshot.blocks
    .filter((block) => deletion?.target.blockId !== block.blockId)
    .map((block) => {
      const operation = replacements.get(block.blockId)
      return operation
        ? {
            ...block,
            markdown: operation.payload.content,
            blockRevision: prepared.blockRevisions[block.blockId],
          }
        : { ...block }
    })

  if (insertion) {
    const index = gapIndex(snapshot, insertion.target)
    if (index === null) throw new Error('CDR_CORE_CONFLICT: stale-base')
    const inserted: DocumentBlock = {
      blockId: insertion.payload.candidateBlockId,
      blockRevision: prepared.blockRevisions[insertion.payload.candidateBlockId],
      markdown: insertion.payload.content,
    }
    blocks = [...blocks.slice(0, index), inserted, ...blocks.slice(index)]
  }

  return {
    ...snapshot,
    revisionId: prepared.revisionId,
    blocks,
  }
}
