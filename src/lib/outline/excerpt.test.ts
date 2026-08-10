import { describe, it, expect } from 'vitest'
import { appendExcerpt } from './excerpt'
import { parseOutline } from './markdown'
import { childrenOf } from './model'

const rootsOf = (text: string | null) => {
  if (text == null) throw new Error('expected an excerpt to be written')
  const tree = parseOutline(text)
  return childrenOf(tree, null)
}

describe('appendExcerpt', () => {
  it('writes the excerpt into an empty note as a manual node', () => {
    const out = appendExcerpt('', '这段话值得记下来')
    const roots = rootsOf(out)
    expect(roots.map(n => n.content)).toEqual(['这段话值得记下来'])
    // manual, not auto: mdx sources carry no marks, so nothing can re-derive
    // this node — an auto node would be dropped on the next sync.
    expect(roots[0].source).toBe('manual')
  })

  it('appends after existing nodes instead of replacing them', () => {
    const existing = appendExcerpt('', '第一条')!
    const out = appendExcerpt(existing, '第二条')
    expect(rootsOf(out).map(n => n.content)).toEqual(['第一条', '第二条'])
  })

  it('collapses a multi-line selection into one node', () => {
    const out = appendExcerpt('', 'first line\nsecond line')
    expect(rootsOf(out).map(n => n.content)).toEqual(['first line second line'])
  })

  it('keeps existing children of earlier excerpts', () => {
    const withChild = '- 原文一\n  - 我的判断\n'   // 两空格缩进 = 子节点(serializeOutline 的格式)
    const out = appendExcerpt(withChild, '原文二')!
    const tree = parseOutline(out)
    const roots = childrenOf(tree, null)
    expect(roots.map(n => n.content)).toEqual(['原文一', '原文二'])
    expect(childrenOf(tree, roots[0].id).map(n => n.content)).toEqual(['我的判断'])
  })

  it('refuses a blank selection rather than writing an empty node', () => {
    expect(appendExcerpt('- 原文\n', '   \n  ')).toBeNull()
  })
})
