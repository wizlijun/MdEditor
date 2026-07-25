import { describe, it, expect } from 'vitest'
import { EditorState } from 'prosemirror-state'
import type { DecorationSet } from 'prosemirror-view'
import { parseMarkdown } from '@moraya/core'
import { placeholderPlugin } from './placeholder-plugin'

function decorationsFor(markdown: string) {
  const plugin = placeholderPlugin('Start writing…')
  const state = EditorState.create({ doc: parseMarkdown(markdown), plugins: [plugin] })
  const decos = plugin.props.decorations?.call(plugin, state) as DecorationSet | null | undefined
  return decos ? decos.find() : []
}

describe('placeholderPlugin', () => {
  it('decorates the lone empty paragraph of an empty document', () => {
    const found = decorationsFor('')
    expect(found).toHaveLength(1)
    expect((found[0] as unknown as { type: { attrs: Record<string, string> } }).type.attrs)
      .toMatchObject({ class: 'is-empty', 'data-placeholder': 'Start writing…' })
  })

  it('leaves a document with content alone', () => {
    expect(decorationsFor('hello')).toHaveLength(0)
  })

  it('does not hint blank lines inside a non-empty document', () => {
    expect(decorationsFor('# title\n\n\n\nbody')).toHaveLength(0)
  })
})
