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

import { indexStatus, _setIndexApi, estimateRebuildSeconds, elideMiddle, formatElapsedMs } from './index-status.svelte'
import type { SearchStats } from './api'

beforeEach(() => {
  indexStatus.reset()
  listenMock.mockClear()
  resetListenCalls()
  _setIndexApi({ stats: async () => null, progress: async () => null, rebuild: async () => {} })
})

describe('indexStatus', () => {
  it('中途打开时从 progress() 拉到当前进度,而不是空等下一个事件', async () => {
    _setIndexApi({
      stats: async () => ({
        files: 10, blocks: 40, dbBytes: 1024, builtAt: null, tokenizerId: 'v1',
        skippedLarge: [{ path: 'big.md', sizeBytes: 2_000_000 }],
        originCounts: { human: 2, derived: 6, source: 2 }, typeCounts: {},
      }),
      progress: async () => ({ phase: 'indexing', done: 3, total: 10, current: 'a.md', elapsedMs: 5 }),
      rebuild: async () => {},
    })
    await indexStatus.refresh()
    expect(indexStatus.progress?.done).toBe(3)
    expect(indexStatus.stats?.files).toBe(10)
    expect(indexStatus.stats?.skippedLarge).toEqual([{ path: 'big.md', sizeBytes: 2_000_000 }])
  })

  // The property `refresh()` actually claims, and the one the old
  // `Promise.all([stats(), progress()])` quietly broke: `notemd_search_stats`
  // takes the index lock, which a rebuild holds for its whole duration;
  // `notemd_search_progress` exists precisely so progress stays readable
  // during that window. Mocking both as instantly-resolving (as every other
  // test here does) cannot tell the two implementations apart — bundling and
  // splitting look identical when nothing blocks. So this test makes
  // `stats()` hang exactly the way the real command does under a rebuild,
  // and asserts progress lands anyway, without awaiting the hung read.
  it('stats() 卡在索引锁上时,progress 仍然照常落地(不被打包等待拖住)', async () => {
    let releaseStats: (v: SearchStats | null) => void = () => {}
    _setIndexApi({
      stats: () => new Promise<SearchStats | null>((resolve) => { releaseStats = resolve }),
      progress: async () => ({ phase: 'indexing', done: 42, total: 600, current: 'a.md', elapsedMs: 7 }),
      rebuild: async () => {},
    })

    const pending = indexStatus.refresh() // 故意不 await:重建期间它本就不会返回
    await Promise.resolve()
    await Promise.resolve()

    expect(indexStatus.progress?.done).toBe(42)
    expect(indexStatus.stats).toBeNull() // 锁还没放,统计当然还没有
    expect(indexStatus.loading).toBe(true)

    releaseStats({
      files: 600, blocks: 600, dbBytes: 4096, builtAt: null, tokenizerId: 'v1', skippedLarge: [],
      originCounts: { human: 100, derived: 400, source: 100 }, typeCounts: {},
    })
    await pending
    expect(indexStatus.stats?.files).toBe(600)
    expect(indexStatus.loading).toBe(false)
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

describe('indexStatus.requestRebuild', () => {
  it('确认对话框取消时不触发重建', async () => {
    const rebuild = vi.fn()
    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild })
    await indexStatus.requestRebuild(async () => false) // 用户点了取消
    expect(rebuild).not.toHaveBeenCalled()
  })

  it('确认后才触发重建', async () => {
    const rebuild = vi.fn()
    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild })
    await indexStatus.requestRebuild(async () => true)
    expect(rebuild).toHaveBeenCalledTimes(1)
  })

  // 后端已在跑时会返回 rebuild already running —— 这不是崩溃,要说人话。
  it('把已在运行呈现为提示而不是错误', async () => {
    _setIndexApi({
      stats: async () => null, progress: async () => null,
      rebuild: async () => { throw new Error('rebuild already running') },
    })
    await indexStatus.requestRebuild(async () => true)
    expect(indexStatus.busyNotice).toBe(true)
    expect(indexStatus.error).toBeNull()
  })

  it('把已在运行以外的失败呈现为 rebuildError,而不是静默吞掉或误标成 busyNotice', async () => {
    _setIndexApi({
      stats: async () => null, progress: async () => null,
      rebuild: async () => { throw new Error('disk full') },
    })
    await indexStatus.requestRebuild(async () => true)
    expect(indexStatus.rebuildError).toBe('disk full')
    expect(indexStatus.busyNotice).toBe(false)
  })

  // The bug this guards against: `requestRebuild` used to write non-busy
  // failures into the same `error` field that gates which template branch
  // the whole settings panel renders — including the rebuild button itself.
  // A failed rebuild therefore made its own retry button disappear. A
  // rebuild failure must leave `error` untouched so the branch (and the
  // button in it) stays reachable — that's the actual retry path, not a
  // cosmetic detail.
  it('一次重建失败后 panel 级 error 保持 null——按钮所在的分支不会被换成报错态', async () => {
    _setIndexApi({
      stats: async () => null, progress: async () => null,
      rebuild: async () => { throw new Error('disk full') },
    })
    await indexStatus.requestRebuild(async () => true)
    expect(indexStatus.error).toBeNull()
  })

  it('每次调用先清掉上一次的 busyNotice,不会陈旧地留着', async () => {
    _setIndexApi({
      stats: async () => null, progress: async () => null,
      rebuild: async () => { throw new Error('rebuild already running') },
    })
    await indexStatus.requestRebuild(async () => true)
    expect(indexStatus.busyNotice).toBe(true)

    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild: async () => {} })
    await indexStatus.requestRebuild(async () => true)
    expect(indexStatus.busyNotice).toBe(false)
  })

  it('每次调用也先清掉上一次的 rebuildError,不会陈旧地留着', async () => {
    _setIndexApi({
      stats: async () => null, progress: async () => null,
      rebuild: async () => { throw new Error('disk full') },
    })
    await indexStatus.requestRebuild(async () => true)
    expect(indexStatus.rebuildError).toBe('disk full')

    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild: async () => {} })
    await indexStatus.requestRebuild(async () => true)
    expect(indexStatus.rebuildError).toBeNull()
  })
})

describe('busyNotice staleness — must clear once the OTHER rebuild is actually done, not just on the next requestRebuild() call', () => {
  it('refresh() clears a stale busyNotice once progress() reports nothing is running', async () => {
    _setIndexApi({
      stats: async () => null, progress: async () => null,
      rebuild: async () => { throw new Error('rebuild already running') },
    })
    await indexStatus.requestRebuild(async () => true)
    expect(indexStatus.busyNotice).toBe(true)

    // e.g. triggered by `subscribe()`'s search://index-updated handler once
    // the OTHER rebuild finishes — progress() now returns null.
    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild: async () => {} })
    await indexStatus.refresh()
    expect(indexStatus.busyNotice).toBe(false)
  })

  it('refresh() leaves busyNotice set while the other rebuild is still running', async () => {
    _setIndexApi({
      stats: async () => null, progress: async () => null,
      rebuild: async () => { throw new Error('rebuild already running') },
    })
    await indexStatus.requestRebuild(async () => true)
    expect(indexStatus.busyNotice).toBe(true)

    _setIndexApi({
      stats: async () => null,
      progress: async () => ({ phase: 'indexing', done: 3, total: 10, current: null, elapsedMs: 5 }),
      rebuild: async () => {},
    })
    await indexStatus.refresh()
    expect(indexStatus.busyNotice).toBe(true)
  })

  it('a done progress event clears a stale busyNotice too', () => {
    indexStatus.busyNotice = true
    indexStatus.applyProgress({ phase: 'done', done: 10, total: 10, current: null, elapsedMs: 10 })
    expect(indexStatus.busyNotice).toBe(false)
  })

  it('a non-done progress event does not clear busyNotice — the other rebuild is still running', () => {
    indexStatus.busyNotice = true
    indexStatus.applyProgress({ phase: 'indexing', done: 5, total: 10, current: null, elapsedMs: 5 })
    expect(indexStatus.busyNotice).toBe(true)
  })
})

describe('rebuild progress display helpers', () => {
  it('estimateRebuildSeconds anchors to the ~1000 files/sec design budget, floored at 1s', () => {
    expect(estimateRebuildSeconds(0)).toBe(1)
    expect(estimateRebuildSeconds(1)).toBe(1)
    expect(estimateRebuildSeconds(1000)).toBe(1)
    expect(estimateRebuildSeconds(2500)).toBe(3)
  })

  it('elideMiddle leaves short paths untouched', () => {
    expect(elideMiddle('a.md', 48)).toBe('a.md')
  })

  it('elideMiddle shortens an overlong path and keeps the filename tail intact', () => {
    const long = '/Users/bruce/vault/projects/deeply/nested/folder/structure/note-name.md'
    const out = elideMiddle(long, 40)
    expect(out.length).toBe(40)
    expect(out).toContain('…')
    expect(out.endsWith('note-name.md')).toBe(true)
  })

  it('formatElapsedMs renders sub-minute durations as seconds', () => {
    expect(formatElapsedMs(1234)).toBe('1.2s')
  })

  it('formatElapsedMs renders minute-scale durations as m/s', () => {
    expect(formatElapsedMs(65_000)).toBe('1m 05s')
  })
})
