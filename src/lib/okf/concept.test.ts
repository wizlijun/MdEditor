import { describe, it, expect } from 'vitest'
import { parse as parseYaml } from 'yaml'
import { CONCEPT_TYPE, touchConceptFrontmatter, conceptFileText } from './concept'
// @ts-expect-error - plain-JS lint core shared with scripts/okf-lint.mjs
import { lintText } from '../../../scripts/okf-lint-core.mjs'

describe('touchConceptFrontmatter', () => {
  it('writes type first for a brand-new document', () => {
    expect(touchConceptFrontmatter(null, { type: CONCEPT_TYPE.note, title: '会议纪要' }))
      .toBe('type: Note\ntitle: 会议纪要')
  })

  it('never overwrites a key the document already has', () => {
    const raw = 'type: Book\ntitle: 原标题'
    expect(touchConceptFrontmatter(raw, { type: CONCEPT_TYPE.note, title: '新标题' })).toBe(raw)
  })

  it('keeps unknown keys and their original order, appending only what is missing', () => {
    const out = touchConceptFrontmatter('title: X\nroam-uid: abc123', { type: CONCEPT_TYPE.outlineNote })
    expect(out).toBe('title: X\nroam-uid: abc123\ntype: Outline Note')
  })

  it('leaves a non-mapping frontmatter untouched', () => {
    expect(touchConceptFrontmatter('- just\n- a list', { type: CONCEPT_TYPE.note })).toBe('- just\n- a list')
  })

  it('serializes the source/trust field families as real YAML structures', () => {
    const out = touchConceptFrontmatter(null, {
      type: CONCEPT_TYPE.book,
      title: '追忆似水年华',
      generated: { by: 'notemd/6.801.5', at: '2026-08-03T02:00:00Z' },
      sources: [{ id: 'orig', resource: '/books/proust.epub', author: 'Marcel Proust' }],
    })
    const parsed = parseYaml(out)
    expect(parsed.generated).toEqual({ by: 'notemd/6.801.5', at: '2026-08-03T02:00:00Z' })
    expect(parsed.sources[0].resource).toBe('/books/proust.epub')
  })

  it('round-trips a document that already carries OKF field families byte for byte', () => {
    const raw = [
      'type: Metric',
      'title: Income statement',
      'sources:',
      '  - id: fpa-handbook',
      '    resource: https://wiki.acme/finance',
      'verified: { by: human:bruce, at: 2026-06-25T09:00:00Z }',
    ].join('\n')
    expect(touchConceptFrontmatter(raw, { type: CONCEPT_TYPE.note, title: 'X' })).toBe(raw)
  })
})

describe('conceptFileText', () => {
  it('wraps frontmatter and body into a conformant document', () => {
    const text = conceptFileText({ type: CONCEPT_TYPE.note, title: '标题' }, '# 标题\n')
    expect(text).toBe('---\ntype: Note\ntitle: 标题\n---\n# 标题\n')
    expect(lintText('标题.md', text)).toEqual([])
  })
})
