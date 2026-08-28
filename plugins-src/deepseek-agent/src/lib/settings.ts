import { bridge } from './bridge'

export const CONCURRENCY_OPTIONS = [1, 2, 3, 4, 5] as const

export function normalizeMaxConcurrency(value: unknown): number {
  const parsed = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(parsed) || !Number.isInteger(parsed)) return 1
  return Math.max(1, Math.min(5, parsed))
}

export async function loadMaxConcurrency(): Promise<number> {
  const result = await bridge().request('host.settings.get')
  return normalizeMaxConcurrency(result?.settings?.maxConcurrency)
}

export async function saveMaxConcurrency(value: unknown): Promise<number> {
  const normalized = normalizeMaxConcurrency(value)
  await bridge().request('host.settings.set', {
    key: 'maxConcurrency',
    value: String(normalized),
  })
  return normalized
}
