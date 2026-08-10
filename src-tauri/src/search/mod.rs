//! The Tauri adapter: an index handle in app state, plus the file watcher.
//! Deliberately thin — every decision about scanning, tokenizing and ranking is
//! `searchidx`'s, so the GUI and the CLI cannot answer the same query
//! differently.

pub mod watch;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use searchidx::{ScanOptions, SearchIndex};
use tauri::{AppHandle, Manager};

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
pub fn open_vault(app: &AppHandle, vault_root: &Path) {
    let idx_handle = handle(app);
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
                *lock(&idx_handle) = Some(idx);
                watch::restart(&app, &root);
            }
            Err(e) => crate::dlog(&format!("[search] index unavailable: {e}")),
        }
    });
}
