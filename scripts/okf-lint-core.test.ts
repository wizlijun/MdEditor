import { describe, it, expect } from 'vitest'
// @ts-expect-error - plain-JS core shared with the CLI (see insights-report-core for the same pattern)
import { lintText, shouldIgnore } from './okf-lint-core.mjs'

const rules = (name: string, text: string, opts?: object): string[] =>
  lintText(name, text, opts).map((v: { rule: string }) => v.rule)

describe('okf lint — 硬约束 1/2:frontmatter 可解析且含非空 type', () => {
  it('passes a concept doc with a non-empty type', () => {
    expect(rules('orders.md', '---\ntype: BigQuery Table\ntitle: Orders\n---\n\n# Schema\n')).toEqual([])
  })

  it('flags a doc with no frontmatter at all', () => {
    expect(rules('note.md', '# Just a heading\n\nbody\n')).toEqual(['frontmatter-missing'])
  })

  it('flags frontmatter without a type key', () => {
    expect(rules('note.md', '---\ntitle: Orders\ncreated: 2026-08-03\n---\n\nbody\n')).toEqual(['type-missing'])
  })

  it('flags an empty type value', () => {
    expect(rules('note.md', '---\ntype:\ntitle: Orders\n---\n')).toEqual(['type-missing'])
  })

  it('flags unparsable YAML frontmatter', () => {
    expect(rules('note.md', '---\ntype: [unclosed\n---\n')).toEqual(['frontmatter-unparsable'])
  })

  it('recognises an empty frontmatter block instead of reading it as a rule', () => {
    // `---\n---` 是退化但合法的首部;当成正文分隔线会把整份文档判成"没有 frontmatter"
    expect(rules('note.md', '---\n---\n# body\n')).toEqual(['type-missing'])
  })

  it('accepts CRLF frontmatter', () => {
    expect(rules('note.md', '---\r\ntype: Note\r\n---\r\nbody\r\n')).toEqual([])
  })

  it('accepts extra unknown keys (§4.1 生产者 MAY 加任意键)', () => {
    expect(rules('note.md', '---\ntype: Note\nroam-uid: abc\nnested: { a: 1 }\n---\n')).toEqual([])
  })
})

describe('okf lint — 硬约束 3:保留文件名', () => {
  it('flags log.md used as a concept document', () => {
    expect(rules('log.md', '---\ntype: Note\n---\n# Log\n')).toEqual(['reserved-as-concept'])
  })

  it('accepts a plain log.md with no frontmatter', () => {
    expect(rules('log.md', '# Directory Update Log\n\n## 2026-05-22\n* **Update**: x\n')).toEqual([])
  })

  it('accepts a plain index.md with no frontmatter', () => {
    expect(rules('index.md', '# Section\n\n* [T](t.md) - d\n')).toEqual([])
  })

  it('flags a non-root index.md carrying frontmatter (§8)', () => {
    expect(rules('index.md', '---\nokf_version: "0.2"\n---\n# Section\n')).toEqual(['reserved-as-concept'])
  })

  it('accepts okf_version on the bundle-root index.md', () => {
    expect(rules('index.md', '---\nokf_version: "0.2"\n---\n# Section\n', { bundleRoot: true })).toEqual([])
  })

  it('flags any other key on the bundle-root index.md (§8:只允许 okf_version)', () => {
    expect(rules('index.md', '---\nokf_version: "0.2"\ntitle: Bundle\n---\n', { bundleRoot: true }))
      .toEqual(['index-extra-keys'])
  })
})

describe('okf lint — 报告内容', () => {
  it('reports the file name and a human-readable message', () => {
    const [v] = lintText('note.md', '# x\n')
    expect(v.file).toBe('note.md')
    expect(v.message).toMatch(/frontmatter/i)
  })
})

describe('shouldIgnore — 扫描时的排除规则', () => {
  it('matches a directory prefix', () => {
    expect(shouldIgnore('sync/foo.md', ['sync/'])).toBe(true)
    expect(shouldIgnore('sync/deep/foo.md', ['sync/'])).toBe(true)
    expect(shouldIgnore('notes/foo.md', ['sync/'])).toBe(false)
  })
  it('matches a * glob within one segment', () => {
    expect(shouldIgnore('stat/2026-08-04-daily-stat.md', ['stat/*-stat.md'])).toBe(true)
    expect(shouldIgnore('stat/a/b-stat.md', ['stat/*-stat.md'])).toBe(false)
  })
  it('matches ** across segments', () => {
    expect(shouldIgnore('a/b/c.md', ['a/**/c.md'])).toBe(true)
  })
  it('is false with no patterns', () => {
    expect(shouldIgnore('a.md', [])).toBe(false)
  })
})
