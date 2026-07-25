import { Plugin, PluginKey } from 'prosemirror-state'
import { Decoration, DecorationSet } from 'prosemirror-view'

const placeholderKey = new PluginKey<DecorationSet>('placeholder')

/**
 * Show a hint inside an empty document so the caret's location is obvious.
 *
 * An empty markdown file parses to `doc > paragraph` with nothing in it, which
 * renders as a single blank line — visually indistinguishable from the rest of
 * the pane, leaving no cue about where typing goes. This marks that lone empty
 * paragraph so CSS can render `data-placeholder` via `::before`.
 *
 * Only the "whole document is empty" case is decorated; empty paragraphs inside
 * a non-empty document are ordinary blank lines and get no hint.
 *
 * Restricted to a `paragraph` on purpose: typing `# ` turns the block into an
 * empty *heading*, and leaving the floated hint on it pushed the caret onto the
 * next line at heading font size. Starting a heading is already "writing", so
 * the hint has served its purpose by then.
 */
export function placeholderPlugin(text: string): Plugin {
  return new Plugin({
    key: placeholderKey,
    props: {
      decorations(state) {
        const doc = state.doc
        if (doc.childCount !== 1) return null
        const first = doc.firstChild
        if (!first || first.type.name !== 'paragraph' || first.content.size > 0) return null
        return DecorationSet.create(doc, [
          Decoration.node(0, first.nodeSize, {
            class: 'is-empty',
            'data-placeholder': text,
          }),
        ])
      },
    },
  })
}
