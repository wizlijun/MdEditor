//! The Tauri adapter: an index handle in app state, plus the file watcher.
//! Deliberately thin — every decision about scanning, tokenizing and ranking is
//! `searchidx`'s, so the GUI and the CLI cannot answer the same query
//! differently.

pub mod options;
pub mod watch;

use std::path::Path;
// `AtomicBool` is `RebuildFlag`'s single-flight guard; `AtomicU64` is
// `SearchGen`'s query ticket. Two independent mechanisms that happen to live
// in the same module — both are needed.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

// No `ScanOptions` here: it was imported by the query-responsiveness rework's
// own `notemd_search_rebuild`, which this merge does not keep (see that
// command's doc comment). Every remaining scan site goes through
// `scan_options`/`searchidx::ScanOptions::default()` by path.
use searchidx::{Hit, IndexStats, Limits, SearchIndex};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Kept as an alias — rather than rewriting this module's call sites — so
/// there is exactly one place (`options::for_vault`) that ever constructs a
/// `ScanOptions`; see that module's doc comment for why that has to be true
/// across the GUI/CLI process boundary.
pub use options::for_vault as scan_options;

/// `None` until a vault is configured (or after a failed open — the index is
/// optional, the app is not).
pub type IndexHandle = Arc<Mutex<Option<SearchIndex>>>;

/// The latest rebuild progress snapshot, readable by `notemd_search_progress`.
///
/// **Deliberately a different `Mutex` from `IndexHandle`.** A rebuild holds
/// the index lock for its entire duration (see `notemd_search_rebuild`'s doc
/// comment) — if progress were stored behind that same lock, a settings page
/// polling for progress would block on the very lock it exists to report on,
/// and would only ever observe progress *after* the rebuild that produced it
/// had already finished. That defeats the point, so this is its own lock.
#[derive(Default, Clone)]
pub struct ProgressState(Arc<Mutex<Option<searchidx::Progress>>>);

impl ProgressState {
    pub fn set(&self, p: Option<searchidx::Progress>) {
        if let Ok(mut g) = self.0.lock() {
            *g = p
        }
    }
    pub fn get(&self) -> Option<searchidx::Progress> {
        self.0.lock().ok().and_then(|g| g.clone())
    }
    pub fn clear(&self) {
        self.set(None)
    }
}

/// The most recent scan's list of files skipped for exceeding the size
/// threshold — path plus actual size (`searchidx::SkippedFile`).
///
/// **Deliberately its own `Mutex`, same reasoning as [`ProgressState`].** A
/// `ScanStats` (which is where this list lives) is a return value from
/// `ensure_built`/`sweep`/`rebuild_with_progress` — it is never written to
/// the index's own SQLite tables, so without somewhere to stash it, the
/// settings page would only ever be able to show whatever the *last call it
/// personally made* happened to return, going stale the instant a scan it
/// didn't initiate (the watcher's periodic sweep, another window's rebuild)
/// silently supersedes it. Every scan site (`open_vault`'s initial
/// build+sweep, `notemd_search_rebuild`, the watcher's flood-sweep in
/// `watch::drain`) writes here so the settings page always reflects the most
/// recent scan, whoever ran it.
#[derive(Default, Clone)]
pub struct SkippedState(Arc<Mutex<Vec<searchidx::SkippedFile>>>);

impl SkippedState {
    pub fn set(&self, v: Vec<searchidx::SkippedFile>) {
        if let Ok(mut g) = self.0.lock() {
            *g = v
        }
    }
    pub fn get(&self) -> Vec<searchidx::SkippedFile> {
        self.0.lock().map(|g| g.clone()).unwrap_or_default()
    }
}

/// Single-flight guard for `notemd_search_rebuild`. A second rebuild started
/// while one is already running is refused outright (`try_begin` returns
/// `false`), never queued — queueing would turn three impatient clicks into
/// three consecutive full rebuilds.
#[derive(Default, Clone)]
pub struct RebuildFlag(Arc<AtomicBool>);

impl RebuildFlag {
    pub fn try_begin(&self) -> bool {
        self.0.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }
    pub fn end(&self) {
        self.0.store(false, Ordering::SeqCst)
    }
}

/// RAII cleanup for the rebuild background thread: clears `ProgressState` and
/// releases `RebuildFlag` in `Drop`, so both happen whether the thread's work
/// closure returns normally *or* unwinds. A bare `progress.clear(); flag.end();`
/// placed after the closure call would be skipped entirely if
/// `rebuild_with_progress`/`log_rebuild_with` panics instead of returning —
/// `searchidx` has a number of `unwrap`/`expect` call sites, and debug/test
/// builds unwind by default (only release sets `panic = "abort"`, and even
/// that's crate-config, not guaranteed here). A stuck `RebuildFlag` is a
/// silent, permanent regression: "refused, not queued" quietly becomes
/// "refused for the rest of the process's life" the first time a rebuild
/// panics.
struct RebuildGuard {
    progress: ProgressState,
    flag: RebuildFlag,
}

impl Drop for RebuildGuard {
    fn drop(&mut self) {
        self.progress.clear();
        self.flag.end();
    }
}

/// The exact `IndexHandle`/`ProgressState`/`RebuildFlag`/`SkippedState`
/// values `init` hands to `app.manage`, factored out so a test can assert on
/// *this* construction directly rather than on locals a test made up
/// separately. This codebase has never enabled the `tauri::test` feature (no
/// other module uses it), so there is no `AppHandle` to drive `init` itself
/// from a unit test — calling this same function `init` calls is the closest
/// a test can get to "the real wiring" without adding that dependency for
/// one assertion.
fn managed_state() -> (IndexHandle, ProgressState, RebuildFlag, SkippedState) {
    (
        Arc::new(Mutex::new(None)),
        ProgressState::default(),
        RebuildFlag::default(),
        SkippedState::default(),
    )
}

/// Per-window ticket dispenser for in-flight queries.
///
/// Typing produces overlapping queries, and a superseded one is not merely
/// uninteresting — it holds the index mutex, so every later keystroke queues
/// behind work whose answer nobody will ever look at. Each running query
/// compares its own ticket against the newest one from SQLite's progress
/// callback and stops the moment it is behind: the difference between "the
/// stale response is discarded" (what the frontend already did) and "the
/// stale *work* stops" (what makes the next keystroke fast).
///
/// The number is minted HERE rather than passed in by the caller. A frontend
/// counter resets to zero on every webview reload while this state does not,
/// which would leave every query after a reload permanently "superseded" —
/// search silently dead until app restart.
///
/// Keyed by window label, because two windows searching at once are two
/// users' worth of intent: cancelling one on behalf of the other would leave
/// a panel showing nothing with no way to ask again.
#[derive(Default, Clone)]
pub struct SearchGen(Arc<Mutex<std::collections::HashMap<String, Arc<AtomicU64>>>>);

impl SearchGen {
    /// Reserve the newest ticket for `window`, handing back the counter it was
    /// drawn from so the abort check itself is a plain atomic load — it runs
    /// from SQLite's progress callback tens of thousands of times per scan,
    /// which is no place for a map lookup behind a mutex.
    pub fn next(&self, window: &str) -> (u64, Arc<AtomicU64>) {
        let mut m = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let counter = m.entry(window.to_string()).or_default().clone();
        let ticket = counter.fetch_add(1, Ordering::AcqRel) + 1;
        (ticket, counter)
    }
}

/// True once a later query from the same window has been dispensed.
fn superseded(counter: &AtomicU64, ticket: u64) -> bool {
    counter.load(Ordering::Acquire) > ticket
}

/// Manage app state and, if a vault is already configured (the common case:
/// app relaunch with an existing vault), open it and start watching — mirrors
/// `agents_sync::init`'s auto-start. `open_vault` is also called directly from
/// the folder picker for a freshly chosen/changed vault; see `lib.rs`.
pub fn init(app: &AppHandle) {
    let (idx_handle, progress, flag, skipped) = managed_state();
    app.manage::<IndexHandle>(idx_handle);
    app.manage(SearchGen::default());
    app.manage(watch::WatchState::default());
    app.manage(progress);
    app.manage(flag);
    app.manage(skipped);
    if let Some(root) = crate::sotvault::resolve_vault_root(app) {
        open_vault(app, &root);
    }
}

pub fn handle(app: &AppHandle) -> IndexHandle {
    app.state::<IndexHandle>().inner().clone()
}

/// Fetch the shared `SkippedState` handle — used by both `open_vault` (this
/// module) and `watch::drain` (a sibling module), which is why this is `pub`
/// rather than kept private like `managed_state`.
pub fn skipped_state(app: &AppHandle) -> SkippedState {
    app.state::<SkippedState>().inner().clone()
}

/// Lock the index handle, recovering from poisoning rather than propagating a
/// panic: one thread panicking mid-reindex must not take the whole app's
/// search feature down with it (global constraint — a broken index must never
/// stop the app).
pub fn lock(handle: &IndexHandle) -> MutexGuard<'_, Option<SearchIndex>> {
    handle.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Pure decision logic for `open_vault`'s `SkippedState` write: given whether
/// this thread's generation is still current and its sweep's result, what (if
/// anything) should be installed into `SkippedState`. `None` means "write
/// nothing" — either because a newer `open_vault` call superseded this one
/// (`is_current` false — a stale thread must never overwrite the *current*
/// vault's skip list with an abandoned vault's, the exact bug review round 1
/// caught: this write used to run before the generation check existed at
/// all) or because the sweep itself failed (nothing to report).
///
/// Factored out of `open_vault`'s closure so this gating is a plain,
/// deterministic function a unit test can drive directly — no thread racing
/// required, since the defect was a pure ordering/gating bug (a write
/// landing on the wrong side of an `if`), not something that only manifests
/// under real concurrency.
fn skipped_write_if_current(
    is_current: bool,
    sweep_result: Result<searchidx::ScanStats, String>,
) -> Option<Vec<searchidx::SkippedFile>> {
    if !is_current {
        return None;
    }
    sweep_result.ok().map(|s| s.files_skipped_large)
}

/// Open (building if empty) the index for `vault_root` and start watching.
/// Failures are logged and swallowed: a broken index must never keep the vault
/// from opening.
///
/// The generation is reserved *synchronously*, on the caller's thread, before
/// any of the slow open/build/sweep work is spawned — so if the user switches
/// vaults again before this call's background thread finishes, call order
/// (not finish order) decides which vault's `SearchIndex` ends up in
/// `IndexHandle`. See `watch::WatchState`'s doc comment for why `IndexHandle`
/// and the watcher must be governed by the same counter.
pub fn open_vault(app: &AppHandle, vault_root: &Path) {
    let my_gen = watch::reserve_generation(app);
    let idx_handle = handle(app);
    // Drop the previous vault's index *now*, synchronously, before any of the
    // slow work below is spawned. Otherwise, for the entire duration of the
    // new vault's open+build+sweep, every query is answered from the OLD
    // vault's index — and `HitDto::abs_path` is built from that index's own
    // `vault_root()`, so clicking a result opens a file inside the vault the
    // user just left. If the new open then fails, that state is permanent:
    // `open_vault` only runs at launch and at vault-pick. Empty means the
    // three commands return `NOT_READY`, which is exactly the honest answer
    // and which the panel already renders.
    *lock(&idx_handle) = None;
    let root = vault_root.to_path_buf();
    let app = app.clone();
    std::thread::spawn(move || {
        let opts = scan_options(&root);
        // The store's staleness stamp is no longer `sync_dir` (C-T6
        // repointed it at `SourceGlobs::stamp()` — see `store::open`'s doc
        // comment). Derived from `opts.source_globs` — the field
        // `search::options::for_vault` (the declared single construction
        // point for `ScanOptions`) already populated above — rather than
        // computed independently, so there is exactly one place that decides
        // what the configured patterns are. Review round 1: an earlier
        // version of this call independently recomputed
        // `SourceGlobs::default()` here (and in `cli::search::run`) instead
        // of reading it off `opts` — harmless while both were stopgaps, but
        // a landmine for whenever `for_vault` starts returning the *real*
        // configured patterns (C-T8): only one of the two call sites getting
        // repointed at the real value would make the GUI and the CLI stamp
        // different values for the same vault and invalidate each other's
        // index on every alternation, and if *neither* is repointed the
        // stamp silently stays `""` forever — the whole invalidation
        // mechanism this task built goes permanently inert with the test
        // suite still green, because nothing relates the stamp to
        // `opts.source_globs`. Reading it off `opts` here makes both classes
        // of drift structurally impossible: C-T8 only has to fix
        // `for_vault`, and every caller of `opts.source_globs` (this
        // included) picks up the real value for free.
        let globs_stamp = opts.source_globs.stamp();
        match SearchIndex::open(&root, &globs_stamp) {
            Ok(mut idx) => {
                if let Err(e) = idx.ensure_built(&opts) {
                    crate::log_cat!("search", "error", "initial build failed: {e}");
                }
                // `sweep` (unlike `ensure_built`, which is a no-op — and so
                // returns an empty `ScanStats` — once the index already has
                // rows) always walks the vault, so its `ScanStats` is the
                // authoritative "what did the last scan actually skip"
                // answer for the settings page. See `SkippedState`'s doc
                // comment for why this has to be stashed anywhere at all.
                //
                // The result is captured but NOT acted on yet — see the
                // `is_current` check below. Unlike `notemd_search_rebuild`
                // and `watch::drain`, this sweep runs on an `idx` that is
                // not yet installed in the shared `IndexHandle`, so it gets
                // none of that mutex's free serialization against a
                // concurrent vault switch. It needs the same explicit
                // generation gate `IndexHandle` already gets below, or a
                // superseded thread can overwrite the *current* vault's
                // `SkippedState` with the abandoned vault's skip list —
                // review round 1 caught this landing one statement above
                // the gate it should have shared.
                let sweep_result = idx.sweep(&opts, None);
                match &sweep_result {
                    // spec §5: `renamed` has to be in this line, or a vault
                    // opened after a directory rename — the case the fast
                    // path exists for — is indistinguishable in the log from
                    // one opened with nothing to do.
                    Ok(s) => crate::log_cat!(
                        "search",
                        "info",
                        "open sweep: {} renamed, {} indexed, {} removed, {} ms",
                        s.files_renamed,
                        s.files_indexed,
                        s.files_removed,
                        s.took_ms
                    ),
                    Err(e) => crate::log_cat!("search", "warn", "sweep failed: {e}"),
                }
                // Discard this thread's work if a newer `open_vault` call has
                // superseded it — otherwise a slow open for a vault the user
                // has since switched away from could overwrite the new
                // vault's (already-current) `IndexHandle` entry, or its
                // `SkippedState`. Snapshotted once into `current` and reused
                // for both gated writes below, rather than calling
                // `is_current` twice — a second call could observe a
                // *different* answer if another `open_vault` reserves a
                // generation in between, which would let the two writes
                // disagree with each other about whether this thread is
                // still current.
                let current = watch::is_current(&app, my_gen);
                if let Some(skipped) = skipped_write_if_current(current, sweep_result) {
                    skipped_state(&app).set(skipped);
                }
                if !current {
                    crate::log_cat!("search", "info", "open_vault superseded, discarding");
                    return;
                }
                *lock(&idx_handle) = Some(idx);
                watch::restart(&app, &root, my_gen);
            }
            Err(e) => crate::log_cat!("search", "error", "index unavailable: {e}"),
        }
    });
}

// --- Tauri commands -------------------------------------------------------
//
// Thin on purpose: everything about what matches and how it ranks lives in
// `searchidx::query`, so these three commands cannot answer a query
// differently than `notemd search` does. See the module doc comment.

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HitDto {
    pub path: String,
    /// Absolute path, so the panel can open the file without re-deriving the
    /// vault root in the frontend — built from the index's own
    /// `vault_root()`, never from anything the caller passes in.
    pub abs_path: String,
    pub line: u32,
    pub line_end: u32,
    pub text: String,
    pub breadcrumb: String,
    pub level: String,
    pub score: f64,
    pub doc_date: Option<String>,
    pub source_ref: String,
    pub agent_by: Option<String>,
    pub human_verified: bool,
    /// `"human" | "derived" | "source"` — `searchidx::Origin::as_str()`
    /// verbatim, same string the `origin:` query filter and the CLI's
    /// `--json` output already use (task B-T6). The panel's two poles
    /// (`grouping.ts`) key off this.
    pub origin: String,
    /// `files.concept_type` verbatim, e.g. `"Book Summary"`; `None` when the
    /// file has no `type`. Only the middle (`derived`) band is subdivided by
    /// this — see `grouping.ts`.
    pub concept_type: Option<String>,
    /// `searchidx::Hit::pinned` verbatim: this hit's file is the wikilink
    /// page the query names exactly. The panel lifts these into their own
    /// group above the origin poles (`grouping.ts`) — without that, a pinned
    /// hit would be sorted first by the backend and then buried inside
    /// whichever origin group its file happens to belong to.
    pub pinned: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub route: String,
    pub took_ms: u64,
    pub total: usize,
    pub hits: Vec<HitDto>,
    /// The retrieval hit its time budget. `hits` is a partial answer.
    pub truncated: bool,
    /// FTS missed and the (expensive) scan fallback was not run because this
    /// was a shallow query — the panel offers it instead of paying for it.
    pub deep_available: bool,
}

/// Wire shape for one entry of `SearchStatsDto.skipped_large` — a file the
/// most recent scan skipped for exceeding the size threshold.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SkippedDto {
    pub path: String,
    pub size_bytes: u64,
}

fn skipped_dto(s: &searchidx::SkippedFile) -> SkippedDto {
    SkippedDto { path: s.path.clone(), size_bytes: s.size }
}

/// Wire shape for `SearchStatsDto.origin_counts` — mirrors
/// `searchidx::OriginCounts` field for field. Task B-T8 (design spec §6):
/// the settings page's "how many files did I write vs. an agent produce vs.
/// raw material" breakdown. `unlabeled` added C-T11, alongside
/// `searchidx::OriginCounts` itself — see that struct's doc comment.
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct OriginCountsDto {
    pub human: i64,
    pub derived: i64,
    pub source: i64,
    pub unlabeled: i64,
}

fn origin_counts_dto(o: searchidx::OriginCounts) -> OriginCountsDto {
    OriginCountsDto { human: o.human, derived: o.derived, source: o.source, unlabeled: o.unlabeled }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatsDto {
    pub files: i64,
    pub blocks: i64,
    pub db_bytes: u64,
    pub built_at: Option<String>,
    pub tokenizer_id: String,
    /// From `SkippedState` — the most recent scan's oversize skips, not a
    /// live re-scan (see that type's doc comment for why it's stashed there
    /// instead of recomputed here).
    pub skipped_large: Vec<SkippedDto>,
    /// Task B-T8: per-tier file counts (design spec §6/§9). Settings-page-only
    /// — never consulted by ranking, which reads `Hit::origin` per hit instead.
    pub origin_counts: OriginCountsDto,
    /// Task B-T8: `derived`'s distribution by `concept_type`, `origin =
    /// 'derived'` and a non-null type only — see `searchidx::type_counts`'s
    /// doc comment for why an untyped-derived bucket is deliberately absent
    /// here rather than under a sentinel key. Keys are raw `concept_type`
    /// strings and MUST NOT be translated by the frontend (same convention
    /// as the search panel's group headers, `src/lib/search/grouping.ts`).
    pub type_counts: std::collections::BTreeMap<String, i64>,
}

/// Shown to the user when no vault is open yet, or `open_vault`'s background
/// thread has not finished opening one. That is an ordinary state during
/// startup/vault-switch, not a failure — kept as one literal so the three
/// commands below cannot drift into different wording for the same state.
const NOT_READY: &str = "search index not ready";

fn require_index(guard: &Option<SearchIndex>) -> Result<&SearchIndex, String> {
    guard.as_ref().ok_or_else(|| NOT_READY.to_string())
}

fn require_index_mut(guard: &mut Option<SearchIndex>) -> Result<&mut SearchIndex, String> {
    guard.as_mut().ok_or_else(|| NOT_READY.to_string())
}

fn hit_to_dto(h: Hit, vault_root: &Path) -> HitDto {
    HitDto {
        abs_path: vault_root.join(&h.path).to_string_lossy().to_string(),
        source_ref: h.source_ref(),
        origin: h.origin.as_str().to_string(),
        concept_type: h.concept_type,
        pinned: h.pinned,
        path: h.path,
        line: h.line,
        line_end: h.line_end,
        text: h.text,
        breadcrumb: h.breadcrumb,
        level: h.level,
        score: h.score,
        doc_date: h.doc_date,
        agent_by: h.agent_by,
        human_verified: h.human_verified,
    }
}

/// The index logging's single exit point. The granularity is deliberate
/// (design spec §5): phases + a milestone every 500 files + exceptions
/// individually — **never per file**. `log_bus` is a single 3000-line ring
/// buffer shared with git-sync, plugins and the frontend console bridge;
/// logging one line per file on a real vault (8,826 files) would evict the
/// whole buffer roughly three times over in one rebuild, wiping out
/// everyone else's logs and even the search log's own early lines. Per-file
/// detail belongs in the settings page's live progress (`current`), which
/// is not buffer-bound.
///
/// `extra` is a caller-supplied callback (Task 4 uses it to write progress
/// state and emit an event). Logging and progress share this one callback
/// so the core crate is never asked to support multiple subscribers.
pub(crate) fn log_rebuild_with(
    idx: &mut SearchIndex,
    opts: &searchidx::ScanOptions,
    extra: Option<&(dyn Fn(&searchidx::Progress) + Send + Sync)>,
) -> Result<searchidx::ScanStats, String> {
    use searchidx::Phase;
    crate::log_cat!(
        "search",
        "info",
        "rebuild start: vault={} mode=full threshold={}MB excludes={:?}",
        idx.vault_root().display(),
        opts.large_file_threshold_mb,
        opts.exclude_dirs
    );

    let last_logged = std::sync::atomic::AtomicUsize::new(0);
    let cb = move |p: &searchidx::Progress| {
        if let Some(f) = extra {
            f(p)
        }
        match p.phase {
            Phase::Walking => {}
            Phase::Indexing => {
                if p.done == 0 {
                    return;
                }
                // One line every 500 files — never per file (see doc comment above).
                let prev = last_logged.load(std::sync::atomic::Ordering::Relaxed);
                if p.done >= prev + 500 {
                    last_logged.store(p.done, std::sync::atomic::Ordering::Relaxed);
                    crate::log_cat!("search", "info", "indexing {}/{}", p.done, p.total);
                }
            }
            Phase::Removing | Phase::Done => {}
        }
    };
    let stats = idx.rebuild_with_progress(opts, Some(&cb))?;

    let skipped = stats.files_skipped_large.len();
    crate::log_cat!(
        "search",
        "info",
        "walk complete: {} indexable found, {} skipped for size",
        stats.files_indexed + skipped,
        skipped
    );
    // Named individually (spec §5) — but bounded. The log ring is 3000 lines
    // shared with git sync, plugins and everything else; a vault holding raw
    // material near the threshold can easily skip hundreds of files, and an
    // unbounded per-file dump would evict every other category's history on a
    // single rebuild. The full list is not lost by capping here: it is what
    // `SkippedState` carries to `notemd_search_stats`, which is what the
    // settings page actually renders.
    const SKIP_LOG_CAP: usize = 50;
    for f in stats.files_skipped_large.iter().take(SKIP_LOG_CAP) {
        crate::log_cat!("search", "warn", "skipped (over threshold): {} ({} bytes)", f.path, f.size);
    }
    if skipped > SKIP_LOG_CAP {
        crate::log_cat!(
            "search",
            "warn",
            "…and {} more skipped for size (full list in the Search & Index settings tab)",
            skipped - SKIP_LOG_CAP
        );
    }

    // Best-effort: a failure to read the db file back must not turn a
    // successful rebuild into an error — the summary line is diagnostic,
    // not load-bearing.
    let db_bytes = idx.stats().map(|s| s.db_bytes).unwrap_or(0);
    crate::log_cat!(
        "search",
        "info",
        "rebuild done: {} indexed, {} removed, {} ms, db={} bytes",
        stats.files_indexed,
        stats.files_removed,
        stats.took_ms,
        db_bytes
    );
    Ok(stats)
}

/// Wire shape for `notemd_search_progress` and the `search://progress` event
/// — see `progress_dto`'s doc comment for the phase-string mapping.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProgressDto {
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub current: Option<String>,
    pub elapsed_ms: u128,
}

/// The event a settings page listens on for live progress during a rebuild
/// (paired with `notemd_search_progress` for a page opened mid-rebuild, which
/// would otherwise see nothing until the next event).
const PROGRESS_EVENT: &str = "search://progress";

/// `searchidx::Phase` has no `Serialize` of its own (the core crate has no
/// wire-format opinions), so the host spells out the mapping once, here.
fn phase_str(p: searchidx::Phase) -> &'static str {
    use searchidx::Phase;
    match p {
        Phase::Walking => "walking",
        Phase::Indexing => "indexing",
        Phase::Removing => "removing",
        Phase::Done => "done",
    }
}

fn progress_dto(p: &searchidx::Progress) -> ProgressDto {
    ProgressDto {
        phase: phase_str(p.phase).to_string(),
        done: p.done,
        total: p.total,
        current: p.current.clone(),
        elapsed_ms: p.elapsed_ms,
    }
}

fn stats_to_dto(s: IndexStats, skipped_large: Vec<SkippedDto>) -> SearchStatsDto {
    SearchStatsDto {
        files: s.files,
        blocks: s.blocks,
        db_bytes: s.db_bytes,
        built_at: s.built_at,
        tokenizer_id: s.tokenizer_id,
        skipped_large,
        origin_counts: origin_counts_dto(s.origin_counts),
        type_counts: s.type_counts,
    }
}

/// Returned instead of results when a newer query has taken over. Not an
/// error the user should ever see: the frontend drops it silently, because by
/// definition a fresher answer is already on its way.
pub const CANCELLED: &str = "search cancelled";

/// `(async)` is load-bearing, not decoration. A plain `#[tauri::command]` is
/// generated as `kind = "sync"` and runs on the IPC handler's thread — the
/// main thread — so every millisecond spent in SQLite is a millisecond the
/// window cannot paint or accept input. With a 1.3M-block index the scan
/// fallback measured 14.3s, which is exactly the "typing freezes the app"
/// report this exists to fix. Holding a `std::sync::Mutex` guard across the
/// body stays sound because `(async)` on a *sync* fn runs it on the blocking
/// threadpool (`kind = "sync_threadpool"`), with no await point inside.
///
/// It also keeps a query issued *during a rebuild* from freezing the window:
/// the rebuild holds the index lock for its whole duration, so run inline
/// this call would block the event loop rather than just itself.
///
/// Not applied blanket to every command in this file — only the two that can
/// block on `IndexHandle`. `notemd_search_rebuild` deliberately does its lock
/// taking on a thread it spawns itself and returns immediately, and
/// `notemd_search_progress` never touches `IndexHandle` at all (that is the
/// entire point of `ProgressState`); both stay in the blocking context, where
/// they answer with the least possible latency — which for the progress poll
/// during a rebuild is exactly the property being protected.
#[tauri::command(async)]
pub fn notemd_search(
    app: AppHandle,
    window: tauri::Window,
    query: String,
    limit: Option<usize>,
    deep: Option<bool>,
    timeout_ms: Option<u64>,
) -> Result<SearchResponse, String> {
    let started = Instant::now();
    let (ticket, counter) = app.state::<SearchGen>().next(window.label());
    let idx_handle = handle(&app);
    search_locked(&idx_handle, started, &query, limit, deep, timeout_ms, &counter, ticket)
}

/// Everything `notemd_search` does once its ticket is drawn — split out with
/// no `AppHandle` in sight so a test can drive it against a real index while
/// another thread holds the very lock this blocks on. That scenario (a query
/// issued during a rebuild or a flood sweep) is the whole point of the
/// deadline placement below, and it is not reachable from a test of the
/// `#[tauri::command]` itself: this codebase has never enabled the
/// `tauri::test` feature, so there is no `AppHandle` to call it with.
///
/// `started` is the *command entry* instant and is used for `took_ms` only —
/// what the user waited is honestly the whole wait, lock included.
#[allow(clippy::too_many_arguments)]
fn search_locked(
    idx_handle: &IndexHandle,
    started: Instant,
    query: &str,
    limit: Option<usize>,
    deep: Option<bool>,
    timeout_ms: Option<u64>,
    counter: &Arc<AtomicU64>,
    ticket: u64,
) -> Result<SearchResponse, String> {
    let guard = lock(idx_handle);
    // The time budget starts HERE, after the lock — not at command entry.
    // Whoever we queued behind (a rebuild's background thread, or
    // `watch::drain`'s `Batch::FullSweep`, which calls `sweep` with no
    // deadline of its own) holds this lock for seconds to minutes on a real
    // vault. Anchored at entry instead, `timeout_ms` would already be spent
    // by the time we get here, `Limits::abort` would fire on the very first
    // progress callback, and SQLite would be interrupted before returning a
    // single row — reported to the panel as `route` set, zero hits,
    // `truncated: true`, which it renders as "No matches" plus a "hit its
    // time limit" footer. A wrong answer, not a slow one: exactly the
    // "nothing matches" vs "we have not looked everywhere yet" confusion
    // `Answer`'s doc comment in `searchidx/src/query.rs` exists to prevent.
    // The budget is for the search; the wait is not part of the search.
    let deadline = timeout_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
    // Waiting for that lock is exactly when a query goes stale, so this is
    // the check that matters most: whatever we queued behind, the user has
    // typed since.
    if superseded(counter, ticket) {
        crate::log_cat!("search", "debug", "query={query:?} superseded");
        return Err(CANCELLED.to_string());
    }
    let idx = require_index(&guard)?;

    let abort_counter = counter.clone();
    let limits = Limits {
        // Default deep so non-interactive callers keep the old behaviour;
        // only the panel's live typing opts into the fast-path-only tier.
        deep: deep.unwrap_or(true),
        abort: Some(Arc::new(move || {
            superseded(&abort_counter, ticket)
                || deadline.is_some_and(|d| Instant::now() >= d)
        })),
    };
    // Read off `limits` *before* the search, because `limits` is moved-from
    // by nothing here but is borrowed below — and more importantly because
    // `deep_used` is what the log line reports, and it must be the value the
    // query actually ran with, not one recomputed from `deep` afterwards.
    let deep_used = limits.deep;
    // Review round 1, Important 2: this used to call `idx.search_with`,
    // which ranks with `Weights::default()` unconditionally — so a user who
    // configured `searchWeights` (once C-T11 ships the UI) would see the
    // settings page read the value back correctly while every actual query
    // kept ranking with the shipped constants, GUI and CLI both.
    // `weights_for_vault` is the single construction point (task C-T8) both
    // adapters must go through; reading it here, off the index's own
    // `vault_root()` rather than a value threaded in from the command, keeps
    // this the one place that decides what a query's weights are.
    let weights = crate::search::options::weights_for_vault(idx.vault_root());
    // Same rule, same place, one query later: `wikipageDir` decides which
    // note (if any) is pinned above everything else, and reading it here off
    // the index's own `vault_root()` keeps this the one place that decides
    // what a query's ranking inputs are. Cheap enough for the typing hot path
    // for the same reason `weights_for_vault` above is — one small JSON file,
    // already in the OS page cache.
    let conventions = crate::search::options::conventions_for_vault(idx.vault_root());
    let answer =
        idx.search_ranked(query, limit.unwrap_or(50), &limits, &weights, &conventions)?;
    // An abort has two causes and they are not the same answer: superseded
    // means "throw this away", deadline means "partial, and say so".
    if superseded(counter, ticket) {
        crate::log_cat!("search", "debug", "query={query:?} superseded");
        return Err(CANCELLED.to_string());
    }
    // Deliberately `debug`, not `info`: `log_bus` is one 3000-line ring buffer
    // shared with git sync, plugins and core, and these lines are produced at
    // typing speed. Visible by default they would evict everything else; at
    // `debug` the Logs window's level filter keeps them out until asked for.
    crate::log_cat!(
        "search",
        "debug",
        "query={query:?} route={} hits={} {}ms deep={deep_used} truncated={}",
        answer.route.as_str(),
        answer.hits.len(),
        started.elapsed().as_millis(),
        answer.truncated
    );
    let root = idx.vault_root().to_path_buf();
    Ok(SearchResponse {
        route: answer.route.as_str().to_string(),
        took_ms: started.elapsed().as_millis() as u64,
        total: answer.hits.len(),
        truncated: answer.truncated,
        deep_available: answer.deep_available,
        hits: answer.hits.into_iter().map(|h| hit_to_dto(h, &root)).collect(),
    })
}

/// Off the IPC thread for the same reason as `notemd_search` above: this
/// takes the index lock, so a settings page opened during a rebuild would
/// otherwise wedge the whole app until the rebuild finished.
#[tauri::command(async)]
pub fn notemd_search_stats(app: AppHandle) -> Result<SearchStatsDto, String> {
    let idx_handle = handle(&app);
    let guard = lock(&idx_handle);
    let idx = require_index(&guard)?;
    let skipped = skipped_state(&app).get().iter().map(skipped_dto).collect();
    Ok(stats_to_dto(idx.stats()?, skipped))
}

/// Rebuild is a rare, explicit, user-initiated action (a button, not
/// something on the hot path), and it still holds the index lock for the
/// whole duration of the actual rebuild rather than swapping the
/// `SearchIndex` out of `IndexHandle` to rebuild it lock-free. Concurrent
/// `notemd_search`/`notemd_search_stats` calls block for that window, but
/// they get an honest "still building" wait; the alternative — taking the
/// index out from under the `Mutex` while it rebuilds — would make every
/// concurrent caller see `NOT_READY` (a vault-not-configured state) for an
/// index that in fact exists and is just busy, which is a worse lie. The file
/// watcher's flood-sweep path (`watch::drain`) already holds this same lock
/// across a full `sweep`, so this is the established pattern, not a new
/// tradeoff.
///
/// What is different here is that the *command* no longer blocks on that
/// lock: the lock is taken on a spawned background thread and this call
/// returns immediately, so a caller polling `notemd_search_progress` (which
/// deliberately never touches `IndexHandle`) is never stuck behind the very
/// rebuild it's asking about. That is why this stays a plain
/// `#[tauri::command] -> Result<(), String>` rather than the
/// `#[tauri::command(async)] -> Result<SearchStatsDto, String>` shape the
/// query-responsiveness rework gave it: an `async` command that returns the
/// finished stats has to *wait out the rebuild* to have stats to return,
/// which is precisely the wait the live progress bar, current-file line and
/// elapsed timer exist to replace. Callers that want the post-rebuild numbers
/// get them the way the settings store already does — refresh stats on the
/// `search://index-updated` event this emits when the thread finishes.
///
/// A rebuild already running is refused via `RebuildFlag`, not queued —
/// three impatient clicks must not become three consecutive full rebuilds.
#[tauri::command]
pub fn notemd_search_rebuild(app: AppHandle) -> Result<(), String> {
    let flag = app.state::<RebuildFlag>().inner().clone();
    if !flag.try_begin() {
        return Err("rebuild already running".into());
    }
    let progress = app.state::<ProgressState>().inner().clone();
    let skipped = skipped_state(&app);
    let idx_handle = handle(&app);
    let app2 = app.clone();
    std::thread::spawn(move || {
        // Cleanup (progress cleared, flag released) is guaranteed by
        // `RebuildGuard::drop`, not by statement order below — see its doc
        // comment. That covers both the ordinary return path and a panic
        // unwinding out of the closure this guards.
        let cleanup = RebuildGuard { progress: progress.clone(), flag: flag.clone() };
        let result = (|| -> Result<(), String> {
            let mut guard = lock(&idx_handle);
            let idx = require_index_mut(&mut guard)?;
            let root = idx.vault_root().to_path_buf();
            let opts = scan_options(&root);
            let progress_for_cb = progress.clone();
            let app_for_cb = app2.clone();
            // Owned closure, not a borrow: it must outlive `log_rebuild_with`'s
            // call only, which it does trivially since it lives on this
            // spawned thread's stack for the duration of that call — no
            // `thread::scope` needed because nothing here is borrowed *from*
            // this thread's stack across a thread boundary.
            let cb = move |p: &searchidx::Progress| {
                progress_for_cb.set(Some(p.clone()));
                let _ = app_for_cb.emit(PROGRESS_EVENT, progress_dto(p));
            };
            let stats = log_rebuild_with(idx, &opts, Some(&cb))?;
            skipped.set(stats.files_skipped_large);
            Ok(())
        })();
        // Explicit drop (rather than waiting for scope end) keeps the
        // pre-guard ordering: progress cleared and the flag released before
        // the completion event fires, same as before this fix. Still
        // panic-safe — if the closure above unwinds instead of returning,
        // `cleanup` is still in scope at that point and `Drop` runs during
        // the unwind, same effect as this explicit call.
        drop(cleanup);
        if let Err(e) = &result {
            crate::log_cat!("search", "error", "rebuild failed: {e}");
        }
        // Lets an open search panel refresh without polling, win or lose —
        // matches `watch::drain`'s successful-sweep emit.
        let _ = app2.emit(watch::INDEX_UPDATED_EVENT, ());
    });
    Ok(())
}

/// Never touches `IndexHandle` — that is the entire point of `ProgressState`
/// living behind its own lock. Called during a rebuild (to poll live
/// progress) and immediately after one starts (a settings page opened
/// mid-rebuild gets this instead of waiting for the next `search://progress`
/// event).
#[tauri::command]
pub fn notemd_search_progress(app: AppHandle) -> Option<ProgressDto> {
    app.state::<ProgressState>().get().as_ref().map(progress_dto)
}

/// How many files in the vault, *right now on disk*, `patterns` would
/// **designate** — the settings page's live preview while a user is
/// drafting a candidate source-glob pattern (task C-T10's `suggestGlobs`,
/// design spec §7.1/§7.2).
///
/// Deliberately walks the vault instead of querying the search index. A
/// pattern's whole reason for existing is to decide whether `.srt`/`.vtt`/
/// `.txt` files get indexed at all (`is_indexable`'s extension gate) — so
/// exactly the files a *new, not-yet-saved* pattern would newly admit are
/// the ones guaranteed not to be in the index yet. Counting from the index
/// would therefore always undercount, and the direction of that error is the
/// bad one: it nudges a user second-guessing a narrow pattern toward
/// widening it "because the count looked low," when the true count (once
/// saved) would already have been fine.
///
/// **A file counts only when `is_indexable` AND the candidate pattern
/// itself matches it** — review round 1, Critical 1. `is_indexable` alone
/// is not "does this pattern designate this file": its `.md` branch is
/// unconditionally `true` regardless of `source_globs` (`.md` is always in
/// scope for indexing, glob-configured or not — see that function's own
/// doc comment), so every candidate, however narrow, would otherwise carry
/// the vault's *entire* ordinary-`.md` baseline. Two consequences that made
/// that wrong, not just imprecise: spec §7.1's narrow→wide candidate ladder
/// could invert (a narrower directory with more `.srt` files could
/// outcount a wider one whose sub-pattern admits none, because both merely
/// tied on the same untouched `.md` baseline), and spec §7.2's "0 files
/// matched" warning — the only safety net for the exact case-flipped-path
/// incident spec §4.1 names as why matching is case-literal — became
/// structurally unreachable, since the count could never drop below the
/// vault's `.md` total. `opts.source_globs.matches(&rel)` is the same
/// field/method `is_indexable` itself already calls internally for the
/// `.srt`/`.vtt`/`.txt` branch; reusing it here for `.md` too is still "one
/// judgment, not a second rule" — it is the *existing* rule, just no longer
/// short-circuited by `.md`'s special case.
///
/// Every other field of `ScanOptions` (the exclude list, in particular)
/// comes from the vault's *actual saved* settings via `for_vault`, so a
/// directory the user has already excluded from search does not inflate a
/// pattern's preview count; only `source_globs` is swapped for the
/// candidate `patterns` being typed, which have not been saved yet.
///
/// `(async)`: a full vault walk is real I/O, and per the doc comment on
/// `notemd_search` above, a plain (non-async) `#[tauri::command]` runs on
/// the main thread — this must not be the one that makes typing in a
/// settings text field freeze the window.
#[tauri::command(async)]
pub fn notemd_search_glob_matches(app: AppHandle, patterns: Vec<String>) -> Result<usize, String> {
    let vault_root = crate::sotvault::resolve_vault_root(&app).ok_or("Vault not configured")?;
    Ok(count_glob_matches(&vault_root, &patterns))
}

/// The command's actual work, split out with a plain `&Path` (no
/// `AppHandle`) so it is drivable from a test the same way
/// `skipped_write_if_current`/`search_locked` are elsewhere in this file —
/// this codebase has never enabled the `tauri::test` feature.
fn count_glob_matches(vault_root: &Path, patterns: &[String]) -> usize {
    let mut opts = scan_options(vault_root);
    opts.source_globs = searchidx::globs::parse(patterns);

    let mut count = 0usize;
    for entry in searchidx::scan::walk_builder(vault_root).build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Some(rel) = searchidx::norm::rel_path(vault_root, entry.path()) else { continue };
        // Designation AND acceptance — see this function's caller's doc
        // comment (review round 1, Critical 1) for why `is_indexable` alone
        // is not enough: its `.md` branch ignores `source_globs` entirely.
        if searchidx::scan::is_indexable(&rel, &opts) && opts.source_globs.matches(&rel) {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod command_tests {
    use super::*;

    fn sample_hit() -> Hit {
        Hit {
            path: "notes/a.md".to_string(),
            line: 12,
            line_end: 14,
            text: "hello world".to_string(),
            breadcrumb: "notes/a.md > Intro".to_string(),
            level: "para".to_string(),
            score: 1.5,
            doc_date: Some("2026-08-10".to_string()),
            agent_by: Some("claude/1.0".to_string()),
            human_verified: true,
            origin: searchidx::Origin::Derived,
            concept_type: Some("Book Summary".to_string()),
            pinned: true,
        }
    }

    #[test]
    fn hit_to_dto_builds_abs_path_from_index_vault_root_not_the_relative_path() {
        let dto = hit_to_dto(sample_hit(), Path::new("/Users/x/Vault"));
        assert_eq!(dto.abs_path, Path::new("/Users/x/Vault/notes/a.md").to_string_lossy());
        assert_eq!(dto.path, "notes/a.md");
    }

    #[test]
    fn hit_to_dto_source_ref_matches_hit_source_ref() {
        let h = sample_hit();
        let expected = h.source_ref();
        let dto = hit_to_dto(h, Path::new("/vault"));
        assert_eq!(dto.source_ref, expected);
        assert_eq!(dto.source_ref, "notes/a.md#L12");
    }

    #[test]
    fn hit_to_dto_preserves_every_field_without_reinterpretation() {
        let dto = hit_to_dto(sample_hit(), Path::new("/vault"));
        assert_eq!(dto.line, 12);
        assert_eq!(dto.line_end, 14);
        assert_eq!(dto.text, "hello world");
        assert_eq!(dto.breadcrumb, "notes/a.md > Intro");
        assert_eq!(dto.level, "para");
        assert_eq!(dto.score, 1.5);
        assert_eq!(dto.doc_date.as_deref(), Some("2026-08-10"));
        assert_eq!(dto.agent_by.as_deref(), Some("claude/1.0"));
        assert!(dto.human_verified);
        assert_eq!(dto.origin, "derived");
        assert_eq!(dto.concept_type.as_deref(), Some("Book Summary"));
    }

    #[test]
    fn stats_to_dto_passes_fields_through_unchanged() {
        let mut type_counts = std::collections::BTreeMap::new();
        type_counts.insert("Book Summary".to_string(), 2i64);
        type_counts.insert("Answer".to_string(), 1i64);
        let s = IndexStats {
            files: 3,
            blocks: 40,
            db_bytes: 12345,
            built_at: Some("2026-08-10T00:00:00Z".to_string()),
            tokenizer_id: "jieba/1".to_string(),
            origin_counts: searchidx::OriginCounts { human: 1, derived: 3, source: 2, unlabeled: 4 },
            type_counts,
        };
        let skipped = vec![SkippedDto { path: "big.md".to_string(), size_bytes: 999 }];
        let dto = stats_to_dto(s, skipped);
        assert_eq!(dto.files, 3);
        assert_eq!(dto.blocks, 40);
        assert_eq!(dto.db_bytes, 12345);
        assert_eq!(dto.built_at.as_deref(), Some("2026-08-10T00:00:00Z"));
        assert_eq!(dto.tokenizer_id, "jieba/1");
        assert_eq!(dto.skipped_large.len(), 1);
        assert_eq!(dto.skipped_large[0].path, "big.md");
        assert_eq!(dto.skipped_large[0].size_bytes, 999);
        assert_eq!(dto.origin_counts.human, 1);
        assert_eq!(dto.origin_counts.derived, 3);
        assert_eq!(dto.origin_counts.source, 2);
        assert_eq!(dto.origin_counts.unlabeled, 4);
        assert_eq!(dto.type_counts.get("Book Summary").copied(), Some(2));
        assert_eq!(dto.type_counts.get("Answer").copied(), Some(1));
    }

    /// `SkippedState` is a fresh, empty `Vec` by default — a rebuild that has
    /// never run must not somehow report a phantom skipped file.
    #[test]
    fn skipped_state_defaults_to_empty() {
        let s = SkippedState::default();
        assert!(s.get().is_empty());
    }

    /// `set`/`get` round-trip exactly — the whole point of this state is that
    /// a scan site can stash a `ScanStats.files_skipped_large` and a later,
    /// unrelated command (`notemd_search_stats`) can read it back unchanged.
    #[test]
    fn skipped_state_round_trips_the_most_recent_set() {
        let s = SkippedState::default();
        s.set(vec![searchidx::SkippedFile { path: "a.md".into(), size: 10 }]);
        assert_eq!(s.get(), vec![searchidx::SkippedFile { path: "a.md".into(), size: 10 }]);
        // A later `set` (a second scan) must replace, not accumulate.
        s.set(vec![searchidx::SkippedFile { path: "b.md".into(), size: 20 }]);
        assert_eq!(s.get(), vec![searchidx::SkippedFile { path: "b.md".into(), size: 20 }]);
    }

    /// Review round 1, finding 1: `open_vault`'s `SkippedState` write used to
    /// land one statement *before* the `is_current` check that guards
    /// `IndexHandle`'s own write — so a superseded vault-switch thread could
    /// overwrite the *current* vault's skipped list with the abandoned
    /// vault's. This pins the fix's core claim: a stale generation must
    /// suppress the write, exactly like it already suppresses the
    /// `IndexHandle` write, regardless of whether the sweep itself
    /// succeeded.
    #[test]
    fn a_stale_generation_suppresses_the_skipped_write_even_on_a_successful_sweep() {
        let stats = searchidx::ScanStats {
            files_skipped_large: vec![searchidx::SkippedFile { path: "big.md".into(), size: 999 }],
            ..Default::default()
        };
        assert_eq!(skipped_write_if_current(false, Ok(stats)), None);
    }

    /// The mirror case: a current generation with a successful sweep must
    /// install exactly that sweep's skipped list, not an empty one or
    /// something derived differently.
    #[test]
    fn a_current_generation_installs_the_sweeps_own_skipped_list() {
        let stats = searchidx::ScanStats {
            files_skipped_large: vec![searchidx::SkippedFile { path: "big.md".into(), size: 999 }],
            ..Default::default()
        };
        assert_eq!(
            skipped_write_if_current(true, Ok(stats)),
            Some(vec![searchidx::SkippedFile { path: "big.md".into(), size: 999 }])
        );
    }

    /// A failed sweep has nothing to report, current generation or not — this
    /// is the same "no crash, no phantom state" contract `open_vault`'s
    /// pre-existing `Err(e) => log` arm already had for the sweep failure
    /// itself; this just pins that the (separate) `SkippedState` write
    /// doesn't invent something to write in that case.
    #[test]
    fn a_failed_sweep_writes_nothing_even_when_current() {
        assert_eq!(skipped_write_if_current(true, Err("boom".to_string())), None);
        assert_eq!(skipped_write_if_current(false, Err("boom".to_string())), None);
    }

    #[test]
    fn require_index_reports_a_state_not_a_stack_trace_when_absent() {
        // `Result::unwrap_err` needs `T: Debug`, which `&SearchIndex` isn't
        // (`searchidx::SearchIndex` deliberately doesn't derive it), so this
        // matches instead of unwrapping.
        let guard: Option<SearchIndex> = None;
        match require_index(&guard) {
            Err(e) => assert_eq!(e, "search index not ready"),
            Ok(_) => panic!("expected an error for an absent index"),
        }
    }

    #[test]
    fn require_index_mut_reports_the_same_message_when_absent() {
        let mut guard: Option<SearchIndex> = None;
        match require_index_mut(&mut guard) {
            Err(e) => assert_eq!(e, "search index not ready"),
            Ok(_) => panic!("expected an error for an absent index"),
        }
    }

    /// Pins the wire shape the frontend is written against (task brief §1):
    /// every field name below is load-bearing for Task 17's TypeScript.
    #[test]
    fn hit_dto_serializes_with_camel_case_field_names() {
        let dto = hit_to_dto(sample_hit(), Path::new("/vault"));
        let v = serde_json::to_value(&dto).unwrap();
        for key in [
            "path",
            "absPath",
            "line",
            "lineEnd",
            "text",
            "breadcrumb",
            "level",
            "score",
            "docDate",
            "sourceRef",
            "agentBy",
            "humanVerified",
            // Review round 1: these two were added to `HitDto` for grouping
            // (task B-T7) but never added HERE. Not exploitable today — the
            // struct-level `rename_all = "camelCase"` is already exercised
            // by the 12 keys above — but a `#[serde(skip)]` or a typo'd
            // `#[serde(rename)]` on either field would silently collapse
            // every hit into `groupHits`'s `derivedOther` bucket (missing
            // `origin` reads as `undefined`, matching neither `'human'` nor
            // `'source'`) and make both poles vanish, with every Rust test
            // still green. This is the one test whose whole job is to catch
            // that class of bug.
            "origin",
            "conceptType",
            // Same class of bug as the two above, with a sharper failure: a
            // missing `pinned` reads as `undefined` in the panel, which is
            // falsy, so the 置顶 group silently never renders — the whole
            // feature gone with every Rust test still green.
            "pinned",
        ] {
            assert!(v.get(key).is_some(), "missing key {key} in {v}");
        }
    }

    #[test]
    fn search_response_serializes_with_camel_case_field_names() {
        let resp = SearchResponse {
            route: "t1-fts".to_string(),
            took_ms: 3,
            total: 0,
            hits: vec![],
            truncated: false,
            deep_available: true,
        };
        let v = serde_json::to_value(&resp).unwrap();
        for key in ["route", "tookMs", "total", "hits", "truncated", "deepAvailable"] {
            assert!(v.get(key).is_some(), "missing key {key} in {v}");
        }
    }

    /// 后端自己发号,是因为前端计数器在 webview reload 后会归零 —— 若由前端
    /// 传号,reload 之后每一次搜索都会被判定为「已被更新的查询取代」而永远
    /// 返回 cancelled,搜索直到重启 app 都是死的。
    #[test]
    fn a_newer_query_supersedes_an_older_one_from_the_same_window() {
        let gen = SearchGen::default();
        let (first, c1) = gen.next("main");
        assert!(!superseded(&c1, first), "a query must not cancel itself");
        let (second, c2) = gen.next("main");
        assert!(second > first);
        assert!(superseded(&c1, first), "the older query must stop working");
        assert!(!superseded(&c2, second));
    }

    /// 时间预算必须从「拿到索引锁之后」开始算,而不是从命令入口。
    ///
    /// 锁的持有者 —— 重建的后台线程,或 `watch::drain` 的 `Batch::FullSweep`
    /// (它调 `sweep(&opts, None)`,自己根本没有 deadline)—— 在真实 vault 上
    /// 一持就是几秒到几分钟。预算若锚在命令入口,等到锁到手时早已花光:
    /// `Limits::abort` 会在 SQLite 的第一次 progress 回调上就触发,语句一行
    /// 都没返回就被 interrupt,响应是 route 非空 + 0 命中 + `truncated: true`,
    /// 面板照此渲染成「没有匹配」外加「已达时间上限」的页脚。50 条命中的
    /// 查询被报成没有匹配 —— 那是错答案,不是慢答案,正是
    /// `searchidx/src/query.rs` 里 `Answer` 的文档注释要防的
    /// 「没有匹配」vs「还没找遍」混淆。
    ///
    /// 这条必须用真索引 + 真的持锁线程:缺陷是「预算被等锁吃掉」,只有真的
    /// 等过一次锁才能暴露。600 个文件是为了让 FTS 语句跑得足够久、必然触发
    /// progress 回调(和上面里程碑测试同一量级),否则语句可能在第一次回调
    /// 之前就跑完,把预期的红变成假绿。
    #[test]
    fn the_time_budget_starts_after_the_index_lock_not_at_command_entry() {
        const BUDGET_MS: u64 = 50;
        // 必须远大于 BUDGET_MS:等锁期间预算若在流逝,出来时就已经透支。
        const HOLD: Duration = Duration::from_millis(400);

        let v = tempfile::tempdir().unwrap();
        for i in 0..600 {
            std::fs::write(v.path().join(format!("f{i}.md")), format!("alpha body {i}\n")).unwrap();
        }
        let d = tempfile::tempdir().unwrap();
        let mut idx =
            searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
        idx.sweep(&searchidx::ScanOptions::default(), None).unwrap();
        let handle: IndexHandle = Arc::new(Mutex::new(Some(idx)));

        // 模拟重建/洪水 sweep 持锁。用 channel 交接而不是 sleep,保证查询
        // 线程一定是「排在锁后面」而不是碰巧先抢到。
        let holder_handle = handle.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let guard = lock(&holder_handle);
            tx.send(()).unwrap();
            std::thread::sleep(HOLD);
            drop(guard);
        });
        rx.recv().unwrap();

        let started = Instant::now();
        let counter: Arc<AtomicU64> = Arc::new(AtomicU64::new(1));
        let resp = search_locked(
            &handle,
            started,
            "alpha",
            Some(50),
            Some(true),
            Some(BUDGET_MS),
            &counter,
            1,
        )
        .expect("查询本身不该失败");
        holder.join().unwrap();

        // 先自证测试确实等过锁 —— 否则下面两条断言在任何实现下都会绿,
        // 这条测试就成了摆设。
        assert!(
            started.elapsed() >= HOLD,
            "测试没有真的排在锁后面(只用了 {:?}),没有复现出等锁场景",
            started.elapsed()
        );
        assert!(
            !resp.hits.is_empty(),
            "等锁把 {BUDGET_MS}ms 预算吃光了:命中被报成 0 条(route={}, truncated={})",
            resp.route,
            resp.truncated
        );
        assert!(
            !resp.truncated,
            "查询在预算内跑完却被标成 truncated —— 面板会显示「已达时间上限」"
        );
        assert!(
            !resp.deep_available,
            "FTS 已经命中,不该再提示深搜"
        );
    }

    /// 两个窗口同时搜索是两个人的意图。互相取消会让其中一个面板空着,而且
    /// 它没有任何办法重新提问 —— 它并不知道自己被别人取消了。
    #[test]
    fn windows_do_not_cancel_each_other() {
        let gen = SearchGen::default();
        let (mine, mine_counter) = gen.next("main");
        let _ = gen.next("daily");
        let _ = gen.next("daily");
        assert!(!superseded(&mine_counter, mine));
    }

    /// Review round 1, Important 2: `search_locked` used to call
    /// `idx.search_with`, which ranks with `Weights::default()`
    /// unconditionally — a `searchWeights` setting could be saved and read
    /// back correctly by the settings page while every actual query, GUI
    /// and CLI, still ranked with the shipped 1.25/1.0/0.9/0.3 constants.
    /// `the_cli_and_the_gui_resolve_the_same_weights` (the contract test)
    /// could not catch that: it only asserts the two adapters *resolve* the
    /// same value, not that either one *uses* it. This drives `search_locked`
    /// itself — the real function `notemd_search` calls — against a real
    /// on-disk index and a real `.notemd/settings.json`, and asserts that
    /// flipping the configured weights from the default ordering to an
    /// inverted one actually reorders the query's results.
    #[test]
    fn a_configured_weight_changes_result_order_through_the_real_query_path() {
        let v = tempfile::tempdir().unwrap();
        // Same FTS-matchable body in both files (only the frontmatter/
        // location differs) so the two hits tie on everything BUT the
        // origin weight multiplier — the only thing that should be able to
        // separate them.
        std::fs::write(v.path().join("derived.md"), "---\ntype: Answer\n---\n\nwidget\n").unwrap();
        std::fs::create_dir_all(v.path().join("raw")).unwrap();
        std::fs::write(v.path().join("raw/source.md"), "widget\n").unwrap();
        std::fs::create_dir_all(v.path().join(".notemd")).unwrap();
        std::fs::write(
            v.path().join(".notemd/settings.json"),
            r#"{"searchSourceGlobs": ["raw/**"]}"#,
        )
        .unwrap();

        let opts = crate::search::options::for_vault(v.path());
        let d = tempfile::tempdir().unwrap();
        let mut idx =
            searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db"), &opts.source_globs.stamp()).unwrap();
        idx.ensure_built(&opts).unwrap();
        let handle: IndexHandle = Arc::new(Mutex::new(Some(idx)));
        let counter: Arc<AtomicU64> = Arc::new(AtomicU64::new(1));

        // Default weights: `derived.md` (Origin::Derived, ×1.0) outranks
        // `raw/source.md` (Origin::Source, ×0.9).
        let default_resp =
            search_locked(&handle, Instant::now(), "widget", Some(10), Some(true), None, &counter, 1).unwrap();
        assert_eq!(
            default_resp.hits.first().map(|h| h.path.as_str()),
            Some("derived.md"),
            "默认权重下 derived 应排在 source 前面 —— 测试前提不成立"
        );

        // Invert the configured weights so `source` dominates `derived`.
        std::fs::write(
            v.path().join(".notemd/settings.json"),
            r#"{"searchSourceGlobs": ["raw/**"], "searchWeights": {"source": 5.0, "derived": 0.1}}"#,
        )
        .unwrap();
        let inverted_resp =
            search_locked(&handle, Instant::now(), "widget", Some(10), Some(true), None, &counter, 1).unwrap();
        assert_eq!(
            inverted_resp.hits.first().map(|h| h.path.as_str()),
            Some("raw/source.md"),
            "配置的反转权重必须真正改变排序,而不是被 Weights::default() 悄悄吃掉"
        );
    }

    /// wikipage 置顶 spec §4:`notemd_search` 必须真的用上解析出来的
    /// `Conventions`,而不是只把它解析对了。
    ///
    /// 这条是照着上面 weights 那条写的 —— C-T8 的 review 记过一次「解析正确
    /// 但查询没用上,而且没有任何测试能发现」。契约测试只证明 GUI 和 CLI
    /// 解析出同一个值;唯有从 `search_locked` 走一遍才证明这个值影响了名次。
    ///
    /// 同时也是「改目录名不必重建索引」在宿主层的落点:两次查询共用同一个
    /// `IndexHandle`,中间只改了 `.notemd/settings.json`。
    #[test]
    fn a_configured_wikipage_dir_actually_pins_through_the_command_path() {
        let v = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(v.path().join("wikipage")).unwrap();
        std::fs::write(v.path().join("wikipage/张三.md"), "---\ntitle: 张三\n---\n- \n").unwrap();
        // 逐字同形、只多一条 `verified` 的诱饵:没有置顶时它靠 human_verified
        // 稳赢,所以第一条断言不是空断言。
        std::fs::write(
            v.path().join("张三.md"),
            "---\ntitle: 张三\nverified:\n  by: human:bruce\n---\n- \n",
        )
        .unwrap();
        std::fs::create_dir_all(v.path().join(".notemd")).unwrap();
        std::fs::write(v.path().join(".notemd/settings.json"), r#"{}"#).unwrap();

        let opts = crate::search::options::for_vault(v.path());
        let d = tempfile::tempdir().unwrap();
        let mut idx =
            searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db"), &opts.source_globs.stamp()).unwrap();
        idx.ensure_built(&opts).unwrap();
        let handle: IndexHandle = Arc::new(Mutex::new(Some(idx)));
        let counter: Arc<AtomicU64> = Arc::new(AtomicU64::new(1));

        let q = |c: &Arc<AtomicU64>| {
            search_locked(&handle, Instant::now(), "张三", Some(10), Some(true), None, c, 1).unwrap()
        };

        // 默认目录名(未配置)下,wikipage 下的那篇被置顶。
        let default_resp = q(&counter);
        assert_eq!(
            default_resp.hits.first().map(|h| h.path.as_str()),
            Some("wikipage/张三.md"),
            "默认 wikipageDir 下 wikipage 里的同名页必须置顶"
        );
        assert!(default_resp.hits[0].pinned, "置顶标记必须一路传到 DTO");

        // 把目录名改到别处 —— 同一个索引,不重建。
        std::fs::write(v.path().join(".notemd/settings.json"), r#"{"wikipageDir": "概念"}"#).unwrap();
        let renamed = q(&counter);
        assert_eq!(
            renamed.hits.first().map(|h| h.path.as_str()),
            Some("张三.md"),
            "改了 wikipageDir 之后旧目录不该再置顶(而且不需要重建索引)"
        );
        assert!(renamed.hits.iter().all(|h| !h.pinned));
    }

    /// 一次全量重建产出的 search 分类日志必须在个位到几十行,而不是逐文件。
    /// log_bus 是全局共享的 3000 行环形缓冲 —— 逐文件写会把 git sync、插件、
    /// 前端 console 的日志全部冲掉,连自己早期的行也留不住。
    #[test]
    fn a_full_rebuild_logs_milestones_not_every_file() {
        let _g = crate::log_bus::test_guard();
        crate::log_bus::clear();

        let v = tempfile::tempdir().unwrap();
        for i in 0..300 {
            std::fs::write(v.path().join(format!("f{i}.md")), format!("body {i}\n")).unwrap();
        }
        let d = tempfile::tempdir().unwrap();
        let mut idx = searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
        log_rebuild_with(&mut idx, &searchidx::ScanOptions::default(), None).unwrap();

        let lines: Vec<_> = crate::log_bus::snapshot()
            .into_iter()
            .filter(|l| l.category == "search")
            .collect();
        assert!(!lines.is_empty(), "一条都没记");
        assert!(lines.len() < 30, "300 个文件产生了 {} 行 search 日志,粒度太细", lines.len());
        assert!(lines.iter().any(|l| l.message.contains("300")), "汇总行应含文件总数: {lines:?}");
        // 300 < 500,门槛必须一次都不触发。光看总行数不够:核心 crate 自己的
        // 节流(每 25 文件一次回调)已经把 300 文件的回调次数压到个位数,
        // 单凭 `lines.len() < 30` 分辨不出"宿主按 500 门槛节流"和"宿主完全
        // 不节流、逐回调就记"——把每 500 的门槛改成每 1(或干脆去掉判断)
        // 都不会让上面那条断言变红。这条断言直接钉住 500 门槛本身。
        assert!(
            lines.iter().all(|l| !l.message.starts_with("indexing")),
            "300 个文件不该越过 500 门槛,但记了里程碑行: {lines:?}"
        );
    }

    /// 上一条测试(300 文件)只证明门槛不提前触发——不证明它在真正越过时
    /// 触发,也不证明触发的格式/`total` 是对的。600 文件精确越过一次 500
    /// 门槛(核心 crate 的节流让回调落在 …476, 501, 526… 上,第一个
    /// `>= 500` 的回调是 501),所以正确实现必须、且只能产生一条
    /// `indexing 501/600`。这条测试和上面那条一起钉住两个方向。
    #[test]
    fn a_full_rebuild_past_the_gate_logs_exactly_one_correctly_formatted_milestone() {
        let _g = crate::log_bus::test_guard();
        crate::log_bus::clear();

        let v = tempfile::tempdir().unwrap();
        for i in 0..600 {
            std::fs::write(v.path().join(format!("f{i}.md")), format!("body {i}\n")).unwrap();
        }
        let d = tempfile::tempdir().unwrap();
        let mut idx = searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
        log_rebuild_with(&mut idx, &searchidx::ScanOptions::default(), None).unwrap();

        let lines: Vec<_> = crate::log_bus::snapshot()
            .into_iter()
            .filter(|l| l.category == "search")
            .collect();
        let milestones: Vec<_> = lines.iter().filter(|l| l.message.starts_with("indexing")).collect();
        assert_eq!(
            milestones.len(),
            1,
            "600 个文件只越过一次 500 门槛,该且仅该产生一条里程碑行: {lines:?}"
        );
        // 断言格式与 total 精确,但 done 只断言落在合理区间:核心 crate 除了
        // 每 25 文件的计数节流,还有一条 200ms 的时间节流,一次时间触发的回调
        // 落在中间就会把整个序列重新对齐(501 → 502、517 之类)。本机实测跑完
        // 600 个文件只要 58ms,今天永远命中 501;但在负载高的机器上跨过 200ms
        // 就会翻红——那是机器慢,不是实现坏。区间上界仍然紧到能抓住真正的
        // 回归:门槛写错(比如每 100 一条)会让第一条落在 500 以下或产生多条,
        // 两条断言各自都会红。
        let caps = milestones[0]
            .message
            .strip_prefix("indexing ")
            .and_then(|rest| rest.split_once('/'))
            .and_then(|(done, total)| Some((done.parse::<usize>().ok()?, total)))
            .unwrap_or_else(|| panic!("里程碑行的格式必须是 `indexing <done>/<total>`: {:?}", milestones[0]));
        assert_eq!(caps.1, "600", "里程碑行的 total 必须是文件总数: {:?}", milestones[0]);
        assert!(
            (500..=525).contains(&caps.0),
            "第一条里程碑必须紧跟在越过 500 门槛之后(允许一次时间节流的错相): {:?}",
            milestones[0]
        );
    }

    /// 超阈值文件按 spec §5 要逐个点名,但不能无限点:log_bus 是 git sync、
    /// 插件、search 共用的 3000 行环形缓冲,一个放了大量素材的 vault 一次重建
    /// 就能刷掉其他所有分类的历史。上限 50 条 + 一条 "…and N more" 汇总,
    /// 完整清单仍由 SkippedState → notemd_search_stats → 设置页承担。
    #[test]
    fn the_skipped_for_size_warnings_are_capped_and_summarize_the_remainder() {
        let _g = crate::log_bus::test_guard();
        crate::log_bus::clear();

        let v = tempfile::tempdir().unwrap();
        for i in 0..60 {
            std::fs::write(v.path().join(format!("big{i:02}.md")), "x").unwrap();
        }
        let d = tempfile::tempdir().unwrap();
        let mut idx = searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
        // 阈值 0 = 任何非空文件都算超标。这里只是让 60 个 1 字节文件全部进
        // skipped 列表,免得为了测日志上限真去写 60 个 10MB 文件;设置层不
        // 允许 0(见 vault_settings::merge 的零值拒绝),这是测试专用取值。
        let opts = searchidx::ScanOptions { large_file_threshold_mb: 0, exclude_dirs: Vec::new(), ..Default::default() };
        let stats = log_rebuild_with(&mut idx, &opts, None).unwrap();
        assert_eq!(stats.files_skipped_large.len(), 60, "60 个文件都该被跳过");

        let warns: Vec<_> = crate::log_bus::snapshot()
            .into_iter()
            .filter(|l| l.category == "search" && l.message.starts_with("skipped (over threshold)"))
            .collect();
        assert_eq!(warns.len(), 50, "逐文件行必须封顶在 50 条: {}", warns.len());

        let summary: Vec<_> = crate::log_bus::snapshot()
            .into_iter()
            .filter(|l| l.category == "search" && l.message.contains("more skipped for size"))
            .collect();
        assert_eq!(summary.len(), 1, "余下的必须有且只有一条汇总行: {summary:?}");
        assert!(
            summary[0].message.contains("10"),
            "汇总行要说清还剩多少条(60 - 50 = 10): {:?}", summary[0]
        );
    }

    /// 查询侧必须留下一行可过滤的痕迹 —— 在此之前 `search` 分类只有索引
    /// 事件,日志窗口按分类筛出来看不到任何「搜索」发生过。
    #[test]
    fn a_query_logs_one_debug_line_under_the_search_category() {
        let _g = crate::log_bus::test_guard();
        crate::log_bus::clear();

        let v = tempfile::tempdir().unwrap();
        std::fs::write(v.path().join("a.md"), "alpha body\n").unwrap();
        let d = tempfile::tempdir().unwrap();
        let mut idx =
            searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
        idx.sweep(&searchidx::ScanOptions::default(), None).unwrap();
        let handle: IndexHandle = Arc::new(Mutex::new(Some(idx)));

        let counter: Arc<AtomicU64> = Arc::new(AtomicU64::new(1));
        search_locked(&handle, Instant::now(), "alpha", Some(50), Some(false), None, &counter, 1)
            .expect("查询本身不该失败");

        let lines: Vec<_> = crate::log_bus::snapshot()
            .into_iter()
            .filter(|l| l.category == "search" && l.message.starts_with("query="))
            .collect();
        assert_eq!(lines.len(), 1, "一次查询恰好一行: {lines:?}");
        // debug 级是刻意的:log_bus 是全局 3000 行环形缓冲,查询行按打字速度
        // 产生,默认可见会把 git sync / 插件的日志顶出去。
        assert_eq!(lines[0].level, "debug");
        let m = &lines[0].message;
        for expected in ["query=\"alpha\"", "route=t1-fts", "hits=1", "deep=false", "truncated=false"] {
            assert!(m.contains(expected), "日志行缺 {expected}: {m}");
        }
    }

    /// 被更新 ticket 抢占的查询也要留痕,否则「打字时查询去哪了」在日志里
    /// 是一片空白。
    #[test]
    fn a_superseded_query_logs_that_it_was_superseded() {
        let _g = crate::log_bus::test_guard();
        crate::log_bus::clear();

        let v = tempfile::tempdir().unwrap();
        std::fs::write(v.path().join("a.md"), "alpha body\n").unwrap();
        let d = tempfile::tempdir().unwrap();
        let idx = searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
        let handle: IndexHandle = Arc::new(Mutex::new(Some(idx)));

        // ticket 1 出发时计数器已经走到 2 —— 正是用户又敲了一个键的情形。
        let counter: Arc<AtomicU64> = Arc::new(AtomicU64::new(2));
        // `SearchResponse` 没有 Debug(它是 serde DTO),所以不能用 expect_err。
        let got = search_locked(&handle, Instant::now(), "alpha", Some(50), Some(false), None, &counter, 1);
        assert_eq!(got.err().as_deref(), Some(CANCELLED), "被抢占的查询必须报 CANCELLED");

        let lines: Vec<_> = crate::log_bus::snapshot()
            .into_iter()
            .filter(|l| l.category == "search" && l.message.contains("superseded"))
            .collect();
        assert_eq!(lines.len(), 1, "被抢占也要留一行: {lines:?}");
        assert_eq!(lines[0].level, "debug");
    }

    /// 不到上限时不该冒出汇总行 —— 否则用户会以为还有没列出来的文件。
    #[test]
    fn under_the_cap_no_and_n_more_line_is_emitted() {
        let _g = crate::log_bus::test_guard();
        crate::log_bus::clear();

        let v = tempfile::tempdir().unwrap();
        for i in 0..3 {
            std::fs::write(v.path().join(format!("big{i}.md")), "x").unwrap();
        }
        let d = tempfile::tempdir().unwrap();
        let mut idx = searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
        let opts = searchidx::ScanOptions { large_file_threshold_mb: 0, exclude_dirs: Vec::new(), ..Default::default() };
        log_rebuild_with(&mut idx, &opts, None).unwrap();

        let lines = crate::log_bus::snapshot();
        assert_eq!(
            lines.iter().filter(|l| l.message.starts_with("skipped (over threshold)")).count(),
            3
        );
        assert!(
            !lines.iter().any(|l| l.message.contains("more skipped for size")),
            "3 < 50,不该有汇总行: {lines:?}"
        );
    }

    /// 本任务的核心不变量:重建进行中,进度依然读得到。
    /// 如果进度状态和索引句柄共用一把锁,这条会挂在 lock 上超时。
    #[test]
    fn progress_is_readable_while_the_index_lock_is_held() {
        let state = ProgressState::default();
        let handle: IndexHandle = Default::default();
        let _held = handle.lock().unwrap(); // 模拟重建期间持锁

        state.set(Some(searchidx::Progress {
            phase: searchidx::Phase::Indexing,
            done: 7,
            total: 100,
            current: Some("a.md".into()),
            elapsed_ms: 12,
        }));
        let got = state.get().expect("持索引锁时进度必须仍可读");
        assert_eq!(got.done, 7);
        assert_eq!(got.total, 100);
    }

    /// 重建进行中再点重建必须被拒,而不是排队 —— 排队意味着用户连点三下
    /// 就锁死三轮全量重建。
    #[test]
    fn a_second_rebuild_is_refused_while_one_is_running() {
        let flag = RebuildFlag::default();
        assert!(flag.try_begin(), "第一次应当拿到");
        assert!(!flag.try_begin(), "进行中第二次必须被拒");
        flag.end();
        assert!(flag.try_begin(), "结束后应当可以再来");
    }

    /// review round 1, finding 2: `progress.clear(); flag.end();` 写在
    /// 重建闭包之后的普通语句里,一旦闭包内部 panic(`searchidx` 有不少
    /// `unwrap`/`expect`,debug/test 构建默认 unwind),两行都会被跳过 ——
    /// `RebuildFlag` 从此永久卡在 `true`,"拒绝,不排队"悄悄变成
    /// "此后进程生命周期内一律拒绝"。这条复现 `notemd_search_rebuild`
    /// 生成线程的确切用法:构造 `RebuildGuard`,再在其作用域内 panic,
    /// 断言 join 后 flag 已经被释放。
    #[test]
    fn rebuild_flag_recovers_after_the_guarded_thread_panics() {
        let flag = RebuildFlag::default();
        let progress = ProgressState::default();
        assert!(flag.try_begin(), "第一次应当拿到");
        progress.set(Some(searchidx::Progress {
            phase: searchidx::Phase::Indexing,
            done: 1,
            total: 10,
            current: None,
            elapsed_ms: 0,
        }));

        let flag_for_thread = flag.clone();
        let progress_for_thread = progress.clone();
        let joined = std::thread::spawn(move || {
            let _cleanup = RebuildGuard { progress: progress_for_thread, flag: flag_for_thread };
            panic!("simulated rebuild_with_progress panic (e.g. an unwrap inside searchidx)");
        })
        .join();

        assert!(joined.is_err(), "线程应当因 panic 结束");
        assert!(
            flag.try_begin(),
            "guard 的 Drop 必须在 panic 展开时也释放 flag,而不是永久卡住"
        );
        assert!(progress.get().is_none(), "guard 的 Drop 也必须在 panic 时清空进度");
    }

    /// review round 1, finding 1: 之前 `progress_is_readable_while_the_index_lock_is_held`
    /// 里 `state`/`handle` 是两个各自独立 `Default::default()` 的局部变量,
    /// 天然不可能互相争用 —— 哪怕生产实现真的把 `ProgressState` 改成复用
    /// `IndexHandle` 的锁*类型*(同类型、不同实例),那条测试也测不出来
    /// (已用临时 mutation 验证过,见 task-4-report.md)。这条改为断言
    /// `init` 真正会用的构造(`managed_state`,`init` 自身直接调用它)—— 比较
    /// `IndexHandle` 和 `ProgressState` 内部 `Arc` 的指针,断言是两块不同的
    /// 分配。
    ///
    /// 这只证明"不同分配",不证明"运行时不会相互阻塞"——真正的阻塞式验证
    /// 需要 `tauri::test` feature,这个代码库从未启用过,不为这一条断言
    /// 引入它。但指针不同已经足以在未来有人把两者合并成一把锁(例如
    /// `ProgressState` 从 `IndexHandle` 的 `Arc::clone` 构造)时变红。
    #[test]
    fn init_wires_progress_state_and_index_handle_to_distinct_locks() {
        let (idx_handle, progress, _flag, skipped) = managed_state();
        let idx_ptr = Arc::as_ptr(&idx_handle) as *const () as usize;
        let progress_ptr = Arc::as_ptr(&progress.0) as *const () as usize;
        let skipped_ptr = Arc::as_ptr(&skipped.0) as *const () as usize;
        assert_ne!(
            idx_ptr, progress_ptr,
            "ProgressState 与 IndexHandle 指向同一块分配 —— 违反本任务的核心不变量"
        );
        assert_ne!(
            idx_ptr, skipped_ptr,
            "SkippedState 与 IndexHandle 指向同一块分配 —— 同样违反独立于索引锁的不变量"
        );
        assert_ne!(
            progress_ptr, skipped_ptr,
            "SkippedState 与 ProgressState 指向同一块分配 —— 两者应各自独立"
        );
    }

    /// 完成后进度必须清空,否则设置页会一直显示一个停在 100% 的旧进度。
    #[test]
    fn progress_is_cleared_when_the_run_finishes() {
        let state = ProgressState::default();
        state.set(Some(searchidx::Progress {
            phase: searchidx::Phase::Done,
            done: 5,
            total: 5,
            current: None,
            elapsed_ms: 1,
        }));
        state.clear();
        assert!(state.get().is_none());
    }

    /// 钉住 wire 形状:每个字段名对设置页的 TypeScript 都是 load-bearing 的。
    #[test]
    fn progress_dto_serializes_with_camel_case_field_names() {
        let p = searchidx::Progress {
            phase: searchidx::Phase::Indexing,
            done: 3,
            total: 10,
            current: Some("a.md".into()),
            elapsed_ms: 42,
        };
        let dto = progress_dto(&p);
        let v = serde_json::to_value(&dto).unwrap();
        for key in ["phase", "done", "total", "current", "elapsedMs"] {
            assert!(v.get(key).is_some(), "missing key {key} in {v}");
        }
        assert_eq!(dto.phase, "indexing");
    }

    #[test]
    fn search_stats_dto_serializes_with_camel_case_field_names() {
        let mut type_counts = std::collections::BTreeMap::new();
        type_counts.insert("Book Summary".to_string(), 2i64);
        let dto = stats_to_dto(
            IndexStats {
                files: 1,
                blocks: 1,
                db_bytes: 1,
                built_at: None,
                tokenizer_id: "jieba/1".to_string(),
                origin_counts: searchidx::OriginCounts { human: 1, derived: 2, source: 3, unlabeled: 4 },
                type_counts,
            },
            vec![SkippedDto { path: "big.md".to_string(), size_bytes: 42 }],
        );
        let v = serde_json::to_value(&dto).unwrap();
        for key in ["files", "blocks", "dbBytes", "builtAt", "tokenizerId", "skippedLarge", "originCounts", "typeCounts"] {
            assert!(v.get(key).is_some(), "missing key {key} in {v}");
        }
        let skipped = v.get("skippedLarge").unwrap().as_array().unwrap();
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].get("path").is_some(), "{skipped:?}");
        assert!(skipped[0].get("sizeBytes").is_some(), "{skipped:?}");
        let origin_counts = v.get("originCounts").unwrap();
        for key in ["human", "derived", "source", "unlabeled"] {
            assert!(origin_counts.get(key).is_some(), "missing key {key} in {origin_counts}");
        }
        assert_eq!(v.get("typeCounts").unwrap().get("Book Summary").unwrap().as_i64(), Some(2));
    }

    /// A candidate pattern only unlocks the `.srt`/`.vtt`/`.txt` files under
    /// it — the same `is_indexable` extension gate the real scan/rebuild
    /// uses. A transcript outside the pattern must not be counted. Fixture
    /// carries ordinary `.md` both inside and outside the pattern too (review
    /// round 1: the original fixture had no `.md` at all and so could not
    /// tell the pre-fix "`is_indexable` alone" semantics apart from the
    /// corrected "designation AND acceptance" one) — only the in-pattern
    /// `.md` should count, alongside the in-pattern `.srt`.
    #[test]
    fn count_glob_matches_counts_files_inside_the_candidate_pattern_only() {
        let v = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(v.path().join("media")).unwrap();
        std::fs::create_dir_all(v.path().join("elsewhere")).unwrap();
        std::fs::write(v.path().join("media/talk.srt"), "1\n00:00:00,000 --> 00:00:01,000\nhi\n").unwrap();
        std::fs::write(v.path().join("media/note.md"), "hi\n").unwrap();
        std::fs::write(v.path().join("elsewhere/other.srt"), "1\n00:00:00,000 --> 00:00:01,000\nhi\n").unwrap();
        std::fs::write(v.path().join("elsewhere/other.md"), "hi\n").unwrap();

        let patterns = vec!["media/**".to_string()];
        assert_eq!(count_glob_matches(v.path(), &patterns), 2, "只有落在模式内的文件（.md 和转写皆是）该被计入");
    }

    /// Corrected meaning (review round 1, Critical 1): a file counts only
    /// when it is BOTH `is_indexable` AND matched by the candidate pattern
    /// itself (`opts.source_globs.matches`) — not `is_indexable` alone,
    /// whose `.md` branch is unconditionally `true` regardless of
    /// `source_globs`. An ordinary `.md` file the candidate pattern does not
    /// cover must not be counted, or every candidate — no matter how narrow
    /// — would carry the vault's entire `.md` baseline, which is exactly
    /// what inverted spec §7.1's own narrow→wide worked example and made
    /// spec §7.2's "0 files matched" warning unreachable (see the two tests
    /// below, which pin those two failure modes directly). This replaces
    /// `count_glob_matches_includes_ordinary_md_files_unconditionally`,
    /// which declared the old (wrong) behavior intentional.
    #[test]
    fn count_glob_matches_excludes_md_files_the_pattern_does_not_cover() {
        let v = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(v.path().join("ebook")).unwrap();
        std::fs::write(v.path().join("ebook/a.md"), "x\n").unwrap();
        std::fs::write(v.path().join("elsewhere.md"), "x\n").unwrap();

        let patterns = vec!["ebook/**".to_string()];
        assert_eq!(count_glob_matches(v.path(), &patterns), 1, "只有落在模式内的 .md 才该计入,不是全库基线");
    }

    /// spec §7.1's own worked example (narrow → wide candidates around one
    /// srt-heavy directory), scaled down for test speed but structurally
    /// identical in shape: an ordinary-`.md` baseline living both inside and
    /// outside the directory in question, plus a directory of transcripts
    /// nested one level deeper than some of that `.md`.
    ///
    /// Under the pre-fix "`is_indexable` alone" semantics, the narrowest
    /// candidate (`ebook/三体/**`) would carry the SAME vault-wide `.md`
    /// baseline as the wider ones (`is_indexable`'s `.md` branch ignores
    /// `source_globs`) while only the wider candidates' extra `.srt` files
    /// counted for real — so a directory with many `.srt` files nested
    /// under a narrow pattern could outrank a wider sibling pattern that
    /// admits none, and the ladder would not even be monotonic once
    /// `notes/`'s unrelated `.md` files are added to the mix. This pins the
    /// corrected, monotonic ordering instead.
    #[test]
    fn count_glob_matches_ladder_is_monotonic_across_narrow_to_wide_candidates() {
        let v = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(v.path().join("notes")).unwrap();
        std::fs::create_dir_all(v.path().join("ebook/三体")).unwrap();
        // Unrelated `.md` baseline elsewhere in the vault — the old
        // semantics let this leak into every candidate below.
        for i in 0..5 {
            std::fs::write(v.path().join(format!("notes/n{i}.md")), "x").unwrap();
        }
        for i in 0..4 {
            std::fs::write(v.path().join(format!("ebook/e{i}.md")), "x").unwrap();
        }
        for i in 0..8 {
            std::fs::write(v.path().join(format!("ebook/t{i}.srt")), "1\n00:00:00,000 --> 00:00:01,000\nhi\n")
                .unwrap();
        }
        std::fs::write(v.path().join("ebook/三体/s.md"), "x").unwrap();
        for i in 0..2 {
            std::fs::write(
                v.path().join(format!("ebook/三体/t{i}.srt")),
                "1\n00:00:00,000 --> 00:00:01,000\nhi\n",
            )
            .unwrap();
        }

        let narrow = count_glob_matches(v.path(), &["ebook/三体/**".to_string()]);
        let mid = count_glob_matches(v.path(), &["ebook/**/*.md".to_string()]);
        let wide = count_glob_matches(v.path(), &["ebook/**".to_string()]);

        assert_eq!(narrow, 3, "ebook/三体/** = 1 md + 2 srt");
        assert_eq!(mid, 5, "ebook/**/*.md 命中 ebook 下全部 5 个 .md,不含任何 srt");
        assert_eq!(wide, 15, "ebook/** = 5 md + 10 srt");
        assert!(
            narrow <= mid && mid <= wide,
            "候选从窄到宽,命中数不该出现反转: narrow={narrow} mid={mid} wide={wide}"
        );
    }

    /// spec §7.2's zero-match warning is the only safety net for the exact
    /// case-flipped-path incident spec §4.1 names as why matching is
    /// case-literal. Under the pre-fix semantics this could never be
    /// reachable — every vault `.md` was unconditionally counted regardless
    /// of the pattern, so the floor was the vault's `.md` total, never 0.
    #[test]
    fn count_glob_matches_is_zero_for_a_pattern_that_designates_nothing() {
        let v = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(v.path().join("Sync")).unwrap();
        std::fs::write(v.path().join("Sync/a.md"), "x\n").unwrap();
        std::fs::write(v.path().join("note.md"), "x\n").unwrap();

        // Case-literal by design (spec §4.1) — "sync" must not match "Sync".
        let patterns = vec!["sync/**".to_string()];
        assert_eq!(count_glob_matches(v.path(), &patterns), 0, "大小写不匹配的模式必须命中 0,警示才打得出来");
    }

    /// Excluded directories (a saved, real setting — distinct from the
    /// unsaved candidate `patterns` being previewed) must still be excluded
    /// from the preview count, or a user who already excluded a huge
    /// directory would see it flood back into every pattern's number.
    /// Fixture carries an in-pattern, non-excluded `.md` too (review round
    /// 1: the original fixture had no `.md` at all) so this test is not
    /// blind to the corrected designation-AND-acceptance semantics either.
    #[test]
    fn count_glob_matches_still_honors_the_vaults_real_exclude_dirs() {
        let v = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(v.path().join(".notemd")).unwrap();
        std::fs::write(
            v.path().join(".notemd/settings.json"),
            r#"{"searchExcludeDirs": ["media/raw"]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(v.path().join("media/raw")).unwrap();
        std::fs::write(v.path().join("media/raw/a.srt"), "1\n00:00:00,000 --> 00:00:01,000\nhi\n").unwrap();
        std::fs::write(v.path().join("media/raw/a.md"), "x\n").unwrap();
        std::fs::write(v.path().join("media/b.srt"), "1\n00:00:00,000 --> 00:00:01,000\nhi\n").unwrap();
        std::fs::write(v.path().join("media/b.md"), "x\n").unwrap();
        // Review round 2, item 3: an ordinary `.md` entirely OUTSIDE the
        // candidate pattern too — without this, the fixture cannot tell
        // "designation AND acceptance" apart from "is_indexable alone",
        // because every `.md` it contained happened to also be covered by
        // the pattern (mutation round 1 left this test green under both
        // semantics).
        std::fs::write(v.path().join("elsewhere.md"), "x\n").unwrap();

        let patterns = vec!["media/**".to_string()];
        assert_eq!(
            count_glob_matches(v.path(), &patterns),
            2,
            "已排除目录下的 .srt 和 .md、以及模式外的 .md 都不该被计入"
        );
    }
}
