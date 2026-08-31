import type { InstalledV2 } from './types'

export const INSTALLED_CACHE_KEY = 'notemd.plugin-market.installed.v1'

type ReadableStorage = Pick<Storage, 'getItem'>
type WritableStorage = Pick<Storage, 'setItem'>

interface InstalledCacheV1 {
  version: 1
  installed: InstalledV2[]
}

function storageOrNull<T>(storage: T | null | undefined): T | null {
  if (storage === null) return null
  if (storage !== undefined) return storage
  try {
    return localStorage as T
  } catch {
    return null
  }
}

function installedPlugin(value: unknown): InstalledV2 | null {
  if (!value || typeof value !== 'object') return null
  const row = value as Record<string, unknown>
  if (
    typeof row.id !== 'string' || row.id === '' ||
    typeof row.version !== 'string' || row.version === '' ||
    typeof row.enabled !== 'boolean' ||
    !(row.name === null || typeof row.name === 'string') ||
    !(row.category === undefined || row.category === null || typeof row.category === 'string') ||
    !Array.isArray(row.capabilities) ||
    !row.capabilities.every((cap) => typeof cap === 'string')
  ) return null

  return {
    id: row.id,
    version: row.version,
    enabled: row.enabled,
    name: row.name,
    category: row.category as string | null | undefined,
    capabilities: [...row.capabilities] as string[],
  }
}

/** Restore only the small, local-first snapshot used before the registry loads. */
export function readInstalledCache(storage?: ReadableStorage | null): InstalledV2[] {
  const target = storageOrNull(storage)
  if (!target) return []
  try {
    const raw = target.getItem(INSTALLED_CACHE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as Partial<InstalledCacheV1>
    if (parsed.version !== 1 || !Array.isArray(parsed.installed)) return []
    return parsed.installed
      .map(installedPlugin)
      .filter((row): row is InstalledV2 => row !== null)
  } catch {
    return []
  }
}

/** Persist category + installed/enabled state after every authoritative refresh. */
export function writeInstalledCache(
  installed: readonly InstalledV2[],
  storage?: WritableStorage | null,
): void {
  const target = storageOrNull(storage)
  if (!target) return
  try {
    const snapshot: InstalledCacheV1 = {
      version: 1,
      installed: installed.map((row) => ({
        id: row.id,
        version: row.version,
        enabled: row.enabled,
        name: row.name,
        category: row.category,
        capabilities: [...row.capabilities],
      })),
    }
    target.setItem(INSTALLED_CACHE_KEY, JSON.stringify(snapshot))
  } catch {
    // A disabled/full webview store must not prevent the market from opening.
  }
}
