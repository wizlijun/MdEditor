//! Integration coverage for the ui-only plugin path (子项目② Task 5).
//!
//! Proves the whole ui-only plugin path end to end at the PURE / integration
//! layer — no real webview, no AppHandle:
//!   • discovery loads a ui-only manifest (no binary) — 子项目② Task 1;
//!   • the `plugin://` protocol resolves/serves the fixture assets and routes
//!     `POST /__rpc__` with Origin authentication — Task 2;
//!   • ui_rpc dispatches host methods behind the SAME capability gate the
//!     process side uses — Task 3.
//!
//! Approach: a plain integration test file. Every seam these tests need is
//! already `pub` on the `notemd_lib` crate — `discovery::scan_root`,
//! `protocol::{resolve_asset, mime_for, csp_header, handle_parsed, PluginView,
//! Routed}`, and `ui_rpc::{dispatch_with, HostServices, OpenOptions,
//! SaveOptions, DialogFilter}` plus `host_api::ToastEmitter`. Nothing here
//! reaches into a `#[cfg(test)]` item, so no test-support shim is required; the
//! StubServices pattern from ui_rpc's own unit tests is replicated locally
//! (those stubs are private to that module).

use std::path::{Path, PathBuf};

use notemd_lib::plugin_runtime::discovery::scan_root;
use notemd_lib::plugin_runtime::host_api::ToastEmitter;
use notemd_lib::plugin_runtime::protocol::{
    self, AssetError, PluginView, Routed,
};
use notemd_lib::plugin_runtime::state::{self, InstallState, InstalledPlugin};
use notemd_lib::plugin_runtime::ui_rpc::{
    dispatch_with, HostServices, OpenOptions, SaveOptions,
};
use plugin_protocol as proto;
use serde_json::json;

const PLUGIN_ID: &str = "test.ui-fixture";

/// Path to the on-disk fixture's `current/` install tree.
fn fixture_current() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/v2-ui")
        .join(PLUGIN_ID)
        .join("current")
}

/// The fixture's served `ui/` directory.
fn fixture_ui() -> PathBuf {
    fixture_current().join("ui")
}

/// Recursively copy `src` into `dst` (dst created if absent).
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

// ── discovery: ui-only manifest loads without a binary ─────────────────────

#[test]
fn discovery_loads_ui_only_plugin_without_binary() {
    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path();

    // Lay out the install tree exactly like discovery expects:
    // <root>/<id>/current/{manifest.json, ui/...}.
    copy_tree(&fixture_current(), &root.join(PLUGIN_ID).join("current"));

    // Enable it in state.json (mirrors dev-install / discovery's own tests).
    let mut st = InstallState::default();
    st.installed.insert(
        PLUGIN_ID.to_string(),
        InstalledPlugin { version: "1.0.0".into(), enabled: true },
    );
    state::save(root, &st).unwrap();

    // Host far ahead of the fixture's ">=0.0.0" — engines satisfied.
    let map = scan_root(root, "9.9.9");

    // The ui-only manifest must load: NO binary-presence rejection (②T1).
    assert_eq!(map.len(), 1, "ui-only plugin should be discovered");
    let (manifest, current) = &map[PLUGIN_ID];
    assert_eq!(manifest.id, PLUGIN_ID);
    assert!(manifest.binary.is_empty(), "fixture declares no binary");
    assert_eq!(manifest.ui.as_deref(), Some("ui/"));
    assert_eq!(*current, root.join(PLUGIN_ID).join("current"));

    // Window contribution + capabilities came through typed.
    assert_eq!(manifest.contributes.windows.len(), 1);
    let win = &manifest.contributes.windows[0];
    assert_eq!(win.id, "main");
    assert_eq!(win.entry, "index.html");
    assert_eq!(win.open_command.as_deref(), Some("open"));
    assert_eq!(
        manifest.capabilities,
        vec!["toast".to_string(), "vault.read".to_string()]
    );
}

// ── resolve_asset against the on-disk fixture ui/ ──────────────────────────

#[test]
fn resolve_asset_serves_fixture_and_blocks_traversal() {
    let ui = fixture_ui();

    let index = protocol::resolve_asset(&ui, "/index.html").unwrap();
    assert!(index.ends_with("index.html"));
    assert!(std::fs::read_to_string(&index).unwrap().contains("app.js"));

    let app = protocol::resolve_asset(&ui, "/app.js").unwrap();
    assert!(app.ends_with("app.js"));
    assert!(std::fs::read_to_string(&app).unwrap().contains("host.vault.info"));

    // `../manifest.json` escapes ui/ into current/ — must be rejected.
    assert_eq!(
        protocol::resolve_asset(&ui, "/../manifest.json").unwrap_err(),
        AssetError::Traversal
    );
}

#[test]
fn mime_and_csp_for_fixture_assets() {
    assert_eq!(protocol::mime_for(Path::new("index.html")), "text/html");
    assert_eq!(protocol::mime_for(Path::new("app.js")), "text/javascript");
    // 'self' under plugin://<id> is this plugin only. Remote access stays
    // denied except for the exact versioned font package used by the built-in
    // Effie theme. `blob:` remains the media exception: the Editor Kit's
    // MediaResolver (`src/editor-kit/media.ts`) has no other output form — it
    // reads vault bytes over the capability-gated bridge and hands moraya a
    // `URL.createObjectURL(...)`, because a plugin webview has zero Tauri IPC.
    assert_eq!(
        protocol::csp_header(PLUGIN_ID),
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net/npm/lxgw-wenkai-lite-webfont@1.7.0/; \
         font-src 'self' https://cdn.jsdelivr.net/npm/lxgw-wenkai-lite-webfont@1.7.0/; \
         img-src 'self' data: blob:; media-src 'self' blob:; connect-src 'self'; \
         object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'"
    );
}

// ── handle_parsed routing against a PluginView backed by the fixture ───────

/// A [`PluginView`] that resolves the fixture plugin to its on-disk ui/ dir and
/// the capabilities the fixture manifest declares.
struct FixtureView;

impl PluginView for FixtureView {
    fn lookup(&self, plugin_id: &str) -> Option<(PathBuf, Vec<String>)> {
        if plugin_id == PLUGIN_ID {
            Some((
                fixture_ui(),
                vec!["toast".to_string(), "vault.read".to_string()],
            ))
        } else {
            None
        }
    }
}

/// Unwrap a `Routed::Response` (panics on `Rpc`/`HostAsset`).
fn resp(r: Routed) -> tauri::http::Response<Vec<u8>> {
    match r {
        Routed::Response(r) => r,
        Routed::Rpc(..) => panic!("expected a direct response, got Routed::Rpc"),
        Routed::HostAsset(path) => {
            panic!("expected a direct response, got Routed::HostAsset({path})")
        }
    }
}

#[test]
fn handle_parsed_get_index_html_200_with_csp() {
    let r = resp(protocol::handle_parsed(
        &FixtureView,
        "GET",
        PLUGIN_ID,
        "/index.html",
        None,
        "en",
        "default",
    ));
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "text/html");
    assert_eq!(
        r.headers()["content-security-policy"].to_str().unwrap(),
        protocol::csp_header(PLUGIN_ID)
    );
    assert!(
        String::from_utf8_lossy(r.body()).contains("app.js"),
        "served body should be the fixture index.html"
    );
}

#[test]
fn handle_parsed_get_app_js_200_no_csp() {
    let r = resp(protocol::handle_parsed(
        &FixtureView,
        "GET",
        PLUGIN_ID,
        "/app.js",
        None,
        "en",
        "default",
    ));
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "text/javascript");
    assert!(
        r.headers().get("content-security-policy").is_none(),
        "only html carries a CSP header"
    );
}

#[test]
fn handle_parsed_rpc_correct_origin_routes_with_capabilities() {
    match protocol::handle_parsed(
        &FixtureView,
        "POST",
        PLUGIN_ID,
        "/__rpc__",
        Some(&format!("plugin://{PLUGIN_ID}")),
        "en",
        "default",
    ) {
        Routed::Rpc(id, capabilities) => {
            assert_eq!(id, PLUGIN_ID);
            assert!(
                capabilities.iter().any(|c| c == "vault.read"),
                "capabilities must include vault.read: {capabilities:?}"
            );
        }
        Routed::Response(r) => {
            panic!("expected Routed::Rpc, got status {}", r.status())
        }
        Routed::HostAsset(path) => {
            panic!("expected Routed::Rpc, got Routed::HostAsset({path})")
        }
    }
}

#[test]
fn handle_parsed_rpc_wrong_origin_403() {
    // A *foreign* origin is rejected. A missing `Origin` is not: WebKit omits the
    // header on a same-origin POST to a custom scheme, so `protocol.rs` treats
    // `None` as same-origin on purpose (see the comment there) — pinned by
    // `handle_parsed_rpc_missing_origin_is_same_origin` below.
    for origin in [Some("plugin://evil.plugin"), Some("tauri://localhost")] {
        let r = resp(protocol::handle_parsed(
            &FixtureView,
            "POST",
            PLUGIN_ID,
            "/__rpc__",
            origin,
            "en",
            "default",
        ));
        assert_eq!(r.status(), 403, "origin {origin:?} must be rejected");
    }
}

#[test]
fn handle_parsed_rpc_missing_origin_is_same_origin() {
    let r = protocol::handle_parsed(&FixtureView, "POST", PLUGIN_ID, "/__rpc__", None, "en", "default");
    assert!(
        matches!(r, Routed::Rpc(..)),
        "a missing Origin is WebKit's same-origin POST and must route as RPC",
    );
}

#[test]
fn handle_parsed_get_traversal_403() {
    let r = resp(protocol::handle_parsed(
        &FixtureView,
        "GET",
        PLUGIN_ID,
        "/../manifest.json",
        None,
        "en",
        "default",
    ));
    assert_eq!(r.status(), 403);
}

#[test]
fn handle_parsed_unknown_plugin_404() {
    let r = resp(protocol::handle_parsed(
        &FixtureView,
        "GET",
        "other.plugin",
        "/index.html",
        None,
        "en",
        "default",
    ));
    assert_eq!(r.status(), 404);
}

// ── ui_rpc capability gate via dispatch_with + a stub HostServices ─────────
//
// StubServices from ui_rpc's own tests is `#[cfg(test)]` private, so this
// replicates a minimal stub: a tempdir vault root, no dialogs, no clipboard.

struct StubServices {
    vault: Option<PathBuf>,
}

impl HostServices for StubServices {
    fn pick_paths(&self, _opts: &OpenOptions) -> Result<Option<Vec<PathBuf>>, String> {
        Ok(None)
    }
    fn pick_save(&self, _opts: &SaveOptions) -> Result<Option<PathBuf>, String> {
        Ok(None)
    }
    fn vault_root(&self) -> Option<PathBuf> {
        self.vault.clone()
    }
    fn wiki_daily_dirs(&self) -> (Option<String>, Option<String>) {
        (None, None)
    }
    fn clipboard_write(&self, _text: &str) -> Result<(), String> {
        Ok(())
    }
}

fn noop_emitter() -> ToastEmitter {
    std::sync::Arc::new(|_| {})
}

fn rpc_req(method: &str, params: serde_json::Value) -> proto::RpcRequest {
    proto::RpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(1),
        method: method.into(),
        params,
    }
}

/// The fixture grants `vault.read`, so `host.vault.info` (cap vault.read)
/// dispatches to a result.
#[tokio::test]
async fn ui_rpc_vault_info_granted_returns_result() {
    let vault = tempfile::tempdir().unwrap();
    let services = StubServices { vault: Some(vault.path().to_path_buf()) };
    let log_dir = tempfile::tempdir().unwrap();

    // Capabilities exactly as the fixture manifest declares them.
    let caps = vec!["toast".to_string(), "vault.read".to_string()];
    let r = dispatch_with(
        &services,
        PLUGIN_ID,
        &caps,
        rpc_req("host.vault.info", json!({})),
        log_dir.path(),
        &noop_emitter(),
    )
    .await;

    assert!(r.error.is_none(), "vault.read granted: {:?}", r.error);
    let result = r.result.expect("host.vault.info returns a result");
    assert_eq!(result["root"], vault.path().to_string_lossy().to_string());
    // Frontend default dir names apply when unset.
    assert_eq!(result["wiki_dir"], "wikipage");
    assert_eq!(result["daily_dir"], "dailynote");
}

/// `host.dialog.open` needs capability `dialog`, which the fixture does NOT
/// grant → capability denied (-32001).
#[tokio::test]
async fn ui_rpc_dialog_not_in_fixture_caps_is_denied() {
    let services = StubServices { vault: None };
    let log_dir = tempfile::tempdir().unwrap();

    let caps = vec!["toast".to_string(), "vault.read".to_string()];
    let r = dispatch_with(
        &services,
        PLUGIN_ID,
        &caps,
        rpc_req("host.dialog.open", json!({})),
        log_dir.path(),
        &noop_emitter(),
    )
    .await;

    let e = r.error.expect("dialog cap absent → error");
    assert_eq!(e.code, proto::ERR_CAPABILITY_DENIED);
    assert_eq!(e.code, -32001);
    assert!(e.message.contains("dialog"), "{}", e.message);
}
