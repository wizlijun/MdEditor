//! Roam inline syntax → note.md markdown. Ported from
//! `plugins-src/roam-import/src/lib/roam-import/syntax.ts`; keep the two in
//! step — the shared golden fixture (Task 7) is what catches drift.
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

fn re(p: &str) -> Regex { Regex::new(p).unwrap() }

pub fn convert_inline(s: &str) -> String {
    map_non_code(s, |seg| {
        let seg = re(r"\{\{\[\[embed\]\]:\s*\(\(([a-zA-Z0-9_-]+)\)\)\s*\}\}").replace_all(seg, "(($1))");
        let seg = re(r"\{\{embed:\s*\(\(([a-zA-Z0-9_-]+)\)\)\s*\}\}").replace_all(&seg, "(($1))");
        let seg = seg.replace("{{[[TODO]]}}", "[ ]").replace("{{[[DONE]]}}", "[x]")
                     .replace("{{TODO}}", "[ ]").replace("{{DONE}}", "[x]");
        let seg = re(r"__([^_\n](?:[^\n]*?[^_\n])?)__").replace_all(&seg, "*$1*");
        re(r"#\[\[([^\]\n]+)\]\]").replace_all(&seg, "[[$1]]").into_owned()
    })
}

const MONTHS: [(&str, &str); 12] = [
    ("january", "01"), ("february", "02"), ("march", "03"), ("april", "04"),
    ("may", "05"), ("june", "06"), ("july", "07"), ("august", "08"),
    ("september", "09"), ("october", "10"), ("november", "11"), ("december", "12"),
];

/// Roam's daily title ("August 15th, 2022") → "2022-08-15"; anything else None.
pub fn to_iso_date(target: &str) -> Option<String> {
    let caps = re(r"^([A-Za-z]+) (\d{1,2})(?:st|nd|rd|th), (\d{4})$").captures(target)?;
    let mo = MONTHS.iter().find(|(n, _)| *n == caps[1].to_lowercase())?.1;
    let dd: u32 = caps[2].parse().ok()?;
    if !(1..=31).contains(&dd) { return None; }
    Some(format!("{}-{}-{:02}", &caps[3], mo, dd))
}

/// note.md only resolves ISO date links (`[[yyyy-MM-dd]]`), so English daily
/// titles must be rewritten or the link points at nothing.
pub fn normalize_date_links(s: &str) -> String {
    map_non_code(s, |seg| {
        re(r"\[\[([^\]\n]+)\]\]")
            .replace_all(seg, |c: &Captures| match to_iso_date(&c[1]) {
                Some(iso) => format!("[[{iso}]]"),
                None => c[0].to_string(),
            })
            .into_owned()
    })
}

/// A continuation line shaped like a node property would be swallowed by
/// parse_outline. One leading space keeps it content (renders the same).
pub fn escape_reserved_props(s: &str) -> String {
    let prop = re(r"^(type|line|id|collapsed|created|updated|status|answered|by):: ");
    s.split('\n')
        .enumerate()
        .map(|(i, ln)| if i > 0 && prop.is_match(ln) { format!(" {ln}") } else { ln.to_string() })
        .collect::<Vec<_>>()
        .join("\n")
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
        assert_eq!(escape_reserved_props("head\nid:: x"), "head\n id:: x");
        assert_eq!(escape_reserved_props("id:: x"), "id:: x");
    }
}
