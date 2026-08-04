//! Three-part prompt composition and claude argv assembly. The order is fixed
//! and documented for template authors: ①the template's prompt ②this run's
//! input ③the current-document context.
use crate::task::TaskDef;

/// The tab context captured when the window was opened (two fields off the v1
/// `context.tab`).
#[derive(Debug, Clone, PartialEq)]
pub struct TabContext {
    pub path: String,
    pub selection: String,
}

pub fn compose(task_prompt: &str, user_prompt: &str, ctx: Option<&TabContext>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !task_prompt.trim().is_empty() {
        parts.push(task_prompt.trim().to_string());
    }
    if !user_prompt.trim().is_empty() {
        parts.push(user_prompt.trim().to_string());
    }
    if let Some(c) = ctx {
        let mut b = format!("## 当前文档\n路径:{}", c.path);
        if !c.selection.trim().is_empty() {
            b.push_str(&format!("\n选中内容:\n{}", c.selection.trim()));
        }
        parts.push(b);
    }
    parts.join("\n\n")
}

/// claude's arguments (the executable itself excluded).
///
/// Deliberately no `--bare`: that skips discovery of CLAUDE.md, skills and
/// .mcp.json, which is the entire point of running inside a task template.
pub fn build_argv(task: &TaskDef, prompt: &str) -> Vec<String> {
    let mut v = vec![
        "-p".to_string(),
        prompt.to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
    ];
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
        }
    }

    #[test]
    fn joins_three_parts_in_fixed_order() {
        let ctx = TabContext {
            path: "/v/a.md".into(),
            selection: "sel".into(),
        };
        let got = compose("TASK", "USER", Some(&ctx));
        assert_eq!(got, "TASK\n\nUSER\n\n## 当前文档\n路径:/v/a.md\n选中内容:\nsel");
    }

    #[test]
    fn omits_empty_parts() {
        assert_eq!(compose("TASK", "   ", None), "TASK");
    }

    #[test]
    fn context_without_selection_keeps_only_the_path() {
        let ctx = TabContext {
            path: "/v/a.md".into(),
            selection: "  ".into(),
        };
        assert_eq!(compose("", "", Some(&ctx)), "## 当前文档\n路径:/v/a.md");
    }

    #[test]
    fn argv_is_stream_json_verbose_and_never_bare() {
        let got = build_argv(&task(), "hi");
        assert_eq!(
            got,
            vec!["-p", "hi", "--output-format", "stream-json", "--verbose", "--max-turns", "50"]
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
    fn argv_omits_max_turns_when_the_task_leaves_it_unset() {
        let mut t = task();
        t.max_turns = None;
        assert!(!build_argv(&t, "hi").iter().any(|a| a == "--max-turns"));
    }
}
