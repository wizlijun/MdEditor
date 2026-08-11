import { describe, it, expect, vi, beforeEach } from 'vitest'

const invoke = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }))

import { vaultSettings, loadVaultSettings, saveSyncDir, DEFAULT_SYNC_DIR, saveLargeFileThreshold, DEFAULT_LARGE_FILE_THRESHOLD_MB, saveSearchExcludeDirs, saveSearchLargeFileThreshold } from './vault-settings.svelte'

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
