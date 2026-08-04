// Source mode for the Editor Kit: a transparent <textarea> over a
// syntax-highlighted <pre>, the same construction the main window uses in
// `src/components/SourceView.svelte`. The textarea owns the caret and editing;
// the <pre> owns everything the user sees. Highlighting and auto-pairing come
// from the main program's zero-dependency modules, so behaviour cannot drift.

import { renderSourceHtml } from '../lib/source-highlight'
import { autoPairInsert } from '../lib/autopair'

export interface SourcePane {
  getValue(): string
  setValue(v: string): void
  /** Replaces the hint shown in an empty buffer. */
  setPlaceholder(text: string): void
  focus(): void
  destroy(): void
}

export function mountSource(
  host: HTMLElement,
  initial: string,
  onChange: (v: string) => void,
  placeholder?: string,
): SourcePane {
  const wrap = document.createElement('div')
  wrap.className = 'kit-source'

  const pre = document.createElement('pre')
  pre.className = 'kit-source-hl'
  pre.setAttribute('aria-hidden', 'true')

  const ta = document.createElement('textarea')
  ta.className = 'kit-source-ta'
  ta.spellcheck = false
  ta.value = initial
  if (placeholder) ta.placeholder = placeholder

  const paint = () => { pre.innerHTML = renderSourceHtml(ta.value, [], -1) }

  ta.addEventListener('input', () => { paint(); onChange(ta.value) })

  // Auto-close paired markdown markers ([[ ** __ ^^ ~~ == and `), matching
  // SourceView.svelte: collapsed selection, single printable key, no modifiers.
  ta.addEventListener('keydown', (ev) => {
    if (ev.metaKey || ev.ctrlKey || ev.altKey) return
    if (ev.key.length !== 1) return
    const pos = ta.selectionStart ?? 0
    if (pos !== (ta.selectionEnd ?? 0)) return
    const res = autoPairInsert(ta.value, pos, ev.key)
    if (!res) return
    ev.preventDefault()
    ta.value = ta.value.slice(0, pos) + res.insert + ta.value.slice(pos)
    ta.setSelectionRange(pos + res.caret, pos + res.caret)
    paint()
    onChange(ta.value)
  })

  ta.addEventListener('scroll', () => {
    pre.scrollTop = ta.scrollTop
    pre.scrollLeft = ta.scrollLeft
  })

  wrap.append(pre, ta)
  host.appendChild(wrap)
  paint()

  return {
    getValue: () => ta.value,
    setValue: (v) => { ta.value = v; paint() },
    // Empty string clears it: `placeholder=""` renders nothing, which is what
    // "no hint" means for a textarea.
    setPlaceholder: (text) => { ta.placeholder = text },
    focus: () => ta.focus(),
    destroy: () => wrap.remove(),
  }
}
