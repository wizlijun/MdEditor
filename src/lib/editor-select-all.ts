import { AllSelection, TextSelection, type Selection } from 'prosemirror-state'
import type { Node as PmNode } from 'prosemirror-model'

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
