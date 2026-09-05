// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'

const actions = vi.hoisted(() => ({ doSign: vi.fn(), doManualCreate: vi.fn() }))
vi.mock('../lib/store.svelte', () => actions)
import SignSheet from './SignSheet.svelte'

let app: ReturnType<typeof mount> | undefined
beforeEach(() => {
  // This plugin's older happy-dom returns -1 for every native control. Match
  // browser defaults so the shared modal's real tabIndex filtering is tested.
  vi.spyOn(HTMLElement.prototype, 'tabIndex', 'get').mockImplementation(function (this: HTMLElement) {
    const explicit = this.getAttribute('tabindex')
    return explicit === null && this.matches('input, button, textarea, select') ? 0 : Number(explicit ?? -1)
  })
})
afterEach(async () => {
  if (app) await unmount(app)
  app = undefined
  document.body.innerHTML = ''
  vi.clearAllMocks()
  vi.restoreAllMocks()
})

describe('sign sheet save and modal boundary', () => {
  it('names the dialog and does not dismiss an in-flight write with Escape or backdrop', async () => {
    let reject!: (reason: Error) => void
    actions.doSign.mockReturnValue(new Promise<void>((_, rejectPromise) => { reject = rejectPromise }))
    const onClose = vi.fn()
    app = mount(SignSheet, {
      target: document.body,
      props: { candidate: { id: 'candidate', title: 'A choice', prediction: 'A prediction', confidence: 0.75, prediction_source: 'quoted' }, onClose },
    })
    flushSync()
    await tick()
    expect(document.querySelector('[role="dialog"]')?.getAttribute('aria-labelledby')).toBe('sign-title')
    expect(document.activeElement).toBe(document.querySelector('.title-input'))
    document.querySelector<HTMLButtonElement>('.primary')!.click()
    flushSync()
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    document.querySelector<HTMLElement>('.overlay')!.click()
    expect(onClose).not.toHaveBeenCalled()
    expect(actions.doSign).toHaveBeenCalledOnce()
    reject(new Error('write refused'))
    await vi.waitFor(() => expect(document.querySelector('[role="alert"]')?.textContent).toContain('write refused'))
    expect(document.querySelector<HTMLButtonElement>('.primary')!.disabled).toBe(false)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    expect(onClose).toHaveBeenCalledOnce()
  })
})
