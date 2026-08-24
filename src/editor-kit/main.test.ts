/**
 * @vitest-environment happy-dom
 */
// Orchestration tests for the kit entry: mode switching, the single-markdown
// source of truth, and the flush semantics `setMode()` / `destroy()` promise.
//
// Rich mode is stubbed rather than really mounted: ProseMirror needs a real
// layout engine, and what is under test here is main.ts's bookkeeping, not
// moraya. The stub deliberately models the one moraya behaviour that matters —
// `onChange` is debounced, so the editor can hold text it has not reported yet
// (moraya's change plugin clears that timer on destroy without flushing it).
import { describe, it, expect, vi, beforeEach } from 'vitest'

interface FakeRich {
  doc: string
  onChange: (md: string) => void
  view: { focus: ReturnType<typeof vi.fn> }
  getMarkdown(): string
  setContent(md: string): void
  destroy: ReturnType<typeof vi.fn>
}

const h = vi.hoisted(() => ({
  instances: [] as unknown[],
  setKitBaseDir: vi.fn(),
  setRichPlaceholder: vi.fn(),
}))

vi.mock('./rich', () => ({
  setKitBaseDir: h.setKitBaseDir,
  // What it actually does (rebuilding the placeholder plugin and reconfiguring
  // the view) is covered by rich.test.ts; here only the routing matters.
  setRichPlaceholder: h.setRichPlaceholder,
  mountRich: async (
    host: HTMLElement,
    initial: string,
    _vaultRoot: string,
    onChange: (md: string) => void,
  ) => {
    const el = document.createElement('div')
    el.className = 'moraya-editor'
    host.appendChild(el)
    const inst: FakeRich = {
      doc: initial,
      onChange,
      view: { focus: vi.fn() },
      getMarkdown: () => inst.doc,
      setContent: (md: string) => { inst.doc = md },
      destroy: vi.fn(),
    }
    h.instances.push(inst)
    return inst
  },
}))

import { mountMarkdownEditor } from './main'

const richInstances = h.instances as FakeRich[]

/** Text typed into rich mode that the debounce has not reported yet. */
function typeUnflushed(text: string) {
  richInstances[richInstances.length - 1].doc = text
}

beforeEach(() => {
  richInstances.length = 0
  h.setKitBaseDir.mockClear()
  h.setRichPlaceholder.mockClear()
  ;(window as unknown as { notemd: unknown }).notemd = {
    request: vi.fn(async (method: string) => {
      if (method === 'host.vault.info') return { root: '/vault' }
      if (method === 'host.theme.css') return { light_css: '', dark_css: '', follow_system: false }
      return {}
    }),
    onMessage: vi.fn(),
  }
})

function container(): HTMLElement {
  const el = document.createElement('div')
  document.body.appendChild(el)
  return el
}

describe('mountMarkdownEditor — source mode', () => {
  it('mounts a source pane and round-trips markdown', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: '# Title', mode: 'source' })
    expect(ed.getMode()).toBe('source')
    const ta = c.querySelector('textarea')!
    expect(ta.value).toBe('# Title')
    ed.setMarkdown('changed')
    expect(ta.value).toBe('changed')
    expect(ed.getMarkdown()).toBe('changed')
    ed.destroy()
  })

  it('reports typing through onChange', async () => {
    const c = container()
    const onChange = vi.fn()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: '', mode: 'source', onChange })
    const ta = c.querySelector('textarea')!
    ta.value = 'abc'
    ta.dispatchEvent(new Event('input'))
    expect(onChange).toHaveBeenCalledWith('abc')
    expect(ed.getMarkdown()).toBe('abc')
    ed.destroy()
  })

  it('empties the container on destroy', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'x', mode: 'source' })
    expect(c.childElementCount).toBe(1)
    ed.destroy()
    expect(c.childElementCount).toBe(0)
  })
})

describe('mountMarkdownEditor — mode switching', () => {
  it('defaults to rich mode', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'hello' })
    expect(ed.getMode()).toBe('rich')
    expect(ed.getMarkdown()).toBe('hello')
    ed.destroy()
  })

  it('carries the live text across source → rich → source', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'one', mode: 'source' })
    const ta = c.querySelector('textarea')!
    ta.value = 'two'
    ta.dispatchEvent(new Event('input'))

    await ed.setMode('rich')
    expect(ed.getMode()).toBe('rich')
    expect(ed.getMarkdown()).toBe('two')
    expect(c.querySelector('textarea')).toBeNull()

    await ed.setMode('source')
    expect(c.querySelector('textarea')!.value).toBe('two')
    expect(ed.getMarkdown()).toBe('two')
    ed.destroy()
  })

  it('tears the previous pane down when switching', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'x' })
    await ed.setMode('source')
    expect(richInstances[0].destroy).toHaveBeenCalled()
    expect(c.querySelectorAll('.moraya-editor').length).toBe(0)
    ed.destroy()
  })

  it('is a no-op when the mode is unchanged', async () => {
    const c = container()
    const onChange = vi.fn()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'x', mode: 'source', onChange })
    await ed.setMode('source')
    expect(onChange).not.toHaveBeenCalled()
    expect(c.querySelectorAll('textarea').length).toBe(1)
    ed.destroy()
  })
})

describe('mountMarkdownEditor — flush semantics', () => {
  it('flushes a pending rich-mode change before switching modes', async () => {
    const c = container()
    const onChange = vi.fn()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'draft', onChange })
    // Typed inside the 200 ms debounce window: the editor holds it, the
    // consumer has not been told, and moraya's destroy() would drop the timer.
    typeUnflushed('draft + last sentence')

    await ed.setMode('source')

    expect(onChange).toHaveBeenCalledWith('draft + last sentence')
    expect(ed.getMarkdown()).toBe('draft + last sentence')
    expect(c.querySelector('textarea')!.value).toBe('draft + last sentence')
    ed.destroy()
  })

  it('flushes a pending rich-mode change on destroy', async () => {
    const c = container()
    const onChange = vi.fn()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'draft', onChange })
    typeUnflushed('draft + last sentence')

    ed.destroy()

    expect(onChange).toHaveBeenCalledWith('draft + last sentence')
    expect(c.childElementCount).toBe(0)
  })

  it('does not emit when nothing changed', async () => {
    const c = container()
    const onChange = vi.fn()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'draft', onChange })
    await ed.setMode('source')
    ed.destroy()
    expect(onChange).not.toHaveBeenCalled()
  })

  it('emits once, not twice, for the same pending text', async () => {
    const c = container()
    const onChange = vi.fn()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'draft', onChange })
    typeUnflushed('typed')
    await ed.setMode('source')
    ed.destroy()
    expect(onChange).toHaveBeenCalledTimes(1)
  })
})

// `opts.placeholder` is read once at mount, so a consumer that rotates its
// prompt (Idea Spark shows a different one per new document) needs a setter —
// and the new text has to survive a mode switch, which re-mounts the pane.
describe('mountMarkdownEditor — setPlaceholder', () => {
  it('updates the live textarea in source mode', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: '', mode: 'source', placeholder: '第一句' })
    const ta = c.querySelector('textarea')!
    expect(ta.placeholder).toBe('第一句')
    ed.setPlaceholder('第二句')
    expect(ta.placeholder).toBe('第二句')
    ed.destroy()
  })

  it('routes to the rich pane in rich mode', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: '', placeholder: '第一句' })
    ed.setPlaceholder('第二句')
    expect(h.setRichPlaceholder).toHaveBeenCalledWith(richInstances[0], '第二句')
    ed.destroy()
  })

  it('remembers the new text across a mode switch', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: '', mode: 'source', placeholder: '第一句' })
    ed.setPlaceholder('第二句')
    await ed.setMode('rich')
    await ed.setMode('source')
    expect(c.querySelector('textarea')!.placeholder).toBe('第二句')
    ed.destroy()
  })
})

describe('mountMarkdownEditor — document base dir', () => {
  it('joins a vault-relative baseDir onto the vault root', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: '', mode: 'source', baseDir: 'inbox/ideas' })
    expect(h.setKitBaseDir).toHaveBeenCalledWith('/vault/inbox/ideas')
    ed.destroy()
  })

  it('still sets the base dir when baseDir is omitted', async () => {
    // moraya keeps the base dir in module-global state, so skipping the call
    // would leak the previous mount's directory into this one.
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: '', mode: 'source' })
    expect(h.setKitBaseDir).toHaveBeenCalledWith('/vault')
    ed.destroy()
  })
})

describe('KitOptions.powerMode contract', () => {
  it('distinguishes "omitted" from "explicit null"', () => {
    // 这条钉的是 main.ts 里 `'powerMode' in opts` 的判据:显式传 null 表示
    // 「调用方自管、别去问宿主」,与省略不是一回事。
    expect('powerMode' in ({ initialMarkdown: '' } as Record<string, unknown>)).toBe(false)
    expect('powerMode' in ({ initialMarkdown: '', powerMode: null } as Record<string, unknown>)).toBe(true)
  })
})

// 插件窗口用的就是这个 kit,所以主窗口修过的输入法问题必须在这儿也成立:
// 结束变换的那一下按键(退格删掉最后一个预编辑字符 / 回车确认候选)会在
// compositionend **之后**才到,ProseMirror 认得但只是不处理、不 preventDefault,
// 于是 contenteditable 自己又删一个已确定的字符。rich 模式必须把它取消掉。
describe('mountMarkdownEditor — 输入法收尾那一击(见 src/lib/ime.ts)', () => {
  function press(el: HTMLElement, key = 'Backspace') {
    const ev = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })
    el.dispatchEvent(ev)
    return ev
  }
  /** 站在编辑区里发一次「变换刚结束」。 */
  function endComposition(el: HTMLElement) {
    el.dispatchEvent(new Event('compositionstart', { bubbles: true }))
    el.dispatchEvent(new Event('compositionend', { bubbles: true }))
  }

  it('rich 模式:收尾那一击被取消', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'x', mode: 'rich' })
    const pm = c.querySelector('.moraya-editor') as HTMLElement
    endComposition(pm)
    expect(press(pm).defaultPrevented, '不取消的话 contenteditable 会多删一个字').toBe(true)
    expect(press(pm).defaultPrevented, '只吃一下,第二下是用户真的又按了').toBe(false)
    ed.destroy()
  })

  it('rich 模式:没变换过的按键原样放行', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'x', mode: 'rich' })
    const pm = c.querySelector('.moraya-editor') as HTMLElement
    expect(press(pm).defaultPrevented).toBe(false)
    ed.destroy()
  })

  it('切到 source 后不再插手 —— textarea 自己会挡,再取消一次是多的', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'x', mode: 'rich' })
    await ed.setMode('source')
    const ta = c.querySelector('textarea') as HTMLElement
    endComposition(ta)
    expect(press(ta).defaultPrevented).toBe(false)
    ed.destroy()
  })

  it('destroy 之后不留监听器', async () => {
    const c = container()
    const ed = await mountMarkdownEditor(c, { initialMarkdown: 'x', mode: 'rich' })
    const pm = c.querySelector('.moraya-editor') as HTMLElement
    ed.destroy()
    endComposition(pm)
    expect(press(pm).defaultPrevented).toBe(false)
  })
})
