import type { DocumentRevision } from '../lib/cdr/core'
import type { Operation } from '../lib/cdr/operation'
import type { LayoutBlock } from './block-layout'

export function sameDocument(a: readonly LayoutBlock[], b: readonly LayoutBlock[]): boolean {
  return a.length === b.length && a.every((block, i) => (
    block.blockId === b[i].blockId && block.markdown === b[i].markdown
  ))
}

function sameOrder(a: readonly LayoutBlock[], b: readonly LayoutBlock[]): boolean {
  return a.length === b.length && a.every((block, i) => block.blockId === b[i].blockId)
}

/** Three-way merge only disjoint edits; content conflicts never become LWW. */
export function mergeAcknowledgedDraft(
  base: readonly LayoutBlock[],
  draft: readonly LayoutBlock[],
  authoritative: readonly LayoutBlock[],
): LayoutBlock[] {
  if (!sameOrder(base, draft) && !sameOrder(base, authoritative) && !sameOrder(draft, authoritative)) {
    throw new Error('Document structure changed on both sides; compare the retained draft.')
  }
  const local = new Map(draft.map((block) => [block.blockId, block]))
  const remote = new Map(authoritative.map((block) => [block.blockId, block]))
  const original = new Map(base.map((block) => [block.blockId, block]))
  for (const block of base) {
    const left = local.get(block.blockId)
    const right = remote.get(block.blockId)
    if ((!left && right && right.markdown !== block.markdown)
      || (!right && left && left.markdown !== block.markdown)) {
      throw new Error('A deleted block was edited on the other side; compare the retained draft.')
    }
  }
  const order = sameOrder(base, draft) ? authoritative : draft
  return order.map((block) => {
    const before = original.get(block.blockId)
    const left = local.get(block.blockId)
    const right = remote.get(block.blockId)
    if (!before) {
      if (left && right && left.markdown !== right.markdown) {
        throw new Error('The same restored identity has different content on both sides; compare the retained draft.')
      }
      return { ...block }
    }
    if (left && right && left.markdown !== before.markdown && right.markdown !== before.markdown
      && left.markdown !== right.markdown) {
      throw new Error('A block changed on both sides; compare the retained draft.')
    }
    return { ...(left && left.markdown !== before.markdown ? left : right ?? block) }
  })
}

/** Normalize an editor draft into one ordered, atomic domain-neutral batch. */
export function operationsForDraft(
  base: DocumentRevision,
  draft: readonly LayoutBlock[],
  history: readonly DocumentRevision[],
  operationId: () => string,
): Operation[] {
  const operations: Operation[] = []
  const wanted = new Set(draft.map((block) => block.blockId))
  const working = base.blocks.map((block) => block.blockId)
  const existing = new Map(base.blocks.map((block) => [block.blockId, block]))
  for (const block of base.blocks) {
    if (wanted.has(block.blockId)) continue
    operations.push({ kind: 'block.delete', operationId: operationId(),
      target: { blockId: block.blockId, expectedBlockRevision: block.blockRevision }, payload: {} })
    working.splice(working.indexOf(block.blockId), 1)
  }
  for (let index = 0; index < draft.length; index += 1) {
    const block = draft[index]
    let original = existing.get(block.blockId)
    const currentIndex = working.indexOf(block.blockId)
    if (currentIndex < 0) {
      const source = [...history].reverse().find((revision) => revision.blocks.some((item) => item.blockId === block.blockId))
      original = source?.blocks.find((item) => item.blockId === block.blockId)
      operations.push({
        kind: 'block.insert', operationId: operationId(),
        target: { leftBlockId: working[index - 1] ?? null, rightBlockId: working[index] ?? null },
        payload: { candidateBlockId: block.blockId, content: original?.markdown ?? block.markdown,
          ...(source ? { restoreFrom: { revisionId: source.revisionId, blockId: block.blockId } } : {}) },
      })
      working.splice(index, 0, block.blockId)
    } else if (currentIndex !== index) {
      const source = { leftBlockId: working[currentIndex - 1] ?? null, rightBlockId: working[currentIndex + 1] ?? null }
      working.splice(currentIndex, 1)
      operations.push({ kind: 'block.move', operationId: operationId(),
        target: { blockId: block.blockId, expectedBlockRevision: original!.blockRevision },
        payload: { source, destination: { leftBlockId: working[index - 1] ?? null, rightBlockId: working[index] ?? null } } })
      working.splice(index, 0, block.blockId)
    }
    if (original && original.markdown !== block.markdown) {
      operations.push({ kind: 'block.replace', operationId: operationId(),
        target: { blockId: block.blockId, expectedBlockRevision: original.blockRevision }, payload: { content: block.markdown } })
    }
  }
  return operations
}
