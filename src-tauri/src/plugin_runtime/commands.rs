//! Tauri commands exposing the v2 runtime to the frontend (plan Task 8).
//!
//! Lifecycles register lazily in [`RUNNING`]: the first trigger builds a
//! [`SpawnCtx`] from the AppHandle once; from then on the lifecycle machine
//! owns (re)spawning without any tauri types (crash restarts included).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::Manager;

use super::lifecycle::{self, PluginLifecycle, SpawnCtx, Trigger, RUNNING};
use super::{discovery, host_api, installer, market, state, STATE};

/// Execute `command` on a v2 plugin: lazy activation (spec §4.2) followed by
/// `command.execute`. `context` is the same shape v1 plugins receive.
#[tauri::command]
pub async fn plugin_v2_execute(
    app: tauri::AppHandle,
    plugin_id: String,
    command: String,
    context: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let lc = get_or_register(&app, &plugin_id)?;
    lc.ensure_active(&Trigger::Command(command.clone())).await?;
    lc.execute(plugin_protocol::ExecuteCommandParams { command, context })
        .await
}

/// CLI counterpart of [`plugin_v2_execute`]. Activation must use the
/// manifest's `onCli:<subcommand>` event rather than the GUI command event.
#[tauri::command]
pub async fn plugin_v2_execute_cli(
    app: tauri::AppHandle,
    plugin_id: String,
    subcommand: String,
    command: String,
    context: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let lc = get_or_register(&app, &plugin_id)?;
    lc.ensure_active(&Trigger::Cli(subcommand)).await?;
    lc.execute(plugin_protocol::ExecuteCommandParams { command, context })
        .await
}

/// Open a plugin-contributed window (spec §7.2). The frontend routes a menu
/// command to this instead of `plugin_v2_execute` when the command matches a
/// window's `open_command` (see `open_windows` in the adapted manifest).
#[tauri::command]
pub fn plugin_v2_open_window(
    app: tauri::AppHandle,
    plugin_id: String,
    window_id: String,
    seed: Option<serde_json::Value>,
) -> Result<(), String> {
    let existed = app
        .get_webview_window(&super::windows::window_label(&plugin_id, &window_id))
        .is_some();
    let seed_json = seed.as_ref().map(|s| s.to_string());
    // 新建窗口经 URL query 携带(eval 推送在 webview 加载前会落空);
    // 已开窗口 query 到不了(singleton 分支 focus 后直接返回),改走 push。
    super::windows::open_plugin_window(
        &app,
        &plugin_id,
        &window_id,
        if existed { None } else { seed_json.as_deref() },
    )?;
    if existed {
        if let Some(s) = seed {
            super::windows::push_to_window(
                &app,
                &plugin_id,
                &window_id,
                &serde_json::json!({ "type": "seed", "payload": s }),
            );
        }
    }
    Ok(())
}

/// `(plugin_id, window_id)` of every window contributed by a plugin that holds
/// the `editor.kit` capability — the push list for a theme change. Pure over
/// the STATE map so the filter is unit-testable without an AppHandle.
fn theme_push_targets(
    plugins: &std::collections::BTreeMap<String, (plugin_protocol::ManifestV2, PathBuf)>,
) -> Vec<(String, String)> {
    plugins
        .iter()
        .filter(|(_, (m, _))| m.capabilities.iter().any(|c| c == "editor.kit"))
        .flat_map(|(pid, (m, _))| {
            m.contributes
                .windows
                .iter()
                .map(move |w| (pid.clone(), w.id.clone()))
        })
        .collect()
}

/// Notify every OPEN plugin window that holds `editor.kit` that the user's
/// theme changed. The push carries the bundle built from the main window's
/// in-memory theme ids, so an async settings save cannot make the plugin fetch
/// the previous values from disk.
///
/// Push (not poll) because a plugin webview is isolated: it sees neither the
/// main window's <style> slots nor its Tauri events. `push_to_window` is a
/// no-op for a window that isn't open, so iterating the manifest's contributed
/// windows is safe — the "already open" filter is implicit.
#[tauri::command]
pub fn plugin_v2_theme_changed(
    app: tauri::AppHandle,
    light_id: String,
    dark_id: String,
    follow_system: bool,
) {
    let targets = match STATE.read() {
        Ok(st) => theme_push_targets(&st.plugins),
        Err(_) => return,
    };
    let bundle = crate::themes::commands::theme_css_bundle_for_settings(
        &app,
        &light_id,
        &dark_id,
        follow_system,
    );
    let payload = theme_changed_payload(bundle);
    for (pid, wid) in targets {
        super::windows::push_to_window(&app, &pid, &wid, &payload);
    }
}

fn theme_changed_payload(theme: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "type": "theme-changed", "theme": theme })
}

// ── Marketplace commands (子项目③ Task 2) ────────────────────────────────
//
// The frontend market window (Task 6) drives these; the capability-consent
// modal calls `plugin_market_preview` first so the user consents to the
// *actually verified* package's capabilities before `plugin_market_install`.

/// A plugin has a backing process (and thus a lifecycle) iff it declares a
/// `binary`. UI-only plugins (decision-log, weekly-review, base, custom-editor
/// fixtures) have an empty binary map — their window opens directly from STATE
/// and their UI talks to the host over `host.*`, so they never get a
/// process/lifecycle.
///
/// The corollary that bit roam-import once it grew a backend (2026-08-03): a
/// manifest declaring `binary` MUST be packaged per triple, because a
/// `universal` package carries no `bin/` and this function would then hand it a
/// lifecycle it cannot serve. `zip_pkg` in scripts/release-plugins.sh refuses
/// that combination.
fn is_process_plugin(m: &plugin_protocol::ManifestV2) -> bool {
    !m.binary.is_empty()
}

/// Find `id`@`version` in the registry index and resolve this host's arch
/// download URL + sha256. UI-only plugins (decision-log etc.) publish under the
/// `universal` key rather than a host triple, so we prefer the host triple then
/// fall back to `universal`. Errors only if neither is present.
fn resolve_download(entry: &market::RegistryEntry) -> Result<(String, String), String> {
    let triple = discovery::current_arch_triple()
        .ok_or_else(|| format!("unsupported host arch '{}'", std::env::consts::ARCH))?;
    let url = entry
        .download
        .get(triple)
        .or_else(|| entry.download.get("universal"))
        .ok_or_else(|| format!("plugin '{}' has no download for arch '{triple}'", entry.id))?;
    let sha = entry
        .sha256
        .get(triple)
        .or_else(|| entry.sha256.get("universal"))
        .ok_or_else(|| format!("plugin '{}' has no sha256 for arch '{triple}'", entry.id))?;
    Ok((url.clone(), sha.clone()))
}

/// The package + signature `plugin_market_preview` last verified, keyed by
/// `id@version`. `plugin_market_install` reuses it instead of pulling the same
/// multi-megabyte package a second time seconds later — the user consented to
/// exactly these bytes, and install re-runs the FULL verify pipeline on them
/// regardless, so reuse changes nothing about what is trusted. Holds one entry:
/// a new preview replaces it, and installing clears it.
static PREVIEWED_PKG: std::sync::Mutex<Option<(String, Vec<u8>, String)>> =
    std::sync::Mutex::new(None);

fn take_previewed(key: &str) -> Option<(Vec<u8>, String)> {
    let mut g = PREVIEWED_PKG.lock().ok()?;
    match g.as_ref() {
        Some((k, _, _)) if k == key => g.take().map(|(_, pkg, sig)| (pkg, sig)),
        _ => None,
    }
}

/// Emit one `plugin-download-progress` event. `phase` distinguishes the
/// consent-time verification download from the install one; the frontend shows
/// a bar for both so a slow transfer never looks like a hang.
fn emit_progress(app: &tauri::AppHandle, id: &str, phase: &str, received: u64, total: Option<u64>) {
    use tauri::Emitter;
    let _ = app.emit(
        "plugin-download-progress",
        serde_json::json!({ "id": id, "phase": phase, "received": received, "total": total }),
    );
}

/// Download `url`, streaming progress to the frontend as it goes.
async fn download_with_progress(
    app: &tauri::AppHandle,
    id: &str,
    phase: &str,
    url: &str,
) -> Result<Vec<u8>, String> {
    let app = app.clone();
    let id = id.to_string();
    let phase = phase.to_string();
    market::download_reporting(url, move |received, total| {
        emit_progress(&app, &id, &phase, received, total);
    })
    .await
}

async fn find_entry(
    app: &tauri::AppHandle,
    id: &str,
    version: &str,
) -> Result<market::RegistryEntry, String> {
    let base = market::registry_base_url(app);
    let index = market::fetch_index(&base).await?;
    index
        .plugins
        .into_iter()
        .find(|e| e.id == id && e.version == version)
        .ok_or_else(|| format!("plugin '{id}' version '{version}' not found in registry"))
}

/// Fetch + return the full registry index as JSON (the "available" list).
#[tauri::command]
pub async fn plugin_market_index(
    app: tauri::AppHandle,
    force_refresh: Option<bool>,
) -> Result<serde_json::Value, String> {
    let base = market::registry_base_url(&app);
    let index = if force_refresh.unwrap_or(false) {
        market::fetch_index_fresh(&base).await?
    } else {
        market::fetch_index(&base).await?
    };
    serde_json::to_value(index).map_err(|e| e.to_string())
}

/// Download + verify `id`@`version` into a throwaway temp dir and return the
/// *validated* manifest as JSON — WITHOUT installing. The consent UI shows this
/// manifest's `capabilities`, so what the user consents to is exactly what
/// passed signature + hash verification (spec §8.2 / ②评审 V1).
#[tauri::command]
pub async fn plugin_market_preview(
    app: tauri::AppHandle,
    id: String,
    version: String,
) -> Result<serde_json::Value, String> {
    let entry = find_entry(&app, &id, &version).await?;
    let (url, sha) = resolve_download(&entry)?;
    let sig_url = format!("{url}.minisig");
    let host_version = app.package_info().version.to_string();

    let pkg = download_with_progress(&app, &id, "preview", &url).await?;
    let sig = String::from_utf8(download_with_progress(&app, &id, "preview", &sig_url).await?)
        .map_err(|e| format!("signature is not valid utf-8: {e}"))?;

    // Stage into a temp dir purely to run the full verify pipeline; discard it.
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let manifest = installer::verify_and_stage(
        &pkg,
        &sig,
        &sha,
        market::PLUGIN_REGISTRY_PUBKEY,
        &id,
        &host_version,
        tmp.path(),
    )
    .map_err(|e| e.to_string())?;
    // Hand the verified bytes to the install that usually follows, so the user
    // doesn't wait through a second download of the same package.
    if let Ok(mut g) = PREVIEWED_PKG.lock() {
        *g = Some((format!("{id}@{version}"), pkg, sig));
    }
    // tmp drops here — the staged copy is thrown away; only the manifest survives.
    serde_json::to_value(manifest).map_err(|e| e.to_string())
}

/// Download → verify → commit-install `id`@`version`, enable it in state.json,
/// then reconcile the live runtime (no restart) and tell the frontend to
/// re-fetch manifests + rebuild its menu via the `plugins-changed` event.
/// Install telemetry is fire-and-forget.
#[tauri::command]
pub async fn plugin_market_install(
    app: tauri::AppHandle,
    id: String,
    version: String,
) -> Result<(), String> {
    let entry = find_entry(&app, &id, &version).await?;
    let (url, sha) = resolve_download(&entry)?;
    let sig_url = format!("{url}.minisig");
    let host_version = app.package_info().version.to_string();
    let root = state::plugins_root(&app).ok_or("cannot resolve app data dir")?;

    let (pkg, sig) = match take_previewed(&format!("{id}@{version}")) {
        Some(cached) => cached,
        None => {
            let pkg = download_with_progress(&app, &id, "install", &url).await?;
            let sig =
                String::from_utf8(download_with_progress(&app, &id, "install", &sig_url).await?)
                    .map_err(|e| format!("signature is not valid utf-8: {e}"))?;
            (pkg, sig)
        }
    };

    // Verify + stage into a temp dir, then atomically commit into the tree.
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    installer::verify_and_stage(
        &pkg,
        &sig,
        &sha,
        market::PLUGIN_REGISTRY_PUBKEY,
        &id,
        &host_version,
        tmp.path(),
    )
    .map_err(|e| e.to_string())?;
    // Fence + destroy the old UI before `current` is repointed. Doing this only
    // in the post-install reconcile leaves a gap where old JavaScript can issue
    // an RPC while the install symlink already names the new package.
    let window_replacement = prepare_plugin_window_replacement(&app, &id).await?;
    installer::commit_install(&root, &id, &version, tmp.path()).map_err(|e| e.to_string())?;

    // Record installed + enabled in state.json.
    let mut install = state::load(&root);
    install.installed.insert(
        id.clone(),
        state::InstalledPlugin {
            version: version.clone(),
            enabled: true,
        },
    );
    state::save(&root, &install)?;

    // Fire-and-forget install telemetry (never blocks / errors the install).
    let base = market::registry_base_url(&app);
    let (rid, rver) = (id.clone(), version.clone());
    tauri::async_runtime::spawn(async move {
        market::report_install(&base, &rid, &rver).await;
    });

    // Bring the live runtime in line with the new tree, rebuild the native menu
    // (a brand-new plugin's menu item now appears without a restart), then nudge
    // the UI.
    lifecycle::reconcile_pre_fenced(&app).await?;
    drop(window_replacement);
    crate::reconcile_global_shortcuts(&app);
    crate::rebuild_menu(&app);
    notify_plugins_changed(&app);
    Ok(())
}

/// Uninstall `id` (optionally keeping its data dir), drop it from state.json,
/// reconcile the runtime (deactivating it live), and notify the frontend.
#[tauri::command]
pub async fn plugin_market_uninstall(
    app: tauri::AppHandle,
    id: String,
    keep_data: bool,
) -> Result<(), String> {
    let root = state::plugins_root(&app).ok_or("cannot resolve app data dir")?;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("cannot resolve app data dir: {e}"))?;

    let window_replacement = prepare_plugin_window_replacement(&app, &id).await?;
    installer::uninstall(&root, &id, keep_data, &app_data).map_err(|e| e.to_string())?;

    let mut install = state::load(&root);
    install.installed.remove(&id);
    state::save(&root, &install)?;

    lifecycle::reconcile_pre_fenced(&app).await?;
    drop(window_replacement);
    crate::reconcile_global_shortcuts(&app);
    crate::rebuild_menu(&app);
    notify_plugins_changed(&app);
    Ok(())
}

/// Flip `id`'s `enabled` flag in state.json, reconcile (disabling deactivates
/// it live; enabling lets the next trigger activate lazily), and notify.
#[tauri::command]
pub async fn plugin_market_set_enabled(
    app: tauri::AppHandle,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let root = state::plugins_root(&app).ok_or("cannot resolve app data dir")?;

    let mut install = state::load(&root);
    match install.installed.get_mut(&id) {
        Some(p) => p.enabled = enabled,
        None => return Err(format!("plugin '{id}' is not installed")),
    }
    let window_replacement = prepare_plugin_window_replacement(&app, &id).await?;
    state::save(&root, &install)?;

    lifecycle::reconcile_pre_fenced(&app).await?;
    drop(window_replacement);
    crate::reconcile_global_shortcuts(&app);
    crate::rebuild_menu(&app);
    notify_plugins_changed(&app);
    Ok(())
}

/// Fence one marketplace target and remove every webview contributed by its
/// currently active manifest before any on-disk mutation. The returned guard
/// must live through [`lifecycle::reconcile_pre_fenced`].
async fn prepare_plugin_window_replacement(
    app: &tauri::AppHandle,
    plugin_id: &str,
) -> Result<super::windows::PluginWindowReplacement, String> {
    let old_plugins = super::STATE
        .read()
        .map_err(|_| "plugin state lock poisoned".to_string())?
        .plugins
        .clone();
    let plugin_ids = vec![plugin_id.to_string()];
    let replacement = super::windows::begin_plugin_window_replacement(&plugin_ids)?;
    super::windows::destroy_replaced_plugin_windows(app, &old_plugins, &plugin_ids).await?;
    Ok(replacement)
}

/// List installed plugins from state.json joined with each
/// `<root>/<id>/current/manifest.json`: `{id, version, enabled, name,
/// description, i18n, capabilities}`. A plugin whose manifest is unreadable is
/// still listed (with null metadata/empty capabilities) so the UI can offer to
/// uninstall it.
#[tauri::command]
pub fn plugin_market_installed(app: tauri::AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let root = state::plugins_root(&app).ok_or("cannot resolve app data dir")?;
    let install = state::load(&root);

    let mut out = Vec::with_capacity(install.installed.len());
    for (id, entry) in &install.installed {
        let manifest_path = root.join(id).join("current").join("manifest.json");
        let manifest: Option<plugin_protocol::ManifestV2> = std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());
        let (name, description, i18n, capabilities, category) = match &manifest {
            Some(m) => (
                serde_json::Value::String(m.name.clone()),
                serde_json::to_value(&m.description).unwrap_or(serde_json::Value::Null),
                serde_json::to_value(&m.i18n).unwrap_or(serde_json::Value::Null),
                serde_json::to_value(&m.capabilities).unwrap_or(serde_json::Value::Array(vec![])),
                serde_json::Value::String(
                    crate::plugin_host::plugin_menu_group_for_plugin(
                        id,
                        m.contributes
                            .menus
                            .iter()
                            .find_map(|menu| menu.get("submenu").and_then(|value| value.as_str())),
                    )
                    .to_string(),
                ),
            ),
            None => (
                serde_json::Value::Null,
                serde_json::Value::Null,
                serde_json::Value::Null,
                serde_json::Value::Array(vec![]),
                serde_json::Value::String("other".to_string()),
            ),
        };
        out.push(serde_json::json!({
            "id": id,
            "version": entry.version,
            "enabled": entry.enabled,
            "name": name,
            "description": description,
            "i18n": i18n,
            "category": category,
            "capabilities": capabilities,
        }));
    }
    Ok(out)
}

/// Tell the frontend the installed-plugin set changed: it re-fetches
/// `get_plugin_manifests` and reapplies its own in-webview plugin menu. The
/// *native* menu (macOS menu bar) is rebuilt separately by `crate::rebuild_menu`
/// right before this fires, so a brand-new plugin's native menu item appears
/// without a restart.
fn notify_plugins_changed<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Emitter;
    let _ = app.emit("plugins-changed", ());
}

/// Called from `plugin_runtime::init` after discovery populated STATE:
/// register a lifecycle for every discovered plugin, then eagerly activate
/// the ones whose events match `Startup` (spec §4.3). The activation itself
/// is pushed onto the tauri async runtime — `init` runs inside `setup`,
/// outside any tokio context, and `startup_activation` needs `tokio::spawn`.
pub fn startup_activate_all<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    // Only process-backed plugins (those declaring a `binary`) have a lifecycle.
    // UI-only plugins (roam-import, base, etc.) have no process to activate —
    // their window opens directly from STATE without a lifecycle, and their UI
    // reaches the host through `host.*` bridge calls, not the process channel.
    // Registering them would spawn nothing and just log a spurious
    // "no binary for host arch" error every startup, so skip them here.
    let ids: Vec<String> = {
        let st = STATE.read().unwrap();
        st.plugins
            .iter()
            .filter(|(_, (m, _))| is_process_plugin(m))
            .map(|(id, _)| id.clone())
            .collect()
    };
    let mut lifecycles = Vec::new();
    for id in ids {
        match get_or_register(app, &id) {
            Ok(lc) => lifecycles.push(lc),
            Err(e) => eprintln!("[plugin_runtime] cannot register '{id}': {e}"),
        }
    }
    if lifecycles.is_empty() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        lifecycle::startup_activation(lifecycles);
    });
}

/// Every installed v2 manifest. Used by the agent slot to work out which
/// plugins can serve it (`agent_provider::providers`).
pub fn installed_manifests() -> Vec<plugin_protocol::ManifestV2> {
    STATE
        .read()
        .unwrap()
        .plugins
        .values()
        .map(|(m, _)| m.clone())
        .collect()
}

/// STATE lookup half of registration — AppHandle-free so it is unit-testable.
fn lookup_v2(plugin_id: &str) -> Result<(plugin_protocol::ManifestV2, PathBuf), String> {
    STATE
        .read()
        .unwrap()
        .plugins
        .get(plugin_id)
        .cloned()
        .ok_or_else(|| format!("unknown v2 plugin: {plugin_id}"))
}

/// RUNNING registration half — idempotent: on a lost race the entry that got
/// in first wins and the freshly built (never-spawned) one is dropped.
fn register_lifecycle(plugin_id: &str, lc: Arc<PluginLifecycle>) -> Arc<PluginLifecycle> {
    RUNNING
        .write()
        .unwrap()
        .entry(plugin_id.to_string())
        .or_insert(lc)
        .clone()
}

fn unregister_lifecycle_if_same(plugin_id: &str, lifecycle: &Arc<PluginLifecycle>) {
    let mut running = RUNNING.write().unwrap();
    if running
        .get(plugin_id)
        .is_some_and(|current| Arc::ptr_eq(current, lifecycle))
    {
        running.remove(plugin_id);
    }
}

fn retire_lifecycle(plugin_id: &str, lifecycle: Arc<PluginLifecycle>) {
    lifecycle.retire();
    let id = plugin_id.to_string();
    tauri::async_runtime::spawn(async move {
        lifecycle.deactivate().await;
        unregister_lifecycle_if_same(&id, &lifecycle);
    });
}

/// Look up the live lifecycle for `plugin_id`, registering a fresh one from
/// STATE on first use. `pub(crate)` so `ui_rpc::forward_to_plugin` can reuse the
/// exact same registration path a menu command uses (子项目②b).
pub(crate) fn get_or_register<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    plugin_id: &str,
) -> Result<Arc<PluginLifecycle>, String> {
    let (manifest, install_dir) = lookup_v2(plugin_id)?;
    if !is_process_plugin(&manifest) {
        // UI-only plugin: no process. Callers that reach here (e.g. a window UI
        // forwarding a `plugin.*` call) are misusing the channel — UI-only
        // plugins talk to the host via `host.*`, and their window opens without
        // a lifecycle. Fail with a clear message, not "no binary for host arch".
        return Err(format!(
            "plugin '{plugin_id}' is UI-only (no process); it has no command/ui.request channel"
        ));
    }
    if let Some(lc) = RUNNING.read().unwrap().get(plugin_id).cloned() {
        if !lc.is_retired() && lc.matches_install(&manifest, &install_dir) {
            return Ok(lc);
        }
        // Reconcile fences the superseded instance before its async shutdown.
        // Do not hand that old Arc to a window request or let it win a fresh
        // registration race; its fenced shutdown removes it, then a retry
        // registers STATE's current package normally.
        retire_lifecycle(plugin_id, lc);
        return Err(format!(
            "plugin '{plugin_id}' runtime is being replaced; retry the request"
        ));
    }
    let ctx = build_spawn_ctx(app, &manifest, &install_dir)?;
    let lc = Arc::new(PluginLifecycle::new(
        manifest.clone(),
        install_dir.clone(),
        ctx,
    ));
    let registered = register_lifecycle(plugin_id, lc);
    // STATE may have pivoted after the first lookup but before registration.
    // Re-read it so a lifecycle built from the previous package cannot slip in
    // after reconcile's final RUNNING snapshot.
    let still_current = match lookup_v2(plugin_id) {
        Ok((current_manifest, current_dir)) => {
            registered.matches_install(&current_manifest, &current_dir)
        }
        Err(_) => false,
    };
    if registered.is_retired() || !still_current {
        retire_lifecycle(plugin_id, registered);
        return Err(format!(
            "plugin '{plugin_id}' runtime is being replaced; retry the request"
        ));
    }
    Ok(registered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_manifest(id: &str) -> plugin_protocol::ManifestV2 {
        serde_json::from_value(serde_json::json!({
            "manifest_version": 2, "id": id, "name": "Fixture", "version": "1.0.0",
            "kind": "native", "engines": { "notemd": ">=0.0.0" },
            "activation": { "events": ["onCommand:noop"] }, "capabilities": []
        }))
        .unwrap()
    }

    fn noop_spawn_ctx() -> SpawnCtx {
        SpawnCtx {
            binary: PathBuf::from("/nonexistent/fixture-bin"),
            log_dir: std::env::temp_dir(),
            host_sink: std::sync::Arc::new(|_req| None),
            host_version: "0.0.0".into(),
            locale: "en".into(),
            app_data: std::env::temp_dir(),
        }
    }

    /// The real STATE-lookup half of get_or_register: unknown id → Err;
    /// present id → the stored (manifest, install_dir) pair.
    #[test]
    fn lookup_v2_err_on_unknown_and_ok_on_present() {
        let _registry_guard = lifecycle::REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let missing = lookup_v2("bogus.unknown-plugin");
        assert_eq!(
            missing.unwrap_err(),
            "unknown v2 plugin: bogus.unknown-plugin"
        );

        // Unique id + cleanup keeps the global STATE mutation race-free
        // against other tests (none of which use this id).
        let id = "test.commands-lookup-fixture";
        STATE.write().unwrap().plugins.insert(
            id.to_string(),
            (fixture_manifest(id), PathBuf::from("/tmp/fixture-install")),
        );
        let found = lookup_v2(id).unwrap();
        assert_eq!(found.0.id, id);
        assert_eq!(found.1, PathBuf::from("/tmp/fixture-install"));
        STATE.write().unwrap().plugins.remove(id);
    }

    /// The real RUNNING-registration half: double registration returns the
    /// first Arc (idempotent; the loser is dropped without spawning).
    #[test]
    fn register_lifecycle_is_idempotent() {
        let _registry_guard = lifecycle::REGISTRY_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let id = "test.commands-register-fixture";
        let first = Arc::new(PluginLifecycle::new(
            fixture_manifest(id),
            PathBuf::from("/tmp/a"),
            noop_spawn_ctx(),
        ));
        let second = Arc::new(PluginLifecycle::new(
            fixture_manifest(id),
            PathBuf::from("/tmp/b"),
            noop_spawn_ctx(),
        ));
        let won_first = register_lifecycle(id, first.clone());
        let won_second = register_lifecycle(id, second);
        assert!(Arc::ptr_eq(&won_first, &first));
        assert!(
            Arc::ptr_eq(&won_second, &first),
            "second registration must return the first Arc"
        );
        RUNNING.write().unwrap().remove(id);
    }

    /// The theme-change push list: only plugins that declared `editor.kit`,
    /// and one entry per window they contribute. A plugin without the
    /// capability is never told the theme changed (its window has no kit to
    /// restyle), and a capability-holder that contributes no window yields no
    /// target at all.
    #[test]
    fn theme_push_targets_selects_only_editor_kit_windows() {
        let with_window = |id: &str, caps: &[&str], windows: &[&str]| {
            let mut m = fixture_manifest(id);
            m.capabilities = caps.iter().map(|c| c.to_string()).collect();
            m.contributes.windows = windows
                .iter()
                .map(|w| {
                    serde_json::from_value(serde_json::json!({
                        "id": w, "entry": "index.html", "title": "W",
                        "width": 800.0, "height": 600.0
                    }))
                    .unwrap()
                })
                .collect();
            (m, PathBuf::from("/tmp/install"))
        };

        let mut plugins = std::collections::BTreeMap::new();
        plugins.insert(
            "pub.kit".to_string(),
            with_window("pub.kit", &["editor.kit", "vault.read"], &["main", "side"]),
        );
        plugins.insert(
            "pub.nokit".to_string(),
            with_window("pub.nokit", &["vault.read"], &["main"]),
        );
        plugins.insert(
            "pub.kit-headless".to_string(),
            with_window("pub.kit-headless", &["editor.kit"], &[]),
        );

        assert_eq!(
            theme_push_targets(&plugins),
            vec![
                ("pub.kit".to_string(), "main".to_string()),
                ("pub.kit".to_string(), "side".to_string()),
            ]
        );
        assert!(theme_push_targets(&std::collections::BTreeMap::new()).is_empty());
    }

    #[test]
    fn theme_change_push_carries_the_authoritative_bundle() {
        let theme = serde_json::json!({
            "light_css": ".moraya-editor { font-family: New; }",
            "dark_css": ".moraya-editor { font-family: NewDark; }",
            "follow_system": true
        });
        let payload = theme_changed_payload(theme.clone());
        assert_eq!(payload["type"], "theme-changed");
        assert_eq!(payload["theme"], theme);
    }

    #[test]
    fn is_process_plugin_distinguishes_ui_only_from_binary() {
        // Binary-backed fixture → process plugin.
        let mut m = fixture_manifest("pub.binary");
        m.binary
            .insert("aarch64-apple-darwin".into(), "bin/x".into());
        assert!(
            is_process_plugin(&m),
            "a plugin with a binary is process-backed"
        );

        // UI-only (no binary) → not a process plugin; must be skipped by
        // startup_activate_all and rejected by get_or_register with a clear msg.
        let ui = fixture_manifest("pub.ui-only");
        assert!(ui.binary.is_empty());
        assert!(!is_process_plugin(&ui), "a binary-less plugin is UI-only");
    }

    /// A ui-only plugin publishes only under the `universal` key; resolve_download
    /// must fall back to it on this host, and error only when neither the host
    /// triple nor `universal` is present (FIX-1).
    fn registry_entry(
        id: &str,
        download: &[(&str, &str)],
        sha: &[(&str, &str)],
    ) -> market::RegistryEntry {
        use std::collections::BTreeMap;
        let dl: BTreeMap<String, String> = download
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let sh: BTreeMap<String, String> = sha
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        market::RegistryEntry {
            id: id.to_string(),
            version: "1.0.0".to_string(),
            min_host: ">=0.0.0".to_string(),
            archs: dl.keys().cloned().collect(),
            size: 1,
            sha256: sh,
            name: id.to_string(),
            category: None,
            description: None,
            i18n: None,
            icon_url: None,
            changelog_url: None,
            download: dl,
        }
    }

    #[test]
    fn resolve_download_falls_back_to_universal() {
        let entry = registry_entry(
            "roam",
            &[(
                "universal",
                "https://plugins.notemd.net/api/download/roam/1.0.0/universal",
            )],
            &[("universal", "uu")],
        );
        let (url, sha) = resolve_download(&entry).unwrap();
        assert!(
            url.ends_with("universal"),
            "url {url} must resolve to the universal package"
        );
        assert_eq!(sha, "uu");
    }

    #[test]
    fn resolve_download_errors_when_neither_triple_nor_universal() {
        let entry = registry_entry("x", &[], &[]);
        let err = resolve_download(&entry).unwrap_err();
        assert!(err.contains("no download for arch"), "got {err}");
    }
}

/// Assemble everything the lifecycle needs to (re)spawn without an AppHandle.
fn build_spawn_ctx<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    manifest: &plugin_protocol::ManifestV2,
    install_dir: &Path,
) -> Result<SpawnCtx, String> {
    // Same arch mapping discovery validated against at scan time.
    let triple = discovery::current_arch_triple()
        .ok_or_else(|| format!("unsupported host arch '{}'", std::env::consts::ARCH))?;
    let rel = manifest.binary.get(triple).ok_or_else(|| {
        format!(
            "plugin '{}': no binary for host arch '{triple}'",
            manifest.id
        )
    })?;
    let log_dir = app
        .path()
        .app_log_dir()
        .map_err(|e| format!("cannot resolve app log dir: {e}"))?
        .join("plugins");
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("cannot resolve app data dir: {e}"))?;
    Ok(SpawnCtx {
        binary: install_dir.join(rel),
        host_sink: host_api::make_sink_for_app(
            manifest.id.clone(),
            manifest.capabilities.clone(),
            app.clone(),
            log_dir.clone(),
        ),
        log_dir,
        host_version: app.package_info().version.to_string(),
        locale: crate::read_saved_locale(app),
        app_data,
    })
}
