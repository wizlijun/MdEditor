//! `.txt` chunking — the plainest possible chunker in this crate.
//!
//! `.txt` files carry no markup this product controls (no headings, no
//! lists, no fenced code) and no timecodes to strip (that's `transcript`'s
//! job for `.srt`/`.vtt`) — just paragraphs separated by blank lines. So the
//! only structural signal available is the blank line itself: a run of one
//! or more blank lines closes the current paragraph, and a run of several in
//! a row must not manufacture empty blocks in between (an OCR export or a
//! hand-edited transcript-adjacent note commonly has ragged blank-line runs).
//!
//! Unlike `prose::chunk`, there is no heading structure to derive `Section`/
//! `File`-level rollups from, so every block here is `BlockLevel::Line` with
//! an empty `breadcrumb` — there is nothing to put in one.

use crate::block::{Block, BlockLevel};

/// Chunk a plain-text body. `body_start_line` is the 1-based line the body
/// begins on in the whole file (matches the signature of `prose::chunk`,
/// `outline::chunk` and `transcript::chunk` — `chunk::parse_file` picks
/// between the four based on extension).
pub fn chunk(body: &str, body_start_line: u32) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut pending_text = String::new();
    let mut pending_start: Option<u32> = None;
    let mut pending_end: u32 = 0;

    for (i, line) in body.lines().enumerate() {
        let line_no = body_start_line + i as u32;
        if line.trim().is_empty() {
            flush(&mut blocks, &mut pending_text, &mut pending_start, pending_end);
            continue;
        }
        if pending_start.is_none() {
            pending_start = Some(line_no);
        }
        if !pending_text.is_empty() {
            pending_text.push('\n');
        }
        pending_text.push_str(line);
        pending_end = line_no;
    }
    flush(&mut blocks, &mut pending_text, &mut pending_start, pending_end);
    blocks
}

/// Emit whatever the accumulator holds as a `Line` block and clear it —
/// mirrors `transcript::flush`.
fn flush(blocks: &mut Vec<Block>, pending_text: &mut String, pending_start: &mut Option<u32>, pending_end: u32) {
    let Some(start) = pending_start.take() else { return };
    let text = std::mem::take(pending_text);
    if text.trim().is_empty() {
        return;
    }
    blocks.push(Block {
        line_start: start,
        line_end: pending_end,
        breadcrumb: String::new(),
        text,
        level: BlockLevel::Line,
        is_annotation: false,
        agent_by: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_lines_separate_paragraphs() {
        let b = chunk("第一段第一行\n第一段第二行\n\n第二段\n", 1);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].line_start, 1);
        assert_eq!(b[1].line_start, 4, "行号取段落首行");
    }

    #[test]
    fn runs_of_blank_lines_do_not_produce_empty_blocks() {
        assert_eq!(chunk("甲\n\n\n\n乙\n", 1).len(), 2);
    }

    #[test]
    fn an_empty_body_yields_no_blocks() {
        assert!(chunk("", 1).is_empty());
        assert!(chunk("\n\n\n", 1).is_empty());
    }

    /// A multi-line paragraph must fold into one block spanning its first and
    /// last line, not one block per line — the same shape `line_start`/
    /// `line_end` promise for every other chunker in this crate.
    #[test]
    fn a_multi_line_paragraph_is_one_block_spanning_its_lines() {
        let b = chunk("line one\nline two\nline three\n", 1);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].line_start, 1);
        assert_eq!(b[0].line_end, 3);
        assert_eq!(b[0].text, "line one\nline two\nline three");
    }

    /// `body_start_line` offsets every emitted line, matching the contract
    /// every other chunker in this crate honors (frontmatter precedes the
    /// body).
    #[test]
    fn body_start_line_offsets_every_block() {
        let b = chunk("para\n", 4);
        assert_eq!(b[0].line_start, 4);
        assert_eq!(b[0].line_end, 4);
    }

    #[test]
    fn breadcrumb_and_level_are_line_with_no_breadcrumb() {
        let b = chunk("just text\n", 1);
        assert_eq!(b[0].level, BlockLevel::Line);
        assert_eq!(b[0].breadcrumb, "");
    }
}
