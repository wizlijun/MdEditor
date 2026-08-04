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
  it('never runs a second save while one is in flight, and flush awaits the original save', async () => {
    let resolveFirst: (() => void) | null = null
    let active = 0
    let maxActive = 0
    const save = vi.fn(() => {
      active++
      maxActive = Math.max(maxActive, active)
      if (save.mock.calls.length === 1) {
        return new Promise<void>((resolve) => {
          resolveFirst = () => { active--; resolve() }
        })
      }
      active--
      return Promise.resolve()
    })
    const a = createAutosave(save, 1500)

    // 定时器触发,第一次 save() 开始飞,但不 resolve
    a.schedule()
    await vi.advanceTimersByTimeAsync(1500)
    expect(save).toHaveBeenCalledTimes(1)
    expect(resolveFirst).not.toBeNull()

    // 用户继续打字:重新 pending,挂新定时器(不等它触发)
    a.schedule()

    // 在第一次 save 仍未 resolve 时调用 flush:必须等原始那次,而不是立刻多发一次
    let flushSettled = false
    const flushPromise = a.flush().then(() => { flushSettled = true })

    // 让微任务队列走几轮,确认 flush 没有提前 resolve、也没有并发发起第二次 save
    for (let i = 0; i < 5; i++) await Promise.resolve()
    expect(flushSettled).toBe(false)
    expect(save).toHaveBeenCalledTimes(1) // 还没有并发的第二次调用

    // 放行第一次 save
    resolveFirst!()
    await flushPromise

    // flush 补发了一次(因为 flush 前又 schedule 过),但两次从未同时在飞
    expect(save).toHaveBeenCalledTimes(2)
    expect(flushSettled).toBe(true)
    expect(maxActive).toBe(1)

    // 定时器早前已被 flush 取消,不会再触发第三次
    await vi.advanceTimersByTimeAsync(3000)
    expect(save).toHaveBeenCalledTimes(2)
  })
  it('dispose prevents a later schedule from reviving the instance', async () => {
    const save = vi.fn().mockResolvedValue(undefined)
    const a = createAutosave(save, 1500)
    a.dispose()
    a.schedule()
    await vi.advanceTimersByTimeAsync(3000)
    expect(save).not.toHaveBeenCalled()
  })
})
