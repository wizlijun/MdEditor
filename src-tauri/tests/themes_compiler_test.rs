use notemd_lib::themes::compiler::strip_include_when_export;
use notemd_lib::themes::compiler::rewrite_selector_text;

#[test]
fn strips_basic_form() {
    let css = "@include-when-export url(https://fonts.example.com/x.css);\n:root {}";
    assert_eq!(strip_include_when_export(css), "\n:root {}");
}

#[test]
fn strips_with_extra_whitespace() {
    let css = "  @include-when-export   url(  https://x  )  ;\n.a {}";
    assert_eq!(strip_include_when_export(css), "\n.a {}");
}

#[test]
fn strips_when_url_contains_semicolons() {
    // Real-world case: Google Fonts URLs use `;` inside the query string
    // (CSS2 family/wght syntax). The stripper must skip past the closing
    // `)` of url(...) before looking for the terminating semicolon.
    let css = "@include-when-export url(https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@400;500;600;700);\nhtml { color: red; }";
    let out = strip_include_when_export(css);
    // The complete at-rule (and its url + url-internal semicolons) must be
    // gone; the next CSS rule must be intact.
    assert_eq!(out, "\nhtml { color: red; }", "stripped output: {out:?}");
}

#[test]
fn strips_multiple_occurrences() {
    let css = "@include-when-export url(a);\n.a {}\n@include-when-export url(b);\n.b {}";
    let out = strip_include_when_export(css);
    assert!(!out.contains("@include-when-export"));
    assert!(out.contains(".a"));
    assert!(out.contains(".b"));
}

#[test]
fn preserves_css_with_no_directive() {
    let css = ":root { --x: 1; }\n.a { color: red; }";
    assert_eq!(strip_include_when_export(css), css);
}

#[test]
fn does_not_strip_similarly_named_rules() {
    let css = "/* @include-when-export-comment */\n.a {}";
    assert_eq!(strip_include_when_export(css), css);
}

const SCOPE: &str = r#"[data-theme="x"] .moraya-editor"#;

#[test]
fn rewrites_root() {
    assert_eq!(rewrite_selector_text(":root", "x"), SCOPE);
}

#[test]
fn rewrites_write() {
    assert_eq!(rewrite_selector_text("#write", "x"), SCOPE);
}

#[test]
fn rewrites_html_and_body() {
    assert_eq!(rewrite_selector_text("html", "x"), SCOPE);
    assert_eq!(rewrite_selector_text("body", "x"), SCOPE);
}

#[test]
fn write_child_combinator_becomes_descendant() {
    assert_eq!(rewrite_selector_text("#write > h1", "x"), format!("{SCOPE} h1"));
    assert_eq!(rewrite_selector_text("#write>h1", "x"), format!("{SCOPE} h1"));
}

#[test]
fn write_descendant_unchanged_in_form() {
    assert_eq!(rewrite_selector_text("#write h1", "x"), format!("{SCOPE} h1"));
}

#[test]
fn prefixes_class_selector() {
    assert_eq!(
        rewrite_selector_text(".md-fences", "x"),
        format!("{SCOPE} .md-fences")
    );
}

#[test]
fn prefixes_compound_selector() {
    assert_eq!(
        rewrite_selector_text("a.external", "x"),
        format!("{SCOPE} a.external")
    );
}

#[test]
fn selector_list_each_element_prefixed() {
    let out = rewrite_selector_text("h1, h2, h3", "x");
    assert_eq!(out, format!("{SCOPE} h1, {SCOPE} h2, {SCOPE} h3"));
}

#[test]
fn selector_list_mixed_scope_and_other() {
    // `:root` rewrites to scope; `.foo` gets prefixed; dedupe identical results.
    let out = rewrite_selector_text(":root, .foo", "x");
    assert_eq!(out, format!("{SCOPE}, {SCOPE} .foo"));
}

#[test]
fn body_with_class_is_not_treated_as_scope() {
    // body.modal-open is a compound, not a bare body — prefix without replacement.
    assert_eq!(
        rewrite_selector_text("body.modal-open", "x"),
        format!("{SCOPE} body.modal-open")
    );
}

#[test]
fn scope_attribute_uses_id_verbatim() {
    let out = rewrite_selector_text("#write", "claude-like");
    assert_eq!(out, r#"[data-theme="claude-like"] .moraya-editor"#);
}

use notemd_lib::themes::compiler::compile_theme_css;

#[test]
fn end_to_end_minimal_theme() {
    let src = "/*\n * Theme Name: X\n */\n:root { --c: red; }\n#write h1 { color: var(--c); }";
    let out = compile_theme_css(src, "x", "/tmp/themes/x").expect("compile ok");
    assert!(out.contains(r#"[data-theme="x"] .moraya-editor"#));
    assert!(out.contains("--c: red"));
    assert!(out.contains("color: var(--c)"));
    assert!(!out.contains("#write"));
}

#[test]
fn end_to_end_strips_include_when_export() {
    let src = "@include-when-export url(https://x);\n:root {}";
    let out = compile_theme_css(src, "x", "/tmp/x").unwrap();
    assert!(!out.contains("@include-when-export"));
}

#[test]
fn end_to_end_preserves_media_print() {
    let src = "@media print { #write { color: black; } }";
    let out = compile_theme_css(src, "x", "/tmp/x").unwrap();
    assert!(out.contains("@media print"));
    assert!(out.contains(r#"[data-theme="x"] .moraya-editor"#));
}

#[test]
fn end_to_end_preserves_imports() {
    let src = "@import url(https://cdn.example.com/font.css);\n:root {}";
    let out = compile_theme_css(src, "x", "/tmp/x").unwrap();
    assert!(out.contains("@import"));
    assert!(out.contains("https://cdn.example.com/font.css"));
}

#[test]
fn end_to_end_rewrites_font_face_url() {
    let src = "@font-face { font-family: 'X'; src: url('./fonts/x.woff2') format('woff2'); }";
    let out = compile_theme_css(src, "claude-like", "/themes/claude-like").unwrap();
    assert!(out.contains("file:///themes/claude-like/fonts/x.woff2"));
}

#[test]
fn malformed_css_returns_err() {
    let src = ":root { color: ";  // unterminated
    let result = compile_theme_css(src, "x", "/tmp/x");
    assert!(result.is_err());
}

use notemd_lib::themes::compiler::rewrite_url_value;

#[test]
fn relative_url_resolves_against_asset_dir() {
    let out = rewrite_url_value("./fonts/x.woff2", "/Users/u/themes/claude-like");
    assert_eq!(out, "file:///Users/u/themes/claude-like/fonts/x.woff2");
}

#[test]
fn implicit_relative_url() {
    let out = rewrite_url_value("fonts/x.woff2", "/Users/u/themes/cl");
    assert_eq!(out, "file:///Users/u/themes/cl/fonts/x.woff2");
}

#[test]
fn parent_path_returns_safe_blank() {
    let out = rewrite_url_value("../escape.woff2", "/Users/u/themes/cl");
    assert_eq!(out, "about:blank");
    let out = rewrite_url_value("./a/../../b.woff2", "/Users/u/themes/cl");
    assert_eq!(out, "about:blank");
}

#[test]
fn https_url_is_left_alone() {
    let out = rewrite_url_value("https://cdn.example.com/x.woff2", "/Users/u/themes/cl");
    assert_eq!(out, "https://cdn.example.com/x.woff2");
}

#[test]
fn data_url_is_left_alone() {
    let out = rewrite_url_value("data:font/woff2;base64,AAAA", "/Users/u/themes/cl");
    assert_eq!(out, "data:font/woff2;base64,AAAA");
}

#[test]
fn empty_url_is_left_alone() {
    let out = rewrite_url_value("", "/Users/u/themes/cl");
    assert_eq!(out, "");
}

// ── Scope targets qualified by an ancestor ──────────────────────────────────
// Typora themes routinely gate rules on a body class (`.mac-os #write`,
// `#typora-source #write`). Those ancestors do not exist here, and the rewrite
// used to keep them *and* prepend the scope, yielding `SCOPE .mac-os SCOPE` —
// unmatchable, so the rule silently vanished. Hundreds per theme did, including
// the `caret-color` that left the editing caret invisible.

#[test]
fn ancestor_qualified_write_collapses_to_one_scope() {
    assert_eq!(rewrite_selector_text(".mac-os #write", "x"), SCOPE);
    assert_eq!(rewrite_selector_text("#typora-source #write", "x"), SCOPE);
}

#[test]
fn ancestor_qualified_write_keeps_its_descendants() {
    assert_eq!(rewrite_selector_text(".mac-os #write p", "x"), format!("{SCOPE} p"));
    assert_eq!(
        rewrite_selector_text(".mac-os #write > h1", "x"),
        format!("{SCOPE} h1"),
    );
}

#[test]
fn compound_body_ancestor_is_dropped_too() {
    // `body.mac-os` is not a bare scope target, so it stayed in place and killed
    // the rule just the same.
    assert_eq!(rewrite_selector_text("body.mac-os #write", "x"), SCOPE);
}

#[test]
fn rewritten_selectors_never_repeat_the_scope() {
    for sel in [".mac-os #write", "body.mac-os #write p", "html body #write", "#write"] {
        let out = rewrite_selector_text(sel, "x");
        assert_eq!(out.matches(".moraya-editor").count(), 1, "{sel} → {out}");
    }
}
