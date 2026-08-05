// Kit 侧的 Power Mode 配置源。
//
// 与主窗口的 host-config.svelte.ts 是两条独立通道:插件 webview 没有 Tauri IPC,
// 只能走 window.notemd 桥。这个文件因此不 import 任何 @tauri-apps/*。
import { normalizeConfig, isSurfaceEnabled } from '../lib/power-mode/config'
import type { PowerModeConfig } from '../lib/power-mode/types'

interface HostBridge {
  pluginId: string
  request(method: string, params?: unknown): Promise<unknown>
}

function bridge(): HostBridge | null {
  const b = (window as unknown as { notemd?: HostBridge }).notemd
  return b && typeof b.request === 'function' ? b : null
}

/**
 * 本窗口该用的配置,已算过生效面;不该开时返回 null。
 *
 * 任何失败(宿主太老没这条 RPC、插件没声明 editor.kit、桥不在)都降级成 null:
 * 特效是装饰,不该把编辑器拖下水。
 */
export async function loadSurfaceConfig(): Promise<PowerModeConfig | null> {
  const b = bridge()
  if (!b) return null
  try {
    const res = await b.request('host.power_mode.config')
    const raw = (res as { config?: unknown } | null)?.config
    if (raw === null || raw === undefined) return null
    const cfg = normalizeConfig(raw)
    return isSurfaceEnabled(cfg, b.pluginId) ? cfg : null
  } catch {
    return null
  }
}

/**
 * 插件窗口收不到 Tauri 的 `settings://changed` 广播(没有 IPC),所以用「窗口重新
 * 获得焦点」当刷新时机:用户在 Power Mode 设置窗口改完,切回来就是新的。
 */
export function watchSurfaceFocus(reload: () => void): () => void {
  window.addEventListener('focus', reload)
  return () => window.removeEventListener('focus', reload)
}
