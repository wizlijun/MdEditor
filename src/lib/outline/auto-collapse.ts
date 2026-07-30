// A note whose questions have all been answered is mostly answer text: the
// panel stops being scannable exactly when it fills up with ✦ replies. So an
// answered question arrives collapsed, and the reader opens the ones they care
// about.
//
// This is a VIEW default, not a fact about the file. The ids land in a set the
// serializer skips, so merely opening — or refreshing after an agent run —
// never writes `collapsed:: true` into the user's note.
import { childrenOf, type OutlineTree } from './model'

/**
 * Collapse every question that already has an answer under it, and return the
 * ids collapsed this way. Questions still open, and nodes the file itself says
 * are collapsed, are left alone — the caller must not persist the returned ids.
 */
export function collapseAnsweredOnLoad(tree: OutlineTree): Set<string> {
  const auto = new Set<string>()
  for (const q of tree.nodes.values()) {
    if (q.source !== 'question') continue
    if (q.status === 'open') continue
    if (q.collapsed) continue // the file already said so; that one persists
    if (!childrenOf(tree, q.id).some((c) => c.source === 'answer')) continue
    q.collapsed = true
    auto.add(q.id)
  }
  return auto
}
