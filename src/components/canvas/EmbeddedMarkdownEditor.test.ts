// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { mount, unmount } from 'svelte'
import EmbeddedMarkdownEditor from './EmbeddedMarkdownEditor.svelte'

const h = vi.hoisted(() => ({
  initialContent: '',
  emitChange: null as ((value: string) => void) | null,
}))

vi.mock('../../lib/editor-bridge', () => ({
  updateDocumentBaseDir: vi.fn(),
  mountRichEditor: vi.fn(async (
    root: HTMLElement,
    initialContent: string,
    onChange: (value: string) => void,
  ) => {
    h.initialContent = initialContent
    h.emitChange = onChange
    let content = initialContent
    const editor = document.createElement('div')
    editor.className = 'ProseMirror'
    const src = /!\[[^\]]*\]\(([^)]+)\)/.exec(initialContent)?.[1]
    if (src) {
      const img = document.createElement('img')
      img.setAttribute('src', src)
      editor.appendChild(img)
    }
    // Keep the probe outside Svelte's managed loading placeholder so its
    // conditional removal cannot also remove this simulated Core DOM.
    document.body.appendChild(editor)
    return {
      view: { focus: vi.fn() },
      getMarkdown: () => content,
      setContent: (value: string) => { content = value },
      destroy: vi.fn(),
    }
  }),
}))

describe('EmbeddedMarkdownEditor Canvas resource profile', () => {
  let component: ReturnType<typeof mount> | null = null

  afterEach(async () => {
    if (component) await unmount(component)
    component = null
    document.body.innerHTML = ''
    h.initialContent = ''
    h.emitChange = null
    vi.restoreAllMocks()
    Reflect.deleteProperty(document, 'execCommand')
  })

  it('never mounts a remote image src and restores the original Markdown in callbacks', async () => {
    const onChange = vi.fn()
    component = mount(EmbeddedMarkdownEditor, {
      target: document.body,
      props: {
        markdown: '![remote](https://example.com/tracker.png)',
        filePath: '/vault/board.canvas',
        mediaResolver: {
          loadLocalImage: async () => '',
          loadLocalMedia: async () => '',
          loadRemoteMedia: async () => '',
        },
        onChange,
        onFlush: vi.fn(),
      },
    })

    await vi.waitFor(() => expect(h.initialContent).not.toBe(''))
    expect(h.initialContent).toMatch(/!\[remote\]\(data:image\/gif,notemd-canvas-remote-/)
    expect(h.initialContent).not.toMatch(/!\[[^\]]*\]\(https?:/)
    expect(document.querySelector('img[src^="http"]')).toBeNull()

    h.emitChange?.(h.initialContent)
    expect(onChange).toHaveBeenCalledWith('![remote](https://example.com/tracker.png)')
  })

  it('captures pasted HTML containing remote media before the editor can parse it', async () => {
    const execCommand = vi.fn(() => true)
    Object.defineProperty(document, 'execCommand', { value: execCommand, configurable: true })
    component = mount(EmbeddedMarkdownEditor, {
      target: document.body,
      props: {
        markdown: '',
        filePath: '/vault/board.canvas',
        mediaResolver: {
          loadLocalImage: async () => '',
          loadLocalMedia: async () => '',
          loadRemoteMedia: async () => '',
        },
        onChange: vi.fn(),
        onFlush: vi.fn(),
      },
    })
    await vi.waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const event = new Event('paste', { bubbles: true, cancelable: true }) as ClipboardEvent
    Object.defineProperty(event, 'clipboardData', {
      value: {
        getData: (type: string) => type === 'text/html'
          ? '<img src="https://example.com/tracker.png">'
          : 'remote image',
      },
    })

    document.querySelector('.embedded-markdown')?.dispatchEvent(event)

    expect(event.defaultPrevented).toBe(true)
    expect(execCommand).toHaveBeenCalledWith('insertText', false, 'remote image')
    expect(document.querySelector('img[src^="http"]')).toBeNull()
  })

  it('flushes synchronously for its owning tab before save or tab teardown', async () => {
    const onFlush = vi.fn()
    component = mount(EmbeddedMarkdownEditor, {
      target: document.body,
      props: {
        markdown: 'latest canvas text',
        tabId: 'canvas-tab',
        filePath: '/vault/board.canvas',
        mediaResolver: {
          loadLocalImage: async () => '',
          loadLocalMedia: async () => '',
          loadRemoteMedia: async () => '',
        },
        onChange: vi.fn(),
        onFlush,
      },
    })
    await vi.waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())

    window.dispatchEvent(new CustomEvent('notemd:flush-doc', { detail: { tabId: 'other-tab' } }))
    expect(onFlush).not.toHaveBeenCalled()

    window.dispatchEvent(new CustomEvent('notemd:flush-doc', { detail: { tabId: 'canvas-tab' } }))
    expect(onFlush).toHaveBeenCalledWith('latest canvas text')
  })
})
