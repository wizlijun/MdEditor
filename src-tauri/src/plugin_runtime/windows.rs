//! Plugin windows (spec §7.2, 子项目② Task 4).
//!
//! Opens a plugin-contributed window loading `plugin://<id>/<entry>` (served by
//! [`super::protocol`]) with the fetch-RPC bridge injected as an initialization
//! script. Plugin windows are granted ZERO Tauri IPC (no capability entry) —
//! the `plugin://` protocol is their only channel to the host. Host→UI push
//! rides `WebviewWindow::eval("window.__notemd_dispatch(<json>)")`.
//!
//! Layering: [`window_label`], [`bridge_script`], and [`dispatch_eval`] are pure
//! string builders (unit-testable, no AppHandle); [`open_plugin_window`] and
//! [`push_to_window`] are the AppHandle-backed shells.

use serde_json::Value;
use tauri::{Manager, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent};

/// Window label: `plugin-<sanitized id>-<window id>`. Dots in the plugin id
/// become hyphens so the label stays in Tauri's safe label character set.
pub fn window_label(plugin_id: &str, window_id: &str) -> String {
    format!("plugin-{}-{}", plugin_id.replace('.', "-"), window_id)
}

/// JS injected as the window's initialization script. Defines the frozen
/// `window.notemd` bridge (fetch-RPC + host-push subscription) and the
/// `window.__notemd_dispatch` push entry point the host `eval`s into.
///
/// `plugin_id`/`locale`/`theme` are embedded as JSON literals via
/// `serde_json::to_string`, so any quoting/escaping is handled safely. The seq
/// counter and listeners array live inside an IIFE so nothing leaks beyond the
/// two intended globals.
///
/// **Idempotency guard.** The first line `if (window.notemd) return;` makes
/// re-running the script a no-op. Plugin *windows* receive it via
/// `initialization_script` (runs once per webview, before page scripts); the
/// `plugin://` protocol ALSO injects it into every served `text/html` response
/// (子项目④) so *iframes* — which never get an initialization_script (that is
/// per-webview-window, not per-iframe) — pick up the bridge too. A window thus
/// sees the script twice (init + injected); the guard makes the second run
/// harmless.
pub(crate) fn bridge_script(plugin_id: &str, locale: &str, theme: &str) -> String {
    // JSON string literals — safe to embed directly in JS source.
    let pid = serde_json::to_string(plugin_id).unwrap_or_else(|_| "\"\"".into());
    let loc = serde_json::to_string(locale).unwrap_or_else(|_| "\"\"".into());
    let thm = serde_json::to_string(theme).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(function () {{
  if (window.notemd) return;
  let __seq = 0;
  const __listeners = [];
  const pluginId = {pid};
  // Locale is embedded at build time, but a live language switch can't rebuild
  // this init script — so the host writes the new code into localStorage and
  // reloads the window; on reload we prefer that override. This makes language
  // changes propagate to every plugin uniformly, with no per-plugin code.
  let locale = {loc};
  try {{ const __l = localStorage.getItem('__notemd_locale'); if (__l) locale = __l; }} catch (e) {{}}
  const theme = {thm};
  window.notemd = Object.freeze({{
    pluginId,
    locale,
    theme,
    async request(method, params) {{
      const r = await fetch('/__rpc__', {{
        method: 'POST',
        headers: {{ 'content-type': 'application/json' }},
        body: JSON.stringify({{ jsonrpc: '2.0', id: __seq++, method, params: params ?? null }})
      }});
      const j = await r.json();
      if (j.error) throw new Error(j.error.code + ': ' + j.error.message);
      return j.result;
    }},
    onMessage(cb) {{ __listeners.push(cb); }}
  }});
  window.__notemd_dispatch = (payload) => {{ __listeners.forEach((cb) => cb(payload)); }};
}})();"#
    )
}

/// The `eval` string that pushes `payload` into a plugin window. Extracted as a
/// pure fn so the exact wire shape is unit-testable without a live webview.
pub fn dispatch_eval(payload: &Value) -> String {
    // serialize is infallible for a serde_json::Value.
    let json = serde_json::to_string(payload).unwrap_or_else(|_| "null".into());
    format!("window.__notemd_dispatch({json})")
}

/// Payload pushed into a plugin window for OS drag-drop (spec §8).
pub(crate) fn drag_drop_payload(phase: &str, paths: &[std::path::PathBuf]) -> Value {
    serde_json::json!({
        "type": "drag-drop",
        "phase": phase,
        "paths": paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
    })
}

/// Resolve a plugin window's title, in locale-resolution order (spec: a
/// localized name always beats the manifest's English `title`, because every
/// plugin today sets `title` to its English name):
///
/// 1. `i18n[locale].windows[window_id]` — new, optional per-window title convention.
/// 2. `i18n[locale].name` — localized plugin display name.
/// 3. `win_title` — the window contribution's own `title` field (English).
/// 4. `plugin_name` — the manifest's top-level `name` (English).
///
/// Locale lookup also tries the base language when `locale` carries a region
/// suffix (`zh-CN` → `zh`), since `i18n` is keyed by base language only. Empty
/// strings at any level are treated as absent (fall through to the next
/// level), guarding against manifests that set `i18n.<locale>.name: ""`.
pub(crate) fn window_title(
    i18n: Option<&Value>,
    locale: &str,
    window_id: &str,
    win_title: Option<&str>,
    plugin_name: &str,
) -> String {
    let non_empty = |v: Option<&Value>| -> Option<String> {
        v.and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    if let Some(i18n) = i18n {
        // Try the exact locale, then its base language (`zh-CN` → `zh`).
        let base = locale.split(['-', '_']).next().unwrap_or(locale);
        for candidate in [locale, base] {
            let Some(entry) = i18n.get(candidate) else { continue };
            if let Some(t) = non_empty(entry.get("windows").and_then(|w| w.get(window_id))) {
                return t;
            }
            if let Some(t) = non_empty(entry.get("name")) {
                return t;
            }
            if candidate == base {
                break;
            }
        }
    }

    win_title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| plugin_name.to_string())
}

/// 插件窗口 URL。`seed_json` 非空时挂成 `?seed=<urlencoded>`——protocol.rs 按
/// `url.path()` 解析资产,query 只被页面 JS 读取(预填等一次性 payload 的通道;
/// 已开的 singleton 窗口到不了这里,那一路走 `push_to_window`)。
pub(crate) fn plugin_window_url(plugin_id: &str, entry: &str, seed_json: Option<&str>) -> String {
    let base = format!("plugin://{plugin_id}/{entry}");
    match seed_json {
        None => base,
        Some(s) => {
            let enc: String = url::form_urlencoded::byte_serialize(s.as_bytes()).collect();
            format!("{base}?seed={enc}")
        }
    }
}

/// Open (or focus, if a singleton is already up) the window contributed under
/// `window_id` by `plugin_id`. `locale`/`theme` are read from the app to seed
/// the bridge. The window loads `plugin://<id>/<entry>` and gets NO capability
/// entry, so its only host channel is the `plugin://` fetch-RPC bridge.
/// `seed_json` rides the URL query of a freshly built window; it is ignored on
/// the singleton-focus path (a loaded webview reads pushes, not its URL).
pub fn open_plugin_window<R: Runtime>(
    app: &tauri::AppHandle<R>,
    plugin_id: &str,
    window_id: &str,
    seed_json: Option<&str>,
) -> Result<(), String> {
    // STATE lookup: manifest (for the window contribution + fallback title).
    let (manifest, _install_dir) = super::STATE
        .read()
        .map_err(|_| "plugin state lock poisoned".to_string())?
        .plugins
        .get(plugin_id)
        .cloned()
        .ok_or_else(|| format!("unknown plugin: {plugin_id}"))?;

    let win = manifest
        .contributes
        .windows
        .iter()
        .find(|w| w.id == window_id)
        .ok_or_else(|| format!("plugin '{plugin_id}' has no window '{window_id}'"))?;

    let label = window_label(plugin_id, window_id);

    // Singleton: an existing window with this label is shown+focused, not rebuilt.
    if win.singleton {
        if let Some(existing) = app.get_webview_window(&label) {
            let _ = existing.show();
            let _ = existing.unminimize();
            let _ = existing.set_focus();
            return Ok(());
        }
    }

    let locale = crate::read_saved_locale(app);
    let theme = read_saved_theme(app);
    let title = window_title(
        manifest.i18n.as_ref(),
        &locale,
        window_id,
        win.title.as_deref(),
        &manifest.name,
    );

    // `plugin://<id>/<entry>` is served by super::protocol. A custom scheme uses
    // WebviewUrl::CustomProtocol (External is documented http/https-only).
    let url = plugin_window_url(plugin_id, &win.entry, seed_json)
        .parse()
        .map_err(|e| format!("bad plugin url: {e}"))?;

    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::CustomProtocol(url))
        .title(title)
        .inner_size(win.width, win.height)
        .resizable(true)
        .decorations(true)
        // macOS normally spends the first click only activating an inactive
        // window. Plugin controls must respond on that click, like native
        // utility windows and inspectors do.
        .accept_first_mouse(true)
        .visible(false)
        .initialization_script(bridge_script(plugin_id, &locale, &theme));

    if let (Some(w), Some(h)) = (win.min_width, win.min_height) {
        builder = builder.min_inner_size(w, h);
    }

    let window = builder
        .build()
        .map_err(|e| format!("window build failed: {e}"))?;

    // Prune this plugin's fs.read:dialog grants when the window is destroyed:
    // GRANTED_PATHS is process-global, so a dialog-granted path would otherwise
    // stay readable for the whole app lifetime. Only the freshly-built window
    // gets the handler (the singleton-focus path above returns early, and its
    // window already has one). `Destroyed` fires after the webview is gone.
    //
    // Also forwards OS drag-drop into the window: Tauri's OS-level drag-drop
    // handler eats HTML5 drag-drop inside these isolated webviews, so the host
    // must relay `WindowEvent::DragDrop` as a push payload instead (spec §8).
    // The closure can't hold `window` itself (it's moved into the handler
    // below via `window.on_window_event`), so it re-fetches the window by
    // label off a cloned `AppHandle` on each drag-drop event.
    let pid = plugin_id.to_string();
    let app2 = app.clone();
    let label2 = label.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::DragDrop(dd) => {
            let payload = match dd {
                tauri::DragDropEvent::Enter { paths, .. } => drag_drop_payload("enter", paths),
                tauri::DragDropEvent::Drop { paths, .. } => drag_drop_payload("drop", paths),
                tauri::DragDropEvent::Leave => drag_drop_payload("leave", &[]),
                _ => return, // Over: high-frequency, not forwarded.
            };
            if let Some(w) = app2.get_webview_window(&label2) {
                let _ = w.eval(dispatch_eval(&payload));
            }
        }
        WindowEvent::Destroyed => {
            super::ui_rpc::clear_grants(&pid);
            // Tear down the plugin process when its window closes so long-lived
            // reader tasks / network connections (e.g. openclaw's UDS+relay and
            // its 8s claim poller) don't outlive the UI. Current plugins are
            // single-window, so a closed window means "nothing left to serve" —
            // deactivate() aborts the plugin's tasks; it lazily re-activates on
            // the next open. (Multi-window plugins would need a
            // last-window-closed check; none exist yet.)
            if let Some(lc) = super::lifecycle::RUNNING.read().unwrap().get(&pid).cloned() {
                tauri::async_runtime::spawn(async move { lc.deactivate().await });
            }
        }
        _ => {}
    });

    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
    Ok(())
}

/// Push `payload` to an already-open plugin window (used by later plugins that
/// stream host events into the UI). No-op if the window isn't open.
pub fn push_to_window<R: Runtime>(
    app: &tauri::AppHandle<R>,
    plugin_id: &str,
    window_id: &str,
    payload: &Value,
) {
    let label = window_label(plugin_id, window_id);
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.eval(dispatch_eval(payload));
    }
}

/// Live-refresh every open plugin window to `locale`. Plugin windows are
/// isolated webviews whose locale is injected into a build-time init script, so
/// a language switch can't reach them by re-eval alone. Uniform mechanism: write
/// the new locale into each window's `localStorage` (which the bridge prefers on
/// next load) and reload it — the plugin re-bootstraps in the new language with
/// zero per-plugin code. Passing the locale explicitly (not via a disk read)
/// keeps this race-free with the frontend's async settings persist.
pub fn refresh_plugin_windows_locale<R: Runtime>(app: &tauri::AppHandle<R>, locale: &str) {
    // Exact labels of contributed plugin windows, so we never touch app windows
    // that merely share a "plugin-" label prefix (e.g. the plugin market).
    let labels: Vec<String> = match super::STATE.read() {
        Ok(state) => state
            .plugins
            .iter()
            .flat_map(|(pid, (manifest, _))| {
                manifest
                    .contributes
                    .windows
                    .iter()
                    .map(move |w| window_label(pid, &w.id))
            })
            .collect(),
        Err(_) => return,
    };
    let loc = serde_json::to_string(locale).unwrap_or_else(|_| "\"en\"".into());
    let js = format!(
        "try{{localStorage.setItem('__notemd_locale',{loc});}}catch(e){{}}location.reload();"
    );
    for label in labels {
        if let Some(win) = app.get_webview_window(&label) {
            let _ = win.eval(&js);
        }
    }
}

/// Read the persisted UI theme from settings.json (mirrors `read_saved_locale`).
/// Defaults to `"default"` when the file is missing/unreadable or the key absent.
///
/// `settings.json`'s `theme` key has been an object (`{light, dark,
/// followSystem}`) since 4517e63 (skin → theme.{light,dark,followSystem}
/// migration); reading it with `.as_str()` therefore always misses and this
/// always returned `"default"`, injecting a wrong `window.notemd.theme` into
/// every plugin window. Delegate to `themes::commands::parse_theme_settings`
/// — the parser Task 7 already wrote (and unit-tests) for exactly this shape,
/// including the pre-migration `skin` string fallback — and use its light
/// slot as the single id this legacy field carries.
fn read_saved_theme<R: Runtime>(app: &tauri::AppHandle<R>) -> String {
    let Ok(dir) = app.path().app_config_dir() else {
        return "default".to_string();
    };
    let Ok(text) = std::fs::read_to_string(dir.join("settings.json")) else {
        return "default".to_string();
    };
    let Ok(json) = serde_json::from_str::<Value>(&text) else {
        return "default".to_string();
    };
    theme_id_from_settings(&json)
}

/// Pure core of [`read_saved_theme`]: pick the single theme id fed to
/// `window.notemd.theme` from an already-parsed `settings.json` value.
/// Extracted so the fix is unit-testable without an `AppHandle`.
fn theme_id_from_settings(json: &Value) -> String {
    crate::themes::commands::parse_theme_settings(json).0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_url_with_and_without_seed() {
        assert_eq!(
            plugin_window_url("a.b", "index.html", None),
            "plugin://a.b/index.html"
        );
        let u = plugin_window_url("a.b", "index.html", Some(r#"{"t":"溯源 x"}"#));
        assert!(u.starts_with("plugin://a.b/index.html?seed=%7B%22t%22"), "{u}");
        assert!(!u.contains('"'), "JSON 必须整体转义进 query:{u}");
    }

    #[test]
    fn window_label_sanitizes_dots_to_hyphens() {
        assert_eq!(window_label("notemd.roam-import", "main"), "plugin-notemd-roam-import-main");
        assert_eq!(window_label("test.ui.fixture", "w1"), "plugin-test-ui-fixture-w1");
        // No dots → unchanged body.
        assert_eq!(window_label("plain", "main"), "plugin-plain-main");
    }

    #[test]
    fn bridge_script_embeds_identity_json_literals() {
        let s = bridge_script("notemd.roam-import", "zh", "midnight");
        // JSON literals (quoted), safely embedded.
        assert!(s.contains(r#""notemd.roam-import""#), "pluginId literal: {s}");
        assert!(s.contains(r#""zh""#), "locale literal");
        assert!(s.contains(r#""midnight""#), "theme literal");
    }

    #[test]
    fn bridge_script_has_idempotency_guard() {
        // The guard is the first statement inside the IIFE so a second run
        // (window init + HTML injection) is a no-op instead of redefining
        // window.notemd (which is frozen) and re-registering listeners.
        let s = bridge_script("p.id", "en", "default");
        assert!(s.contains("if (window.notemd) return;"), "guard present: {s}");
        let guard = s.find("if (window.notemd) return;").unwrap();
        let freeze = s.find("Object.freeze").unwrap();
        assert!(guard < freeze, "guard must precede the bridge definition");
    }

    #[test]
    fn bridge_script_defines_bridge_surface() {
        let s = bridge_script("p.id", "en", "default");
        assert!(s.contains("Object.freeze"), "freezes the bridge");
        assert!(s.contains("window.notemd"), "defines window.notemd");
        assert!(s.contains("/__rpc__"), "posts to the rpc endpoint");
        assert!(s.contains("__notemd_dispatch"), "defines the push entry point");
        assert!(s.contains("'jsonrpc': '2.0'") || s.contains("jsonrpc: '2.0'"), "jsonrpc envelope");
    }

    #[test]
    fn bridge_script_escapes_quotes_in_identity() {
        // A pathological id with a quote must not break out of the JS literal.
        let s = bridge_script(r#"p"x"#, "en", "default");
        // serde_json escapes the embedded quote → \" inside the literal.
        assert!(s.contains(r#""p\"x""#), "escaped id literal: {s}");
    }

    #[test]
    fn dispatch_eval_wraps_payload_json() {
        let payload = serde_json::json!({ "type": "progress", "value": 42 });
        let s = dispatch_eval(&payload);
        assert!(s.starts_with("window.__notemd_dispatch("), "{s}");
        assert!(s.ends_with(")"), "{s}");
        // The inner JSON round-trips.
        let inner = &s["window.__notemd_dispatch(".len()..s.len() - 1];
        let back: Value = serde_json::from_str(inner).unwrap();
        assert_eq!(back["type"], "progress");
        assert_eq!(back["value"], 42);
    }

    #[test]
    fn window_title_prefers_per_window_i18n_title() {
        let i18n = serde_json::json!({
            "zh": { "name": "示例插件", "windows": { "main": "主窗口标题" } }
        });
        assert_eq!(
            window_title(Some(&i18n), "zh", "main", Some("Example Plugin"), "Example Plugin"),
            "主窗口标题"
        );
    }

    #[test]
    fn window_title_falls_back_to_i18n_name() {
        let i18n = serde_json::json!({ "zh": { "name": "示例插件" } });
        assert_eq!(
            window_title(Some(&i18n), "zh", "main", Some("Example Plugin"), "Example Plugin"),
            "示例插件"
        );
    }

    #[test]
    fn window_title_falls_back_to_win_title_when_no_i18n_match() {
        // Locale not present in i18n at all.
        let i18n = serde_json::json!({ "ja": { "name": "サンプル" } });
        assert_eq!(
            window_title(Some(&i18n), "zh", "main", Some("Win Title"), "Plugin Name"),
            "Win Title"
        );
        // No i18n at all.
        assert_eq!(
            window_title(None, "zh", "main", Some("Win Title"), "Plugin Name"),
            "Win Title"
        );
    }

    #[test]
    fn window_title_falls_back_to_plugin_name_as_last_resort() {
        assert_eq!(window_title(None, "zh", "main", None, "Plugin Name"), "Plugin Name");
        let i18n = serde_json::json!({ "zh": {} });
        assert_eq!(
            window_title(Some(&i18n), "zh", "main", None, "Plugin Name"),
            "Plugin Name"
        );
    }

    #[test]
    fn window_title_region_suffix_falls_back_to_base_language() {
        let i18n = serde_json::json!({ "zh": { "name": "示例插件" } });
        assert_eq!(
            window_title(Some(&i18n), "zh-CN", "main", Some("Example Plugin"), "Example Plugin"),
            "示例插件"
        );
        // Per-window title also resolves through the base language.
        let i18n2 = serde_json::json!({ "zh": { "windows": { "main": "主窗口" } } });
        assert_eq!(
            window_title(Some(&i18n2), "zh-Hans", "main", None, "Plugin Name"),
            "主窗口"
        );
    }

    #[test]
    fn window_title_ignores_empty_strings_and_falls_through() {
        // Empty per-window title falls through to i18n name.
        let i18n = serde_json::json!({
            "zh": { "name": "示例插件", "windows": { "main": "" } }
        });
        assert_eq!(
            window_title(Some(&i18n), "zh", "main", Some("Example Plugin"), "Example Plugin"),
            "示例插件"
        );
        // Empty i18n name falls through to win_title.
        let i18n2 = serde_json::json!({ "zh": { "name": "" } });
        assert_eq!(
            window_title(Some(&i18n2), "zh", "main", Some("Win Title"), "Plugin Name"),
            "Win Title"
        );
        // Empty win_title falls through to plugin_name.
        assert_eq!(window_title(None, "zh", "main", Some(""), "Plugin Name"), "Plugin Name");
    }

    #[test]
    fn theme_id_from_settings_reads_the_post_4517e63_object_shape() {
        // Regression: `.as_str()` on the `theme` key always missed this shape
        // and silently fell back to "default" for every real vault.
        let json = serde_json::json!({
            "theme": { "light": "effie", "dark": "onedark", "followSystem": false }
        });
        assert_eq!(theme_id_from_settings(&json), "effie");
    }

    #[test]
    fn theme_id_from_settings_falls_back_through_legacy_and_defaults() {
        // Pre-migration `skin` string, still real on disk until the next save.
        assert_eq!(theme_id_from_settings(&serde_json::json!({"skin": "effie"})), "effie");
        // Nothing usable at all → the documented "default".
        assert_eq!(theme_id_from_settings(&serde_json::json!({})), "default");
    }

    #[test]
    fn drag_drop_payload_shapes() {
        let p = drag_drop_payload("drop", &[std::path::PathBuf::from("/a/b.epub")]);
        assert_eq!(p["type"], "drag-drop");
        assert_eq!(p["phase"], "drop");
        assert_eq!(p["paths"][0], "/a/b.epub");
        let e = drag_drop_payload("leave", &[]);
        assert_eq!(e["phase"], "leave");
        assert!(e["paths"].as_array().unwrap().is_empty());
    }
}
