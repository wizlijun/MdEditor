/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { applyKitTheme, watchKitTheme } from './theme'

const onMessage = vi.fn()

function stubBridge(request: unknown) {
  ;(window as unknown as { notemd: unknown }).notemd = { request, onMessage }
}

beforeEach(() => {
  document.querySelectorAll('style[data-kit-theme]').forEach((n) => n.remove())
  onMessage.mockClear()
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
  it('registers host-push and colour-scheme listeners only once per window', () => {
    // The bridge's onMessage is push-only with no unsubscribe, and the theme
    // slot is window-level state: repeated mounts must not stack listeners.
    stubBridge(vi.fn().mockResolvedValue({ light_css: '', dark_css: '' }))
    const mql = window.matchMedia('(prefers-color-scheme: dark)')
    const addSpy = vi.spyOn(Object.getPrototypeOf(mql), 'addEventListener')

    watchKitTheme()
    watchKitTheme()
    watchKitTheme()

    expect(onMessage).toHaveBeenCalledTimes(1)
    expect(addSpy).toHaveBeenCalledTimes(1)
    addSpy.mockRestore()
  })
})
