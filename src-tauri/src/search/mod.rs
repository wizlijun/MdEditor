//! The Tauri adapter: an index handle in app state, plus the file watcher.
//! Deliberately thin — every decision about scanning, tokenizing and ranking is
//! `searchidx`'s, so the GUI and the CLI cannot answer the same query
//! differently.

pub mod options;
pub mod watch;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use searchidx::{Hit, IndexStats, SearchIndex};
use serde::Serialize;
use tauri::{AppHandle, Manager};

/// Kept as an alias — rather than rewriting this module's call sites — so
/// there is exactly one place (`options::for_vault`) that ever constructs a
/// `ScanOptions`; see that module's doc comment for why that has to be true
/// across the GUI/CLI process boundary.
pub use options::for_vault as scan_options;

/// `None` until a vault is configured (or after a failed open — the index is
/// optional, the app is not).
pub type IndexHandle = Arc<Mutex<Option<SearchIndex>>>;

/// Manage app state and, if a vault is already configured (the common case:
/// app relaunch with an existing vault), open it and start watching — mirrors
/// `agents_sync::init`'s auto-start. `open_vault` is also called directly from
/// the folder picker for a freshly chosen/changed vault; see `lib.rs`.
pub fn init(app: &AppHandle) {
    app.manage::<IndexHandle>(Arc::new(Mutex::new(None)));
    app.manage(watch::WatchState::default());
    if let Some(root) = crate::sotvault::resolve_vault_root(app) {
        open_vault(app, &root);
    }
}

pub fn handle(app: &AppHandle) -> IndexHandle {
    app.state::<IndexHandle>().inner().clone()
}

/// Lock the index handle, recovering from poisoning rather than propagating a
/// panic: one thread panicking mid-reindex must not take the whole app's
/// search feature down with it (global constraint — a broken index must never
/// stop the app).
pub fn lock(handle: &IndexHandle) -> MutexGuard<'_, Option<SearchIndex>> {
    handle.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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
                if let Err(e) = idx.sweep(&opts, None) {
                    crate::log_cat!("search", "warn", "sweep failed: {e}");
                }
                // Discard this thread's work if a newer `open_vault` call has
                // superseded it — otherwise a slow open for a vault the user
                // has since switched away from could overwrite the new
                // vault's (already-current) `IndexHandle` entry.
                if !watch::is_current(&app, my_gen) {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatsDto {
    pub files: i64,
    pub blocks: i64,
    pub db_bytes: u64,
    pub built_at: Option<String>,
    pub tokenizer_id: String,
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
    for path in &stats.files_skipped_large {
        crate::log_cat!("search", "warn", "skipped (over threshold): {path}");
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

fn stats_to_dto(s: IndexStats) -> SearchStatsDto {
    SearchStatsDto {
        files: s.files,
        blocks: s.blocks,
        db_bytes: s.db_bytes,
        built_at: s.built_at,
        tokenizer_id: s.tokenizer_id,
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
    Ok(stats_to_dto(idx.stats()?))
}

/// Rebuild is a rare, explicit, user-initiated action (a button, not
/// something on the hot path), and it holds the index lock for its whole
/// duration rather than swapping the `SearchIndex` out of `IndexHandle` to
/// rebuild it lock-free. Concurrent `notemd_search`/`notemd_search_stats`
/// calls block for that window, but they get an honest "still building"
/// wait; the alternative — taking the index out from under the `Mutex` while
/// it rebuilds — would make every concurrent caller see `NOT_READY` (a
/// vault-not-configured state) for an index that in fact exists and is just
/// busy, which is a worse lie. The file watcher's flood-sweep path
/// (`watch::drain`) already holds this same lock across a full `sweep`, so
/// this is the established pattern, not a new tradeoff.
#[tauri::command]
pub fn notemd_search_rebuild(app: AppHandle) -> Result<SearchStatsDto, String> {
    let idx_handle = handle(&app);
    let mut guard = lock(&idx_handle);
    let idx = require_index_mut(&mut guard)?;
    let root = idx.vault_root().to_path_buf();
    idx.rebuild(&scan_options(&root))?;
    Ok(stats_to_dto(idx.stats()?))
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
        let dto = stats_to_dto(s);
        assert_eq!(dto.files, 3);
        assert_eq!(dto.blocks, 40);
        assert_eq!(dto.db_bytes, 12345);
        assert_eq!(dto.built_at.as_deref(), Some("2026-08-10T00:00:00Z"));
        assert_eq!(dto.tokenizer_id, "jieba/1");
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

    #[test]
    fn search_stats_dto_serializes_with_camel_case_field_names() {
        let dto = stats_to_dto(IndexStats {
            files: 1,
            blocks: 1,
            db_bytes: 1,
            built_at: None,
            tokenizer_id: "jieba/1".to_string(),
        });
        let v = serde_json::to_value(&dto).unwrap();
        for key in ["files", "blocks", "dbBytes", "builtAt", "tokenizerId"] {
            assert!(v.get(key).is_some(), "missing key {key} in {v}");
        }
    }
}
