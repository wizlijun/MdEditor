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

export function sameBlockOrder(layout: BlockLayout, blocks: readonly LayoutBlock[]): boolean {
  return layout.spans.length === blocks.length
    && layout.spans.every((span, index) => span.blockId === blocks[index]?.blockId)
}
