// src/lib/bridge.ts — typed accessor for the host-injected `window.notemd`
// bridge (see src-tauri/src/plugin_runtime/windows.rs bridge_script).
//
// A plugin window has ZERO Tauri IPC. Two channels:
//  1. UI → plugin process: `request(method, params)`. When `method` is NOT a
//     `host.*` name, the host forwards it to THIS plugin's backend process as
//     `ui.request{method,params}`. We prefix every backend call with `plugin.`
//     so the host routes it to the process (which strips the prefix).
//  2. Plugin process → UI: `onMessage(cb)` receives every `host.ui.post`
//     payload the backend pushes.

/** The bridge surface the host injects as an initialization script. */
export interface NotemdBridge {
  pluginId: string
  /** Locale the host resolved from settings: 'en' | 'zh' | 'ja' | 'de'. */
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

/** The injected bridge. Throws if accessed outside a host plugin window. */
export function bridge(): NotemdBridge {
  const b = window.notemd
  if (!b) throw new Error('window.notemd bridge missing (not running inside a plugin window)')
  return b
}

/**
 * Call a backend method. The `plugin.` prefix makes the host route it to THIS
 * plugin's process, which dispatches the clean name via `on_ui_request`
 * (tasks.list / context.get / run.start / run.cancel / history.list).
 */
export function request(method: string, params?: unknown): Promise<any> {
  return bridge().request('plugin.' + method, params)
}

/**
 * `host.editor.open` — open a vault-relative file in the main editor window.
 * Note the missing `plugin.` prefix: `host.*` methods are answered by the host
 * bridge itself. The backend process CANNOT do this — editor.open isn't on the
 * process channel's dispatch table (host_api.rs:176-183), only the window's.
 */
export function openInEditor(path: string): Promise<unknown> {
  return bridge().request('host.editor.open', { path })
}

/** Subscribe to every backend push (the `host.ui.post` payloads). */
export function onMessage(cb: (m: any) => void): void {
  bridge().onMessage((payload) => cb(payload))
}
