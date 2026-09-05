//! The Tauri adapter: an index handle in app state, plus the file watcher.
//! Deliberately thin — every decision about scanning, tokenizing and ranking is
//! `searchidx`'s, so the GUI and the CLI cannot answer the same query
//! differently.

mod attention_links;
pub mod options;
pub(crate) mod plan;
pub(crate) mod smart;
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

/// Where `open_vault`'s background thread currently is, so the UI can tell
/// the three states an empty `IndexHandle` used to collapse into one
/// indistinguishable "not ready".
///
/// `IndexHandle == None` is genuinely ambiguous: no vault yet, an open still
/// running, or an open that *failed* — and the third is permanent (nothing
/// re-runs `open_vault` on its own), while the second clears itself in
/// seconds to minutes. Rendering them identically is what let a failed open
/// masquerade as "the index is still building" indefinitely, with the
/// panel's Rebuild button hidden behind the same branch and
/// `notemd_search_rebuild` unable to recover anyway (it needs the very
/// handle that failed to open). Reported by `notemd_search_index_state`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenPhase {
    /// No `open_vault` has run yet (no vault configured).
    Idle,
    /// A background open/build/sweep is in flight.
    Opening,
    /// An index is installed in `IndexHandle`.
    Ready,
    /// The last open failed; the message is the backend's own error text.
    Failed(String),
}

/// Shared, lock-of-its-own holder for [`OpenPhase`] — same reasoning as
/// [`ProgressState`]: it must stay readable while the index lock is held for
/// a whole rebuild, since "what is the index doing" is exactly the question
/// asked *during* that window.
#[derive(Clone)]
pub struct OpenState(Arc<Mutex<OpenPhase>>);

impl Default for OpenState {
    fn default() -> Self {
        OpenState(Arc::new(Mutex::new(OpenPhase::Idle)))
    }
}

impl OpenState {
    pub fn set(&self, p: OpenPhase) {
        if let Ok(mut g) = self.0.lock() {
            *g = p
        }
    }
    pub fn get(&self) -> OpenPhase {
        self.0
            .lock()
            .map(|g| g.clone())
            .unwrap_or(OpenPhase::Idle)
    }
}

/// Serializes `open_vault`'s background threads against each other.
///
/// Two opens for the same vault overlap routinely — the startup open is still
/// walking the vault when a settings save (`searchSourceGlobs` changed)
/// triggers a reopen. Both then hold their *own* SQLite connection to the
/// same file, and the second one needs to write (`store::rebuild_in_place`
/// empties the tables when the glob stamp changed), so it loses on
/// `SQLITE_BUSY` — observed in the wild as `index unavailable: database is
/// locked` one millisecond after the reopen line. The first thread then
/// finishes, sees it has been superseded, and discards its own perfectly good
/// index: nothing is installed, and the vault has no search until the app is
/// restarted.
///
/// The generation counter alone cannot prevent that: it decides who *installs*,
/// not who *runs*. This lock makes the overlap impossible instead of merely
/// arbitrated — the newer open waits for the older one to let go of the file.
/// The wait is bounded by the older thread's own supersession checks, which
/// bail out at every phase boundary once it is no longer current.
///
/// Lock order is always `OpenLock` → `IndexHandle`, never the reverse
/// (`open_vault` clears the handle on the *caller's* thread, before the lock
/// is ever taken), so this cannot deadlock against a rebuild or a sweep.
pub type OpenLock = Arc<Mutex<()>>;

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
fn managed_state() -> (IndexHandle, ProgressState, RebuildFlag, SkippedState, OpenState, OpenLock) {
    (
        Arc::new(Mutex::new(None)),
        ProgressState::default(),
        RebuildFlag::default(),
        SkippedState::default(),
        OpenState::default(),
        OpenLock::default(),
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
    let (idx_handle, progress, flag, skipped, open_state, open_lock) = managed_state();
    app.manage::<IndexHandle>(idx_handle);
    app.manage(SearchGen::default());
    app.manage(watch::WatchState::default());
    app.manage(progress);
    app.manage(flag);
    app.manage(skipped);
    app.manage(open_state);
    app.manage::<OpenLock>(open_lock);
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

/// 把 `idx` 装进共享的 `IndexHandle` —— **代际检查与写入绑死在同一个函数
/// 里**,中间没有任何缝隙可以插进耗时操作。返回 `false` = 本线程已被更新一代
/// `open_vault` 取代,索引在此处被丢弃(`idx` 按值传入,函数返回即析构),
/// 调用方必须立刻放弃、且不写任何 `OpenPhase`。
///
/// 存在的理由是这个 bug 已经在这份代码里出现过两次:一次是 `SkippedState`
/// 的写入落在了代际检查的错误一侧(review round 1),一次是 T9 在
/// 「检查」与「写入」之间插进了注意力摄取 —— 一段读上千个小文件、用户完全
/// 来得及切走 vault 的耗时操作 —— 却沿用了摄取**之前**那次快照。后果不是
/// 索引内容被污染(`idx` 是线程本地的),而是整个索引对象被装回一个用户已经
/// 离开的 vault:搜索结果与 `HitDto::abs_path` 全部指向旧 vault。
/// `watch::WatchState` 的文档注释把这条不变式写成了「任何 `IndexHandle` 的
/// 写入者都必须在写之前那一刻检查 `is_current`」,这个函数是它的执行者。
///
/// `is_current_now` 是**闭包**而不是 `bool`:传值就意味着调用方可以传一个
/// 陈旧的快照进来 —— 正是本 bug 的形状。传闭包则求值时刻由本函数决定,而它
/// 在**取到锁之后**才求值,把窗口收窄到锁的粒度内:任何更新一代的
/// `open_vault` 都是先(在调用方线程上、同步地)`reserve_generation` 再去清
/// `IndexHandle`,所以「我们持锁时读到的代际」已经涵盖了每一个能抢在前面的
/// 新 open;而在我们之后才预定代际的那个 open,会在我们放锁后照常把这里清成
/// `None` 再装自己的,终局仍然正确。
fn install_if_current(
    handle: &IndexHandle,
    idx: SearchIndex,
    is_current_now: impl FnOnce() -> bool,
) -> bool {
    let mut guard = lock(handle);
    if !is_current_now() {
        return false;
    }
    *guard = Some(idx);
    true
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
    let open_state = app.state::<OpenState>().inner().clone();
    let open_lock = app.state::<OpenLock>().inner().clone();
    let progress_state = app.state::<ProgressState>().inner().clone();
    // Set *synchronously*, before the thread is spawned, for the same reason
    // the generation is reserved here: a settings page that reads the state
    // between the call and the thread's first statement must already see
    // "opening", not the previous open's stale `Ready`/`Failed`.
    open_state.set(OpenPhase::Opening);
    // …and tell any open settings page *now*, not only when the open
    // finishes. Observed in the wild: saving a source-glob change triggers a
    // reopen, the panel keeps rendering the previous open's stats — Rebuild
    // button and all — and the click that button invites can only come back
    // `rebuild failed: search index not ready`, because the handle it needs
    // was just cleared. One event at the start costs a refresh and keeps the
    // page honest about what the backend is doing.
    let _ = app.emit(watch::INDEX_UPDATED_EVENT, ());
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
        // The open's own start line. Until this existed, a launch that spent
        // minutes on a cold build wrote *nothing at all* under the `search`
        // category — so "is it building or is it broken?" was unanswerable
        // from the log too, not just from the settings page.
        crate::log_cat!("search", "info", "opening index: vault={}", root.display());
        // Held for the whole open+build+sweep: while this thread has its own
        // connection to the index file, no *other* `open_vault` may open a
        // second one. See `OpenLock` for the failure this prevents.
        let _open_guard = open_lock.lock().unwrap_or_else(|p| p.into_inner());
        // Waiting on that lock can take as long as the previous open's build,
        // so re-check before doing anything expensive: whoever we waited for
        // may itself have been superseded by a *third* open that is now the
        // one that matters.
        if !watch::is_current(&app, my_gen) {
            crate::log_cat!("search", "info", "open_vault superseded, discarding");
            return;
        }
        // Cleared (and the phase settled) however this thread leaves —
        // including a panic out of `searchidx` — so a failed open can never
        // strand the panel on a progress bar that stopped moving, nor on
        // "opening" forever. Same rationale as `RebuildGuard`.
        let mut outcome = OpenGuard {
            progress: progress_state.clone(),
            state: open_state.clone(),
            phase: Some(OpenPhase::Failed("open interrupted".into())),
        };
        match SearchIndex::open(&root, &globs_stamp) {
            Ok(mut idx) => {
                // The cold-start build is minutes long on a real vault and
                // reported *nothing* until this callback existed: the panel
                // could only say "the index is still building", with no
                // phase, no counts and no elapsed time, because
                // `ProgressState` was written by `notemd_search_rebuild`
                // alone. Same throttled callback shape as that command, so
                // the settings page's progress block is driven identically
                // whoever started the scan.
                let progress_for_cb = progress_state.clone();
                let app_for_cb = app.clone();
                let cb = move |p: &searchidx::Progress| {
                    progress_for_cb.set(Some(p.clone()));
                    let _ = app_for_cb.emit(PROGRESS_EVENT, progress_dto(p));
                };
                match idx.ensure_built_with_progress(&opts, Some(&cb)) {
                    // `ScanStats::default()` — no build ran, the index already
                    // had rows. Deliberately silent: the interesting event is
                    // a cold build, not its absence.
                    Ok(s) if s.files_indexed > 0 => crate::log_cat!(
                        "search",
                        "info",
                        "cold build done: {} indexed, {} ms",
                        s.files_indexed,
                        s.took_ms
                    ),
                    Ok(_) => {}
                    Err(e) => crate::log_cat!("search", "error", "initial build failed: {e}"),
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
                let sweep_result = idx.sweep_with_progress(&opts, None, Some(&cb));
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
                    // Deliberately writes no phase: the newer open owns the
                    // state now, and stamping `Ready`/`Failed` on behalf of a
                    // vault the user has left is exactly the split-brain the
                    // generation counter exists to prevent.
                    outcome.phase = None;
                    return;
                }
                // 索引建好后立刻摄取一次注意力数据:它是排序的输入,晚一步就
                // 意味着用户开 vault 后的第一次搜索拿的是没有注意力的排序。
                // 放在上面那道门之后 —— 被取代的线程不必白读一遍 analytics。
                //
                // ⚠️ 这是一段**耗时**操作(读 `.notemd/analytics/` 下最多一年
                // 的日文件),所以上面那次 `current` 快照到此为止就作废了:
                // 用户完全可能在这期间切到另一个 vault。装 `IndexHandle` 的
                // 代际检查因此**不能**沿用它,见下面 `install_if_current`。
                // 任何以后想在这里插入耗时操作的人:同一条纪律。
                let links = attention_links::links_for_vault(&root);
                match idx.refresh_attention(&links) {
                    Ok(n) => crate::log_cat!("search", "info", "attention ingest: {n} files"),
                    // 摄取失败只降级排序(加成退化成 ×1.0),绝不让整个 open
                    // 失败:没有注意力数据的索引仍然完全可用,而 vault 里根本
                    // 没有 analytics 是完全正常的状态(全新 vault、从没开过
                    // Reading Insights)。把它升级成 open 失败等于让一个可选的
                    // 排序输入否决掉整个搜索功能。
                    //
                    // 级别与 `watch::drain_attention` 那次摄取失败保持一致
                    // (两处都是 `warn`,最终评审 M-2):同一件事记成两个级别,
                    // 按级别过滤日志排障时必然漏掉其中一半。
                    Err(e) => crate::log_cat!("search", "warn", "attention ingest failed: {e}"),
                }
                // 代际在**写入的那一刻**重新读一次,而不是复用摄取之前的快照。
                if !install_if_current(&idx_handle, idx, || watch::is_current(&app, my_gen)) {
                    crate::log_cat!("search", "info", "open_vault superseded, discarding");
                    // 同上:被取代的线程不写任何 phase。
                    outcome.phase = None;
                    return;
                }
                watch::restart(&app, &root, my_gen);
                outcome.phase = Some(OpenPhase::Ready);
                // Settled *before* the event, not left to scope end: the
                // event makes the frontend re-read `notemd_search_index_state`
                // immediately, and a listener that got there first would read
                // the phase this thread has not written yet — i.e. still
                // "opening", with no further event coming to correct it. That
                // is the very "stuck on "still building"" symptom, rebuilt out
                // of a drop-order detail.
                drop(outcome);
                // Tells an open settings page the numbers are available now.
                // Without this, a page that watched the whole build finish
                // kept rendering "still building" until it was closed and
                // reopened — the event was previously emitted only by
                // `notemd_search_rebuild` and the watcher's sweep, never by
                // the one scan every launch actually runs.
                let _ = app.emit(watch::INDEX_UPDATED_EVENT, ());
            }
            Err(e) => {
                crate::log_cat!("search", "error", "index unavailable: {e}");
                // A failed open is permanent — nothing retries on its own —
                // so it has to be *visible*, with the reason, and paired with
                // a retry the user can reach (`notemd_search_reopen`).
                outcome.phase = Some(OpenPhase::Failed(e));
                drop(outcome); // settled before the event — see the Ok arm
                let _ = app.emit(watch::INDEX_UPDATED_EVENT, ());
            }
        }
    });
}

/// RAII settling of `ProgressState`/`OpenState` for `open_vault`'s background
/// thread, for the same reason [`RebuildGuard`] exists: `searchidx` has
/// `unwrap`/`expect` call sites and debug builds unwind, and a panic that
/// left `OpenPhase::Opening` behind would tell the user "still building"
/// about a thread that no longer exists — the exact indistinguishable-forever
/// state this whole change removes.
struct OpenGuard {
    progress: ProgressState,
    state: OpenState,
    /// What to settle on. `None` means "write nothing" — used by a superseded
    /// thread, whose opinion about the current vault is worthless.
    phase: Option<OpenPhase>,
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        self.progress.clear();
        if let Some(p) = self.phase.take() {
            self.state.set(p);
        }
    }
}

// --- Tauri commands -------------------------------------------------------
//
// Thin on purpose: everything about what matches and how it ranks lives in
// `searchidx::query`, so these three commands cannot answer a query
// differently than `notemd search` does. See the module doc comment.

#[derive(Clone, Serialize)]
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
    /// 有注意力数据、**且仍在索引里**的文件数
    /// (`searchidx::IndexStats::attention_files` 原样)。与 `files` 一起构成
    /// 设置页的覆盖率行 —— 「摄取根本没跑起来」在别处没有任何可见症状,这是
    /// 唯一的发现途径。口径是交集(分子永远 ≤ 分母),理由见
    /// `searchidx::store::attention_file_count`。
    pub attention_files: i64,
    /// 上一轮摄取用的 `as_of` 日(`searchidx::IndexStats::attention_as_of`
    /// 原样)。**必须**保持三态可分:`null` = 摄取从未在这个索引上跑过;
    /// 有值且 `attentionFiles == 0` = 跑过但零结果;有值且 > 0 = 正常。
    /// 前端要把前两种说成不同的话,所以这里不许在 DTO 层压成一个布尔或
    /// 空串 —— 一压就再也分不开了。
    pub attention_as_of: Option<String>,
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
        attention_files: s.attention_files,
        attention_as_of: s.attention_as_of,
    }
}

/// Returned instead of results when a newer query has taken over. Not an
/// error the user should ever see: the frontend drops it silently, because by
/// definition a fresher answer is already on its way.
pub const CANCELLED: &str = "search cancelled";

/// Deterministic multi-query search for the global smart-search window.
///
/// The command wrapper lives in this module (rather than being re-exported
/// from `smart`) because `tauri::generate_handler!` resolves the command
/// macro's hidden wrapper at the path supplied to it.  Keeping this entry
/// here therefore lets the app register the unsurprising
/// `search::notemd_smart_search` path while the implementation stays isolated.
#[tauri::command(async)]
pub fn notemd_smart_search(
    app: AppHandle,
    window: tauri::Window,
    query: String,
    limit: Option<usize>,
    deep: Option<bool>,
    timeout_ms: Option<u64>,
) -> Result<smart::SmartSearchResponse, String> {
    smart::run_smart_search_command(app, window, query, limit, deep, timeout_ms)
}

/// Extract the authoritative filters and compute trusted local-date anchors
/// for the isolated planner. This never opens the Vault or the index.
#[tauri::command]
pub fn notemd_search_plan_context(
    original_query: String,
    reference_time: String,
    timezone: String,
) -> Result<plan::SearchPlanContext, String> {
    plan::search_plan_context(&original_query, &reference_time, &timezone)
}

/// Validate and execute a bounded `SearchPlanV1` without accepting a command
/// or raw query DSL from the planner.
#[tauri::command(async)]
pub fn notemd_planned_search(
    app: AppHandle,
    window: tauri::Window,
    original_query: String,
    plan: serde_json::Value,
    baseline_plan: Option<serde_json::Value>,
    reference_time: String,
    timezone: String,
    limit: Option<usize>,
    deep: Option<bool>,
    timeout_ms: Option<u64>,
    retain_run: Option<bool>,
) -> Result<plan::PlannedSearchResponse, String> {
    let limit = Some(match limit {
        Some(value @ 1..=100) => value,
        _ => 100,
    });
    let timeout_ms = Some(match timeout_ms {
        None | Some(0) => 2_000,
        Some(value) => value.min(5_000),
    });
    let mut response = plan::run_planned_search_command(
        app,
        window.clone(),
        original_query.clone(),
        plan,
        baseline_plan,
        reference_time,
        timezone,
        limit,
        deep,
        timeout_ms,
    )?;
    #[cfg(not(target_os = "ios"))]
    if retain_run.unwrap_or(false) {
        crate::smart_lookup::retain_planned_search(&window, &original_query, &mut response)?;
    }
    #[cfg(target_os = "ios")]
    if retain_run.unwrap_or(false) {
        return Err("retained Smart Lookup runs are unavailable on iOS".to_string());
    }
    Ok(response)
}

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
/// Not applied blanket to every command in this file — only the three that can
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
    // `0` is the wire spelling of "no count cap" (the panel's 显示全部) —
    // mapped to the sentinel here at the host boundary, same as the CLI's
    // `--all`; see `searchidx::NO_LIMIT`'s doc comment.
    let limit = match limit.unwrap_or(50) {
        0 => searchidx::NO_LIMIT,
        n => n,
    };
    let answer = idx.search_ranked(query, limit, &limits, &weights, &conventions)?;
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

/// Wire shape for [`OpenPhase`]. Flat `{state, error}` rather than serde's
/// enum shapes because the frontend switches on a plain string.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStateDto {
    /// `"idle" | "opening" | "ready" | "failed"`.
    pub state: String,
    /// Set only for `"failed"`.
    pub error: Option<String>,
}

/// What `IndexHandle == None` actually means right now — never touches that
/// handle (so it answers during a rebuild that holds it, exactly like
/// `notemd_search_progress`).
///
/// Exists because `notemd_search_stats`'s `NOT_READY` cannot distinguish
/// "still opening" from "the open failed and nothing will retry it", and the
/// settings page rendered both as *the same sentence with no way out*.
#[tauri::command]
pub fn notemd_search_index_state(app: AppHandle) -> IndexStateDto {
    index_state_dto(app.state::<OpenState>().get())
}

/// The `OpenPhase` → wire mapping, factored out of the command so the exact
/// strings the frontend branches on are pinned by a unit test without an
/// `AppHandle` (this codebase has never enabled `tauri::test`).
fn index_state_dto(p: OpenPhase) -> IndexStateDto {
    match p {
        OpenPhase::Idle => IndexStateDto { state: "idle".into(), error: None },
        OpenPhase::Opening => IndexStateDto { state: "opening".into(), error: None },
        OpenPhase::Ready => IndexStateDto { state: "ready".into(), error: None },
        OpenPhase::Failed(e) => IndexStateDto { state: "failed".into(), error: Some(e) },
    }
}

/// Re-run `open_vault` for the configured vault — the user-reachable recovery
/// from a failed open.
///
/// `notemd_search_rebuild` cannot serve that purpose and never could: it
/// starts by taking the index out of `IndexHandle`, which is precisely what a
/// failed open never put there, so it answers `NOT_READY` and leaves the
/// user with no action at all. Reopening is also the *correct* remedy for the
/// realistic failure (`database is locked` — a transient loss against another
/// connection), where nothing is wrong with the index worth rebuilding.
///
/// Idempotent by construction: `open_vault` reserves a generation and every
/// older thread stands down, so an impatient double-click costs one wasted
/// open, not two competing ones.
///
/// `async` — i.e. off the main thread — because `open_vault` clears
/// `IndexHandle` synchronously on its caller's thread, and that lock is held
/// for the whole duration of a rebuild or a watcher sweep. On the default
/// (main-thread) command kind, clicking Retry during one of those would
/// freeze the entire UI until it finished — the same reason
/// `notemd_search_stats` is `async`.
#[tauri::command(async)]
pub fn notemd_search_reopen(app: AppHandle) -> Result<(), String> {
    let root = crate::sotvault::resolve_vault_root(&app).ok_or("Vault not configured")?;
    open_vault(&app, &root);
    Ok(())
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
            // fixture 值:与其它字段一样取一个可辨认的非默认数,这样
            // 「DTO 把它压成 0」之类的错误不会伪装成正常。
            attention_minutes: 7.5,
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
            attention_files: 2,
            attention_as_of: Some("2026-08-12".to_string()),
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
        assert_eq!(dto.attention_files, 2);
        assert_eq!(dto.attention_as_of.as_deref(), Some("2026-08-12"));
    }

    /// `attention_as_of` 的三态在 DTO 层不许被压平:`None`(摄取从未跑过)
    /// 和 `Some(day)` + `attention_files == 0`(跑过、零结果)是设置页要分开
    /// 说的两件事。这条钉住 `None` 原样过桥,不被换成空串、也不被 `0` 文件数
    /// 顺手抹成同一种状态。
    #[test]
    fn a_never_ingested_index_keeps_a_null_as_of_distinct_from_a_zero_result_run() {
        let base = |as_of: Option<String>| IndexStats {
            files: 3,
            blocks: 40,
            db_bytes: 1,
            built_at: None,
            tokenizer_id: "jieba/1".to_string(),
            origin_counts: searchidx::OriginCounts::default(),
            type_counts: std::collections::BTreeMap::new(),
            attention_files: 0,
            attention_as_of: as_of,
        };
        let never = stats_to_dto(base(None), Vec::new());
        let zero_result = stats_to_dto(base(Some("2026-08-12".to_string())), Vec::new());
        assert_eq!(never.attention_files, 0);
        assert_eq!(zero_result.attention_files, 0);
        assert_eq!(never.attention_as_of, None);
        assert_eq!(zero_result.attention_as_of.as_deref(), Some("2026-08-12"));
        // 序列化后仍然可分:`null` 与日期字符串,不是同一个值。
        let a = serde_json::to_value(&never).unwrap();
        let b = serde_json::to_value(&zero_result).unwrap();
        assert!(a.get("attentionAsOf").unwrap().is_null(), "{a}");
        assert_eq!(b.get("attentionAsOf").unwrap().as_str(), Some("2026-08-12"));
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

    /// 一个只在测试里用的索引:db 落在 `tempdir` 里(`open_at`,不是 `open`
    /// —— 后者会写进真实的 app-data 目录)。
    fn scratch_index(dir: &std::path::Path) -> SearchIndex {
        SearchIndex::open_at(dir, &dir.join("index.db"), "").expect("open scratch index")
    }

    /// T9 review 抓到的 Critical:注意力摄取被插在「检查代际」与「写
    /// `IndexHandle`」之间,而写入沿用了摄取**之前**的那次快照 —— 用户在慢
    /// 摄取期间切走 vault,旧线程照样把自己的索引装了回去。这条钉住修法的
    /// 核心主张:写入那一刻代际不成立就必须什么都不写。
    #[test]
    fn a_generation_that_went_stale_during_the_slow_work_installs_nothing() {
        let d = tempfile::tempdir().unwrap();
        let handle: IndexHandle = Arc::new(Mutex::new(None));
        // `false` = 摄取期间用户切了 vault(更新一代 open 已预定代际)。
        assert!(!install_if_current(&handle, scratch_index(d.path()), || false));
        assert!(lock(&handle).is_none(), "被取代的线程把索引装了回去");
    }

    /// 同一个门的另一半:代际仍然成立时必须**真的**装进去 —— 否则「谁也装不
    /// 进去」会变成一个绿着的测试套件配上一个永远 `NOT_READY` 的搜索面板。
    #[test]
    fn a_still_current_generation_installs_the_index() {
        let d = tempfile::tempdir().unwrap();
        let handle: IndexHandle = Arc::new(Mutex::new(None));
        assert!(install_if_current(&handle, scratch_index(d.path()), || true));
        let guard = lock(&handle);
        let idx = guard.as_ref().expect("索引没装进 IndexHandle");
        assert_eq!(
            idx.vault_root(),
            Path::new(&searchidx::paths::normalized_vault_root(d.path())),
            "装进去的不是这个 vault 的索引"
        );
    }

    /// 陈旧的**快照**没法伪装成新鲜的检查:`install_if_current` 收的是闭包,
    /// 求值时刻由它自己决定,而且必须发生在拿到锁**之后**。这条用一个记录调用
    /// 顺序的闭包钉住「先锁、后判、再写」,因为一旦有人把签名改回 `bool`,
    /// 调用方就又能把耗时操作之前的快照传进来了 —— 那正是本 bug 的形状。
    #[test]
    fn the_generation_is_read_after_the_lock_is_taken_not_before() {
        let d = tempfile::tempdir().unwrap();
        let handle: IndexHandle = Arc::new(Mutex::new(None));
        let checked = Arc::new(AtomicBool::new(false));
        let seen_locked = Arc::new(AtomicBool::new(false));
        {
            let checked = checked.clone();
            let seen_locked = seen_locked.clone();
            let probe_handle = handle.clone();
            let installed = install_if_current(&handle, scratch_index(d.path()), move || {
                checked.store(true, Ordering::SeqCst);
                // 闭包跑的时候锁必须已经在 `install_if_current` 手里。
                seen_locked.store(probe_handle.try_lock().is_err(), Ordering::SeqCst);
                true
            });
            assert!(installed);
        }
        assert!(checked.load(Ordering::SeqCst), "代际检查压根没跑");
        assert!(seen_locked.load(Ordering::SeqCst), "检查发生在取锁之前,窗口没收窄");
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

    /// `limit: Some(0)` 是「显示全部」的线上拼写:必须映射成
    /// `searchidx::NO_LIMIT` 返回全部命中,而不是字面量 0(零条结果)。
    /// 100 个命中文件特意超过默认的 50 上限,两种语义在此必然分岔。
    #[test]
    fn a_zero_limit_means_every_hit_not_zero_hits() {
        let v = tempfile::tempdir().unwrap();
        for i in 0..100 {
            std::fs::write(v.path().join(format!("f{i}.md")), format!("alpha body {i}\n")).unwrap();
        }
        let d = tempfile::tempdir().unwrap();
        let mut idx =
            searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db"), "sync").unwrap();
        idx.sweep(&searchidx::ScanOptions::default(), None).unwrap();
        let handle: IndexHandle = Arc::new(Mutex::new(Some(idx)));
        let counter: Arc<AtomicU64> = Arc::new(AtomicU64::new(1));

        let capped =
            search_locked(&handle, Instant::now(), "alpha", None, Some(true), None, &counter, 1)
                .expect("默认查询不该失败");
        assert_eq!(capped.hits.len(), 50, "不传 limit 的默认仍是 50 条");

        let all =
            search_locked(&handle, Instant::now(), "alpha", Some(0), Some(true), None, &counter, 1)
                .expect("limit=0 查询不该失败");
        assert_eq!(all.hits.len(), 100, "limit=0 必须返回全部命中");
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

    /// 权重文件改完**不需要重开 vault**:`weights_for_vault` 在 `search_locked`
    /// 里每次查询现读一次盘(`vault_settings::read` 没有任何缓存),所以下一次
    /// 搜索就用上新值。
    ///
    /// 这条与下面那条(origin 四档)是同一个机制,但**不能靠它代劳**:注意力
    /// 的 `k` 除了当乘数,还决定 `fts_arms` 建不建第二条候选臂 —— 四档没有这个
    /// 消费者。谁要是嫌 typing hot path 上每次读盘贵、给 `weights_for_vault`
    /// 加个缓存(那行注释就写着 "cheap enough for the typing hot path",正是招
    /// 人动手的地方),四档那条照样绿,注意力这条会静默失效。
    ///
    /// 观察点刻意**不用**「保底臂捞回窗口外的文档」:那需要 80+ 个命中把
    /// `(limit*8).max(64)` 的窗口撑破,而且目标文档与窗口末位的 bm25 差距必须
    /// 小于 `1+k`(×1.4 天花板)才可能真的可见 —— fixture 稍一漂移就变成永真或
    /// 永假。这里改用「两篇正文逐字相同的文档比顺序」:它们在 bm25、origin、
    /// 日期、批注上全部打平,注意力是唯一自变量,与候选窗口无关。
    #[test]
    fn a_changed_attention_weight_takes_effect_without_reopening_the_vault() {
        let v = tempfile::tempdir().unwrap();
        // 逐字相同的正文 → 除注意力外的一切都打平。
        for name in ["a.md", "b.md"] {
            std::fs::write(v.path().join(name), "---\ntype: Note\n---\n\nwidget\n").unwrap();
        }
        std::fs::create_dir_all(v.path().join(".notemd")).unwrap();
        let settings = v.path().join(".notemd/settings.json");
        std::fs::write(&settings, r#"{"searchWeights": {"attention": 0}}"#).unwrap();

        let opts = crate::search::options::for_vault(v.path());
        let d = tempfile::tempdir().unwrap();
        let mut idx = searchidx::SearchIndex::open_at(
            v.path(),
            &d.path().join("i.db"),
            &opts.source_globs.stamp(),
        )
        .unwrap();
        idx.ensure_built(&opts).unwrap();

        // 走真实摄取路径铺数据(不新增测试专用接口):当天的 analytics 日文件
        // + 一次 `refresh_attention`。日期必须是今天,否则衰减会把它压没。
        let today = searchidx::chunk::ymd_from_unix_public(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        );
        let adir = v.path().join(".notemd/analytics");
        std::fs::create_dir_all(&adir).unwrap();
        std::fs::write(
            adir.join(format!("{today}.DEV-1.json")),
            format!(
                r#"{{"deviceId":"DEV-1","deviceName":"m","day":"{today}","docs":{{"rel:b.md":{{"read_ms":36000000,"edit_ms":0,"open_count":1,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}}}"#
            ),
        )
        .unwrap();
        assert_eq!(idx.refresh_attention(&[]).unwrap(), 1, "摄取必须写进 1 条,否则后面验的是空气");

        let handle: IndexHandle = Arc::new(Mutex::new(Some(idx)));
        let counter: Arc<AtomicU64> = Arc::new(AtomicU64::new(1));

        let off = search_locked(
            &handle, Instant::now(), "widget", Some(10), Some(true), None, &counter, 1,
        )
        .unwrap();
        assert_eq!(
            off.hits.first().map(|h| h.path.as_str()),
            Some("a.md"),
            "attention: 0 时两篇必须完全打平、按稳定顺序出 a.md —— 测试前提不成立"
        );

        // 只改文件,不碰 handle、不重开、不重建索引。
        std::fs::write(&settings, r#"{"searchWeights": {"attention": 0.4}}"#).unwrap();

        let on = search_locked(
            &handle, Instant::now(), "widget", Some(10), Some(true), None, &counter, 1,
        )
        .unwrap();
        assert_eq!(
            on.hits.first().map(|h| h.path.as_str()),
            Some("b.md"),
            "改完权重文件后下一次查询就该用上新值(不需要重开 vault),读过的 b.md 必须反超"
        );
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
        let (idx_handle, progress, _flag, skipped, open_state, _open_lock) = managed_state();
        let idx_ptr = Arc::as_ptr(&idx_handle) as *const () as usize;
        let progress_ptr = Arc::as_ptr(&progress.0) as *const () as usize;
        let skipped_ptr = Arc::as_ptr(&skipped.0) as *const () as usize;
        // `OpenState` joins the same invariant for the same reason: it is read
        // *while* a rebuild holds the index lock (that is the whole window it
        // describes), so sharing that lock would make it unreadable exactly
        // when it matters.
        let open_ptr = Arc::as_ptr(&open_state.0) as *const () as usize;
        assert_ne!(
            idx_ptr, open_ptr,
            "OpenState 与 IndexHandle 指向同一块分配 —— 索引锁被重建占满时状态就读不出来了"
        );
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

    /// 前端按这四个字符串分支(`index-status.svelte.ts`),错一个字就是
    /// 「打开失败」被当成「构建中」渲染 —— 也就是这次要修的那个 bug 本身。
    #[test]
    fn index_state_dto_uses_the_exact_strings_the_frontend_branches_on() {
        assert_eq!(index_state_dto(OpenPhase::Idle).state, "idle");
        assert_eq!(index_state_dto(OpenPhase::Opening).state, "opening");
        assert_eq!(index_state_dto(OpenPhase::Ready).state, "ready");
        let failed = index_state_dto(OpenPhase::Failed("database is locked".into()));
        assert_eq!(failed.state, "failed");
        assert_eq!(
            failed.error.as_deref(),
            Some("database is locked"),
            "失败原因必须原样带到界面 —— 用户看到的就是这句,再据此决定重试还是查日志"
        );
        for p in [OpenPhase::Idle, OpenPhase::Opening, OpenPhase::Ready] {
            assert!(index_state_dto(p).error.is_none(), "只有 failed 带 error");
        }
    }

    /// `open_vault` 的后台线程无论怎么离开(正常结束、`searchidx` 里
    /// panic 展开)都必须把状态落定:留下 `Opening` 就等于告诉用户
    /// 「还在建」,而那个线程已经没了 —— 正是本次要根治的
    /// 「永远显示构建中、按钮消失」。
    #[test]
    fn open_guard_settles_the_phase_and_clears_progress_however_the_thread_leaves() {
        let progress = ProgressState::default();
        let state = OpenState::default();
        progress.set(Some(searchidx::Progress {
            phase: searchidx::Phase::Indexing,
            done: 7,
            total: 99,
            current: Some("a.md".into()),
            elapsed_ms: 5,
        }));
        state.set(OpenPhase::Opening);

        // 模拟 `searchidx` 里 panic 展开出线程闭包:守卫在展开途中 drop。
        let (p2, s2) = (progress.clone(), state.clone());
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _g = OpenGuard {
                progress: p2,
                state: s2,
                phase: Some(OpenPhase::Failed("open interrupted".into())),
            };
            panic!("searchidx exploded");
        }));
        assert!(unwound.is_err(), "这条测试的前提就是真的 panic 了");
        assert_eq!(state.get(), OpenPhase::Failed("open interrupted".into()));
        assert!(progress.get().is_none(), "进度必须清空,否则设置页停在一个不动的进度条上");
    }

    /// 被更新一代 `open_vault` 取代的线程,对「当前 vault」的看法是无效的:
    /// 它既不能宣布 Ready(可能是上一个 vault 的索引),也不能宣布 Failed
    /// (会把正在正常构建的新 vault 标成坏的)。
    #[test]
    fn a_superseded_open_writes_no_phase() {
        let state = OpenState::default();
        state.set(OpenPhase::Opening); // 新一代 open 刚设的
        drop(OpenGuard { progress: ProgressState::default(), state: state.clone(), phase: None });
        assert_eq!(state.get(), OpenPhase::Opening, "过期线程不得覆盖当前一代的状态");
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
                attention_files: 5,
                attention_as_of: Some("2026-08-13".to_string()),
            },
            vec![SkippedDto { path: "big.md".to_string(), size_bytes: 42 }],
        );
        let v = serde_json::to_value(&dto).unwrap();
        for key in [
            "files",
            "blocks",
            "dbBytes",
            "builtAt",
            "tokenizerId",
            "skippedLarge",
            "originCounts",
            "typeCounts",
            "attentionFiles",
            "attentionAsOf",
        ] {
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
