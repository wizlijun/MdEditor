import { describe, it, expect, vi, beforeEach } from 'vitest'

const mockGet = vi.fn()
const mockSet = vi.fn()
const mockSave = vi.fn()
const mockDelete = vi.fn()

vi.mock('@tauri-apps/plugin-store', () => ({
  Store: { load: vi.fn(async () => ({ get: mockGet, set: mockSet, save: mockSave, delete: mockDelete })) },
}))

beforeEach(() => {
  vi.clearAllMocks()
  vi.resetModules()
  mockGet.mockResolvedValue(undefined)
})

describe('settings', () => {
  it('loadSettings hydrates autoSave from store, defaults to false', async () => {
    const { loadSettings, settings } = await import('./settings.svelte')
    mockGet.mockImplementation(async (key: string) => key === 'autoSave' ? true : undefined)
    await loadSettings()
    expect(settings.autoSave).toBe(true)
  })

  it('pushRecentFile prepends and de-dupes, capped at 10', async () => {
    const { pushRecentFile, getRecentFiles } = await import('./settings.svelte')
    for (let i = 0; i < 12; i++) await pushRecentFile(`/tmp/${i}.md`)
    await pushRecentFile('/tmp/3.md')  // existing → moves to front
    const list = getRecentFiles()
    expect(list.length).toBe(10)
    expect(list[0]).toBe('/tmp/3.md')
    expect(list).not.toContain('/tmp/0.md')
    expect(list).not.toContain('/tmp/1.md')
  })

  it('setRecentMode / getRecentMode round-trips by key (extension)', async () => {
    const { setRecentMode, getRecentMode } = await import('./settings.svelte')
    await setRecentMode('md', 'rich')
    expect(getRecentMode('md')).toBe('rich')
    expect(getRecentMode('html')).toBe(null)
  })

  it('loadSettings hydrates recentModesByExt and getRecentMode reads it', async () => {
    const stored = { md: 'rich', py: 'source' }
    mockGet.mockImplementation(async (key: string) =>
      key === 'recentModesByExt' ? stored : undefined,
    )
    const { loadSettings, getRecentMode } = await import('./settings.svelte')
    await loadSettings()
    expect(getRecentMode('md')).toBe('rich')
    expect(getRecentMode('py')).toBe('source')
    expect(getRecentMode('json')).toBe(null)
  })

  it('loadSettings hydrates toastAutoClose from store, defaults to false', async () => {
    const { loadSettings, settings } = await import('./settings.svelte')
    mockGet.mockImplementation(async (key: string) =>
      key === 'toastAutoClose' ? true : undefined,
    )
    await loadSettings()
    expect(settings.toastAutoClose).toBe(true)
  })

  it('loadSettings defaults toastAutoClose to false when missing', async () => {
    const { loadSettings, settings } = await import('./settings.svelte')
    await loadSettings()
    expect(settings.toastAutoClose).toBe(false)
  })

  it('saveSettings persists toastAutoClose', async () => {
    const { loadSettings, saveSettings, settings } = await import('./settings.svelte')
    await loadSettings()
    settings.toastAutoClose = true
    await saveSettings()
    expect(mockSet).toHaveBeenCalledWith('toastAutoClose', true)
  })

})

describe('recent files: opened-at, tombstones, removal', () => {
  it('pushRecentFile records a lastOpened timestamp for the path', async () => {
    const { pushRecentFile, getRecentOpenedAt } = await import('./settings.svelte')
    const before = Date.now()
    await pushRecentFile('/tmp/a.md')
    const ts = getRecentOpenedAt()['/tmp/a.md']
    expect(ts).toBeGreaterThanOrEqual(before)
  })

  it('removeRecentFile drops the path, clears its timestamp, and tombstones it', async () => {
    const { pushRecentFile, removeRecentFile, getRecentFiles, getRecentOpenedAt, getRecentTombstones } =
      await import('./settings.svelte')
    await pushRecentFile('/tmp/a.md')
    await pushRecentFile('/tmp/b.md')
    await removeRecentFile('/tmp/a.md')
    expect(getRecentFiles()).not.toContain('/tmp/a.md')
    expect(getRecentFiles()).toContain('/tmp/b.md')
    expect(getRecentOpenedAt()['/tmp/a.md']).toBeUndefined()
    expect(getRecentTombstones()).toContain('/tmp/a.md')
  })

  it('loadSettings hydrates recentOpenedAt and recentTombstones', async () => {
    mockGet.mockImplementation(async (key: string) => {
      if (key === 'recentOpenedAt') return { '/tmp/x.md': 123 }
      if (key === 'recentTombstones') return ['/tmp/gone.md']
      return undefined
    })
    const { loadSettings, getRecentOpenedAt, getRecentTombstones } = await import('./settings.svelte')
    await loadSettings()
    expect(getRecentOpenedAt()['/tmp/x.md']).toBe(123)
    expect(getRecentTombstones()).toContain('/tmp/gone.md')
  })

  it('getDeviceId generates and persists an id when absent', async () => {
    mockGet.mockResolvedValue(undefined)
    const { loadSettings, getDeviceId } = await import('./settings.svelte')
    await loadSettings()
    const id = getDeviceId()
    expect(typeof id).toBe('string')
    expect(id.length).toBeGreaterThan(0)
    expect(mockSet.mock.calls.some((a) => a[0] === 'device.id')).toBe(true)
  })

  it('setRecentsChangedHandler is invoked on pushRecentFile', async () => {
    const { setRecentsChangedHandler, pushRecentFile } = await import('./settings.svelte')
    const fn = vi.fn()
    setRecentsChangedHandler(fn)
    await pushRecentFile('/tmp/z.md')
    expect(fn).toHaveBeenCalled()
  })
})

describe('theme settings', () => {
  it('migrates legacy `skin: "effie"` into theme.{light,dark,followSystem:false}', async () => {
    mockGet.mockImplementation(async (key: string) => {
      if (key === 'skin') return 'effie'
      if (key === 'theme') return undefined
      return undefined
    })
    const { loadSettings, settings } = await import('./settings.svelte')
    await loadSettings()
    expect(settings.theme).toEqual({
      light: 'effie',
      dark: 'effie',
      followSystem: false,
    })
    const deleteCall = mockSet.mock.calls.find((args) => args[0] === 'skin')
    // The migration should also delete the legacy `skin` key on save.
    expect(deleteCall).toBeUndefined()
  })

  it('respects existing theme settings when present', async () => {
    const stored = { light: 'default', dark: 'effie', followSystem: true }
    mockGet.mockImplementation(async (key: string) =>
      key === 'theme' ? stored : undefined,
    )
    const { loadSettings, settings } = await import('./settings.svelte')
    await loadSettings()
    expect(settings.theme).toEqual(stored)
  })

  it('defaults to {light:"default", dark:"default", followSystem:true} when nothing stored', async () => {
    mockGet.mockResolvedValue(undefined)
    const { loadSettings, settings } = await import('./settings.svelte')
    await loadSettings()
    expect(settings.theme).toEqual({
      light: 'default',
      dark: 'default',
      followSystem: true,
    })
  })

  it('persists theme via saveSettings', async () => {
    mockGet.mockResolvedValue(undefined)
    const { loadSettings, saveSettings, settings } = await import('./settings.svelte')
    await loadSettings()
    settings.theme = { light: 'effie', dark: 'default', followSystem: false }
    await saveSettings()
    const setCall = mockSet.mock.calls.find((args) => args[0] === 'theme')
    expect(setCall?.[1]).toEqual({ light: 'effie', dark: 'default', followSystem: false })
  })
})

describe('plugin-scoped settings', () => {
  it('loads plugin-scoped keys from the store', async () => {
    mockGet.mockImplementation(async (k: string) => {
      if (k === 'plugins') return { share: { baseUrl: 'https://x', records: { a: 1 } } }
      return undefined
    })
    const { loadSettings, getPluginScopedAll } = await import('./settings.svelte')
    await loadSettings()
    expect(getPluginScopedAll('share')).toEqual({ 'share.baseUrl': 'https://x', 'share.records': { a: 1 } })
  })

  it('returns empty object for unknown plugin', async () => {
    mockGet.mockResolvedValue(undefined)
    const { loadSettings, getPluginScopedAll } = await import('./settings.svelte')
    await loadSettings()
    expect(getPluginScopedAll('mystery')).toEqual({})
  })

  it('mergePluginScoped routes share.records to shareDb and updates other keys', async () => {
    mockGet.mockResolvedValue({ share: { records: { a: 1 } } })
    const { loadSettings, getPluginScopedAll, mergePluginScoped } = await import('./settings.svelte')
    await loadSettings()
    await mergePluginScoped({ 'share.records': { b: 2 }, 'share.baseUrl': 'https://y' })
    const all = getPluginScopedAll('share')
    expect(all['share.baseUrl']).toBe('https://y')
    expect(all['share.records']).toEqual({ b: 2 })
    // share.records should NOT be persisted in the main 'plugins' store key
    const setCall = mockSet.mock.calls.find((args) => args[0] === 'plugins')
    expect(setCall?.[1]).toEqual({
      share: { records: { a: 1 }, baseUrl: 'https://y' },
    })
  })

  it('preserves untouched keys at the same level (only patched keys are replaced)', async () => {
    mockGet.mockResolvedValue({ share: { baseUrl: 'https://x', apiKey: 'secret' } })
    const { loadSettings, getPluginScopedAll, mergePluginScoped } = await import('./settings.svelte')
    await loadSettings()
    await mergePluginScoped({ 'share.baseUrl': 'https://y' })
    const all = getPluginScopedAll('share')
    expect(all['share.baseUrl']).toBe('https://y')
    expect(all['share.apiKey']).toBe('secret')
  })
})
