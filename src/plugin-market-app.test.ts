// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, unmount } from 'svelte'
import type { InstalledV2, RegistryEntry, RegistryIndex } from './lib/market/types'
import { readInstalledCache, writeInstalledCache } from './lib/market/cache'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  i18n: { locale: 'en' },
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
    'pluginMarket.update': 'Update to {version}',
    'pluginMarket.onDevice': 'Installed on this device.',
    'pluginMarket.noneAvailable': 'No plugins available.',
    'pluginMarket.noneInstalled': 'No plugins installed.',
    'pluginMarket.localStateError': 'Could not refresh installed plugins: {error}',
    'pluginMarket.networkError': 'Could not reach the plugin registry: {error}',
    'pluginCategory.thinking': 'Thinking',
    'capability.vault.read': 'Read files',
  }
  return {
    i18n: mocks.i18n,
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
  mocks.i18n.locale = 'en'
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
    expect(document.querySelector('.update-card')?.textContent).toContain('v1.1.0')
    expect(document.querySelector('.update-card .update-action')?.textContent).toContain('1.1.0')
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

  it('keeps cached translations when an older installed manifest refreshes offline', async () => {
    mocks.i18n.locale = 'zh'
    const cachedPlugin: InstalledV2 = {
      ...installed('Idea Spark'),
      id: 'notemd.idea-spark',
      description: 'Capture a spark.',
      i18n: {
        zh: { name: '奇思妙想', description: '捕捉一闪而过的灵感。' },
      },
    }
    writeInstalledCache([cachedPlugin])
    const registry = deferred<RegistryIndex>()
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'plugin_market_installed') {
        return Promise.resolve([{
          ...cachedPlugin,
          i18n: null,
        }])
      }
      if (command === 'plugin_market_index') return registry.promise
      return Promise.resolve()
    })

    component = mount(PluginMarketApp, { target: document.body })
    await vi.waitFor(() => expect(document.body.textContent).toContain('奇思妙想（Idea Spark）'))
    registry.reject(new Error('offline'))

    await vi.waitFor(() => expect(document.body.textContent).toContain('Could not reach the plugin registry'))
    expect(document.body.textContent).toContain('捕捉一闪而过的灵感。')
    expect(readInstalledCache()[0]?.i18n).toEqual(cachedPlugin.i18n)
  })

  it('localizes plugin names and descriptions while retaining non-Western English names', async () => {
    mocks.i18n.locale = 'zh'
    const localPlugin: InstalledV2 = {
      ...installed('Idea Spark'),
      id: 'notemd.idea-spark',
      description: null,
      i18n: {
        zh: { name: '奇思妙想' },
      },
    }
    const installedListing = entry('notemd.idea-spark', 'Idea Spark', '1.1.0')
    installedListing.description = 'Capture a spark.'
    installedListing.i18n = {
      zh: { name: '奇思妙想', description: '捕捉一闪而过的灵感。' },
    }
    const availablePlugin = entry('notemd.trace-source', 'Trace Source')
    availablePlugin.i18n = {
      zh: { name: '溯源', description: '追溯文字的原始出处。' },
    }
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'plugin_market_installed') return Promise.resolve([localPlugin])
      if (command === 'plugin_market_index') {
        return Promise.resolve({ plugins: [installedListing, availablePlugin] })
      }
      return Promise.resolve()
    })

    component = mount(PluginMarketApp, { target: document.body })
    await vi.waitFor(() => expect(document.body.textContent).toContain('奇思妙想（Idea Spark）'))
    expect(document.body.textContent).toContain('捕捉一闪而过的灵感。')
    expect(document.body.textContent).toContain('溯源（Trace Source）')
    expect(document.body.textContent).toContain('追溯文字的原始出处。')
  })
})
