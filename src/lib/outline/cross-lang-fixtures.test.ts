// The Rust chunker (searchidx/src/outline.rs) and this parser read the same
// files. They are pinned to the same fixtures so that a change to either one
// that moves a line from one node to another fails loudly, instead of silently
// giving two agents two different trees for one .note.md.
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
const expected = JSON.parse(readFileSync(join(DIR, 'expected.json'), 'utf8')) as
  Record<string, Array<{ line_start: number; text: string; breadcrumb: string }>>

/** Depth-first node order, matching the Rust chunker's emission order. */
function flatten(tree: ReturnType<typeof parseOutline>, parentId: string | null, trail: string[]) {
  const out: Array<{ text: string; breadcrumb: string }> = []
  for (const n of childrenOf(tree, parentId)) {
    const first = n.content.split('\n')[0]
    out.push({ text: n.content, breadcrumb: trail.map((s) => s.slice(0, 40)).join(' > ') })
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
  }
})
