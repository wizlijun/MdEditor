//! Dispatch of plugin→host `host.*` calls with capability enforcement
//! (spec §5). Unauthorized method → JSON-RPC error -32001 capability_denied.
//!
//! # Testability deviation from plan
//!
//! The plan's `make_sink` took an `AppHandle<R>` directly. We instead take a
//! `ToastEmitter = Arc<dyn Fn(serde_json::Value) + Send + Sync>` so unit tests
//! can inject a recording closure without needing a real Tauri runtime. The
//! production entry point `make_sink_for_app<R: tauri::Runtime>` wraps
//! `app.emit` into that emitter. Task 8 calls `make_sink_for_app`.
//!
//! `host.ui.post` (子项目②b) follows the same injected-closure shape: a
//! `UiPoster = Arc<dyn Fn(&str /*window_id*/, &Value /*payload*/) + Send + Sync>`.
//! `make_sink_for_app` wires it to `windows::push_to_window`; unit tests inject a
//! recording poster. Only the process sink carries a `UiPoster`; the UI RPC
//! bridge never needs one (a window doesn't push to itself over the host bridge).

use plugin_protocol as proto;
use std::sync::Arc;

/// Callable that emits a `plugin-toast` event payload to the frontend.
/// In production this wraps `AppHandle::emit`; in tests it records payloads.
pub type ToastEmitter = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

/// Callable that pushes a `payload` into the plugin's window `window_id`
/// (子项目②b `host.ui.post`). In production this wraps
/// `windows::push_to_window`; in tests it records `(window_id, payload)`.
pub type UiPoster = Arc<dyn Fn(&str, &serde_json::Value) + Send + Sync>;

/// 方法所需 capability；`None` = 免授权（spec §5 表）。
/// 进程侧（make_sink）与 UI 侧（ui_rpc::dispatch）共用同一张表。
pub fn method_capability(method: &str) -> Option<&'static str> {
    match method {
        "host.log.info" | "host.log.warn" | "host.log.error" => None,
        "host.toast" => Some("toast"),
        // 子项目②b: plugin process → its own window push.
        "host.ui.post" => Some("ui"),
        "host.dialog.open" | "host.dialog.save" => Some("dialog"),
        "host.vault.info" | "host.vault.read" | "host.vault.read_bytes" | "host.vault.exists"
        | "host.vault.list" => Some("vault.read"),
        "host.vault.write" | "host.vault.mkdir" => Some("vault.write"),
        // fs.read:dialog — readable only for paths previously returned by a
        // host.dialog.open/save in this session (spec §5 prompt semantics).
        "host.fs.read_text" | "host.fs.read_bytes" => Some("fs.read:dialog"),
        "host.clipboard.write" => Some("clipboard.write"),
        "host.editor.open" => Some("editor.open"),
        "host.location.get" => Some("location"),
        // AI agent 中转(转发到 notemd.claude-agent)与托盘全局提醒。
        "host.agent.run" | "host.agent.status" => Some("agent"),
        "host.notify" => Some("notify"),
        // editor.kit — the host-embedded editor bundle and the theme CSS that
        // styles it. UI bridge only (`ui_rpc::dispatch` serves it from the live
        // AppHandle); on the process channel it stays -32601.
        "host.theme.css" => Some("editor.kit"),
        _ => Some("__unknown__"), // 未实现的方法一律拒绝
    }
}

/// Shared handling for the methods that the process sink and the UI RPC bridge
/// implement identically: `host.log.*` and `host.toast`. Returns:
/// - `Some(Ok(value))` — handled; `value` is the JSON result payload.
/// - `Some(Err(detail))` — handled but failed (currently unused; kept for symmetry).
/// - `None` — not one of these methods; the caller handles it (or errors).
///
/// The capability gate is the CALLER's responsibility (it precedes this call in
/// both sinks); `host.log.*` needs no capability and `host.toast` requires the
/// already-checked `toast`. This function only executes the side effect.
pub(crate) fn handle_common(
    method: &str,
    params: serde_json::Value,
    plugin_id: &str,
    log_dir: &std::path::Path,
    emitter: &ToastEmitter,
) -> Option<Result<serde_json::Value, String>> {
    match method {
        m @ ("host.log.info" | "host.log.warn" | "host.log.error") => {
            if let Ok(p) = serde_json::from_value::<proto::LogParams>(params) {
                let level = m.rsplit('.').next().unwrap_or("info");
                crate::plugin_runtime::process::append_plugin_log(
                    log_dir, plugin_id, level, &p.message,
                );
            }
            Some(Ok(serde_json::json!({"ok": true})))
        }
        "host.toast" => {
            if let Ok(p) = serde_json::from_value::<proto::ToastParams>(params) {
                emitter(serde_json::json!({
                    "plugin_id": plugin_id,
                    "level": p.level,
                    "message": p.message,
                    "detail": p.detail,
                }));
            }
            Some(Ok(serde_json::json!({"ok": true})))
        }
        _ => None,
    }
}

/// Core sink constructor. Uses an injectable `ToastEmitter` / `UiPoster` so unit
/// tests can record side effects without a real Tauri runtime.
///
/// Behaviour:
/// - Unknown method (`__unknown__`) → error -32601 for requests; silent no-op for notifications.
/// - Missing capability → error -32001 for requests; silent no-op for notifications.
/// - `host.log.*` → `append_plugin_log` (no capability required); returns `{ok:true}` for requests.
/// - `host.toast` → calls `emitter` with the payload JSON; returns `{ok:true}` for requests.
/// - `host.ui.post` (needs `ui`) → parses [`proto::UiPostParams`] and calls
///   `ui_poster(window_id, payload)`; fire-and-forget (returns `{ok:true}` for
///   requests, but openclaw pushes it as a notification so there is no reply).
pub fn make_sink(
    plugin_id: String,
    capabilities: Vec<String>,
    log_dir: std::path::PathBuf,
    emitter: ToastEmitter,
    ui_poster: UiPoster,
    services: Option<Arc<dyn crate::plugin_runtime::ui_rpc::HostServices>>,
) -> crate::plugin_runtime::process::HostSink {
    Arc::new(move |req: proto::RpcRequest| {
        let reply_err = |id: u64, code: i64, message: String| {
            Some(proto::RpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: None,
                error: Some(proto::RpcError { code, message }),
            })
        };
        let ok = |id: Option<u64>| {
            id.map(|id| proto::RpcResponse {
                jsonrpc: "2.0".into(),
                id,
                result: Some(serde_json::json!({"ok": true})),
                error: None,
            })
        };

        match method_capability(&req.method) {
            Some("__unknown__") => {
                // Notification: silent no-op (no id to respond to)
                req.id.and_then(|id| {
                    reply_err(
                        id,
                        proto::ERR_METHOD_NOT_FOUND,
                        format!("unknown method {}", req.method),
                    )
                })
            }
            Some(cap) if !capabilities.iter().any(|c| c == cap) => {
                // Notification: silent no-op
                req.id.and_then(|id| {
                    reply_err(
                        id,
                        proto::ERR_CAPABILITY_DENIED,
                        format!("method {} requires capability '{cap}'", req.method),
                    )
                })
            }
            // 子项目②b: process → its own window push. Only exists on the process
            // channel (a window never pushes to itself via the host bridge), so
            // it is handled here rather than in the shared `handle_common`.
            _ if req.method == "host.ui.post" => {
                if let Ok(p) = serde_json::from_value::<proto::UiPostParams>(req.params) {
                    ui_poster(&p.window_id, &p.payload);
                }
                ok(req.id)
            }
            _ => match handle_common(&req.method, req.params.clone(), &plugin_id, &log_dir, &emitter) {
                Some(Ok(_)) => ok(req.id),
                Some(Err(detail)) => req
                    .id
                    .and_then(|id| reply_err(id, proto::ERR_INTERNAL, detail)),
                // vault.* 在进程通道上可用（pos-log 等后台插件的写盘通道），前提
                // 是宿主给了 services；dialog/fs/clipboard 仍只在 UI 桥 —— 对它
                // 们回 -32601 依旧关键：进程插件声明了 `dialog` 也不能在宿主线
                // 程上弹对话框。
                None => {
                    use crate::plugin_runtime::ui_rpc as rpc;
                    let vault_out: Option<Result<serde_json::Value, String>> =
                        services.as_ref().and_then(|svc| {
                            let s: &dyn rpc::HostServices = svc.as_ref();
                            match req.method.as_str() {
                                "host.vault.info" => Some(Ok(rpc::vault_info(s))),
                                "host.vault.read" => Some(rpc::vault_read(s, &req.params)),
                                "host.vault.read_bytes" => Some(rpc::vault_read_bytes(s, &req.params)),
                                "host.vault.write" => Some(rpc::vault_write(s, &req.params)),
                                "host.vault.exists" => Some(rpc::vault_exists(s, &req.params)),
                                "host.vault.list" => Some(rpc::vault_list(s, &req.params)),
                                "host.vault.mkdir" => Some(rpc::vault_mkdir(s, &req.params)),
                                "host.location.get" => Some(s.location_get()),
                                "host.agent.run" => Some(s.agent_execute("run-task", req.params.clone())),
                                "host.agent.status" => Some(s.agent_execute("run-status", req.params.clone())),
                                // notify_push, not notify_user: the OpenPath
                                // action has to clear the same vault fence
                                // `editor.open` does.
                                "host.notify" => Some(rpc::notify_push(s, &req.params)),
                                _ => None,
                            }
                        });
                    match vault_out {
                        Some(Ok(v)) => req.id.map(|id| proto::RpcResponse {
                            jsonrpc: "2.0".into(),
                            id,
                            result: Some(v),
                            error: None,
                        }),
                        Some(Err(detail)) => req
                            .id
                            .and_then(|id| reply_err(id, proto::ERR_INTERNAL, detail)),
                        None => req.id.and_then(|id| {
                            reply_err(
                                id,
                                proto::ERR_METHOD_NOT_FOUND,
                                format!(
                                    "method {} is not available on the process channel",
                                    req.method
                                ),
                            )
                        }),
                    }
                }
            },
        }
    })
}

/// Production entry point (called by Task 8 / lifecycle). Wraps `app.emit` into a
/// `ToastEmitter` and `windows::push_to_window` into a `UiPoster`, then delegates
/// to `make_sink`.
pub fn make_sink_for_app<R: tauri::Runtime>(
    plugin_id: String,
    capabilities: Vec<String>,
    app: tauri::AppHandle<R>,
    log_dir: std::path::PathBuf,
) -> crate::plugin_runtime::process::HostSink {
    use tauri::Emitter;
    let emitter: ToastEmitter = {
        let app = app.clone();
        Arc::new(move |payload| {
            let _ = app.emit("plugin-toast", payload);
        })
    };
    let ui_poster: UiPoster = {
        let app = app.clone();
        let pid = plugin_id.clone();
        Arc::new(move |window_id, payload| {
            crate::plugin_runtime::windows::push_to_window(&app, &pid, window_id, payload);
        })
    };
    let services: Arc<dyn crate::plugin_runtime::ui_rpc::HostServices> =
        Arc::new(crate::plugin_runtime::ui_rpc::TauriServices::new(app.clone()));
    make_sink(plugin_id, capabilities, log_dir, emitter, ui_poster, Some(services))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_protocol as proto;
    use std::sync::Mutex;

    /// Build a `ToastEmitter` that records every payload it sees.
    fn recording_emitter() -> (ToastEmitter, Arc<Mutex<Vec<serde_json::Value>>>) {
        let seen: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_in = seen.clone();
        let emitter: ToastEmitter = Arc::new(move |v| {
            seen_in.lock().unwrap().push(v);
        });
        (emitter, seen)
    }

    /// A `UiPoster` that ignores everything — used by tests that don't exercise
    /// `host.ui.post`.
    fn noop_poster() -> UiPoster {
        Arc::new(|_, _| {})
    }

    /// Build a `UiPoster` that records every `(window_id, payload)` it sees.
    #[allow(clippy::type_complexity)]
    fn recording_poster() -> (UiPoster, Arc<Mutex<Vec<(String, serde_json::Value)>>>) {
        let seen: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let seen_in = seen.clone();
        let poster: UiPoster = Arc::new(move |window_id: &str, payload: &serde_json::Value| {
            seen_in.lock().unwrap().push((window_id.to_string(), payload.clone()));
        });
        (poster, seen)
    }

    fn req(method: &str, id: Option<u64>, params: serde_json::Value) -> proto::RpcRequest {
        proto::RpcRequest {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params,
        }
    }

    fn notification(method: &str, params: serde_json::Value) -> proto::RpcRequest {
        req(method, None, params)
    }

    // ── ① toast WITHOUT capability, as a request → -32001 ──────────────────

    #[test]
    fn toast_without_capability_request_returns_32001() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, seen) = recording_emitter();
        let sink = make_sink("pub.test".into(), vec![], dir.path().to_path_buf(), emitter, noop_poster(), None);

        let resp = sink(req(
            "host.toast",
            Some(42),
            serde_json::json!({"level": "info", "message": "hi"}),
        ))
        .unwrap();

        assert_eq!(resp.id, 42);
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, proto::ERR_CAPABILITY_DENIED);
        assert!(err.message.contains("toast"), "message: {}", err.message);

        // No emission happened
        assert!(seen.lock().unwrap().is_empty());
    }

    // ── ① toast WITHOUT capability, as a notification → silent no-op ────────

    #[test]
    fn toast_without_capability_notification_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, seen) = recording_emitter();
        let sink = make_sink("pub.test".into(), vec![], dir.path().to_path_buf(), emitter, noop_poster(), None);

        let resp = sink(notification(
            "host.toast",
            serde_json::json!({"level": "info", "message": "hi"}),
        ));

        assert!(resp.is_none(), "notification must return None");
        assert!(seen.lock().unwrap().is_empty());
    }

    // ── ② unknown method → -32601 (request) / None (notification) ───────────

    #[test]
    fn unknown_method_request_returns_32601() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, _) = recording_emitter();
        let sink = make_sink(
            "pub.test".into(),
            vec!["toast".into()],
            dir.path().to_path_buf(),
            emitter,
            noop_poster(),
            None,
        );

        let resp = sink(req("host.unknown.method", Some(7), serde_json::json!({}))).unwrap();
        assert_eq!(resp.id, 7);
        let err = resp.error.unwrap();
        assert_eq!(err.code, proto::ERR_METHOD_NOT_FOUND);
        assert!(
            err.message.contains("host.unknown.method"),
            "message: {}",
            err.message
        );
    }

    #[test]
    fn unknown_method_notification_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, _) = recording_emitter();
        let sink = make_sink(
            "pub.test".into(),
            vec![],
            dir.path().to_path_buf(),
            emitter,
            noop_poster(),
            None,
        );
        let resp = sink(notification("host.doesnt.exist", serde_json::json!({})));
        assert!(resp.is_none());
    }

    // ── ③ log.* without any capability → writes [level] line to <id>.log ────

    #[test]
    fn log_writes_level_tagged_line_to_plugin_log() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, _) = recording_emitter();
        // No capabilities at all — log is free.
        let sink = make_sink("pub.myplugin".into(), vec![], dir.path().to_path_buf(), emitter, noop_poster(), None);

        let resp = sink(req(
            "host.log.info",
            Some(1),
            serde_json::json!({"message": "hello from plugin"}),
        ))
        .unwrap();

        // Request → {ok: true}
        assert_eq!(resp.result, Some(serde_json::json!({"ok": true})));
        assert!(resp.error.is_none());

        let log_path = dir.path().join("pub.myplugin.log");
        let content = std::fs::read_to_string(&log_path)
            .expect("log file should have been created");
        assert!(
            content.contains("[info] hello from plugin"),
            "log content: {content:?}"
        );
    }

    #[test]
    fn log_warn_and_error_write_correct_level_tags() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, _) = recording_emitter();
        let sink = make_sink("pub.myplugin".into(), vec![], dir.path().to_path_buf(), emitter, noop_poster(), None);

        sink(req(
            "host.log.warn",
            None,
            serde_json::json!({"message": "watchout"}),
        ));
        sink(req(
            "host.log.error",
            None,
            serde_json::json!({"message": "boom"}),
        ));

        let log_path = dir.path().join("pub.myplugin.log");
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("[warn] watchout"), "content: {content:?}");
        assert!(content.contains("[error] boom"), "content: {content:?}");
    }

    // ── ④ toast WITH capability → emitter called + response {ok:true} ────────

    #[test]
    fn toast_with_capability_request_emits_payload_and_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, seen) = recording_emitter();
        let sink = make_sink(
            "pub.test".into(),
            vec!["toast".into()],
            dir.path().to_path_buf(),
            emitter,
            noop_poster(),
            None,
        );

        let resp = sink(req(
            "host.toast",
            Some(99),
            serde_json::json!({"level": "success", "message": "done!", "detail": null}),
        ))
        .unwrap();

        assert_eq!(resp.id, 99);
        assert_eq!(resp.result, Some(serde_json::json!({"ok": true})));
        assert!(resp.error.is_none());

        let emitted = seen.lock().unwrap();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0]["plugin_id"], "pub.test");
        assert_eq!(emitted[0]["level"], "success");
        assert_eq!(emitted[0]["message"], "done!");
    }

    // ── ④ toast WITH capability as notification → emitter called, no response ─

    #[test]
    fn toast_with_capability_notification_emits_and_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, seen) = recording_emitter();
        let sink = make_sink(
            "pub.plug".into(),
            vec!["toast".into()],
            dir.path().to_path_buf(),
            emitter,
            noop_poster(),
            None,
        );

        let resp = sink(notification(
            "host.toast",
            serde_json::json!({"level": "warn", "message": "heads up"}),
        ));

        assert!(resp.is_none(), "notification must return None");
        let emitted = seen.lock().unwrap();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0]["plugin_id"], "pub.plug");
        assert_eq!(emitted[0]["level"], "warn");
    }

    // ── method_capability table ───────────────────────────────────────────────

    #[test]
    fn method_capability_table() {
        assert_eq!(method_capability("host.log.info"), None);
        assert_eq!(method_capability("host.log.warn"), None);
        assert_eq!(method_capability("host.log.error"), None);
        assert_eq!(method_capability("host.toast"), Some("toast"));
        assert_eq!(method_capability("host.ui.post"), Some("ui"));
        assert_eq!(method_capability("host.dialog.open"), Some("dialog"));
        assert_eq!(method_capability("host.dialog.save"), Some("dialog"));
        assert_eq!(method_capability("host.vault.info"), Some("vault.read"));
        assert_eq!(method_capability("host.vault.read"), Some("vault.read"));
        assert_eq!(method_capability("host.vault.read_bytes"), Some("vault.read"));
        assert_eq!(method_capability("host.vault.exists"), Some("vault.read"));
        assert_eq!(method_capability("host.vault.list"), Some("vault.read"));
        assert_eq!(method_capability("host.vault.write"), Some("vault.write"));
        assert_eq!(method_capability("host.vault.mkdir"), Some("vault.write"));
        assert_eq!(method_capability("host.fs.read_text"), Some("fs.read:dialog"));
        assert_eq!(method_capability("host.fs.read_bytes"), Some("fs.read:dialog"));
        assert_eq!(method_capability("host.clipboard.write"), Some("clipboard.write"));
        assert_eq!(method_capability("host.editor.open"), Some("editor.open"));
        assert_eq!(method_capability("host.agent.run"), Some("agent"));
        assert_eq!(method_capability("host.agent.status"), Some("agent"));
        assert_eq!(method_capability("host.notify"), Some("notify"));
        assert_eq!(method_capability("host.theme.css"), Some("editor.kit"));
        assert_eq!(method_capability("host.unknown"), Some("__unknown__"));
        assert_eq!(method_capability("anything.else"), Some("__unknown__"));
    }

    // ── UI-only methods over the process channel → -32601, never a panic ──────

    #[test]
    fn vault_without_services_on_process_channel_returns_32601_even_when_authorized() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, seen) = recording_emitter();
        // The plugin legitimately HOLDS vault.read — the gate passes — but the
        // sink was built WITHOUT services (services: None), so vault stays
        // unavailable here. (With Some(services) it works — see
        // vault_round_trip_on_process_channel_with_services.)
        let sink = make_sink(
            "pub.test".into(),
            vec!["vault.read".into()],
            dir.path().to_path_buf(),
            emitter,
            noop_poster(),
            None,
        );

        let resp = sink(req(
            "host.vault.read",
            Some(5),
            serde_json::json!({"path": "a.md"}),
        ))
        .unwrap();
        let err = resp.error.unwrap();
        assert_eq!(err.code, proto::ERR_METHOD_NOT_FOUND);
        assert!(
            err.message.contains("process channel"),
            "message: {}",
            err.message
        );

        // Notification variant: silent no-op.
        let resp = sink(notification("host.vault.read", serde_json::json!({"path": "a.md"})));
        assert!(resp.is_none());
        assert!(seen.lock().unwrap().is_empty());
    }

    // ── 子项目②b host.ui.post ─────────────────────────────────────────────────

    /// WITH the `ui` capability, `host.ui.post` parses `{window_id, payload}` and
    /// forwards it to the injected `ui_poster`; a request gets `{ok:true}`.
    #[test]
    fn ui_post_with_capability_calls_poster_and_returns_ok() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, _) = recording_emitter();
        let (poster, posted) = recording_poster();
        let sink = make_sink(
            "pub.chat".into(),
            vec!["ui".into()],
            dir.path().to_path_buf(),
            emitter,
            poster,
            None,
        );

        let resp = sink(req(
            "host.ui.post",
            Some(7),
            serde_json::json!({"window_id": "main", "payload": {"kind": "frame", "seq": 3}}),
        ))
        .unwrap();
        assert_eq!(resp.id, 7);
        assert_eq!(resp.result, Some(serde_json::json!({"ok": true})));
        assert!(resp.error.is_none());

        let posted = posted.lock().unwrap();
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0].0, "main");
        assert_eq!(posted[0].1, serde_json::json!({"kind": "frame", "seq": 3}));
    }

    /// As a notification (openclaw's actual usage: fire-and-forget streaming),
    /// the poster is still called but there is no reply.
    #[test]
    fn ui_post_notification_calls_poster_and_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, _) = recording_emitter();
        let (poster, posted) = recording_poster();
        let sink = make_sink(
            "pub.chat".into(),
            vec!["ui".into()],
            dir.path().to_path_buf(),
            emitter,
            poster,
            None,
        );

        let resp = sink(notification(
            "host.ui.post",
            serde_json::json!({"window_id": "main", "payload": {"seq": 1}}),
        ));
        assert!(resp.is_none(), "notification must return None");

        let posted = posted.lock().unwrap();
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0].0, "main");
        assert_eq!(posted[0].1, serde_json::json!({"seq": 1}));
    }

    /// WITHOUT the `ui` capability, `host.ui.post` is denied (-32001) and the
    /// poster is NEVER called — the capability gate is the sole guard.
    #[test]
    fn ui_post_without_capability_returns_32001_and_does_not_post() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, _) = recording_emitter();
        let (poster, posted) = recording_poster();
        let sink = make_sink(
            "pub.chat".into(),
            vec![], // no `ui`
            dir.path().to_path_buf(),
            emitter,
            poster,
            None,
        );

        let resp = sink(req(
            "host.ui.post",
            Some(9),
            serde_json::json!({"window_id": "main", "payload": {"seq": 1}}),
        ))
        .unwrap();
        assert_eq!(resp.id, 9);
        let err = resp.error.unwrap();
        assert_eq!(err.code, proto::ERR_CAPABILITY_DENIED);
        assert!(err.message.contains("ui"), "message: {}", err.message);

        // The poster must NOT have been called.
        assert!(posted.lock().unwrap().is_empty());
    }

    /// Missing `ui` capability as a notification → silent no-op, poster untouched.
    #[test]
    fn ui_post_without_capability_notification_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        let (emitter, _) = recording_emitter();
        let (poster, posted) = recording_poster();
        let sink = make_sink(
            "pub.chat".into(),
            vec![],
            dir.path().to_path_buf(),
            emitter,
            poster,
            None,
        );
        let resp = sink(notification(
            "host.ui.post",
            serde_json::json!({"window_id": "main", "payload": {}}),
        ));
        assert!(resp.is_none());
        assert!(posted.lock().unwrap().is_empty());
    }

    // ── vault.* on the process channel (pos-log 前置) ─────────────────────────

    /// 最小 HostServices 桩：vault_root 之外，第二个字段记录
    /// `agent_execute`/`notify_user` 调用，供 host.agent.*/host.notify 测试断言。
    struct ServicesStub(std::path::PathBuf, Arc<Mutex<Vec<(String, serde_json::Value)>>>);
    impl crate::plugin_runtime::ui_rpc::HostServices for ServicesStub {
        fn pick_paths(
            &self,
            _o: &crate::plugin_runtime::ui_rpc::OpenOptions,
        ) -> Result<Option<Vec<std::path::PathBuf>>, String> {
            Err("no dialogs on process channel".into())
        }
        fn pick_save(
            &self,
            _o: &crate::plugin_runtime::ui_rpc::SaveOptions,
        ) -> Result<Option<std::path::PathBuf>, String> {
            Err("no dialogs on process channel".into())
        }
        fn vault_root(&self) -> Option<std::path::PathBuf> {
            Some(self.0.clone())
        }
        fn wiki_daily_dirs(&self) -> (Option<String>, Option<String>) {
            (None, None)
        }
        fn clipboard_write(&self, _t: &str) -> Result<(), String> {
            Err("no clipboard on process channel".into())
        }
        fn agent_execute(&self, command: &str, context: serde_json::Value) -> Result<serde_json::Value, String> {
            self.1.lock().unwrap().push((command.to_string(), context));
            Ok(serde_json::json!({ "run_id": "r-test" }))
        }
        fn notify_user(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
            self.1.lock().unwrap().push(("notify".into(), params.clone()));
            Ok(serde_json::json!({ "ok": true, "id": 1 }))
        }
    }

    #[test]
    fn vault_round_trip_on_process_channel_with_services() {
        let log_dir = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let (emitter, _) = recording_emitter();
        let sink = make_sink(
            "pub.test".into(),
            vec!["vault.read".into(), "vault.write".into()],
            log_dir.path().to_path_buf(),
            emitter,
            noop_poster(),
            Some(Arc::new(ServicesStub(vault.path().to_path_buf(), Arc::new(Mutex::new(Vec::new()))))),
        );
        // write → {ok:true}
        let resp = sink(req(
            "host.vault.write",
            Some(1),
            serde_json::json!({"path": "pos/x.md", "content": "- line\n"}),
        ))
        .unwrap();
        assert!(resp.error.is_none(), "write err: {:?}", resp.error);
        assert_eq!(resp.result.unwrap()["ok"], true);
        // exists → true
        let resp = sink(req(
            "host.vault.exists",
            Some(2),
            serde_json::json!({"path": "pos/x.md"}),
        ))
        .unwrap();
        assert_eq!(resp.result.unwrap()["exists"], true);
        // read → 原文
        let resp = sink(req(
            "host.vault.read",
            Some(3),
            serde_json::json!({"path": "pos/x.md"}),
        ))
        .unwrap();
        assert_eq!(resp.result.unwrap()["content"], "- line\n");
        // 越权：无 capability 的方法仍被拒
        let resp = sink(req(
            "host.clipboard.write",
            Some(4),
            serde_json::json!({"text": "x"}),
        ))
        .unwrap();
        assert_eq!(resp.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);
        // dialog 类持 capability 也仍是 -32601（进程通道不做对话框）
        let sink2 = make_sink(
            "pub.test".into(),
            vec!["dialog".into()],
            log_dir.path().to_path_buf(),
            recording_emitter().0,
            noop_poster(),
            Some(Arc::new(ServicesStub(vault.path().to_path_buf(), Arc::new(Mutex::new(Vec::new()))))),
        );
        let resp = sink2(req("host.dialog.open", Some(5), serde_json::json!({}))).unwrap();
        assert_eq!(resp.error.unwrap().code, proto::ERR_METHOD_NOT_FOUND);
    }

    #[test]
    fn vault_on_process_channel_without_services_stays_32601() {
        let dir = tempfile::tempdir().unwrap();
        let sink = make_sink(
            "pub.test".into(),
            vec!["vault.read".into()],
            dir.path().to_path_buf(),
            recording_emitter().0,
            noop_poster(),
            None,
        );
        let resp = sink(req(
            "host.vault.read",
            Some(1),
            serde_json::json!({"path": "a.md"}),
        ))
        .unwrap();
        assert_eq!(resp.error.unwrap().code, proto::ERR_METHOD_NOT_FOUND);
    }

    // ── host.agent.*/host.notify (Task 3) ──────────────────────────────────

    #[test]
    fn agent_run_on_process_channel_relays_run_task_with_capability() {
        let log_dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = make_sink(
            "pub.test".into(),
            vec!["agent".into()],
            log_dir.path().to_path_buf(),
            recording_emitter().0,
            noop_poster(),
            Some(Arc::new(ServicesStub(std::env::temp_dir(), seen.clone()))),
        );
        let resp = sink(req(
            "host.agent.run",
            Some(1),
            serde_json::json!({"task": "ai-read-ebook", "prompt": "p", "note_path": "/v/b/book.md"}),
        ))
        .unwrap();
        assert_eq!(resp.result.unwrap()["run_id"], "r-test");
        let calls = seen.lock().unwrap();
        assert_eq!(calls[0].0, "run-task");
        assert_eq!(calls[0].1["task"], "ai-read-ebook");
    }

    #[test]
    fn agent_and_notify_without_capability_are_denied() {
        let log_dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = make_sink(
            "pub.test".into(),
            vec![], // 无 agent/notify
            log_dir.path().to_path_buf(),
            recording_emitter().0,
            noop_poster(),
            Some(Arc::new(ServicesStub(std::env::temp_dir(), seen.clone()))),
        );
        let resp = sink(req("host.agent.run", Some(1), serde_json::json!({}))).unwrap();
        assert_eq!(resp.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);
        let resp = sink(req("host.notify", Some(2), serde_json::json!({}))).unwrap();
        assert_eq!(resp.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn agent_status_relays_run_status() {
        let log_dir = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let summary = vault.path().join("summary.md");
        std::fs::write(&summary, "# x").unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = make_sink(
            "pub.test".into(),
            vec!["agent".into(), "notify".into()],
            log_dir.path().to_path_buf(),
            recording_emitter().0,
            noop_poster(),
            Some(Arc::new(ServicesStub(vault.path().to_path_buf(), seen.clone()))),
        );
        sink(req("host.agent.status", Some(1), serde_json::json!({"task": "ai-read-ebook", "run_id": "r1"})));
        sink(req(
            "host.notify",
            Some(2),
            serde_json::json!({"title": "t", "action": {"kind": "open_path", "path": summary.to_string_lossy()}}),
        ));
        let calls = seen.lock().unwrap();
        assert_eq!(calls[0].0, "run-status");
        assert_eq!(calls[1].0, "notify");
    }

    /// The process channel is the one plugins like claude-agent actually push
    /// reminders on, so the vault fence has to hold HERE too — not only on the
    /// UI bridge.
    #[test]
    fn notify_on_process_channel_refuses_a_path_outside_the_vault() {
        let log_dir = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("id_rsa");
        std::fs::write(&secret, "x").unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = make_sink(
            "pub.test".into(),
            vec!["notify".into()],
            log_dir.path().to_path_buf(),
            recording_emitter().0,
            noop_poster(),
            Some(Arc::new(ServicesStub(vault.path().to_path_buf(), seen.clone()))),
        );
        let resp = sink(req(
            "host.notify",
            Some(1),
            serde_json::json!({"title": "t", "action": {"kind": "open_path", "path": secret.to_string_lossy()}}),
        ))
        .unwrap();
        assert!(
            resp.error.unwrap().message.contains("escapes the vault"),
            "an out-of-vault reminder must be refused"
        );
        assert!(seen.lock().unwrap().is_empty());
    }
}
