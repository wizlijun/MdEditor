//! Roam inline syntax → note.md markdown. Ported from
//! `plugins-src/roam-import/src/lib/roam-import/syntax.ts`; keep the two in
//! step — the shared golden fixture (Task 7) is what catches drift.
use crate::outline::{fence_close_len, fence_open_len};
use regex::{Captures, Regex};
use std::sync::OnceLock;

/// Split on code (``` fences and `inline` spans): even indices are prose,
/// odd indices are code and must pass through untouched.
fn map_non_code(s: &str, f: impl Fn(&str) -> String) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?s)(```.*?```|`[^`\n]*`)").unwrap());
    let mut out = String::new();
    let mut last = 0;
    for m in re.find_iter(s) {
        out.push_str(&f(&s[last..m.start()]));
        out.push_str(m.as_str());
        last = m.end();
    }
    out.push_str(&f(&s[last..]));
    out
}

fn embed_bracket_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{\{\[\[embed\]\]:\s*\(\(([a-zA-Z0-9_-]+)\)\)\s*\}\}").unwrap())
}

fn embed_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{\{embed:\s*\(\(([a-zA-Z0-9_-]+)\)\)\s*\}\}").unwrap())
}

fn underscore_italic_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"__([^_\n](?:[^\n]*?[^_\n])?)__").unwrap())
}

fn hashtag_link_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"#\[\[([^\]\n]+)\]\]").unwrap())
}

fn iso_date_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([A-Za-z]+) (\d{1,2})(?:st|nd|rd|th), (\d{4})$").unwrap())
}

fn wikilink_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[\[([^\]\n]+)\]\]").unwrap())
}

fn reserved_prop_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(type|line|id|collapsed|created|updated|status|answered|by):: ").unwrap())
}

/// The bullet shape `parse_outline` recognizes, anchored exactly as its own
/// `bullet_pattern` is (`^((?:  )*)-(?: (.*))?$`) — an even number of leading
/// spaces, then `-`, then either a space or the end of the line.
///
/// The "end of the line" half is not optional here: an *empty* bullet is
/// written `- `, and that trailing space does not survive editors, formatters
/// or git hooks, so the parser accepts the bare `-` too. A Roam block holding
/// an empty shift-enter line is therefore the same hazard `- milk` is, and has
/// to be escaped the same way. (Only `-` exactly — `--`, `---` and `-dash` are
/// not bullets to the parser and must stay untouched.)
fn bullet_line_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?:  )*-(?: |$)").unwrap())
}

pub fn convert_inline(s: &str) -> String {
    map_non_code(s, |seg| {
        let seg = embed_bracket_pattern().replace_all(seg, "(($1))");
        let seg = embed_pattern().replace_all(&seg, "(($1))");
        let seg = seg.replace("{{[[TODO]]}}", "[ ]").replace("{{[[DONE]]}}", "[x]")
                     .replace("{{TODO}}", "[ ]").replace("{{DONE}}", "[x]");
        let seg = underscore_italic_pattern().replace_all(&seg, "*$1*");
        hashtag_link_pattern().replace_all(&seg, "[[$1]]").into_owned()
    })
}

const MONTHS: [(&str, &str); 12] = [
    ("january", "01"), ("february", "02"), ("march", "03"), ("april", "04"),
    ("may", "05"), ("june", "06"), ("july", "07"), ("august", "08"),
    ("september", "09"), ("october", "10"), ("november", "11"), ("december", "12"),
];

/// Roam's daily title ("August 15th, 2022") → "2022-08-15"; anything else None.
pub fn to_iso_date(target: &str) -> Option<String> {
    let caps = iso_date_pattern().captures(target)?;
    let mo = MONTHS.iter().find(|(n, _)| *n == caps[1].to_lowercase())?.1;
    let dd: u32 = caps[2].parse().ok()?;
    if !(1..=31).contains(&dd) { return None; }
    Some(format!("{}-{}-{:02}", &caps[3], mo, dd))
}

/// note.md only resolves ISO date links (`[[yyyy-MM-dd]]`), so English daily
/// titles must be rewritten or the link points at nothing.
pub fn normalize_date_links(s: &str) -> String {
    map_non_code(s, |seg| {
        wikilink_pattern()
            .replace_all(seg, |c: &Captures| match to_iso_date(&c[1]) {
                Some(iso) => format!("[[{iso}]]"),
                None => c[0].to_string(),
            })
            .into_owned()
    })
}

/// Neutralize the continuation lines `parse_outline` would read as structure
/// rather than as this block's own text:
///
/// * `key:: value` — a node *property*, which would be swallowed off the
///   block's content and (for `id::`) rewrite its identity.
/// * `  - text` — a *child bullet*. A Roam block holding a shift-enter list
///   (`shopping\n- milk\n- eggs`) is exactly this shape.
///
/// Both are fixed the same way, with one leading space: it renders
/// identically, and it makes the line stop matching. For a bullet that is
/// mechanical — `^((?:  )*)- ` needs an EVEN number of leading spaces, so an
/// odd count cannot match at any depth. Adding a space also never produces a
/// line either pattern matches, which is what makes the escape idempotent.
///
/// **Fence-aware, because the escape must not rewrite code the user pasted.**
/// When the block's FIRST line opens a fence, `parse_outline` switches to raw
/// mode and takes every following line verbatim until a closer at least as
/// long — nothing in there can be misread as structure, so nothing in there
/// may be touched. A Roam block that is a fenced YAML sample would otherwise
/// come out of the sync with a space silently inserted into its `- foo` lines.
/// Lines *after* that closer are read normally again, so they are escaped
/// again.
///
/// A fence opened on a *later* line is not raw mode — `parse_outline` only
/// ever enters it from a bullet's first line — so those lines stay escaped,
/// even though they look like code. That is not a nicety: without the escape
/// the block loses its `id::` to a phantom child and the merge re-creates it
/// on every sync. Round-trip fidelity wins over fence cosmetics; the
/// first-line case (the common one, and the one Roam's own code blocks
/// produce) is exact.
pub fn escape_structural_lines(s: &str) -> String {
    let prop = reserved_prop_pattern();
    let bullet = bullet_line_pattern();
    // >0 = inside the raw fence the first line opened, exactly as
    // `parse_outline` tracks it (same two helpers, so the rule cannot drift).
    let mut fence = 0usize;
    let mut out: Vec<String> = Vec::new();
    for (i, ln) in s.split('\n').enumerate() {
        if i == 0 {
            // The first line IS the bullet's own text; it is never structure.
            fence = fence_open_len(ln).unwrap_or(0);
            out.push(ln.to_string());
        } else if fence > 0 {
            if fence_close_len(ln).is_some_and(|close| close >= fence) {
                fence = 0;
            }
            out.push(ln.to_string());
        } else if prop.is_match(ln) || bullet.is_match(ln) {
            out.push(format!(" {ln}"));
        } else {
            out.push(ln.to_string());
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_and_done_become_checkboxes() {
        assert_eq!(convert_inline("{{[[TODO]]}} buy milk"), "[ ] buy milk");
        assert_eq!(convert_inline("{{DONE}} shipped"), "[x] shipped");
    }

    #[test]
    fn embeds_collapse_to_block_refs() {
        assert_eq!(convert_inline("{{[[embed]]: ((abc123))}}"), "((abc123))");
        assert_eq!(convert_inline("{{embed: ((abc123))}}"), "((abc123))");
    }

    #[test]
    fn roam_underscore_italics_become_asterisks() {
        assert_eq!(convert_inline("__slanted__"), "*slanted*");
    }

    #[test]
    fn hashtag_page_links_become_plain_wikilinks() {
        assert_eq!(convert_inline("#[[Hemory]] note"), "[[Hemory]] note");
    }

    #[test]
    fn code_spans_are_left_alone() {
        assert_eq!(convert_inline("`__x__` and __y__"), "`__x__` and *y*");
        assert_eq!(convert_inline("```\n{{TODO}}\n```"), "```\n{{TODO}}\n```");
    }

    #[test]
    fn english_daily_titles_become_iso() {
        assert_eq!(to_iso_date("August 15th, 2022").as_deref(), Some("2022-08-15"));
        assert_eq!(to_iso_date("March 1st, 2026").as_deref(), Some("2026-03-01"));
        assert_eq!(to_iso_date("Hemory"), None);
    }

    #[test]
    fn date_links_are_normalized_but_other_links_are_not() {
        assert_eq!(
            normalize_date_links("see [[August 15th, 2022]] and [[Hemory]]"),
            "see [[2022-08-15]] and [[Hemory]]"
        );
    }

    #[test]
    fn continuation_lines_that_look_like_props_get_escaped() {
        // A second line reading `id:: x` would be eaten as a node property by
        // parse_outline; one leading space makes it content again.
        assert_eq!(escape_structural_lines("head\nid:: x"), "head\n id:: x");
        assert_eq!(escape_structural_lines("id:: x"), "id:: x");
    }

    #[test]
    fn continuation_lines_that_look_like_bullets_get_escaped() {
        // A Roam shift-enter list. Left alone, `- milk` is re-read as a CHILD
        // of the block, which then loses its own `id::` line to the child's
        // continuation indent — see convert::block_content.
        assert_eq!(escape_structural_lines("shopping\n- milk\n- eggs"), "shopping\n - milk\n - eggs");
        // Nested list items match the same pattern at any even indent.
        assert_eq!(escape_structural_lines("a\n  - b"), "a\n   - b");
        // The first line IS the bullet's text; it is never a child bullet.
        assert_eq!(escape_structural_lines("- milk"), "- milk");
        // A dash that is not a bullet (odd indent, or no trailing space) is
        // already unambiguous and must not be touched.
        assert_eq!(escape_structural_lines("a\n - b\n-dash\nx - y"), "a\n - b\n-dash\nx - y");
    }

    /// The escape must not edit code. Inside the fence the block's own first
    /// line opened, `parse_outline` takes every line verbatim, so there is
    /// nothing to neutralize — and a space inserted into a YAML sample's
    /// `- foo` is the user's pasted code, silently altered.
    #[test]
    fn lines_inside_the_blocks_own_fence_are_left_exactly_as_they_are() {
        assert_eq!(
            escape_structural_lines("```yaml\n- foo\n- bar\n```"),
            "```yaml\n- foo\n- bar\n```"
        );
        // Property-shaped lines are code in there too.
        assert_eq!(
            escape_structural_lines("```\nid:: not-a-property\n```"),
            "```\nid:: not-a-property\n```"
        );
        // Only a closer at least as long ends raw mode — a shorter run inside
        // a longer fence is still code.
        assert_eq!(
            escape_structural_lines("````\n```\n- inner\n````"),
            "````\n```\n- inner\n````"
        );
        // An unterminated fence runs to the end of the block.
        assert_eq!(escape_structural_lines("```js\n- not a bullet"), "```js\n- not a bullet");
    }

    /// …and once the fence closes, `parse_outline` is reading structure again,
    /// so the escape has to come back on.
    #[test]
    fn lines_after_the_fence_closes_are_escaped_again() {
        assert_eq!(
            escape_structural_lines("```\n- inside\n```\n- after\nid:: x"),
            "```\n- inside\n```\n - after\n id:: x"
        );
    }

    /// A fence opened on a LATER line is not raw mode — `parse_outline` only
    /// enters it from a bullet's first line — so those lines are still read as
    /// structure and must still be escaped, code-looking or not.
    #[test]
    fn a_fence_opened_mid_block_does_not_suspend_the_escape() {
        assert_eq!(
            escape_structural_lines("prose\n```yaml\n- foo\n```"),
            "prose\n```yaml\n - foo\n```"
        );
    }

    /// The escape only ever ADDS a leading space, so the escaped line no longer
    /// matches the pattern that produced it — running it twice (a hand-edited
    /// file fed back through, say) cannot pile spaces up. Neither can the
    /// fence tracking: a leading space never turns a line into a fence
    /// opener/closer, so the second pass sees the same raw regions.
    #[test]
    fn escaping_an_already_escaped_line_changes_nothing() {
        for s in [
            "head\nid:: x\n- milk",
            "```yaml\n- foo\n```\n- after",
            "prose\n```\n- foo\n```",
        ] {
            let once = escape_structural_lines(s);
            assert_eq!(escape_structural_lines(&once), once, "not idempotent: {s:?}");
        }
        assert_eq!(escape_structural_lines("head\nid:: x\n- milk"), "head\n id:: x\n - milk");
    }
}
