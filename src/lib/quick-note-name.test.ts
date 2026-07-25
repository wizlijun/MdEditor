import { describe, it, expect } from 'vitest'
import {
  quickNoteFileName,
  isAutoQuickNoteName,
  titleSlug,
  quickNoteRenameTarget,
} from './quick-note-name'

describe('quickNoteFileName', () => {
  it('formats YYYY-MM-DD-HHmmss-quick.md', () => {
    expect(quickNoteFileName(new Date(2026, 6, 25, 19, 30, 45))).toBe('2026-07-25-193045-quick.md')
  })

  it('zero-pads every field', () => {
    expect(quickNoteFileName(new Date(2026, 0, 2, 3, 4, 5))).toBe('2026-01-02-030405-quick.md')
  })
})

describe('isAutoQuickNoteName', () => {
  it('accepts the generated name', () => {
    expect(isAutoQuickNoteName('2026-07-25-193045-quick.md')).toBe(true)
  })

  it('rejects already-renamed and unrelated names', () => {
    expect(isAutoQuickNoteName('2026-07-25-产品思考.md')).toBe(false)
    expect(isAutoQuickNoteName('quick.md')).toBe(false)
    expect(isAutoQuickNoteName('2026-07-25-19-30-Quick.md')).toBe(false)
  })
})

describe('titleSlug', () => {
  it('keeps CJK verbatim', () => {
    expect(titleSlug('产品思考')).toBe('产品思考')
  })

  it('turns whitespace runs into single dashes', () => {
    expect(titleSlug('hello   world  again')).toBe('hello-world-again')
  })

  it('replaces filesystem-illegal characters', () => {
    expect(titleSlug('a/b:c*d?e"f<g>h|i')).toBe('a-b-c-d-e-f-g-h-i')
  })

  it('trims leading and trailing dashes', () => {
    expect(titleSlug('  spaced  ')).toBe('spaced')
  })

  it('caps long titles without leaving a trailing dash', () => {
    const slug = titleSlug('x'.repeat(80))
    expect(slug).toBe('x'.repeat(50))
  })

  it('returns null when nothing usable survives', () => {
    expect(titleSlug('///')).toBeNull()
    expect(titleSlug('   ')).toBeNull()
  })
})

describe('quickNoteRenameTarget', () => {
  it('renames an auto-named note to date + H1 slug, dropping the time', () => {
    expect(quickNoteRenameTarget('2026-07-25-193045-quick.md', '# 产品思考\n\nbody'))
      .toBe('2026-07-25-产品思考.md')
  })

  it('uses the first H1 only', () => {
    expect(quickNoteRenameTarget('2026-07-25-193045-quick.md', '# One\n\n# Two'))
      .toBe('2026-07-25-One.md')
  })

  it('ignores deeper headings', () => {
    expect(quickNoteRenameTarget('2026-07-25-193045-quick.md', '## Sub\n\ntext')).toBeNull()
  })

  it('is a no-op without an H1', () => {
    expect(quickNoteRenameTarget('2026-07-25-193045-quick.md', 'just text')).toBeNull()
  })

  it('renames only once — an already-renamed note is left alone', () => {
    expect(quickNoteRenameTarget('2026-07-25-产品思考.md', '# 改了标题')).toBeNull()
  })

  it('leaves non-quick-note files alone', () => {
    expect(quickNoteRenameTarget('notes.md', '# Title')).toBeNull()
  })

  it('still renames when the title happens to be "quick" — the time is dropped', () => {
    expect(quickNoteRenameTarget('2026-07-25-193045-quick.md', '# quick'))
      .toBe('2026-07-25-quick.md')
  })
})
