import { basename } from '../fs'
import { Marked, type TokenizerAndRendererExtension } from 'marked'
import markedKatex from 'marked-katex-extension'
import { markedHighlight } from 'marked-highlight'
import hljs from 'highlight.js'
import { blockCitationExtension } from '../blockio/marked-citation'
import type { Tab } from '../tabs.svelte'

/**
 * Shared host-side render pipeline used by every plugin that declares the
 * `renderer.html` capability. Produces an inline-body HTML fragment with:
 *  - markdown rendered via marked + KaTeX + highlight.js
 *  - mermaid / graphviz code blocks rasterised to inline SVG
 *  - <img> tags pointing at local files rewritten to data: URLs
 *
 * The result has no <!doctype>, no <head>, no wrapping. Each plugin
 * (share, md2pdf, …) wraps it for its own output format.
 */

export function htmlEscape(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

/**
 * Extract the first ATX-style `# ` heading text from markdown source.
 * Setext-style (`Title\n===`) is intentionally not supported — too rare in
 * the wild and complicates the regex.
 */
export function extractH1FromMarkdown(md: string): string | null {
  const match = md.match(/^[ \t]*#[ \t]+(.+?)[ \t#]*$/m)
  return match ? match[1].trim() : null
}

/**
 * Document title for downstream consumers (PDF header, share page <title>).
 * Markdown tabs prefer the first H1; everything else (and markdown without
 * a heading) falls back to the basename minus the last extension. Dotfiles
 * keep their full filename ("/proj/.env" → ".env").
 */
export function buildPdfTitle(tab: Tab): string {
  if (tab.kind === 'markdown') {
    const h1 = extractH1FromMarkdown(tab.currentContent)
    if (h1) return h1
  }
  const base = basename(tab.filePath)
  const dot = base.lastIndexOf('.')
  return dot <= 0 ? base : base.slice(0, dot)
}

/**
 * Heuristic: does the markdown source contain inline or display math
 * delimiters that KaTeX would render? Used to skip the ~500-1000 KB
 * KaTeX font CSS for documents with no math.
 */
export function hasMathContent(md: string): boolean {
  if (/\$[^\$\n]+\$/.test(md)) return true
  if (/\$\$[\s\S]+?\$\$/.test(md)) return true
  if (/\\\([\s\S]+?\\\)/.test(md)) return true
  if (/\\\[[\s\S]+?\\\]/.test(md)) return true
  return false
}

const highlightCaretExtension: TokenizerAndRendererExtension = {
  name: 'highlightCaret',
  level: 'inline',
  start(src: string) { return src.indexOf('^^') },
  tokenizer(src: string) {
    const m = /^\^\^([^^]+)\^\^/.exec(src)
    if (!m) return undefined
    return { type: 'highlightCaret', raw: m[0], text: m[1] } as any
  },
  renderer(token: any) {
    return `<mark>${htmlEscape(String(token.text))}</mark>`
  },
}

const highlightEqExtension: TokenizerAndRendererExtension = {
  name: 'highlightEq',
  level: 'inline',
  start(src: string) { return src.indexOf('==') },
  tokenizer(src: string) {
    const m = /^==([^=\n]+)==/.exec(src)
    if (!m) return undefined
    return { type: 'highlightEq', raw: m[0], text: m[1] } as any
  },
  renderer(token: any) {
    return `<mark>${htmlEscape(String(token.text))}</mark>`
  },
}

// CriticMarkup annotations: {==text==}{>>note<<} and standalone {>>note<<}.
// Must win over highlightEq: its start() points at the `{`, which precedes
// the inner `==`, so the lexer tries this extension first.
const criticAnnotationExtension: TokenizerAndRendererExtension = {
  name: 'criticAnnotation',
  level: 'inline',
  start(src: string) {
    const a = src.indexOf('{==')
    const b = src.indexOf('{>>')
    if (a < 0) return b
    if (b < 0) return a
    return Math.min(a, b)
  },
  tokenizer(src: string) {
    let m = /^\{==([^\n]+?)==\}\{>>([^\n]*?)<<\}/.exec(src)
    if (m) {
      const token = { type: 'criticAnnotation', raw: m[0], note: m[2], tokens: [] } as any
      this.lexer.inline(m[1], token.tokens)
      return token
    }
    m = /^\{>>([^\n]*?)<<\}/.exec(src)
    if (m) return { type: 'criticAnnotation', raw: m[0], note: m[1], tokens: [] } as any
    return undefined
  },
  renderer(token: any) {
    const note = String(token.note)
    // 批注含半角/全角问号 = 向 agent 提问,徽标用 ⁇(U+2047),与编辑器一致
    // (isQuestionText 约定,spec 2026-07-27-annotation-qa-loop)。
    const glyph = /[?？]/.test(note) ? '⁇' : '※'
    const badge = `<sup class="crit-badge" title="${htmlEscape(note)}">${glyph}</sup>`
    if (!token.tokens?.length) return badge
    return `<mark class="crit-anno">${this.parser.parseInline(token.tokens)}</mark>${badge}`
  },
}

/** Styles for exported/printed CriticMarkup annotations (light + dark). */
export const CRITIC_CSS = `
.crit-anno { background: #fff3bf; border-bottom: 1px dashed #d9a400; padding: 0 1px; }
.crit-badge { color: #b8860b; cursor: help; font-size: 0.75em; margin-left: 1px; user-select: none; }
@media (prefers-color-scheme: dark) {
  .crit-anno { background: #4d3f00; border-bottom-color: #e3b341; color: inherit; }
  .crit-badge { color: #e3b341; }
}
`

const sharedMarked = new Marked(
  // The rich editor maps every markdown soft break to a hard line break
  // (moraya's softbreak → hardbreak). `breaks: true` mirrors that in exported
  // / previewed HTML so multi-line blockquotes and wrapped paragraphs keep
  // their line breaks instead of collapsing onto one line.
  { breaks: true, gfm: true },
  markedHighlight({
    langPrefix: 'hljs language-',
    highlight(code: string, lang: string): string {
      const language = lang && hljs.getLanguage(lang) ? lang : 'plaintext'
      return hljs.highlight(code, { language }).value
    },
  }),
  markedKatex({ throwOnError: false }),
)
sharedMarked.use({ extensions: [criticAnnotationExtension, blockCitationExtension, highlightCaretExtension, highlightEqExtension] })

/**
 * Synchronously render a small markdown fragment to an HTML string using the
 * shared pipeline. Used for the non-key:value regions inside frontmatter.
 */
export function renderMarkdownInline(md: string): string {
  return sharedMarked.parse(md, { async: false }) as string
}

/**
 * Render a tab to an HTML body fragment (no <html>/<head>). markdown runs
 * through the shared marked + KaTeX + hljs pipeline; html tabs are passed
 * through; code tabs are syntax-highlighted in a `<pre>`.
 */
export async function renderTabBody(tab: Tab): Promise<string> {
  if (tab.kind === 'html') return tab.currentContent
  if (tab.kind === 'code') {
    const lang = tab.language && hljs.getLanguage(tab.language) ? tab.language : 'plaintext'
    const highlighted = hljs.highlight(tab.currentContent, { language: lang }).value
    return `<pre><code class="hljs language-${htmlEscape(lang)}">${highlighted}</code></pre>`
  }
  return await sharedMarked.parse(tab.currentContent, { async: true })
}

// ---- image inline ----------------------------------------------------------

type ImageReader = (absolutePath: string) => Promise<Uint8Array>

function mimeFromExt(p: string): string {
  const lower = p.toLowerCase()
  if (lower.endsWith('.png')) return 'image/png'
  if (lower.endsWith('.jpg') || lower.endsWith('.jpeg')) return 'image/jpeg'
  if (lower.endsWith('.gif')) return 'image/gif'
  if (lower.endsWith('.webp')) return 'image/webp'
  if (lower.endsWith('.svg')) return 'image/svg+xml'
  if (lower.endsWith('.bmp')) return 'image/bmp'
  return 'application/octet-stream'
}

function bytesToBase64(bytes: Uint8Array): string {
  let s = ''
  const CHUNK = 8192
  for (let i = 0; i < bytes.length; i += CHUNK) {
    s += String.fromCharCode(...bytes.subarray(i, i + CHUNK))
  }
  return btoa(s)
}

function dirname(p: string): string {
  const i = p.lastIndexOf('/')
  return i <= 0 ? '/' : p.slice(0, i)
}

function resolveImagePath(src: string, tabPath: string): string | null {
  if (/^(https?:|data:|mailto:)/i.test(src)) return null
  let p = src
  if (p.startsWith('file://')) {
    try {
      const u = new URL(p)
      p = decodeURIComponent(u.pathname)
    } catch {
      return null
    }
  }
  if (p.startsWith('/')) return p
  return `${dirname(tabPath)}/${p}`.replace(/\/\.\//g, '/')
}

/**
 * Replace <img> tags whose src points at local files with base64 data URLs.
 * Remote URLs (https://) are left untouched. Unreadable images become
 * `<em>alt</em>` text (or `<em>[image]</em>` if no alt).
 */
export async function inlineImages(
  html: string,
  tabPath: string | null,
  reader: ImageReader,
): Promise<string> {
  if (!tabPath) return html

  const doc = new DOMParser().parseFromString(`<div>${html}</div>`, 'text/html')
  const root = doc.body.firstElementChild
  if (!root) return html

  const imgs = Array.from(root.querySelectorAll('img'))
  for (const img of imgs) {
    const src = img.getAttribute('src') ?? ''
    if (!src) continue
    if (/^(https?:|data:|mailto:)/i.test(src)) continue
    const abs = resolveImagePath(src, tabPath)
    if (!abs) continue
    try {
      const bytes = await reader(abs)
      const mime = mimeFromExt(abs)
      img.setAttribute('src', `data:${mime};base64,${bytesToBase64(bytes)}`)
    } catch {
      const alt = img.getAttribute('alt')?.trim() || '[image]'
      const em = doc.createElement('em')
      em.textContent = alt
      img.replaceWith(em)
    }
  }
  return root.innerHTML
}

let testImageReader: ImageReader | null = null
export function __setImageReaderForTests(r: ImageReader | null): void {
  testImageReader = r
}

async function realImageReader(absolutePath: string): Promise<Uint8Array> {
  const { readFile } = await import('@tauri-apps/plugin-fs')
  return readFile(absolutePath)
}

function pickImageReader(): ImageReader {
  return testImageReader ?? realImageReader
}

// ---- diagrams --------------------------------------------------------------

async function renderDiagramsToString(html: string): Promise<string> {
  const { renderDiagrams } = await import('../diagram-render')
  const staging = document.createElement('div')
  staging.setAttribute(
    'style',
    'position:absolute;left:-10000px;top:0;width:800px;visibility:hidden;',
  )
  staging.innerHTML = html
  document.body.appendChild(staging)
  try {
    await renderDiagrams(staging)
    return staging.innerHTML
  } finally {
    staging.remove()
  }
}

// ---- public entry point ----------------------------------------------------

/**
 * Render a tab into inline-body HTML with images inlined as data URIs and
 * mermaid / graphviz code blocks rasterised to inline SVG. Returns just the
 * body — no <!doctype>, no <head>, no wrapping. Each plugin wraps for its
 * own output format.
 */
export async function renderTabAsInlineBody(tab: Tab): Promise<string> {
  const body = await renderTabBody(tab)
  const inlined = await inlineImages(body, tab.filePath, pickImageReader())
  return await renderDiagramsToString(inlined)
}
