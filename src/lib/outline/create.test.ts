// src/lib/outline/create.test.ts
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { newOutlineFileText, newPageFileText, ensureOutlineFile } from './create'
import { CONCEPT_TYPE } from '../okf/concept'
// @ts-expect-error - plain-JS lint core shared with scripts/okf-lint.mjs
import { lintText } from '../../../scripts/okf-lint-core.mjs'

const exists = vi.fn()
const writeTextFile = vi.fn()
vi.mock('@tauri-apps/plugin-fs', () => ({
  exists: (...a: unknown[]) => exists(...a),
  writeTextFile: (...a: unknown[]) => writeTextFile(...a),
}))

const humanActor = vi.fn()
vi.mock('../okf/identity', () => ({
  humanActor: (...a: unknown[]) => humanActor(...a),
}))

describe('newOutlineFileText', () => {
  it('produces front-matter (title/created/updated) + one empty bullet', () => {
    const text = newOutlineFileText('我的笔记', '2026-07-10T09:00:00.000Z')
    expect(text.startsWith('---\n')).toBe(true)
    expect(text).toContain('title: 我的笔记')
    expect(text).toContain('created: 2026-07-10T09:00:00.000Z')
    expect(text).toContain('updated: 2026-07-10T09:00:00.000Z')
    expect(text.endsWith('---\n- \n') || text.endsWith('---\n-\n')).toBe(true)
  })
  it('newOutlineFileText keeps raw title even when filename would differ', () => {
    const text = newOutlineFileText('a/b 原始标题', '2026-07-10T09:00:00.000Z')
    expect(text).toContain('a/b 原始标题')
  })
  it('passes the OKF hard constraints', () => {
    const text = newOutlineFileText('我的笔记', '2026-07-10T09:00:00.000Z')
    expect(lintText('我的笔记.note.md', text)).toEqual([])
  })
  it('newPageFileText writes a conformant plain page (vault 外建页)', () => {
    const text = newPageFileText('某个概念')
    expect(text).toBe(`---\ntype: ${CONCEPT_TYPE.note}\ntitle: 某个概念\n---\n# 某个概念\n`)
    expect(lintText('某个概念.md', text)).toEqual([])
  })
  it('newPageFileText 带署名时写 generated', () => {
    const text = newPageFileText('某个概念', { by: 'human:bruce', at: '2026-08-20T10:31:00.000Z' })
    expect(text).toContain('generated:\n  by: human:bruce')
    expect(lintText('某个概念.md', text)).toEqual([])
  })
  it('保留名不因为署名而破例——index/log 仍然只写正文', () => {
    expect(newPageFileText('index', { by: 'human:bruce', at: '2026-08-20T10:31:00.000Z' }))
      .toBe('# index\n')
  })
  it('never stamps a reserved file name as a concept (§8/§9)', () => {
    // [[index]] 落到 vault 外会建 index.md —— 保留名不得用作概念文档,
    // 所以只写正文、不写 frontmatter,文件名保持用户看到的样子。
    expect(newPageFileText('index')).toBe('# index\n')
    expect(lintText('index.md', newPageFileText('index'))).toEqual([])
    expect(lintText('log.md', newPageFileText('log'))).toEqual([])
  })
  it('takes the concept type from the caller (daily notes are Daily Note)', () => {
    const text = newOutlineFileText('2026-07-10', '2026-07-10T09:00:00.000Z', CONCEPT_TYPE.dailyNote)
    expect(text).toContain(`type: ${CONCEPT_TYPE.dailyNote}`)
  })
  it('带署名时写 generated,且排在 updated 之前(只补缺失键,顺序即写入序)', () => {
    const text = newOutlineFileText('我的笔记', '2026-07-10T09:00:00.000Z', undefined, {
      by: 'human:bruce', at: '2026-07-10T09:00:00.000Z',
    })
    expect(text).toContain('generated:\n  by: human:bruce\n  at: 2026-07-10T09:00:00.000Z')
    expect(lintText('我的笔记.note.md', text)).toEqual([])
  })
  it('不带署名时逐字保持原样——旧行为零变化', () => {
    expect(newOutlineFileText('我的笔记', '2026-07-10T09:00:00.000Z'))
      .not.toContain('generated')
  })
})

describe('ensureOutlineFile', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    exists.mockResolvedValue(false)
    writeTextFile.mockResolvedValue(undefined)
  })

  it('signs a newly-created .note.md via humanActor() — wiring test, not just the primitive', async () => {
    // Drives the real ensureOutlineFile() call site (used by companion notes,
    // daily notes, and wikilink page creation) with a resolved identity and
    // asserts the signature reached the text actually handed to
    // writeTextFile() — catches a dropped `author` argument or a renamed
    // { by, at } shape that a hand-built-author unit test on
    // newOutlineFileText cannot.
    humanActor.mockResolvedValue('human:bruce')
    await ensureOutlineFile('/vault/notes/foo.note.md', '我的笔记')

    expect(writeTextFile).toHaveBeenCalledWith(
      '/vault/notes/foo.note.md',
      expect.stringContaining('generated:\n  by: human:bruce\n  at:'),
    )
  })

  it('writes no generated key when identity resolution fails', async () => {
    humanActor.mockRejectedValue(new Error('no identity'))
    await ensureOutlineFile('/vault/notes/foo.note.md', '我的笔记')

    expect(writeTextFile).toHaveBeenCalledWith(
      '/vault/notes/foo.note.md',
      expect.not.stringContaining('generated'),
    )
  })

  it('does not touch an existing file — no write, no signature to worry about', async () => {
    exists.mockResolvedValue(true)
    humanActor.mockResolvedValue('human:bruce')
    await ensureOutlineFile('/vault/notes/foo.note.md', '我的笔记')

    expect(writeTextFile).not.toHaveBeenCalled()
  })
})
