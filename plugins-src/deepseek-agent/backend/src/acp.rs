//! The Agent Client Protocol wire layer. Pure functions only — framing,
//! classification, and the projection onto [`Event`] — so every rule here is
//! unit-testable without a subprocess. The socket work lives in `engine.rs`.
//!
//! ## What the protocol actually gives us
//!
//! Verified against `@agentclientprotocol/sdk@0.25.1` and the DeepSeek Harness
//! server at `packages/acp/acp/src/index.ts`. This matters because the surface is
//! much narrower than ACP-in-general:
//!
//! - We send exactly four methods: `initialize`, `session/new`, `session/prompt`,
//!   and the `session/cancel` **notification**.
//! - The agent sends exactly two: the `session/update` **notification** and the
//!   `session/request_permission` **request** (which we must answer).
//! - `session/update` carries only `agent_message_chunk` with committed
//!   assistant text. The server's own comment: "Raw chunks, reasoning, tools,
//!   plans, titles, and retry markers are presentation or trace data and stay off
//!   the automation wire." **There are no tool-call events to map.**
//! - `session/request_permission` carries only a `toolCallId` — no tool name, no
//!   path, no arguments. A policy keyed on paths is therefore impossible; see
//!   `policy.rs` for what we do instead.
//! - `session/new` rejects a non-empty `mcpServers` or `additionalDirectories`.
//! - `loadSession` / `session/list` / `session/resume` do not exist. No resume.
use agent_run_core::event::{Event, RunResult};
use agent_run_core::usage::{Cost, CostKind, Usage};
use serde_json::{json, Value};

/// The protocol version we speak (`@agentclientprotocol/sdk` `PROTOCOL_VERSION`).
pub const PROTOCOL_VERSION: u64 = 1;

pub const METHOD_INITIALIZE: &str = "initialize";
pub const METHOD_SESSION_NEW: &str = "session/new";
pub const METHOD_SESSION_PROMPT: &str = "session/prompt";
pub const METHOD_SESSION_CANCEL: &str = "session/cancel";
pub const METHOD_SESSION_UPDATE: &str = "session/update";
pub const METHOD_REQUEST_PERMISSION: &str = "session/request_permission";

/// One decoded frame off the agent's stdout.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// An answer to something we asked. `Err` carries the JSON-RPC error message.
    Response {
        id: u64,
        result: Result<Value, String>,
    },
    /// The agent is asking US something and wants an answer at this id. The id is
    /// kept as a raw `Value` because JSON-RPC allows strings as well as numbers
    /// and it has to be echoed back byte-identically.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// Fire-and-forget from the agent.
    Notification { method: String, params: Value },
}

/// Classify one NDJSON line. `None` means "no frame here" — a blank line, a
/// non-JSON diagnostic that leaked onto stdout, or a shape we do not recognize.
/// Noise is dropped rather than fatal: the run must survive a chatty child.
pub fn parse_incoming(line: &str) -> Option<Incoming> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    let obj = v.as_object()?;
    let id = obj.get("id");

    if let Some(method) = obj.get("method").and_then(|m| m.as_str()) {
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        return Some(match id {
            // A JSON-RPC null id is "no id" — a notification that spelled it out.
            Some(Value::Null) | None => Incoming::Notification {
                method: method.to_string(),
                params,
            },
            Some(id) => Incoming::Request {
                id: id.clone(),
                method: method.to_string(),
                params,
            },
        });
    }

    // A response. Ours always carry numeric ids; anything else is not for us.
    let id = id?.as_u64()?;
    if let Some(err) = obj.get("error") {
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        // The server puts the useful detail (invalid params, auth) in `data`.
        let detail = err.get("data").and_then(|d| d.as_str());
        return Some(Incoming::Response {
            id,
            result: Err(match detail {
                Some(d) if !d.is_empty() => format!("{message}: {d}"),
                _ => message.to_string(),
            }),
        });
    }
    Some(Incoming::Response {
        id,
        result: Ok(obj.get("result").cloned().unwrap_or(Value::Null)),
    })
}

/// One NDJSON line: a request we expect an answer to.
pub fn request_frame(id: u64, method: &str, params: Value) -> String {
    format!(
        "{}\n",
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
    )
}

/// One NDJSON line: a notification (no id, no answer).
pub fn notification_frame(method: &str, params: Value) -> String {
    format!(
        "{}\n",
        json!({ "jsonrpc": "2.0", "method": method, "params": params })
    )
}

/// One NDJSON line: our answer to the agent's request, echoing its id verbatim.
pub fn response_frame(id: &Value, result: Value) -> String {
    format!(
        "{}\n",
        json!({ "jsonrpc": "2.0", "id": id, "result": result })
    )
}

/// The `initialize` params. We advertise NO optional client capabilities: the
/// harness self-serves filesystem and terminal access inside its own sandbox, and
/// claiming `fs`/`terminal` would make it call back into us for work we have no
/// business doing on its behalf.
pub fn initialize_params() -> Value {
    json!({ "protocolVersion": PROTOCOL_VERSION, "clientCapabilities": {} })
}

/// The `session/new` params. `mcpServers` MUST be empty — a non-empty list is
/// rejected outright by this server (`validateSessionParams`), so vault tools are
/// mounted in the harness composition instead (see `composition.rs`).
pub fn new_session_params(cwd: &str) -> Value {
    json!({ "cwd": cwd, "mcpServers": [] })
}

/// The `session/prompt` params. Only `text` and `resource_link` blocks are legal;
/// we send one text block, because that is what `compose` already produced.
pub fn prompt_params(session_id: &str, text: &str) -> Value {
    json!({
        "sessionId": session_id,
        "prompt": [{ "type": "text", "text": text }],
    })
}

pub fn cancel_params(session_id: &str) -> Value {
    json!({ "sessionId": session_id })
}

/// Check the handshake answer. There are no optional capability bits to probe —
/// this server advertises only `promptCapabilities`, all false — so the one thing
/// worth asserting is that we are talking to something that speaks our protocol
/// version at all. Fail loud beats running a session that silently misbehaves.
pub fn check_initialize(result: &Value) -> Result<u64, String> {
    let got = result
        .get("protocolVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            format!("the agent's initialize reply has no numeric protocolVersion: {result}")
        })?;
    if got != PROTOCOL_VERSION {
        return Err(format!(
            "ACP protocol mismatch: this plugin speaks v{PROTOCOL_VERSION}, the agent answered v{got}. \
             Update DeepSeek Harness (npm i -g @deepseek-ai/dsh-acp-demo), or the plugin."
        ));
    }
    Ok(got)
}

/// The session id out of a `session/new` reply.
pub fn session_id(result: &Value) -> Result<String, String> {
    result
        .get("sessionId")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("session/new returned no session id: {result}"))
}

/// Project a `session/update` notification onto the shared event contract.
///
/// Only `agent_message_chunk` text survives. `agent_thought_chunk` and every
/// other update kind are consumed and dropped — not because we are being
/// selective, but because this server never sends them (and a future one that
/// did would be adding trace data, not results).
pub fn update_to_event(params: &Value) -> Option<Event> {
    let update = params.get("update")?;
    if update.get("sessionUpdate")?.as_str()? != "agent_message_chunk" {
        return None;
    }
    let content = update.get("content")?;
    if content.get("type")?.as_str()? != "text" {
        return None;
    }
    let text = content.get("text")?.as_str()?;
    (!text.is_empty()).then(|| Event::Text {
        text: text.to_string(),
    })
}

/// How a permission request should be answered. Derived from the task's
/// `policy.json`; see `policy.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Reject,
}

impl Decision {
    pub fn label(self) -> &'static str {
        match self {
            Decision::Allow => "allowed",
            Decision::Reject => "rejected",
        }
    }
}

/// Build the `session/request_permission` result for a decision.
///
/// The agent offers a list of options and we must name one of ITS ids — we
/// cannot invent `"allow"`. When no option of the wanted kind is on offer, the
/// only honest answer is `cancelled`: selecting the wrong kind would approve
/// something the policy refused.
pub fn permission_result(options: &[Value], decision: Decision) -> Value {
    let wanted: [&str; 2] = match decision {
        Decision::Allow => ["allow_once", "allow_always"],
        Decision::Reject => ["reject_once", "reject_always"],
    };
    let picked = options.iter().find_map(|o| {
        let kind = o.get("kind")?.as_str()?;
        wanted
            .contains(&kind)
            .then(|| o.get("optionId")?.as_str().map(str::to_string))
            .flatten()
    });
    match picked {
        Some(id) => json!({ "outcome": { "outcome": "selected", "optionId": id } }),
        None => json!({ "outcome": { "outcome": "cancelled" } }),
    }
}

/// The tool call a permission request is about. Only an opaque id is available —
/// the server sends `toolCall: { toolCallId }` and nothing else — so this is a
/// label for the run log, never something to match a policy rule against.
pub fn permission_tool_id(params: &Value) -> String {
    params
        .pointer("/toolCall/toolCallId")
        .and_then(|v| v.as_str())
        .unwrap_or("(unnamed tool call)")
        .to_string()
}

pub fn permission_options(params: &Value) -> Vec<Value> {
    params
        .get("options")
        .and_then(|o| o.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Normalize the optional usage extension carried by newer `session/prompt`
/// replies. ACP uses camelCase, while some harness builds have emitted
/// snake_case (and `cacheRead`/`reasoning` spellings), so accept all known
/// aliases without making usage mandatory for older servers.
pub fn prompt_usage(result: &Value) -> Option<Usage> {
    let raw = result.get("usage").and_then(Value::as_object);
    let tokens = |aliases: &[&str]| {
        raw.and_then(|usage| {
            aliases
                .iter()
                .find_map(|key| usage.get(*key).and_then(as_u64))
        })
    };
    let model = raw
        .and_then(|usage| first(usage, &["model", "model_id", "modelId"]))
        .or_else(|| first_value(result, &["model", "model_id", "modelId"]))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let cost = parse_cost(result).or_else(|| raw.and_then(|usage| parse_cost_object(usage)));

    let input_tokens = tokens(&["input_tokens", "inputTokens"]);
    let cache_read_tokens = tokens(&[
        "cache_read_tokens",
        "cacheReadTokens",
        "cached_read_tokens",
        "cachedReadTokens",
    ]);
    let cache_write_tokens = tokens(&[
        "cache_write_tokens",
        "cacheWriteTokens",
        "cached_write_tokens",
        "cachedWriteTokens",
    ]);
    let output_tokens = tokens(&["output_tokens", "outputTokens"]);
    let reasoning_tokens = tokens(&[
        "reasoning_tokens",
        "reasoningTokens",
        "thought_tokens",
        "thoughtTokens",
    ]);
    let reported_total_tokens = tokens(&["total_tokens", "totalTokens"]);

    let observed = input_tokens.is_some()
        || cache_read_tokens.is_some()
        || cache_write_tokens.is_some()
        || output_tokens.is_some()
        || reasoning_tokens.is_some()
        || reported_total_tokens.is_some()
        || cost.is_some();
    let usage = Usage {
        model,
        input_tokens: input_tokens.unwrap_or(0),
        cache_read_tokens: cache_read_tokens.unwrap_or(0),
        cache_write_tokens: cache_write_tokens.unwrap_or(0),
        output_tokens: output_tokens.unwrap_or(0),
        reasoning_tokens: reasoning_tokens.unwrap_or(0),
        reported_total_tokens: reported_total_tokens.unwrap_or(0),
        cost,
    };
    (observed && !usage.is_empty()).then_some(usage)
}

fn as_u64(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        let number = value.as_f64()?;
        (number.is_finite() && number >= 0.0 && number.fract() == 0.0 && number <= u64::MAX as f64)
            .then_some(number as u64)
    })
}

fn first<'a>(object: &'a serde_json::Map<String, Value>, aliases: &[&str]) -> Option<&'a Value> {
    aliases.iter().find_map(|key| object.get(*key))
}

fn first_value<'a>(value: &'a Value, aliases: &[&str]) -> Option<&'a Value> {
    let object = value.as_object()?;
    first(object, aliases)
}

fn parse_cost(result: &Value) -> Option<Cost> {
    let object = result.as_object()?;
    parse_cost_object(object)
}

fn parse_cost_object(object: &serde_json::Map<String, Value>) -> Option<Cost> {
    let direct = first(
        object,
        &["total_cost_usd", "totalCostUsd", "cost_usd", "costUsd"],
    )
    .and_then(Value::as_f64);
    let amount = direct.or_else(|| match object.get("cost")? {
        Value::Number(value) => value.as_f64(),
        Value::Object(cost) => {
            let currency = first(cost, &["currency", "currency_code", "currencyCode"])
                .and_then(Value::as_str)
                .unwrap_or("USD");
            if !currency.eq_ignore_ascii_case("USD") {
                return None;
            }
            first(cost, &["amount_usd", "amountUsd", "amount"]).and_then(Value::as_f64)
        }
        _ => None,
    })?;
    (amount.is_finite() && amount >= 0.0).then_some(Cost {
        amount_usd: amount,
        kind: CostKind::ProviderReported,
        pricing_as_of: None,
    })
}

/// Turn a terminal `stopReason` plus the text we accumulated into a run result.
///
/// Only `end_turn` is success. `cancelled` is reported by the caller as a
/// cancellation, not an error. Everything else — including a variant a newer SDK
/// might add — is a failure: a partial answer must never be recorded as a clean
/// finish.
pub fn result_for_stop(
    stop: &str,
    text: &str,
    session_id: &str,
    usage: Option<Usage>,
) -> RunResult {
    let (is_error, note) = match stop {
        "end_turn" => (false, None),
        "cancelled" => (true, Some("the run was cancelled")),
        "max_tokens" => (true, Some("the model hit its token limit before finishing")),
        "refusal" => (true, Some("the model refused the request")),
        "max_turn_requests" => (true, Some("the run hit its turn-request budget")),
        _ => (true, None),
    };
    let body = text.trim();
    let result = match (note, body.is_empty()) {
        (None, _) => body.to_string(),
        (Some(n), true) => format!("{n} (stopReason: {stop})"),
        (Some(n), false) => format!("{body}\n\n[{n} — stopReason: {stop}]"),
    };
    let result = if result.is_empty() {
        format!("the agent finished with stopReason: {stop}")
    } else {
        result
    };
    RunResult {
        is_error,
        result,
        session_id: (!session_id.is_empty()).then(|| session_id.to_string()),
        // ACP reports no turn count. Leaving it None is honest; a fabricated
        // number would show up in the window as if it had been measured.
        num_turns: None,
        usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_a_response_a_request_and_a_notification() {
        assert_eq!(
            parse_incoming(r#"{"jsonrpc":"2.0","id":3,"result":{"sessionId":"s1"}}"#),
            Some(Incoming::Response {
                id: 3,
                result: Ok(json!({ "sessionId": "s1" })),
            })
        );
        assert_eq!(
            parse_incoming(
                r#"{"jsonrpc":"2.0","id":"a1","method":"session/request_permission","params":{"x":1}}"#
            ),
            Some(Incoming::Request {
                id: json!("a1"),
                method: METHOD_REQUEST_PERMISSION.into(),
                params: json!({ "x": 1 }),
            })
        );
        assert_eq!(
            parse_incoming(r#"{"jsonrpc":"2.0","method":"session/update","params":{"y":2}}"#),
            Some(Incoming::Notification {
                method: METHOD_SESSION_UPDATE.into(),
                params: json!({ "y": 2 }),
            })
        );
    }

    /// A child that prints a banner, a warning, or a half-written line must not
    /// take the run down with it.
    #[test]
    fn drops_noise_instead_of_failing() {
        for line in [
            "",
            "   ",
            "not json at all",
            r#"{"jsonrpc":"2.0"}"#,
            r#"{"id":"not-ours","result":{}}"#,
            "{\"half\": ",
        ] {
            assert_eq!(parse_incoming(line), None, "should have been dropped: {line}");
        }
    }

    #[test]
    fn an_explicit_null_id_is_a_notification_not_a_request() {
        assert_eq!(
            parse_incoming(r#"{"jsonrpc":"2.0","id":null,"method":"session/update","params":{}}"#),
            Some(Incoming::Notification {
                method: METHOD_SESSION_UPDATE.into(),
                params: json!({}),
            })
        );
    }

    #[test]
    fn an_error_response_keeps_the_servers_own_detail() {
        let got = parse_incoming(
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"Invalid params","data":"mcpServers is not supported"}}"#,
        );
        assert_eq!(
            got,
            Some(Incoming::Response {
                id: 2,
                result: Err("Invalid params: mcpServers is not supported".into()),
            })
        );
    }

    #[test]
    fn an_error_without_data_still_reports_its_message() {
        let got = parse_incoming(r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32603}}"#);
        assert_eq!(
            got,
            Some(Incoming::Response {
                id: 2,
                result: Err("unknown error".into()),
            })
        );
    }

    #[test]
    fn frames_are_one_line_each_and_parse_back() {
        let r = request_frame(7, METHOD_SESSION_PROMPT, json!({ "sessionId": "s" }));
        assert!(r.ends_with('\n') && r.matches('\n').count() == 1);
        let v: Value = serde_json::from_str(r.trim()).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 7);
        assert_eq!(v["method"], METHOD_SESSION_PROMPT);

        let n = notification_frame(METHOD_SESSION_CANCEL, cancel_params("s1"));
        let v: Value = serde_json::from_str(n.trim()).unwrap();
        assert!(v.get("id").is_none(), "a notification must carry no id: {v}");
        assert_eq!(v["params"]["sessionId"], "s1");
    }

    /// The agent's id may be a string; echoing back a number would orphan its
    /// pending request and wedge the turn.
    #[test]
    fn a_response_echoes_the_agents_id_type_exactly() {
        let f = response_frame(&json!("abc"), json!({ "ok": true }));
        let v: Value = serde_json::from_str(f.trim()).unwrap();
        assert_eq!(v["id"], json!("abc"));
        let f = response_frame(&json!(12), json!({}));
        let v: Value = serde_json::from_str(f.trim()).unwrap();
        assert_eq!(v["id"], json!(12));
    }

    /// A non-empty mcpServers is rejected by this server, so the params builder
    /// must never grow one by accident.
    #[test]
    fn session_new_asks_for_one_workspace_and_no_mcp_servers() {
        let p = new_session_params("/v/.notemd/agent-tasks/selfcheck");
        assert_eq!(p["cwd"], "/v/.notemd/agent-tasks/selfcheck");
        assert_eq!(p["mcpServers"], json!([]));
        assert!(p.get("additionalDirectories").is_none());
    }

    #[test]
    fn a_prompt_is_a_single_text_block() {
        let p = prompt_params("s1", "答一下");
        assert_eq!(p["sessionId"], "s1");
        assert_eq!(p["prompt"][0]["type"], "text");
        assert_eq!(p["prompt"][0]["text"], "答一下");
        assert_eq!(p["prompt"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn the_handshake_accepts_our_version_and_rejects_another() {
        assert_eq!(check_initialize(&json!({ "protocolVersion": 1 })), Ok(1));
        let e = check_initialize(&json!({ "protocolVersion": 2 })).unwrap_err();
        assert!(e.contains("protocol mismatch"), "{e}");
        assert!(e.contains("v2"), "{e}");
        let e = check_initialize(&json!({ "agentInfo": {} })).unwrap_err();
        assert!(e.contains("no numeric protocolVersion"), "{e}");
    }

    #[test]
    fn a_session_id_is_required_and_must_not_be_empty() {
        assert_eq!(session_id(&json!({ "sessionId": "s1" })), Ok("s1".into()));
        assert!(session_id(&json!({ "sessionId": "" })).is_err());
        assert!(session_id(&json!({})).is_err());
    }

    #[test]
    fn a_committed_message_chunk_becomes_text() {
        let p = json!({
            "sessionId": "s1",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "答案在这里" },
            },
        });
        assert_eq!(
            update_to_event(&p),
            Some(Event::Text {
                text: "答案在这里".into()
            })
        );
    }

    /// Thoughts, tool activity and plans are trace data this protocol does not
    /// promise; consuming them without surfacing them is the contract.
    #[test]
    fn every_other_update_kind_is_consumed_silently() {
        for kind in [
            "agent_thought_chunk",
            "tool_call",
            "tool_call_update",
            "plan",
            "user_message_chunk",
        ] {
            let p = json!({
                "update": { "sessionUpdate": kind, "content": { "type": "text", "text": "x" } },
            });
            assert_eq!(update_to_event(&p), None, "{kind} should not surface");
        }
        // Malformed or empty updates are dropped, not panicked on.
        assert_eq!(update_to_event(&json!({})), None);
        assert_eq!(
            update_to_event(&json!({
                "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "" } }
            })),
            None
        );
        assert_eq!(
            update_to_event(&json!({
                "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "image" } }
            })),
            None
        );
    }

    #[test]
    fn a_permission_decision_names_one_of_the_agents_own_option_ids() {
        let options = vec![
            json!({ "optionId": "allow-once", "name": "Allow once", "kind": "allow_once" }),
            json!({ "optionId": "reject-once", "name": "Reject", "kind": "reject_once" }),
        ];
        assert_eq!(
            permission_result(&options, Decision::Allow),
            json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } })
        );
        assert_eq!(
            permission_result(&options, Decision::Reject),
            json!({ "outcome": { "outcome": "selected", "optionId": "reject-once" } })
        );
    }

    /// Selecting an option of the wrong kind would approve what the policy
    /// refused. When the wanted kind is absent, `cancelled` is the only safe answer.
    #[test]
    fn a_decision_with_no_matching_option_cancels_rather_than_guessing() {
        let only_reject = vec![json!({ "optionId": "no", "kind": "reject_once" })];
        assert_eq!(
            permission_result(&only_reject, Decision::Allow),
            json!({ "outcome": { "outcome": "cancelled" } })
        );
        assert_eq!(
            permission_result(&[], Decision::Reject),
            json!({ "outcome": { "outcome": "cancelled" } })
        );
        // An option missing its id is not usable either.
        let malformed = vec![json!({ "kind": "allow_once" })];
        assert_eq!(
            permission_result(&malformed, Decision::Allow),
            json!({ "outcome": { "outcome": "cancelled" } })
        );
    }

    #[test]
    fn allow_always_counts_as_an_allow_option() {
        let options = vec![json!({ "optionId": "always", "kind": "allow_always" })];
        assert_eq!(
            permission_result(&options, Decision::Allow)["outcome"]["optionId"],
            "always"
        );
    }

    #[test]
    fn a_permission_request_yields_its_opaque_call_id_and_options() {
        let p = json!({
            "sessionId": "s1",
            "toolCall": { "toolCallId": "call-7" },
            "options": [{ "optionId": "allow-once", "kind": "allow_once" }],
        });
        assert_eq!(permission_tool_id(&p), "call-7");
        assert_eq!(permission_options(&p).len(), 1);
        // The server sends nothing else — no name, no path — so a missing id is
        // labelled rather than guessed at.
        assert_eq!(permission_tool_id(&json!({})), "(unnamed tool call)");
        assert!(permission_options(&json!({})).is_empty());
    }

    #[test]
    fn prompt_usage_accepts_acp_camel_case_and_provider_cost() {
        let got = prompt_usage(&json!({
            "stopReason": "end_turn",
            "model": "deepseek-v4-pro",
            "usage": {
                "inputTokens": 11,
                "cachedReadTokens": 22,
                "cachedWriteTokens": 33,
                "outputTokens": 44,
                "thoughtTokens": 40,
                "totalTokens": 110
            },
            "cost": { "amount": 0.0123, "currency": "USD" }
        }))
        .expect("usage");
        assert_eq!(got.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(got.input_tokens, 11);
        assert_eq!(got.cache_read_tokens, 22);
        assert_eq!(got.cache_write_tokens, 33);
        assert_eq!(got.output_tokens, 44);
        assert_eq!(got.reasoning_tokens, 40);
        assert_eq!(got.reported_total_tokens, 110);
        let cost = got.cost.expect("provider cost");
        assert_eq!(cost.amount_usd, 0.0123);
        assert_eq!(cost.kind, CostKind::ProviderReported);
    }

    #[test]
    fn prompt_usage_accepts_harness_aliases_and_rejects_bad_costs() {
        let got = prompt_usage(&json!({
            "usage": {
                "model_id": "deepseek-test",
                "input_tokens": 1,
                "cacheReadTokens": 2,
                "cache_write_tokens": 3,
                "output_tokens": 4,
                "reasoningTokens": 5,
                "total_tokens": 10,
                "cost": { "amountUsd": 0.25 }
            }
        }))
        .expect("usage");
        assert_eq!(got.model.as_deref(), Some("deepseek-test"));
        assert_eq!(got.total_tokens(), 10);
        assert_eq!(got.cost.as_ref().map(|cost| cost.amount_usd), Some(0.25));

        let non_usd = prompt_usage(&json!({
            "usage": { "inputTokens": 1 },
            "cost": { "amount": 1.0, "currency": "CNY" }
        }))
        .expect("tokens still survive");
        assert_eq!(non_usd.cost, None);
        assert_eq!(
            prompt_usage(&json!({ "costUsd": -1, "model": "not-enough" })),
            None
        );
    }

    #[test]
    fn a_prompt_reply_without_usage_stays_none() {
        assert_eq!(prompt_usage(&json!({ "stopReason": "end_turn" })), None);
        assert_eq!(
            prompt_usage(&json!({ "stopReason": "end_turn", "model": "deepseek-v4-pro" })),
            None
        );
    }

    #[test]
    fn only_end_turn_is_a_clean_finish() {
        let usage = Usage {
            input_tokens: 3,
            output_tokens: 2,
            ..Usage::default()
        };
        let r = result_for_stop("end_turn", "  答完了  ", "s1", Some(usage.clone()));
        assert!(!r.is_error);
        assert_eq!(r.result, "答完了");
        assert_eq!(r.session_id.as_deref(), Some("s1"));
        assert_eq!(r.num_turns, None, "ACP reports no turn count");
        assert_eq!(r.usage, Some(usage));
    }

    /// A partial answer must never be recorded as success — including under a
    /// stopReason a newer SDK invents.
    #[test]
    fn every_other_stop_reason_is_a_failure_that_keeps_the_partial_text() {
        for stop in ["max_tokens", "refusal", "max_turn_requests", "cancelled"] {
            let r = result_for_stop(stop, "写到一半", "s1", None);
            assert!(r.is_error, "{stop} must be an error");
            assert!(r.result.contains("写到一半"), "{stop}: {}", r.result);
            assert!(r.result.contains(stop), "{stop}: {}", r.result);
        }
        let unknown = result_for_stop("some_future_reason", "", "s1", None);
        assert!(unknown.is_error);
        assert!(unknown.result.contains("some_future_reason"));
    }

    #[test]
    fn a_silent_clean_finish_still_says_something() {
        let r = result_for_stop("end_turn", "   ", "", None);
        assert!(!r.is_error);
        assert!(r.result.contains("end_turn"), "{}", r.result);
        assert_eq!(r.session_id, None);
    }
}
