import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { createAutosave } from './autosave'

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

describe('createAutosave', () => {
  it('saves once after the user stops typing', async () => {
    const save = vi.fn().mockResolvedValue(undefined)
    const a = createAutosave(save, 1500)
    a.schedule(); a.schedule(); a.schedule()
    expect(save).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(1500)
    expect(save).toHaveBeenCalledTimes(1)
  })
  it('flush writes immediately and cancels the pending timer', async () => {
    const save = vi.fn().mockResolvedValue(undefined)
    const a = createAutosave(save, 1500)
    a.schedule()
    await a.flush()
    expect(save).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(3000)
    expect(save).toHaveBeenCalledTimes(1) // 定时器没有再打一次
  })
  it('flush without pending work does not call save', async () => {
    const save = vi.fn().mockResolvedValue(undefined)
    await createAutosave(save, 1500).flush()
    expect(save).not.toHaveBeenCalled()
  })
  it('keeps working after save throws', async () => {
    const save = vi.fn().mockRejectedValueOnce(new Error('disk full')).mockResolvedValue(undefined)
    const a = createAutosave(save, 1500)
    a.schedule()
    await vi.advanceTimersByTimeAsync(1500)
    a.schedule()
    await vi.advanceTimersByTimeAsync(1500)
    expect(save).toHaveBeenCalledTimes(2)
  })
  it('dispose cancels without saving', async () => {
    const save = vi.fn().mockResolvedValue(undefined)
    const a = createAutosave(save, 1500)
    a.schedule()
    a.dispose()
    await vi.advanceTimersByTimeAsync(3000)
    expect(save).not.toHaveBeenCalled()
  })
})
