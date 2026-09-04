import { invoke } from '@tauri-apps/api/core'
import { activeTab, saveActive } from '../tabs.svelte'
import { getPluginScopedKey, settings } from '../settings.svelte'
import { pushToast } from '../toast.svelte'
import { isIOS } from '../platform.svelte'
import { bakeShareHtml } from '../plugins/share-baker'
import { computeActiveThemeId } from '../theme-loader'
import { publishHtml, vaultRelativeSrc } from './publish'
import { sotvaultStore, ensureVaultCopyForShare } from '../sotvault.svelte'
import { isUnder } from '../sotvault-logic'
import { unpublish } from './unpublish'
import { copyShareLink } from './copy-link'
import { uploadImage } from './upload-image'
import { ShareError } from './types'
import { t } from '../i18n/store.svelte'
import type { Messages } from '../i18n/en'

export function getShareConfig(): { baseUrl: string; defaultExpiry: 'never'|'7d'|'30d'|'90d'; slugRandomSuffix: boolean } | null {
  const baseUrl = getPluginScopedKey('share.baseUrl') as string | undefined
  const apiKey = getPluginScopedKey('share.apiKey') as string | undefined
  if (!baseUrl || !apiKey) return null
  return {
    baseUrl: baseUrl.replace(/\/+$/, ''),
    defaultExpiry: (getPluginScopedKey('share.defaultExpiry') as any) ?? 'never',
    slugRandomSuffix: (getPluginScopedKey('share.slugRandomSuffix') as boolean | undefined) ?? true,
  }
}

const SHARE_ERROR_KEYS: Record<string, keyof Messages> = {
  not_configured: 'share.err.not_configured',
  vault_required: 'share.err.vault_required',
  no_path: 'share.err.no_path',
  empty_content: 'share.err.empty_content',
  network: 'share.err.network',
  auth: 'share.err.auth',
  forbidden: 'share.err.forbidden',
  too_large: 'share.err.too_large',
  conflict: 'share.err.conflict',
  unsupported: 'share.err.unsupported',
  server: 'share.err.server',
  http: 'share.err.http',
  parse: 'share.err.parse',
  corrupt_record: 'share.err.corrupt_record',
}

function reportError(e: unknown, action: string) {
  if (e instanceof ShareError) {
    const key = SHARE_ERROR_KEYS[e.kind]
    pushToast({
      level: 'error',
      message: t('share.errPrefix', { msg: key ? t(key) : e.kind }),
      detail: e.detail,
    })
  } else {
    pushToast({ level: 'error', message: t('share.actionFailed', { action }), detail: String(e) })
  }
}

/**
 * THE single share pre-step, shared by every entry point (desktop menu plugin
 * action, `notemd share` CLI, and the iOS/mobile command). Ensures the file
 * being shared resolves under the vault and returns its vault-relative `src`.
 *
 * A file outside the vault is synced in first — vault-homed exactly like writing
 * a note against an outside md — so the source stays the edit surface, save-push
 * keeps the vault copy current on every edit, and other machines resolve the
 * shared md under their vault. Files already under the vault pass straight
 * through. Throws ShareError('vault_required') when no vault is configured.
 *
 * @param flush optional GUI hook (e.g. saveActive) to persist in-memory edits
 *   before the sync reads the source from disk. Omit for headless CLI.
 */
export async function prepareShareSrc(path: string, flush?: () => Promise<void>): Promise<string> {
  const root = sotvaultStore.vaultRoot
  if (root && isUnder(path, root)) return vaultRelativeSrc(path, root)
  if (!root) throw new ShareError('vault_required')
  await flush?.()
  const vaultPath = await ensureVaultCopyForShare(path)
  return vaultRelativeSrc(vaultPath, root)
}

export async function sharePublishCurrent(): Promise<void> {
  const tab = activeTab()
  if (!tab) return
  if (tab.kind === 'canvas') {
    return reportError(new ShareError('unsupported'), t('share.action.share'))
  }
  const cfg = getShareConfig()
  if (!cfg) return reportError(new ShareError('not_configured'), t('share.action.share'))
  if (!tab.filePath) return reportError(new ShareError('no_path'), t('share.action.share'))

  try {
    if (tab.kind === 'image') {
      const { url, isUpdate } = await uploadImage({
        path: tab.filePath, filename: tab.title,
        baseUrl: cfg.baseUrl, defaultExpiry: cfg.defaultExpiry,
      })
      pushToast({
        level: 'success',
        message: isUpdate ? t('share.imageUpdated') : t('share.imageShared'),
        detail: url,
      })
      const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
      await writeText(url)
      if (await isIOS()) {
        try { await invoke('present_share_sheet', { url, text: tab.title }) }
        catch { /* present_share_sheet not implemented yet — Swift bridge deferred to post-v1 */ }
      }
      return
    }

    let src: string
    try {
      src = await prepareShareSrc(tab.filePath, saveActive)
    } catch (e) {
      if (e instanceof ShareError) return reportError(e, t('share.action.share'))
      throw e
    }

    // Bake with the ACTIVE theme, not bakeShareHtml's 'default' fallback —
    // same resolution as the CLI (settings.theme + prefers-color-scheme).
    // globalThis.matchMedia exists in both the desktop and iOS webviews; the
    // optional chain keeps this headless/test safe.
    const systemDark = globalThis.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false
    const themeId = computeActiveThemeId(settings.theme, systemDark)
    // bakeShareHtml throws ShareError('too_large') itself when input/output
    // exceeds 25 MB — the catch below localizes it.
    const html = await bakeShareHtml(tab, themeId)
    if (!html) return reportError(new ShareError('empty_content'), t('share.action.share'))

    const { url, isUpdate } = await publishHtml({
      path: tab.filePath, filename: tab.title, html,
      baseUrl: cfg.baseUrl,
      defaultExpiry: cfg.defaultExpiry,
      slugRandomSuffix: cfg.slugRandomSuffix,
      src,
    })
    const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
    await writeText(url)
    pushToast({
      level: 'success',
      message: isUpdate ? t('share.contentUpdated') : t('share.shared'),
      detail: url,
    })
    if (await isIOS()) {
      try { await invoke('present_share_sheet', { url, text: tab.title }) }
      catch { /* present_share_sheet not implemented yet — Swift bridge deferred to post-v1 */ }
    }
  } catch (e) {
    reportError(e, t('share.action.share'))
  }
}

export async function shareUnpublishCurrent(): Promise<void> {
  const tab = activeTab()
  if (!tab?.filePath) return
  if (tab.kind === 'canvas') {
    return reportError(new ShareError('unsupported'), t('share.action.unpublish'))
  }
  const cfg = getShareConfig()
  // A deleted config with a lingering record must surface, not silently no-op.
  if (!cfg) return reportError(new ShareError('not_configured'), t('share.action.unpublish'))
  try {
    await unpublish({ path: tab.filePath, baseUrl: cfg.baseUrl })
    pushToast({ level: 'success', message: t('share.unpublished') })
  } catch (e) { reportError(e, t('share.action.unpublish')) }
}

export async function shareCopyLinkCurrent(): Promise<void> {
  const tab = activeTab()
  if (!tab?.filePath) return
  if (tab.kind === 'canvas') {
    return reportError(new ShareError('unsupported'), t('share.action.copyLink'))
  }
  try {
    const url = await copyShareLink(tab.filePath)
    pushToast({ level: 'success', message: t('share.linkCopied'), detail: url })
  } catch (e) { reportError(e, t('share.action.copyLink')) }
}
