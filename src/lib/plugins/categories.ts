import type { Messages } from '../i18n/en'

export const PLUGIN_CATEGORY_ORDER = [
  'ai',
  'record',
  'reading',
  'inspiration',
  'advance',
  'reflect',
  'create',
  'import-export',
  'experience',
  'other',
] as const

export type PluginCategory = typeof PLUGIN_CATEGORY_ORDER[number]

const KNOWN_CATEGORIES = new Set<string>(PLUGIN_CATEGORY_ORDER)
const LEGACY_CATEGORIES: Record<string, PluginCategory> = {
  agents: 'ai',
  capture: 'record',
  'capture-import': 'record',
  thinking: 'reflect',
  'thinking-review': 'reflect',
  'publish-export': 'import-export',
  editing: 'experience',
  'editor-extensions': 'experience',
}

/** Current category is authoritative by id so old manifests/caches migrate offline. */
export const OFFICIAL_PLUGIN_CATEGORIES: Readonly<Record<string, PluginCategory>> = {
  'notemd.pos-log': 'record',
  'notemd.roam-import': 'import-export',
  'notemd.ebook-import': 'reading',
  'notemd.trace-source': 'reading',
  'notemd.idea-spark': 'inspiration',
  'notemd.next': 'advance',
  'notemd.claude-agent': 'ai',
  'notemd.codex-agent': 'ai',
  'notemd.deepseek-agent': 'ai',
  'notemd.openclaw-chat': 'advance',
  'notemd.decision-log': 'reflect',
  'notemd.weekly-review': 'reflect',
  'notemd.memory': 'ai',
  'notemd.md2pdf': 'import-export',
  'notemd.power-mode': 'experience',
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

export function isSystemPluginCategory(category: PluginCategory): boolean {
  return category === 'ai' || category === 'import-export' || category === 'experience'
}

export function pluginAiRoleLabelKey(role: PluginAiRole): keyof Messages {
  return `pluginMarket.aiBadge.${role}` as keyof Messages
}

export function pluginCategoryLabelKey(category: PluginCategory): keyof Messages {
  const keys: Record<PluginCategory, keyof Messages> = {
    ai: 'pluginCategory.ai',
    record: 'pluginCategory.record',
    reading: 'pluginCategory.reading',
    inspiration: 'pluginCategory.inspiration',
    advance: 'pluginCategory.advance',
    reflect: 'pluginCategory.reflect',
    create: 'pluginCategory.create',
    'import-export': 'pluginCategory.importExport',
    experience: 'pluginCategory.experience',
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
