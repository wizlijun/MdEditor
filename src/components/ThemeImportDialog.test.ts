// @vitest-environment happy-dom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, unmount, flushSync } from 'svelte'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}))
vi.mock('../lib/toast.svelte', () => ({ pushToast: vi.fn() }))

beforeEach(() => {
  // vi.resetModules() intentionally omitted: resetting modules causes Svelte's internal
  // DOM operations.js singleton (first_child_getter) to be re-evaluated before the
  // happy-dom context re-runs init_operations(), producing a "Cannot read properties of
  // undefined (reading 'call')" crash on mount. clearAllMocks() is sufficient here
  // because the module-level vi.mock() factory persists correctly across tests.
  vi.clearAllMocks()
  document.body.innerHTML = ''
})

const sampleReport = {
  themes: [
    { id: 'claude-like', name: 'Claude-Like', appearance: 'light' as const, source_file: 'claude-like.css', conflict: false },
    { id: 'default',     name: 'Default',     appearance: 'light' as const, source_file: 'default.css',     conflict: true  },
  ],
  asset_dirs: ['claude-like'],
  errors: [{ file: 'broken.css', message: 'parse error' }],
  staging_dir: '/tmp/staging',
}

let extraApp: ReturnType<typeof mount> | null = null
afterEach(async () => { if (extraApp) await unmount(extraApp); extraApp = null })
async function mountForInteraction(onClose = vi.fn()) {
  const { default: ThemeImportDialog } = await import('./ThemeImportDialog.svelte')
  extraApp = mount(ThemeImportDialog, {
    target: document.body,
    props: { report: { ...sampleReport, themes: sampleReport.themes.map((theme) => ({ ...theme, conflict: false })) }, onClose },
  })
  flushSync()
  await Promise.resolve()
  return onClose
}
async function settleInteraction() {
  for (let i = 0; i < 5; i++) await Promise.resolve()
  flushSync()
}

describe('ThemeImportDialog — safe modal interaction', () => {
  it('keeps a failed installation open with an accessible error and a working retry', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('theme storage unavailable')).mockResolvedValue(2)
    const onClose = await mountForInteraction()
    const install = document.querySelector<HTMLButtonElement>('button.primary')!
    install.click()
    await settleInteraction()
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('theme storage unavailable')
    expect(onClose).not.toHaveBeenCalled()
    expect(install.disabled).toBe(false)
    install.click()
    await settleInteraction()
    expect(invoke).toHaveBeenCalledTimes(2)
    expect(onClose).toHaveBeenCalledOnce()
  })

  it('blocks Escape, backdrop cancellation, and duplicate installation while saving', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    let finish!: (value: number) => void
    vi.mocked(invoke).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve as (value: number) => void }))
    const onClose = await mountForInteraction()
    const dialog = document.querySelector('[role="dialog"]')!
    const install = dialog.querySelector<HTMLButtonElement>('button.primary')!
    install.click()
    await settleInteraction()
    expect(dialog.getAttribute('aria-busy')).toBe('true')
    expect(Array.from(dialog.querySelectorAll('button')).every((button) => button.disabled)).toBe(true)
    document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    document.querySelector<HTMLElement>('.overlay')!.click()
    install.click()
    await settleInteraction()
    expect(invoke).toHaveBeenCalledTimes(1)
    expect(onClose).not.toHaveBeenCalled()
    finish(2)
    await settleInteraction()
    expect(onClose).toHaveBeenCalledOnce()
  })

  it('reports cleanup failure but still lets cancellation leave the modal', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    const { pushToast } = await import('../lib/toast.svelte')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('staging cleanup failed'))
    const onClose = await mountForInteraction()
    document.querySelector<HTMLButtonElement>('.actions button')!.click()
    await settleInteraction()
    expect(pushToast).toHaveBeenCalledWith(expect.objectContaining({ level: 'error', detail: 'Error: staging cleanup failed' }))
    expect(onClose).toHaveBeenCalledOnce()
  })
})

describe('ThemeImportDialog', () => {
  it('renders theme names, conflict markers, asset dirs, and errors', async () => {
    const { default: ThemeImportDialog } = await import('./ThemeImportDialog.svelte')
    const app = mount(ThemeImportDialog as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { report: sampleReport, onClose: () => {} },
    })
    expect(document.body.textContent).toContain('Claude-Like')
    expect(document.body.textContent).toContain('Default')
    expect(document.body.textContent).toContain('will overwrite existing')
    expect(document.body.textContent).toContain('claude-like')   // asset dir
    expect(document.body.textContent).toContain('broken.css')    // error row
    expect(document.body.textContent).toContain('parse error')
    unmount(app)
  })

  it('requires overwrite checkbox when any theme is in conflict', async () => {
    const { default: ThemeImportDialog } = await import('./ThemeImportDialog.svelte')
    const app = mount(ThemeImportDialog as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { report: sampleReport, onClose: () => {} },
    })
    const btn = Array.from(document.body.querySelectorAll('button'))
      .find((b) => b.textContent?.includes('Import')) as HTMLButtonElement
    expect(btn.disabled).toBe(true)
    const cb = document.body.querySelector('input[type="checkbox"]') as HTMLInputElement
    cb.checked = true
    cb.dispatchEvent(new Event('change', { bubbles: true }))
    await new Promise((r) => setTimeout(r, 0))
    expect(btn.disabled).toBe(false)
    unmount(app)
  })

  it('invokes theme_install on confirm and calls onClose', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    ;(invoke as ReturnType<typeof vi.fn>).mockResolvedValue(2)
    const onClose = vi.fn()
    const noConflictReport = { ...sampleReport, themes: sampleReport.themes.map(t => ({ ...t, conflict: false })) }
    const { default: ThemeImportDialog } = await import('./ThemeImportDialog.svelte')
    const app = mount(ThemeImportDialog as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { report: noConflictReport, onClose },
    })
    const btn = Array.from(document.body.querySelectorAll('button'))
      .find((b) => b.textContent?.includes('Import')) as HTMLButtonElement
    btn.click()
    await new Promise((r) => setTimeout(r, 0))
    expect(invoke).toHaveBeenCalledWith('theme_install', expect.objectContaining({ report: expect.any(Object), overwrite: false }))
    expect(onClose).toHaveBeenCalled()
    unmount(app)
  })

  it('invokes theme_cancel_import on cancel', async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    ;(invoke as ReturnType<typeof vi.fn>).mockResolvedValue(undefined)
    const onClose = vi.fn()
    const { default: ThemeImportDialog } = await import('./ThemeImportDialog.svelte')
    const app = mount(ThemeImportDialog as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { report: sampleReport, onClose },
    })
    const btn = Array.from(document.body.querySelectorAll('button'))
      .find((b) => b.textContent?.includes('Cancel')) as HTMLButtonElement
    btn.click()
    await new Promise((r) => setTimeout(r, 0))
    expect(invoke).toHaveBeenCalledWith('theme_cancel_import', { stagingDir: '/tmp/staging' })
    expect(onClose).toHaveBeenCalled()
    unmount(app)
  })
})
