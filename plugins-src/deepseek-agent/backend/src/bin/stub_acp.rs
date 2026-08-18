//! A scripted stand-in for `dsh-acp-demo`, for tests. It speaks the AGENT side
//! of ACP over stdio and does nothing else — no model, no network, no harness.
//!
//! Modelled on the harness's own `packages/subagent/subagent-acp/tests/
//! mock-acp-server.ts`, and driven the same way: entirely by environment
//! variables, so one binary covers every path the engine has to survive.
//!
//! | Variable | Effect |
//! | --- | --- |
//! | `STUB_TEXT` | text streamed as one `agent_message_chunk` (default "stub answer") |
//! | `STUB_CHUNKS` | stream the text this many times, 60 ms apart — for the quiet-timeout test |
//! | `STUB_STOP` | the `stopReason` returned from `session/prompt` (default `end_turn`) |
//! | `STUB_HANG` | never answer `session/prompt`; wait for `session/cancel` |
//! | `STUB_PERMISSION` | ask `session/request_permission` before answering, and stream the outcome |
//! | `STUB_NO_ALLOW` | offer only a reject-shaped option |
//! | `STUB_BAD_VERSION` | answer `initialize` with a protocolVersion we do not speak |
//! | `STUB_NO_SESSION_ID` | answer `session/new` without a session id |
//! | `STUB_PROMPT_ERROR` | answer `session/prompt` with a JSON-RPC error |
//! | `STUB_ECHO_CWD` | stream the process cwd and the announced session cwd instead of the text |
//! | `STUB_ECHO_ENV` | stream the named variable's value instead of the text |
//! | `STUB_NOISE` | print a non-JSON banner line before the protocol starts |
//! | `STUB_DIE_EARLY` | print install-style noise on stdout, nothing on stderr, exit 1 before the protocol |
//! | `STUB_ARGV_FILE` | write the argv it was launched with to this path |
use std::io::{BufRead, Write};

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|s| !s.is_empty())
}

fn flag(k: &str) -> bool {
    env(k).is_some_and(|v| v == "1")
}

fn send(v: serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn ok(id: &serde_json::Value, result: serde_json::Value) {
    send(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }));
}

fn main() {
    if let Some(p) = env("STUB_ARGV_FILE") {
        let argv: Vec<String> = std::env::args().collect();
        let _ = std::fs::write(p, argv.join("\n"));
    }
    if flag("STUB_DIE_EARLY") {
        // 真实事故的形状(2026-08-18):`pnpm run` 先补装依赖,几分钟的进度全在
        // stdout,stderr 一个字没有,最后 exit 1,协议一帧未发。
        println!("Scope: all 238 workspace projects");
        println!("Progress: resolved 925, reused 908, downloaded 1, added 915");
        println!("[ERROR] TimeoutError: The operation was aborted due to timeout");
        std::process::exit(1);
    }
    if flag("STUB_NOISE") {
        // A real harness prints diagnostics; the client must not choke on them.
        println!("dsh-acp-demo booting (this line is not a protocol frame)");
        eprintln!("stub: stderr diagnostics go here");
    }

    let text = env("STUB_TEXT").unwrap_or_else(|| "stub answer".to_string());
    let chunks: usize = env("STUB_CHUNKS")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let stop = env("STUB_STOP").unwrap_or_else(|| "end_turn".to_string());

    let mut session_cwd = String::new();
    // One loop, one reader. A prompt that is waiting — for a permission answer
    // or for a cancel — parks its id here and is finished later, rather than
    // blocking inside the handler while the answer sits unread on stdin.
    let mut parked: Option<serde_json::Value> = None;
    let mut permission_outcome: Option<String> = None;

    let stdin = std::io::stdin();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        let id = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = v.get("params").cloned().unwrap_or(serde_json::Value::Null);

        // The client's answer to our permission request. Not a method call —
        // it carries a result, so it is matched by id.
        if method.is_empty() && id.as_str() == Some("perm-1") {
            let o = &v["result"]["outcome"];
            permission_outcome = Some(match o["outcome"].as_str() {
                Some("selected") => format!("selected:{}", o["optionId"].as_str().unwrap_or("?")),
                other => other.unwrap_or("?").to_string(),
            });
            if let Some(prompt_id) = parked.take() {
                finish_prompt(&prompt_id, &session_cwd, &text, chunks, &stop, &permission_outcome);
            }
            continue;
        }

        match method {
            "initialize" => {
                let version = if flag("STUB_BAD_VERSION") { 99 } else { 1 };
                ok(
                    &id,
                    serde_json::json!({
                        "protocolVersion": version,
                        "agentInfo": { "name": "stub-acp", "version": "0.0.0" },
                        "agentCapabilities": {
                            "promptCapabilities": { "image": false, "audio": false, "embeddedContext": false }
                        },
                        "authMethods": [],
                    }),
                );
            }
            "session/new" => {
                session_cwd = params
                    .get("cwd")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default()
                    .to_string();
                if flag("STUB_NO_SESSION_ID") {
                    ok(&id, serde_json::json!({}));
                } else {
                    ok(&id, serde_json::json!({ "sessionId": "stub-session-1" }));
                }
            }
            "session/prompt" => {
                if flag("STUB_PROMPT_ERROR") {
                    send(serde_json::json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": { "code": -32603, "message": "Internal error",
                                   "data": "turn failed: the stub was told to fail" },
                    }));
                    continue;
                }
                if flag("STUB_PERMISSION") && permission_outcome.is_none() {
                    let options = if flag("STUB_NO_ALLOW") {
                        serde_json::json!([{ "optionId": "no", "name": "Reject", "kind": "reject_once" }])
                    } else {
                        serde_json::json!([
                            { "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" },
                            { "optionId": "reject-once", "name": "Reject", "kind": "reject_once" },
                        ])
                    };
                    // A STRING id: legal JSON-RPC, and it catches a client that
                    // assumes the agent numbers its requests the way we do.
                    send(serde_json::json!({
                        "jsonrpc": "2.0", "id": "perm-1",
                        "method": "session/request_permission",
                        "params": {
                            "sessionId": params.get("sessionId").cloned().unwrap_or_default(),
                            "toolCall": { "toolCallId": "call-7" },
                            "options": options,
                        },
                    }));
                    parked = Some(id);
                    continue;
                }
                if flag("STUB_HANG") {
                    // Stream first so the client has seen activity, then park:
                    // only its quiet timeout or a session/cancel ends this turn.
                    stream(&session_cwd, &text, chunks, &None);
                    parked = Some(id);
                    continue;
                }
                finish_prompt(&id, &session_cwd, &text, chunks, &stop, &permission_outcome);
            }
            "session/cancel" => {
                // A notification: no response. A cooperative agent settles its
                // pending prompt as cancelled, says so, and exits.
                if let Some(prompt_id) = parked.take() {
                    ok(&prompt_id, serde_json::json!({ "stopReason": "cancelled" }));
                }
                std::process::exit(0);
            }
            _ => {}
        }
    }
}

/// Stream the scripted body, plus a thought the client must consume silently.
fn stream(session_cwd: &str, text: &str, chunks: usize, outcome: &Option<String>) {
    let body = if flag("STUB_ECHO_CWD") {
        format!(
            "{}\n{}",
            std::env::current_dir().unwrap_or_default().display(),
            session_cwd
        )
    } else if let Some(name) = env("STUB_ECHO_ENV") {
        std::env::var(&name).unwrap_or_else(|_| format!("<{name} unset>"))
    } else if let Some(o) = outcome {
        format!("{text} [permission: {o}]")
    } else {
        text.to_string()
    };
    for i in 0..chunks {
        if i > 0 {
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        send(serde_json::json!({
            "jsonrpc": "2.0", "method": "session/update",
            "params": {
                "sessionId": "stub-session-1",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": { "type": "text", "text": body },
                },
            },
        }));
        send(serde_json::json!({
            "jsonrpc": "2.0", "method": "session/update",
            "params": {
                "sessionId": "stub-session-1",
                "update": {
                    "sessionUpdate": "agent_thought_chunk",
                    "content": { "type": "text", "text": "thinking…" },
                },
            },
        }));
    }
}

fn finish_prompt(
    id: &serde_json::Value,
    session_cwd: &str,
    text: &str,
    chunks: usize,
    stop: &str,
    outcome: &Option<String>,
) {
    stream(session_cwd, text, chunks, outcome);
    ok(id, serde_json::json!({ "stopReason": stop }));
}
