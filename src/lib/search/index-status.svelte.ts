import { listen } from '@tauri-apps/api/event'
import { searchApi, type SearchStats, type SearchProgress } from './api'
import { isIndexNotReady } from './store.svelte'

// Injectable surface, mirroring `searchApi`'s three index-lifecycle methods —
// lets the store be exercised without a Tauri host (same idiom as
// `_setSearchImpl` in `store.svelte.ts`).
export interface IndexApi {
  stats: () => Promise<SearchStats | null>
  progress: () => Promise<SearchProgress | null>
  rebuild: () => Promise<void>
}

let impl: IndexApi = searchApi
export function _setIndexApi(api: IndexApi): void {
  impl = api
}

// Monotonic request id, same reason as `SearchStore.run`'s `seq`: two
// overlapping `refresh()` calls (e.g. the settings dialog closed and
// reopened quickly) must not let the slower one clobber the faster one's
// fresher result.
let seq = 0

class IndexStatusStore {
  stats = $state<SearchStats | null>(null)
  progress = $state<SearchProgress | null>(null)
  loading = $state(false)
  error = $state<string | null>(null)
  // "Index not ready" is the normal, expected state right after startup or a
  // vault switch — surfaced distinctly from `error` so the UI can show a
  // plain sentence instead of something that looks like a crash.
  notReady = $state(false)

  /**
   * Pulls a fresh snapshot of both `stats` and `progress`. Deliberately reads
   * `progress()` here too (not just on the next `search://progress` event):
   * a settings page opened while a rebuild is already running has missed
   * every event fired so far, and without this poll it would sit blank
   * until the next throttled callback fires on the backend.
   */
  async refresh(): Promise<void> {
    const mine = ++seq
    this.loading = true
    this.error = null
    this.notReady = false
    try {
      const [stats, progress] = await Promise.all([impl.stats(), impl.progress()])
      if (mine !== seq) return // superseded by a newer refresh() — drop the stale response
      this.stats = stats
      this.progress = progress
    } catch (e) {
      if (mine !== seq) return
      const msg = e instanceof Error ? e.message : String(e)
      if (isIndexNotReady(msg)) {
        this.notReady = true
      } else {
        this.error = msg
      }
    } finally {
      if (mine === seq) this.loading = false
    }
  }

  /**
   * Applies a `search://progress` event payload. `phase === 'done'` clears
   * `progress` back to `null` rather than storing the terminal snapshot —
   * without this the panel would show a frozen 100% forever, since no
   * further progress event ever follows "done".
   */
  applyProgress(p: SearchProgress): void {
    this.progress = p.phase === 'done' ? null : p
  }

  /**
   * Subscribes to both events a live settings page cares about:
   * `search://progress` (drives `applyProgress`) and `search://index-updated`
   * (fired once after every rebuild, win or lose — drives a full `refresh()`
   * so `stats` picks up the new counts). Returns an unsubscribe function,
   * meant to be returned directly from a Svelte `$effect`.
   */
  subscribe(): () => void {
    let cancelled = false
    let unlistenProgress: (() => void) | null = null
    let unlistenUpdated: (() => void) | null = null

    void listen<SearchProgress>('search://progress', (e) => this.applyProgress(e.payload)).then((un) => {
      if (cancelled) { un(); return }
      unlistenProgress = un
    })
    void listen('search://index-updated', () => { void this.refresh() }).then((un) => {
      if (cancelled) { un(); return }
      unlistenUpdated = un
    })

    return () => {
      cancelled = true
      unlistenProgress?.()
      unlistenUpdated?.()
    }
  }

  reset(): void {
    seq++ // invalidate any in-flight refresh() so its response can't land after this
    this.stats = null
    this.progress = null
    this.loading = false
    this.error = null
    this.notReady = false
  }
}

export const indexStatus = new IndexStatusStore()
