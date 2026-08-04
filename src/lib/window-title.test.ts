import { describe, it, expect } from 'vitest'
import { windowTitleFor } from './window-title'

describe('windowTitleFor', () => {
  it('is the bare app name when no single document is showing', () => {
    expect(windowTitleFor(null, false)).toBe('note.md')
    expect(windowTitleFor(null, true)).toBe('note.md')
  })
  it('shows the document name', () => {
    expect(windowTitleFor('foo.md', false)).toBe('foo.md — note.md')
  })
  it('marks a mirrored source with ↔', () => {
    expect(windowTitleFor('foo.md', true)).toBe('↔ foo.md — note.md')
  })
})
