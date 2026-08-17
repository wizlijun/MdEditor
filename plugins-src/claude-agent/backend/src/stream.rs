//! Line-by-line parsing of `--output-format stream-json --verbose`. Only four
//! kinds of event matter to us; everything else (including non-JSON noise) is
//! dropped rather than surfaced.
//!
//! The `Event` shape itself is the wire contract shared with the window and with
//! deepseek-agent, so it lives in `agent-run-core`; only the claude-specific
//! PARSING is here.
pub use agent_run_core::event::{Event, RunResult};

/// Parse one line. `None` means the line produces no event.
pub fn parse_line(line: &str) -> Option<Event> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    match v.get("type")?.as_str()? {
        "system" => Some(Event::System {
            subtype: v
                .get("subtype")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "assistant" => {
            let blocks = v.pointer("/message/content")?.as_array()?;
            for b in blocks {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("tool_use") => {
                        let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                        return Some(Event::ToolUse {
                            name: name.to_string(),
                            brief: tool_brief(b),
                        });
                    }
                    Some("text") => {
                        let t = b.get("text").and_then(|t| t.as_str()).unwrap_or("");
                        if !t.is_empty() {
                            return Some(Event::Text { text: t.to_string() });
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        "result" => Some(Event::Result(RunResult {
            is_error: v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false),
            result: v
                .get("result")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string(),
            session_id: v
                .get("session_id")
                .and_then(|s| s.as_str())
                .map(str::to_string),
            num_turns: v.get("num_turns").and_then(|n| n.as_u64()),
        })),
        _ => None,
    }
}

/// One-line summary of a tool call: file path first, then command, then pattern.
fn tool_brief(block: &serde_json::Value) -> String {
    let i = match block.get("input") {
        Some(i) => i,
        None => return String::new(),
    };
    for k in ["file_path", "path", "command", "pattern", "url"] {
        if let Some(s) = i.get(k).and_then(|v| v.as_str()) {
            return s.chars().take(120).collect();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_result_line() {
        let l = r#"{"type":"result","subtype":"success","result":"done","session_id":"s1","num_turns":12,"is_error":false}"#;
        assert_eq!(
            parse_line(l),
            Some(Event::Result(RunResult {
                is_error: false,
                result: "done".into(),
                session_id: Some("s1".into()),
                num_turns: Some(12),
            }))
        );
    }

    #[test]
    fn parses_a_tool_use_with_a_file_brief() {
        let l = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/a.rs"}}]}}"#;
        assert_eq!(
            parse_line(l),
            Some(Event::ToolUse {
                name: "Read".into(),
                brief: "src/a.rs".into()
            })
        );
    }

    #[test]
    fn parses_assistant_text() {
        let l = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}"#;
        assert_eq!(parse_line(l), Some(Event::Text { text: "hello".into() }));
    }

    #[test]
    fn drops_noise_lines() {
        assert_eq!(parse_line("not json at all"), None);
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line(r#"{"type":"user","message":{}}"#), None);
        assert_eq!(parse_line(r#"{"no_type":1}"#), None);
    }

    #[test]
    fn treats_is_error_true_as_a_failed_result() {
        let l = r#"{"type":"result","subtype":"error_max_turns","result":"hit limit","is_error":true}"#;
        match parse_line(l) {
            Some(Event::Result(r)) => {
                assert!(r.is_error);
                assert_eq!(r.result, "hit limit");
            }
            other => panic!("expected a result event, got {other:?}"),
        }
    }

    #[test]
    fn truncates_an_absurdly_long_tool_brief() {
        let long = "x".repeat(500);
        let l = format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","name":"Bash","input":{{"command":"{long}"}}}}]}}}}"#
        );
        match parse_line(&l) {
            Some(Event::ToolUse { brief, .. }) => assert_eq!(brief.chars().count(), 120),
            other => panic!("expected a tool_use event, got {other:?}"),
        }
    }
}
