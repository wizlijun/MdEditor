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

/// Reserved URL prefix under every `plugin://<id>` origin that mirrors the
/// host's own bundled frontend assets read-only (spec §3.4, Editor Kit).
///
/// It mirrors a whole *directory tree* ([`HOST_ASSET_DIR`]), not a file
/// allowlist: the kit entry (`assets/editor-kit-v1.js`) statically imports
/// several hash-named shared chunks and the moraya runtime dynamically imports
/// more, so any filename-based allowlist would break on the next `pnpm build`.
const HOST_PREFIX: &str = "/__host__/";

/// The only host directory reachable through [`HOST_PREFIX`].
///
/// Vite emits every hashed chunk, stylesheet and font under `dist/assets/`;
/// what lives at the dist *root* is the app's own HTML entry points
/// (`index.html`, `insights.html`, `daily-notes.html`, …). Confining the mirror
/// to this one directory keeps those entry points behind a structural boundary
/// instead of relying on [`is_html_document`]'s content sniffing, and costs the
/// kit nothing: its relative imports and `new URL('./…', import.meta.url)`
/// references all resolve inside the same directory.
const HOST_ASSET_DIR: &str = "assets/";

/// Capability a plugin manifest must declare to reach [`HOST_PREFIX`].
const EDITOR_KIT_CAP: &str = "editor.kit";

/// Outcome of the pure routing layer. `Rpc` means the request passed all the
/// pure checks (known plugin, POST /__rpc__, matching Origin) and its body
/// should be dispatched by `ui_rpc::dispatch` in the shell — a step that needs
/// the AppHandle-backed services and so cannot live in this pure core.
/// `Response` is a fully-formed reply for every other case (asset serve,
/// 404/403/405) that the shell returns verbatim.
pub enum Routed {
    /// Dispatch RPC: `(plugin_id, capabilities)`. Body comes from the request.
    Rpc(String, Vec<String>),
    /// Serve a host-bundled frontend asset (Editor Kit). The payload is the
    /// host asset path with the [`HOST_PREFIX`] already stripped and validated
    /// (e.g. `/assets/editor-kit-v1.js`); the shell reads the bytes through
    /// `app.asset_resolver()`, which this pure core cannot reach.
    HostAsset(String),
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
        "GET" => match path.strip_prefix(HOST_PREFIX) {
            Some(rest) => route_host_asset(&capabilities, rest),
            None => Routed::Response(serve_asset(&ui_root, plugin_id, path, locale, theme)),
        },
        "POST" => Routed::Response(plain(http::StatusCode::NOT_FOUND, "not found")),
        _ => Routed::Response(plain(http::StatusCode::METHOD_NOT_ALLOWED, "method not allowed")),
    }
}

/// Route `GET /__host__/<rest>` to the host's bundled frontend assets.
///
/// The checks, in the order they run — the order is itself load-bearing:
///
/// 1. **Capability gate → 404.** `editor.kit` is a *declarative* gate of the
///    same kind as the `host.*` capabilities (`host_api.rs`): it is read
///    straight off the plugin's own manifest, and nothing at install time
///    whitelists it or asks the user to approve it. So it does NOT stop a
///    malicious plugin — one that simply writes `"editor.kit"` into its
///    manifest walks through. What it does buy: an ordinary plugin has no such
///    surface by default, and the declaration is a durable, auditable line in a
///    manifest the marketplace can review. Against a plugin that *has* declared
///    it, the real limits are steps 2–4 plus the standing premise that `dist/`
///    holds no secrets. The gate runs FIRST so an undeclared plugin gets a
///    byte-identical 404 for every `__host__` URL — well-formed or not — and
///    cannot infer the reserved prefix exists by diffing status codes.
/// 2. **Segment validation → 403.** Empty, `.` and `..` segments are rejected.
///    This matters beyond tidiness: in dev builds `AssetResolver` resolves
///    against the `frontendDist` *directory* and `PathBuf::components()` keeps
///    `ParentDir`, so an unchecked `..` really would read outside `dist/`.
///    Percent-escapes are refused here rather than decoded: `get_asset`
///    percent-decodes again downstream (`manager/mod.rs`), and declining to
///    decode at all means this boundary never depends on reasoning about how
///    many decode passes each side performs. No bundled asset name needs an
///    escape, so the ban costs nothing.
/// 3. **Directory confinement → 404.** Only [`HOST_ASSET_DIR`] is mirrored;
///    the dist root's HTML entry points are structurally out of reach. 404
///    rather than 403, to stay consistent with step 1's silence.
/// 4. **Read-only GET** (this function is only reachable from the GET arm).
fn route_host_asset(capabilities: &[String], rest: &str) -> Routed {
    if !capabilities.iter().any(|c| c == EDITOR_KIT_CAP) {
        return Routed::Response(plain(http::StatusCode::NOT_FOUND, "not found"));
    }
    let bad_segment = |seg: &str| {
        seg.is_empty() || seg == "." || seg == ".." || seg.contains('%') || seg.contains('\\')
    };
    if rest.split('/').any(bad_segment) {
        return Routed::Response(plain(http::StatusCode::FORBIDDEN, "forbidden"));
    }
    if !rest.starts_with(HOST_ASSET_DIR) {
        return Routed::Response(plain(http::StatusCode::NOT_FOUND, "not found"));
    }
    Routed::HostAsset(format!("/{rest}"))
}

/// True when `bytes` open an HTML document.
///
/// Guards the host-asset branch against a Tauri quirk: in production
/// `AssetResolver::get` falls back to the app shell's `index.html` for ANY
/// unresolved key instead of returning `None`, so a stale/renamed chunk would
/// otherwise be answered `200` with HTML under a `text/javascript`
/// content-type — a baffling parse error inside the plugin webview instead of
/// an honest 404. No host asset we serve here is HTML.
///
/// This is defence in depth, not the boundary: the app's HTML entry points live
/// at the dist root and are already out of reach via [`HOST_ASSET_DIR`].
pub fn is_html_document(bytes: &[u8]) -> bool {
    let body = bytes.strip_prefix(b"\xef\xbb\xbf".as_slice()).unwrap_or(bytes);
    let head = &body[..body.len().min(64)];
    let head = String::from_utf8_lossy(head).trim_start().to_ascii_lowercase();
    head.starts_with("<!doctype") || head.starts_with("<html")
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
        Routed::HostAsset(asset_path) => serve_host_asset(app, &asset_path),
    }
}

/// Read a host-bundled frontend asset through `AssetResolver` (spec §3.4).
///
/// # Dev mode
///
/// This works under `pnpm tauri dev` too, but only because Tauri's
/// `AssetResolver::get_for_scheme` has a `#[cfg(dev)]` branch that falls back
/// to reading `frontendDist` (`../dist`) **from disk** when `devUrl` is set —
/// assets are not embedded in dev builds. So the bytes come from the last
/// `pnpm build`, NOT from the Vite dev server: `dist/` must exist and be fresh,
/// or the kit is missing (404) / stale. Release builds read the embedded copy.
fn serve_host_asset<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    asset_path: &str,
) -> http::Response<Vec<u8>> {
    let Some(asset) = app.asset_resolver().get(asset_path.to_string()) else {
        return plain(http::StatusCode::NOT_FOUND, "not found");
    };
    // Move, don't clone: the largest bundled chunks run to ~1.3 MB and this
    // runs once per request.
    let bytes = asset.bytes;
    // See `is_html_document`: production lookups fall back to index.html.
    if is_html_document(&bytes) {
        return plain(http::StatusCode::NOT_FOUND, "not found");
    }
    http::Response::builder()
        .status(http::StatusCode::OK)
        .header("content-type", mime_for(Path::new(asset_path)))
        .header("cache-control", "no-cache")
        .body(bytes)
        .unwrap()
}

/// Read the persisted UI theme from settings.json (mirrors `read_saved_theme`
/// in `windows.rs`; kept local so this module stays self-contained). Defaults
/// to `"default"` when the file is missing/unreadable or the key is absent.
///
/// Delegates to `themes::commands::parse_theme_settings` (same reasoning as
/// `windows::read_saved_theme`): the `theme` key has been an object since
/// 4517e63, so a plain `.as_str()` read always missed and this always
/// returned `"default"` — wrong for every real vault.
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
    crate::themes::commands::parse_theme_settings(&json).0
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
        view_with_caps(dir, vec!["toast".to_string()])
    }

    /// Same as [`view_for`] but with an explicit capability list (for the
    /// `editor.kit`-gated `__host__` route).
    fn view_with_caps(dir: &Path, capabilities: Vec<String>) -> MapView {
        let mut map = HashMap::new();
        map.insert("test.plugin".to_string(), (dir.to_path_buf(), capabilities));
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

    /// Unwrap the `Routed::Response` branch (panics on the other variants).
    fn resp(r: Routed) -> http::Response<Vec<u8>> {
        match r {
            Routed::Response(r) => r,
            Routed::Rpc(..) => panic!("expected a direct response, got Routed::Rpc"),
            Routed::HostAsset(p) => panic!("expected a direct response, got Routed::HostAsset({p})"),
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
            other => panic!("missing origin must route, got status {}", resp(other).status()),
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
            other => panic!("expected Routed::Rpc, got status {}", resp(other).status()),
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

    // ── __host__ (Editor Kit host assets) ───────────────────────────────

    #[test]
    fn host_asset_route_requires_editor_kit_capability() {
        let dir = ui_fixture();
        // 无 editor.kit → 404
        let view = view_with_caps(dir.path(), vec!["vault.read".into()]);
        let r = resp(handle_parsed(
            &view, "GET", "test.plugin", "/__host__/assets/editor-kit-v1.js", None, "en", "default",
        ));
        assert_eq!(r.status(), http::StatusCode::NOT_FOUND);

        // 有 editor.kit → HostAsset,且 __host__ 前缀被剥掉
        let view = view_with_caps(dir.path(), vec!["editor.kit".into()]);
        match handle_parsed(
            &view, "GET", "test.plugin", "/__host__/assets/chunk-abc.js", None, "en", "default",
        ) {
            Routed::HostAsset(p) => assert_eq!(p, "/assets/chunk-abc.js"),
            other => panic!("expected HostAsset, got status {}", resp(other).status()),
        }

        // 路径穿越照旧拒绝
        let r = resp(handle_parsed(
            &view, "GET", "test.plugin", "/__host__/../secret", None, "en", "default",
        ));
        assert_eq!(r.status(), http::StatusCode::FORBIDDEN);
    }

    /// `__host__/` is a read-only mirror of the WHOLE host asset tree, not a
    /// two-file allowlist: the kit entry statically imports hashed shared
    /// chunks and the moraya runtime dynamically imports more.
    #[test]
    fn host_asset_maps_arbitrary_asset_paths() {
        let dir = ui_fixture();
        let view = view_with_caps(dir.path(), vec!["editor.kit".into()]);
        for (url, expected) in [
            ("/__host__/assets/editor-kit-v1.js", "/assets/editor-kit-v1.js"),
            ("/__host__/assets/editor-kit-v1.css", "/assets/editor-kit-v1.css"),
            ("/__host__/assets/index-D3adB33f.js", "/assets/index-D3adB33f.js"),
            ("/__host__/assets/KaTeX_Main-Regular-x1.woff2", "/assets/KaTeX_Main-Regular-x1.woff2"),
            ("/__host__/assets/nested/deep/a.js", "/assets/nested/deep/a.js"),
        ] {
            match handle_parsed(&view, "GET", "test.plugin", url, None, "en", "default") {
                Routed::HostAsset(p) => assert_eq!(p, expected, "for {url}"),
                other => panic!("{url}: expected HostAsset, got {}", resp(other).status()),
            }
        }
    }

    /// Percent-escapes are refused outright (not decoded): `AssetResolver`
    /// percent-decodes downstream, so accepting them would reopen traversal
    /// via double encoding (`%252e%252e` → `%2e%2e` → `..`).
    #[test]
    fn host_asset_rejects_percent_escapes_and_bad_segments() {
        let dir = ui_fixture();
        let view = view_with_caps(dir.path(), vec!["editor.kit".into()]);
        for url in [
            "/__host__/%2e%2e/secret",
            "/__host__/assets/%252e%252e/secret",
            "/__host__/assets/../../secret",
            "/__host__/assets/./x.js",
            "/__host__/assets//x.js",
            "/__host__/",
            "/__host__/assets/..%2fsecret",
            "/__host__/assets/x\\..\\secret",
        ] {
            let r = resp(handle_parsed(&view, "GET", "test.plugin", url, None, "en", "default"));
            assert_eq!(r.status(), http::StatusCode::FORBIDDEN, "{url} must be 403");
        }
    }

    /// The mirror is confined to `assets/`. The dist root holds the app's own
    /// HTML entry points; keeping them unreachable must be a structural rule,
    /// not a job for `is_html_document`'s content sniffing.
    #[test]
    fn host_asset_is_confined_to_the_assets_dir() {
        let dir = ui_fixture();
        let view = view_with_caps(dir.path(), vec!["editor.kit".into()]);
        for url in [
            "/__host__/index.html",
            "/__host__/insights.html",
            "/__host__/daily-notes.html",
            "/__host__/plugin-market.html",
            "/__host__/assetsx/a.js", // prefix must be a whole segment
            "/__host__/assets",       // the directory itself is not an asset
            "/__host__/other/a.js",
        ] {
            let r = resp(handle_parsed(&view, "GET", "test.plugin", url, None, "en", "default"));
            assert_eq!(r.status(), http::StatusCode::NOT_FOUND, "{url} must be 404");
        }
    }

    /// An undeclared plugin must not be able to tell the reserved prefix exists
    /// by diffing responses: every `__host__` URL — well-formed or malformed —
    /// has to answer byte-identically to any other missing plugin asset. This
    /// is what the capability gate running BEFORE path validation buys, and
    /// what would silently break if someone reordered the two for fail-fast.
    #[test]
    fn host_asset_gate_precedes_validation_for_ungranted_plugins() {
        let dir = ui_fixture();
        let view = view_with_caps(dir.path(), vec![]);
        let baseline =
            resp(handle_parsed(&view, "GET", "test.plugin", "/nope.js", None, "en", "default"));
        for url in [
            "/__host__/assets/a.js",
            "/__host__/./x",
            "/__host__/",
            "/__host__/%2e%2e/x",
            "/__host__/../secret",
            "/__host__/index.html",
        ] {
            let r = resp(handle_parsed(&view, "GET", "test.plugin", url, None, "en", "default"));
            assert_eq!(r.status(), baseline.status(), "{url}");
            assert_eq!(r.body(), baseline.body(), "{url}");
        }
    }

    /// The reserved prefix is exactly `/__host__/` — a plugin's own asset whose
    /// name merely starts with those bytes is still served from its ui root.
    #[test]
    fn host_asset_prefix_does_not_shadow_plugin_assets() {
        let dir = ui_fixture();
        let view = view_with_caps(dir.path(), vec!["editor.kit".into()]);
        // Not the reserved prefix → normal (missing) plugin asset → 404, not HostAsset.
        let r = resp(handle_parsed(
            &view, "GET", "test.plugin", "/__host__evil.js", None, "en", "default",
        ));
        assert_eq!(r.status(), http::StatusCode::NOT_FOUND);
        // A real plugin asset still resolves normally for an editor.kit plugin.
        let r = resp(handle_parsed(&view, "GET", "test.plugin", "/app.js", None, "en", "default"));
        assert_eq!(r.status(), 200);
        assert_eq!(r.body(), b"console.log(1)");
    }

    /// Only GET reaches the host-asset mirror; the reserved path is read-only.
    #[test]
    fn host_asset_is_get_only() {
        let dir = ui_fixture();
        let view = view_with_caps(dir.path(), vec!["editor.kit".into()]);
        let r = resp(handle_parsed(
            &view, "POST", "test.plugin", "/__host__/assets/a.js",
            Some("plugin://test.plugin"), "en", "default",
        ));
        assert_eq!(r.status(), http::StatusCode::NOT_FOUND);
        let r = resp(handle_parsed(
            &view, "PUT", "test.plugin", "/__host__/assets/a.js", None, "en", "default",
        ));
        assert_eq!(r.status(), http::StatusCode::METHOD_NOT_ALLOWED);
    }

    /// An unknown plugin id never reaches the capability gate.
    #[test]
    fn host_asset_unknown_plugin_404() {
        let dir = ui_fixture();
        let view = view_with_caps(dir.path(), vec!["editor.kit".into()]);
        let r = resp(handle_parsed(
            &view, "GET", "other.plugin", "/__host__/assets/a.js", None, "en", "default",
        ));
        assert_eq!(r.status(), http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn html_fallback_sniffing() {
        // Tauri's production asset lookup falls back to index.html for unknown
        // keys; those bytes must never be served as a kit chunk.
        assert!(is_html_document(b"<!doctype html>\n<html>"));
        assert!(is_html_document(b"  \n<!DOCTYPE HTML>"));
        assert!(is_html_document(b"<html lang=\"en\">"));
        assert!(is_html_document(b"\xef\xbb\xbf<!doctype html>"), "BOM-prefixed");
        assert!(!is_html_document(b"import x from './y.js';"));
        assert!(!is_html_document(b".a{color:red}"));
        assert!(!is_html_document(b""));
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
