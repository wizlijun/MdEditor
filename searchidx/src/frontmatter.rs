//! Tolerant, shallow frontmatter reading.
//!
//! OKF v0.2 §11 puts a *consumer* obligation on us: a missing optional field, an
//! unknown `type`, unknown extra keys, a broken block — none of them may cause
//! the document to be rejected. A strict YAML library does the opposite: it
//! raises on malformed input, which would make a typo delete a file from search.
//! So this is a hand-rolled reader for a fixed set of shallow keys, and every
//! failure path degrades to `None` rather than to an error.

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Frontmatter {
    pub concept_type: Option<String>,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub created: Option<String>,
    pub date: Option<String>,
    pub generated_at: Option<String>,
    /// True when a `verified` entry carries a `by:` with the OKF `human:` prefix.
    pub human_verified: bool,
}

/// `(raw_frontmatter, body, 1-based line number of the body's first line)`.
/// The delimiter must start at byte 0 — a `---` further down is a horizontal
/// rule, not a header.
pub fn split(text: &str) -> (Option<&str>, &str, u32) {
    if !text.starts_with("---\n") {
        return (None, text, 1);
    }
    let rest = &text[4..];
    let Some(end) = find_closing_delimiter(rest) else {
        return (None, text, 1);
    };
    let raw = rest[..end].trim_end_matches('\n');
    let after = &rest[end..];
    let body = after
        .strip_prefix("---\n")
        .unwrap_or_else(|| after.strip_prefix("---").unwrap_or(after));
    // 1 (opening ---) + frontmatter lines + 1 (closing ---) + 1 = first body line
    let fm_lines = rest[..end].matches('\n').count() as u32;
    (Some(raw), body, fm_lines + 3)
}

fn find_closing_delimiter(rest: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

pub fn parse(raw: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    let mut lines = raw.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((key, value)) = split_key(line) else {
            continue;
        };
        // Only column-0 keys are read; nested keys are consumed by their block.
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        match key {
            "type" => fm.concept_type = scalar(value),
            "title" => fm.title = scalar(value),
            "created" => fm.created = scalar(value),
            "date" => fm.date = scalar(value),
            "tags" => {
                fm.tags = if value.trim().is_empty() {
                    collect_block(&mut lines)
                        .into_iter()
                        .filter_map(|v| scalar(v.trim().trim_start_matches("- ")))
                        .collect()
                } else {
                    parse_inline_list(value)
                };
            }
            "generated" => {
                for entry in collect_block(&mut lines) {
                    if let Some(("at", v)) = split_key(entry.trim().trim_start_matches("- ")) {
                        fm.generated_at = scalar(v);
                    }
                }
            }
            "verified" => {
                for entry in collect_block(&mut lines) {
                    if let Some(("by", v)) = split_key(entry.trim().trim_start_matches("- ")) {
                        if scalar(v).is_some_and(|s| s.starts_with("human:")) {
                            fm.human_verified = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fm
}

/// Consume the indented (or `- `-prefixed) lines that belong to the key we just
/// read. Handles both `verified: {by: ...}` written as a bare mapping and as a
/// one-element list — OKF §11 says a bare mapping MUST be treated as a
/// single-element list, so both shapes land in the same `Vec`.
fn collect_block<'a>(lines: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(next) = lines.peek() {
        if next.trim().is_empty() || !next.starts_with(char::is_whitespace) {
            break;
        }
        out.push(lines.next().unwrap().to_string());
    }
    out
}

fn split_key(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once(':')?;
    let k = k.trim().trim_start_matches("- ").trim();
    if k.is_empty() || k.contains(char::is_whitespace) {
        return None;
    }
    Some((k, v))
}

fn scalar(value: &str) -> Option<String> {
    let v = value.trim().trim_matches('"').trim_matches('\'').trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

fn parse_inline_list(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(scalar)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_returns_body_and_its_first_line_number() {
        let (fm, body, line) = split("---\ntype: note\n---\nhello\n");
        assert_eq!(fm, Some("type: note"));
        assert_eq!(body, "hello\n");
        assert_eq!(line, 4);
    }

    #[test]
    fn split_without_frontmatter_starts_at_line_one() {
        let (fm, body, line) = split("# Title\n");
        assert_eq!(fm, None);
        assert_eq!(body, "# Title\n");
        assert_eq!(line, 1);
    }

    /// `---` 必须在第 0 字符。文中间出现的 `---` 是分隔线,不是 frontmatter。
    #[test]
    fn split_ignores_a_later_delimiter() {
        let (fm, _, line) = split("text\n---\na: b\n---\n");
        assert_eq!(fm, None);
        assert_eq!(line, 1);
    }

    #[test]
    fn parses_shallow_scalar_keys() {
        let f = parse("type: concept\ntitle: My Note\ncreated: 2026-08-10");
        assert_eq!(f.concept_type.as_deref(), Some("concept"));
        assert_eq!(f.title.as_deref(), Some("My Note"));
        assert_eq!(f.created.as_deref(), Some("2026-08-10"));
    }

    #[test]
    fn parses_inline_and_block_tag_lists() {
        assert_eq!(parse("tags: [a, b]").tags, vec!["a", "b"]);
        assert_eq!(parse("tags:\n  - a\n  - b").tags, vec!["a", "b"]);
        assert_eq!(parse("tags: a").tags, vec!["a"]);
    }

    #[test]
    fn reads_generated_at_from_the_nested_generated_block() {
        let f = parse("generated:\n  by: claude/1\n  at: 2026-08-01T10:00:00Z");
        assert_eq!(f.generated_at.as_deref(), Some("2026-08-01T10:00:00Z"));
    }

    /// OKF §7:人工确认必须用 `human:` 前缀。§11:裸 mapping 当单元素列表处理。
    #[test]
    fn human_verified_accepts_both_a_bare_mapping_and_a_list() {
        assert!(parse("verified:\n  by: human:me\n  at: 2026-08-01").human_verified);
        assert!(parse("verified:\n  - by: human:me").human_verified);
        assert!(!parse("verified:\n  - by: claude/1").human_verified);
        assert!(!parse("title: x").human_verified);
    }

    /// 宽容义务:坏 frontmatter 不得让文件消失。
    #[test]
    fn malformed_frontmatter_yields_empty_fields_not_an_error() {
        let f = parse("type: [unclosed\n\t\tgarbage: : :\n%%%");
        assert_eq!(f.title, None);
        assert!(f.tags.is_empty());
    }
}
