//! The plugin runtime (spec §3-§5): discovers installed plugins, runs their
//! resident processes over NDJSON JSON-RPC, and serves their windows. It is the
//! only plugin execution path — `plugin_host` exposes what it discovers to the
//! frontend and the CLI as a manifest view model.

pub mod adapter;
pub mod commands;
pub mod discovery;
pub mod host_api;
pub mod installer;
pub mod lifecycle;
pub mod location;
pub mod market;
pub mod process;
pub mod protocol;
pub mod state;
pub mod ui_rpc;
pub mod windows;

use std::collections::BTreeMap;
use std::sync::{LazyLock, RwLock};

pub struct RuntimeState {
    /// id → (manifest, install_dir<current/>)
    /// Ordered by id — iteration order surfaces as the Plugins menu order,
    /// and a HashMap's is randomized per process (the menu would reshuffle on
    /// every launch).
    pub plugins: BTreeMap<String, (plugin_protocol::ManifestV2, std::path::PathBuf)>,
}

pub static STATE: LazyLock<RwLock<RuntimeState>> =
    LazyLock::new(|| RwLock::new(RuntimeState { plugins: BTreeMap::new() }));

/// Called once during `setup`, before anything reads the active plugin set
/// (menu building, `plugin_host::is_plugin_enabled`). Scans the install tree,
/// then activates the plugins whose activation events fire at startup.
pub fn init<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    {
        let mut st = STATE.write().unwrap();
        let host_version = app.package_info().version.to_string();
        match discovery::scan(app, &host_version) {
            Ok(map) => st.plugins = map,
            Err(e) => eprintln!("[plugin_runtime] scan failed: {e}"),
        }
        eprintln!("[plugin_runtime] {} plugin(s)", st.plugins.len());
    } // release the STATE write lock before registration re-reads it
    commands::startup_activate_all(app);
}
