// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'

const h = vi.hoisted(() => ({
  createEditor: vi.fn(),
  setBaseDir: vi.fn(),
  destroy: vi.fn(),
}))

vi.mock('@moraya/core', () => ({
  createEditor: h.createEditor,
  setDocumentBaseDir: h.setBaseDir,
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
import { mountRichEditor, updateDocumentBaseDir } from './editor-bridge'

function press(el: HTMLElement, key = 'Backspace') {
  const ev = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })
  el.dispatchEvent(ev)
  return ev
}

function makeCoreInstance(container: HTMLElement) {
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
}

beforeEach(() => {
  document.body.innerHTML = ''
  h.destroy.mockReset()
  h.setBaseDir.mockReset()
  h.createEditor.mockReset()
  h.createEditor.mockImplementation(async ({ container }: { container: HTMLElement }) => {
    return makeCoreInstance(container)
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

  it('串行挂载并为每个编辑器恢复其捕获的资源基目录', async () => {
    const firstHost = document.createElement('div')
    const secondHost = document.createElement('div')
    document.body.append(firstHost, secondHost)

    let releaseFirst: (() => void) | undefined
    const firstGate = new Promise<void>((resolve) => { releaseFirst = resolve })
    h.createEditor
      .mockImplementationOnce(async ({ container }: { container: HTMLElement }) => {
        await firstGate
        return makeCoreInstance(container)
      })
      .mockImplementationOnce(async ({ container }: { container: HTMLElement }) => {
        return makeCoreInstance(container)
      })

    updateDocumentBaseDir('/vault/one/note.md')
    const firstMount = mountRichEditor(firstHost, 'one', vi.fn())
    await vi.waitFor(() => expect(h.createEditor).toHaveBeenCalledTimes(1))

    updateDocumentBaseDir('/vault/two/note.md')
    const secondMount = mountRichEditor(secondHost, 'two', vi.fn())
    await Promise.resolve()
    expect(h.createEditor).toHaveBeenCalledTimes(1)

    releaseFirst?.()
    const first = await firstMount
    const second = await secondMount

    expect(h.setBaseDir.mock.calls).toContainEqual(['/vault/one'])
    expect(h.setBaseDir.mock.calls).toContainEqual(['/vault/two'])
    expect(h.createEditor).toHaveBeenCalledTimes(2)

    first.destroy()
    second.destroy()
  })

  it('允许 Canvas 为单个编辑器注入受限资源 resolver', async () => {
    const host = document.createElement('div')
    const restrictedResolver = {
      loadLocalImage: vi.fn(async () => ''),
      loadLocalMedia: vi.fn(async () => ''),
      loadRemoteMedia: vi.fn(async () => ''),
    }

    const editor = await mountRichEditor(host, '![x](x.png)', vi.fn(), undefined, restrictedResolver)

    expect(h.createEditor).toHaveBeenCalledWith(expect.objectContaining({
      mediaResolver: restrictedResolver,
    }))
    editor.destroy()
  })

  it('一次挂载失败不会阻塞后续编辑器', async () => {
    const firstHost = document.createElement('div')
    const secondHost = document.createElement('div')
    h.createEditor
      .mockRejectedValueOnce(new Error('mount failed'))
      .mockImplementationOnce(async ({ container }: { container: HTMLElement }) => (
        makeCoreInstance(container)
      ))

    await expect(mountRichEditor(firstHost, 'one', vi.fn())).rejects.toThrow('mount failed')
    await expect(mountRichEditor(secondHost, 'two', vi.fn())).resolves.toBeTruthy()
    expect(h.createEditor).toHaveBeenCalledTimes(2)
  })
})
