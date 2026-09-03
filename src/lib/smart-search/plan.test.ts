import { describe, expect, it } from 'vitest'
import { buildSearchPlanPrompt, shouldTune } from './plan'

describe('smart-search planning', () => {
  it('pins time context and asks for JSON instead of an answer', () => {
    const prompt = buildSearchPlanPrompt({
      mode: 'plan', question: '找上个月的发布决定',
      referenceTime: '2026-09-03T09:00:00+08:00', timezone: 'Asia/Taipei',
      locale: 'zh-CN', lockedFilters: { origins: ['human'] },
    })
    expect(prompt).toContain('MODE: plan')
    expect(prompt).toContain('REFERENCE_TIME: 2026-09-03T09:00:00+08:00')
    expect(prompt).toContain('document_date|content_date|activity_time|ambiguous')
    expect(prompt).toContain('TIME: use null')
    expect(prompt).toContain('LOCKED_FILTERS_JSON: {"origins":["human"]}')
    expect(prompt).toContain('{"kind":"calendar_month","offset":-1}')
    expect(prompt).toContain('Do not use Markdown fences and do not answer')
  })

  it('tunes from telemetry without receiving Vault snippets', () => {
    const prompt = buildSearchPlanPrompt({
      mode: 'tune', question: 'release risks', referenceTime: '2026-09-03T00:00:00Z',
      timezone: 'UTC', locale: 'en', lockedFilters: {}, previousPlan: '{"schemaVersion":1}',
      telemetry: { total: 0, distinctDocuments: 0, truncated: false, subqueries: [] },
    })
    expect(prompt).toContain('PREVIOUS_SEARCH_PLAN_JSON')
    expect(prompt).toContain('RESOLVED_IMMUTABLE_PLAN_JSON')
    expect(prompt).toContain('RETRIEVAL_TELEMETRY_JSON')
    expect(prompt).not.toContain('SEARCH SOURCES')
  })

  it('tunes only empty or completely searched low-coverage results', () => {
    expect(shouldTune({ total: 0, distinctDocuments: 0, truncated: false, subqueries: [] }))
      .toBe(false)
    expect(shouldTune({
      total: 2,
      distinctDocuments: 2,
      truncated: false,
      subqueries: [{ id: 'q1', purpose: 'recall', hitCount: 2, executed: true, truncated: false }],
    })).toBe(true)
    expect(shouldTune({
      total: 2,
      distinctDocuments: 2,
      truncated: true,
      subqueries: [{ id: 'q1', purpose: 'recall', hitCount: 2, executed: true, truncated: true }],
    })).toBe(false)
    expect(shouldTune({
      total: 0,
      distinctDocuments: 0,
      truncated: false,
      subqueries: [{ id: 'q1', purpose: 'recall', hitCount: 0, executed: false, truncated: false }],
    })).toBe(false)
  })
})
