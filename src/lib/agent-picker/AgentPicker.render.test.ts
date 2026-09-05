// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, unmount } from 'svelte'
import AgentPicker from './AgentPicker.svelte'

let component: ReturnType<typeof mount> | undefined
afterEach(async () => {
  if (component) await unmount(component)
  component = undefined
  document.body.innerHTML = ''
  vi.restoreAllMocks()
})

function mountPicker(selected = 'claude') {
  component = mount(AgentPicker, { target: document.body, props: {
    options: [{ id: 'claude', name: 'Claude' }, { id: 'codex', name: 'Codex' }],
    selected, onselect: () => {}, label: (key: string) => key,
  } })
  flushSync()
  return document.querySelector<HTMLButtonElement>('[aria-haspopup=menu]')!
}

async function openMenu(trigger: HTMLButtonElement) {
  trigger.focus()
  trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }))
  flushSync()
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
  return [...document.querySelectorAll<HTMLButtonElement>('.menu-panel .menu-row')]
}

describe('shared agent picker keyboard interaction', () => {
  it('uses the shared menu and returns focus after selecting or dismissing', async () => {
    let selected = ''
    component = mount(AgentPicker, { target: document.body, props: {
      options: [{ id: 'claude', name: 'Claude' }, { id: 'codex', name: 'Codex' }],
      selected: 'claude', onselect: (id: string) => { selected = id }, label: (key: string) => key,
    } })
    flushSync()
    const trigger = document.querySelector<HTMLButtonElement>('[aria-haspopup=menu]')!
    trigger.focus()
    trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }))
    flushSync()
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
    const items = [...document.querySelectorAll<HTMLButtonElement>('.menu-panel .menu-row')]
    expect(items).toHaveLength(2)
    expect(document.activeElement).toBe(items[0])
    items[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }))
    expect(document.activeElement).toBe(items[1])
    items[1].click()
    flushSync()
    expect(selected).toBe('codex')
    expect(document.activeElement).toBe(trigger)
    expect(document.querySelector('[role=menu]')).toBeNull()
    trigger.click()
    flushSync()
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    flushSync()
    expect(document.querySelector('[role=menu]')).toBeNull()
    expect(document.activeElement).toBe(trigger)
  })

  it('commits the measured, visible menu before trying to focus its selected provider', async () => {
    const nativeFocus = HTMLElement.prototype.focus
    const focusedVisibility: string[] = []
    vi.spyOn(HTMLElement.prototype, 'focus').mockImplementation(function (this: HTMLElement, options?: FocusOptions) {
      const menu = this.closest<HTMLElement>('[role=menu]')
      if (menu) focusedVisibility.push(menu.style.visibility)
      nativeFocus.call(this, options)
    })

    const items = await openMenu(mountPicker('codex'))
    // happy-dom permits focusing visibility:hidden nodes; verify the actual
    // DOM commit at the moment focus is invoked, not just its activeElement.
    expect(focusedVisibility).toEqual([''])
    expect(document.activeElement).toBe(items[1])
  })

  it('wraps arrow navigation and supports Home, End and Tab dismissal', async () => {
    const trigger = mountPicker()
    const items = await openMenu(trigger)
    const press = (key: string) => document.activeElement!.dispatchEvent(
      new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }),
    )
    press('ArrowUp')
    expect(document.activeElement).toBe(items[1])
    press('ArrowDown')
    expect(document.activeElement).toBe(items[0])
    press('End')
    expect(document.activeElement).toBe(items[1])
    press('Home')
    expect(document.activeElement).toBe(items[0])
    press('Tab')
    flushSync()
    expect(document.querySelector('[role=menu]')).toBeNull()
    expect(document.activeElement).toBe(trigger)
  })

  it('does not move focus when dismissed before the measurement frame', async () => {
    const trigger = mountPicker()
    trigger.focus()
    trigger.click()
    flushSync()
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }))
    flushSync()
    const next = document.createElement('button')
    document.body.append(next)
    next.focus()
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
    expect(document.querySelector('[role=menu]')).toBeNull()
    expect(document.activeElement).toBe(next)
  })
})
