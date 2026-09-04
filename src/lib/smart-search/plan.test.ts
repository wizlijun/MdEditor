import { describe, expect, it } from 'vitest'
import { buildSearchPlanPrompt } from './plan'

describe('smart-search planning', () => {
  it('pins time and locked filters without any tune or Vault payload', () => {
    const prompt = buildSearchPlanPrompt({
      question: '找上个月的发布决定',
      referenceTime: '2026-09-03T09:00:00+08:00',
      referenceDate: '2026-09-03',
      timezone: 'Asia/Taipei',
      locale: 'zh-CN',
      lockedFilters: { origins: ['human'] },
      timeAnchors: {
        today: { after: '2026-09-03', before: '2026-09-03' },
        lastMonth: { after: '2026-08-01', before: '2026-08-31' },
      },
    })
    expect(prompt).toContain('MODE: plan')
    expect(prompt).toContain('REFERENCE_TIME: 2026-09-03T09:00:00+08:00')
    expect(prompt).toContain('document_date|content_date|activity_time|ambiguous')
    expect(prompt).toContain('TIME GATE')
    expect(prompt).toContain('Inspect every temporal mention')
    expect(prompt).toContain('use ambiguous instead of silently choosing one')
    expect(prompt).toContain('REFERENCE_DATE: 2026-09-03')
    expect(prompt).toContain('TRUSTED_TIME_ANCHORS_JSON: {"today":{"after":"2026-09-03","before":"2026-09-03"},"lastMonth":{"after":"2026-08-01","before":"2026-08-31"}}')
    expect(prompt).toContain('copy the matching anchor exact after/before pair into absolute_range')
    expect(prompt).toContain('never calculate anchor dates yourself')
    expect(prompt).toContain('one complete explicit date')
    expect(prompt).toContain('after and before equal')
    expect(prompt).toContain('N days covers exactly N calendar dates')
    expect(prompt).toContain('Do not infer a date window when QUESTION has no explicit temporal cue')
    expect(prompt).toContain('keep time null')
    expect(prompt).toContain('do not copy time.sourceText into terms or phrases')
    expect(prompt).toContain('LOCKED_FILTERS_JSON: {"origins":["human"]}')
    expect(prompt).toContain('Do not use Markdown fences and do not answer')
    expect(prompt).toContain('at most two logical query arms')
    expect(prompt).not.toContain('PREVIOUS_SEARCH_PLAN_JSON')
    expect(prompt).not.toContain('RETRIEVAL_TELEMETRY_JSON')
    expect(prompt).not.toContain('MODE: tune')
    expect(prompt).not.toContain('calendar_day')
    expect(prompt).not.toContain('calendar_quarter')
    expect(prompt.indexOf('TIME GATE')).toBeLessThan(prompt.indexOf('Keep search terms concise'))
  })
})
