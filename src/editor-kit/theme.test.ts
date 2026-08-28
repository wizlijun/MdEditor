/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { applyKitTheme, watchKitTheme } from './theme'

const onMessage = vi.fn()
let themeMessage: ((payload: unknown) => void) | null = null

function stubBridge(request: unknown) {
  ;(window as unknown as { notemd: unknown }).notemd = { request, onMessage }
}

beforeEach(() => {
  document.querySelectorAll('style[data-kit-theme]').forEach((n) => n.remove())
  onMessage.mockReset()
  themeMessage = null
  onMessage.mockImplementation((cb: (payload: unknown) => void) => {
    themeMessage = cb
  })
})

describe('applyKitTheme', () => {
  it('writes the light css into a single theme slot', async () => {
    stubBridge(vi.fn().mockResolvedValue({ light_css: '.a{}', dark_css: '.b{}', follow_system: false }))
    await applyKitTheme()
    await applyKitTheme()
    const slots = document.querySelectorAll('style[data-kit-theme]')
    expect(slots.length).toBe(1)
    expect(slots[0].textContent).toBe('.a{}')
  })

  it('empties the slot when the host cannot serve a theme', async () => {
    stubBridge(vi.fn().mockRejectedValue(new Error('-32001: capability denied')))
    await applyKitTheme()
    expect(document.querySelector('style[data-kit-theme]')!.textContent).toBe('')
  })

  it('empties the slot when there is no bridge at all', async () => {
    delete (window as unknown as { notemd?: unknown }).notemd
    await applyKitTheme()
    expect(document.querySelector('style[data-kit-theme]')!.textContent).toBe('')
  })
})

describe('watchKitTheme', () => {
  it('keeps an authoritative push that arrives while the initial RPC is pending', async () => {
    // The bridge's onMessage is push-only with no unsubscribe, and the theme
    // slot is window-level state: repeated mounts must not stack listeners.
    // A host push carries the authoritative in-memory bundle so the kit never
    // re-reads an older settings.json while the settings save is in flight.
    let resolveInitial!: (theme: { light_css: string; dark_css: string }) => void
    const request = vi.fn().mockReturnValue(new Promise((resolve) => { resolveInitial = resolve }))
    stubBridge(request)
    const mql = window.matchMedia('(prefers-color-scheme: dark)')
    const addSpy = vi.spyOn(Object.getPrototypeOf(mql), 'addEventListener')

    watchKitTheme()
    watchKitTheme()
    watchKitTheme()

    expect(onMessage).toHaveBeenCalledTimes(1)
    expect(addSpy).toHaveBeenCalledTimes(1)
    const initial = applyKitTheme()
    themeMessage?.({
      type: 'theme-changed',
      theme: { light_css: '.new{}', dark_css: '.new-dark{}', follow_system: false },
    })
    resolveInitial({ light_css: '.old{}', dark_css: '' })
    await initial
    expect(document.querySelector('style[data-kit-theme]')?.textContent).toBe('.new{}')
    expect(request).toHaveBeenCalledTimes(1)
    addSpy.mockRestore()
  })
})
