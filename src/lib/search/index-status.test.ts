import { describe, it, expect, vi, beforeEach } from 'vitest'

// `subscribe()` calls the real `listen()` from `@tauri-apps/api/event`, which
// has no Tauri host to talk to under vitest. Mocked so each call can be
// driven by hand: captured (event name + handler), with its own `resolve` so
// a test controls *when* the returned unlisten function becomes available —
// that timing is exactly what the post-teardown race test below depends on.
// `vi.hoisted` is required (not a plain top-level `const`) because `vi.mock`
// factories run before ordinary statements, including the static
// `import ... from './index-status.svelte'` below, which is what actually
// triggers this mock to load.
const { listenMock, getListenCalls, resetListenCalls } = vi.hoisted(() => {
  type Handler = (event: { payload: unknown }) => void
  let calls: { event: string; handler: Handler; resolve: (un: () => void) => void }[] = []
  const listenMock = vi.fn((event: string, handler: Handler) =>
    new Promise<() => void>((resolve) => {
      calls.push({ event, handler, resolve })
    }))
  return {
    listenMock,
    getListenCalls: () => calls,
    resetListenCalls: () => { calls = [] },
  }
})
vi.mock('@tauri-apps/api/event', () => ({ listen: listenMock }))

import { indexStatus, _setIndexApi } from './index-status.svelte'

beforeEach(() => {
  indexStatus.reset()
  listenMock.mockClear()
  resetListenCalls()
  _setIndexApi({ stats: async () => null, progress: async () => null, rebuild: async () => {} })
})

describe('indexStatus', () => {
  it('中途打开时从 progress() 拉到当前进度,而不是空等下一个事件', async () => {
    _setIndexApi({
      stats: async () => ({ files: 10, blocks: 40, dbBytes: 1024, builtAt: null, tokenizerId: 'v1' }),
      progress: async () => ({ phase: 'indexing', done: 3, total: 10, current: 'a.md', elapsedMs: 5 }),
      rebuild: async () => {},
    })
    await indexStatus.refresh()
    expect(indexStatus.progress?.done).toBe(3)
    expect(indexStatus.stats?.files).toBe(10)
  })

  it('进度事件覆盖轮询到的快照', async () => {
    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild: async () => {} })
    indexStatus.applyProgress({ phase: 'indexing', done: 8, total: 10, current: 'b.md', elapsedMs: 9 })
    expect(indexStatus.progress?.done).toBe(8)
  })

  it('完成事件清空进度,避免停在 100% 不动', async () => {
    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild: async () => {} })
    indexStatus.applyProgress({ phase: 'indexing', done: 9, total: 10, current: null, elapsedMs: 9 })
    indexStatus.applyProgress({ phase: 'done', done: 10, total: 10, current: null, elapsedMs: 10 })
    expect(indexStatus.progress).toBeNull()
  })

  // 索引未就绪是启动期的正常状态,不是崩溃 —— 面板必须说人话。
  it('把后端的 not ready 呈现为状态而不是报错串', async () => {
    _setIndexApi({
      stats: async () => { throw new Error('search index not ready') },
      progress: async () => null,
      rebuild: async () => {},
    })
    await indexStatus.refresh()
    expect(indexStatus.notReady).toBe(true)
    expect(indexStatus.error).toBeNull()
  })
})

describe('indexStatus.subscribe', () => {
  it('routes a search://progress event to applyProgress', async () => {
    indexStatus.subscribe()
    await Promise.resolve() // let subscribe()'s synchronous listen() calls land
    const call = getListenCalls().find((c) => c.event === 'search://progress')
    expect(call).toBeTruthy()
    call!.resolve(() => {})
    await Promise.resolve()

    call!.handler({ payload: { phase: 'indexing', done: 5, total: 10, current: 'x.md', elapsedMs: 1 } })
    expect(indexStatus.progress?.done).toBe(5)
  })

  it('routes a search://index-updated event to a refresh()', async () => {
    const refreshSpy = vi.spyOn(indexStatus, 'refresh')
    indexStatus.subscribe()
    await Promise.resolve()
    const call = getListenCalls().find((c) => c.event === 'search://index-updated')
    expect(call).toBeTruthy()
    call!.resolve(() => {})
    await Promise.resolve()

    call!.handler({ payload: undefined })
    expect(refreshSpy).toHaveBeenCalled()
    refreshSpy.mockRestore()
  })

  it('the returned teardown unlistens both the progress and index-updated listeners', async () => {
    const unlistenProgress = vi.fn()
    const unlistenUpdated = vi.fn()
    const unsubscribe = indexStatus.subscribe()
    await Promise.resolve()
    getListenCalls().find((c) => c.event === 'search://progress')!.resolve(unlistenProgress)
    getListenCalls().find((c) => c.event === 'search://index-updated')!.resolve(unlistenUpdated)
    await Promise.resolve()

    unsubscribe()
    expect(unlistenProgress).toHaveBeenCalledTimes(1)
    expect(unlistenUpdated).toHaveBeenCalledTimes(1)
  })

  // The race the reviewer flagged: teardown can run BEFORE `listen()`'s
  // promise resolves (e.g. the settings tab is switched away in the same
  // tick it was switched to). A `listen()` that resolves after that must
  // still be unlistened immediately rather than being left live — otherwise
  // switching tabs repeatedly accumulates listeners, each firing an extra
  // `applyProgress`/`refresh()` for events the panel no longer displays.
  it('a listen() that resolves after teardown does not leave a live listener', async () => {
    const unlistenProgress = vi.fn()
    const unlistenUpdated = vi.fn()
    const unsubscribe = indexStatus.subscribe()
    await Promise.resolve()

    unsubscribe() // torn down before either listen() promise has resolved

    getListenCalls().find((c) => c.event === 'search://progress')!.resolve(unlistenProgress)
    getListenCalls().find((c) => c.event === 'search://index-updated')!.resolve(unlistenUpdated)
    await Promise.resolve()

    expect(unlistenProgress).toHaveBeenCalledTimes(1)
    expect(unlistenUpdated).toHaveBeenCalledTimes(1)
  })
})
