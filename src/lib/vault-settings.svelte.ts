// Vault-scoped settings mirror for the settings UI. Backed by the
// `{vault}/.notemd/settings.json` file the Rust `notemd_vault_settings_*`
// commands read/write, so these values travel with the git-synced vault.
import { invoke } from '@tauri-apps/api/core'

/** Default sync sub-directory; mirrors `vault_settings::DEFAULT_SYNC_DIR`. */
export const DEFAULT_SYNC_DIR = 'sync'

/** 大文件阈值默认值(MB);镜像 Rust DEFAULT_LARGE_FILE_THRESHOLD_MB。 */
export const DEFAULT_LARGE_FILE_THRESHOLD_MB = 10

/** Raw settings DTO as returned by the backend (absent field = null). */
export interface VaultSettingsDto {
  syncDir?: string | null
  wikipageDir?: string | null
  dailynoteDir?: string | null
  largeFileThresholdMb?: number | null
  searchExcludeDirs?: string[] | null
  searchLargeFileThresholdMb?: number | null
}

export const vaultSettings = $state<{
  syncDir: string
  largeFileThresholdMb: number
  searchExcludeDirs: string[]
  // The *effective* index-skip threshold: the explicitly-saved value if one
  // exists, otherwise `largeFileThresholdMb` (the git gate) — the same
  // one-way fallback `search::options::for_vault` applies on the Rust side
  // (see that module's doc comment). This is a display value only: loading
  // it this way does NOT itself save anything, so opening the settings page
  // can never silently trip the one-way door. Only `saveSearchLargeFileThreshold`
  // does that, and only when the user clicks Save.
  searchLargeFileThresholdMb: number
  // Whether `searchLargeFileThresholdMb` has been explicitly saved (vs. the
  // displayed value above being a fallback to the git gate) — the settings
  // UI needs this to explain, truthfully, whether the one-way door has
  // already been walked through.
  searchLargeFileThresholdExplicit: boolean
  vaultPath: string | null
  loaded: boolean
}>({
  syncDir: DEFAULT_SYNC_DIR,
  largeFileThresholdMb: DEFAULT_LARGE_FILE_THRESHOLD_MB,
  searchExcludeDirs: [],
  searchLargeFileThresholdMb: DEFAULT_LARGE_FILE_THRESHOLD_MB,
  searchLargeFileThresholdExplicit: false,
  vaultPath: null,
  loaded: false,
})

/** Load the current vault path and sync dir. Never throws — an unconfigured
 *  vault (backend rejects) leaves path null and the sync dir at its default. */
export async function loadVaultSettings(): Promise<void> {
  const root = await invoke<string | null>('sotvault_vault_root').catch(() => null)
  const dto = await invoke<VaultSettingsDto>('notemd_vault_settings_get').catch(
    () => ({}) as VaultSettingsDto,
  )
  vaultSettings.vaultPath = root ?? null
  vaultSettings.syncDir = dto?.syncDir ?? DEFAULT_SYNC_DIR
  vaultSettings.largeFileThresholdMb = dto?.largeFileThresholdMb ?? DEFAULT_LARGE_FILE_THRESHOLD_MB
  vaultSettings.searchExcludeDirs = dto?.searchExcludeDirs ?? []
  vaultSettings.searchLargeFileThresholdExplicit = dto?.searchLargeFileThresholdMb != null
  vaultSettings.searchLargeFileThresholdMb =
    dto?.searchLargeFileThresholdMb ?? vaultSettings.largeFileThresholdMb
  vaultSettings.loaded = true
}

/** Persist the sync dir (backend validates; rejection propagates to the caller
 *  for a toast). Only the syncDir field is sent — other fields are untouched. */
export async function saveSyncDir(raw: string): Promise<void> {
  const merged = await invoke<VaultSettingsDto>('notemd_vault_settings_set', { syncDir: raw })
  vaultSettings.syncDir = merged?.syncDir ?? DEFAULT_SYNC_DIR
  // 让改动进程内即时生效:刷新前端 vault 状态(vaultRoot/records)+ 通知依赖 vault 的
  // 特性(reading-insights 等)重挂载,不必重启 app。
  const { refreshSotvault } = await import('./sotvault.svelte')
  await refreshSotvault()
}

/** 持久化大文件阈值(MB,>=1)。后端校验;不改 vault 目录结构,故无需 refreshSotvault。
 *
 *  Review round 1, finding 2: this is the git gate `searchLargeFileThresholdMb`
 *  falls back to (see that field's doc comment) — while the search threshold has
 *  never been explicitly saved, its displayed value MUST track this one live,
 *  or the settings page shows a stale number that contradicts the hint text
 *  sitting right above the search-threshold input, explaining that exact
 *  relationship. Only re-derived when `!searchLargeFileThresholdExplicit`; once
 *  the user has explicitly saved a search threshold, this function must not
 *  touch it — that's the one-way door itself. */
export async function saveLargeFileThreshold(mb: number): Promise<void> {
  const merged = await invoke<VaultSettingsDto>('notemd_vault_settings_set', {
    largeFileThresholdMb: mb,
  })
  vaultSettings.largeFileThresholdMb =
    merged?.largeFileThresholdMb ?? DEFAULT_LARGE_FILE_THRESHOLD_MB
  if (!vaultSettings.searchLargeFileThresholdExplicit) {
    vaultSettings.searchLargeFileThresholdMb = vaultSettings.largeFileThresholdMb
  }
}

/** Persist the search-excluded directory list (backend validates each entry
 *  via validate_rel_dir). An empty array is a meaningful value — it clears
 *  any previously configured exclusions, it is not "not provided". */
export async function saveSearchExcludeDirs(dirs: string[]): Promise<void> {
  const merged = await invoke<VaultSettingsDto>('notemd_vault_settings_set', {
    searchExcludeDirs: dirs,
  })
  vaultSettings.searchExcludeDirs = merged?.searchExcludeDirs ?? []
}

/** 持久化索引跳过阈值(MB,>=1)。**单向门**:保存后,索引阈值就与
 *  `largeFileThresholdMb`(git 大文件门禁)彻底脱钩 —— 之后再改 git 门禁,
 *  索引阈值不会跟着变(见 `search::options::for_vault` 的回落逻辑)。 */
export async function saveSearchLargeFileThreshold(mb: number): Promise<void> {
  const merged = await invoke<VaultSettingsDto>('notemd_vault_settings_set', {
    searchLargeFileThresholdMb: mb,
  })
  vaultSettings.searchLargeFileThresholdExplicit = merged?.searchLargeFileThresholdMb != null
  vaultSettings.searchLargeFileThresholdMb =
    merged?.searchLargeFileThresholdMb ?? vaultSettings.largeFileThresholdMb
}
