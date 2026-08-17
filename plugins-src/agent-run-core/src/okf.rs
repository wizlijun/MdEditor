//! OKF v0.2 的最后一道闸:agent 写进 vault `answers/` 的长答案必须是合规概念文档。
//!
//! 提示词里已经要求它自己写 frontmatter(templates/answer-note-question/CLAUDE.md),
//! 但提示词是约束不是保证 —— 模型漏写,vault 里就多一份没有 `type` 的文档(§4.1
//! 的唯一必填键)。所以收尾时补一次:**只补缺的,已经有 frontmatter 的一律不碰**。
use std::path::Path;

/// 文档是否已带首部 frontmatter 块。
fn has_frontmatter(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return false;
    };
    rest.contains("\n---")
}

/// 正文里的首个 ATX H1。
fn first_h1(text: &str) -> Option<&str> {
    text.lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l[2..].trim())
        .filter(|t| !t.is_empty())
}

/// YAML 双引号标量:标题里的引号/反斜杠必须转义,否则整份 frontmatter 不可解析。
fn quote(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

/// 默认 `type`:任务没声明 `okf_type` 时的兜底(`answers/` 下的长答案)。
/// 取值必须是 `src/lib/okf/concept.ts` 登记过的类型。
pub const DEFAULT_TYPE: &str = "Answer";

/// 给一份文档补上 OKF 概念头;已经有 frontmatter 就返回 None(不动它)。
/// `ty` 是 §4.1 的唯一必填键 `type`,`by` 是 §7 的 actor
/// (`<producer>/<version>`),`at` 是 ISO 8601;`generated` 按 §5.2 写成单一
/// mapping。
pub fn stamped(text: &str, ty: &str, by: &str, at: &str) -> Option<String> {
    if has_frontmatter(text) {
        return None;
    }
    let mut head = format!("---\ntype: {ty}\n");
    if let Some(title) = first_h1(text) {
        head.push_str(&format!("title: {}\n", quote(title)));
    }
    head.push_str(&format!("generated: {{ by: {}, at: {} }}\n---\n", by, at));
    Some(format!("{head}{text}"))
}

/// 对 vault 内的这批交付物就地补头。返回补过的份数。
/// 读不到/写不动的文件跳过 —— 收尾的元数据补写永远不该让一次成功的运行失败。
pub fn stamp_vault_docs(vault: &Path, rels: &[String], ty: &str, by: &str, at: &str) -> usize {
    let mut n = 0;
    for rel in rels {
        let p = vault.join(rel);
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let Some(next) = stamped(&text, ty, by, at) else { continue };
        if std::fs::write(&p, next).is_ok() {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_a_bare_answer_with_type_title_and_actor() {
        let out = stamped(
            "# KV cache 怎么省显存\n\n因为…\n",
            DEFAULT_TYPE,
            "claude-code/opus-5",
            "2026-08-04T09:00:00Z",
        )
        .expect("a bare answer must be stamped");
        assert_eq!(
            out,
            concat!(
                "---\n",
                "type: Answer\n",
                "title: \"KV cache 怎么省显存\"\n",
                "generated: { by: claude-code/opus-5, at: 2026-08-04T09:00:00Z }\n",
                "---\n",
                "# KV cache 怎么省显存\n\n因为…\n",
            )
        );
    }

    /// 一次 run 的目标文件(如电子书摘要)用任务声明的 type,不是 Answer。
    #[test]
    fn stamps_a_task_declared_type() {
        let out = stamped("# 深度工作 — 摘要\n", "Book Summary", "claude-agent/1.0.0", "T").unwrap();
        assert!(out.starts_with("---\ntype: Book Summary\ntitle: \"深度工作 — 摘要\"\n"), "got: {out}");
    }

    #[test]
    fn leaves_a_document_that_already_has_front_matter_alone() {
        assert!(stamped("---\ntype: Answer\n---\n# x\n", DEFAULT_TYPE, "a/1", "t").is_none());
    }

    #[test]
    fn omits_the_title_when_there_is_no_h1() {
        let out = stamped("答案正文,没有标题\n", DEFAULT_TYPE, "a/1", "t").unwrap();
        assert!(out.starts_with("---\ntype: Answer\ngenerated: "), "got: {out}");
    }

    #[test]
    fn escapes_a_hostile_title() {
        let out = stamped("# a \"quoted\" \\ title\n", DEFAULT_TYPE, "a/1", "t").unwrap();
        assert!(out.contains("title: \"a \\\"quoted\\\" \\\\ title\"\n"), "got: {out}");
    }

    #[test]
    fn stamping_a_directory_only_touches_what_needs_it() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("answers")).unwrap();
        std::fs::write(dir.path().join("answers/a.md"), "# A\n").unwrap();
        std::fs::write(dir.path().join("answers/b.md"), "---\ntype: Answer\n---\n# B\n").unwrap();
        let rels = vec!["answers/a.md".to_string(), "answers/b.md".to_string(), "answers/missing.md".to_string()];
        assert_eq!(stamp_vault_docs(dir.path(), &rels, DEFAULT_TYPE, "claude-code/opus-5", "T"), 1);
        assert!(std::fs::read_to_string(dir.path().join("answers/a.md")).unwrap().starts_with("---\ntype: Answer\n"));
        assert_eq!(std::fs::read_to_string(dir.path().join("answers/b.md")).unwrap(), "---\ntype: Answer\n---\n# B\n");
    }
}
