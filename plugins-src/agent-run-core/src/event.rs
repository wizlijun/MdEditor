//! The wire contract every agent plugin's backend shares with its window.
//!
//! Two harnesses feed it from very different sources — claude-agent parses
//! `--output-format stream-json`, deepseek-agent maps ACP `session/update`
//! notifications — but the window sees one shape, so `events.ts` and its reducer
//! are written once.
//!
//! **Variants are added, never changed.** An older window ignores a `kind` it
//! does not know (the reducer's fall-through returns the view unchanged), so a
//! new variant cannot break a plugin front-end that shipped before it.
//!
//! Note what is deliberately NOT here: a `ToolUse` from ACP. `dsh-acp` emits
//! only committed assistant messages — "Raw chunks, reasoning, tools, plans,
//! titles, and retry markers … stay off the automation wire"
//! (`packages/acp/acp/src/index.ts`). deepseek runs therefore produce `Text` and
//! `Result` only, and that is a property of the protocol, not a gap to fill in.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    System {
        subtype: String,
    },
    Text {
        text: String,
    },
    ToolUse {
        name: String,
        brief: String,
    },
    /// A permission request and how it was answered — in the stream so the run
    /// log shows WHY a tool call did or did not happen. Only ACP produces these;
    /// headless claude pre-approves everything in `settings.local.json` instead.
    Permission {
        tool: String,
        decision: String,
    },
    Result(RunResult),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResult {
    pub is_error: bool,
    pub result: String,
    pub session_id: Option<String>,
    pub num_turns: Option<u64>,
}

/// Every step an engine emits. The window path turns these into `host.ui.post`;
/// the detached runner only cares about the terminal one.
#[derive(Debug)]
pub enum Step {
    Event(Event),
    Done(crate::record::RunRecord),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window matches on `kind`; renaming one silently blanks a run log.
    #[test]
    fn every_variant_serializes_under_the_kind_the_window_matches_on() {
        let cases = [
            (Event::Text { text: "hi".into() }, "text"),
            (
                Event::ToolUse {
                    name: "Read".into(),
                    brief: "a.md".into(),
                },
                "tool_use",
            ),
            (
                Event::System {
                    subtype: "init".into(),
                },
                "system",
            ),
            (
                Event::Permission {
                    tool: "bash".into(),
                    decision: "allowed".into(),
                },
                "permission",
            ),
        ];
        for (ev, want) in cases {
            let v = serde_json::to_value(&ev).unwrap();
            assert_eq!(v["kind"], want, "serialized as {v}");
        }
    }

    #[test]
    fn a_result_carries_its_fields_beside_the_kind() {
        let v = serde_json::to_value(Event::Result(RunResult {
            is_error: false,
            result: "done".into(),
            session_id: Some("s1".into()),
            num_turns: Some(3),
        }))
        .unwrap();
        assert_eq!(v["kind"], "result");
        assert_eq!(v["result"], "done");
        assert_eq!(v["session_id"], "s1");
        assert_eq!(v["num_turns"], 3);
    }

    #[test]
    fn events_round_trip_through_json() {
        let ev = Event::Permission {
            tool: "call-7".into(),
            decision: "rejected".into(),
        };
        let back: Event = serde_json::from_str(&serde_json::to_string(&ev).unwrap()).unwrap();
        assert_eq!(back, ev);
    }
}
