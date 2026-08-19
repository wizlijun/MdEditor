import { classifyPath } from './fs'
import { basename } from './paths'

/** Matches a URI scheme prefix like `http:`, `mailto:`, `file:` (RFC 3986). */
const SCHEME_RE = /^[a-z][a-z0-9+.-]*:/i

export type LinkAction =
  /** External URL — hand to the system browser / default URL handler. */
  | { kind: 'browser'; url: string }
  /** Editable text/markdown/code file — open in a new editor tab. */
  | { kind: 'edit'; path: string }
  /** Non-editable local file (image, pdf, …) — open with the system default app. */
  | { kind: 'system'; path: string }
  /** In-document anchor or an unresolvable relative link — do nothing. */
  | { kind: 'ignore' }

/**
 * Resolve `href` against the directory of `basePath` (an absolute file path).
 * Returns an absolute path, or null when the link is relative but no base is
 * available (e.g. an untitled buffer). Normalises `.` / `..` segments.
 */
function resolveRelative(href: string, basePath: string | undefined): string | null {
  if (href.startsWith('/')) return normalize(href)
  if (!basePath) return null
  const dir = basePath.slice(0, basePath.lastIndexOf('/'))
  return normalize(`${dir}/${href}`)
}

function normalize(path: string): string {
  const out: string[] = []
  for (const seg of path.split('/')) {
    if (seg === '' || seg === '.') continue
    if (seg === '..') { out.pop(); continue }
    out.push(seg)
  }
  return '/' + out.join('/')
}

/**
 * Decide what a clicked link should do, given the current document's path
 * (used to resolve relative links). Pure — the caller performs the side effect.
 *
 *  - `#anchor`                         → ignore (in-document)
 *  - `http(s)://`, `mailto:`, `tel:` … → browser
 *  - `file://…` or a local path to an editable text file → edit (new tab)
 *  - a local path to an image / other  → system (default app)
 */
export function classifyLink(href: string, basePath: string | undefined): LinkAction {
  const raw = href.trim()
  if (!raw || raw.startsWith('#')) return { kind: 'ignore' }

  let target = raw
  if (/^file:\/\//i.test(target)) {
    target = decodeURIComponent(target.replace(/^file:\/\//i, ''))
  } else if (SCHEME_RE.test(target)) {
    // Any other scheme (http, https, mailto, tel, ftp, …) → system handler.
    return { kind: 'browser', url: raw }
  }

  // Local path (absolute or relative). Drop query string and fragment.
  const clean = target.split('#')[0].split('?')[0]
  const abs = resolveRelative(clean, basePath)
  if (!abs) return { kind: 'ignore' }

  const cls = classifyPath(abs)
  // Editable = text-bearing kinds. Images and unknown types open externally.
  if (cls && cls.kind !== 'image') return { kind: 'edit', path: abs }
  return { kind: 'system', path: abs }
}

/**
 * Resolve a `[[wikilink]]` target to an absolute `.md` path, relative to the
 * current document's directory.
 *
 *  - `[[foo]]`          → <dir>/foo.md
 *  - `[[foo|Display]]`  → <dir>/foo.md   (alias after `|` is display-only)
 *  - `[[notes/bar]]`    → <dir>/notes/bar.md
 *  - `[[baz.md]]`       → <dir>/baz.md   (existing extension kept)
 *
 * Returns null when the target is empty or the document is unsaved (no base
 * directory to resolve against).
 */
export function resolveWikilinkPath(name: string, basePath: string | undefined): string | null {
  let rel = name.split('|')[0].trim()
  if (!rel) return null
  if (!/\.[a-z0-9]+$/i.test(rel)) rel += '.md' // bare name → .md
  return resolveRelative(rel, basePath)
}

/**
 * Full text for a newly-created `[[wikilink]]` target file that lives outside
 * the vault (RichEditor's `openWikilink`). Extracted out of the component so
 * the write path is unit-testable without mounting the editor — a 0-byte
 * `.md` here violates OKF §4.1's only hard producer constraint (parseable
 * frontmatter + non-empty `type`) and used to be exactly what this call site
 * wrote. Routes through the same `newPageFileText` as the outline panel's
 * vault-external page creation, so both paths sign identically.
 */
export async function newWikilinkFileText(absPath: string): Promise<string> {
  const [{ newPageFileText }, { humanActor }] = await Promise.all([
    import('./outline/create'),
    import('./okf/identity'),
  ])
  const by = await humanActor().catch(() => null)
  const title = basename(absPath).replace(/\.md$/i, '')
  return newPageFileText(title, by ? { by, at: new Date().toISOString() } : undefined)
}

/**
 * Undo the backslash-escaping that @moraya/core's markdown serializer applies
 * to `[` and `]`, but only within `[[wikilink]]` spans, so wikilinks persist in
 * their literal `[[name]]` form instead of `\[\[name\]\]`. Idempotent on text
 * that is already clean.
 */
export function restoreWikilinks(md: string): string {
  return md.replace(
    /\\?\[\\?\[([^[\]\n|\\]+(?:\|[^[\]\n\\]+)?)\\?\]\\?\]/g,
    '[[$1]]',
  )
}
