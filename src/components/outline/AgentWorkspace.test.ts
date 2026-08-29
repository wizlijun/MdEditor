// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'

const writeText = vi.fn()

vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText }))
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: { load: vi.fn(async () => ({ get: vi.fn(), set: vi.fn(), save: vi.fn() })) },
}))
vi.mock('../../lib/agent-workspace/store.svelte', () => ({
  activeProvider: () => 'notemd.test-agent',
  agentProviders: () => ['notemd.test-agent'],
  agentRun: {
    phase: 'idle', notePath: null, startedAt: null, steps: 0, last: '',
    outcome: null, message: '', artifacts: [], provider: null,
  },
  agentPluginAvailable: () => true,
  dismissRun: vi.fn(),
  harnessStatuses: {},
  isAgentBusy: () => false,
  refreshHarnesses: vi.fn(async () => {}),
  restoreProvider: vi.fn(),
  setProvider: vi.fn(),
  startNoteRun: vi.fn(),
}))

async function settle() {
  for (let i = 0; i < 5; i++) await Promise.resolve()
  flushSync()
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.clearAllMocks()
  document.body.innerHTML = ''
  writeText.mockResolvedValue(undefined)
})

afterEach(async () => {
  const { i18n } = await import('../../lib/i18n/store.svelte')
  i18n.locale = 'en'
  vi.useRealTimers()
})

describe('AgentWorkspace Copy context', () => {
  it('copies both full paths with clear roles and resets the success label', async () => {
    const { default: AgentWorkspace } = await import('./AgentWorkspace.svelte')
    const app = mount(AgentWorkspace as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: {
        sourcePath: '/Users/me/Vault/reading/source.md',
        notePath: '/Users/me/Vault/notes/source.note.md',
        onfinished: () => {},
      },
    })
    const button = document.body.querySelector<HTMLButtonElement>('.copy-context')!

    expect(button.textContent?.trim()).toBe('Copy context')
    button.click()
    await settle()

    expect(writeText).toHaveBeenCalledWith(
      '- Source document (primary content): /Users/me/Vault/reading/source.md\n' +
      '- Sidecar note (highlights, annotations, and questions): /Users/me/Vault/notes/source.note.md',
    )
    expect(button.textContent?.trim()).toBe('Copied')

    vi.advanceTimersByTime(1400)
    flushSync()
    expect(button.textContent?.trim()).toBe('Copy context')
    unmount(app)
  })

  it('does not claim success when the clipboard write fails', async () => {
    writeText.mockRejectedValue(new Error('clipboard unavailable'))
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const { default: AgentWorkspace } = await import('./AgentWorkspace.svelte')
    const app = mount(AgentWorkspace as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: {
        sourcePath: '/vault/source.md',
        notePath: '/vault/source.note.md',
        onfinished: () => {},
      },
    })
    const button = document.body.querySelector<HTMLButtonElement>('.copy-context')!

    button.click()
    await settle()

    expect(button.textContent?.trim()).toBe('Copy context')
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
    unmount(app)
  })

  it('localizes both the control and the copied context', async () => {
    const { i18n } = await import('../../lib/i18n/store.svelte')
    i18n.locale = 'zh'
    const { default: AgentWorkspace } = await import('./AgentWorkspace.svelte')
    const app = mount(AgentWorkspace as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: {
        sourcePath: '/资料/原文.md',
        notePath: '/资料/原文.note.md',
        onfinished: () => {},
      },
    })
    const button = document.body.querySelector<HTMLButtonElement>('.copy-context')!

    expect(button.textContent?.trim()).toBe('复制上下文')
    button.click()
    await settle()

    expect(writeText).toHaveBeenCalledWith(expect.stringContaining('源文档（正文内容）：/资料/原文.md'))
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining('手记文件（高亮、批注与待答问题）：/资料/原文.note.md'))
    expect(button.textContent?.trim()).toBe('已复制')

    unmount(app)
  })

  it('is disabled until both paths are available', async () => {
    const { default: AgentWorkspace } = await import('./AgentWorkspace.svelte')
    const app = mount(AgentWorkspace as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { sourcePath: '/vault/source.md', notePath: null, onfinished: () => {} },
    })

    expect(document.body.querySelector<HTMLButtonElement>('.copy-context')!.disabled).toBe(true)
    unmount(app)
  })
})
