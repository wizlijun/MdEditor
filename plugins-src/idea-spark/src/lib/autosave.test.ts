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
  it('two concurrent flushes both wait for the LAST save to land', async () => {
    // 回归:settle 曾经只等一次 in-flight、然后看一眼 pending 就返回。两个 flush
    // 同时等同一次在飞的保存时,先醒的消费掉 pending 并补发第二次,后醒的看到
    // pending===false 就提前 resolve —— 调用方据此换掉编辑器内容,补发那次随后
    // 落地,把旧文档的文件名写回 store,新草稿于是覆盖了旧 idea。
    let resolveFirst: (() => void) | null = null
    let secondLanded = false
    const save = vi.fn(() => {
      if (save.mock.calls.length === 1) {
        return new Promise<void>((resolve) => { resolveFirst = resolve })
      }
      return Promise.resolve().then(() => Promise.resolve()).then(() => { secondLanded = true })
    })
    const a = createAutosave(save, 1500)

    // 第一次 save 起飞并卡住(慢盘/网络盘)
    a.schedule()
    await vi.advanceTimersByTimeAsync(1500)
    expect(save).toHaveBeenCalledTimes(1)

    // 用户又打了字,然后两个强制写盘点接连发生(Cmd+S,紧接着「新想法」)
    a.schedule()
    // 每个 flush **在自己 resolve 的那一刻**看到的世界 —— 不能等 Promise.all
    // 之后再看 `secondLanded`:那时先醒的那个 flush 早就把第二次 save 等完了,
    // 提前放行的那个也会被它掩护过去,断言就测不出东西。
    let f1Saw: boolean | null = null
    let f2Saw: boolean | null = null
    const f1 = a.flush().then(() => { f1Saw = secondLanded })
    const f2 = a.flush().then(() => { f2Saw = secondLanded })

    for (let i = 0; i < 5; i++) await Promise.resolve()
    expect(f1Saw).toBeNull()
    expect(f2Saw).toBeNull()

    resolveFirst!()
    await Promise.all([f1, f2])

    // 两个 flush 都必须在补发的第二次 save 落地之后才 resolve。
    // 旧实现下 f2Saw === false:它看到 pending 已被 f1 消费掉就直接返回了。
    expect(save).toHaveBeenCalledTimes(2)
    expect(f1Saw).toBe(true)
    expect(f2Saw).toBe(true)
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
