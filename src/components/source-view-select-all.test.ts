/**
 * @vitest-environment happy-dom
 */
// Regression: the native Edit-menu "Select All" (PredefinedMenuItem) no-ops in
// rich mode (see src-tauri/src/lib.rs for why), so it was replaced with a
// custom menu item that broadcasts `mdeditor:select-all` — the same convention
// already used for `mdeditor:find-*`. This pins down SourceView's side: its
// listener must focus + select the whole textarea.
import { describe, it, expect, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => null), convertFileSrc: (s: string) => s }))
vi.mock('@tauri-apps/plugin-os', () => ({ platform: () => 'macos', type: () => 'macos' }))
vi.mock('@tauri-apps/plugin-fs', () => ({ exists: async () => false, readTextFile: async () => '', writeTextFile: async () => {} }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: async () => null, save: async () => null }))

const CONTENT = 'line one\nline two\nline three'

async function mountSourceView() {
  const { mount } = await import('svelte')
  const SourceView = (await import('./SourceView.svelte')).default
  const target = document.createElement('div')
  document.body.appendChild(target)
  mount(SourceView, { target, props: { value: CONTENT, oninput: () => {}, tabId: 'tab-1' } })
  await new Promise((r) => setTimeout(r, 20))
  return target
}

describe('SourceView select-all integration', () => {
  it('selects the entire textarea value on mdeditor:select-all', async () => {
    const target = await mountSourceView()
    const textarea = target.querySelector('textarea') as HTMLTextAreaElement
    // Start from an empty, un-focused selection so the assertion below can't
    // pass by accident.
    textarea.setSelectionRange(0, 0)
    textarea.blur()

    window.dispatchEvent(new CustomEvent('mdeditor:select-all'))

    expect(document.activeElement).toBe(textarea)
    expect(textarea.selectionStart).toBe(0)
    expect(textarea.selectionEnd).toBe(CONTENT.length)
  })
})
