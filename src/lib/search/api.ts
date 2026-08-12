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
  /** `'human' | 'derived' | 'source' | 'unlabeled'` — mirrors `HitDto.origin`
   *  (`searchidx::Origin::as_str()`). `'human'`/`'source'` are the two poles
   *  the search panel pins near the ends of its grouping; `'unlabeled'`
   *  (added 2026-08-12 design, C-T2) is the backend's honest "nobody has
   *  claimed this file" tier — it gets its own fourth group, after `source`
   *  (see `grouping.ts`). */
  origin: 'human' | 'derived' | 'source' | 'unlabeled'
  /** `files.concept_type` verbatim (frontmatter `type`), e.g. `'Book Summary'`.
   *  `null` when the file has no `type`. Only origin `'derived'` hits are
   *  subdivided by this in the panel — see `src/lib/search/grouping.ts`. */
  conceptType: string | null
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

// One entry of `SearchStats.skippedLarge` — mirrors `SkippedDto` in
// `src-tauri/src/search/mod.rs` field for field.
export interface SearchSkippedFile {
  path: string
  sizeBytes: number
}

// Mirrors `OriginCountsDto` in `src-tauri/src/search/mod.rs` field for field
// — per-tier file counts for the settings page (task B-T8, design spec
// §6/§9; `unlabeled` added C-T11). Purely a settings-page display; ranking
// reads `SearchHit.origin` per hit instead, never these totals.
export interface SearchOriginCounts {
  human: number
  derived: number
  source: number
  unlabeled: number
}

export interface SearchStats {
  files: number
  blocks: number
  dbBytes: number
  builtAt: string | null
  tokenizerId: string
  skippedLarge: SearchSkippedFile[]
  originCounts: SearchOriginCounts
  /** `derived`'s distribution by raw `concept_type` string — `origin =
   *  'derived'` and a non-null type only. Keys are NEVER translated (same
   *  convention as `grouping.ts`'s derived-type group headers): a plugin
   *  can introduce a new type without touching i18n. Untyped `derived`
   *  files are not itemized here; the settings tab computes that count as
   *  `originCounts.derived - sum(Object.values(typeCounts))`. */
  typeCounts: Record<string, number>
}

// Wire shape for `notemd_search_progress` / the `search://progress` event —
// mirrors `ProgressDto` in `src-tauri/src/search/mod.rs` (`phase_str`'s
// mapping) field for field, including the camelCase `elapsedMs`.
export interface SearchProgress {
  phase: 'walking' | 'indexing' | 'removing' | 'done'
  done: number
  total: number
  current: string | null
  elapsedMs: number
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
  // `null` while no rebuild is running — a settings page opened mid-rebuild
  // calls this to catch up instead of waiting for the next progress event.
  progress: () => invoke<SearchProgress | null>('notemd_search_progress'),
  // Fire-and-forget: the backend command spawns a background thread and
  // returns immediately (see `notemd_search_rebuild`'s doc comment in
  // `src-tauri/src/search/mod.rs`), so there is no `SearchStats` payload to
  // return here any more — progress/completion are observed via `progress()`
  // and the `search://progress` / `search://index-updated` events instead.
  rebuild: () => invoke<void>('notemd_search_rebuild'),
  // Task C-T11 (design spec §7.1): how many files under the REAL vault (not
  // the index — see `src/lib/search/glob-suggest.ts`'s doc comment for why
  // the index would undercount) a candidate/saved source-glob pattern set
  // matches. Mirrors `notemd_search_glob_matches` in
  // `src-tauri/src/search/mod.rs` — walks the vault fresh on every call, so
  // callers presenting several candidates issue one call per candidate
  // rather than batching them into one list (each candidate needs its own,
  // independent count).
  globMatches: (patterns: string[]) => invoke<number>('notemd_search_glob_matches', { patterns }),
}
