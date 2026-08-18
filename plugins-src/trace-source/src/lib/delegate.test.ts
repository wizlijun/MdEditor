// The delegation call: what goes out on `host.agent.run`. Every assertion is
// about a contract that fails at the far end of the bridge and can't be caught
// by types (claude-agent rejects a notify spec missing any field; open_path /
// expect_file must be absolute; the Output: line is what makes expect_file
// predictable at all).
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { delegateTrace, traceOutputRel } from './delegate'
import { TRACE_TASK_FILES } from './trace-template'

// No jsdom here (nothing needs a DOM): stand `window` up by hand so the
// bridge's `window.notemd` and `globalThis.notemd` name the same property.
type Global = typeof globalThis & { notemd?: unknown }
const g = globalThis as Global
const withWindow = g as unknown as { window?: unknown }
withWindow.window ??= g

function useBridge(request: ReturnType<typeof vi.fn>): void {
  g.notemd = {
    pluginId: 'notemd.trace-source',
    locale: 'en',
    theme: 'default',
    request,
    onMessage: () => {},
  }
}

beforeEach(() => {
  delete g.notemd
})

describe('traceOutputRel', () => {
  it('时间戳-source-trace 定名,落在指定目录', () => {
    expect(traceOutputRel(new Date(2026, 7, 18, 14, 30, 12), 'inbox/traces')).toBe(
      'inbox/traces/2026-08-18-143012-source-trace.md',
    )
    expect(traceOutputRel(new Date(2026, 7, 18, 14, 30, 12), 'my/dir/')).toBe(
      'my/dir/2026-08-18-143012-source-trace.md',
    )
  })
})

describe('delegateTrace', () => {
  it('prompt=委托文本+Output 行,notify 指向摘要绝对路径,无 note_path', async () => {
    const request = vi.fn().mockResolvedValue({ run_id: 'r1' })
    useBridge(request)

    const r = await delegateTrace('只查论文\n\n> 引文', '/V', 'inbox/traces')

    expect(r.ok).toBe(true)
    const outRel = (r as { outRel: string }).outRel
    expect(outRel).toMatch(/^inbox\/traces\/\d{4}-\d{2}-\d{2}-\d{6}-source-trace\.md$/)
    const [method, params] = request.mock.calls.at(-1)!
    expect(method).toBe('host.agent.run')
    expect(params.task).toBe('trace-source')
    expect(params.prompt).toBe(`只查论文\n\n> 引文\n\nOutput: ${outRel}\n`)
    expect(params.note_path).toBeUndefined()
    expect(params.notify.open_path).toBe(`/V/${outRel}`)
    expect(params.notify.expect_file).toBe(`/V/${outRel}`)
    expect(Object.keys(params.notify).sort()).toEqual([
      'expect_file', 'open_path', 'title_fail', 'title_ok',
    ])
  })

  it('运行前先播种模板,顺序在 run 之前', async () => {
    const request = vi.fn(async (method: string) => {
      if (method === 'host.vault.exists') return { exists: false }
      if (method === 'host.vault.write') return { ok: true }
      return { run_id: 'r1' }
    })
    useBridge(request)

    await delegateTrace('x', '/V', 'inbox/traces')

    const methods = request.mock.calls.map(([m]) => m)
    const writes = methods.filter((m) => m === 'host.vault.write')
    expect(writes).toHaveLength(Object.keys(TRACE_TASK_FILES).length)
    expect(methods.lastIndexOf('host.vault.write')).toBeLessThan(methods.indexOf('host.agent.run'))
  })

  it('播种失败仍然发起运行', async () => {
    const request = vi.fn(async (method: string) => {
      if (
        method === 'host.vault.exists' ||
        method === 'host.vault.write' ||
        method === 'host.vault.read'
      )
        throw new Error('io: read-only')
      return { run_id: 'r1' }
    })
    useBridge(request)
    await expect(delegateTrace('x', '/V', 'inbox/traces')).resolves.toMatchObject({ ok: true, runId: 'r1' })
  })

  it('vault 根带尾斜杠不产生 //', async () => {
    const request = vi.fn().mockResolvedValue({ run_id: 'r1' })
    useBridge(request)
    const r = await delegateTrace('x', '/V/', 'inbox/traces')
    const [, params] = request.mock.calls.at(-1)!
    expect(params.notify.open_path).toBe(`/V/${(r as { outRel: string }).outRel}`)
  })

  it('agent_unavailable 前缀映射 agent-missing', async () => {
    const request = vi.fn(async (method: string) => {
      if (method === 'host.agent.run') throw new Error('-32000: agent_unavailable: not installed')
      return { exists: true }
    })
    useBridge(request)
    const r = await delegateTrace('x', '/V', 'inbox/traces')
    expect(r).toMatchObject({ ok: false, reason: 'agent-missing' })
  })

  it('没有 run id 的应答按错误报,绝不算已开始', async () => {
    useBridge(vi.fn().mockResolvedValue({}))
    const r = await delegateTrace('x', '/V', 'inbox/traces')
    expect(r).toMatchObject({ ok: false, reason: 'error' })
  })
})

describe('interpretStatus', () => {
  it('解读 running/done/lost 三种形状,未知形状按 lost', async () => {
    const { interpretStatus } = await import('./delegate')
    expect(interpretStatus({ state: 'running', steps: 3, last: 'Read a.md' })).toEqual({
      kind: 'running', steps: 3, last: 'Read a.md',
    })
    expect(interpretStatus({ state: 'running' })).toEqual({ kind: 'running', steps: 0, last: '' })
    expect(interpretStatus({ state: 'done', record: { status: 'success', result: 'ok' } })).toEqual({
      kind: 'done', success: true, message: 'ok',
    })
    expect(interpretStatus({ state: 'done', record: { status: 'error', stderr_tail: 'boom' } })).toEqual({
      kind: 'done', success: false, message: 'boom',
    })
    expect(interpretStatus({ state: 'done' })).toEqual({ kind: 'lost' })
    expect(interpretStatus(null)).toEqual({ kind: 'lost' })
    expect(interpretStatus({ nonsense: 1 })).toEqual({ kind: 'lost' })
  })
})
