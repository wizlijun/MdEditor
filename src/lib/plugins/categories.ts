import type { Messages } from '../i18n/en'

export const PLUGIN_CATEGORY_ORDER = [
  'agents',
  'capture-import',
  'thinking-review',
  'publish-export',
  'editor-extensions',
  'other',
] as const

export type PluginCategory = typeof PLUGIN_CATEGORY_ORDER[number]

const KNOWN_CATEGORIES = new Set<string>(PLUGIN_CATEGORY_ORDER)

export function normalizePluginCategory(category: string | null | undefined): PluginCategory {
  return category && KNOWN_CATEGORIES.has(category) ? category as PluginCategory : 'other'
}

export function pluginCategoryLabelKey(category: PluginCategory): keyof Messages {
  const keys: Record<PluginCategory, keyof Messages> = {
    agents: 'pluginCategory.agents',
    'capture-import': 'pluginCategory.captureImport',
    'thinking-review': 'pluginCategory.thinkingReview',
    'publish-export': 'pluginCategory.publishExport',
    'editor-extensions': 'pluginCategory.editorExtensions',
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
