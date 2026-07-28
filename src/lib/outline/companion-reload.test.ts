import { describe, it, expect } from 'vitest'
import { decideCompanionReload } from './companion-reload'

describe('decideCompanionReload', () => {
  it('ignores our own write echo (hash unchanged)', () => {
    expect(decideCompanionReload({ diskHash: 'h', lastHash: 'h', dirty: false })).toBe('ignore')
  })

  it('ignores the echo even while dirty — same bytes are not a conflict', () => {
    expect(decideCompanionReload({ diskHash: 'h', lastHash: 'h', dirty: true })).toBe('ignore')
  })

  it('reloads silently when the note changed and we have nothing unsaved', () => {
    expect(decideCompanionReload({ diskHash: 'new', lastHash: 'old', dirty: false })).toBe('reload')
  })

  it('raises a conflict instead of overwriting unsaved local edits', () => {
    expect(decideCompanionReload({ diskHash: 'new', lastHash: 'old', dirty: true })).toBe('conflict')
  })

  it('with no baseline, still reloads when clean', () => {
    expect(decideCompanionReload({ diskHash: 'new', lastHash: null, dirty: false })).toBe('reload')
  })

  it('with no baseline and dirty, prefers the conflict banner', () => {
    expect(decideCompanionReload({ diskHash: 'new', lastHash: null, dirty: true })).toBe('conflict')
  })
})
