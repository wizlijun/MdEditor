// Pure filename-generation logic for Idea Spark. No IO — callers pass in
// whatever context (today's date, the set of names already on disk) so this
// stays trivially testable and reusable from both the App and any future
// CLI/tests.
import { isReservedConceptName } from './okf/concept'

/** Characters that are unsafe (or at least unwelcome) in a filesystem name,
 *  or that collide with markdown syntax (`#`) / template syntax (`%`, backtick). */
const FORBIDDEN_CHARS = /[\\/:*?"<>|#%`]/g

/**
 * Derives a filename-safe slug from the first non-empty line of a markdown
 * document's *body* — a leading YAML frontmatter block, if present, is
 * skipped first (works whether that line is a heading or a plain paragraph —
 * heading markers are stripped as part of the forbidden-character pass, so no
 * special-casing is needed). Falls back to `'idea'` when there is no usable
 * text at all: an empty document, a frontmatter-only document (nothing left
 * after the closing fence), a document whose frontmatter fence is never
 * closed (see `stripLeadingFrontmatter`), or a title made entirely of
 * forbidden characters.
 *
 * No longer used to name idea files on disk — new ideas are named by
 * creation timestamp (`timestampFileName`) so autosave never has to guess a
 * title before the user has written one. This now backs the inbox list's row
 * title: a one-line stand-in for a document that may never grow a heading.
 */
export function slugFromMarkdown(md: string): string {
  const line = firstNonEmptyLine(stripLeadingFrontmatter(md))
  if (line == null) return 'idea'

  const cleaned = line
    .replace(FORBIDDEN_CHARS, '')
    .trim()
    .replace(/\s+/g, '-')
    .replace(/^-+|-+$/g, '')
  if (!cleaned) return 'idea'

  // Truncate by Unicode code point, not UTF-16 code unit or byte, so a
  // 40-character cut never splits a surrogate pair (emoji, astral CJK) in
  // half and produces a mangled/unpaired surrogate.
  const chars = Array.from(cleaned)
  if (chars.length <= 40) return cleaned
  const truncated = chars.slice(0, 40).join('').replace(/-+$/g, '')
  return truncated || 'idea'
}

/**
 * Splits a document into `[frontmatter, body]` — the ONE place that decides
 * what counts as a leading YAML frontmatter block. Everything that needs
 * either half goes through here (`stripLeadingFrontmatter` for the body,
 * store.svelte.ts `frontmatterOf` for the block), because two scanners are two
 * chances to disagree about where the body starts.
 *
 * Only a fence on the very first line counts — a `---` line further down the
 * document is a thematic break / mid-document separator, not frontmatter, and
 * must not be touched. A document that opens with `---` but never closes the
 * fence is reported as having no frontmatter at all (`[null, md]`): don't guess
 * where the body starts.
 *
 * The returned frontmatter excludes both fences and is normalized to `\n` line
 * endings, so a CRLF file round-trips without dragging carriage returns along.
 */
export function splitFrontmatter(md: string): [string | null, string] {
  const lines = md.split(/\r?\n/)
  if (lines[0]?.trim() !== '---') return [null, md]
  for (let i = 1; i < lines.length; i++) {
    if (lines[i].trim() === '---') return [lines.slice(1, i).join('\n'), lines.slice(i + 1).join('\n')]
  }
  return [null, md]
}

/** The body half of {@link splitFrontmatter} — the document minus its frontmatter. */
export function stripLeadingFrontmatter(md: string): string {
  return splitFrontmatter(md)[1]
}

function firstNonEmptyLine(md: string): string | null {
  for (const raw of md.split(/\r?\n/)) {
    const line = raw.trim()
    if (line) return line
  }
  return null
}

/**
 * `${today}-${slug}.md`, deduplicated against `taken` (existing filenames in
 * the idea directory) by appending `-2`, `-3`, ... . Also guards against ever
 * returning a reserved concept name (`index.md`/`log.md`) — structurally
 * unreachable given the mandatory `${today}-` prefix, but checked anyway as
 * defense in depth since naming.ts is the one place that decides this.
 */
export function ideaFileName(md: string, today: string, taken: Set<string>): string {
  const slug = slugFromMarkdown(md)
  const base = `${today}-${slug}`
  let candidate = `${base}.md`
  let n = 2
  while (taken.has(candidate) || isReservedConceptName(candidate)) {
    candidate = `${base}-${n}.md`
    n += 1
  }
  return candidate
}

/**
 * `YYYY-MM-DD-HHmm-idea.md`, taken from the **creation moment's local time**
 * (`toISOString()` would name a late-evening idea after tomorrow). Names are
 * deliberately not derived from the title: autosave writes to disk before the
 * user has typed a heading, and renaming after the fact would scatter one
 * idea across several files. A collision (two ideas opened in the same
 * minute) appends `-2`, `-3`, ….
 */
export function timestampFileName(now: Date, taken: Set<string>): string {
  const p = (n: number) => String(n).padStart(2, '0')
  const base = `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}-idea`
  let name = `${base}.md`
  let n = 2
  while (taken.has(name)) {
    name = `${base}-${n}.md`
    n += 1
  }
  return name
}

/** `inbox/ideas/a.md` → `inbox/ideas/a.proof.md`. */
export function proofPathFor(ideaRelPath: string): string {
  return ideaRelPath.endsWith('.md')
    ? `${ideaRelPath.slice(0, -3)}.proof.md`
    : `${ideaRelPath}.proof.md`
}
