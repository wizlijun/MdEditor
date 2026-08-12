//! `.srt` / `.vtt` transcript chunking.
//!
//! A line-oriented scanner, not a subtitle-parsing crate: `searchidx` must not
//! depend on `tauri` and the format is simple enough (sequence number,
//! timecode, text, blank) that a hand-rolled scanner is both smaller and
//! easier to keep tolerant than a general-purpose library would be.
//!
//! ## Line classification
//!
//! Each line of the body is classified once, in this order. Rules 3 and 4
//! only fire while `awaiting_timecode` is true for the current cue (reset to
//! true at every blank line, cleared the moment a real timecode — or
//! anything unrecognized — has been consumed) — see "Structural gating"
//! below for why that state exists.
//!
//! 1. Blank → cue/block boundary, never indexed. Resets `awaiting_timecode`.
//! 2. `WEBVTT` header (only meaningful as the file's first non-blank line,
//!    but matched wherever it appears — being lenient here costs nothing)
//!    → dropped.
//! 3. A cue sequence number (`.srt`): the trimmed line is non-empty, every
//!    character is an ASCII digit, AND the line immediately after it is
//!    itself a timecode line (rule 4's check, applied one line ahead) →
//!    dropped. The lookahead is load-bearing, not decoration — see
//!    "Structural gating".
//! 4. A timecode line: contains `-->` (both `,` and `.` millisecond
//!    separators pass through this same check unmodified — the arrow is the
//!    only thing that matters), evaluated only while `awaiting_timecode` →
//!    dropped, and clears `awaiting_timecode` for the rest of this cue.
//! 5. A `.vtt` cue identifier: an arbitrary label line that sits between a
//!    blank line and a timecode line. It cannot be recognized by its own
//!    content (it's free text), only by position — so this is a *lookahead*:
//!    the line immediately after it must be a timecode line. Checking the
//!    line itself instead of looking ahead would silently eat the first line
//!    of real cue text on any `.srt`-style file with no identifiers, since
//!    "the line right after a blank line" is also just... the normal shape
//!    of every cue's text line.
//! 6. Anything else is text and is accumulated into the pending block. Also
//!    clears `awaiting_timecode` — once a line fails every structural check
//!    above, the cue is either past its timecode or malformed, and either
//!    way nothing later in the same cue should be reinterpreted as
//!    structure again.
//!
//! A malformed cue (e.g. `-> ` instead of `-->`) matches none of the special
//! cases above, so it falls through to "text" and is kept, never dropped and
//! never panics — the general tolerance obligation (see the module doc on
//! `frontmatter.rs`) applies here too: a scanner that rejected on a small
//! formatting slip would delete a transcript from search over a typo.
//!
//! ## Structural gating (round 1 review: Important 1 and 2)
//!
//! The first version of this module checked rules 3 and 4 unconditionally,
//! anywhere in the file. Two real inputs broke that:
//!
//! - A cue whose entire spoken content is a bare number (a countdown, a
//!   score) read as an orphan `.srt` sequence number and the whole cue
//!   vanished — no error, nothing to grep for.
//! - Dialogue containing a literal `-->` (a transcribed technical talk
//!   saying "a arrow b") was read as a timecode and swallowed the same way.
//!
//! Both are well-formed transcript content, so silently dropping them broke
//! the tolerance obligation this module claims to uphold. The fix is one
//! piece of state, `awaiting_timecode`: true from the start of a cue (a
//! blank line) until this cue's real timecode — or, for a malformed cue,
//! the first unrecognizable line — has been consumed. Rules 3 and 4 are
//! only checked while it's true; once a cue is past its timecode, a bare
//! number or a stray `-->` inside its text is just text, forever, for the
//! rest of that cue. For rule 4 specifically this was a deliberate choice
//! between two options the review called out: add the guard, or keep the
//! loose `contains("-->")` check and document the accepted loss. The guard
//! was chosen — it was free (the state already had to exist for rule 3) and
//! strictly more correct, so there was no real tradeoff to accept.
//!
//! ## Accidental rescues (round 1 review: Minor 3)
//!
//! Two inputs produce the right answer today for reasons that are
//! coincidental, not guaranteed by the rules as stated — noted here so a
//! future simplification of one rule doesn't silently break the other:
//!
//! - A numeric `.vtt` cue identifier (some tools emit an incrementing
//!   integer id, the same shape as an `.srt` sequence number) is caught by
//!   rule 3 before rule 5's lookahead ever sees it — same skip outcome, but
//!   rule 5's own digit-shaped path is untested by construction. If rule 3
//!   is ever narrowed (e.g. to require `.srt`-only context), a numeric-id
//!   `.vtt` file could stop parsing correctly even though rule 5 looks like
//!   it should still catch it.
//! - A UTF-8 BOM prefixing the file's very first line defeats rule 3's
//!   `is_ascii_digit` check on that line (the BOM character is not a
//!   digit), but the line is then coincidentally rescued by rule 5's
//!   identifier lookahead (start-of-file counts as "directly after a blank"
//!   here, and the following line is still a timecode). Correct output, by
//!   accident rather than by a rule that says BOM-prefixed lines are
//!   handled.
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
    // True from a cue's start (a blank line) until its real timecode — or,
    // for a malformed cue, the first line that fails every structural check
    // — has been consumed. Rules 3 and 4 (sequence number / timecode) are
    // only evaluated while this is true; see the module doc's "Structural
    // gating" section for why (round 1 review: Important 1 and 2).
    let mut awaiting_timecode = true;

    let mut i = 0usize;
    while i < lines.len() {
        let line_no = body_start_line + i as u32;
        let trimmed = lines[i].trim();

        if trimmed.is_empty() {
            prev_blank = true;
            awaiting_timecode = true;
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

        if awaiting_timecode {
            if is_cue_number(trimmed, lines.get(i + 1).copied()) {
                prev_blank = false;
                i += 1;
                continue;
            }

            if is_timecode(trimmed) {
                prev_blank = false;
                awaiting_timecode = false;
                i += 1;
                continue;
            }

            // `.vtt` cue identifier: free text, recognizable only because
            // the line right after it is a timecode. Look ahead — do not
            // guess from this line's own content.
            if prev_blank && lines.get(i + 1).map(|next| is_timecode(next.trim())).unwrap_or(false) {
                prev_blank = false;
                i += 1;
                continue;
            }
        }

        // Ordinary text line: either this cue's timecode has already been
        // consumed (a bare number or a stray `-->` here is just text — the
        // whole point of the `awaiting_timecode` gate), or none of the
        // pre-timecode shapes matched (a malformed cue, kept verbatim
        // rather than dropped).
        prev_blank = false;
        awaiting_timecode = false;
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

/// A bare-digits line counts as an `.srt` sequence number only when `next`
/// (the line immediately after it) is itself a timecode — the same
/// structural precondition rule 5's identifier lookahead already uses.
/// Without this, a cue whose entire spoken content happens to BE a number
/// reads as an orphan sequence number and the whole cue silently vanishes
/// (round 1 review, Important 1).
fn is_cue_number(trimmed: &str, next: Option<&str>) -> bool {
    !trimmed.is_empty()
        && trimmed.chars().all(|c| c.is_ascii_digit())
        && next.map(|n| is_timecode(n.trim())).unwrap_or(false)
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

    /// Round 1 review, Important 1a: a cue whose ENTIRE text is a bare
    /// number (a spoken count, a score) must not be mistaken for an `.srt`
    /// sequence number and silently deleted. Here it is `awaiting_timecode`
    /// (already cleared by the real timecode one line above `42`) that does
    /// the saving, not `is_cue_number`'s own lookahead — see
    /// `a_digit_only_cue_with_no_timecode_at_all_is_kept_as_text` below for
    /// the narrower case that isolates the lookahead itself.
    #[test]
    fn a_cue_whose_entire_text_is_a_bare_number_is_kept() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\n42\n";
        let b = chunk(srt, 1);
        assert!(b.iter().any(|x| x.text.contains('4') && x.text.contains('2')), "{b:?}");
        assert!(!b.is_empty(), "the whole cue must not vanish");
    }

    /// Round 1 review, Important 1b: a bare number line *inside* a
    /// multi-line cue (after the real timecode has already been consumed)
    /// must survive too — `awaiting_timecode` (cleared by the real timecode
    /// two lines above `3`) is what protects this one; see the lookahead
    /// isolation test below for the case only `is_cue_number`'s own
    /// precondition can save.
    #[test]
    fn a_bare_number_inside_multiline_cue_text_survives() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\ncountdown\n3\nliftoff\n";
        let b = chunk(srt, 1);
        let joined = b.iter().map(|x| x.text.clone()).collect::<String>();
        assert!(joined.contains("countdown"));
        assert!(joined.contains('3'), "the bare '3' line must not be eaten: {b:?}");
        assert!(joined.contains("liftoff"));
    }

    /// Isolates `is_cue_number`'s own next-is-timecode lookahead from the
    /// `awaiting_timecode` state gate. The two tests above both put their
    /// bare number *after* a real timecode has already cleared
    /// `awaiting_timecode` — so the state gate alone protects them, and a
    /// mutation that deletes `is_cue_number`'s lookahead (leaving only "any
    /// digit line while awaiting_timecode") does not redden either one. This
    /// file has NO real timecode anywhere: every line is a bare digit until
    /// `awaiting_timecode` never gets cleared by rule 4, so without
    /// `is_cue_number`'s own lookahead every one of `5`/`4`/`3` keeps
    /// re-matching rule 3 and is deleted, leaving only `liftoff`.
    #[test]
    fn a_digit_only_cue_with_no_timecode_at_all_is_kept_as_text() {
        let malformed = "5\n4\n3\nliftoff\n";
        let b = chunk(malformed, 1);
        let joined = b.iter().map(|x| x.text.clone()).collect::<String>();
        assert!(joined.contains('5'), "{b:?}");
        assert!(joined.contains('4'), "{b:?}");
        assert!(joined.contains("liftoff"));
    }

    /// Round 1 review, Important 2: real dialogue containing a literal
    /// `-->` (e.g. a transcribed technical talk saying "a arrow b") must not
    /// be mistaken for a timecode once the cue's actual timecode has already
    /// been consumed. `is_timecode`'s bare `contains("-->")` is only ever
    /// consulted while `awaiting_timecode` is true for the current cue —
    /// chosen over documenting the loss, since the guard is free (the state
    /// already exists for Important 1) and strictly more correct.
    #[test]
    fn dialogue_text_containing_an_arrow_is_not_mistaken_for_a_timecode() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nthe pipeline is a --> b --> c\n";
        let b = chunk(srt, 1);
        let joined = b.iter().map(|x| x.text.clone()).collect::<String>();
        assert!(joined.contains("the pipeline is a --> b --> c"), "{b:?}");
    }
}
