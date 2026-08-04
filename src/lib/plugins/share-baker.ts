import { parse as parseYaml } from 'yaml'
import { basename } from '../fs'
import type { Tab } from '../tabs.svelte'
import { trustTier, lifecycleOf } from '../okf/trust'
import { invoke } from '@tauri-apps/api/core'
import {
  htmlEscape,
  renderTabAsInlineBody,
  renderTabBody as sharedRenderTabBody,
  inlineImages as sharedInlineImages,
  __setImageReaderForTests as sharedSetImageReader,
  buildPdfTitle,
  CRITIC_CSS,
} from './host-render-html'
import katexCss from 'katex/dist/katex.min.css?raw'
import hljsLightCss from 'highlight.js/styles/github.css?raw'
import hljsDarkCss from 'highlight.js/styles/github-dark.css?raw'
import shareBeaconJs from './share-beacon.js?raw'
import { ShareError } from '../share/types'

/// Load the compiled CSS for the requested theme via the `theme_load_compiled`
/// Tauri command (same routing as theme-loader.ts — avoids needing fs:scope
/// permission for the app-data directory).
async function readThemeCss(themeId: string): Promise<string> {
  try { return await invoke<string>('theme_load_compiled', { id: themeId }) }
  catch (e) { console.warn('[share-baker] readThemeCss', themeId, e); return '' }
}

export const MAX_HTML_BYTES = 25 * 1024 * 1024

export function shareHeaderLabel(path: string | null): string {
  if (!path) return 'Untitled'
  return basename(path)
}

export function isoDateStamp(d: Date = new Date()): string {
  return d.toISOString().slice(0, 10)
}

export function viewportMetaTag(): string {
  return '<meta name="viewport" content="width=device-width, initial-scale=1">'
}

export function themeCssBlock(): string {
  return `
:root { color-scheme: light dark; }
body {
  margin: 0; padding: 24px;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  font-size: clamp(15px, 2.4vw, 18px); line-height: 1.6;
  background: #ffffff; color: #1a1a1a;
}
.share-shell { max-width: 720px; margin: 0 auto; }
.share-header { font-size: 0.85em; opacity: 0.6; margin-bottom: 32px; padding-bottom: 12px; border-bottom: 1px solid rgba(0,0,0,0.1); }
.share-footer { font-size: 0.8em; opacity: 0.5; margin-top: 64px; padding-top: 12px; border-top: 1px solid rgba(0,0,0,0.1); text-align: center; }
img { max-width: 100%; height: auto; }
pre { overflow-x: auto; padding: 12px; background: rgba(0,0,0,0.04); border-radius: 6px; }
code { word-wrap: break-word; font-family: ui-monospace, SFMono-Regular, monospace; }
.katex-display { overflow-x: auto; overflow-y: hidden; }
table { border-collapse: collapse; max-width: 100%; }
th, td { padding: 6px 10px; border: 1px solid rgba(0,0,0,0.1); }
@media (prefers-color-scheme: dark) {
  body { background: #1a1a1a; color: #e0e0e0; }
  .share-header, .share-footer { border-color: rgba(255,255,255,0.1); }
  pre { background: rgba(255,255,255,0.06); }
  th, td { border-color: rgba(255,255,255,0.15); }
}
`.trim()
}

/**
 * Phone-width adjustments for the shared page. Skin CSS is desktop-tuned
 * (designed for the editor pane, where the user controls the window width),
 * so on narrow viewports we trim padding, hide effie's gutter labels,
 * and let tables scroll horizontally instead of overflowing.
 */
export function mobileOverridesCssBlock(): string {
  return `
@media (max-width: 600px) {
  body { padding: 16px; }
  .share-header { margin-bottom: 24px; }
  .share-footer { margin-top: 40px; }
  .moraya-editor { font-size: 16px; }
  /* effie: drop the 2.5em left gutter and hide the H-labels — no room on
     phones, and the labels look orphaned without their indent buddy. */
  [data-theme="effie"] .moraya-editor { padding-left: 0; }
  [data-theme="effie"] .moraya-editor h1::before,
  [data-theme="effie"] .moraya-editor h2::before,
  [data-theme="effie"] .moraya-editor h3::before,
  [data-theme="effie"] .moraya-editor h4::before { display: none; }
  /* Tables: scroll horizontally instead of bleeding off the viewport. */
  .moraya-editor table { display: block; overflow-x: auto; max-width: 100%; }
}
`.trim()
}

export function guardSize(html: string): void {
  const bytes = new TextEncoder().encode(html).byteLength
  if (bytes > MAX_HTML_BYTES) throw new ShareError('too_large', `${bytes} bytes`)
}

const DEFAULT_DESCRIPTION_MAX = 200

/**
 * Extract a plain-text description suitable for `<meta name="description">`
 * and og:description from a markdown source. Strips front matter, code
 * fences, headings, blockquotes, list markers, and inline markdown
 * (bold/italic/code/link/image), then takes the first prose paragraph and
 * truncates with an ellipsis at `maxLen` chars.
 *
 * Returns '' when nothing usable is found (e.g. doc is just a heading or
 * just code blocks). Callers can fall back to a default like the title.
 */
export function extractShareDescription(
  md: string,
  maxLen = DEFAULT_DESCRIPTION_MAX,
): string {
  let body = md

  // Strip YAML front matter at the top of the file (--- ... ---).
  body = body.replace(/^---\n[\s\S]*?\n---\n?/, '')

  // Strip fenced code blocks — the description should never contain code.
  body = body.replace(/^```[\s\S]*?^```\s*$/gm, '')
  body = body.replace(/^~~~[\s\S]*?^~~~\s*$/gm, '')

  // Walk lines, find the first non-empty prose paragraph (skip headings,
  // blockquotes, table rows, list bullets, hr).
  const lines = body.split('\n')
  let para = ''
  for (const raw of lines) {
    const trimmed = raw.trim()
    if (!trimmed) {
      if (para) break // end of first paragraph
      continue
    }
    if (trimmed.startsWith('#')) continue
    if (trimmed.startsWith('>')) continue
    if (trimmed.startsWith('|')) continue
    if (/^[-*_]{3,}\s*$/.test(trimmed)) continue // hr
    if (/^[-*+]\s+/.test(trimmed)) continue // list bullet
    if (/^\d+\.\s+/.test(trimmed)) continue // ordered list
    para += (para ? ' ' : '') + trimmed
  }

  // Strip the most common inline markdown so the description reads as plain text.
  para = para
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, '$1') // images → alt
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1') // links → text
    .replace(/`([^`]+)`/g, '$1') // inline code
    .replace(/\*\*([^*]+)\*\*/g, '$1') // bold
    .replace(/__([^_]+)__/g, '$1')
    .replace(/(^|\W)\*([^*\n]+)\*(\W|$)/g, '$1$2$3') // em
    .replace(/(^|\W)_([^_\n]+)_(\W|$)/g, '$1$2$3')
    .replace(/<[^>]+>/g, '') // raw HTML tags
    .replace(/\s+/g, ' ')
    .trim()

  if (para.length > maxLen) {
    // Trim at the last word boundary inside maxLen-1 to avoid mid-word cut.
    const cut = para.slice(0, maxLen - 1)
    const lastSpace = cut.lastIndexOf(' ')
    para = (lastSpace > maxLen * 0.6 ? cut.slice(0, lastSpace) : cut).trimEnd() + '…'
  }
  return para
}

/**
 * Default thumbnail for shared pages. Hosted in the public note.md repo so any
 * unfurler (WeChat, Slack, Twitter, Discord, iMessage) can fetch it. The
 * file is 64×64 / ~7 KB — the smallest square icon size that still works
 * as a recognizable thumbnail across platforms. WeChat / Slack render it
 * at ~40-80px so this is plenty; if Twitter's `summary` crawler complains
 * about under-min dimensions it'll just upscale, not refuse the card.
 *
 * Inlined images in the doc body are base64 data URIs and can't serve as
 * og:image (unfurlers must fetch absolute URLs out-of-band), so we always
 * supply this as the fallback.
 */
export const DEFAULT_OG_IMAGE_URL =
  'https://raw.githubusercontent.com/wizlijun/note.md/main/src-tauri/icons/64x64.png'
const DEFAULT_OG_IMAGE_SIZE = 64

/**
 * Build the open-graph / twitter / WeChat link-card metadata block. WeChat
 * (个人 + 企业) uses `<title>` and `<meta name="description">`; everything
 * else (Slack, Twitter, Discord, Notion, iMessage on macOS) reads og:*.
 *
 * og:url is intentionally omitted — the share URL isn't known until after
 * upload, and most consumers infer canonical URL from the request anyway.
 *
 * og:image defaults to the note.md logo so link cards always have a thumbnail.
 * Callers can pass a different `imageUrl` to override per-document — e.g.
 * a future plugin setting or a hash-extracted hero image.
 */
export function metadataBlock(opts: {
  title: string
  description: string
  filename: string
  imageUrl?: string
}): string {
  const t = htmlEscape(opts.title)
  const d = htmlEscape(opts.description)
  const f = htmlEscape(opts.filename)
  const img = htmlEscape(opts.imageUrl ?? DEFAULT_OG_IMAGE_URL)
  const lines = [`<title>${t}</title>`]
  // Description is optional — empty string means we couldn't extract one
  // (e.g. image tab, doc that's only a heading), so don't emit empty meta.
  if (opts.description) {
    lines.push(`<meta name="description" content="${d}">`)
  }
  lines.push(`<meta property="og:type" content="article">`)
  lines.push(`<meta property="og:title" content="${t}">`)
  if (opts.description) {
    lines.push(`<meta property="og:description" content="${d}">`)
  }
  lines.push(`<meta property="og:site_name" content="note.md">`)
  lines.push(`<meta property="og:image" content="${img}">`)
  // Width/height hints let unfurlers skip a HEAD probe — they're advisory,
  // mismatched values still display fine.
  lines.push(`<meta property="og:image:width" content="${DEFAULT_OG_IMAGE_SIZE}">`)
  lines.push(`<meta property="og:image:height" content="${DEFAULT_OG_IMAGE_SIZE}">`)
  lines.push(`<meta property="og:image:alt" content="${t}">`)
  lines.push(`<meta name="twitter:card" content="summary">`)
  lines.push(`<meta name="twitter:title" content="${t}">`)
  if (opts.description) {
    lines.push(`<meta name="twitter:description" content="${d}">`)
  }
  lines.push(`<meta name="twitter:image" content="${img}">`)
  // Filename hint is useful for some tools that surface it as a subtitle.
  lines.push(`<meta name="filename" content="${f}">`)
  return lines.join('\n')
}

/**
 * 对外分享页的 OKF 出处元数据。分享出去的 HTML 是公开的,所以这里**只发布
 * 不指向任何人、任何本地路径的信号**:概念类型、派生的信任层级(§5.3)、
 * 生命周期(§5.4)。actor id(`human:<id>`)与 `sources[].resource`(常是
 * 本机绝对路径)一律不出现 —— 出处的价值不该以泄露隐私为代价。
 *
 * 文档没有 frontmatter 时返回空串。
 */
export function okfMetaBlock(md: string): string {
  const m = md.match(/^---\r?\n([\s\S]*?)\r?\n---(\r?\n|$)/)
  if (!m) return ''
  const fm = m[1]
  const type = conceptTypeOf(fm)
  const { status, staleAfter } = lifecycleOf(fm)
  const lines: string[] = []
  if (type) lines.push(`<meta name="okf:type" content="${htmlEscape(type)}">`)
  lines.push(`<meta name="okf:trust-tier" content="${trustTier(fm)}">`)
  if (status !== 'stable') lines.push(`<meta name="okf:status" content="${status}">`)
  if (staleAfter) lines.push(`<meta name="okf:stale-after" content="${htmlEscape(staleAfter)}">`)
  return lines.join('\n')
}

/** frontmatter 里的 `type`(缺失/非字符串 → null)。 */
function conceptTypeOf(fm: string): string | null {
  try {
    const v = parseYaml(fm)
    const t = v !== null && typeof v === 'object' ? (v as { type?: unknown }).type : null
    return typeof t === 'string' && t.trim() !== '' ? t : null
  } catch {
    return null
  }
}

// Re-export shared pipeline pieces so existing share-baker tests keep their
// imports stable. Real implementations live in host-render-html.ts.
export const renderTabBody = sharedRenderTabBody
export const inlineImages = sharedInlineImages
export const __setImageReaderForTests = sharedSetImageReader

/** The shared inline `<style>` head used by both the share page and the
 *  git-history rich preview: katex + hljs(light/dark) + base responsive block +
 *  the user's theme CSS + mobile overrides + CriticMarkup. */
function themedStyleHead(themeCss: string): string {
  return `<style>${katexCss}</style>
<style>${hljsLightCss}</style>
<style>@media (prefers-color-scheme: dark) { ${hljsDarkCss} }</style>
<style>${themeCssBlock()}</style>
<style>${themeCss}</style>
<style>${mobileOverridesCssBlock()}</style>
<style>${CRITIC_CSS}</style>`
}

/** Render a Tab to a self-contained, THEME-STYLED HTML document for the
 *  git-history rich preview. Same theme/katex/hljs styling as the share page,
 *  but WITHOUT the share chrome (no header/footer/beacon). */
export async function bakeThemedPreviewHtml(tab: Tab, themeId: string = 'default'): Promise<string> {
  const inlineBody = await renderTabAsInlineBody(tab)
  const themeCss = await readThemeCss(themeId)
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
${viewportMetaTag()}
${themedStyleHead(themeCss)}
</head>
<body data-theme="${htmlEscape(themeId)}">
<main class="moraya-editor">${inlineBody}</main>
</body>
</html>`
}

/**
 * Render a tab into a fully self-contained HTML document suitable for posting
 * to the share Worker. Inlines images as base64 (via the shared host-render
 * pipeline), bakes light + dark themes plus the user's chosen skin, adds
 * mobile-responsive viewport overrides, wraps in a minimal header/footer
 * shell.
 *
 * `themeId` defaults to 'default' so callers (and tests) can omit it. The
 * theme CSS is read from disk via readTextFile and scoped via
 * `[data-theme="<id>"] .moraya-editor`, matching the in-app preview's
 * selector contract.
 *
 * Throws `ShareError('too_large')` (byte count in `detail`) if the input or
 * the result exceeds 25 MB.
 */
export async function bakeShareHtml(tab: Tab, themeId: string = 'default'): Promise<string> {
  // Guard raw content size before running the rendering pipeline to avoid
  // stack overflows in the markdown parser on pathologically large inputs.
  const rawBytes = new TextEncoder().encode(tab.currentContent).byteLength
  if (rawBytes > MAX_HTML_BYTES) throw new ShareError('too_large', `${rawBytes} bytes`)

  const inlineBody = await renderTabAsInlineBody(tab)
  // Visible header label stays as the filename (small subtitle below the
  // title). The page <title> + og:title use buildPdfTitle which prefers the
  // first H1 — much better for link-card unfurls than "PROJECT_ANALYSIS.md".
  const filename = shareHeaderLabel(tab.filePath)
  const headerLabel = htmlEscape(filename)
  const pageTitle = buildPdfTitle(tab)
  const description =
    tab.kind === 'markdown' ? extractShareDescription(tab.currentContent) : ''
  const date = isoDateStamp()
  const themeCss = await readThemeCss(themeId)
  const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
${viewportMetaTag()}
${metadataBlock({ title: pageTitle, description, filename })}
${tab.kind === 'markdown' ? okfMetaBlock(tab.currentContent) : ''}
${themedStyleHead(themeCss)}
</head>
<body data-theme="${htmlEscape(themeId)}">
<div class="share-shell">
<header class="share-header">${headerLabel} · ${date}</header>
<main class="moraya-editor">${inlineBody}</main>
<footer class="share-footer">Powered by <a href="https://notemd.net">note.md</a></footer>
</div>
<script>${shareBeaconJs}</script>
</body>
</html>`
  guardSize(html)
  return html
}
