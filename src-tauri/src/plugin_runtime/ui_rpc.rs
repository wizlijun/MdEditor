//! UI→host RPC dispatch (子项目② Task 3). A plugin window is granted ZERO
//! Tauri IPC; the `plugin://<id>/__rpc__` fetch endpoint (protocol.rs) is the
//! only bridge, and it authenticates the caller by request Origin. This module
//! is the handler behind that endpoint: it enforces the SAME capability table
//! as the process-side host_api (`method_capability`), then executes the method.
//!
//! # Threading contract
//!
//! The dialog methods BLOCK until the user closes the native panel
//! (`blocking_pick_files` etc. internally hop to the main thread and wait).
//! Dispatch must therefore never run on the main thread — the `plugin://`
//! scheme handler in lib.rs uses `register_asynchronous_uri_scheme_protocol`
//! and answers every request from a dedicated spawned thread, which keeps the
//! main run loop free to actually show the dialog.
//!
//! # Testability shape (deviation note)
//!
//! The plan sketched "async dialog + oneshot"; with the dedicated-thread model
//! above, a synchronous injectable trait is equivalent and simpler. Production
//! [`dispatch`] wraps a live `AppHandle` in [`TauriServices`]; unit tests call
//! [`dispatch_with`] with stubs, so the whole method surface is exercised with
//! NO real dialogs and NO AppHandle. Vault filesystem ops (read/write/exists/
//! list/mkdir) run directly against `services.vault_root()` — a tempdir root
//! fully substitutes for a real vault.
//!
//! # fs.read:dialog authorization
//!
//! `host.fs.read_text` may ONLY read a path that a prior `host.dialog.open` /
//! `host.dialog.save` returned in this session. The allow-set is a module-level
//! registry keyed by plugin id, and it is maintained by DISPATCH itself (not by
//! the injected services), so the invariant holds for every `HostServices`
//! implementation.
//!
//! # Error convention
//!
//! Execution failures → code -32000 (`proto::ERR_INTERNAL`) with a
//! `"<kind>: <detail>"` message; kinds: `vault_required` / `not_granted` /
//! `too_large` / `io` (io covers bad params, dialog and clipboard failures).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use plugin_protocol as proto;

use super::host_api::{handle_common, method_capability, ToastEmitter};

/// Read/write cap for `host.vault.read` / `host.vault.write` /
/// `host.fs.read_text` / `host.fs.read_bytes` (200 MB). NOTE:
/// `fs.read_bytes` base64-encodes the whole file into one RPC string (~1.33×
/// the file), so a read near this cap materializes ~266 MB of String plus the
/// JS-side decode — raised from 10 MB to admit large Roam exports, at that
/// memory cost. That trade is deliberate and *one-off*: it is paid once per
/// user-driven import of a file the user explicitly picked in a dialog.
/// `host.vault.read_bytes` deliberately does NOT share it — see
/// `MAX_VAULT_BYTES`.
const MAX_TEXT_BYTES: u64 = 200 * 1024 * 1024;

/// Read cap for `host.vault.read_bytes` (10 MB, spec §3.3).
///
/// Separate from `MAX_TEXT_BYTES` on purpose. `vault.read_bytes` is the Editor
/// Kit MediaResolver's byte source: it fires *implicitly*, once per embedded
/// image while a document renders, with no user gesture to pace it. Every read
/// base64-encodes the file into a single JSON-RPC string (~1.33×) that must be
/// serialized, crossed into the webview and decoded on the main thread, so
/// inheriting the 200 MB import cap would let one oversized image stall the
/// window with ~267 MB of string. 10 MB comfortably covers any image or short
/// clip a note embeds; anything larger fails fast with `too_large` and renders
/// as a broken `<img>` instead of freezing the UI.
const MAX_VAULT_BYTES: u64 = 10 * 1024 * 1024;

/// Standard base64 alphabet (RFC 4648, `+`/`/`, `=` padding).
const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 encode. Hand-rolled to avoid pulling `base64` in as a direct
/// dependency — the codebase already hand-rolls the matching decode in lib.rs.
fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_ALPHABET[((n >> 18) & 0x3f) as usize] as char);
        out.push(B64_ALPHABET[((n >> 12) & 0x3f) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[((n >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[(n & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Effective outline dir names when the vault-level settings leave them unset —
/// mirrors `DEFAULT_DIRS` in `src/lib/outline/dirs.svelte.ts`.
const DEFAULT_WIKI_DIR: &str = "wikipage";
const DEFAULT_DAILY_DIR: &str = "dailynote";

// ── fs.read:dialog granted-paths registry ───────────────────────────────

/// plugin_id → paths returned by dialogs this session. Process-global on
/// purpose: grants must survive across dispatches (one per HTTP request).
static GRANTED_PATHS: LazyLock<Mutex<HashMap<String, HashSet<PathBuf>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn grant_path(plugin_id: &str, path: &Path) {
    if let Ok(mut m) = GRANTED_PATHS.lock() {
        m.entry(plugin_id.to_string()).or_default().insert(path.to_path_buf());
    }
}

fn is_granted(plugin_id: &str, path: &Path) -> bool {
    GRANTED_PATHS
        .lock()
        .map(|m| m.get(plugin_id).is_some_and(|s| s.contains(path)))
        .unwrap_or(false)
}

/// Drop every fs.read:dialog grant held by `plugin_id`. `GRANTED_PATHS` is
/// process-global and would otherwise leak a plugin's dialog-granted paths for
/// the whole app lifetime; windows.rs wires this to the plugin window's
/// `Destroyed` event so a grant lives no longer than the window that earned it.
pub(crate) fn clear_grants(plugin_id: &str) {
    if let Ok(mut m) = GRANTED_PATHS.lock() {
        m.remove(plugin_id);
    }
}

// ── Injectable host services ────────────────────────────────────────────

/// A file filter for the native dialogs: a label plus its extensions.
#[derive(Debug, Clone)]
pub struct DialogFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

/// Options for `host.dialog.open`.
#[derive(Debug, Clone, Default)]
pub struct OpenOptions {
    pub title: Option<String>,
    pub filters: Vec<DialogFilter>,
    pub directory: bool,
    pub multiple: bool,
}

/// Options for `host.dialog.save`.
#[derive(Debug, Clone, Default)]
pub struct SaveOptions {
    pub title: Option<String>,
    pub default_filename: Option<String>,
    pub filters: Vec<DialogFilter>,
}

/// Every host effect dispatch needs a runtime for. Production wraps a live
/// `AppHandle` ([`TauriServices`]); tests inject stubs. `Send + Sync` so the
/// trait object can cross the async boundary.
pub trait HostServices: Send + Sync {
    /// Show an open dialog; blocks until closed. `None` = user cancelled.
    fn pick_paths(&self, opts: &OpenOptions) -> Result<Option<Vec<PathBuf>>, String>;
    /// Show a save dialog; blocks until closed. `None` = user cancelled.
    fn pick_save(&self, opts: &SaveOptions) -> Result<Option<PathBuf>, String>;
    /// The configured vault root, or `None` when no vault is configured.
    fn vault_root(&self) -> Option<PathBuf>;
    /// `(wiki_dir, daily_dir)` vault-relative names, each `None` when unset
    /// (dispatch applies the `DEFAULT_*` fallbacks).
    fn wiki_daily_dirs(&self) -> (Option<String>, Option<String>);
    /// Write UTF-8 text to the OS clipboard.
    fn clipboard_write(&self, text: &str) -> Result<(), String>;
    /// One-shot location read → `{country, province, city, poi}`. Blocks until
    /// a fix + reverse-geocode completes (or times out). Default: unsupported;
    /// the Tauri impl runs CoreLocation on the main thread (see `location.rs`).
    fn location_get(&self) -> Result<serde_json::Value, String> {
        Err("location not supported".into())
    }
    /// Open a vault file in the main editor window. Default: unavailable
    /// (the process sink has no main-window context). `abs_path` is the
    /// vault-resolved absolute path from `resolve_in_vault`.
    fn open_in_editor(&self, _abs_path: &Path) -> Result<(), String> {
        Err("io: editor.open is only available from a plugin UI window".into())
    }
    /// AI agent 中转：`command` 为 `"run-task"`/`"run-status"`,`context`/结果原样
    /// 透传给 `notemd.claude-agent` 插件。默认不可用；生产实现只在
    /// `TauriServices`(Task 4),经其对内部插件的直调完成中转。
    fn agent_execute(&self, _command: &str, _context: serde_json::Value) -> Result<serde_json::Value, String> {
        Err("agent_unavailable: no relay on this channel".into())
    }
    /// 推一条托盘全局提醒（角标 + 菜单项 + 点击动作）。默认不可用；生产实现只在
    /// `TauriServices`(Task 4),落地到 `reminders.rs` 的注册表。
    fn notify_user(&self, _params: &serde_json::Value) -> Result<serde_json::Value, String> {
        Err("notify not supported here".into())
    }
}

// ── Dispatch ────────────────────────────────────────────────────────────

fn err(id: Option<u64>, code: i64, message: String) -> proto::RpcResponse {
    proto::RpcResponse {
        jsonrpc: "2.0".into(),
        id: id.unwrap_or(0),
        result: None,
        error: Some(proto::RpcError { code, message }),
    }
}

fn ok(id: Option<u64>, result: serde_json::Value) -> proto::RpcResponse {
    proto::RpcResponse {
        jsonrpc: "2.0".into(),
        id: id.unwrap_or(0),
        result: Some(result),
        error: None,
    }
}

/// Method-routing decision (子项目②b). `host.*` methods are HOST APIs, served
/// locally by [`dispatch_with`] under the capability gate. Every other method is
/// the PLUGIN's own API surface (convention `plugin.<name>`): it is forwarded to
/// the plugin process and does NOT go through the host capability gate — the
/// caller is already Origin-authenticated (protocol.rs proved the request came
/// from this plugin's own window), and the plugin's `on_ui_request` self-gates.
pub fn is_host_method(method: &str) -> bool {
    method.starts_with("host.")
}

/// Capability gate for the methods [`dispatch`] intercepts BEFORE delegating to
/// [`dispatch_with`] — they need the live `AppHandle` (app config dir, compiled
/// theme artifacts), which the injectable [`HostServices`] deliberately does not
/// carry. `None` = allowed.
///
/// Fails CLOSED, matching the two shared gates ([`dispatch_with`] and
/// `host_api::make_sink`) code for code and wording for wording: unknown method
/// → -32601, missing capability → -32001. A future interception that mistypes
/// its method name is therefore rejected rather than silently served.
fn capability_denial(
    method: &str,
    capabilities: &[String],
    id: Option<u64>,
) -> Option<proto::RpcResponse> {
    match method_capability(method) {
        Some("__unknown__") => Some(err(
            id,
            proto::ERR_METHOD_NOT_FOUND,
            format!("unknown method {method}"),
        )),
        Some(cap) if !capabilities.iter().any(|c| c == cap) => Some(err(
            id,
            proto::ERR_CAPABILITY_DENIED,
            format!("method {method} requires capability '{cap}'"),
        )),
        _ => None,
    }
}

/// Production entry point: for `host.*` methods, builds the live services
/// (dialogs, vault, clipboard, toast emitter, plugin log dir) from `app` and
/// delegates to [`dispatch_with`]. For NON-host methods (the plugin's own API,
/// convention `plugin.<name>`), forwards to the plugin process via
/// [`forward_to_plugin`] — no host capability gate (子项目②b).
/// Called by protocol.rs for every authenticated `POST plugin://<id>/__rpc__`.
pub async fn dispatch<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    plugin_id: &str,
    capabilities: &[String],
    req: proto::RpcRequest,
) -> proto::RpcResponse {
    // Non-host method → forward to the plugin's own process (子项目②b). This
    // bypasses the host capability table on purpose: it is the plugin's API, and
    // the window's Origin already authenticates it as this plugin's own UI.
    if !is_host_method(&req.method) {
        let id = req.id;
        return match forward_to_plugin(app, plugin_id, &req.method, req.params).await {
            Ok(v) => ok(id, v),
            Err(detail) => err(id, proto::ERR_INTERNAL, detail),
        };
    }

    use tauri::Manager;

    // `host.theme.css` is answered HERE instead of in `dispatch_with`: the
    // bundle comes from the app config dir + compiled theme artifacts, i.e. it
    // needs the live AppHandle that the injectable `HostServices` deliberately
    // does not carry. The gate is the same capability table, applied manually.
    // The process channel therefore never serves it (it falls through
    // `make_sink` to -32601) — correct: a background plugin has no webview to
    // style.
    if req.method == "host.theme.css" {
        if let Some(denial) = capability_denial(&req.method, capabilities, req.id) {
            return denial;
        }
        return ok(req.id, crate::themes::commands::theme_css_bundle(app));
    }

    let log_dir = app
        .path()
        .app_log_dir()
        .map(|d| d.join("plugins"))
        .unwrap_or_else(|_| std::env::temp_dir());
    let emitter: ToastEmitter = {
        use tauri::Emitter;
        let app = app.clone();
        std::sync::Arc::new(move |payload| {
            let _ = app.emit("plugin-toast", payload);
        })
    };
    let services = TauriServices { app: app.clone() };
    dispatch_with(&services, plugin_id, capabilities, req, &log_dir, &emitter).await
}

/// Forward a UI-window RPC to the plugin's OWN process (子项目②b). Reuses the
/// exact lifecycle registration a menu command uses (`commands::get_or_register`),
/// activates the process if needed, then round-trips `ui.request`.
///
/// Prefix convention: a leading `plugin.` is STRIPPED before forwarding, so the
/// UI's `notemd.request('plugin.connect', …)` reaches the plugin's
/// `on_ui_request` as the clean method name `connect`. A non-host method without
/// the `plugin.` prefix is forwarded verbatim (both are supported; `plugin.` is
/// the documented convention).
async fn forward_to_plugin<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    plugin_id: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let forwarded = method.strip_prefix("plugin.").unwrap_or(method);
    let lc = super::commands::get_or_register(app, plugin_id)?;
    lc.ensure_active(&super::lifecycle::Trigger::Startup).await?;
    lc.ui_request(forwarded, params).await
}

/// Injectable core (unit tests / Task 5 integration tests). Mirrors the process
/// sink's capability gate exactly (unknown method → -32601, unauthorized →
/// -32001), runs `handle_common` (log/toast) first, then the dialog/vault/fs/
/// clipboard methods.
pub async fn dispatch_with(
    services: &dyn HostServices,
    plugin_id: &str,
    capabilities: &[String],
    req: proto::RpcRequest,
    log_dir: &Path,
    emitter: &ToastEmitter,
) -> proto::RpcResponse {
    let id = req.id;

    // Capability gate — identical to host_api::make_sink.
    match method_capability(&req.method) {
        Some("__unknown__") => {
            return err(
                id,
                proto::ERR_METHOD_NOT_FOUND,
                format!("unknown method {}", req.method),
            );
        }
        Some(cap) if !capabilities.iter().any(|c| c == cap) => {
            return err(
                id,
                proto::ERR_CAPABILITY_DENIED,
                format!("method {} requires capability '{cap}'", req.method),
            );
        }
        _ => {}
    }

    // Shared log/toast handling (same implementation as the process sink).
    if let Some(res) = handle_common(&req.method, req.params.clone(), plugin_id, log_dir, emitter) {
        return match res {
            Ok(v) => ok(id, v),
            Err(detail) => err(id, proto::ERR_INTERNAL, detail),
        };
    }

    let out: Result<serde_json::Value, String> = match req.method.as_str() {
        "host.dialog.open" => dialog_open(services, plugin_id, &req.params),
        "host.dialog.save" => dialog_save(services, plugin_id, &req.params),
        "host.fs.read_text" => fs_read_text(plugin_id, &req.params),
        "host.fs.read_bytes" => fs_read_bytes(plugin_id, &req.params),
        "host.clipboard.write" => clipboard_write(services, &req.params),
        "host.location.get" => services.location_get(),
        "host.vault.info" => Ok(vault_info(services)),
        "host.vault.read" => vault_read(services, &req.params),
        "host.vault.read_bytes" => vault_read_bytes(services, &req.params),
        "host.vault.write" => vault_write(services, &req.params),
        "host.vault.exists" => vault_exists(services, &req.params),
        "host.vault.list" => vault_list(services, &req.params),
        "host.vault.mkdir" => vault_mkdir(services, &req.params),
        "host.vault.remove" => vault_remove(services, &req.params),
        "host.vault.rename" => vault_rename(services, &req.params),
        "host.editor.open" => editor_open(services, &req.params),
        "host.agent.run"    => services.agent_execute("run-task", req.params.clone()),
        "host.agent.status" => services.agent_execute("run-status", req.params.clone()),
        "host.notify"       => notify_push(services, &req.params),
        // handle_common took log/toast; the gate rejected everything unknown.
        other => Err(format!("io: unhandled method {other}")),
    };
    match out {
        Ok(v) => ok(id, v),
        Err(detail) => err(id, proto::ERR_INTERNAL, detail),
    }
}

// ── Method bodies ────────────────────────────────────────────────────────

fn parse_filters(params: &serde_json::Value) -> Vec<DialogFilter> {
    params
        .get("filters")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    let name = f.get("name")?.as_str()?.to_string();
                    let extensions = f
                        .get("extensions")?
                        .as_array()?
                        .iter()
                        .filter_map(|e| e.as_str().map(str::to_string))
                        .collect();
                    Some(DialogFilter { name, extensions })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn opt_str(params: &serde_json::Value, key: &str) -> Option<String> {
    params.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

fn opt_bool(params: &serde_json::Value, key: &str) -> bool {
    params.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn req_str<'a>(params: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("io: param '{key}' (string) is required"))
}

/// `{ title?, filters?, directory?, multiple? } → { paths: [String] | null }`.
/// Every returned path is inserted into the fs.read:dialog allow-set.
fn dialog_open(
    services: &dyn HostServices,
    plugin_id: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let opts = OpenOptions {
        title: opt_str(params, "title"),
        filters: parse_filters(params),
        directory: opt_bool(params, "directory"),
        multiple: opt_bool(params, "multiple"),
    };
    let picked = services.pick_paths(&opts).map_err(|e| format!("io: dialog: {e}"))?;
    Ok(match picked {
        None => serde_json::json!({ "paths": null }),
        Some(paths) => {
            for p in &paths {
                grant_path(plugin_id, p);
            }
            let strs: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            serde_json::json!({ "paths": strs })
        }
    })
}

/// `{ title?, default_filename?, filters? } → { path: String | null }`.
/// The chosen path is inserted into the fs.read:dialog allow-set.
fn dialog_save(
    services: &dyn HostServices,
    plugin_id: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let opts = SaveOptions {
        title: opt_str(params, "title"),
        default_filename: opt_str(params, "default_filename"),
        filters: parse_filters(params),
    };
    let picked = services.pick_save(&opts).map_err(|e| format!("io: dialog: {e}"))?;
    Ok(match picked {
        None => serde_json::json!({ "path": null }),
        Some(path) => {
            grant_path(plugin_id, &path);
            serde_json::json!({ "path": path.to_string_lossy() })
        }
    })
}

/// UTF-8 read with the `MAX_TEXT_BYTES` cap; shared by vault.read and fs.read_text.
fn read_text_capped(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("io: {e}"))?;
    if meta.len() > MAX_TEXT_BYTES {
        return Err(format!("too_large: file exceeds {MAX_TEXT_BYTES} bytes"));
    }
    std::fs::read_to_string(path).map_err(|e| format!("io: {e}"))
}

/// `{ path } → { content }` — only for paths a dialog returned this session.
fn fs_read_text(plugin_id: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let path = PathBuf::from(req_str(params, "path")?);
    if !is_granted(plugin_id, &path) {
        return Err("not_granted: path not granted via dialog".into());
    }
    Ok(serde_json::json!({ "content": read_text_capped(&path)? }))
}

/// `{ path } → { base64 }` — raw bytes (base64-encoded) of a dialog-granted
/// path, subject to the same `MAX_TEXT_BYTES` cap. Used for binary exports the UTF-8 text
/// bridge cannot carry (e.g. Roam's `.zip` export, unzipped client-side).
fn fs_read_bytes(plugin_id: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let path = PathBuf::from(req_str(params, "path")?);
    if !is_granted(plugin_id, &path) {
        return Err("not_granted: path not granted via dialog".into());
    }
    let meta = std::fs::metadata(&path).map_err(|e| format!("io: {e}"))?;
    if meta.len() > MAX_TEXT_BYTES {
        return Err(format!("too_large: file exceeds {MAX_TEXT_BYTES} bytes"));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("io: {e}"))?;
    Ok(serde_json::json!({ "base64": base64_encode(&bytes) }))
}

/// `{ text } → { ok: true }`.
fn clipboard_write(
    services: &dyn HostServices,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let text = req_str(params, "text")?;
    services.clipboard_write(text).map_err(|e| format!("io: clipboard: {e}"))?;
    Ok(serde_json::json!({ "ok": true }))
}

/// `{} → { root, wiki_dir, daily_dir }` (all null when no vault is configured;
/// dir names fall back to the frontend's defaults when unset).
pub(crate) fn vault_info(services: &dyn HostServices) -> serde_json::Value {
    match services.vault_root() {
        None => serde_json::json!({ "root": null, "wiki_dir": null, "daily_dir": null }),
        Some(root) => {
            let (wiki, daily) = services.wiki_daily_dirs();
            serde_json::json!({
                "root": root.to_string_lossy(),
                "wiki_dir": wiki.unwrap_or_else(|| DEFAULT_WIKI_DIR.into()),
                "daily_dir": daily.unwrap_or_else(|| DEFAULT_DAILY_DIR.into()),
            })
        }
    }
}

/// Vault-relative-path sanitization shared by [`resolve_in_vault`] and
/// [`vault_leaf_path`]: rejects absolute paths and any `..` segment,
/// collapses `.`. Pure and lexical — no filesystem access, no canonicalize.
fn sanitize_rel(rel_raw: &str) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel_raw.trim());
    if rel_path.is_absolute() {
        return Err("io: path must be vault-relative".into());
    }
    let mut rel = PathBuf::new();
    for comp in rel_path.components() {
        use std::path::Component;
        match comp {
            Component::Normal(seg) => rel.push(seg),
            Component::CurDir => {}
            Component::ParentDir => return Err("io: path escapes the vault".into()),
            Component::RootDir | Component::Prefix(_) => {
                return Err("io: path must be vault-relative".into())
            }
        }
    }
    Ok(rel)
}

/// Resolve a plugin-supplied vault-relative `path` to an absolute path that is
/// guaranteed to stay within the vault root:
/// 1. lexical: absolute paths and any `..` segment are rejected outright;
/// 2. canonicalize-containment: the deepest EXISTING ancestor (write targets
///    may not exist yet) is canonicalized and must remain under the
///    canonicalized root — this also defeats symlink escapes.
///
/// NOTE: step 2 canonicalizes the WHOLE path, including the final component —
/// i.e. if `path` names a live symlink, the returned `PathBuf` is the link's
/// resolved TARGET, not the link itself. That is exactly right for read/write
/// (follow the link, act on its content) but wrong for an operation that must
/// act on the directory ENTRY itself (delete/rename a link without touching
/// whatever it points to) — those callers must use [`vault_leaf_path`] instead.
fn resolve_in_vault(services: &dyn HostServices, params: &serde_json::Value) -> Result<PathBuf, String> {
    let root = services
        .vault_root()
        .ok_or_else(|| "vault_required: configure a Vault first".to_string())?;
    let rel = sanitize_rel(req_str(params, "path")?)?;
    let root_c = root
        .canonicalize()
        .map_err(|e| format!("io: vault root unavailable: {e}"))?;
    contained_in(&root_c, root_c.join(&rel))
}

/// Like [`resolve_in_vault`], but for operations that must act on the leaf
/// ENTRY itself — `host.vault.remove` / `host.vault.rename` — rather than
/// whatever a symlink at that leaf resolves to.
///
/// `resolve_in_vault`'s canonicalize-the-whole-path semantics dereference the
/// final component: a live symlink `link.md -> real.md` (both inside the
/// vault, so no escape) would resolve to `real.md`'s path, silently
/// redirecting a delete/rename onto a DIFFERENT file the caller never named —
/// exactly the "wrong file" bug that motivates this function.
///
/// It still runs the full `resolve_in_vault` fence first (result discarded,
/// checked for `Err` only), so a genuine escape is rejected exactly as before
/// — including via a symlinked ANCESTOR directory, or a leaf symlink whose
/// target lies outside the vault. Only once that fence has passed does it
/// re-join the same sanitized relative path onto the (uncanonicalized) vault
/// root, so the final component is never resolved: `symlink_metadata` /
/// `remove_file` / `rename` on the result see the entry itself.
fn vault_leaf_path(services: &dyn HostServices, params: &serde_json::Value) -> Result<PathBuf, String> {
    resolve_in_vault(services, params)?;
    let root = services
        .vault_root()
        .ok_or_else(|| "vault_required: configure a Vault first".to_string())?;
    let rel = sanitize_rel(req_str(params, "path")?)?;
    Ok(root.join(rel))
}

/// Step 2 of the fence, on its own so callers that start from an ABSOLUTE path
/// (`host.notify`'s open_path) get the identical guarantee: canonicalize the
/// deepest EXISTING ancestor (write targets may not exist yet), require it to
/// stay under the canonicalized root — which also defeats symlink escapes —
/// then re-append the not-yet-existing tail.
fn contained_in(root_c: &Path, target: PathBuf) -> Result<PathBuf, String> {
    let mut probe = target;
    let mut missing_tail: Vec<std::ffi::OsString> = Vec::new();
    let canon = loop {
        match probe.canonicalize() {
            Ok(c) => break c,
            Err(_) => {
                let Some(name) = probe.file_name() else {
                    return Err("io: path escapes the vault".into());
                };
                missing_tail.push(name.to_os_string());
                let Some(parent) = probe.parent() else {
                    return Err("io: path escapes the vault".into());
                };
                probe = parent.to_path_buf();
            }
        }
    };
    if !canon.starts_with(root_c) {
        return Err("io: path escapes the vault".into());
    }
    let mut out = canon;
    for name in missing_tail.into_iter().rev() {
        out.push(name);
    }
    Ok(out)
}

/// A reminder's `open_path`, fenced to the vault. Unlike `host.vault.*` this
/// one ACCEPTS an absolute path (the pushing plugin knows the run's real target
/// and the tray click handler needs an absolute path anyway), but it must still
/// land inside the vault: without this a plugin holding only `notify` could get
/// the user to one-click open `~/.ssh/config`, i.e. more reach than
/// `editor.open` — which does go through `resolve_in_vault`.
fn resolve_reminder_path(services: &dyn HostServices, raw: &str) -> Result<PathBuf, String> {
    let root = services
        .vault_root()
        .ok_or_else(|| "vault_required: configure a Vault first".to_string())?;
    let root_c = root
        .canonicalize()
        .map_err(|e| format!("io: vault root unavailable: {e}"))?;
    let p = Path::new(raw.trim());
    if p.as_os_str().is_empty() {
        return Err("io: path must not be empty".into());
    }
    if p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("io: path escapes the vault".into());
    }
    let target = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root_c.join(p)
    };
    contained_in(&root_c, target)
}

/// `host.notify` → the tray reminder registry. The `open_path` action is fenced
/// to the vault here (and rewritten to its canonical absolute form) before the
/// reminder is ever registered.
pub(crate) fn notify_push(
    services: &dyn HostServices,
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let is_open_path = params.pointer("/action/kind").and_then(|v| v.as_str()) == Some("open_path");
    if !is_open_path {
        return services.notify_user(params);
    }
    let raw = params
        .pointer("/action/path")
        .and_then(|v| v.as_str())
        .ok_or("io: open_path needs a 'path'")?;
    let abs = resolve_reminder_path(services, raw)?;
    let mut params = params.clone();
    params["action"]["path"] = serde_json::json!(abs.to_string_lossy());
    services.notify_user(&params)
}

/// `{ path } → { content }` (UTF-8, `MAX_TEXT_BYTES` cap).
pub(crate) fn vault_read(services: &dyn HostServices, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p = resolve_in_vault(services, params)?;
    Ok(serde_json::json!({ "content": read_text_capped(&p)? }))
}

/// `{ path } → { base64 }` — vault-internal file's raw bytes (base64-encoded),
/// capped at `MAX_VAULT_BYTES` (10 MB, spec §3.3 — deliberately far below the
/// `MAX_TEXT_BYTES` import cap; see that constant). Used by isolated plugin
/// webviews (zero Tauri IPC) to render vault-hosted images.
pub(crate) fn vault_read_bytes(services: &dyn HostServices, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p = resolve_in_vault(services, params)?;
    let meta = std::fs::metadata(&p).map_err(|e| format!("io: {e}"))?;
    if meta.len() > MAX_VAULT_BYTES {
        return Err(format!("too_large: file exceeds {MAX_VAULT_BYTES} bytes"));
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("io: {e}"))?;
    Ok(serde_json::json!({ "base64": base64_encode(&bytes) }))
}

/// `{ path, content } → { ok: true }`; creates parent directories. Content is
/// capped at the same `MAX_TEXT_BYTES` as reads (UTF-8 byte length).
pub(crate) fn vault_write(services: &dyn HostServices, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p = resolve_in_vault(services, params)?;
    let content = req_str(params, "content")?;
    if content.len() as u64 > MAX_TEXT_BYTES {
        return Err(format!("too_large: content exceeds {MAX_TEXT_BYTES} bytes"));
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("io: {e}"))?;
    }
    std::fs::write(&p, content).map_err(|e| format!("io: {e}"))?;
    // A `.sh` written through this bridge is almost always an agent-task
    // precheck hook, and claude-agent's runner is fail-OPEN on spawn failure
    // (`precheck.rs::run`: a script it cannot execute is treated as "proceed").
    // A precheck that isn't executable is therefore a guard that silently never
    // runs — the same reason claude-agent chmods its own built-in templates
    // (`plugins-src/claude-agent/backend/src/task.rs` `seed_builtin_templates`).
    // Best effort: a failed chmod must not fail the write.
    #[cfg(unix)]
    if p.extension().and_then(|e| e.to_str()) == Some("sh") {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
    }
    Ok(serde_json::json!({ "ok": true }))
}

/// `{ path } → { exists: bool }`.
pub(crate) fn vault_exists(services: &dyn HostServices, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p = resolve_in_vault(services, params)?;
    Ok(serde_json::json!({ "exists": p.exists() }))
}

/// `{ path } → { entries: [{ name, is_dir }] }`, sorted by name.
pub(crate) fn vault_list(services: &dyn HostServices, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p = resolve_in_vault(services, params)?;
    let mut entries: Vec<(String, bool)> = Vec::new();
    for entry in std::fs::read_dir(&p).map_err(|e| format!("io: {e}"))? {
        let entry = entry.map_err(|e| format!("io: {e}"))?;
        if let Some(name) = entry.file_name().to_str() {
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            entries.push((name.to_string(), is_dir));
        }
    }
    entries.sort();
    let entries: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|(name, is_dir)| serde_json::json!({ "name": name, "is_dir": is_dir }))
        .collect();
    Ok(serde_json::json!({ "entries": entries }))
}

/// `{ path } → { ok: true }` (mkdir -p).
pub(crate) fn vault_mkdir(services: &dyn HostServices, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p = resolve_in_vault(services, params)?;
    std::fs::create_dir_all(&p).map_err(|e| format!("io: {e}"))?;
    Ok(serde_json::json!({ "ok": true }))
}

/// `{ path } → { ok: true }`. 只删文件:目录要靠别的方式清理,插件误传一个
/// 目录名不该把整棵子树带走。目标不存在按成功处理(幂等,调用方重试安全)。
///
/// Uses [`vault_leaf_path`], NOT `resolve_in_vault`: `path` naming a live
/// symlink must remove the LINK entry itself, never dereference it and delete
/// whatever it points to (see `vault_leaf_path`'s doc). `symlink_metadata`
/// (not `metadata`) is what makes that possible — it reports the link itself
/// instead of following it, so a symlink is caught by neither the "already
/// gone" nor the "is a directory" arm below and falls straight to
/// `remove_file`, which likewise removes the link entry, not its target.
pub(crate) fn vault_remove(services: &dyn HostServices, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p = vault_leaf_path(services, params)?;
    match std::fs::symlink_metadata(&p) {
        Err(_) => return Ok(serde_json::json!({ "ok": true })), // 已不存在
        Ok(m) if m.is_dir() => return Err("io: refusing to remove a directory".into()),
        Ok(_) => {}
    }
    std::fs::remove_file(&p).map_err(|e| format!("io: {e}"))?;
    Ok(serde_json::json!({ "ok": true }))
}

/// `{ from, to } → { ok: true }`。两端都过 vault 围栏;目标已存在一律报错而不
/// 覆盖 —— 重命名撞名时静默吃掉用户的另一个文件是不可接受的。
///
/// Both ends resolve through [`vault_leaf_path`] (not `resolve_in_vault`): a
/// live symlink named by `from` or `to` must be moved/targeted as the link
/// entry itself, not silently redirected onto whatever it points to.
///
/// The "must not already exist" check is an atomic `O_EXCL` create
/// (`create_new(true)`), not `to.exists()` followed by `rename` — two bugs in
/// one fix:
/// - TOCTOU: `exists()` then `rename` leaves a window where a concurrently
///   created `to` gets silently overwritten (POSIX `rename` clobbers its
///   destination unconditionally). `create_new` performs the "does it exist"
///   check and the create atomically at the OS level.
/// - dangling symlinks: `Path::exists()` follows symlinks, so a dangling one
///   at `to` reads as "doesn't exist" and would be silently clobbered.
///   `create_new`'s `O_EXCL` fails on ANY existing directory entry at `to` —
///   symlink, dangling or not — exactly like `symlink_metadata` used for the
///   same reason in `vault_remove`.
///
/// The placeholder file `create_new` leaves at `to` is what `rename`
/// atomically replaces immediately after — and if that `rename` fails (most
/// commonly: `from` doesn't exist), the placeholder is explicitly removed
/// before the error is returned, so a failed rename never leaves behind a
/// 0-byte file the user never created.
pub(crate) fn vault_rename(services: &dyn HostServices, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let from = vault_leaf_path(services, &serde_json::json!({ "path": req_str(params, "from")? }))?;
    let to = vault_leaf_path(services, &serde_json::json!({ "path": req_str(params, "to")? }))?;
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("io: {e}"))?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&to)
        .map_err(|_| "io: destination already exists".to_string())?;
    // If `rename` fails (most commonly: `from` doesn't exist — a stale UI
    // state, a concurrent delete, a typo'd path), the placeholder file
    // `create_new` just planted at `to` must not be left behind: that would
    // be a 0-byte file the user never created, silently appearing in the
    // vault. Best-effort cleanup — a failed remove here must not shadow the
    // real rename error.
    if let Err(e) = std::fs::rename(&from, &to) {
        let _ = std::fs::remove_file(&to);
        return Err(format!("io: {e}"));
    }
    Ok(serde_json::json!({ "ok": true }))
}

/// `{ path } → { ok: true }`. Resolves a vault-relative path and opens it in
/// the main editor (focuses the main window). UI-bridge only — the process
/// sink's default `open_in_editor` returns an error.
pub(crate) fn editor_open(services: &dyn HostServices, params: &serde_json::Value) -> Result<serde_json::Value, String> {
    let p = resolve_in_vault(services, params)?;
    services.open_in_editor(&p)?;
    Ok(serde_json::json!({ "ok": true }))
}

// ── Production HostServices (live AppHandle) ──────────────────────────────

/// Live implementation wired to a Tauri `AppHandle`. Constructed per-dispatch
/// by [`dispatch`]. The `blocking_*` dialog calls hop to the main thread
/// internally and wait — safe here because dispatch never runs on main (see
/// module doc, "Threading contract").
pub struct TauriServices<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriServices<R> {
    /// 供进程 sink（host_api::make_sink_for_app）复用同一套 vault 实现。
    pub(crate) fn new(app: tauri::AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: tauri::Runtime> HostServices for TauriServices<R> {
    fn pick_paths(&self, opts: &OpenOptions) -> Result<Option<Vec<PathBuf>>, String> {
        use tauri_plugin_dialog::{DialogExt, FilePath};
        let mut builder = self.app.dialog().file();
        if let Some(t) = &opts.title {
            builder = builder.set_title(t.as_str());
        }
        for f in &opts.filters {
            let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
            builder = builder.add_filter(f.name.as_str(), &exts);
        }
        let single = |p: Option<FilePath>| p.map(|p| vec![p]);
        let picked: Option<Vec<FilePath>> = match (opts.directory, opts.multiple) {
            (true, true) => builder.blocking_pick_folders(),
            (true, false) => single(builder.blocking_pick_folder()),
            (false, true) => builder.blocking_pick_files(),
            (false, false) => single(builder.blocking_pick_file()),
        };
        match picked {
            None => Ok(None),
            Some(files) => files
                .into_iter()
                .map(|f| f.into_path().map_err(|e| e.to_string()))
                .collect::<Result<Vec<PathBuf>, String>>()
                .map(Some),
        }
    }

    fn pick_save(&self, opts: &SaveOptions) -> Result<Option<PathBuf>, String> {
        use tauri_plugin_dialog::DialogExt;
        let mut builder = self.app.dialog().file();
        if let Some(t) = &opts.title {
            builder = builder.set_title(t.as_str());
        }
        if let Some(name) = &opts.default_filename {
            builder = builder.set_file_name(name.as_str());
        }
        for f in &opts.filters {
            let exts: Vec<&str> = f.extensions.iter().map(String::as_str).collect();
            builder = builder.add_filter(f.name.as_str(), &exts);
        }
        match builder.blocking_save_file() {
            None => Ok(None),
            Some(p) => p.into_path().map(Some).map_err(|e| e.to_string()),
        }
    }

    fn vault_root(&self) -> Option<PathBuf> {
        crate::sotvault::resolve_vault_root(&self.app)
    }

    fn wiki_daily_dirs(&self) -> (Option<String>, Option<String>) {
        let Some(root) = self.vault_root() else {
            return (None, None);
        };
        let settings = crate::sotvault::vault_settings::read(&root);
        // Same validation resolve_sync_dir applies: invalid configured values
        // fall back to the defaults (returned as None here).
        let valid = |v: Option<String>| {
            v.and_then(|s| crate::sotvault::vault_settings::validate_rel_dir(&s).ok())
        };
        (valid(settings.wikipage_dir), valid(settings.dailynote_dir))
    }

    fn clipboard_write(&self, text: &str) -> Result<(), String> {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        self.app.clipboard().write_text(text).map_err(|e| e.to_string())
    }

    fn location_get(&self) -> Result<serde_json::Value, String> {
        // fetch_once kicks CoreLocation off on the main thread and blocks this
        // (off-main, per-request) thread on a condvar until the delegate/geocode
        // completes. Blocking here is fine.
        super::location::fetch_once(&self.app)
    }

    fn open_in_editor(&self, abs_path: &Path) -> Result<(), String> {
        let s = abs_path
            .to_str()
            .ok_or_else(|| "io: path is not valid UTF-8".to_string())?;
        crate::emit_open_file_delayed(&self.app, s);
        crate::show_main_window(&self.app);
        Ok(())
    }

    /// host.agent.* 中转:同步 trait 方法,但 lifecycle 是 async——spawn 到
    /// tauri 异步运行时,std channel 等结果。
    ///
    /// 阻塞的是**调用线程**,两条通道都不再碰读循环:
    /// - 进程通道(后台插件经 host_api sink 调用)阻塞的是宿主为该插件的 host.*
    ///   队列单开的**一条线程**(见 `process::PluginProcess::spawn`),读循环照常
    ///   路由应答;同一插件后续的 host.* 排在它后面,仅此而已;
    /// - UI 通道阻塞的是宿主为这条 `plugin://` 请求单开的**一条 OS 线程**
    ///   (lib.rs 的 register_asynchronous_uri_scheme_protocol 里
    ///   `std::thread::spawn`),不占 tokio worker,也不碰主线程。
    ///
    /// 这里**不再自设超时**。曾经是 30s、后来 300s,两次都栽在同一件事上:那个
    /// spawn 出去的 future 仍在跑、run-task 往往已经登记成功并开始阅读 —— run_id
    /// 丢了,调用方判失败发失败提醒,claude 却照常跑完落盘,用户看到"失败"、磁盘
    /// 上却是成功(v6.804.5 的「agent relay timeout after 300s」:实际 4 分 48 秒
    /// 成功收尾)。任何固定上限都是在拿"跑得久"当"挂了"。
    ///
    /// 不设上限也不会永久挂起:spawn 的 future 自身一定收敛 —— `execute` 的静默
    /// 超时(见 `process::PluginProcess::request`,只有插件彻底不出声才触发)、
    /// 插件进程死亡时的 pending drain、以及 future panic 时的 tx 落地(下面的
    /// Disconnected 分支)兜住了全部出口。
    fn agent_execute(&self, command: &str, context: serde_json::Value) -> Result<serde_json::Value, String> {
        const AGENT_PLUGIN: &str = "notemd.claude-agent";
        let app = self.app.clone();
        let command = command.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        tauri::async_runtime::spawn(async move {
            let out = async {
                let lc = super::commands::get_or_register(&app, AGENT_PLUGIN)
                    .map_err(|e| format!("agent_unavailable: {e}"))?;
                lc.ensure_active(&super::lifecycle::Trigger::Command(command.clone()))
                    .await
                    .map_err(|e| format!("agent_unavailable: {e}"))?;
                lc.execute(plugin_protocol::ExecuteCommandParams { command, context }).await
            }
            .await;
            let _ = tx.send(out);
        });
        // "发送端消失" = spawn 的任务 panic 了,单独说清楚,别混进业务错误里。
        match rx.recv() {
            Ok(out) => out,
            Err(std::sync::mpsc::RecvError) => {
                Err("agent relay dropped: the relay task ended without answering".into())
            }
        }
    }

    fn notify_user(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let (title, action) = crate::reminders::parse_notify_params(params)?;
        let id = crate::reminders::push(title, action);
        Ok(serde_json::json!({ "ok": true, "id": id }))
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// Stub services: tempdir vault + canned dialog results + recording hooks.
    /// No real dialogs, no AppHandle.
    #[derive(Default)]
    struct StubServices {
        vault: Option<PathBuf>,
        wiki: Option<String>,
        daily: Option<String>,
        /// paths the next dialog.open returns (empty ⇒ user cancelled)
        dialog_returns: Vec<PathBuf>,
        /// path the next dialog.save returns
        save_returns: Option<PathBuf>,
        /// records the OpenOptions dispatch parsed out of the params
        last_open: Mutex<Option<OpenOptions>>,
        /// recorded clipboard writes
        clipboard: Arc<Mutex<Vec<String>>>,
        /// recorded editor.open paths
        opened: Arc<Mutex<Vec<PathBuf>>>,
        /// recorded `agent_execute`/`notify_user` calls: (kind, arg) where kind
        /// is the relayed command ("run-task"/"run-status") or "notify".
        agent_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    }

    impl HostServices for StubServices {
        fn pick_paths(&self, opts: &OpenOptions) -> Result<Option<Vec<PathBuf>>, String> {
            *self.last_open.lock().unwrap() = Some(opts.clone());
            if self.dialog_returns.is_empty() {
                return Ok(None);
            }
            Ok(Some(self.dialog_returns.clone()))
        }
        fn pick_save(&self, _opts: &SaveOptions) -> Result<Option<PathBuf>, String> {
            Ok(self.save_returns.clone())
        }
        fn vault_root(&self) -> Option<PathBuf> {
            self.vault.clone()
        }
        fn wiki_daily_dirs(&self) -> (Option<String>, Option<String>) {
            (self.wiki.clone(), self.daily.clone())
        }
        fn clipboard_write(&self, text: &str) -> Result<(), String> {
            self.clipboard.lock().unwrap().push(text.to_string());
            Ok(())
        }
        fn open_in_editor(&self, abs_path: &Path) -> Result<(), String> {
            self.opened.lock().unwrap().push(abs_path.to_path_buf());
            Ok(())
        }
        fn agent_execute(&self, command: &str, context: serde_json::Value) -> Result<serde_json::Value, String> {
            self.agent_calls.lock().unwrap().push((command.to_string(), context));
            Ok(serde_json::json!({ "run_id": "r-test" }))
        }
        fn notify_user(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
            self.agent_calls.lock().unwrap().push(("notify".into(), params.clone()));
            Ok(serde_json::json!({ "ok": true, "id": 1 }))
        }
    }

    fn noop_emitter() -> ToastEmitter {
        Arc::new(|_| {})
    }

    fn req(method: &str, params: serde_json::Value) -> proto::RpcRequest {
        proto::RpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(1),
            method: method.into(),
            params,
        }
    }

    /// NOTE: `plugin_id` must be unique per test that touches the dialog
    /// allow-set — GRANTED_PATHS is process-global and tests run in parallel.
    async fn run_as(
        services: &dyn HostServices,
        plugin_id: &str,
        caps: &[&str],
        method: &str,
        params: serde_json::Value,
    ) -> proto::RpcResponse {
        let dir = tempfile::tempdir().unwrap();
        let caps: Vec<String> = caps.iter().map(|s| s.to_string()).collect();
        dispatch_with(services, plugin_id, &caps, req(method, params), dir.path(), &noop_emitter())
            .await
    }

    async fn run(
        services: &dyn HostServices,
        caps: &[&str],
        method: &str,
        params: serde_json::Value,
    ) -> proto::RpcResponse {
        run_as(services, "test.plugin", caps, method, params).await
    }

    // ── capability gate ──────────────────────────────────────────────────

    #[tokio::test]
    async fn unauthorized_method_returns_32001() {
        let s = StubServices::default();
        let r = run(&s, &[], "host.vault.read", serde_json::json!({"path": "a.md"})).await;
        let e = r.error.unwrap();
        assert_eq!(e.code, proto::ERR_CAPABILITY_DENIED);
        assert!(e.message.contains("vault.read"), "{}", e.message);
    }

    #[tokio::test]
    async fn unknown_method_returns_32601() {
        let s = StubServices::default();
        let r = run(&s, &["vault.read"], "host.bogus", serde_json::json!({})).await;
        let e = r.error.unwrap();
        assert_eq!(e.code, proto::ERR_METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn every_new_method_is_capability_gated() {
        let s = StubServices::default();
        for (method, cap) in [
            ("host.dialog.open", "dialog"),
            ("host.dialog.save", "dialog"),
            ("host.vault.info", "vault.read"),
            ("host.vault.write", "vault.write"),
            ("host.vault.mkdir", "vault.write"),
            ("host.fs.read_text", "fs.read:dialog"),
            ("host.fs.read_bytes", "fs.read:dialog"),
            ("host.clipboard.write", "clipboard.write"),
            ("host.editor.open", "editor.open"),
            ("host.theme.css", "editor.kit"),
        ] {
            let r = run(&s, &[], method, serde_json::json!({})).await;
            let e = r.error.unwrap();
            assert_eq!(e.code, proto::ERR_CAPABILITY_DENIED, "{method}");
            assert!(e.message.contains(cap), "{method}: {}", e.message);
        }
    }

    // ── host.theme.css gate (dispatch() intercepts it; needs an AppHandle) ──
    //
    // The bundle itself is read from the live app config dir / compiled theme
    // artifacts, so only the GATE is unit-testable here — and the gate is the
    // whole security surface: `dispatch` must refuse a plugin that did not
    // declare `editor.kit` before it ever touches the theme files.

    #[test]
    fn theme_css_is_denied_without_the_editor_kit_capability() {
        let caps: Vec<String> = vec!["vault.read".into(), "editor.open".into()];
        let denial = capability_denial("host.theme.css", &caps, Some(3))
            .expect("host.theme.css must be denied without 'editor.kit'");
        assert_eq!(denial.id, 3);
        assert!(denial.result.is_none());
        let e = denial.error.unwrap();
        assert_eq!(e.code, proto::ERR_CAPABILITY_DENIED);
        assert!(e.message.contains("editor.kit"), "{}", e.message);
        assert!(e.message.contains("host.theme.css"), "{}", e.message);

        // No capabilities at all → same denial.
        assert!(capability_denial("host.theme.css", &[], Some(1)).is_some());
    }

    /// Fail CLOSED on an unknown method, exactly like the two shared gates
    /// (`dispatch_with` and `host_api::make_sink` both answer -32601). A
    /// mistyped method name in a future interception must never be read as
    /// "allowed" just because the caller happens to hold some capability.
    #[test]
    fn capability_denial_rejects_unknown_methods() {
        let caps: Vec<String> = vec!["editor.kit".into()];
        let denial = capability_denial("host.nope", &caps, Some(1))
            .expect("an unknown method must be denied, not fall through");
        assert_eq!(denial.error.unwrap().code, proto::ERR_METHOD_NOT_FOUND);
        // Free methods (no capability at all) still pass.
        assert!(capability_denial("host.log.info", &[], Some(1)).is_none());
    }

    #[test]
    fn theme_css_is_allowed_with_the_editor_kit_capability() {
        let caps: Vec<String> = vec!["editor.kit".into()];
        assert!(
            capability_denial("host.theme.css", &caps, Some(1)).is_none(),
            "a plugin holding editor.kit must pass the gate"
        );
    }

    // ── 子项目②b method routing (host.* vs plugin.*) ──────────────────────
    //
    // `dispatch` needs a live AppHandle to forward non-host methods, so the
    // full forward round-trip is an integration concern (Task 3). Here we
    // pin the pure routing DECISION — `is_host_method` — which is exactly the
    // branch `dispatch` takes: host.* → local `dispatch_with` (capability gate),
    // everything else → `forward_to_plugin` (no host gate).

    #[test]
    fn is_host_method_routes_host_methods_locally() {
        // Every host.* method the UI bridge serves locally.
        for m in [
            "host.log.info",
            "host.toast",
            "host.dialog.open",
            "host.vault.read",
            "host.vault.write",
            "host.fs.read_text",
            "host.clipboard.write",
            "host.ui.post",
            "host.bogus", // unknown host.* still routes local → -32601 there
        ] {
            assert!(is_host_method(m), "{m} must route to the local host bridge");
        }
    }

    #[test]
    fn is_host_method_forwards_plugin_and_bare_methods() {
        // The plugin's own API surface — forwarded to its process, no host gate.
        for m in [
            "plugin.connect",
            "plugin.send",
            "plugin.disconnect",
            "connect",       // bare (no plugin. prefix) still forwards
            "anything.else", // any non-host method forwards
            "hosting",       // NOT "host." — must not be mistaken for a host method
            "",              // empty is not a host method → forwards (plugin errors)
        ] {
            assert!(!is_host_method(m), "{m:?} must forward to the plugin process");
        }
    }

    /// The documented prefix-strip: a leading `plugin.` is removed before the
    /// method reaches the plugin's `on_ui_request` (so `plugin.connect` →
    /// `connect`); a bare method is forwarded verbatim. This mirrors the exact
    /// transform `forward_to_plugin` applies before `lc.ui_request(..)`.
    #[test]
    fn plugin_prefix_is_stripped_before_forwarding() {
        let strip = |m: &str| m.strip_prefix("plugin.").unwrap_or(m).to_string();
        assert_eq!(strip("plugin.connect"), "connect");
        assert_eq!(strip("plugin.pair_create"), "pair_create");
        assert_eq!(strip("connect"), "connect"); // bare → verbatim
        // Only a LEADING `plugin.` is stripped; an embedded one is untouched.
        assert_eq!(strip("do.plugin.thing"), "do.plugin.thing");
    }

    // ── shared toast path (handle_common) ────────────────────────────────

    #[tokio::test]
    async fn toast_goes_through_shared_handler() {
        let s = StubServices::default();
        let seen: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_in = seen.clone();
        let emitter: ToastEmitter = Arc::new(move |v| seen_in.lock().unwrap().push(v));
        let dir = tempfile::tempdir().unwrap();
        let r = dispatch_with(
            &s,
            "test.plugin",
            &["toast".to_string()],
            req("host.toast", serde_json::json!({"level": "info", "message": "hi"})),
            dir.path(),
            &emitter,
        )
        .await;
        assert_eq!(r.result, Some(serde_json::json!({"ok": true})));
        let emitted = seen.lock().unwrap();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0]["plugin_id"], "test.plugin");
        assert_eq!(emitted[0]["message"], "hi");
    }

    // ── vault round-trip ─────────────────────────────────────────────────

    #[tokio::test]
    async fn vault_write_read_exists_list_mkdir_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let s = StubServices { vault: Some(dir.path().to_path_buf()), ..Default::default() };

        // mkdir sub/
        let r = run(&s, &["vault.write"], "host.vault.mkdir", serde_json::json!({"path": "sub"})).await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(dir.path().join("sub").is_dir());

        // write auto-creates missing parents
        let r = run(
            &s,
            &["vault.write"],
            "host.vault.write",
            serde_json::json!({"path": "sub/deep/a.md", "content": "hello"}),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("sub/deep/a.md")).unwrap(),
            "hello"
        );

        // read it back
        let r = run(&s, &["vault.read"], "host.vault.read", serde_json::json!({"path": "sub/deep/a.md"})).await;
        assert_eq!(r.result.unwrap()["content"], "hello");

        // exists true / false
        let r = run(&s, &["vault.read"], "host.vault.exists", serde_json::json!({"path": "sub/deep/a.md"})).await;
        assert_eq!(r.result.unwrap()["exists"], true);
        let r = run(&s, &["vault.read"], "host.vault.exists", serde_json::json!({"path": "nope.md"})).await;
        assert_eq!(r.result.unwrap()["exists"], false);

        // list sub → entries with is_dir flags
        let r = run(&s, &["vault.read"], "host.vault.list", serde_json::json!({"path": "sub"})).await;
        let entries = r.result.unwrap()["entries"].clone();
        assert_eq!(entries, serde_json::json!([{"name": "deep", "is_dir": true}]));
        let r = run(&s, &["vault.read"], "host.vault.list", serde_json::json!({"path": "sub/deep"})).await;
        let entries = r.result.unwrap()["entries"].clone();
        assert_eq!(entries, serde_json::json!([{"name": "a.md", "is_dir": false}]));
    }

    #[tokio::test]
    async fn vault_read_bytes_returns_base64_and_respects_gate() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("img.png"), b"\x89PNG").unwrap();

        // 有 vault.read → base64(b"\x89PNG") == "iVBORw=="
        let s = StubServices { vault: Some(vault.path().to_path_buf()), ..Default::default() };
        let r = run_as(&s, "p.id", &["vault.read"], "host.vault.read_bytes", serde_json::json!({"path": "img.png"})).await;
        assert_eq!(r.result.unwrap()["base64"], "iVBORw==");

        // 无 capability → -32001
        let r = run_as(&s, "p.id", &[], "host.vault.read_bytes", serde_json::json!({"path": "img.png"})).await;
        assert_eq!(r.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);

        // 越界路径 → Err(resolve_in_vault 拒绝)
        let r = run_as(&s, "p.id", &["vault.read"], "host.vault.read_bytes", serde_json::json!({"path": "../x"})).await;
        assert!(r.error.is_some());
    }

    /// `vault.read_bytes` fires implicitly, once per embedded image, with no
    /// user gesture pacing it — so it gets its own 10 MB cap (spec §3.3) and
    /// must NOT inherit the 200 MB dialog-import cap.
    #[tokio::test]
    async fn vault_read_bytes_over_its_own_cap_is_too_large() {
        assert!(MAX_VAULT_BYTES < MAX_TEXT_BYTES, "the vault byte cap must stay the tighter one");
        let vault = tempfile::tempdir().unwrap();
        let s = StubServices { vault: Some(vault.path().to_path_buf()), ..Default::default() };

        // Exactly at the cap is fine; one byte over is not.
        std::fs::write(vault.path().join("ok.bin"), vec![b'x'; MAX_VAULT_BYTES as usize]).unwrap();
        let r = run_as(&s, "p.id", &["vault.read"], "host.vault.read_bytes", serde_json::json!({"path": "ok.bin"})).await;
        assert!(r.error.is_none(), "{:?}", r.error);

        std::fs::write(vault.path().join("big.bin"), vec![b'x'; MAX_VAULT_BYTES as usize + 1]).unwrap();
        let r = run_as(&s, "p.id", &["vault.read"], "host.vault.read_bytes", serde_json::json!({"path": "big.bin"})).await;
        let e = r.error.unwrap();
        assert_eq!(e.code, proto::ERR_INTERNAL);
        assert!(e.message.starts_with("too_large:"), "{}", e.message);
    }

    /// A plugin-seeded agent precheck (`precheck.sh`) that isn't executable is
    /// a guard that silently never runs: claude-agent's runner is fail-open on
    /// spawn failure. Non-`.sh` writes must keep the plain default mode.
    #[cfg(unix)]
    #[tokio::test]
    async fn vault_write_marks_shell_scripts_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let s = StubServices { vault: Some(dir.path().to_path_buf()), ..Default::default() };

        let r = run(
            &s,
            &["vault.write"],
            "host.vault.write",
            serde_json::json!({"path": ".notemd/agent-tasks/t/precheck.sh", "content": "#!/bin/sh\nexit 0\n"}),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        let mode = std::fs::metadata(dir.path().join(".notemd/agent-tasks/t/precheck.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755, "precheck.sh must be executable, got {mode:o}");

        let r = run(
            &s,
            &["vault.write"],
            "host.vault.write",
            serde_json::json!({"path": "note.md", "content": "hi"}),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        let mode = std::fs::metadata(dir.path().join("note.md")).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0, "a plain .md must not become executable, got {mode:o}");
    }

    #[tokio::test]
    async fn vault_remove_deletes_files_and_refuses_directories() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("a.md"), "x").unwrap();
        std::fs::create_dir(vault.path().join("sub")).unwrap();
        let s = StubServices { vault: Some(vault.path().to_path_buf()), ..Default::default() };
        // 有 capability → 删除成功
        let r = run_as(&s, "p.id", &["vault.write"], "host.vault.remove", serde_json::json!({"path": "a.md"})).await;
        assert_eq!(r.result.unwrap()["ok"], true);
        assert!(!vault.path().join("a.md").exists());
        // 幂等:再删一次仍然 ok
        let r = run_as(&s, "p.id", &["vault.write"], "host.vault.remove", serde_json::json!({"path": "a.md"})).await;
        assert_eq!(r.result.unwrap()["ok"], true);
        // 目录 → 拒绝
        let r = run_as(&s, "p.id", &["vault.write"], "host.vault.remove", serde_json::json!({"path": "sub"})).await;
        assert!(r.error.unwrap().message.contains("directory"));
        // 无 capability → -32001
        let r = run_as(&s, "p.id", &[], "host.vault.remove", serde_json::json!({"path": "b.md"})).await;
        assert_eq!(r.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);
        // 越界 → 错误
        let r = run_as(&s, "p.id", &["vault.write"], "host.vault.remove", serde_json::json!({"path": "../x"})).await;
        assert!(r.error.is_some());
    }

    #[tokio::test]
    async fn vault_rename_moves_within_vault_and_never_clobbers() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("a.md"), "x").unwrap();
        std::fs::write(vault.path().join("taken.md"), "y").unwrap();
        let s = StubServices { vault: Some(vault.path().to_path_buf()), ..Default::default() };
        let r = run_as(
            &s,
            "p.id",
            &["vault.write"],
            "host.vault.rename",
            serde_json::json!({"from": "a.md", "to": "sub/b.md"}),
        )
        .await;
        assert_eq!(r.result.unwrap()["ok"], true);
        assert!(vault.path().join("sub/b.md").exists());
        assert!(!vault.path().join("a.md").exists());
        // 目标已存在 → 不覆盖
        let r = run_as(
            &s,
            "p.id",
            &["vault.write"],
            "host.vault.rename",
            serde_json::json!({"from": "sub/b.md", "to": "taken.md"}),
        )
        .await;
        assert!(r.error.unwrap().message.contains("exists"));
        assert_eq!(std::fs::read_to_string(vault.path().join("taken.md")).unwrap(), "y");
        // 两端都过校验
        let r = run_as(
            &s,
            "p.id",
            &["vault.write"],
            "host.vault.rename",
            serde_json::json!({"from": "sub/b.md", "to": "../out.md"}),
        )
        .await;
        assert!(r.error.is_some());
    }

    /// Regression for the "wrong file" bug: `resolve_in_vault` canonicalizes
    /// the final path component, so acting on its result for a LIVE symlink
    /// (target also inside the vault — not an escape) would silently
    /// delete/rename the symlink's TARGET instead of the link entry the
    /// caller named. `vault_remove`/`vault_rename` must operate on the entry
    /// itself; the real file behind it must never be touched.
    #[cfg(unix)]
    #[tokio::test]
    async fn vault_remove_and_rename_on_a_live_symlink_never_touch_the_target() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("real.md"), "real content").unwrap();
        if std::os::unix::fs::symlink("real.md", vault.path().join("link.md")).is_err() {
            eprintln!("skipping: symlink creation not supported here");
            return;
        }
        let s = StubServices { vault: Some(vault.path().to_path_buf()), ..Default::default() };

        // remove the link → the link entry is gone, real.md is untouched.
        let r = run_as(&s, "p.id", &["vault.write"], "host.vault.remove", serde_json::json!({"path": "link.md"})).await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.result.unwrap()["ok"], true);
        assert!(
            std::fs::symlink_metadata(vault.path().join("link.md")).is_err(),
            "the link entry must be gone"
        );
        assert_eq!(
            std::fs::read_to_string(vault.path().join("real.md")).unwrap(),
            "real content",
            "the symlink's target must survive a remove of the link"
        );

        // rename a fresh link → only the link entry moves, real.md is untouched.
        std::os::unix::fs::symlink("real.md", vault.path().join("link2.md")).unwrap();
        let r = run_as(
            &s,
            "p.id",
            &["vault.write"],
            "host.vault.rename",
            serde_json::json!({"from": "link2.md", "to": "moved-link.md"}),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.result.unwrap()["ok"], true);
        assert!(std::fs::symlink_metadata(vault.path().join("link2.md")).is_err());
        let moved_meta = std::fs::symlink_metadata(vault.path().join("moved-link.md")).unwrap();
        assert!(moved_meta.file_type().is_symlink(), "the moved entry must still be a symlink");
        assert_eq!(
            std::fs::read_to_string(vault.path().join("real.md")).unwrap(),
            "real content",
            "the symlink's target must survive a rename of the link"
        );
    }

    /// Regression: `Path::exists()` follows symlinks, so a DANGLING symlink at
    /// `to` (directory entry present, target missing) reads as "doesn't
    /// exist" and would be silently clobbered by a naive `exists()` +
    /// `rename` check. The atomic `create_new` guard must reject it exactly
    /// like a normal existing file.
    #[cfg(unix)]
    #[tokio::test]
    async fn vault_rename_refuses_to_clobber_a_dangling_symlink_destination() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("a.md"), "x").unwrap();
        if std::os::unix::fs::symlink("nonexistent-target.md", vault.path().join("dead.md")).is_err() {
            eprintln!("skipping: symlink creation not supported here");
            return;
        }
        assert!(!vault.path().join("dead.md").exists(), "sanity: exists() must read a dangling link as absent");

        let s = StubServices { vault: Some(vault.path().to_path_buf()), ..Default::default() };
        let r = run_as(
            &s,
            "p.id",
            &["vault.write"],
            "host.vault.rename",
            serde_json::json!({"from": "a.md", "to": "dead.md"}),
        )
        .await;
        assert!(r.error.unwrap().message.contains("exists"));
        assert_eq!(std::fs::read_to_string(vault.path().join("a.md")).unwrap(), "x", "source must be untouched");
        assert!(
            std::fs::symlink_metadata(vault.path().join("dead.md")).unwrap().file_type().is_symlink(),
            "the dangling symlink at `to` must survive, unclobbered"
        );
    }

    /// Regression: `create_new` plants a 0-byte placeholder at `to` BEFORE
    /// `rename` runs. If `rename` then fails — most commonly because `from`
    /// doesn't exist (stale UI state, a concurrent delete, a typo) — that
    /// placeholder must be cleaned up, not left behind as a ghost file the
    /// user never created.
    #[tokio::test]
    async fn vault_rename_cleans_up_the_placeholder_when_from_is_missing() {
        let vault = tempfile::tempdir().unwrap();
        let s = StubServices { vault: Some(vault.path().to_path_buf()), ..Default::default() };

        let r = run_as(
            &s,
            "p.id",
            &["vault.write"],
            "host.vault.rename",
            serde_json::json!({"from": "nope.md", "to": "dest.md"}),
        )
        .await;
        assert!(r.error.is_some());
        assert!(
            !vault.path().join("dest.md").exists(),
            "a failed rename must not leave a ghost file at `to`"
        );
    }

    #[tokio::test]
    async fn path_traversal_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let s = StubServices { vault: Some(dir.path().to_path_buf()), ..Default::default() };
        for bad in ["../escape.md", "sub/../../escape.md", "/etc/passwd"] {
            let r = run(&s, &["vault.read"], "host.vault.read", serde_json::json!({"path": bad})).await;
            let e = r.error.unwrap();
            assert_eq!(e.code, proto::ERR_INTERNAL, "path {bad}");
            assert!(e.message.starts_with("io:"), "path {bad}: {}", e.message);
        }
    }

    #[tokio::test]
    async fn symlink_escape_is_rejected() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "s3cret").unwrap();
        let dir = tempfile::tempdir().unwrap();
        if std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).is_err() {
            eprintln!("skipping: symlink creation not supported here");
            return;
        }
        let s = StubServices { vault: Some(dir.path().to_path_buf()), ..Default::default() };

        // read through the symlink
        let r = run(&s, &["vault.read"], "host.vault.read", serde_json::json!({"path": "link/secret.txt"})).await;
        let e = r.error.unwrap();
        assert!(e.message.contains("escapes the vault"), "{}", e.message);

        // write through the symlink (not-yet-existing target under a
        // symlinked, escaping ancestor)
        let r = run(
            &s,
            &["vault.write"],
            "host.vault.write",
            serde_json::json!({"path": "link/new.md", "content": "x"}),
        )
        .await;
        let e = r.error.unwrap();
        assert!(e.message.contains("escapes the vault"), "{}", e.message);
        assert!(!outside.path().join("new.md").exists());
    }

    #[tokio::test]
    async fn vault_required_when_root_none() {
        let s = StubServices { vault: None, ..Default::default() };
        let r = run(&s, &["vault.read"], "host.vault.read", serde_json::json!({"path": "a.md"})).await;
        let e = r.error.unwrap();
        assert_eq!(e.code, proto::ERR_INTERNAL);
        assert!(e.message.starts_with("vault_required:"), "{}", e.message);
    }

    #[tokio::test]
    async fn read_over_cap_is_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let big = vec![b'x'; (MAX_TEXT_BYTES + 1) as usize];
        std::fs::write(dir.path().join("big.txt"), &big).unwrap();
        let s = StubServices { vault: Some(dir.path().to_path_buf()), ..Default::default() };
        let r = run(&s, &["vault.read"], "host.vault.read", serde_json::json!({"path": "big.txt"})).await;
        let e = r.error.unwrap();
        assert_eq!(e.code, proto::ERR_INTERNAL);
        assert!(e.message.starts_with("too_large:"), "{}", e.message);
    }

    #[tokio::test]
    async fn write_over_cap_is_too_large_but_small_write_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let s = StubServices { vault: Some(dir.path().to_path_buf()), ..Default::default() };

        // A write just over the cap is rejected and nothing is written.
        let big = "x".repeat((MAX_TEXT_BYTES + 1) as usize);
        let r = run(
            &s,
            &["vault.write"],
            "host.vault.write",
            serde_json::json!({"path": "big.md", "content": big}),
        )
        .await;
        let e = r.error.unwrap();
        assert_eq!(e.code, proto::ERR_INTERNAL);
        assert!(e.message.starts_with("too_large:"), "{}", e.message);
        assert!(!dir.path().join("big.md").exists(), "rejected write must not create the file");

        // A small write still succeeds.
        let r = run(
            &s,
            &["vault.write"],
            "host.vault.write",
            serde_json::json!({"path": "small.md", "content": "ok"}),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(std::fs::read_to_string(dir.path().join("small.md")).unwrap(), "ok");
    }

    // ── vault.info ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn vault_info_reports_root_and_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let s = StubServices {
            vault: Some(dir.path().to_path_buf()),
            wiki: Some("wiki".into()),
            daily: Some("journal".into()),
            ..Default::default()
        };
        let r = run(&s, &["vault.read"], "host.vault.info", serde_json::json!({})).await;
        let res = r.result.unwrap();
        assert_eq!(res["root"], dir.path().to_string_lossy().to_string());
        assert_eq!(res["wiki_dir"], "wiki");
        assert_eq!(res["daily_dir"], "journal");
    }

    #[tokio::test]
    async fn vault_info_applies_frontend_defaults_when_dirs_unset() {
        let dir = tempfile::tempdir().unwrap();
        let s = StubServices { vault: Some(dir.path().to_path_buf()), ..Default::default() };
        let r = run(&s, &["vault.read"], "host.vault.info", serde_json::json!({})).await;
        let res = r.result.unwrap();
        assert_eq!(res["wiki_dir"], "wikipage");
        assert_eq!(res["daily_dir"], "dailynote");
    }

    #[tokio::test]
    async fn vault_info_all_null_without_root() {
        let s = StubServices::default();
        let r = run(&s, &["vault.read"], "host.vault.info", serde_json::json!({})).await;
        let res = r.result.unwrap();
        assert!(res["root"].is_null());
        assert!(res["wiki_dir"].is_null());
        assert!(res["daily_dir"].is_null());
    }

    // ── fs.read:dialog authorization ─────────────────────────────────────

    #[tokio::test]
    async fn fs_read_text_denied_then_allowed_after_dialog() {
        let pid = "test.grant-flow"; // unique: global allow-set
        let outside = tempfile::tempdir().unwrap();
        let export = outside.path().join("export.json");
        std::fs::write(&export, r#"{"k":1}"#).unwrap();
        let export_str = export.to_string_lossy().to_string();

        let s = StubServices { dialog_returns: vec![export.clone()], ..Default::default() };

        // Before any dialog: not granted.
        let r = run_as(&s, pid, &["fs.read:dialog"], "host.fs.read_text", serde_json::json!({"path": export_str})).await;
        let e = r.error.unwrap();
        assert_eq!(e.code, proto::ERR_INTERNAL);
        assert!(e.message.starts_with("not_granted:"), "{}", e.message);

        // dialog.open → dispatch inserts the returned paths into the allow-set.
        let r = run_as(
            &s,
            pid,
            &["dialog"],
            "host.dialog.open",
            serde_json::json!({"filters": [{"name": "JSON", "extensions": ["json"]}]}),
        )
        .await;
        let paths = r.result.unwrap()["paths"].clone();
        assert_eq!(paths, serde_json::json!([export_str]));

        // Now read_text succeeds.
        let r = run_as(&s, pid, &["fs.read:dialog"], "host.fs.read_text", serde_json::json!({"path": export_str})).await;
        assert_eq!(r.result.unwrap()["content"], r#"{"k":1}"#);
    }

    #[test]
    fn clear_grants_removes_the_plugins_grants() {
        let pid = "test.clear-grants"; // unique: global allow-set
        let p = PathBuf::from("/tmp/test-clear-grants/export.json");
        grant_path(pid, &p);
        assert!(is_granted(pid, &p), "path should be granted after grant_path");
        clear_grants(pid);
        assert!(!is_granted(pid, &p), "path must not be granted after clear_grants");
    }

    #[tokio::test]
    async fn grants_are_per_plugin() {
        let outside = tempfile::tempdir().unwrap();
        let f = outside.path().join("mine.txt");
        std::fs::write(&f, "mine").unwrap();
        let f_str = f.to_string_lossy().to_string();

        let s = StubServices { dialog_returns: vec![f.clone()], ..Default::default() };
        let r = run_as(&s, "test.grant-owner", &["dialog"], "host.dialog.open", serde_json::json!({})).await;
        assert!(r.error.is_none());

        // A DIFFERENT plugin cannot read the path granted to the first one.
        let r = run_as(&s, "test.grant-thief", &["fs.read:dialog"], "host.fs.read_text", serde_json::json!({"path": f_str})).await;
        let e = r.error.unwrap();
        assert!(e.message.starts_with("not_granted:"), "{}", e.message);
    }

    // ── fs.read_bytes (base64) ───────────────────────────────────────────

    #[test]
    fn base64_encode_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        // Bytes that exercise the +/ characters of the standard alphabet.
        assert_eq!(base64_encode(&[0xff, 0xef, 0xff]), "/+//");
    }

    #[tokio::test]
    async fn fs_read_bytes_denied_then_returns_base64_after_dialog() {
        let pid = "test.read-bytes-flow"; // unique: global allow-set
        let outside = tempfile::tempdir().unwrap();
        let archive = outside.path().join("export.zip");
        let raw: &[u8] = &[0x50, 0x4b, 0x03, 0x04, 0x00, 0xff, 0xef]; // PK + non-utf8
        std::fs::write(&archive, raw).unwrap();
        let archive_str = archive.to_string_lossy().to_string();

        let s = StubServices { dialog_returns: vec![archive.clone()], ..Default::default() };

        // Before any dialog: not granted.
        let r = run_as(&s, pid, &["fs.read:dialog"], "host.fs.read_bytes", serde_json::json!({"path": archive_str})).await;
        let e = r.error.unwrap();
        assert!(e.message.starts_with("not_granted:"), "{}", e.message);

        // dialog.open grants the path.
        let _ = run_as(&s, pid, &["dialog"], "host.dialog.open", serde_json::json!({})).await;

        // read_bytes returns the correct base64 of the raw bytes.
        let r = run_as(&s, pid, &["fs.read:dialog"], "host.fs.read_bytes", serde_json::json!({"path": archive_str})).await;
        assert_eq!(r.result.unwrap()["base64"], base64_encode(raw));
    }

    #[tokio::test]
    async fn fs_read_bytes_over_cap_is_too_large() {
        let pid = "test.read-bytes-big"; // unique: global allow-set
        let outside = tempfile::tempdir().unwrap();
        let big = outside.path().join("big.bin");
        std::fs::write(&big, vec![b'z'; (MAX_TEXT_BYTES + 1) as usize]).unwrap();
        let big_str = big.to_string_lossy().to_string();

        let s = StubServices { dialog_returns: vec![big.clone()], ..Default::default() };
        let _ = run_as(&s, pid, &["dialog"], "host.dialog.open", serde_json::json!({})).await;

        let r = run_as(&s, pid, &["fs.read:dialog"], "host.fs.read_bytes", serde_json::json!({"path": big_str})).await;
        let e = r.error.unwrap();
        assert_eq!(e.code, proto::ERR_INTERNAL);
        assert!(e.message.starts_with("too_large:"), "{}", e.message);
    }

    // ── dialogs ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn dialog_open_parses_options_and_forwards_them() {
        let s = StubServices::default();
        let _ = run(
            &s,
            &["dialog"],
            "host.dialog.open",
            serde_json::json!({
                "title": "Pick",
                "directory": true,
                "multiple": true,
                "filters": [{"name": "JSON", "extensions": ["json"]}],
            }),
        )
        .await;
        let opts = s.last_open.lock().unwrap().clone().unwrap();
        assert_eq!(opts.title.as_deref(), Some("Pick"));
        assert!(opts.directory);
        assert!(opts.multiple);
        assert_eq!(opts.filters.len(), 1);
        assert_eq!(opts.filters[0].name, "JSON");
        assert_eq!(opts.filters[0].extensions, vec!["json".to_string()]);
    }

    #[tokio::test]
    async fn dialog_open_cancelled_returns_null_paths() {
        let s = StubServices::default(); // dialog_returns empty → None
        let r = run(&s, &["dialog"], "host.dialog.open", serde_json::json!({})).await;
        assert!(r.result.unwrap()["paths"].is_null());
    }

    #[tokio::test]
    async fn dialog_save_returns_path_and_grants_it() {
        let pid = "test.grant-save"; // unique: global allow-set
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("out.txt");
        std::fs::write(&target, "saved").unwrap();
        let target_str = target.to_string_lossy().to_string();
        let s = StubServices { save_returns: Some(target.clone()), ..Default::default() };

        let r = run_as(&s, pid, &["dialog"], "host.dialog.save", serde_json::json!({"default_filename": "out.txt"})).await;
        assert_eq!(r.result.unwrap()["path"], target_str);

        // The saved path is readable via fs.read_text (it was dialog-granted).
        let r = run_as(&s, pid, &["fs.read:dialog"], "host.fs.read_text", serde_json::json!({"path": target_str})).await;
        assert_eq!(r.result.unwrap()["content"], "saved");
    }

    // ── clipboard ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn clipboard_write_calls_service() {
        let s = StubServices::default();
        let clip = s.clipboard.clone();
        let r = run(&s, &["clipboard.write"], "host.clipboard.write", serde_json::json!({"text": "copied"})).await;
        assert_eq!(r.result.unwrap(), serde_json::json!({"ok": true}));
        assert_eq!(*clip.lock().unwrap(), vec!["copied".to_string()]);
    }

    #[tokio::test]
    async fn missing_params_error_with_io_kind() {
        let s = StubServices { vault: Some(std::env::temp_dir()), ..Default::default() };
        let r = run(&s, &["clipboard.write"], "host.clipboard.write", serde_json::json!({})).await;
        let e = r.error.unwrap();
        assert_eq!(e.code, proto::ERR_INTERNAL);
        assert!(e.message.starts_with("io:"), "{}", e.message);
    }

    // ── editor.open ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn editor_open_resolves_vault_path_and_records_call() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("note.md");
        std::fs::write(&file, "# hello").unwrap();
        let s = StubServices {
            vault: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let opened = s.opened.clone();
        let r = run(&s, &["editor.open"], "host.editor.open", serde_json::json!({"path": "note.md"})).await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.result.unwrap(), serde_json::json!({"ok": true}));
        let calls = opened.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].ends_with("note.md"), "expected path ending in note.md, got {:?}", calls[0]);
    }

    #[tokio::test]
    async fn editor_open_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let s = StubServices {
            vault: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let r = run(&s, &["editor.open"], "host.editor.open", serde_json::json!({"path": "../secret.md"})).await;
        let e = r.error.unwrap();
        assert_eq!(e.code, proto::ERR_INTERNAL);
        assert!(e.message.contains("escapes the vault"), "expected 'escapes the vault' in: {}", e.message);
    }

    // ── host.agent.*/host.notify on the UI RPC bridge ──────────────────────
    //
    // dispatch_with's `host.agent.run`/`host.agent.status`/`host.notify` arms
    // are a SEPARATE match from host_api::make_sink's (子项目②b: process
    // channel vs UI channel are two independent dispatchers). Task 3's tests
    // only covered the process channel via make_sink; these pin the UI bridge's
    // own command-name mapping and capability gate so a "run-task"/"run-status"
    // swap or a missing capability check here would fail a test.

    #[tokio::test]
    async fn host_agent_run_maps_to_run_task_with_capability() {
        let s = StubServices::default();
        let calls = s.agent_calls.clone();
        let r = run(
            &s,
            &["agent"],
            "host.agent.run",
            serde_json::json!({"task": "ai-read-ebook", "prompt": "p"}),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.result.unwrap()["run_id"], "r-test");
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "run-task");
        assert_eq!(calls[0].1, serde_json::json!({"task": "ai-read-ebook", "prompt": "p"}));
    }

    #[tokio::test]
    async fn host_agent_status_maps_to_run_status_with_capability() {
        let s = StubServices::default();
        let calls = s.agent_calls.clone();
        let r = run(
            &s,
            &["agent"],
            "host.agent.status",
            serde_json::json!({"task": "ai-read-ebook", "run_id": "r1"}),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "run-status");
        assert_eq!(calls[0].1, serde_json::json!({"task": "ai-read-ebook", "run_id": "r1"}));
    }

    #[tokio::test]
    async fn host_notify_dispatches_to_notify_user_with_capability() {
        let dir = tempfile::tempdir().unwrap();
        let summary = dir.path().join("ssot/ebooks/b/2026-08-04-summary.md");
        std::fs::create_dir_all(summary.parent().unwrap()).unwrap();
        std::fs::write(&summary, "# x").unwrap();
        let s = StubServices {
            vault: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let calls = s.agent_calls.clone();
        let params = serde_json::json!({
            "title": "t",
            "action": {"kind": "open_path", "path": summary.to_string_lossy()},
        });
        let r = run(&s, &["notify"], "host.notify", params.clone()).await;
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!(r.result.unwrap(), serde_json::json!({"ok": true, "id": 1}));
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "notify");
        assert_eq!(calls[0].1["title"], "t");
        // The registry gets the canonicalized absolute path, not the raw one
        // (/tmp → /private/tmp on macOS).
        assert_eq!(
            std::path::Path::new(calls[0].1["action"]["path"].as_str().unwrap()),
            summary.canonicalize().unwrap(),
        );
    }

    /// `host.notify`'s OpenPath used to take ANY absolute path and hand it
    /// straight to the tray click handler — a plugin declaring only `notify`
    /// could get the user to one-click open `~/.ssh/config`, i.e. strictly more
    /// reach than `editor.open`, which is fenced. Same fence now.
    #[tokio::test]
    async fn host_notify_refuses_an_open_path_outside_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("id_rsa");
        std::fs::write(&secret, "x").unwrap();
        let s = StubServices {
            vault: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let calls = s.agent_calls.clone();

        for path in [
            secret.to_string_lossy().to_string(),
            "../escape.md".to_string(),
            format!("{}/../escape.md", dir.path().display()),
        ] {
            let r = run(
                &s,
                &["notify"],
                "host.notify",
                serde_json::json!({"title": "t", "action": {"kind": "open_path", "path": path}}),
            )
            .await;
            let e = r.error.unwrap_or_else(|| panic!("{path} was accepted"));
            assert!(e.message.contains("escapes the vault"), "{path}: {}", e.message);
        }
        assert!(
            calls.lock().unwrap().is_empty(),
            "a refused reminder must never reach the registry"
        );
    }

    /// A vault-relative path is accepted too (and made absolute), and the
    /// plugin-window action carries no path to fence.
    #[tokio::test]
    async fn host_notify_accepts_a_vault_relative_path_and_the_window_action() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("ssot")).unwrap();
        std::fs::write(dir.path().join("ssot/a.md"), "# a").unwrap();
        let s = StubServices {
            vault: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let calls = s.agent_calls.clone();

        let r = run(
            &s,
            &["notify"],
            "host.notify",
            serde_json::json!({"title": "t", "action": {"kind": "open_path", "path": "ssot/a.md"}}),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);

        let r = run(
            &s,
            &["notify"],
            "host.notify",
            serde_json::json!({"title": "t", "action": {
                "kind": "open_plugin_window", "plugin_id": "notemd.claude-agent", "window": "main"}}),
        )
        .await;
        assert!(r.error.is_none(), "{:?}", r.error);

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(std::path::Path::new(calls[0].1["action"]["path"].as_str().unwrap()).is_absolute());
        assert_eq!(calls[1].1["action"]["kind"], "open_plugin_window");
    }

    #[tokio::test]
    async fn host_agent_and_notify_without_capability_are_denied_and_stub_untouched() {
        let s = StubServices::default();
        let calls = s.agent_calls.clone();

        let r = run(&s, &[], "host.agent.run", serde_json::json!({})).await;
        assert_eq!(r.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);

        let r = run(&s, &[], "host.agent.status", serde_json::json!({})).await;
        assert_eq!(r.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);

        let r = run(&s, &[], "host.notify", serde_json::json!({})).await;
        assert_eq!(r.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);

        assert!(calls.lock().unwrap().is_empty(), "stub must not be called when capability is denied");
    }
}
