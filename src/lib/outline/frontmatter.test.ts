// src/lib/outline/frontmatter.test.ts
import { describe, it, expect } from 'vitest'
import { touchFrontmatter, fmHas, outlineConceptType, signCompanionNoteText, signFrontmatterBlock } from './frontmatter'
import { CONCEPT_TYPE } from '../okf/concept'

const NOW = '2026-07-10T09:00:00.000Z'
const DIRS = { wikipage: 'wikipage', dailynote: 'dailynote' }

describe('touchFrontmatter', () => {
  it('builds full front-matter from null', () => {
    const out = touchFrontmatter(null, { title: '我的笔记', now: NOW })
    expect(out).toContain('title: 我的笔记')
    expect(out).toContain(`created: ${NOW}`)
    expect(out).toContain(`updated: ${NOW}`)
  })
  it('keeps existing title/created, refreshes updated, preserves unknown keys', () => {
    const raw = 'title: 旧标题\ncreated: 2020-01-01T00:00:00.000Z\nupdated: 2020-01-02T00:00:00.000Z\nroam-uid: abc'
    const out = touchFrontmatter(raw, { title: '新标题', now: NOW })
    expect(out).toContain('title: 旧标题')
    expect(out).toContain('created: 2020-01-01T00:00:00.000Z')
    expect(out).toContain(`updated: ${NOW}`)
    expect(out).toContain('roam-uid: abc')
    // 存量文件在下一次保存时机会性补上 OKF 必填的 type,追加在末尾以保留原顺序
    expect(out).toBe(`title: 旧标题\ncreated: 2020-01-01T00:00:00.000Z\nupdated: ${NOW}\nroam-uid: abc\ntype: ${CONCEPT_TYPE.outlineNote}`)
  })
  it('uses provided created fallback when missing', () => {
    const out = touchFrontmatter('title: t', { title: 't', created: '2019-05-05T00:00:00.000Z', now: NOW })
    expect(out).toContain('created: 2019-05-05T00:00:00.000Z')
  })
  it('appends missing keys at end, preserving existing key order', () => {
    const out = touchFrontmatter('roam-uid: abc', { title: 'T', now: NOW })
    expect(out).toBe(`roam-uid: abc\ntype: ${CONCEPT_TYPE.outlineNote}\ntitle: T\ncreated: ${NOW}\nupdated: ${NOW}`)
  })
  it('leaves non-mapping front-matter untouched (conservative)', () => {
    const raw = 'just some prose'
    expect(touchFrontmatter(raw, { title: 't', now: NOW })).toBe(raw)
  })
  it('stamps the OKF type first, defaulting to Outline Note', () => {
    const out = touchFrontmatter(null, { title: 'T', now: NOW })
    expect(out.split('\n')[0]).toBe(`type: ${CONCEPT_TYPE.outlineNote}`)
  })
  it('accepts an explicit concept type', () => {
    const out = touchFrontmatter(null, { title: '2026-07-10', type: CONCEPT_TYPE.dailyNote, now: NOW })
    expect(out).toContain(`type: ${CONCEPT_TYPE.dailyNote}`)
  })
  it('never rewrites a type the file already declares', () => {
    const out = touchFrontmatter('type: Book\ntitle: t', { title: 't', type: CONCEPT_TYPE.wikiPage, now: NOW })
    expect(out).toContain('type: Book')
    expect(out).not.toContain(CONCEPT_TYPE.wikiPage)
  })
  it('已有文件再 touch 不会长出 generated——只在创建时签', () => {
    const raw = 'type: Outline Note\ntitle: T\ncreated: 2026-01-01T00:00:00.000Z'
    const out = touchFrontmatter(raw, { title: 'T', now: '2026-08-20T10:00:00.000Z' })
    expect(out).not.toContain('generated')
  })
  it('已有 generated 的文件,再传一个署名也不覆盖', () => {
    const raw = 'type: Outline Note\ntitle: T\ngenerated:\n  by: claude-code/opus-5\n  at: 2026-01-01T00:00:00.000Z'
    const out = touchFrontmatter(raw, {
      title: 'T', now: '2026-08-20T10:00:00.000Z',
      generated: { by: 'human:bruce', at: '2026-08-20T10:00:00.000Z' },
    })
    expect(out).toContain('by: claude-code/opus-5')
    expect(out).not.toContain('human:bruce')
  })
})

describe('signCompanionNoteText', () => {
  // flushDisk() 在 OutlineEditor.svelte 里用 `!existed` 判定「首次落盘=创建」,
  // 只在那个分支调用这个纯函数;这里直接测函数本身,不发明测试专用 hook。
  const FIRST_WRITE = '---\ntype: Outline Note\ntitle: T\ncreated: 2026-01-01T00:00:00.000Z\nupdated: 2026-01-01T00:00:00.000Z\n---\n- \n'

  it('首次落盘且给了署名 —— 补 generated,body 原样保留', () => {
    const out = signCompanionNoteText(FIRST_WRITE, { by: 'human:bruce', at: '2026-01-01T00:00:00.000Z' })
    expect(out).toContain('generated:\n  by: human:bruce\n  at: 2026-01-01T00:00:00.000Z')
    expect(out.endsWith('---\n- \n')).toBe(true)
  })
  it('已经有 generated 的文本(比如再次落盘)——不覆盖、不新增', () => {
    const withGenerated = FIRST_WRITE.replace('created: 2026-01-01T00:00:00.000Z\n', 'created: 2026-01-01T00:00:00.000Z\ngenerated:\n  by: claude-code/opus-5\n  at: 2026-01-01T00:00:00.000Z\n')
    const out = signCompanionNoteText(withGenerated, { by: 'human:bruce', at: '2026-08-20T10:00:00.000Z' })
    expect(out).toBe(withGenerated)
  })
  it('没传署名(身份取不到)——原文一字不改', () => {
    expect(signCompanionNoteText(FIRST_WRITE)).toBe(FIRST_WRITE)
  })
  it('没有 frontmatter 的文本 —— 原样返回,不硬造一段', () => {
    const noFm = '- 只有正文\n'
    expect(signCompanionNoteText(noFm, { by: 'human:bruce', at: '2026-01-01T00:00:00.000Z' })).toBe(noFm)
  })
})

describe('signFrontmatterBlock', () => {
  // signCompanionNoteText 落盘用的是完整文本;signOutlineFrontmatterOnCreate
  // (store.svelte.ts)patch 的是 OutlineTree.frontmatter —— 同一种 raw 形状
  // (不含 --- 分隔符),两处 patch 逻辑必须是同一个函数,不能各写一份。
  const RAW = 'type: Outline Note\ntitle: T\ncreated: 2026-01-01T00:00:00.000Z'

  it('缺 generated 且给了署名 —— 补一条,其余字段不动', () => {
    const out = signFrontmatterBlock(RAW, { by: 'human:bruce', at: '2026-01-01T00:00:00.000Z' })
    expect(out).toContain('generated:\n  by: human:bruce\n  at: 2026-01-01T00:00:00.000Z')
    expect(out).toContain('type: Outline Note')
    expect(out).toContain('title: T')
  })
  it('已有 generated ——不覆盖', () => {
    const withGenerated = `${RAW}\ngenerated:\n  by: claude-code/opus-5\n  at: 2026-01-01T00:00:00.000Z`
    expect(signFrontmatterBlock(withGenerated, { by: 'human:bruce', at: '2026-08-20T10:00:00.000Z' })).toBe(withGenerated)
  })
  it('没传署名——原样返回', () => {
    expect(signFrontmatterBlock(RAW)).toBe(RAW)
  })
  it('raw 为 null——原样返回 null,不凭空造出一段 frontmatter', () => {
    expect(signFrontmatterBlock(null, { by: 'human:bruce', at: '2026-01-01T00:00:00.000Z' })).toBeNull()
  })
})

describe('outlineConceptType', () => {
  it('maps the dailynote directory to Daily Note', () => {
    expect(outlineConceptType('/v/dailynote/2026/2026-07-10.note.md', DIRS)).toBe(CONCEPT_TYPE.dailyNote)
  })
  it('maps the wikipage directory to Wiki Page', () => {
    expect(outlineConceptType('/v/wikipage/某个概念.note.md', DIRS)).toBe(CONCEPT_TYPE.wikiPage)
  })
  it('falls back to Outline Note for a companion note anywhere else', () => {
    expect(outlineConceptType('/v/notes/report.note.md', DIRS)).toBe(CONCEPT_TYPE.outlineNote)
  })
  it('honours renamed convention directories', () => {
    expect(outlineConceptType('/v/日记/2026/2026-07-10.note.md', { wikipage: 'wiki', dailynote: '日记' }))
      .toBe(CONCEPT_TYPE.dailyNote)
  })
  it('only matches a whole path segment', () => {
    expect(outlineConceptType('/v/my-dailynote-archive/x.note.md', DIRS)).toBe(CONCEPT_TYPE.outlineNote)
  })
})

describe('fmHas', () => {
  it('detects top-level keys', () => {
    expect(fmHas('title: x\ncreated: y', 'created')).toBe(true)
    expect(fmHas('title: x', 'created')).toBe(false)
    expect(fmHas(null, 'title')).toBe(false)
  })
})
