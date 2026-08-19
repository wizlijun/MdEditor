//! JSON-RPC 帧与工具 schema。**无 IO、无状态**,外壳与 server 共用。

use serde_json::{json, Value};

/// 探针实测 Cowork 发的版本。客户端报什么就回什么,这只是没得回时的默认值。
pub const DEFAULT_PROTOCOL_VERSION: &str = "2025-11-25";

pub fn initialize_result(client_version: Option<&str>) -> Value {
    json!({
        "protocolVersion": client_version.unwrap_or(DEFAULT_PROTOCOL_VERSION),
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "notemd", "version": env!("CARGO_PKG_VERSION") },
    })
}

/// 两个工具,只读。
///
/// `search` 只收 `query`/`limit`/`context`:过滤语法写在 query 字符串里,
/// 与 CLI 逐字相同 —— 一个语法,一个解析器。把 tag/type/path/… 拆成独立参数
/// 等于把同一套过滤语义实现两遍。
pub fn tool_definitions() -> Value {
    json!({ "tools": [
        {
            "name": "search",
            "description": concat!(
                "Full-text search over the user's note.md vault, with Chinese ",
                "segmentation, relevance ranking, and origin weighting (human-written ",
                "notes rank above machine summaries).\n\n",
                "Filters go inside `query`, same syntax as the `notemd search` CLI:\n",
                "  tag:x  type:x  path:x  ext:x  after:YYYY-MM-DD  before:YYYY-MM-DD\n",
                "  page:[[X]]  origin:human|derived|source\n\n",
                "Each hit carries `origin` (which tier the file falls in) and ",
                "`provenance.agent_by` (set when a file was written by an agent — follow ",
                "its `sources` rather than citing it as primary). Paths are vault-relative; ",
                "resolve them against your own mount only when `mount.status` is \"matched\"."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms, optionally with the filters above." },
                    "limit": { "type": "integer", "description": "Max hits. 0 = no cap.", "default": 20 },
                    "context": { "type": "integer", "description": "Lines of surrounding context per hit.", "default": 0 }
                },
                "required": ["query"]
            }
        },
        {
            "name": "vault_info",
            "description": concat!(
                "Identity and freshness of the vault this server is serving. Call once per ",
                "session before relying on `search`: compare `vault_id` against the ",
                ".notemd/vault-id in your own mounted folder. Zero side effects."
            ),
            "inputSchema": { "type": "object", "properties": {} }
        }
    ]})
}

pub fn error(id: &Value, code: i64, msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } })
}

/// 降级信号:`result.isError`,**不是**协议层 error。模型据此退回 grep,
/// 而不是把整轮工具调用判死。
pub fn tool_error(id: &Value, msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": {
        "isError": true,
        "content": [{ "type": "text", "text": msg }]
    }})
}

pub fn tool_ok(id: &Value, payload: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": {
        "content": [{ "type": "text", "text": serde_json::to_string(payload).unwrap_or_default() }]
    }})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_has_exactly_two_readonly_tools() {
        let v = tool_definitions();
        let tools = v["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["search", "vault_info"]);
        // 只读面:工具名里不得出现任何写动作词。
        for n in &names {
            for bad in ["write", "create", "delete", "update", "edit", "move"] {
                assert!(!n.contains(bad), "只读面被破坏: {n}");
            }
        }
    }

    #[test]
    fn search_schema_exposes_only_query_limit_context() {
        let v = tool_definitions();
        let search = &v["tools"][0];
        let mut props: Vec<&str> =
            search["inputSchema"]["properties"].as_object().unwrap()
                .keys().map(|s| s.as_str()).collect();
        props.sort();
        assert_eq!(props, vec!["context", "limit", "query"]);
        assert_eq!(search["inputSchema"]["required"], serde_json::json!(["query"]));
    }

    #[test]
    fn initialize_echoes_client_protocol_version() {
        let v = initialize_result(Some("2025-06-18"));
        assert_eq!(v["protocolVersion"], "2025-06-18");
        let v = initialize_result(None);
        assert_eq!(v["protocolVersion"], DEFAULT_PROTOCOL_VERSION);
    }

    #[test]
    fn tool_error_is_result_not_protocol_error() {
        // 降级信号必须走 result.isError,不能走协议层 error —— 否则模型
        // 会把整轮工具调用判死,而不是退回 grep(spec §1.2)。
        let v = tool_error(&serde_json::json!(1), "note.md 未运行");
        assert!(v.get("error").is_none());
        assert_eq!(v["result"]["isError"], true);
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("note.md"));
    }
}
