//! Prose `.md` chunking via pulldown-cmark's `OffsetIter`.
//!
//! Markdown's edge cases — nested fences, HTML blocks, setext headings, lazy
//! continuation — are exactly the sort of thing a hand-rolled scanner gets
//! subtly wrong, and a subtly wrong chunker produces subtly wrong line anchors.
//! So the boundaries come from the de-facto standard parser and the only thing
//! we do ourselves is map byte offsets back to 1-based line numbers.
//!
//! Internally everything is computed in body-relative line numbers (as if the
//! body started the file) and `body_start_line - 1` is added to every block
//! exactly once, at the end. That is simpler than threading the offset through
//! every helper and re-deriving it for the section-slicing arithmetic.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::block::{breadcrumb_of, Block, BlockLevel};
use crate::norm::{line_of, line_starts};

/// Chunk a prose body. `body_start_line` is the 1-based line the body begins on
/// in the whole file (4 when a 2-line frontmatter precedes it, 1 otherwise).
pub fn chunk(body: &str, body_start_line: u32) -> Vec<Block> {
    if body.trim().is_empty() {
        return Vec::new();
    }
    let starts = line_starts(body);
    // Body-relative (1-based) line number for a byte offset into `body`.
    let bl = |byte: usize| line_of(&starts, byte);

    let mut blocks: Vec<Block> = Vec::new();
    // Heading text per depth (index 0 = H1's text), truncated whenever a
    // shallower-or-equal heading arrives.
    let mut chain: Vec<String> = Vec::new();
    // Stack of sections currently open, outermost first: (start_line,
    // breadcrumb, depth). Nesting is intended, not a bug to collapse away — a
    // `# Title` section spanning the whole document and a `## Sub` section
    // spanning just its own span coexist, giving retrieval both a coarse and
    // a fine resolution to match against (design spec §3.3).
    let mut open_sections: Vec<(u32, String, usize)> = Vec::new();
    let mut sections: Vec<Block> = Vec::new();

    // Text accumulator for whichever heading is currently open. Kept separate
    // from `pending` below because a heading's inline content (Text/SoftBreak)
    // must never be confused with a sibling paragraph's — the two never
    // nest, but sharing one buffer would make that an invariant to maintain
    // rather than a fact the type system gives us for free.
    let mut heading: Option<(u32, u32, usize, String)> = None;
    // Text accumulator for the current paragraph / code block / list item.
    // A list item's Start opens this; if the item is "loose" (has an inner
    // Paragraph), the Paragraph's End closes it first and the Item's End
    // becomes a no-op — see the Item handling below.
    let mut pending: Option<(u32, u32, String)> = None;
    // How many `Tag::Item`s are currently open. A nested list lives *inside*
    // its parent item, so without this the inner item's `End` would close the
    // parent's accumulator and its `Start` would concatenate the child's text
    // straight onto the parent's with no separator — which manufactured a
    // token ("alpha onebeta two") that exists nowhere in the vault, on every
    // parent/child boundary of every nested bullet list. Only the outermost
    // `End(Item)` closes the block; deeper ones just contribute a line.
    let mut item_depth = 0usize;

    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_FOOTNOTES;

    for (event, range) in Parser::new_ext(body, opts).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let depth = heading_index(level);
                let start_line = bl(range.start);
                // A heading at or above an open section's depth ends that
                // section (and every section nested inside it, since they are
                // all at least as deep); a strictly deeper heading opens a
                // new nested section instead of closing anything.
                while let Some(&(_, _, d)) = open_sections.last() {
                    if d < depth {
                        break;
                    }
                    let (s, crumb, _) = open_sections.pop().unwrap();
                    let end = start_line.saturating_sub(1).max(s);
                    sections.push(section_block(&starts, body, s, end, crumb));
                }
                let end_line = bl(range.end.saturating_sub(1));
                heading = Some((start_line, end_line, depth, String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((s, e, depth, text)) = heading.take() {
                    let text = text.trim().to_string();
                    blocks.push(Block {
                        line_start: s,
                        line_end: e,
                        breadcrumb: breadcrumb_of(&chain),
                        text: text.clone(),
                        level: BlockLevel::Line,
                        is_annotation: false,
                        agent_by: None,
                    });
                    chain.truncate(depth);
                    chain.push(text);
                    open_sections.push((s, breadcrumb_of(&chain), depth));
                }
            }
            Event::Start(Tag::Item) => {
                item_depth += 1;
                match pending.as_mut() {
                    // A nested item under an item that is still accumulating:
                    // separate it with a newline, exactly the way `SoftBreak`
                    // already separates wrapped lines of one paragraph, and
                    // widen the block to cover it.
                    Some(p) => {
                        p.2.push('\n');
                        p.1 = p.1.max(bl(range.end.saturating_sub(1)));
                    }
                    None => {
                        let s = bl(range.start);
                        let e = bl(range.end.saturating_sub(1));
                        pending = Some((s, e, String::new()));
                    }
                }
            }
            Event::End(TagEnd::Item) => {
                item_depth = item_depth.saturating_sub(1);
                if item_depth == 0 {
                    close_pending(&mut pending, &mut blocks, &chain);
                }
            }
            Event::Start(Tag::Paragraph) | Event::Start(Tag::CodeBlock(_)) => {
                if pending.is_none() {
                    let s = bl(range.start);
                    let e = bl(range.end.saturating_sub(1));
                    pending = Some((s, e, String::new()));
                }
            }
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::CodeBlock) => {
                close_pending(&mut pending, &mut blocks, &chain);
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(h) = heading.as_mut() {
                    h.3.push_str(&t);
                } else if let Some(p) = pending.as_mut() {
                    p.2.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(h) = heading.as_mut() {
                    h.3.push(' ');
                } else if let Some(p) = pending.as_mut() {
                    p.2.push('\n');
                }
            }
            _ => {}
        }
    }

    let last_line = bl(body.len().saturating_sub(1));
    while let Some((s, crumb, _)) = open_sections.pop() {
        sections.push(section_block(&starts, body, s, last_line, crumb));
    }
    blocks.extend(sections);

    blocks.push(Block {
        line_start: 1,
        line_end: last_line,
        breadcrumb: String::new(),
        text: body.trim().to_string(),
        level: BlockLevel::File,
        is_annotation: false,
        agent_by: None,
    });

    let shift = body_start_line - 1;
    for b in &mut blocks {
        b.line_start += shift;
        b.line_end += shift;
    }
    blocks
}

/// Emit whatever the accumulator holds as a `Line` block and clear it.
/// Extracted so the paragraph/code-block path and the outermost-list-item
/// path cannot drift apart.
fn close_pending(
    pending: &mut Option<(u32, u32, String)>,
    blocks: &mut Vec<Block>,
    chain: &[String],
) {
    let Some((s, e, text)) = pending.take() else { return };
    if text.trim().is_empty() {
        return;
    }
    blocks.push(Block {
        line_start: s,
        line_end: e,
        breadcrumb: breadcrumb_of(chain),
        text: text.trim().to_string(),
        level: BlockLevel::Line,
        is_annotation: false,
        agent_by: None,
    });
}

fn heading_index(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 0,
        HeadingLevel::H2 => 1,
        HeadingLevel::H3 => 2,
        HeadingLevel::H4 => 3,
        HeadingLevel::H5 => 4,
        HeadingLevel::H6 => 5,
    }
}

/// Slice `body` on line boundaries (body-relative, 1-based, inclusive) rather
/// than replaying the parser's byte offsets a second time.
fn section_block(starts: &[usize], body: &str, start_line: u32, end_line: u32, breadcrumb: String) -> Block {
    let from = starts.get((start_line - 1) as usize).copied().unwrap_or(0);
    let to = starts.get(end_line as usize).copied().unwrap_or(body.len()).min(body.len());
    Block {
        line_start: start_line,
        line_end: end_line.max(start_line),
        breadcrumb,
        text: body[from..to].trim().to_string(),
        level: BlockLevel::Section,
        is_annotation: false,
        agent_by: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(blocks: &[Block], level: BlockLevel) -> Vec<(u32, u32)> {
        blocks.iter().filter(|b| b.level == level).map(|b| (b.line_start, b.line_end)).collect()
    }

    #[test]
    fn paragraphs_headings_and_fences_become_line_blocks() {
        let md = "# Title\n\npara one\n\n```rs\nlet x = 1;\n```\n";
        let b = chunk(md, 1);
        let texts: Vec<&str> = b.iter().filter(|x| x.level == BlockLevel::Line).map(|x| x.text.as_str()).collect();
        assert!(texts.iter().any(|t| t.contains("para one")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("let x = 1;")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("Title")), "{texts:?}");
    }

    /// 行号必须落在源文本的真实行上——这是 `path:line:text` 与 `#L120` 回源锚
    /// 的全部价值所在,错一行就等于骗了 agent。
    #[test]
    fn line_numbers_point_at_the_real_source_lines() {
        let md = "# T\n\nalpha\n\nbeta\n";
        let b = chunk(md, 1);
        let alpha = b.iter().find(|x| x.text.contains("alpha")).unwrap();
        assert_eq!((alpha.line_start, alpha.line_end), (3, 3));
        let beta = b.iter().find(|x| x.text.contains("beta")).unwrap();
        assert_eq!((beta.line_start, beta.line_end), (5, 5));
    }

    /// frontmatter 之后的正文要按它在**整个文件**里的行号编号。
    #[test]
    fn body_start_line_offsets_every_block() {
        let b = chunk("alpha\n", 4);
        assert_eq!(lines_of(&b, BlockLevel::Line), vec![(4, 4)]);
    }

    #[test]
    fn breadcrumb_is_the_heading_chain() {
        let md = "# A\n\n## B\n\ntext\n";
        let b = chunk(md, 1);
        let t = b.iter().find(|x| x.text.contains("text")).unwrap();
        assert_eq!(t.breadcrumb, "A > B");
    }

    /// 面包屑每级截 40 字,避免长标题把 breadcrumb 撑爆(spec §3.6)。
    #[test]
    fn breadcrumb_truncates_each_level_to_40_chars() {
        let long = "x".repeat(60);
        let md = format!("# {long}\n\ntext\n");
        let b = chunk(&md, 1);
        let t = b.iter().find(|x| x.text.contains("text")).unwrap();
        assert_eq!(t.breadcrumb.chars().count(), 40);
    }

    #[test]
    fn section_and_file_level_blocks_are_derived() {
        let md = "# A\n\nalpha\n\n# B\n\nbeta\n";
        let b = chunk(md, 1);
        assert_eq!(lines_of(&b, BlockLevel::File), vec![(1, 7)]);
        let sections = lines_of(&b, BlockLevel::Section);
        assert_eq!(sections.len(), 2, "{sections:?}");
        assert_eq!(sections[0], (1, 4));
    }

    /// 无标题的纯正文也必须有 file 级块,否则「这文档讲什么」类查询召不回。
    #[test]
    fn a_file_without_headings_still_gets_a_file_block() {
        let b = chunk("just text\n", 1);
        assert_eq!(lines_of(&b, BlockLevel::File), vec![(1, 1)]);
    }

    #[test]
    fn an_empty_body_yields_no_blocks() {
        assert!(chunk("", 1).is_empty());
        assert!(chunk("   \n\n", 1).iter().all(|b| !b.text.trim().is_empty() || b.level == BlockLevel::File));
    }

    /// Baseline for the nested case below: sibling items are separate blocks
    /// and each one's line range is its own line.
    #[test]
    fn a_tight_list_gives_one_block_per_item() {
        let md = "- alphaxq one\n- betaxq two\n";
        let b = chunk(md, 1);
        let items: Vec<&Block> = b.iter().filter(|x| x.level == BlockLevel::Line).collect();
        assert_eq!(items.len(), 2, "{:?}", items.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert_eq!((items[0].text.as_str(), items[0].line_start, items[0].line_end), ("alphaxq one", 1, 1));
        assert_eq!((items[1].text.as_str(), items[1].line_start, items[1].line_end), ("betaxq two", 2, 2));
    }

    /// 存进索引的每个词都必须真的在源文件里出现过——这是 `path#Lnnn` 引用可信的
    /// 全部前提。嵌套 bullet 的父子边界曾把两行直接拼接(`alphaxq onebetaxq
    /// two`),凭空造出一个 vault 里不存在的词,同时毁掉两个真实的词;而
    /// `drop_redundant_rollups` 会把没被污染的 File 级块当冗余丢掉,于是这个坏块
    /// 正是唯一会被召回的那个。
    #[test]
    fn a_nested_list_item_is_separated_from_its_parent_not_fused_onto_it() {
        let md = "- alphaxq one\n  - betaxq two\n";
        let b = chunk(md, 1);
        let items: Vec<&Block> = b.iter().filter(|x| x.level == BlockLevel::Line).collect();
        assert_eq!(items.len(), 1, "{:?}", items.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert_eq!(items[0].text, "alphaxq one\nbetaxq two");
        assert_eq!((items[0].line_start, items[0].line_end), (1, 2));
        // Every word stored must exist in the source, and no word may be
        // destroyed by the join.
        for block in &b {
            for word in block.text.split_whitespace() {
                assert!(md.contains(word), "invented token {word:?} in {:?}", block.text);
            }
        }
        assert!(b.iter().any(|x| x.text.contains("alphaxq")));
        assert!(b.iter().any(|x| x.text.contains("betaxq")));
    }

    /// Three levels, two children under one parent: the accumulator must
    /// survive the inner `End(Item)`s and close exactly once, at the outermost
    /// one, with a separator at every boundary.
    #[test]
    fn deeply_nested_list_items_each_get_their_own_line_in_the_block() {
        let md = "- parentxq\n  - childxq one\n  - childxq two\n    - grandchildxq\n";
        let b = chunk(md, 1);
        let items: Vec<&Block> = b.iter().filter(|x| x.level == BlockLevel::Line).collect();
        assert_eq!(items.len(), 1, "{:?}", items.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert_eq!(items[0].text, "parentxq\nchildxq one\nchildxq two\ngrandchildxq");
        assert_eq!((items[0].line_start, items[0].line_end), (1, 4));
    }

    /// A `#` title followed by `##` subsections — the most common document
    /// shape in this product — must yield a section per level, not one
    /// section for the whole file. Coarse (title) and fine (subsection)
    /// sections coexist; ranking sorts out the overlap later (spec §3.3).
    ///
    /// Lines: 1 `# Title`, 2 blank, 3 `intro`, 4 blank, 5 `## First`, 6 blank,
    /// 7 `alpha`, 8 blank, 9 `## Second`, 10 blank, 11 `beta`. `Title` never
    /// meets a depth-0 heading again, so it stays open to EOF (line 11).
    /// `First` closes when `Second` arrives (line 9), so it ends at line 8.
    #[test]
    fn nested_sections_are_emitted() {
        let md = "# Title\n\nintro\n\n## First\n\nalpha\n\n## Second\n\nbeta\n";
        let b = chunk(md, 1);
        let sections = lines_of(&b, BlockLevel::Section);
        assert!(sections.contains(&(1, 11)), "{sections:?}"); // whole-doc Title section
        assert!(sections.contains(&(5, 8)), "{sections:?}"); // First's own span, not Second's
        assert!(sections.contains(&(9, 11)), "{sections:?}"); // Second's own span
        assert_eq!(sections.len(), 3, "{sections:?}");
    }

    /// `# D` at depth 0 must close every open section at depth >= 0 — C (2),
    /// B (1), and A (0) — not just the innermost one, and D's own section
    /// then runs to end of input since nothing closes it.
    ///
    /// Lines: 1 `# A`, 5 `## B`, 9 `### C`, 13 `# D`, 15 `d` (final paragraph).
    /// `D` arriving closes A/B/C all at line 12 (the blank line before it);
    /// `D`'s own section spans 13-15, closed only at EOF.
    #[test]
    fn deeper_nesting_closes_correctly_when_a_shallow_heading_arrives() {
        let md = "# A\n\na\n\n## B\n\nb\n\n### C\n\nc\n\n# D\n\nd\n";
        let b = chunk(md, 1);
        let sections = lines_of(&b, BlockLevel::Section);
        assert_eq!(sections.len(), 4, "{sections:?}");
        assert!(sections.contains(&(1, 12)), "{sections:?}"); // A
        assert!(sections.contains(&(5, 12)), "{sections:?}"); // B
        assert!(sections.contains(&(9, 12)), "{sections:?}"); // C
        assert!(sections.contains(&(13, 15)), "{sections:?}"); // D
    }

    /// A heading closes every open section at >= its own depth, but leaves
    /// shallower ancestors open: `## mid` must close `### deep` but not `# A`.
    ///
    /// Lines: 1 `# A`, 5 `### deep`, 9 `## mid`, 11 `m` (final paragraph).
    /// `mid` (depth 1) closes `deep` (depth 2) at line 8 but does not touch
    /// `A` (depth 0 < 1), which stays open to EOF (line 11).
    #[test]
    fn a_shallower_heading_closes_only_deeper_open_sections() {
        let md = "# A\n\na\n\n### deep\n\nd\n\n## mid\n\nm\n";
        let b = chunk(md, 1);
        let sections = lines_of(&b, BlockLevel::Section);
        assert_eq!(sections.len(), 3, "{sections:?}");
        assert!(sections.contains(&(5, 8)), "{sections:?}"); // deep, closed by mid
        assert!(sections.contains(&(9, 11)), "{sections:?}"); // mid, closed at EOF
        // `# A`'s section must still be open past `## mid` — it only closes
        // at end of input, so it spans the whole document.
        assert!(sections.contains(&(1, 11)), "{sections:?}");
    }
}
