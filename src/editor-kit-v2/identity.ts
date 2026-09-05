import { Fragment, Schema, Slice, type Node as PmNode } from 'prosemirror-model'
import type { EditorState, Transaction } from 'prosemirror-state'
import type { Mapping } from 'prosemirror-transform'
import type { BlockLayout } from './block-layout'

export const BLOCK_ID_ATTR = 'cdrBlockId'

/** Internal only: no parseDOM/toDOM hook exports this identity to the clipboard. */
export function withBlockIdentitySchema(schema: Schema): Schema {
  let nodes = schema.spec.nodes
  nodes.forEach((name, spec) => {
    if (name !== 'doc' && schema.nodes[name].isBlock) {
      nodes = nodes.update(name, { ...spec, attrs: { ...spec.attrs, [BLOCK_ID_ATTR]: { default: null } } })
    }
  })
  return new Schema({ ...schema.spec, nodes })
}

export function nodeBlockId(node: PmNode): string | null {
  return typeof node.attrs[BLOCK_ID_ATTR] === 'string' ? node.attrs[BLOCK_ID_ATTR] : null
}

export function tagBlockNode(node: PmNode, blockId: string): PmNode {
  return node.type.create({ ...node.attrs, [BLOCK_ID_ATTR]: blockId }, node.content, node.marks)
}

export function readBlockLayout(doc: PmNode): BlockLayout {
  const spans: Array<{ blockId: string; startIndex: number; endIndex: number }> = []
  doc.forEach((node, _position, index) => {
    const blockId = nodeBlockId(node)
    if (!blockId) return
    const previous = spans.at(-1)
    if (previous?.blockId === blockId) previous.endIndex = index + 1
    else spans.push({ blockId, startIndex: index, endIndex: index + 1 })
  })
  return { spans }
}

/**
 * Identity steps join the original transaction before history records it.
 * Existing contiguous multi-node blocks remain spans. A join/wrap inherits the
 * first surviving identity; fully selected content inherits the left boundary.
 * Repeated non-contiguous IDs (copy/split of a span) receive a new identity.
 */
export function normalizeBlockIdentity(
  state: EditorState,
  before: EditorState,
  freshId: () => string,
  mapping?: Mapping,
): Transaction | null {
  const preferred = nodeBlockId(before.doc.child(Math.min(before.doc.childCount - 1, before.selection.$from.index(0))))
  const used = new Set<string>()
  let previousId: string | null = null
  let previousEmpty = false
  const tr = state.tr
  const occupied = new Set<string>()
  state.doc.forEach((node) => { const id = nodeBlockId(node); if (id) occupied.add(id) })
  const inverse = mapping?.invert()
  state.doc.forEach((node, position) => {
    let id = nodeBlockId(node)
    if (!id) {
      node.descendants((child) => {
        if (!id) id = nodeBlockId(child)
        return !id
      })
    }
    if (!id && inverse) {
      const oldPosition = Math.max(0, Math.min(before.doc.content.size - 1, inverse.map(position + 1, -1)))
      const oldId = nodeBlockId(before.doc.child(before.doc.resolve(oldPosition).index(0)))
      // Replacing an entire node need not leave an attribute behind. Use the
      // transaction mapping, not the user's unrelated current selection. Do
      // not steal the identity of a still-present neighbour on a pure insert.
      if (oldId && !occupied.has(oldId)) id = oldId
    }
    id ??= previousId ?? preferred ?? freshId()
    const empty = node.type.name === 'paragraph' && node.content.size === 0
    // Markdown parsers discard blank paragraphs. Give an inserted empty
    // paragraph its own durable block instead of hiding it in a text span.
    if (used.has(id) && (id !== previousId || empty || previousEmpty)) id = freshId()
    if (nodeBlockId(node) !== id) tr.setNodeMarkup(position, undefined, { ...node.attrs, [BLOCK_ID_ATTR]: id })
    used.add(id)
    previousId = id
    previousEmpty = empty
  })
  return tr.docChanged ? tr : null
}

/** Even programmatically supplied clipboard slices must not duplicate IDs. */
export function withoutBlockIdentity(slice: Slice): Slice {
  const clean = (node: PmNode): PmNode => {
    if (node.isText) return node
    const children: PmNode[] = []
    node.forEach((child) => children.push(clean(child)))
    return node.type.create({ ...node.attrs, [BLOCK_ID_ATTR]: null }, Fragment.from(children), node.marks)
  }
  const nodes: PmNode[] = []
  slice.content.forEach((node) => nodes.push(clean(node)))
  return new Slice(Fragment.from(nodes), slice.openStart, slice.openEnd)
}
