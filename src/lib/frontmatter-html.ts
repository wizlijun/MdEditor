import { parseDocument, isMap } from 'yaml'
import { segmentFrontmatter } from './frontmatter-segment'
import { htmlEscape, renderMarkdownInline } from './plugins/host-render-html'

/**
 * Static (string-only) frontmatter renderer for baked HTML output — the shared
 * page. Mirrors the rich editor's collapsed key/value view (frontmatter-view.ts)
 * but produces plain HTML with no contenteditable cells and no event wiring,
 * so it needs no DOM and survives being posted to the share Worker.
 *
 * Without this the raw `---` block leaks into the shared page: marked has no
 * frontmatter rule, so `---\ntitle: x\n---` renders as an <hr>, a paragraph of
 * YAML, and a second <hr> — metadata shouting over the actual document.
 */

/** Frontmatter block (without the `---` fences) plus the remaining body. */
export interface SplitDoc {
  /** null when the document has no frontmatter fence at all. */
  frontmatter: string | null
  body: string
}

/**
 * Split a leading `---` … `---` frontmatter block off a markdown source.
 * Only a fence on the very first line counts — a `---` thematic break further
 * down the document is body content, not metadata.
 */
export function splitFrontmatter(md: string): SplitDoc {
  if (!/^---[ \t]*\r?(\n|$)/.test(md)) return { frontmatter: null, body: md }
  const lines = md.split('\n')
  for (let i = 1; i < lines.length; i++) {
    if (lines[i].replace(/\r$/, '').trimEnd() !== '---') continue
    const fm = lines.slice(1, i).map((l) => l.replace(/\r$/, '')).join('\n')
    return { frontmatter: fm, body: lines.slice(i + 1).join('\n') }
  }
  // Unterminated fence — treat the whole thing as body rather than swallowing
  // the document into a metadata box.
  return { frontmatter: null, body: md }
}

/**
 * Render a frontmatter block as a COLLAPSED `<details>`: the summary lists the
 * top-level keys, the body holds the key/value table (plus any non-key:value
 * prose regions, rendered as markdown). Returns '' for an empty block — there
 * is nothing worth a disclosure widget.
 */
export function frontmatterDetailsHtml(fm: string): string {
  if (fm.trim() === '') return ''
  const parts: string[] = []
  const keys: string[] = []
  for (const seg of segmentFrontmatter(fm)) {
    if (seg.kind === 'kv') {
      const table = kvTableHtml(seg.text)
      parts.push(table.html)
      keys.push(...table.keys)
    } else if (seg.text.trim() !== '') {
      parts.push(`<div class="frontmatter-md">${renderMarkdownInline(seg.text)}</div>`)
    }
  }
  if (!parts.length) return ''
  const label = keys.length ? keys.join(', ') : 'frontmatter'
  return (
    '<details class="frontmatter-details">' +
    `<summary class="frontmatter-summary"><span class="frontmatter-summary-keys">${htmlEscape(label)}</span></summary>` +
    `<div class="frontmatter-segments">${parts.join('')}</div>` +
    '</details>'
  )
}

function kvTableHtml(segText: string): { html: string; keys: string[] } {
  let doc
  try {
    doc = parseDocument(segText)
  } catch {
    return { html: rawFallbackHtml(segText), keys: [] }
  }
  // parseDocument collects syntax errors instead of throwing; a broken segment
  // (or one that isn't a mapping) falls back to wrapped raw text.
  if (doc.errors.length > 0 || !isMap(doc.contents)) {
    return { html: rawFallbackHtml(segText), keys: [] }
  }

  const keys: string[] = []
  const rows: string[] = []
  for (const pair of doc.contents.items) {
    const key = String((pair.key as { value?: unknown })?.value ?? pair.key)
    keys.push(key)
    const value = (pair.value as { toJSON?: () => unknown } | null)?.toJSON?.() ?? null
    rows.push(
      `<tr><td class="fm-key">${htmlEscape(key)}</td><td class="fm-val">${valueHtml(value)}</td></tr>`,
    )
  }
  return { html: `<table class="frontmatter-table"><tbody>${rows.join('')}</tbody></table>`, keys }
}

function valueHtml(value: unknown): string {
  if (value == null) return ''
  if (Array.isArray(value)) {
    return `<ul class="fm-list">${value.map((v) => `<li>${valueHtml(v)}</li>`).join('')}</ul>`
  }
  if (typeof value === 'object') {
    const lines = Object.entries(value as Record<string, unknown>).map(
      ([k, v]) =>
        `<div class="fm-nested-line"><span class="fm-nested-key">${htmlEscape(k)}: </span>${valueHtml(v)}</div>`,
    )
    return `<div class="fm-nested">${lines.join('')}</div>`
  }
  // Scalar (incl. multi-line strings, which the `.fm-val` pre-wrap keeps intact).
  return htmlEscape(String(value))
}

function rawFallbackHtml(raw: string): string {
  return `<pre class="frontmatter-raw">${htmlEscape(raw)}</pre>`
}

/**
 * Styling for the baked `<details>`. Kept in rgba (not the editor's
 * `color-mix(… Canvas)`) to match the share page's hardcoded light/dark
 * palette, and scoped under `.moraya-editor` so it outranks theme skins that
 * style bare `table`/`td`. `white-space: normal` guards against skins that put
 * the editor in pre-wrap, which would turn the tag-to-tag whitespace of a
 * pretty-printed fragment into visible blank lines.
 */
export const FRONTMATTER_CSS = `
.moraya-editor .frontmatter-details {
  white-space: normal;
  margin: 0 0 1.4em;
  border: 1px solid rgba(0,0,0,0.12);
  border-radius: 6px;
  background: rgba(0,0,0,0.02);
}
.moraya-editor .frontmatter-summary {
  cursor: pointer;
  padding: 4px 10px;
  font-size: 0.85em;
  opacity: 0.6;
  user-select: none;
  list-style-position: inside;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.moraya-editor .frontmatter-details[open] > .frontmatter-summary {
  border-bottom: 1px solid rgba(0,0,0,0.1);
  margin-bottom: 6px;
}
.moraya-editor .frontmatter-segments { padding: 0 10px 8px; }
.moraya-editor .frontmatter-table {
  border-collapse: collapse;
  width: 100%;
  table-layout: fixed;
  font-size: 0.92em;
  margin: 0;
}
.moraya-editor .frontmatter-table td {
  border: 1px solid rgba(0,0,0,0.12);
  padding: 4px 8px;
  vertical-align: top;
  white-space: pre-wrap;
  overflow-wrap: break-word;
  word-break: break-word;
}
.moraya-editor .frontmatter-table .fm-key {
  width: 30%;
  font-weight: 600;
  opacity: 0.75;
  background: rgba(0,0,0,0.03);
}
.moraya-editor .frontmatter-table .fm-list { margin: 0; padding-left: 1.2em; }
.moraya-editor .frontmatter-table .fm-nested-key { font-weight: 600; opacity: 0.7; }
.moraya-editor .frontmatter-md { font-size: 0.92em; }
.moraya-editor .frontmatter-md > :first-child { margin-top: 0; }
.moraya-editor .frontmatter-md > :last-child { margin-bottom: 0; }
.moraya-editor .frontmatter-raw {
  margin: 0;
  padding: 0 0 0 10px;
  background: none;
  white-space: pre-wrap;
  overflow-wrap: break-word;
  border-left: 3px solid rgba(0,0,0,0.2);
}
@media (prefers-color-scheme: dark) {
  .moraya-editor .frontmatter-details {
    border-color: rgba(255,255,255,0.14);
    background: rgba(255,255,255,0.04);
  }
  .moraya-editor .frontmatter-details[open] > .frontmatter-summary {
    border-bottom-color: rgba(255,255,255,0.12);
  }
  .moraya-editor .frontmatter-table td { border-color: rgba(255,255,255,0.15); }
  .moraya-editor .frontmatter-table .fm-key { background: rgba(255,255,255,0.05); }
  .moraya-editor .frontmatter-raw { border-left-color: rgba(255,255,255,0.25); }
}
`.trim()
