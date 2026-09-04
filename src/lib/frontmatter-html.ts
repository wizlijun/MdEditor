import { parseDocument, isMap } from 'yaml'
import { segmentFrontmatter } from './frontmatter-segment'
import { htmlEscape, renderMarkdownInline } from './plugins/host-render-html'
import { frontmatterInlineParts } from './frontmatter-inline'
import { t } from './i18n/store.svelte'

const SHAREABLE_SCHEME_RE = /^(?:https?|mailto|tel):/i
const ANY_SCHEME_RE = /^[a-z][a-z0-9+.-]*:/i

/**
 * Static (string-only) frontmatter renderer for baked HTML output — the shared
 * page. Mirrors the rich editor's collapsed property view (frontmatter-view.ts)
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
 * top-level keys, the body holds the key/value properties (plus any non-key:value
 * prose regions, rendered as markdown). Returns '' for an empty block — there
 * is nothing worth a disclosure widget.
 */
export function frontmatterDetailsHtml(fm: string): string {
  if (fm.trim() === '') return ''
  const parts: string[] = []
  const keys: string[] = []
  for (const seg of segmentFrontmatter(fm)) {
    if (seg.kind === 'kv') {
      const properties = kvPropertiesHtml(seg.text)
      parts.push(properties.html)
      keys.push(...properties.keys)
    } else if (seg.text.trim() !== '') {
      parts.push(`<div class="frontmatter-md">${renderMarkdownInline(seg.text)}</div>`)
    }
  }
  if (!parts.length) return ''
  const label = keys.length ? keys.join(', ') : 'frontmatter'
  return (
    '<details class="frontmatter-details">' +
    `<summary class="frontmatter-summary"><span class="frontmatter-summary-title">${htmlEscape(t('frontmatter.metadata'))}</span>` +
    `<span class="frontmatter-summary-keys">${htmlEscape(label)}</span></summary>` +
    `<div class="frontmatter-segments">${parts.join('')}</div>` +
    '</details>'
  )
}

function kvPropertiesHtml(segText: string): { html: string; keys: string[] } {
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
  const properties: string[] = []
  for (const pair of doc.contents.items) {
    const key = String((pair.key as { value?: unknown })?.value ?? pair.key)
    keys.push(key)
    const value = (pair.value as { toJSON?: () => unknown } | null)?.toJSON?.() ?? null
    properties.push(
      `<div class="fm-property"><div class="fm-key">${htmlEscape(key)}</div><div class="fm-val">${valueHtml(value)}</div></div>`,
    )
  }
  return { html: `<div class="frontmatter-properties">${properties.join('')}</div>`, keys }
}

function valueHtml(value: unknown): string {
  if (value == null) return ''
  if (Array.isArray(value)) {
    const chips = value.every(isChipValue) ? ' fm-chips' : ''
    return `<ul class="fm-list${chips}">${value.map((v) => `<li>${valueHtml(v)}</li>`).join('')}</ul>`
  }
  if (typeof value === 'object') {
    const lines = Object.entries(value as Record<string, unknown>).map(
      ([k, v]) =>
        `<div class="fm-nested-line"><span class="fm-nested-key">${htmlEscape(k)}: </span>${valueHtml(v)}</div>`,
    )
    return `<div class="fm-nested">${lines.join('')}</div>`
  }
  // Scalar (incl. multi-line strings, which the `.fm-val` pre-wrap keeps intact).
  return inlineValueHtml(String(value))
}

function isChipValue(value: unknown): boolean {
  return value == null || typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean'
}

function inlineValueHtml(value: string): string {
  return frontmatterInlineParts(value).map((part) => {
    if (part.kind === 'text') return htmlEscape(part.text)
    if (part.kind === 'wikilink') {
      return `<span class="fm-wikilink">${htmlEscape(part.label)}</span>`
    }
    if (part.kind === 'link') {
      if (ANY_SCHEME_RE.test(part.href) && !SHAREABLE_SCHEME_RE.test(part.href)) return htmlEscape(part.raw)
      return `<a class="fm-inline-link" href="${htmlEscape(part.href)}">${htmlEscape(part.label)}</a>`
    }
    return `<a class="fm-inline-link" href="${htmlEscape(part.href)}">${htmlEscape(part.raw)}</a>`
  }).join('')
}

function rawFallbackHtml(raw: string): string {
  return `<pre class="frontmatter-raw">${htmlEscape(raw)}</pre>`
}

/**
 * Styling for the baked `<details>`. Kept in rgba (not the editor's
 * `color-mix(… Canvas)`) to match the share page's hardcoded light/dark
 * palette, and scoped under `.moraya-editor` so it outranks theme skins.
 * `white-space: normal` guards against skins that put
 * the editor in pre-wrap, which would turn the tag-to-tag whitespace of a
 * pretty-printed fragment into visible blank lines.
 */
export const FRONTMATTER_CSS = `
.moraya-editor .frontmatter-details {
  white-space: normal;
  margin: 0 0 1.4em;
  border: 1px solid rgba(0,0,0,0.09);
  border-radius: 12px;
  background: rgba(0,0,0,0.04);
}
.moraya-editor .frontmatter-summary {
  cursor: pointer;
  padding: 11px 16px 7px;
  font-size: 0.88em;
  color: rgba(0,0,0,0.62);
  user-select: none;
  list-style-position: inside;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.moraya-editor .frontmatter-summary-title { font-weight: 600; }
.moraya-editor .frontmatter-summary-keys {
  margin-left: 0.8em;
  font: 0.92em ui-monospace, SFMono-Regular, Menlo, monospace;
  opacity: 0.65;
}
.moraya-editor .frontmatter-details[open] > .frontmatter-summary {
  margin-bottom: 3px;
}
.moraya-editor .frontmatter-details[open] > .frontmatter-summary .frontmatter-summary-keys {
  display: none;
}
.moraya-editor .frontmatter-segments { padding: 0 16px 14px; }
.moraya-editor .frontmatter-properties {
  display: grid;
  gap: 1px;
  font: 0.92em ui-monospace, SFMono-Regular, Menlo, monospace;
}
.moraya-editor .frontmatter-properties .fm-property {
  display: grid;
  grid-template-columns: minmax(9rem, 18%) minmax(0, 1fr);
  column-gap: 1.25rem;
  align-items: start;
  margin: 0 -6px;
  padding: 2px 6px;
}
.moraya-editor .frontmatter-properties .fm-key,
.moraya-editor .frontmatter-properties .fm-val {
  min-width: 0;
  white-space: pre-wrap;
  overflow-wrap: break-word;
  word-break: break-word;
  line-height: 1.55;
}
.moraya-editor .frontmatter-properties .fm-key { color: rgba(0,0,0,0.56); }
.moraya-editor .frontmatter-properties .fm-list { margin: 0; padding-left: 1.2em; }
.moraya-editor .frontmatter-properties .fm-list.fm-chips {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 5px;
  padding: 0;
  list-style: none;
}
.moraya-editor .frontmatter-properties .fm-list.fm-chips > li {
  padding: 0 9px;
  border-radius: 999px;
  background: rgba(0,0,0,0.09);
  line-height: 1.55;
}
.moraya-editor .frontmatter-properties .fm-nested-key { font-weight: 600; opacity: 0.7; }
.moraya-editor .frontmatter-properties .fm-inline-link {
  color: #0969da;
  cursor: pointer;
  text-decoration: underline;
  text-underline-offset: 0.12em;
}
.moraya-editor .frontmatter-properties .fm-wikilink { color: #0969da; }
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
    border-color: rgba(255,255,255,0.12);
    background: rgba(255,255,255,0.04);
  }
  .moraya-editor .frontmatter-summary { color: rgba(255,255,255,0.62); }
  .moraya-editor .frontmatter-properties .fm-key { color: rgba(255,255,255,0.56); }
  .moraya-editor .frontmatter-properties .fm-list.fm-chips > li { background: rgba(255,255,255,0.1); }
  .moraya-editor .frontmatter-properties .fm-inline-link { color: #79c0ff; }
  .moraya-editor .frontmatter-properties .fm-wikilink { color: #79c0ff; }
  .moraya-editor .frontmatter-raw { border-left-color: rgba(255,255,255,0.25); }
}
@media (max-width: 520px) {
  .moraya-editor .frontmatter-properties .fm-property {
    grid-template-columns: minmax(7rem, 34%) minmax(0, 1fr);
    column-gap: 0.75rem;
  }
}
`.trim()
