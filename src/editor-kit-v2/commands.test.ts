// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from 'vitest'
import { createSchema, parseMarkdown, serializeMarkdown } from '@moraya/core'
import { history } from 'prosemirror-history'
import { EditorState, Plugin, TextSelection } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import { canExecuteEditorCommand, executeEditorCommand } from './commands'

let view: EditorView | undefined
function editor(markdown = '中文 paragraph') {
  const schema = createSchema({ mediaResolver: {
    loadLocalImage: async (path: string) => path,
    loadLocalMedia: async (path: string) => path,
    loadRemoteMedia: async (path: string) => path,
  } })
  view = new EditorView(document.body, { state: EditorState.create({
    schema, doc: parseMarkdown(markdown, schema), plugins: [history()],
  }) })
  return view
}

function selectText(current: EditorView, text: string) {
  let start = -1
  current.state.doc.descendants((node, position) => {
    if (node.isText && node.text?.includes(text)) start = position + node.text.indexOf(text)
  })
  if (start < 0) throw new Error(`missing text ${text}`)
  current.dispatch(current.state.tr.setSelection(TextSelection.create(current.state.doc, start, start + text.length)))
}

afterEach(() => { view?.destroy(); view = undefined; document.body.innerHTML = '' })

describe('schema-local Editor Kit commands', () => {
  it('probes formatting without dispatching or changing the selection, then applies the instance schema mark', () => {
    const current = editor()
    selectText(current, '中文')
    const before = current.state
    expect(canExecuteEditorCommand(current, { kind: 'bold' })).toBe(true)
    expect(current.state).toBe(before)
    expect(executeEditorCommand(current, { kind: 'bold' })).toBe(true)
    expect(serializeMarkdown(current.state.doc)).toContain('**中文**')
    expect(executeEditorCommand(current, { kind: 'undo' })).toBe(true)
    expect(serializeMarkdown(current.state.doc)).not.toContain('**')
    expect(executeEditorCommand(current, { kind: 'redo' })).toBe(true)
    expect(serializeMarkdown(current.state.doc)).toContain('**中文**')
  })

  it('reports a transaction rejected by the governed surface as not applied', () => {
    const current = editor()
    current.updateState(current.state.reconfigure({ plugins: [new Plugin({ filterTransaction: (tr) => !tr.docChanged })] }))
    selectText(current, '中文')
    expect(executeEditorCommand(current, { kind: 'bold' })).toBe(false)
    expect(serializeMarkdown(current.state.doc)).not.toContain('**')
  })

  it('supports headings, paragraph, quote, lists, task items and indentation', () => {
    const current = editor('First\n\nSecond')
    expect(executeEditorCommand(current, { kind: 'heading', level: 2 })).toBe(true)
    expect(current.state.doc.firstChild?.type.name).toBe('heading')
    expect(executeEditorCommand(current, { kind: 'paragraph' })).toBe(true)
    expect(executeEditorCommand(current, { kind: 'blockquote' })).toBe(true)
    expect(executeEditorCommand(current, { kind: 'blockquote' })).toBe(true)
    current.dispatch(current.state.tr.setSelection(TextSelection.create(current.state.doc, 1, current.state.doc.content.size - 1)))
    expect(executeEditorCommand(current, { kind: 'task-list' })).toBe(true)
    expect(serializeMarkdown(current.state.doc)).toContain('- [ ] First')
    selectText(current, 'Second')
    expect(executeEditorCommand(current, { kind: 'indent' })).toBe(true)
    expect(executeEditorCommand(current, { kind: 'outdent' })).toBe(true)
    expect(current.state.doc.firstChild?.childCount).toBe(2)
  })

  it('inserts and replaces link marks without deleting selected text and rejects executable addresses', () => {
    const current = editor()
    selectText(current, '中文')
    expect(executeEditorCommand(current, { kind: 'link', href: 'https://example.com' })).toBe(true)
    expect(serializeMarkdown(current.state.doc)).toContain('[中文](https://example.com)')
    expect(executeEditorCommand(current, { kind: 'link', href: 'https://example.org' })).toBe(true)
    expect(serializeMarkdown(current.state.doc)).toContain('[中文](https://example.org)')
    expect(executeEditorCommand(current, { kind: 'unlink' })).toBe(true)
    expect(serializeMarkdown(current.state.doc)).not.toContain('https://')
    expect(canExecuteEditorCommand(current, { kind: 'link', href: 'javascript:alert(1)' })).toBe(false)
    expect(canExecuteEditorCommand(current, { kind: 'link', href: 'java\nscript:alert(1)' })).toBe(false)
    expect(canExecuteEditorCommand(current, { kind: 'link', href: 'https://example.com/\tpage' })).toBe(false)
    expect(canExecuteEditorCommand(current, { kind: 'image', src: 'file:///private/image.png' })).toBe(false)
    expect(canExecuteEditorCommand(current, { kind: 'image', src: 'data:text/html,test' })).toBe(false)
    expect(canExecuteEditorCommand(current, { kind: 'image', src: './images/photo.png' })).toBe(true)
    expect(canExecuteEditorCommand(current, { kind: 'link', href: 'mailto:team@example.com' })).toBe(true)
  })

  it('switches ordered, bullet and task list types without adding unintended nesting and toggles the same list off', () => {
    const current = editor('1. First\n2. Second')
    selectText(current, 'First')
    expect(executeEditorCommand(current, { kind: 'bullet-list' })).toBe(true)
    expect(current.state.doc.firstChild?.type.name).toBe('bullet_list')
    expect(current.state.doc.firstChild?.childCount).toBe(2)
    expect(executeEditorCommand(current, { kind: 'task-list' })).toBe(true)
    expect(serializeMarkdown(current.state.doc)).toContain('- [ ] First')
    expect(executeEditorCommand(current, { kind: 'ordered-list' })).toBe(true)
    expect(serializeMarkdown(current.state.doc)).toContain('1. First')
    expect(serializeMarkdown(current.state.doc)).not.toContain('[ ]')
    expect(executeEditorCommand(current, { kind: 'ordered-list' })).toBe(true)
    expect(current.state.doc.firstChild?.type.name).toBe('paragraph')
  })

  it('creates tables and edits rows/columns while preserving data, including header promotion', () => {
    const current = editor('Before')
    expect(executeEditorCommand(current, { kind: 'table' })).toBe(true)
    expect(canExecuteEditorCommand(current, { kind: 'table.add-row' })).toBe(true)
    let tablePosition = -1
    current.state.doc.descendants((node, position) => { if (node.type.name === 'table') tablePosition = position })
    current.dispatch(current.state.tr.setSelection(TextSelection.near(current.state.doc.resolve(tablePosition + 3))))
    current.dispatch(current.state.tr.insertText('Heading'))
    expect(executeEditorCommand(current, { kind: 'table.add-row' })).toBe(true)
    current.dispatch(current.state.tr.insertText('Body'))
    expect(executeEditorCommand(current, { kind: 'table.add-column' })).toBe(true)
    expect(executeEditorCommand(current, { kind: 'table.delete-column' })).toBe(true)
    selectText(current, 'Heading')
    expect(executeEditorCommand(current, { kind: 'table.delete-row' })).toBe(true)
    const table = current.state.doc.nodeAt(tablePosition)!
    expect(table.childCount).toBe(3)
    expect(table.firstChild?.type.name).toBe('table_header_row')
    expect(table.firstChild?.firstChild?.type.name).toBe('table_header')
    expect(table.textContent).toContain('Body')
    expect(table.textContent).not.toContain('Heading')
  })

  it('advances the final table cell by appending one row and keeps previous navigation document-neutral', () => {
    const current = editor('| A | B |\n| --- | --- |\n| C | D |')
    selectText(current, 'D')
    expect(executeEditorCommand(current, { kind: 'table.next-cell' })).toBe(true)
    expect(current.state.doc.firstChild?.childCount).toBe(3)
    const doc = current.state.doc
    expect(executeEditorCommand(current, { kind: 'table.previous-cell' })).toBe(true)
    expect(current.state.doc).toBe(doc)
  })
})
