//! Plugin runtime v2 (spec §3-§5). Coexists with the v1 one-shot host until
//! all first-batch plugins migrate (子项目④). Everything is gated behind
//! `plugins_v2.enabled` in settings.json or NOTEMD_PLUGINS_V2=1.

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
use std::path::Path;
use std::sync::{LazyLock, RwLock};

pub struct RuntimeState {
    pub enabled_flag: bool,
    /// id → (manifest, install_dir<current/>)
    /// Ordered by id — iteration order surfaces as the Plugins menu order,
    /// and a HashMap's is randomized per process (the menu would reshuffle on
    /// every launch).
    pub plugins: BTreeMap<String, (plugin_protocol::ManifestV2, std::path::PathBuf)>,
}

pub static STATE: LazyLock<RwLock<RuntimeState>> =
    LazyLock::new(|| RwLock::new(RuntimeState { enabled_flag: false, plugins: BTreeMap::new() }));

/// Pure(ish) flag check against an explicit config dir: env var override first,
/// then `settings.json` in `config_dir`. The CLI (no AppHandle) calls this
/// directly with `cli::resolve_config_dir()`; the AppHandle version wraps it.
pub fn v2_flag_enabled_at(config_dir: &Path) -> bool {
    // Env override wins (dev/CI): "1" forces on, "0" forces off.
    match std::env::var("NOTEMD_PLUGINS_V2").as_deref() {
        Ok("1") => return true,
        Ok("0") => return false,
        _ => {}
    }
    // Default ON (since 6.718.2 — plugin system v2 shipped live). Only an
    // explicit `"plugins_v2.enabled": false` in settings.json opts out; a
    // missing file / absent key / unparseable JSON all mean on.
    // 读法仿 read_saved_locale（lib.rs）
    let Ok(text) = std::fs::read_to_string(config_dir.join("settings.json")) else { return true };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return true };
    json.get("plugins_v2.enabled").and_then(|v| v.as_bool()).unwrap_or(true)
}

pub fn v2_flag_enabled<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    // On resolution failure fall through with an empty path: the settings read
    // fails closed while the env-var override still applies.
    let dir = tauri::Manager::path(app).app_config_dir().unwrap_or_default();
    v2_flag_enabled_at(&dir)
}

/// setup 阶段调用（plugin_host::init 之后）。flag 关 ⇒ 空 STATE，零成本。
pub fn init<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let flag = v2_flag_enabled(app);
    {
        let mut st = STATE.write().unwrap();
        st.enabled_flag = flag;
        if !flag {
            return;
        }
        let host_version = app.package_info().version.to_string();
        match discovery::scan(app, &host_version) {
            Ok(map) => st.plugins = map,
            Err(e) => eprintln!("[plugin_runtime] scan failed: {e}"),
        }
        eprintln!("[plugin_runtime] v2 enabled, {} plugin(s)", st.plugins.len());
    } // release the STATE write lock before registration re-reads it
    commands::startup_activate_all(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: none of these tests set NOTEMD_PLUGINS_V2 — mutating the process
    // environment would race with parallel tests. The env override is a plain
    // `== "1"` check exercised implicitly (unset ⇒ falls through to settings).

    #[test]
    fn flag_reads_settings_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), r#"{ "plugins_v2.enabled": true }"#)
            .unwrap();
        assert!(v2_flag_enabled_at(dir.path()));
    }

    #[test]
    fn flag_default_on_unless_explicitly_false() {
        // Skip if the ambient env forces a value (would mask the settings path).
        if std::env::var("NOTEMD_PLUGINS_V2").is_ok() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        // No settings.json at all → default ON.
        assert!(v2_flag_enabled_at(dir.path()));
        // Key absent → default ON.
        std::fs::write(dir.path().join("settings.json"), r#"{ "locale": "en" }"#).unwrap();
        assert!(v2_flag_enabled_at(dir.path()));
        // Explicit false → opt out.
        std::fs::write(dir.path().join("settings.json"), r#"{ "plugins_v2.enabled": false }"#)
            .unwrap();
        assert!(!v2_flag_enabled_at(dir.path()));
    }

    #[test]
    fn flag_default_on_when_settings_malformed() {
        // Default ON means a corrupt settings.json can't silently disable v2;
        // only an explicit `false` opts out.
        if std::env::var("NOTEMD_PLUGINS_V2").is_ok() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), "{ not json").unwrap();
        assert!(v2_flag_enabled_at(dir.path()));
    }
}
