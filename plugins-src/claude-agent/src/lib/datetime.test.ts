import { describe, it, expect } from 'vitest'
import { fmtShort, fmtFull } from './datetime'

// The backend stamps run times in UTC (`chrono::Utc::now().to_rfc3339()`).
// The UI must render them in the user's LOCAL timezone, not slice the UTC
// clock verbatim. The timezone-independent invariant: two spellings of the
// SAME instant (one in UTC, one with an explicit offset) must format to the
// exact same local string — which only holds if we parse the instant instead
// of slicing text.
const UTC = '2026-07-31T00:42:33Z'
const SAME_INSTANT_PLUS8 = '2026-07-31T08:42:33+08:00'

describe('fmtShort', () => {
  it('renders MM-DD HH:mm', () => {
    expect(fmtShort(UTC)).toMatch(/^\d{2}-\d{2} \d{2}:\d{2}$/)
  })

  it('formats the same instant identically regardless of offset spelling', () => {
    expect(fmtShort(UTC)).toBe(fmtShort(SAME_INSTANT_PLUS8))
  })

  it('returns the input unchanged when it is not a valid date', () => {
    expect(fmtShort('not-a-date')).toBe('not-a-date')
  })
})

describe('fmtFull', () => {
  it('renders YYYY-MM-DD HH:mm:ss', () => {
    expect(fmtFull(UTC)).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/)
  })

  it('formats the same instant identically regardless of offset spelling', () => {
    expect(fmtFull(UTC)).toBe(fmtFull(SAME_INSTANT_PLUS8))
  })

  it('returns the input unchanged when it is not a valid date', () => {
    expect(fmtFull('not-a-date')).toBe('not-a-date')
  })
})
