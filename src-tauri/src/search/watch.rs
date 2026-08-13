//! notify wiring. All policy — debounce, flood degradation — lives in
//! `searchidx::watch`; this file only turns OS events into `Pending::note`
//! calls and drives the drain loop.
//!
//! A watcher of its own rather than a subscriber to `vault_sync`'s: that one is
//! tightly coupled to its `run_loop`, and merging them would put a new
//! feature's bugs inside the sync path. Listed as P3 debt in the design spec,
//! on purpose (docs/2026-08-10-vault-search-index-design.md §"P3 判据触发").

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

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

/// 注意力摄取的防抖窗口。比索引的 300ms 长两个量级,因为触发它的东西
/// 不一样:洞察 store 在你读文档的整个过程里持续 flush 当天的文件,而
/// 摄取是**全量重算**。60 秒意味着最坏情况下每分钟一次全量重算,而不是
/// 每次 flush 一次。
pub const ATTENTION_DEBOUNCE_SECS: u64 = 60;

/// 这条路径是一份 analytics 日文件吗。
///
/// 针眼开得很窄,而且是**白名单**:`should_forward` 那条「任何 `.` 开头
/// 的路径段一律挡掉」的规则挡的是 `.git` 的几万个对象,放宽它的代价是
/// 一次 git 操作变成一场重索引风暴。所以这里精确匹配目录前缀 + `.json`
/// 后缀 + 深度恰好两级,`.notemd` 下的其它任何东西(设置、镜像记录)都
/// 不放行 —— 包括 `.notemd/analytics-backup/`,前缀里的那个 `/` 就是拦
/// 它的。
fn is_analytics(rel: &str) -> bool {
    rel.starts_with(".notemd/analytics/")
        && rel.ends_with(".json")
        && rel.matches('/').count() == 2
}

/// 距上次摄取够久了吗。到点只是**允许**摄取,不是命令它摄取:真正干活
/// 还要 `attention_dirty` 为真,否则这就成了一个每分钟空转一次全量重算
/// 的定时任务。
fn attention_due(since_last: Duration) -> bool {
    since_last >= Duration::from_secs(ATTENTION_DEBOUNCE_SECS)
}

/// 这一轮该不该摄取 —— **两个条件与它们的求值顺序绑死在一个函数里**,因为
/// 正确性全在这两点上,而它们各有一个看起来完全合理的错法:
///
/// * `&&` 写成 `||`(设计简报的伪代码就是 `||`):窗口一到点就摄取,不管有
///   没有新数据 —— 一个每分钟做一次全量重算的空转定时任务;而且 `||` 的短路
///   会在到点那一轮**根本不读标志**,下一轮再 `swap` 出个真来,多摄一次。
/// * 顺序反过来写成 `dirty.swap(false, ..) && attention_due(..)`:返回值完全
///   正确,但窗口没开时也把标志清掉了 —— 那次 flush 就此人间蒸发,直到用户
///   下次读文档才会被重新标脏。这是**静默**的错(排序偏旧,无症状)。
///
/// 所以 `swap` 只在窗口确认打开之后才执行,复位也因此永远伴随一次真正的摄取。
/// 三条 mutation 各有一条测试守着,见 `mod tests`。
fn take_attention_turn(dirty: &AtomicBool, since_last: Duration) -> bool {
    attention_due(since_last) && dirty.swap(false, Ordering::SeqCst)
}

/// 一次摄取尝试的结局。存在的理由是让「被取代」与「索引没准备好」在类型上
/// 分得开 —— 前者是代际闸门**拦下**的,测试要能精确断言到它,而不是从一个
/// 含混的 `None` 里猜。
#[derive(Debug, PartialEq)]
enum Ingest {
    /// 代际闸门拦下:摄取期间用户切走了 vault,一个字都没往索引里写。
    Superseded,
    /// 索引还没装进 `IndexHandle`(或已被 `open_vault` 清空)。
    NotReady,
    Done(Result<usize, String>),
}

/// 摄取的代际闸门:**取锁 → 查代际 → 才写**,三步锁死在一个函数里,中间
/// 没有缝隙插得进耗时操作。返回 `Superseded` = 本 watcher 已被更新一代的
/// `open_vault` 取代,`refresh_attention` **一次都没被调用**。
///
/// 这是 `install_if_current` 那条纪律在摄取路径上的第二个执行点,形状略有
/// 不同:那边有个线程本地的 `SearchIndex` 可以整个丢掉,这边 `as_mut()` 改
/// 的是**共享的**那一个,没有东西可丢 —— 一旦写下去就是「旧 vault 的注意力
/// 进了新 vault 的索引」,而且完全静默(只是排序变怪)。所以检查必须发生在
/// 锁里、写之前那一刻。
///
/// `is_current_now` 是**闭包**而不是 `bool`,理由与 `install_if_current` 逐字
/// 相同:传值就意味着调用方可以把耗时操作**之前**的那次快照递进来 —— T9 被
/// 判 Critical 的正是这个形状。
fn refresh_attention_if_current(
    handle: &crate::search::IndexHandle,
    links: &[searchidx::attention::MirrorLink],
    is_current_now: impl FnOnce() -> bool,
) -> Ingest {
    let mut guard = crate::search::lock(handle);
    if !is_current_now() {
        return Ingest::Superseded;
    }
    let Some(idx) = guard.as_mut() else {
        return Ingest::NotReady;
    };
    Ingest::Done(idx.refresh_attention(links))
}

/// 一次事件的两路产物。它们走的是**完全不同的通道**:`index` 进
/// `Pending` 触发重索引,`attention` 只置一个标志触发摄取。合并成一个
/// 返回值仅仅因为二者来自同一次路径遍历 —— 语义上必须分开,把 analytics
/// 混进 `Pending` 等于每次洞察 flush 都重扫一遍 vault。
#[derive(Debug, Default, PartialEq)]
struct Relevant {
    index: Vec<String>,
    attention: bool,
}

/// The vault-relative paths `event` should produce `Pending::note` calls for,
/// plus whether it touched the analytics directory.
/// Extracted out of the `RecommendedWatcher` callback so it can be
/// unit-tested with synthetic `notify::Event`s — the way
/// `agents_sync::watcher::should_process` is — instead of only being
/// exercisable via real filesystem events.
fn relevant_paths(event: &Event, vault_root: &Path) -> Relevant {
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return Relevant::default();
    }
    let mut out = Relevant::default();
    for rel in event.paths.iter().filter_map(|p| searchidx::norm::rel_path(vault_root, p)) {
        if should_forward(&rel) {
            out.index.push(rel);
        } else if is_analytics(&rel) {
            out.attention = true;
        }
    }
    out
}

/// (Re)start the watcher for `vault_root`, under the generation `my_gen`
/// reserved by the caller (`open_vault`) via `reserve_generation`. Safe to
/// call again — e.g. when the user picks a different vault folder: the
/// previous watcher/thread notices a newer generation and exits instead of
/// continuing to write into the old vault's (still-live) `IndexHandle`
/// contents.
pub fn restart(app: &AppHandle, vault_root: &Path, my_gen: u64) {
    let (tx, rx) = mpsc::channel::<String>();
    let root = vault_root.to_path_buf();
    let filter_root = root.clone();
    // 摄取的独立通道。刻意**不**走 `tx`:`Pending` 是重索引队列,一条
    // analytics 事件进去就是一次全量重扫,而洞察 store 在用户读文档时是
    // 持续 flush 的。一个标志位既表达了「有新数据」,又天然把任意多次
    // flush 合并成一次摄取。
    let attention_dirty = Arc::new(AtomicBool::new(false));
    let dirty_setter = attention_dirty.clone();

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            let Ok(event) = res else { return };
            let relevant = relevant_paths(&event, &filter_root);
            if relevant.attention {
                dirty_setter.store(true, Ordering::SeqCst);
            }
            for rel in relevant.index {
                let _ = tx.send(rel);
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
        // 从「现在」起算,而不是 0:`open_vault` 刚刚摄取过一次,watcher
        // 起来的头一分钟没有必要再全量重算一遍。
        let mut last_attention = Instant::now();
        let stale = || app.state::<WatchState>().generation.load(Ordering::SeqCst) != my_gen;
        loop {
            if stale() {
                return;
            }
            match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
                Ok(rel) => pending.note(rel),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if !pending.is_empty() {
                        if stale() {
                            return;
                        }
                        drain(&app, &root, pending.take());
                    }
                    // 摄取的节奏与重索引完全独立。两个条件、以及它们的求值
                    // 顺序,都在 `take_attention_turn` 里(连同各自的错法与
                    // 守着它们的测试)。复位发生在摄取**之前**,所以摄取期间
                    // 到达的事件不会被这一轮吞掉。
                    if take_attention_turn(&attention_dirty, last_attention.elapsed()) {
                        if stale() {
                            return;
                        }
                        drain_attention(&app, &root, my_gen);
                        last_attention = Instant::now();
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

/// 把 vault 的注意力数据全量重算进当前索引。触发者是 `.notemd/analytics/`
/// 下的写入,不是文档变更 —— 所以这里既不碰 `Pending`,也不做任何扫描。
///
/// 代际闸门在 `refresh_attention_if_current` 里(取锁 → 查代际 → 才写),
/// 而不是复用调用方循环里那次 `stale()`:读 analytics(最多一年的日文件)
/// 加上 `links_for_vault`(读 `.notemd/mirrors/`)是耗时操作,用户完全来得及
/// 在这期间切走 vault。传的是**闭包**,所以求值时刻由闸门自己定,调用方递不
/// 进一份陈旧的快照。
fn drain_attention(app: &AppHandle, root: &Path, my_gen: u64) {
    let idx_handle = crate::search::handle(app);
    // 锁外做 IO,与 `drain` 一样:一次摄取不该把并发的搜索命令堵住整段读盘。
    // 也正因为这段 IO 慢,上面那道闸门才必须在它**之后**、锁**之内**求值。
    let links = crate::search::attention_links::links_for_vault(root);
    let ok = match refresh_attention_if_current(&idx_handle, &links, || is_current(app, my_gen)) {
        Ingest::Done(Ok(n)) => {
            crate::log_cat!("search", "info", "attention ingest: {n} files");
            true
        }
        // 摄取失败只降级排序(加成退化成 ×1.0),索引本身完全可用 —— 与
        // `open_vault` 里同一条判断,所以只记一行,不动索引状态。
        Ingest::Done(Err(e)) => {
            crate::log_cat!("search", "error", "attention ingest failed: {e}");
            false
        }
        Ingest::Superseded => {
            crate::log_cat!("search", "info", "attention ingest superseded, discarding");
            false
        }
        Ingest::NotReady => false,
    };
    if ok {
        // 排序输入变了,开着的搜索面板重跑一次查询才看得到新次序。
        let _ = app.emit(INDEX_UPDATED_EVENT, ());
    }
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
                    "flood sweep: {} renamed, {} indexed, {} removed, {} ms",
                    stats.files_renamed,
                    stats.files_indexed,
                    stats.files_removed,
                    stats.took_ms
                );
                crate::search::skipped_state(app).set(stats.files_skipped_large);
                true
            }
            Err(e) => {
                crate::log_cat!("search", "error", "flood sweep failed: {e}");
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
    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};
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
        assert_eq!(relevant_paths(&event, root).index, vec!["notes/a.md".to_string()]);
    }

    #[test]
    fn relevant_paths_drops_non_markdown_and_dot_dirs() {
        let root = Path::new("/vault");
        for p in ["/vault/notes/a.txt", "/vault/.git/HEAD.md", "/vault/.notemd/settings.md"] {
            let r = relevant_paths(&modify_event(p), root);
            assert_eq!(r, Relevant::default(), "{p} 不该产生任何通道的活儿");
        }
    }

    #[test]
    fn relevant_paths_ignores_access_events() {
        let root = Path::new("/vault");
        let access = Event::new(EventKind::Access(AccessKind::Any))
            .add_path(PathBuf::from("/vault/notes/a.md"));
        assert!(relevant_paths(&access, root).index.is_empty());
        // 读取事件也不该触发摄取:analytics 文件被**读**不代表它变了。
        let read = Event::new(EventKind::Access(AccessKind::Any))
            .add_path(PathBuf::from("/vault/.notemd/analytics/2026-08-13.DEV-1.json"));
        assert!(!relevant_paths(&read, root).attention);
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

    /// analytics 文件必须被认出来 —— 它们不是 `.md`,且在 `.notemd/` 下,
    /// 两条现行规则各挡它一次。
    #[test]
    fn analytics_files_are_recognized() {
        assert!(is_analytics(".notemd/analytics/2026-08-13.DEV-1.json"));
    }

    /// 针眼只对 analytics 开。`.git` 与 `.notemd` 的其余内容必须照旧挡住,
    /// 否则一次 git 操作就是一场重索引风暴。
    #[test]
    fn the_pinhole_does_not_open_up_the_rest_of_the_dot_dirs() {
        for p in [
            ".git/objects/ab/cdef",
            ".notemd/settings.json",
            ".notemd/mirrors/DEV-1.json",
            ".notemd/analytics-backup/x.json",
        ] {
            assert!(!is_analytics(p), "{p} 不该被当成 analytics");
        }
    }

    /// analytics 事件绝不能进 `Pending`:它触发的是摄取,不是重索引。
    /// 混进去等于每次洞察 flush 都重扫一遍 vault。
    #[test]
    fn analytics_events_never_reach_the_reindex_queue() {
        assert!(!should_forward(".notemd/analytics/2026-08-13.DEV-1.json"));
    }

    /// 60 秒防抖:洞察 store 是持续 flush 的,防抖不到位索引会一直在忙。
    #[test]
    fn the_attention_debounce_is_sixty_seconds() {
        assert_eq!(ATTENTION_DEBOUNCE_SECS, 60);
    }

    /// 白名单还要挡住深度不对的路径:analytics 下再开子目录,或者根本没进
    /// 目录(`.notemd/analytics.json`),都不是日文件。
    #[test]
    fn the_pinhole_is_exactly_one_directory_deep() {
        assert!(!is_analytics(".notemd/analytics/sub/2026-08-13.DEV-1.json"));
        assert!(!is_analytics(".notemd/analytics.json"));
        assert!(!is_analytics(".notemd/analytics/2026-08-13.DEV-1.json.tmp"));
    }

    /// 一次 analytics 事件走的是摄取通道:标志置起,而重索引队列拿到零条
    /// 路径。这是「独立通道」在事件层面的样子。
    #[test]
    fn an_analytics_event_takes_the_ingest_channel_only() {
        let root = Path::new("/vault");
        let r = relevant_paths(
            &modify_event("/vault/.notemd/analytics/2026-08-13.DEV-1.json"),
            root,
        );
        assert!(r.attention);
        assert!(r.index.is_empty());
    }

    /// 普通 `.md` 事件不该碰摄取通道 —— 否则每次保存都是一次全量重算。
    #[test]
    fn an_ordinary_markdown_event_does_not_touch_the_ingest_channel() {
        let root = Path::new("/vault");
        let r = relevant_paths(&modify_event("/vault/notes/a.md"), root);
        assert!(!r.attention);
        assert_eq!(r.index, vec!["notes/a.md".to_string()]);
    }

    /// 防抖窗口:59 秒不到点,60 秒到点。到点只是**允许**摄取,真干活还
    /// 要标志为真 —— 见 `take_attention_turn`。
    #[test]
    fn the_attention_window_opens_at_sixty_seconds() {
        assert!(!attention_due(Duration::from_secs(59)));
        assert!(attention_due(Duration::from_secs(60)));
        assert!(attention_due(Duration::from_secs(600)));
    }

    fn dirty(v: bool) -> AtomicBool {
        AtomicBool::new(v)
    }

    /// 正路:窗口开着且有新数据 —— 干活,并且标志被复位(否则下一个窗口会
    /// 再摄取一遍同样的数据)。
    #[test]
    fn a_dirty_flag_inside_an_open_window_takes_its_turn_and_resets() {
        let f = dirty(true);
        assert!(take_attention_turn(&f, Duration::from_secs(60)));
        assert!(!f.load(Ordering::SeqCst), "标志没复位,下一轮会白摄取一次");
    }

    /// 窗口开着但没有新事件 —— **什么都不做**。设计简报的伪代码写的是
    /// `||`,照抄就会在这里返回 true:一个每分钟做一次全量重算的空转定时
    /// 任务,vault 一天被无缘无故重算 1440 次。
    #[test]
    fn an_open_window_with_no_new_events_does_nothing() {
        let f = dirty(false);
        assert!(!take_attention_turn(&f, Duration::from_secs(600)));
    }

    /// 窗口没开时**连标志都不许碰**。这条钉的是短路顺序:写成
    /// `dirty.swap(false, ..) && attention_due(..)` 返回值仍然全对,但那次
    /// flush 的脏标记已经被吃掉了 —— 用户读过的文档要等到他下次再读才会进
    /// 排序。静默、无症状,只有这条断言看得见。
    #[test]
    fn a_closed_window_does_not_consume_the_dirty_flag() {
        let f = dirty(true);
        assert!(!take_attention_turn(&f, Duration::from_secs(59)));
        assert!(f.load(Ordering::SeqCst), "窗口没开就把标志吃了,这次 flush 丢了");
        // 窗口一开,刚才那次 flush 必须还在,照常触发摄取。
        assert!(take_attention_turn(&f, Duration::from_secs(60)));
    }

    /// 既没到点也没有新事件:两个条件都不满足,更不能干活。
    #[test]
    fn a_closed_window_with_a_clean_flag_does_nothing() {
        assert!(!take_attention_turn(&dirty(false), Duration::from_secs(1)));
    }

    /// 一个只在测试里用的索引:db 落在 `tempdir` 里(`open_at`,不是 `open`
    /// —— 后者会写进真实的 app-data 目录)。与 `search::mod` 的同名测试
    /// 辅助函数同形。
    fn scratch_handle(dir: &Path) -> crate::search::IndexHandle {
        let idx = searchidx::SearchIndex::open_at(dir, &dir.join("index.db"), "")
            .expect("open scratch index");
        std::sync::Arc::new(std::sync::Mutex::new(Some(idx)))
    }

    /// 上一轮摄取有没有真的落到这个索引上。`refresh_attention` 每次都会盖
    /// `meta.attention_as_of`,所以它比行数可靠:零结果的摄取也留痕。
    fn ingested(handle: &crate::search::IndexHandle) -> bool {
        crate::search::lock(handle)
            .as_ref()
            .expect("scratch 索引不该是空的")
            .stats()
            .expect("stats")
            .attention_as_of
            .is_some()
    }

    /// **本环最要紧的一条。** 摄取期间用户切走了 vault —— 闸门必须拦下,而
    /// 且不是「返回值好看」那种拦下:`refresh_attention` 一次都不能被调用,
    /// 索引里不许留下任何摄取痕迹。否则就是旧 vault 的注意力写进了新 vault
    /// 的索引 —— T9 在 `open_vault` 里犯过同形状的错(见 `install_if_current`)。
    #[test]
    fn attention_from_a_vault_the_user_left_is_never_written() {
        let d = tempfile::tempdir().unwrap();
        let handle = scratch_handle(d.path());
        // `false` = 读 analytics 的这段时间里,更新一代 open 已经预定了代际。
        assert_eq!(refresh_attention_if_current(&handle, &[], || false), Ingest::Superseded);
        assert!(!ingested(&handle), "被取代的线程把注意力写进了索引");
    }

    /// 同一道门的另一半:代际仍然成立时必须**真的**摄取。否则「谁也写不
    /// 进去」会是一个绿着的测试套件配上一个永远没有注意力加成的排序。
    #[test]
    fn a_still_current_generation_ingests() {
        let d = tempfile::tempdir().unwrap();
        let handle = scratch_handle(d.path());
        assert_eq!(refresh_attention_if_current(&handle, &[], || true), Ingest::Done(Ok(0)));
        assert!(ingested(&handle), "代际成立却没摄取");
    }

    /// 陈旧的**快照**没法伪装成新鲜的检查:闸门收的是闭包,求值时刻由它自己
    /// 决定,而且必须发生在拿到锁**之后** —— 窗口收到锁的粒度以内。一旦有人
    /// 把签名改回 `bool`,调用方就又能把耗时 IO 之前的快照传进来了,那正是
    /// T9 那个 bug 的形状。与 `search::mod` 里 `install_if_current` 的同名
    /// 测试同一条纪律。
    #[test]
    fn the_generation_is_read_after_the_lock_is_taken_not_before() {
        let d = tempfile::tempdir().unwrap();
        let handle = scratch_handle(d.path());
        let checked = std::sync::Arc::new(AtomicBool::new(false));
        let seen_locked = std::sync::Arc::new(AtomicBool::new(false));
        {
            let checked = checked.clone();
            let seen_locked = seen_locked.clone();
            let probe = handle.clone();
            let out = refresh_attention_if_current(&handle, &[], move || {
                checked.store(true, Ordering::SeqCst);
                // 闭包跑的时候锁必须已经在闸门手里。
                seen_locked.store(probe.try_lock().is_err(), Ordering::SeqCst);
                true
            });
            assert_eq!(out, Ingest::Done(Ok(0)));
        }
        assert!(checked.load(Ordering::SeqCst), "代际检查压根没跑");
        assert!(seen_locked.load(Ordering::SeqCst), "检查发生在取锁之前,窗口没收窄");
    }

    /// 索引还没装进 `IndexHandle`(`open_vault` 清空之后、装回之前的那个
    /// 窗口)时,摄取安静地跳过 —— 不 panic、不 emit。
    #[test]
    fn an_absent_index_is_skipped_not_panicked_on() {
        let handle: crate::search::IndexHandle = std::sync::Arc::new(std::sync::Mutex::new(None));
        assert_eq!(refresh_attention_if_current(&handle, &[], || true), Ingest::NotReady);
    }

    #[test]
    fn relevant_paths_accepts_create_and_remove() {
        let root = Path::new("/vault");
        let create =
            Event::new(EventKind::Create(CreateKind::File)).add_path(PathBuf::from("/vault/a.md"));
        assert_eq!(relevant_paths(&create, root).index, vec!["a.md".to_string()]);
        let remove = Event::new(EventKind::Remove(RemoveKind::File))
            .add_path(PathBuf::from("/vault/a.md"));
        assert_eq!(relevant_paths(&remove, root).index, vec!["a.md".to_string()]);
    }
}
