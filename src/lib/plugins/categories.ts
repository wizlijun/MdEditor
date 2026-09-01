import type { Messages } from '../i18n/en'

export const PLUGIN_CATEGORY_ORDER = [
  'record',
  'reading',
  'inspiration',
  'advance',
  'reflect',
  'create',
  'other',
] as const

export type PluginCategory = typeof PLUGIN_CATEGORY_ORDER[number]

const KNOWN_CATEGORIES = new Set<string>(PLUGIN_CATEGORY_ORDER)
const LEGACY_CATEGORIES: Record<string, PluginCategory> = {
  agents: 'advance',
  capture: 'record',
  'capture-import': 'record',
  thinking: 'reflect',
  'thinking-review': 'reflect',
  'import-export': 'create',
  'publish-export': 'create',
  editing: 'create',
  'editor-extensions': 'create',
}

/** Current category is authoritative by id so old manifests/caches migrate offline. */
export const OFFICIAL_PLUGIN_CATEGORIES: Readonly<Record<string, PluginCategory>> = {
  'notemd.pos-log': 'record',
  'notemd.roam-import': 'record',
  'notemd.ebook-import': 'reading',
  'notemd.trace-source': 'reading',
  'notemd.idea-spark': 'inspiration',
  'notemd.next': 'advance',
  'notemd.claude-agent': 'advance',
  'notemd.codex-agent': 'advance',
  'notemd.deepseek-agent': 'advance',
  'notemd.openclaw-chat': 'advance',
  'notemd.decision-log': 'reflect',
  'notemd.weekly-review': 'reflect',
  'notemd.md2pdf': 'create',
  'notemd.power-mode': 'create',
}

export function normalizePluginCategory(
  category: string | null | undefined,
  pluginId?: string,
): PluginCategory {
  if (pluginId && OFFICIAL_PLUGIN_CATEGORIES[pluginId]) {
    return OFFICIAL_PLUGIN_CATEGORIES[pluginId]
  }
  if (!category) return 'other'
  if (KNOWN_CATEGORIES.has(category)) return category as PluginCategory
  return LEGACY_CATEGORIES[category] ?? 'other'
}

export type PluginAiRole = 'read' | 'inspire' | 'reason' | 'execute'

const PLUGIN_AI_ROLES: Readonly<Record<string, PluginAiRole>> = {
  'notemd.ebook-import': 'read',
  'notemd.idea-spark': 'inspire',
  'notemd.trace-source': 'reason',
  'notemd.claude-agent': 'execute',
  'notemd.codex-agent': 'execute',
  'notemd.deepseek-agent': 'execute',
  'notemd.openclaw-chat': 'execute',
}

export function pluginAiRole(pluginId: string): PluginAiRole | null {
  return PLUGIN_AI_ROLES[pluginId] ?? null
}

export function pluginAiRoleLabelKey(role: PluginAiRole): keyof Messages {
  return `pluginMarket.aiBadge.${role}` as keyof Messages
}

export function pluginCategoryLabelKey(category: PluginCategory): keyof Messages {
  const keys: Record<PluginCategory, keyof Messages> = {
    record: 'pluginCategory.record',
    reading: 'pluginCategory.reading',
    inspiration: 'pluginCategory.inspiration',
    advance: 'pluginCategory.advance',
    reflect: 'pluginCategory.reflect',
    create: 'pluginCategory.create',
    other: 'pluginCategory.other',
  }
  return keys[category]
}

export interface PluginCategoryGroup<T> {
  key: PluginCategory
  items: T[]
}

/** Fixed group order; input order remains stable inside each group. */
export function groupPluginsByCategory<T extends { category?: string | null }>(
  items: readonly T[],
): PluginCategoryGroup<T>[] {
  const buckets = new Map<PluginCategory, T[]>(
    PLUGIN_CATEGORY_ORDER.map((key) => [key, []]),
  )
  for (const item of items) {
    const pluginId = 'id' in item && typeof item.id === 'string' ? item.id : undefined
    buckets.get(normalizePluginCategory(item.category, pluginId))!.push(item)
  }
  return PLUGIN_CATEGORY_ORDER
    .map((key) => ({ key, items: buckets.get(key)! }))
    .filter((group) => group.items.length > 0)
}
