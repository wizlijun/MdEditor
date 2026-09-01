import { describe, expect, it } from 'vitest'
import {
  groupPluginsByCategory,
  normalizePluginCategory,
  pluginAiRole,
  PLUGIN_CATEGORY_ORDER,
} from './categories'

describe('plugin capability categories', () => {
  it('uses the documented top-level category order', () => {
    expect(PLUGIN_CATEGORY_ORDER).toEqual([
      'record', 'reading', 'inspiration', 'advance', 'reflect', 'create', 'experience', 'other',
    ])
  })

  it('keeps the fixed capability order and stable input order within a group', () => {
    const rows = [
      { id: 'reading', category: 'reading' },
      { id: 'third.agent-b', category: 'advance' },
      { id: 'third.agent-a', category: 'advance' },
      { id: 'third.record', category: 'record' },
      { id: 'third.create', category: 'create' },
    ]
    const groups = groupPluginsByCategory(rows)
    expect(groups.map((group) => group.key)).toEqual([
      'record', 'reading', 'advance', 'create',
    ])
    expect(groups[2].items.map((row) => row.id)).toEqual(['third.agent-b', 'third.agent-a'])
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
    expect(normalizePluginCategory('agents')).toBe('advance')
    expect(normalizePluginCategory('capture')).toBe('record')
    expect(normalizePluginCategory('thinking-review')).toBe('reflect')
    expect(normalizePluginCategory('publish-export')).toBe('create')
    expect(normalizePluginCategory('editor-extensions')).toBe('experience')
  })

  it('uses official plugin ids to migrate ambiguous cached categories', () => {
    expect(normalizePluginCategory('capture', 'notemd.idea-spark')).toBe('inspiration')
    expect(normalizePluginCategory('capture', 'notemd.trace-source')).toBe('reading')
    expect(normalizePluginCategory('thinking', 'notemd.next')).toBe('advance')
    expect(normalizePluginCategory('thinking', 'notemd.weekly-review')).toBe('reflect')
    expect(normalizePluginCategory('import-export', 'notemd.roam-import')).toBe('record')
    expect(normalizePluginCategory('editing', 'notemd.power-mode')).toBe('experience')
  })

  it('declares AI roles only for plugins that directly provide AI collaboration', () => {
    expect(pluginAiRole('notemd.ebook-import')).toBe('read')
    expect(pluginAiRole('notemd.idea-spark')).toBe('inspire')
    expect(pluginAiRole('notemd.trace-source')).toBe('reason')
    expect(pluginAiRole('notemd.codex-agent')).toBe('execute')
    expect(pluginAiRole('notemd.next')).toBeNull()
  })
})
