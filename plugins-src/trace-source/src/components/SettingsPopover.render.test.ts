// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import SettingsPopover from './SettingsPopover.svelte'

let app: ReturnType<typeof mount> | undefined
afterEach(async () => {
  if (app) await unmount(app)
  app = undefined
  document.body.innerHTML = ''
})

describe('settings modal keyboard and persistence feedback', () => {
  it('enters the labelled field, preserves focus, and does not submit an IME Enter', async () => {
    const trigger = document.createElement('button')
    document.body.append(trigger)
    trigger.focus()
    const onclose = vi.fn()
    const oncommit = vi.fn()
    app = mount(SettingsPopover, { target: document.body, props: { traceDir: 'inbox/traces', onclose, oncommit, oneditprompt: vi.fn() } })
    flushSync()
    await tick()
    const field = document.querySelector<HTMLInputElement>('#trace-dir')!
    expect(document.activeElement).toBe(field)
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', isComposing: true, bubbles: true }))
    expect(oncommit).not.toHaveBeenCalled()
    const last = document.querySelector<HTMLButtonElement>('.primary')!
    last.focus()
    last.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }))
    expect(document.activeElement).toBe(field)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    expect(onclose).toHaveBeenCalledOnce()
    await unmount(app)
    app = undefined
    await tick()
    expect(document.activeElement).toBe(trigger)
  })

  it('keeps an in-flight save open and leaves a rejected draft available to retry', async () => {
    let reject!: (reason: Error) => void
    const oncommit = vi.fn(() => new Promise<void>((_, rejectPromise) => { reject = rejectPromise }))
    const onclose = vi.fn()
    app = mount(SettingsPopover, { target: document.body, props: { traceDir: 'inbox/traces', onclose, oncommit, oneditprompt: vi.fn() } })
    flushSync()
    await tick()
    document.querySelector<HTMLButtonElement>('.primary')!.click()
    flushSync()
    document.querySelector<HTMLButtonElement>('.primary')!.click()
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    document.querySelector<HTMLElement>('.backdrop')!.click()
    expect(oncommit).toHaveBeenCalledOnce()
    expect(onclose).not.toHaveBeenCalled()
    expect(document.querySelector('[role="dialog"]')?.getAttribute('aria-busy')).toBe('true')
    reject(new Error('disk unavailable'))
    await vi.waitFor(() => expect(document.querySelector('[role="alert"]')?.textContent).toContain('disk unavailable'))
    expect(document.querySelector<HTMLInputElement>('#trace-dir')!.value).toBe('inbox/traces')
    expect(document.querySelector<HTMLButtonElement>('.primary')!.disabled).toBe(false)
    expect(onclose).not.toHaveBeenCalled()
  })
})
