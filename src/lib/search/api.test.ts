import { beforeEach, describe, expect, it, vi } from 'vitest'

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke }))

import { DEFAULT_LIMIT, searchApi } from './api'

beforeEach(() => invoke.mockReset())

describe('searchApi.smart', () => {
  it('calls the independent smart-search command with the ordinary default limit', async () => {
    invoke.mockResolvedValue({
      route: 'smart-fts', tookMs: 1, total: 0, hits: [], truncated: false,
      deepAvailable: false, extractedTerms: [], subqueries: [],
    })

    await searchApi.smart('launch risk')

    expect(invoke).toHaveBeenCalledWith('notemd_smart_search', {
      query: 'launch risk',
      limit: DEFAULT_LIMIT,
      deep: undefined,
      timeoutMs: undefined,
    })
  })

  it('passes the shared deep, timeout and no-count-cap spellings unchanged', async () => {
    invoke.mockResolvedValue({
      route: 'smart-scan', tookMs: 4, total: 0, hits: [], truncated: true,
      deepAvailable: false, extractedTerms: [], subqueries: [],
    })

    await searchApi.smart('发布风险', { limit: 0, deep: true, timeoutMs: 4000 })

    expect(invoke).toHaveBeenCalledWith('notemd_smart_search', {
      query: '发布风险',
      limit: 0,
      deep: true,
      timeoutMs: 4000,
    })
  })
})
