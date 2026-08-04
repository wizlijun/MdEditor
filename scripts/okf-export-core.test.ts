import { describe, it, expect } from 'vitest'
// @ts-expect-error - plain-JS core shared with scripts/okf-export.mjs
import { rewriteLinks, buildIndex, buildLog, stampConcept, bundleIndexHead } from './okf-export-core.mjs'
// @ts-expect-error - plain-JS lint core
import { lintText } from './okf-lint-core.mjs'

const index = new Map([
  ['某个概念', 'wikipage/某个概念.md'],
  ['Orders', 'tables/orders.md'],
])

describe('rewriteLinks — wikilink → OKF 的 Markdown 链接(§6)', () => {
  it('rewrites a resolved wikilink to a bundle-absolute link', () => {
    expect(rewriteLinks('见 [[某个概念]] 一节', index))
      .toBe('见 [某个概念](/wikipage/某个概念.md) 一节')
  })

  it('keeps the alias as the link text', () => {
    expect(rewriteLinks('见 [[Orders|订单表]]', index)).toBe('见 [订单表](/tables/orders.md)')
  })

  it('degrades an unresolved wikilink to plain text rather than a dead link', () => {
    expect(rewriteLinks('见 [[不存在的页]]', index)).toBe('见 不存在的页')
  })

  it('leaves an ordinary markdown link alone', () => {
    expect(rewriteLinks('[x](/a.md) and `[[code]]`', index)).toBe('[x](/a.md) and `[[code]]`')
  })

  it('leaves a block reference alone — it has no link form', () => {
    expect(rewriteLinks('见 ((abc123))', index)).toBe('见 ((abc123))')
  })
})

describe('stampConcept — 导出副本一定合规', () => {
  it('adds type to a document that has none', () => {
    const out = stampConcept('# 标题\n正文\n', 'notes/x.md')
    expect(lintText('notes/x.md', out)).toEqual([])
    expect(out).toContain('type: Note')
    expect(out).toContain('title: 标题')
  })

  it('never rewrites a type the document already declares', () => {
    const src = '---\ntype: Book\ntitle: t\n---\n正文\n'
    expect(stampConcept(src, 'notes/x.md')).toBe(src)
  })

  it('types a companion note as an Outline Note', () => {
    expect(stampConcept('- 一条\n', 'notes/x.note.md')).toContain('type: Outline Note')
  })
})

describe('buildIndex / buildLog(§8/§9)', () => {
  it('groups index entries by directory, newest section first', () => {
    const md = buildIndex([
      { rel: 'tables/orders.md', title: 'Orders', description: 'One row per order.' },
      { rel: 'note.md', title: 'Note', description: '' },
    ])
    expect(md).toContain('* [Note](note.md)')
    expect(md).toContain('* [Orders](tables/orders.md) - One row per order.')
    // 保留名自身不得作为概念文档被索引
    expect(md).not.toContain('index.md')
  })

  it('writes the log newest-first with ISO date headings', () => {
    const md = buildLog([
      { date: '2026-05-22', subject: 'Added the orders table' },
      { date: '2026-05-15', subject: 'Created the structure' },
    ])
    expect(md.indexOf('## 2026-05-22')).toBeLessThan(md.indexOf('## 2026-05-15'))
    expect(md).toContain('* Added the orders table')
    expect(lintText('log.md', md)).toEqual([])
  })

  it('declares okf_version and nothing else on the bundle-root index', () => {
    const md = bundleIndexHead() + buildIndex([])
    expect(lintText('index.md', md, { bundleRoot: true })).toEqual([])
    expect(md.startsWith('---\nokf_version: "0.2"\n---\n')).toBe(true)
  })
})
