import { parseDocument, isScalar, isMap } from 'yaml'
import type { FrontmatterViewFactory } from '@moraya/core'
import { segmentFrontmatter } from './frontmatter-segment'
import { renderMarkdownInline } from './plugins/host-render-html'
import { frontmatterInlineParts } from './frontmatter-inline'
import { t } from './i18n/store.svelte'

/**
 * Render YAML frontmatter for the rich editor. The block is segmented into
 * contiguous `key: value` regions (rendered as property rows with editable
 * scalar values) and other content (rendered as read-only markdown). See
 * docs/superpowers/specs/2026-07-08-frontmatter-table-render-design.md.
 *
 * `onChange` receives the full new raw YAML after a scalar value is edited.
 * DOM-only and pure enough to unit-test under happy-dom.
 */
export function renderFrontmatter(
  raw: string,
  onChange?: (newRaw: string) => void,
): HTMLElement {
  const root = document.createElement('div')
  root.className = 'frontmatter-segments'
  for (const seg of segmentFrontmatter(raw)) {
    if (seg.kind === 'kv') {
      root.appendChild(renderKvProperties(seg.text, seg.start, seg.end, raw, onChange))
    } else if (seg.text.trim() !== '') {
      root.appendChild(renderMdBlock(seg.text))
    }
  }
  return root
}

/** Kept for the DOM-builder tests; renders the whole block as one view. */
export function buildFrontmatterDom(raw: string): HTMLElement {
  return renderFrontmatter(raw)
}

function renderMdBlock(md: string): HTMLElement {
  const div = document.createElement('div')
  div.className = 'frontmatter-md'
  div.innerHTML = renderMarkdownInline(md)
  return div
}

function renderKvProperties(
  segText: string,
  segStart: number,
  segEnd: number,
  fullRaw: string,
  onChange?: (newRaw: string) => void,
): HTMLElement {
  let doc
  try {
    doc = parseDocument(segText)
  } catch {
    return rawFallback(segText)
  }
  // parseDocument collects syntax errors instead of throwing; a broken segment
  // (or one that isn't a mapping) falls back to wrapped raw text.
  if (doc.errors.length > 0 || !isMap(doc.contents)) return rawFallback(segText)
  const contents = doc.contents

  const properties = document.createElement('div')
  properties.className = 'frontmatter-properties'

  for (const pair of contents.items) {
    const key = String((pair.key as { value?: unknown })?.value ?? pair.key)
    const valueNode = pair.value

    const row = document.createElement('div')
    row.className = 'fm-property'
    const keyEl = document.createElement('div')
    keyEl.className = 'fm-key'
    keyEl.textContent = key

    const valEl = document.createElement('div')
    valEl.className = 'fm-val'

    if (isEditableScalar(valueNode)) {
      const originalValue = (valueNode as { value: unknown }).value
      const original = scalarText(valueNode)
      valEl.classList.add('fm-editable')
      valEl.contentEditable = 'true'
      valEl.spellcheck = false
      valEl.appendChild(renderInlineValue(original))
      wireScalarEdit(valEl, key, original, originalValue, segText, segStart, segEnd, fullRaw, onChange)
    } else {
      valEl.appendChild(renderReadonlyValue(valueNode?.toJSON?.() ?? null))
    }

    row.append(keyEl, valEl)
    properties.appendChild(row)
  }
  return properties
}

function isEditableScalar(node: unknown): boolean {
  if (!isScalar(node)) return false
  const v = (node as { value: unknown }).value
  if (v === null) return true
  if (typeof v === 'string') return !v.includes('\n')  // multi-line stays read-only
  return typeof v === 'number' || typeof v === 'boolean'
}

function scalarText(node: unknown): string {
  const v = (node as { value: unknown }).value
  return v == null ? '' : String(v)
}

/**
 * Interpret edited cell text keeping the field's original scalar type: a
 * numeric field stays numeric, a boolean stays boolean, everything else is a
 * string. Avoids turning `count: 3` into the quoted string `count: "5"`.
 */
function coerceLikeOriginal(text: string, originalValue: unknown): unknown {
  const t = text.trim()
  if (typeof originalValue === 'number') {
    const n = Number(t)
    return t !== '' && !Number.isNaN(n) ? n : text
  }
  if (typeof originalValue === 'boolean') {
    if (t === 'true') return true
    if (t === 'false') return false
    return text
  }
  return text
}

function wireScalarEdit(
  cell: HTMLElement,
  key: string,
  original: string,
  originalValue: unknown,
  segText: string,
  segStart: number,
  segEnd: number,
  fullRaw: string,
  onChange?: (newRaw: string) => void,
): void {
  const commit = () => {
    const next = cell.textContent ?? ''
    if (next === original) return
    try {
      const d = parseDocument(segText)
      d.set(key, coerceLikeOriginal(next, originalValue))
      const newSeg = String(d)
      const newRaw = fullRaw.slice(0, segStart) + newSeg + fullRaw.slice(segEnd)
      onChange?.(newRaw)
    } catch {
      // Keep the edit visible; a re-render will resync if the model changes.
    }
  }
  cell.addEventListener('blur', commit)
  cell.addEventListener('keydown', (e) => {
    const ev = e as KeyboardEvent
    if (ev.key === 'Enter') { ev.preventDefault(); cell.blur() }
    else if (ev.key === 'Escape') {
      ev.preventDefault()
      cell.replaceChildren(renderInlineValue(original))
      cell.blur()
    }
  })
}

function rawFallback(raw: string): HTMLElement {
  const pre = document.createElement('pre')
  pre.className = 'frontmatter-raw'
  pre.textContent = raw
  return pre
}

function renderReadonlyValue(value: unknown): Node {
  if (value == null) return document.createTextNode('')

  if (Array.isArray(value)) {
    const ul = document.createElement('ul')
    ul.className = 'fm-list'
    if (value.every(isChipValue)) ul.classList.add('fm-chips')
    for (const item of value) {
      const li = document.createElement('li')
      li.appendChild(renderReadonlyValue(item))
      ul.appendChild(li)
    }
    return ul
  }

  if (typeof value === 'object') {
    const box = document.createElement('div')
    box.className = 'fm-nested'
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      const line = document.createElement('div')
      line.className = 'fm-nested-line'
      const keyEl = document.createElement('span')
      keyEl.className = 'fm-nested-key'
      keyEl.textContent = `${k}: `
      line.appendChild(keyEl)
      line.appendChild(renderReadonlyValue(v))
      box.appendChild(line)
    }
    return box
  }

  // Scalar (incl. multi-line strings, which the `.fm-val` pre-wrap keeps intact).
  return renderInlineValue(String(value))
}

function isChipValue(value: unknown): boolean {
  return value == null || typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean'
}

function renderInlineValue(value: string): DocumentFragment {
  const fragment = document.createDocumentFragment()
  for (const part of frontmatterInlineParts(value)) {
    if (part.kind === 'text') {
      fragment.appendChild(document.createTextNode(part.text))
    } else if (part.kind === 'wikilink') {
      const link = document.createElement('span')
      link.className = 'wikilink fm-inline-link'
      link.dataset.wikilink = part.target
      decorateLinkLabel(link, part.raw, part.label)
      fragment.appendChild(link)
    } else if (part.kind === 'link') {
      const link = document.createElement('a')
      link.className = 'fm-inline-link'
      link.setAttribute('href', part.href)
      link.draggable = false
      decorateLinkLabel(link, part.raw, part.label)
      fragment.appendChild(link)
    } else {
      const link = document.createElement('span')
      link.className = 'url-autolink fm-inline-link'
      link.dataset.url = part.href
      link.textContent = part.raw
      fragment.appendChild(link)
    }
  }
  return fragment
}

/**
 * CSS shows the compact label in reading mode and reveals this raw child while
 * its editable value is focused. `textContent` always remains the raw source,
 * so an unchanged blur can never erase a wikilink target or Markdown href.
 */
function decorateLinkLabel(link: HTMLElement, raw: string, label: string): void {
  link.dataset.fmLabel = label
  link.setAttribute('aria-label', label)
  const source = document.createElement('span')
  source.className = 'fm-link-source'
  source.textContent = raw
  link.appendChild(source)
}

/** Top-level keys across all kv segments, for the collapsed-state summary. */
function summaryKeys(raw: string): string[] {
  const keys: string[] = []
  for (const seg of segmentFrontmatter(raw)) {
    if (seg.kind !== 'kv') continue
    try {
      const doc = parseDocument(seg.text)
      if (doc.errors.length === 0 && isMap(doc.contents)) {
        for (const p of doc.contents.items) {
          keys.push(String((p.key as { value?: unknown })?.value ?? p.key))
        }
      }
    } catch { /* ignore malformed segment */ }
  }
  return keys
}

/**
 * Wrap the frontmatter in a collapsible <details>. It starts collapsed so the
 * metadata doesn't dominate the document; the open/closed state is stashed on
 * the (persistent) container so it survives the re-render triggered by editing
 * a value. The <summary> shows the top-level keys as a hint.
 */
export function buildFrontmatterView(
  container: HTMLElement,
  raw: string,
  onChange?: (newRaw: string) => void,
): HTMLElement {
  const details = document.createElement('details')
  details.className = 'frontmatter-details'
  if (container.dataset.fmOpen === '1') details.open = true

  const summary = document.createElement('summary')
  summary.className = 'frontmatter-summary'
  const title = document.createElement('span')
  title.className = 'frontmatter-summary-title'
  title.textContent = t('frontmatter.metadata')
  const keys = summaryKeys(raw)
  const label = document.createElement('span')
  label.className = 'frontmatter-summary-keys'
  label.textContent = keys.length ? keys.join(', ') : 'frontmatter'
  summary.append(title, label)
  details.appendChild(summary)

  details.appendChild(renderFrontmatter(raw, onChange))

  details.addEventListener('toggle', () => {
    container.dataset.fmOpen = details.open ? '1' : '0'
  })
  return details
}

/** Factory wired into the moraya editor via `frontmatterViewFactory`. */
export const frontmatterFactory: FrontmatterViewFactory = {
  render(container: HTMLElement, raw: string, onChange?: (newRaw: string) => void) {
    container.appendChild(buildFrontmatterView(container, raw, onChange))
    return { destroy() { /* DOM owned by container; nothing to release */ } }
  },
}
