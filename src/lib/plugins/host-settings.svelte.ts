// Keep the main window's plugin-settings mirror coherent with writes made by
// isolated plugin windows through host.settings.set. Rust has already saved the
// value by the time this event arrives; this listener only updates memory.
import { listen } from '@tauri-apps/api/event'
import { applyPluginScopedExternalValue } from '../settings.svelte'

interface PluginSettingsChanged {
  plugin_id: string
  key: string
  value: unknown
}

function isChange(value: unknown): value is PluginSettingsChanged {
  if (!value || typeof value !== 'object') return false
  const p = value as Partial<PluginSettingsChanged>
  return typeof p.plugin_id === 'string' && p.plugin_id.length > 0
    && typeof p.key === 'string' && p.key.length > 0
    && Object.prototype.hasOwnProperty.call(p, 'value')
}

export async function initPluginSettingsHost(): Promise<void> {
  await listen<unknown>('plugin-settings://changed', (event) => {
    if (!isChange(event.payload)) return
    applyPluginScopedExternalValue(event.payload.plugin_id, event.payload.key, event.payload.value)
  })
}
