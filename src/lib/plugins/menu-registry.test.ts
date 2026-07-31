import { describe, it, expect } from 'vitest'
import {
  collectMenuItems, evaluateEnabled, mkPluginMenuId, parsePluginMenuId,
  CORE_MENU_ENABLED_ITEMS,
} from './menu-registry'
import type { PluginManifest } from './types'

const baseManifest = (): PluginManifest => ({
  id: 'share', name: 'Share', version: '1.0.0', binary: 'bin',
  host_capabilities: ['toast'],
  menus: [
    { location: 'file', label: 'Share Current File...', shortcut: 'Cmd+Shift+L', command: 'publish', enabled_when: 'currentTab.hasContent' },
    { location: 'file', label: 'Unshare', command: 'unpublish' },
  ],
  context_menus: [
    { location: 'tab', label: 'Share This Tab...', command: 'publish' },
  ],
})

describe('mkPluginMenuId / parsePluginMenuId', () => {
  it('round-trips', () => {
    const id = mkPluginMenuId('share', 'publish')
    expect(parsePluginMenuId(id)).toEqual({ pluginId: 'share', command: 'publish' })
  })
  it('rejects non-plugin ids', () => {
    expect(parsePluginMenuId('save')).toBe(null)
  })
})

describe('collectMenuItems', () => {
  it('groups by location', () => {
    const items = collectMenuItems([baseManifest()])
    expect(items.file.length).toBe(2)
    expect(items.tabContext.length).toBe(1)
    expect(items.editorContext.length).toBe(0)
  })
  it('produces menu ids in plugin:<id>:<command> format', () => {
    const items = collectMenuItems([baseManifest()])
    expect(items.file[0].id).toBe('plugin:share:publish')
  })
})

describe('CORE_MENU_ENABLED_ITEMS', () => {
  it('covers the six conditional core menu ids with original enabled_when expressions', () => {
    expect(CORE_MENU_ENABLED_ITEMS).toHaveLength(6)
    const byId = Object.fromEntries(CORE_MENU_ENABLED_ITEMS.map((i) => [i.id, i]))
    expect(byId['sync-to-vault'].enabledWhen).toBe('currentTab.canSyncToVault')
    expect(byId['view-sync-source'].enabledWhen).toBe('currentTab.hasSyncSource')
    expect(byId['share'].enabledWhen).toBe('currentTab.hasContent')
    expect(byId['unshare'].enabledWhen).toBe('settings["share.records"][currentTab.path]')
    expect(byId['copy-share-link'].enabledWhen).toBe('settings["share.records"][currentTab.path]')
    expect(byId['toggle-git-history'].enabledWhen).toBe('currentTab.isInVault')
    expect(byId['unshare'].pluginId).toBe('share')
  })
})

describe('evaluateEnabled', () => {
  it('returns true when enabled_when is omitted', () => {
    const items = collectMenuItems([baseManifest()])
    const ctx = { currentTab: null, settings: {}, vaultConfigured: false }
    expect(evaluateEnabled(items.file[1], ctx)).toBe(true)
  })
  it('evaluates expression against context', () => {
    const items = collectMenuItems([baseManifest()])
    const empty = { currentTab: null, settings: {}, vaultConfigured: false }
    const full = {
      currentTab: { path: '/x.md', filename: 'x.md', extension: 'md', kind: 'markdown' as const,
                    hasContent: true, isDirty: false, isUntitled: false },
      settings: {},
      vaultConfigured: false,
    }
    expect(evaluateEnabled(items.file[0], empty)).toBe(false)
    expect(evaluateEnabled(items.file[0], full)).toBe(true)
  })
})
