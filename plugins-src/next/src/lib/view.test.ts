import { describe, expect, it } from 'vitest'
import { isDormantDue, placedItems, previewPosition } from './view'
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
