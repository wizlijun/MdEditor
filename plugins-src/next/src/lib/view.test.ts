import { describe, expect, it } from 'vitest'
import { isDormantDue, placedItems, previewPosition, sortWorkspaceItems } from './view'
import type { WorkspaceItem } from './repository'

const dormant = (trigger: string): WorkspaceItem => ({
  key: trigger,
  idea_id: trigger,
  state: 'dormant',
  title: `Idea ${trigger}`,
  path: `${trigger}-idea.md`,
  proofed: false,
  orphan: false,
  relinkCandidates: [],
  relinkMatch: null,
  projection: {
    idea_id: trigger,
    state: 'dormant',
    last_event_id: 'e1',
    last_at: '2026-08-01T00:00:00Z',
    wake_trigger: trigger,
  },
})

describe('Next view projection', () => {
  it('sorts by priority, due urgency, and newest creation by default', () => {
    const items = [
      { ...dormant('p1-later'), priority: 'P1' as const, due: '2026-09-10', created: '2026-09-04T00:00:00Z' },
      { ...dormant('p0'), priority: 'P0' as const, due: '2026-09-20', created: '2026-09-01T00:00:00Z' },
      { ...dormant('p1-sooner-old'), priority: 'P1' as const, due: '2026-09-05', created: '2026-09-02T00:00:00Z' },
      { ...dormant('p1-sooner-new'), priority: 'P1' as const, due: '2026-09-05', created: '2026-09-03T00:00:00Z' },
      { ...dormant('p2'), priority: 'P2' as const, due: '2026-09-01', created: '2026-09-05T00:00:00Z' },
    ]

    expect(sortWorkspaceItems(items, 'priority').map((item) => item.key)).toEqual([
      'p0', 'p1-sooner-new', 'p1-sooner-old', 'p1-later', 'p2',
    ])
    expect(items.map((item) => item.key)).toEqual([
      'p1-later', 'p0', 'p1-sooner-old', 'p1-sooner-new', 'p2',
    ])
  })

  it('supports due-first and newest-first modes with deterministic fallbacks', () => {
    const items = [
      { ...dormant('no-due'), priority: 'P0' as const, created: '2026-09-05T00:00:00Z' },
      { ...dormant('invalid'), priority: 'P0' as const, due: 'not-a-date', created: 'not-a-date' },
      { ...dormant('due-p2'), priority: 'P2' as const, due: '2026-09-02', created: '2026-09-02T00:00:00Z' },
      { ...dormant('due-p1-old'), priority: 'P1' as const, due: '2026-09-02', created: '2026-09-01T00:00:00Z' },
      { ...dormant('newest'), priority: 'P3' as const, due: '2026-09-20', created: '2026-09-10T00:00:00Z' },
      { ...dormant('event-newer'), priority: 'P3' as const, due: '2026-09-20', created: undefined, projection: { ...dormant('event-newer').projection!, last_at: '2026-09-11T00:00:00Z' } },
    ]

    expect(sortWorkspaceItems(items, 'due').map((item) => item.key)).toEqual([
      'due-p1-old', 'due-p2', 'event-newer', 'newest', 'no-due', 'invalid',
    ])
    expect(sortWorkspaceItems(items, 'created').map((item) => item.key)).toEqual([
      'event-newer', 'newest', 'no-due', 'due-p2', 'due-p1-old', 'invalid',
    ])
  })

  it('uses the card key as a stable final order', () => {
    const items = ['z', 'a', 'm'].map((key) => ({
      ...dormant(key), priority: 'P2' as const, due: '2026-09-01', created: '2026-09-01T00:00:00Z',
    }))
    expect(sortWorkspaceItems(items, 'priority').map((item) => item.key)).toEqual(['a', 'm', 'z'])
  })

  it('resurfaces only explicit dates that have arrived', () => {
    const today = new Date('2026-08-29T12:00:00')
    expect(isDormantDue(dormant('2026-08-28'), today)).toBe(true)
    expect(isDormantDue(dormant('2026-08-29'), today)).toBe(true)
    expect(isDormantDue(dormant('2026-08-30'), today)).toBe(false)
    expect(isDormantDue(dormant('when a customer asks again'), today)).toBe(false)
  })

  it('searches placed states without turning capture into a backlog view', () => {
    const closed: WorkspaceItem = {
      ...dormant('2026-08-28'),
      state: 'closed',
      title: 'Buy existing tool',
      projection: {
        idea_id: 'closed',
        state: 'closed',
        last_event_id: 'e2',
        last_at: '2026-08-02T00:00:00Z',
        exit: { kind: 'transferred', via: 'buy' },
        target: 'A product',
      },
    }
    const capture = { ...dormant('capture'), state: 'capture' as const, projection: undefined }
    const orphanCapture = { ...capture, orphan: true }
    expect(placedItems([closed, capture], 'product')).toEqual([closed])
    expect(placedItems([capture], '')).toEqual([])
    expect(placedItems([orphanCapture], '')).toEqual([orphanCapture])
  })

  it('positions a short preview beside its card using the rendered tip size', () => {
    expect(previewPosition(
      { left: 100, right: 348, top: 420 },
      { width: 380, height: 80 },
      { width: 760, height: 520 },
    )).toEqual({ x: 358, y: 420 })

    expect(previewPosition(
      { left: 600, right: 720, top: 100 },
      { width: 380, height: 160 },
      { width: 760, height: 520 },
    )).toEqual({ x: 210, y: 100 })
  })

  it('keeps the preview inside a narrow viewport when neither side can fit', () => {
    const position = previewPosition(
      { left: 100, right: 200, top: 490 },
      { width: 276, height: 300 },
      { width: 300, height: 520 },
    )
    expect(position).toEqual({ x: 12, y: 208 })
  })
})
