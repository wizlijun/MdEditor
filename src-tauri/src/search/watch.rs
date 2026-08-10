//! notify wiring. All policy — debounce, flood degradation — lives in
//! `searchidx::watch`; this file only turns OS events into `Pending::note`
//! calls and drives the drain loop.
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

/// Bumped every time `restart` runs. The drain-loop thread checks this on
/// every wakeup and exits the instant it sees a newer generation — otherwise a
/// watcher started for a vault the user has since switched away from would
/// keep reindexing files into (or out of) the *new* vault's index. See
/// `agents_sync::AgentsSyncState` for the same pattern.
#[derive(Default)]
pub struct WatchState {
    generation: AtomicU64,
}

/// (Re)start the watcher for `vault_root`. Safe to call again — e.g. when the
/// user picks a different vault folder: the previous watcher/thread notices
/// the generation bump and exits instead of continuing to write into the old
/// vault's (still-live) `IndexHandle` contents.
pub fn restart(app: &AppHandle, vault_root: &Path) {
    let state = app.state::<WatchState>();
    let my_gen = state.generation.fetch_add(1, Ordering::SeqCst) + 1;

    let (tx, rx) = mpsc::channel::<String>();
    let root = vault_root.to_path_buf();
    let filter_root = root.clone();

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            let Ok(event) = res else { return };
            if !matches!(
                event.kind,
                EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
            ) {
                return;
            }
            for p in &event.paths {
                if let Some(rel) = searchidx::norm::rel_path(&filter_root, p) {
                    // `.git`, `.notemd`, etc.: a git operation must not
                    // trigger a reindex storm. `.md`-only keeps the channel
                    // free of every other file type notify reports.
                    if rel.ends_with(".md") && !rel.split('/').any(|s| s.starts_with('.')) {
                        let _ = tx.send(rel);
                    }
                }
            }
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            crate::dlog(&format!("[search] watcher unavailable: {e}"));
            return;
        }
    };
    if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
        crate::dlog(&format!("[search] cannot watch {}: {e}", root.display()));
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
                Ok(rel) => pending.note(rel),
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
        Batch::Files(paths) => {
            let mut ok = true;
            for rel in paths {
                match idx.index_one(&rel, &opts) {
                    Ok(outcome) => log_outcome(&rel, outcome),
                    Err(e) => {
                        crate::dlog(&format!("[search] reindex {rel} failed: {e}"));
                        ok = false;
                    }
                }
            }
            ok
        }
        Batch::FullSweep => match idx.sweep(&opts, None) {
            Ok(_) => true,
            Err(e) => {
                crate::dlog(&format!("[search] flood sweep failed: {e}"));
                false
            }
        },
    };
    drop(guard);
    if ok {
        // Lets an open search panel refresh without polling.
        let _ = app.emit("search://index-updated", ());
    }
}

/// Log *why* a file left the index (or that it was, in fact, indexed) rather
/// than collapsing every outcome into a bare bool — distinguishing "gone",
/// "oversized" and "excluded" matters for someone reading `/tmp/mdeditor.log`
/// after the fact.
fn log_outcome(rel: &str, outcome: IndexOutcome) {
    match outcome {
        IndexOutcome::Indexed => {}
        IndexOutcome::RemovedMissing => {
            crate::dlog(&format!("[search] {rel} left the index (file gone)"))
        }
        IndexOutcome::RemovedOversized => {
            crate::dlog(&format!("[search] {rel} left the index (now oversized)"))
        }
        IndexOutcome::RemovedNotIndexable => {
            crate::dlog(&format!("[search] {rel} left the index (excluded/not indexable)"))
        }
    }
}
