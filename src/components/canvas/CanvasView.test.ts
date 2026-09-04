// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import CanvasView from './CanvasView.svelte'
import type { Tab } from '../../lib/tabs.svelte'

const h = vi.hoisted(() => ({
  setContent: vi.fn(),
  openFile: vi.fn(async () => {}),
  storeGet: vi.fn(async () => null),
  storeSet: vi.fn(async () => {}),
  storeSave: vi.fn(async () => {}),
  invoke: vi.fn(),
  sotRoot: '/vault' as string | null,
  folderRoot: null as string | null,
}))

vi.mock('../../lib/tabs.svelte', () => ({
  setContent: h.setContent,
  openFile: h.openFile,
}))
vi.mock('../../lib/dialogs', () => ({ showError: vi.fn() }))
vi.mock('../../lib/sotvault.svelte', () => ({
  sotvaultStore: { get vaultRoot() { return h.sotRoot }, tick: 0 },
}))
vi.mock('../../lib/folder-view.svelte', () => ({
  folderView: { get rootDir() { return h.folderRoot } },
}))
vi.mock('../../lib/plugins/host-render-html', () => ({
  renderMarkdownInline: (markdown: string) => `<p>${markdown.replaceAll('&', '&amp;').replaceAll('<', '&lt;')}</p>`,
}))
vi.mock('@tauri-apps/api/core', () => ({ invoke: h.invoke }))
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: { load: vi.fn(async () => ({ get: h.storeGet, set: h.storeSet, save: h.storeSave })) },
}))

class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

const SAMPLE = JSON.stringify({
  nodes: [
    { id: 'text-1', type: 'text', text: '# 画布卡片', x: 0, y: 0, width: 260, height: 160, pluginField: 42 },
    { id: 'link-1', type: 'link', url: 'https://example.com/', x: 360, y: 0, width: 240, height: 140 },
  ],
  edges: [
    { id: 'edge-1', fromNode: 'text-1', fromSide: 'right', toNode: 'link-1', toSide: 'left', label: '参考' },
  ],
  topLevelExtension: { enabled: true },
})

function tab(): Tab {
  return {
    id: 'canvas-tab', filePath: '/vault/boards/demo.canvas', title: 'demo.canvas',
    initialContent: SAMPLE, currentContent: SAMPLE, mode: 'rich', kind: 'canvas',
    externalState: 'fresh', externalBannerDismissed: false,
    lastKnownMtime: 0, lastKnownHash: '',
  }
}

describe('CanvasView', () => {
  let component: ReturnType<typeof mount> | null = null

  beforeEach(() => {
    document.body.innerHTML = ''
    h.setContent.mockClear()
    h.invoke.mockReset()
    h.sotRoot = '/vault'
    h.folderRoot = null
    h.storeGet.mockResolvedValue(null)
    vi.stubGlobal('ResizeObserver', ResizeObserverStub)
  })

  afterEach(async () => {
    if (component) await unmount(component)
    component = null
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('renders standard nodes/edge and serializes a new text node without Flow state', async () => {
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await tick()
    await tick()
    flushSync()

    expect(document.querySelectorAll('.svelte-flow__node')).toHaveLength(2)
    await vi.waitFor(() => expect(document.body.textContent).toContain('# 画布卡片'))
    expect(document.body.textContent).toContain('example.com')

    const addText = Array.from(document.querySelectorAll('button'))
      .find((button) => button.textContent?.includes('文本')) as HTMLButtonElement
    addText.click()
    flushSync()

    const serialized = h.setContent.mock.calls.at(-1)?.[1] as string
    expect(serialized).toContain('"topLevelExtension"')
    expect(serialized).toContain('"pluginField"')
    expect(serialized).not.toContain('"selected"')
    expect(serialized).not.toContain('"viewport"')
    expect(JSON.parse(serialized).nodes).toHaveLength(3)
  })

  it('fails closed for malformed JSON and never rewrites the tab', async () => {
    const broken = tab()
    broken.currentContent = broken.initialContent = '{"nodes":['
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: broken },
    })
    await tick()
    expect(document.body.textContent).toContain('无法编辑这个画布')
    expect(h.setContent).not.toHaveBeenCalled()
  })

  it('uses a containing Folder View root when the Canvas is outside the SOT Vault', async () => {
    h.sotRoot = '/other-vault'
    h.folderRoot = '/workspace'
    h.invoke.mockResolvedValue(Uint8Array.from([1, 137, 80, 78, 71]).buffer)
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:group-background')
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {})
    const withBackground = tab()
    withBackground.filePath = '/workspace/boards/demo.canvas'
    withBackground.currentContent = withBackground.initialContent = JSON.stringify({
      nodes: [{
        id: 'group-1', type: 'group', x: 0, y: 0, width: 300, height: 200,
        label: '背景', background: 'assets/background.png', backgroundStyle: 'cover',
      }],
      edges: [],
    })

    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: withBackground },
    })

    await vi.waitFor(() => expect(h.invoke).toHaveBeenCalledWith('canvas_resource_read', {
      root: '/workspace', target: '/workspace/assets/background.png',
    }))
    await vi.waitFor(() => {
      const background = document.querySelector('.group-background') as HTMLElement | null
      expect(background?.style.backgroundImage).toContain('blob:group-background')
      expect(background?.classList.contains('cover')).toBe(true)
    })
  })

  it('imports an outside dropped file and stores the returned root-relative path', async () => {
    h.invoke.mockImplementation(async (command: string) => {
      if (command === 'canvas_resource_import') {
        return {
          relativePath: 'demo_files/archive.zip',
          canonicalPath: '/vault/boards/demo_files/archive.zip',
          size: 12,
        }
      }
      throw new Error(`unexpected command: ${command}`)
    })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await tick()
    window.dispatchEvent(new CustomEvent('notemd:canvas-native-drop', {
      detail: {
        tabId: 'canvas-tab', paths: ['/tmp/archive.zip'], position: { x: 100, y: 100 },
      },
    }))

    await vi.waitFor(() => expect(h.invoke).toHaveBeenCalledWith('canvas_resource_import', {
      root: '/vault', canvasPath: '/vault/boards/demo.canvas', sourcePath: '/tmp/archive.zip',
    }))
    await vi.waitFor(() => {
      const serialized = h.setContent.mock.calls.at(-1)?.[1] as string
      expect(JSON.parse(serialized).nodes).toEqual(expect.arrayContaining([
        expect.objectContaining({ type: 'file', file: 'demo_files/archive.zip' }),
      ]))
    })
  })

  it('resolves a file node through backend containment before opening it', async () => {
    const withFile = tab()
    withFile.currentContent = withFile.initialContent = JSON.stringify({
      nodes: [{
        id: 'file-1', type: 'file', file: 'assets/archive.canvas',
        x: 0, y: 0, width: 260, height: 160,
      }],
      edges: [],
    })
    h.invoke.mockResolvedValue({ canonicalPath: '/vault/assets/archive.canvas' })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: withFile },
    })
    await vi.waitFor(() => expect(document.querySelector('.canvas-card')).toBeTruthy())

    ;(document.querySelector('.canvas-card') as HTMLElement).dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))

    await vi.waitFor(() => expect(h.invoke).toHaveBeenCalledWith('canvas_resource_resolve', {
      root: '/vault', target: '/vault/assets/archive.canvas',
    }))
    await vi.waitFor(() => expect(h.openFile).toHaveBeenCalledWith('/vault/assets/archive.canvas'))
  })
})
