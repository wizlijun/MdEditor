use crate::themes::compiler::compile_theme_css;
use crate::themes::import::{prepare_import, install_prepared, cleanup_staging, ImportReport};
use crate::themes::paths::{compiled_path, ensure_dirs, source_path, themes_dir, asset_dir};
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
/// - `{"skin": "some-id"}` with no usable `theme` — the REAL pre-4517e63 shape
///   on disk: both slots get that id and `follow_system` is false (there was no
///   dark counterpart). The frontend migrates this in memory only, so until the
///   next `saveSettings` the host must read it too, or a plugin window would
///   show "default" while the main window shows the user's skin.
/// - `{"theme": "some-id"}` — same single-id treatment, defensively (this shape
///   was never persisted, but costs nothing to accept).
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
    // Both single-id legacy shapes resolve the same way; an unusable id is not
    // trusted (it would only name a file that cannot exist).
    let legacy = |raw: Option<&str>| match raw {
        Some(s) if is_usable_theme_id(s) => {
            let id = s.trim().to_string();
            Some((id.clone(), id, false))
        }
        _ => None,
    };

    if let Some(theme) = settings.get("theme") {
        // Defensive: `theme` as a bare string.
        if let Some(id) = theme.as_str() {
            return legacy(Some(id)).unwrap_or_else(fallback);
        }
        if let Some(obj) = theme.as_object() {
            let raw = |key: &str| obj.get(key).and_then(|v| v.as_str());
            // Mirrors the frontend's migration test (`typeof light/dark ===
            // 'string'`): an object carrying neither slot is not the new shape,
            // so fall through to the legacy key rather than claiming defaults.
            if raw("light").is_some() || raw("dark").is_some() {
                let follow = obj
                    .get("followSystem")
                    .map(|v| v.as_bool() != Some(false))
                    .unwrap_or(true);
                return (
                    sanitize_theme_id(raw("light")),
                    sanitize_theme_id(raw("dark")),
                    follow,
                );
            }
        }
    }
    legacy(settings.get("skin").and_then(|v| v.as_str())).unwrap_or_else(fallback)
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
        let css = compiled_path(app, id)
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        unscope_theme_css(&css, id)
    };
    theme_bundle_json(&load(&light_id), &load(&dark_id), follow)
}

/// Drop the `[data-theme="<id>"] ` disambiguator from every compiled selector,
/// leaving the bare `.moraya-editor` host.
///
/// Why: `compiler::rewrite_selector_text` prefixes every selector with
/// [`crate::themes::compiler::scope_prefix`] so the main window can hold both
/// theme slots at once and pick one via a `data-theme` ancestor. A plugin window
/// has no such ancestor (`src/editor-kit/main.ts` mounts a bare `.kit-host`
/// containing the `.moraya-editor`), so shipping the scoped CSS verbatim would
/// match NOTHING — a fully populated style slot with zero visual effect. That
/// dimension is meaningless there anyway: the kit's single slot holds exactly
/// one theme at a time.
///
/// Deterministic substring replacement of the prefix the compiler itself
/// generates (shared helper, so the two cannot drift) — not a regex guess.
pub(crate) fn unscope_theme_css(css: &str, theme_id: &str) -> String {
    let prefix = crate::themes::compiler::scope_prefix(theme_id);
    css.replace(&prefix, ".moraya-editor")
}

/// Assemble the `host.theme.css` reply. Its three key names are a CROSS-LANGUAGE
/// contract with `src/editor-kit/theme.ts`, where every field is optional and
/// falls back to `''` — i.e. a rename on either side degrades silently to an
/// unstyled editor. Kept as a pure fn so a unit test can pin the keys.
fn theme_bundle_json(light_css: &str, dark_css: &str, follow_system: bool) -> serde_json::Value {
    serde_json::json!({
        "light_css": light_css,
        "dark_css": dark_css,
        "follow_system": follow_system,
    })
}

/// Keep only ids the theme store can actually address (`themes/id.rs` rules):
/// an absent, blank or malformed id — including a traversal attempt like
/// `../../secret` — degrades to `"default"` instead of reaching the filesystem.
fn sanitize_theme_id(id: Option<&str>) -> String {
    match id {
        Some(s) if is_usable_theme_id(s) => s.trim().to_string(),
        _ => DEFAULT_THEME_ID.to_string(),
    }
}

/// `themes/id.rs` rules — the same validator `registry.rs` / `import.rs` use, so
/// this admits every id that can actually name a theme on disk and nothing else.
fn is_usable_theme_id(id: &str) -> bool {
    crate::themes::id::is_valid_theme_id(id.trim()).is_ok()
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

    /// The shape real pre-4517e63 vaults actually have on disk: a root-level
    /// `skin` string and NO `theme` key. Until the frontend's in-memory
    /// migration is persisted by a later `saveSettings`, this is what a plugin
    /// window would otherwise read as "default" while the main window shows
    /// the user's real skin.
    #[test]
    fn parse_theme_settings_falls_back_to_the_root_skin_key() {
        assert_eq!(
            parse_theme_settings(&json!({"skin": "effie", "autoSave": true})),
            ("effie".to_string(), "effie".to_string(), false)
        );
        // A usable `theme` always wins over the legacy key.
        assert_eq!(
            parse_theme_settings(&json!({
                "skin": "effie",
                "theme": {"light": "default", "dark": "onedark", "followSystem": true}
            })),
            ("default".to_string(), "onedark".to_string(), true)
        );
        // An unusable `skin` is ignored rather than trusted.
        for v in [json!({"skin": 42}), json!({"skin": ""}), json!({"skin": "../x"})] {
            assert_eq!(
                parse_theme_settings(&v),
                ("default".to_string(), "default".to_string(), true),
                "input: {v}"
            );
        }
    }

    /// C1: the compiled artifact scopes every selector behind a `data-theme`
    /// ancestor that a plugin window does not have. The prefix must be gone,
    /// and NOTHING else may change.
    #[test]
    fn unscope_theme_css_strips_the_scope_prefix_verbatim() {
        let scoped = concat!(
            "[data-theme=\"effie\"] .moraya-editor {\n  line-height: 1.6;\n}\n\n",
            "[data-theme=\"effie\"] .moraya-editor h1 {\n  font-size: 2em;\n}\n\n",
            "@media print {\n  [data-theme=\"effie\"] .moraya-editor p > a {\n    color: #000;\n  }\n}\n"
        );
        let expected = concat!(
            ".moraya-editor {\n  line-height: 1.6;\n}\n\n",
            ".moraya-editor h1 {\n  font-size: 2em;\n}\n\n",
            "@media print {\n  .moraya-editor p > a {\n    color: #000;\n  }\n}\n"
        );
        assert_eq!(unscope_theme_css(scoped, "effie"), expected);

        // Unrelated CSS is untouched, and a mismatched id is left alone (the
        // bundle always unscopes with the id it just read the file for).
        assert_eq!(unscope_theme_css("@font-face { src: url(x); }", "effie"), "@font-face { src: url(x); }");
        assert_eq!(unscope_theme_css(scoped, "onedark"), scoped);
        assert_eq!(unscope_theme_css("", "effie"), "");
    }

    /// The prefix stripped above is exactly the one the compiler emits — the
    /// two derive it from the same helper, so they cannot drift.
    #[test]
    fn unscope_theme_css_undoes_the_compilers_own_scoping() {
        let compiled = crate::themes::compiler::compile_theme_css(
            "/*\n * Theme Name: X\n */\n:root { --c: red; }\n#write h1 { color: var(--c); }",
            "effie",
            "/tmp/themes/effie",
        )
        .expect("compile ok");
        assert!(compiled.contains("[data-theme="), "precondition: compiled CSS is scoped");
        let out = unscope_theme_css(&compiled, "effie");
        assert!(!out.contains("[data-theme="), "scope survived: {out}");
        assert!(out.contains(".moraya-editor h1"), "host selector lost: {out}");
        assert!(out.contains("--c: red"), "declarations must be untouched: {out}");
    }

    /// C1 (bundle level) + I3: the wire shape of `host.theme.css`. The key
    /// names are a cross-language contract with `src/editor-kit/theme.ts`,
    /// where every field is optional — a rename on either side would be
    /// SILENT, so pin it here.
    #[test]
    fn theme_bundle_json_pins_the_wire_contract_with_the_kit() {
        let v = theme_bundle_json("/* light */", "/* dark */", true);
        let obj = v.as_object().expect("bundle must be a JSON object");
        assert_eq!(obj.len(), 3, "exactly three fields: {v}");
        assert_eq!(v["light_css"].as_str(), Some("/* light */"));
        assert_eq!(v["dark_css"].as_str(), Some("/* dark */"));
        assert_eq!(v["follow_system"].as_bool(), Some(true));
        assert_eq!(
            theme_bundle_json("", "", false)["follow_system"].as_bool(),
            Some(false)
        );

        // What `theme_css_bundle` actually ships (same composition): neither
        // CSS field may still carry the scope attribute.
        let scoped = "[data-theme=\"effie\"] .moraya-editor h1 { color: red; }";
        let shipped = theme_bundle_json(
            &unscope_theme_css(scoped, "effie"),
            &unscope_theme_css(scoped, "effie"),
            true,
        );
        for key in ["light_css", "dark_css"] {
            let css = shipped[key].as_str().unwrap();
            assert!(!css.contains("[data-theme="), "{key} still scoped: {css}");
            assert!(css.contains(".moraya-editor h1"), "{key} lost its host: {css}");
        }
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
