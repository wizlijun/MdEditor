import { describe, it, expect } from 'vitest'
// The very same bytes the Rust crate asserts it writes
// (plugins-src/roam-import/backend/tests/golden.rs). Imported with ?raw — the
// repo's way of reaching a plugins-src asset from src (see lib/print.ts) — so
// no Node type declarations are needed for `pnpm check`.
import text from '../../../plugins-src/roam-import/backend/tests/fixtures/daily.note.md?raw'
import fmCases from '../../../plugins-src/roam-import/backend/tests/fixtures/frontmatter-touch.json'
import { parseOutline, serializeOutline } from './markdown'
import { touchFrontmatter } from './frontmatter'

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

  /** The three shapes a Roam block can forge that `parseOutline` would
   *  otherwise read as structure. Each one, left unescaped, costs the block the
   *  `id::` that is its identity — after which the merge re-creates it on every
   *  sync and the user's note multiplies without bound. All three are asserted
   *  from this side too, because it is this parser they have to survive. */
  const nodeById = (id: string) => [...parseOutline(text).nodes.values()].find((n) => n.id === id)

  it('keeps an escaped property line as content, not a property', () => {
    expect(nodeById('Nb7sT1uEv')?.content).toBe('meeting notes\n id:: not-a-property')
  })

  it('keeps an escaped bullet line as content, not a child node', () => {
    // A Roam shift-enter list (`shopping\n- milk\n- eggs`).
    const block = nodeById('RmQ2xL8vC')
    expect(block?.content).toBe('shopping\n - milk\n - eggs')
    const tree = parseOutline(text)
    expect([...tree.nodes.values()].some((n) => n.content === 'milk')).toBe(false)
    expect([...tree.nodes.values()].filter((n) => n.parentId === 'RmQ2xL8vC')).toHaveLength(0)
  })

  it('closes a fence the Roam block never closed, so the tail is not swallowed', () => {
    expect(nodeById('Fp3nH6wDs')?.content).toBe('```js\nconst x = 1\n```')
    // The blocks after it survived as their own nodes rather than being eaten
    // into the fence body.
    expect(nodeById('Fp3nH6wDs')?.createdAt).toBe('2026-08-02T14:15:00.000Z')
    expect(nodeById('Fp3nH6wDs')?.updatedAt).toBe('2026-08-02T14:16:40.000Z')
  })

  it("keeps the user's own blocks free of any id", () => {
    const tree = parseOutline(text)
    const mine = [...tree.nodes.values()].filter((n) => n.content === 'my own take on this one')
    expect(mine).toHaveLength(1)
    expect(mine[0].persistId).toBeUndefined()
  })

  /** The annotation → question → answer subtree the user and an agent wrote.
   *  It is local (no `id::`), so the merge must carry it through verbatim —
   *  including the properties `convertPage` never produces and which a careless
   *  merge would reset. */
  it('carries an annotation, its question and its fenced answer through intact', () => {
    const nodes = [...parseOutline(text).nodes.values()]
    const anno = nodes.find((n) => n.source === 'annotation')!
    expect(anno.content).toBe('the line I marked up')
    expect(anno.anchorLine).toBe(12)

    const q = nodes.find((n) => n.source === 'question' && n.status === 'answered')!
    expect(q.parentId).toBe(anno.id)

    const answer = nodes.find((n) => n.source === 'answer')!
    expect(answer.parentId).toBe(q.id)
    expect(answer.answeredAt).toBe('2026-08-02T11:05:00.000Z')
    expect(answer.answeredBy).toBe('claude-code')
    // The fence body: a shorter inner ``` must not close the outer ````, and
    // nothing inside it becomes an outline node.
    expect(answer.content).toContain('```rust')
    expect(nodes.some((n) => n.content.startsWith('a list inside the fence'))).toBe(false)
  })
})

/** Front-matter drift guard. The daily fixture above structurally cannot catch
 *  this: it round-trips `tree.frontmatter` verbatim and never touches it, so
 *  `outline.rs`'s hand-rolled line-based `touch_frontmatter` had no counterpart
 *  assertion. Same cases, same expected bytes, asserted from both sides —
 *  `backend/tests/golden.rs::frontmatter_touch_matches_the_shared_fixture` is
 *  the Rust half. */
describe('roam-import front-matter touch parity', () => {
  const cases = (fmCases as { cases: Array<Record<string, string | null>> }).cases

  it('has cases to check', () => {
    expect(cases.length).toBeGreaterThan(0)
  })

  it.each(cases)('$name', (c) => {
    expect(
      touchFrontmatter(c.raw as string | null, {
        // The fixture carries the OKF §4.1 type explicitly rather than letting
        // either side default: what has to match is the value that lands on
        // disk, and the plugin writes daily notes (`Daily Note`), not the
        // `Outline Note` this host function falls back to.
        type: c.type as string,
        title: c.title as string,
        created: c.created as string,
        now: c.now as string,
      }),
    ).toBe(c.expected)
  })
})
