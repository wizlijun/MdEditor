import { AllSelection, TextSelection, type Selection, type Transaction } from 'prosemirror-state'
import type { Node as PmNode } from 'prosemirror-model'
import { isSelectAllShortcut } from './select-all-shortcut'

export { isSelectAllShortcut } from './select-all-shortcut'

/**
 * Build the selection that "Select All" applies in the rich editor.
 *
 * A doc that opens with `frontmatter` cannot use a plain AllSelection. That
 * node renders through a NodeView whose container is `contentEditable=false`
 * (a collapsible <details>, see lib/frontmatter-view.ts), and WebKit clamps the
 * *painted* selection to that non-editable subtree when the DOM range starts
 * inside it: state is correct — Backspace wipes the whole document — but the
 * user only sees the metadata block highlighted, which reads as "Cmd+A only
 * selected the folded part".
 *
 * Starting after the frontmatter keeps the range entirely inside editable
 * content so it paints, and is the better semantic anyway: Cmd+A should grab
 * the prose, not the metadata table rendered above it.
 */
export function selectAllSelection(doc: PmNode): Selection {
  const first = doc.firstChild
  if (first?.type.name === 'frontmatter' && first.nodeSize < doc.content.size) {
    return TextSelection.between(doc.resolve(first.nodeSize), doc.resolve(doc.content.size))
  }
  return new AllSelection(doc)
}

/** The slice of EditorView select-all needs — kept structural so it can be stubbed in tests. */
export interface SelectAllTarget {
  state: { doc: PmNode; tr: Transaction }
  dispatch: (tr: Transaction) => void
  focus: () => void
}

/**
 * Apply Select All to a rich editor view.
 *
 * Single entry point on purpose: the right-click menu, the Cmd+A key handler
 * and the Edit-menu item all land here, so they cannot drift apart again — the
 * bug this consolidates was exactly that divergence (right-click was correct,
 * Cmd+A was not).
 */
export function applySelectAll(view: SelectAllTarget): void {
  view.dispatch(view.state.tr.setSelection(selectAllSelection(view.state.doc)))
  view.focus()
}

type Chord = Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey'>

/**
 * Handle a keydown that may be the Select All chord; returns whether it was
 * consumed.
 *
 * `stopPropagation` is what makes this stick. moraya-core binds its own `Mod-a`
 * on the ProseMirror element (dist/index.js: raw AllSelection, or *just the
 * enclosing code_block* when the caret is inside one) — leaving that to run
 * afterwards is what made Cmd+A select only part of the document.
 */
export function handleSelectAllKeydown(
  e: Chord & { preventDefault: () => void; stopPropagation: () => void },
  view: SelectAllTarget,
): boolean {
  if (!isSelectAllShortcut(e)) return false
  e.preventDefault()
  e.stopPropagation()
  applySelectAll(view)
  return true
}
