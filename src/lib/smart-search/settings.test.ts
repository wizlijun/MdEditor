import { describe, expect, it } from 'vitest'
import { DEFAULT_SMART_LOOKUP_SETTINGS, normalizeSmartLookupSettings } from './settings'

describe('smart lookup settings', () => {
  it('uses the low-cost defaults', () => {
    expect(normalizeSmartLookupSettings(undefined)).toEqual(DEFAULT_SMART_LOOKUP_SETTINGS)
    expect(DEFAULT_SMART_LOOKUP_SETTINGS.planner.timeoutMs).toBe(8_000)
    expect(DEFAULT_SMART_LOOKUP_SETTINGS.results.autoDeepOnZero).toBe(false)
    expect(DEFAULT_SMART_LOOKUP_SETTINGS.summary.modelByProvider).toEqual({})
  })

  it('keeps valid fields while independently resetting damaged fields', () => {
    const normalized = normalizeSmartLookupSettings({
      planner: { enabled: false, provider: 'notemd.codex-agent', timeoutMs: 99_000 },
      results: { limit: 20, groupBy: 'date', autoDeepOnZero: true, deepTimeoutMs: 500 },
      summary: {
        provider: 'auto', sourceLimit: 6, charLimit: 6_000, style: 'sentence', timeoutMs: 30_000,
        modelByProvider: {
          'notemd.codex-agent': 'model:gpt-fast',
          bad: 'profile:cheap',
          control: 'model:bad\0model',
          oversized: `model:${'x'.repeat(260)}`,
        },
      },
      handoff: { defaultProvider: '../bad\0', includeSelectedRefs: false },
    })

    expect(normalized.planner).toMatchObject({
      enabled: false, provider: 'notemd.codex-agent', timeoutMs: 8_000,
    })
    expect(normalized.results).toEqual({
      limit: 20, groupBy: 'date', autoDeepOnZero: true, deepTimeoutMs: 4_000,
    })
    expect(normalized.summary).toMatchObject({
      provider: 'auto', sourceLimit: 6, charLimit: 6_000, style: 'sentence', timeoutMs: 30_000,
      modelByProvider: { 'notemd.codex-agent': 'model:gpt-fast' },
    })
    expect(normalized.handoff).toEqual({ defaultProvider: 'ask', includeSelectedRefs: false })
  })

  it('accepts every inclusive numeric boundary', () => {
    const min = normalizeSmartLookupSettings({
      planner: { timeoutMs: 3_000 },
      results: { deepTimeoutMs: 1_000 },
      summary: { sourceLimit: 1, charLimit: 1_000, timeoutMs: 5_000 },
    })
    const max = normalizeSmartLookupSettings({
      planner: { timeoutMs: 15_000 },
      results: { deepTimeoutMs: 5_000 },
      summary: { sourceLimit: 6, charLimit: 6_000, timeoutMs: 30_000 },
    })
    expect(min.planner.timeoutMs).toBe(3_000)
    expect(min.summary.charLimit).toBe(1_000)
    expect(max.planner.timeoutMs).toBe(15_000)
    expect(max.summary.sourceLimit).toBe(6)
  })
})
