import { describe, expect, it } from 'vitest'
import { Schema, type Node as PmNode } from 'prosemirror-model'
import { closeHistory, history, redoDepth, undo, undoDepth } from 'prosemirror-history'
import { EditorState, TextSelection, type Command, type Transaction } from 'prosemirror-state'
import type { EditorView } from 'prosemirror-view'
import { Mapping, Step } from 'prosemirror-transform'
import { BlockEditStep, governedRedo, governedUndo, withGovernedHistory } from './governed-history'
import { BLOCK_ID_ATTR } from './identity'

const schema = new Schema({
  nodes: {
    doc: { content: 'block+' },
    paragraph: { content: 'inline*', group: 'block', attrs: { [BLOCK_ID_ATTR]: { default: null } } },
    text: { group: 'inline' },
  },
  marks: { strong: {} },
})

function doc(...entries: Array<[string, string]>): PmNode {
  return schema.node('doc', null, entries.map(([id, text]) => schema.node('paragraph', { [BLOCK_ID_ATTR]: id }, text ? schema.text(text) : undefined)))
}

function entries(doc: PmNode): Array<[string, string]> {
  const result: Array<[string, string]> = []
  doc.forEach((node) => result.push([node.attrs[BLOCK_ID_ATTR], node.textContent]))
  return result
}

function harness(initial: PmNode) {
  let state = EditorState.create({ doc: initial, plugins: withGovernedHistory([history()], (tr) => !!tr.getMeta('remote')) })
  return {
    get state() { return state },
    apply(tr: Transaction) { state = state.apply(tr) },
    remote(next: PmNode) { state = state.apply(state.tr.replaceWith(0, state.doc.content.size, next.content).setMeta('remote', true).setMeta('addToHistory', false)) },
    command(command: Command) { return command(state, (tr) => { state = state.apply(tr) }) },
    edit(id: string, text: string, composition?: number) {
      let from = 0
      state.doc.forEach((node, pos) => { if (node.attrs[BLOCK_ID_ATTR] === id) from = pos + 1 })
      state = state.apply(state.tr.setSelection(TextSelection.create(state.doc, from)))
      let tr = state.tr.insertText(text)
      if (composition !== undefined) tr = tr.setMeta('composition', composition)
      state = state.apply(tr)
    },
  }
}

describe('governed ProseMirror history', () => {
  it('really undoes a local edit after the last block moves to the front, then redoes it', () => {
    const editor = harness(doc(['b', 'B'], ['a', 'A']))
    editor.edit('a', 'local ')
    editor.remote(doc(['a', 'local A'], ['b', 'B']))
    expect(undoDepth(editor.state)).toBe(1)
    expect(editor.command(governedUndo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['a', 'A'], ['b', 'B']])
    expect(editor.state.selection.$from.parent.attrs[BLOCK_ID_ATTR]).toBe('a')
    expect(editor.command(governedRedo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['a', 'local A'], ['b', 'B']])
  })

  it('keeps separate local histories on both sides of a remote move', () => {
    const editor = harness(doc(['a', 'A'], ['b', 'B'], ['c', 'C']))
    editor.edit('a', 'local ')
    editor.apply(closeHistory(editor.state.tr))
    editor.edit('b', 'local ')
    editor.remote(doc(['b', 'local B'], ['c', 'C'], ['a', 'local A']))
    expect(undoDepth(editor.state)).toBe(2)
    expect(editor.command(governedUndo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['b', 'B'], ['c', 'C'], ['a', 'local A']])
    expect(editor.command(governedUndo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['b', 'B'], ['c', 'C'], ['a', 'A']])
    expect(editor.command(governedRedo)).toBe(true)
    expect(editor.command(governedRedo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['b', 'local B'], ['c', 'C'], ['a', 'local A']])
  })

  it('does not mark a remotely interleaved block when undoing a cross-block mark change', () => {
    const editor = harness(doc(['a', 'AA'], ['b', 'BB'], ['c', 'CC']))
    editor.apply(editor.state.tr.addMark(1, 7, schema.mark('strong')))
    const [a, b, c] = [editor.state.doc.child(0), editor.state.doc.child(1), editor.state.doc.child(2)]
    editor.remote(schema.node('doc', null, [a, c, b]))
    expect(editor.command(governedUndo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['a', 'AA'], ['c', 'CC'], ['b', 'BB']])
    editor.state.doc.descendants((node) => { if (node.isText) expect(node.marks).toHaveLength(0) })
    expect(editor.command(governedRedo)).toBe(true)
    expect(editor.state.doc.child(1).firstChild!.marks).toHaveLength(0)
    expect(editor.state.doc.child(0).firstChild!.marks).toHaveLength(1)
    expect(editor.state.doc.child(2).firstChild!.marks).toHaveLength(1)
  })

  it('never deletes a remotely interleaved C when undoing replacement across A and B', () => {
    const editor = harness(doc(['a', 'A'], ['b', 'B'], ['c', 'C']))
    editor.apply(editor.state.tr.replaceWith(0, 6, doc(['a', 'AA'], ['b', 'BB']).content))
    editor.remote(doc(['a', 'AA'], ['c', 'C'], ['b', 'BB']))
    expect(editor.command(governedUndo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['a', 'A'], ['c', 'C'], ['b', 'B']])
  })

  it('undoes and redoes a structural move while preserving remote text on another block', () => {
    const editor = harness(doc(['a', 'A'], ['b', 'B'], ['c', 'C']))
    editor.apply(editor.state.tr.replaceWith(0, editor.state.doc.content.size, doc(['c', 'C'], ['a', 'A'], ['b', 'B']).content))
    editor.remote(doc(['c', 'C'], ['a', 'remote A'], ['b', 'B']))
    expect(editor.command(governedUndo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['a', 'remote A'], ['b', 'B'], ['c', 'C']])
    expect(editor.command(governedRedo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['c', 'C'], ['a', 'remote A'], ['b', 'B']])
  })

  it('rejects same-block remote replacement instead of claiming an empty undo succeeded', () => {
    const editor = harness(doc(['a', 'A'], ['b', 'B']))
    editor.edit('a', 'local ')
    editor.remote(doc(['a', 'remote A'], ['b', 'B']))
    const before = editor.state
    expect(governedUndo(editor.state)).toBe(false)
    expect(editor.command(governedUndo)).toBe(false)
    expect(editor.state).toBe(before)
    expect(undoDepth(editor.state)).toBe(1)
    expect(redoDepth(editor.state)).toBe(0)
  })

  it('rejects the entire grouped undo if one inverse conflicts, even when native undo has a partial change', () => {
    const editor = harness(doc(['a', 'A'], ['b', 'B']))
    editor.edit('a', 'local ', 1)
    editor.edit('b', 'local ', 1)
    expect(undoDepth(editor.state)).toBe(1)
    editor.remote(doc(['a', 'local A'], ['b', 'remote B']))
    let partial: Transaction | undefined
    undo(editor.state, (tr) => { partial = tr })
    expect(partial?.docChanged).toBe(true)
    const before = editor.state
    expect(editor.command(governedUndo)).toBe(false)
    expect(editor.state).toBe(before)
    expect(entries(editor.state.doc)).toEqual([['a', 'local A'], ['b', 'remote B']])
    expect(undoDepth(editor.state)).toBe(1)
  })

  it('rejects structural order conflicts without losing the history event', () => {
    const editor = harness(doc(['a', 'A'], ['b', 'B'], ['c', 'C']))
    editor.apply(editor.state.tr.delete(0, 3))
    editor.remote(doc(['c', 'C'], ['b', 'B']))
    expect(editor.command(governedUndo)).toBe(false)
    expect(entries(editor.state.doc)).toEqual([['c', 'C'], ['b', 'B']])
    expect(undoDepth(editor.state)).toBe(1)
  })

  it('keeps redo attached to its block across another remote move, including its text bookmark', () => {
    const editor = harness(doc(['a', 'A'], ['b', 'B'], ['c', 'C']))
    editor.edit('a', 'local ')
    editor.remote(doc(['c', 'C'], ['a', 'local A'], ['b', 'B']))
    editor.apply(editor.state.tr.setSelection(TextSelection.create(editor.state.doc, 5)))
    expect(editor.command(governedUndo)).toBe(true)
    editor.remote(doc(['b', 'B'], ['c', 'C'], ['a', 'A']))
    expect(editor.command(governedRedo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['b', 'B'], ['c', 'C'], ['a', 'local A']])
    expect(editor.state.selection.$from.parent.attrs[BLOCK_ID_ATTR]).toBe('a')
  })

  it('leaves a conflicted event available and can undo it after the exact expected body is restored', () => {
    const editor = harness(doc(['a', 'A'], ['b', 'B']))
    editor.edit('a', 'local ')
    editor.remote(doc(['a', 'remote A'], ['b', 'B']))
    expect(editor.command(governedUndo)).toBe(false)
    editor.remote(doc(['b', 'B'], ['a', 'local A']))
    expect(editor.command(governedUndo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['b', 'B'], ['a', 'A']])
  })

  it('consumes browser historyUndo input on conflict without dispatching a partial/empty undo', () => {
    const editor = harness(doc(['a', 'A']))
    editor.edit('a', 'local ')
    editor.remote(doc(['a', 'remote A']))
    const before = editor.state
    let prevented = false, dispatched = false
    const plugin = editor.state.plugins[0]
    const view = { state: editor.state, editable: true, dispatch() { dispatched = true } } as unknown as EditorView
    const event = { inputType: 'historyUndo', preventDefault() { prevented = true } } as unknown as InputEvent
    expect(plugin.props.handleDOMEvents!.beforeinput!.call(plugin, view, event)).toBe(true)
    expect(prevented).toBe(true)
    expect(dispatched).toBe(false)
    expect(editor.state).toBe(before)
    expect(undoDepth(editor.state)).toBe(1)
  })

  it('restores deleted content, multiple nodes per ID and a cleared document with native undo/redo', () => {
    const initial = doc(['a', 'A one'], ['a', 'A two'], ['b', 'B'])
    const editor = harness(initial)
    editor.apply(editor.state.tr.replaceWith(0, initial.content.size, doc(['empty', '']).content))
    expect(editor.command(governedUndo)).toBe(true)
    expect(editor.state.doc.eq(initial)).toBe(true)
    expect(editor.command(governedRedo)).toBe(true)
    expect(entries(editor.state.doc)).toEqual([['empty', '']])
  })

  it('round-trips the semantic step and revalidates its ID/body preconditions after mapping', () => {
    const before = doc(['a', 'A'], ['b', 'B']), after = doc(['a', 'AA'], ['b', 'B'])
    const step = Step.fromJSON(schema, JSON.parse(JSON.stringify(new BlockEditStep(before, after))))
    expect(step.apply(before).doc!.eq(after)).toBe(true)
    const moved = doc(['b', 'remote B'], ['a', 'A'])
    const applied = step.map(new Mapping())!.apply(moved)
    expect(entries(applied.doc!)).toEqual([['b', 'remote B'], ['a', 'AA']])
    expect(step.apply(doc(['a', 'new A'], ['b', 'B'])).failed).toContain('HISTORY_CONFLICT')
    expect(() => new BlockEditStep(doc(['a', 'A'], ['b', 'B'], ['a', 'duplicate']), after)).toThrow('HISTORY_IDENTITY')
  })

  it('preserves history depth/options and does not rewrite the actual transaction steps', () => {
    let state = EditorState.create({ doc: doc(['a', 'A']), plugins: withGovernedHistory([history({ depth: 1 })], () => false) })
    const tr = state.tr.insertText('local ', 1)
    const steps = [...tr.steps]
    state = state.apply(tr)
    expect(tr.steps).toEqual(steps)
    expect(tr.steps[0]).not.toBeInstanceOf(BlockEditStep)
    expect(undoDepth(state)).toBe(1)
    expect(governedUndo(state)).toBe(true)
  })
})
