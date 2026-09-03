import type { ModelPreference } from './model-routing'

export type LookupGroupBy = 'auto' | 'source' | 'date'
export type SummaryStyle = 'sentence' | 'bullets'

export interface SmartLookupSettings {
  planner: {
    enabled: boolean
    provider: 'auto' | string
    modelByProvider: Record<string, ModelPreference>
    timeoutMs: number
  }
  results: {
    limit: 20 | 50 | 100
    groupBy: LookupGroupBy
    autoDeepOnZero: boolean
    deepTimeoutMs: number
  }
  summary: {
    enabled: boolean
    provider: 'same_as_planner' | 'auto' | string
    modelByProvider: Record<string, ModelPreference>
    sourceLimit: number
    charLimit: number
    style: SummaryStyle
    timeoutMs: number
  }
  handoff: {
    defaultProvider: 'ask' | string
    includeSelectedRefs: boolean
  }
}

export const DEFAULT_SMART_LOOKUP_SETTINGS: SmartLookupSettings = {
  planner: {
    enabled: true,
    provider: 'auto',
    modelByProvider: {},
    timeoutMs: 8_000,
  },
  results: {
    limit: 50,
    groupBy: 'auto',
    autoDeepOnZero: false,
    deepTimeoutMs: 4_000,
  },
  summary: {
    enabled: true,
    provider: 'same_as_planner',
    modelByProvider: {},
    sourceLimit: 4,
    charLimit: 4_000,
    style: 'bullets',
    timeoutMs: 15_000,
  },
  handoff: {
    defaultProvider: 'ask',
    includeSelectedRefs: true,
  },
}

function object(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {}
}

function bool(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback
}

function boundedInteger(value: unknown, min: number, max: number, fallback: number): number {
  return Number.isInteger(value) && Number(value) >= min && Number(value) <= max
    ? Number(value)
    : fallback
}

function choice<T extends string | number>(value: unknown, values: readonly T[], fallback: T): T {
  return values.includes(value as T) ? value as T : fallback
}

function provider(value: unknown, fallback: string): string {
  return typeof value === 'string' && value.length > 0 && value.length <= 256 && !value.includes('\0')
    ? value
    : fallback
}

function isModelPreference(value: unknown): value is ModelPreference {
  return value === 'profile:fast'
    || value === 'profile:default'
    || (typeof value === 'string'
      && value.startsWith('model:')
      && value.length > 6
      && value.length <= 256
      && !Array.from(value).some((character) => /[\u0000-\u001f\u007f]/u.test(character)))
}

function modelMap(value: unknown): Record<string, ModelPreference> {
  const result: Record<string, ModelPreference> = {}
  for (const [key, preference] of Object.entries(object(value))) {
    if (provider(key, '') && isModelPreference(preference)) result[key] = preference
  }
  return result
}

/** Invalid fields fall back independently; one damaged value never resets the group. */
export function normalizeSmartLookupSettings(value: unknown): SmartLookupSettings {
  const root = object(value)
  const planner = object(root.planner)
  const results = object(root.results)
  const summary = object(root.summary)
  const handoff = object(root.handoff)
  const defaults = DEFAULT_SMART_LOOKUP_SETTINGS

  return {
    planner: {
      enabled: bool(planner.enabled, defaults.planner.enabled),
      provider: provider(planner.provider, defaults.planner.provider),
      modelByProvider: modelMap(planner.modelByProvider),
      timeoutMs: boundedInteger(planner.timeoutMs, 3_000, 15_000, defaults.planner.timeoutMs),
    },
    results: {
      limit: choice(results.limit, [20, 50, 100] as const, defaults.results.limit),
      groupBy: choice(results.groupBy, ['auto', 'source', 'date'] as const, defaults.results.groupBy),
      autoDeepOnZero: bool(results.autoDeepOnZero, defaults.results.autoDeepOnZero),
      deepTimeoutMs: boundedInteger(results.deepTimeoutMs, 1_000, 5_000, defaults.results.deepTimeoutMs),
    },
    summary: {
      enabled: bool(summary.enabled, defaults.summary.enabled),
      provider: provider(summary.provider, defaults.summary.provider),
      modelByProvider: modelMap(summary.modelByProvider),
      sourceLimit: boundedInteger(summary.sourceLimit, 1, 6, defaults.summary.sourceLimit),
      charLimit: boundedInteger(summary.charLimit, 1_000, 6_000, defaults.summary.charLimit),
      style: choice(summary.style, ['sentence', 'bullets'] as const, defaults.summary.style),
      timeoutMs: boundedInteger(summary.timeoutMs, 5_000, 30_000, defaults.summary.timeoutMs),
    },
    handoff: {
      defaultProvider: provider(handoff.defaultProvider, defaults.handoff.defaultProvider),
      includeSelectedRefs: bool(handoff.includeSelectedRefs, defaults.handoff.includeSelectedRefs),
    },
  }
}
