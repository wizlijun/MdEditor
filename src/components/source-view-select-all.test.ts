/**
 * @vitest-environment happy-dom
 */
// Regression: the native Edit-menu "Select All" (PredefinedMenuItem) no-ops in
// rich mode (see src-tauri/src/lib.rs for why), so it was replaced with a
// custom menu item that broadcasts `notemd:select-all` — the same convention
// already used for `notemd:find-*`. This pins down SourceView's side: its
// listener must focus + select the whole textarea.
import { afterEach, beforeEach, describe, it, expect, vi } from 'vitest'

const platform = vi.hoisted(() => ({ apple: true }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => null), convertFileSrc: (s: string) => s }))
vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'macos', type: () => 'macos' }))
vi.mock('@tauri-apps/plugin-fs', () => ({ exists: async () => false, readTextFile: async () => '', writeTextFile: async () => {} }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: async () => null, save: async () => null }))
vi.mock('../lib/platform-sync', () => ({ isApplePlatformSync: () => platform.apple }))

const CONTENT = 'line one\nline two\nline three'
const cleanups: Array<() => void | Promise<void>> = []

beforeEach(() => {
  platform.apple = true
  document.body.innerHTML = ''
})

afterEach(async () => {
  await Promise.all(cleanups.splice(0).map((cleanup) => cleanup()))
})

async function mountSourceView() {
  const { mount, unmount } = await import('svelte')
  const SourceView = (await import('./SourceView.svelte')).default
  const target = document.createElement('div')
  document.body.appendChild(target)
  const component = mount(SourceView, { target, props: { value: CONTENT, oninput: () => {}, tabId: 'tab-1' } })
  cleanups.push(() => unmount(component))
  await new Promise((r) => setTimeout(r, 20))
  return target
}

describe('SourceView select-all integration', () => {
  it('handles Cmd+A on the main source textarea instead of relying on WebKit native selection', async () => {
    const target = await mountSourceView()
    const textarea = target.querySelector('textarea') as HTMLTextAreaElement
    const bubbled = vi.fn()
    window.addEventListener('keydown', bubbled)
    textarea.focus()
    textarea.setSelectionRange(4, 4)

    const event = new KeyboardEvent('keydown', {
      key: 'a', metaKey: true, bubbles: true, cancelable: true,
    })
    textarea.dispatchEvent(event)
    window.removeEventListener('keydown', bubbled)

    expect(event.defaultPrevented).toBe(true)
    expect(bubbled).not.toHaveBeenCalled()
    expect(textarea.selectionStart).toBe(0)
    expect(textarea.selectionEnd).toBe(CONTENT.length)
  })

  it('keeps Ctrl+A native on Apple platforms and handles it off Apple platforms', async () => {
    const target = await mountSourceView()
    const textarea = target.querySelector('textarea') as HTMLTextAreaElement
    textarea.focus()
    textarea.setSelectionRange(4, 4)

    const appleCtrl = new KeyboardEvent('keydown', {
      key: 'a', ctrlKey: true, bubbles: true, cancelable: true,
    })
    textarea.dispatchEvent(appleCtrl)
    expect(appleCtrl.defaultPrevented).toBe(false)
    expect(textarea.selectionStart).toBe(4)
    expect(textarea.selectionEnd).toBe(4)

    platform.apple = false
    const nonAppleCtrl = new KeyboardEvent('keydown', {
      key: 'A', ctrlKey: true, bubbles: true, cancelable: true,
    })
    textarea.dispatchEvent(nonAppleCtrl)
    expect(nonAppleCtrl.defaultPrevented).toBe(true)
    expect(textarea.selectionStart).toBe(0)
    expect(textarea.selectionEnd).toBe(CONTENT.length)
  })

  it('selects the entire textarea value on notemd:select-all', async () => {
    const target = await mountSourceView()
    const textarea = target.querySelector('textarea') as HTMLTextAreaElement
    // Start from an empty, un-focused selection so the assertion below can't
    // pass by accident.
    textarea.setSelectionRange(0, 0)
    textarea.blur()

    window.dispatchEvent(new CustomEvent('notemd:select-all'))

    expect(document.activeElement).toBe(textarea)
    expect(textarea.selectionStart).toBe(0)
    expect(textarea.selectionEnd).toBe(CONTENT.length)
  })
})
