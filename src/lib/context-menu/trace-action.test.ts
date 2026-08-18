import { describe, expect, it } from 'vitest'
import { buildTraceSeed } from './trace-action'

describe('buildTraceSeed', () => {
  it('引用块逐行加 >、vault 内路径转相对,不再有 /溯源 前缀', () => {
    const s = buildTraceSeed('两行\n选区', '/V/notes/a.md', '/V')
    expect(s.startsWith('> 两行\n> 选区')).toBe(true)
    expect(s).not.toContain('/溯源')
    expect(s).toContain('\n\nSource-Doc: notes/a.md\n')
  })

  it('协议键语言中立:不出现中文的源文档字段名', () => {
    const s = buildTraceSeed('x', '/V/notes/a.md', '/V')
    expect(s).not.toContain('源文档')
  })

  it('vault 外文档保留绝对路径,vaultRoot 为 null 同理', () => {
    expect(buildTraceSeed('x', '/elsewhere/b.md', '/V')).toContain('Source-Doc: /elsewhere/b.md')
    expect(buildTraceSeed('x', '/elsewhere/b.md', null)).toContain('Source-Doc: /elsewhere/b.md')
  })

  it('超长选区截断到 8000 字符,截断标注语言中立', () => {
    const s = buildTraceSeed('好'.repeat(9000), '/V/a.md', '/V')
    expect(s.length).toBeLessThan(8600)
    expect(s).toContain('(selection truncated)')
  })

  it('无路径时省略 Source-Doc 行', () => {
    expect(buildTraceSeed('x', '', '/V')).not.toContain('Source-Doc:')
  })
})
