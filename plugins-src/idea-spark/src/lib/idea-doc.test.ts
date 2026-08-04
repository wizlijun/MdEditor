import { describe, it, expect } from 'vitest'
import { parse as parseYaml } from 'yaml'
import { buildIdeaDoc, rebuildIdeaDoc } from './idea-doc'
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

describe('rebuildIdeaDoc', () => {
  const original = 'type: Idea\ncreated: 2026-01-01T00:00:00Z\nstatus: draft\ntags:\n  - transfer'

  it('keeps the original created instead of stamping the save time', () => {
    const out = rebuildIdeaDoc(original, 'body', '2026-08-04T00:00:00Z')
    expect(out).toContain('created: 2026-01-01T00:00:00Z')
    expect(out).not.toContain('2026-08-04T00:00:00Z')
  })

  it('keeps keys this plugin knows nothing about', () => {
    const out = rebuildIdeaDoc(original, 'body', '2026-08-04T00:00:00Z')
    const meta = parseYaml(out.match(/^---\n([\s\S]*?)\n---\n/)![1])
    expect(meta.status).toBe('draft')
    expect(meta.tags).toEqual(['transfer'])
  })

  it('heals a frontmatter that is missing the mandatory type', () => {
    const out = rebuildIdeaDoc('created: 2026-01-01T00:00:00Z', 'body', '2026-08-04T00:00:00Z')
    expect(out).toContain('type: Idea')
    expect(lintText('2026-08-04-x.md', out)).toEqual([])
  })

  it('supplies created when the original frontmatter has none', () => {
    const out = rebuildIdeaDoc('type: Idea', 'body', '2026-08-04T00:00:00Z')
    expect(out).toContain('created: 2026-08-04T00:00:00Z')
  })

  it('produces a lint-clean document and keeps the body verbatim', () => {
    const body = '# Title\n\n---\n\nmore'
    const out = rebuildIdeaDoc(original, body, '2026-08-04T00:00:00Z')
    expect(out.endsWith(body)).toBe(true)
    expect(lintText('2026-08-04-title.md', out)).toEqual([])
  })

  // `touchConceptFrontmatter` deliberately refuses to rewrite a frontmatter it
  // cannot read as a mapping, so these blocks come back untouched — without a
  // guard the saved file would have no `type` at all (OKF §4.1 hard constraint)
  // — and on syntactically broken YAML it throws outright, which would abort
  // the save entirely.
  describe('frontmatter that cannot carry a type', () => {
    const cases: Array<[string, string]> = [
      ['a sequence', '- one\n- two'],
      ['a bare scalar', 'just a note to self'],
      ['unparsable YAML', 'a: [1, 2\nb: "unterminated'],
      ['an empty type', 'type:\ncreated: 2026-01-01T00:00:00Z'],
      ['a non-string type', 'type: 42'],
    ]

    for (const [label, fm] of cases) {
      it(`still writes a lint-clean Idea document for ${label}`, () => {
        const out = rebuildIdeaDoc(fm, 'body', '2026-08-04T00:00:00Z')
        expect(lintText('2026-08-04-x.md', out)).toEqual([])
        const meta = parseYaml(out.match(/^---\n([\s\S]*?)\n---\n/)![1])
        expect(meta.type).toBe('Idea')
      })

      it(`keeps the unusable block's bytes in the body for ${label}`, () => {
        const out = rebuildIdeaDoc(fm, 'body', '2026-08-04T00:00:00Z')
        const content = out.slice(out.indexOf('\n---\n', 4) + 5)
        expect(content).toContain(fm.trim())
        expect(content.endsWith('body')).toBe(true)
      })
    }

    it('does not invent an empty salvage paragraph when the block is blank', () => {
      const out = rebuildIdeaDoc('   ', 'body', '2026-08-04T00:00:00Z')
      expect(lintText('2026-08-04-x.md', out)).toEqual([])
      expect(out.endsWith('---\nbody')).toBe(true)
    })
  })
})
