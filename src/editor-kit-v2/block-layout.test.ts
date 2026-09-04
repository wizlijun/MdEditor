import { describe, expect, it } from 'vitest'
import { createSchema } from '@moraya/core'
import { EditorState } from 'prosemirror-state'
import {
  materializeBlocks,
  replaceBlockSpans,
  serializeSpan,
  spanContaining,
} from './block-layout'

const mediaResolver = {
  loadLocalImage: async (path: string) => path,
  loadLocalMedia: async (path: string) => path,
  loadRemoteMedia: async (url: string) => url,
}

const schema = createSchema({ mediaResolver })

describe('Editor Kit v2 block layout', () => {
  it('maps one stable block across its contiguous heading, paragraph, and list nodes', () => {
    const materialized = materializeBlocks([
      { blockId: 'section-a', markdown: '## Context\n\nNarrative.\n\n- first\n- second' },
      { blockId: 'section-b', markdown: 'Following section.' },
    ], schema)

    expect(materialized.doc.childCount).toBe(4)
    expect(materialized.layout.spans).toEqual([
      { blockId: 'section-a', startIndex: 0, endIndex: 3 },
      { blockId: 'section-b', startIndex: 3, endIndex: 4 },
    ])
    expect(spanContaining(materialized.layout, 2)?.blockId).toBe('section-a')
    expect(serializeSpan(materialized.doc, materialized.layout.spans[0])).toContain('- first')
  })

  it('recomputes following spans when one remote replacement changes node count', () => {
    const initial = materializeBlocks([
      { blockId: 'section-a', markdown: 'One node.' },
      { blockId: 'section-b', markdown: 'Still B.' },
    ], schema)
    const state = EditorState.create({ schema, doc: initial.doc })
    const replaced = replaceBlockSpans(state.tr, initial.layout, [
      { blockId: 'section-a', markdown: 'First node.\n\nSecond node.' },
    ])

    expect(replaced.layout.spans).toEqual([
      { blockId: 'section-a', startIndex: 0, endIndex: 2 },
      { blockId: 'section-b', startIndex: 2, endIndex: 3 },
    ])
    expect(serializeSpan(replaced.transaction.doc, replaced.layout.spans[1])).toBe('Still B.')
  })

  it('plans multiple replacements by stable ID rather than mutable positions', () => {
    const initial = materializeBlocks([
      { blockId: 'section-a', markdown: 'A.' },
      { blockId: 'section-b', markdown: 'B.' },
      { blockId: 'section-c', markdown: 'C.' },
    ], schema)
    const state = EditorState.create({ schema, doc: initial.doc })
    const replaced = replaceBlockSpans(state.tr, initial.layout, [
      { blockId: 'section-a', markdown: 'A one.\n\nA two.' },
      { blockId: 'section-c', markdown: 'C one.\n\nC two.\n\nC three.' },
    ])

    expect(replaced.layout.spans).toEqual([
      { blockId: 'section-a', startIndex: 0, endIndex: 2 },
      { blockId: 'section-b', startIndex: 2, endIndex: 3 },
      { blockId: 'section-c', startIndex: 3, endIndex: 6 },
    ])
    expect(serializeSpan(replaced.transaction.doc, replaced.layout.spans[1])).toBe('B.')
  })

  it('rejects duplicate and unknown replacements before changing the transaction', () => {
    const initial = materializeBlocks([{ blockId: 'section-a', markdown: 'A.' }], schema)
    const state = EditorState.create({ schema, doc: initial.doc })
    const duplicateTransaction = state.tr

    expect(() => replaceBlockSpans(duplicateTransaction, initial.layout, [
      { blockId: 'section-a', markdown: 'First.' },
      { blockId: 'section-a', markdown: 'Second.' },
    ])).toThrow('EDITOR_KIT_V2_DUPLICATE_BLOCK')
    expect(duplicateTransaction.doc.eq(initial.doc)).toBe(true)

    const unknownTransaction = state.tr
    expect(() => replaceBlockSpans(unknownTransaction, initial.layout, [
      { blockId: 'missing', markdown: 'Unknown.' },
    ])).toThrow('EDITOR_KIT_V2_UNKNOWN_BLOCK')
    expect(unknownTransaction.doc.eq(initial.doc)).toBe(true)
  })

  it('rejects a snapshot with no stable block identity', () => {
    expect(() => materializeBlocks([], schema)).toThrow('EDITOR_KIT_V2_BLOCK_IDENTITY')
  })
})
