// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
const mocks = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: async () => () => {} }))
vi.mock('../../lib/i18n/store.svelte', () => ({ i18n: { locale: 'en' }, t: (key: string) => key }))
import ConsentModal from './ConsentModal.svelte'

let component: ReturnType<typeof mount> | undefined
afterEach(async () => { if (component) await unmount(component); component = undefined; document.body.innerHTML = ''; vi.resetAllMocks() })

describe('plugin permission sheet', () => {
  it('names the sheet, traps focus, and lets verification be retried without installing', async () => {
    const close = vi.fn()
    mocks.invoke.mockRejectedValueOnce(new Error('Network unavailable')).mockResolvedValue({ id: 'test', capabilities: ['vault.read'] })
    const opener = document.createElement('button'); document.body.append(opener); opener.focus()
    component = mount(ConsentModal, { target: document.body, props: { id: 'test', version: '1.0.0', name: 'Test', onClose: close, onInstalled: vi.fn() } })
    await vi.waitFor(() => expect(document.querySelector('[role=alert]')?.textContent).toContain('Network unavailable'))
    const dialog = document.querySelector<HTMLElement>('[role=dialog]')!
    expect(dialog.getAttribute('aria-labelledby')).toBe('plugin-consent-title')
    expect(dialog.contains(document.activeElement)).toBe(true)
    opener.focus()
    expect(dialog.contains(document.activeElement)).toBe(true)
    const retry = [...dialog.querySelectorAll('button')].find((button) => button.textContent === 'pluginMarket.refresh')!
    retry.click()
    await vi.waitFor(() => expect(document.querySelector('[role=alert]')).toBeNull())
    expect(mocks.invoke.mock.calls.map(([command]) => command)).toEqual(['plugin_market_preview', 'plugin_market_preview'])
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    expect(close).toHaveBeenCalledOnce()
    await unmount(component); component = undefined; await Promise.resolve()
    expect(document.activeElement).toBe(opener)
  })

  it('does not close or duplicate the install while installation is pending', async () => {
    const close = vi.fn()
    mocks.invoke.mockImplementation((method: string) => method === 'plugin_market_preview' ? Promise.resolve({ capabilities: [] }) : new Promise(() => {}))
    component = mount(ConsentModal, { target: document.body, props: { id: 'test', version: '1.0.0', name: 'Test', onClose: close, onInstalled: vi.fn() } })
    await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>('.primary')?.disabled).toBe(false))
    document.querySelector<HTMLButtonElement>('.primary')!.click(); flushSync()
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    document.querySelector<HTMLElement>('.overlay')!.click()
    expect(close).not.toHaveBeenCalled()
    expect(document.querySelector<HTMLButtonElement>('.primary')!.disabled).toBe(true)
  })
})
