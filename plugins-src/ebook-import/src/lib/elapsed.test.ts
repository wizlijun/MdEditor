import { describe, expect, it } from 'vitest'
import { formatElapsed } from './elapsed'

const start = '2026-08-26T04:00:00Z'
const at = (secs: number) => Date.parse(start) + secs * 1000

describe('formatElapsed', () => {
  it('counts seconds under a minute', () => {
    expect(formatElapsed(start, at(0))).toBe('0s')
    expect(formatElapsed(start, at(42))).toBe('42s')
  })

  it('counts minutes and seconds past a minute', () => {
    expect(formatElapsed(start, at(60))).toBe('1m0s')
    expect(formatElapsed(start, at(192))).toBe('3m12s')
  })

  // Reading a big book really does take hours, and "187m" is unreadable.
  it('counts hours and minutes past an hour', () => {
    expect(formatElapsed(start, at(3600))).toBe('1h0m')
    expect(formatElapsed(start, at(3600 * 3 + 60 * 7 + 30))).toBe('3h7m')
  })

  // The start time comes from the backend (UTC) and `now` from the window's
  // clock. They can disagree by a second or two; a run must never read as
  // having started in the future.
  it('never goes negative on a clock that disagrees', () => {
    expect(formatElapsed(start, at(-5))).toBe('0s')
  })

  it('is empty when nothing has started', () => {
    expect(formatElapsed(undefined, at(60))).toBe('')
    expect(formatElapsed('not a date', at(60))).toBe('')
  })
})
