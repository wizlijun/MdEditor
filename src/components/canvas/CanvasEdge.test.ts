// @vitest-environment happy-dom
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import { Position, SvelteFlow } from '@xyflow/svelte'
import CanvasEdge from './CanvasEdge.svelte'

class ResizeObserverStub {
  constructor(private readonly callback: ResizeObserverCallback) {}
  observe(target: Element): void {
    const element = target as HTMLElement
    const width = Number.parseFloat(element.style.width) || 120
    const height = Number.parseFloat(element.style.height) || 60
    Object.defineProperties(element, {
      offsetWidth: { configurable: true, value: width },
      offsetHeight: { configurable: true, value: height },
    })
    const rect = {
      x: 0, y: 0, top: 0, left: 0, right: width, bottom: height,
      width, height, toJSON: () => ({}),
    } as DOMRect
    element.getBoundingClientRect = () => rect
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
    queueMicrotask(() => this.callback([{ target, contentRect: rect } as ResizeObserverEntry], this as unknown as ResizeObserver))
  }
  unobserve(): void {}
  disconnect(): void {}
}

describe('CanvasEdge inline label', () => {
  let component: ReturnType<typeof mount> | null = null
  const onLabelCommit = vi.fn()

  beforeEach(() => {
    document.body.innerHTML = '<div id="flow" style="width:800px;height:600px"></div>'
    onLabelCommit.mockClear()
    vi.stubGlobal('ResizeObserver', ResizeObserverStub)
  })

  afterEach(async () => {
    if (component) await unmount(component)
    component = null
    vi.unstubAllGlobals()
    document.body.innerHTML = ''
  })

  async function mountSelectedEdge(): Promise<void> {
    component = mount(SvelteFlow, {
      target: document.querySelector('#flow') as HTMLElement,
      props: {
        nodes: [
          {
            id: 'a', type: 'input', position: { x: 0, y: 0 }, width: 120, height: 60,
            sourcePosition: Position.Right,
            handles: [{ type: 'source', position: Position.Right, x: 120, y: 26, width: 8, height: 8 }],
            data: { label: 'A' },
          },
          {
            id: 'b', type: 'output', position: { x: 300, y: 0 }, width: 120, height: 60,
            targetPosition: Position.Left,
            handles: [{ type: 'target', position: Position.Left, x: -8, y: 26, width: 8, height: 8 }],
            data: { label: 'B' },
          },
        ],
        edges: [{
          id: 'edge-view',
          source: 'a',
          target: 'b',
          type: 'canvas-edge',
          selected: true,
          label: '旧标签',
          data: { canonicalId: 'edge-canonical', tabId: 'canvas-tab', onLabelCommit },
        }],
        edgeTypes: { 'canvas-edge': CanvasEdge },
        fitView: false,
      },
    })
    await vi.waitFor(() => expect(document.querySelector('[data-canvas-edge-label="edge-view"]')).toBeTruthy())
  }

  async function beginEditing(value: string): Promise<HTMLInputElement> {
    const button = document.querySelector('button[data-canvas-edge-label="edge-view"]') as HTMLButtonElement
    button.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
    await tick()
    const input = document.querySelector('input[data-canvas-edge-label="edge-view"]') as HTMLInputElement
    input.value = value
    input.dispatchEvent(new Event('input', { bubbles: true }))
    return input
  }

  it('flushes only for its owning tab', async () => {
    await mountSelectedEdge()
    const input = await beginEditing('新标签')

    window.dispatchEvent(new CustomEvent('notemd:flush-doc', { detail: { tabId: 'other-tab' } }))
    expect(onLabelCommit).not.toHaveBeenCalled()
    expect(input.isConnected).toBe(true)

    window.dispatchEvent(new CustomEvent('notemd:flush-doc', { detail: { tabId: 'canvas-tab' } }))
    await tick()
    expect(onLabelCommit).toHaveBeenCalledOnce()
    expect(onLabelCommit).toHaveBeenCalledWith('edge-canonical', '新标签')
    expect(document.querySelector('input[data-canvas-edge-label="edge-view"]')).toBeNull()
  })

  it('does not commit or cancel on IME Enter and Escape', async () => {
    await mountSelectedEdge()
    const input = await beginEditing('输入法候选')

    for (const key of ['Enter', 'Escape']) {
      const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })
      Object.defineProperty(event, 'isComposing', { value: true })
      input.dispatchEvent(event)
    }

    expect(onLabelCommit).not.toHaveBeenCalled()
    expect(input.isConnected).toBe(true)
  })

  it('gives reconnect handles a 44px coarse-pointer hit target', () => {
    const source = readFileSync(resolve(process.cwd(), 'src/components/canvas/CanvasEdge.svelte'), 'utf8')
    expect(source).toContain('class="canvas-edge-reconnect canvas-edge-touch-target"')
    expect(source).toMatch(/@media \(pointer: coarse\)[\s\S]*canvas-edge-touch-target[\s\S]*width: 44px !important;[\s\S]*height: 44px !important;/)
  })
})
