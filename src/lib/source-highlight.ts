// Source-mode overlay renderer.
//
// Source mode paints a syntax-highlighted <pre> underneath a *transparent*
// textarea: the textarea owns the caret and editing, the <pre> owns everything
// the user actually sees. That means find/replace hits cannot rely on the
// textarea's native selection — an unfocused textarea paints no selection at
// all, and focusing it would steal the caret away from the find input. Search
// hits therefore have to be marked in this overlay, the same way rich mode
// marks them with ProseMirror decorations.
//
// Everything is computed in RAW (unescaped) line coordinates and escaped only
// when a segment is emitted, so a hit that overlaps a `{>>note<<}` span splits
// cleanly instead of injecting tags into already-escaped HTML.

export interface HitRange {
  start: number
  end: number
}

interface Mark {
  start: number
  end: number
  cls: string
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

/** CriticMarkup spans on a raw line: `{==highlight==}` and `{>>note<<}`. */
function critMarks(line: string): Mark[] {
  const marks: Mark[] = []
  for (const m of line.matchAll(/(\{==.+?==\})?(\{>>.*?<<\})/g)) {
    const whole = m[0]
    const hl = m[1]
    const base = m.index ?? 0
    if (hl) marks.push({ start: base, end: base + hl.length, cls: 'crit-hl' })
    const noteStart = base + (hl ? hl.length : 0)
    marks.push({ start: noteStart, end: base + whole.length, cls: 'crit-note' })
  }
  return marks
}

/**
 * Render one line to HTML given the marks covering it. Segments are cut at
 * every mark boundary, so each segment carries a flat set of classes — no
 * nesting, which keeps overlapping marks (a search hit inside a note) valid.
 */
function renderLine(line: string, marks: Mark[]): string {
  if (marks.length === 0) return escapeHtml(line) || ' '
  const bounds = new Set<number>([0, line.length])
  for (const m of marks) {
    if (m.start > 0 && m.start < line.length) bounds.add(m.start)
    if (m.end > 0 && m.end < line.length) bounds.add(m.end)
  }
  const points = [...bounds].sort((a, b) => a - b)
  let out = ''
  for (let i = 0; i < points.length - 1; i++) {
    const from = points[i]
    const to = points[i + 1]
    const text = escapeHtml(line.slice(from, to))
    const cls = marks.filter((m) => m.start <= from && m.end >= to).map((m) => m.cls)
    out += cls.length ? `<span class="${cls.join(' ')}">${text}</span>` : text
  }
  return out || ' '
}

/**
 * How many hits may be painted at once.
 *
 * Building the HTML is cheap even uncapped (~23 ms for 10k hits across 800 KB
 * of prose — measured), but the string is re-applied with {@html}, so every
 * marked hit becomes a real inline box inside a GPU-promoted <pre> that gets
 * rebuilt on each keystroke of the find query. Searching a single common
 * letter in a big document put ~10k extra boxes in that layer and wedged the
 * UI. Only the hits around the current one can be on screen anyway, so paint a
 * window around it — the counter still reports the true total.
 */
const MAX_MARKS = 400

/** Window `hits` down to MAX_MARKS around `currentHit`. */
function windowHits(hits: HitRange[], currentHit: number, maxMarks: number) {
  if (hits.length <= maxMarks) return { hits, currentHit }
  const half = Math.floor(maxMarks / 2)
  const anchor = currentHit >= 0 ? currentHit : 0
  const lo = Math.min(Math.max(anchor - half, 0), hits.length - maxMarks)
  return { hits: hits.slice(lo, lo + maxMarks), currentHit: currentHit - lo }
}

/**
 * Full overlay HTML for the source view.
 *
 * `hits` are absolute offsets into `value` (as produced by the find engine);
 * `currentHit` indexes the one the user is parked on, which gets the stronger
 * `search-hit-current` class. Pass an empty list to render without any search
 * marks.
 */
export function renderSourceHtml(
  value: string, allHits: HitRange[] = [], currentHitIdx = -1, maxMarks = MAX_MARKS,
): string {
  const { hits, currentHit } = windowHits(allHits, currentHitIdx, maxMarks)
  const lines = value.split('\n')
  const out: string[] = []
  let lineStart = 0
  // Hits arrive in document order; walk them alongside the lines instead of
  // rescanning the whole list per line (documents can carry 10k hits).
  let hitIdx = 0
  for (const line of lines) {
    const lineEnd = lineStart + line.length
    const marks: Mark[] = []

    const heading = line.match(/^(#{1,6})(\s.*)?$/)
    if (heading) {
      marks.push({ start: 0, end: line.length, cls: `h h${heading[1].length}` })
    } else {
      marks.push(...critMarks(line))
    }

    while (hitIdx < hits.length && hits[hitIdx].end <= lineStart) hitIdx++
    for (let i = hitIdx; i < hits.length && hits[i].start <= lineEnd; i++) {
      const start = Math.max(hits[i].start - lineStart, 0)
      const end = Math.min(hits[i].end - lineStart, line.length)
      if (end <= start) continue
      marks.push({ start, end, cls: i === currentHit ? 'search-hit-current' : 'search-hit' })
    }

    out.push(renderLine(line, marks))
    lineStart = lineEnd + 1
  }
  // Trailing newline keeps the <pre> exactly as tall as the textarea when the
  // document ends with one.
  return out.join('\n') + '\n'
}
