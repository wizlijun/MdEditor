import { describe, expect, it } from 'vitest'
import { buildTraceSeed } from './trace-action'

describe('buildTraceSeed', () => {
  it('引用块逐行加 >、vault 内路径转相对、首行是 /溯源', () => {
    const s = buildTraceSeed('两行\n选区', '/V/notes/a.md', '/V')
    expect(s.startsWith('/溯源 \n\n')).toBe(true)
    expect(s).toContain('> 两行\n> 选区')
    expect(s).toContain('\n\n源文档: notes/a.md\n')
  })

  it('vault 外文档保留绝对路径,vaultRoot 为 null 同理', () => {
    expect(buildTraceSeed('x', '/elsewhere/b.md', '/V')).toContain('源文档: /elsewhere/b.md')
    expect(buildTraceSeed('x', '/elsewhere/b.md', null)).toContain('源文档: /elsewhere/b.md')
  })

  it('超长选区截断到 8000 字符', () => {
    const s = buildTraceSeed('好'.repeat(9000), '/V/a.md', '/V')
    expect(s.length).toBeLessThan(8600)
    expect(s).toContain('选区过长已截断')
  })

  it('无路径时省略源文档行', () => {
    expect(buildTraceSeed('x', '', '/V')).not.toContain('源文档:')
  })
})
