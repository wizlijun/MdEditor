import { describe, expect, it } from 'vitest'
import {
  bindAiJob,
  claimAiRead,
  failAiRead,
  filterBooks,
  latestSummary,
  mergeLibrary,
  onLibraryAiEvent,
  replayPendingLibraryAi,
  stashOrApplyLibraryAi,
  type LibraryBook,
  type RawBook,
} from './library'

const raw = (name: string, summaries: string[] = []): RawBook => ({
  rel: `ssot/ebooks/2026-08/${name}`,
  name,
  month: '2026-08',
  summaries,
})

const book = (name: string, extra: Partial<LibraryBook> = {}): LibraryBook => ({
  ...raw(name),
  ...extra,
})

describe('mergeLibrary', () => {
  it('turns a backend listing into rows', () => {
    const got = mergeLibrary([], [raw('Seven Powers'), raw('Old Book')])
    expect(got.map((b) => b.name)).toEqual(['Seven Powers', 'Old Book'])
    expect(got[0].aiStatus).toBeUndefined()
  })

  // The list refreshes whenever an import finishes — which is exactly when
  // another book may be mid-AI-read. Rebuilding rows from scratch would reset
  // that row to "no AI read", losing the only handle on a run in progress.
  it('keeps the AI state of a book that is being read across a refresh', () => {
    const prev = [book('Seven Powers', { aiJobId: 4, aiStatus: 'running', aiStartedAt: 'T' })]
    const got = mergeLibrary(prev, [raw('Seven Powers'), raw('New Import')])
    expect(got[0]).toMatchObject({ aiJobId: 4, aiStatus: 'running', aiStartedAt: 'T' })
    expect(got[1].aiStatus).toBeUndefined()
  })

  it('picks up summaries written since the last refresh', () => {
    const prev = [book('Seven Powers', { aiJobId: 4, aiStatus: 'done' })]
    const got = mergeLibrary(prev, [raw('Seven Powers', ['2026-08-26-summary.md'])])
    expect(got[0].summaries).toEqual(['2026-08-26-summary.md'])
  })

  // A book deleted from disk between refreshes must go, even mid-read: the row
  // would offer to open a summary that isn't there.
  it('drops a book the backend no longer reports', () => {
    const prev = [book('Gone', { aiStatus: 'running' })]
    expect(mergeLibrary(prev, [raw('Still Here')]).map((b) => b.name)).toEqual(['Still Here'])
  })
})

describe('latestSummary', () => {
  it('is the newest of the summaries on disk, as a vault path', () => {
    const b = book('Seven Powers', {
      summaries: ['2026-08-26-summary.md', '2026-08-04-summary.md'],
    })
    expect(latestSummary(b)).toBe('ssot/ebooks/2026-08/Seven Powers/2026-08-26-summary.md')
  })

  // The read that just finished this session wrote a file the last listing
  // could not have known about.
  it('prefers the summary the current run reported over the last listing', () => {
    const b = book('Seven Powers', {
      summaries: ['2026-08-04-summary.md'],
      aiSummaryRel: 'ssot/ebooks/2026-08/Seven Powers/2026-08-26-summary.md',
    })
    expect(latestSummary(b)).toBe('ssot/ebooks/2026-08/Seven Powers/2026-08-26-summary.md')
  })

  it('is undefined for a book nobody has read', () => {
    expect(latestSummary(book('Unread'))).toBeUndefined()
  })
})

describe('filterBooks', () => {
  const list = [book('Seven Powers'), book('Deep Work'), book('深度工作')]

  it('matches part of a name, ignoring case', () => {
    expect(filterBooks(list, 'wor').map((b) => b.name)).toEqual(['Deep Work'])
    expect(filterBooks(list, 'SEVEN').map((b) => b.name)).toEqual(['Seven Powers'])
    expect(filterBooks(list, '深度').map((b) => b.name)).toEqual(['深度工作'])
  })

  it('returns everything for an empty or whitespace query', () => {
    expect(filterBooks(list, '')).toHaveLength(3)
    expect(filterBooks(list, '   ')).toHaveLength(3)
  })

  it('filters by topic, unclassified state, and topic label text', () => {
    const themed = [
      book('Seven Powers', { topic_id: 'business', topic_label: 'Business Strategy' }),
      book('DDIA', { topic_id: 'software', topic_label: 'Software Engineering' }),
      book('Legacy'),
    ]
    expect(filterBooks(themed, '', 'software').map((b) => b.name)).toEqual(['DDIA'])
    expect(filterBooks(themed, '', '__unclassified__').map((b) => b.name)).toEqual(['Legacy'])
    expect(filterBooks(themed, 'strategy').map((b) => b.name)).toEqual(['Seven Powers'])
  })
})

describe('claimAiRead', () => {
  // Same reason reserve() exists in queue.ts: the button has to disappear
  // within this tick, not after the RPC resolves, or a double-click queues two
  // reads of the same book.
  it('marks the row queued before the request is even sent', () => {
    const got = claimAiRead([book('A'), book('B')], 'ssot/ebooks/2026-08/B')
    expect(got[1].aiStatus).toBe('queued')
    expect(got[1].aiJobId).toBeUndefined()
    expect(got[0].aiStatus).toBeUndefined()
  })

  it('clears the error left by a previous failed read', () => {
    const prev = [book('A', { aiStatus: 'failed', aiError: 'boom' })]
    expect(claimAiRead(prev, prev[0].rel)[0].aiError).toBeUndefined()
  })
})

describe('failAiRead', () => {
  // When `ai_read_start` itself rejects there is no job id to address the row
  // by — and no push is coming either. Without this the row would sit on
  // "waiting for AI" forever, with no way to try again.
  it('rolls a claimed row back to failed so it can be retried', () => {
    const list = claimAiRead([book('A'), book('B')], 'ssot/ebooks/2026-08/A')
    const got = failAiRead(list, 'ssot/ebooks/2026-08/A', 'no vault configured')
    expect(got[0]).toMatchObject({ aiStatus: 'failed', aiError: 'no vault configured' })
    expect(got[1].aiStatus).toBeUndefined()
  })
})

describe('bindAiJob', () => {
  it('folds the id the backend allocated into the row that claimed the read', () => {
    const list = claimAiRead([book('A')], 'ssot/ebooks/2026-08/A')
    expect(bindAiJob(list, 'ssot/ebooks/2026-08/A', 9)[0].aiJobId).toBe(9)
  })
})

describe('onLibraryAiEvent', () => {
  const claimed = (jobId?: number) => {
    const l = claimAiRead([book('A')], 'ssot/ebooks/2026-08/A')
    return jobId == null ? l : bindAiJob(l, 'ssot/ebooks/2026-08/A', jobId)
  }

  it('starts, finishes and fails the row it addresses', () => {
    let l = claimed(4)
    l = onLibraryAiEvent(l, 4, { event: 'started', started_at: 'T', summary_rel: 's.md' })
    expect(l[0]).toMatchObject({ aiStatus: 'running', aiStartedAt: 'T', aiSummaryRel: 's.md' })
    l = onLibraryAiEvent(l, 4, { event: 'done', summary_rel: 's.md' })
    expect(l[0].aiStatus).toBe('done')
    l = onLibraryAiEvent(claimed(4), 4, { event: 'failed', error: 'nope' })
    expect(l[0]).toMatchObject({ aiStatus: 'failed', aiError: 'nope' })
  })

  // The backend spawns the AI worker BEFORE writing ai_read_start's response,
  // and `started` is the first thing that worker posts — so it can beat the
  // allocated id home. Dropping it leaves the row stuck on "waiting for AI"
  // for the whole run, with no elapsed time, even though it is running.
  it('lands an event that beat its own job id home', () => {
    const l = onLibraryAiEvent(claimed(), 7, { event: 'started', started_at: 'T' })
    expect(l[0]).toMatchObject({ aiJobId: 7, aiStatus: 'running', aiStartedAt: 'T' })
  })

  // …but only when there is exactly one row it could belong to. Two unbound
  // claims and this is a guess, so guess nothing rather than move the wrong row.
  it('does not guess when two rows are waiting for an id', () => {
    let l = claimAiRead([book('A'), book('B')], 'ssot/ebooks/2026-08/A')
    l = claimAiRead(l, 'ssot/ebooks/2026-08/B')
    expect(onLibraryAiEvent(l, 7, { event: 'started' })).toEqual(l)
  })

  it('ignores an event for a job no row owns', () => {
    const l = claimed(4)
    expect(onLibraryAiEvent(l, 99, { event: 'done' })).toEqual(l)
  })

  // Every row in the library is fed the same pushes as the import queue's
  // rows. A library that is not involved in this run must come back untouched.
  it('leaves an untouched list alone', () => {
    const l = [book('A'), book('B')]
    expect(onLibraryAiEvent(l, 1, { event: 'started' })).toEqual(l)
  })
})

describe('concurrent unbound AI reads', () => {
  it('does not guess that an unknown push belongs to the sole unbound row', () => {
    const list = claimAiRead([book('A')], 'ssot/ebooks/2026-08/A')
    const stashed = stashOrApplyLibraryAi(list, [], 41, {
      event: 'started',
      started_at: 'A-start',
    })
    expect(stashed.list[0]).toMatchObject({ aiStatus: 'queued', aiJobId: undefined })
    const bound = bindAiJob(stashed.list, 'ssot/ebooks/2026-08/A', 41)
    const replayed = replayPendingLibraryAi(bound, stashed.pending, 41)
    expect(replayed.list[0]).toMatchObject({ aiStatus: 'running', aiStartedAt: 'A-start' })
  })

  it('stashes early pushes until each RPC response identifies its row', () => {
    let list = claimAiRead([book('A'), book('B')], 'ssot/ebooks/2026-08/A')
    list = claimAiRead(list, 'ssot/ebooks/2026-08/B')
    let pending: import('./library').PendingLibraryAiEvent[] = []

    let result = stashOrApplyLibraryAi(list, pending, 41, {
      event: 'started',
      started_at: 'A-start',
    })
    list = result.list
    pending = result.pending
    result = stashOrApplyLibraryAi(list, pending, 42, { event: 'failed', error: 'B-fail' })
    pending = result.pending
    expect(pending).toHaveLength(2)
    expect(result.list.every((row) => row.aiStatus === 'queued')).toBe(true)

    list = bindAiJob(result.list, 'ssot/ebooks/2026-08/B', 42)
    let replay = replayPendingLibraryAi(list, pending, 42)
    list = replay.list
    pending = replay.pending
    expect(list[1]).toMatchObject({ aiJobId: 42, aiStatus: 'failed', aiError: 'B-fail' })
    expect(pending.map((item) => item.jobId)).toEqual([41])

    list = bindAiJob(list, 'ssot/ebooks/2026-08/A', 41)
    replay = replayPendingLibraryAi(list, pending, 41)
    expect(replay.list[0]).toMatchObject({
      aiJobId: 41,
      aiStatus: 'running',
      aiStartedAt: 'A-start',
    })
    expect(replay.pending).toEqual([])
  })
})
