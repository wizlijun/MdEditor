import type { PluginManifest, EnabledWhenContext } from './types'
import { evaluateEnabledWhen } from './enabled-when'
import { pluginMenuLabel, pluginContextMenuLabel } from './plugin-i18n'

export interface CollectedItem {
  id: string                    // 'plugin:<pluginId>:<command>'；core 化项为裸 id（如 'sync-to-vault'）
  pluginId: string
  command: string
  label: string
  shortcut?: string
  enabledWhen?: string
}

export interface CollectedItems {
  file: CollectedItem[]
  edit: CollectedItem[]
  view: CollectedItem[]
  window: CollectedItem[]
  help: CollectedItem[]
  plugins: CollectedItem[]
  tabContext: CollectedItem[]
  editorContext: CollectedItem[]
}

export function mkPluginMenuId(pluginId: string, command: string): string {
  return `plugin:${pluginId}:${command}`
}

export function parsePluginMenuId(id: string): { pluginId: string; command: string } | null {
  if (!id.startsWith('plugin:')) return null
  const rest = id.slice('plugin:'.length)
  const sep = rest.indexOf(':')
  if (sep < 0) return null
  return { pluginId: rest.slice(0, sep), command: rest.slice(sep + 1) }
}

export function collectMenuItems(manifests: PluginManifest[]): CollectedItems {
  const out: CollectedItems = {
    file: [], edit: [], view: [], window: [], help: [], plugins: [],
    tabContext: [], editorContext: [],
  }
  for (const m of manifests) {
    for (const me of m.menus ?? []) {
      const item: CollectedItem = {
        id: mkPluginMenuId(m.id, me.command),
        pluginId: m.id, command: me.command,
        label: pluginMenuLabel(m, me.command, me.label), shortcut: me.shortcut, enabledWhen: me.enabled_when,
      }
      out[me.location].push(item)
    }
    for (const ce of m.context_menus ?? []) {
      const item: CollectedItem = {
        id: mkPluginMenuId(m.id, ce.command),
        pluginId: m.id, command: ce.command,
        label: pluginContextMenuLabel(m, ce.command, ce.label), enabledWhen: ce.enabled_when,
      }
      if (ce.location === 'tab') out.tabContext.push(item)
      else out.editorContext.push(item)
    }
  }
  return out
}

/** Core-ized（原插件）菜单项的 enabled_when 表——App.svelte 的 enabled-sync
 *  effect 将它们与插件项同样对待（表达式与原 manifest 逐字一致）。
 *  始终可用的核心项（三个 view toggle 中除 git-history 外）不需要出现在这里。 */
export const CORE_MENU_ENABLED_ITEMS: CollectedItem[] = [
  { id: 'sync-to-vault', pluginId: 'sotvault', command: 'sync-to-vault', label: '', enabledWhen: 'currentTab.canSyncToVault' },
  { id: 'view-sync-source', pluginId: 'sotvault', command: 'view-sync-source', label: '', enabledWhen: 'currentTab.hasSyncSource' },
  { id: 'share', pluginId: 'share', command: 'share', label: '', enabledWhen: 'currentTab.hasContent' },
  { id: 'unshare', pluginId: 'share', command: 'unshare', label: '', enabledWhen: 'settings["share.records"][currentTab.path]' },
  { id: 'copy-share-link', pluginId: 'share', command: 'copy-share-link', label: '', enabledWhen: 'settings["share.records"][currentTab.path]' },
  { id: 'toggle-git-history', pluginId: 'git-history', command: 'toggle', label: '', enabledWhen: 'currentTab.isInVault' },
]

export function evaluateEnabled(item: CollectedItem, ctx: EnabledWhenContext): boolean {
  if (!item.enabledWhen) return true
  try { return evaluateEnabledWhen(item.enabledWhen, ctx) }
  catch (e) {
    console.warn(`[plugin:${item.pluginId}] enabled_when error`, e)
    return false
  }
}
