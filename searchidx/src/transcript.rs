//! `.srt` / `.vtt` transcript chunking.
//!
//! A line-oriented scanner, not a subtitle-parsing crate: `searchidx` must not
//! depend on `tauri` and the format is simple enough (sequence number,
//! timecode, text, blank) that a hand-rolled scanner is both smaller and
//! easier to keep tolerant than a general-purpose library would be.
//!
//! ## Line classification
//!
//! Each line of the body is classified once, in this order:
//!
//! 1. Blank → cue/block boundary, never indexed.
//! 2. `WEBVTT` header (only meaningful as the file's first non-blank line,
//!    but matched wherever it appears — being lenient here costs nothing)
//!    → dropped.
//! 3. A cue sequence number (`.srt`): the trimmed line is non-empty and every
//!    character is an ASCII digit → dropped.
//! 4. A timecode line: contains `-->` (both `,` and `.` millisecond
//!    separators pass through this same check unmodified — the arrow is the
//!    only thing that matters) → dropped.
//! 5. A `.vtt` cue identifier: an arbitrary label line that sits between a
//!    blank line and a timecode line. It cannot be recognized by its own
//!    content (it's free text), only by position — so this is a *lookahead*:
//!    the line immediately after it must be a timecode line. Checking the
//!    line itself instead of looking ahead would silently eat the first line
//!    of real cue text on any `.srt`-style file with no identifiers, since
//!    "the line right after a blank line" is also just... the normal shape
//!    of every cue's text line.
//! 6. Anything else is text and is accumulated into the pending block.
//!
//! A malformed cue (e.g. `-> ` instead of `-->`) matches none of the special
//! cases above, so it falls through to "text" and is kept, never dropped and
//! never panics — the general tolerance obligation (see the module doc on
//! `frontmatter.rs`) applies here too: a scanner that rejected on a small
//! formatting slip would delete a transcript from search over a typo.
//!
//! ## Line numbers survive text-stripping
//!
//! The scanner walks `body.lines()` by index and derives each line's
//! *original* 1-based file line as `body_start_line + index` — the same
//! offset for every line, skipped or not. Skipped lines (sequence numbers,
//! timecodes, headers, identifiers) still consume an index, so the numbering
//! never drifts even though their text never reaches a block. A block's
//! `line_start`/`line_end` are the original line numbers of the first and
//! last *text* line folded into it — never the line the block happens to
//! start accumulating text nearest to, and never a body-relative number that
//! forgot to add `body_start_line`.
//!
//! ## Merging (`TARGET_CHARS`)
//!
//! A single cue is typically five to ten characters — far too small to be a
//! useful retrieval unit, and small enough that a sentence spanning two or
//! three cues could never match any one block's text. So consecutive cues
//! are merged into one block until the accumulated text reaches
//! [`TARGET_CHARS`]. The merge boundary is only ever evaluated at a blank
//! line (a cue boundary), never in the middle of a single cue's text, so a
//! multi-line cue can never be split across two blocks.

use crate::block::{Block, BlockLevel};

/// Target block size in `chars()` (not bytes — the corpus is largely
/// Chinese, where one character is 3 UTF-8 bytes but still one unit of
/// meaning). 200 is the same order of magnitude as a prose paragraph: small
/// enough that the `#Lnnn` anchor a hit points at stays a precise, readable
/// span; large enough that bm25 isn't inflated by a flood of tiny blocks the
/// way one-block-per-cue would produce. A lone cue runs five to ten
/// characters, so without this merge a sentence spanning two or three cues —
/// the ordinary case in a transcript — could never match any single block,
/// which defeats the main reason to index transcripts as prose at all.
pub const TARGET_CHARS: usize = 200;

/// Chunk a `.srt`/`.vtt` body. `body_start_line` is the 1-based line the body
/// begins on in the whole file (matches the signature of `prose::chunk` and
/// `outline::chunk` — dispatch in Task 5 picks between the three).
pub fn chunk(body: &str, body_start_line: u32) -> Vec<Block> {
    let lines: Vec<&str> = body.lines().collect();

    let mut blocks: Vec<Block> = Vec::new();
    let mut pending_text = String::new();
    let mut pending_start: Option<u32> = None;
    let mut pending_end: u32 = 0;
    // Was the previous non-boundary-consuming step a blank line? Only used to
    // decide whether the *current* line could be a `.vtt` cue identifier —
    // an identifier must directly follow a blank line.
    let mut prev_blank = true;

    let mut i = 0usize;
    while i < lines.len() {
        let line_no = body_start_line + i as u32;
        let trimmed = lines[i].trim();

        if trimmed.is_empty() {
            prev_blank = true;
            // Cue boundary: close the pending block once it has reached the
            // target size, rather than mid-cue.
            if pending_text.chars().count() >= TARGET_CHARS {
                flush(&mut blocks, &mut pending_text, &mut pending_start, pending_end);
            }
            i += 1;
            continue;
        }

        if trimmed.starts_with("WEBVTT") {
            prev_blank = false;
            i += 1;
            continue;
        }

        if is_cue_number(trimmed) {
            prev_blank = false;
            i += 1;
            continue;
        }

        if is_timecode(trimmed) {
            prev_blank = false;
            i += 1;
            continue;
        }

        // `.vtt` cue identifier: free text, recognizable only because the
        // line right after it is a timecode. Look ahead — do not guess from
        // this line's own content.
        if prev_blank && lines.get(i + 1).map(|next| is_timecode(next.trim())).unwrap_or(false) {
            prev_blank = false;
            i += 1;
            continue;
        }

        // Ordinary text line (or a malformed cue line that matched none of
        // the special cases above — kept, not dropped).
        prev_blank = false;
        if pending_start.is_none() {
            pending_start = Some(line_no);
        }
        if !pending_text.is_empty() {
            pending_text.push('\n');
        }
        pending_text.push_str(trimmed);
        pending_end = line_no;

        i += 1;
    }

    flush(&mut blocks, &mut pending_text, &mut pending_start, pending_end);
    blocks
}

fn is_cue_number(trimmed: &str) -> bool {
    !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit())
}

fn is_timecode(trimmed: &str) -> bool {
    trimmed.contains("-->")
}

/// Emit whatever the accumulator holds as a `Line` block and clear it.
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

    const SRT: &str = "\
1
00:00:01,000 --> 00:00:03,000
今天讲一个关于检索的问题

2
00:00:03,000 --> 00:00:06,000
它的难点不在存储

3
00:00:06,000 --> 00:00:09,000
而在判断什么值得留下
";

    /// 引用契约:时间码被剔出索引文本,但行号必须仍指向原文件的真实行。
    /// 第一条字幕的文本在第 3 行。
    #[test]
    fn line_numbers_point_at_the_original_text_lines() {
        let b = chunk(SRT, 1);
        assert_eq!(b[0].line_start, 3, "必须是文本行,不是序号行也不是时间码行");
    }

    #[test]
    fn timecodes_and_cue_numbers_are_not_indexed() {
        let joined = chunk(SRT, 1).iter().map(|b| b.text.clone()).collect::<String>();
        assert!(!joined.contains("00:00:01"), "时间码会污染分词");
        assert!(!joined.contains("-->"));
    }

    /// 单条字幕只有五到十个字,逐条成块会让块数暴涨,而且跨条的句子永远
    /// 匹配不上 —— 这是收录转写的主要动机之一。
    #[test]
    fn consecutive_cues_are_merged_so_a_sentence_can_span_them() {
        let b = chunk(SRT, 1);
        assert_eq!(b.len(), 1, "三条短字幕应合成一块,实际 {:?}", b);
        assert!(b[0].text.contains("难点不在存储"));
        assert!(b[0].text.contains("什么值得留下"));
        assert_eq!(b[0].line_end, 11, "块尾须是最后一条字幕的文本行");
    }

    /// WEBVTT 头与可选的 cue 标识行都不进索引。
    #[test]
    fn a_vtt_header_and_cue_identifier_are_skipped() {
        let vtt = "WEBVTT\n\nintro\n00:00:01.000 --> 00:00:03.000\n开场白\n";
        let b = chunk(vtt, 1);
        assert_eq!(b.len(), 1);
        assert!(!b[0].text.contains("WEBVTT"));
        assert!(!b[0].text.contains("intro"));
        assert_eq!(b[0].line_start, 5);
    }

    /// 容忍义务:格式异常的行按普通文本收,不报错、不 panic。
    #[test]
    fn a_malformed_cue_is_kept_as_text_rather_than_dropped() {
        let bad = "1\n00:00:01,000 -> 00:00:03,000\n内容还在\n";
        let b = chunk(bad, 1);
        assert!(b.iter().any(|x| x.text.contains("内容还在")));
    }
}
