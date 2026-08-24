// @vitest-environment happy-dom
//
// IME 变换中（未确定文字 + 候选窗口）按下的键属于输入法，不属于我们。
//
// 这条测试钉住的是一个用户能直接看见的缺陷：日文/中文输入法在候选窗口开着时
// 按退格，本该只删掉预编辑串里的一个字符，结果**多删一个** —— 因为
// `OutlineNode.onKeydown` 里的 `Backspace && atStart` 分支同时也跑了一遍，
// 把当前节点并进了上一个节点。同一个键被输入法和应用各处理一次。
//
// 反向的一条（isComposing=false 时仍然合并）同样必须在：光是「不合并」很容易
// 用「把整个 Backspace 分支删掉」来通过，那不是修复，那是砍功能。
//
// 参照实现是 `SearchPanel`（`e.isComposing` 早退，见 SearchPanel.svelte:197）——
// 面板早就有这个判断，编辑器没有。
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, unmount, flushSync } from 'svelte'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async () => { throw new Error('no tauri host in vitest') }),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}))
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: { load: vi.fn(async () => ({ get: vi.fn(async () => undefined), set: vi.fn(async () => {}), save: vi.fn(async () => {}) })) },
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({
  confirm: vi.fn(async () => false),
  message: vi.fn(async () => {}),
}))
vi.mock('@tauri-apps/plugin-fs', () => ({
  exists: vi.fn(async () => false),
  readTextFile: vi.fn(async () => ''),
  writeTextFile: vi.fn(async () => {}),
  mkdir: vi.fn(async () => {}),
}))

import { outline } from '../../lib/outline/store.svelte'
import { createTree, addNode } from '../../lib/outline/model'

beforeEach(() => {
  vi.clearAllMocks()
  document.body.innerHTML = ''
  outline.tree = createTree()
  outline.editingId = null
  outline.selectedIds = new Set()
})

/** 上一节点「あいう」，当前节点空且在编辑态 —— 正是「新起一行开始打日文」的样子。 */
function twoNodes() {
  const t = createTree()
  addNode(t, { id: 'prev', parentId: null, order: 0, content: 'あいう', collapsed: false, source: 'manual' })
  addNode(t, { id: 'cur', parentId: null, order: 100, content: '', collapsed: false, source: 'manual' })
  outline.tree = t
  outline.editingId = 'cur'
  return t
}

async function mountCurrent() {
  const { default: OutlineNode } = await import('./OutlineNode.svelte')
  const app = mount(OutlineNode as unknown as Parameters<typeof mount>[0], {
    target: document.body,
    props: {
      node: outline.tree.nodes.get('cur'),
      depth: 0,
      onPageClick: () => {},
    },
  })
  flushSync()
  return app
}

/** happy-dom 的 KeyboardEvent 不带 `isComposing`，按真实浏览器的样子补上。 */
function backspace(el: HTMLTextAreaElement, isComposing: boolean) {
  const ev = new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true })
  Object.defineProperty(ev, 'isComposing', { value: isComposing })
  el.dispatchEvent(ev)
  flushSync()
}

describe('OutlineNode — Backspace 与 IME 变换中状态', () => {
  it('变换中的退格不碰大纲结构（这一下是输入法的，不是我们的）', async () => {
    twoNodes()
    const app = await mountCurrent()

    const ta = document.body.querySelector('textarea') as HTMLTextAreaElement
    expect(ta, '编辑态应渲染出 textarea').toBeTruthy()
    // 候选窗口开着、预编辑串还没落进 value 时，光标就停在节点行首。
    ta.setSelectionRange(0, 0)

    backspace(ta, true)

    expect(outline.tree.nodes.has('cur'), '变换中按退格不该把当前节点并掉').toBe(true)
    expect(outline.tree.nodes.get('prev')!.content).toBe('あいう')
    unmount(app)
  })

  it('双保险:webview 只给 composition 事件、keydown 上什么标记都没有时也不误删', async () => {
    twoNodes()
    const app = await mountCurrent()

    const ta = document.body.querySelector('textarea') as HTMLTextAreaElement
    ta.setSelectionRange(0, 0)
    ta.dispatchEvent(new Event('compositionstart', { bubbles: true }))
    flushSync()

    backspace(ta, false)   // 故意不带 isComposing —— 就是那些不可靠的 webview

    expect(outline.tree.nodes.has('cur')).toBe(true)

    // compositionend 之后键盘归还给编辑器,行首退格必须重新生效
    ta.dispatchEvent(new Event('compositionend', { bubbles: true }))
    flushSync()
    backspace(ta, false)
    expect(outline.tree.nodes.has('cur'), 'compositionend 之后不能一直哑着').toBe(false)

    unmount(app)
  })

  it('非变换态的行首退格照旧并入上一节点', async () => {
    twoNodes()
    const app = await mountCurrent()

    const ta = document.body.querySelector('textarea') as HTMLTextAreaElement
    ta.setSelectionRange(0, 0)

    backspace(ta, false)

    expect(outline.tree.nodes.has('cur'), '这是行首退格本来的语义，不能被顺手删掉').toBe(false)
    unmount(app)
  })
})
