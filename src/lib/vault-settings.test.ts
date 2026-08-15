import { describe, it, expect, vi, beforeEach } from 'vitest'

const invoke = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }))

import { vaultSettings, loadVaultSettings, saveSyncDir, DEFAULT_SYNC_DIR, saveLargeFileThreshold, DEFAULT_LARGE_FILE_THRESHOLD_MB, saveSearchExcludeDirs, saveSearchLargeFileThreshold, saveSearchWeights, DEFAULT_SEARCH_WEIGHTS } from './vault-settings.svelte'

/** Route invoke by command name so load's two parallel calls resolve. */
function route(map: Record<string, unknown>) {
  invoke.mockImplementation((cmd: string) =>
    cmd in map ? Promise.resolve(map[cmd]) : Promise.reject(new Error(`unexpected ${cmd}`)),
  )
}

beforeEach(() => {
  invoke.mockReset()
  vaultSettings.syncDir = DEFAULT_SYNC_DIR
  vaultSettings.largeFileThresholdMb = DEFAULT_LARGE_FILE_THRESHOLD_MB
  vaultSettings.searchExcludeDirs = []
  vaultSettings.searchLargeFileThresholdMb = DEFAULT_LARGE_FILE_THRESHOLD_MB
  vaultSettings.searchLargeFileThresholdExplicit = false
  vaultSettings.vaultPath = null
  vaultSettings.loaded = false
})

describe('loadVaultSettings', () => {
  it('populates vault path and sync dir from the backend', async () => {
    route({ sotvault_vault_root: '/v', notemd_vault_settings_get: { syncDir: 'box' } })
    await loadVaultSettings()
    expect(vaultSettings.vaultPath).toBe('/v')
    expect(vaultSettings.syncDir).toBe('box')
    expect(vaultSettings.loaded).toBe(true)
  })

  it('defaults the sync dir when the config omits it', async () => {
    route({ sotvault_vault_root: '/v', notemd_vault_settings_get: {} })
    await loadVaultSettings()
    expect(vaultSettings.syncDir).toBe(DEFAULT_SYNC_DIR)
  })

  it('defaults searchExcludeDirs to an empty list when the config omits it', async () => {
    route({ sotvault_vault_root: '/v', notemd_vault_settings_get: {} })
    await loadVaultSettings()
    expect(vaultSettings.searchExcludeDirs).toEqual([])
  })

  it('adopts a configured searchExcludeDirs list', async () => {
    route({ sotvault_vault_root: '/v', notemd_vault_settings_get: { searchExcludeDirs: ['sessions', 'tmp'] } })
    await loadVaultSettings()
    expect(vaultSettings.searchExcludeDirs).toEqual(['sessions', 'tmp'])
  })

  it('falls back the displayed search threshold to the git gate when unset', async () => {
    route({
      sotvault_vault_root: '/v',
      notemd_vault_settings_get: { largeFileThresholdMb: 25 },
    })
    await loadVaultSettings()
    expect(vaultSettings.searchLargeFileThresholdMb).toBe(25)
    expect(vaultSettings.searchLargeFileThresholdExplicit).toBe(false)
  })

  it('falls back the displayed search threshold to the built-in default when both are unset', async () => {
    route({ sotvault_vault_root: '/v', notemd_vault_settings_get: {} })
    await loadVaultSettings()
    expect(vaultSettings.searchLargeFileThresholdMb).toBe(DEFAULT_LARGE_FILE_THRESHOLD_MB)
    expect(vaultSettings.searchLargeFileThresholdExplicit).toBe(false)
  })

  it('prefers an explicitly configured search threshold over the git gate', async () => {
    route({
      sotvault_vault_root: '/v',
      notemd_vault_settings_get: { largeFileThresholdMb: 25, searchLargeFileThresholdMb: 50 },
    })
    await loadVaultSettings()
    expect(vaultSettings.searchLargeFileThresholdMb).toBe(50)
    expect(vaultSettings.searchLargeFileThresholdExplicit).toBe(true)
  })

  it('leaves vault path null and sync dir default when vault is not configured', async () => {
    // Both backend calls reject ("Vault not configured"); load must not throw.
    invoke.mockRejectedValue(new Error('Vault not configured'))
    await loadVaultSettings()
    expect(vaultSettings.vaultPath).toBeNull()
    expect(vaultSettings.syncDir).toBe(DEFAULT_SYNC_DIR)
    expect(vaultSettings.loaded).toBe(true)
  })
})

describe('saveSyncDir', () => {
  it('sends only the syncDir field and adopts the merged result', async () => {
    route({ notemd_vault_settings_set: { syncDir: 'box', wikipageDir: 'wiki' } })
    await saveSyncDir('  box  ')
    expect(invoke).toHaveBeenCalledWith('notemd_vault_settings_set', { syncDir: '  box  ' })
    expect(vaultSettings.syncDir).toBe('box')
  })

  it('propagates a backend validation error and leaves the store unchanged', async () => {
    vaultSettings.syncDir = 'sync'
    invoke.mockRejectedValue(new Error('directory must stay within the vault'))
    await expect(saveSyncDir('../escape')).rejects.toThrow()
    expect(vaultSettings.syncDir).toBe('sync')
  })
})

describe('saveLargeFileThreshold', () => {
  it('sends largeFileThresholdMb and adopts the merged result', async () => {
    invoke.mockResolvedValue({ largeFileThresholdMb: 20 })
    await saveLargeFileThreshold(20)
    expect(invoke).toHaveBeenCalledWith('notemd_vault_settings_set', { largeFileThresholdMb: 20 })
    expect(vaultSettings.largeFileThresholdMb).toBe(20)
  })

  it('falls back to the default when the response omits the field', async () => {
    invoke.mockResolvedValue({})
    await saveLargeFileThreshold(5)
    expect(vaultSettings.largeFileThresholdMb).toBe(DEFAULT_LARGE_FILE_THRESHOLD_MB)
  })

  // Review round 1, finding 2: saving the git gate must keep the *displayed*
  // search-index threshold live while it's still following that gate —
  // otherwise the settings page can show a stale effective value that
  // contradicts the one-way-door hint text sitting right next to it.
  it('keeps the effective search threshold following the git gate when it has not been explicitly set', async () => {
    vaultSettings.searchLargeFileThresholdExplicit = false
    vaultSettings.searchLargeFileThresholdMb = 10
    invoke.mockResolvedValue({ largeFileThresholdMb: 30 })
    await saveLargeFileThreshold(30)
    expect(vaultSettings.searchLargeFileThresholdMb).toBe(30)
  })

  it('does not override an explicitly-set search threshold — the one-way door stays shut', async () => {
    vaultSettings.searchLargeFileThresholdExplicit = true
    vaultSettings.searchLargeFileThresholdMb = 50
    invoke.mockResolvedValue({ largeFileThresholdMb: 30 })
    await saveLargeFileThreshold(30)
    expect(vaultSettings.searchLargeFileThresholdMb).toBe(50)
  })
})

describe('saveSearchExcludeDirs', () => {
  it('sends searchExcludeDirs and adopts the merged result', async () => {
    invoke.mockResolvedValue({ searchExcludeDirs: ['sessions'] })
    await saveSearchExcludeDirs(['sessions'])
    expect(invoke).toHaveBeenCalledWith('notemd_vault_settings_set', { searchExcludeDirs: ['sessions'] })
    expect(vaultSettings.searchExcludeDirs).toEqual(['sessions'])
  })

  it('sends an empty array to clear exclusions, distinct from omitting the field', async () => {
    vaultSettings.searchExcludeDirs = ['sessions']
    invoke.mockResolvedValue({ searchExcludeDirs: [] })
    await saveSearchExcludeDirs([])
    expect(invoke).toHaveBeenCalledWith('notemd_vault_settings_set', { searchExcludeDirs: [] })
    expect(vaultSettings.searchExcludeDirs).toEqual([])
  })

  it('propagates a backend validation error and leaves the store unchanged', async () => {
    vaultSettings.searchExcludeDirs = ['sessions']
    invoke.mockRejectedValue(new Error('directory must stay within the vault'))
    await expect(saveSearchExcludeDirs(['../escape'])).rejects.toThrow()
    expect(vaultSettings.searchExcludeDirs).toEqual(['sessions'])
  })
})

describe('saveSearchLargeFileThreshold', () => {
  it('sends searchLargeFileThresholdMb and adopts the merged result as explicit', async () => {
    invoke.mockResolvedValue({ searchLargeFileThresholdMb: 50 })
    await saveSearchLargeFileThreshold(50)
    expect(invoke).toHaveBeenCalledWith('notemd_vault_settings_set', { searchLargeFileThresholdMb: 50 })
    expect(vaultSettings.searchLargeFileThresholdMb).toBe(50)
    expect(vaultSettings.searchLargeFileThresholdExplicit).toBe(true)
  })

  // This is the one-way-door contract itself: once a save round-trips through
  // the backend, the displayed value must come from the response's own
  // searchLargeFileThresholdMb, not silently fall back to the git gate again
  // — a bug here would make the door look two-way in the UI even though the
  // backend already stopped following the git gate.
  it('does not fall back to the git gate after an explicit save, even if the gate value differs', async () => {
    vaultSettings.largeFileThresholdMb = 10
    invoke.mockResolvedValue({ searchLargeFileThresholdMb: 50, largeFileThresholdMb: 10 })
    await saveSearchLargeFileThreshold(50)
    expect(vaultSettings.searchLargeFileThresholdMb).toBe(50)
  })

  it('falls back to the git gate value if the backend response omits the field', async () => {
    vaultSettings.largeFileThresholdMb = 25
    invoke.mockResolvedValue({})
    await saveSearchLargeFileThreshold(5)
    expect(vaultSettings.searchLargeFileThresholdMb).toBe(25)
    expect(vaultSettings.searchLargeFileThresholdExplicit).toBe(false)
  })
})

// Final review I-2: the backend stores `search_weights` as ONE WHOLESALE
// STRUCT (`vault_settings::merge` does `out.search_weights = Some(w)`, not a
// per-field merge), so any field this mirror forgets is a field DELETED from
// `settings.json` on the next save. `attention` — the attention-boost `k`,
// which has no input box and whose interesting value is `0` ("turn the
// feature off") — was exactly that field: setting it by hand and then saving
// the four tier weights once silently reset it to the 0.4 default, with no
// message and no way to notice.
describe('saveSearchWeights — the attention weight survives a tier-weight save (final review I-2)', () => {
  it('carries a hand-set attention: 0 through load → save instead of dropping it', async () => {
    route({
      sotvault_vault_root: '/v',
      notemd_vault_settings_get: { searchWeights: { human: 1.25, derived: 1, source: 0.9, unlabeled: 0.3, attention: 0 } },
    })
    await loadVaultSettings()
    expect(vaultSettings.searchWeights.attention).toBe(0)

    // What the settings page sends: the whole draft, which is seeded from
    // `vaultSettings.searchWeights`.
    invoke.mockResolvedValue({ searchWeights: { ...vaultSettings.searchWeights } })
    await saveSearchWeights({ ...vaultSettings.searchWeights, human: 2 })

    const payload = invoke.mock.calls.at(-1)?.[1] as { searchWeights: Record<string, number> }
    expect(payload.searchWeights.attention).toBe(0)
    expect(vaultSettings.searchWeights.attention).toBe(0)
  })

  it('defaults attention to the shipped 0.4 when the config has never set it', async () => {
    route({ sotvault_vault_root: '/v', notemd_vault_settings_get: {} })
    await loadVaultSettings()
    expect(vaultSettings.searchWeights.attention).toBe(DEFAULT_SEARCH_WEIGHTS.attention)
    expect(DEFAULT_SEARCH_WEIGHTS.attention).toBe(0.4) // byte-identical to `Weights::default()`
  })

  // Mirrors `searchidx::query::Weights::sanitized`'s attention gate
  // (`0.0..=2.0`, inclusive at the bottom — the inverse of the four tiers,
  // which reject 0). A hand-edited out-of-range value is already ignored at
  // query time; echoing it back would make `validate_search_weights` reject
  // the whole save and lock the user out of the tier inputs with an error
  // about a field this page does not show.
  it('falls back to the default when the stored attention is out of the 0..=2 range', async () => {
    route({ sotvault_vault_root: '/v', notemd_vault_settings_get: { searchWeights: { attention: 9 } } })
    await loadVaultSettings()
    expect(vaultSettings.searchWeights.attention).toBe(DEFAULT_SEARCH_WEIGHTS.attention)
  })

  it('accepts the whole legal range including both ends', async () => {
    for (const k of [0, 0.4, 2]) {
      route({ sotvault_vault_root: '/v', notemd_vault_settings_get: { searchWeights: { attention: k } } })
      await loadVaultSettings()
      expect(vaultSettings.searchWeights.attention).toBe(k)
    }
  })
})
