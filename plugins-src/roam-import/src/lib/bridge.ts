// src/lib/bridge.ts — typed accessor for the host-injected `window.notemd`
// fetch-RPC bridge (see src-tauri/src/plugin_runtime/windows.rs bridge_script).
//
// A plugin window has ZERO Tauri IPC; every host effect goes through
// `notemd.request(method, params)`, which POSTs to `plugin://<id>/__rpc__` and
// resolves with the method's `result` (or throws on `error`).

/** The bridge surface the host injects as an initialization script. */
export interface NotemdBridge {
  pluginId: string
  /** BCP-ish locale code the host resolved from settings: 'en' | 'zh' | 'ja' | 'de'. */
  locale: string
  /** Active UI theme id (unused by this plugin; color-scheme handles appearance). */
  theme: string
  /** Call a host method; resolves with its result, rejects with an Error on RPC error. */
  request(method: string, params?: unknown): Promise<any>
  /** Subscribe to host→UI pushes (unused by roam-import; present for completeness). */
  onMessage(cb: (payload: unknown) => void): void
}

declare global {
  interface Window {
    notemd: NotemdBridge
  }
}

/** The injected bridge. Throws if accessed outside a host plugin window. */
export function bridge(): NotemdBridge {
  const b = window.notemd
  if (!b) throw new Error('window.notemd bridge missing (not running inside a plugin window)')
  return b
}

// ── host method result shapes (subset this plugin consumes) ──────────────────

export interface VaultInfo {
  root: string | null
  wiki_dir: string | null
  daily_dir: string | null
}

/** `host.vault.info` → root + configured wiki/daily dir names. */
export function vaultInfo(): Promise<VaultInfo> {
  return bridge().request('host.vault.info')
}

/**
 * `host.dialog.open` → `{ paths }` (null when the user cancelled). `filterName`
 * is the localized label the native file picker shows for the extension group
 * (e.g. macOS's file-type dropdown) — callers pass `t('dialog.filter')` so it
 * isn't hardcoded English here (this module owns no UI text of its own).
 */
export async function dialogOpenJson(title: string | undefined, filterName: string): Promise<string | null> {
  const res: { paths: string[] | null } = await bridge().request('host.dialog.open', {
    title,
    multiple: false,
    // Roam's "Export All (JSON)" downloads a .zip; a manually-unzipped .json is
    // also accepted. io.readRoamExport branches on the picked extension.
    filters: [{ name: filterName, extensions: ['json', 'zip'] }],
  })
  return res.paths?.[0] ?? null
}

/**
 * `host.fs.read_text` → file content. Only paths a prior `host.dialog.open`
 * returned this session are readable (fs.read:dialog grant).
 */
export async function fsReadText(path: string): Promise<string> {
  const res: { content: string } = await bridge().request('host.fs.read_text', { path })
  return res.content
}

/**
 * `host.fs.read_bytes` → raw file bytes (base64-decoded here). Only paths a
 * prior `host.dialog.open` returned this session are readable (fs.read:dialog
 * grant). Used for binary exports (Roam's `.zip`) the UTF-8 text bridge cannot
 * carry.
 */
export async function fsReadBytes(path: string): Promise<Uint8Array> {
  const res: { base64: string } = await bridge().request('host.fs.read_bytes', { path })
  const bin = atob(res.base64)
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
  return bytes
}

/** `host.vault.read` → file content (vault-relative path). */
export async function vaultRead(path: string): Promise<string> {
  const res: { content: string } = await bridge().request('host.vault.read', { path })
  return res.content
}

/** `host.vault.write` — writes text, creating parent dirs (vault-relative path). */
export async function vaultWrite(path: string, content: string): Promise<void> {
  await bridge().request('host.vault.write', { path, content })
}

/** `host.vault.exists` → whether a vault-relative path exists. */
export async function vaultExists(path: string): Promise<boolean> {
  const res: { exists: boolean } = await bridge().request('host.vault.exists', { path })
  return res.exists
}

// Note: host.vault.write creates parent directories itself, so this plugin never
// needs an explicit host.vault.mkdir (the capability is still declared for it).

/** `host.clipboard.write` — copy text to the OS clipboard. */
export async function clipboardWrite(text: string): Promise<void> {
  await bridge().request('host.clipboard.write', { text })
}

/** `host.toast` — surface a message through the host toast system. */
export async function toast(
  level: 'success' | 'info' | 'warn' | 'error',
  message: string,
  detail?: string,
): Promise<void> {
  try {
    await bridge().request('host.toast', { level, message, detail })
  } catch {
    /* toast is best-effort */
  }
}

/** Call this plugin's OWN backend (`on_ui_request`). The host strips the
 *  `plugin.` prefix before forwarding (ui_rpc.rs:258). */
export function pluginRequest(method: string, params?: unknown): Promise<any> {
  return bridge().request(`plugin.${method}`, params)
}

/** `plugin.probe`'s three-state read on the local `roam` CLI. */
export type ProbeState = 'missing' | 'not_connected' | 'ready'
export interface RoamProbe {
  state: ProbeState
  found: string | null
  version: string | null
  graphs: string[]
}

/** `plugin.sync_day`'s result. `found: false` means Roam had no daily page
 *  for `date` and nothing was written — distinct from a zero-block sync. */
export interface SyncOutcome {
  date: string
  path: string
  found: boolean
  created: number
  updated: number
  kept_local: number
  roam_gone_kept: number
  /** Blocks an earlier, id-less import wrote that this sync re-keyed to their
   *  Roam uid (backend `adopt.rs`). Non-zero only on the run that repairs a
   *  note; carried here so the type matches what the backend sends, not to be
   *  shown — the trace it exists for is the plugin log. */
  adopted: number
}

/** `plugin.probe` — three-state read of the local `roam` CLI: not installed,
 *  installed but no graph connected yet, or ready with a version + graphs. */
export function probe(roamPath?: string): Promise<RoamProbe> {
  return pluginRequest('probe', roamPath ? { roam_path: roamPath } : {})
}

/** `plugin.sync_day` — sync one day's Roam daily note into the vault. */
export function syncDay(date: string, opts?: { graph?: string; roamPath?: string }): Promise<SyncOutcome> {
  return pluginRequest('sync_day', {
    date,
    ...(opts?.graph ? { graph: opts.graph } : {}),
    ...(opts?.roamPath ? { roam_path: opts.roamPath } : {}),
  })
}

/** One page whose Roam title changed since the last sync, and the file move
 *  that followed (backend `incremental::Renamed`). `[[wikilink]]`s elsewhere
 *  in the vault still point at the old name — this is the one place the user
 *  learns which files moved. */
export interface SyncRenamed {
  uid: string
  from: string
  to: string
}

/** One page the run placed, and where (backend `incremental::Planned`).
 *
 *  This is what makes `--dry-run` and the window's pre-flight answer the
 *  question they are asked — *which* pages, at *which* paths — rather than
 *  just how many. `wrote` is "this changed on disk" after a real run, and
 *  "a real run would deal with this one" after a dry run (which writes and
 *  compares nothing, so it declines to guess).
 *
 *  Pages Roam no longer has, and blockless tag pages, have no target path and
 *  so are not listed — `pages.length` is deliberately not `scanned`. */
export interface SyncPlanned {
  uid: string
  title: string
  rel: string
  wrote: boolean
}

/** `plugin.sync_since`'s result (backend `incremental::SyncReport`, plus the
 * `ok` flag `sync_report_value` stamps on it).
 *
 * Resolved for BOTH a clean and a not-clean run. `ok` — not `failed === 0` —
 * is what says the run was clean: `failed` counts only the one page that
 * stopped the run outright, while an unreadable ledger or a rename the sync
 * refused to perform report `failed === 0` with a non-empty `errors` on
 * purpose. A report with `ok === false` must be shown as a problem, never as
 * a success banner.
 *
 * It resolves rather than rejecting so the window can show what the run *did*
 * next to what went wrong. The CLI is the one that turns a not-clean run into
 * a rejection, because exit 4 is the only "not clean" the host's generic CLI
 * layer can express (`cli_sync_outcome` in `plugin.rs`).
 *
 * A rejected `syncSince` therefore means the run never produced a report at
 * all — no vault, no `roam` CLI, an unsafe folder name, or a `--graph` that
 * disagrees with the ledger's. */
export interface SyncReport {
  /** `errors.length === 0`. See above: this, not `failed`, is "clean". */
  ok: boolean
  from: string | null
  to: string | null
  scanned: number
  synced: number
  skipped: number
  failed: number
  pages: SyncPlanned[]
  renamed: SyncRenamed[]
  errors: string[]
  dry_run: boolean
}

/** `plugin.sync_since` — sync everything changed since the ledger's
 *  watermark (or `since`, when given, which overrides it for one run without
 *  moving the watermark backwards). Resolves with a report whether or not the
 *  run was clean; check `ok`. Rejects only when there is no report to give —
 *  see `SyncReport`. */
export function syncSince(opts?: {
  since?: string
  graph?: string
  roamPath?: string
  dryRun?: boolean
}): Promise<SyncReport> {
  return pluginRequest('sync_since', {
    ...(opts?.since ? { since: opts.since } : {}),
    ...(opts?.graph ? { graph: opts.graph } : {}),
    ...(opts?.roamPath ? { roam_path: opts.roamPath } : {}),
    ...(opts?.dryRun ? { dry_run: opts.dryRun } : {}),
  })
}

/** `plugin.sync_status`'s result: the ledger's last-synced watermark, or
 *  `null` when there has never been a sync (backend `sync_status`, ledger-only
 *  — no fetch, safe to call on mount). */
export interface SyncStatus {
  last_synced_at: string | null
}

/** `plugin.sync_status` — read the ledger's last-synced timestamp without
 *  triggering a sync. */
export function syncStatus(): Promise<SyncStatus> {
  return pluginRequest('sync_status')
}
