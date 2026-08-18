import { describe, it, expect } from 'vitest'
import { readFileSync, existsSync } from 'node:fs'
import { join } from 'node:path'
import { FILES, TARGETS, canonical } from '../../../scripts/sync-agent-picker.mjs'

// The picker is a standard only for as long as every copy is the same control.
// The main app and each plugin are separate Vite builds with no shared module
// graph, so the copies are real files — and a real file is a file somebody can
// edit. This is what turns "please keep them in sync" into a build failure.
describe('agent picker copies', () => {
  for (const target of TARGETS) {
    for (const name of FILES) {
      it(`${target}/${name} matches the canonical source`, () => {
        const path = join(process.cwd(), target, name)
        expect(existsSync(path), `${path} is missing — run scripts/sync-agent-picker.mjs`).toBe(
          true,
        )
        expect(readFileSync(path, 'utf8')).toBe(canonical(name))
      })
    }
  }
})
