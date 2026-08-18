#!/usr/bin/env node
// Copy the canonical agent picker into every plugin that shows one.
//
// The picker has to be identical everywhere — that is what makes it a standard
// rather than three controls that merely resemble each other. The main app and
// each plugin are separate Vite builds with no shared module graph, so "shared"
// here means "one source of truth plus a copy step", with a test
// (src/lib/agent-picker/copies.test.ts) that fails the build when a copy drifts.
//
//   node scripts/sync-agent-picker.mjs           # write the copies
//   node scripts/sync-agent-picker.mjs --check   # fail if any copy is stale
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')

/** Canonical sources, relative to the repo root. */
export const FILES = ['AgentPicker.svelte', 'types.ts']

/** Plugins that render a picker. The agent plugins themselves do NOT: their
 *  window IS one agent, so offering to switch inside it would be a lie. */
export const TARGETS = [
  'plugins-src/idea-spark/src/lib/agent-picker',
  'plugins-src/ebook-import/src/lib/agent-picker',
  'plugins-src/trace-source/src/lib/agent-picker',
]

const SRC = 'src/lib/agent-picker'

/** The banner every copy carries, so nobody edits one by accident. */
function stamp(name) {
  const line = `GENERATED COPY — do not edit. Source: ${SRC}/${name}. Run \`node scripts/sync-agent-picker.mjs\` after changing it.`
  return name.endsWith('.svelte')
    ? `<!-- ${line} -->\n`
    : `// ${line}\n`
}

export function canonical(name) {
  return stamp(name) + readFileSync(join(ROOT, SRC, name), 'utf8')
}

function main() {
  const check = process.argv.includes('--check')
  const stale = []
  for (const target of TARGETS) {
    for (const name of FILES) {
      const want = canonical(name)
      const path = join(ROOT, target, name)
      const have = existsSync(path) ? readFileSync(path, 'utf8') : null
      if (have === want) continue
      if (check) {
        stale.push(`${target}/${name}`)
        continue
      }
      mkdirSync(dirname(path), { recursive: true })
      writeFileSync(path, want)
      console.log(`  wrote ${target}/${name}`)
    }
  }
  if (check && stale.length) {
    console.error(
      `agent picker copies are stale:\n  ${stale.join('\n  ')}\n` +
        'Run `node scripts/sync-agent-picker.mjs`.',
    )
    process.exit(1)
  }
  console.log(check ? 'agent picker copies are in sync' : 'agent picker synced')
}

if (import.meta.url === `file://${process.argv[1]}`) main()
