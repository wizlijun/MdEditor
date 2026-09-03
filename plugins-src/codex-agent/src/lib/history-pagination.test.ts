import { describe, expect, it } from 'vitest'
import { nextHistoryBatchSize, nextHistoryCount } from './history-pagination'

describe('recent history pagination', () => {
  it('reveals five at a time and clamps the final batch', () => {
    expect(nextHistoryCount(5, 11)).toBe(10)
    expect(nextHistoryBatchSize(5, 11)).toBe(5)
    expect(nextHistoryCount(10, 11)).toBe(11)
    expect(nextHistoryBatchSize(10, 11)).toBe(1)
    expect(nextHistoryBatchSize(5, 5)).toBe(0)
  })
})
