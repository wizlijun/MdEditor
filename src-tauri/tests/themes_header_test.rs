use notemd_lib::themes::header::{parse_header, ParsedHeader};

#[test]
fn parses_full_header() {
    let css = "/*\n * Theme Name: Claude-Like\n * Author: anonymous\n * Version: 1.0.0\n * Appearance: light\n * Description: Warm paper.\n */\n:root { --bg: #fff; }";
    let h = parse_header(css);
    assert_eq!(h.name.as_deref(), Some("Claude-Like"));
    assert_eq!(h.author.as_deref(), Some("anonymous"));
    assert_eq!(h.version.as_deref(), Some("1.0.0"));
    assert_eq!(h.appearance.as_deref(), Some("light"));
    assert_eq!(h.description.as_deref(), Some("Warm paper."));
}

#[test]
fn returns_empty_when_no_header() {
    let css = ":root { --bg: #fff; }";
    let h = parse_header(css);
    assert!(h.name.is_none());
    assert!(h.appearance.is_none());
}

#[test]
fn case_insensitive_keys() {
    let css = "/*\n * THEME NAME: Foo\n * appearance: DARK\n */\nbody {}";
    let h = parse_header(css);
    assert_eq!(h.name.as_deref(), Some("Foo"));
    assert_eq!(h.appearance.as_deref(), Some("DARK"));
}

#[test]
fn ignores_lines_without_colon() {
    let css = "/*\n * Hello, world!\n * Theme Name: X\n */";
    let h = parse_header(css);
    assert_eq!(h.name.as_deref(), Some("X"));
}

#[test]
fn trims_whitespace_around_value() {
    let css = "/*\n * Theme Name:    Spacey   \n */";
    let h = parse_header(css);
    assert_eq!(h.name.as_deref(), Some("Spacey"));
}

#[test]
fn only_first_comment_block_is_inspected() {
    let css = "/*\n * Theme Name: First\n */\n:root {}\n/*\n * Theme Name: Second\n */";
    let h = parse_header(css);
    assert_eq!(h.name.as_deref(), Some("First"));
}

#[test]
fn handles_bom_and_charset_before_header() {
    let css = "\u{FEFF}@charset \"UTF-8\";\n/*\n * Theme Name: X\n */";
    let h = parse_header(css);
    assert_eq!(h.name.as_deref(), Some("X"));
}

#[test]
fn handles_crlf_line_endings() {
    let css = "/*\r\n * Theme Name: X\r\n * Appearance: dark\r\n */";
    let h = parse_header(css);
    assert_eq!(h.name.as_deref(), Some("X"));
    assert_eq!(h.appearance.as_deref(), Some("dark"));
}

#[test]
fn no_comment_at_all_returns_empty() {
    let h = parse_header("");
    assert!(h.name.is_none());
    assert!(h.appearance.is_none());
    assert!(h.author.is_none());
}
