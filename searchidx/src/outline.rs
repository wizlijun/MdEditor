//! `.note.md` (sidecar note) chunking: one block per outline node.
//!
//! This is a THIRD implementation of the outline format — TypeScript owns
//! `src/lib/outline/markdown.ts`, the roam-import plugin owns a Rust port. This
//! one is read-only and only cares about line attribution, so it stays small;
//! the shared fixtures in tests/fixtures/outline are what stop the three from
//! drifting. See tests/outline_fixtures.rs.
//!
//! ## Deliberate simplifications (documented, not fixed)
//!
//! Conformance work on this module targets **content attribution** — which
//! lines exist as nodes, and whether any content gets lost — not byte-exact
//! agreement with `markdown.ts` on indentation strictness. The distinction
//! matters: attribution drift can make real content unfindable; indentation
//! drift only changes *how finely* a malformed file gets chunked, and the
//! design spec pre-accepts that as acceptable slop on files nobody wrote by
//! hand-following the outline format precisely. Three gaps fall on the
//! "granularity, not findability" side of that line and are left as-is:
//!
//! 1. **Continuation/property lines match regardless of indent.** `markdown.ts`
//!    requires a continuation line to start with the *exact* `contIndent`
//!    (`'  '.repeat(currentDepth) + '  '`) for the current node, and falls
//!    back to manufacturing a new root node otherwise. This module accepts
//!    any indent, as long as [`property`] or the plain-append path can make
//!    sense of the line. On a file with consistent 2-space nesting this never
//!    matters; on a file with ragged indentation, this module folds lines
//!    into the wrong node's `text` (still findable, wrong breadcrumb) where
//!    `markdown.ts` would split them into an extra sibling node.
//!    What this module does *not* do any more is flatten the line once it has
//!    matched: it strips at most `contIndent` leading spaces
//!    ([`strip_cont_indent`]), the same slice `markdown.ts` takes, so
//!    indentation *inside* a fenced answer body survives into the stored text
//!    the panel renders and `--json` emits.
//! 2. **`fence_len` doesn't require "backticks then only whitespace" the way
//!    TS's close regex (`/^(\`{3,})\s*$/`) does** — trailing non-whitespace
//!    after the backticks (e.g. `` ``` js``) is still counted as a fence
//!    delimiter here. Affects only how a malformed close line is read, not
//!    whether the fenced content is preserved.
//! 3. **Bullet depth is `indent_chars / 2` with no divisibility check.** A
//!    line indented by an odd number of spaces, or by tabs, lands at
//!    `indent / 2` (integer division) here, where `markdown.ts`'s bullet
//!    regex (`^((?:  )*)-...`) requires indent in *exact* multiples of two
//!    spaces and otherwise doesn't match as a bullet at all (falling through
//!    to the root-node fallback instead). Again: a different chunk boundary
//!    on a file nobody wrote correctly, not lost content.
//!
//! The shared fixtures in `tests/fixtures/outline` do not exercise any of
//! these three — they exist to pin content attribution, which is the part of
//! this module's behavior that actually affects whether a user's text is
//! findable.

use crate::block::{breadcrumb_of, Block, BlockLevel};

/// Property lines like `type:: question`. Same key set as the TS parser.
fn property(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    let (k, v) = t.split_once(":: ")?;
    matches!(k, "type" | "line" | "id" | "collapsed" | "created" | "updated" | "status" | "answered" | "by")
        .then(|| (k, v.trim()))
}

/// A bullet line, as `(depth, content)`.
///
/// A **bare `-`** counts, with empty content. That is not leniency, it is the
/// authoritative parser's rule: `markdown.ts`'s `^((?:  )*)-(?: (.*))?$`
/// makes the space optional because `serializeOutline` writes an empty block
/// as `- ` — trailing space and all — and editors, formatters and git hooks
/// routinely strip trailing whitespace from a vault file. Reading the bare
/// form as "not a bullet" made this module fold the dash into the previous
/// node's text and then re-parent that node's children onto the *wrong*
/// ancestor, so the empty node's whole subtree got the wrong breadcrumb: a
/// content-attribution divergence, not a formatting nicety. See
/// `tests/fixtures/outline/empty-bullet.note.md`.
fn bullet(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    let indent = line.len() - t.len();
    // `-` alone, or `- ` plus content. NOT a prefix test on `-`, which would
    // swallow `--`/`---` (setext rules, front-matter fences) the same way the
    // TS regex avoids by requiring end-of-line or exactly one space.
    let rest = t.strip_prefix("- ").or_else(|| (t == "-").then_some(""))?;
    Some((indent / 2, rest))
}

/// Strip at most `max` leading spaces — the current node's continuation
/// indent — instead of every leading space. `markdown.ts` slices exactly
/// `contIndent` off a continuation line (falling back to `^ {0,N}` when the
/// line is under-indented), so anything indented *deeper* than its node keeps
/// that extra indentation. `trim_start()` here used to flatten it, which
/// silently reformatted the interior of fenced code inside an answer — the
/// exact text the panel shows and `--json` emits. Tabs are left alone,
/// matching TS, which only ever counts spaces.
fn strip_cont_indent(line: &str, max: usize) -> &str {
    let n = line.chars().take_while(|c| *c == ' ').count().min(max);
    &line[n..]
}

/// Number of leading backticks, if there are at least 3 (a fence delimiter).
/// Only ever checked against a bullet's own inline content (`- \`\`\`` opens
/// an answer body fence right on the item line) — mirroring the TS parser's
/// `content.match(/^(\`{3,})/)` check, which runs once at push time and never
/// again. A fence marker that shows up on a later continuation line does NOT
/// open raw mode in `markdown.ts`, so it must not here either: a Rust-only
/// standalone-line fence branch used to exist here and was removed, because
/// it made this module *more* protective of embedded `- ` lines than the
/// authoritative parser is — silent disagreement on which lines are nodes,
/// not just a formatting nicety. See `tests/fixtures/outline/fence-continuation.note.md`.
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
    // The current node's continuation indent, mirroring `markdown.ts`'s
    // `'  '.repeat(currentDepth) + '  '`: how many leading spaces belong to
    // the outline structure rather than to the content. See
    // `strip_cont_indent`.
    let mut cont_indent = 0usize;
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
                blocks[idx].text.push_str(strip_cont_indent(raw, cont_indent));
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
            cont_indent = (depth + 1) * 2;
            // A fenced answer body opens right on the bullet's own line (e.g.
            // `- \`\`\`markdown`), not on a later continuation line — same
            // rule the TS parser applies when it pushes the node (checked
            // once, here, at push time — see the doc comment on `fence_len`).
            if let Some(n) = fence_len(content) {
                fence = n;
            }
            continue;
        }

        let Some(idx) = current else {
            // A non-bullet, non-blank line before the first bullet isn't
            // outline syntax, but `markdown.ts` doesn't drop it either — it
            // downgrades it to a hand-written root node (spec: 不丢内容). A
            // `.note.md` opened in a plain editor and saved with a stray
            // leading line must not make that line silently unfindable.
            // See tests/fixtures/outline/prose-before-bullet.note.md.
            if raw.trim().is_empty() {
                continue;
            }
            let content = raw.trim().to_string();
            chain.push(content.clone());
            blocks.push(Block {
                line_start: line_no,
                line_end: line_no,
                breadcrumb: String::new(),
                text: content,
                level: BlockLevel::Line,
                is_annotation: false,
                agent_by: None,
            });
            current = Some(blocks.len() - 1);
            // `markdown.ts` downgrades this line with `push(0, …)` and sets
            // `currentDepth = 0`, so its continuation indent is a root
            // node's.
            cont_indent = 2;
            continue;
        };
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
            blocks[idx].text.push_str(strip_cont_indent(raw, cont_indent));
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

    /// A fence that opens on a *continuation* line (not the bullet's own
    /// inline content) does NOT put `markdown.ts` into raw mode — it only
    /// ever checks for an opening fence once, at push time, against the
    /// bullet's own content. So a `- ` line inside such a fence is real
    /// bullet syntax to both parsers: it becomes its own node, not part of
    /// its parent's text. This used to be named
    /// `bullets_inside_an_answer_fence_are_content_not_nodes` and asserted
    /// the opposite (2 nodes) — that was wrong; it encoded a Rust-only
    /// "smarter than TS" fence branch that has since been removed.
    ///
    /// The input deliberately ends right after the embedded bullet line,
    /// without a closing fence line. A closing fence line here would ALSO
    /// exercise the (documented, intentionally unfixed) indent-strictness gap
    /// on continuation-line matching in this module's doc comment — this
    /// test is only about fence-open attribution, so it stays clear of that
    /// second variable. See `tests/fixtures/outline/fence-continuation.note.md`
    /// for the TS-cross-checked version of this shape.
    #[test]
    fn a_fence_opened_on_a_continuation_line_does_not_protect_embedded_bullets() {
        let md = "- q\n  - a\n    ```\n    - not a bullet\n";
        let b = nodes(md);
        assert_eq!(b.len(), 3, "{:?}", b.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert_eq!(b[0].text, "q");
        assert_eq!(b[1].text, "a\n```");
        assert_eq!(b[2].text, "not a bullet");
        assert_eq!(b[2].breadcrumb, "q > a");
    }

    /// Unlike the previous test's continuation-line fence, a fence that opens
    /// right on the bullet's own line (the real shape `wrapAnswerBody`
    /// produces — see `docs/2026-08-10-vault-search-index-design.md` and
    /// `src/lib/outline/markdown.test.ts`'s fenced-answer cases) IS
    /// recognized by both parsers, and does protect embedded `- ` lines.
    #[test]
    fn a_fence_opening_on_the_bullets_own_line_still_protects_its_body() {
        let md = "- q\n  - ```\n    - not a bullet\n    ```\n    type:: answer\n";
        let b = nodes(md);
        assert_eq!(b.len(), 2, "{:?}", b.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert!(b[1].text.contains("not a bullet"));
        assert_eq!(b[1].text, "```\n- not a bullet\n```");
    }

    /// `markdown.ts` downgrades a non-bullet line before the first `- ` to a
    /// hand-written root node rather than dropping it. See
    /// `tests/fixtures/outline/prose-before-bullet.note.md` for the
    /// TS-cross-checked version.
    #[test]
    fn content_before_the_first_bullet_becomes_a_root_node_not_lost() {
        let b = nodes("stray prose\n- a\n");
        assert_eq!(b.len(), 2, "{:?}", b.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert_eq!(b[0].text, "stray prose");
        assert_eq!(b[0].breadcrumb, "");
        assert_eq!(b[1].text, "a");
    }

    /// 空块被 `serializeOutline` 写成 `- `(破折号+空格+空内容),而编辑器/
    /// 格式化器/git 钩子例行删掉行尾空白 —— 于是「只有缩进和一个 `-`」的行同样
    /// 是空 bullet,`markdown.ts` 的 `-(?: (.*))?$` 正是为此而写。读成「不是
    /// bullet」会把破折号折进上一个节点的文本,并把它的子节点改挂到错误的祖先
    /// 上:内容归属分歧,不是格式细节。
    #[test]
    fn a_bare_dash_is_an_empty_bullet_and_still_parents_its_children() {
        let b = nodes("- top\n-\n  - child\n");
        assert_eq!(b.len(), 3, "{:?}", b.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert_eq!(b[0].text, "top");
        assert_eq!(b[1].text, "", "the bare dash is a node of its own, with empty content");
        assert_eq!(b[1].line_start, 2);
        assert_eq!(b[2].text, "child");
        assert_eq!(b[2].breadcrumb, "", "the child hangs under the empty node, not under `top`");
    }

    /// The bare-dash rule must stay a whole-line rule, not a `-` prefix test:
    /// `markdown.ts` requires end-of-line or exactly one space after the dash
    /// precisely so `--`/`---` (front-matter fences, horizontal rules) are not
    /// swallowed as bullets.
    #[test]
    fn a_double_dash_or_horizontal_rule_is_not_a_bullet() {
        let b = nodes("- a\n--\n---\n");
        assert_eq!(b.len(), 1, "{:?}", b.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert_eq!(b[0].text, "a\n--\n---");
    }

    /// 围栏答复正文里的缩进是内容,不是排版噪音:面板显示的、`--json` 吐给
    /// agent 的正是这段文本。`trim_start` 会把每行压平;TS 只剥掉本节点的续行
    /// 缩进(contIndent),多出来的缩进原样留着。
    #[test]
    fn indentation_inside_a_fenced_answer_body_survives() {
        let md = "- q\n  - ```\n    fn main() {\n        let x = 1;\n    }\n    ```\n";
        let b = nodes(md);
        assert_eq!(b.len(), 2, "{:?}", b.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert_eq!(b[1].text, "```\nfn main() {\n    let x = 1;\n}\n```");
    }

    /// The same rule outside a fence: a continuation line indented deeper than
    /// its node keeps the surplus.
    #[test]
    fn a_continuation_line_keeps_indentation_beyond_its_nodes_own() {
        let b = nodes("- a\n      deep\n");
        assert_eq!(b[0].text, "a\n    deep");
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
