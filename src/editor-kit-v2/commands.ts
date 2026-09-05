import { lift, setBlockType, toggleMark, wrapIn } from 'prosemirror-commands'
import { liftListItem, sinkListItem, wrapInList } from 'prosemirror-schema-list'
import { TextSelection, type Command, type EditorState, type Transaction } from 'prosemirror-state'
import type { Node as PmNode } from 'prosemirror-model'
import type { EditorView } from 'prosemirror-view'
import type { EditorCommand } from './contract'
import { governedRedo, governedUndo } from './governed-history'

const marks: Record<string, string> = {
  bold: 'strong', italic: 'em', strikethrough: 'strike_through', code: 'code', highlight: 'highlight',
}

function children(node: PmNode): PmNode[] {
  const result: PmNode[] = []
  node.forEach((child) => result.push(child))
  return result
}

function tableContext(state: EditorState) {
  const { $from } = state.selection
  for (let depth = $from.depth; depth > 0; depth--) {
    if ($from.node(depth).type.name !== 'table') continue
    if ($from.depth < depth + 2) return null
    return { table: $from.node(depth), pos: $from.before(depth), row: $from.index(depth), column: $from.index(depth + 1) }
  }
  return null
}

function tableCommand(kind: EditorCommand['kind']): Command {
  return (state, dispatch) => {
    const { table, table_header_row, table_row, table_header, table_cell, paragraph } = state.schema.nodes
    if (!table || !table_header_row || !table_row || !table_header || !table_cell || !paragraph) return false
    const cell = (header = false) => (header ? table_header : table_cell).createAndFill({ alignment: 'left' })!
    if (kind === 'table') {
      if (!dispatch) return true
      const tableNode = table.create(null, [
        table_header_row.create(null, [cell(true), cell(true), cell(true)]),
        table_row.create(null, [cell(), cell(), cell()]),
        table_row.create(null, [cell(), cell(), cell()]),
      ])
      const tr = state.tr.replaceSelectionWith(tableNode)
      tr.doc.descendants((node, position) => {
        if (node === tableNode) tr.setSelection(TextSelection.near(tr.doc.resolve(position + 3)))
      })
      dispatch(tr.scrollIntoView())
      return true
    }
    const context = tableContext(state)
    if (!context) return false
    const rows = children(context.table)
    let row = context.row, column = context.column
    if (kind === 'table.add-row') {
      rows.splice(row + 1, 0, table_row.create(null, Array.from({ length: rows[row].childCount }, () => cell())))
      row++
    } else if (kind === 'table.delete-row') {
      if (rows.length <= 1) return false
      rows.splice(row, 1)
      if (row === 0) rows[0] = table_header_row.create(null, children(rows[0]).map((item) => table_header.create(item.attrs, item.content)))
      row = Math.min(row, rows.length - 1)
    } else if (kind === 'table.add-column' || kind === 'table.delete-column') {
      if (kind === 'table.delete-column' && rows[0].childCount <= 1) return false
      for (let index = 0; index < rows.length; index++) {
        const cells = children(rows[index])
        if (kind === 'table.add-column') cells.splice(column + 1, 0, cell(index === 0))
        else cells.splice(column, 1)
        rows[index] = rows[index].type.create(rows[index].attrs, cells)
      }
      column = kind === 'table.add-column' ? column + 1 : Math.min(column, rows[0].childCount - 1)
    } else if (kind === 'table.next-cell' || kind === 'table.previous-cell') {
      const step = kind === 'table.next-cell' ? 1 : -1
      column += step
      if (column < 0) { row--; column = row >= 0 ? rows[row].childCount - 1 : 0 }
      if (row >= 0 && column >= rows[row].childCount) { row++; column = 0 }
      if (row < 0) return false
      if (row >= rows.length) rows.push(table_row.create(null, Array.from({ length: rows[0].childCount }, () => cell())))
    } else return false
    if (!dispatch) return true
    const replacement = table.create(context.table.attrs, rows)
    const tr = state.tr
    if (!replacement.eq(context.table)) tr.replaceWith(context.pos, context.pos + context.table.nodeSize, replacement)
    let position = context.pos + 1
    for (let index = 0; index < row; index++) position += rows[index].nodeSize
    position += 1
    for (let index = 0; index < column; index++) position += rows[row].child(index).nodeSize
    tr.setSelection(TextSelection.near(tr.doc.resolve(position + 1))).scrollIntoView()
    dispatch(tr)
    return true
  }
}

function safeAddress(value: string): boolean {
  const address = value.trim()
  if (!address || /[\u0000-\u001f\u007f]/.test(value)) return false
  const scheme = /^([a-z][a-z0-9+.-]*):/i.exec(address)?.[1]
  return !scheme || /^(https?|mailto|tel)$/i.test(scheme)
}

function commandFor(command: EditorCommand, state: EditorState): Command | null {
  const { nodes, marks: schemaMarks } = state.schema
  const mark = schemaMarks[marks[command.kind]]
  if (mark) return toggleMark(mark)
  if (command.kind.startsWith('table')) return tableCommand(command.kind)
  switch (command.kind) {
    case 'undo': return governedUndo
    case 'redo': return governedRedo
    case 'paragraph': return nodes.paragraph ? setBlockType(nodes.paragraph) : null
    case 'heading': return nodes.heading ? setBlockType(nodes.heading, { level: command.level }) : null
    case 'code-block': return nodes.code_block ? setBlockType(nodes.code_block, { language: '' }) : null
    case 'blockquote': {
      if (!nodes.blockquote) return null
      const { $from } = state.selection
      for (let depth = $from.depth; depth > 0; depth--) if ($from.node(depth).type === nodes.blockquote) return lift
      return wrapIn(nodes.blockquote)
    }
    case 'indent': return nodes.list_item ? sinkListItem(nodes.list_item) : null
    case 'outdent': return nodes.list_item ? liftListItem(nodes.list_item) : null
    case 'bullet-list': case 'ordered-list': case 'task-list': {
      const type = command.kind === 'ordered-list' ? nodes.ordered_list : nodes.bullet_list
      if (!type || !nodes.list_item) return null
      return (current, dispatch) => {
        const { $from } = current.selection
        for (let depth = $from.depth; depth > 0; depth--) {
          const list = $from.node(depth)
          if (list.type !== nodes.bullet_list && list.type !== nodes.ordered_list) continue
          const isTask = children(list).every((item) => item.attrs.checked !== null)
          const wantTask = command.kind === 'task-list'
          if (list.type === type && isTask === wantTask) return liftListItem(nodes.list_item)(current, dispatch)
          if (!dispatch) return true
          const tr = current.tr
          if (list.type !== type) tr.setNodeMarkup($from.before(depth), type)
          // Only this list's direct items change. Nested task lists keep their
          // own checked state when the parent switches style.
          let position = $from.before(depth) + 1
          list.forEach((item) => {
            tr.setNodeMarkup(position, undefined, { ...item.attrs, checked: wantTask ? false : null })
            position += item.nodeSize
          })
          dispatch(tr)
          return true
        }
        if (!wrapInList(type)(current)) return false
        if (!dispatch) return true
        return wrapInList(type)(current, (tr) => {
          if (command.kind === 'task-list') {
            const start = tr.selection.$from
            for (let depth = start.depth; depth > 0; depth--) {
              if (start.node(depth).type !== type) continue
              tr.doc.nodesBetween(start.before(depth), start.after(depth), (node, pos) => {
                if (node.type === nodes.list_item) tr.setNodeMarkup(pos, undefined, { ...node.attrs, checked: false })
              })
              break
            }
          }
          dispatch(tr)
        })
      }
    }
    case 'horizontal-rule': return nodes.horizontal_rule ? (current, dispatch) => {
      if (dispatch) dispatch(current.tr.replaceSelectionWith(nodes.horizontal_rule.create()).scrollIntoView())
      return true
    } : null
    case 'unlink': return schemaMarks.link ? (current, dispatch) => {
      const { from, to } = current.selection
      if (from === to || !current.doc.rangeHasMark(from, to, schemaMarks.link)) return false
      if (dispatch) dispatch(current.tr.removeMark(from, to, schemaMarks.link))
      return true
    } : null
    case 'link': return schemaMarks.link && safeAddress(command.href) ? (current, dispatch) => {
      if (!dispatch) return true
      const { from, to } = current.selection
      const tr = current.tr
      if (from === to) {
        const text = command.text?.trim() || command.href.trim()
        tr.insertText(text, from, to).addMark(from, from + text.length, schemaMarks.link.create({ href: command.href.trim() }))
      } else tr.addMark(from, to, schemaMarks.link.create({ href: command.href.trim() }))
      dispatch(tr.scrollIntoView())
      return true
    } : null
    case 'image': return nodes.image && safeAddress(command.src) ? (current, dispatch) => {
      if (dispatch) dispatch(current.tr.replaceSelectionWith(nodes.image.create({ src: command.src.trim(), alt: command.alt ?? '' })).scrollIntoView())
      return true
    } : null
    default: return null
  }
}

/** A probe never receives dispatch and cannot change the document or selection. */
export function canExecuteEditorCommand(view: EditorView, command: EditorCommand): boolean {
  return commandFor(command, view.state)?.(view.state) ?? false
}

export function executeEditorCommand(view: EditorView, command: EditorCommand): boolean {
  const before = view.state
  let transaction: Transaction | undefined
  const handled = commandFor(command, before)?.(before, (tr) => { transaction = tr }, view) ?? false
  if (!handled || !transaction) return false
  view.dispatch(transaction)
  if (view.state === before) return false
  view.focus()
  return true
}
