// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'

const h = vi.hoisted(() => ({
  createEditor: vi.fn(),
  destroy: vi.fn(),
}))

vi.mock('@moraya/core', () => ({
  createEditor: h.createEditor,
  setDocumentBaseDir: vi.fn(),
}))
vi.mock('./adapters/tauri-media-resolver', () => ({ tauriMediaResolver: {} }))
vi.mock('./adapters/tauri-link-opener', () => ({ tauriLinkOpener: {} }))
vi.mock('./adapters/renderer-registry', () => ({ rendererRegistry: {} }))
vi.mock('./adapters/spreadsheet-factory', () => ({ spreadsheetFactory: {} }))
vi.mock('./frontmatter-view', () => ({ frontmatterFactory: {} }))
vi.mock('./tabs.svelte', () => ({ activeTab: vi.fn(() => null) }))
vi.mock('./platform-sync', () => ({ isApplePlatformSync: vi.fn(() => true) }))
vi.mock('./insights/tracker.svelte', () => ({ analyticsPluginForEditor: vi.fn(() => ({})) }))

import { createImeGuard } from './ime'
import { mountRichEditor } from './editor-bridge'

function press(el: HTMLElement, key = 'Backspace') {
  const ev = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })
  el.dispatchEvent(ev)
  return ev
}

beforeEach(() => {
  document.body.innerHTML = ''
  h.destroy.mockReset()
  h.createEditor.mockImplementation(async ({ container }: { container: HTMLElement }) => {
    const pm = document.createElement('div')
    pm.className = 'ProseMirror moraya-editor'
    pm.contentEditable = 'true'
    container.appendChild(pm)
    const state = {
      plugins: [],
      reconfigure: vi.fn(() => state),
    }
    return {
      view: { state, updateState: vi.fn() },
      getMarkdown: () => '',
      setContent: vi.fn(),
      destroy: h.destroy,
    }
  })
})

describe('mountRichEditor — 主 Rich 工厂自带同一套 IME 保护', () => {
  it('复用调用方 guard，取消收尾退格并随编辑器销毁', async () => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    const guard = createImeGuard()
    const editor = await mountRichEditor(host, '已确认', vi.fn(), guard)
    const pm = host.querySelector('.ProseMirror') as HTMLElement

    pm.dispatchEvent(new Event('compositionstart', { bubbles: true }))
    expect(guard.blocks(new KeyboardEvent('keydown', { key: 'Backspace' }))).toBe(true)
    pm.dispatchEvent(new Event('compositionend', { bubbles: true }))
    expect(press(pm).defaultPrevented).toBe(true)
    expect(press(pm).defaultPrevented, '正常的下一次退格仍应交给编辑器').toBe(false)

    editor.destroy()
    pm.dispatchEvent(new Event('compositionstart', { bubbles: true }))
    pm.dispatchEvent(new Event('compositionend', { bubbles: true }))
    expect(press(pm).defaultPrevented).toBe(false)
    expect(h.destroy).toHaveBeenCalledTimes(1)
  })
})
