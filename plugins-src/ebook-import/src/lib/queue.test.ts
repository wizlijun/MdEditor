import { describe, it, expect } from 'vitest'
import { addPaths, nextToStart, onJobEvent, type Queue } from './queue'

const empty: Queue = { items: [], activeId: null }

describe('addPaths', () => {
  it('accepts epub/pdf/docx and filters everything else', () => {
    const q = addPaths(empty, [
      '/books/alice.epub',
      '/books/notes.txt',
      '/books/report.PDF',
      '/books/thesis.docx',
      '/books/image.png',
    ])
    expect(q.items.map((i) => i.path)).toEqual([
      '/books/alice.epub',
      '/books/report.PDF',
      '/books/thesis.docx',
    ])
  })

  it('derives a display name from the path', () => {
    const q = addPaths(empty, ['/a/b/c/alice.epub'])
    expect(q.items[0].name).toBe('alice.epub')
  })

  it('starts every new item pending with empty logs', () => {
    const q = addPaths(empty, ['/a.epub'])
    expect(q.items[0]).toMatchObject({ status: 'pending', logs: [] })
  })

  it('dedups the same path when an existing item is pending or running', () => {
    const q1 = addPaths(empty, ['/a.epub'])
    const q2 = addPaths(q1, ['/a.epub'])
    expect(q2.items).toHaveLength(1)

    const running: Queue = {
      items: [{ id: 1, path: '/b.epub', name: 'b.epub', status: 'running', logs: [] }],
      activeId: 1,
    }
    const q3 = addPaths(running, ['/b.epub'])
    expect(q3.items).toHaveLength(1)
  })

  it('allows re-adding a path whose previous item already finished', () => {
    const done: Queue = {
      items: [{ id: 1, path: '/a.epub', name: 'a.epub', status: 'done', logs: [], destRel: 'x' }],
      activeId: null,
    }
    const q = addPaths(done, ['/a.epub'])
    expect(q.items).toHaveLength(2)
    expect(q.items[1].status).toBe('pending')
  })

  it('assigns increasing ids across multiple calls', () => {
    const q1 = addPaths(empty, ['/a.epub'])
    const q2 = addPaths(q1, ['/b.epub'])
    expect(q2.items[0].id).not.toBe(q2.items[1].id)
  })

  it('does nothing for an empty path list', () => {
    const q = addPaths(empty, [])
    expect(q).toEqual(empty)
  })
})

describe('nextToStart', () => {
  it('returns the first pending item when nothing is active', () => {
    const q = addPaths(empty, ['/a.epub', '/b.pdf'])
    const n = nextToStart(q)
    expect(n?.path).toBe('/a.epub')
  })

  it('returns null while an item is active', () => {
    const q = addPaths(empty, ['/a.epub'])
    const active: Queue = { ...q, activeId: q.items[0].id }
    expect(nextToStart(active)).toBeNull()
  })

  it('returns null when there is nothing pending', () => {
    const q: Queue = {
      items: [{ id: 1, path: '/a.epub', name: 'a.epub', status: 'done', logs: [] }],
      activeId: null,
    }
    expect(nextToStart(q)).toBeNull()
  })

  it('finds the next pending item once the active one clears', () => {
    let q = addPaths(empty, ['/a.epub', '/b.epub'])
    const first = q.items[0]
    q = {
      ...q,
      activeId: first.id,
      items: q.items.map((i) => (i.id === first.id ? { ...i, status: 'running', jobId: 42 } : i)),
    }
    expect(nextToStart(q)).toBeNull()

    q = onJobEvent(q, 42, { event: 'done', dest_rel: 'ssot/ebooks/2026-08/a/' })
    expect(q.activeId).toBeNull()
    const n = nextToStart(q)
    expect(n?.path).toBe('/b.epub')
  })
})

describe('onJobEvent', () => {
  function running(): Queue {
    return { items: [{ id: 1, path: '/a.epub', name: 'a.epub', status: 'running', jobId: 7, logs: [] }], activeId: 1 }
  }

  it('appends log lines', () => {
    let q = running()
    q = onJobEvent(q, 7, { event: 'log', line: 'converting…' })
    q = onJobEvent(q, 7, { event: 'log', line: 'done stage 1' })
    expect(q.items[0].logs).toEqual(['converting…', 'done stage 1'])
    expect(q.items[0].status).toBe('running')
  })

  it('records progress (stage/page/total)', () => {
    let q = running()
    q = onJobEvent(q, 7, { event: 'progress', stage: 'ocr', page: 3, total: 10 })
    expect(q.items[0]).toMatchObject({ stage: 'ocr', page: 3, total: 10, status: 'running' })
  })

  it('marks done, writes destRel, and clears activeId', () => {
    let q = running()
    q = onJobEvent(q, 7, { event: 'done', dest_rel: 'ssot/ebooks/2026-08/a/' })
    expect(q.items[0]).toMatchObject({ status: 'done', destRel: 'ssot/ebooks/2026-08/a/' })
    expect(q.activeId).toBeNull()
  })

  it('marks failed, writes the error, and clears activeId', () => {
    let q = running()
    q = onJobEvent(q, 7, { event: 'failed', error: 'calibre not found' })
    expect(q.items[0]).toMatchObject({ status: 'failed', error: 'calibre not found' })
    expect(q.items[0].cancelled).toBeFalsy()
    expect(q.activeId).toBeNull()
  })

  // Design decision: a user-cancelled job arrives as event:"failed",
  // error:"cancelled" (see backend import_cancel). We surface this as
  // status:"failed" + cancelled:true rather than a distinct ItemStatus, so
  // the ItemStatus union stays exactly {pending,running,done,failed} per
  // the task-9 brief, while the UI can still render a neutral "cancelled"
  // badge instead of an error by checking item.cancelled first.
  it('maps error:"cancelled" to a distinguishable cancelled state, not an error', () => {
    let q = running()
    q = onJobEvent(q, 7, { event: 'failed', error: 'cancelled' })
    expect(q.items[0].status).toBe('failed')
    expect(q.items[0].cancelled).toBe(true)
    expect(q.items[0].error).toBeUndefined()
    expect(q.activeId).toBeNull()
  })

  it('ignores events for an unknown job id', () => {
    const q = running()
    const q2 = onJobEvent(q, 999, { event: 'log', line: 'nope' })
    expect(q2).toEqual(q)
  })
})
