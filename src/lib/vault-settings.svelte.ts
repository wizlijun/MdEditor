// Vault-scoped settings mirror for the settings UI. Backed by the
// `{vault}/.notemd/settings.json` file the Rust `notemd_vault_settings_*`
// commands read/write, so these values travel with the git-synced vault.
import { invoke } from '@tauri-apps/api/core'

/** Default sync sub-directory; mirrors `vault_settings::DEFAULT_SYNC_DIR`. */
export const DEFAULT_SYNC_DIR = 'sync'

/** 大文件阈值默认值(MB);镜像 Rust DEFAULT_LARGE_FILE_THRESHOLD_MB。 */
export const DEFAULT_LARGE_FILE_THRESHOLD_MB = 10

/** The ranking weights, all five fields present. Mirrors
 *  `searchidx::query::Weights` / `vault_settings::SearchWeights` — kept as a
 *  plain object here (not re-exporting a Rust type) the same way the backend
 *  DTO does, since this module otherwise has no dependency on `searchidx`.
 *
 *  **Every field the backend knows about MUST be listed here**, including the
 *  ones this app has no input box for. `vault_settings::merge` stores
 *  `search_weights` as ONE WHOLESALE STRUCT (`out.search_weights = Some(w)`,
 *  not a per-field merge), so a field missing from this mirror is a field
 *  silently *deleted* from `settings.json` the next time the settings page
 *  saves — which is exactly what happened to `attention` between the backend
 *  landing it and this mirror not being updated (final review I-2). */
export interface SearchWeights {
  human: number
  derived: number
  source: number
  unlabeled: number
  /** `k` in the attention boost `1 + k·f(minutes)`. **No input box** — it is
   *  round-tripped through this mirror so that saving the four tier weights
   *  cannot wipe a value the user set by hand in `.notemd/settings.json`
   *  (today the only way to set it, and `0` — "turn the feature off" — is the
   *  value that matters most).
   *
   *  Its legal range is the INVERSE of the four tiers': `0.0..=2.0`
   *  **inclusive at the bottom**, because `0` is the off switch, whereas the
   *  tiers reject `0` (a `0` multiplier collapses a whole tier's scores).
   *  See `vault_settings::validate_search_weights`'s `low_inclusive`
   *  parameter and `searchidx::query::Weights::sanitized`. */
  attention: number
}

/** The shipped constants — byte-identical to `searchidx::query::Weights`'s
 *  `Default` impl (design spec §3.1; `attention` per the attention-weighted
 *  retrieval spec §4.3). The "restore defaults" button in the settings UI
 *  fills the draft with this, it does not call the backend. */
export const DEFAULT_SEARCH_WEIGHTS: SearchWeights = {
  human: 1.25,
  derived: 1.0,
  source: 0.9,
  unlabeled: 0.3,
  attention: 0.4,
}

/** Mirror of `searchidx::query::Weights::sanitized`'s `attention` gate:
 *  finite and within `0.0..=2.0` (inclusive), otherwise the default. Applied
 *  to whatever the backend hands back so the value this module round-trips is
 *  the one that is actually in effect at query time.
 *
 *  Without it, a hand-edited out-of-range `attention` in `settings.json`
 *  (which the read side already ignores in favour of the default) would be
 *  echoed straight back in the next save payload, and
 *  `validate_search_weights` would reject the WHOLE save — leaving the user
 *  unable to change the four tier weights from a page that shows no
 *  `attention` field at all to explain why. */
function saneAttention(v: number | undefined | null): number {
  return typeof v === 'number' && Number.isFinite(v) && v >= 0 && v <= 2
    ? v
    : DEFAULT_SEARCH_WEIGHTS.attention
}

/** Backend DTO weights → the full five-field shape, defaults filling any
 *  absent field. One function so load and save cannot drift apart on which
 *  fields they carry — the drift between them is what I-2 was. */
function mergedWeights(dto: Partial<SearchWeights> | null | undefined): SearchWeights {
  const merged = { ...DEFAULT_SEARCH_WEIGHTS, ...(dto ?? {}) }
  return { ...merged, attention: saneAttention(merged.attention) }
}

/** Raw settings DTO as returned by the backend (absent field = null). */
export interface VaultSettingsDto {
  syncDir?: string | null
  wikipageDir?: string | null
  dailynoteDir?: string | null
  largeFileThresholdMb?: number | null
  searchExcludeDirs?: string[] | null
  searchLargeFileThresholdMb?: number | null
  /** `searchidx`'s glob-pattern whitelist for raw source material (task
   *  C-T8/C-T11, design spec §4.1/§7.1). `null`/absent = unconfigured, which
   *  is a distinct state from an explicit empty list — see
   *  `vault_settings::VaultSettings::search_source_globs`'s doc comment on
   *  the Rust side; this module mirrors that distinction rather than
   *  collapsing it to `[]` the way `searchExcludeDirs` above does, because
   *  collapsing it here would make "no patterns configured" and "patterns
   *  explicitly cleared" indistinguishable to the settings page. */
  searchSourceGlobs?: string[] | null
  searchWeights?: Partial<SearchWeights> | null
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
  // `null` = never explicitly configured (`search::options::for_vault` seeds
  // `<syncDir>/**` on the Rust side in that case) — kept distinct from `[]`
  // (explicitly cleared) for the same reason `VaultSettingsDto.searchSourceGlobs`
  // is, see that field's doc comment.
  searchSourceGlobs: string[] | null
  searchWeights: SearchWeights
  vaultPath: string | null
  loaded: boolean
}>({
  syncDir: DEFAULT_SYNC_DIR,
  largeFileThresholdMb: DEFAULT_LARGE_FILE_THRESHOLD_MB,
  searchExcludeDirs: [],
  searchLargeFileThresholdMb: DEFAULT_LARGE_FILE_THRESHOLD_MB,
  searchLargeFileThresholdExplicit: false,
  searchSourceGlobs: null,
  searchWeights: { ...DEFAULT_SEARCH_WEIGHTS },
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
  vaultSettings.searchSourceGlobs = dto?.searchSourceGlobs ?? null
  vaultSettings.searchWeights = mergedWeights(dto?.searchWeights)
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

/** Persist the raw-source-material glob-pattern whitelist (design spec
 *  §4.1/§7.1, task C-T8/C-T11). Saving a *different resolved pattern set*
 *  triggers a full index rebuild on the backend
 *  (`notemd_vault_settings_set` → `search_source_globs_changed` →
 *  `search::open_vault`) — fire-and-forget from here, same as
 *  `searchApi.rebuild`; the caller UI is what has to say so up front (design
 *  spec §7.3's contrast note), this function returns long before the
 *  rebuild itself finishes.
 *
 *  Blank-pattern rejection (spec §8: "空白模式 —— 保存时拒绝并指出是哪一条")
 *  is deliberately NOT done here. `searchidx::globs::parse` is tolerant of
 *  an unparseable entry (drops it, does not fail the whole list — that is a
 *  *consumer* obligation), so nothing on the Rust side rejects a blank
 *  string either. Silently saving a blank row is a different problem from
 *  the crate being tolerant of one: the user just typed that row and would
 *  reasonably believe it was kept. The settings page must validate before
 *  ever calling this — see `SettingsDialog.svelte`'s `onSaveSourceGlobs`. */
export async function saveSearchSourceGlobs(patterns: string[]): Promise<void> {
  const merged = await invoke<VaultSettingsDto>('notemd_vault_settings_set', {
    searchSourceGlobs: patterns,
  })
  vaultSettings.searchSourceGlobs = merged?.searchSourceGlobs ?? null
}

/** Persist the four per-tier ranking weights (design spec §3.1/§7.3, task
 *  C-T7/C-T11). Effective on the very next query — unlike
 *  `saveSearchSourceGlobs` above, this never triggers a rebuild: weights are
 *  read fresh at query time (`search::options::weights_for_vault`), never
 *  stamped into the index the way patterns are. Backend validation
 *  (`vault_settings::validate_search_weights`) rejects a NaN/negative/zero/
 *  >5.0 component per field and keeps whatever was previously stored — this
 *  lets that rejection propagate (does not catch it) so the caller can show
 *  it and revert its own draft to `vaultSettings.searchWeights`. */
export async function saveSearchWeights(weights: SearchWeights): Promise<void> {
  const merged = await invoke<VaultSettingsDto>('notemd_vault_settings_set', {
    searchWeights: weights,
  })
  vaultSettings.searchWeights = mergedWeights(merged?.searchWeights)
}
