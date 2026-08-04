import { describe, it, expect } from 'vitest'
import { parse as parseYaml } from 'yaml'
import { buildIdeaDoc } from './idea-doc'
// The host repo's OKF hard-constraint linter is plain JS with no dependency
// on the main app's src/ tree (only on the `yaml` package this plugin
// already depends on), so it can be imported straight from the plugin test
// exactly like plugins-src/roam-import and src/lib/okf/concept.test.ts do —
// this gives Step 4 a *real* okf-lint pass instead of a hand-rolled stand-in.
// @ts-expect-error - plain-JS lint core shared with scripts/okf-lint.mjs
import { lintText } from '../../../../scripts/okf-lint-core.mjs'

describe('buildIdeaDoc', () => {
  it('wraps the body with OKF frontmatter carrying type: Idea and created', () => {
    const out = buildIdeaDoc('一个念头', '2026-08-04T00:00:00Z')
    expect(out.startsWith('---\n')).toBe(true)
    expect(out).toContain('type: Idea')
    expect(out).toContain('created: 2026-08-04T00:00:00Z')
    expect(out.endsWith('一个念头')).toBe(true)
  })

  it('is valid, parseable YAML frontmatter with a non-empty type', () => {
    const out = buildIdeaDoc('x', '2026-08-04T00:00:00Z')
    const m = out.match(/^---\n([\s\S]*?)\n---\n/)
    expect(m).not.toBeNull()
    const meta = parseYaml(m![1])
    expect(typeof meta.type).toBe('string')
    expect(meta.type.length).toBeGreaterThan(0)
  })

  it('satisfies the OKF v0.2 hard constraints per the shared lint core', () => {
    const out = buildIdeaDoc('x', '2026-08-04T00:00:00Z')
    expect(lintText('2026-08-04-x.md', out)).toEqual([])
  })

  it('never adopts a reserved name (index.md/log.md) as a lint-passing document', () => {
    // Even a well-formed Idea doc must be rejected by the linter if it were
    // ever saved under a reserved filename — this is the linter's job, not
    // buildIdeaDoc's, but the interaction is worth pinning down here since
    // this is where the two modules meet.
    const out = buildIdeaDoc('x', '2026-08-04T00:00:00Z')
    expect(lintText('index.md', out)).not.toEqual([])
  })

  it('preserves the body verbatim, including embedded --- lines', () => {
    const body = '正文第一行\n\n---\n\n正文第二段'
    const out = buildIdeaDoc(body, '2026-08-04T00:00:00Z')
    expect(out.endsWith(body)).toBe(true)
  })
})
