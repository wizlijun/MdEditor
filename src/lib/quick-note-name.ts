// Naming rules for quick notes. Pure (no Tauri imports) so it is directly
// testable and safe to import from both the store and the save paths.

import { sanitizeFileName } from './outline/slug'

/** Auto-generated quick-note basename, capturing its `YYYY-MM-DD` date. */
const AUTO_QUICK_RE = /^(\d{4}-\d{2}-\d{2})-\d{6}-quick\.md$/i

/** Longest slug kept from a title, in characters. */
const MAX_SLUG_LEN = 50

/** `YYYY-MM-DD-HHmmss-quick.md` for the given moment. */
export function quickNoteFileName(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  const date = `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
  const time = `${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`
  return `${date}-${time}-quick.md`
}

/** True while `basename` is still the untouched auto-generated quick-note name. */
export function isAutoQuickNoteName(basename: string): boolean {
  return AUTO_QUICK_RE.test(basename)
}

/**
 * A title turned into a filename fragment. Non-ASCII (CJK) is kept verbatim per
 * the project's file-over-app naming rule — only filesystem-illegal characters
 * are replaced. Returns null when nothing usable survives, so callers skip the
 * rename rather than produce a `…-untitled.md`.
 */
export function titleSlug(title: string): string | null {
  const collapsed = title.replace(/\s+/g, '-')
  const safe = sanitizeFileName(collapsed)
  if (safe === 'untitled') return null
  const slug = safe
    .replace(/-{2,}/g, '-')
    .slice(0, MAX_SLUG_LEN)
    .replace(/^-+|-+$/g, '')
  return slug === '' ? null : slug
}

/** First ATX H1 in `text`, or null. Mirrors folder-view's `parseFirstH1`. */
export function firstH1(text: string): string | null {
  const m = text.match(/^#\s+(.+?)\s*$/m)
  return m ? m[1] : null
}

/**
 * The basename an auto-named quick note should take once it has an H1 title:
 * its date plus the title (`2026-07-25-产品思考.md`). The creation-time `HHmmss`
 * only exists to keep untitled notes apart — once a title names the note, the
 * date alone reads better, and same-day duplicates are resolved by the caller.
 *
 * Returns null when the note was already renamed (the name no longer matches
 * the auto pattern), has no H1, or the title yields no usable slug — renaming
 * happens once, so later title edits leave the path (and any links to it) alone.
 */
export function quickNoteRenameTarget(basename: string, content: string): string | null {
  const stamped = AUTO_QUICK_RE.exec(basename)
  if (!stamped) return null
  const title = firstH1(content)
  if (!title) return null
  const slug = titleSlug(title)
  if (!slug) return null
  return `${stamped[1]}-${slug}.md`
}
