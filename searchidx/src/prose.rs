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
    // The section currently accumulating: (start_line, breadcrumb, depth).
    let mut open_section: Option<(u32, String, usize)> = None;
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

    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_FOOTNOTES;

    for (event, range) in Parser::new_ext(body, opts).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let depth = heading_index(level);
                let start_line = bl(range.start);
                // A heading at or above the open section's depth ends it; a
                // deeper heading (a subsection) is absorbed into it instead.
                if let Some((s, crumb, d)) = open_section.take() {
                    if depth <= d {
                        sections.push(section_block(&starts, body, s, start_line.saturating_sub(1).max(s), crumb));
                    } else {
                        open_section = Some((s, crumb, d));
                    }
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
                    if open_section.is_none() {
                        open_section = Some((s, breadcrumb_of(&chain), depth));
                    }
                }
            }
            Event::Start(Tag::Paragraph) | Event::Start(Tag::CodeBlock(_)) | Event::Start(Tag::Item) => {
                if pending.is_none() {
                    let s = bl(range.start);
                    let e = bl(range.end.saturating_sub(1));
                    pending = Some((s, e, String::new()));
                }
            }
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::CodeBlock) | Event::End(TagEnd::Item) => {
                if let Some((s, e, text)) = pending.take() {
                    if !text.trim().is_empty() {
                        blocks.push(Block {
                            line_start: s,
                            line_end: e,
                            breadcrumb: breadcrumb_of(&chain),
                            text: text.trim().to_string(),
                            level: BlockLevel::Line,
                            is_annotation: false,
                            agent_by: None,
                        });
                    }
                }
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
    if let Some((s, crumb, _)) = open_section.take() {
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
}
