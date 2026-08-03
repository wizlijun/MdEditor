import { describe, it, expect } from 'vitest'
// The very same bytes the Rust crate asserts it writes
// (plugins-src/roam-import/backend/tests/golden.rs). Imported with ?raw — the
// repo's way of reaching a plugins-src asset from src (see lib/print.ts) — so
// no Node type declarations are needed for `pnpm check`.
import text from '../../../plugins-src/roam-import/backend/tests/fixtures/daily.note.md?raw'
import { parseOutline, serializeOutline } from './markdown'

/** Format-drift guard, TS half. The Rust backend writes this exact file
 *  (plugins-src/roam-import/backend/tests/golden.rs asserts it byte-for-byte);
 *  the host must read it back and re-serialize it unchanged. */
describe('roam-import golden daily note', () => {
  it('round-trips through the host outline parser unchanged', () => {
    expect(serializeOutline(parseOutline(text))).toBe(text)
  })

  it('keeps every Roam block addressable by id', () => {
    const tree = parseOutline(text)
    const persisted = [...tree.nodes.values()].filter((n) => n.persistId === true)
    expect(persisted.length).toBeGreaterThan(0)
    for (const n of persisted) expect(n.id).not.toMatch(/^local-/)
  })

  /** The two things the Roam side must not have mangled on its way through:
   *  a continuation line that reads like a property (Roam's escape keeps it
   *  content), and the user's own writing (no id::, so nothing on the Roam
   *  side can ever claim it). */
  it('keeps an escaped property line as content, not a property', () => {
    const tree = parseOutline(text)
    const block = [...tree.nodes.values()].find((n) => n.id === 'Nb7sT1uEv')
    expect(block?.content).toBe('meeting notes\n id:: not-a-property')
  })

  it("keeps the user's own blocks free of any id", () => {
    const tree = parseOutline(text)
    const mine = [...tree.nodes.values()].filter((n) => n.content === 'my own take on this one')
    expect(mine).toHaveLength(1)
    expect(mine[0].persistId).toBeUndefined()
  })
})
