//! claude-specific prompt trimmings: which toolbelt this machine has, and the
//! argv that carries the composed prompt into `claude -p`. The composition
//! itself (three-part order, source-document context) is shared and lives in
//! `agent-run-core`.
pub use agent_run_core::prompt::{compose, with_source_context, TabContext};

use crate::task::TaskDef;
use std::path::Path;

/// State the toolbelt this run actually has.
///
/// The templates tell the model to answer from the document in front of it, and
/// a model told that will not go looking for anything else — so a granted tool
/// it was never told about stays unused. Which MCP servers exist is a property
/// of the machine, so like `with_source_context` this is generated per run
/// rather than written into a template that is seeded once and never updated.
pub fn with_toolbelt(prompt: &str, servers: &[String]) -> String {
    let mut para = String::from(
        "## 可用工具\n\
         需要核实外部事实、时效性内容或文中提到的外部资料时,可用 `WebSearch` / `WebFetch` 检索,\n\
         也可用 `Task` 派子 agent、`Skill` 调技能。用了外部来源就把 URL 标在它支撑的那句话旁边。\n\
         这些是**补充**:手上的文档仍是答案的地基,不要用检索结果代替通读原文。",
    );
    if !servers.is_empty() {
        let list = servers
            .iter()
            .map(|s| format!("`mcp__{s}`"))
            .collect::<Vec<_>>()
            .join("、");
        para.push_str(&format!(
            "\n本机还接入了这些 MCP 服务:{list} —— 对上号时优先用它们。"
        ));
    }
    match prompt.trim() {
        "" => para,
        p => format!("{p}\n\n{para}"),
    }
}

/// claude's arguments (the executable itself excluded).
///
/// Deliberately no `--bare`: that skips discovery of CLAUDE.md, skills and
/// .mcp.json, which is the entire point of running inside a task template.
#[cfg(test)]
pub fn build_argv(task: &TaskDef, prompt: &str) -> Vec<String> {
    build_argv_with_settings(task, prompt, None)
}

pub fn build_argv_with_settings(
    task: &TaskDef,
    prompt: &str,
    settings: Option<&Path>,
) -> Vec<String> {
    let mut v = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
    ];
    if let Some(path) = settings {
        // Never load the shared task-local settings.local.json: concurrent
        // scoped runs each have a private dynamic policy passed via --settings.
        v.extend([
            "--setting-sources".into(),
            "user,project".into(),
            "--settings".into(),
            path.to_string_lossy().into_owned(),
        ]);
    }
    if let Some(t) = task.max_turns {
        v.push("--max-turns".into());
        v.push(t.to_string());
    }
    if let Some(m) = task.model.as_deref().filter(|s| !s.is_empty()) {
        v.push("--model".into());
        v.push(m.to_string());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> TaskDef {
        TaskDef {
            id: "t".into(),
            name: "T".into(),
            description: String::new(),
            prompt: "P".into(),
            max_turns: Some(50),
            timeout_seconds: 1800,
            model: None,
            precheck: None,
            okf_type: None,
            directive: Vec::new(),
            source_plugin: None,
        }
    }

    #[test]
    fn a_run_is_told_which_mcp_servers_it_can_reach() {
        // Granting a tool is not enough: a prompt that says "answer from the
        // source text" gets a model that never reaches for anything else.
        let got = with_toolbelt("TASK", &["exa".to_string(), "hemory-vault".to_string()]);
        assert!(got.starts_with("TASK\n\n"), "{got}");
        assert!(got.contains("mcp__exa"), "{got}");
        assert!(got.contains("mcp__hemory-vault"), "{got}");
        assert!(got.contains("WebSearch"), "{got}");
    }

    #[test]
    fn a_machine_without_mcp_servers_is_still_told_about_the_web() {
        let got = with_toolbelt("TASK", &[]);
        assert!(got.contains("WebSearch"), "{got}");
        assert!(!got.contains("mcp__"), "no empty MCP list: {got}");
    }

    #[test]
    fn argv_is_stream_json_verbose_and_never_bare() {
        let got = build_argv(&task(), "hi");
        assert_eq!(
            got,
            vec![
                "-p",
                "hi",
                "--output-format",
                "stream-json",
                "--verbose",
                "--max-turns",
                "50"
            ]
        );
        assert!(!got.iter().any(|a| a == "--bare"));
    }

    #[test]
    fn argv_passes_model_through_when_set() {
        let mut t = task();
        t.model = Some("claude-opus-5".into());
        let got = build_argv(&t, "hi");
        assert!(got.windows(2).any(|w| w == ["--model", "claude-opus-5"]));
    }

    #[test]
    fn argv_uses_private_settings_and_excludes_the_shared_local_source() {
        let got = build_argv_with_settings(&task(), "hi", Some(Path::new("/runs/r1.json")));
        assert!(got.windows(2).any(|w| w == ["--settings", "/runs/r1.json"]));
        assert!(got
            .windows(2)
            .any(|w| w == ["--setting-sources", "user,project"]));
    }

    #[test]
    fn argv_omits_max_turns_when_the_task_leaves_it_unset() {
        let mut t = task();
        t.max_turns = None;
        assert!(!build_argv(&t, "hi").iter().any(|a| a == "--max-turns"));
    }
}
