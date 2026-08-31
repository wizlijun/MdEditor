import { describe, expect, it } from 'vitest'
import { reduceEvents, sourceKey, validateAppend, validateEvent } from './domain'

const at = '2026-08-29T14:10:00Z'

function source(path: string, created?: string) {
  return { path, ...(created ? { created } : {}) }
}

function commit(eventId: string, ideaId: string, path?: string) {
  return {
    at,
    event_id: eventId,
    idea_id: ideaId,
    action: 'commit',
    ...(path ? { source: source(path) } : {}),
    commitment: `验证 ${ideaId}`,
    next_action: '检查证据',
    close_condition: '得到结论',
  }
}

describe('validateEvent', () => {
  it('accepts a first disposition whose legacy source has no created', () => {
    const result = validateEvent(commit('e1', 'i1', 'inbox/ideas/a-idea.md'))
    expect(result).toMatchObject({ ok: true, known: true })
  })

  it('validates action-specific required fields and review_at', () => {
    const result = validateEvent({
      at,
      event_id: 'e1',
      idea_id: 'i1',
      action: 'wait',
      source: source('inbox/ideas/a-idea.md'),
      waiting_for: '',
      review_at: 'not-a-date',
    })
    expect(result.ok).toBe(false)
    if (result.ok) return
    expect(result.errors.map((error) => error.field)).toEqual(['waiting_for', 'review_at'])
  })

  it('requires an auditable timestamp and rejects impossible calendar dates', () => {
    expect(validateEvent({ ...commit('e1', 'i1', 'a-idea.md'), at: '2026-08-29' }).ok).toBe(false)
    expect(validateEvent({
      at,
      event_id: 'e2',
      idea_id: 'i2',
      action: 'wait',
      source: source('b-idea.md'),
      waiting_for: '结果',
      review_at: '2026-02-30',
    }).ok).toBe(false)
  })

  it.each([
    [{ kind: 'done' }, {}, true],
    [{ kind: 'done', via: 'delegate' }, { result: 'vault/delivery.md' }, true],
    [{ kind: 'done', delivery: 'article' }, {}, true],
    [{ kind: 'done', delivery: 'article' }, { result: 'writing/article.md' }, true],
    [{ kind: 'done', via: 'delegate', delivery: 'article' }, { result: 'writing/article.md' }, true],
    [{ kind: 'done', delivery: 'email' }, {}, true],
    [{ kind: 'stopped', via: 'ignore' }, {}, true],
    [{ kind: 'stopped', via: 'drop' }, {}, false],
    [{ kind: 'stopped', via: 'disproved' }, { reason: '关键假设不成立' }, true],
    [{ kind: 'transferred', via: 'project' }, {}, false],
    [{ kind: 'transferred', via: 'project' }, { target: 'projects/next.md' }, true],
    [{ kind: 'transferred', via: 'publish' }, { target: 'https://example.com/legacy' }, true],
    [{ kind: 'compressed', via: 'automate' }, { target: 'scripts/next.ts' }, true],
  ])('reads the v1 settlement matrix without claiming unknown extensions', (exit, extra, expected) => {
    const result = validateEvent({
      at,
      event_id: 'e1',
      idea_id: 'i1',
      action: 'settle',
      source: source('inbox/ideas/a-idea.md'),
      exit,
      ...extra,
    })
    expect(result.ok).toBe(expected)
  })

  it('reads project assignments and preserves previously unknown project shapes', () => {
    expect(validateEvent({ ...commit('e1', 'i1', 'a-idea.md'), project: 'Next' }).ok).toBe(true)
    expect(validateEvent({ ...commit('e2', 'i2', 'b-idea.md'), project: null }).ok).toBe(true)
    expect(validateEvent({ ...commit('e3', 'i3', 'c-idea.md'), project: { legacy: true } }).ok).toBe(true)
  })

  it('accepts an unknown action as an unsupported envelope and preserves extra fields', () => {
    const raw = {
      at,
      event_id: 'e1',
      idea_id: 'i1',
      action: 'future-action',
      source: source('inbox/ideas/a-idea.md'),
      future_payload: { answer: 42 },
    }
    const result = validateEvent(raw)
    expect(result).toMatchObject({ ok: true, known: false })
    if (!result.ok || result.known) return
    expect(result.event.future_payload).toEqual({ answer: 42 })
  })
})

describe('reduceEvents', () => {
  it('inherits, changes, and explicitly clears a project marker across placements', () => {
    const result = reduceEvents([
      { ...commit('e1', 'idea-1', 'inbox/ideas/a-idea.md'), project: 'Next' },
      { at, event_id: 'e2', idea_id: 'idea-1', action: 'wait', waiting_for: 'review', review_at: '2026-09-02' },
      { at, event_id: 'e3', idea_id: 'idea-1', action: 'park', wake_trigger: 'later', project: 'Writing' },
      { at, event_id: 'e4', idea_id: 'idea-1', action: 'reopen' },
      { ...commit('e5', 'idea-1'), project: null },
    ])

    expect(result.hasBlockingIssues).toBe(false)
    expect(result.ideas.get('idea-1')).not.toHaveProperty('project')
  })

  it('keeps a project marker when a later event omits the field', () => {
    const result = reduceEvents([
      { ...commit('e1', 'idea-1', 'inbox/ideas/a-idea.md'), project: 'Next' },
      { at, event_id: 'e2', idea_id: 'idea-1', action: 'wait', waiting_for: 'review', review_at: '2026-09-02' },
    ])
    expect(result.ideas.get('idea-1')).toMatchObject({ state: 'waiting', project: 'Next' })
  })

  it('clears a project marker on relink and ignores an unowned legacy project shape', () => {
    const cleared = reduceEvents([
      { ...commit('e1', 'idea-1', 'inbox/ideas/a-idea.md'), project: 'Next' },
      {
        at,
        event_id: 'e2',
        idea_id: 'idea-1',
        action: 'relink',
        source: source('archive/a-idea.md'),
        project: null,
      },
    ])
    expect(cleared.hasBlockingIssues).toBe(false)
    expect(cleared.ideas.get('idea-1')).not.toHaveProperty('project')

    const legacy = reduceEvents([{ ...commit('e3', 'idea-2', 'inbox/ideas/b-idea.md'), project: { legacy: true } }])
    expect(legacy.hasBlockingIssues).toBe(false)
    expect(legacy.ideas.get('idea-2')).not.toHaveProperty('project')
  })

  it('reduces commit → wait → commit → park → reopen without changing idea_id', () => {
    const events = [
      commit('e1', 'idea-1', 'inbox/ideas/a-idea.md'),
      { at, event_id: 'e2', idea_id: 'idea-1', action: 'wait', waiting_for: '设计稿', review_at: '2026-09-02' },
      { ...commit('e3', 'idea-1'), next_action: '验收设计稿' },
      { at, event_id: 'e4', idea_id: 'idea-1', action: 'park', wake_trigger: '再次收到同类请求' },
      { at, event_id: 'e5', idea_id: 'idea-1', action: 'reopen' },
    ]
    const result = reduceEvents(events)
    expect(result.hasBlockingIssues).toBe(false)
    expect(result.ideas).toHaveLength(1)
    expect(result.ideas.get('idea-1')).toMatchObject({
      idea_id: 'idea-1',
      state: 'capture',
      last_event_id: 'e5',
      source: { path: 'inbox/ideas/a-idea.md' },
    })
  })

  it('requires reopen before a closed idea can become WIP again', () => {
    const result = reduceEvents([
      {
        at,
        event_id: 'e1',
        idea_id: 'idea-1',
        action: 'settle',
        source: source('inbox/ideas/a-idea.md'),
        exit: { kind: 'stopped', via: 'ignore' },
      },
      commit('e2', 'idea-1'),
    ])
    expect(result.issues).toContainEqual(expect.objectContaining({ code: 'invalid_transition', event_id: 'e2' }))
    expect(result.ideas.get('idea-1')?.state).toBe('unsupported')
  })

  it.each([
    [
      { at, event_id: 'e1', idea_id: 'idea-1', action: 'park', source: source('inbox/ideas/a-idea.md'), wake_trigger: 'later' },
      { at, event_id: 'e2', idea_id: 'idea-1', action: 'park', wake_trigger: 'even later' },
    ],
    [
      { at, event_id: 'e1', idea_id: 'idea-1', action: 'settle', source: source('inbox/ideas/a-idea.md'), exit: { kind: 'done' } },
      { at, event_id: 'e2', idea_id: 'idea-1', action: 'settle', exit: { kind: 'stopped', via: 'ignore' } },
    ],
  ])('requires reopen before replacing a dormant or closed disposition', (first, second) => {
    const result = reduceEvents([first, second])
    expect(result.issues).toContainEqual(expect.objectContaining({ code: 'invalid_transition', event_id: 'e2' }))
  })

  it('keeps idea state through an explicit relink and retains historical source claims', () => {
    const oldSource = source('inbox/ideas/a-idea.md', '2026-08-01T00:00:00Z')
    const newSource = source('archive/a-idea.md', '2026-08-01T00:00:00Z')
    const result = reduceEvents([
      { ...commit('e1', 'idea-1'), source: oldSource },
      { at, event_id: 'e2', idea_id: 'idea-1', action: 'relink', source: newSource },
    ])
    expect(result.ideas.get('idea-1')).toMatchObject({ state: 'wip', source: newSource, last_event_id: 'e2' })
    expect(result.sourceToIdeaId.get(sourceKey(oldSource))).toBe('idea-1')
    expect(result.sourceToIdeaId.get(sourceKey(newSource))).toBe('idea-1')
  })

  it('rejects an implicit path change instead of guessing a relink', () => {
    const result = reduceEvents([
      commit('e1', 'idea-1', 'inbox/ideas/a-idea.md'),
      { ...commit('e2', 'idea-1'), source: source('archive/a-idea.md') },
    ])
    expect(result.issues).toContainEqual(expect.objectContaining({ code: 'source_mismatch' }))
    expect(result.ideas.get('idea-1')?.state).toBe('unsupported')
  })

  it('treats a byte-for-byte semantic duplicate event_id as idempotent', () => {
    const first = commit('same-id', 'idea-1', 'inbox/ideas/a-idea.md')
    // Object key order is storage noise, not a payload difference.
    const duplicate = {
      close_condition: first.close_condition,
      next_action: first.next_action,
      commitment: first.commitment,
      source: first.source,
      action: first.action,
      idea_id: first.idea_id,
      event_id: first.event_id,
      at: first.at,
    }
    const result = reduceEvents([first, duplicate])
    expect(result.idempotentEventIds).toEqual(['same-id'])
    expect(result.hasBlockingIssues).toBe(false)
    expect(result.eventById).toHaveLength(1)
  })

  it('marks both ideas unsupported when one event_id has conflicting payloads', () => {
    const result = reduceEvents([
      commit('same-id', 'idea-1', 'inbox/ideas/a-idea.md'),
      commit('same-id', 'idea-2', 'inbox/ideas/b-idea.md'),
    ])
    expect(result.issues).toContainEqual(expect.objectContaining({
      code: 'event_id_conflict',
      related_idea_ids: ['idea-1', 'idea-2'],
    }))
    expect(result.ideas.get('idea-1')?.state).toBe('unsupported')
    expect(result.ideas.get('idea-2')?.state).toBe('unsupported')
    expect(result.hasBlockingIssues).toBe(true)
  })

  it('blocks one source identity from being claimed by two idea_ids', () => {
    const shared = source('inbox/ideas/a-idea.md', '2026-08-01T00:00:00Z')
    const result = reduceEvents([
      { ...commit('e1', 'idea-1'), source: shared },
      { ...commit('e2', 'idea-2'), source: shared },
    ])
    expect(result.issues).toContainEqual(expect.objectContaining({ code: 'source_identity_conflict' }))
    expect(result.ideas.get('idea-1')?.state).toBe('unsupported')
    expect(result.ideas.get('idea-2')?.state).toBe('unsupported')
  })

  it('allows a reused path when distinct created markers prove they are distinct sources', () => {
    const path = 'inbox/ideas/a-idea.md'
    const result = reduceEvents([
      { ...commit('e1', 'idea-1'), source: source(path, '2026-08-01T00:00:00Z') },
      { ...commit('e2', 'idea-2'), source: source(path, '2026-08-02T00:00:00Z') },
    ])
    expect(result.hasBlockingIssues).toBe(false)
    expect(result.wipCount).toBe(2)
  })

  it('treats a same-path claim as ambiguous when either source lacks created', () => {
    const path = 'inbox/ideas/a-idea.md'
    const result = reduceEvents([
      { ...commit('e1', 'idea-1'), source: source(path) },
      { ...commit('e2', 'idea-2'), source: source(path, '2026-08-02T00:00:00Z') },
    ])
    expect(result.issues).toContainEqual(expect.objectContaining({ code: 'source_identity_conflict' }))
    expect(result.hasBlockingIssues).toBe(true)
  })

  it('marks an unknown action unsupported and never lets later known events fake understanding', () => {
    const future = {
      at,
      event_id: 'e1',
      idea_id: 'idea-1',
      action: 'incubate',
      source: source('inbox/ideas/a-idea.md'),
      opaque: { keep: true },
    }
    const result = reduceEvents([future, commit('e2', 'idea-1')])
    expect(result.ideas.get('idea-1')).toMatchObject({
      state: 'unsupported',
      unsupported_actions: ['incubate'],
    })
    expect(result.eventById.get('e1')).toEqual(future)
    expect(result.issues.map((issue) => issue.code)).toEqual(['unsupported_action', 'idea_unsupported'])
  })

  it('does not require created, but does require source.path on the first disposition', () => {
    const result = reduceEvents([commit('e1', 'idea-1')])
    expect(result.issues).toContainEqual(expect.objectContaining({ code: 'missing_initial_source' }))
    expect(result.hasBlockingIssues).toBe(true)
  })

  it('keeps all externally-created WIP visible and reports the soft 3-slot boundary', () => {
    const result = reduceEvents([
      commit('e1', 'i1', 'inbox/ideas/1-idea.md'),
      commit('e2', 'i2', 'inbox/ideas/2-idea.md'),
      commit('e3', 'i3', 'inbox/ideas/3-idea.md'),
      commit('e4', 'i4', 'inbox/ideas/4-idea.md'),
    ])
    expect(result.ideas).toHaveLength(4)
    expect(result.wipCount).toBe(4)
    expect(result.wipLimit).toBe(3)
    expect(result.wipAtLimit).toBe(true)
    expect(result.wipExceeded).toBe(true)
    expect(result.hasBlockingIssues).toBe(false)
  })
})

describe('validateAppend', () => {
  it('keeps exact replay idempotent for a readable historical extension shape', () => {
    const historical = {
      ...commit('historical', 'historical-idea', 'inbox/ideas/historical-idea.md'),
      project: { legacy: true },
    }
    expect(validateAppend(reduceEvents([historical]), historical)).toMatchObject({
      ok: true,
      idempotent: true,
    })
  })

  it.each([
    [{ ...commit('writer-project', 'writer-1', 'inbox/ideas/writer-1-idea.md'), project: '   ' }, 'project'],
    [{
      at,
      event_id: 'writer-article',
      idea_id: 'writer-2',
      action: 'settle',
      source: source('inbox/ideas/writer-2-idea.md'),
      exit: { kind: 'done', delivery: 'article' },
    }, 'result'],
    [{
      at,
      event_id: 'writer-delivery',
      idea_id: 'writer-3',
      action: 'settle',
      source: source('inbox/ideas/writer-3-idea.md'),
      exit: { kind: 'done', delivery: 'email' },
    }, 'exit.delivery'],
  ])('strictly validates extensions emitted by this writer', (event, field) => {
    expect(validateAppend(reduceEvents([]), event)).toEqual({
      ok: false,
      issues: [expect.objectContaining({ code: 'invalid_event', fields: expect.arrayContaining([field]) })],
    })
  })

  const threeWip = reduceEvents([
    commit('e1', 'i1', 'inbox/ideas/1-idea.md'),
    commit('e2', 'i2', 'inbox/ideas/2-idea.md'),
    commit('e3', 'i3', 'inbox/ideas/3-idea.md'),
  ])

  it('allows a fourth commitment in soft mode and rejects it in optional hard mode', () => {
    const fourth = commit('e4', 'i4', 'inbox/ideas/4-idea.md')
    expect(validateAppend(threeWip, fourth)).toMatchObject({ ok: true, idempotent: false })
    expect(validateAppend(threeWip, fourth, { hardLimit: true })).toEqual({
      ok: false,
      issues: [expect.objectContaining({ code: 'wip_limit_exceeded' })],
    })
  })

  it('allows an idempotent replay and rejects a conflicting event_id', () => {
    const exact = commit('e1', 'i1', 'inbox/ideas/1-idea.md')
    expect(validateAppend(threeWip, exact)).toMatchObject({ ok: true, idempotent: true })
    expect(validateAppend(threeWip, { ...exact, commitment: '不同内容' })).toEqual({
      ok: false,
      issues: [expect.objectContaining({ code: 'event_id_conflict' })],
    })
  })

  it('allows an exact replay even when another event has made the ledger read-only', () => {
    const exact = commit('e1', 'i1', 'inbox/ideas/1-idea.md')
    const projection = reduceEvents([
      exact,
      { ...commit('broken', 'i2', 'inbox/ideas/2-idea.md'), next_action: '' },
    ])
    expect(projection.hasBlockingIssues).toBe(true)
    expect(validateAppend(projection, exact)).toMatchObject({ ok: true, idempotent: true })
  })

  it('rejects an ambiguous source without created when the path belongs to multiple historical identities', () => {
    const path = 'inbox/ideas/shared-idea.md'
    const projection = reduceEvents([
      { ...commit('e1', 'i1'), source: source(path, '2026-08-01T00:00:00Z') },
      { ...commit('e2', 'i2'), source: source(path, '2026-08-02T00:00:00Z') },
    ])
    const result = validateAppend(projection, {
      at,
      event_id: 'e3',
      idea_id: 'i1',
      action: 'relink',
      source: source(path),
    })
    expect(result).toEqual({
      ok: false,
      issues: [expect.objectContaining({
        code: 'source_identity_conflict',
        related_idea_ids: ['i2', 'i1'],
      })],
    })
  })

  it('allows a hard-limit update to an existing WIP because it does not consume a new slot', () => {
    const update = { ...commit('e4', 'i1'), next_action: '更新下一步' }
    expect(validateAppend(threeWip, update, { hardLimit: true })).toMatchObject({
      ok: true,
      idempotent: false,
    })
  })

  it('blocks hard-mode commits while any unknown action makes capacity uncertain', () => {
    const projection = reduceEvents([{
      at,
      event_id: 'future-1',
      idea_id: 'future-idea',
      action: 'incubate',
      source: source('inbox/ideas/future-idea.md'),
    }])
    const result = validateAppend(
      projection,
      commit('e2', 'idea-2', 'inbox/ideas/2-idea.md'),
      { hardLimit: true },
    )
    expect(result).toEqual({
      ok: false,
      issues: [expect.objectContaining({ code: 'wip_limit_uncertain' })],
    })
  })
})
