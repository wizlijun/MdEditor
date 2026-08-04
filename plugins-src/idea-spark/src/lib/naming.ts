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
 * skipped first so a saved idea (which always has one once round-tripped
 * through `buildIdeaDoc`) still names itself off its real title instead of
 * the `---` fence (works whether that line is a heading or a plain
 * paragraph — heading markers are stripped as part of the forbidden-
 * character pass, so no special-casing is needed). Falls back to `'idea'`
 * when there is no usable text at all: an empty document, a frontmatter-only
 * document (nothing left after the closing fence), a document whose
 * frontmatter fence is never closed (see `stripLeadingFrontmatter`), or a
 * title made entirely of forbidden characters.
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
 * Skips a leading YAML frontmatter block (`---` fence, content, `---`
 * fence). Exported because the App needs the exact same "where does the body
 * start" rule when it loads a saved idea back into the editor (store.svelte.ts
 * `bodyOf`) — two implementations of this would be two chances to disagree
 * about what counts as frontmatter. Only a fence at the very first line counts — a `---` line further
 * down the document is a thematic break / mid-document separator, not
 * frontmatter, and must not be touched. If the document opens with `---`
 * but the fence is never closed, the whole document is returned unchanged
 * (don't guess where the "body" starts; fall back to scanning from the top,
 * same as before this function existed).
 */
export function stripLeadingFrontmatter(md: string): string {
  const lines = md.split(/\r?\n/)
  if (lines[0]?.trim() !== '---') return md
  for (let i = 1; i < lines.length; i++) {
    if (lines[i].trim() === '---') return lines.slice(i + 1).join('\n')
  }
  return md
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

/** `inbox/ideas/a.md` → `inbox/ideas/a.proof.md`. */
export function proofPathFor(ideaRelPath: string): string {
  return ideaRelPath.endsWith('.md')
    ? `${ideaRelPath.slice(0, -3)}.proof.md`
    : `${ideaRelPath}.proof.md`
}
