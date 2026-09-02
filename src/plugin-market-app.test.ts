// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, unmount } from 'svelte'
import type { InstalledV2, RegistryEntry, RegistryIndex } from './lib/market/types'
import { readInstalledCache, writeInstalledCache } from './lib/market/cache'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  confirm: vi.fn(),
  pushToast: vi.fn(),
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
vi.mock('@tauri-apps/plugin-dialog', () => ({ confirm: mocks.confirm }))
vi.mock('./lib/settings.svelte', () => ({ loadSettings: vi.fn(async () => {}) }))
vi.mock('./lib/toast.svelte', () => ({ pushToast: mocks.pushToast }))
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
    'pluginMarket.updateAll': 'Update All ({count})',
    'pluginMarket.updatingAll': 'Updating {done}/{total}…',
    'pluginMarket.updateAllConfirm': 'Update all {count} plugins?',
    'pluginMarket.updateAllComplete': 'Updated all {count} plugins',
    'pluginMarket.updateAllPartial': 'Updated {succeeded}; {failed} failed',
    'pluginMarket.onDevice': 'Installed on this device.',
    'pluginMarket.noneAvailable': 'No plugins available.',
    'pluginMarket.noneInstalled': 'No plugins installed.',
    'pluginMarket.localStateError': 'Could not refresh installed plugins: {error}',
    'pluginMarket.networkError': 'Could not reach the plugin registry: {error}',
    'pluginCategory.record': 'Capture',
    'pluginCategory.ai': 'AI',
    'pluginCategory.reading': 'Read',
    'pluginCategory.inspiration': 'Ideas',
    'pluginCategory.advance': 'Move Forward',
    'pluginCategory.reflect': 'Reflect',
    'pluginCategory.create': 'Create',
    'pluginCategory.importExport': 'Import & Export',
    'pluginCategory.experience': 'Experience',
    'pluginCategory.other': 'Other',
    'pluginMarket.aiTitle': 'Create with AI',
    'pluginMarket.aiSubtitle': 'Focused AI collaborators for understanding, ideas, and action.',
    'pluginMarket.aiBadge.read': 'AI Read',
    'pluginMarket.aiBadge.inspire': 'AI Ideas',
    'pluginMarket.aiBadge.reason': 'AI Reasoning',
    'pluginMarket.aiBadge.execute': 'AI Action',
    'pluginMarket.systemFeature': 'System Feature',
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
  mocks.confirm.mockReset()
  mocks.confirm.mockResolvedValue(true)
  mocks.pushToast.mockReset()
  mocks.i18n.locale = 'en'
})

afterEach(async () => {
  if (component) await unmount(component)
  component = null
  document.body.innerHTML = ''
  localStorage.clear()
})

describe('plugin market staged loading', () => {
  it('uses a normal online request on startup and forces a fresh online request after clicking Refresh', async () => {
    let indexCalls = 0
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'plugin_market_installed') return Promise.resolve([])
      if (command === 'plugin_market_index') {
        indexCalls += 1
        return Promise.resolve({
          plugins: indexCalls === 1
            ? [entry('notemd.next', 'Next')]
            : [entry('notemd.idea-spark', 'Idea Spark')],
        })
      }
      return Promise.resolve()
    })

    component = mount(PluginMarketApp, { target: document.body })
    await vi.waitFor(() => expect(document.body.textContent).toContain('Next'))
    expect(mocks.invoke).toHaveBeenCalledWith('plugin_market_index', { forceRefresh: false })

    const refresh = document.querySelector('.refresh') as HTMLButtonElement
    await vi.waitFor(() => expect(refresh.disabled).toBe(false))
    refresh.click()

    await vi.waitFor(() => expect(document.body.textContent).toContain('Idea Spark'))
    expect(document.body.textContent).not.toContain('Next')
    expect(mocks.invoke).toHaveBeenCalledWith('plugin_market_index', { forceRefresh: true })
  })

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
    expect(document.querySelectorAll('.category-block[data-category="advance"]')).toHaveLength(1)
    expect(document.querySelectorAll('.category-block[data-category="advance"] .plugin-card')).toHaveLength(1)
    expect(document.querySelectorAll('.category-block[data-category="inspiration"]')).toHaveLength(1)
    expect(document.querySelectorAll('.category-block[data-category="inspiration"] .plugin-card')).toHaveLength(1)
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

  it('puts the three agents and Memory in the first system category without moving other AI plugins', async () => {
    const claude = entry('notemd.claude-agent', 'Claude Agent')
    const codex = entry('notemd.codex-agent', 'Codex Agent')
    const deepseek = entry('notemd.deepseek-agent', 'DeepSeek Agent')
    const memory = entry('notemd.memory', 'Memory')
    const ebook = entry('notemd.ebook-import', 'Ebook Import')
    for (const item of [claude, codex, deepseek]) item.category = 'agents'
    memory.category = 'thinking-review'
    ebook.category = 'reading'
    const next = entry('notemd.next', 'Next')
    next.category = 'thinking'
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'plugin_market_installed') return Promise.resolve([])
      if (command === 'plugin_market_index') {
        return Promise.resolve({ plugins: [next, ebook, memory, codex, claude, deepseek] })
      }
      return Promise.resolve()
    })

    component = mount(PluginMarketApp, { target: document.body })
    await vi.waitFor(() => expect(document.body.textContent).toContain('DeepSeek Agent'))
    const categories = [...document.querySelectorAll<HTMLElement>('.category-block')]
    expect(categories[0]?.dataset.category).toBe('ai')
    expect(categories[0]?.querySelector('h2')?.textContent).toBe('AI')
    expect(categories[0]?.querySelector('.system-badge')).not.toBeNull()
    expect(categories[0]?.querySelectorAll('.plugin-card')).toHaveLength(4)
    expect(categories[0]?.textContent).toContain('Claude Agent')
    expect(categories[0]?.textContent).toContain('Codex Agent')
    expect(categories[0]?.textContent).toContain('DeepSeek Agent')
    expect(categories[0]?.textContent).toContain('Memory')
    expect(document.querySelector('[data-plugin-id="notemd.codex-agent"] .ai-badge')?.textContent)
      .toContain('AI Action')
    expect(document.querySelector('[data-plugin-id="notemd.next"] .ai-badge')).toBeNull()
    expect(document.querySelectorAll('[data-category="reading"] .plugin-card')).toHaveLength(1)
    expect(document.querySelectorAll('[data-category="advance"] .plugin-card')).toHaveLength(1)
    expect(document.querySelector('.ai-spotlight')).toBeNull()
  })

  it('separates import/export and experience as system feature categories', async () => {
    const roam = entry('notemd.roam-import', 'Roam Import')
    roam.category = 'record'
    const pdf = entry('notemd.md2pdf', 'Export to PDF')
    pdf.category = 'create'
    const power = entry('notemd.power-mode', 'Power Mode')
    power.category = 'editing'
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'plugin_market_installed') return Promise.resolve([])
      if (command === 'plugin_market_index') return Promise.resolve({ plugins: [roam, pdf, power] })
      return Promise.resolve()
    })

    component = mount(PluginMarketApp, { target: document.body })
    await vi.waitFor(() => expect(document.body.textContent).toContain('Power Mode'))
    expect(document.querySelectorAll('[data-category="import-export"] .plugin-card')).toHaveLength(2)
    expect(document.querySelectorAll('[data-category="experience"] .plugin-card')).toHaveLength(1)
    expect(document.querySelectorAll('.system-badge')).toHaveLength(2)
    expect(document.querySelector('[data-category="create"]')).toBeNull()
  })

  it('confirms once and updates every changed plugin without opening individual consent modals', async () => {
    let localPlugins: InstalledV2[] = [
      { ...installed('Next'), id: 'notemd.next' },
      { ...installed('Idea Spark'), id: 'notemd.idea-spark' },
    ]
    const index: RegistryIndex = {
      plugins: [
        entry('notemd.next', 'Next', '1.1.0'),
        entry('notemd.idea-spark', 'Idea Spark', '1.2.0'),
      ],
    }
    mocks.invoke.mockImplementation((command: string, args?: { id?: string; version?: string }) => {
      if (command === 'plugin_market_installed') return Promise.resolve(localPlugins)
      if (command === 'plugin_market_index') return Promise.resolve(index)
      if (command === 'plugin_market_install') {
        localPlugins = localPlugins.map((plugin) => plugin.id === args?.id
          ? { ...plugin, version: args.version ?? plugin.version }
          : plugin)
        return Promise.resolve()
      }
      return Promise.resolve()
    })

    component = mount(PluginMarketApp, { target: document.body })
    await vi.waitFor(() => expect(document.querySelector('.update-all')?.textContent).toContain('2'))
    ;(document.querySelector('.update-all') as HTMLButtonElement).click()

    await vi.waitFor(() => expect(
      mocks.invoke.mock.calls.filter(([command]) => command === 'plugin_market_install'),
    ).toHaveLength(2))
    await vi.waitFor(() => expect(document.querySelector('.update-all')).toBeNull())
    expect(mocks.confirm).toHaveBeenCalledOnce()
    expect(document.querySelector('[role="dialog"]')).toBeNull()
    expect(mocks.pushToast).toHaveBeenCalledWith(expect.objectContaining({
      level: 'success',
      message: 'Updated all 2 plugins',
    }))
  })

  it('continues updating the remaining plugins after one batch item fails', async () => {
    const localPlugins: InstalledV2[] = [
      { ...installed('Next'), id: 'notemd.next' },
      { ...installed('Idea Spark'), id: 'notemd.idea-spark' },
    ]
    const index: RegistryIndex = {
      plugins: [
        entry('notemd.next', 'Next', '1.1.0'),
        entry('notemd.idea-spark', 'Idea Spark', '1.2.0'),
      ],
    }
    mocks.invoke.mockImplementation((command: string, args?: { id?: string }) => {
      if (command === 'plugin_market_installed') return Promise.resolve(localPlugins)
      if (command === 'plugin_market_index') return Promise.resolve(index)
      if (command === 'plugin_market_install' && args?.id === 'notemd.next') {
        return Promise.reject(new Error('signature mismatch'))
      }
      return Promise.resolve()
    })

    component = mount(PluginMarketApp, { target: document.body })
    await vi.waitFor(() => expect(document.querySelector('.update-all')).not.toBeNull())
    ;(document.querySelector('.update-all') as HTMLButtonElement).click()

    await vi.waitFor(() => expect(
      mocks.invoke.mock.calls.filter(([command]) => command === 'plugin_market_install'),
    ).toHaveLength(2))
    await vi.waitFor(() => expect(mocks.pushToast).toHaveBeenCalledWith(expect.objectContaining({
      level: 'error',
      message: 'Updated 1; 1 failed',
      detail: expect.stringContaining('signature mismatch'),
    })))
  })
})
