import { describe, it, expect } from 'vitest'
import { newFileText } from './new-file'
import { CONCEPT_TYPE } from './okf/concept'
// @ts-expect-error - plain-JS lint core shared with scripts/okf-lint.mjs
import { lintText } from '../../scripts/okf-lint-core.mjs'

describe('newFileText', () => {
  it('prepends OKF frontmatter taking the title from the H1', () => {
    expect(newFileText('# 火星上的第一家咖啡馆\n\n菜单很简单。\n'))
      .toBe(`---\ntype: ${CONCEPT_TYPE.note}\ntitle: 火星上的第一家咖啡馆\n---\n# 火星上的第一家咖啡馆\n\n菜单很简单。\n`)
  })

  it('produces a document that satisfies the OKF hard constraints', () => {
    expect(lintText('untitled.md', newFileText('# 标题\n\n正文\n'))).toEqual([])
  })

  it('falls back to no title when the body has no H1', () => {
    expect(newFileText('正文没有标题\n')).toBe(`---\ntype: ${CONCEPT_TYPE.note}\n---\n正文没有标题\n`)
  })

  it('leaves a body that already carries frontmatter alone', () => {
    const body = '---\ntype: Book\n---\n# X\n'
    expect(newFileText(body)).toBe(body)
  })

  it('带上人写署名时写进 generated(OKF §5.2/§7)', () => {
    const text = newFileText('# 标题\n\n正文\n', { by: 'human:bruce', at: '2026-08-20T10:31:00.000Z' })
    expect(text).toBe(
      `---\ntype: ${CONCEPT_TYPE.note}\ntitle: 标题\ngenerated:\n  by: human:bruce\n  at: 2026-08-20T10:31:00.000Z\n---\n# 标题\n\n正文\n`,
    )
  })

  it('署名后仍满足 OKF 硬约束', () => {
    const text = newFileText('# 标题\n\n正文\n', { by: 'human:bruce', at: '2026-08-20T10:31:00.000Z' })
    expect(lintText('untitled.md', text)).toEqual([])
  })

  it('拿不到身份就不签——宁可无署名,也不写一个假的', () => {
    expect(newFileText('# 标题\n', undefined))
      .toBe(`---\ntype: ${CONCEPT_TYPE.note}\ntitle: 标题\n---\n# 标题\n`)
  })
})
