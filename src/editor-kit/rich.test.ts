import { describe, it, expect } from 'vitest'
import { EditorState, type Plugin } from 'prosemirror-state'
import type { DecorationSet } from 'prosemirror-view'
import { parseMarkdown } from '@moraya/core'

// Same construction as src/lib/placeholder-plugin.test.ts: `parseMarkdown`
// builds a doc against moraya's internal default schema, so this exercises
// the plugin's `decorations` prop without a real DOM/EditorView (mounting
// the full moraya editor needs Tauri-free jsdom support it doesn't have).
function decorationsFor(markdown: string, plugins: Plugin[]) {
  const state = EditorState.create({ doc: parseMarkdown(markdown), plugins })
  const decos = plugins[0]?.props.decorations?.call(plugins[0], state) as DecorationSet | null | undefined
  return decos ? decos.find() : []
}

describe('kit rich placeholder', () => {
  it('is part of the plugin set only when a placeholder was given', async () => {
    // mountRich 需要真实 DOM + moraya,jsdom 下挂不起来;这里验证的是接线
    // 契约:传了 placeholder 才追加插件。
    const { richPlugins } = await import('./rich')
    expect(richPlugins(undefined)).toHaveLength(0)
    expect(richPlugins('写点什么')).toHaveLength(1)
  })

  it('decorates the lone empty paragraph with the given text', async () => {
    const { richPlugins } = await import('./rich')
    const plugins = richPlugins('写点什么')
    const found = decorationsFor('', plugins)
    expect(found).toHaveLength(1)
    expect((found[0] as unknown as { type: { attrs: Record<string, string> } }).type.attrs)
      .toMatchObject({ class: 'is-empty', 'data-placeholder': '写点什么' })
  })

  it('leaves a document with content alone', async () => {
    const { richPlugins } = await import('./rich')
    const plugins = richPlugins('写点什么')
    expect(decorationsFor('hello', plugins)).toHaveLength(0)
  })
})

// `placeholderPlugin` closes over its text at construction, so `setPlaceholder`
// can only work by building a new plugin and swapping it in. What must hold:
// the new text really renders, and moraya's own plugins survive the swap (they
// carry history / input rules / keymaps — losing them would break editing).
describe('kit rich placeholder — swapPlaceholder', () => {
  it('replaces the old placeholder plugin with one carrying the new text', async () => {
    const { richPlugins, swapPlaceholder } = await import('./rich')
    const before = richPlugins('第一句')
    expect((decorationsFor('', before)[0] as unknown as { type: { attrs: Record<string, string> } }).type.attrs)
      .toMatchObject({ 'data-placeholder': '第一句' })

    const after = swapPlaceholder(before, '第二句')

    expect(after).toHaveLength(1)
    expect(after[0]).not.toBe(before[0])
    expect((decorationsFor('', after)[0] as unknown as { type: { attrs: Record<string, string> } }).type.attrs)
      .toMatchObject({ class: 'is-empty', 'data-placeholder': '第二句' })
  })

  it('keeps every other plugin, in order', async () => {
    const { richPlugins, swapPlaceholder } = await import('./rich')
    const { Plugin } = await import('prosemirror-state')
    // Stand-ins for moraya's own plugins. ProseMirror auto-assigns them a key
    // (`plugin$…`), which is never the placeholder's, so the filter must leave
    // them exactly where they were.
    const other = new Plugin({})
    const another = new Plugin({})
    const after = swapPlaceholder([other, ...richPlugins('第一句'), another], '第二句')
    expect(after.slice(0, 2)).toEqual([other, another])
    expect(after).toHaveLength(3)
  })

  it('appends one when the editor was mounted without a placeholder', async () => {
    const { richPlugins, swapPlaceholder } = await import('./rich')
    const after = swapPlaceholder(richPlugins(undefined), '第一句')
    expect(after).toHaveLength(1)
    expect((decorationsFor('', after)[0] as unknown as { type: { attrs: Record<string, string> } }).type.attrs)
      .toMatchObject({ 'data-placeholder': '第一句' })
  })
})
