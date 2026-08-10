import { parseOutline, serializeOutline } from './markdown'
import { addNode, childrenOf, newId, nowIso, type OutlineTree } from './model'

/**
 * Append a quoted excerpt to a sidecar note's text, returning the new text
 * (null when there is nothing worth writing).
 *
 * This is the whole "document → note" path for mdx. Files that can carry
 * CriticMarkup get their annotation written inline and the note derives from
 * it; mdx cannot be written at all (see `lib/mdx/display.ts`), so the excerpt
 * *is* the anchor and lives only in the note.
 */
export function appendExcerpt(noteText: string, excerpt: string): string | null {
  const tree: OutlineTree = parseOutline(noteText)
  return appendExcerptToTree(tree, excerpt) ? serializeOutline(tree) : null
}

/**
 * Same append against a tree already held in memory — used when the sidecar
 * panel has the file attached, since writing the file underneath it would be
 * overwritten by its next save.
 */
export function appendExcerptToTree(tree: OutlineTree, excerpt: string): boolean {
  // Outline nodes are one line each; a multi-line selection becomes one node
  // rather than silently losing everything after the first newline.
  const content = excerpt.replace(/\s*\r?\n\s*/g, ' ').trim()
  if (!content) return false

  const roots = childrenOf(tree, null)
  const last = roots.length ? roots[roots.length - 1].order : null
  addNode(tree, {
    id: newId(),
    parentId: null,
    order: last == null ? 0 : last + 100,
    content,
    collapsed: false,
    source: 'manual',
    createdAt: nowIso(),
    updatedAt: nowIso(),
  })
  return true
}
