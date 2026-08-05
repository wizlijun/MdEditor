// src/lib/bridge.ts — typed accessor for the host-injected `window.notemd`
// fetch-RPC bridge (see src-tauri/src/plugin_runtime/windows.rs bridge_script).
//
// A plugin window has ZERO Tauri IPC; every host effect goes through
// `notemd.request(method, params)`, which POSTs to `plugin://<id>/__rpc__` and
// resolves with the method's `result` (or throws on `error`).

export interface NotemdBridge {
  pluginId: string
  /** BCP-ish locale code the host resolved from settings: 'en' | 'zh' | 'ja' | 'de'. */
  locale: string
  /** Active UI theme id (unused here; color-scheme handles appearance). */
  theme: string
  request(method: string, params?: unknown): Promise<any>
  onMessage(cb: (payload: unknown) => void): void
}

declare global {
  interface Window { notemd: NotemdBridge }
}

export function bridge(): NotemdBridge {
  const b = window.notemd
  if (!b) throw new Error('window.notemd bridge missing (not running inside a plugin window)')
  return b
}

export interface SurfaceEntry {
  id: string
  /** manifest 的英文名。 */
  name: string
  /** locale → 本地化名。 */
  names: Record<string, string>
}

export interface PowerModeConfigPayload {
  /** null = 插件没装/停用(理论上打不开本窗口);{} = 装了但没配过。 */
  config: Record<string, unknown> | null
  surfaces: SurfaceEntry[]
}

/** `host.power_mode.config` → 生效配置 + 可配置生效面清单。 */
export function loadPowerMode(): Promise<PowerModeConfigPayload> {
  return bridge().request('host.power_mode.config')
}

/**
 * `host.power_mode.update` —— 宿主 emit 给主窗口前端,由它落进 settings.json 的
 * 插件域。settings store 是主窗口独家持有的,所以写入必须绕这一圈。
 */
export function savePowerMode(config: unknown): Promise<{ ok: true }> {
  return bridge().request('host.power_mode.update', { config })
}
