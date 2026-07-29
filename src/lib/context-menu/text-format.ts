// Pure text helpers shared by SourceView keyboard shortcuts and the source
// context-menu adapter. No DOM, no ProseMirror — trivially unit-testable.

export interface WrapResult {
  value: string
  selStart: number
  selEnd: number
}

/**
 * Toggle a paired marker (open/close) around [start,end) in `value`.
 * Handles three cases, mirroring SourceView's existing Cmd+B logic:
 *  1. selection already includes the markers → strip them
 *  2. markers sit just outside the selection → strip them
 *  3. otherwise wrap (or insert empty markers on a collapsed selection)
 */
export function applyWrap(
  value: string, start: number, end: number, open: string, close: string,
): WrapResult {
  const sel = value.slice(start, end)
  const selWrapped = sel.startsWith(open) && sel.endsWith(close)
                  && sel.length > open.length + close.length
  const beforeOpen = start >= open.length && value.slice(start - open.length, start) === open
  const afterClose = value.slice(end, end + close.length) === close
  const outerWrapped = beforeOpen && afterClose

  if (selWrapped) {
    const inner = sel.slice(open.length, sel.length - close.length)
    return { value: value.slice(0, start) + inner + value.slice(end),
             selStart: start, selEnd: start + inner.length }
  }
  if (outerWrapped) {
    const newStart = start - open.length
    return { value: value.slice(0, newStart) + sel + value.slice(end + close.length),
             selStart: newStart, selEnd: newStart + sel.length }
  }
  return {
    value: value.slice(0, start) + open + sel + close + value.slice(end),
    selStart: start + open.length, selEnd: end + open.length,
  }
}

const WORD_CHAR = /[\w一-龥]/

/** Expand a cursor position to the surrounding word run; collapsed if not on a word. */
export function expandToWord(value: string, cursor: number): { start: number; end: number } {
  if (!WORD_CHAR.test(value[cursor] ?? '') && !WORD_CHAR.test(value[cursor - 1] ?? '')) {
    return { start: cursor, end: cursor }
  }
  let start = cursor
  let end = cursor
  while (start > 0 && WORD_CHAR.test(value[start - 1])) start--
  while (end < value.length && WORD_CHAR.test(value[end])) end++
  return { start, end }
}

/**
 * Insert a CriticMarkup annotation at [start,end): wraps a non-empty selection
 * as `{==sel==}{>>seed<<}`, or inserts a bare `{>>seed<<}` on a collapsed
 * selection. The caret lands right before `seed` (i.e. between `>>` and the
 * note text) so the user can type the note directly.
 *
 * `seed` is what "Ask" pre-fills: `'?'` makes the annotation a question the
 * moment it exists, so what you then type reads as a question mark-suffixed
 * sentence without the user having to remember the `?`.
 */
export function insertNoteMarkup(
  value: string, start: number, end: number, seed = '',
): WrapResult {
  const sel = value.slice(start, end)
  const insert = sel ? `{==${sel}==}{>>${seed}<<}` : `{>>${seed}<<}`
  const caret = start + insert.length - 3 - seed.length
  return {
    value: value.slice(0, start) + insert + value.slice(end),
    selStart: caret,
    selEnd: caret,
  }
}
