import { describe, expect, it } from 'vitest'
import { createSchema, parseMarkdown } from '@moraya/core'
import { EditorState } from 'prosemirror-state'
import {
  deleteBlockSpan,
  insertBlockSpan,
  materializeBlocks,
  planLocalBlockEdit,
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

  it('inserts and deletes a multi-node block while keeping every following span aligned', () => {
    const initial = materializeBlocks([
      { blockId: 'section-a', markdown: 'A.' },
      { blockId: 'section-b', markdown: 'B.' },
    ], schema)
    const state = EditorState.create({ schema, doc: initial.doc })
    const inserted = insertBlockSpan(state.tr, initial.layout, 1, {
      blockId: 'section-middle',
      markdown: 'Middle one.\n\nMiddle two.',
    })
    expect(inserted.layout.spans).toEqual([
      { blockId: 'section-a', startIndex: 0, endIndex: 1 },
      { blockId: 'section-middle', startIndex: 1, endIndex: 3 },
      { blockId: 'section-b', startIndex: 3, endIndex: 4 },
    ])
    expect(serializeSpan(inserted.transaction.doc, inserted.layout.spans[1])).toContain('Middle two.')

    const deleted = deleteBlockSpan(inserted.transaction, inserted.layout, 'section-middle')
    expect(deleted.transaction.doc.eq(initial.doc)).toBe(true)
    expect(deleted.layout).toEqual(initial.layout)
  })

  it('keeps Enter and multi-paragraph paste inside one stable block span', () => {
    const initial = materializeBlocks([
      { blockId: 'section-a', markdown: 'Alpha.' },
      { blockId: 'section-b', markdown: 'Bravo.' },
    ], schema)
    const split = parseMarkdown('Alpha\n\ncontinued\n\nBravo.', schema)
    const plan = planLocalBlockEdit(initial.doc, split, initial.layout, 'section-a')

    expect(plan).toEqual({
      blockId: 'section-a',
      layout: {
        spans: [
          { blockId: 'section-a', startIndex: 0, endIndex: 2 },
          { blockId: 'section-b', startIndex: 2, endIndex: 3 },
        ],
      },
    })
    expect(serializeSpan(split, plan!.layout.spans[0])).toBe('Alpha\n\ncontinued')
  })

  it('keeps a paragraph join inside one block and shifts following spans back', () => {
    const initial = materializeBlocks([
      { blockId: 'section-a', markdown: 'Alpha.\n\ncontinued' },
      { blockId: 'section-b', markdown: 'Bravo.' },
    ], schema)
    const joined = parseMarkdown('Alpha continued\n\nBravo.', schema)
    const plan = planLocalBlockEdit(initial.doc, joined, initial.layout, 'section-a')

    expect(plan?.blockId).toBe('section-a')
    expect(plan?.layout.spans).toEqual([
      { blockId: 'section-a', startIndex: 0, endIndex: 1 },
      { blockId: 'section-b', startIndex: 1, endIndex: 2 },
    ])
  })

  it('does not rematerialize an acknowledged block when its Markdown already matches', () => {
    const initial = materializeBlocks([
      { blockId: 'section-a', markdown: 'Second paragraph.' },
      { blockId: 'section-b', markdown: 'Following section.' },
    ], schema)
    const splitTransaction = EditorState.create({ schema, doc: initial.doc }).tr.split(7)
    const plan = planLocalBlockEdit(initial.doc, splitTransaction.doc, initial.layout, 'section-a')!
    const markdown = serializeSpan(splitTransaction.doc, plan.layout.spans[0])
    const splitState = EditorState.create({ schema, doc: splitTransaction.doc })

    const acknowledged = replaceBlockSpans(splitState.tr, plan.layout, [
      { blockId: 'section-a', markdown },
      { blockId: 'section-b', markdown: 'Following section.' },
    ])

    expect(acknowledged.transaction.docChanged).toBe(false)
    expect(acknowledged.transaction.doc.eq(splitTransaction.doc)).toBe(true)
    expect(acknowledged.layout).toEqual(plan.layout)
  })

  it('uses the selected block to disambiguate insertion at a governed boundary', () => {
    const initial = materializeBlocks([
      { blockId: 'section-a', markdown: 'Alpha.' },
      { blockId: 'section-b', markdown: 'Bravo.' },
    ], schema)
    const inserted = parseMarkdown('Alpha.\n\nNew paragraph.\n\nBravo.', schema)

    expect(planLocalBlockEdit(initial.doc, inserted, initial.layout, null)).toBeNull()
    expect(planLocalBlockEdit(initial.doc, inserted, initial.layout, 'section-a')?.blockId).toBe('section-a')
    expect(planLocalBlockEdit(initial.doc, inserted, initial.layout, 'section-b')?.blockId).toBe('section-b')
  })

  it('rejects transactions that join or reorder different governed blocks', () => {
    const initial = materializeBlocks([
      { blockId: 'section-a', markdown: 'Alpha.' },
      { blockId: 'section-b', markdown: 'Bravo.' },
    ], schema)
    const joined = parseMarkdown('Alpha Bravo.', schema)
    const reordered = parseMarkdown('Bravo.\n\nAlpha.', schema)

    expect(planLocalBlockEdit(initial.doc, joined, initial.layout, 'section-a')).toBeNull()
    expect(planLocalBlockEdit(initial.doc, reordered, initial.layout, 'section-a')).toBeNull()
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
