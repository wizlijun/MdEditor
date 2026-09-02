// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { EditorState, TextSelection, type Transaction } from 'prosemirror-state'

const h = vi.hoisted(() => ({
  createEditor: vi.fn(),
  destroy: vi.fn(),
}))

vi.mock('@moraya/core', async () => {
  const actual = await vi.importActual<typeof import('@moraya/core')>('@moraya/core')
  return {
    ...actual,
    createEditor: h.createEditor,
    setDocumentBaseDir: vi.fn(),
  }
})

vi.mock('./media', () => ({
  bridgeMediaResolver: vi.fn(() => ({})),
}))

vi.mock('../lib/platform-sync', () => ({
  isApplePlatformSync: vi.fn(() => true),
}))

import { parseMarkdown } from '@moraya/core'
import { mountRich } from './rich'

const MARKDOWN = `# Heading

\`\`\`js
const a = 1
\`\`\`

trailing paragraph
`

function firstCodePosition(state: EditorState): number {
  let found = -1
  state.doc.descendants((node, pos) => {
    if (found < 0 && node.type.name === 'code_block') found = pos + 1
  })
  if (found < 0) throw new Error('fixture has no code block')
  return found
}

beforeEach(() => {
  document.body.innerHTML = ''
  h.destroy.mockReset()
  h.createEditor.mockImplementation(async ({ container }: { container: HTMLElement }) => {
    const pm = document.createElement('div')
    pm.className = 'ProseMirror moraya-editor'
    pm.contentEditable = 'true'
    container.appendChild(pm)

    let state = EditorState.create({ doc: parseMarkdown(MARKDOWN) })
    state = state.apply(state.tr.setSelection(TextSelection.create(state.doc, firstCodePosition(state))))
    const view = {
      get state() { return state },
      dispatch: (tr: Transaction) => { state = state.apply(tr) },
      focus: vi.fn(),
    }
    return {
      view,
      getMarkdown: () => MARKDOWN,
      setContent: vi.fn(),
      destroy: h.destroy,
    }
  })
})

describe('mountRich — Select All', () => {
  it('captures Cmd+A before moraya and selects past the current code block', async () => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    const editor = await mountRich(host, MARKDOWN, '/vault', vi.fn())
    const pm = host.querySelector('.ProseMirror') as HTMLElement
    const reachedEditor = vi.fn()
    pm.addEventListener('keydown', reachedEditor)

    const ev = new KeyboardEvent('keydown', {
      key: 'a', metaKey: true, bubbles: true, cancelable: true,
    })
    pm.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(true)
    expect(reachedEditor).not.toHaveBeenCalled()
    expect(editor.view.state.selection.to).toBe(editor.view.state.doc.content.size)
    expect(editor.view.state.doc.textBetween(
      editor.view.state.selection.from,
      editor.view.state.selection.to,
      '\n',
    )).toContain('trailing paragraph')

    editor.destroy()
  })

  it('removes the capture listener when the rich pane is destroyed', async () => {
    const host = document.createElement('div')
    document.body.appendChild(host)
    const editor = await mountRich(host, MARKDOWN, '/vault', vi.fn())
    const pm = host.querySelector('.ProseMirror') as HTMLElement

    editor.destroy()
    const ev = new KeyboardEvent('keydown', {
      key: 'a', metaKey: true, bubbles: true, cancelable: true,
    })
    pm.dispatchEvent(ev)

    expect(ev.defaultPrevented).toBe(false)
    expect(h.destroy).toHaveBeenCalledTimes(1)
  })
})
