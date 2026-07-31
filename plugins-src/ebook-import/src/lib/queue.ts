// src/lib/queue.ts — pure, immutable queue state machine driving the import
// window's serial scheduler. Every export takes a `Queue` and returns a new
// one; nothing here talks to the bridge (App.svelte owns that: it calls
// `addPaths`/`onJobEvent` on drops/pushes, calls `nextToStart` after each,
// and — when it gets an item back — issues `plugin.import_start` itself and
// folds the returned `job_id` into the item, since that RPC is the only step
// with a side effect).

/** Extensions the backend pipeline (Task 8) knows how to import. */
const ACCEPTED_EXTENSIONS = ['epub', 'pdf', 'docx']

export type ItemStatus = 'pending' | 'running' | 'done' | 'failed'

export interface QueueItem {
  id: number
  path: string
  name: string
  status: ItemStatus
  stage?: string
  page?: number
  total?: number
  destRel?: string
  error?: string
  /**
   * Set when this item's job was cancelled by the user (backend
   * `import_cancel`), which surfaces as `event:"failed", error:"cancelled"`.
   * Design decision: rather than adding a 5th ItemStatus, a cancelled job
   * keeps `status:"failed"` (so any code that only knows the 4 core
   * statuses still behaves sensibly) and sets this flag with `error` left
   * unset, so the UI can render a neutral "cancelled" badge instead of an
   * error message by checking `cancelled` before `error`.
   */
  cancelled?: boolean
  jobId?: number
  logs: string[]
}

export interface Queue {
  items: QueueItem[]
  activeId: number | null
}

/** Payload shape of a host `type:"job"` push (Task 8's `run_job`/`import_cancel`). */
export interface JobEvent {
  event: 'log' | 'progress' | 'done' | 'failed'
  line?: string
  stage?: string
  page?: number
  total?: number
  dest_rel?: string
  error?: string
}

function extensionOf(path: string): string {
  const m = /\.([^./\\]+)$/.exec(path)
  return m ? m[1].toLowerCase() : ''
}

function baseName(path: string): string {
  const parts = path.split(/[/\\]/)
  return parts[parts.length - 1] || path
}

function nextId(items: QueueItem[]): number {
  return items.reduce((max, i) => Math.max(max, i.id), 0) + 1
}

/**
 * Adds `paths` to the queue: silently drops anything whose extension isn't
 * epub/pdf/docx, and skips a path that already has a pending or running item
 * (re-dropping the same file while it's mid-import shouldn't queue it twice).
 * A path whose only prior item already finished (done/failed) is a fresh
 * add — that's a deliberate retry, not a duplicate.
 */
export function addPaths(q: Queue, paths: string[]): Queue {
  let items = q.items
  let changed = false
  for (const path of paths) {
    if (!ACCEPTED_EXTENSIONS.includes(extensionOf(path))) continue
    const dup = items.some((i) => i.path === path && (i.status === 'pending' || i.status === 'running'))
    if (dup) continue
    items = [
      ...items,
      {
        id: nextId(items),
        path,
        name: baseName(path),
        status: 'pending',
        logs: [],
      },
    ]
    changed = true
  }
  return changed ? { ...q, items } : q
}

/** The first pending item to start, or null while one is already active. */
export function nextToStart(q: Queue): QueueItem | null {
  if (q.activeId != null) return null
  return q.items.find((i) => i.status === 'pending') ?? null
}

/**
 * Synchronously claims `itemId` as the active item: flips its status to
 * "running" and sets `q.activeId` — `jobId` stays unset (the
 * `plugin.import_start` RPC hasn't resolved yet; the caller fills it in once
 * it does).
 *
 * MUST be called in the same synchronous step as `nextToStart()`, before
 * awaiting the RPC it feeds. The scheduler in App.svelte has several
 * re-entrant call sites (a drop, "Add files…", a job's done/failed push, and
 * its own failure-retry path all call `schedule()`), and JS only yields to
 * another of those at an `await`. If `activeId` were written only after the
 * RPC resolves, two `schedule()` calls racing across that await would both
 * read `nextToStart(q)` as the same pending item and both start it. Calling
 * `reserve` right after `nextToStart` — with no `await` in between — closes
 * that window: the second call sees the first's `reserve` synchronously and
 * gets `null` from `nextToStart` instead.
 */
export function reserve(q: Queue, itemId: number): Queue {
  return {
    ...q,
    activeId: itemId,
    items: q.items.map((i) => (i.id === itemId ? { ...i, status: 'running' } : i)),
  }
}

/**
 * Folds a `type:"job"` push into the queue: looks up the item by `jobId`
 * (not `activeId` — a late event for a job the UI has already moved past
 * should still land on the right row) and updates it in place. `done`/
 * `failed` also clear `activeId` so the scheduler can start the next pending
 * item; events for an unknown `jobId` (e.g. arriving after `clear()` removed
 * the item) leave the queue untouched.
 */
export function onJobEvent(q: Queue, jobId: number, ev: JobEvent): Queue {
  const idx = q.items.findIndex((i) => i.jobId === jobId)
  if (idx === -1) return q

  const item = q.items[idx]
  let next: QueueItem
  switch (ev.event) {
    case 'log':
      next = ev.line == null ? item : { ...item, logs: [...item.logs, ev.line] }
      break
    case 'progress':
      next = {
        ...item,
        ...(ev.stage !== undefined ? { stage: ev.stage } : {}),
        ...(ev.page !== undefined ? { page: ev.page } : {}),
        ...(ev.total !== undefined ? { total: ev.total } : {}),
      }
      break
    case 'done':
      next = { ...item, status: 'done', destRel: ev.dest_rel }
      break
    case 'failed':
      next =
        ev.error === 'cancelled'
          ? { ...item, status: 'failed', cancelled: true, error: undefined }
          : { ...item, status: 'failed', error: ev.error }
      break
  }

  const items = q.items.map((i, i2) => (i2 === idx ? next : i))
  const activeId = ev.event === 'done' || ev.event === 'failed' ? (q.activeId === item.id ? null : q.activeId) : q.activeId
  return { items, activeId }
}

/** One `type:"job"` push whose `job_id` couldn't yet be matched to an item. */
export interface PendingJobEvent {
  jobId: number
  ev: JobEvent
}

/**
 * Backend race (Task 9 review, Finding 1): `import_start` spawns the job
 * thread BEFORE writing its RPC response, so a fast failure (calibre
 * missing, OCR keys missing, pdfium missing) can push its `job_id`'s first
 * event — sometimes `done`/`failed` itself — before the UI's `schedule()`
 * has resolved `import_start` and folded that `job_id` into the item. At
 * that instant `onJobEvent` would find no item with a matching `jobId` and
 * silently no-op the event, permanently: the row stays "running", `activeId`
 * never clears, and the queue stalls.
 *
 * `stashOrApply` is the receiving half of the fix: called from `onMessage`
 * for every `type:"job"` push in place of a bare `onJobEvent`. If the
 * `jobId` already matches an item, it applies immediately (the common
 * case). Otherwise, only if there's an item that's `activeId` but whose
 * `jobId` is still unset (i.e. `import_start` hasn't resolved yet) does it
 * stash the event for later replay — that's the one case where "unknown
 * jobId" plausibly means "ours, just not labeled yet" rather than "stale/
 * already gone". Anything else (no active-unresolved item) is a genuinely
 * unknown jobId and is dropped, same as `onJobEvent` always did.
 */
export function stashOrApply(
  q: Queue,
  pending: PendingJobEvent[],
  jobId: number,
  ev: JobEvent,
): { q: Queue; pending: PendingJobEvent[]; applied: boolean } {
  const known = q.items.some((i) => i.jobId === jobId)
  if (known) {
    return { q: onJobEvent(q, jobId, ev), pending, applied: true }
  }
  const activeUnresolved =
    q.activeId != null && q.items.some((i) => i.id === q.activeId && i.jobId == null)
  if (activeUnresolved) {
    return { q, pending: [...pending, { jobId, ev }], applied: false }
  }
  return { q, pending, applied: false }
}

/**
 * Replays every stashed event whose `jobId` matches, in arrival order,
 * through `onJobEvent` — called right after `schedule()` folds a resolved
 * `job_id` into an item, so events that raced ahead of the RPC response
 * (see `stashOrApply`) land on the right row instead of being lost. Matching
 * entries are consumed; anything left in `pending` (for a different,
 * still-unresolved job) is returned untouched.
 */
export function replayPending(
  q: Queue,
  pending: PendingJobEvent[],
  jobId: number,
): { q: Queue; pending: PendingJobEvent[] } {
  let next = q
  const rest: PendingJobEvent[] = []
  for (const p of pending) {
    if (p.jobId === jobId) next = onJobEvent(next, jobId, p.ev)
    else rest.push(p)
  }
  return { q: next, pending: rest }
}
