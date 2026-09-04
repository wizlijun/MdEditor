// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'
import CanvasView from './CanvasView.svelte'
import type { Tab } from '../../lib/tabs.svelte'
import { clearCanvasUiSessions } from '../../lib/canvas/session'
import { formFactor } from '../../lib/platform.svelte'
import type { CanvasViewportState } from './canvas-view-state'

const h = vi.hoisted(() => ({
  setContent: vi.fn(),
  openFile: vi.fn(async () => {}),
  storeGet: vi.fn(async (): Promise<CanvasViewportState | null> => null),
  storeSet: vi.fn(async () => {}),
  storeSave: vi.fn(async () => {}),
  invoke: vi.fn(),
  clipboardRead: vi.fn(async () => ''),
  clipboardWrite: vi.fn(async (_text: string) => {}),
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
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({
  readText: h.clipboardRead,
  writeText: h.clipboardWrite,
}))
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: { load: vi.fn(async () => ({ get: h.storeGet, set: h.storeSet, save: h.storeSave })) },
}))

class ResizeObserverStub {
  private readonly targets = new Set<Element>()

  constructor(private readonly callback: ResizeObserverCallback) {}

  observe(target: Element): void {
    this.targets.add(target)
    const element = target as HTMLElement
    const width = Number.parseFloat(element.style.width) || 100
    const height = Number.parseFloat(element.style.height) || 100
    Object.defineProperties(element, {
      offsetWidth: { configurable: true, value: width },
      offsetHeight: { configurable: true, value: height },
    })
    const contentRect = {
      x: 0, y: 0, top: 0, left: 0, right: width, bottom: height,
      width, height, toJSON: () => ({}),
    } as DOMRect
    element.getBoundingClientRect = () => contentRect
    for (const handle of element.querySelectorAll<HTMLElement>('.source, .target')) {
      Object.defineProperties(handle, {
        offsetWidth: { configurable: true, value: 8 },
        offsetHeight: { configurable: true, value: 8 },
      })
      handle.getBoundingClientRect = () => ({
        x: 0, y: 0, top: 0, left: 0, right: 8, bottom: 8,
        width: 8, height: 8, toJSON: () => ({}),
      }) as DOMRect
    }
    queueMicrotask(() => {
      if (this.targets.has(target) && target.isConnected) {
        this.callback([{ target, contentRect } as ResizeObserverEntry], this as unknown as ResizeObserver)
      }
    })
  }

  unobserve(target: Element): void {
    this.targets.delete(target)
  }

  disconnect(): void {
    this.targets.clear()
  }
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
    h.clipboardRead.mockReset()
    h.clipboardRead.mockResolvedValue('')
    h.clipboardWrite.mockReset()
    h.clipboardWrite.mockResolvedValue(undefined)
    clearCanvasUiSessions()
    h.sotRoot = '/vault'
    h.folderRoot = null
    h.storeGet.mockResolvedValue(null)
    formFactor.value = 'desktop'
    vi.stubGlobal('ResizeObserver', ResizeObserverStub)
  })

  afterEach(async () => {
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())))
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

  it('reactively enables undo and redo toolbar actions', async () => {
    vi.stubGlobal('prompt', vi.fn(() => '空分组'))
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.canvas-toolbar')).toBeTruthy())

    let undo = document.querySelector('button[aria-label^="撤销"]') as HTMLButtonElement
    let redo = document.querySelector('button[aria-label^="重做"]') as HTMLButtonElement
    expect(undo.disabled).toBe(true)
    expect(redo.disabled).toBe(true)

    ;(Array.from(document.querySelectorAll('.canvas-toolbar > button'))
      .find((button) => button.textContent?.includes('分组')) as HTMLButtonElement).click()
    await tick()
    flushSync()

    undo = document.querySelector('button[aria-label^="撤销"]') as HTMLButtonElement
    redo = document.querySelector('button[aria-label^="重做"]') as HTMLButtonElement
    expect(undo.disabled).toBe(false)
    expect(undo.title).toContain('创建分组节点')
    expect(redo.disabled).toBe(true)

    undo.click()
    await tick()
    expect(JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes).toHaveLength(2)
    expect((document.querySelector('button[aria-label^="撤销"]') as HTMLButtonElement).disabled).toBe(true)
    redo = document.querySelector('button[aria-label^="重做"]') as HTMLButtonElement
    expect(redo.disabled).toBe(false)

    redo.click()
    await tick()
    expect(JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes).toHaveLength(3)
  })

  it('creates a geometric group around the selected node', async () => {
    vi.stubGlobal('prompt', vi.fn(() => '重点'))
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelectorAll('.svelte-flow__node')).toHaveLength(2))

    ;(document.querySelector('[data-id="text-1"]') as HTMLElement).click()
    await tick()
    const addGroup = Array.from(document.querySelectorAll('.canvas-toolbar > button'))
      .find((button) => button.textContent?.includes('分组')) as HTMLButtonElement
    addGroup.click()
    flushSync()

    const serialized = h.setContent.mock.calls.at(-1)?.[1] as string
    const nodes = JSON.parse(serialized).nodes as Array<Record<string, unknown>>
    expect(nodes[0]).toMatchObject({
      type: 'group', label: '重点', x: -36, y: -52, width: 332, height: 248,
    })
    expect(nodes.map((node) => node.id)).toEqual([expect.any(String), 'text-1', 'link-1'])
  })

  it('draws an exact standard group rectangle with the frame tool', async () => {
    h.storeGet.mockResolvedValue({ x: 0, y: 0, zoom: 1, updatedAt: 1 })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.svelte-flow__pane')).toBeTruthy())
    const surface = document.querySelector('.canvas-surface') as HTMLElement
    const pane = document.querySelector('.svelte-flow__pane') as HTMLElement
    ;(Array.from(document.querySelectorAll('.canvas-toolbar > button'))
      .find((button) => button.textContent?.trim() === '框组') as HTMLButtonElement).click()
    await tick()

    pane.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, pointerId: 17, pointerType: 'mouse', button: 0, isPrimary: true, clientX: 100, clientY: 90,
    }))
    surface.dispatchEvent(new PointerEvent('pointermove', {
      bubbles: true, pointerId: 17, pointerType: 'mouse', isPrimary: true, clientX: 410, clientY: 310,
    }))
    await tick()
    expect(document.querySelector('.draw-rectangle')).toBeTruthy()

    surface.dispatchEvent(new PointerEvent('pointerup', {
      bubbles: true, pointerId: 17, pointerType: 'mouse', button: 0, isPrimary: true, clientX: 410, clientY: 310,
    }))
    await tick()
    const saved = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string)
    expect(saved.nodes[0]).toMatchObject({ type: 'group', x: 100, y: 90, width: 310, height: 220 })
    expect(document.querySelector('.draw-rectangle')).toBeFalsy()
    expect((document.querySelector('button[aria-label^="撤销"]') as HTMLButtonElement).title)
      .toContain('拖拽创建分组')
  })

  it('draws a minimum-sized group with one-finger touch input', async () => {
    h.storeGet.mockResolvedValue({ x: 0, y: 0, zoom: 1, updatedAt: 1 })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.svelte-flow__pane')).toBeTruthy())
    const surface = document.querySelector('.canvas-surface') as HTMLElement
    const pane = document.querySelector('.svelte-flow__pane') as HTMLElement
    ;(Array.from(document.querySelectorAll('.canvas-toolbar > button'))
      .find((button) => button.textContent?.trim() === '框组') as HTMLButtonElement).click()
    await tick()

    pane.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, pointerId: 18, pointerType: 'touch', button: 0, isPrimary: true, clientX: 100, clientY: 90,
    }))
    surface.dispatchEvent(new PointerEvent('pointermove', {
      bubbles: true, pointerId: 18, pointerType: 'touch', isPrimary: true, clientX: 130, clientY: 120,
    }))
    surface.dispatchEvent(new PointerEvent('pointerup', {
      bubbles: true, pointerId: 18, pointerType: 'touch', button: 0, isPrimary: true, clientX: 130, clientY: 120,
    }))
    await tick()

    expect(JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes[0]).toMatchObject({
      type: 'group', x: 100, y: 90, width: 180, height: 120,
    })
  })

  it('persists keyboard movement and applies group closure semantics', async () => {
    const grouped = tab()
    grouped.currentContent = grouped.initialContent = JSON.stringify({
      nodes: [
        { id: 'group-1', type: 'group', label: '分组', x: 0, y: 0, width: 300, height: 220 },
        { id: 'inside', type: 'text', text: 'inside', x: 40, y: 50, width: 120, height: 80 },
      ],
      edges: [],
    })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: grouped },
    })
    await vi.waitFor(() => expect(document.querySelector('[data-id="group-1"] .group-label')).toBeTruthy())

    const label = document.querySelector('[data-id="group-1"] .group-label') as HTMLElement
    label.click()
    await tick()
    label.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
    await tick()
    let nodes = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes as Array<Record<string, unknown>>
    expect(nodes.find((node) => node.id === 'group-1')).toMatchObject({ x: 1, y: 0 })
    expect(nodes.find((node) => node.id === 'inside')).toMatchObject({ x: 41, y: 50 })

    label.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', shiftKey: true, bubbles: true }))
    await tick()
    nodes = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes as Array<Record<string, unknown>>
    expect(nodes.find((node) => node.id === 'group-1')).toMatchObject({ x: 1, y: 10 })
    expect(nodes.find((node) => node.id === 'inside')).toMatchObject({ x: 41, y: 60 })
  })

  it('renames, styles, clears and ungroups while preserving contained nodes', async () => {
    h.invoke.mockResolvedValue(Uint8Array.from([1, 137, 80, 78, 71]).buffer)
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:group-background')
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {})
    const grouped = tab()
    grouped.currentContent = grouped.initialContent = JSON.stringify({
      nodes: [
        { id: 'group-1', type: 'group', label: '旧名称', background: 'assets/bg.png', x: 0, y: 0, width: 300, height: 220 },
        { id: 'inside', type: 'text', text: 'inside', x: 40, y: 50, width: 120, height: 80 },
      ],
      edges: [],
    })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: grouped },
    })
    await vi.waitFor(() => expect(document.querySelector('[data-id="group-1"] .group-label')).toBeTruthy())
    ;(document.querySelector('[data-id="group-1"] .group-label') as HTMLElement).click()
    await tick()

    ;(document.querySelector('button[title="缩放分组边界以适配其中节点"]') as HTMLButtonElement).click()
    await tick()
    expect(JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes[0]).toMatchObject({
      id: 'group-1', x: 4, y: -2, width: 192, height: 168,
    })

    const name = document.querySelector('.group-name-label input') as HTMLInputElement
    name.value = '新名称'
    name.dispatchEvent(new Event('change', { bubbles: true }))
    await tick()
    expect(JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes[0]).toMatchObject({ label: '新名称' })

    const style = document.querySelector('.group-style-label select') as HTMLSelectElement
    style.value = 'cover'
    style.dispatchEvent(new Event('change', { bubbles: true }))
    await tick()
    expect(JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes[0]).toMatchObject({ backgroundStyle: 'cover' })

    ;(document.querySelector('button[title="移除分组背景图片"]') as HTMLButtonElement).click()
    await tick()
    let saved = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string)
    expect(saved.nodes[0]).not.toHaveProperty('background')
    expect(saved.nodes[0]).not.toHaveProperty('backgroundStyle')

    ;(document.querySelector('button[title="移除分组边框并保留其中节点"]') as HTMLButtonElement).click()
    await tick()
    saved = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string)
    expect(saved.nodes).toEqual([expect.objectContaining({ id: 'inside', text: 'inside' })])
    expect((document.querySelector('button[aria-label^="撤销"]') as HTMLButtonElement).title).toContain('解散分组')
  })

  it('exposes working edge label, end, color and reconnect controls', async () => {
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.svelte-flow__edge[data-id="edge-1"]')).toBeTruthy())
    ;(document.querySelector('.svelte-flow__edge[data-id="edge-1"]') as SVGGElement)
      .dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await tick()
    flushSync()

    expect(document.querySelectorAll('.canvas-edge-reconnect')).toHaveLength(2)
    const fromEnd = document.querySelector('button[title="切换连线起点箭头"]') as HTMLButtonElement
    const toEnd = document.querySelector('button[title="切换连线终点箭头"]') as HTMLButtonElement
    expect(fromEnd.getAttribute('aria-pressed')).toBe('false')
    expect(toEnd.getAttribute('aria-pressed')).toBe('true')
    fromEnd.click()
    await tick()
    toEnd.click()
    await tick()

    ;(document.querySelector('button[title="连线颜色 3"]') as HTMLButtonElement).click()
    await tick()
    const label = document.querySelector('.edge-label input') as HTMLInputElement
    label.value = '更新标签'
    label.dispatchEvent(new Event('change', { bubbles: true }))
    await tick()

    const edge = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).edges[0]
    expect(edge).toMatchObject({ fromEnd: 'arrow', toEnd: 'none', color: '3', label: '更新标签' })
  })

  it('keeps automatically routed edge sides out of the saved canvas', async () => {
    const dynamicEdge = tab()
    dynamicEdge.currentContent = dynamicEdge.initialContent = JSON.stringify({
      nodes: [
        { id: 'a', type: 'text', text: 'a', x: 0, y: 0, width: 120, height: 100 },
        { id: 'b', type: 'text', text: 'b', x: 300, y: 0, width: 120, height: 100 },
      ],
      edges: [{ id: 'dynamic', fromNode: 'a', toNode: 'b' }],
    })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: dynamicEdge },
    })
    await vi.waitFor(() => expect(document.querySelector('.svelte-flow__edge[data-id="dynamic"]')).toBeTruthy())

    ;(document.querySelector('[data-id="a"]') as HTMLElement).click()
    await tick()
    ;(document.querySelector('.canvas-surface') as HTMLElement)
      .dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
    await tick()

    const edge = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).edges[0]
    expect(edge).toEqual({ id: 'dynamic', fromNode: 'a', toNode: 'b' })
  })

  it('edits an edge label inline by double-click or Enter', async () => {
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.svelte-flow__edge[data-id="edge-1"]')).toBeTruthy())
    const edge = document.querySelector('.svelte-flow__edge[data-id="edge-1"]') as SVGGElement
    edge.dispatchEvent(new MouseEvent('click', { bubbles: true }))
    await tick()
    expect(document.querySelectorAll('.canvas-edge-reconnect')).toHaveLength(2)

    ;(document.querySelector('.canvas-surface') as HTMLElement)
      .dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await tick()
    let input = document.querySelector('.canvas-edge-label-content input') as HTMLInputElement
    expect(input).toBeTruthy()
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await tick()

    const labelButton = document.querySelector('.canvas-edge-label-content button') as HTMLButtonElement
    labelButton.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
    await tick()
    input = document.querySelector('.canvas-edge-label-content input') as HTMLInputElement
    expect(input.value).toBe('参考')
    input.value = '画布内标签'
    input.dispatchEvent(new Event('input', { bubbles: true }))
    input.dispatchEvent(new FocusEvent('blur', { bubbles: true }))
    await tick()
    expect(JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).edges[0].label).toBe('画布内标签')
  })

  it('keeps desktop clipboard buttons functional through the Tauri fallback', async () => {
    vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValue(new Error('Web clipboard denied'))
    vi.spyOn(navigator.clipboard, 'readText').mockRejectedValue(new Error('Web clipboard denied'))
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('[data-id="text-1"]')).toBeTruthy())
    ;(document.querySelector('[data-id="text-1"]') as HTMLElement).click()
    await tick()

    const copy = document.querySelector('button[title="复制选中内容"]') as HTMLButtonElement
    expect(copy.disabled).toBe(false)
    copy.click()
    await vi.waitFor(() => expect(h.clipboardWrite).toHaveBeenCalledOnce())
    expect(JSON.parse(h.clipboardWrite.mock.calls[0][0]).nodes).toEqual([
      expect.objectContaining({ id: 'text-1', type: 'text' }),
    ])

    h.clipboardRead.mockResolvedValue('https://example.org/from-clipboard')
    ;(document.querySelector('button[title="粘贴"]') as HTMLButtonElement).click()
    await vi.waitFor(() => {
      const saved = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string)
      expect(saved.nodes).toEqual(expect.arrayContaining([
        expect.objectContaining({ type: 'link', url: 'https://example.org/from-clipboard' }),
      ]))
    })
  })

  it('creates a text card by double-clicking the pane and zooms from the visible Flow control', async () => {
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(800)
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(600)
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.svelte-flow__pane')).toBeTruthy())

    const pane = document.querySelector('.svelte-flow__pane') as HTMLElement
    pane.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, clientX: 250, clientY: 220 }))
    await tick()
    expect(JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes).toHaveLength(3)

    const viewport = document.querySelector('.svelte-flow__viewport') as HTMLElement
    const before = viewport.style.transform
    const zoomIn = document.querySelector('.svelte-flow__controls-zoomin') as HTMLButtonElement
    const zoomOut = document.querySelector('.svelte-flow__controls-zoomout') as HTMLButtonElement
    const fitView = document.querySelector('.svelte-flow__controls-fitview') as HTMLButtonElement
    expect(zoomIn.disabled).toBe(false)
    expect(zoomOut.disabled).toBe(false)
    expect(fitView.disabled).toBe(false)
    zoomIn.click()
    await vi.waitFor(() => expect(viewport.style.transform).not.toBe(before))
    zoomOut.click()
    fitView.click()
  })

  it('exposes select/pan/lasso tools, Space pan and a view-only interaction lock', async () => {
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('button[aria-label="自由套索工具"]')).toBeTruthy())
    const surface = document.querySelector('.canvas-surface') as HTMLElement

    surface.dispatchEvent(new KeyboardEvent('keydown', { key: 'l', bubbles: true }))
    await tick()
    expect(surface.classList.contains('tool-lasso')).toBe(true)
    expect((document.querySelector('button[aria-label="自由套索工具"]') as HTMLButtonElement).classList.contains('tool-active')).toBe(true)

    surface.dispatchEvent(new KeyboardEvent('keydown', { key: ' ', bubbles: true }))
    await tick()
    expect(surface.classList.contains('tool-pan')).toBe(true)
    window.dispatchEvent(new KeyboardEvent('keyup', { key: ' ' }))
    await tick()
    expect(surface.classList.contains('tool-lasso')).toBe(true)

    surface.dispatchEvent(new KeyboardEvent('keydown', { key: 's', bubbles: true }))
    surface.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', metaKey: true, bubbles: true }))
    await tick()
    expect(document.querySelector('.selection-resizer')).toBeTruthy()

    ;(document.querySelector('button[title="临时锁定或解锁当前画布交互"]') as HTMLButtonElement).click()
    await tick()
    expect(surface.classList.contains('tool-pan')).toBe(true)
    expect(document.querySelector('.selection-resizer')).toBeFalsy()
    expect(document.body.textContent).toContain('解锁')
    expect(h.setContent).not.toHaveBeenCalled()
  })

  it('updates the untouched default tool for form-factor changes without overriding a user choice', async () => {
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('button[aria-label="自由套索工具"]')).toBeTruthy())
    const surface = document.querySelector('.canvas-surface') as HTMLElement
    expect(surface.classList.contains('tool-pan')).toBe(false)

    formFactor.value = 'phone'
    await tick()
    expect(surface.classList.contains('tool-pan')).toBe(true)

    ;(document.querySelector('button[aria-label="自由套索工具"]') as HTMLButtonElement).click()
    formFactor.value = 'desktop'
    await tick()
    expect(surface.classList.contains('tool-lasso')).toBe(true)
  })

  it('supports application zoom shortcuts and compact semantic zoom', async () => {
    h.storeGet.mockResolvedValue({ x: 0, y: 0, zoom: 1, updatedAt: 1 })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.zoom-indicator')?.textContent).toContain('100%'))
    const surface = document.querySelector('.canvas-surface') as HTMLElement
    for (let index = 0; index < 6; index++) {
      surface.dispatchEvent(new KeyboardEvent('keydown', { key: '-', metaKey: true, bubbles: true }))
    }
    await tick()
    expect(surface.classList.contains('lod-compact')).toBe(true)
    expect((document.querySelector('.compact-label') as HTMLElement).textContent).toContain('画布卡片')

    surface.dispatchEvent(new KeyboardEvent('keydown', { key: '0', metaKey: true, bubbles: true }))
    await tick()
    expect(surface.classList.contains('lod-compact')).toBe(false)
    expect(document.querySelector('.zoom-indicator')?.textContent).toContain('100%')
  })

  it('aligns and distributes a multi-selection as one undoable document command', async () => {
    const arranged = tab()
    arranged.currentContent = arranged.initialContent = JSON.stringify({
      nodes: [
        { id: 'a', type: 'text', text: 'a', x: 0, y: 0, width: 200, height: 120 },
        { id: 'b', type: 'text', text: 'b', x: 260, y: 40, width: 160, height: 100 },
        { id: 'c', type: 'text', text: 'c', x: 540, y: 80, width: 200, height: 120 },
      ],
      edges: [],
    })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: arranged },
    })
    await vi.waitFor(() => expect(document.querySelectorAll('.svelte-flow__node')).toHaveLength(3))
    const surface = document.querySelector('.canvas-surface') as HTMLElement
    surface.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', metaKey: true, bubbles: true }))
    await tick()

    expect(document.querySelector('.selection-resizer')).toBeTruthy()
    ;(document.querySelector('button[aria-label="左对齐"]') as HTMLButtonElement).click()
    await tick()
    let nodes = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes as Array<Record<string, unknown>>
    expect(nodes.map((node) => node.x)).toEqual([0, 0, 0])
    expect((document.querySelector('button[aria-label^="撤销"]') as HTMLButtonElement).title).toContain('对齐选中节点')

    ;(document.querySelector('button[aria-label^="撤销"]') as HTMLButtonElement).click()
    await tick()
    nodes = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes as Array<Record<string, unknown>>
    expect(nodes.map((node) => node.x)).toEqual([0, 260, 540])

    ;(document.querySelector('button[aria-label="水平等距分布"]') as HTMLButtonElement).click()
    await tick()
    nodes = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes as Array<Record<string, unknown>>
    expect(nodes.find((node) => node.id === 'b')?.x).toBe(290)

    const rightBefore = Math.max(...nodes.map((node) => Number(node.x) + Number(node.width)))
    ;(document.querySelector('button[aria-label="缩放选区右下角"]') as HTMLButtonElement)
      .dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', shiftKey: true, bubbles: true }))
    await tick()
    nodes = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes as Array<Record<string, unknown>>
    expect(Math.max(...nodes.map((node) => Number(node.x) + Number(node.width)))).toBeGreaterThan(rightBefore)
  })

  it('draws a freeform lasso and selects only intersecting nodes', async () => {
    h.storeGet.mockResolvedValue({ x: 0, y: 0, zoom: 1, updatedAt: 1 })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.svelte-flow__pane')).toBeTruthy())
    const surface = document.querySelector('.canvas-surface') as HTMLElement
    const pane = document.querySelector('.svelte-flow__pane') as HTMLElement
    surface.dispatchEvent(new KeyboardEvent('keydown', { key: 'l', bubbles: true }))
    await tick()

    pane.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, pointerId: 7, pointerType: 'mouse', button: 0, isPrimary: true, clientX: 5, clientY: 5,
    }))
    for (const [clientX, clientY] of [[280, 5], [280, 180], [5, 180]] as const) {
      surface.dispatchEvent(new PointerEvent('pointermove', {
        bubbles: true, pointerId: 7, pointerType: 'mouse', isPrimary: true, clientX, clientY,
      }))
    }
    await tick()
    expect(document.querySelector('.lasso-polygon')).toBeTruthy()
    surface.dispatchEvent(new PointerEvent('pointerup', {
      bubbles: true, pointerId: 7, pointerType: 'mouse', button: 0, isPrimary: true, clientX: 5, clientY: 5,
    }))
    await tick()

    expect(document.querySelector('[data-id="text-1"]')?.classList.contains('selected')).toBe(true)
    expect(document.querySelector('[data-id="link-1"]')?.classList.contains('selected')).toBe(false)
    expect(document.querySelector('.lasso-polygon')).toBeFalsy()
    expect(h.setContent).not.toHaveBeenCalled()
  })

  it('selects with a one-finger touch lasso', async () => {
    h.storeGet.mockResolvedValue({ x: 0, y: 0, zoom: 1, updatedAt: 1 })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.svelte-flow__pane')).toBeTruthy())
    const surface = document.querySelector('.canvas-surface') as HTMLElement
    const pane = document.querySelector('.svelte-flow__pane') as HTMLElement
    surface.dispatchEvent(new KeyboardEvent('keydown', { key: 'l', bubbles: true }))
    await tick()

    pane.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, pointerId: 27, pointerType: 'touch', button: 0, isPrimary: true, clientX: 5, clientY: 5,
    }))
    for (const [clientX, clientY] of [[280, 5], [280, 180], [5, 180]] as const) {
      surface.dispatchEvent(new PointerEvent('pointermove', {
        bubbles: true, pointerId: 27, pointerType: 'touch', isPrimary: true, clientX, clientY,
      }))
    }
    surface.dispatchEvent(new PointerEvent('pointerup', {
      bubbles: true, pointerId: 27, pointerType: 'touch', button: 0, isPrimary: true, clientX: 5, clientY: 5,
    }))
    await tick()

    expect(document.querySelector('[data-id="text-1"]')?.classList.contains('selected')).toBe(true)
    expect(document.querySelector('[data-id="link-1"]')?.classList.contains('selected')).toBe(false)
    expect(h.setContent).not.toHaveBeenCalled()
  })

  it('cancels a pending touch lasso and yields navigation to a second finger', async () => {
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.svelte-flow__pane')).toBeTruthy())
    const surface = document.querySelector('.canvas-surface') as HTMLElement
    const pane = document.querySelector('.svelte-flow__pane') as HTMLElement
    surface.dispatchEvent(new KeyboardEvent('keydown', { key: 'l', bubbles: true }))
    await tick()

    pane.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, pointerId: 37, pointerType: 'touch', button: 0, isPrimary: true, clientX: 20, clientY: 20,
    }))
    pane.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, pointerId: 38, pointerType: 'touch', button: 0, isPrimary: false, clientX: 80, clientY: 80,
    }))
    await tick()
    expect(document.querySelector('.lasso-polygon')).toBeFalsy()
    expect(pane.classList.contains('draggable')).toBe(true)

    for (const pointerId of [37, 38]) {
      surface.dispatchEvent(new PointerEvent('pointerup', {
        bubbles: true, pointerId, pointerType: 'touch', button: 0, isPrimary: pointerId === 37,
      }))
    }
    await tick()
    expect(pane.classList.contains('draggable')).toBe(false)
  })

  it('places shortcut-created nodes and pasted text at the last pointer position', async () => {
    h.storeGet.mockResolvedValue({ x: 0, y: 0, zoom: 1, updatedAt: 1 })
    component = mount(CanvasView as unknown as Parameters<typeof mount>[0], {
      target: document.body,
      props: { tab: tab() },
    })
    await vi.waitFor(() => expect(document.querySelector('.svelte-flow__pane')).toBeTruthy())
    const surface = document.querySelector('.canvas-surface') as HTMLElement
    const pane = document.querySelector('.svelte-flow__pane') as HTMLElement
    surface.dispatchEvent(new PointerEvent('pointermove', {
      bubbles: true, pointerId: 3, pointerType: 'mouse', isPrimary: true, clientX: 700, clientY: 500,
    }))
    surface.dispatchEvent(new KeyboardEvent('keydown', { key: '1', bubbles: true }))
    pane.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: 700, clientY: 500 }))
    await tick()

    let nodes = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes as Array<Record<string, unknown>>
    expect(nodes.at(-1)).toMatchObject({ type: 'text', x: 560, y: 410 })

    const addText = document.querySelector('button[title="新建文本卡片"]') as HTMLButtonElement
    addText.dispatchEvent(new PointerEvent('pointerdown', {
      bubbles: true, pointerId: 4, pointerType: 'mouse', button: 0, isPrimary: true, clientX: 20, clientY: 20,
    }))
    addText.click()
    await tick()
    nodes = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes as Array<Record<string, unknown>>
    expect(nodes.at(-1)).toMatchObject({ type: 'text', x: 560, y: 410 })

    const pasteEvent = new ClipboardEvent('paste', { bubbles: true })
    Object.defineProperty(pasteEvent, 'clipboardData', {
      value: { getData: () => 'pointer paste' },
    })
    surface.dispatchEvent(pasteEvent)
    await tick()
    nodes = JSON.parse(h.setContent.mock.calls.at(-1)?.[1] as string).nodes as Array<Record<string, unknown>>
    expect(nodes.at(-1)).toMatchObject({ type: 'text', text: 'pointer paste', x: 560, y: 410 })
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
