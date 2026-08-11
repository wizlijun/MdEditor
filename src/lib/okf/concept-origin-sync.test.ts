// Guards the fixture that searchidx/src/origin.rs's cross-language sync test
// reads (searchidx/tests/fixtures/origin/concept-types.json). That fixture is
// *generated* from CONCEPT_TYPE by scripts/gen-origin-types.mjs, not hand
// authored — see the file header there for why a regex extractor rather than
// a TS import.
//
// A generated-and-committed artifact that nobody re-checks goes stale
// silently: someone adds a value to CONCEPT_TYPE, forgets to re-run the
// generator, and the Rust sync test still passes because it is only reading
// last week's fixture. This test closes that gap from the TypeScript side —
// it re-extracts from the *current* concept.ts on every `pnpm test` and fails
// loudly if the committed fixture no longer matches, with the exact command
// to fix it.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { CONCEPT_TYPE } from './concept'
// @ts-expect-error - plain-JS extractor shared with the Rust-side fixture generator
import { extractConceptTypes } from '../../../scripts/gen-origin-types.mjs'

const FIXTURE = join(process.cwd(), 'searchidx/tests/fixtures/origin/concept-types.json')

describe('searchidx origin fixture stays in sync with CONCEPT_TYPE', () => {
  it('the extractor sees exactly the values CONCEPT_TYPE actually has', () => {
    const source = readFileSync(join(process.cwd(), 'src/lib/okf/concept.ts'), 'utf8')
    expect(extractConceptTypes(source)).toEqual(Object.values(CONCEPT_TYPE))
  })

  it('the committed fixture matches what the generator would produce right now', () => {
    const committed = JSON.parse(readFileSync(FIXTURE, 'utf8'))
    expect(committed, 'stale fixture — run: pnpm gen:origin-types').toEqual(Object.values(CONCEPT_TYPE))
  })
})
