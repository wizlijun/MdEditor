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
  /** Subscribe to host→UI pushes (unused by this plugin; present for completeness). */
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

/** `host.vault.read` → file content (vault-relative path). */
export function vaultRead(path: string): Promise<{ content: string }> {
  return bridge().request('host.vault.read', { path })
}

/** `host.vault.write` — writes text, creating parent dirs (vault-relative path). */
export function vaultWrite(path: string, content: string): Promise<{ ok: true }> {
  return bridge().request('host.vault.write', { path, content })
}

/** `host.vault.exists` → whether a vault-relative path exists. */
export function vaultExists(path: string): Promise<{ exists: boolean }> {
  return bridge().request('host.vault.exists', { path })
}

/** `host.vault.list` → directory entries (vault-relative path). */
export function vaultList(path: string): Promise<{ entries: { name: string; is_dir: boolean }[] }> {
  return bridge().request('host.vault.list', { path })
}

/**
 * `host.vault.remove` — deletes ONE file (vault-relative path).
 *
 * Host semantics worth knowing at the call site: a directory is refused (only
 * `remove_file` is ever called), a path that doesn't exist resolves as success
 * (idempotent, so a retry after a partial failure is safe), and a symlink is
 * removed as the link itself — never followed to its target.
 */
export function vaultRemove(path: string): Promise<{ ok: true }> {
  return bridge().request('host.vault.remove', { path })
}

/**
 * `host.vault.rename` — moves a file within the vault (both ends vault-relative).
 *
 * NEVER clobbers: an existing `to` rejects with an "exists" error and leaves
 * both ends untouched (the host does the check atomically), so a caller may
 * treat rejection as "that name is spoken for" without racing anything.
 * Missing parent directories of `to` are created.
 */
export function vaultRename(from: string, to: string): Promise<{ ok: true }> {
  return bridge().request('host.vault.rename', { from, to })
}
