// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, unmount, flushSync } from 'svelte'
import { t } from '../lib/i18n/store.svelte'

const mocks = vi.hoisted(() => ({
  state: { configured: false, state: 'unconfigured', lastSync: null, errorMsg: null },
  configure: vi.fn(async () => {}), disconnect: vi.fn(async () => {}), sync: vi.fn(async () => {}),
  ask: vi.fn(async () => false), login: vi.fn(async () => null), refresh: vi.fn(async () => {}),
  toast: vi.fn(),
}))
vi.mock('../lib/vault.svelte', () => ({
  vaultStore: mocks.state, configureVault: mocks.configure, disconnectVault: mocks.disconnect,
  syncNow: mocks.sync, refreshStatus: vi.fn(async () => {}), fetchGitHubLogin: mocks.login,
}))
vi.mock('../lib/sotvault.svelte', () => ({ refreshSotvault: mocks.refresh }))
vi.mock('../lib/toast.svelte', () => ({ pushToast: mocks.toast }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ ask: mocks.ask }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn(async () => {}) }))

let app: ReturnType<typeof mount> | null = null
beforeEach(() => {
  vi.clearAllMocks()
  mocks.state.configured = false
  mocks.state.state = 'unconfigured'
  mocks.configure.mockResolvedValue(undefined)
  mocks.ask.mockResolvedValue(false)
  document.body.innerHTML = ''
})
afterEach(async () => {
  if (app) await unmount(app)
  app = null
  vi.useRealTimers()
  document.body.innerHTML = ''
})
async function setup() {
  const { default: VaultSettingsTab } = await import('./VaultSettingsTab.svelte')
  const onBusyChange = vi.fn()
  app = mount(VaultSettingsTab, { target: document.body, props: { onBusyChange } })
  await settle()
  return onBusyChange
}
async function settle() {
  flushSync()
  for (let i = 0; i < 6; i++) await Promise.resolve()
  if (!vi.isFakeTimers()) await vi.dynamicImportSettled()
  flushSync()
}
function input(label: string): HTMLInputElement {
  const field = Array.from(document.querySelectorAll<HTMLInputElement>('input')).find((element) => (
    element.getAttribute('aria-labelledby') ? document.getElementById(element.getAttribute('aria-labelledby')!)?.textContent === label
      : Array.from(element.labels ?? []).some((item) => item.textContent?.trim() === label)
  ))
  if (!field) throw new Error(`Missing labelled field: ${label}`)
  return field
}
function fill(field: HTMLInputElement, value: string) {
  field.value = value
  field.dispatchEvent(new Event('input', { bubbles: true }))
}
function submit() { document.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true })) }

describe('VaultSettingsTab — accessible settings form', () => {
  it('keeps submit and the Save button subject to the same required-field gates', async () => {
    await setup()
    submit()
    await settle()
    expect(mocks.configure).not.toHaveBeenCalled()
    fill(input(t('vault.remoteUrl')), 'https://github.com/test/notes.git')
    await settle()
    submit()
    await settle()
    expect(document.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(true)
    expect(mocks.configure).not.toHaveBeenCalled()
  })

  it('names every field, separates PAT actions from its label, and preserves the configuration payload', async () => {
    await setup()
    fill(input(t('vault.remoteUrl')), 'https://github.com/test/notes.git')
    fill(input(t('vault.pat')), 'github_pat_test')
    fill(input(t('vault.authorEmail')), 'owner@example.test')
    expect(input(t('vault.pat')).closest('label')).toBeNull()
    for (const label of ['vault.remoteUrl', 'vault.branch', 'vault.pat', 'vault.authorName', 'vault.authorEmail'] as const) {
      expect(input(t(label))).toBeTruthy()
    }
    await settle()
    submit()
    await settle()
    expect(mocks.configure).toHaveBeenCalledWith({
      remoteUrl: 'https://github.com/test/notes.git', branch: 'main', pat: 'github_pat_test',
      authorName: 'note.md on iOS', authorEmail: 'owner@example.test',
    })
    expect(mocks.refresh).toHaveBeenCalledOnce()
    expect(mocks.toast).toHaveBeenCalledWith(expect.objectContaining({ level: 'success' }))
  })

  it('keeps the typed fields and error visible after failure, and enables a retry', async () => {
    mocks.configure.mockRejectedValueOnce(new Error('network unavailable'))
    await setup()
    fill(input(t('vault.remoteUrl')), 'https://github.com/test/notes.git')
    fill(input(t('vault.pat')), 'github_pat_test')
    await settle()
    submit()
    await settle()
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('network unavailable')
    expect(input(t('vault.pat')).value).toBe('github_pat_test')
    expect(document.querySelector<HTMLButtonElement>('button[type="submit"]')!.disabled).toBe(false)
    expect(mocks.toast).not.toHaveBeenCalledWith(expect.objectContaining({ level: 'success' }))
    submit()
    await settle()
    expect(mocks.configure).toHaveBeenCalledTimes(2)
    expect(document.querySelector('[role="alert"]')).toBeNull()
  })

  it('announces busy to its modal parent and prevents duplicate form submission', async () => {
    let finish!: () => void
    mocks.configure.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve }))
    const onBusyChange = await setup()
    fill(input(t('vault.remoteUrl')), 'https://github.com/test/notes.git')
    fill(input(t('vault.pat')), 'github_pat_test')
    await settle()
    submit()
    await settle()
    expect(onBusyChange).toHaveBeenLastCalledWith(true)
    expect(document.querySelector('form')?.getAttribute('aria-busy')).toBe('true')
    expect(document.querySelector('fieldset')!.disabled).toBe(true)
    submit()
    expect(mocks.configure).toHaveBeenCalledTimes(1)
    finish()
    await settle()
    expect(onBusyChange).toHaveBeenLastCalledWith(false)
  })

  it('requires the existing confirmation before disconnecting a configured vault', async () => {
    mocks.state.configured = true
    mocks.state.state = 'idle'
    await setup()
    const disconnect = document.querySelector<HTMLButtonElement>('button.danger')!
    disconnect.click()
    await settle()
    expect(mocks.ask).toHaveBeenCalledOnce()
    expect(mocks.disconnect).not.toHaveBeenCalled()
    mocks.ask.mockResolvedValueOnce(true)
    disconnect.click()
    await settle()
    expect(mocks.disconnect).toHaveBeenCalledOnce()
  })

  it('cleans up deferred token lookup when the form unmounts', async () => {
    await setup()
    vi.useFakeTimers()
    fill(input(t('vault.pat')), 'github_pat_123456789012345678901234')
    await settle()
    await unmount(app!)
    app = null
    await vi.advanceTimersByTimeAsync(1000)
    expect(mocks.login).not.toHaveBeenCalled()
  })
})
