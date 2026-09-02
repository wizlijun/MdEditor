import { beforeEach, describe, expect, it, vi } from 'vitest'

const hostRequest = vi.fn()

vi.mock('./bridge', () => ({
  bridge: () => ({ request: hostRequest }),
}))

import {
  loadMaxConcurrency,
  loadUsageDisplay,
  normalizeMaxConcurrency,
  normalizeUsageDisplay,
  saveMaxConcurrency,
  saveUsageDisplay,
} from './settings'

describe('agent concurrency settings', () => {
  beforeEach(() => hostRequest.mockReset())

  it('defaults invalid or missing values to one and clamps to 1–5', () => {
    expect(normalizeMaxConcurrency(undefined)).toBe(1)
    expect(normalizeMaxConcurrency('nope')).toBe(1)
    expect(normalizeMaxConcurrency(0)).toBe(1)
    expect(normalizeMaxConcurrency('3')).toBe(3)
    expect(normalizeMaxConcurrency(9)).toBe(5)
  })

  it('loads only this plugin scope through the host settings bridge', async () => {
    hostRequest.mockResolvedValue({ settings: { maxConcurrency: '4' } })
    await expect(loadMaxConcurrency()).resolves.toBe(4)
    expect(hostRequest).toHaveBeenCalledWith('host.settings.get')
  })

  it('persists the normalized value under the local maxConcurrency key', async () => {
    hostRequest.mockResolvedValue({ ok: true })
    await expect(saveMaxConcurrency(9)).resolves.toBe(5)
    expect(hostRequest).toHaveBeenCalledWith('host.settings.set', {
      key: 'maxConcurrency',
      value: '5',
    })
  })

  it('defaults usage display to tip and accepts result', () => {
    expect(normalizeUsageDisplay(undefined)).toBe('tip')
    expect(normalizeUsageDisplay('toast')).toBe('tip')
    expect(normalizeUsageDisplay('result')).toBe('result')
  })

  it('loads and saves usage display in this plugin scope', async () => {
    hostRequest.mockResolvedValueOnce({ settings: { usageDisplay: 'result' } })
    await expect(loadUsageDisplay()).resolves.toBe('result')
    hostRequest.mockResolvedValueOnce({ ok: true })
    await expect(saveUsageDisplay('result')).resolves.toBe('result')
    expect(hostRequest).toHaveBeenLastCalledWith('host.settings.set', {
      key: 'usageDisplay',
      value: 'result',
    })
  })
})
