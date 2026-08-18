import { describe, it, expect } from 'vitest'
import { getMenuModel } from './menu-model'

describe('getMenuModel', () => {
  it('always includes a clipboard group and the emphasis items', () => {
    const groups = getMenuModel({ hasSelection: true })
    const ids = groups.flatMap(g => g.items.map(i => i.id))
    expect(ids).toContain('cut')
    expect(ids).toContain('highlight')
    expect(ids).toContain('wikilink')
  })

  it('read-only offers nothing that would mutate the document', () => {
    // mdx renders read-only. Every mutating command reached the doc anyway via
    // view.dispatch (editable:false only blocks DOM input), and the change was
    // then dropped — the user watched their annotation appear and vanish.
    const groups = getMenuModel({ hasSelection: true, readOnly: true })
    const ids = groups.flatMap(g => g.items.map(i => i.id))
    expect(ids).toEqual(['copy', 'selectAll'])
  })

  it('marks question/note/highlight/wikilink as emphasis, question first, before other marks', () => {
    const groups = getMenuModel({ hasSelection: true })
    const emphasis = groups.find(g => g.id === 'emphasis')!
    expect(emphasis.items.map(i => i.id)).toEqual(['question', 'trace', 'note', 'highlight', 'wikilink'])
    expect(emphasis.items.every(i => i.emphasis)).toBe(true)
    expect(emphasis.items.map(i => i.icon)).toEqual(['question', 'trace', 'sparkle', 'highlight', 'wikilink'])
  })

  it('trace requires a selection', () => {
    const emphasis = getMenuModel({ hasSelection: false }).find(g => g.id === 'emphasis')!
    const trace = emphasis.items.find(i => i.id === 'trace')!
    expect(trace.needsSelection).toBe(true)
    expect(trace.icon).toBe('trace')
  })

  it('question works with or without a selection, like note', () => {
    const groups = getMenuModel({ hasSelection: false })
    const q = groups.flatMap(g => g.items).find(i => i.id === 'question')!
    expect(q.needsSelection).toBeUndefined()
  })

  it('note works with or without a selection', () => {
    const groups = getMenuModel({ hasSelection: false })
    const note = groups.flatMap(g => g.items).find(i => i.id === 'note')!
    expect(note).toBeDefined()
    expect(note.needsSelection).toBeUndefined()
  })

  it('flags link-from-text as needing a selection', () => {
    const groups = getMenuModel({ hasSelection: false })
    const link = groups.flatMap(g => g.items).find(i => i.id === 'link')!
    expect(link.needsSelection).toBe(true)
  })

  it('exposes block and insert submenus with children', () => {
    const groups = getMenuModel({ hasSelection: false })
    const all = groups.flatMap(g => g.items)
    expect(all.find(i => i.id === 'heading')!.children!.map(c => c.id))
      .toEqual(['h1', 'h2', 'h3'])
    expect(all.find(i => i.id === 'list')!.children!.map(c => c.id))
      .toEqual(['bullet', 'ordered', 'task'])
    expect(all.find(i => i.id === 'insert')!.children!.map(c => c.id))
      .toEqual(['table', 'image', 'math', 'mermaid', 'date'])
  })
})
