// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, unmount } from 'svelte'
import type { InstalledV2, RegistryEntry, RegistryIndex } from './lib/market/types'
import { readInstalledCache, writeInstalledCache } from './lib/market/cache'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ setTitle: vi.fn(async () => {}) }),
}))
vi.mock('@tauri-apps/api/app', () => ({ getVersion: vi.fn(async () => '6.829.2') }))
vi.mock('@tauri-apps/plugin-dialog', () => ({ confirm: vi.fn(async () => true) }))
vi.mock('./lib/settings.svelte', () => ({ loadSettings: vi.fn(async () => {}) }))
vi.mock('./lib/toast.svelte', () => ({ pushToast: vi.fn() }))
vi.mock('./lib/i18n/store.svelte', () => {
  const labels: Record<string, string> = {
    'pluginMarket.windowTitle': 'Plugin Market',
    'pluginMarket.subtitle': 'Browse plugins',
    'pluginMarket.refresh': 'Refresh',
    'pluginMarket.pluginsUnit': 'plugins',
    'pluginMarket.loadingCatalog': 'Checking for more plugins…',
    'pluginMarket.installedHeading': 'Installed',
    'pluginMarket.availableHeading': 'Available',
    'pluginMarket.enabled': 'Enabled',
    'pluginMarket.disabled': 'Disabled',
    'pluginMarket.updateAvailable': 'Update available',
    'pluginMarket.onDevice': 'Installed on this device.',
    'pluginMarket.noneAvailable': 'No plugins available.',
    'pluginMarket.noneInstalled': 'No plugins installed.',
    'pluginMarket.localStateError': 'Could not refresh installed plugins: {error}',
    'pluginMarket.networkError': 'Could not reach the plugin registry: {error}',
    'pluginCategory.thinking': 'Thinking',
    'capability.vault.read': 'Read files',
  }
  return {
    loadLocale: vi.fn(async () => {}),
    watchLocaleChanges: vi.fn(async () => () => {}),
    t: (key: string, params?: Record<string, string | number>) => {
      let value = labels[key] ?? key
      for (const [name, replacement] of Object.entries(params ?? {})) {
        value = value.replace(`{${name}}`, String(replacement))
      }
      return value
    },
  }
})

import PluginMarketApp from './plugin-market-app.svelte'

let component: ReturnType<typeof mount> | null = null

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej })
  return { promise, resolve, reject }
}

function installed(name: string, enabled = true): InstalledV2 {
  return {
    id: 'notemd.next',
    version: '1.0.0',
    enabled,
    name,
    category: 'thinking',
    capabilities: ['vault.read'],
  }
}

function entry(id: string, name: string, version = '1.0.0'): RegistryEntry {
  return {
    id,
    version,
    min_host: '>=6.0.0',
    archs: ['universal'],
    size: 1,
    sha256: { universal: 'aa' },
    name,
    category: 'thinking',
    description: `${name} description`,
    download: { universal: `https://plugins.notemd.net/${id}.zip` },
  }
}

beforeEach(() => {
  localStorage.clear()
  mocks.invoke.mockReset()
})

afterEach(async () => {
  if (component) await unmount(component)
  component = null
  document.body.innerHTML = ''
  localStorage.clear()
})

describe('plugin market staged loading', () => {
  it('shows cached installed plugins first, then device state, then the full catalog', async () => {
    writeInstalledCache([installed('Cached Next', false)])
    const local = deferred<InstalledV2[]>()
    const registry = deferred<RegistryIndex>()
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'plugin_market_installed') return local.promise
      if (command === 'plugin_market_index') return registry.promise
      return Promise.resolve()
    })

    component = mount(PluginMarketApp, { target: document.body })
    await vi.waitFor(() => expect(document.body.textContent).toContain('Cached Next'))
    expect(document.body.textContent).not.toContain('Idea Spark')

    local.resolve([installed('Next')])
    await vi.waitFor(() => expect(
      [...document.querySelectorAll('.plugin-title h3')].some((node) => node.textContent === 'Next'),
    ).toBe(true))
    expect(document.body.textContent).not.toContain('Cached Next')
    expect(document.body.textContent).not.toContain('Idea Spark')

    registry.resolve({
      plugins: [entry('notemd.next', 'Next', '1.1.0'), entry('notemd.idea-spark', 'Idea Spark')],
    })
    await vi.waitFor(() => expect(document.body.textContent).toContain('Idea Spark'))
    expect(document.body.textContent).toContain('Update available')
    expect(document.querySelectorAll('.category-block[data-category="thinking"]')).toHaveLength(1)
    expect(document.querySelectorAll('.category-block[data-category="thinking"] .plugin-card')).toHaveLength(2)
  })

  it('keeps authoritative installed plugins visible when the registry fails', async () => {
    const registry = deferred<RegistryIndex>()
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'plugin_market_installed') return Promise.resolve([installed('Device Next', false)])
      if (command === 'plugin_market_index') return registry.promise
      return Promise.resolve()
    })

    component = mount(PluginMarketApp, { target: document.body })
    await vi.waitFor(() => expect(document.body.textContent).toContain('Device Next'))
    registry.reject(new Error('offline'))

    await vi.waitFor(() => expect(document.body.textContent).toContain('Could not reach the plugin registry'))
    expect(document.body.textContent).toContain('Device Next')
    expect(readInstalledCache()).toEqual([installed('Device Next', false)])
  })
})
