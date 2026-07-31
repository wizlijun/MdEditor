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

/** `host.dialog.open` → `{ paths }` (null when the user cancelled). */
export async function dialogOpenJson(title?: string): Promise<string | null> {
  const res: { paths: string[] | null } = await bridge().request('host.dialog.open', {
    title,
    multiple: false,
    // Roam's "Export All (JSON)" downloads a .zip; a manually-unzipped .json is
    // also accepted. io.readRoamExport branches on the picked extension.
    filters: [{ name: 'Roam export', extensions: ['json', 'zip'] }],
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
