export const HISTORY_PAGE_SIZE = 5

export function nextHistoryCount(current: number, total: number): number {
  return Math.min(total, current + HISTORY_PAGE_SIZE)
}

export function nextHistoryBatchSize(current: number, total: number): number {
  return Math.min(HISTORY_PAGE_SIZE, Math.max(0, total - current))
}
