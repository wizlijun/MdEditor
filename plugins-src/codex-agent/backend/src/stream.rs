//! Stateful projection from `codex exec --json` JSONL to note.md's shared
//! agent event contract.
//!
//! Codex emits one terminal event (`turn.completed`, `turn.failed`, or
//! unrecoverable `error`) and many typed thread items. Unknown additions are
//! ignored so a newer Codex CLI cannot blank the run window.
pub use agent_run_core::event::{Event, RunResult};

#[derive(Debug, Default)]
pub struct StreamState {
    thread_id: Option<String>,
    turns: u64,
    terminal_text: String,
    emitted_message: bool,
    terminal_result: Option<RunResult>,
}

impl StreamState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal_result.is_some()
    }

    pub fn result(&self) -> Option<RunResult> {
        self.terminal_result.clone()
    }

    /// Engine-facing interface: accept one wire line and return zero or one
    /// shared events. A vector keeps the contract open for a future Codex frame
    /// that legitimately projects to more than one display event.
    pub fn accept(&mut self, line: &str) -> Vec<Event> {
        self.parse_line(line).into_iter().collect()
    }

    /// Parse one JSONL frame. One Codex frame produces at most one shared
    /// event; malformed lines and forward-compatible unknown variants produce
    /// none.
    pub fn parse_line(&mut self, line: &str) -> Option<Event> {
        let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
        let kind = v.get("type")?.as_str()?;
        match kind {
            "thread.started" => {
                self.thread_id = v
                    .get("thread_id")
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                Some(Event::System {
                    subtype: "init".into(),
                })
            }
            "turn.started" => {
                self.turns += 1;
                Some(Event::System {
                    subtype: "turn_started".into(),
                })
            }
            "item.started" => tool_event(v.get("item")?),
            "item.updated" => None,
            "item.completed" => self.completed_item(v.get("item")?),
            "turn.completed" if !self.is_terminal() => {
                let result = self.make_result(false, self.terminal_text.clone());
                self.terminal_result = Some(result.clone());
                Some(Event::Result(result))
            }
            "turn.failed" if !self.is_terminal() => {
                let message = v
                    .pointer("/error/message")
                    .and_then(|x| x.as_str())
                    .unwrap_or("Codex turn failed")
                    .to_string();
                let result = self.make_result(true, message);
                self.terminal_result = Some(result.clone());
                Some(Event::Result(result))
            }
            "error" if !self.is_terminal() => {
                let message = v
                    .get("message")
                    .and_then(|x| x.as_str())
                    .unwrap_or("Codex event stream failed")
                    .to_string();
                let result = self.make_result(true, message);
                self.terminal_result = Some(result.clone());
                Some(Event::Result(result))
            }
            _ => None,
        }
    }

    fn completed_item(&mut self, item: &serde_json::Value) -> Option<Event> {
        match item.get("type")?.as_str()? {
            "agent_message" => {
                let text = item.get("text")?.as_str()?.to_string();
                if text.is_empty() {
                    return None;
                }
                if !self.terminal_text.is_empty() {
                    self.terminal_text.push_str("\n\n");
                }
                self.terminal_text.push_str(&text);
                // Separate complete messages: the front-end intentionally
                // merges adjacent Text events because Claude streams fragments.
                let shown = if std::mem::replace(&mut self.emitted_message, true) {
                    format!("\n\n{text}")
                } else {
                    text
                };
                Some(Event::Text { text: shown })
            }
            // File changes have no started event in the exec JSONL contract.
            "file_change" => file_change_event(item),
            _ => None,
        }
    }

    fn make_result(&self, is_error: bool, result: String) -> RunResult {
        RunResult {
            is_error,
            result,
            session_id: self.thread_id.clone(),
            num_turns: Some(self.turns.max(1)),
        }
    }
}

fn cap(text: &str) -> String {
    text.chars().take(120).collect()
}

fn tool_event(item: &serde_json::Value) -> Option<Event> {
    let (name, brief) = match item.get("type")?.as_str()? {
        "command_execution" => (
            "Command".to_string(),
            cap(item.get("command").and_then(|x| x.as_str()).unwrap_or("")),
        ),
        "mcp_tool_call" => {
            let server = item.get("server").and_then(|x| x.as_str()).unwrap_or("mcp");
            let tool = item.get("tool").and_then(|x| x.as_str()).unwrap_or("tool");
            let brief = item
                .get("arguments")
                .map(|x| cap(&x.to_string()))
                .unwrap_or_default();
            (format!("MCP {server}/{tool}"), brief)
        }
        "collab_tool_call" => (
            "Collab".to_string(),
            cap(item.get("tool").and_then(|x| x.as_str()).unwrap_or("agent")),
        ),
        "web_search" => (
            "WebSearch".to_string(),
            cap(item.get("query").and_then(|x| x.as_str()).unwrap_or("")),
        ),
        _ => return None,
    };
    Some(Event::ToolUse { name, brief })
}

fn file_change_event(item: &serde_json::Value) -> Option<Event> {
    let paths: Vec<&str> = item
        .get("changes")?
        .as_array()?
        .iter()
        .filter_map(|c| c.get("path").and_then(|p| p.as_str()))
        .collect();
    Some(Event::ToolUse {
        name: "FileChange".into(),
        brief: cap(&paths.join(", ")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_and_turn_lifecycle_become_system_events() {
        let mut p = StreamState::new();
        assert_eq!(
            p.parse_line(r#"{"type":"thread.started","thread_id":"thr-1"}"#),
            Some(Event::System {
                subtype: "init".into()
            })
        );
        assert_eq!(p.thread_id(), Some("thr-1"));
        assert_eq!(
            p.parse_line(r#"{"type":"turn.started"}"#),
            Some(Event::System {
                subtype: "turn_started".into()
            })
        );
    }

    #[test]
    fn completed_agent_message_is_text_and_the_terminal_result() {
        let mut p = StreamState::new();
        p.parse_line(r#"{"type":"thread.started","thread_id":"thr-1"}"#);
        p.parse_line(r#"{"type":"turn.started"}"#);
        assert_eq!(
            p.parse_line(
                r#"{"type":"item.completed","item":{"id":"i","type":"agent_message","text":"done"}}"#
            ),
            Some(Event::Text {
                text: "done".into()
            })
        );
        assert_eq!(
            p.parse_line(r#"{"type":"turn.completed","usage":{"input_tokens":1}}"#),
            Some(Event::Result(RunResult {
                is_error: false,
                result: "done".into(),
                session_id: Some("thr-1".into()),
                num_turns: Some(1),
            }))
        );
        assert!(p.is_terminal());
        assert_eq!(p.result().unwrap().result, "done");
    }

    #[test]
    fn tool_items_map_to_short_shared_rows() {
        let mut p = StreamState::new();
        let cases = [
            (
                r#"{"type":"item.started","item":{"id":"1","type":"command_execution","command":"rg TODO src","status":"in_progress"}}"#,
                "Command",
                "rg TODO src",
            ),
            (
                r#"{"type":"item.started","item":{"id":"2","type":"mcp_tool_call","server":"notemd","tool":"vault_search","arguments":{"q":"x"},"status":"in_progress"}}"#,
                "MCP notemd/vault_search",
                "{\"q\":\"x\"}",
            ),
            (
                r#"{"type":"item.started","item":{"id":"3","type":"web_search","query":"OpenAI docs"}}"#,
                "WebSearch",
                "OpenAI docs",
            ),
        ];
        for (line, name, brief) in cases {
            assert_eq!(
                p.parse_line(line),
                Some(Event::ToolUse {
                    name: name.into(),
                    brief: brief.into(),
                })
            );
        }
    }

    #[test]
    fn completed_file_change_lists_paths_but_completed_commands_do_not_duplicate() {
        let mut p = StreamState::new();
        assert_eq!(
            p.parse_line(
                r#"{"type":"item.completed","item":{"id":"f","type":"file_change","changes":[{"path":"a.md","kind":"update"},{"path":"b.md","kind":"add"}],"status":"completed"}}"#
            ),
            Some(Event::ToolUse {
                name: "FileChange".into(),
                brief: "a.md, b.md".into(),
            })
        );
        assert_eq!(
            p.parse_line(
                r#"{"type":"item.completed","item":{"id":"c","type":"command_execution","command":"pwd","status":"completed"}}"#
            ),
            None
        );
    }

    #[test]
    fn failed_turn_and_stream_error_are_terminal_failures() {
        let mut failed = StreamState::new();
        failed.parse_line(r#"{"type":"thread.started","thread_id":"t"}"#);
        assert_eq!(
            failed.parse_line(r#"{"type":"turn.failed","error":{"message":"401 Unauthorized"}}"#),
            Some(Event::Result(RunResult {
                is_error: true,
                result: "401 Unauthorized".into(),
                session_id: Some("t".into()),
                num_turns: Some(1),
            }))
        );
        // Some Codex versions emit `error` immediately before turn.failed.
        // The first terminal frame is authoritative; never emit two results.
        assert_eq!(
            failed.parse_line(r#"{"type":"error","message":"again"}"#),
            None
        );

        let mut error = StreamState::new();
        assert!(matches!(
            error.parse_line(r#"{"type":"error","message":"rate limit"}"#),
            Some(Event::Result(RunResult { is_error: true, result, .. })) if result == "rate limit"
        ));
    }

    #[test]
    fn unknown_noise_and_nonfatal_items_are_ignored() {
        let mut p = StreamState::new();
        for line in [
            "not json",
            r#"{"type":"future.event","new":true}"#,
            r#"{"type":"item.updated","item":{"type":"command_execution"}}"#,
            r#"{"type":"item.completed","item":{"id":"r","type":"reasoning","text":"private"}}"#,
            r#"{"type":"item.completed","item":{"id":"e","type":"error","message":"retrying"}}"#,
        ] {
            assert_eq!(p.parse_line(line), None, "{line}");
        }
    }

    #[test]
    fn engine_facing_accept_returns_zero_or_one_events_and_keeps_result() {
        let mut p = StreamState::new();
        assert!(p.accept("diagnostic noise").is_empty());
        assert_eq!(
            p.accept(r#"{"type":"turn.completed"}"#),
            vec![Event::Result(RunResult {
                is_error: false,
                result: String::new(),
                session_id: None,
                num_turns: Some(1),
            })]
        );
        assert_eq!(p.result().unwrap().num_turns, Some(1));
        assert!(p.is_terminal());
    }

    #[test]
    fn separate_agent_messages_do_not_run_together_in_the_shared_ui() {
        let mut p = StreamState::new();
        let a =
            r#"{"type":"item.completed","item":{"id":"a","type":"agent_message","text":"first"}}"#;
        let b =
            r#"{"type":"item.completed","item":{"id":"b","type":"agent_message","text":"second"}}"#;
        assert!(matches!(p.parse_line(a), Some(Event::Text { text }) if text == "first"));
        assert!(matches!(p.parse_line(b), Some(Event::Text { text }) if text == "\n\nsecond"));
        assert!(matches!(
            p.parse_line(r#"{"type":"turn.completed"}"#),
            Some(Event::Result(RunResult { result, .. })) if result == "first\n\nsecond"
        ));
    }
}
