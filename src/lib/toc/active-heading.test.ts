import { describe, expect, it } from 'vitest'
import {
  isScrollAtEnd,
  resolveActiveHeadingIndex,
  sourceViewportAnchorLine,
} from './active-heading'

describe('resolveActiveHeadingIndex', () => {
  const headings = [
    { headingIndex: 0, position: 2 },
    { headingIndex: 2, position: 6 },
    { headingIndex: 3, position: 10 },
  ]

  it('returns null for no headings or before the first heading', () => {
    expect(resolveActiveHeadingIndex([], 20)).toBeNull()
    expect(resolveActiveHeadingIndex(headings, 1)).toBeNull()
  })

  it('activates a heading at its boundary and keeps it between headings', () => {
    expect(resolveActiveHeadingIndex(headings, 2)).toBe(0)
    expect(resolveActiveHeadingIndex(headings, 5)).toBe(0)
    expect(resolveActiveHeadingIndex(headings, 6)).toBe(2)
    expect(resolveActiveHeadingIndex(headings, 9)).toBe(2)
    expect(resolveActiveHeadingIndex(headings, 12)).toBe(3)
  })

  it('uses stable indices with gaps instead of heading text', () => {
    expect(resolveActiveHeadingIndex([
      { headingIndex: 0, position: 10 },
      { headingIndex: 4, position: 30 },
    ], 30)).toBe(4)
  })

  it('selects the final heading at the bottom of a short last section', () => {
    expect(resolveActiveHeadingIndex(headings, 7, true)).toBe(3)
  })
})

describe('source viewport metrics', () => {
  it('converts the viewport midpoint to a 1-based source line', () => {
    expect(sourceViewportAnchorLine(0, 100, 20, 10)).toBe(3)
    expect(sourceViewportAnchorLine(100, 100, 20, 10)).toBe(8)
  })

  it('handles unsettled metrics and bottom tolerance', () => {
    expect(sourceViewportAnchorLine(0, 100, 0)).toBe(0)
    expect(isScrollAtEnd(199, 100, 300)).toBe(true)
    expect(isScrollAtEnd(0, 100, 0)).toBe(false)
    expect(isScrollAtEnd(0, 100, 100)).toBe(false)
  })
})
