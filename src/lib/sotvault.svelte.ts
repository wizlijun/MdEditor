import { invoke } from '@tauri-apps/api/core'
import { pushToast } from './toast.svelte'
import { t } from './i18n/store.svelte'
import { activeTab, reloadTabFromDisk } from './tabs.svelte'
import { setMemoryProjectionVaultRoot } from './memory-projection'
import { hostname } from '@tauri-apps/plugin-os'
import { getDeviceId } from './settings.svelte'
import {
  canSyncToVault as computeCanSync,
  isTracked as computeIsTracked,
  isSyncedSource as computeIsSyncedSource,
  sourceForVault as computeSourceForVault,
  dialogActionFor,
  pushActionForOutcome,
  localYmd,
  mirrorMetaFor,
  deviceSourceFor,
  type SotRecord,
  type MirrorMeta,
} from './sotvault-logic'

export const sotvaultStore = $state<{ vaultRoot: string | null; records: SotRecord[]; mirrorMetas: MirrorMeta[]; tick: number }>({
  vaultRoot: null,
  records: [],
  mirrorMetas: [],
  tick: 0,
})

/** Notified when a refresh assigns a vault root **different from the one
 *  already in the store** — lets features like reading-insights install
 *  themselves once a vault becomes available, independent of app-boot
 *  ordering. Mirrors `setRecentsChangedHandler`.
 *
 *  The "different from" part is load-bearing. `refreshSotvault` runs from
 *  ~8 places that are not vault switches at all (note sync, question capture,
 *  settings saves), and the handler `App.svelte` installs throws away
 *  per-vault caches — including `indexStatus.reset()`, which blanks the
 *  settings page's provenance-tier table (design spec §9's discovery path for
 *  rule 6's deliberate misclassification) to "—" without reloading it: that
 *  tab's entry effect keys on `selectedTab`, which has not changed. Firing it
 *  for a refresh that changed nothing is pure loss.
 *
 *  A handler registered *after* the root was already loaded is the caller's
 *  problem to cover with one direct call — see `App.svelte`, which does
 *  exactly that alongside `setVaultRootChangedHandler`. */
let vaultRootChangedHandler: (() => void) | null = null
export function setVaultRootChangedHandler(fn: (() => void) | null): void {
  vaultRootChangedHandler = fn
}

export async function refreshSotvault(): Promise<void> {
  try {
    // The vault root is a GLOBAL setting (VaultSyncManager.repo_path). sotvault
    // is core-ized and always active, so this is always loaded. Other features —
    // notably reading-insights — rely on it to know whether a vault is configured.
    const root = await invoke<string | null>('sotvault_vault_root')
    // Core-ized: always load records (sotvault is always active).
    const records = await invoke<SotRecord[]>('sotvault_records')
    // Git-synced mirror metas make cross-device mirrors recognizable even on a
    // device that never synced them. Best-effort: empty when unavailable.
    const mirrorMetas = root
      ? await invoke<MirrorMeta[]>('notemd_mirror_metas').catch(() => [] as MirrorMeta[])
      : []
    // Compared against the store's own previous value rather than a private
    // "last seen" variable: one source of truth, and no way for the two to
    // drift apart.
    const rootChanged = sotvaultStore.vaultRoot !== root
    sotvaultStore.vaultRoot = root
    setMemoryProjectionVaultRoot(root)
    sotvaultStore.records = records
    sotvaultStore.mirrorMetas = mirrorMetas
    sotvaultStore.tick++
    // 身份由 vault 的 git 配置推出,换 vault 就换人。
    if (rootChanged) {
      void import('./okf/identity').then(m => { m.resetHumanActor(); m.warmHumanActor() })
    }
    if (rootChanged) vaultRootChangedHandler?.()
  } catch (e) {
    console.warn('[sotvault] refresh:', e)
  }
}

export function canSyncActive(path: string | null): boolean {
  return computeCanSync(path, sotvaultStore.vaultRoot, sotvaultStore.records)
}

export function isTrackedVaultFile(path: string | null): boolean {
  return computeIsTracked(path, sotvaultStore.records)
}

/** Source path a tracked vault copy was synced from, or null. */
export function sourceForVaultPath(path: string | null): string | null {
  return computeSourceForVault(path, sotvaultStore.records)
}

/** True when `path` is a source OUTSIDE the vault that already has a vault
 *  mirror — the state the `↔` marker announces ("you're editing the source"). */
export function isMirroredSource(path: string | null): boolean {
  return computeIsSyncedSource(path, sotvaultStore.records)
}

/** True when the given vault path is a mirror recorded by ANY device. */
export function isMirrorPath(path: string | null): boolean {
  return mirrorMetaFor(path, sotvaultStore.mirrorMetas, sotvaultStore.vaultRoot) !== null
}

/** This device's recorded source for a vault mirror (from git-synced metas). */
export function deviceSourceForVaultPath(path: string | null): string | null {
  return deviceSourceFor(path, sotvaultStore.mirrorMetas, sotvaultStore.vaultRoot, getDeviceId())
}

export interface NoteSibling { notePath: string; deviceName: string }

/** Sibling mirrors' notes (other devices, same content) for an open doc path. */
export async function noteSiblings(path: string | null): Promise<NoteSibling[]> {
  if (!path || !sotvaultStore.vaultRoot) return []
  return invoke<NoteSibling[]>('notemd_mirror_note_siblings', { docPath: path }).catch(() => [])
}

/** Relink a vault mirror to a locally-picked source, then open that source.
 *  Returns true when a relink happened. */
export async function relinkMirrorSource(vaultPath: string): Promise<boolean> {
  const { open } = await import('@tauri-apps/plugin-dialog')
  const picked = await open({ multiple: false, directory: false, filters: [{ name: 'Markdown', extensions: ['md', 'markdown'] }] })
  const newSource = typeof picked === 'string' ? picked : null
  if (!newSource) return false
  const { deviceId, deviceName } = await deviceInfo()
  await invoke('notemd_relink_mirror_source', { vaultPath, newSource, deviceId, deviceName })
  await refreshSotvault()
  const { openFile } = await import('./tabs.svelte')
  await openFile(newSource)
  return true
}

/** Device id/name stamped on each mirror meta (same ids recents/analytics use).
 *  Falls back to `Device-<id8>` when the OS hostname is unavailable — mirrors
 *  `recent-sync.svelte.ts`. */
async function deviceInfo(): Promise<{ deviceId: string; deviceName: string }> {
  const deviceId = getDeviceId()
  const deviceName = (await hostname().catch(() => null)) ?? `Device-${deviceId.slice(0, 8)}`
  return { deviceId, deviceName }
}

/** One-time backfill of app-support records into the git-synced .notemd/mirrors/
 *  store. Idempotent + best-effort; no-op without a configured vault. */
export async function migrateMirrorMeta(): Promise<void> {
  if (!sotvaultStore.vaultRoot) return
  const { deviceId, deviceName } = await deviceInfo()
  try {
    const n = await invoke<number>('notemd_migrate_mirror_meta', { deviceId, deviceName })
    if (n > 0) console.info(`[sotvault] migrated ${n} mirror meta records`)
  } catch (e) {
    console.warn('[sotvault] mirror meta migration:', e)
  }
}

/** Reveal the source file in the OS file browser (opens its folder, highlights it). */
export async function revealVaultSource(sourcePath: string): Promise<void> {
  try {
    const { revealItemInDir } = await import('@tauri-apps/plugin-opener')
    await revealItemInDir(sourcePath)
  } catch (e) {
    pushToast({ level: 'error', message: t('sotvault.revealFailed'), detail: String(e) })
  }
}

export async function syncCurrentToVault(): Promise<void> {
  const tab = activeTab()
  if (!tab?.filePath) {
    pushToast({ level: 'warn', message: t('sotvault.saveFirst') })
    return
  }
  try {
    const datePrefix = await sourceCreationYmd(tab.filePath)
    await invoke('sotvault_sync_to_vault', { srcPath: tab.filePath, datePrefix, ...(await deviceInfo()) })
    await refreshSotvault()
    pushToast({ level: 'success', message: t('sotvault.synced') })
  } catch (e) {
    pushToast({ level: 'error', message: t('sotvault.syncFailed'), detail: String(e) })
  }
}

/**
 * 把源 md 同步进 vault：复制 md + 建/更新映射。手记只住 vault 副本旁,
 * reconcile 永不回写源目录(后端固定行为,无需参数)。返回新 record(含 vault_path)。
 * 不 refreshSotvault——调用方须在把笔记写到 vault 副本旁 **之后** 再刷新，
 * 避免响应式 notePath 提前翻转到尚未写入的空 vault 笔记（数据竞态）。
 */
export async function syncSourceToVaultAsHome(srcPath: string): Promise<SotRecord> {
  const datePrefix = await sourceCreationYmd(srcPath)
  return invoke<SotRecord>('sotvault_sync_to_vault', { srcPath, datePrefix, ...(await deviceInfo()) })
}

/** Ensure a file living OUTSIDE the vault has a vault-homed copy inside it,
 *  reusing this source's existing tracked copy (in-place update — no
 *  proliferating `-2` copies). Same mechanism as writing a note against an
 *  outside md: the sync establishes the source→vault relationship so every
 *  later save pushes the source into the vault copy (save-push). Returns
 *  the vault copy's absolute path. Callers guarantee a vault is configured and
 *  the path is outside it. */
export async function ensureVaultCopyForShare(sourcePath: string): Promise<string> {
  const datePrefix = await sourceCreationYmd(sourcePath)
  const rec = await invoke<SotRecord>('sotvault_sync_to_vault', {
    srcPath: sourcePath, datePrefix, reuseExisting: true, ...(await deviceInfo()),
  })
  await refreshSotvault()
  return rec.vault_path
}

/** Local yyyy-MM-dd of the source file's creation time (birthtime), falling back
 *  to its mtime, then today, if the OS doesn't report a birthtime. */
async function sourceCreationYmd(path: string): Promise<string> {
  try {
    const { stat } = await import('@tauri-apps/plugin-fs')
    const info = await stat(path)
    return localYmd(info.birthtime ?? info.mtime ?? new Date())
  } catch {
    return localYmd()
  }
}

interface UpdateCheck {
  outcome: string
  vaultPath: string | null
  openedIsSource: boolean
}

export async function maybeCheckVaultUpdate(tab: { filePath: string }): Promise<void> {
  if (!tab.filePath) return

  let res: UpdateCheck
  try {
    res = await invoke<UpdateCheck>('sotvault_check_update', { openedPath: tab.filePath })
  } catch (e) {
    console.warn('[sotvault] check_update:', e)
    return
  }

  const action = dialogActionFor(res.outcome)
  if (action === 'none') return
  if (action === 'source-missing') {
    pushToast({ level: 'warn', message: t('sotvault.sourceMovedOrDeleted') })
    return
  }
  const vaultPath = res.vaultPath
  if (!vaultPath) return

  const { ask } = await import('@tauri-apps/plugin-dialog')

  if (action === 'confirm-origin') {
    // Same underlying action (source → vault), but worded for whichever side
    // the user opened.
    const msg = res.openedIsSource
      ? t('sotvault.askLocalChanged')
      : t('sotvault.askSourceUpdated')
    const yes = await ask(msg, { title: t('sotvault.syncTitle') })
    if (yes) await applyVaultUpdate(vaultPath)
    return
  }

  // action === 'conflict'
  const overwrite = await ask(t('sotvault.conflictOverwrite'), { title: t('sotvault.conflictTitle') })
  if (overwrite) {
    await applyVaultUpdate(vaultPath)
    return
  }
  const keep = await ask(t('sotvault.conflictKeep'), { title: t('sotvault.conflictTitle') })
  if (keep) {
    try {
      await invoke('sotvault_accept_current', { vaultPath })
      await refreshSotvault()
    } catch (e) {
      console.warn('[sotvault] accept_current:', e)
    }
  }
  // else: cancel — leave the record untouched; it will prompt again next open.
}

/** 源 md 被保存后，把改动静默推到已存在的 vault 影子副本；两边都改则弹现有冲突框。
 *  用后端 `sotvault_check_update` 作权威判定(未 tracked → outcome=not_tracked → noop),
 *  不依赖前端 records 是否已 refresh——避免"首存紧跟 note-sync 时 records 未就绪→漏推"。
 *  走 apply_update(非 sync_to_vault,后者会 dedup 出第二份副本)。 */
export async function pushSourceToVaultIfTracked(srcPath: string): Promise<void> {
  let res: UpdateCheck
  try {
    res = await invoke<UpdateCheck>('sotvault_check_update', { openedPath: srcPath })
  } catch (e) {
    console.warn('[sotvault] push check:', e)
    return
  }
  const action = pushActionForOutcome(res.outcome)
  if (action === 'noop' || !res.vaultPath) return
  if (action === 'apply-silent') {
    try {
      await invoke('sotvault_apply_update', { vaultPath: res.vaultPath })
      await reloadTabFromDisk(res.vaultPath)   // 幂等:vault 副本没开着就是 no-op
      await refreshSotvault()
    } catch (e) {
      console.warn('[sotvault] push apply:', e)
    }
    return
  }
  // 'prompt-conflict' —— 复用现有冲突对话框
  await maybeCheckVaultUpdate({ filePath: srcPath })
}

async function applyVaultUpdate(vaultPath: string): Promise<void> {
  try {
    await invoke<string>('sotvault_apply_update', { vaultPath })
    await reloadTabFromDisk(vaultPath)
    await refreshSotvault()
    pushToast({ level: 'success', message: t('sotvault.updatedFromSource') })
  } catch (e) {
    pushToast({ level: 'error', message: t('sotvault.updateFailed'), detail: String(e) })
  }
}
