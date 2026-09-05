import { describe, expect, it } from 'vitest'
import { createSchema, parseMarkdown } from '@moraya/core'
import { EditorState, TextSelection } from 'prosemirror-state'
import { materializeBlocks } from './block-layout'
import { BLOCK_ID_ATTR, normalizeBlockIdentity, readBlockLayout, withBlockIdentitySchema, withoutBlockIdentity } from './identity'

const baseSchema = createSchema({ mediaResolver: {
  loadLocalImage: async (path) => path, loadLocalMedia: async (path) => path, loadRemoteMedia: async (url) => url,
} })
const schema = withBlockIdentitySchema(baseSchema)

function initial() {
  const doc = materializeBlocks([{ blockId: 'a', markdown: 'Alpha' }, { blockId: 'b', markdown: 'Beta' }], schema).doc
  return EditorState.create({ schema, doc })
}

describe('governed editor identity normalization', () => {
  it('extends only the instance schema and never changes the frozen v1 schema', () => {
    expect(baseSchema.nodes.paragraph.create().attrs[BLOCK_ID_ATTR]).toBeUndefined()
    expect(schema.nodes.paragraph.create().attrs[BLOCK_ID_ATTR]).toBeNull()
  })

  it('keeps the replaced node identity even if the cursor is in another block', () => {
    const before = initial()
    const from = before.doc.child(0).nodeSize
    const tr = before.tr.replaceWith(from, before.doc.content.size, parseMarkdown('Replacement', schema).content)
    const after = before.apply(tr)
    const normalized = normalizeBlockIdentity(after, before, () => 'new', tr.mapping)!
    expect(readBlockLayout(normalized.doc).spans.map((span) => span.blockId)).toEqual(['a', 'b'])
  })

  it('gives consecutive empty paragraphs unique persistent identities and is idempotent', () => {
    const before = initial()
    const end = before.doc.content.size - 1
    const tr = before.tr.setSelection(TextSelection.create(before.doc, end)).split(end)
    const after = before.apply(tr)
    const normalized = normalizeBlockIdentity(after, before, () => 'empty', tr.mapping)!
    const state = after.apply(normalized)
    expect(readBlockLayout(state.doc).spans.map((span) => span.blockId)).toEqual(['a', 'b', 'empty'])
    expect(normalizeBlockIdentity(state, state, () => 'unused')).toBeNull()
  })

  it('removes every identity from a supplied clipboard slice including nested content', () => {
    const doc = initial().doc
    const slice = withoutBlockIdentity(doc.slice(0, doc.content.size))
    slice.content.forEach((node) => expect(node.attrs[BLOCK_ID_ATTR]).toBeNull())
    expect(slice.content.textBetween(0, slice.content.size, '\n')).toContain('Alpha')
  })
})
