import { describe, expect, it, vi } from 'vitest'
import { seedTraceTemplate, TEMPLATE_VERSION, TRACE_TASK_FILES, TRACE_TASK_ID } from './trace-template'

const at = (suffix: string) => {
  const hit = Object.entries(TRACE_TASK_FILES).find(([p]) => p.endsWith(suffix))
  expect(hit, suffix).toBeTruthy()
  return hit![1]
}

describe('trace-source 模板', () => {
  it('五个文件齐全且都在模板目录下', () => {
    const paths = Object.keys(TRACE_TASK_FILES)
    expect(paths).toHaveLength(5)
    for (const p of paths) expect(p.startsWith(`.notemd/agent-tasks/${TRACE_TASK_ID}/`)).toBe(true)
  })

  it('task.json 可解析,带 template_version,无 directive 字段', () => {
    const t = JSON.parse(at('/task.json'))
    expect(t.template_version).toBe(TEMPLATE_VERSION)
    expect(t.directive).toBeUndefined()
    expect(t.okf_type).toBe('Trace Report')
    expect(t.timeout_seconds).toBe(2700)
    expect(t.precheck).toBe('precheck.sh')
  })

  it('settings 写权限按名字圈定 *-source-trace,目录随用户设置也不越界', () => {
    for (const f of ['/.claude/settings.json', '/.claude/settings.scoped.json']) {
      const s = JSON.parse(at(f))
      expect(s.permissions.allow).toContain('WebSearch')
      expect(s.permissions.allow).toContain('Bash(yt-dlp:*)')
      const writes = s.permissions.allow.filter(
        (a: string) => a.startsWith('Write(') || a.startsWith('Edit('),
      )
      expect(writes.length).toBeGreaterThan(0)
      // 报告目录是用户可改的设置,所以圈定靠文件名约定而非钉死目录——
      // 与 idea-proof 的 **/*.proof.md 同一手法。
      for (const w of writes) expect(w).toMatch(/-source-trace(\.md|\/\*\*)\)/)
      expect(s.permissions.deny).toContain('Task')
    }
  })

  it('CLAUDE.md 用语言中立的协议键 Source-Doc:/Output:,不再用中文键', () => {
    const md = at('/CLAUDE.md')
    expect(md).toContain('Output:')
    expect(md).toContain('Source-Doc:')
    // 旧中文协议键退场:英文界面的委托文本里不该冒出中文字段名,两侧必须一致。
    expect(md).not.toContain('输出:')
    expect(md).not.toContain('源文档:')
  })

  it('CLAUDE.md 含协议要件:source-trace 命名、降级、缘起、红线、委托稿只读', () => {
    const md = at('/CLAUDE.md')
    for (const must of [
      '-source-trace', 'yt-dlp', '未取到字幕', '缘起', 'Trace Material', '绝不',
      '00-request.md', // 委托稿在材料目录里,agent 不得改写、材料编号从 01 起
    ])
      expect(md, must).toContain(must)
    expect(md).toContain('${VAULT}') // 数组拼行没被 JS 求值
  })
})

describe('seedTraceTemplate 迁移', () => {
  function seedIo(existing: Record<string, string>) {
    return {
      exists: vi.fn(async (p: string) => p in existing),
      read: vi.fn(async (p: string) => {
        if (p in existing) return existing[p]
        throw new Error('missing')
      }),
      write: vi.fn(async (_path: string, _content: string) => {}),
    }
  }

  const TASK_PATH = `.notemd/agent-tasks/${TRACE_TASK_ID}/task.json`

  it('空 vault:全量写入', async () => {
    const io = seedIo({})
    await seedTraceTemplate(io)
    expect(io.write.mock.calls.map(([p]) => p).sort()).toEqual(Object.keys(TRACE_TASK_FILES).sort())
  })

  it('旧模板(无 template_version):整体覆写一次,不用用户手删', async () => {
    const io = seedIo({
      [TASK_PATH]: '{"name":"溯源","okf_type":"Trace Report"}',
      [`.notemd/agent-tasks/${TRACE_TASK_ID}/CLAUDE.md`]: '旧协议',
    })
    await seedTraceTemplate(io)
    expect(io.write.mock.calls.map(([p]) => p).sort()).toEqual(Object.keys(TRACE_TASK_FILES).sort())
  })

  it('当前版模板:存在即跳过,用户编辑不被覆盖', async () => {
    const io = seedIo({
      [TASK_PATH]: JSON.stringify({ name: '溯源', template_version: TEMPLATE_VERSION }),
      [`.notemd/agent-tasks/${TRACE_TASK_ID}/CLAUDE.md`]: '用户改过的提示词',
    })
    await seedTraceTemplate(io)
    const written = io.write.mock.calls.map(([p]) => p)
    expect(written).not.toContain(TASK_PATH)
    expect(written).not.toContain(`.notemd/agent-tasks/${TRACE_TASK_ID}/CLAUDE.md`)
    // 缺的文件(settings/precheck)仍然补上
    expect(written.length).toBe(Object.keys(TRACE_TASK_FILES).length - 2)
  })

  it('task.json 损坏按旧模板处理:覆写恢复', async () => {
    const io = seedIo({ [TASK_PATH]: '{broken' })
    await seedTraceTemplate(io)
    expect(io.write.mock.calls.map(([p]) => p)).toContain(TASK_PATH)
  })
})
