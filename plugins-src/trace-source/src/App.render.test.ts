// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'

const kit = vi.hoisted(() => {
  let markdown = ''
  let onChange: ((md: string) => void) | undefined
  return {
    mount: vi.fn(async (_container: HTMLElement, opts: { initialMarkdown: string; onChange?: (md: string) => void }) => {
      markdown = opts.initialMarkdown
      onChange = opts.onChange
      return {
        getMarkdown: () => markdown,
        setMarkdown: (md: string) => { markdown = md },
        getMode: () => 'rich' as const,
        setMode: vi.fn(async () => undefined),
        setPlaceholder: vi.fn(),
        focus: vi.fn(),
        destroy: vi.fn(),
      }
    }),
    edit(md: string) {
      markdown = md
      onChange?.(md)
    },
    markdown: () => markdown,
  }
})

vi.mock('./lib/editor-kit', () => ({ loadKit: async () => kit.mount }))

let component: ReturnType<typeof mount> | null = null

afterEach(() => {
  if (component) unmount(component)
  component = null
  document.body.innerHTML = ''
  kit.mount.mockClear()
})

async function settle(): Promise<void> {
  await Promise.resolve()
  await tick()
  flushSync()
}

describe('Trace Source inbox editing', () => {
  it('点击完成报告会在插件自身编辑区打开并写回,不会调用主编辑器', async () => {
    const name = '2026-08-18-143012-source-trace.md'
    const reportPath = `inbox/traces/${name}`
    const original = '---\ntype: Trace Report\ntitle: 测试报告\n---\n\n原文\n'
    let releaseFirstWrite!: () => void
    const firstWrite = new Promise<void>((resolve) => (releaseFirstWrite = resolve))
    let reportWrites = 0
    const request = vi.fn(async (method: string, params?: { path?: string; content?: string }) => {
      if (method === 'host.vault.info') return { root: '/vault', wiki_dir: null, daily_dir: null }
      if (method === 'host.vault.read' && params?.path === '.notemd/trace-source.json') {
        return { content: '{"traceDir":"inbox/traces","inboxOpen":true,"pendingRuns":{}}' }
      }
      if (method === 'host.vault.read' && params?.path === reportPath) return { content: original }
      if (method === 'host.vault.list' && params?.path === 'inbox/traces') {
        return { entries: [{ name, is_dir: false }] }
      }
      if (method === 'host.vault.write') {
        if (params?.path === reportPath && ++reportWrites === 1) await firstWrite
        return { ok: true }
      }
      if (method === 'host.vault.remove') throw new Error('cannot delete')
      if (method === 'host.agent.providers') return { providers: [] }
      throw new Error(`unexpected RPC: ${method} ${params?.path ?? ''}`)
    })
    window.notemd = { pluginId: 'notemd.trace-source', locale: 'zh', theme: 'system', request, onMessage: () => {} }

    const { default: App } = await import('./App.svelte')
    component = mount(App, { target: document.body })
    flushSync()
    await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>(`.row[title="${name}"]`)).toBeTruthy())

    document.querySelector<HTMLButtonElement>(`.row[title="${name}"]`)!.click()
    await vi.waitFor(() => expect(kit.markdown()).toBe(original))
    expect(request.mock.calls.some(([method]) => method === 'host.editor.open')).toBe(false)

    const firstEdit = `${original}\n较早的补充\n`
    kit.edit(firstEdit)
    await vi.waitFor(
      () => expect(request.mock.calls.filter(
        ([method, params]) => method === 'host.vault.write' && params?.path === reportPath,
      )).toHaveLength(1),
      { timeout: 2_000 },
    )

    const edited = `${original}\n保存期间继续输入的最新内容\n`
    kit.edit(edited)
    const newTrace = Array.from(document.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.textContent?.trim() === '新溯源')!
    newTrace.click()
    await settle()
    // The switch waits: it cannot replace the live buffer while an older
    // snapshot is still being written.
    expect(kit.markdown()).toBe(edited)
    releaseFirstWrite()
    await vi.waitFor(() => {
      expect(request.mock.calls).toContainEqual([
        'host.vault.write',
        { path: reportPath, content: edited },
      ])
    })
    await settle()
    expect(kit.markdown()).toBe('')

    // A failed delete keeps the open document and its latest editable buffer.
    document.querySelector<HTMLButtonElement>(`.row[title="${name}"]`)!.click()
    await vi.waitFor(() => expect(kit.markdown()).toBe(original))
    document.querySelector<HTMLButtonElement>(`.row[title="${name}"]`)!.dispatchEvent(
      new MouseEvent('contextmenu', { bubbles: true, clientX: 12, clientY: 12 }),
    )
    flushSync()
    Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'))
      .find((button) => button.textContent?.trim() === '删除')!
      .click()
    await vi.waitFor(() => expect(document.querySelector('.dialog .danger')).toBeTruthy())
    document.querySelector<HTMLButtonElement>('.dialog .danger')!.click()
    await vi.waitFor(() => expect(document.body.textContent).toContain('cannot delete'))
    expect(kit.markdown()).toBe(original)

    const closingEdit = `${original}\n关窗前的输入\n`
    kit.edit(closingEdit)
    window.dispatchEvent(new Event('beforeunload'))
    await vi.waitFor(() => expect(request.mock.calls).toContainEqual([
      'host.vault.write',
      { path: reportPath, content: closingEdit },
    ]))
  })

  it('点击开始溯源会先写带 human 署名的委托稿,再启动 agent', async () => {
    const request = vi.fn(async (method: string, params?: { path?: string; content?: string }) => {
      if (method === 'host.vault.info') {
        return { root: '/vault', wiki_dir: null, daily_dir: null, author: 'human:bruce' }
      }
      if (method === 'host.vault.read' && params?.path === '.notemd/trace-source.json') {
        throw new Error('first run')
      }
      if (method === 'host.agent.providers') return { providers: [] }
      if (method === 'host.vault.exists') return { exists: true }
      if (method === 'host.vault.write') return { ok: true }
      if (method === 'host.agent.run') return { run_id: 'run-1' }
      if (method === 'host.agent.status') return { state: 'running', steps: 0, last: '' }
      throw new Error(`unexpected RPC: ${method} ${params?.path ?? ''}`)
    })
    window.notemd = { pluginId: 'notemd.trace-source', locale: 'zh', theme: 'system', request, onMessage: () => {} }

    const { default: App } = await import('./App.svelte')
    component = mount(App, { target: document.body })
    flushSync()
    await vi.waitFor(() => expect(kit.mount).toHaveBeenCalled())

    kit.edit('> 这段话是谁说的\n')
    const delegate = Array.from(document.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.textContent?.trim() === '开始溯源')!
    delegate.click()

    await vi.waitFor(() => {
      expect(request.mock.calls.some(([method]) => method === 'host.agent.run')).toBe(true)
    })
    const requestWriteIndex = request.mock.calls.findIndex(
      ([method, params]) => method === 'host.vault.write' && params?.path?.endsWith('/00-request.md'),
    )
    const agentRunIndex = request.mock.calls.findIndex(([method]) => method === 'host.agent.run')
    expect(requestWriteIndex).toBeGreaterThan(-1)
    expect(requestWriteIndex).toBeLessThan(agentRunIndex)

    const [, written] = request.mock.calls[requestWriteIndex]!
    expect(written?.content).toContain('type: Trace Request')
    expect(written?.content).toContain('generated:\n  by: human:bruce\n  at: ')
    expect(written?.content).toContain('\n> 这段话是谁说的\n')
  })
})
