//! Prompt composition shared by every harness. The order is fixed and
//! documented for template authors: ①the template's prompt ②this run's input
//! ③the current-document context. Harness-specific paragraphs (which toolbelt
//! exists, which argv to build) stay in that harness's own crate.
use crate::scope::Scope;
use std::path::Path;

/// The tab context captured when the window was opened.
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

/// Tell the run where the document it is about actually lives.
///
/// A mirrored document's original sits outside the vault, in the directory that
/// gives it meaning. Generated in code, not in the task template: templates are
/// per-vault state, so a template-only change would never reach a vault set up
/// by an older build.
pub fn with_source_context(prompt: &str, vault: &Path, scope: Option<&Scope>) -> String {
    let para = match scope {
        // A scoped run knows the one path; only worth saying when it is NOT the
        // vault copy the note sits beside.
        Some(s) if !s.source.starts_with(vault) => Some(format!(
            "## 源文上下文\n\
             本次这篇笔记对应的源文档在 vault 之外的原目录:`{}`。\n\
             读它(而不是 vault 里的镜像副本)作为上下文;同目录下的相关文件也可读。\n\
             **绝不修改源文档**,只写笔记本身。",
            s.source.to_string_lossy()
        )),
        Some(_) => None,
        // A sweep can't name one path: each note's front-matter carries its own.
        None => Some(
            "## 源文上下文\n\
             部分文档是 vault 外文件的镜像:笔记 front-matter 的 `sources:` 给出原件的绝对路径。\n\
             有 `sources:` 时优先读原件(它才是文档真正的所在,同目录下的相关文件也可读),\n\
             读不到再回退到笔记同目录的镜像副本。**绝不修改源文档**。"
                .to_string(),
        ),
    };
    match para {
        Some(p) if !prompt.trim().is_empty() => format!("{}\n\n{p}", prompt.trim()),
        Some(p) => p,
        None => prompt.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn joins_three_parts_in_fixed_order() {
        let ctx = TabContext {
            path: "/v/a.md".into(),
            selection: "sel".into(),
        };
        let got = compose("TASK", "USER", Some(&ctx));
        assert_eq!(
            got,
            "TASK\n\nUSER\n\n## 当前文档\n路径:/v/a.md\n选中内容:\nsel"
        );
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

    fn scope_with_source(source: &str) -> Scope {
        Scope {
            note: PathBuf::from("/v/Sync/a.note.md"),
            source: PathBuf::from(source),
            source_dir: PathBuf::from(source)
                .parent()
                .map(Path::to_path_buf)
                .unwrap(),
        }
    }

    #[test]
    fn a_scoped_run_is_told_where_the_original_lives() {
        let got = with_source_context(
            "TASK",
            Path::new("/v"),
            Some(&scope_with_source("/proj/docs/a.md")),
        );
        assert!(got.starts_with("TASK\n\n"), "{got}");
        assert!(got.contains("/proj/docs/a.md"), "{got}");
        assert!(got.contains("绝不修改源文档"), "{got}");
    }

    #[test]
    fn a_source_that_is_the_vault_copy_adds_nothing() {
        let got = with_source_context(
            "TASK",
            Path::new("/v"),
            Some(&scope_with_source("/v/Sync/a.md")),
        );
        assert_eq!(got, "TASK");
    }

    #[test]
    fn a_sweep_is_pointed_at_the_notes_own_sources_field() {
        let got = with_source_context("TASK", Path::new("/v"), None);
        assert!(got.contains("`sources:`"), "{got}");
        assert!(got.contains("绝不修改源文档"), "{got}");
    }
}
