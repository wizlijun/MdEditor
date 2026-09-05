import {
  parseOperationBatch,
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
  /** Committed history only; restore can reintroduce the exact content of one historical block. */
  historicalRevisions?: readonly DocumentRevision[]
}

export interface PreparedDocumentChange {
  revisionId: string
  /** One final revision per replace/insert output ID; delete and move have no entry. */
  blockRevisions: Readonly<Record<string, string>>
}

function gapIndex(snapshot: DocumentRevision, target: BlockGapTarget): number | null {
  const ids = snapshot.blocks.map((block) => block.blockId)
  if (target.leftBlockId === null && target.rightBlockId === null) return ids.length === 0 ? 0 : null
  if (target.leftBlockId === null) return ids[0] === target.rightBlockId ? 0 : null
  if (target.rightBlockId === null) return ids.at(-1) === target.leftBlockId ? ids.length : null
  const left = ids.indexOf(target.leftBlockId)
  return left >= 0 && ids[left + 1] === target.rightBlockId ? left + 1 : null
}

/** Build one candidate in order; no mutation escapes when any operation fails. */
function planDocumentChange(
  snapshot: DocumentRevision,
  batch: OperationBatch,
  context: DocumentValidationContext = {},
): { blocks: DocumentBlock[] } | CoreConflict {
  if (batch.documentId !== snapshot.documentId) {
    return { code: 'invalid-operation', message: '操作批属于另一份文档。' }
  }
  if (!batch.operations.length) return { code: 'invalid-operation', message: '操作批不能为空。' }
  try {
    parseOperationBatch(batch)
  } catch {
    return { code: 'invalid-operation', message: '操作批格式或目标重复。' }
  }

  const historical = context.historicalRevisions ?? []
  const knownBlockIds = new Set([
    ...(context.knownBlockIds ?? []),
    ...snapshot.blocks.map((block) => block.blockId),
    ...historical.flatMap((revision) => revision.blocks.map((block) => block.blockId)),
  ])
  const originalBlocks = new Map(snapshot.blocks.map((block) => [block.blockId, block]))
  const restoredBlocks = new Map<string, DocumentBlock>()
  const blocks = snapshot.blocks.map((block) => ({ ...block }))
  for (const operation of batch.operations) {
    if (operation.kind === 'block.insert') {
      const candidate = operation.payload.candidateBlockId
      const restore = operation.payload.restoreFrom
      const source = restore && historical.find((revision) => (
        revision.documentId === snapshot.documentId && revision.revisionId === restore.revisionId
      ))?.blocks.find((block) => block.blockId === restore.blockId)
      if (restore && (!source || source.blockId !== candidate || source.markdown !== operation.payload.content)) {
        return { code: 'invalid-operation', message: '恢复必须引用同一块的精确历史正文。', blockId: candidate }
      }
      if (blocks.some((block) => block.blockId === candidate) || (knownBlockIds.has(candidate) && !restore)) {
        return { code: 'invalid-operation', message: '新块 ID 已在文档生命周期中使用。', blockId: candidate }
      }
      // A delete followed by restore in one batch is not an undo; normalize it to replace/move.
      if (originalBlocks.has(candidate)) {
        return { code: 'invalid-operation', message: '同一操作批不能删除后复用已有块 ID。', blockId: candidate }
      }
      const index = gapIndex({ ...snapshot, blocks }, operation.target)
      if (index === null) {
        return { code: 'stale-base', message: '插入位置已发生变化。', blockId: candidate }
      }
      blocks.splice(index, 0, {
        blockId: candidate,
        blockRevision: source?.blockRevision ?? '',
        markdown: operation.payload.content,
      })
      if (source) restoredBlocks.set(candidate, source)
      knownBlockIds.add(candidate)
      continue
    }

    const block = originalBlocks.get(operation.target.blockId) ?? restoredBlocks.get(operation.target.blockId)
    const index = blocks.findIndex((item) => item.blockId === operation.target.blockId)
    if (!block || index < 0) {
      return { code: 'stale-base', message: '目标块已不存在。', blockId: operation.target.blockId }
    }
    if (block.blockRevision !== operation.target.expectedBlockRevision) {
      return { code: 'stale-base', message: '目标块已被修改，本次操作未覆盖新版本。', blockId: operation.target.blockId }
    }
    if (operation.kind === 'block.replace') {
      blocks[index] = { ...blocks[index], markdown: operation.payload.content }
    } else if (operation.kind === 'block.delete') {
      if (restoredBlocks.has(block.blockId)) {
        return { code: 'invalid-operation', message: '同一操作批不能恢复后删除该块。', blockId: block.blockId }
      }
      blocks.splice(index, 1)
    } else {
      const { source, destination } = operation.payload
      if (source.leftBlockId !== (blocks[index - 1]?.blockId ?? null)
        || source.rightBlockId !== (blocks[index + 1]?.blockId ?? null)) {
        return { code: 'stale-base', message: '移动来源位置已发生变化。', blockId: block.blockId }
      }
      if (destination.leftBlockId === block.blockId || destination.rightBlockId === block.blockId) {
        return { code: 'invalid-operation', message: '移动目标不能引用自身。', blockId: block.blockId }
      }
      const [moving] = blocks.splice(index, 1)
      const destinationIndex = gapIndex({ ...snapshot, blocks }, destination)
      if (destinationIndex === null) {
        return { code: 'stale-base', message: '移动目标位置已发生变化。', blockId: block.blockId }
      }
      blocks.splice(destinationIndex, 0, moving)
    }
  }
  return blocks.length ? { blocks } : { code: 'invalid-operation', message: '文档必须至少保留一个块。' }
}

/** Existing content versions refer to the immutable head; gaps refer to each preceding operation's result. */
export function validateDocumentChange(
  snapshot: DocumentRevision,
  batch: OperationBatch,
  context: DocumentValidationContext = {},
): CoreConflict | null {
  const result = planDocumentChange(snapshot, batch, context)
  return 'code' in result ? result : null
}

/** Apply a pre-validated and pre-hashed batch without performing I/O or allocating IDs. */
export function applyDocumentChange(
  snapshot: DocumentRevision,
  batch: OperationBatch,
  prepared: PreparedDocumentChange,
  context: DocumentValidationContext = {},
): DocumentRevision {
  const result = planDocumentChange(snapshot, batch, context)
  if ('code' in result) throw new Error(`CDR_CORE_CONFLICT: ${result.code}`)
  if (!prepared.revisionId) throw new Error('CDR_CORE_PREPARED_REVISION_REQUIRED')
  const expectedKeys = [...new Set(batch.operations
    .flatMap((operation) => operationContentWrites(operation).map((write) => write.blockId)))].sort()
  const actualKeys = Object.keys(prepared.blockRevisions).sort()
  if (JSON.stringify(expectedKeys) !== JSON.stringify(actualKeys)
    || actualKeys.some((blockId) => !prepared.blockRevisions[blockId])) {
    throw new Error('CDR_CORE_PREPARED_BLOCK_REVISIONS')
  }

  return {
    ...snapshot,
    revisionId: prepared.revisionId,
    blocks: result.blocks.map((block) => ({
      ...block,
      blockRevision: prepared.blockRevisions[block.blockId] ?? block.blockRevision,
    })),
  }
}
