// src/lib/library.ts — pure state for the book library: every book already in
// the vault under `<ebooks_root>/<YYYY-MM>/<Title>/`, not just what this
// session imported. Same shape as queue.ts: every export takes a list and
// returns a new one, and nothing here touches the bridge.

import type { AiStatus, BackendAiEvent, LocalAiEvent } from './queue'

/** One book as `plugin.library_list` reports it (see backend `library.rs`). */
export interface RawBook {
  /** Vault-relative, POSIX-separated book directory — the same shape `dest_rel`
   * has, so it feeds `ai_read_start` unchanged. */
  rel: string
  name: string
  month: string
  topic_id?: string | null
  topic_label?: string | null
  /** `YYYY-MM-DD-summary.md` file names, newest first. */
  summaries: string[]
}

export interface LibraryBook extends RawBook {
  /** The AI-read job this row is following. Unset while a claimed read waits
   * for `ai_read_start` to report the id the backend allocated. */
  aiJobId?: number
  aiStatus?: AiStatus
  aiStartedAt?: string
  aiSummaryRel?: string
  aiError?: string
}

/**
 * Rebuilds the list from a fresh `library_list` answer, carrying each book's
 * AI-read state over by `rel`.
 *
 * The carry-over is the whole point: the list refreshes whenever an import
 * lands, which is exactly when another book may be mid-read. Rebuilding the
 * rows from scratch would reset that row to "no AI read" and drop the `aiJobId`
 * the backend's later pushes address — the run would finish invisibly.
 * The backend's answer is authoritative about which books EXIST, so a book it
 * no longer reports is gone from the list even if it was being read.
 */
export function mergeLibrary(prev: LibraryBook[], incoming: RawBook[]): LibraryBook[] {
  const byRel = new Map(prev.map((b) => [b.rel, b]))
  return incoming.map((b) => {
    const old = byRel.get(b.rel)
    return old ? { ...old, ...b } : { ...b }
  })
}

/** The newest digest for `book` as a vault path, or undefined if unread. */
export function latestSummary(book: LibraryBook): string | undefined {
  // A read that finished this session wrote a file the last listing could not
  // have known about, so the run's own answer wins over the listing.
  if (book.aiSummaryRel) return book.aiSummaryRel
  const newest = book.summaries[0]
  return newest ? `${book.rel}/${newest}` : undefined
}

/** Books whose name contains `query`, case-insensitively. Blank shows all. */
export function filterBooks(
  list: LibraryBook[],
  query: string,
  topicId: string | null = null,
): LibraryBook[] {
  const q = query.trim().toLowerCase()
  return list.filter(
    (b) =>
      (!topicId || (topicId === '__unclassified__' ? !b.topic_id : b.topic_id === topicId)) &&
      (!q || b.name.toLowerCase().includes(q) || (b.topic_label ?? '').toLowerCase().includes(q)),
  )
}

/**
 * Marks `rel`'s row as queued for an AI read, synchronously — before the
 * `ai_read_start` RPC is sent, not after it resolves. Same reasoning as
 * `queue.ts`'s `reserve`: the button has to be gone within this tick, or a
 * double-click sends two reads of the same book. (The backend refuses the
 * second one now, but the row would still flicker through a state it was never
 * in.) `aiJobId` stays unset — the backend allocates it, and `bindAiJob` folds
 * it in once the response arrives.
 */
export function claimAiRead(list: LibraryBook[], rel: string): LibraryBook[] {
  return list.map((b) =>
    b.rel === rel
      ? { ...b, aiStatus: 'queued' as AiStatus, aiJobId: undefined, aiError: undefined }
      : b,
  )
}

/**
 * Rolls `rel`'s row back to failed. For the one case `onLibraryAiEvent` cannot
 * handle: `ai_read_start` itself rejected, so there is no job id to address the
 * row by and no push is ever coming. Without this the row sits on "waiting for
 * AI" forever, with no way to ask again.
 */
export function failAiRead(list: LibraryBook[], rel: string, error: string): LibraryBook[] {
  return list.map((b) =>
    b.rel === rel ? { ...b, aiStatus: 'failed' as AiStatus, aiError: error } : b,
  )
}

/** Folds the job id `ai_read_start` answered with into `rel`'s row. */
export function bindAiJob(list: LibraryBook[], rel: string, jobId: number): LibraryBook[] {
  return list.map((b) => (b.rel === rel ? { ...b, aiJobId: jobId } : b))
}

/**
 * Folds an `ai_read` push into the list. Rows are addressed by `aiJobId`, and
 * an event for a job no row owns is ignored — the same push also goes to the
 * import queue's reducer, and most of them belong there.
 *
 * The exception is the race: the backend spawns the AI worker BEFORE writing
 * `ai_read_start`'s response, and `started` is the first thing that worker
 * posts, so it can beat the allocated id home. When that happens the event
 * lands on the row that has claimed a read but has not been told its id — but
 * only if there is exactly one such row. Two of them and this would be a
 * guess, so it guesses nothing rather than moving the wrong book.
 */
export function onLibraryAiEvent(
  list: LibraryBook[],
  jobId: number,
  ev: BackendAiEvent | LocalAiEvent,
): LibraryBook[] {
  let idx = list.findIndex((b) => b.aiJobId === jobId)
  if (idx === -1) {
    const unbound = list.reduce<number[]>(
      (acc, b, i) => (b.aiStatus === 'queued' && b.aiJobId == null ? [...acc, i] : acc),
      [],
    )
    if (unbound.length !== 1) return list
    idx = unbound[0]
  }

  const b = { ...list[idx], aiJobId: jobId }
  let next: LibraryBook
  switch (ev.event) {
    case 'queued':
      next = { ...b, aiStatus: 'queued', aiError: undefined }
      break
    case 'started':
      next = {
        ...b,
        aiStatus: 'running',
        aiStartedAt: ev.started_at,
        aiSummaryRel: ev.summary_rel,
        aiError: undefined,
      }
      break
    case 'done':
      next = { ...b, aiStatus: 'done', aiSummaryRel: ev.summary_rel ?? b.aiSummaryRel }
      break
    case 'failed':
      next = { ...b, aiStatus: 'failed', aiError: ev.error }
      break
  }
  const out = [...list]
  out[idx] = next
  return out
}
