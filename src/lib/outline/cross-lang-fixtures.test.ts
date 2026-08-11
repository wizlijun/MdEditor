// The Rust chunker (searchidx/src/outline.rs) and this parser read the same
// files. They are pinned to the same fixtures so that a change to either one
// that moves a line from one node to another fails loudly, instead of silently
// giving two agents two different trees for one .note.md.
//
// Four of the six fields in expected.json are pinned here: `text`,
// `breadcrumb`, `is_annotation` and `agent_by`. The last two are not stored on
// an OutlineNode under those names — they are *derived* below from `source`
// and `answeredBy`, exactly the way searchidx::outline derives them from the
// `type::` and `by::` property lines — and they matter because they drive
// retrieval's ×1.2 annotation boost and ×0.85 agent-authored penalty. Asserting
// only text and breadcrumb left them unpinned across languages: verified
// vacuous by mutation (flipping the Rust `is_annotation`/`agent_by` rules kept
// this suite green while `outline_fixtures.rs` went red, so a *divergence*
// would have been reported as a one-sided failure rather than a mismatch).
//
// `line_start`/`line_end` are deliberately not asserted here: they have no
// TypeScript counterpart at all. `parseOutline` builds a tree of nodes with no
// line attribution — the editor works in tree space, not line space — so there
// is nothing on this side to compare against. Those two columns of
// expected.json are pinned by the Rust half alone (tests/outline_fixtures.rs).
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { parseOutline } from './markdown'
import { childrenOf } from './model'

// vitest runs this suite from the repo root (see vitest.config.ts `include`),
// not from this file's directory, so the fixtures live at a cwd-rooted path
// rather than one derived from `__dirname` (which the brief's snippet used —
// that doesn't resolve under this repo's ESM/Vite test runner).
const DIR = join(process.cwd(), 'searchidx/tests/fixtures/outline')
type Expected = { line_start: number; text: string; breadcrumb: string; is_annotation: boolean; agent_by: string | null }
const expected = JSON.parse(readFileSync(join(DIR, 'expected.json'), 'utf8')) as Record<string, Expected[]>

/**
 * Depth-first node order, matching the Rust chunker's emission order.
 *
 * `is_annotation` / `agent_by` mirror searchidx::outline:
 *  - an annotation is `type:: annotation` or `type:: question` (both are the
 *    user's own marginalia; both earn the retrieval boost);
 *  - `by::` records an agent author only when it is NOT a `human:` actor —
 *    OKF §7 makes that prefix the machine-checkable "a person stands behind
 *    this" signal, so a human-signed node must never be penalised as
 *    AI-authored.
 */
function flatten(tree: ReturnType<typeof parseOutline>, parentId: string | null, trail: string[]) {
  const out: Array<{ text: string; breadcrumb: string; is_annotation: boolean; agent_by: string | null }> = []
  for (const n of childrenOf(tree, parentId)) {
    const first = n.content.split('\n')[0]
    out.push({
      text: n.content,
      breadcrumb: trail.map((s) => s.slice(0, 40)).join(' > '),
      is_annotation: ['annotation', 'question'].includes(n.source),
      agent_by: n.answeredBy && !n.answeredBy.startsWith('human:') ? n.answeredBy : null,
    })
    out.push(...flatten(tree, n.id, [...trail, first]))
  }
  return out
}

describe('outline fixtures agree across Rust and TypeScript', () => {
  for (const name of Object.keys(expected)) {
    it(`${name}: node text and breadcrumbs match the shared expectations`, () => {
      const tree = parseOutline(readFileSync(join(DIR, name), 'utf8'))
      const got = flatten(tree, null, [])
      expect(got.map((n) => n.text)).toEqual(expected[name].map((e) => e.text))
      expect(got.map((n) => n.breadcrumb)).toEqual(expected[name].map((e) => e.breadcrumb))
    })

    it(`${name}: annotation and agent-author provenance match the shared expectations`, () => {
      const tree = parseOutline(readFileSync(join(DIR, name), 'utf8'))
      const got = flatten(tree, null, [])
      expect(got.map((n) => n.is_annotation)).toEqual(expected[name].map((e) => e.is_annotation))
      expect(got.map((n) => n.agent_by)).toEqual(expected[name].map((e) => e.agent_by))
    })
  }
})
