//! The Tauri adapter: an index handle in app state, plus the file watcher.
//! Deliberately thin — every decision about scanning, tokenizing and ranking is
//! `searchidx`'s, so the GUI and the CLI cannot answer the same query
//! differently.

pub mod watch;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use searchidx::{Hit, IndexStats, Limits, ScanOptions, SearchIndex};
use serde::Serialize;
use tauri::{AppHandle, Manager};

/// `None` until a vault is configured (or after a failed open — the index is
/// optional, the app is not).
pub type IndexHandle = Arc<Mutex<Option<SearchIndex>>>;

/// Manage app state and, if a vault is already configured (the common case:
/// app relaunch with an existing vault), open it and start watching — mirrors
/// `agents_sync::init`'s auto-start. `open_vault` is also called directly from
/// the folder picker for a freshly chosen/changed vault; see `lib.rs`.
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

pub fn init(app: &AppHandle) {
    app.manage::<IndexHandle>(Arc::new(Mutex::new(None)));
    app.manage(SearchGen::default());
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

pub fn scan_options(vault_root: &Path) -> ScanOptions {
    let vs = crate::sotvault::vault_settings::read(vault_root);
    ScanOptions {
        large_file_threshold_mb: vs.large_file_threshold_mb.unwrap_or(10),
        exclude_dirs: vs.search_exclude_dirs.unwrap_or_default(),
    }
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
                    crate::dlog(&format!("[search] initial build failed: {e}"));
                }
                if let Err(e) = idx.sweep(&opts, None) {
                    crate::dlog(&format!("[search] sweep failed: {e}"));
                }
                // Discard this thread's work if a newer `open_vault` call has
                // superseded it — otherwise a slow open for a vault the user
                // has since switched away from could overwrite the new
                // vault's (already-current) `IndexHandle` entry.
                if !watch::is_current(&app, my_gen) {
                    crate::dlog("[search] open_vault superseded, discarding");
                    return;
                }
                *lock(&idx_handle) = Some(idx);
                watch::restart(&app, &root, my_gen);
            }
            Err(e) => crate::dlog(&format!("[search] index unavailable: {e}")),
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
    /// The retrieval hit its time budget. `hits` is a partial answer.
    pub truncated: bool,
    /// FTS missed and the (expensive) scan fallback was not run because this
    /// was a shallow query — the panel offers it instead of paying for it.
    pub deep_available: bool,
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

fn stats_to_dto(s: IndexStats) -> SearchStatsDto {
    SearchStatsDto {
        files: s.files,
        blocks: s.blocks,
        db_bytes: s.db_bytes,
        built_at: s.built_at,
        tokenizer_id: s.tokenizer_id,
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
    let guard = lock(&idx_handle);
    // Waiting for that lock is exactly when a query goes stale, so this is
    // the check that matters most: whatever we queued behind, the user has
    // typed since.
    if superseded(&counter, ticket) {
        return Err(CANCELLED.to_string());
    }
    let idx = require_index(&guard)?;

    let deadline = timeout_ms.map(|ms| started + Duration::from_millis(ms));
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
    let answer = idx.search_with(&query, limit.unwrap_or(50), &limits)?;
    // An abort has two causes and they are not the same answer: superseded
    // means "throw this away", deadline means "partial, and say so".
    if superseded(&counter, ticket) {
        return Err(CANCELLED.to_string());
    }
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

#[tauri::command(async)]
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
#[tauri::command(async)]
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
