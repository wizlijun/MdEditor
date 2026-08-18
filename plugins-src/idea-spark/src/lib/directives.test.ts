import { describe, expect, it } from 'vitest'
import { parseDirectiveInput, discoverDirectives, matchDirective, type DirectiveIo } from './directives'

describe('parseDirectiveInput', () => {
  it('取首 token 为指令名,rest 保留其余全部(含换行引用块)', () => {
    const r = parseDirectiveInput('/溯源 只查论文\n\n> 引文\n\n源文档: a.md\n')!
    expect(r.name).toBe('溯源')
    expect(r.rest).toBe('只查论文\n\n> 引文\n\n源文档: a.md')
  })

  it('非 / 开头、纯 "/"、空文本都返回 null', () => {
    expect(parseDirectiveInput('普通 idea')).toBeNull()
    expect(parseDirectiveInput('/')).toBeNull()
    expect(parseDirectiveInput('  ')).toBeNull()
  })

  it('容忍名后直接换行', () => {
    expect(parseDirectiveInput('/trace\n> q')!.name).toBe('trace')
  })
})

function io(tasks: Record<string, string | null>): DirectiveIo {
  return {
    list: async (p) => {
      if (p !== '.notemd/agent-tasks') throw new Error('io: unexpected ' + p)
      return { entries: Object.keys(tasks).map((name) => ({ name, is_dir: true })) }
    },
    read: async (p) => {
      const id = p.split('/')[2]
      const body = tasks[id]
      if (body == null) throw new Error('io: missing')
      return { content: body }
    },
  }
}

describe('discoverDirectives', () => {
  it('只收 directive 非空的模板,坏 json/缺文件跳过', async () => {
    const got = await discoverDirectives(io({
      'trace-source': '{"name":"溯源","description":"找出处","directive":["溯源","trace"]}',
      'idea-proof': '{"name":"Idea proof","prompt":"p"}',
      broken: '{not json',
      missing: null,
    }))
    expect(got).toEqual([
      { taskId: 'trace-source', names: ['溯源', 'trace'], display: '溯源', description: '找出处' },
    ])
  })

  it('agent-tasks 目录不存在 → 空表', async () => {
    const bad: DirectiveIo = {
      list: async () => { throw new Error('io: no dir') },
      read: async () => ({ content: '' }),
    }
    expect(await discoverDirectives(bad)).toEqual([])
  })
})

describe('matchDirective', () => {
  const entries = [{ taskId: 't', names: ['溯源', 'trace'], display: '溯源', description: '' }]
  it('任一名字精确命中,未知返回 null', () => {
    expect(matchDirective(entries, 'trace')?.taskId).toBe('t')
    expect(matchDirective(entries, '溯源')?.taskId).toBe('t')
    expect(matchDirective(entries, 'xx')).toBeNull()
  })
})
