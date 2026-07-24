#[cfg(debug_assertions)]
use std::fs::OpenOptions;
#[cfg(debug_assertions)]
use std::io::Write;
use std::sync::Mutex;
#[cfg(not(target_os = "ios"))]
use tauri::image::Image;
#[cfg(not(target_os = "ios"))]
use tauri::menu::{
    AboutMetadata, IconMenuItem, Menu, MenuBuilder, MenuItem, MenuItemBuilder, MenuItemKind,
    PredefinedMenuItem, Submenu, SubmenuBuilder,
};
#[cfg(not(target_os = "ios"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

pub mod app_dirs;
pub mod log_bus;
pub mod shared_config;

#[cfg(not(target_os = "ios"))]
pub mod cli;
#[cfg(not(target_os = "ios"))]
pub mod plugin_host;
#[cfg(target_os = "ios")]
#[path = "plugin_host_ios.rs"]
pub mod plugin_host;
#[cfg(not(target_os = "ios"))]
pub mod plugin_runtime;
#[cfg(not(target_os = "ios"))]
pub mod themes;
#[cfg(not(target_os = "ios"))]
pub mod vault_sync;
#[cfg(not(target_os = "ios"))]
pub mod git_history;
#[cfg(not(target_os = "ios"))]
pub mod preview_window;
#[cfg(not(target_os = "ios"))]
pub mod agents_sync;
#[cfg(not(target_os = "ios"))]
pub mod sotvault;

#[cfg(any(target_os = "ios", test))]
pub mod vault_ios;

pub struct PendingFiles(Mutex<Vec<String>>);
#[cfg(not(target_os = "ios"))]
pub struct TrayRepoItem(Mutex<Option<MenuItem<tauri::Wry>>>);
#[cfg(not(target_os = "ios"))]
pub struct TrayStatusItem(Mutex<Option<IconMenuItem<tauri::Wry>>>);
#[cfg(not(target_os = "ios"))]
pub struct RecentMenu(pub Mutex<Option<Submenu<tauri::Wry>>>);
#[cfg(not(target_os = "ios"))]
pub struct TraySyncNowItem(pub Mutex<Option<MenuItem<tauri::Wry>>>);
#[cfg(not(target_os = "ios"))]
pub struct TrayShownLargeFiles(pub Mutex<Vec<String>>);
#[cfg(not(target_os = "ios"))]
pub struct DailyNotesEnabled(pub std::sync::Mutex<bool>);

#[tauri::command]
fn drain_pending_files(state: tauri::State<'_, PendingFiles>) -> Vec<String> {
    state.0.lock().unwrap().drain(..).collect()
}

/// Append a diagnostic line to /tmp/mdeditor.log in debug builds (best-effort).
/// Also pushes to the unified log bus in all builds.
#[allow(unused_variables)]
fn dlog(msg: &str) {
    crate::log_bus::push("info", msg.to_string());
    #[cfg(debug_assertions)]
    {
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/mdeditor.log")
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let _ = writeln!(f, "{} {}", ts, msg);
        }
    }
}

// ── File helpers ─────────────────────────────────────────────────────────────

fn sanitize_io_err(e: std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound         => "File not found".to_string(),
        std::io::ErrorKind::PermissionDenied => "Permission denied".to_string(),
        std::io::ErrorKind::AlreadyExists    => "File already exists".to_string(),
        _                                    => "Operation failed".to_string(),
    }
}

/// Validate that the path resolves under the user home, /tmp, /var, or /private.
/// Walks up ancestor dirs to handle not-yet-existing files.
fn safe_path(path: &str) -> Result<std::path::PathBuf, String> {
    use std::path::Path;
    if path.is_empty() {
        return Err("Path must not be empty".to_string());
    }
    if !path.starts_with('/') {
        return Err("Path must be absolute".to_string());
    }
    let p = Path::new(path);
    let canonical = std::fs::canonicalize(p).or_else(|_| {
        let mut parts: Vec<std::ffi::OsString> = Vec::new();
        if let Some(fname) = p.file_name() { parts.push(fname.to_owned()); }
        let mut ancestor = p.parent();
        loop {
            match ancestor {
                Some(dir) if dir.as_os_str().is_empty() => break,
                Some(dir) if dir.exists() => {
                    let mut base = std::fs::canonicalize(dir)
                        .map_err(|e| e.to_string())?;
                    for part in parts.iter().rev() { base.push(part); }
                    // Guard against ".." components in the reconstructed path
                    if base.components().any(|c| c == std::path::Component::ParentDir) {
                        return Err("Path traversal detected".to_string());
                    }
                    return Ok(base);
                }
                Some(dir) => {
                    if let Some(n) = dir.file_name() { parts.push(n.to_owned()); }
                    ancestor = dir.parent();
                }
                None => break,
            }
        }
        Err("Cannot resolve path".to_string())
    })?;

    let home = dirs::home_dir().ok_or("Cannot determine home dir")?;
    if canonical.starts_with(&home) { return Ok(canonical); }
    for prefix in &["/tmp", "/var", "/private"] {
        if canonical.starts_with(prefix) { return Ok(canonical); }
    }
    Err("Path outside allowed directories".to_string())
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = input.as_bytes();
    let mut buf: Vec<u8> = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &b in input {
        if matches!(b, b'\n' | b'\r' | b' ') { continue; }
        if b == b'=' { break; }
        let val = TABLE.iter().position(|&c| c == b)
            .ok_or_else(|| "Invalid base64".to_string())? as u32;
        acc = (acc << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            buf.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(buf)
}

/// Write base64-encoded binary data to a file. Creates parent directories.
/// Strips optional `data:...;base64,` prefix automatically.
#[tauri::command]
fn write_file_binary(path: String, base64_data: String) -> Result<(), String> {
    use std::io::Write;
    let dest = safe_path(&path)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(sanitize_io_err)?;
    }
    let raw = if let Some(i) = base64_data.find(";base64,") {
        &base64_data[i + 8..]
    } else {
        base64_data.as_str()
    };
    let bytes = base64_decode(raw)?;
    let mut f = std::fs::File::create(&dest).map_err(sanitize_io_err)?;
    f.write_all(&bytes).map_err(sanitize_io_err)
}

/// Move a file from old_path to new_path. Creates parent directories of new_path.
/// Silently succeeds if old_path does not exist (already migrated).
#[tauri::command]
fn rename_file(old_path: String, new_path: String) -> Result<(), String> {
    let src = safe_path(&old_path)?;
    if !src.exists() { return Ok(()); }
    let dst = safe_path(&new_path)?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(sanitize_io_err)?;
    }
    std::fs::rename(&src, &dst).map_err(sanitize_io_err)
}

/// Quit the application. Called from the frontend after the close-window
/// dirty-tab confirmation loop completes successfully. macOS does NOT quit
/// the app on its own when the last NSWindow is closed (unlike Windows / Linux),
/// so we trigger it explicitly.
#[cfg(not(target_os = "ios"))]
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// Whether an `ExitRequested` with this code should be prevented (i.e. the app
/// stays alive in the menu-bar tray). Per Tauri, `code` is `None` for a
/// user-interaction exit (e.g. the last window closing) and `Some` for a
/// programmatic quit via `app.exit()` / `restart()`. We keep running only for
/// the former; explicit quits — the tray "Quit" item and the `quit_app` command,
/// both `app.exit(0)` (`code == Some`) — must actually exit.
#[cfg(not(target_os = "ios"))]
fn should_prevent_exit(code: Option<i32>) -> bool {
    code.is_none()
}

/// Set the enabled state of a plugin-contributed menu item by id.
/// IDs follow the `plugin:<plugin-id>:<command>` convention.
/// Walks the entire menu tree (top-level + submenus) so it finds items
/// regardless of which submenu they were appended to.
#[cfg(not(target_os = "ios"))]
#[tauri::command]
fn set_plugin_menu_item_enabled(app: tauri::AppHandle, id: String, enabled: bool) -> Result<(), String> {
    fn walk<R: tauri::Runtime>(items: Vec<MenuItemKind<R>>, id: &str, enabled: bool) -> bool {
        for item in items {
            match item {
                MenuItemKind::MenuItem(mi) => {
                    if mi.id().0.as_str() == id {
                        let _ = mi.set_enabled(enabled);
                        return true;
                    }
                }
                MenuItemKind::Submenu(sm) => {
                    if let Ok(child) = sm.items() {
                        if walk(child, id, enabled) {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }
    let menu = app.menu().ok_or_else(|| "no menu set".to_string())?;
    let items = menu.items().map_err(|e| e.to_string())?;
    if walk(items, &id, enabled) {
        Ok(())
    } else {
        Err(format!("menu item not found: {id}"))
    }
}

#[derive(serde::Serialize)]
struct ExtResult {
    ext: String,
    uti: Option<String>,
    ok: bool,
    error: Option<String>,
}

#[cfg(target_os = "macos")]
mod macos_defaults {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};

    // LaunchServices APIs: deprecated in macOS 12+ in favor of NSWorkspace's
    // async setDefaultApplicationAtURL:toOpenContentType:, but still functional
    // and synchronous (which is much easier to call from Rust FFI).
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn LSSetDefaultRoleHandlerForContentType(
            in_content_type: CFStringRef,
            in_role: u32,
            in_handler_bundle_id: CFStringRef,
        ) -> i32;

        fn UTTypeCreatePreferredIdentifierForTag(
            in_tag_class: CFStringRef,
            in_tag: CFStringRef,
            in_conforming_to_uti: CFStringRef,
        ) -> CFStringRef;
    }

    const K_LS_ROLES_ALL: u32 = 0xFFFFFFFF;

    /// Resolve the canonical UTI for a filename extension (e.g. "py" → "public.python-script").
    /// Returns None when the system can't determine a UTI.
    pub fn resolve_uti(ext: &str) -> Option<String> {
        let tag_class = CFString::new("public.filename-extension");
        let ext_cf = CFString::new(ext);
        let uti_ref = unsafe {
            UTTypeCreatePreferredIdentifierForTag(
                tag_class.as_concrete_TypeRef(),
                ext_cf.as_concrete_TypeRef(),
                std::ptr::null(),
            )
        };
        if uti_ref.is_null() {
            return None;
        }
        let uti = unsafe { CFString::wrap_under_create_rule(uti_ref) };
        Some(uti.to_string())
    }

    /// Set `bundle_id` as the default handler for the given UTI across all roles.
    /// Returns `Ok(())` on success or `Err(OSStatus)` on failure.
    pub fn set_handler(uti: &str, bundle_id: &str) -> Result<(), i32> {
        let uti_cf = CFString::new(uti);
        let bundle_cf = CFString::new(bundle_id);
        let status = unsafe {
            LSSetDefaultRoleHandlerForContentType(
                uti_cf.as_concrete_TypeRef(),
                K_LS_ROLES_ALL,
                bundle_cf.as_concrete_TypeRef(),
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(status)
        }
    }
}

/// Set this app as the macOS default handler for each given file extension.
/// For each extension we resolve the UTI and call LaunchServices to register
/// the bundle as the default handler across all roles. Returns a per-extension
/// result so the frontend can report partial success.
#[cfg(not(target_os = "ios"))]
#[tauri::command]
fn set_default_app_for_extensions(app: tauri::AppHandle, exts: Vec<String>) -> Vec<ExtResult> {
    #[cfg(target_os = "macos")]
    {
        let bundle_id = app.config().identifier.clone();
        exts.into_iter()
            .map(|ext| match macos_defaults::resolve_uti(&ext) {
                None => ExtResult {
                    ext,
                    uti: None,
                    ok: false,
                    error: Some("no UTI registered for this extension".into()),
                },
                Some(uti) => match macos_defaults::set_handler(&uti, &bundle_id) {
                    Ok(()) => ExtResult { ext, uti: Some(uti), ok: true, error: None },
                    Err(status) => ExtResult {
                        ext,
                        uti: Some(uti),
                        ok: false,
                        error: Some(format!("LaunchServices OSStatus {}", status)),
                    },
                },
            })
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        exts.into_iter()
            .map(|ext| ExtResult {
                ext,
                uti: None,
                ok: false,
                error: Some("only supported on macOS".into()),
            })
            .collect()
    }
}

fn show_insights_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::WebviewUrl;
    let win = app.get_webview_window("insights").or_else(|| {
        tauri::WebviewWindowBuilder::new(app, "insights", WebviewUrl::App("insights.html".into()))
            .title("Reading Insights")
            .inner_size(900.0, 640.0)
            .min_inner_size(520.0, 360.0)
            .resizable(true)
            .decorations(true)
            .visible(false)
            .build()
            .map_err(|e| eprintln!("[insights] window build failed: {e}"))
            .ok()
    });
    if let Some(w) = win {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// The single Daily Notes window's label.
#[cfg(not(target_os = "ios"))]
const DAILY_NOTES_LABEL: &str = "daily-notes";

/// Ensure the single Daily Notes window exists; focus if already open.
#[cfg(not(target_os = "ios"))]
fn show_daily_notes_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    use tauri::WebviewUrl;
    let win = app.get_webview_window(DAILY_NOTES_LABEL).or_else(|| {
        tauri::WebviewWindowBuilder::new(app, DAILY_NOTES_LABEL, WebviewUrl::App("daily-notes.html".into()))
            .title("Daily Notes")
            .inner_size(720.0, 900.0)
            .min_inner_size(480.0, 480.0)
            .resizable(true)
            .decorations(true)
            .visible(false)
            .build()
            .map_err(|e| eprintln!("[daily-notes] window build failed: {e}"))
            .ok()
    });
    if let Some(w) = win {
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.unminimize();
    }
}

#[cfg(not(target_os = "ios"))]
#[tauri::command]
fn open_daily_notes_window(app: tauri::AppHandle) {
    show_daily_notes_window(&app);
}

#[cfg(not(target_os = "ios"))]
#[tauri::command]
fn set_daily_notes_enabled(app: tauri::AppHandle, enabled: bool) {
    if let Some(st) = app.try_state::<DailyNotesEnabled>() {
        *st.0.lock().unwrap() = enabled;
    }
    rebuild_menu(&app);
}

#[cfg(not(target_os = "ios"))]
fn open_logs_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>, filter: Option<&str>) {
    use tauri::WebviewUrl;
    let win = app.get_webview_window("logs").or_else(|| {
        tauri::WebviewWindowBuilder::new(app, "logs", WebviewUrl::App("logs.html".into()))
            .title("Logs")
            .inner_size(900.0, 640.0)
            .min_inner_size(520.0, 360.0)
            .resizable(true)
            .decorations(true)
            .visible(false)
            .build()
            .map_err(|e| eprintln!("[logs] window build failed: {e}"))
            .ok()
    });
    if let Some(w) = win {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        if let Some(f) = filter {
            // Small delay so the webview has registered its listener before the
            // preset filter arrives (mirrors emit_open_file_delayed usage).
            let app2 = app.clone();
            let f = f.to_string();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(400));
                let _ = app2.emit("nav://logs-filter", f);
            });
        }
    }
}

/// View ▸ Plugin Market… (子项目③). Standalone window cloned from the insights
/// window: it bootstraps its own webview state and drives the market commands
/// (index / preview / install / uninstall / set_enabled) + capability consent.
fn show_plugin_market_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::WebviewUrl;
    let win = app.get_webview_window("plugin-market").or_else(|| {
        tauri::WebviewWindowBuilder::new(
            app,
            "plugin-market",
            WebviewUrl::App("plugin-market.html".into()),
        )
        .title("Plugin Market")
        .inner_size(900.0, 640.0)
        .min_inner_size(520.0, 360.0)
        .resizable(true)
        .decorations(true)
        .visible(false)
        .build()
        .map_err(|e| eprintln!("[plugin-market] window build failed: {e}"))
        .ok()
    });
    if let Some(w) = win {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Frontend entry point for the Plugin Market window (the retired Plugins
/// settings tab's "Open Plugin Market…" button invokes this). Mirrors the
/// View ▸ Plugin Market menu path.
#[cfg(not(target_os = "ios"))]
#[tauri::command]
fn open_plugin_market_window(app: tauri::AppHandle) {
    show_plugin_market_window(&app);
}

#[tauri::command]
async fn editor_show_and_open_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
        let _ = win.unminimize();
        // Defer to existing frontend "open file" event so the editor can decide tabs.
        let _ = win.emit("editor://open-path", &path);
    } else {
        // Main window isn't built; emit a global event the next startup will drain.
        let _ = app.emit("editor://pending-open", &path);
    }
    Ok(())
}

#[tauri::command]
async fn editor_open_remote_buffer(app: tauri::AppHandle, remote_path: String, content: String) -> Result<(), String> {
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
        let _ = win.emit("editor://open-remote-buffer", &serde_json::json!({
            "remote_path": remote_path,
            "content": content
        }));
    }
    Ok(())
}

/// Frontend diagnostics into /tmp/mdeditor.log (debug builds only; dlog is a
/// release no-op). Lets unattended GUI verification trace webview-side state.
#[tauri::command]
fn dbg_log(msg: String) {
    dlog(&format!("[js] {}", msg));
}

#[tauri::command]
fn file_exists(path: String) -> bool {
    std::path::Path::new(&path).exists()
}

#[tauri::command]
fn shared_config_read() -> Result<crate::shared_config::SharedConfig, String> {
    let path = crate::shared_config::config_path().map_err(|e| e.to_string())?;
    crate::shared_config::read(&path).map_err(|e| e.to_string())
}

#[tauri::command]
fn shared_config_write(cfg: crate::shared_config::SharedConfig) -> Result<(), String> {
    let path = crate::shared_config::config_path().map_err(|e| e.to_string())?;
    crate::shared_config::write(&path, &cfg).map_err(|e| e.to_string())
}

fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    } else {
        // Window might have been destroyed, recreate it
        let _ = tauri::WebviewWindowBuilder::new(
            app,
            "main",
            tauri::WebviewUrl::default(),
        )
        .title("note.md")
        .inner_size(1000.0, 700.0)
        .min_inner_size(600.0, 400.0)
        .build();
    }
}

/// Quick-note entry point shared by the tray item and the global hotkey: bring
/// the main window forward, then hand off to the frontend (which owns vault
/// resolution, filename/timestamp, disk write, open + focus) via the
/// `quick-note` event. Kept side-effect-light here so both triggers behave
/// identically regardless of which surface fired.
fn trigger_quick_note<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    show_main_window(app);
    let _ = app.emit("quick-note", ());
}

#[cfg(not(target_os = "ios"))]
fn pick_sync_folder(app: &tauri::AppHandle) {
    let app_clone = app.clone();
    pick_sync_folder_inner(app, move |_path| {
        let _ = vault_sync::vault_sync_start(app_clone.clone());
        refresh_tray_status(&app_clone);
    });
}

#[cfg(not(target_os = "ios"))]
fn pick_sync_folder_inner(app: &tauri::AppHandle, on_done: impl FnOnce(String) + Send + 'static) {
    use tauri_plugin_dialog::DialogExt;
    use tauri_plugin_store::StoreExt;

    let app_clone = app.clone();
    app.dialog()
        .file()
        .set_title("Select Vault Git Repository")
        .pick_folder(move |folder| {
            if let Some(path) = folder {
                let path_str = path.to_string();
                let mgr = app_clone.state::<std::sync::Arc<vault_sync::VaultSyncManager>>();
                *mgr.repo_path.lock().unwrap() = Some(path_str.clone());

                if let Ok(s) = app_clone.store("settings.json") {
                    let _ = s.set("vault_sync.repo_path", serde_json::json!(&path_str));
                    let _ = s.save();
                }

                if let Ok(shared_path) = crate::shared_config::config_path() {
                    if let Ok(mut cfg) = crate::shared_config::read(&shared_path) {
                        cfg.sotvault = Some(path_str.clone());
                        let _ = crate::shared_config::write(&shared_path, &cfg);
                    }
                }

                update_tray_repo_label(&app_clone, &path_str);
                agents_sync::restart(&app_clone, &path_str);
                on_done(path_str);
            }
        });
}

#[cfg(not(target_os = "ios"))]
fn update_tray_repo_label(app: &tauri::AppHandle, path: &str) {
    if let Some(state) = app.try_state::<TrayRepoItem>() {
        if let Some(item) = state.0.lock().unwrap().as_ref() {
            let _ = item.set_text(&format!("Vault: {}", abbreviate_path(path)));
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn abbreviate_path(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() && path.starts_with(&home) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_string()
    }
}

/// Format a Unix-seconds timestamp string as a compact, localized "… ago"
/// relative time. `locale` is one of "en" / "zh" / "ja".
#[cfg(not(target_os = "ios"))]
fn relative_time(unix_secs: &str, locale: &str) -> String {
    let then: u64 = match unix_secs.trim().parse() {
        Ok(v) => v,
        Err(_) => return unix_secs.to_string(),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let d = now.saturating_sub(then);
    let (n, unit): (u64, &str) = if d < 60 {
        (d, "s")
    } else if d < 3600 {
        (d / 60, "m")
    } else if d < 86_400 {
        (d / 3600, "h")
    } else {
        (d / 86_400, "d")
    };
    if d < 5 {
        return match locale {
            "zh" => "刚刚",
            "ja" => "たった今",
            "de" => "gerade eben",
            _ => "just now",
        }
        .to_string();
    }
    match locale {
        "zh" => {
            let u = match unit { "s" => "秒", "m" => "分钟", "h" => "小时", _ => "天" };
            format!("{n}{u}前")
        }
        "ja" => {
            let u = match unit { "s" => "秒", "m" => "分", "h" => "時間", _ => "日" };
            format!("{n}{u}前")
        }
        "de" => {
            let u = match unit { "s" => "Sek.", "m" => "Min.", "h" => "Std.", _ => "T." };
            format!("vor {n} {u}")
        }
        _ => format!("{n}{unit} ago"),
    }
}

/// Localized "last synced …" phrase for the tray status line / tooltip.
#[cfg(not(target_os = "ios"))]
fn last_sync_phrase(locale: &str, last_sync: Option<&str>) -> String {
    match last_sync {
        None => menu_label(locale, "sync.neverSynced"),
        Some(ts) => {
            let rel = relative_time(ts, locale);
            match locale {
                "zh" => format!("{rel}同步"),
                "ja" => format!("{rel}に同期"),
                "de" => format!("{rel} synchronisiert"),
                _ => format!("synced {rel}"),
            }
        }
    }
}

/// Localized human label for a sync state.
#[cfg(not(target_os = "ios"))]
fn state_label(locale: &str, state: vault_sync::SyncState) -> String {
    use vault_sync::SyncState;
    let key = match state {
        SyncState::NotConfigured => "sync.state.notConfigured",
        SyncState::Stopped => "sync.state.stopped",
        SyncState::Running => "sync.state.running",
        SyncState::Syncing => "sync.state.syncing",
        SyncState::Conflict => "sync.state.conflict",
        SyncState::Error => "sync.state.error",
        SyncState::GitUnavailable => "sync.state.gitUnavailable",
    };
    menu_label(locale, key)
}

/// Flat, font-harmonized status dot shown to the left of the tray dropdown's
/// status line: green = healthy, red = problem, grey = idle. These are plain
/// filled circles (no gloss) so they sit cleanly next to the menu text.
#[cfg(not(target_os = "ios"))]
fn status_dot_image(state: vault_sync::SyncState, has_large: bool) -> Option<Image<'static>> {
    use vault_sync::SyncState;
    let bytes: &'static [u8] = if state.is_problem() {
        include_bytes!("../icons/dot-red.png")
    } else if has_large {
        include_bytes!("../icons/dot-yellow.png")
    } else if matches!(state, SyncState::Running | SyncState::Syncing) {
        include_bytes!("../icons/dot-green.png")
    } else {
        include_bytes!("../icons/dot-grey.png")
    };
    Image::from_bytes(bytes).ok()
}

/// Refresh the menu-bar tray icon, tooltip and status menu item so the current
/// version-control health is always visible at a glance. The bottom-right dot
/// on the icon mirrors the vault git status: green = healthy, red = problem
/// (git unavailable / error / conflict), no dot = stopped / not configured.
/// The icon glyph itself stays clean — status is conveyed by the dot only.
#[cfg(not(target_os = "ios"))]
pub fn refresh_tray_status(app: &tauri::AppHandle) {
    use vault_sync::{SyncState, VaultSyncManager};

    let mgr = app.state::<std::sync::Arc<VaultSyncManager>>();
    let state = *mgr.state.lock().unwrap();
    let last_sync = mgr.last_sync.lock().unwrap().clone();
    let skipped_large = mgr.skipped_large_files.lock().unwrap().clone();
    let has_large = !skipped_large.is_empty();

    // Dot indicator states mirroring the vault git status.
    let active = matches!(state, SyncState::Running | SyncState::Syncing);
    let problem = state.is_problem();

    let locale = read_saved_locale(app);
    let status_text = format!(
        "{} · {}",
        state_label(&locale, state),
        last_sync_phrase(&locale, last_sync.as_deref()),
    );
    let tooltip = format!("note.md — {}: {}", menu_label(&locale, "sync.label"), status_text);

    if let Some(tray) = app.tray_by_id("main") {
        let icon = if problem {
            Image::from_bytes(include_bytes!("../icons/tray-icon-error.png"))
        } else if has_large {
            Image::from_bytes(include_bytes!("../icons/tray-icon-warning.png"))
        } else if active {
            Image::from_bytes(include_bytes!("../icons/tray-icon-active.png"))
        } else {
            Image::from_bytes(include_bytes!("../icons/tray-icon.png"))
        };
        if let Ok(img) = icon {
            let _ = tray.set_icon(Some(img));
        }
        // Keep the menu-bar glyph clean: no text title next to the icon.
        let _ = tray.set_title(None::<&str>);
        let _ = tray.set_tooltip(Some(&tooltip));
    }

    if let Some(status_state) = app.try_state::<TrayStatusItem>() {
        if let Some(item) = status_state.0.lock().unwrap().as_ref() {
            let _ = item.set_icon(status_dot_image(state, has_large));
            let _ = item.set_text(&status_text);
        }
    }

    // Disable "Sync Now" while a sync is in progress.
    if let Some(sn) = app.try_state::<TraySyncNowItem>() {
        if let Some(item) = sn.0.lock().unwrap().as_ref() {
            let _ = item.set_enabled(state != SyncState::Syncing);
        }
    }

    // Rebuild tray menu when large-file list changes so the submenu stays accurate.
    if let Some(shown) = app.try_state::<TrayShownLargeFiles>() {
        let mut shown = shown.0.lock().unwrap();
        if *shown != skipped_large {
            *shown = skipped_large.clone();
            drop(shown);
            let locale2 = read_saved_locale(app);
            if let Some(tray) = app.tray_by_id("main") {
                if let Ok((menu, repo_item, status_item, sync_now_item)) = build_tray_menu(app, &locale2) {
                    *app.state::<TrayRepoItem>().0.lock().unwrap() = Some(repo_item);
                    *app.state::<TrayStatusItem>().0.lock().unwrap() = Some(status_item);
                    *app.state::<TraySyncNowItem>().0.lock().unwrap() = Some(sync_now_item);
                    let _ = tray.set_menu(Some(menu));
                }
            }
        }
    }
}

/// Back-compat shim: callers that only know "active or not" now just trigger a
/// full status refresh from the sync manager's real state.
#[cfg(not(target_os = "ios"))]
pub fn update_tray_icon(app: &tauri::AppHandle, _active: bool) {
    refresh_tray_status(app);
}

/// Build the Tauri runtime `Context` from the embedded tauri.conf.json.
///
/// `tauri::generate_context!()` is a proc-macro that emits a `_EMBED_INFO_PLIST`
/// static at its call-site. Calling it from two places in the same crate gives
/// a duplicate-symbol link error. Funneling every consumer through this single
/// helper guarantees one expansion. Both the GUI `run()` and the headless CLI
/// runner use it.
pub fn tauri_context() -> tauri::Context {
    tauri::generate_context!()
}

/// True when this x86_64 process is running under Rosetta 2 on Apple Silicon.
/// `sysctl.proc_translated` is 1 only for translated processes; absent (Intel
/// Mac) or 0 means native. Used to override the updater target so a Rosetta
/// install migrates to the native arm64 build instead of staying on x86 forever.
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
fn running_under_rosetta() -> bool {
    std::process::Command::new("sysctl")
        .args(["-n", "sysctl.proc_translated"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
        .unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    dlog("=== note.md start ===");
    dlog(&format!("argv: {:?}", std::env::args().collect::<Vec<_>>()));

    // rustls 0.23 no longer auto-selects a crypto provider; install ring as
    // the default so reqwest::Client::new() doesn't panic with "No provider
    // set" on first HTTPS use (tauri-plugin-http, tauri-plugin-updater, etc).
    // Safe to ignore Err: it only fires if already installed (e.g. in tests).
    let _ = rustls::crypto::ring::default_provider().install_default();

    let builder = tauri::Builder::default()
        .manage(PendingFiles(Mutex::new(Vec::new())));
    #[cfg(not(target_os = "ios"))]
    let builder = builder.manage(TrayRepoItem(Mutex::new(None)));
    #[cfg(not(target_os = "ios"))]
    let builder = builder.manage(TrayStatusItem(Mutex::new(None)));
    #[cfg(not(target_os = "ios"))]
    let builder = builder.manage(RecentMenu(Mutex::new(None)));
    #[cfg(not(target_os = "ios"))]
    let builder = builder.manage(TraySyncNowItem(Mutex::new(None)));
    #[cfg(not(target_os = "ios"))]
    let builder = builder.manage(TrayShownLargeFiles(Mutex::new(Vec::new())));
    #[cfg(not(target_os = "ios"))]
    let builder = builder.manage(DailyNotesEnabled(std::sync::Mutex::new(false)));
    #[cfg(not(target_os = "ios"))]
    let builder = builder.manage(preview_window::PreviewStore::default());
    #[cfg(not(target_os = "ios"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
        dlog(&format!("single_instance argv: {:?}", argv));
        for arg in argv.iter().skip(1) {
            emit_open_file_delayed(app, arg);
        }
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }));
    let app = builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_os::init());
    #[cfg(not(target_os = "ios"))]
    let app = app
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin({
            // Self-heal Rosetta installs. The updater picks its platform key from
            // the RUNNING process arch, not the hardware: an x86_64 build running
            // under Rosetta on Apple Silicon would keep fetching `darwin-x86_64`
            // forever (the v6.720.1 "ARM upgrade became x86, won't launch"
            // incident). When we detect translation, request `darwin-aarch64` so
            // the next update migrates to the native arm64 build; after that
            // relaunch the process is native and the default target applies.
            #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
            let b = if running_under_rosetta() {
                tauri_plugin_updater::Builder::new().target("darwin-aarch64")
            } else {
                tauri_plugin_updater::Builder::new()
            };
            #[cfg(not(all(target_os = "macos", target_arch = "x86_64")))]
            let b = tauri_plugin_updater::Builder::new();
            b.build()
        })
        .plugin(tauri_plugin_process::init());
    // System-wide quick-note hotkey. Only one shortcut is registered (in
    // `.setup()`), so the handler fires quick-note on any Pressed event without
    // needing to match which accelerator it was.
    #[cfg(not(target_os = "ios"))]
    let app = app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, _shortcut, event| {
                if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                    trigger_quick_note(app);
                }
            })
            .build(),
    );
    // plugin:// scheme — plugin UI static assets + fetch-RPC bridge (spec §7.1).
    // Registration cannot be flag-conditional; the handler answers 404 for
    // flag-off/unknown ids, so v2-off behavior is unchanged.
    //
    // Asynchronous registration + a dedicated thread per request is REQUIRED:
    // WKWebView delivers scheme requests on the main thread, and the RPC branch
    // blocks on native dialogs (host.dialog.*) that themselves need the main
    // run loop — answering inline would deadlock. From the spawned thread the
    // blocking dialog calls are safe (they hop to the then-free main thread).
    #[cfg(not(target_os = "ios"))]
    let app = app.register_asynchronous_uri_scheme_protocol("plugin", |ctx, request, responder| {
        let app = ctx.app_handle().clone();
        std::thread::spawn(move || {
            responder.respond(crate::plugin_runtime::protocol::handle(&app, request));
        });
    });
    let app = app
        .invoke_handler({
            #[cfg(not(target_os = "ios"))]
            { tauri::generate_handler![
                quit_app,
                drain_pending_files,
                set_default_app_for_extensions,
                set_plugin_menu_item_enabled,
                plugin_host::get_plugin_manifests,
                plugin_host::get_all_plugin_manifests,
                plugin_host::plugin_is_enabled,
                plugin_host::invoke_plugin,
                plugin_runtime::commands::plugin_v2_execute,
                plugin_runtime::commands::plugin_v2_open_window,
                plugin_runtime::commands::plugin_market_index,
                plugin_runtime::commands::plugin_market_preview,
                plugin_runtime::commands::plugin_market_install,
                plugin_runtime::commands::plugin_market_uninstall,
                plugin_runtime::commands::plugin_market_set_enabled,
                plugin_runtime::commands::plugin_market_installed,
                cli::state::cli_payload,
                cli::state::cli_finish,
                cli::install::cli_install_status,
                cli::install::cli_install,
                cli::install::cli_uninstall,
                cli::install::cli_install_candidates,
                themes::commands::theme_list,
                themes::commands::theme_reveal,
                themes::commands::theme_load_compiled,
                themes::commands::theme_recompile,
                themes::commands::theme_recompile_all,
                themes::commands::theme_restore_builtins,
                themes::commands::theme_import,
                themes::commands::theme_install,
                themes::commands::theme_cancel_import,
                vault_sync::vault_sync_start,
                vault_sync::vault_sync_stop,
                vault_sync::vault_sync_now,
                vault_sync::vault_sync_status,
                vault_sync::vault_sync_logs,
                sotvault::sotvault_vault_root,
                sotvault::sotvault_vault_debug,
                sotvault::notemd_vault_settings_get,
                sotvault::notemd_vault_settings_set,
                sotvault::notemd_quick_note_dir,
                sotvault::notemd_mirror_metas,
                sotvault::notemd_migrate_mirror_meta,
                sotvault::notemd_relink_mirror_source,
                sotvault::notemd_mirror_note_siblings,
                sotvault::sotvault_records,
                sotvault::sotvault_forget,
                sotvault::sotvault_sync_to_vault,
                sotvault::sotvault_check_update,
                sotvault::sotvault_apply_update,
                sotvault::sotvault_accept_current,
                git_history::git_file_log,
                git_history::git_file_show,
                git_history::git_file_at,
                git_history::git_diff_current,
                preview_window::open_preview_tab,
                preview_window::drain_preview_tabs,
                write_file_binary,
                rename_file,
                open_plugin_market_window,
                open_daily_notes_window,
                set_daily_notes_enabled,
                editor_show_and_open_path,
                editor_open_remote_buffer,
                update_recent_menu,
                set_menu_locale,
                file_exists,
                dbg_log,
                shared_config_read,
                shared_config_write,
                log_bus::logs_append_frontend,
                log_bus::logs_get_snapshot,
                log_bus::logs_clear,
            ] }
            #[cfg(target_os = "ios")]
            { tauri::generate_handler![
                drain_pending_files,
                plugin_host::get_plugin_manifests,
                plugin_host::get_all_plugin_manifests,
                plugin_host::plugin_is_enabled,
                plugin_host::invoke_plugin,
                vault_ios::vault_status,
                vault_ios::list_dir::vault_list_dir,
                vault_ios::vault_configure,
                vault_ios::vault_sync_now,
                vault_ios::vault_disconnect,
                write_file_binary,
                rename_file,
                editor_show_and_open_path,
                editor_open_remote_buffer,
                file_exists,
                dbg_log,
                shared_config_read,
                shared_config_write,
            ] }
        })
        .setup(|app| {
            log_bus::init(app.handle().clone());

            // Register the system-wide quick-note hotkey (Cmd+Ctrl+N). A failure
            // (e.g. the combo is already claimed by another app) is non-fatal:
            // the tray "New Markdown" item still works.
            #[cfg(not(target_os = "ios"))]
            {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                if let Err(e) = app.global_shortcut().register("Cmd+Ctrl+N") {
                    dlog(&format!("global-shortcut register Cmd+Ctrl+N failed: {e}"));
                }
            }
            // Dev builds: drop the webview HTTP cache on every launch. Vite's
            // optimized-deps URLs (`?v=<hash>`) are served `immutable`, but the
            // hash only tracks the lockfile — file:-linked @moraya/core content
            // changes keep the same URL, so WKWebView would pin a stale bundle
            // forever (pnpm sync:core alone can't fix that side).
            #[cfg(dev)]
            {
                use tauri::Manager;
                for (_label, w) in app.webview_windows() {
                    let _ = w.clear_all_browsing_data();
                }
            }

            // Migrate legacy vault_sync.repo_path to shared config sotvault
            {
                if let (Ok(app_data_dir), Ok(shared)) = (
                    app.path().app_data_dir(),
                    crate::shared_config::config_path(),
                ) {
                    let legacy_store = app_data_dir.join("settings.json");
                    let _ = crate::shared_config::migrate_vault_sync_repo_to_shared(&shared, &legacy_store);
                }
            }

            #[cfg(not(target_os = "ios"))]
            {
                let vault_mgr = std::sync::Arc::new(vault_sync::VaultSyncManager::new());
                app.manage(vault_mgr);
                vault_sync::init(&app.handle());
                agents_sync::init(&app.handle());
            }

            // plugin_host MUST run before any code that calls is_plugin_enabled.
            plugin_host::init(&app.handle());
            #[cfg(not(target_os = "ios"))]
            plugin_runtime::init(&app.handle());
            // Request location authorization at launch (macOS): the prompt then
            // appears reliably, and the Position Log plugin can use it afterwards.
            #[cfg(not(target_os = "ios"))]
            plugin_runtime::location::init_at_startup(&app.handle());


            #[cfg(target_os = "ios")]
            vault_ios::init(&app.handle());

            #[cfg(not(target_os = "ios"))]
            {
                // Bootstrap themes: ensure dirs exist, copy any missing built-ins,
                // and (re)compile every theme into .compiled/ so the frontend can
                // load fresh CSS without waiting on a separate compile pass.
                if let Err(e) = bootstrap_themes(&app.handle()) {
                    eprintln!("[themes] bootstrap failed: {e}");
                }

                *app.state::<DailyNotesEnabled>().0.lock().unwrap() = read_daily_notes_enabled(&app.handle());
                let menu_locale = read_saved_locale(&app.handle());
                let plugin_items = plugin_host::collect_top_menu_items(&menu_locale);
                let (menu, recent_submenu) = build_menu(&app.handle(), &plugin_items, &menu_locale)?;
                *app.state::<RecentMenu>().0.lock().unwrap() = Some(recent_submenu);
                app.set_menu(menu)?;
                app.on_menu_event(|app, event| {
                    if event.id().0.as_str() == "hide-app" {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.hide();
                        }
                        return;
                    }
                    if event.id().0.as_str() == "open-insights" {
                        show_insights_window(app);
                        return;
                    }
                    if event.id().0.as_str() == "open-logs" {
                        open_logs_window(app, None);
                        return;
                    }
                    if event.id().0.as_str() == "open-plugin-market" {
                        show_plugin_market_window(app);
                        return;
                    }
                    let _ = app.emit("menu-event", event.id().0.as_str());
                });

                // Persistent menu-bar tray icon. White ✦ sparkle from the
                // note.md brand glyph (active state adds a green badge).
                // Left-click toggles main window visibility; right-click shows menu.
                let tray_icon = Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;
                let (tray_menu, sync_repo_item, status_item, sync_now_item) = build_tray_menu(&app.handle(), &menu_locale)?;
                {
                    let tray_item_state = app.state::<TrayRepoItem>();
                    *tray_item_state.0.lock().unwrap() = Some(sync_repo_item.clone());
                    let status_item_state = app.state::<TrayStatusItem>();
                    *status_item_state.0.lock().unwrap() = Some(status_item.clone());
                    *app.state::<TraySyncNowItem>().0.lock().unwrap() = Some(sync_now_item);
                }
                let _tray = TrayIconBuilder::with_id("main")
                    .icon(tray_icon)
                    .icon_as_template(false)
                    .tooltip("note.md")
                    .menu(&tray_menu)
                    .show_menu_on_left_click(true)
                    .on_menu_event(|app, event| {
                        let id = event.id().0.as_str();
                        match id {
                            "tray-new-quick" => trigger_quick_note(app),
                            "tray-show" => show_main_window(app),
                            "tray-daily-notes-open" => show_daily_notes_window(app),
                            id if id.starts_with("tray-plugin:") => {
                                // tray-plugin:<plugin_id>:<window> — plugin_id may
                                // contain dots but neither part contains a colon.
                                if let Some((plugin_id, window)) =
                                    id["tray-plugin:".len()..].rsplit_once(':')
                                {
                                    let _ = crate::plugin_runtime::windows::open_plugin_window(
                                        app, plugin_id, window,
                                    );
                                }
                            }
                            "tray-sync-repo" => { pick_sync_folder(app); }
                            "tray-sync-now" => { let _ = vault_sync::vault_sync_now(app.clone()); }
                            "tray-sync-log" => { open_logs_window(app, Some("git-sync")); }
                            "tray-edit-agents" => agents_sync::edit_agents_md(app),
                            "tray-quit" => app.exit(0),
                            id if id.starts_with("tray-large-file:") => {
                                if let Some(idx) = id.strip_prefix("tray-large-file:")
                                    .and_then(|s| s.parse::<usize>().ok())
                                {
                                    let mgr = app.state::<std::sync::Arc<vault_sync::VaultSyncManager>>();
                                    let files = mgr.skipped_large_files.lock().unwrap().clone();
                                    let repo = mgr.repo_path.lock().unwrap().clone();
                                    if let (Some(rel), Some(root)) = (files.get(idx), repo) {
                                        let abs = std::path::Path::new(&root).join(rel);
                                        let _ = std::process::Command::new("open").arg("-R").arg(abs).status();
                                    }
                                }
                            }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main_window(tray.app_handle());
                        }
                    })
                    .build(app)?;
            }

            // Initial CLI argv (Linux / Windows / macOS-when-launched-from-shell).
            // macOS Finder double-click does NOT arrive via argv — uses Apple Events
            // captured via `RunEvent::Opened` (handled in app.run() below) AND via
            // `tauri-plugin-deep-link`'s `on_open_url` (frontend-side belt-and-braces).
            let handle = app.handle();
            for arg in std::env::args().skip(1) {
                emit_open_file_delayed(handle, &arg);
            }

            Ok(())
        })
        .build(tauri_context())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        match event {
            RunEvent::Ready => {
                dlog("RunEvent::Ready");
            }
            RunEvent::Opened { urls } => {
                dlog(&format!("RunEvent::Opened {} urls: {:?}",
                    urls.len(),
                    urls.iter().map(|u| u.to_string()).collect::<Vec<_>>()));
                for url in urls {
                    if let Ok(path) = url.to_file_path() {
                        if let Some(p) = path.to_str() {
                            dlog(&format!("  emit open-file: {}", p));
                            emit_open_file_delayed(app_handle, p);
                        }
                    }
                }
            }
            #[cfg(not(target_os = "ios"))]
            RunEvent::Reopen { has_visible_windows, .. } => {
                dlog(&format!("RunEvent::Reopen has_visible_windows={}", has_visible_windows));
                // Always reveal the main window on dock reactivation. The
                // `has_visible_windows` guard mis-fired when the window was
                // hidden (close → hide), leaving no way to reopen. Showing an
                // already-visible window is a no-op.
                show_main_window(app_handle);
            }
            RunEvent::WindowEvent { ref label, event: ref e, .. } => {
                match e {
                    WindowEvent::CloseRequested { api, .. } => {
                        dlog(&format!("WindowEvent CloseRequested on {}", label));
                        api.prevent_close();
                        if let Some(w) = app_handle.get_webview_window(label) {
                            let _ = w.hide();
                        }
                    }
                    WindowEvent::Destroyed => {
                        dlog(&format!("WindowEvent Destroyed on {}", label));
                    }
                    _ => {}
                }
            }
            RunEvent::ExitRequested { code, api, .. } => {
                // Closing the window (user interaction, code None) hides to the
                // tray and keeps the app running; an explicit quit (tray "Quit" /
                // quit_app → app.exit(0), code Some) must proceed.
                if should_prevent_exit(code) {
                    api.prevent_exit();
                }
            }
            RunEvent::Exit => dlog("RunEvent::Exit"),
            _ => {}
        }
    });
}

#[cfg(not(target_os = "ios"))]
fn bootstrap_themes(app: &tauri::AppHandle) -> Result<(), String> {
    use themes::paths::{themes_dir, ensure_dirs};
    use themes::commands::BUILT_IN_THEME_IDS;

    ensure_dirs(app)?;
    let res_dir = app.path().resource_dir().map_err(|e| e.to_string())?.join("resources").join("themes");
    let themes = themes_dir(app)?;
    themes::migration::copy_built_ins_if_missing(&res_dir, &themes, BUILT_IN_THEME_IDS)?;
    let _ = themes::commands::theme_recompile_all(app.clone());
    Ok(())
}

fn emit_open_file_delayed<R: tauri::Runtime>(app: &tauri::AppHandle<R>, path: &str) {
    if let Some(state) = app.try_state::<PendingFiles>() {
        state.0.lock().unwrap().push(path.to_string());
    }
    let app = app.clone();
    let path = path.to_string();
    // Use a plain OS thread + sleep rather than tauri::async_runtime::spawn +
    // tokio::time::sleep. The async task's body never executed when invoked
    // from the single-instance / RunEvent::Opened callbacks (the "emit
    // open-file →" dlog never fired), so files opened while the app was already
    // running — `notemd <file>` re-launch, Finder double-click — never reached
    // the frontend. A raw thread has no dependency on Tauri's async runtime.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        dlog(&format!("emit open-file → {}", path));
        #[cfg(not(target_os = "ios"))]
        show_main_window(&app);
        let _ = app.emit("open-file", path);
    });
}

#[cfg(not(target_os = "ios"))]
#[cfg(not(target_os = "ios"))]
#[derive(serde::Deserialize)]
struct RecentMenuItem {
    index: usize,
    label: String,
}

/// Rebuild the File ▸ Open Recent submenu from the frontend's merged list.
/// Item ids are `open-recent:<index>`; clicks flow through the normal menu-event.
#[cfg(not(target_os = "ios"))]
#[tauri::command]
fn update_recent_menu(app: tauri::AppHandle, items: Vec<RecentMenuItem>) -> Result<(), String> {
    let state = app.state::<RecentMenu>();
    let guard = state.0.lock().unwrap();
    let submenu = guard.as_ref().ok_or("recent menu not initialized")?;

    // Clear existing items.
    loop {
        match submenu.remove_at(0) {
            Ok(Some(_)) => continue,
            _ => break,
        }
    }

    if items.is_empty() {
        let locale = read_saved_locale(&app);
        let placeholder = MenuItemBuilder::with_id("recent-none", menu_label(&locale, "file.noRecent"))
            .enabled(false)
            .build(&app)
            .map_err(|e| e.to_string())?;
        submenu.append(&placeholder).map_err(|e| e.to_string())?;
    } else {
        for it in items {
            let mi = MenuItemBuilder::with_id(format!("open-recent:{}", it.index), it.label)
                .build(&app)
                .map_err(|e| e.to_string())?;
            submenu.append(&mi).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Native menu label catalog. Mirrors the JS i18n catalog for the handful of
/// custom menu strings (macOS-provided items like Undo/Copy/Quit localize
/// themselves). Unknown locales fall back to English.
fn menu_label(locale: &str, key: &str) -> String {
    let (en, zh, ja, de): (&str, &str, &str, &str) = match key {
        "app.about" => ("About note.md", "关于 note.md", "note.md について", "Über note.md"),
        "app.checkUpdates" => ("Check for Updates…", "检查更新…", "更新を確認…", "Nach Updates suchen…"),
        "app.preferences" => ("Preferences…", "偏好设置…", "環境設定…", "Einstellungen…"),
        "app.hide" => ("Hide note.md", "隐藏 note.md", "note.md を隠す", "note.md ausblenden"),
        "menu.file" => ("File", "文件", "ファイル", "Ablage"),
        "menu.edit" => ("Edit", "编辑", "編集", "Bearbeiten"),
        "menu.view" => ("View", "视图", "表示", "Darstellung"),
        "menu.window" => ("Window", "窗口", "ウインドウ", "Fenster"),
        "menu.help" => ("Help", "帮助", "ヘルプ", "Hilfe"),
        "menu.plugins" => ("Plugins", "插件", "プラグイン", "Plugins"),
        "file.openRecent" => ("Open Recent", "打开最近", "最近使ったファイルを開く", "Zuletzt geöffnet"),
        "file.noRecent" => ("No Recent Files", "无最近文件", "最近のファイルなし", "Keine letzten Dateien"),
        "file.new" => ("New", "新建", "新規", "Neu"),
        "file.open" => ("Open…", "打开…", "開く…", "Öffnen…"),
        "file.closeTab" => ("Close Tab", "关闭标签页", "タブを閉じる", "Tab schließen"),
        "file.save" => ("Save", "保存", "保存", "Speichern"),
        "file.saveAs" => ("Save As…", "另存为…", "名前を付けて保存…", "Speichern unter…"),
        "file.print" => ("Print…", "打印…", "プリント…", "Drucken…"),
        "file.import" => ("Import", "导入", "インポート", "Importieren"),
        "edit.find" => ("Find…", "查找…", "検索…", "Suchen…"),
        "edit.findReplace" => ("Find and Replace…", "查找和替换…", "検索と置換…", "Suchen und Ersetzen…"),
        "view.toggleMode" => ("Toggle Source / Rich", "切换源码 / 富文本", "ソース / リッチを切り替え", "Quelltext / Rich umschalten"),
        "view.insights" => ("Reading Insights…", "阅读洞察数据…", "リーディングインサイト…", "Leseeinblicke…"),
        "view.logs" => ("View Logs…", "查看日志…", "ログを表示…", "Protokolle anzeigen…"),
        "plugins.market" => ("Plugin Market…", "插件市场…", "プラグインマーケット…", "Plugin-Markt…"),
        "file.syncToVault" => ("Sync to Vault…", "同步到 Vault…", "Vault に同期…", "Mit Vault synchronisieren…"),
        "file.share" => ("Share Current File…", "分享当前文件…", "現在のファイルを共有…", "Aktuelle Datei teilen…"),
        "file.unshare" => ("Unshare Current File…", "取消分享当前文件…", "現在のファイルの共有を解除…", "Freigabe der aktuellen Datei aufheben…"),
        "file.copyShareLink" => ("Copy Share Link", "复制分享链接", "共有リンクをコピー", "Freigabe-Link kopieren"),
        "view.folderView" => ("Folder View", "文件夹视图", "フォルダビュー", "Ordneransicht"),
        "view.sidecarNotes" => ("Sidecar Notes View", "伴生笔记视图", "サイドカーノートビュー", "Begleitnotizen-Ansicht"),
        "view.history" => ("History View", "历史视图", "履歴ビュー", "Verlaufsansicht"),
        "window.zoomIn" => ("Zoom In", "放大", "拡大", "Vergrößern"),
        "window.zoomOut" => ("Zoom Out", "缩小", "縮小", "Verkleinern"),
        "window.actualSize" => ("Actual Size", "实际大小", "実際のサイズ", "Originalgröße"),
        "help.docs" => ("Documentation", "文档", "ドキュメント", "Dokumentation"),
        "help.cliInstall" => (
            "Install 'notemd' Command in PATH…",
            "将 'notemd' 命令安装到 PATH…",
            "'notemd' コマンドを PATH にインストール…",
            "'notemd'-Befehl in PATH installieren…",
        ),
        "help.cliUninstall" => (
            "Uninstall 'notemd' Command",
            "卸载 'notemd' 命令",
            "'notemd' コマンドをアンインストール",
            "'notemd'-Befehl deinstallieren",
        ),
        // System / framework items (text overrides for PredefinedMenuItem, so
        // they follow the in-app locale instead of the macOS system language).
        "sys.services" => ("Services", "服务", "サービス", "Dienste"),
        "sys.hideOthers" => ("Hide Others", "隐藏其他", "ほかを隠す", "Andere ausblenden"),
        "sys.showAll" => ("Show All", "全部显示", "すべてを表示", "Alle einblenden"),
        "sys.quit" => ("Quit note.md", "退出 note.md", "note.md を終了", "note.md beenden"),
        "sys.undo" => ("Undo", "撤销", "取り消す", "Widerrufen"),
        "sys.redo" => ("Redo", "重做", "やり直す", "Wiederholen"),
        "sys.cut" => ("Cut", "剪切", "カット", "Ausschneiden"),
        "sys.copy" => ("Copy", "拷贝", "コピー", "Kopieren"),
        "sys.paste" => ("Paste", "粘贴", "ペースト", "Einsetzen"),
        "sys.selectAll" => ("Select All", "全选", "すべてを選択", "Alles auswählen"),
        "sys.minimize" => ("Minimize", "最小化", "しまう", "Im Dock ablegen"),
        "sys.maximize" => ("Zoom", "缩放", "拡大／縮小", "Größe anpassen"),
        // Menu-bar tray dropdown
        "tray.newQuick" => ("New Markdown", "新建 Markdown", "新規 Markdown", "Neues Markdown"),
        "tray.show" => ("Show note.md", "显示 note.md", "note.md を表示", "note.md anzeigen"),
        "tray.dailyNotes" => ("Daily Notes", "每日笔记", "デイリーノート", "Tagesnotizen"),
        "tray.vaultSetFolder" => ("Vault: Set Folder…", "Vault：选择文件夹…", "Vault：フォルダを選択…", "Vault: Ordner wählen…"),
        "tray.syncNow" => ("Sync Now", "立即同步", "今すぐ同期", "Jetzt synchronisieren"),
        "tray.largeFiles.title" => ("⚠️ {n} file(s) too large", "⚠️ {n} 个文件过大", "⚠️ {n} 件のファイルが大きすぎます", "⚠️ {n} Datei(en) zu groß"),
        "tray.largeFiles.header" => ("Over the limit — not synced. Move out of the vault:", "超过上限,未同步。请移出 vault:", "上限超過 —— 未同期。vault から移動してください:", "Über dem Limit — nicht synchronisiert. Aus dem Vault verschieben:"),
        "tray.viewLog" => ("View Log…", "查看日志…", "ログを表示…", "Protokoll anzeigen…"),
        "tray.editAgents" => ("Edit AGENTS.md…", "编辑 AGENTS.md…", "AGENTS.md を編集…", "AGENTS.md bearbeiten…"),
        // Sync status line / tooltip
        "sync.label" => ("Sync", "同步", "同期", "Sync"),
        "sync.neverSynced" => ("never synced", "从未同步", "未同期", "noch nie synchronisiert"),
        "sync.state.notConfigured" => ("Not configured", "未配置", "未設定", "Nicht konfiguriert"),
        "sync.state.stopped" => ("Stopped", "已停止", "停止中", "Gestoppt"),
        "sync.state.running" => ("Running", "运行中", "実行中", "Läuft"),
        "sync.state.syncing" => ("Syncing…", "同步中…", "同期中…", "Synchronisiert…"),
        "sync.state.conflict" => ("Conflict — needs attention", "有冲突 — 需处理", "競合 — 要対応", "Konflikt — Eingriff nötig"),
        "sync.state.error" => ("Error", "出错", "エラー", "Fehler"),
        "sync.state.gitUnavailable" => ("Git unavailable", "Git 不可用", "Git 利用不可", "Git nicht verfügbar"),
        _ => (key, key, key, key),
    };
    match locale {
        "zh" => zh,
        "ja" => ja,
        "de" => de,
        _ => en,
    }
    .to_string()
}

/// Best-effort read of the persisted UI locale from the store file so the
/// native menu can be built in the right language at startup. Falls back to
/// English if the file is missing/unreadable or the value is unknown.
/// pub(crate): plugin_runtime::commands reuses it for v2 InitializeParams.
pub(crate) fn read_saved_locale<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> String {
    use tauri::Manager;
    let path = match app.path().app_config_dir() {
        Ok(dir) => dir.join("settings.json"),
        Err(_) => return "en".to_string(),
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return "en".to_string(),
    };
    let json: serde_json::Value = match serde_json::from_str(&text) {
        Ok(j) => j,
        Err(_) => return "en".to_string(),
    };
    match json.get("locale").and_then(|v| v.as_str()) {
        Some(l @ ("en" | "zh" | "ja" | "de")) => l.to_string(),
        _ => "en".to_string(),
    }
}

#[cfg(not(target_os = "ios"))]
pub(crate) fn read_daily_notes_enabled<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> bool {
    use tauri::Manager;
    let Ok(dir) = app.path().app_config_dir() else { return false };
    let Ok(text) = std::fs::read_to_string(dir.join("settings.json")) else { return false };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return false };
    json.get("dailyNotes").and_then(|v| v.get("enabled")).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Build the menu-bar tray dropdown in the given locale. Returns the menu, the
/// (dynamic) "Vault:" item, status item, and sync-now item so the caller can
/// stash them for later updates. Event handling stays on the TrayIcon, so
/// rebuilding just the menu preserves click behavior.
/// A plugin's display name for the current locale: `i18n.<locale>.name` if the
/// manifest provides it, else the manifest `name`.
fn plugin_display_name(m: &plugin_protocol::ManifestV2, locale: &str) -> String {
    m.i18n
        .as_ref()
        .and_then(|v| v.get(locale))
        .and_then(|l| l.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| m.name.clone())
}

fn build_tray_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    locale: &str,
) -> tauri::Result<(Menu<R>, MenuItem<R>, IconMenuItem<R>, MenuItem<R>)> {
    // Quick-note action pinned to the very top of the tray dropdown. The
    // accelerator string is display-only here (the real system-wide hotkey is
    // registered via tauri-plugin-global-shortcut in `run()`); both funnel into
    // the same `quick-note` event on the frontend.
    let new_quick_item = MenuItem::with_id(app, "tray-new-quick", menu_label(locale, "tray.newQuick"), true, Some("Cmd+Ctrl+N"))?;
    let show_item = MenuItem::with_id(app, "tray-show", menu_label(locale, "tray.show"), true, None::<&str>)?;
    // Tray "socket": every enabled plugin that declares `contributes.tray` gets a
    // launch item here, directly below the "Daily Notes" item (when enabled). The
    // label is the entry's `label` or the plugin's localized name; clicking opens
    // the plugin window.
    let plugin_tray_items: Vec<MenuItem<R>> = {
        let mut entries: Vec<(String, String, String)> = Vec::new(); // (plugin_id, window, label)
        if let Ok(st) = crate::plugin_runtime::STATE.read() {
            if st.enabled_flag {
                for (id, (manifest, _dir)) in st.plugins.iter() {
                    for tc in &manifest.contributes.tray {
                        let label = tc
                            .label
                            .clone()
                            .unwrap_or_else(|| plugin_display_name(manifest, locale));
                        entries.push((id.clone(), tc.window.clone(), label));
                    }
                }
            }
        }
        entries.sort_by(|a, b| a.2.cmp(&b.2)); // stable order by label
        entries
            .into_iter()
            .filter_map(|(id, window, label)| {
                MenuItem::with_id(app, format!("tray-plugin:{id}:{window}"), &label, true, None::<&str>).ok()
            })
            .collect()
    };
    let sync_repo_label = {
        let mgr = app.state::<std::sync::Arc<vault_sync::VaultSyncManager>>();
        let guard = mgr.repo_path.lock().unwrap();
        match guard.as_deref() {
            Some(p) => format!("Vault: {}", abbreviate_path(p)),
            None => menu_label(locale, "tray.vaultSetFolder"),
        }
    };
    let sync_repo_item = MenuItem::with_id(app, "tray-sync-repo", &sync_repo_label, true, None::<&str>)?;
    let (status_label, status_dot) = {
        let mgr = app.state::<std::sync::Arc<vault_sync::VaultSyncManager>>();
        let state = *mgr.state.lock().unwrap();
        let last_sync = mgr.last_sync.lock().unwrap().clone();
        let has_large = !mgr.skipped_large_files.lock().unwrap().is_empty();
        let label = format!(
            "{} · {}",
            state_label(locale, state),
            last_sync_phrase(locale, last_sync.as_deref()),
        );
        (label, status_dot_image(state, has_large))
    };
    // Informational (disabled) status line with a flat colored dot icon so the
    // dropdown always shows health in a style that harmonizes with the menu font.
    let status_item = IconMenuItem::with_id(
        app,
        "tray-sync-status",
        &status_label,
        /*enabled=*/ false,
        status_dot,
        None::<&str>,
    )?;

    // Large-file warning submenu (only shown when files were skipped).
    let large_files: Vec<String> = {
        let mgr = app.state::<std::sync::Arc<vault_sync::VaultSyncManager>>();
        let x = mgr.skipped_large_files.lock().unwrap().clone();
        x
    };
    let large_submenu = if large_files.is_empty() {
        None
    } else {
        let header = IconMenuItem::with_id(
            app, "tray-large-header",
            menu_label(locale, "tray.largeFiles.header"),
            /*enabled=*/ false, None, None::<&str>,
        )?;
        let title = menu_label(locale, "tray.largeFiles.title")
            .replace("{n}", &large_files.len().to_string());
        let mut sub = SubmenuBuilder::with_id(app, "tray-large-files", &title).item(&header);
        for (i, f) in large_files.iter().enumerate() {
            let name = std::path::Path::new(f)
                .file_name().and_then(|s| s.to_str()).unwrap_or(f);
            let it = MenuItem::with_id(
                app, &format!("tray-large-file:{i}"), name, true, None::<&str>,
            )?;
            sub = sub.item(&it);
        }
        Some(sub.build()?)
    };

    let sync_now_item = MenuItem::with_id(app, "tray-sync-now", menu_label(locale, "tray.syncNow"), true, None::<&str>)?;
    let sync_log_item = MenuItem::with_id(app, "tray-sync-log", menu_label(locale, "tray.viewLog"), true, None::<&str>)?;
    let edit_agents_item = MenuItem::with_id(app, "tray-edit-agents", menu_label(locale, "tray.editAgents"), true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "tray-quit", menu_label(locale, "sys.quit"), true, None::<&str>)?;
    let daily_enabled = app.try_state::<DailyNotesEnabled>()
        .map(|st| *st.0.lock().unwrap())
        .unwrap_or(false);
    let daily_notes_item = MenuItem::with_id(app, "tray-daily-notes-open", menu_label(locale, "tray.dailyNotes"), true, None::<&str>)?;
    let mut b0 = MenuBuilder::new(app).item(&new_quick_item).separator().item(&show_item);
    // Daily Notes window item only when the feature is enabled; there is no
    // built-in "today's note" tray item anymore (removed by product decision).
    if daily_enabled {
        b0 = b0.item(&daily_notes_item);
    }
    for it in &plugin_tray_items {
        b0 = b0.item(it);
    }
    let b = b0.separator();
    let mut b2 = b
        .item(&sync_repo_item)
        .item(&status_item);
    if let Some(ref sm) = large_submenu {
        b2 = b2.item(sm);
    }
    let menu = b2
        .item(&sync_now_item)
        .item(&sync_log_item)
        .item(&edit_agents_item)
        .separator()
        .item(&quit_item)
        .build()?;
    Ok((menu, sync_repo_item, status_item, sync_now_item))
}

/// Rebuild the app menu (and tray) in the given locale and apply them. Called
/// from JS when the user changes the language. The recent-files submenu resets
/// to its placeholder; JS re-pushes the list via `refreshRecentMenu()` after.
#[tauri::command]
fn set_menu_locale(app: tauri::AppHandle, locale: String) -> Result<(), String> {
    // Propagate the language switch to every open plugin window (isolated
    // webviews with a build-time-injected locale). Done at the base so no plugin
    // needs its own locale-refresh code.
    plugin_runtime::windows::refresh_plugin_windows_locale(&app, &locale);
    apply_menu_locale(&app, &locale)
}

/// The menu/tray rebuild core: rebuild both in `locale`, store the fresh submenu
/// handles in app state, and apply them. `collect_top_menu_items` includes the
/// adapted v2 plugin manifests from `plugin_runtime::STATE`, so a plugin that was
/// just installed/uninstalled/toggled contributes (or stops contributing) its
/// native menu item without a restart.
///
/// **Thread safety**: Tauri 2's menu APIs (`MenuItemBuilder::build`,
/// `AppHandle::set_menu`) must run on the main thread on macOS. The sync
/// `set_menu_locale` command already runs there (Tauri dispatches sync commands
/// on the main thread), but the async market commands do NOT — they call
/// [`rebuild_menu`], which hops onto the main thread via `run_on_main_thread`.
fn apply_menu_locale(app: &tauri::AppHandle, locale: &str) -> Result<(), String> {
    let plugin_items = plugin_host::collect_top_menu_items(locale);
    let (menu, recent_submenu) =
        build_menu(app, &plugin_items, locale).map_err(|e| e.to_string())?;
    *app.state::<RecentMenu>().0.lock().unwrap() = Some(recent_submenu);
    app.set_menu(menu).map_err(|e| e.to_string())?;

    // Rebuild the tray dropdown too (event handling lives on the TrayIcon).
    if let Some(tray) = app.tray_by_id("main") {
        let (tray_menu, sync_repo_item, status_item, sync_now_item) =
            build_tray_menu(app, locale).map_err(|e| e.to_string())?;
        *app.state::<TrayRepoItem>().0.lock().unwrap() = Some(sync_repo_item);
        *app.state::<TrayStatusItem>().0.lock().unwrap() = Some(status_item);
        *app.state::<TraySyncNowItem>().0.lock().unwrap() = Some(sync_now_item);
        tray.set_menu(Some(tray_menu)).map_err(|e| e.to_string())?;
    }
    // Re-apply live status text/icon after the menu handles were replaced.
    #[cfg(not(target_os = "ios"))]
    refresh_tray_status(app);
    Ok(())
}

/// Rebuild the native menu after the installed-plugin set changed (install /
/// uninstall / enable-disable). Reads the saved locale, then rebuilds the menu
/// on the main thread. Safe to call from an async Tauri command (which runs off
/// the main thread) — it dispatches the rebuild via `run_on_main_thread`. Any
/// rebuild error is logged, not propagated, so a menu hiccup never fails the
/// install the user just completed.
#[cfg(not(target_os = "ios"))]
pub(crate) fn rebuild_menu(app: &tauri::AppHandle) {
    let locale = read_saved_locale(app);
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        if let Err(err) = apply_menu_locale(&handle, &locale) {
            eprintln!("[plugin_runtime] menu rebuild failed: {err}");
        }
    }) {
        eprintln!("[plugin_runtime] could not dispatch menu rebuild to main thread: {e}");
    }
}

fn build_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    plugin_items: &[plugin_host::LocatedMenuItem],
    locale: &str,
) -> tauri::Result<(Menu<R>, Submenu<R>)> {
    let app_meta = AboutMetadata {
        name: Some("note.md".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        ..Default::default()
    };

    let app_menu: Submenu<R> = SubmenuBuilder::new(app, "note.md")
        .item(&PredefinedMenuItem::about(app, Some(&menu_label(locale, "app.about")), Some(app_meta))?)
        .item(
            &MenuItemBuilder::with_id("check-for-updates", menu_label(locale, "app.checkUpdates"))
                .build(app)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id("preferences", menu_label(locale, "app.preferences"))
                .accelerator("Cmd+,")
                .build(app)?,
        )
        .separator()
        .item(&PredefinedMenuItem::services(app, Some(&menu_label(locale, "sys.services")))?)
        .separator()
        .item(&MenuItemBuilder::with_id("hide-app", menu_label(locale, "app.hide")).accelerator("Cmd+Shift+H").build(app)?)
        .item(&PredefinedMenuItem::hide_others(app, Some(&menu_label(locale, "sys.hideOthers")))?)
        .item(&PredefinedMenuItem::show_all(app, Some(&menu_label(locale, "sys.showAll")))?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some(&menu_label(locale, "sys.quit")))?)
        .build()?;

    let recent_menu: Submenu<R> = SubmenuBuilder::new(app, menu_label(locale, "file.openRecent"))
        .item(
            &MenuItemBuilder::with_id("recent-none", menu_label(locale, "file.noRecent"))
                .enabled(false)
                .build(app)?,
        )
        .build()?;

    let file_b = SubmenuBuilder::new(app, menu_label(locale, "menu.file"))
        .item(&MenuItemBuilder::with_id("new", menu_label(locale, "file.new")).accelerator("Cmd+N").build(app)?)
        .item(&MenuItemBuilder::with_id("open", menu_label(locale, "file.open")).accelerator("Cmd+O").build(app)?)
        .item(&recent_menu)
        .separator()
        .item(
            &MenuItemBuilder::with_id("close-tab", menu_label(locale, "file.closeTab"))
                .accelerator("Cmd+W")
                .build(app)?,
        )
        .separator()
        .item(&MenuItemBuilder::with_id("save", menu_label(locale, "file.save")).accelerator("Cmd+S").build(app)?)
        .item(
            &MenuItemBuilder::with_id("save-as", menu_label(locale, "file.saveAs"))
                .accelerator("Cmd+Shift+S")
                .build(app)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id("print", menu_label(locale, "file.print"))
                .accelerator("Cmd+P")
                .build(app)?,
        )
        .separator()
        .item(&MenuItemBuilder::with_id("sync-to-vault", menu_label(locale, "file.syncToVault")).build(app)?)
        .item(
            &MenuItemBuilder::with_id("share", menu_label(locale, "file.share"))
                .accelerator("Cmd+Shift+L")
                .build(app)?,
        )
        .item(&MenuItemBuilder::with_id("unshare", menu_label(locale, "file.unshare")).build(app)?)
        .item(&MenuItemBuilder::with_id("copy-share-link", menu_label(locale, "file.copyShareLink")).build(app)?);
    // Plugin-contributed menu items no longer scatter into File/Edit/View/etc.
    // — they ALL live under the Plugins menu (built below), so there is one
    // predictable, discoverable home for every plugin command. Core features
    // keep their native menu slots.
    let file_menu: Submenu<R> = file_b.build()?;

    let edit_b = SubmenuBuilder::new(app, menu_label(locale, "menu.edit"))
        .item(&PredefinedMenuItem::undo(app, Some(&menu_label(locale, "sys.undo")))?)
        .item(&PredefinedMenuItem::redo(app, Some(&menu_label(locale, "sys.redo")))?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some(&menu_label(locale, "sys.cut")))?)
        .item(&PredefinedMenuItem::copy(app, Some(&menu_label(locale, "sys.copy")))?)
        .item(&PredefinedMenuItem::paste(app, Some(&menu_label(locale, "sys.paste")))?)
        .item(&PredefinedMenuItem::select_all(app, Some(&menu_label(locale, "sys.selectAll")))?)
        .separator()
        .item(&MenuItemBuilder::with_id("find", menu_label(locale, "edit.find")).accelerator("Cmd+F").build(app)?)
        .item(&MenuItemBuilder::with_id("find-replace", menu_label(locale, "edit.findReplace")).build(app)?);
    let edit_menu: Submenu<R> = edit_b.build()?;

    let view_b = SubmenuBuilder::new(app, menu_label(locale, "menu.view"))
        .item(
            &MenuItemBuilder::with_id("toggle-mode", menu_label(locale, "view.toggleMode"))
                .accelerator("Cmd+/")
                .build(app)?,
        )
        .item(&PredefinedMenuItem::fullscreen(app, None)?)
        .separator()
        .item(&MenuItemBuilder::with_id("open-insights", menu_label(locale, "view.insights")).build(app)?)
        .item(&MenuItemBuilder::with_id("open-logs", menu_label(locale, "view.logs")).build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("toggle-folder-view", menu_label(locale, "view.folderView")).accelerator("Cmd+Shift+E").build(app)?)
        .item(&MenuItemBuilder::with_id("toggle-sidecar-notes", menu_label(locale, "view.sidecarNotes")).accelerator("Cmd+Shift+O").build(app)?)
        .item(&MenuItemBuilder::with_id("toggle-git-history", menu_label(locale, "view.history")).accelerator("Cmd+Shift+Y").build(app)?);
    let view_menu: Submenu<R> = view_b.build()?;

    let window_b = SubmenuBuilder::new(app, menu_label(locale, "menu.window"))
        .item(&PredefinedMenuItem::minimize(app, Some(&menu_label(locale, "sys.minimize")))?)
        .item(&PredefinedMenuItem::maximize(app, Some(&menu_label(locale, "sys.maximize")))?)
        .separator()
        .item(&MenuItemBuilder::with_id("zoom-in", menu_label(locale, "window.zoomIn")).accelerator("Cmd+=").build(app)?)
        .item(&MenuItemBuilder::with_id("zoom-out", menu_label(locale, "window.zoomOut")).accelerator("Cmd+-").build(app)?)
        .item(&MenuItemBuilder::with_id("zoom-reset", menu_label(locale, "window.actualSize")).accelerator("Cmd+0").build(app)?);
    let window_menu: Submenu<R> = window_b.build()?;

    let help_b = SubmenuBuilder::new(app, menu_label(locale, "menu.help"))
        .item(&MenuItemBuilder::with_id("docs", menu_label(locale, "help.docs")).build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id("cli-install", menu_label(locale, "help.cliInstall")).build(app)?)
        .item(&MenuItemBuilder::with_id("cli-uninstall", menu_label(locale, "help.cliUninstall")).build(app)?);
    let help_menu: Submenu<R> = help_b.build()?;

    // The Plugins menu is always present: its first item, "Plugin Market…",
    // is the entry point to browse / install / update / uninstall plugins
    // (opens the standalone market window). EVERY plugin-contributed menu item
    // follows after a separator, regardless of its declared `location` — the
    // Plugins menu is the single home for all plugin commands.
    let plugins_menu: Submenu<R> = {
        let mut b = SubmenuBuilder::new(app, menu_label(locale, "menu.plugins")).item(
            &MenuItemBuilder::with_id("open-plugin-market", menu_label(locale, "plugins.market"))
                .build(app)?,
        );
        let contributed: Vec<_> = plugin_items.iter().collect();
        if !contributed.is_empty() {
            b = b.separator();
            for it in contributed {
                let mut mb = MenuItemBuilder::with_id(&it.id, &it.label);
                if let Some(s) = &it.shortcut { mb = mb.accelerator(s); }
                b = b.item(&mb.build(app)?);
            }
        }
        b.build()?
    };

    // Suppress unused warning when WindowEvent isn't matched in run loop above
    let _ = std::any::type_name::<WindowEvent>();

    let menu = MenuBuilder::new(app)
        .items(&[&app_menu, &file_menu, &edit_menu, &view_menu, &plugins_menu, &window_menu, &help_menu])
        .build()?;
    Ok((menu, recent_menu))
}

#[cfg(all(test, not(target_os = "ios")))]
mod exit_tests {
    use super::should_prevent_exit;

    #[test]
    fn explicit_quit_is_not_prevented() {
        // Tray "Quit" and the quit_app command both call app.exit(0) → code Some.
        assert!(!should_prevent_exit(Some(0)));
        assert!(!should_prevent_exit(Some(1)));
    }

    #[test]
    fn window_close_keeps_app_in_tray() {
        // User-interaction exit (last window closing) → code None → stay alive.
        assert!(should_prevent_exit(None));
    }
}
