/**
 * @vitest-environment happy-dom
 */
import { describe, it, expect, vi, afterEach } from 'vitest'
import { loadSurfaceConfig, watchSurfaceFocus } from './power-mode-config'

function stubBridge(pluginId: string, result: unknown) {
  const request = vi.fn().mockResolvedValue(result)
  ;(window as any).notemd = { pluginId, locale: 'zh', theme: 'x', request, onMessage() {} }
  return request
}

describe('loadSurfaceConfig', () => {
  afterEach(() => { delete (window as any).notemd })

  it('returns null with no bridge (kit mounted outside a plugin window)', async () => {
    expect(await loadSurfaceConfig()).toBeNull()
  })

  it('returns null when the host reports the plugin is off', async () => {
    stubBridge('notemd.idea-spark', { config: null, surfaces: [] })
    expect(await loadSurfaceConfig()).toBeNull()
  })

  it('defaults an installed-but-unconfigured host to enabled for a plugin window', async () => {
    stubBridge('notemd.idea-spark', { config: {}, surfaces: [] })
    const cfg = await loadSurfaceConfig()
    expect(cfg).not.toBeNull()
    expect(cfg!.explosion.presetId).toBe('particle')
  })

  it('honours an explicit off flag for this very window', async () => {
    stubBridge('notemd.idea-spark', { config: { surfaces: { 'notemd.idea-spark': false } }, surfaces: [] })
    expect(await loadSurfaceConfig()).toBeNull()
  })

  it('returns null instead of throwing when the RPC fails', async () => {
    const request = vi.fn().mockRejectedValue(new Error('nope'))
    ;(window as any).notemd = { pluginId: 'x', locale: 'en', theme: 'y', request, onMessage() {} }
    expect(await loadSurfaceConfig()).toBeNull()
  })
})

describe('watchSurfaceFocus', () => {
  it('reloads on window focus and stops after unsubscribe', () => {
    const reload = vi.fn()
    const off = watchSurfaceFocus(reload)
    window.dispatchEvent(new Event('focus'))
    expect(reload).toHaveBeenCalledTimes(1)
    off()
    window.dispatchEvent(new Event('focus'))
    expect(reload).toHaveBeenCalledTimes(1)
  })
})
