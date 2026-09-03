import { describe, expect, it } from 'vitest'
import { buildSearchPlanPrompt } from './plan'

describe('smart-search planning', () => {
  it('pins time and locked filters without any tune or Vault payload', () => {
    const prompt = buildSearchPlanPrompt({
      question: '找上个月的发布决定',
      referenceTime: '2026-09-03T09:00:00+08:00',
      timezone: 'Asia/Taipei',
      locale: 'zh-CN',
      lockedFilters: { origins: ['human'] },
    })
    expect(prompt).toContain('MODE: plan')
    expect(prompt).toContain('REFERENCE_TIME: 2026-09-03T09:00:00+08:00')
    expect(prompt).toContain('document_date|content_date|activity_time|ambiguous')
    expect(prompt).toContain('do not copy time.sourceText into terms or phrases')
    expect(prompt).toContain('LOCKED_FILTERS_JSON: {"origins":["human"]}')
    expect(prompt).toContain('Do not use Markdown fences and do not answer')
    expect(prompt).toContain('at most two logical query arms')
    expect(prompt).not.toContain('PREVIOUS_SEARCH_PLAN_JSON')
    expect(prompt).not.toContain('RETRIEVAL_TELEMETRY_JSON')
    expect(prompt).not.toContain('MODE: tune')
  })
})
