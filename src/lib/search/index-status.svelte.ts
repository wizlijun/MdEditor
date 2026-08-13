import { listen } from '@tauri-apps/api/event'
import { searchApi, type SearchStats, type SearchProgress, type SearchIndexState } from './api'
import { isIndexNotReady } from './store.svelte'

// Injectable surface, mirroring `searchApi`'s index-lifecycle methods — lets
// the store be exercised without a Tauri host (same idiom as
// `_setSearchImpl` in `store.svelte.ts`).
export interface IndexApi {
  stats: () => Promise<SearchStats | null>
  progress: () => Promise<SearchProgress | null>
  rebuild: () => Promise<void>
  indexState: () => Promise<SearchIndexState>
  reopen: () => Promise<void>
}

let impl: IndexApi = searchApi
/**
 * Swap in a test double. `Partial`, layered over the real `searchApi`, so a
 * test only spells out the methods it drives: a test about `stats()` racing
 * `progress()` says nothing about the index-state read, and shouldn't have to
 * restate it (nor silently lose coverage of the rest of the surface when a
 * new method is added here).
 */
export function _setIndexApi(api: Partial<IndexApi>): void {
  impl = { ...searchApi, ...api }
}

// Monotonic request id, same reason as `SearchStore.run`'s `seq`: two
// overlapping `refresh()` calls (e.g. the settings dialog closed and
// reopened quickly) must not let the slower one clobber the faster one's
// fresher result.
let seq = 0

// Substring match against the backend's exact refusal text
// (`src-tauri/src/search/mod.rs`: `Err("rebuild already running".into())`).
// A raw Tauri command error can arrive either as an `Error` or as the bare
// string itself depending on the invoke path, hence the `instanceof` guard
// below rather than assuming one shape.
const REBUILD_ALREADY_RUNNING = 'rebuild already running'

class IndexStatusStore {
  stats = $state<SearchStats | null>(null)
  progress = $state<SearchProgress | null>(null)
  loading = $state(false)
  error = $state<string | null>(null)
  // "Index not ready" is the normal, expected state right after startup or a
  // vault switch — surfaced distinctly from `error` so the UI can show a
  // plain sentence instead of something that looks like a crash.
  notReady = $state(false)
  // A rebuild refused because one is already running — also not a failure,
  // just a fact the user should be told in plain language rather than as an
  // error banner. Distinct from `error` for the same reason `notReady` is.
  // Cleared whenever we actually observe the other rebuild is no longer
  // running (see `refresh()` and `applyProgress()` below) — not just on the
  // next `requestRebuild()` call, otherwise it sits stale on screen ("a
  // rebuild is already running") long after that rebuild finished.
  busyNotice = $state(false)
  // A failed rebuild attempt, kept separate from `error` on purpose: `error`
  // means "the panel couldn't read index status at all" and gates which
  // template branch renders (including the rebuild button itself); a failed
  // rebuild is a narrower, retriable event that must NOT make the button
  // that just failed disappear from under the user.
  rebuildError = $state<string | null>(null)
  // What `notReady` actually means right now, straight from the backend's
  // `OpenState`. `notReady` alone cannot tell "a scan is running" from "the
  // open failed and nothing will ever retry it" — they are the same empty
  // index handle — and rendering both as "the index is still building" is
  // what left a permanently dead index looking like a slow one, with the
  // Rebuild button hidden behind the same branch. `null` until the first
  // refresh (or on a host too old to answer), which the UI treats exactly
  // like the old undifferentiated state.
  openState = $state<SearchIndexState | null>(null)
  // A failed `reopen()` attempt. Separate from `error`/`rebuildError` for the
  // same reason those are separate from each other: it must not remove the
  // retry button that produced it.
  reopenError = $state<string | null>(null)

  /** True when the index is unavailable *and will stay that way* until the
   *  user acts — the case that needs a retry button rather than a spinner. */
  get openFailed(): boolean {
    return this.openState?.state === 'failed'
  }

  /**
   * Pulls a fresh snapshot of both `stats` and `progress`. Deliberately reads
   * `progress()` here too (not just on the next `search://progress` event):
   * a settings page opened while a rebuild is already running has missed
   * every event fired so far, and without this poll it would sit blank
   * until the next throttled callback fires on the backend.
   *
   * The two reads are applied INDEPENDENTLY, never bundled into one
   * `Promise.all` — that is the whole point of the backend's lock split.
   * `notemd_search_stats` takes the index lock, which a rebuild holds for its
   * entire duration; `notemd_search_progress` deliberately never touches that
   * lock so progress stays readable during exactly that window (design spec
   * §59). Awaiting the pair together would re-couple them on the frontend:
   * `progress` could not land until `stats` got the lock, i.e. not until the
   * rebuild finished — which is precisely when progress stops being useful.
   */
  async refresh(): Promise<void> {
    const mine = ++seq
    this.loading = true
    this.error = null
    this.notReady = false
    // Fire-and-apply: lands as soon as the lock-free command answers, without
    // waiting on the lock-taking `stats()` below. Guarded by the same `seq` so
    // a superseded refresh still can't clobber a fresher one. A progress read
    // failing is not panel-level breakage (stats carries the error reporting),
    // so it is swallowed rather than blanking the page. Called directly (not
    // deferred through `Promise.resolve().then(...)`) so it still lands within
    // the same microtask drain a caller's `await refresh()` completes in; the
    // `try` only guards a *synchronous* throw out of `impl.progress()`, which
    // would otherwise escape this method and leave `loading` stuck true.
    try {
      void impl.progress().then(
        (p) => {
          if (mine !== seq) return
          this.progress = p
          // `p === null` means no rebuild is running right now (by anyone) —
          // the exact condition a stale `busyNotice` was reporting on. If
          // another rebuild is still in flight, leave it set.
          if (p === null) this.busyNotice = false
        },
        () => {},
      )
    } catch { /* same as a rejected progress read: stats carries the error */ }
    // Read independently of `stats()` for exactly the same reason as
    // `progress()` above: `notemd_search_index_state` never touches the index
    // lock, and its whole job is to describe the window in which `stats()`
    // cannot answer — bundling them would make it arrive only once the answer
    // it explains no longer matters. A host without the command (older
    // backend) leaves `openState` null, which the UI renders as the old
    // undifferentiated "not ready".
    try {
      void impl.indexState().then(
        (s) => {
          if (mine !== seq) return
          this.openState = s
          if (s.state !== 'failed') this.reopenError = null
        },
        () => {},
      )
    } catch { /* as above */ }
    try {
      const stats = await impl.stats()
      if (mine !== seq) return // superseded by a newer refresh() — drop the stale response
      this.stats = stats
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
    // Same staleness fix as `refresh()`, for the event-driven path: a
    // 'done' event is direct evidence the rebuild that was refusing new
    // requests has finished, so a `busyNotice` referring to it is stale too.
    if (p.phase === 'done') this.busyNotice = false
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

  /**
   * Rebuild entry point for any UI trigger. `confirm` is called first — a
   * caller-supplied dialog, not a built-in one, so the store stays testable
   * without a Tauri host and the UI owns exactly what the confirmation says.
   * Only a `true` result calls `rebuild()`.
   *
   * Deliberately does NOT set any local "rebuilding" flag around the
   * `rebuild()` call: `notemd_search_rebuild` now returns almost immediately
   * (the real work runs on a background thread holding the index lock), so a
   * flag driven off this call's lifetime would flip back to false while the
   * rebuild is still very much in progress. The UI's busy state must be
   * driven off `progress` instead, which is what actually tracks the work.
   *
   * A `rebuild already running` refusal (a second rebuild request while one
   * is already in flight — a normal, expected race, not a bug) is caught and
   * surfaced as `busyNotice` rather than `error`, so the UI can say "a
   * rebuild is already running" instead of showing something that looks
   * like a crash.
   *
   * Any OTHER failure is written to `rebuildError`, deliberately NOT to
   * `error`: `error` gates which template branch the whole panel renders
   * (including the rebuild button itself), so writing a rebuild failure
   * there made a failed click remove its own retry button from the page.
   * `rebuildError` is a narrower, retriable notice that lives alongside the
   * button instead of replacing it.
   *
   * `search index not ready` is the third case, and it is not an error to
   * report either: it means the index was taken away between this page's last
   * refresh and the click — a vault reopen started underneath it. Observed
   * exactly that way in the wild (saving a source-glob change reopens the
   * index; the still-stale panel offered a Rebuild button that could only
   * come back "not ready"). The honest response is to re-read the state and
   * let the panel render what is actually happening, not to print the
   * backend's own sentence at a user who did nothing wrong.
   */
  async requestRebuild(confirm: () => Promise<boolean>): Promise<void> {
    this.busyNotice = false
    this.rebuildError = null
    const ok = await confirm()
    if (!ok) return
    try {
      await impl.rebuild()
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      if (msg.includes(REBUILD_ALREADY_RUNNING)) {
        this.busyNotice = true
      } else if (isIndexNotReady(msg)) {
        await this.refresh()
      } else {
        this.rebuildError = msg
      }
    }
  }

  /**
   * Re-runs the backend's vault open — the recovery path for `openFailed`.
   *
   * Deliberately NOT routed through `requestRebuild`: a rebuild needs the
   * index handle that a failed open never installed, so it can only answer
   * "not ready" (see `notemd_search_reopen`'s doc comment in
   * `src-tauri/src/search/mod.rs`). No confirmation dialog either — reopening
   * destroys nothing, and the realistic failure it recovers from
   * (`database is locked`, i.e. a lost race against another connection) is
   * fixed by simply trying again.
   *
   * Optimistically moves `openState` to `opening` rather than waiting for the
   * next refresh: the command returns as soon as the background thread is
   * spawned, and leaving `failed` on screen in the meantime would invite a
   * second click on a retry that is already running.
   */
  async requestReopen(): Promise<void> {
    this.reopenError = null
    try {
      await impl.reopen()
      this.openState = { state: 'opening', error: null }
      await this.refresh()
    } catch (e) {
      this.reopenError = e instanceof Error ? e.message : String(e)
    }
  }

  reset(): void {
    seq++ // invalidate any in-flight refresh() so its response can't land after this
    this.stats = null
    this.progress = null
    this.loading = false
    this.error = null
    this.notReady = false
    this.busyNotice = false
    this.rebuildError = null
    this.openState = null
    this.reopenError = null
  }
}

export const indexStatus = new IndexStatusStore()

// ── Presentation helpers ────────────────────────────────────────────────
// Pure functions used by the settings page's progress block / rebuild
// confirmation. Kept here (not inline in the `.svelte` file) so they're
// unit-testable without mounting a component.

/**
 * Order-of-magnitude estimate for the rebuild confirmation dialog, not a
 * promise. Anchored to the design spec's own cold-build budget — "10k
 * files/150MB in <10s" (docs/2026-08-10-vault-search-index-design.md
 * §3.1/§7) — i.e. roughly 1000 files/sec. Real hardware and file sizes vary;
 * this exists only so the dialog can say "a few seconds" instead of a bare
 * "search will be unavailable" with no sense of scale.
 */
export function estimateRebuildSeconds(files: number): number {
  return Math.max(1, Math.ceil(files / 1000))
}

/**
 * Middle-elides an overlong path: `/very/long/path/to/note.md` →
 * `/very/lon…note.md`. Keeps a larger share at the front (directory context
 * matters less than the filename, which lives at the end) while guaranteeing
 * the tail — usually the most identifying part — always survives.
 */
export function elideMiddle(path: string, maxLen = 48): string {
  if (path.length <= maxLen) return path
  const keep = maxLen - 1 // reserve one char for the ellipsis
  const head = Math.ceil(keep * 0.6)
  const tail = keep - head
  return path.slice(0, head) + '…' + path.slice(path.length - tail)
}

/** `elapsedMs` from `SearchProgress` → a short human string ("12.3s", "1m 05s"). */
export function formatElapsedMs(ms: number): string {
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`
  const totalSeconds = Math.floor(ms / 1000)
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${minutes}m ${String(seconds).padStart(2, '0')}s`
}
