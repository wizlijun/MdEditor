import { describe, expect, it } from 'vitest'
import { placeEvent, relinkEvent, reopenEvent, type EventFactory } from './events'
import type { WorkspaceItem } from './repository'

const ids = ['event-1', 'idea-1']
const factory: EventFactory = {
  now: () => '2026-08-29T01:00:00Z',
  id: () => ids.shift()!,
}

const capture: WorkspaceItem = {
  key: 'inbox/ideas/a-idea.md',
  state: 'capture',
  title: 'Idea',
  path: 'inbox/ideas/a-idea.md',
  created: '2026-08-29T00:00:00Z',
  proofed: false,
  orphan: false,
  relinkCandidates: [],
  relinkMatch: null,
}

describe('event construction', () => {
  it('allocates a stable idea id on the first human placement', () => {
    const event = placeEvent(capture, {
      route: 'commit',
      commitment: ' Verify it ',
      next_action: ' Test once ',
      close_condition: ' Evidence ',
    }, factory)
    expect(event).toMatchObject({
      event_id: 'event-1',
      idea_id: 'idea-1',
      action: 'commit',
      source: { path: capture.path, created: capture.created },
      commitment: 'Verify it',
      next_action: 'Test once',
    })
  })

  it('keeps an existing idea id for reopen and explicit relink', () => {
    const placed = { ...capture, idea_id: 'existing' }
    const sequential: EventFactory = { now: factory.now, id: () => 'new-event' }
    expect(reopenEvent(placed, sequential)).toMatchObject({ idea_id: 'existing', action: 'reopen' })
    expect(relinkEvent(placed, {
      path: 'archive/a-idea.md',
      created: capture.created,
      title: 'Idea',
      body: '# Idea',
      proofed: false,
    }, sequential)).toMatchObject({
      idea_id: 'existing',
      action: 'relink',
      source: { path: 'archive/a-idea.md', created: capture.created },
    })
  })

  it('normalizes multi-project tags, dual-writes the legacy primary, and keeps transfer target explicit', () => {
    const placed: WorkspaceItem = {
      ...capture,
      idea_id: 'existing',
      projection: {
        idea_id: 'existing',
        state: 'wip',
        last_event_id: 'old',
        last_at: '2026-08-29T00:00:00Z',
        source: { path: capture.path!, created: capture.created },
        projects: ['Old project'],
        project: 'Old project',
        commitment: 'Test',
        next_action: 'Run',
        close_condition: 'Know',
      },
    }
    const sequential: EventFactory = { now: factory.now, id: () => 'new-event' }

    expect(placeEvent(placed, {
      route: 'wait',
      waiting_for: 'Review',
      review_at: '2026-09-02',
      projects: null,
    }, sequential)).toMatchObject({ action: 'wait', projects: null, project: null })

    expect(placeEvent(placed, {
      route: 'settle',
      exit: { kind: 'transferred', via: 'project' },
      projects: [' Writing ', 'Next', 'writing'],
      target: 'Next',
    }, sequential)).toMatchObject({
      action: 'settle',
      project: 'Writing',
      projects: ['Writing', 'Next'],
      target: 'Next',
    })
  })

  it('constructs a published article as a completed delivery', () => {
    const sequential: EventFactory = { now: factory.now, id: () => 'new-event' }
    expect(placeEvent(capture, {
      route: 'settle',
      exit: { kind: 'done', delivery: 'article' },
      result: ' writing/article.md ',
    }, sequential)).toMatchObject({
      action: 'settle',
      exit: { kind: 'done', delivery: 'article' },
      result: 'writing/article.md',
    })
  })
})
