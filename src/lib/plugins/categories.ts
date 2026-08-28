import type { Messages } from '../i18n/en'

export const PLUGIN_CATEGORY_ORDER = [
  'agents',
  'capture',
  'reading',
  'thinking',
  'import-export',
  'editing',
  'other',
] as const

export type PluginCategory = typeof PLUGIN_CATEGORY_ORDER[number]

const KNOWN_CATEGORIES = new Set<string>(PLUGIN_CATEGORY_ORDER)
const LEGACY_CATEGORIES: Record<string, PluginCategory> = {
  'capture-import': 'capture',
  'thinking-review': 'thinking',
  'publish-export': 'import-export',
  'editor-extensions': 'editing',
}

export function normalizePluginCategory(category: string | null | undefined): PluginCategory {
  if (!category) return 'other'
  if (KNOWN_CATEGORIES.has(category)) return category as PluginCategory
  return LEGACY_CATEGORIES[category] ?? 'other'
}

export function pluginCategoryLabelKey(category: PluginCategory): keyof Messages {
  const keys: Record<PluginCategory, keyof Messages> = {
    agents: 'pluginCategory.agents',
    capture: 'pluginCategory.capture',
    reading: 'pluginCategory.reading',
    thinking: 'pluginCategory.thinking',
    'import-export': 'pluginCategory.importExport',
    editing: 'pluginCategory.editing',
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
    buckets.get(normalizePluginCategory(item.category))!.push(item)
  }
  return PLUGIN_CATEGORY_ORDER
    .map((key) => ({ key, items: buckets.get(key)! }))
    .filter((group) => group.items.length > 0)
}
