// Search-result preview text (design spec 2026-08-12 §2). Pure functions, no
// Svelte import — same shape as `grouping.ts`: the panel calls these, the
// logic itself is unit-testable without a component harness.
//
// Why a *line* and not the whole block: `SearchHit.text` is a whole block, and
// a real vault is full of fenced blocks — measured on the author's vault, 1607
// ```json fences, 1831 ```bash, 1048 ```mermaid (whose node labels are packed
// with `<br/>`). Rendering the block verbatim dumps forty lines of JSON into a
// narrow sidebar. Stripping markup alone does not fix that; picking the one
// line that actually matched does.
//
// **UI-only.** `notemd search`'s default output stays flat `path:line:text`
// (see `src-tauri/tests/search_cli_contract.rs`) — nothing here runs on that
// path.

/** A line that is nothing but a thematic break. Checked before anything else:
 *  the paired-emphasis rules below would otherwise chew `***` down to `*`. */
const DIVIDER = /^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/

/** One leading block marker: heading, quote, task item, bullet, ordered item.
 *  The task-item alternative has to precede the bare bullet, or `- [ ] x`
 *  would lose only the `- `. */
const BLOCK_PREFIX = /^\s*(?:#{1,6}\s+|>\s?|[-*+]\s+\[[ xX]\]\s+|[-*+]\s+|\d+[.)]\s+)/

const ENTITIES: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: ' ',
}

function decodeEntities(s: string): string {
  return s.replace(/&(#x[0-9a-fA-F]+|#\d+|[a-zA-Z]+);/g, (whole, body: string) => {
    if (body[0] === '#') {
      const code =
        body[1] === 'x' || body[1] === 'X'
          ? Number.parseInt(body.slice(2), 16)
          : Number.parseInt(body.slice(1), 10)
      return Number.isFinite(code) && code > 0 ? String.fromCodePoint(code) : whole
    }
    return ENTITIES[body.toLowerCase()] ?? whole
  })
}

/**
 * Strips markup from a single line, keeping the words.
 *
 * The order is load-bearing and must not be shuffled:
 *
 * - entities decode *after* tags are stripped, and the result is never
 *   re-scanned for tags — text written as `&lt;div&gt;` is there to be read as
 *   `<div>`, not to be swallowed as one;
 * - images before markdown links (`![](…)` is a superset of `[](…)`);
 * - wikilinks before inline marks;
 * - CriticMarkup before inline marks, or `{==x==}`'s `==` gets eaten by the
 *   highlight-mark rule first.
 *
 * Emphasis marks are only removed in *pairs*. A blanket delete of `*`/`_`
 * would turn `entity_boost` into `entityboost` — and lines out of JSON and
 * code fences, full of snake_case, are exactly what this function sees most.
 */
export function cleanLineText(line: string): string {
  if (DIVIDER.test(line)) return ''
  let s = line

  // 1–2. HTML: tags become a space (so `a<br/>b` does not fuse into `ab`),
  // then entities decode.
  s = s.replace(/<[^>]*>/g, ' ')
  s = decodeEntities(s)

  // 3. Images — alt text goes too; it is not prose.
  s = s.replace(/!\[\[[^\]]*\]\]/g, ' ')
  s = s.replace(/!\[[^\]]*\]\([^)]*\)/g, ' ')

  // 4–5. Wikilinks, then markdown links.
  s = s.replace(/\[\[[^\]|]*\|([^\]]*)\]\]/g, '$1')
  s = s.replace(/\[\[([^\]]*)\]\]/g, '$1')
  s = s.replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')

  // 6. CriticMarkup. Substitution first — `{~~a~>b~~}` contains no other
  // form, but running it late would leave its `~~` to the strikethrough rule.
  s = s.replace(/\{~~.*?~>(.*?)~~\}/g, '$1')
  s = s.replace(/\{==(.*?)==\}/g, '$1')
  s = s.replace(/\{\+\+(.*?)\+\+\}/g, '$1')
  s = s.replace(/\{--.*?--\}/g, '')
  s = s.replace(/\{>>.*?<<\}/g, '')

  // 7. Leading block markers, repeatedly — `> - nested` carries two.
  for (let i = 0; i < 3; i++) {
    const next = s.replace(BLOCK_PREFIX, '')
    if (next === s) break
    s = next
  }

  // 8. Paired inline marks.
  s = s.replace(/\*\*\*(.+?)\*\*\*/g, '$1')
  s = s.replace(/\*\*(.+?)\*\*/g, '$1')
  s = s.replace(/\*(.+?)\*/g, '$1')
  s = s.replace(/`([^`]+)`/g, '$1')
  s = s.replace(/~~(.+?)~~/g, '$1')
  s = s.replace(/\^\^(.+?)\^\^/g, '$1')
  s = s.replace(/==(.+?)==/g, '$1')
  // Underscores only when the delimiters are not intraword — the whole point
  // of the paired-only rule above. Written with a leading capture instead of a
  // lookbehind so it runs on every WebView this ships to.
  s = s.replace(/(^|[^\w])__([^_]+?)__(?![\w])/g, '$1$2')
  s = s.replace(/(^|[^\w])_([^_]+?)_(?![\w])/g, '$1$2')

  // 10. Whitespace.
  return s.replace(/\s+/g, ' ').trim()
}

/** Opening or closing fence, with the info string (opening only) captured. */
const FENCE = /^\s*(`{3,}|~{3,})\s*(.*)$/

function fenceLang(info: string): string | null {
  const first = info.trim().split(/\s+/)[0] ?? ''
  return first ? first.toLowerCase().slice(0, 12) : null
}

/** Frontmatter, HTML comments and script/style blocks — all of which span
 *  lines, so they have to go before the block is split. */
function stripBlockNoise(raw: string): string {
  let s = raw
  const lines = s.split('\n')
  if (lines[0]?.trim() === '---') {
    const close = lines.findIndex((l, i) => i > 0 && l.trim() === '---')
    if (close > 0) s = lines.slice(close + 1).join('\n')
  }
  s = s.replace(/<!--[\s\S]*?-->/g, '')
  s = s.replace(/<(script|style)\b[\s\S]*?<\/\1\s*>/gi, '')
  return s
}

export interface PreviewLine {
  text: string
  /** Info string of the enclosing fence (lowercased, ≤12 chars), or `null`
   *  when the line is not inside one. The panel renders it as a small chip so
   *  a JSON/mermaid hit reads as code rather than as broken prose. */
  lang: string | null
}

/**
 * Picks the one line of `raw` worth showing: the first whose *cleaned* text
 * contains one of `terms`, falling back to the first cleaned non-empty line.
 *
 * Matching runs on the cleaned text, not the raw line, because `**外**骨骼`
 * only contains a contiguous `外骨骼` once the marks are gone. Cleaning is
 * lazy — it stops at the first matching line.
 *
 * Fence delimiter lines are never eligible; a block that cleans away to
 * nothing yields `{ text: '', lang: null }` and the panel renders no preview.
 */
export function previewLine(raw: string, terms: string[]): PreviewLine {
  const lowered = terms.map((t) => t.toLowerCase()).filter(Boolean)
  let fence: string | null = null
  let lang: string | null = null
  let fallback: PreviewLine | null = null

  for (const line of stripBlockNoise(raw).split('\n')) {
    const m = FENCE.exec(line)
    if (m) {
      const char = m[1][0]
      if (fence === null) {
        fence = char
        lang = fenceLang(m[2])
        continue
      }
      // A `~~~` inside a ``` block is content, not a closing delimiter.
      if (fence === char) {
        fence = null
        lang = null
        continue
      }
    }
    if (!line.trim()) continue
    const text = cleanLineText(line)
    if (!text) continue
    const low = text.toLowerCase()
    if (lowered.some((t) => low.includes(t))) return { text, lang }
    if (!fallback) fallback = { text, lang }
  }
  return fallback ?? { text: '', lang: null }
}

/** Filter prefixes recognized by `searchidx::query::parse`. Their values
 *  constrain file attributes, not prose, so highlighting them would mark the
 *  wrong thing. */
const FILTER_PREFIXES = new Set([
  'tag',
  'type',
  'path',
  'ext',
  'origin',
  'after',
  'before',
  'page',
])

/** Mirrors `split_respecting_quotes` in `searchidx/src/query.rs`. */
function splitRespectingQuotes(raw: string): string[] {
  const out: string[] = []
  let cur = ''
  let inQuotes = false
  for (const c of raw) {
    if (c === '"') {
      inQuotes = !inQuotes
      cur += c
    } else if (/\s/.test(c) && !inQuotes) {
      if (cur) {
        out.push(cur)
        cur = ''
      }
    } else {
      cur += c
    }
  }
  if (cur) out.push(cur)
  return out
}

/**
 * The words and phrases to highlight, parsed the same way the backend parses
 * the query: whitespace splits except inside quotes, a *closed* quote makes a
 * phrase, an unterminated one degrades to a plain term, and filter tokens drop
 * out entirely.
 */
export function parseHighlightTerms(query: string): string[] {
  const out: string[] = []
  for (const token of splitRespectingQuotes(query)) {
    if (token.startsWith('"')) {
      const rest = token.slice(1)
      if (rest.endsWith('"')) {
        const inner = rest.slice(0, -1).trim()
        if (inner) {
          out.push(inner)
          continue
        }
      }
      const bare = rest.replace(/"/g, '').trim()
      if (bare) out.push(bare)
      continue
    }
    const colon = token.indexOf(':')
    if (colon > 0 && FILTER_PREFIXES.has(token.slice(0, colon)) && token.length > colon + 1) {
      continue
    }
    const t = token.trim()
    if (t) out.push(t)
  }
  return out
}

export interface HighlightPart {
  text: string
  hit: boolean
}

/**
 * Splits `text` into alternating plain and matched runs; the panel renders the
 * matched ones as `<mark>`.
 *
 * Longest term first, and a character already claimed cannot be claimed again
 * — otherwise a term list holding both `外骨骼` and `骨` would produce nested,
 * overlapping runs out of one word.
 *
 * Case-insensitivity assumes `toLowerCase()` is length-preserving, which holds
 * for everything short of a handful of Turkish/Greek edge cases; a mismatch
 * there costs a misplaced highlight, never wrong text — the runs are always
 * sliced out of the original string.
 */
export function highlightParts(text: string, terms: string[]): HighlightPart[] {
  if (!text || terms.length === 0) return [{ text, hit: false }]

  const lower = text.toLowerCase()
  const taken = new Array<boolean>(text.length).fill(false)
  const ordered = [...new Set(terms)].sort((a, b) => b.length - a.length)

  for (const term of ordered) {
    const needle = term.toLowerCase()
    if (!needle) continue
    for (let from = 0; ; ) {
      const at = lower.indexOf(needle, from)
      if (at < 0) break
      const end = at + needle.length
      let free = true
      for (let k = at; k < end && free; k++) free = !taken[k]
      if (free) for (let k = at; k < end; k++) taken[k] = true
      from = at + 1
    }
  }

  const parts: HighlightPart[] = []
  let cur = ''
  let curHit = taken[0]
  for (let i = 0; i < text.length; i++) {
    if (taken[i] !== curHit) {
      parts.push({ text: cur, hit: curHit })
      cur = ''
      curHit = taken[i]
    }
    cur += text[i]
  }
  parts.push({ text: cur, hit: curHit })
  return parts
}
