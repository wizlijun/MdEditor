import type { AgentHarness } from '../agent-picker/types'

export type SearchModelPhase = 'plan' | 'answer'
export type ModelPreference = 'profile:fast' | 'profile:default' | `model:${string}`

export type ModelSelector =
  | { model_profile: 'fast' | 'default'; model?: never }
  | { model: string; model_profile?: never }

export function defaultModelPreference(phase: SearchModelPhase): ModelPreference {
  return phase === 'plan' ? 'profile:fast' : 'profile:default'
}

export function modelPreferenceKey(
  surface: string,
  provider: string,
  phase: SearchModelPhase,
): string {
  return `notemd.agent.model.${surface}.${provider}.${phase}`
}

export function rememberedModelPreference(
  surface: string,
  provider: string,
  phase: SearchModelPhase,
  harness: AgentHarness | null | undefined,
  storage: Pick<Storage, 'getItem'> | null = safeStorage(),
): ModelPreference {
  let saved: string | null = null
  try {
    saved = storage?.getItem(modelPreferenceKey(surface, provider, phase)) ?? null
  } catch { /* A blocked localStorage must not disable search. */ }
  return isAvailablePreference(saved, harness)
    ? saved
    : availableDefaultPreference(phase, harness)
}

export function rememberModelPreference(
  surface: string,
  provider: string,
  phase: SearchModelPhase,
  preference: ModelPreference,
  storage: Pick<Storage, 'setItem'> | null = safeStorage(),
): void {
  try {
    storage?.setItem(modelPreferenceKey(surface, provider, phase), preference)
  } catch { /* The current run can still use the in-memory choice. */ }
}

export function selectorForPreference(preference: ModelPreference): ModelSelector {
  if (preference === 'profile:fast') return { model_profile: 'fast' }
  if (preference === 'profile:default') return { model_profile: 'default' }
  return { model: preference.slice('model:'.length) }
}

export function resolvedModelHint(
  preference: ModelPreference,
  harness: AgentHarness | null | undefined,
): string | null {
  if (preference === 'profile:fast') {
    return harness?.capabilities?.model_routing?.profiles?.fast?.model ?? null
  }
  if (preference === 'profile:default') {
    return harness?.capabilities?.model_routing?.profiles?.default?.model
      ?? harness?.default_model
      ?? null
  }
  return preference.slice('model:'.length)
}

export function selectableModelPreferences(
  harness: AgentHarness | null | undefined,
): ModelPreference[] {
  return (harness?.capabilities?.model_routing?.selectable_models ?? [])
    .filter((model) => model.trim().length > 0)
    .map((model) => `model:${model}` as const)
}

function isAvailablePreference(
  value: string | null,
  harness: AgentHarness | null | undefined,
): value is ModelPreference {
  if (value === 'profile:fast') {
    return harness?.capabilities?.model_routing?.profiles?.fast?.available === true
  }
  if (value === 'profile:default') {
    return harness?.capabilities?.model_routing?.profiles?.default?.available === true
  }
  if (!value?.startsWith('model:') || value.length === 'model:'.length) return false
  return harness?.capabilities?.model_routing?.selectable_models?.includes(
    value.slice('model:'.length),
  ) === true
}

function availableDefaultPreference(
  phase: SearchModelPhase,
  harness: AgentHarness | null | undefined,
): ModelPreference {
  const routing = harness?.capabilities?.model_routing
  if (phase === 'plan' && routing?.profiles?.fast?.available === true) return 'profile:fast'
  if (routing?.profiles?.default?.available === true) return 'profile:default'
  const exact = routing?.selectable_models?.find((model) => model.trim().length > 0)
  return exact ? `model:${exact}` : defaultModelPreference(phase)
}

function safeStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage
  } catch {
    return null
  }
}
