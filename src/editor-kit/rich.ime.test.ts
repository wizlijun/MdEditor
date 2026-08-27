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

vi.mock('./media', () => ({
  bridgeMediaResolver: vi.fn(() => ({})),
}))

vi.mock('../lib/platform-sync', () => ({
  isApplePlatformSync: vi.fn(() => true),
}))

import { mountRich } from './rich'

function press(el: HTMLElement, key = 'Backspace') {
  const ev = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })
  el.dispatchEvent(ev)
  return ev
}

function endComposition(el: HTMLElement) {
  el.dispatchEvent(new Event('compositionstart', { bubbles: true }))
  el.dispatchEvent(new Event('compositionend', { bubbles: true }))
}

beforeEach(() => {
  document.body.innerHTML = ''
  h.destroy.mockReset()
  h.createEditor.mockImplementation(async ({ container }: { container: HTMLElement }) => {
    const pm = document.createElement('div')
    pm.className = 'ProseMirror moraya-editor'
    pm.contentEditable = 'true'
    container.appendChild(pm)
    return {
      view: {},
      getMarkdown: () => '',
      setContent: vi.fn(),
      destroy: h.destroy,
    }
  })
})

describe('mountRich — Rich 工厂自带 IME 收尾保护', () => {
  it('直接挂载也会取消收尾退格，且只取消一下', async () => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    const editor = await mountRich(host, '已确认', '/vault', vi.fn())
    const pm = host.querySelector('.ProseMirror') as HTMLElement

    endComposition(pm)

    expect(press(pm).defaultPrevented).toBe(true)
    expect(press(pm).defaultPrevented).toBe(false)
    editor.destroy()
  })

  it('普通退格照常放行，销毁后不残留保护监听器', async () => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    const editor = await mountRich(host, '已确认', '/vault', vi.fn())
    const pm = host.querySelector('.ProseMirror') as HTMLElement

    expect(press(pm).defaultPrevented).toBe(false)
    editor.destroy()
    endComposition(pm)
    expect(press(pm).defaultPrevented).toBe(false)
    expect(h.destroy).toHaveBeenCalledTimes(1)
  })

  it('嵌在 Rich NodeView 里的可编辑组件也经过同一个祖先守卫', async () => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    const editor = await mountRich(host, '---\ntitle: 旧标题\n---', '/vault', vi.fn())
    const pm = host.querySelector('.ProseMirror') as HTMLElement
    const nodeView = document.createElement('div')
    nodeView.contentEditable = 'false'
    const cell = document.createElement('div')
    cell.contentEditable = 'true'
    nodeView.appendChild(cell)
    pm.appendChild(nodeView)

    endComposition(cell)

    expect(press(cell).defaultPrevented, 'NodeView 内部的 Rich 单元格不能漏过祖先保护').toBe(true)
    editor.destroy()
  })
})
