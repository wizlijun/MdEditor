import { describe, expect, it } from 'vitest'
import type { AgentHarness } from '../agent-picker/types'
import {
  availableModelPreference,
  modelPreferenceKey,
  rememberedModelPreference,
  rememberModelPreference,
  resolvedModelHint,
  selectorForPreference,
} from './model-routing'

function memory(seed: Record<string, string> = {}) {
  const values = new Map(Object.entries(seed))
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    values,
  }
}

const harness: AgentHarness = {
  harness: 'Test',
  ok: true,
  default_model: 'quality',
  capabilities: {
    tasks: ['search-plan', 'search-summary', 'vault-research'],
    search_plan_schemas: [1],
    terminal_result: true,
    input_only_isolation: true,
    model_routing: {
      invocation_override: true,
      profiles: {
        fast: { model: 'fast-model', available: true },
        default: { model: 'quality', available: true },
      },
      selectable_models: ['fast-model', 'quality'],
    },
  },
}

describe('smart-search model routing', () => {
  it('defaults both automatic planning and manual summary to fast', () => {
    expect(rememberedModelPreference('global-search', 'p', 'plan', harness, memory()))
      .toBe('profile:fast')
    expect(rememberedModelPreference('global-search', 'p', 'summary', harness, memory()))
      .toBe('profile:fast')
  })

  it('keeps settings separate by provider and phase', () => {
    const storage = memory()
    rememberModelPreference('global-search', 'claude', 'plan', 'model:fast-model', storage)
    expect(storage.values.get(modelPreferenceKey('global-search', 'claude', 'plan')))
      .toBe('model:fast-model')
    expect(rememberedModelPreference('global-search', 'codex', 'plan', harness, storage))
      .toBe('profile:fast')
    expect(rememberedModelPreference('global-search', 'claude', 'summary', harness, storage))
      .toBe('profile:fast')
  })

  it('drops stale exact models and creates mutually exclusive selectors', () => {
    const storage = memory({
      [modelPreferenceKey('global-search', 'p', 'summary')]: 'model:removed',
    })
    expect(rememberedModelPreference('global-search', 'p', 'summary', harness, storage))
      .toBe('profile:fast')
    expect(selectorForPreference('profile:fast')).toEqual({ model_profile: 'fast' })
    expect(selectorForPreference('model:quality')).toEqual({ model: 'quality' })
    expect(availableModelPreference('model:removed', 'summary', harness)).toBe('profile:fast')
  })

  it('uses the capability model as the pre-run audit hint', () => {
    expect(resolvedModelHint('profile:fast', harness)).toBe('fast-model')
    expect(resolvedModelHint('profile:default', harness)).toBe('quality')
    expect(resolvedModelHint('model:custom', harness)).toBe('custom')
  })

  it('falls back to the available default when the fast profile is unavailable', () => {
    const withoutFast: AgentHarness = structuredClone(harness)
    withoutFast.capabilities!.model_routing.profiles.fast.available = false
    expect(rememberedModelPreference('global-search', 'p', 'plan', withoutFast, memory()))
      .toBe('profile:default')
  })

  it('survives an older or partial capability object', () => {
    const partial = { harness: 'Old', ok: true, capabilities: {} } as AgentHarness
    expect(() => resolvedModelHint('profile:fast', partial)).not.toThrow()
    expect(resolvedModelHint('profile:fast', partial)).toBeNull()
  })
})
