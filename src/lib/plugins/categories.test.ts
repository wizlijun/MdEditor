import { describe, expect, it } from 'vitest'
import { groupPluginsByCategory, normalizePluginCategory, PLUGIN_CATEGORY_ORDER } from './categories'

describe('plugin capability categories', () => {
  it('uses the documented top-level category order', () => {
    expect(PLUGIN_CATEGORY_ORDER).toEqual([
      'agents', 'capture', 'reading', 'thinking', 'import-export', 'editing', 'other',
    ])
  })

  it('keeps the fixed capability order and stable input order within a group', () => {
    const rows = [
      { id: 'reading', category: 'reading' },
      { id: 'agent-b', category: 'agents' },
      { id: 'agent-a', category: 'agents' },
      { id: 'capture', category: 'capture' },
      { id: 'editor', category: 'editing' },
    ]
    const groups = groupPluginsByCategory(rows)
    expect(groups.map((group) => group.key)).toEqual([
      'agents', 'capture', 'reading', 'editing',
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

  it('maps previous category keys to their closest current category', () => {
    expect(normalizePluginCategory('capture-import')).toBe('capture')
    expect(normalizePluginCategory('thinking-review')).toBe('thinking')
    expect(normalizePluginCategory('publish-export')).toBe('import-export')
    expect(normalizePluginCategory('editor-extensions')).toBe('editing')
  })
})
