import { describe, it, expect, vi, beforeEach } from 'vitest'

const invoke = vi.fn()
const ask = vi.fn()
const pushToast = vi.fn()
const reloadTabFromDisk = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ ask: (...a: unknown[]) => ask(...a) }))
vi.mock('./toast.svelte', () => ({ pushToast: (...a: unknown[]) => pushToast(...a) }))
vi.mock('./tabs.svelte', () => ({
  activeTab: () => ({ filePath: '/src/a.md' }),
  reloadTabFromDisk: (...a: unknown[]) => reloadTabFromDisk(...a),
}))
// hostname() feeds deviceName; stat() feeds the sync date prefix. Both are
// dynamically imported inside sotvault.svelte, so mock the modules.
vi.mock('@tauri-apps/plugin-os', () => ({ hostname: async () => 'Test-Mac' }))
vi.mock('@tauri-apps/plugin-fs', () => ({ stat: async () => ({ birthtime: new Date(0) }) }))

import { getDeviceId } from './settings.svelte'
import {
  maybeCheckVaultUpdate, refreshSotvault, setVaultRootChangedHandler, sotvaultStore, syncCurrentToVault,
} from './sotvault.svelte'

const VAULT = '/v/Sync/a.md'

/** check_update result for opening the vault copy. */
const vaultCheck = (outcome: string) => ({ outcome, vaultPath: VAULT, openedIsSource: false })
/** check_update result for opening the source file. */
const sourceCheck = (outcome: string) => ({ outcome, vaultPath: VAULT, openedIsSource: true })

beforeEach(() => {
  invoke.mockReset(); ask.mockReset(); pushToast.mockReset(); reloadTabFromDisk.mockReset()
  sotvaultStore.vaultRoot = null
  sotvaultStore.records = []
  sotvaultStore.mirrorMetas = []
})

describe('refreshSotvault', () => {
  it('loads vault root and records (core-ized: always active)', async () => {
    // sotvault is core-ized — vault root and records are always loaded.
    invoke
      .mockResolvedValueOnce('/v')                          // sotvault_vault_root
      .mockResolvedValueOnce([{ source: '/s', vault: '/v/Sync/s.md' }]) // sotvault_records
      .mockResolvedValueOnce([])                            // notemd_mirror_metas
    await refreshSotvault()
    expect(sotvaultStore.vaultRoot).toBe('/v')
    expect(invoke).toHaveBeenCalledWith('sotvault_records')
    expect(sotvaultStore.records).toHaveLength(1)
  })

  // The handler is `App.svelte`'s vault-switch hook: it resets the search
  // index status cache (file/block counts, db size, AND the whole provenance
  // tier table the settings page shows), reinstalls the reading-insights
  // tracker, reloads the wikilink blocklist and re-points the folder view.
  // `refreshSotvault` is called from ~8 places that are NOT vault switches —
  // note sync, question capture, settings saves — and firing on every one of
  // them blanks the Search & Index tab's tier table to "—" while it is open,
  // with no reload (the tab's entry effect is keyed on `selectedTab`, which
  // has not changed). Spec §9 makes that table the discovery path for rule
  // 6's deliberate misclassification, so blanking it mid-read is exactly the
  // wrong moment.
  it('notifies the vault-root handler only when the root actually changes', async () => {
    const onRootChanged = vi.fn()
    setVaultRootChangedHandler(onRootChanged)
    let root: string | null = '/v'
    invoke.mockImplementation((cmd: string) => {
      if (cmd === 'sotvault_vault_root') return Promise.resolve(root)
      if (cmd === 'sotvault_records') return Promise.resolve([])
      if (cmd === 'notemd_mirror_metas') return Promise.resolve([])
      return Promise.reject(new Error(`unexpected ${cmd}`))
    })

    await refreshSotvault()
    expect(onRootChanged).toHaveBeenCalledTimes(1) // null -> '/v'

    // The ~8 non-switch callers: same root, nothing to re-derive.
    await refreshSotvault()
    await refreshSotvault()
    expect(onRootChanged).toHaveBeenCalledTimes(1)

    // A real switch still fires, and so does clearing the vault.
    root = '/w'
    await refreshSotvault()
    expect(onRootChanged).toHaveBeenCalledTimes(2)
    root = null
    await refreshSotvault()
    expect(onRootChanged).toHaveBeenCalledTimes(3)

    setVaultRootChangedHandler(null)
  })

  it('loads mirror metas into the store', async () => {
    // Route by command name so meta loading doesn't depend on the (root, records,
    // metas) call order — mirrors the store's best-effort meta fetch.
    invoke.mockImplementation((cmd: string) => {
      if (cmd === 'sotvault_vault_root') return Promise.resolve('/v')
      if (cmd === 'sotvault_records') return Promise.resolve([])
      if (cmd === 'notemd_mirror_metas') return Promise.resolve([
        { mirror: 'sync/a.md', deviceId: 'd1', deviceName: 'Mac', source: '/s/a.md', syncedAt: 1, checksum: 'sha256:x' },
      ])
      return Promise.reject(new Error(`unexpected ${cmd}`))
    })
    await refreshSotvault()
    expect(sotvaultStore.mirrorMetas).toHaveLength(1)
    expect(sotvaultStore.mirrorMetas[0].mirror).toBe('sync/a.md')
  })
})

describe('maybeCheckVaultUpdate', () => {
  it('does nothing on up_to_date', async () => {
    invoke.mockResolvedValueOnce(vaultCheck('up_to_date'))
    await maybeCheckVaultUpdate({ filePath: VAULT })
    expect(ask).not.toHaveBeenCalled()
  })

  it('does nothing when not tracked', async () => {
    invoke.mockResolvedValueOnce({ outcome: 'not_tracked', vaultPath: null, openedIsSource: false })
    await maybeCheckVaultUpdate({ filePath: '/random/x.md' })
    expect(ask).not.toHaveBeenCalled()
  })

  it('toasts on source_missing, no dialog', async () => {
    invoke.mockResolvedValueOnce(vaultCheck('source_missing'))
    await maybeCheckVaultUpdate({ filePath: VAULT })
    expect(ask).not.toHaveBeenCalled()
    expect(pushToast).toHaveBeenCalledWith(expect.objectContaining({ level: 'warn' }))
  })

  it('applies update when origin_updated is confirmed (vault copy opened)', async () => {
    invoke
      .mockResolvedValueOnce(vaultCheck('origin_updated')) // sotvault_check_update
      .mockResolvedValueOnce('NEW CONTENT')                // sotvault_apply_update
      .mockResolvedValueOnce('/v')                         // sotvault_vault_root (refresh)
      .mockResolvedValueOnce([])                           // sotvault_records (refresh)
    ask.mockResolvedValueOnce(true)
    await maybeCheckVaultUpdate({ filePath: VAULT })
    expect(invoke).toHaveBeenCalledWith('sotvault_apply_update', { vaultPath: VAULT })
    expect(reloadTabFromDisk).toHaveBeenCalledWith(VAULT)
  })

  it('prompts to sync when the opened SOURCE file changed since last sync', async () => {
    invoke
      .mockResolvedValueOnce(sourceCheck('origin_updated')) // check_update keyed by source
      .mockResolvedValueOnce('NEW CONTENT')                 // apply_update
      .mockResolvedValueOnce('/v')                          // refresh root
      .mockResolvedValueOnce([])                            // refresh records
    ask.mockResolvedValueOnce(true)
    await maybeCheckVaultUpdate({ filePath: '/src/a.md' })
    // applies to the resolved vault path, not the opened source path
    expect(invoke).toHaveBeenCalledWith('sotvault_apply_update', { vaultPath: VAULT })
  })

  it('does not apply when origin_updated is declined', async () => {
    invoke.mockResolvedValueOnce(vaultCheck('origin_updated'))
    ask.mockResolvedValueOnce(false)
    await maybeCheckVaultUpdate({ filePath: VAULT })
    expect(invoke).toHaveBeenCalledTimes(1)
  })

  it('conflict: overwrite path applies update', async () => {
    invoke
      .mockResolvedValueOnce(vaultCheck('conflict'))
      .mockResolvedValueOnce('NEW')   // apply_update
      .mockResolvedValueOnce('/v')    // refresh root
      .mockResolvedValueOnce([])      // refresh records
    ask.mockResolvedValueOnce(true)   // overwrite? yes
    await maybeCheckVaultUpdate({ filePath: VAULT })
    expect(invoke).toHaveBeenCalledWith('sotvault_apply_update', { vaultPath: VAULT })
  })

  it('conflict: keep-vault path accepts current', async () => {
    invoke
      .mockResolvedValueOnce(vaultCheck('conflict'))
      .mockResolvedValueOnce(undefined) // accept_current
      .mockResolvedValueOnce('/v')      // refresh root
      .mockResolvedValueOnce([])        // refresh records
    ask.mockResolvedValueOnce(false)    // overwrite? no
    ask.mockResolvedValueOnce(true)     // keep vault & stop prompting? yes
    await maybeCheckVaultUpdate({ filePath: VAULT })
    expect(invoke).toHaveBeenCalledWith('sotvault_accept_current', { vaultPath: VAULT })
  })
})

describe('syncCurrentToVault', () => {
  it('passes deviceId and deviceName to sotvault_sync_to_vault', async () => {
    invoke
      .mockResolvedValueOnce(undefined) // sotvault_sync_to_vault
      .mockResolvedValueOnce('/v')      // refresh root
      .mockResolvedValueOnce([])        // refresh records
    await syncCurrentToVault()
    const call = invoke.mock.calls.find((c) => c[0] === 'sotvault_sync_to_vault')
    expect(call?.[1]).toMatchObject({ deviceId: getDeviceId(), deviceName: 'Test-Mac' })
  })
})
