import { searchApi, type SearchHit, type SearchOptions, type SearchResponse } from './api'

// The one backend error string worth a proper translation in the UI (the
// common case: index still opening/building at startup or right after a
// vault switch) — see `src-tauri/src/search/mod.rs`'s `NOT_READY` const,
// which this is a copy of. Kept as a substring match rather than equality
// (same idiom as `HistoryPanel.svelte`'s `String(e).includes('git-unavailable')`)
// so a wrapped or lightly-reworded backend message still resolves correctly
// instead of silently falling through to raw, untranslated English.
const NOT_READY = 'search index not ready'
export function isIndexNotReady(error: string | null): boolean {
  return error != null && error.includes(NOT_READY)
}

// Mirror of `src-tauri/src/search/mod.rs`'s `CANCELLED`. Not a failure and
// never shown: it means a newer query took this one's place, so a fresher
// answer is already on its way.
const CANCELLED = 'search cancelled'
export function isCancelled(error: unknown): boolean {
  return String(error instanceof Error ? error.message : error).includes(CANCELLED)
}

// Injectable so the store is testable without a Tauri host.
let impl: (q: string, opts?: SearchOptions) => Promise<SearchResponse> = searchApi.query
export function _setSearchImpl(fn: typeof impl) { impl = fn }

// Monotonic request id. Typing fires overlapping queries and the network does
// not promise ordering — without this, a slow early response can overwrite the
// results the user is actually looking at. Stopping the superseded query's
// *work* is the backend's job (it tickets queries per window; a counter kept
// here would reset on webview reload and the backend's would not), so this
// number never leaves the frontend.
let seq = 0

export interface RunOptions {
  /** Pay for the full-scan fallback. Live typing does not; Enter does. */
  deep?: boolean
  timeoutMs?: number
  /**
   * Return every hit — sent as `limit: 0`, the wire spelling the backend maps
   * to `searchidx::NO_LIMIT`. Without it the api layer's `DEFAULT_LIMIT`
   * applies.
   */
  all?: boolean
}

class SearchStore {
  query = $state('')
  hits = $state<SearchHit[]>([])
  route = $state<string | null>(null)
  tookMs = $state(0)
  total = $state(0)
  loading = $state(false)
  error = $state<string | null>(null)
  /** The last answer was cut short by its time budget. */
  truncated = $state(false)
  /** The last answer was shallow and a deeper scan is still on the table. */
  deepAvailable = $state(false)
  /** Whether the last run was itself deep — so a refresh can match it. */
  lastDeep = $state(false)
  /** Whether the last run asked for every hit — so the panel knows the
   *  「显示全部」 offer is already spent, and a refresh can match it. */
  lastAll = $state(false)

  async run(q: string, opts: RunOptions = {}): Promise<void> {
    this.query = q
    if (!q.trim()) { this.clear(); return }
    const mine = ++seq
    this.lastDeep = opts.deep === true
    this.lastAll = opts.all === true
    this.loading = true
    this.error = null
    try {
      const res = await impl(q, {
        deep: opts.deep,
        timeoutMs: opts.timeoutMs,
        ...(opts.all ? { limit: 0 } : {}),
      })
      if (mine !== seq) return // superseded by a newer run() — stale response, drop it
      this.hits = res.hits
      this.route = res.route
      this.tookMs = res.tookMs
      this.total = res.total
      this.truncated = res.truncated === true
      this.deepAvailable = res.deepAvailable === true
    } catch (e) {
      if (mine !== seq) return
      // A cancellation is the backend agreeing with us that this query no
      // longer matters. Rendering it as an error would turn our own
      // optimization into a visible failure.
      if (isCancelled(e)) return
      this.error = e instanceof Error ? e.message : String(e)
      this.hits = []
      this.deepAvailable = false
      this.truncated = false
    } finally {
      if (mine === seq) this.loading = false
    }
  }

  clear(): void {
    seq++ // invalidate any in-flight run() so its response can't land after this
    this.query = ''
    this.hits = []
    this.route = null
    this.tookMs = 0
    this.total = 0
    this.loading = false
    this.error = null
    this.truncated = false
    this.deepAvailable = false
    this.lastDeep = false
    this.lastAll = false
  }
}

export const searchStore = new SearchStore()
