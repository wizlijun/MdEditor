// src/lib/outline/create.test.ts
import { describe, it, expect } from 'vitest'
import { newOutlineFileText, newPageFileText } from './create'
import { CONCEPT_TYPE } from '../okf/concept'
// @ts-expect-error - plain-JS lint core shared with scripts/okf-lint.mjs
import { lintText } from '../../../scripts/okf-lint-core.mjs'

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
})
