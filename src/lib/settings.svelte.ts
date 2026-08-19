import { Store } from '@tauri-apps/plugin-store'

type Mode = 'source' | 'rich'

export interface MdblockSettings {
  enabled: boolean
  autoRefreshOnSave: boolean
  injectAiHint: boolean
  similarityThreshold: number
  splitCoverageThreshold: number
  chunkSizeChars: number
  chunkStrategy: 'size' | 'section'
  sectionCutLevel: number          // 1..6
  sectionMinChars: number
  hover: {
    enabled: boolean
    showSourceGutter: boolean
    showRichOverlay: boolean
    badgeFormat: 'short' | 'full'
  }
}

export const DEFAULT_MDBLOCK_SETTINGS: MdblockSettings = {
  enabled: false,
  autoRefreshOnSave: false,
  injectAiHint: true,
  similarityThreshold: 0.5,
  splitCoverageThreshold: 0.3,
  chunkSizeChars: 2400,
  chunkStrategy: 'section',
  sectionCutLevel: 2,
  sectionMinChars: 400,
  hover: {
    enabled: false,
    showSourceGutter: true,
    showRichOverlay: true,
    badgeFormat: 'short',
  },
}

export interface ThemeSettings {
  light: string
  dark: string
  followSystem: boolean
}

const DEFAULT_THEME: ThemeSettings = { light: 'default', dark: 'default', followSystem: true }

export const settings = $state<{
  autoSave: boolean
  toastAutoClose: boolean
  theme: ThemeSettings
  mdblock: MdblockSettings
  dailyNotes: { enabled: boolean }
  mcpServer: { enabled: boolean }
}>({
  autoSave: false,
  toastAutoClose: false,
  theme: { ...DEFAULT_THEME },
  mdblock: structuredClone(DEFAULT_MDBLOCK_SETTINGS),
  dailyNotes: { enabled: false },
  // Default ON — a missing key must also mean ON (mirrored in loadSettings
  // below with `!== false`), so an upgrading user gets MCP without having to
  // discover the setting first.
  mcpServer: { enabled: true },
})

let store: Awaited<ReturnType<typeof Store.load>> | null = null
let recentFiles: string[] = []
let recentModesByExt: Record<string, Mode> = {}
let recentOpenedAt: Record<string, number> = {}
let recentTombstones: string[] = []
let deviceId: string | null = null
let recentsChangedHandler: (() => void) | null = null
const TOMBSTONE_CAP = 200
let pluginScoped: Record<string, Record<string, unknown>> = {}
let settingsHydrated = false

/**
 * Share-plugin records (one entry per shared file path → slug/url/created_at/…).
 * Kept in a separate on-disk file (`share_db.json`) rather than the main
 * settings store so the user's primary settings stay small and human-readable,
 * and so the records map can grow without bloating every loadSettings.
 *
 * In-memory shape: { records: Record<filePath, ShareRecord> }
 * Capping: at most 200 entries; on overflow, records older than 30 days
 * (by `created_at`) are evicted.
 */
type ShareRecord = Record<string, unknown>
const SHARE_DB_FILE = 'share_db.json'
const SHARE_DB_MAX = 200
const SHARE_DB_EVICT_OLDER_THAN_DAYS = 30
let shareDbStore: Awaited<ReturnType<typeof Store.load>> | null = null
let shareRecords: Record<string, ShareRecord> = {}

async function getStore() {
  if (!store) store = await Store.load('settings.json')
  return store
}

async function getShareDbStore() {
  if (!shareDbStore) shareDbStore = await Store.load(SHARE_DB_FILE)
  return shareDbStore
}

/**
 * Hydrate `shareRecords` from `share_db.json`. On first run after the
 * migration, also lifts any pre-existing `plugins.share.records` map out of
 * the main settings store into the new file, then strips it from settings
 * so the main store stays lean.
 */
export async function loadShareDb(): Promise<void> {
  const dbs = await getShareDbStore()
  const stored = await dbs.get<Record<string, ShareRecord>>('records')
  if (stored && typeof stored === 'object') {
    shareRecords = stored
    return
  }
  // Migration: settings.json may still hold `plugins.share.records`.
  try {
    const s = await getStore()
    const plugins = await s.get<Record<string, Record<string, unknown>>>('plugins')
    const legacy = plugins?.share?.records
    if (legacy && typeof legacy === 'object') {
      shareRecords = applyShareDbCap(legacy as Record<string, ShareRecord>)
      await persistShareDb()
      // Strip from main settings; preserve every other share-scoped key.
      if (plugins?.share) {
        const { records: _drop, ...rest } = plugins.share as Record<string, unknown>
        plugins.share = rest
        await s.set('plugins', plugins)
        await s.save()
      }
      // Reflect the strip in in-memory pluginScoped if already loaded.
      if (pluginScoped.share && 'records' in pluginScoped.share) {
        const sub = pluginScoped.share as Record<string, unknown>
        delete sub.records
      }
    }
  } catch (e) {
    console.warn('[settings] share_db migration:', e)
  }
}

export async function loadSettings(): Promise<void> {
  const s = await getStore()
  settings.autoSave = (await s.get<boolean>('autoSave')) ?? false
  settings.toastAutoClose = (await s.get<boolean>('toastAutoClose')) ?? false

  // Theme migration: prefer new shape; fall back to legacy single skin id.
  const storedTheme = await s.get<ThemeSettings>('theme')
  if (storedTheme && typeof storedTheme.light === 'string' && typeof storedTheme.dark === 'string') {
    settings.theme = {
      light: storedTheme.light,
      dark: storedTheme.dark,
      followSystem: storedTheme.followSystem !== false,
    }
  } else {
    const legacy = await s.get<string>('skin')
    if (legacy) {
      settings.theme = { light: legacy, dark: legacy, followSystem: false }
      // Drop the legacy key so future loads take the new path.
      await s.delete('skin')
    } else {
      settings.theme = { ...DEFAULT_THEME }
    }
  }

  recentFiles = (await s.get<string[]>('recentFiles')) ?? []
  recentModesByExt = (await s.get<Record<string, Mode>>('recentModesByExt')) ?? {}
  recentOpenedAt = (await s.get<Record<string, number>>('recentOpenedAt')) ?? {}
  recentTombstones = (await s.get<string[]>('recentTombstones')) ?? []
  deviceId = (await s.get<string>('device.id')) ?? null
  if (!deviceId) {
    deviceId = crypto.randomUUID()
    await s.set('device.id', deviceId)
    await s.save()
  }
  pluginScoped = (await s.get<Record<string, Record<string, unknown>>>('plugins')) ?? {}
  const storedMdblock = await s.get<MdblockSettings>('mdblock')
  settings.mdblock = storedMdblock
    ? {
        ...DEFAULT_MDBLOCK_SETTINGS,
        ...storedMdblock,
        hover: { ...DEFAULT_MDBLOCK_SETTINGS.hover, ...(storedMdblock.hover ?? {}) },
      }
    : structuredClone(DEFAULT_MDBLOCK_SETTINGS)
  const storedDaily = await s.get<{ enabled: boolean }>('dailyNotes')
  settings.dailyNotes.enabled = storedDaily?.enabled ?? false
  // Missing key = enabled (see the design note on the initial state above).
  const storedMcp = await s.get<{ enabled: boolean }>('mcpServer')
  settings.mcpServer.enabled = storedMcp?.enabled !== false
  settingsHydrated = true
  pluginScopedVersion.value++
  await loadShareDb()
}

export async function saveSettings(): Promise<void> {
  if (!settingsHydrated) return
  const s = await getStore()
  await s.set('autoSave', settings.autoSave)
  await s.set('toastAutoClose', settings.toastAutoClose)
  await s.set('theme', settings.theme)
  await s.set('recentFiles', recentFiles)
  await s.set('recentModesByExt', recentModesByExt)
  await s.set('recentOpenedAt', recentOpenedAt)
  await s.set('recentTombstones', recentTombstones)
  await s.set('plugins', pluginScoped)
  await s.set('mdblock', settings.mdblock)
  await s.set('dailyNotes', { enabled: settings.dailyNotes.enabled })
  await s.set('mcpServer', { enabled: settings.mcpServer.enabled })
  await s.save()
  // Notify other webviews (e.g. the standalone Daily Notes window) so they can
  // re-read settings and re-apply theme/locale live. Fire-and-forget.
  import('@tauri-apps/api/event').then((m) => m.emit('settings://changed')).catch(() => {})
}

export function getRecentFiles(): readonly string[] {
  return recentFiles
}

export async function pushRecentFile(path: string): Promise<void> {
  recentFiles = [path, ...recentFiles.filter((p) => p !== path)].slice(0, 10)
  recentOpenedAt[path] = Date.now()
  // Drop timestamps for paths no longer in the list.
  for (const k of Object.keys(recentOpenedAt)) {
    if (!recentFiles.includes(k)) delete recentOpenedAt[k]
  }
  // Re-opening a previously-failed file clears its tombstone.
  recentTombstones = recentTombstones.filter((p) => p !== path)
  await saveSettings()
  recentsChangedHandler?.()
}

/** Remove a recent (e.g. it failed to open) and tombstone it so a synced copy won't resurrect it. */
export async function removeRecentFile(path: string): Promise<void> {
  recentFiles = recentFiles.filter((p) => p !== path)
  delete recentOpenedAt[path]
  recentTombstones = [path, ...recentTombstones.filter((p) => p !== path)].slice(0, TOMBSTONE_CAP)
  await saveSettings()
  recentsChangedHandler?.()
}

export function getRecentOpenedAt(): Readonly<Record<string, number>> {
  return recentOpenedAt
}

export function getRecentTombstones(): readonly string[] {
  return recentTombstones
}

export function getDeviceId(): string {
  if (!deviceId) deviceId = crypto.randomUUID()
  return deviceId
}

/** Registered by the recent-sync module; fired after any change to the recents list. */
export function setRecentsChangedHandler(fn: (() => void) | null): void {
  recentsChangedHandler = fn
}

/**
 * One-shot "we asked about the notemd CLI install" flag, persisted under
 * `cli.promptShown` in the settings store. Read by App.svelte on first
 * launch to decide whether to nudge the user into installing the symlink.
 */
export async function getCliPromptShown(): Promise<boolean> {
  const s = await getStore()
  return (await s.get<boolean>('cli.promptShown')) ?? false
}

export async function setCliPromptShown(v: boolean): Promise<void> {
  const s = await getStore()
  await s.set('cli.promptShown', v)
  await s.save()
}

/** `key` is the extension (or special basename) returned by `modeKeyFor`. */
export function getRecentMode(key: string): Mode | null {
  return recentModesByExt[key] ?? null
}

/** `key` is the extension (or special basename) returned by `modeKeyFor`. */
export async function setRecentMode(key: string, mode: Mode): Promise<void> {
  recentModesByExt[key] = mode
  await saveSettings()
}

// --- Plugin-scoped settings ---

/**
 * Reactive version counter — increments on every `loadSettings` and every
 * `mergePluginScoped`. Subscribers (like App.svelte's enabled_when re-eval
 * effect) should depend on `.value` to re-run when plugin settings change.
 */
export const pluginScopedVersion = $state<{ value: number }>({ value: 0 })

/**
 * Get all keys for a single plugin id, returned with their fully-qualified
 * names (e.g. `share.baseUrl`). Returns `{}` if the plugin has no settings yet.
 *
 * For `share`, the records map is synthesized from the separate share_db
 * store so plugin code that reads `settings['share.records']` keeps working
 * regardless of where the data physically lives.
 */
export function getPluginScopedAll(pluginId: string): Record<string, unknown> {
  const sub = pluginScoped[pluginId] ?? {}
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(sub)) {
    out[`${pluginId}.${k}`] = v
  }
  if (pluginId === 'share') {
    out['share.records'] = { ...shareRecords }
  }
  return out
}

/** Read the current share records map (used by enabled_when / UI). */
export function getShareRecords(): Readonly<Record<string, ShareRecord>> {
  return shareRecords
}

/**
 * Apply 30-day eviction when the records map exceeds the cap. Records
 * lacking a parseable `created_at` are kept (we won't drop data we can't
 * date). Returns the (possibly trimmed) map; pure.
 */
function applyShareDbCap(records: Record<string, ShareRecord>): Record<string, ShareRecord> {
  const entries = Object.entries(records)
  if (entries.length <= SHARE_DB_MAX) return records
  const cutoff = Date.now() - SHARE_DB_EVICT_OLDER_THAN_DAYS * 24 * 3600 * 1000
  const kept: Record<string, ShareRecord> = {}
  for (const [k, rec] of entries) {
    const ts = parseCreatedAt(rec)
    if (ts == null || ts >= cutoff) kept[k] = rec
  }
  return kept
}

function parseCreatedAt(rec: ShareRecord): number | null {
  const v = (rec as Record<string, unknown>).created_at
  if (typeof v !== 'string') return null
  const t = Date.parse(v)
  return Number.isFinite(t) ? t : null
}

async function persistShareDb(): Promise<void> {
  const s = await getShareDbStore()
  await s.set('records', shareRecords)
  await s.save()
}

/**
 * Replace the share records map (share publish/unpublish rewrites the full
 * map in one shot). Applies the 200-cap + 30-day eviction policy before
 * persisting.
 */
async function replaceShareRecords(next: Record<string, ShareRecord>): Promise<void> {
  shareRecords = applyShareDbCap({ ...next })
  await persistShareDb()
}

/** Read a single fully-qualified plugin-scoped key (e.g. 'share.baseUrl'). */
export function getPluginScopedKey(fqKey: string): unknown {
  const dot = fqKey.indexOf('.')
  if (dot <= 0) return undefined
  const id = fqKey.slice(0, dot)
  const key = fqKey.slice(dot + 1)
  return pluginScoped[id]?.[key]
}

/**
 * Merge a flat patch where keys are fully-qualified `<plugin-id>.<key>`.
 * Each entry is stored under `pluginScoped[<plugin-id>][<key>]`.
 */
export async function mergePluginScoped(patch: Record<string, unknown>): Promise<void> {
  let needSaveSettings = false
  for (const [fqKey, value] of Object.entries(patch)) {
    // Intercept share.records → routed to share_db.json, not settings.json.
    if (fqKey === 'share.records') {
      if (value && typeof value === 'object') {
        await replaceShareRecords(value as Record<string, ShareRecord>)
      }
      continue
    }
    const dot = fqKey.indexOf('.')
    if (dot <= 0) continue
    const id = fqKey.slice(0, dot)
    const key = fqKey.slice(dot + 1)
    if (!pluginScoped[id]) pluginScoped[id] = {}
    pluginScoped[id][key] = value
    needSaveSettings = true
  }
  pluginScopedVersion.value++
  if (needSaveSettings) await saveSettings()
}

/**
 * 读某个插件域下的一个键。
 *
 * 与 `getPluginScopedKey(fqKey)` 的区别:插件 id 单独传。后者按**第一个点**切
 * 分,对 v2 的 `publisher.name` 形状 id(如 `notemd.power-mode`)会切成
 * `notemd`,读到别处去。新代码一律用这个。
 */
export function getPluginScopedValue(pluginId: string, key: string): unknown {
  return pluginScoped[pluginId]?.[key]
}

/** 写某个插件域下的一个键并落盘。点号安全,理由同 `getPluginScopedValue`。 */
export async function setPluginScopedValue(pluginId: string, key: string, value: unknown): Promise<void> {
  if (!pluginScoped[pluginId]) pluginScoped[pluginId] = {}
  pluginScoped[pluginId][key] = value
  pluginScopedVersion.value++
  await saveSettings()
}

