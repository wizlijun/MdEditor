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
    pub generated_by: Option<String>,
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
                for entry in entries_for(value, &mut lines) {
                    match split_key(entry.trim().trim_start_matches("- ")) {
                        Some(("at", v)) => fm.generated_at = scalar(v),
                        Some(("by", v)) => fm.generated_by = scalar(v),
                        _ => {}
                    }
                }
            }
            "verified" => {
                for entry in entries_for(value, &mut lines) {
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

/// The `key: value` entries belonging to a mapping-valued key, whichever of
/// YAML's two shapes the author used:
///
/// ```text
/// generated:                                    # block form
///   by: claude-code/opus-5
/// generated: { by: claude-code/opus-5 }         # flow form
/// verified: [{ by: human:bruce }]               # flow sequence of mappings
/// ```
///
/// The flow form is not an exotic case here: it is the **only** form
/// `src-tauri/templates/AGENTS.md` documents — the convention block written
/// into every vault's `AGENTS.md` shows `generated: { by, at }` and
/// `verified: [{ by, at }]` on one line, and the claude-agent plugin
/// (`plugins-src/claude-agent/backend/src/okf.rs`) stamps exactly that. Only
/// reading the block form meant `origin::derive`'s rules 2 and 3 silently
/// never fired for documents written to this project's own published
/// convention: agent output reached the `Human` tier through rule 4, and a
/// person's inline `human:` signature lost both its tier and its
/// `human_verified` boost.
///
/// Flow is only *preferred*, never assumed: the block form must keep working
/// for every key line the pre-flow reader accepted, because a silent
/// regression here lands agent output back in the `Human` tier — the failure
/// spec §3.2 calls the expensive one. So a non-empty value is tried as flow
/// and the block is taken instead whenever that yields nothing `split_key`
/// would accept. That covers the three legal shapes a bare emptiness test
/// gets wrong:
///
/// ```text
/// generated:  # stamped by the agent      <- trailing comment, block below
/// generated: {                            <- flow mapping wrapped over lines
///   by: claude-code/opus-5
/// }
/// generated: {}                           <- empty flow, block below
/// ```
///
/// Not consuming the following lines when flow *did* parse is safe — `parse`'s
/// main loop skips every whitespace-led line anyway, so nothing leaks into a
/// column-0 key.
fn entries_for<'a>(
    value: &str,
    lines: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>,
) -> Vec<String> {
    let inline = parse_inline_entries(strip_trailing_comment(value));
    if inline.iter().any(|e| split_key(e).is_some()) {
        return inline;
    }
    collect_block(lines)
}

/// Drop a trailing YAML comment so `generated:  # stamped by the agent` is
/// still recognised as the block form. Only a `#` that opens the line or
/// follows whitespace starts a comment, so a `#` inside a value survives.
fn strip_trailing_comment(value: &str) -> &str {
    let bytes = value.as_bytes();
    for (i, _) in value.char_indices().filter(|&(_, c)| c == '#') {
        if i == 0 || bytes[i - 1].is_ascii_whitespace() {
            return &value[..i];
        }
    }
    value
}

/// Split a flow mapping/sequence into the same flat `key: value` entry
/// strings [`collect_block`] produces, so both shapes share one consumer.
///
/// Flattening a sequence's mappings into one list is exactly what the block
/// form already does (`collect_block` returns every indented line of a
/// `- by:` list as one `Vec`), and it is what OKF §11 asks for from the other
/// direction: a bare mapping MUST be readable as a single-element list, so
/// `{ … }` and `[{ … }]` must not be distinguishable here.
///
/// Deliberately character-level, not a YAML parse: the braces/brackets are
/// only trimmed off each comma-separated part's ends, never removed from the
/// middle, and the `key: value` split is left to `split_key`, so a value
/// containing a colon survives exactly as it does in the block form. Every
/// unparseable shape degrades to an entry `split_key` rejects — it must never
/// error or panic (module doc comment; OKF §11's consumer obligation).
fn parse_inline_entries(value: &str) -> Vec<String> {
    let trim_delims = |c: char| matches!(c, '[' | ']' | '{' | '}') || c.is_whitespace();
    value
        .split(',')
        .map(|part| part.trim_matches(trim_delims).to_string())
        .filter(|part| !part.is_empty())
        .collect()
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

    #[test]
    fn reads_generated_by_alongside_generated_at() {
        let f = parse("generated:\n  by: claude/1\n  at: 2026-08-01T10:00:00Z");
        assert_eq!(f.generated_by.as_deref(), Some("claude/1"));
        assert_eq!(f.generated_at.as_deref(), Some("2026-08-01T10:00:00Z"));
    }

    /// OKF §7:人工撰写或人工确认必须用 `human:` 前缀。原样保留,
    /// 由 origin 推导去判断前缀 —— 解析层不做语义。
    #[test]
    fn a_human_generated_by_is_preserved_verbatim() {
        assert_eq!(parse("generated:\n  by: human:bruce").generated_by.as_deref(), Some("human:bruce"));
    }

    #[test]
    fn generated_by_is_none_when_absent() {
        assert_eq!(parse("title: x").generated_by, None);
        assert_eq!(parse("generated:\n  at: 2026-01-01").generated_by, None);
    }

    /// OKF §7:人工确认必须用 `human:` 前缀。§11:裸 mapping 当单元素列表处理。
    #[test]
    fn human_verified_accepts_both_a_bare_mapping_and_a_list() {
        assert!(parse("verified:\n  by: human:me\n  at: 2026-08-01").human_verified);
        assert!(parse("verified:\n  - by: human:me").human_verified);
        assert!(!parse("verified:\n  - by: claude/1").human_verified);
        assert!(!parse("title: x").human_verified);
    }

    // --- flow (inline) forms of `generated:` / `verified:` -----------------
    //
    // `src-tauri/templates/AGENTS.md` — the convention block written into every
    // vault — documents these keys ONLY in flow form (`generated: { by, at }`,
    // `verified: [{ by, at }]`), and `plugins-src/claude-agent/backend/src/
    // okf.rs` emits exactly that. Reading only the block form meant rules 2 and
    // 3 of `origin::derive` never fired for any document written to this
    // project's own published convention. One fixture per form below.

    /// Inline map — the literal shape of AGENTS.md's own example.
    #[test]
    fn reads_generated_from_an_inline_flow_mapping() {
        let f = parse("generated: { by: claude-code/opus-5, at: 2026-08-03T14:22:00Z }");
        assert_eq!(f.generated_by.as_deref(), Some("claude-code/opus-5"));
        assert_eq!(f.generated_at.as_deref(), Some("2026-08-03T14:22:00Z"));
    }

    /// Inline sequence, and inline bare mapping — OKF §11 says a bare
    /// `verified` mapping MUST be read as a single-element list, so both
    /// shapes have to reach the same place.
    #[test]
    fn human_verified_accepts_the_inline_flow_forms() {
        assert!(parse("verified: [{ by: human:bruce, at: 2026-08-03T14:22:00Z }]").human_verified);
        assert!(parse("verified: { by: human:bruce, at: 2026-08-03T14:22:00Z }").human_verified);
        assert!(parse("verified: [{ by: claude/1 }, { by: human:bruce }]").human_verified);
        assert!(!parse("verified: [{ by: claude/1 }]").human_verified);
        assert!(!parse("verified: { by: claude/1 }").human_verified);
    }

    /// Block form (contrast) — the shape that already worked must keep
    /// working now that a non-empty value on the key's own line takes a
    /// different branch.
    #[test]
    fn the_block_form_still_reads_the_same_values_as_the_inline_form() {
        let block = parse("generated:\n  by: claude-code/opus-5\n  at: 2026-08-03T14:22:00Z");
        let inline = parse("generated: { by: claude-code/opus-5, at: 2026-08-03T14:22:00Z }");
        assert_eq!(block.generated_by, inline.generated_by);
        assert_eq!(block.generated_at, inline.generated_at);
        assert!(parse("verified:\n  - by: human:me").human_verified);
    }

    /// Reading flow must not cost us block. Three legal shapes put something
    /// after the colon while the real value is still on the following lines;
    /// treating "non-empty value" as "this is flow" abandoned the block and
    /// sent agent output back into `Human` — the direction spec §3.2 calls the
    /// expensive failure. Each case below is a document a person or an agent
    /// can plausibly write, so each is asserted separately rather than as one
    /// table: a shared loop would let one shape's regression hide in another's
    /// pass.
    #[test]
    fn a_block_form_survives_anything_that_follows_its_colon() {
        // A trailing YAML comment on the key line.
        let commented = parse("generated:  # stamped by the agent\n  by: claude-code/opus-5");
        assert_eq!(commented.generated_by.as_deref(), Some("claude-code/opus-5"));
        assert!(parse("verified:  # signed off\n  - by: human:me").human_verified);

        // A flow mapping the author wrapped across lines.
        let wrapped = parse("generated: {\n  by: claude-code/opus-5\n}");
        assert_eq!(wrapped.generated_by.as_deref(), Some("claude-code/opus-5"));

        // An empty flow mapping with the entries indented beneath it.
        let empty_flow = parse("generated: {}\n  by: claude-code/opus-5");
        assert_eq!(empty_flow.generated_by.as_deref(), Some("claude-code/opus-5"));
    }

    /// The comment stripper must not eat a `#` that is part of a value —
    /// only one opening the line or following whitespace starts a comment.
    #[test]
    fn a_hash_inside_an_inline_value_is_not_a_comment() {
        let fm = parse("generated: { by: agent#7/1.0 }");
        assert_eq!(fm.generated_by.as_deref(), Some("agent#7/1.0"));
    }

    /// 宽容义务 again, now for the flow forms: an unterminated brace, an empty
    /// mapping, a scalar where a mapping was expected — none may error or
    /// panic, all degrade to `None`/`false`.
    #[test]
    fn a_malformed_inline_form_yields_none_not_an_error() {
        for bad in [
            "generated: {",
            "generated: { by",
            "generated: {}",
            "generated: [{{{",
            "generated: not-a-mapping",
            "generated: [",
            "generated: ,,,",
        ] {
            let f = parse(bad);
            assert_eq!(f.generated_by, None, "{bad}");
            assert_eq!(f.generated_at, None, "{bad}");
        }
        for bad in ["verified: {", "verified: [{ human:me }]", "verified: []", "verified: human:me"] {
            assert!(!parse(bad).human_verified, "{bad}");
        }
    }

    /// Same rule the block form follows via `scalar()`/`split_key()`: only the
    /// FIRST colon separates key from value, so a value that itself contains a
    /// colon survives intact. `human:`-prefixed actors (OKF §7) depend on this.
    #[test]
    fn an_inline_value_may_contain_a_colon() {
        assert_eq!(parse("generated: { by: claude/1: x }").generated_by.as_deref(), Some("claude/1: x"));
        assert_eq!(parse("generated: { by: human:bruce }").generated_by.as_deref(), Some("human:bruce"));
    }

    /// 宽容义务:坏 frontmatter 不得让文件消失。
    #[test]
    fn malformed_frontmatter_yields_empty_fields_not_an_error() {
        let f = parse("type: [unclosed\n\t\tgarbage: : :\n%%%");
        assert_eq!(f.title, None);
        assert!(f.tags.is_empty());
    }
}
