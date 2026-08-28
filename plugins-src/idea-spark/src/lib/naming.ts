// Pure filename-generation logic for Idea Spark. No IO — callers pass in
// whatever context (today's date, the set of names already on disk) so this
// stays trivially testable and reusable from both the App and any future
// CLI/tests.
import { isReservedConceptName } from './okf/concept'

/** The filename contract that distinguishes an idea from other Markdown. */
export const IDEA_FILE_SUFFIX = '.idea.md'

export function isIdeaFileName(name: string): boolean {
  return name.endsWith(IDEA_FILE_SUFFIX)
}

/** Keeps rename input ergonomic while ensuring the result is still an idea. */
export function withIdeaFileSuffix(name: string): string {
  if (isIdeaFileName(name)) return name
  const base = name.endsWith('.md') ? name.slice(0, -3) : name
  return `${base}${IDEA_FILE_SUFFIX}`
}

/** Characters that are unsafe (or at least unwelcome) in a filesystem name,
 *  or that collide with markdown syntax (`#`) / template syntax (`%`, backtick). */
const FORBIDDEN_CHARS = /[\\/:*?"<>|#%`]/g

/**
 * Dead in production as of T7, exactly like `ideaFileName` below (its only
 * remaining caller, itself dead since T4). It briefly backed the inbox list's
 * row title; that job moved to `titleFromMarkdown`, which keeps spaces,
 * punctuation and length instead of mangling them into a file name. Nothing
 * outside this file's own tests reaches it any more:
 *   * row titles → `titleFromMarkdown`
 *   * file names → `timestampFileName`
 * Kept rather than deleted because this task's brief didn't ask for the
 * removal; a later cleanup should take both functions out together.
 *
 * Derives a filename-safe slug from the first non-empty line of a markdown
 * document's *body* — a leading YAML frontmatter block, if present, is
 * skipped first (works whether that line is a heading or a plain paragraph —
 * heading markers are stripped as part of the forbidden-character pass, so no
 * special-casing is needed). Falls back to `'idea'` when there is no usable
 * text at all: an empty document, a frontmatter-only document (nothing left
 * after the closing fence), a document whose frontmatter fence is never
 * closed (see `stripLeadingFrontmatter`), or a title made entirely of
 * forbidden characters.
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
 * The document's own title, for *reading*: its first non-empty body line with
 * the markdown that decorates it stripped — the `#` of a heading, a `>` quote
 * marker, a `-`/`*`/`+`/`1.` list bullet — and nothing else. Null when the
 * document has no usable text (empty, frontmatter only, an unclosed fence).
 *
 * Deliberately NOT `slugFromMarkdown`, which is a *file name* generator: that
 * one turns spaces into hyphens, deletes every character a filesystem or
 * markdown might object to, and hard-truncates at 40 code points. As a row
 * label those transformations are all damage — `# Ship the thing` would read
 * `Ship-the-thing` — and the truncation is redundant besides, since the inbox
 * column already ellipsizes in CSS (which, unlike a cut, *says* that it cut).
 * Spaces, punctuation and length are therefore left exactly as written.
 */
export function titleFromMarkdown(md: string): string | null {
  const line = firstNonEmptyLine(stripLeadingFrontmatter(md))
  if (line == null) return null
  const stripped = line
    // `#{1,6}` needs the space (or the end of the line) after it, so a line
    // opening on `#hashtag` keeps its hash — that is a word, not a heading.
    .replace(/^\s{0,3}(?:#{1,6}(?:\s+|$)|>\s*|[-*+]\s+|\d+[.)]\s+)/, '')
    .trim()
  return stripped || null
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
 * Dead in production as of T4: no caller in `store.svelte.ts` reaches this
 * anymore — new ideas are named by `timestampFileName` instead (title-based
 * naming was dropped so autosave never has to guess a title before the user
 * has written one). Kept only for its own tests; left for a later cleanup
 * task to remove rather than deleted here, since this task's brief didn't
 * ask for it.
 *
 * `${today}-${slug}.idea.md`, deduplicated against `taken` (existing filenames
 * in the idea directory) by appending `-2`, `-3`, ... . Also guards against ever
 * returning a reserved concept name (`index.md`/`log.md`) — structurally
 * unreachable given the mandatory `${today}-` prefix, but checked anyway as
 * defense in depth since naming.ts is the one place that decides this.
 */
export function ideaFileName(md: string, today: string, taken: Set<string>): string {
  const slug = slugFromMarkdown(md)
  const base = `${today}-${slug}`
  let candidate = `${base}${IDEA_FILE_SUFFIX}`
  let n = 2
  while (taken.has(candidate) || taken.has(proofPathFor(candidate)) || isReservedConceptName(candidate)) {
    candidate = `${base}-${n}${IDEA_FILE_SUFFIX}`
    n += 1
  }
  return candidate
}

/**
 * `YYYY-MM-DD-HHmm.idea.md`, taken from the **creation moment's local time**
 * (`toISOString()` would name a late-evening idea after tomorrow). Names are
 * deliberately not derived from the title: autosave writes to disk before the
 * user has typed a heading, and renaming after the fact would scatter one
 * idea across several files. A collision (two ideas opened in the same
 * minute) appends `-2`, `-3`, ….
 */
export function timestampFileName(now: Date, taken: Set<string>): string {
  const p = (n: number) => String(n).padStart(2, '0')
  const base = `${now.getFullYear()}-${p(now.getMonth() + 1)}-${p(now.getDate())}-${p(now.getHours())}${p(now.getMinutes())}`
  let name = `${base}${IDEA_FILE_SUFFIX}`
  let n = 2
  while (taken.has(name) || taken.has(proofPathFor(name))) {
    name = `${base}-${n}${IDEA_FILE_SUFFIX}`
    n += 1
  }
  return name
}

/** `inbox/ideas/a.idea.md` → `inbox/ideas/a.idea.proof.md`. */
export function proofPathFor(ideaRelPath: string): string {
  return ideaRelPath.endsWith('.md')
    ? `${ideaRelPath.slice(0, -3)}.proof.md`
    : `${ideaRelPath}.proof.md`
}
