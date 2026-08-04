import { describe, it, expect } from 'vitest'
import { pickPlaceholder, placeholderLines, PLACEHOLDER_KEYS } from './placeholder'
import { setLocale } from './strings'

describe('pickPlaceholder', () => {
  const lines = ['a', 'b', 'c', 'd', 'e']
  it('cycles through every line before repeating', () => {
    expect([0, 1, 2, 3, 4].map((n) => pickPlaceholder(lines, n))).toEqual(lines)
    expect(pickPlaceholder(lines, 5)).toBe('a')
    expect(pickPlaceholder(lines, 12)).toBe('c')
  })
  it('survives a negative or fractional counter without throwing', () => {
    expect(typeof pickPlaceholder(lines, -1)).toBe('string')
    expect(typeof pickPlaceholder(lines, 2.7)).toBe('string')
  })
  it('returns an empty string for an empty pool', () => {
    expect(pickPlaceholder([], 3)).toBe('')
  })
})

describe('placeholderLines', () => {
  it('gives five non-empty lines in every locale, none ending in a full stop', () => {
    for (const locale of ['en', 'zh', 'ja', 'de'] as const) {
      setLocale(locale)
      const lines = placeholderLines()
      expect(lines, locale).toHaveLength(PLACEHOLDER_KEYS.length)
      for (const line of lines) {
        expect(line.trim().length, `${locale}: ${line}`).toBeGreaterThan(0)
        expect(/[。.!!??]$/.test(line.trim()), `${locale} 不该以句号结尾: ${line}`).toBe(false)
      }
    }
  })
})
