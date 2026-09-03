import { describe, expect, it, vi } from 'vitest'
import type { SmartSearchResponse } from '../search/api'
import { SmartSearchStore } from './store.svelte'

function response(path: string): SmartSearchResponse {
  return {
    route: 'smart-fts',
    tookMs: 3,
    total: 1,
    truncated: false,
    deepAvailable: false,
    extractedTerms: ['risk'],
    subqueries: [{
      id: 'strict', kind: 'strict', query: 'risk', terms: ['risk'],
      executed: true, route: 't1-fts', hitCount: 1, deepUsed: false, truncated: false,
    }],
    hits: [{
      path,
      absPath: `/vault/${path}`,
      line: 4,
      lineEnd: 4,
      text: 'risk',
      breadcrumb: '',
      level: 'line',
      score: 1,
      docDate: null,
      sourceRef: `${path}#L4`,
      agentBy: null,
      humanVerified: false,
      origin: 'human',
      conceptType: null,
      pinned: false,
      fusedScore: 0.8,
      relevanceReasons: ['strict_query'],
      matchedQueries: ['strict'],
    }],
  }
}

describe('SmartSearchStore', () => {
  it('keeps the newest live query when an older response arrives last', async () => {
    const resolvers: Array<(value: SmartSearchResponse) => void> = []
    const store = new SmartSearchStore(() => new Promise((resolve) => resolvers.push(resolve)))

    const oldRun = store.run('old question')
    const newRun = store.run('new question')
    resolvers[1](response('new.md'))
    resolvers[0](response('old.md'))
    const [oldResult, newResult] = await Promise.all([oldRun, newRun])

    expect(oldResult).toBeNull()
    expect(newResult?.hits[0].path).toBe('new.md')
    expect(store.query).toBe('new question')
    expect(store.hits[0].path).toBe('new.md')
    expect(store.subqueries[0].id).toBe('strict')
  })

  it('clears without calling the backend for blank input', async () => {
    const search = vi.fn()
    const store = new SmartSearchStore(search)
    await store.run('   ')
    expect(search).not.toHaveBeenCalled()
    expect(store.hits).toEqual([])
  })

  it('does not present a superseded cancellation as an error', async () => {
    const store = new SmartSearchStore(async () => { throw new Error('search cancelled') })
    await store.run('question')
    expect(store.error).toBeNull()
    expect(store.loading).toBe(false)
  })
})
