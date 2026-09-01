import { beforeEach, describe, expect, it, vi } from 'vitest'

const request = vi.fn()

vi.mock('./bridge', () => ({
  bridge: () => ({ request }),
}))

import {
  loadNextSettings,
  loadWipLimit,
  normalizeDefaultDueDays,
  normalizeNextSettings,
  normalizeWipLimit,
  planningDefaults,
} from './settings'

describe('Next WIP settings', () => {
  beforeEach(() => vi.clearAllMocks())

  it.each([undefined, null, '', 0, -1, 2.5, Number.NaN, Number.POSITIVE_INFINITY, 'many'])
  ('falls back to five for an invalid WIP limit: %s', (value) => {
    expect(normalizeWipLimit(value)).toBe(5)
  })

  it.each([[1, 1], [7, 7], ['12', 12]])('accepts a positive integer WIP limit: %s', (value, expected) => {
    expect(normalizeWipLimit(value)).toBe(expected)
  })

  it('reads the plugin-scoped value from the authenticated host settings API', async () => {
    request.mockResolvedValueOnce({ settings: { wipLimit: 8 } })

    await expect(loadWipLimit()).resolves.toBe(8)
    expect(request).toHaveBeenCalledWith('host.settings.get')
  })

  it('uses the default when settings cannot be read', async () => {
    request.mockRejectedValueOnce(new Error('settings unavailable'))
    await expect(loadWipLimit()).resolves.toBe(5)
  })

  it('normalizes all planning defaults without inventing a deadline', () => {
    expect(normalizeNextSettings({
      wipLimit: 7,
      defaultPriority: 'P1',
      defaultDueDays: 0,
      defaultContext: '  @电脑  ',
    })).toEqual({ wipLimit: 7, defaultPriority: 'P1', defaultDueDays: 0, defaultContext: '@电脑' })
    expect(planningDefaults(normalizeNextSettings({}), new Date(2026, 8, 1, 9))).toEqual({
      priority: 'P2', contexts: [],
    })
  })

  it('turns a configured calendar-day offset into a fixed default due date', () => {
    const settings = normalizeNextSettings({ defaultPriority: 'P0', defaultDueDays: 7, defaultContext: '@电话' })
    expect(planningDefaults(settings, new Date(2026, 8, 1, 23, 30))).toEqual({
      priority: 'P0', due: '2026-09-08', contexts: ['@电话'],
    })
    expect(normalizeDefaultDueDays(-1)).toBe(0)
    expect(normalizeDefaultDueDays(2.5)).toBe(0)
  })

  it('loads every Next setting in one host request and falls back atomically', async () => {
    request.mockResolvedValueOnce({ settings: {
      wipLimit: 9, defaultPriority: 'P3', defaultDueDays: 3, defaultContext: '@外出',
    } })
    await expect(loadNextSettings()).resolves.toEqual({
      wipLimit: 9, defaultPriority: 'P3', defaultDueDays: 3, defaultContext: '@外出',
    })
  })
})
