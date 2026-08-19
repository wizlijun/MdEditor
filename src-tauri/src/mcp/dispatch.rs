//! 方法分发。`initialize` 与 `tools/list` **不需要 env**(主程序可以没开);
//! 只有 `tools/call` 需要。

use serde_json::{json, Value};

use crate::mcp::protocol;
use crate::mcp::tools::{self, ToolEnv};

pub fn handle(env: Option<&ToolEnv>, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone(); // 无 id = 通知,不回
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    Some(match method {
        "initialize" => {
            let v = params.get("protocolVersion").and_then(|v| v.as_str());
            json!({ "jsonrpc": "2.0", "id": id, "result": protocol::initialize_result(v) })
        }
        "tools/list" => json!({ "jsonrpc": "2.0", "id": id, "result": protocol::tool_definitions() }),
        "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
        "resources/list" => json!({ "jsonrpc": "2.0", "id": id, "result": { "resources": [] } }),
        "prompts/list" => json!({ "jsonrpc": "2.0", "id": id, "result": { "prompts": [] } }),
        "tools/call" => {
            let Some(env) = env else {
                return Some(protocol::tool_error(
                    &id,
                    "note.md 未运行。启动 note.md 后即可检索;在此之前请用 grep/rg 兜底。",
                ));
            };
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let out = match name {
                "search" => tools::search(env, &args),
                "vault_info" => tools::vault_info(env),
                other => Err(format!("未知工具 '{other}';本 server 只提供 search 与 vault_info")),
            };
            match out {
                Ok(v) => protocol::tool_ok(&id, &v),
                Err(e) => protocol::tool_error(&id, &e),
            }
        }
        other => protocol::error(&id, -32601, &format!("no such method: {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(id: i64, method: &str) -> serde_json::Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method })
    }

    /// 通知(无 id)不产生响应。
    #[test]
    fn notification_yields_no_response() {
        let m = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(None, &m).is_none());
    }

    /// **note.md 未运行时 `tools/list` 仍必须给出完整工具定义**(spec §1.2)。
    /// MCP 客户端在会话启动那一刻枚举工具;此时返回空列表,agent 整场会话
    /// 都不会再问第二次。
    #[test]
    fn tools_list_works_without_env() {
        let r = handle(None, &req(1, "tools/list")).unwrap();
        assert_eq!(r["result"]["tools"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn initialize_works_without_env() {
        let m = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                        "params": { "protocolVersion": "2025-11-25" } });
        let r = handle(None, &m).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-11-25");
    }

    /// 没有 env(主程序不在)时调工具 ⇒ isError,不是协议 error。
    #[test]
    fn tools_call_without_env_is_tool_error() {
        let m = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                        "params": { "name": "search", "arguments": { "query": "x" } } });
        let r = handle(None, &m).unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r.get("error").is_none());
    }

    #[test]
    fn unknown_method_is_protocol_error() {
        let r = handle(None, &req(3, "nope/nope")).unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }

    #[test]
    fn unknown_tool_name_is_tool_error() {
        let m = json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                        "params": { "name": "delete_everything", "arguments": {} } });
        let r = handle(None, &m).unwrap();
        assert_eq!(r["result"]["isError"], true);
    }

    /// 端到端:`tools::search` → `protocol::tool_ok`(把 payload 字符串化进
    /// `content[0].text`)这条粘合线之前没有任何测试用 `Some(env)` 真的走过
    /// 一遍——之前每条 dispatch 测试都传 `None`。
    #[test]
    fn tools_call_search_with_env_reaches_tool_ok_payload() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("notes")).unwrap();
        std::fs::write(d.path().join("notes/a.md"), "# T\n\nzebraquux 在这里\n").unwrap();
        let opts = crate::cli::search::scan_options_for(d.path());
        let mut idx =
            searchidx::SearchIndex::open(d.path(), &opts.source_globs.stamp()).unwrap();
        idx.ensure_built(&opts).unwrap();
        let env = ToolEnv {
            vault_root: d.path().to_path_buf(),
            index: std::sync::Arc::new(std::sync::Mutex::new(Some(idx))),
            roots: None,
            open_phase: None,
        };

        let m = json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call",
                        "params": { "name": "search", "arguments": { "query": "zebraquux" } } });
        let r = handle(Some(&env), &m).unwrap();
        assert!(r.get("error").is_none());
        assert!(
            r["result"]["isError"].as_bool().unwrap_or(false) == false,
            "成功响应不该带 isError:true"
        );

        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        let payload: serde_json::Value =
            serde_json::from_str(text).expect("content[0].text 必须是可解析 JSON");
        assert_eq!(payload["vault_id"].as_str().unwrap().len(), 36);
        assert!(payload["hits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["path"] == "notes/a.md"));
    }
}
