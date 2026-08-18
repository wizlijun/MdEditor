import type { EditorActions } from './EditorContextMenu.svelte'
import { applyWrap, expandToWord, insertNoteMarkup } from './text-format'
import { setContent } from '../tabs.svelte'

const WRAP_BY_ID: Record<string, [string, string]> = {
  bold: ['**', '**'], italic: ['*', '*'], highlight: ['^^', '^^'],
  strike: ['~~', '~~'], code: ['`', '`'],
}

export interface SourceHandle {
  el: HTMLTextAreaElement
  tabId: string
  value(): string
}

function replaceRange(h: SourceHandle, start: number, end: number, text: string, caret?: number) {
  const v = h.value()
  const next = v.slice(0, start) + text + v.slice(end)
  setContent(h.tabId, next)
  const pos = caret ?? start + text.length
  requestAnimationFrame(() => { h.el.focus(); h.el.setSelectionRange(pos, pos) })
}

function wrap(h: SourceHandle, open: string, close: string) {
  let start = h.el.selectionStart ?? 0
  let end = h.el.selectionEnd ?? 0
  if (start === end) { const w = expandToWord(h.value(), start); start = w.start; end = w.end }
  const r = applyWrap(h.value(), start, end, open, close)
  setContent(h.tabId, r.value)
  requestAnimationFrame(() => { h.el.focus(); h.el.setSelectionRange(r.selStart, r.selEnd) })
}

function wikilink(h: SourceHandle) {
  let start = h.el.selectionStart ?? 0
  let end = h.el.selectionEnd ?? 0
  if (start === end) { const w = expandToWord(h.value(), start); start = w.start; end = w.end }
  const inner = h.value().slice(start, end)
  const text = `[[${inner}]]`
  replaceRange(h, start, end, text, inner ? start + text.length : start + 2)
}

function link(h: SourceHandle) {
  const start = h.el.selectionStart ?? 0
  const end = h.el.selectionEnd ?? 0
  if (start === end) return
  const inner = h.value().slice(start, end)
  const text = `[${inner}](url)`
  replaceRange(h, start, end, text, start + inner.length + 3)
  requestAnimationFrame(() =>
    h.el.setSelectionRange(start + inner.length + 3, start + inner.length + 6))
}

/** Toggle a single-line prefix on the line containing the cursor. */
function linePrefix(h: SourceHandle, prefix: string) {
  const v = h.value()
  const pos = h.el.selectionStart ?? 0
  const lineStart = v.lastIndexOf('\n', pos - 1) + 1
  const lineEnd = v.indexOf('\n', pos) === -1 ? v.length : v.indexOf('\n', pos)
  const line = v.slice(lineStart, lineEnd)
  const stripped = line.replace(/^(#{1,6}\s|>\s|-\s\[[ x]\]\s|-\s|\d+\.\s)/, '')
  const next = line === prefix + stripped ? stripped : prefix + stripped
  replaceRange(h, lineStart, lineEnd, next, lineStart + next.length)
}

function insertText(h: SourceHandle, text: string) {
  const start = h.el.selectionStart ?? 0
  const end = h.el.selectionEnd ?? 0
  replaceRange(h, start, end, text)
}

/**
 * Paste clipboard text at the current selection. `execCommand('paste')` is
 * blocked in Tauri's WKWebView, so read the clipboard directly and splice.
 */
async function pasteText(h: SourceHandle) {
  try {
    const text = await navigator.clipboard.readText()
    if (!text) return
    insertText(h, text)
  } catch { /* clipboard permission denied */ }
}

export function createSourceActions(h: SourceHandle): EditorActions {
  return {
    canRun(id) {
      if (id === 'link') return (h.el.selectionStart ?? 0) !== (h.el.selectionEnd ?? 0)
      return true
    },
    async run(id) {
      if (id in WRAP_BY_ID) { const [o, c] = WRAP_BY_ID[id]; return wrap(h, o, c) }
      switch (id) {
        case 'cut':       h.el.focus(); document.execCommand('cut'); return
        case 'copy':      h.el.focus(); document.execCommand('copy'); return
        case 'paste':     return pasteText(h)
        case 'selectAll': h.el.focus(); h.el.select(); return
        case 'wikilink':  return wikilink(h)
        case 'note':
        case 'question': {
          const start = h.el.selectionStart ?? 0
          const end = h.el.selectionEnd ?? 0
          // 'question' seeds the note with `?` so it reads as a question the
          // moment it exists; the caret lands before it.
          const r = insertNoteMarkup(h.value(), start, end, id === 'question' ? '?' : '')
          setContent(h.tabId, r.value)
          requestAnimationFrame(() => { h.el.focus(); h.el.setSelectionRange(r.selStart, r.selEnd) })
          return
        }
        case 'trace': {
          const text = h.value().slice(h.el.selectionStart ?? 0, h.el.selectionEnd ?? 0)
          if (!text.trim()) return
          const { openTraceDelegation } = await import('./trace-action')
          return openTraceDelegation(text)
        }
        case 'link':      return link(h)
        case 'h1':        return linePrefix(h, '# ')
        case 'h2':        return linePrefix(h, '## ')
        case 'h3':        return linePrefix(h, '### ')
        case 'quote':     return linePrefix(h, '> ')
        case 'bullet':    return linePrefix(h, '- ')
        case 'ordered':   return linePrefix(h, '1. ')
        case 'task':      return linePrefix(h, '- [ ] ')
        case 'codeblock': return insertText(h, '```\n\n```\n')
        case 'hr':        return insertText(h, '\n---\n')
        case 'table':     return insertText(h, '| A | B | C |\n| --- | --- | --- |\n|  |  |  |\n')
        case 'math':      return insertText(h, '$$\n\n$$\n')
        case 'mermaid':   return insertText(h, '```mermaid\n\n```\n')
        case 'date':      return insertText(h, new Date().toISOString().slice(0, 10))
        case 'image': {
          const { open } = await import('@tauri-apps/plugin-dialog')
          const result = await open({ multiple: false,
            filters: [{ name: 'Images', extensions: ['png','jpg','jpeg','gif','svg','webp','bmp','avif'] }] })
          if (typeof result !== 'string') return
          insertText(h, `![](${result})`)
          return
        }
      }
    },
  }
}
