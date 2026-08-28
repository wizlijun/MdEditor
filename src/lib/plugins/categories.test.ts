import { describe, expect, it } from 'vitest'
import { groupPluginsByCategory, normalizePluginCategory, PLUGIN_CATEGORY_ORDER } from './categories'

describe('plugin capability categories', () => {
  it('keeps the fixed capability order and stable input order within a group', () => {
    const rows = [
      { id: 'capture', category: 'capture-import' },
      { id: 'agent-b', category: 'agents' },
      { id: 'agent-a', category: 'agents' },
      { id: 'editor', category: 'editor-extensions' },
    ]
    const groups = groupPluginsByCategory(rows)
    expect(groups.map((group) => group.key)).toEqual([
      'agents', 'capture-import', 'editor-extensions',
    ])
    expect(groups[0].items.map((row) => row.id)).toEqual(['agent-b', 'agent-a'])
  })

  it('sends unknown and missing categories to Other', () => {
    expect(normalizePluginCategory('future-category')).toBe('other')
    expect(normalizePluginCategory(null)).toBe('other')
    expect(groupPluginsByCategory([
      { id: 'unknown', category: 'future-category' },
      { id: 'missing' },
    ])).toEqual([{ key: 'other', items: [
      { id: 'unknown', category: 'future-category' },
      { id: 'missing' },
    ] }])
  })

  it('keeps Other last', () => {
    expect(PLUGIN_CATEGORY_ORDER.at(-1)).toBe('other')
  })
})
