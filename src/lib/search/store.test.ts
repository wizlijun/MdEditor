import { describe, it, expect, vi, beforeEach } from 'vitest'
import { searchStore, _setSearchImpl } from './store.svelte'

beforeEach(() => searchStore.clear())

describe('searchStore', () => {
  it('stores hits and the route reported by the backend', async () => {
    _setSearchImpl(async () => ({ route: 't1-fts', tookMs: 3, total: 1, hits: [
      { path: 'a.md', absPath: '/v/a.md', line: 2, lineEnd: 2, text: 'x', breadcrumb: '',
        level: 'line', score: 0.5, docDate: null, sourceRef: 'a.md#L2', agentBy: null, humanVerified: false },
    ] }))
    await searchStore.run('x')
    expect(searchStore.hits.length).toBe(1)
    expect(searchStore.route).toBe('t1-fts')
    expect(searchStore.loading).toBe(false)
  })

  // 一个空查询打一次后端是纯浪费,而且会把上一次的结果闪掉。
  it('does not call the backend for a blank query', async () => {
    const impl = vi.fn()
    _setSearchImpl(impl)
    await searchStore.run('   ')
    expect(impl).not.toHaveBeenCalled()
    expect(searchStore.hits).toEqual([])
  })

  // 索引还没建好时面板必须说人话,而不是抛一个 Rust 错误串给用户看。
  it('surfaces a backend failure as an error message, not a throw', async () => {
    _setSearchImpl(async () => { throw new Error('search index not ready') })
    await searchStore.run('x')
    expect(searchStore.error).toBeTruthy()
    expect(searchStore.loading).toBe(false)
  })

  // 快速输入会连发请求;晚到的旧响应不能覆盖新结果。
  it('ignores a stale response that arrives after a newer one', async () => {
    const resolvers: Array<(v: unknown) => void> = []
    _setSearchImpl(() => new Promise((r) => resolvers.push(r as (v: unknown) => void)))
    const first = searchStore.run('old')
    const second = searchStore.run('new')
    resolvers[1]({ route: 't1-fts', tookMs: 1, total: 1, hits: [{ path: 'new.md' }] })
    resolvers[0]({ route: 't1-fts', tookMs: 1, total: 1, hits: [{ path: 'old.md' }] })
    await Promise.all([first, second])
    expect(searchStore.hits[0].path).toBe('new.md')
  })
})
