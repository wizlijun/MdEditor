// Pure status-derivation for the idea history list. Callers gather the raw
// facts (directory listing, pending-run map, failed set) via the bridge and
// this module turns them into what the UI actually renders — no IO here.
import { isReservedConceptName } from './okf/concept'

export type IdeaStatus = 'draft' | 'running' | 'done' | 'failed'

/**
 * Derives one idea's status. `name` is whatever key convention the caller
 * uses consistently across `files`/`pending`/`failed` (a bare filename or a
 * vault-relative path both work, since this function only ever compares
 * values within those same collections).
 *
 * Priority: done (proof file exists) > running (active run) > failed
 * (previously marked failed, no active run) > draft (default).
 */
export function deriveStatus(
  name: string,
  files: Set<string>,
  pending: Record<string, string>,
  failed: Set<string>,
): IdeaStatus {
  const base = name.endsWith('.md') ? name.slice(0, -3) : name
  if (files.has(`${base}.proof.md`)) return 'done'
  if (name in pending) return 'running'
  if (failed.has(name)) return 'failed'
  return 'draft'
}

/**
 * Filters a directory listing down to idea files: `.md` files that aren't a
 * `.proof.md` sidecar (those describe an idea, they aren't one), aren't a
 * directory, and aren't a reserved concept name (`index.md`/`log.md`).
 * Sorted newest-first — idea filenames are `YYYY-MM-DD-...`, so a plain
 * descending string sort puts the newest date first.
 */
export function listIdeas(entries: Array<{ name: string; is_dir: boolean }>): string[] {
  return entries
    .filter((e) => !e.is_dir)
    .filter((e) => e.name.endsWith('.md') && !e.name.endsWith('.proof.md'))
    .filter((e) => !isReservedConceptName(e.name))
    .map((e) => e.name)
    .sort((a, b) => (a < b ? 1 : a > b ? -1 : 0))
}
