import { describe, expect, it } from 'vitest'
import {
  LedgerFormatError,
  newLedger,
  parseLedger,
  serializeLedger,
  upgradeLedgerToV2,
} from './ledger'

const valid = `---
type: Next
version: 1
source_dirs:
  - inbox/ideas
events:
  - at: 2026-08-29T01:00:00Z
    event_id: e1
    idea_id: i1
    action: future-action
    future_field: keep-me
future_top: keep-me-too
---
# Ignored mirror
`

describe('Next event document', () => {
  it('preserves unknown top-level fields and events losslessly', () => {
    const parsed = parseLedger(valid)
    const again = parseLedger(serializeLedger(parsed))
    expect(again.extra).toEqual({ future_top: 'keep-me-too' })
    expect(again.events[0]).toMatchObject({ action: 'future-action', future_field: 'keep-me' })
  })

  it('creates a minimal readable document', () => {
    const markdown = serializeLedger(newLedger(['inbox/ideas', 'inbox/ideas']))
    expect(markdown).toMatch(/^---\ntype: Next\n/)
    expect(markdown).toContain('# Next')
    expect(parseLedger(markdown).source_dirs).toEqual(['inbox/ideas'])
  })

  it.each([
    ['missing frontmatter', '# Next', 'missing_frontmatter'],
    ['wrong type', '---\ntype: Note\nversion: 1\nsource_dirs: []\nevents: []\n---', 'wrong_type'],
    ['new version', '---\ntype: Next\nversion: 3\nsource_dirs: []\nevents: []\n---', 'unsupported_version'],
    ['unsafe source', '---\ntype: Next\nversion: 1\nsource_dirs: [../ideas]\nevents: []\n---', 'invalid_source_dirs'],
    ['bad events', '---\ntype: Next\nversion: 1\nsource_dirs: []\nevents: nope\n---', 'invalid_events'],
  ])('refuses %s instead of falling back to an empty ledger', (_name, markdown, code) => {
    expect(() => parseLedger(markdown)).toThrowError(LedgerFormatError)
    try {
      parseLedger(markdown)
    } catch (error) {
      expect(error).toMatchObject({ code })
    }
  })

  it('keeps unknown event actions visible in the readable mirror', () => {
    const markdown = serializeLedger(parseLedger(valid))
    expect(markdown).toContain('future-action')
    expect(markdown).toContain('`i1`')
  })

  it('keeps project markers and article delivery visible in the readable mirror', () => {
    const markdown = serializeLedger({
      ...newLedger(['inbox/ideas']),
      events: [{
        at: '2026-08-31T00:00:00Z',
        event_id: 'e1',
        idea_id: 'i1',
        action: 'settle',
        project: 'Writing',
        exit: { kind: 'done', delivery: 'article' },
        result: 'writing/article.md',
      }],
    })
    expect(markdown).toContain('- project: Writing')
    expect(markdown).toContain('- outcome: done · delivery: article')
    expect(markdown).toContain('- result: writing/article.md')
  })

  it('shows every confirmed project tag in the readable mirror', () => {
    const markdown = serializeLedger({
      ...newLedger(['inbox/ideas']),
      events: [{
        at: '2026-08-31T12:00:00Z',
        event_id: 'e-projects',
        idea_id: 'i-projects',
        action: 'park',
        source: { path: 'inbox/ideas/a-idea.md' },
        wake_trigger: 'later',
        projects: ['Next', 'Writing'],
        project: 'Next',
      }],
    })
    expect(markdown).toContain('- projects: Next · Writing')
  })

  it('reads and writes v2 task directories without changing raw v1 events', () => {
    const markdown = `---
type: Next
version: 2
source_dirs: [inbox/ideas]
task_dirs: [inbox/tasks]
events:
  - at: 2026-08-29T01:00:00Z
    event_id: legacy
    idea_id: legacy-idea
    action: future-action
    future_field: keep-me
  - at: 2026-09-01T01:00:00Z
    event_id: task
    item_id: task-1
    item_kind: task
    action: park
    source: { path: inbox/tasks/a-task.md }
    wake_trigger: later
future_top: keep-me-too
---`
    const parsed = parseLedger(markdown)
    expect(parsed).toMatchObject({ version: 2, task_dirs: ['inbox/tasks'] })
    const again = parseLedger(serializeLedger(parsed))
    expect(again.events).toEqual(parsed.events)
    expect(again.events[0]).not.toHaveProperty('item_id')
    expect(again.extra).toEqual({ future_top: 'keep-me-too' })
  })

  it('upgrades on demand without mutating the v1 document or rewriting its events', () => {
    const original = parseLedger(valid)
    const rawEvent = original.events[0]
    const upgraded = upgradeLedgerToV2(original, ['inbox/tasks', 'inbox/tasks'])

    expect(original.version).toBe(1)
    expect(original).not.toHaveProperty('task_dirs')
    expect(upgraded).toMatchObject({ version: 2, task_dirs: ['inbox/tasks'] })
    expect(upgraded.events[0]).toBe(rawEvent)
    expect(upgraded.source_dirs).toEqual(['inbox/ideas'])
    expect(upgraded.extra).toEqual({ future_top: 'keep-me-too' })
  })

  it('does not introduce v2 fields while serializing a v1 ledger', () => {
    const markdown = serializeLedger(newLedger(['inbox/ideas']))
    expect(markdown).toContain('version: 1')
    expect(markdown).not.toContain('task_dirs:')
  })
})
