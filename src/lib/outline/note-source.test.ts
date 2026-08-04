import { describe, it, expect } from 'vitest'
import { mainPathOfNote, sourcesForNote } from './note-source'

describe('mainPathOfNote', () => {
  it('maps a companion note back to its document', () => {
    expect(mainPathOfNote('/v/sync/foo.note.md')).toBe('/v/sync/foo.md')
    expect(mainPathOfNote('/v/sync/foo.notes.md')).toBe('/v/sync/foo.md')
  })
  it('returns null for a standalone outline note', () => {
    expect(mainPathOfNote('/v/wikipage/某页.md')).toBeNull()
  })
})

describe('sourcesForNote', () => {
  const resolve = (p: string) => (p === '/v/sync/foo.md' ? '/Users/me/Desktop/foo.md' : null)

  it('records the mirrored original as the note source (§5.1)', () => {
    expect(sourcesForNote('/v/sync/foo.note.md', resolve))
      .toEqual([{ resource: '/Users/me/Desktop/foo.md' }])
  })
  it('is undefined when the document is not a mirror', () => {
    expect(sourcesForNote('/v/notes/bar.note.md', resolve)).toBeUndefined()
  })
  it('is undefined for a note with no companion document', () => {
    expect(sourcesForNote('/v/wikipage/页.md', resolve)).toBeUndefined()
  })
})
