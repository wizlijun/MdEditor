#!/usr/bin/env node
// Regenerates searchidx/tests/fixtures/origin/concept-types.json from the
// single source of truth, CONCEPT_TYPE in src/lib/okf/concept.ts.
//
//   node scripts/gen-origin-types.mjs        # rewrite the fixture
//   node scripts/gen-origin-types.mjs --check # exit 1 if the fixture is stale
//
// Why a regex extractor instead of importing concept.ts directly: this repo
// has no ts-node/tsx in its dependency tree (see scripts/okf-lint-core.mjs for
// the established precedent of mirroring a TS registry into plain JS/data
// rather than adding a TS loader just for tooling). CONCEPT_TYPE's values are
// a flat object of `key: 'Value',` lines with no computed values, so a
// line-oriented extraction over the literal `{ ... } as const` block is exact
// for that shape and needs no build step.
//
// Consumers of the output:
//   - searchidx/src/origin.rs's `every_registered_concept_type_has_a_mapped_origin`
//     test reads this fixture and asserts every value has a tier in
//     `mapped_type_origin`. That catches "added a type, forgot the mapping".
//   - src/lib/okf/concept-origin-sync.test.ts asserts this committed fixture
//     still equals what running this script right now would produce. That
//     catches "added a type, forgot to regenerate the fixture" — the failure
//     mode that would otherwise let the Rust test above silently go stale.
import { readFileSync, writeFileSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const SRC = join(ROOT, 'src/lib/okf/concept.ts')
const OUT = join(ROOT, 'searchidx/tests/fixtures/origin/concept-types.json')

export function extractConceptTypes(tsSource) {
  const start = tsSource.indexOf('export const CONCEPT_TYPE = {')
  if (start === -1) throw new Error('CONCEPT_TYPE block not found in concept.ts')
  const end = tsSource.indexOf('} as const', start)
  if (end === -1) throw new Error('CONCEPT_TYPE block has no closing `} as const`')
  const block = tsSource.slice(start, end)

  const values = []
  const lineRe = /^\s*\/?\/?[\w$]*\s*:\s*'([^']*)'\s*,?\s*$/
  for (const rawLine of block.split('\n')) {
    const line = rawLine.trim()
    if (line === '' || line.startsWith('//') || line.startsWith('/*') || line.startsWith('*') || line.startsWith('export')) continue
    const m = line.match(/^[\w$]+\s*:\s*'([^']*)'\s*,?\s*$/)
    if (m) values.push(m[1])
  }
  if (values.length === 0) throw new Error('extracted zero values — regex likely stale against concept.ts formatting')
  return values
}

function main() {
  const check = process.argv.includes('--check')
  const source = readFileSync(SRC, 'utf8')
  const values = extractConceptTypes(source)
  const json = JSON.stringify(values, null, 2) + '\n'

  if (check) {
    const current = readFileSync(OUT, 'utf8')
    if (current !== json) {
      console.error(`${OUT} is stale — run: node scripts/gen-origin-types.mjs`)
      process.exit(1)
    }
    console.log('origin fixture is up to date')
    return
  }

  writeFileSync(OUT, json)
  console.log(`wrote ${values.length} types to ${OUT}`)
}

if (import.meta.url === `file://${process.argv[1]}`) main()
