export interface NotemdBridge {
  pluginId: string
  locale: string
  theme: string
  request(method: string, params?: unknown): Promise<any>
  onMessage(cb: (payload: unknown) => void): void
}

declare global {
  interface Window {
    notemd: NotemdBridge
  }
}

export function bridge(): NotemdBridge {
  const value = window.notemd
  if (!value) throw new Error('window.notemd bridge missing (not running inside a plugin window)')
  return value
}

export interface VaultEntry {
  name: string
  is_dir: boolean
}

export function vaultRead(path: string): Promise<{ content: string }> {
  return bridge().request('host.vault.read', { path })
}

export function vaultWrite(path: string, content: string): Promise<{ ok: true }> {
  return bridge().request('host.vault.write', { path, content })
}

export function vaultRename(from: string, to: string): Promise<{ ok: true }> {
  return bridge().request('host.vault.rename', { from, to })
}

export function vaultRemove(path: string): Promise<{ ok: true }> {
  return bridge().request('host.vault.remove', { path })
}

export function vaultExists(path: string): Promise<{ exists: boolean }> {
  return bridge().request('host.vault.exists', { path })
}

export function vaultList(path: string): Promise<{ entries: VaultEntry[] }> {
  return bridge().request('host.vault.list', { path })
}

export function editorOpen(path: string): Promise<{ ok: true }> {
  return bridge().request('host.editor.open', { path })
}

export async function toast(
  level: 'success' | 'info' | 'warn' | 'error',
  message: string,
  detail?: string,
): Promise<void> {
  try {
    await bridge().request('host.toast', { level, message, detail })
  } catch {
    // A toast must never turn a successful state transition into a failure.
  }
}
