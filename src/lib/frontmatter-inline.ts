import { isBlockedWikilink } from './wikilink/blocklist'

export type FrontmatterInlinePart =
  | { kind: 'text'; text: string }
  | { kind: 'wikilink'; raw: string; target: string; label: string }
  | { kind: 'link'; raw: string; href: string; label: string }
  | { kind: 'url'; raw: string; href: string }

const URL_RE = /^https?:\/\/[^\s<>()[\]{}"']+/
const TRAILING_PUNCT_RE = /[.,;:!?]+$/
const URI_SCHEME_RE = /^([a-z][a-z0-9+.-]*):/i
const ALLOWED_SCHEMES = new Set(['http', 'https', 'mailto', 'tel', 'file'])

function safeHref(href: string): boolean {
  const compact = href.trim().replace(/[\u0000-\u0020\u007f]+/g, '')
  if (!compact) return false
  const scheme = compact.match(URI_SCHEME_RE)?.[1].toLowerCase()
  return !scheme || ALLOWED_SCHEMES.has(scheme)
}

interface MarkdownLink {
  raw: string
  label: string
  href: string
}

/** Parse the common Markdown link form, including balanced URL parentheses. */
function markdownLinkAt(input: string, start: number): MarkdownLink | null {
  if (input[start] !== '[' || input[start - 1] === '!') return null

  let labelEnd = start + 1
  for (; labelEnd < input.length; labelEnd++) {
    if (input[labelEnd] === '\n') return null
    if (input[labelEnd] === '\\') { labelEnd++; continue }
    if (input[labelEnd] === ']') break
  }
  if (labelEnd >= input.length || input[labelEnd + 1] !== '(') return null

  let depth = 1
  let end = labelEnd + 2
  for (; end < input.length; end++) {
    const ch = input[end]
    if (ch === '\n') return null
    if (ch === '\\') { end++; continue }
    if (ch === '(') depth++
    else if (ch === ')' && --depth === 0) break
  }
  if (depth !== 0) return null

  const destination = input.slice(labelEnd + 2, end).trim()
  let href = destination
  if (destination.startsWith('<')) {
    const close = destination.indexOf('>')
    if (close < 0) return null
    href = destination.slice(1, close)
  } else {
    href = destination.split(/\s/, 1)[0]
  }
  href = href.replace(/\\([\\()])/g, '$1')
  if (!href) return null

  return {
    raw: input.slice(start, end + 1),
    label: input.slice(start + 1, labelEnd),
    href,
  }
}

/**
 * Split a YAML string value into the three link forms the rich editor already
 * knows how to open. Text is never interpreted as HTML. Each part keeps its
 * raw source so an editable scalar's textContent remains byte-for-byte equal
 * to the parsed scalar until the user actually edits it.
 */
export function frontmatterInlineParts(input: string): FrontmatterInlinePart[] {
  const parts: FrontmatterInlinePart[] = []
  let text = ''
  const flush = () => {
    if (!text) return
    parts.push({ kind: 'text', text })
    text = ''
  }

  let i = 0
  while (i < input.length) {
    if (input.startsWith('[[', i)) {
      const end = input.indexOf(']]', i + 2)
      if (end >= 0) {
        const raw = input.slice(i, end + 2)
        const inner = input.slice(i + 2, end)
        const [targetPart, ...aliasParts] = inner.split('|')
        const target = targetPart.trim()
        if (target && !isBlockedWikilink(target)) {
          flush()
          parts.push({
            kind: 'wikilink',
            raw,
            target,
            label: aliasParts.length ? aliasParts.join('|').trim() || target : target,
          })
        } else {
          text += raw
        }
        i = end + 2
        continue
      }
    }

    if (input[i] === '[' && input[i - 1] !== '!') {
      const match = markdownLinkAt(input, i)
      if (match) {
        const { raw, label, href } = match
        if (safeHref(href)) {
          flush()
          parts.push({ kind: 'link', raw, href, label })
        } else {
          text += raw
        }
        i += raw.length
        continue
      }
    }

    if (input.startsWith('http://', i) || input.startsWith('https://', i)) {
      const match = input.slice(i).match(URL_RE)
      if (match) {
        const raw = match[0].replace(TRAILING_PUNCT_RE, '')
        if (raw) {
          flush()
          parts.push({ kind: 'url', raw, href: raw })
          i += raw.length
          continue
        }
      }
    }

    text += input[i]
    i++
  }

  flush()
  return parts
}
