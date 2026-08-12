import { describe, it, expect } from 'vitest'
import { suggestGlobs } from './glob-suggest'

describe('suggestGlobs', () => {
  it('从窄到宽给出候选', () => {
    const s = suggestGlobs('ebook/三体/book.md')
    expect(s.map((x) => x.pattern)).toEqual(['ebook/三体/**', 'ebook/**/*.md', 'ebook/**'])
  })

  it('根目录下的文件不产出空目录段的模式', () => {
    expect(suggestGlobs('a.md').every((x) => !x.pattern.startsWith('/'))).toBe(true)
  })

  it('候选去重且始终非空', () => {
    // 单层路径下,「目录/**」(rung 1)与「目录/**」(rung 3, 同一顶层目录)会
    // 塌成同一条 —— rung 2(带扩展名过滤)与它们不同,因此去重后应剩两条。
    const s = suggestGlobs('clips/a.txt')
    expect(new Set(s.map((x) => x.pattern)).size).toBe(s.length)
    expect(s.length).toBeGreaterThan(0)
  })

  it('三层嵌套路径:rung1 用完整父目录,rung2/3 只用顶层目录', () => {
    const s = suggestGlobs('a/b/c/d.md')
    expect(s.map((x) => x.pattern)).toEqual(['a/b/c/**', 'a/**/*.md', 'a/**'])
  })

  it('无扩展名文件不产出带扩展名过滤的候选', () => {
    const s = suggestGlobs('clips/README')
    expect(s.map((x) => x.pattern)).toEqual(['clips/**'])
  })

  it('根目录下无扩展名文件仍产出非空候选且不以斜杠开头', () => {
    const s = suggestGlobs('README')
    expect(s.length).toBeGreaterThan(0)
    expect(s.every((x) => !x.pattern.startsWith('/'))).toBe(true)
  })

  it('反斜杠路径分隔符也能正确解析(Windows 粘贴场景)', () => {
    const s = suggestGlobs('ebook\\三体\\book.md')
    expect(s.map((x) => x.pattern)).toEqual(['ebook/三体/**', 'ebook/**/*.md', 'ebook/**'])
  })
})
