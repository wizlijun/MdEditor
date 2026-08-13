//! notify wiring. All policy — debounce, flood degradation — lives in
//! `searchidx::watch`; this file only turns OS events into `Pending::note` /
//! `Pending::force_sweep` calls and drives the drain loop.
//!
//! A watcher of its own rather than a subscriber to `vault_sync`'s: that one is
//! tightly coupled to its `run_loop`, and merging them would put a new
//! feature's bugs inside the sync path. Listed as P3 debt in the design spec,
//! on purpose (docs/2026-08-10-vault-search-index-design.md §"P3 判据触发").

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use searchidx::watch::{Batch, Pending, DEBOUNCE_MS};
use searchidx::IndexOutcome;
use tauri::{AppHandle, Emitter, Manager};

/// Governs the watcher's lifetime **and** `IndexHandle`'s contents together —
/// the two must never be allowed to drift apart, or `IndexHandle` can end up
/// holding one vault's `SearchIndex` while the live watcher watches a
/// different one (a real bug caught in review: see `search::open_vault`,
/// which reserves a generation via `reserve_generation` *synchronously*,
/// before its slow open/build/sweep work starts, and checks `is_current`
/// immediately before writing `IndexHandle`). The drain-loop thread spawned
/// by `restart` checks the same counter on every wakeup and exits the instant
/// it sees a newer generation, so a watcher for a vault the user has since
/// switched away from cannot keep reindexing into (or out of) the *new*
/// vault's index. See `agents_sync::AgentsSyncState` for the sibling pattern
/// — the difference here is that a second piece of shared state
/// (`IndexHandle`) is also governed by this counter, not just a watcher
/// thread, so any future writer of `IndexHandle` must reserve/check a
/// generation the same way `open_vault` does, or reintroduce the split-brain
/// bug this exists to prevent.
#[derive(Default)]
pub struct WatchState {
    generation: AtomicU64,
}

/// Reserve the next generation number, synchronously, on the caller's thread
/// — never from inside a spawned thread after slow I/O. `open_vault` calls
/// this *before* spawning its background thread so that when two
/// `open_vault` calls race (the user switches vaults again before the first
/// finishes indexing), the winner is decided by *call order*, not by which
/// thread's open/build/sweep happens to finish first.
pub fn reserve_generation(app: &AppHandle) -> u64 {
    app.state::<WatchState>().generation.fetch_add(1, Ordering::SeqCst) + 1
}

/// Whether `gen` is still the most recently reserved generation.
pub fn is_current(app: &AppHandle, generation: u64) -> bool {
    app.state::<WatchState>().generation.load(Ordering::SeqCst) == generation
}

/// Whether an event on `rel` (already vault-relative, `/`-separated) should
/// be forwarded to `Pending`. `.md`-only, and any dot-prefixed path segment
/// (`.git/`, `.notemd/`, …) is excluded so a git operation doesn't trigger a
/// reindex storm — broader than `vault_sync/watcher.rs`'s `.git`-only check,
/// on purpose: a vault can also carry other dot-directories (`.notemd`,
/// `.trash`, …) that must never feed the index.
fn should_forward(rel: &str) -> bool {
    rel.ends_with(".md") && !rel.split('/').any(|s| s.starts_with('.'))
}

/// The vault-relative paths `event` should produce `Pending::note` calls for.
/// Extracted out of the `RecommendedWatcher` callback so it can be
/// unit-tested with synthetic `notify::Event`s — the way
/// `agents_sync::watcher::should_process` is — instead of only being
/// exercisable via real filesystem events.
fn relevant_paths(event: &Event, vault_root: &Path) -> Vec<String> {
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return Vec::new();
    }
    event
        .paths
        .iter()
        .filter_map(|p| searchidx::norm::rel_path(vault_root, p))
        .filter(|rel| should_forward(rel))
        .collect()
}

/// Whether `event` is a directory-level change the per-file channel cannot
/// express. macOS FSEvents / Windows ReadDirectoryChangesW report a directory
/// rename as an event on the directory path *only* — no per-child events — so
/// everything `should_forward` admits misses it and the index keeps every old
/// path until the next full sweep. Detection:
///
/// - rename/create where the path now *is* a directory (the new-name end);
/// - rename/remove where the path is gone *and* extensionless (the old-name
///   end — a vanished `LICENSE`-style file also matches, which costs one
///   cheap stat-walk sweep and nothing else).
///
/// Content/metadata `Modify`s on directories are deliberately excluded: some
/// backends emit them as noise on every child write, and treating those as
/// directory changes would degrade the watcher to sweep-per-batch.
fn needs_sweep(event: &Event, vault_root: &Path) -> bool {
    use notify::event::ModifyKind;
    let renameish = matches!(event.kind, EventKind::Modify(ModifyKind::Name(_)));
    let createish = renameish || matches!(event.kind, EventKind::Create(_));
    let removeish = renameish || matches!(event.kind, EventKind::Remove(_));
    if !createish && !removeish {
        return false;
    }
    event.paths.iter().any(|p| {
        let Some(rel) = searchidx::norm::rel_path(vault_root, p) else {
            return false;
        };
        if should_forward(&rel) || rel.split('/').any(|s| s.starts_with('.')) {
            return false;
        }
        (createish && p.is_dir())
            || (removeish && !p.exists() && Path::new(&rel).extension().is_none())
    })
}

/// What the watcher callback sends the drain loop: a vault-relative `.md`
/// path, or the fact that a directory changed (which has no path the per-file
/// channel could carry).
enum Note {
    File(String),
    DirChange,
}

/// (Re)start the watcher for `vault_root`, under the generation `my_gen`
/// reserved by the caller (`open_vault`) via `reserve_generation`. Safe to
/// call again — e.g. when the user picks a different vault folder: the
/// previous watcher/thread notices a newer generation and exits instead of
/// continuing to write into the old vault's (still-live) `IndexHandle`
/// contents.
pub fn restart(app: &AppHandle, vault_root: &Path, my_gen: u64) {
    let (tx, rx) = mpsc::channel::<Note>();
    let root = vault_root.to_path_buf();
    let filter_root = root.clone();

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            let Ok(event) = res else { return };
            if needs_sweep(&event, &filter_root) {
                let _ = tx.send(Note::DirChange);
            }
            for rel in relevant_paths(&event, &filter_root) {
                let _ = tx.send(Note::File(rel));
            }
        },
        // Matches `vault_sync/watcher.rs` and `agents_sync/watcher.rs`: the
        // poll interval only governs the `PollWatcher` fallback, but setting
        // it keeps this watcher's degraded-backend behavior consistent with
        // its two siblings rather than silently diverging from the house
        // pattern.
        notify::Config::default().with_poll_interval(Duration::from_secs(2)),
    ) {
        Ok(w) => w,
        Err(e) => {
            crate::log_cat!("search", "error", "watcher unavailable: {e}");
            return;
        }
    };
    if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
        crate::log_cat!("search", "error", "cannot watch {}: {e}", root.display());
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        // Keep the watcher alive for the life of this thread: notify stops
        // delivering events the instant `RecommendedWatcher` is dropped, so it
        // must live inside the loop that consumes `rx`, not be dropped at the
        // end of `restart`.
        let _watcher = watcher;
        let mut pending = Pending::default();
        let stale = || app.state::<WatchState>().generation.load(Ordering::SeqCst) != my_gen;
        loop {
            if stale() {
                return;
            }
            match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
                Ok(Note::File(rel)) => pending.note(rel),
                Ok(Note::DirChange) => pending.force_sweep(),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if pending.is_empty() {
                        continue;
                    }
                    if stale() {
                        return;
                    }
                    drain(&app, &root, pending.take());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

fn drain(app: &AppHandle, root: &Path, batch: Batch) {
    let idx_handle = crate::search::handle(app);
    let opts = crate::search::scan_options(root);
    // The lock is held only across the reindex calls below — not across
    // `scan_options` (a settings-file read) or the event emit after — so a
    // concurrent Tauri search command is blocked for the reindex itself and
    // nothing more.
    let mut guard = crate::search::lock(&idx_handle);
    let Some(idx) = guard.as_mut() else { return };
    let ok = match batch {
        // One `apply_batch` rather than a loop of `index_one`: a rename
        // arrives as a removal plus a creation, and only a whole batch can
        // see both ends and settle it as an UPDATE instead of a rebuild plus
        // a delete.
        Batch::Files(paths) => match idx.apply_batch(&paths, &opts) {
            Ok(out) => {
                // A summary line only when something was renamed: a batch
                // that reindexed a file the user just saved is the ordinary
                // case and has never been logged, and starting now would put
                // a line in the ring on every keystroke-triggered save.
                if out.renamed > 0 {
                    crate::log_cat!(
                        "search",
                        "info",
                        "batch: {} renamed, {} reindexed, {} removed",
                        out.renamed,
                        out.reindexed,
                        out.removed
                    );
                }
                for (rel, outcome) in &out.removals {
                    log_outcome(rel, *outcome);
                }
                true
            }
            Err(e) => {
                crate::log_cat!("search", "error", "batch of {} paths failed: {e}", paths.len());
                false
            }
        },
        Batch::FullSweep => match idx.sweep(&opts, None) {
            Ok(stats) => {
                // Same reasoning as `open_vault`'s sweep call — this is a
                // scan the settings page's skipped-files list would
                // otherwise never learn happened (it wasn't routed through
                // `notemd_search_rebuild`, the only other writer it might
                // expect).
                // spec §5: without `renamed` in the line, a sweep that
                // rebuilt nothing *because a directory was renamed* reads
                // exactly like a sweep with nothing to do.
                crate::log_cat!(
                    "search",
                    "info",
                    "full sweep: {} renamed, {} indexed, {} removed, {} ms",
                    stats.files_renamed,
                    stats.files_indexed,
                    stats.files_removed,
                    stats.took_ms
                );
                crate::search::skipped_state(app).set(stats.files_skipped_large);
                true
            }
            Err(e) => {
                crate::log_cat!("search", "error", "full sweep failed: {e}");
                false
            }
        },
    };
    drop(guard);
    if ok {
        // Lets an open search panel refresh without polling.
        let _ = app.emit(INDEX_UPDATED_EVENT, ());
    }
}

/// The only contract between this watcher and `SearchPanel.svelte`, and a
/// string on both sides with no compiler anywhere in between: rename either
/// end and the panel simply stops auto-refreshing, silently, for everyone.
/// `the_index_updated_event_name_matches_the_panels_listener` reads the Svelte
/// file and pins the pair.
pub const INDEX_UPDATED_EVENT: &str = "search://index-updated";

/// Log *why* a file left the index (or that it was, in fact, indexed) rather
/// than collapsing every outcome into a bare bool — distinguishing "gone",
/// "oversized" and "excluded" matters for someone reading `/tmp/mdeditor.log`
/// after the fact.
fn log_outcome(rel: &str, outcome: IndexOutcome) {
    match outcome {
        IndexOutcome::Indexed => {}
        IndexOutcome::RemovedMissing => {
            crate::log_cat!("search", "info", "{rel} left the index (file gone)")
        }
        IndexOutcome::RemovedOversized => {
            crate::log_cat!("search", "warn", "{rel} left the index (now oversized)")
        }
        IndexOutcome::RemovedNotIndexable => {
            crate::log_cat!("search", "info", "{rel} left the index (excluded/not indexable)")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind, RenameMode};
    use std::path::PathBuf;

    #[test]
    fn should_forward_accepts_plain_markdown() {
        assert!(should_forward("a.md"));
        assert!(should_forward("notes/sub/a.md"));
    }

    #[test]
    fn should_forward_rejects_non_markdown() {
        assert!(!should_forward("a.txt"));
        assert!(!should_forward("notes/a.png"));
        // Not a suffix match: a directory or filename that merely contains
        // ".md" must not slip through.
        assert!(!should_forward("a.md.tmp"));
    }

    #[test]
    fn should_forward_rejects_dot_git() {
        assert!(!should_forward(".git/HEAD.md"));
        assert!(!should_forward("sub/.git/objects/pack.md"));
    }

    #[test]
    fn should_forward_rejects_dot_notemd() {
        assert!(!should_forward(".notemd/settings.md"));
        assert!(!should_forward("a/.notemd/index.md"));
    }

    fn modify_event(path: &str) -> Event {
        Event::new(EventKind::Modify(ModifyKind::Any)).add_path(PathBuf::from(path))
    }

    #[test]
    fn relevant_paths_resolves_and_filters_relative_to_root() {
        let root = Path::new("/vault");
        let event = modify_event("/vault/notes/a.md");
        assert_eq!(relevant_paths(&event, root), vec!["notes/a.md".to_string()]);
    }

    #[test]
    fn relevant_paths_drops_non_markdown_and_dot_dirs() {
        let root = Path::new("/vault");
        assert!(relevant_paths(&modify_event("/vault/notes/a.txt"), root).is_empty());
        assert!(relevant_paths(&modify_event("/vault/.git/HEAD.md"), root).is_empty());
        assert!(relevant_paths(&modify_event("/vault/.notemd/settings.md"), root).is_empty());
    }

    #[test]
    fn relevant_paths_ignores_access_events() {
        let root = Path::new("/vault");
        let access = Event::new(EventKind::Access(AccessKind::Any))
            .add_path(PathBuf::from("/vault/notes/a.md"));
        assert!(relevant_paths(&access, root).is_empty());
    }

    /// `search://index-updated` exists in exactly two places — the `emit`
    /// above and `SearchPanel.svelte`'s `listen` — with no shared declaration
    /// and no build step that checks them against each other. Rename either
    /// and the panel just stops refreshing after a save: no error, no log
    /// line, nothing to notice. So the test reads the other side's source and
    /// asserts the literal is really there.
    #[test]
    fn the_index_updated_event_name_matches_the_panels_listener() {
        let panel = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../src/components/side-panel/SearchPanel.svelte");
        let src = std::fs::read_to_string(&panel)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", panel.display()));
        let needle = format!("listen('{INDEX_UPDATED_EVENT}'");
        assert!(
            src.contains(&needle),
            "SearchPanel.svelte does not listen for {INDEX_UPDATED_EVENT} \
             (looked for {needle:?}) — auto-refresh is silently dead"
        );
    }

    /// 目录改名在 FSEvents/ReadDirectoryChangesW 上只报目录自身路径,不逐个报
    /// 子文件 —— 事件若被 `.md` 过滤器整个吞掉,索引会一直留着旧路径(v6.813.4
    /// 实测:改目录名后搜索结果指向不存在的文件)。目录事件必须触发全量 sweep。
    #[test]
    fn a_directory_rename_event_needs_a_sweep() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Paper Bushcraft");
        std::fs::create_dir(&dir).unwrap();
        let ev = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any))).add_path(dir);
        assert!(needs_sweep(&ev, tmp.path()));
    }

    /// 改名事件的「旧路径」端:磁盘上已不存在,无扩展名 → 按目录对待。
    /// (目录移出 vault / 移入废纸篓时只有这一端。)
    #[test]
    fn a_vanished_extensionless_path_needs_a_sweep() {
        let tmp = tempfile::tempdir().unwrap();
        let ev = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
            .add_path(tmp.path().join("2026-08"));
        assert!(needs_sweep(&ev, tmp.path()));
    }

    /// 普通文件事件不许 sweep:.md 走文件通道,其余(图片等)维持忽略 ——
    /// 否则每次保存/贴图都全量扫一遍。
    #[test]
    fn plain_file_events_do_not_need_a_sweep() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("cover.png"), b"x").unwrap();
        let png = Event::new(EventKind::Create(CreateKind::File))
            .add_path(tmp.path().join("cover.png"));
        assert!(!needs_sweep(&png, tmp.path()));
        let md = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
            .add_path(tmp.path().join("a.md"));
        assert!(!needs_sweep(&md, tmp.path()));
    }

    /// 内容/元数据类 Modify 不算目录变化:FSEvents 可能对目录本身发此类噪声,
    /// 若都触发 sweep,watcher 就退化成「每批全量」。
    #[test]
    fn dir_content_modify_noise_does_not_need_a_sweep() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("notes");
        std::fs::create_dir(&dir).unwrap();
        let ev = Event::new(EventKind::Modify(ModifyKind::Any)).add_path(dir);
        assert!(!needs_sweep(&ev, tmp.path()));
    }

    /// 点目录(.git/.notemd/…)下的目录事件与 `should_forward` 同规则排除。
    #[test]
    fn dot_dir_events_do_not_need_a_sweep() {
        let tmp = tempfile::tempdir().unwrap();
        let git = tmp.path().join(".git");
        std::fs::create_dir_all(git.join("objects")).unwrap();
        let ev = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
            .add_path(git.join("objects"));
        assert!(!needs_sweep(&ev, tmp.path()));
    }

    /// 真实后端(FSEvents/inotify)整链验证:目录改名必须产生至少一个
    /// `needs_sweep` 认定的事件。合成事件测不出 notify 对目录改名的真实
    /// 事件形状 —— v6.813.4 的盲区恰恰漏在这里(`.md` 过滤器把目录事件
    /// 整个吞掉),这条测试直接对着真文件系统钉死它。
    #[test]
    fn real_backend_reports_a_directory_rename_sweep_worthily() {
        use std::sync::{Arc, Mutex};
        let tmp = tempfile::tempdir().unwrap();
        // FSEvents 报 /private/var 实路径,tempdir 给 /var 符号链接 —— 不
        // canonicalize 则 rel_path 解不出相对路径,事件全被丢掉。
        let root = tmp.path().canonicalize().unwrap();
        let old = root.join("olddir");
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("a.md"), "x").unwrap();

        let hit = Arc::new(Mutex::new(false));
        let hit_w = hit.clone();
        let watch_root = root.clone();
        let mut w = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(ev) = res {
                    if needs_sweep(&ev, &watch_root) {
                        *hit_w.lock().unwrap() = true;
                    }
                }
            },
            notify::Config::default(),
        )
        .unwrap();
        use notify::Watcher as _;
        w.watch(&root, RecursiveMode::Recursive).unwrap();
        // 让 watcher 启动期的历史/噪声事件先过去,再清旗、改名。
        std::thread::sleep(Duration::from_millis(500));
        *hit.lock().unwrap() = false;
        std::fs::rename(&old, root.join("newdir")).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if *hit.lock().unwrap() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("directory rename produced no sweep-worthy event within 5s");
    }

    #[test]
    fn relevant_paths_accepts_create_and_remove() {
        let root = Path::new("/vault");
        let create =
            Event::new(EventKind::Create(CreateKind::File)).add_path(PathBuf::from("/vault/a.md"));
        assert_eq!(relevant_paths(&create, root), vec!["a.md".to_string()]);
        let remove = Event::new(EventKind::Remove(RemoveKind::File))
            .add_path(PathBuf::from("/vault/a.md"));
        assert_eq!(relevant_paths(&remove, root), vec!["a.md".to_string()]);
    }
}
