import { describe, it, expect } from 'vitest'
import { applyWrap, expandToWord, insertNoteMarkup } from './text-format'

describe('applyWrap', () => {
  it('wraps a selection', () => {
    const r = applyWrap('foo bar baz', 4, 7, '**', '**')
    expect(r.value).toBe('foo **bar** baz')
    expect(r.selStart).toBe(6)
    expect(r.selEnd).toBe(9)
  })

  it('unwraps when the selection itself includes the markers', () => {
    const r = applyWrap('foo **bar** baz', 4, 11, '**', '**')
    expect(r.value).toBe('foo bar baz')
    expect(r.selStart).toBe(4)
    expect(r.selEnd).toBe(7)
  })

  it('unwraps when markers sit just outside the selection', () => {
    const r = applyWrap('foo **bar** baz', 6, 9, '**', '**')
    expect(r.value).toBe('foo bar baz')
    expect(r.selStart).toBe(4)
    expect(r.selEnd).toBe(7)
  })

  it('inserts empty markers on a collapsed selection', () => {
    const r = applyWrap('foo ', 4, 4, '**', '**')
    expect(r.value).toBe('foo ****')
    expect(r.selStart).toBe(6)
    expect(r.selEnd).toBe(6)
  })
})

describe('expandToWord', () => {
  it('expands to the ascii word under the cursor', () => {
    expect(expandToWord('foo bar baz', 5)).toEqual({ start: 4, end: 7 })
  })
  it('expands to a CJK run', () => {
    expect(expandToWord('你好世界', 2)).toEqual({ start: 0, end: 4 })
  })
  it('returns the cursor collapsed when not on a word', () => {
    expect(expandToWord('foo   bar', 4)).toEqual({ start: 4, end: 4 })
  })
})

describe('insertNoteMarkup', () => {
  it('wraps a selection and puts the caret inside the note', () => {
    const r = insertNoteMarkup('hello world', 6, 11)
    expect(r.value).toBe('hello {==world==}{>><<}')
    expect(r.selStart).toBe(r.value.length - 3)
    expect(r.selEnd).toBe(r.selStart)
  })

  it('inserts a bare point annotation on a collapsed selection', () => {
    const r = insertNoteMarkup('hello', 5, 5)
    expect(r.value).toBe('hello{>><<}')
    expect(r.selStart).toBe('hello{>>'.length)
    expect(r.selEnd).toBe(r.selStart)
  })

  it('inserts mid-text without touching surroundings', () => {
    const r = insertNoteMarkup('abcd', 2, 2)
    expect(r.value).toBe('ab{>><<}cd')
    expect(r.selStart).toBe('ab{>>'.length)
  })

  it('seeds the note and leaves the caret before the seed (collapsed)', () => {
    const r = insertNoteMarkup('hello', 5, 5, '?')
    expect(r.value).toBe('hello{>>?<<}')
    expect(r.selStart).toBe('hello{>>'.length)
    expect(r.selEnd).toBe(r.selStart)
  })

  it('seeds the note when wrapping a selection', () => {
    const r = insertNoteMarkup('hello world', 6, 11, '?')
    expect(r.value).toBe('hello {==world==}{>>?<<}')
    expect(r.selStart).toBe('hello {==world==}{>>'.length)
    expect(r.selEnd).toBe(r.selStart)
  })
})
