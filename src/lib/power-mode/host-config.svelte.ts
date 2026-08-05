// 主窗口专用的 Power Mode 配置源。
//
// ⚠️ 这个文件碰 Tauri IPC,Editor Kit **绝不能** import 它 —— 插件 webview 没有
// IPC,引入即炸掉整个 kit。Kit 侧的配置源是 src/editor-kit/power-mode-config.ts。
import { listen } from '@tauri-apps/api/event'
import { getPluginScopedValue, setPluginScopedValue } from '../settings.svelte'
import { normalizeConfig, isSurfaceEnabled } from './config'
import type { PowerModeConfig } from './types'

export const POWER_MODE_PLUGIN_ID = 'notemd.power-mode'
const CONFIG_KEY = 'config'

/** null = 插件没装/停用,或配置从未写过。 */
let cached: PowerModeConfig | null = null

/** 主编辑窗口该用的配置;生效面关着时返回 null。 */
export function mainWindowConfig(): PowerModeConfig | null {
  return isSurfaceEnabled(cached, 'main') ? cached : null
}

function hydrate(): void {
  const raw = getPluginScopedValue(POWER_MODE_PLUGIN_ID, CONFIG_KEY)
  cached = raw === undefined ? null : normalizeConfig(raw)
}

/**
 * 启动时调一次。
 *
 * 插件窗口没有 Tauri IPC,它的写入走 `host.power_mode.update` → 宿主 emit
 * `power-mode://update` → 这里落盘。settings.json 由主窗口独家持有,所以写入
 * 必须回到这一侧,不能让 Rust 直接改文件。
 */
export async function initPowerModeHost(): Promise<void> {
  hydrate()
  await listen<unknown>('power-mode://update', async (e) => {
    const next = normalizeConfig(e.payload)
    cached = next
    try {
      await setPluginScopedValue(POWER_MODE_PLUGIN_ID, CONFIG_KEY, next)
    } catch (err) {
      console.warn('[power-mode] persist failed:', err)
    }
  })
}
