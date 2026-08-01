//! `plugin://` custom URI scheme (spec §7.1, 子项目② Task 2).
//!
//! Serves plugin UI static assets (`GET plugin://<id>/<path>` from
//! `<install>/current/<ui>/`) and carries the UI→host fetch-RPC bridge
//! (`POST plugin://<id>/__rpc__`). Plugin windows are granted ZERO Tauri
//! IPC (no capability entry) — this protocol IS the only bridge, and the
//! request Origin (`plugin://<id>`) authenticates the calling plugin.
//!
//! Layering: `resolve_asset`/`mime_for`/`csp_header`/`handle_parsed` are a
//! pure, unit-testable core (no AppHandle, no global STATE); `handle` is the
//! thin shell registered on the Builder that binds them to `STATE`.

use std::path::{Path, PathBuf};

use tauri::http;

// ── Asset resolution ────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub enum AssetError {
    NotFound,
    Traversal,
}

/// GET asset resolution: URL path → absolute file path under `ui_root`.
///
/// - Percent-decodes first, so encoded traversal (`%2e%2e`) hits the same
///   guard as a literal `..`.
/// - Empty path / `/` is NOT an implicit index — the entry point is explicit
///   in the manifest. A trailing `/` on a non-empty path appends `index.html`.
/// - `..` segments are rejected before any filesystem access; containment is
///   then re-verified against the canonicalized root, which also defeats
///   symlink escapes.
pub fn resolve_asset(ui_root: &Path, url_path: &str) -> Result<PathBuf, AssetError> {
    let decoded = urlencoding::decode(url_path).map_err(|_| AssetError::NotFound)?;
    let trimmed = decoded.trim_start_matches('/');
    if trimmed.is_empty() {
        return Err(AssetError::NotFound);
    }
    let rel = if trimmed.ends_with('/') {
        format!("{trimmed}index.html")
    } else {
        trimmed.to_string()
    };
    if rel.split('/').any(|seg| seg == "..") {
        return Err(AssetError::Traversal);
    }
    let root = ui_root.canonicalize().map_err(|_| AssetError::NotFound)?;
    let resolved = root.join(&rel).canonicalize().map_err(|_| AssetError::NotFound)?;
    if !resolved.starts_with(&root) {
        return Err(AssetError::Traversal);
    }
    if !resolved.is_file() {
        return Err(AssetError::NotFound);
    }
    Ok(resolved)
}

pub fn mime_for(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("html") => "text/html",
        Some("js") | Some("mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json") | Some("map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

/// `'self'` under a `plugin://<id>` origin is this plugin only; every remote
/// load is denied (spec §7.1). `object-src`/`base-uri`/`form-action`/`frame-src`
/// are locked explicitly: `base-uri` and `form-action` do NOT inherit
/// `default-src`, so without them a `<form action="https://…">` (or a `<base>`
/// hijack) would still be a navigation/exfil channel.
///
/// **Custom-editor framing (子项目④).** A custom-editor page is loaded in an
/// `<iframe>` BY the main app (origin `tauri://localhost`). That direction is
/// governed by `frame-ancestors`, which is ABSENT here — so framing is allowed.
/// (`frame-src 'none'` only restricts what THIS page may itself frame, i.e.
/// nested iframes, not who may frame it.) The bridge injected into the served
/// HTML keeps `connect-src 'self'`, so the in-iframe `window.notemd.request()`
/// fetch to `/__rpc__` still passes.
pub fn csp_header(_plugin_id: &str) -> String {
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; connect-src 'self'; object-src 'none'; \
     base-uri 'none'; form-action 'none'; frame-src 'none'"
        .to_string()
}

/// Inject `<script>{bridge}</script>` into a served plugin HTML document so an
/// iframe (which gets no `initialization_script`) still exposes `window.notemd`.
/// Insert right after the first `<head...>` open tag; if there is no `<head>`,
/// fall back to right after `<body...>`; if neither exists, prepend. The
/// bridge's own `if (window.notemd) return;` guard makes this harmless for
/// plugin *windows* (which also get it via `initialization_script`).
///
/// Case-insensitive tag search; preserves every original byte of `html`.
pub fn inject_bridge(html: &str, bridge: &str) -> String {
    let script = format!("<script>{bridge}</script>");
    // Find the end of the first `<head ...>` (or `<body ...>`) open tag.
    let lower = html.to_ascii_lowercase();
    let insert_at = find_tag_end(&lower, "<head")
        .or_else(|| find_tag_end(&lower, "<body"));
    match insert_at {
        Some(pos) => {
            let mut out = String::with_capacity(html.len() + script.len());
            out.push_str(&html[..pos]);
            out.push_str(&script);
            out.push_str(&html[pos..]);
            out
        }
        // No head/body: prepend so the bridge still runs before page scripts.
        None => format!("{script}{html}"),
    }
}

/// Byte offset just past the `>` of the first `<tag...>` open tag in `lower`
/// (which must already be lowercased), or `None` if the tag isn't present or
/// its `>` is missing. `tag` is a lowercase prefix like `"<head"`.
fn find_tag_end(lower: &str, tag: &str) -> Option<usize> {
    let start = lower.find(tag)?;
    let close = lower[start..].find('>')?;
    Some(start + close + 1)
}

// ── Request handling (pure core) ────────────────────────────────────────

/// Minimal read view over the loaded-plugin table so `handle_parsed` is
/// testable without an AppHandle or the global STATE.
pub trait PluginView {
    /// id → (ui_root, capabilities). `None` ⇒ flag off / unknown id / no ui.
    fn lookup(&self, plugin_id: &str) -> Option<(PathBuf, Vec<String>)>;
}

const RPC_PATH: &str = "/__rpc__";

/// Outcome of the pure routing layer. `Rpc` means the request passed all the
/// pure checks (known plugin, POST /__rpc__, matching Origin) and its body
/// should be dispatched by `ui_rpc::dispatch` in the shell — a step that needs
/// the AppHandle-backed services and so cannot live in this pure core.
/// `Response` is a fully-formed reply for every other case (asset serve,
/// 404/403/405) that the shell returns verbatim.
pub enum Routed {
    /// Dispatch RPC: `(plugin_id, capabilities)`. Body comes from the request.
    Rpc(String, Vec<String>),
    Response(http::Response<Vec<u8>>),
}

/// Pure routing core (GET/auth/404 logic — no AppHandle, no global STATE).
/// The authenticated `POST /__rpc__` case is returned as [`Routed::Rpc`] for
/// the shell to dispatch via `ui_rpc`.
pub fn handle_parsed(
    view: &dyn PluginView,
    method: &str,
    plugin_id: &str,
    path: &str,
    origin: Option<&str>,
    locale: &str,
    theme: &str,
) -> Routed {
    let Some((ui_root, capabilities)) = view.lookup(plugin_id) else {
        return Routed::Response(plain(http::StatusCode::NOT_FOUND, "unknown plugin"));
    };
    match method {
        "POST" if path == RPC_PATH => {
            // Origin authenticates the caller as this plugin's own window. WKWebView
            // does NOT attach an `Origin` header to a same-origin POST fetch from a
            // custom-scheme (`plugin://`) document, so the legitimate caller arrives
            // with `origin == None`. That is safe to accept: WebKit only routes a
            // `plugin://<id>` request to this handler from that same window (cross-
            // origin fetches to a custom scheme are blocked by WebKit + the per-
            // plugin CSP), and any request that DOES carry an Origin must match
            // exactly. Reject only an explicit foreign origin.
            let expected = format!("plugin://{plugin_id}");
            if let Some(o) = origin {
                if o != expected {
                    return Routed::Response(plain(http::StatusCode::FORBIDDEN, "origin mismatch"));
                }
            }
            Routed::Rpc(plugin_id.to_string(), capabilities)
        }
        "GET" => Routed::Response(serve_asset(&ui_root, plugin_id, path, locale, theme)),
        "POST" => Routed::Response(plain(http::StatusCode::NOT_FOUND, "not found")),
        _ => Routed::Response(plain(http::StatusCode::METHOD_NOT_ALLOWED, "method not allowed")),
    }
}

/// GET asset serving, extracted from `handle_parsed` for readability.
///
/// For `text/html` assets the fetch-RPC bridge (`super::windows::bridge_script`)
/// is injected as an inline `<script>` so iframes — which get no
/// `initialization_script` — still expose `window.notemd`. Other MIME types are
/// served byte-for-byte.
fn serve_asset(
    ui_root: &Path,
    plugin_id: &str,
    path: &str,
    locale: &str,
    theme: &str,
) -> http::Response<Vec<u8>> {
    match resolve_asset(ui_root, path) {
        Ok(file) => {
            let Ok(bytes) = std::fs::read(&file) else {
                return plain(http::StatusCode::NOT_FOUND, "not found");
            };
            let mime = mime_for(&file);
            let mut builder = http::Response::builder()
                .status(http::StatusCode::OK)
                .header("content-type", mime)
                .header("cache-control", "no-cache");
            let body = if mime == "text/html" {
                builder = builder.header("content-security-policy", csp_header(plugin_id));
                // Inject the bridge into the HTML (iframes have no init script).
                match String::from_utf8(bytes) {
                    Ok(html) => {
                        let bridge = super::windows::bridge_script(plugin_id, locale, theme);
                        inject_bridge(&html, &bridge).into_bytes()
                    }
                    // Non-UTF-8 "html" — serve as-is rather than guess.
                    Err(e) => e.into_bytes(),
                }
            } else {
                bytes
            };
            builder.body(body).unwrap()
        }
        Err(AssetError::Traversal) => plain(http::StatusCode::FORBIDDEN, "forbidden"),
        Err(AssetError::NotFound) => plain(http::StatusCode::NOT_FOUND, "not found"),
    }
}

fn plain(status: http::StatusCode, msg: &str) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(msg.as_bytes().to_vec())
        .unwrap()
}

// ── AppHandle shell ─────────────────────────────────────────────────────

struct StateView;

impl PluginView for StateView {
    fn lookup(&self, plugin_id: &str) -> Option<(PathBuf, Vec<String>)> {
        let st = super::STATE.read().ok()?;
        let (manifest, install_dir) = st.plugins.get(plugin_id)?;
        let ui = manifest.ui.as_ref()?; // e.g. "ui/"; install_dir = <install>/current
        Some((install_dir.join(ui), manifest.capabilities.clone()))
    }
}

/// Shell binding [`handle_parsed`] to the global STATE and, for the
/// authenticated `POST /__rpc__` case, to `ui_rpc::dispatch`. macOS-only URL
/// shape: requests arrive as `plugin://<id>/<path>` (id = URL host; other
/// platforms would nest it in the path, which we don't handle).
///
/// # Threading contract (MUST run off the main thread)
///
/// WKWebView delivers custom-scheme requests on the main thread, and the RPC
/// branch can block for minutes (`host.dialog.*` waits for the user) — blocking
/// there would freeze the run loop AND deadlock the dialog, which itself needs
/// the main thread. lib.rs therefore registers this via
/// `register_asynchronous_uri_scheme_protocol` and calls `handle` from a
/// dedicated spawned thread per request, where `block_on(ui_rpc::dispatch)` is
/// safe.
pub fn handle<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: http::Request<Vec<u8>>,
) -> http::Response<Vec<u8>> {
    let Ok(url) = url::Url::parse(&request.uri().to_string()) else {
        return plain(http::StatusCode::NOT_FOUND, "bad url");
    };
    let Some(plugin_id) = url.host_str().map(str::to_string) else {
        return plain(http::StatusCode::NOT_FOUND, "missing plugin id");
    };
    let origin = request
        .headers()
        .get(http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    // locale/theme seed the bridge injected into served HTML (mirrors how
    // windows.rs seeds it into the window initialization_script).
    let locale = crate::read_saved_locale(app);
    let theme = read_saved_theme(app);

    match handle_parsed(
        &StateView,
        request.method().as_str(),
        &plugin_id,
        url.path(),
        origin.as_deref(),
        &locale,
        &theme,
    ) {
        Routed::Response(r) => r,
        Routed::Rpc(id, capabilities) => dispatch_rpc(app, &id, &capabilities, request.body()),
    }
}

/// Read the persisted UI theme from settings.json (mirrors `read_saved_theme`
/// in `windows.rs`; kept local so this module stays self-contained). Defaults
/// to `"default"` when the file is missing/unreadable or the key is absent.
fn read_saved_theme<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> String {
    use tauri::Manager;
    let Ok(dir) = app.path().app_config_dir() else {
        return "default".to_string();
    };
    let Ok(text) = std::fs::read_to_string(dir.join("settings.json")) else {
        return "default".to_string();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return "default".to_string();
    };
    json.get("theme").and_then(|v| v.as_str()).unwrap_or("default").to_string()
}

/// RPC seam: parse the JSON-RPC body, run `ui_rpc::dispatch` (production entry;
/// builds the live services from `app`), serialize the response. Body parse
/// failure → JSON-RPC -32700.
fn dispatch_rpc<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    plugin_id: &str,
    capabilities: &[String],
    body: &[u8],
) -> http::Response<Vec<u8>> {
    let req: plugin_protocol::RpcRequest = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => {
            let err = serde_json::json!({
                "jsonrpc": "2.0", "id": null,
                "error": { "code": -32700, "message": format!("parse error: {e}") }
            });
            return json_response(&err);
        }
    };
    let resp = tauri::async_runtime::block_on(super::ui_rpc::dispatch(
        app,
        plugin_id,
        capabilities,
        req,
    ));
    json_response(&resp)
}

fn json_response<T: serde::Serialize>(value: &T) -> http::Response<Vec<u8>> {
    let body = serde_json::to_vec(value)
        .unwrap_or_else(|_| br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialize"}}"#.to_vec());
    http::Response::builder()
        .status(http::StatusCode::OK)
        .header("content-type", "application/json")
        .body(body)
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct MapView(HashMap<String, (PathBuf, Vec<String>)>);

    impl PluginView for MapView {
        fn lookup(&self, plugin_id: &str) -> Option<(PathBuf, Vec<String>)> {
            self.0.get(plugin_id).cloned()
        }
    }

    fn ui_fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "<html></html>").unwrap();
        std::fs::write(dir.path().join("app.js"), "console.log(1)").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/index.html"), "<html>sub</html>").unwrap();
        dir
    }

    fn view_for(dir: &Path) -> MapView {
        let mut map = HashMap::new();
        map.insert(
            "test.plugin".to_string(),
            (dir.to_path_buf(), vec!["toast".to_string()]),
        );
        MapView(map)
    }

    // ── resolve_asset ───────────────────────────────────────────────────

    #[test]
    fn resolve_happy_path() {
        let dir = ui_fixture();
        let p = resolve_asset(dir.path(), "/index.html").unwrap();
        assert_eq!(std::fs::read_to_string(p).unwrap(), "<html></html>");
    }

    #[test]
    fn resolve_trailing_slash_appends_index() {
        let dir = ui_fixture();
        let p = resolve_asset(dir.path(), "/sub/").unwrap();
        assert!(p.ends_with("sub/index.html"), "{p:?}");
    }

    #[test]
    fn resolve_root_is_not_implicit_index() {
        let dir = ui_fixture();
        assert_eq!(resolve_asset(dir.path(), "").unwrap_err(), AssetError::NotFound);
        assert_eq!(resolve_asset(dir.path(), "/").unwrap_err(), AssetError::NotFound);
    }

    #[test]
    fn resolve_rejects_plain_dotdot() {
        let dir = ui_fixture();
        assert_eq!(resolve_asset(dir.path(), "/../secret").unwrap_err(), AssetError::Traversal);
        assert_eq!(
            resolve_asset(dir.path(), "/sub/../../secret").unwrap_err(),
            AssetError::Traversal
        );
    }

    #[test]
    fn resolve_rejects_percent_encoded_dotdot() {
        let dir = ui_fixture();
        assert_eq!(
            resolve_asset(dir.path(), "/%2e%2e/secret").unwrap_err(),
            AssetError::Traversal
        );
    }

    #[test]
    fn resolve_rejects_symlink_escape() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "s3cret").unwrap();
        let dir = ui_fixture();
        if std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).is_err() {
            eprintln!("skipping: symlink creation not supported here");
            return;
        }
        assert_eq!(
            resolve_asset(dir.path(), "/link/secret.txt").unwrap_err(),
            AssetError::Traversal
        );
    }

    #[test]
    fn resolve_missing_file_not_found() {
        let dir = ui_fixture();
        assert_eq!(resolve_asset(dir.path(), "/nope.js").unwrap_err(), AssetError::NotFound);
    }

    // ── mime_for / csp_header ───────────────────────────────────────────

    #[test]
    fn mime_table_spot_checks() {
        assert_eq!(mime_for(Path::new("a.html")), "text/html");
        assert_eq!(mime_for(Path::new("a.js")), "text/javascript");
        assert_eq!(mime_for(Path::new("a.mjs")), "text/javascript");
        assert_eq!(mime_for(Path::new("a.css")), "text/css");
        assert_eq!(mime_for(Path::new("a.map")), "application/json");
        assert_eq!(mime_for(Path::new("a.svg")), "image/svg+xml");
        assert_eq!(mime_for(Path::new("a.woff2")), "font/woff2");
        assert_eq!(mime_for(Path::new("a.wasm")), "application/wasm");
        assert_eq!(mime_for(Path::new("a.PNG")), "image/png");
        assert_eq!(mime_for(Path::new("a.bin")), "application/octet-stream");
        assert_eq!(mime_for(Path::new("noext")), "application/octet-stream");
    }

    #[test]
    fn csp_exact_string() {
        assert_eq!(
            csp_header("test.plugin"),
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data:; connect-src 'self'; object-src 'none'; \
             base-uri 'none'; form-action 'none'; frame-src 'none'"
        );
    }

    // ── handle_parsed ───────────────────────────────────────────────────

    /// Unwrap the `Routed::Response` branch (panics on `Rpc`).
    fn resp(r: Routed) -> http::Response<Vec<u8>> {
        match r {
            Routed::Response(r) => r,
            Routed::Rpc(..) => panic!("expected a direct response, got Routed::Rpc"),
        }
    }

    #[test]
    fn handle_unknown_plugin_404() {
        let dir = ui_fixture();
        let view = view_for(dir.path());
        let r = resp(handle_parsed(&view, "GET", "other.plugin", "/index.html", None, "en", "default"));
        assert_eq!(r.status(), 404);
    }

    #[test]
    fn handle_get_html_has_csp_and_no_cache() {
        let dir = ui_fixture();
        let view = view_for(dir.path());
        let r = resp(handle_parsed(&view, "GET", "test.plugin", "/index.html", None, "en", "default"));
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["content-type"], "text/html");
        assert_eq!(r.headers()["cache-control"], "no-cache");
        assert_eq!(
            r.headers()["content-security-policy"].to_str().unwrap(),
            csp_header("test.plugin")
        );
        // The served HTML now carries the injected bridge (see dedicated tests
        // below); the original markup is still present.
        let body = String::from_utf8(r.body().clone()).unwrap();
        assert!(body.contains("<html></html>"), "original html preserved: {body}");
    }

    #[test]
    fn handle_get_html_injects_bridge_with_guard() {
        let dir = ui_fixture();
        let view = view_for(dir.path());
        let r = resp(handle_parsed(&view, "GET", "test.plugin", "/index.html", None, "zh", "midnight"));
        let body = String::from_utf8(r.body().clone()).unwrap();
        // Bridge script + idempotency guard are injected into the HTML.
        assert!(body.contains("<script>"), "script tag injected: {body}");
        assert!(body.contains("window.notemd"), "bridge defines window.notemd");
        assert!(body.contains("if (window.notemd) return;"), "idempotency guard present");
        assert!(body.contains("/__rpc__"), "bridge posts to rpc endpoint");
        // locale/theme seeded from the request.
        assert!(body.contains(r#""zh""#), "locale literal seeded");
        assert!(body.contains(r#""midnight""#), "theme literal seeded");
    }

    #[test]
    fn handle_get_js_has_no_csp_and_no_bridge() {
        let dir = ui_fixture();
        let view = view_for(dir.path());
        let r = resp(handle_parsed(&view, "GET", "test.plugin", "/app.js", None, "en", "default"));
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["content-type"], "text/javascript");
        assert_eq!(r.headers()["cache-control"], "no-cache");
        assert!(r.headers().get("content-security-policy").is_none());
        // JS assets are served byte-for-byte — no bridge injection.
        assert_eq!(r.body(), b"console.log(1)");
    }

    #[test]
    fn handle_get_traversal_403() {
        let dir = ui_fixture();
        let view = view_for(dir.path());
        let r = resp(handle_parsed(&view, "GET", "test.plugin", "/%2e%2e/secret", None, "en", "default"));
        assert_eq!(r.status(), 403);
    }

    #[test]
    fn handle_rpc_wrong_origin_403() {
        let dir = ui_fixture();
        let view = view_for(dir.path());
        // An EXPLICIT foreign origin is still rejected. A missing Origin is NOT
        // here — see handle_rpc_missing_origin_routes (WKWebView strips it).
        for origin in [Some("plugin://other.plugin"), Some("tauri://localhost")] {
            let r = resp(handle_parsed(&view, "POST", "test.plugin", "/__rpc__", origin, "en", "default"));
            assert_eq!(r.status(), 403, "origin {origin:?} must be rejected");
        }
    }

    #[test]
    fn handle_rpc_missing_origin_routes() {
        // WKWebView omits the Origin header on same-origin POST fetches from a
        // custom-scheme (plugin://) document, so the plugin's OWN window arrives
        // with origin == None. It must route (regression: a strict `!= Some`
        // check 403'd every ui-plugin RPC, breaking all vault reads/writes).
        let dir = ui_fixture();
        let view = view_for(dir.path());
        match handle_parsed(&view, "POST", "test.plugin", "/__rpc__", None, "en", "default") {
            Routed::Rpc(id, _) => assert_eq!(id, "test.plugin"),
            Routed::Response(r) => panic!("missing origin must route, got status {}", r.status()),
        }
    }

    #[test]
    fn handle_rpc_right_origin_routes_to_dispatch_with_capabilities() {
        let dir = ui_fixture();
        let view = view_for(dir.path());
        match handle_parsed(
            &view,
            "POST",
            "test.plugin",
            "/__rpc__",
            Some("plugin://test.plugin"),
            "en",
            "default",
        ) {
            Routed::Rpc(id, capabilities) => {
                assert_eq!(id, "test.plugin");
                assert_eq!(capabilities, vec!["toast".to_string()]);
            }
            Routed::Response(r) => panic!("expected Routed::Rpc, got status {}", r.status()),
        }
    }

    #[test]
    fn handle_rpc_wrong_path_post_404() {
        let dir = ui_fixture();
        let view = view_for(dir.path());
        let r = resp(handle_parsed(
            &view,
            "POST",
            "test.plugin",
            "/other",
            Some("plugin://test.plugin"),
            "en",
            "default",
        ));
        assert_eq!(r.status(), 404);
    }

    #[test]
    fn handle_put_405() {
        let dir = ui_fixture();
        let view = view_for(dir.path());
        let r = resp(handle_parsed(&view, "PUT", "test.plugin", "/index.html", None, "en", "default"));
        assert_eq!(r.status(), 405);
    }

    // ── inject_bridge (pure) ────────────────────────────────────────────

    #[test]
    fn inject_bridge_after_head() {
        let out = inject_bridge("<html><head></head><body>x</body></html>", "B()");
        assert_eq!(out, "<html><head><script>B()</script></head><body>x</body></html>");
    }

    #[test]
    fn inject_bridge_after_head_with_attrs() {
        let out = inject_bridge("<head lang=\"en\">z</head>", "B()");
        assert_eq!(out, "<head lang=\"en\"><script>B()</script>z</head>");
    }

    #[test]
    fn inject_bridge_falls_back_to_body_when_no_head() {
        let out = inject_bridge("<html><body>x</body></html>", "B()");
        assert_eq!(out, "<html><body><script>B()</script>x</body></html>");
    }

    #[test]
    fn inject_bridge_prepends_when_no_head_or_body() {
        let out = inject_bridge("<div>hi</div>", "B()");
        assert_eq!(out, "<script>B()</script><div>hi</div>");
    }

    #[test]
    fn inject_bridge_case_insensitive_head() {
        let out = inject_bridge("<HTML><HEAD></HEAD></HTML>", "B()");
        assert_eq!(out, "<HTML><HEAD><script>B()</script></HEAD></HTML>");
    }

    #[test]
    fn inject_bridge_preserves_original_bytes() {
        // Everything except the inserted <script> is byte-identical.
        let html = "<html><head><title>T</title></head><body><p>café</p></body></html>";
        let out = inject_bridge(html, "B()");
        assert_eq!(out.replace("<script>B()</script>", ""), html);
    }
}
