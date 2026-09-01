import { bridge } from './bridge'
import { DEFAULT_PRIORITY, localDateAfter, normalizeContexts, normalizePriority, type PlanningMetadata, type Priority } from './metadata'
import { DEFAULT_WIP_LIMIT } from './model'

export interface NextSettings {
  wipLimit: number
  defaultPriority: Priority
  defaultDueDays: number
  defaultContext: string
}

export const DEFAULT_NEXT_SETTINGS: NextSettings = {
  wipLimit: DEFAULT_WIP_LIMIT,
  defaultPriority: DEFAULT_PRIORITY,
  defaultDueDays: 0,
  defaultContext: '',
}

export function normalizeWipLimit(value: unknown): number {
  const parsed = typeof value === 'number' ? value : Number(value)
  return Number.isSafeInteger(parsed) && parsed >= 1 ? parsed : DEFAULT_WIP_LIMIT
}

export function normalizeDefaultDueDays(value: unknown): number {
  const parsed = typeof value === 'number' ? value : Number(value)
  return Number.isSafeInteger(parsed) && parsed >= 0 ? parsed : DEFAULT_NEXT_SETTINGS.defaultDueDays
}

export function normalizeDefaultContext(value: unknown): string {
  return typeof value === 'string' ? value.normalize('NFKC').trim() : ''
}

export function normalizeNextSettings(value: unknown): NextSettings {
  const settings = value !== null && typeof value === 'object'
    ? value as Record<string, unknown>
    : {}
  return {
    wipLimit: normalizeWipLimit(settings.wipLimit),
    defaultPriority: normalizePriority(settings.defaultPriority),
    defaultDueDays: normalizeDefaultDueDays(settings.defaultDueDays),
    defaultContext: normalizeDefaultContext(settings.defaultContext),
  }
}

export function planningDefaults(settings: NextSettings | undefined, now = new Date()): PlanningMetadata {
  const effective = settings ?? DEFAULT_NEXT_SETTINGS
  return {
    priority: effective.defaultPriority,
    ...(effective.defaultDueDays > 0 ? { due: localDateAfter(effective.defaultDueDays, now) } : {}),
    contexts: normalizeContexts(effective.defaultContext ? [effective.defaultContext] : []),
  }
}

export async function loadNextSettings(): Promise<NextSettings> {
  try {
    const result = await bridge().request('host.settings.get')
    const stored = result?.settings
    const settings = stored !== null && typeof stored === 'object'
      ? stored as Record<string, unknown>
      : {}
    // 1.6.0 exposed the four values through the Host-generated Settings tab,
    // which stored them as separate local keys. The plugin-owned page writes a
    // single object so saving is atomic; prefer it while retaining the legacy
    // shape as a no-loss upgrade path.
    return normalizeNextSettings(
      Object.prototype.hasOwnProperty.call(settings, 'configuration')
        ? settings.configuration
        : settings,
    )
  } catch {
    return { ...DEFAULT_NEXT_SETTINGS }
  }
}

export async function saveNextSettings(value: NextSettings): Promise<NextSettings> {
  const normalized = normalizeNextSettings(value)
  await bridge().request('host.settings.set', {
    key: 'configuration',
    value: normalized,
  })
  return normalized
}

export async function loadWipLimit(): Promise<number> {
  return (await loadNextSettings()).wipLimit
}
