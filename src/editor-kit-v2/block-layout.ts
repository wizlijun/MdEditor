import { parseMarkdown, serializeMarkdown } from '@moraya/core'
import type { Node as PmNode, Schema } from 'prosemirror-model'
import type { Transaction } from 'prosemirror-state'

export interface LayoutBlock {
  blockId: string
  markdown: string
}

export interface BlockSpan {
  blockId: string
  startIndex: number
  endIndex: number
}

export interface BlockLayout {
  spans: readonly BlockSpan[]
}

export interface BlockReplacement {
  blockId: string
  markdown: string
}

export function assertBlockIdentity(blocks: readonly LayoutBlock[]): void {
  const ids = blocks.map((block) => block.blockId)
  if (ids.length === 0 || ids.some((id) => id.length === 0) || new Set(ids).size !== ids.length) {
    throw new Error('EDITOR_KIT_V2_BLOCK_IDENTITY: snapshots need non-empty unique block IDs')
  }
}

function parseBlock(block: LayoutBlock, schema: Schema): PmNode {
  const parsed = parseMarkdown(block.markdown, schema)
  if (parsed.childCount === 0) {
    throw new Error(`EDITOR_KIT_V2_BLOCK_SHAPE: ${block.blockId} has no top-level editor node`)
  }
  return parsed
}

function layoutFrom(blocks: readonly LayoutBlock[], parsed: readonly PmNode[]): BlockLayout {
  let cursor = 0
  return {
    spans: blocks.map((block, index) => {
      const startIndex = cursor
      cursor += parsed[index].childCount
      return { blockId: block.blockId, startIndex, endIndex: cursor }
    }),
  }
}

export function materializeBlocks(
  blocks: readonly LayoutBlock[],
  schema: Schema,
): { doc: PmNode; layout: BlockLayout } {
  assertBlockIdentity(blocks)
  const parsed = blocks.map((block) => parseBlock(block, schema))
  const nodes: PmNode[] = []
  for (const block of parsed) block.forEach((node) => nodes.push(node))
  return { doc: schema.topNodeType.create(null, nodes), layout: layoutFrom(blocks, parsed) }
}

export function spanContaining(layout: BlockLayout, nodeIndex: number): BlockSpan | null {
  return layout.spans.find((span) => nodeIndex >= span.startIndex && nodeIndex < span.endIndex) ?? null
}

export function serializeSpan(doc: PmNode, span: BlockSpan): string {
  const nodes: PmNode[] = []
  for (let index = span.startIndex; index < span.endIndex; index += 1) nodes.push(doc.child(index))
  return serializeMarkdown(doc.type.schema.topNodeType.create(null, nodes)).trimEnd()
}

function positionAt(doc: PmNode, index: number): number {
  let position = 0
  for (let current = 0; current < index; current += 1) position += doc.child(current).nodeSize
  return position
}

export function positionAtBlockStart(doc: PmNode, layout: BlockLayout, blockId: string): number {
  const span = layout.spans.find((item) => item.blockId === blockId)
  if (!span) throw new Error(`EDITOR_KIT_V2_UNKNOWN_BLOCK: ${blockId}`)
  return positionAt(doc, span.startIndex)
}

function rangeMatches(doc: PmNode, span: BlockSpan, expected: PmNode): boolean {
  if (span.endIndex - span.startIndex !== expected.childCount) return false
  for (let offset = 0; offset < expected.childCount; offset += 1) {
    if (!doc.child(span.startIndex + offset).eq(expected.child(offset))) return false
  }
  return true
}

export function replaceBlockSpans(
  transaction: Transaction,
  layout: BlockLayout,
  replacements: readonly BlockReplacement[],
): { transaction: Transaction; layout: BlockLayout } {
  const seen = new Set<string>()
  const planned = replacements.map((replacement) => {
    if (seen.has(replacement.blockId)) throw new Error(`EDITOR_KIT_V2_DUPLICATE_BLOCK: ${replacement.blockId}`)
    seen.add(replacement.blockId)
    const spanIndex = layout.spans.findIndex((span) => span.blockId === replacement.blockId)
    if (spanIndex < 0) throw new Error(`EDITOR_KIT_V2_UNKNOWN_BLOCK: ${replacement.blockId}`)
    return {
      spanIndex,
      span: layout.spans[spanIndex],
      parsed: parseBlock(replacement, transaction.doc.type.schema),
    }
  }).sort((a, b) => b.span.startIndex - a.span.startIndex)

  const counts = layout.spans.map((span) => span.endIndex - span.startIndex)
  let next = transaction
  for (const item of planned) {
    if (!rangeMatches(next.doc, item.span, item.parsed)) {
      next = next.replaceWith(
        positionAt(next.doc, item.span.startIndex),
        positionAt(next.doc, item.span.endIndex),
        item.parsed.content,
      )
    }
    counts[item.spanIndex] = item.parsed.childCount
  }

  let cursor = 0
  const spans = layout.spans.map((span, index) => {
    const startIndex = cursor
    cursor += counts[index]
    return { blockId: span.blockId, startIndex, endIndex: cursor }
  })
  return { transaction: next, layout: { spans } }
}

export function insertBlockSpan(
  transaction: Transaction,
  layout: BlockLayout,
  blockIndex: number,
  block: LayoutBlock,
): { transaction: Transaction; layout: BlockLayout } {
  if (blockIndex < 0 || blockIndex > layout.spans.length) {
    throw new Error('EDITOR_KIT_V2_INSERT_INDEX')
  }
  if (layout.spans.some((span) => span.blockId === block.blockId)) {
    throw new Error(`EDITOR_KIT_V2_DUPLICATE_BLOCK: ${block.blockId}`)
  }
  const parsed = parseBlock(block, transaction.doc.type.schema)
  const nodeIndex = blockIndex === layout.spans.length
    ? transaction.doc.childCount
    : layout.spans[blockIndex].startIndex
  const next = transaction.replaceWith(positionAt(transaction.doc, nodeIndex), positionAt(transaction.doc, nodeIndex), parsed.content)
  const counts = layout.spans.map((span) => span.endIndex - span.startIndex)
  counts.splice(blockIndex, 0, parsed.childCount)
  const blockIds = layout.spans.map((span) => span.blockId)
  blockIds.splice(blockIndex, 0, block.blockId)
  let cursor = 0
  return {
    transaction: next,
    layout: {
      spans: blockIds.map((blockId, index) => {
        const startIndex = cursor
        cursor += counts[index]
        return { blockId, startIndex, endIndex: cursor }
      }),
    },
  }
}

export function deleteBlockSpan(
  transaction: Transaction,
  layout: BlockLayout,
  blockId: string,
): { transaction: Transaction; layout: BlockLayout } {
  const spanIndex = layout.spans.findIndex((span) => span.blockId === blockId)
  if (spanIndex < 0) throw new Error(`EDITOR_KIT_V2_UNKNOWN_BLOCK: ${blockId}`)
  if (layout.spans.length === 1) throw new Error('EDITOR_KIT_V2_LAST_BLOCK')
  const span = layout.spans[spanIndex]
  const next = transaction.delete(
    positionAt(transaction.doc, span.startIndex),
    positionAt(transaction.doc, span.endIndex),
  )
  const counts = layout.spans.map((item) => item.endIndex - item.startIndex)
  counts.splice(spanIndex, 1)
  const blockIds = layout.spans.map((item) => item.blockId)
  blockIds.splice(spanIndex, 1)
  let cursor = 0
  return {
    transaction: next,
    layout: {
      spans: blockIds.map((remainingBlockId, index) => {
        const startIndex = cursor
        cursor += counts[index]
        return { blockId: remainingBlockId, startIndex, endIndex: cursor }
      }),
    },
  }
}

export function sameBlockOrder(layout: BlockLayout, blocks: readonly LayoutBlock[]): boolean {
  return layout.spans.length === blocks.length
    && layout.spans.every((span, index) => span.blockId === blocks[index]?.blockId)
}
