import { describe, expect, it } from 'vitest'
import { INSTALLED_CACHE_KEY, readInstalledCache, writeInstalledCache } from './cache'
import type { InstalledV2 } from './types'

function store(seed: Record<string, string> = {}) {
  const values = new Map(Object.entries(seed))
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    values,
  }
}

const installed: InstalledV2 = {
  id: 'notemd.next',
  version: '1.3.0',
  enabled: false,
  name: 'Next',
  category: 'thinking',
  capabilities: ['vault.read', 'vault.write'],
}

describe('plugin market installed cache', () => {
  it('round-trips category and enabled state for the local-first view', () => {
    const storage = store()
    writeInstalledCache([installed], storage)

    expect(readInstalledCache(storage)).toEqual([installed])
  })

  it('ignores malformed rows without discarding valid installed plugins', () => {
    const storage = store({
      [INSTALLED_CACHE_KEY]: JSON.stringify({
        version: 1,
        installed: [installed, { id: 'broken', enabled: 'yes' }],
      }),
    })

    expect(readInstalledCache(storage)).toEqual([installed])
  })

  it('fails closed for unknown versions, invalid JSON, and blocked storage', () => {
    expect(readInstalledCache(store({
      [INSTALLED_CACHE_KEY]: JSON.stringify({ version: 2, installed: [installed] }),
    }))).toEqual([])
    expect(readInstalledCache(store({ [INSTALLED_CACHE_KEY]: '{nope' }))).toEqual([])

    const blocked = {
      getItem: () => { throw new Error('denied') },
      setItem: () => { throw new Error('denied') },
    }
    expect(readInstalledCache(blocked)).toEqual([])
    expect(() => writeInstalledCache([installed], blocked)).not.toThrow()
  })

  it('works when localStorage is unavailable', () => {
    expect(readInstalledCache(null)).toEqual([])
    expect(() => writeInstalledCache([installed], null)).not.toThrow()
  })
})
