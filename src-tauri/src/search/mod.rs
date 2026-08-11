//! The Tauri adapter: an index handle in app state, plus the file watcher.
//! Deliberately thin — every decision about scanning, tokenizing and ranking is
//! `searchidx`'s, so the GUI and the CLI cannot answer the same query
//! differently.

pub mod options;
pub mod watch;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use searchidx::{Hit, IndexStats, SearchIndex};
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

/// Manage app state and, if a vault is already configured (the common case:
/// app relaunch with an existing vault), open it and start watching — mirrors
/// `agents_sync::init`'s auto-start. `open_vault` is also called directly from
/// the folder picker for a freshly chosen/changed vault; see `lib.rs`.
pub fn init(app: &AppHandle) {
    let (idx_handle, progress, flag, skipped) = managed_state();
    app.manage::<IndexHandle>(idx_handle);
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
        match SearchIndex::open(&root) {
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
                if let Err(e) = &sweep_result {
                    crate::log_cat!("search", "warn", "sweep failed: {e}");
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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub route: String,
    pub took_ms: u64,
    pub total: usize,
    pub hits: Vec<HitDto>,
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
    for f in &stats.files_skipped_large {
        crate::log_cat!("search", "warn", "skipped (over threshold): {} ({} bytes)", f.path, f.size);
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
    }
}

#[tauri::command]
pub fn notemd_search(app: AppHandle, query: String, limit: Option<usize>) -> Result<SearchResponse, String> {
    let started = std::time::Instant::now();
    let idx_handle = handle(&app);
    let guard = lock(&idx_handle);
    let idx = require_index(&guard)?;
    let (hits, route) = idx.search(&query, limit.unwrap_or(50))?;
    let root = idx.vault_root().to_path_buf();
    Ok(SearchResponse {
        route: route.as_str().to_string(),
        took_ms: started.elapsed().as_millis() as u64,
        total: hits.len(),
        hits: hits.into_iter().map(|h| hit_to_dto(h, &root)).collect(),
    })
}

#[tauri::command]
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
/// `SearchIndex` out of `IndexHandle` to rebuild it lock-free — the file
/// watcher's flood-sweep path (`watch::drain`) already holds this same lock
/// across a full `sweep`, so this is the established pattern, not a new
/// tradeoff. What changes here is that the command itself no longer blocks
/// on it: the lock is taken on a spawned background thread, and this command
/// returns immediately, so a caller polling `notemd_search_progress` (which
/// deliberately never touches `IndexHandle`) is never stuck behind the very
/// rebuild it's asking about.
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
    }

    #[test]
    fn stats_to_dto_passes_fields_through_unchanged() {
        let s = IndexStats {
            files: 3,
            blocks: 40,
            db_bytes: 12345,
            built_at: Some("2026-08-10T00:00:00Z".to_string()),
            tokenizer_id: "jieba/1".to_string(),
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
        ] {
            assert!(v.get(key).is_some(), "missing key {key} in {v}");
        }
    }

    #[test]
    fn search_response_serializes_with_camel_case_field_names() {
        let resp = SearchResponse { route: "t1-fts".to_string(), took_ms: 3, total: 0, hits: vec![] };
        let v = serde_json::to_value(&resp).unwrap();
        for key in ["route", "tookMs", "total", "hits"] {
            assert!(v.get(key).is_some(), "missing key {key} in {v}");
        }
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
        let mut idx = searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db")).unwrap();
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
        let mut idx = searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db")).unwrap();
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
        assert_eq!(
            milestones[0].message, "indexing 501/600",
            "里程碑行的格式与 total 必须精确: {:?}", milestones[0]
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
        let dto = stats_to_dto(
            IndexStats {
                files: 1,
                blocks: 1,
                db_bytes: 1,
                built_at: None,
                tokenizer_id: "jieba/1".to_string(),
            },
            vec![SkippedDto { path: "big.md".to_string(), size_bytes: 42 }],
        );
        let v = serde_json::to_value(&dto).unwrap();
        for key in ["files", "blocks", "dbBytes", "builtAt", "tokenizerId", "skippedLarge"] {
            assert!(v.get(key).is_some(), "missing key {key} in {v}");
        }
        let skipped = v.get("skippedLarge").unwrap().as_array().unwrap();
        assert_eq!(skipped.len(), 1);
        assert!(skipped[0].get("path").is_some(), "{skipped:?}");
        assert!(skipped[0].get("sizeBytes").is_some(), "{skipped:?}");
    }
}
