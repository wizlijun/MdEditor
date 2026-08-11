import { searchApi, type SearchHit, type SearchResponse } from './api'

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

// Injectable so the store is testable without a Tauri host.
let impl: (q: string, limit?: number) => Promise<SearchResponse> = searchApi.query
export function _setSearchImpl(fn: typeof impl) { impl = fn }

// Monotonic request id. Typing fires overlapping queries and the network does
// not promise ordering — without this, a slow early response can overwrite the
// results the user is actually looking at.
let seq = 0

class SearchStore {
  query = $state('')
  hits = $state<SearchHit[]>([])
  route = $state<string | null>(null)
  tookMs = $state(0)
  total = $state(0)
  loading = $state(false)
  error = $state<string | null>(null)

  async run(q: string): Promise<void> {
    this.query = q
    if (!q.trim()) { this.clear(); return }
    const mine = ++seq
    this.loading = true
    this.error = null
    try {
      const res = await impl(q)
      if (mine !== seq) return // superseded by a newer run() — stale response, drop it
      this.hits = res.hits
      this.route = res.route
      this.tookMs = res.tookMs
      this.total = res.total
    } catch (e) {
      if (mine !== seq) return
      this.error = e instanceof Error ? e.message : String(e)
      this.hits = []
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
  }
}

export const searchStore = new SearchStore()
