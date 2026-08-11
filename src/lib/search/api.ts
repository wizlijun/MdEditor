import { invoke } from '@tauri-apps/api/core'

export interface SearchHit {
  path: string
  absPath: string
  line: number
  lineEnd: number
  text: string
  breadcrumb: string
  level: 'file' | 'section' | 'line'
  score: number
  docDate: string | null
  sourceRef: string
  agentBy: string | null
  humanVerified: boolean
}

export interface SearchResponse {
  route: string
  tookMs: number
  total: number
  hits: SearchHit[]
  /** The backend hit `timeoutMs`; `hits` is a partial answer. */
  truncated: boolean
  /** FTS missed and a deeper (slower) scan is available but was not run. */
  deepAvailable: boolean
}

/**
 * Cancellation is NOT expressed here. The backend stamps every query with its
 * own per-window ticket and aborts any query a later ticket has overtaken —
 * mid-statement, so the superseded query stops holding the index lock rather
 * than merely having its response discarded. Sequencing it from this side
 * would break on webview reload, where a frontend counter restarts at zero
 * and the backend's does not.
 */
export interface SearchOptions {
  limit?: number
  /**
   * Allow the bounded full-scan fallback when the index misses. Live typing
   * passes `false` (the fallback costs seconds on a large vault); Enter and
   * the auto-retry pass `true`.
   */
  deep?: boolean
  /** Abort and return partial results after this many ms. */
  timeoutMs?: number
}

export interface SearchStats {
  files: number
  blocks: number
  dbBytes: number
  builtAt: string | null
  tokenizerId: string
}

export const searchApi = {
  query: (query: string, opts: SearchOptions = {}) =>
    invoke<SearchResponse>('notemd_search', {
      query,
      limit: opts.limit ?? 50,
      deep: opts.deep,
      timeoutMs: opts.timeoutMs,
    }),
  stats: () => invoke<SearchStats>('notemd_search_stats'),
  rebuild: () => invoke<SearchStats>('notemd_search_rebuild'),
}
