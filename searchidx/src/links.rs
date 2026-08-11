//! Link extraction. The rows are written now and read later: the backlink layer
//! keeps its own pipeline for this release (design spec §2 — one refactor at a
//! time), but a `links` table added later would force a full rebuild of every
//! user's index, so we pay the cheap write today.
//!
//! Wikilinks resolve BY FILENAME elsewhere in the product; we store the raw
//! target verbatim and leave resolution to the consumer.

use crate::block::Link;

/// Scan `body` line by line for `[[wiki]]` and `[md](targets)`, tagging each
/// with its 1-based line number (`body_start_line` is the line the body's
/// first line occupies in the whole file).
pub fn extract(body: &str, body_start_line: u32) -> Vec<Link> {
    let mut out = Vec::new();
    for (i, line) in body.lines().enumerate() {
        let line_no = body_start_line + i as u32;
        collect_wiki(line, line_no, &mut out);
        collect_md(line, line_no, &mut out);
    }
    out
}

/// `[[target]]` and `[[target|display]]`. All slice boundaries below come from
/// `find` on `line` (or from `line.len()`), so they always land on char
/// boundaries even for multi-byte (e.g. Chinese) content.
fn collect_wiki(line: &str, line_no: u32, out: &mut Vec<Link>) {
    let mut i = 0usize;
    while let Some(rel) = line[i..].find("[[") {
        let s = i + rel + 2;
        let Some(len) = line[s..].find("]]") else { break };
        let raw = &line[s..s + len];
        let target = raw.split('|').next().unwrap_or(raw).trim();
        if !target.is_empty() {
            out.push(Link { kind: "wiki".into(), target: target.to_string(), line: line_no });
        }
        i = s + len + 2;
        if i >= line.len() {
            break;
        }
    }
}

/// `[text](target)`, skipping `![alt](target)` images. All slice boundaries
/// below come from `find`/`len` on `line`, never from a fixed byte offset, so
/// they stay on char boundaries for multi-byte content.
fn collect_md(line: &str, line_no: u32, out: &mut Vec<Link>) {
    let mut i = 0usize;
    while let Some(rel) = line[i..].find("](") {
        let open = i + rel; // byte index of the `]` that opens `](`.
        // `![alt](...)` is an image, not a document link: find the `[` this
        // `]` closes and check whether it's immediately preceded by `!`.
        // `rfind` and `chars().next_back()` both return/operate on char
        // boundaries, so this never slices mid-codepoint.
        let is_image = line[..open]
            .rfind('[')
            .and_then(|b| line[..b].chars().next_back())
            .is_some_and(|c| c == '!');
        let s = open + 2;
        let Some(len) = line[s..].find(')') else { break };
        let target = line[s..s + len].split_whitespace().next().unwrap_or("").trim();
        if !is_image && !target.is_empty() {
            out.push(Link { kind: "md".into(), target: target.to_string(), line: line_no });
        }
        i = s + len + 1;
        if i > line.len() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_wikilinks_with_their_line() {
        let l = extract("see [[Target Page]] here\n", 1);
        assert_eq!(l, vec![Link { kind: "wiki".into(), target: "Target Page".into(), line: 1 }]);
    }

    /// 别名形式 `[[target|display]]` 的目标是竖线**前面**那半。
    #[test]
    fn wikilink_alias_keeps_only_the_target() {
        let l = extract("[[a/b|Display]]\n", 1);
        assert_eq!(l[0].target, "a/b");
    }

    #[test]
    fn extracts_markdown_links_but_not_images() {
        let l = extract("[text](./a.md)\n![img](./p.png)\n", 1);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].kind, "md");
        assert_eq!(l[0].target, "./a.md");
    }

    #[test]
    fn line_numbers_are_offset_by_body_start() {
        let l = extract("x\n[[T]]\n", 4);
        assert_eq!(l[0].line, 5);
    }

    #[test]
    fn a_body_without_links_yields_nothing() {
        assert!(extract("plain text [not a link\n", 1).is_empty());
    }

    /// A line can carry both a wikilink and a markdown link; neither collector
    /// should be confused by the other's syntax.
    #[test]
    fn a_line_with_both_a_wikilink_and_a_markdown_link_yields_both() {
        let l = extract("see [[Target]] and [text](./a.md)\n", 1);
        assert_eq!(l.len(), 2);
        assert!(l.iter().any(|x| x.kind == "wiki" && x.target == "Target"));
        assert!(l.iter().any(|x| x.kind == "md" && x.target == "./a.md"));
    }

    /// An image and a real link on the same line: only the real link counts.
    #[test]
    fn an_image_and_a_link_on_the_same_line_only_the_link_counts() {
        let l = extract("![alt](./p.png) then [text](./a.md)\n", 1);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].target, "./a.md");
    }

    /// `](` appearing inside inline code is not a link opener. We don't attempt
    /// full inline-code-span awareness (this is a lightweight line scanner, not
    /// a markdown parser), but a bare `](` with no following `)` must not panic
    /// and must not fabricate a link with a garbage target.
    #[test]
    fn a_bare_bracket_paren_inside_inline_code_does_not_panic() {
        let l = extract("see `foo](bar` in code\n", 1);
        // No closing `)` after the `](` inside the code span in this example,
        // so nothing should be extracted.
        assert!(l.is_empty());
    }

    /// Multi-byte (Chinese) content around link syntax must not panic on byte
    /// slicing.
    #[test]
    fn multibyte_content_around_links_does_not_panic() {
        let l = extract("中文内容 [[目标页]] 更多中文 [文字](./甲.md) 结尾\n", 1);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].target, "目标页");
        assert_eq!(l[1].target, "./甲.md");
    }
}
