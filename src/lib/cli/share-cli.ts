/**
 * `notemd share` — share is core: it runs through the TS lib (the same
 * publish/unpublish/copy-link/upload-image the desktop menu uses), no plugin
 * binary. Extracted from CliRunner.svelte so the CLI contract (envelope,
 * exit codes, clipboard gating) is unit-testable with injected deps.
 *
 * Output contract (matches what plugin commands emit through CliRunner, and
 * the exit codes `notemd help` documents):
 * - success --json: { ok: true, data: { ... } }, exit 0
 * - failure --json: { ok: false, error: { code, message } } on stdout + stderr, exit 4
 * - failure non-json: stderr only, exit 4
 * - exit 2 reserved for file/argument errors (missing arg, unreadable file)
 */
import { invoke } from '@tauri-apps/api/core'
import { stat, readTextFile } from '@tauri-apps/plugin-fs'
import { writeText as clipWriteText } from '@tauri-apps/plugin-clipboard-manager'
import { sha256Hex } from '../hash'
import { settings } from '../settings.svelte'
import { computeActiveThemeId } from '../theme-loader'
import { bakeShareHtml } from '../plugins/share-baker'
import { publishHtml } from '../share/publish'
import { unpublish } from '../share/unpublish'
import { copyShareLink } from '../share/copy-link'
import { uploadImage } from '../share/upload-image'
import { ShareError } from '../share/types'
import { getRecord } from '../share/records'
import { basenameOf, extensionOf, inferKind, type CliPayload } from './cli-runner'
import type { Tab } from '../tabs.svelte'
import type { FileKind } from '../fs'

export interface CliFinishResult {
  exit_code: number
  stdout?: string
  stderr: string[]
}

/** Injected effects. `finish` is required (routes to `cli_finish`); the rest
 *  default to the real window/plugin-bound implementations and exist so tests
 *  can observe or stub them. */
export interface ShareCliDeps {
  finish: (r: CliFinishResult) => Promise<void>
  /** prefers-color-scheme probe (window-bound). */
  systemDark?: () => boolean
  /** Clipboard writer for publish/upload results (copy-link gates internally). */
  writeClipboard?: (text: string) => Promise<void>
  /** Vault diagnostics appended to share-failure stderr. */
  diagnostics?: (file: string) => Promise<string[]>
}

/** Map the broader cli-runner kind to the editor's FileKind (no 'plaintext'). */
function toFileKind(k: ReturnType<typeof inferKind>): FileKind {
  if (k === 'plaintext') return 'code'
  return k
}

/** 分享报错时的 vault 诊断:读了哪个配置文件、sotvault 值、各层解析结果、文件与
 *  vault 的关系。原样打印(不改大小写),便于发现 Sync/sync 之类不一致。best-effort。 */
async function shareVaultDiagnostics(filePath: string): Promise<string[]> {
  const lines: string[] = []
  const add = (k: string, v: unknown) =>
    lines.push(`  ${k}: ${v === undefined ? '(undefined)' : v === null ? 'null' : typeof v === 'string' ? v : JSON.stringify(v)}`)
  add('file', filePath)
  try {
    const { homeDir } = await import('@tauri-apps/api/path')
    const { exists, readTextFile } = await import('@tauri-apps/plugin-fs')
    const cfgPath = `${await homeDir()}/Library/Application Support/com.laobu.mdeditor-shared/config.json`
    add('shared config', cfgPath)
    const cfgExists = await exists(cfgPath).catch(() => false)
    add('shared config exists', cfgExists)
    if (cfgExists) {
      const raw = await readTextFile(cfgPath).catch(() => '')
      let sotvault: unknown = '(parse failed)'
      try { sotvault = JSON.parse(raw).sotvault } catch { /* keep placeholder */ }
      add('config.sotvault', sotvault)
    }
  } catch (e) { add('config read error', String(e)) }
  try {
    const backendRoot = await invoke<string | null>('sotvault_vault_root').catch(() => null)
    add('sotvault_vault_root() → backend', backendRoot)
    const { sotvaultStore } = await import('../sotvault.svelte')
    add('store.vaultRoot', sotvaultStore.vaultRoot)
    if (backendRoot) {
      const r = backendRoot.endsWith('/') ? backendRoot : `${backendRoot}/`
      add('file under vault? (case-sensitive)', filePath === backendRoot || filePath.startsWith(r))
    }
  } catch (e) { add('resolve error', String(e)) }
  try {
    const dbg = await invoke<unknown>('sotvault_vault_debug').catch((e) => ({ error: String(e) }))
    lines.push(`  backend debug: ${JSON.stringify(dbg)}`)
  } catch (e) { add('backend debug error', String(e)) }
  return lines
}

/** stat/read the file and build the virtual Tab shape shared by the share
 *  path and the generic plugin path. For image files content stays empty —
 *  downstream consumers (bakeShareHtml, uploadImage) re-read bytes via
 *  tauri-plugin-fs. On read failure, finishes with exit 2 and returns null. */
export async function buildVirtualTab(
  file: string,
  finish: ShareCliDeps['finish'],
): Promise<{ tab: Tab; extension: string | null; fileKind: FileKind } | null> {
  let fileContent = ''
  let fileMtime = 0
  try {
    const info = await stat(file)
    fileMtime = info.mtime ? new Date(info.mtime).getTime() : 0
    if (inferKind(extensionOf(basenameOf(file))) !== 'image') {
      fileContent = await readTextFile(file)
    }
  } catch (e) {
    await finish({ exit_code: 2, stderr: [`notemd: cannot read '${file}': ${e}`] })
    return null
  }
  const filename = basenameOf(file)
  const extension = extensionOf(filename)
  const fileKind = toFileKind(inferKind(extension))
  // Build a real Tab shape — share-baker reads filePath, currentContent, kind, title.
  const tab: Tab = {
    id: 'cli',
    filePath: file,
    title: filename,
    initialContent: fileContent,
    currentContent: fileContent,
    mode: 'source',
    kind: fileKind,
    externalState: 'fresh',
    externalBannerDismissed: false,
    lastKnownMtime: fileMtime,
    lastKnownHash: fileContent ? await sha256Hex(fileContent) : '',
  }
  return { tab, extension, fileKind }
}

export async function runShareCli(payload: CliPayload, deps: ShareCliDeps): Promise<void> {
  const { finish } = deps
  const systemDark = deps.systemDark
    ?? (() => globalThis.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false)
  const writeClipboard = deps.writeClipboard
    ?? (async (text: string) => { await clipWriteText(text) })
  const diagnostics = deps.diagnostics ?? shareVaultDiagnostics

  if (!payload.file) {
    await finish({ exit_code: 2, stderr: ['notemd: missing file argument'] })
    return
  }
  const file = payload.file

  /** Emit a share operation failure (exit 4). Writes JSON envelope to stdout
   *  when --json; always writes human message to stderr. */
  async function failShare(code: string, message: string, extraStderr: string[] = []): Promise<void> {
    const stderr = [`notemd: share failed: ${message}`, ...extraStderr]
    if (payload.global.json) {
      await finish({
        exit_code: 4,
        stdout: JSON.stringify({ ok: false, error: { code, message } }),
        stderr,
      })
    } else {
      await finish({ exit_code: 4, stderr })
    }
  }

  async function failNotConfigured(): Promise<void> {
    const msg = 'share not configured (baseUrl/apiKey)'
    if (payload.global.json) {
      await finish({
        exit_code: 4,
        stdout: JSON.stringify({ ok: false, error: { code: 'not_configured', message: msg } }),
        stderr: [`notemd: ${msg}`],
      })
    } else {
      await finish({ exit_code: 4, stderr: [`notemd: ${msg}`] })
    }
  }

  try {
    if (payload.plugin_command === 'copy-link') {
      // copy-link needs no config: it reads the local record. Clipboard is
      // gated the usual way: skip under --json and --no-clipboard.
      const rec = getRecord(file)
      const url = await copyShareLink(file, {
        clipboard: payload.global.clipboard && !payload.global.json,
      })
      const slug = rec && rec.kind !== 'image' ? rec.slug : undefined
      const data: Record<string, string> = { url }
      if (slug) data.slug = slug
      await finish({
        exit_code: 0,
        stdout: payload.global.json ? JSON.stringify({ ok: true, data }) : url,
        stderr: [],
      })
      return
    }

    if (payload.plugin_command === 'unpublish') {
      // Config check FIRST — matching the old binary, which never stat'ed the
      // file for unshare (it operates on the local record + DELETE call, not
      // file content).
      const { getShareConfig } = await import('../share')
      const cfg = getShareConfig()
      if (!cfg) return failNotConfigured()
      // Read record BEFORE unpublish so we capture slug (unpublish deletes it).
      const recBefore = getRecord(file)
      const slugBefore = recBefore && recBefore.kind !== 'image' ? recBefore.slug : undefined
      await unpublish({ path: file, baseUrl: cfg.baseUrl })
      const data: Record<string, unknown> = { removed: true }
      if (slugBefore) data.slug = slugBefore
      await finish({
        exit_code: 0,
        stdout: payload.global.json
          ? JSON.stringify({ ok: true, data })
          : `unshared ${file}`,
        stderr: [],
      })
      return
    }

    // 'publish' (default; --update maps here too).
    // stat/read the file BEFORE the config check: a nonexistent file must exit
    // 2 (file error, old contract) even when share is also unconfigured.
    const built = await buildVirtualTab(file, finish)
    if (!built) return
    const { tab, fileKind } = built

    const { getShareConfig } = await import('../share')
    const cfg = getShareConfig()
    if (!cfg) return failNotConfigured()

    if (fileKind === 'image') {
      const { url, isUpdate } = await uploadImage({
        path: file, filename: tab.title,
        baseUrl: cfg.baseUrl, defaultExpiry: cfg.defaultExpiry,
      })
      // slug not applicable for image shares (records use id/ext, not slug)
      if (payload.global.clipboard && !payload.global.json) await writeClipboard(url).catch(() => {})
      await finish({
        exit_code: 0,
        stdout: payload.global.json
          ? JSON.stringify({ ok: true, data: { url, is_update: isUpdate } })
          : url,
        stderr: [],
      })
      return
    }

    // Share via CLI runs the SAME vault-home pre-step as the menu (headless: no
    // flush — the file on disk is the source of truth). Fails the command with a
    // clear message when there's no vault to home the outside file into.
    let src: string
    try {
      // CLI 不走 GUI(App.svelte)的启动 refreshSotvault,故 sotvaultStore.vaultRoot
      // 一直是 null;prepareShareSrc 用它判 vault → 误报 vault_required。先加载。
      const { refreshSotvault } = await import('../sotvault.svelte')
      await refreshSotvault()
      const { prepareShareSrc } = await import('../share')
      src = await prepareShareSrc(file)
    } catch (e) {
      // 详细诊断:报错时列出读了哪个配置文件、sotvault 值、各层解析结果、文件路径
      // (原样打印,便于发现 Sync/sync 之类大小写不一致)。
      const msg = e instanceof Error ? e.message : String(e)
      const code = e instanceof ShareError ? e.kind : 'share_failed'
      const diagLines = await diagnostics(file)
      await failShare(code, msg, diagLines)
      return
    }

    const themeId = computeActiveThemeId(settings.theme, systemDark())
    // bakeShareHtml throws ShareError('too_large') itself when input/output
    // exceeds 25 MB — the outer catch maps it to exit 4 / code 'too_large'.
    const html = await bakeShareHtml(tab, themeId)
    if (!html) {
      await failShare('empty_content', 'empty_content')
      return
    }

    const { url, slug, isUpdate } = await publishHtml({
      path: file, filename: tab.title, html,
      baseUrl: cfg.baseUrl,
      defaultExpiry: cfg.defaultExpiry,
      slugRandomSuffix: cfg.slugRandomSuffix,
      src,
    })
    if (payload.global.clipboard && !payload.global.json) await writeClipboard(url).catch(() => {})
    // The old binary emitted created_at in the --json data; publishHtml just
    // wrote the record, so read it back (preserves the original created_at on
    // updates).
    const recAfter = getRecord(file)
    const createdAt = recAfter && recAfter.kind !== 'image' ? recAfter.created_at : undefined
    const data: Record<string, unknown> = { url, slug, is_update: isUpdate }
    if (createdAt) data.created_at = createdAt
    await finish({
      exit_code: 0,
      stdout: payload.global.json ? JSON.stringify({ ok: true, data }) : url,
      stderr: [],
    })
  } catch (e) {
    if (e instanceof ShareError) {
      await failShare(e.kind, `${e.kind}${e.detail ? ': ' + e.detail : ''}`)
    } else {
      await failShare('share_failed', String(e))
    }
  }
}
