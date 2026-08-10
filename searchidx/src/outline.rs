//! `.note.md` (sidecar note) chunking: one block per outline node.
//!
//! This is a THIRD implementation of the outline format — TypeScript owns
//! `src/lib/outline/markdown.ts`, the roam-import plugin owns a Rust port. This
//! one is read-only and only cares about line attribution, so it stays small;
//! the shared fixtures in tests/fixtures/outline are what stop the three from
//! drifting. See tests/outline_fixtures.rs.

use crate::block::{breadcrumb_of, Block, BlockLevel};

/// Property lines like `type:: question`. Same key set as the TS parser.
fn property(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    let (k, v) = t.split_once(":: ")?;
    matches!(k, "type" | "line" | "id" | "collapsed" | "created" | "updated" | "status" | "answered" | "by")
        .then(|| (k, v.trim()))
}

fn bullet(line: &str) -> Option<(usize, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = line.trim_start().strip_prefix("- ")?;
    Some((indent / 2, rest))
}

/// Number of leading backticks, if there are at least 3 (a fence delimiter).
/// Used both for standalone fence lines and for a bullet's own inline content
/// (`- \`\`\`` opens an answer body fence right on the item line) — mirroring
/// the TS parser's `content.match(/^(\`{3,})/)` check done at push time.
fn fence_len(line: &str) -> Option<usize> {
    let t = line.trim_start();
    let n = t.chars().take_while(|c| *c == '`').count();
    (n >= 3).then_some(n)
}

/// Chunk an outline body. `body_start_line` is the 1-based line the body starts
/// on within the whole file.
pub fn chunk(body: &str, body_start_line: u32) -> Vec<Block> {
    let lines: Vec<&str> = body.lines().collect();
    let mut blocks: Vec<Block> = Vec::new();
    let mut chain: Vec<String> = Vec::new();
    // index into `blocks` of the node currently accumulating lines
    let mut current: Option<usize> = None;
    let mut fence = 0usize;

    for (i, raw) in lines.iter().enumerate() {
        let line_no = body_start_line + i as u32;

        if fence > 0 {
            if fence_len(raw).is_some_and(|n| n >= fence) {
                fence = 0;
            }
            if let Some(idx) = current {
                blocks[idx].line_end = line_no;
                blocks[idx].text.push('\n');
                blocks[idx].text.push_str(raw.trim_start());
            }
            continue;
        }
        if let Some(n) = fence_len(raw) {
            fence = n;
            if let Some(idx) = current {
                blocks[idx].line_end = line_no;
            }
            continue;
        }

        if let Some((depth, content)) = bullet(raw) {
            chain.truncate(depth);
            let breadcrumb = breadcrumb_of(&chain);
            chain.push(content.to_string());
            blocks.push(Block {
                line_start: line_no,
                line_end: line_no,
                breadcrumb,
                text: content.to_string(),
                level: BlockLevel::Line,
                is_annotation: false,
                agent_by: None,
            });
            current = Some(blocks.len() - 1);
            // A fenced answer body opens right on the bullet's own line (e.g.
            // `- \`\`\`markdown`), not on a later continuation line — same
            // rule the TS parser applies when it pushes the node. Checking
            // only the standalone-line branch above would miss this and let
            // every `- ` inside the fence be mistaken for a sibling bullet.
            if let Some(n) = fence_len(content) {
                fence = n;
            }
            continue;
        }

        let Some(idx) = current else { continue };
        blocks[idx].line_end = line_no;
        if let Some((key, value)) = property(raw) {
            match key {
                "type" if value == "annotation" || value == "question" => {
                    blocks[idx].is_annotation = true;
                }
                // `human:` prefix, not a hardcoded id: OKF §7 makes the prefix
                // the machine-checkable signal for "a person stands behind this".
                "by" if !value.starts_with("human:") => {
                    blocks[idx].agent_by = Some(value.to_string());
                }
                _ => {}
            }
            continue;
        }
        if !raw.trim().is_empty() {
            blocks[idx].text.push('\n');
            blocks[idx].text.push_str(raw.trim_start());
        }
    }

    if !blocks.is_empty() {
        let last = blocks.iter().map(|b| b.line_end).max().unwrap_or(body_start_line);
        blocks.push(Block {
            line_start: body_start_line,
            line_end: last,
            breadcrumb: String::new(),
            text: body.trim().to_string(),
            level: BlockLevel::File,
            is_annotation: false,
            agent_by: None,
        });
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `chunk()` always appends a trailing `BlockLevel::File` block (mirroring
    /// `prose::chunk`'s sibling behavior — see
    /// `a_file_level_block_is_derived_for_the_whole_outline` below, which needs
    /// it present). Everything else in this module is about per-node
    /// attribution, so the helper filters down to the `Line` blocks it's
    /// actually testing — same convention `prose.rs`'s own tests use
    /// (`lines_of`), rather than baking the File-block count into every
    /// `len()` assertion here.
    fn nodes(md: &str) -> Vec<Block> {
        chunk(&crate::norm::strip_cr(md), 1)
            .into_iter()
            .filter(|b| b.level == BlockLevel::Line)
            .collect()
    }

    #[test]
    fn each_bullet_becomes_a_block_spanning_its_property_lines() {
        let b = nodes("- alpha\n  type:: annotation\n- beta\n");
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].line_start, b[0].line_end), (1, 2));
        assert_eq!(b[0].text, "alpha");
        assert_eq!((b[1].line_start, b[1].line_end), (3, 3));
    }

    /// 属性行是元数据,不是内容:它们决定 is_annotation/agent_by,但绝不进
    /// 可检索文本,否则每个节点都会被 "type" "annotation" 这类噪音词污染。
    #[test]
    fn property_lines_are_metadata_not_searchable_text() {
        let b = nodes("- alpha\n  type:: annotation\n  by:: claude/1\n");
        assert_eq!(b[0].text, "alpha");
        assert!(b[0].is_annotation);
        assert_eq!(b[0].agent_by.as_deref(), Some("claude/1"));
    }

    /// spec §3.5:人写信号靠**前缀匹配**,不写死某个 id。
    #[test]
    fn a_human_actor_is_not_recorded_as_an_agent_author() {
        let b = nodes("- alpha\n  by:: human:bruce\n");
        assert_eq!(b[0].agent_by, None);
    }

    #[test]
    fn breadcrumb_is_the_ancestor_chain() {
        let b = nodes("- top\n  - mid\n    - leaf\n");
        assert_eq!(b[2].text, "leaf");
        assert_eq!(b[2].breadcrumb, "top > mid");
    }

    /// 围栏内的 `- ` 不是 bullet。答复正文里带列表是常态,切错就等于把一条
    /// 答复劈成几个假节点。
    #[test]
    fn bullets_inside_an_answer_fence_are_content_not_nodes() {
        let md = "- q\n  - a\n    ```\n    - not a bullet\n    ```\n";
        let b = nodes(md);
        assert_eq!(b.len(), 2, "{:?}", b.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert!(b[1].text.contains("not a bullet"));
    }

    /// The fence marker can also open right on the bullet's own line (the
    /// real shape `wrapAnswerBody` produces) — not just on a later
    /// continuation line, as the previous test covers.
    #[test]
    fn a_fence_opening_on_the_bullets_own_line_still_protects_its_body() {
        let md = "- q\n  - ```\n    - not a bullet\n    ```\n    type:: answer\n";
        let b = nodes(md);
        assert_eq!(b.len(), 2, "{:?}", b.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert!(b[1].text.contains("not a bullet"));
        assert_eq!(b[1].text, "```\n- not a bullet\n```");
    }

    #[test]
    fn question_nodes_count_as_annotations() {
        let b = nodes("- ?\n  type:: question\n");
        assert!(b[0].is_annotation);
    }

    #[test]
    fn a_file_level_block_is_derived_for_the_whole_outline() {
        // Unfiltered: `nodes()` strips File-level blocks for the tests above,
        // but this test is specifically about that block's existence.
        let b = chunk(&crate::norm::strip_cr("- a\n- b\n"), 1);
        assert!(b.iter().any(|x| x.level == crate::block::BlockLevel::File));
    }
}
