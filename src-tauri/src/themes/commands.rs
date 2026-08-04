use crate::themes::compiler::compile_theme_css;
use crate::themes::import::{prepare_import, install_prepared, cleanup_staging, ImportReport};
use crate::themes::paths::{compiled_path, compiled_dir, ensure_dirs, source_path, themes_dir, asset_dir};
use crate::themes::registry::{scan_themes_dir, ThemeMeta};
use tauri::{Emitter, Manager};

/// Ids of the themes we ship with the app. Used for the `built_in` flag and
/// the "Restore built-in themes" affordance.
pub const BUILT_IN_THEME_IDS: &[&str] = &["default", "effie"];

#[tauri::command]
pub fn theme_list(app: tauri::AppHandle) -> Result<Vec<ThemeMeta>, String> {
    ensure_dirs(&app)?;
    let dir = themes_dir(&app)?;
    scan_themes_dir(&dir, BUILT_IN_THEME_IDS)
}

#[tauri::command]
pub fn theme_reveal(app: tauri::AppHandle) -> Result<(), String> {
    ensure_dirs(&app)?;
    let dir = themes_dir(&app)?;
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    { let _ = dir; Err("not supported on this platform".into()) }
}

/// Read the compiled CSS for theme `id` from disk and return it. Used by the
/// frontend theme-loader to populate <style> slots without needing
/// tauri-plugin-fs scope permission for the app-data directory.
#[tauri::command]
pub fn theme_load_compiled(app: tauri::AppHandle, id: String) -> Result<String, String> {
    let path = compiled_path(&app, &id)?;
    std::fs::read_to_string(&path).map_err(|e| format!("read {path:?}: {e}"))
}

#[tauri::command]
pub fn theme_recompile(app: tauri::AppHandle, id: String) -> Result<(), String> {
    ensure_dirs(&app)?;
    let source = source_path(&app, &id)?;
    let compiled = compiled_path(&app, &id)?;
    let assets = asset_dir(&app, &id)?;
    let src = std::fs::read_to_string(&source).map_err(|e| e.to_string())?;
    let out = compile_theme_css(&src, &id, assets.to_str().unwrap_or(""))?;
    std::fs::write(&compiled, out).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn theme_recompile_all(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    ensure_dirs(&app)?;
    let list = theme_list(app.clone())?;
    let mut errs: Vec<String> = Vec::new();
    for meta in list {
        if let Err(e) = theme_recompile(app.clone(), meta.id.clone()) {
            errs.push(format!("{}: {e}", meta.id));
        }
    }
    Ok(errs)
}

#[tauri::command]
pub fn theme_restore_builtins(app: tauri::AppHandle) -> Result<usize, String> {
    use crate::themes::migration::force_copy_built_ins;
    ensure_dirs(&app)?;
    let res_dir = app.path().resource_dir().map_err(|e| e.to_string())?.join("resources").join("themes");
    let themes = themes_dir(&app)?;
    let n = force_copy_built_ins(&res_dir, &themes, BUILT_IN_THEME_IDS)?;
    // Recompile so the .compiled/ cache reflects the restored sources.
    let _ = theme_recompile_all(app.clone());
    Ok(n)
}

#[tauri::command]
pub fn theme_import(app: tauri::AppHandle, zip_path: String) -> Result<ImportReport, String> {
    let existing: Vec<String> = theme_list(app)?.into_iter().map(|m| m.id).collect();
    prepare_import(std::path::Path::new(&zip_path), &existing)
}

#[tauri::command]
pub fn theme_install(app: tauri::AppHandle, report: ImportReport, overwrite: bool) -> Result<usize, String> {
    let dir = themes_dir(&app)?;
    let n = install_prepared(&report, &dir, overwrite)?;
    let _ = app.emit("themes-updated", ());
    Ok(n)
}

#[tauri::command]
pub fn theme_cancel_import(_app: tauri::AppHandle, staging_dir: String) {
    cleanup_staging(std::path::Path::new(&staging_dir));
}

// ── Theme CSS bundle for isolated plugin webviews (Editor Kit) ────────────
//
// A plugin window is an isolated webview: it never receives the <style> slots
// the main frontend injects, so the Editor Kit asks the host for the compiled
// CSS over the `host.theme.css` bridge method (capability `editor.kit`).

/// Theme id used whenever settings.json says nothing usable.
const DEFAULT_THEME_ID: &str = "default";

/// `settings.json` → `(light_id, dark_id, follow_system)`.
///
/// Pure so the tolerance rules are unit-testable without an AppHandle. Shapes:
/// - `{"theme": {"light": …, "dark": …, "followSystem": …}}` — current form.
///   A missing/blank slot id falls back to `"default"`; `followSystem` is true
///   unless it is exactly `false` (same rule as `loadSettings` in
///   `src/lib/settings.svelte.ts`).
/// - `{"theme": "some-id"}` — the historical single-skin string: both slots get
///   that id and `follow_system` is false (there is no dark counterpart).
/// - key missing, or anything else (number/array/null/non-object root) →
///   `("default", "default", true)`. Never panics.
pub(crate) fn parse_theme_settings(settings: &serde_json::Value) -> (String, String, bool) {
    let fallback = || {
        (
            DEFAULT_THEME_ID.to_string(),
            DEFAULT_THEME_ID.to_string(),
            true,
        )
    };
    let Some(theme) = settings.get("theme") else {
        return fallback();
    };
    // Historical form: `theme` was a single skin id string.
    if let Some(id) = theme.as_str() {
        let id = sanitize_theme_id(Some(id));
        return (id.clone(), id, false);
    }
    if !theme.is_object() {
        return fallback();
    }
    let slot = |key: &str| sanitize_theme_id(theme.get(key).and_then(|v| v.as_str()));
    let follow = theme
        .get("followSystem")
        .map(|v| v.as_bool() != Some(false))
        .unwrap_or(true);
    (slot("light"), slot("dark"), follow)
}

/// Read `<app config dir>/settings.json` and hand its parsed value to
/// [`parse_theme_settings`]. A missing/unreadable/invalid file yields `Null`,
/// which the parser maps to the defaults.
fn read_theme_settings<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> (String, String, bool) {
    let value = app
        .path()
        .app_config_dir()
        .ok()
        .and_then(|dir| std::fs::read_to_string(dir.join("settings.json")).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .unwrap_or(serde_json::Value::Null);
    parse_theme_settings(&value)
}

/// `{light_css, dark_css, follow_system}` — the compiled CSS of both theme
/// slots, for a webview that cannot see the main window's <style> slots
/// (Editor Kit in an isolated plugin window; served as `host.theme.css`).
///
/// Read-only and total: a slot whose compiled artifact is missing (theme never
/// compiled, id since deleted) comes back as an empty string rather than an
/// error, because a themeless editor must still mount.
///
/// Deviation from the plan sketch: generic over `R: tauri::Runtime` because the
/// caller (`ui_rpc::dispatch`) is itself generic over the runtime.
pub fn theme_css_bundle<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> serde_json::Value {
    let (light_id, dark_id, follow) = read_theme_settings(app);
    let load = |id: &str| -> String {
        compiled_path(app, id)
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default()
    };
    serde_json::json!({
        "light_css": load(&light_id),
        "dark_css": load(&dark_id),
        "follow_system": follow,
    })
}

/// Keep only ids the theme store can actually address (`themes/id.rs` rules):
/// an absent, blank or malformed id — including a traversal attempt like
/// `../../secret` — degrades to `"default"` instead of reaching the filesystem.
fn sanitize_theme_id(id: Option<&str>) -> String {
    match id {
        Some(s) if crate::themes::id::is_valid_theme_id(s.trim()).is_ok() => s.trim().to_string(),
        _ => DEFAULT_THEME_ID.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_theme_settings_reads_the_object_shape() {
        let v = json!({"theme": {"light": "default", "dark": "effie", "followSystem": false}});
        assert_eq!(
            parse_theme_settings(&v),
            ("default".to_string(), "effie".to_string(), false)
        );
        let v = json!({"theme": {"light": "effie", "dark": "effie", "followSystem": true}});
        assert_eq!(
            parse_theme_settings(&v),
            ("effie".to_string(), "effie".to_string(), true)
        );
        // followSystem absent ⇒ true (frontend rule: anything but `false`).
        let v = json!({"theme": {"light": "effie", "dark": "default"}});
        assert_eq!(
            parse_theme_settings(&v),
            ("effie".to_string(), "default".to_string(), true)
        );
    }

    #[test]
    fn parse_theme_settings_accepts_the_legacy_string_shape() {
        let v = json!({"theme": "effie"});
        assert_eq!(
            parse_theme_settings(&v),
            ("effie".to_string(), "effie".to_string(), false)
        );
    }

    #[test]
    fn parse_theme_settings_defaults_when_the_key_is_missing() {
        assert_eq!(
            parse_theme_settings(&json!({})),
            ("default".to_string(), "default".to_string(), true)
        );
        assert_eq!(
            parse_theme_settings(&json!({"autoSave": true})),
            ("default".to_string(), "default".to_string(), true)
        );
        // No settings file at all → read_theme_settings passes Null.
        assert_eq!(
            parse_theme_settings(&serde_json::Value::Null),
            ("default".to_string(), "default".to_string(), true)
        );
    }

    #[test]
    fn parse_theme_settings_survives_malformed_shapes() {
        for v in [
            json!({"theme": 42}),
            json!({"theme": null}),
            json!({"theme": ["effie"]}),
            json!({"theme": true}),
            json!([1, 2, 3]),
            json!("not an object"),
        ] {
            assert_eq!(
                parse_theme_settings(&v),
                ("default".to_string(), "default".to_string(), true),
                "input: {v}"
            );
        }
        // Object shape with unusable slot values → per-slot default, no panic.
        let v = json!({"theme": {"light": 1, "dark": "", "followSystem": "yes"}});
        assert_eq!(
            parse_theme_settings(&v),
            ("default".to_string(), "default".to_string(), true)
        );
        // A traversal attempt never becomes a path.
        let v = json!({"theme": {"light": "../../etc/passwd", "dark": "Effie!"}});
        assert_eq!(
            parse_theme_settings(&v),
            ("default".to_string(), "default".to_string(), true)
        );
    }
}
