// Pure status-derivation for the idea history list. Callers gather the raw
// facts (directory listing, pending-run map, failed set) via the bridge and
// this module turns them into what the UI actually renders — no IO here.
import type { IconName } from './icons'
import { isIdeaFileName } from './naming'
import type { MessageKey } from './strings'

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
 * Filters a directory listing down to ordinary files whose names match the
 * plugin's strict `*-idea.md` contract. Other Markdown and proof sidecars are
 * documents in the same directory, but they aren't ideas.
 * Sorted newest-first — idea filenames are `YYYY-MM-DD-...`, so a plain
 * descending string sort puts the newest date first.
 */
export function listIdeas(entries: Array<{ name: string; is_dir: boolean }>): string[] {
  return entries
    .filter((e) => !e.is_dir)
    .filter((e) => isIdeaFileName(e.name))
    .map((e) => e.name)
    .sort((a, b) => (a < b ? 1 : a > b ? -1 : 0))
}

// ── how a status is spelled on an inbox row ─────────────────────────────────
//
// These two tables live here, next to `IdeaStatus`, rather than inside
// `InboxPanel.svelte` where they used to. The reason is `STATUS_MARK.done`
// below: it encodes a *product* convention, and a convention that only a
// component holds is one no test can reach. In pure TS it is guarded by
// `status.test.ts` like everything else in this file.
//
// This module stays IO-free and DOM-free: both imports above are type-only.

/** The accessible name / tooltip of a status, as a `strings.ts` key. */
export const STATUS_KEY: Record<IdeaStatus, MessageKey> = {
  draft: 'statusDraft',
  running: 'statusRunning',
  done: 'statusDone',
  failed: 'statusFailed',
}

/**
 * What a row wears in its status column, per design §5. `null` is "no badge":
 * most rows are drafts, and a badge on every one of them is just noise.
 *
 * Two kinds on purpose, and the split is not cosmetic:
 *
 *   * `running` and `failed` are stroke icons (drawn at 12px). They used to be
 *     the emoji ⏳ and the glyph ⚠, neither of which could follow the row's
 *     color — a macOS emoji is a color bitmap that ignores the theme entirely,
 *     and `failed` has to be able to take the warning red from `.mark.failed`.
 *   * `done` keeps the literal `✦`, and MUST keep it. That is a product
 *     convention, not an icon: across note.md `✦` means "written by AI" and
 *     `●` means "written by you" (CLAUDE.md, belief 3). An argued idea is
 *     marked with the same `✦` the proof document itself carries, so swapping
 *     in a generic check mark would quietly throw that meaning away. There is
 *     deliberately no `done`-ish icon in `icons.ts` to swap in, and
 *     `status.test.ts` pins this entry.
 */
export type StatusMark = { kind: 'icon'; icon: IconName } | { kind: 'glyph'; text: string } | null

export const STATUS_MARK: Record<IdeaStatus, StatusMark> = {
  draft: null,
  running: { kind: 'icon', icon: 'running' },
  done: { kind: 'glyph', text: '✦' },
  failed: { kind: 'icon', icon: 'failed' },
}
