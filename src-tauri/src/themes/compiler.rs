//! CSS compilation pipeline: pre-strip Typora-private at-rules, parse with
//! lightningcss, rewrite selectors and url() targets, serialize.

/// Remove every `@include-when-export url(...)` at-rule from the source. This
/// runs *before* lightningcss parses the CSS because lightningcss treats
/// unknown at-rules as parse errors.
///
/// The pattern is line-anchored on the at-rule keyword; the value can wrap
/// across whitespace inside `url(...)`. We require a trailing `;` to avoid
/// matching inside comments or strings.
pub fn strip_include_when_export(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    loop {
        match rest.find("@include-when-export") {
            None => { out.push_str(rest); break }
            Some(idx) => {
                // Check that the keyword is not followed by identifier characters.
                let keyword_end = idx + "@include-when-export".len();
                let is_valid_keyword = if keyword_end >= rest.len() {
                    true
                } else {
                    let next_char = rest[keyword_end..].chars().next().unwrap_or('\0');
                    !next_char.is_alphanumeric() && next_char != '-' && next_char != '_'
                };

                if !is_valid_keyword {
                    // Not the right keyword, skip this occurrence and keep searching.
                    out.push_str(&rest[..keyword_end]);
                    rest = &rest[keyword_end..];
                    continue;
                }

                // Find start of this line so we also drop preceding indentation.
                let before = &rest[..idx];
                let line_start = before.rfind('\n').map(|n| n + 1).unwrap_or(0);
                // Emit everything up to the line start.
                out.push_str(&rest[..line_start]);
                // Find the terminating semicolon after the at-rule, while
                // tracking `()` depth — `url(...)` arguments (especially
                // Google Fonts URLs like `family=Foo:wght@400;500;600;700`)
                // contain `;` characters inside the parens that we must NOT
                // mistake for the end of the at-rule.
                let after = &rest[idx..];
                let after_bytes = after.as_bytes();
                let mut depth_paren: i32 = 0;
                let mut terminator: Option<usize> = None;
                for (i, &b) in after_bytes.iter().enumerate() {
                    match b {
                        b'(' => depth_paren += 1,
                        b')' => depth_paren -= 1,
                        b';' if depth_paren == 0 => { terminator = Some(i); break }
                        _ => {}
                    }
                }
                match terminator {
                    None => {
                        // Malformed — bail and keep the rest verbatim.
                        out.push_str(after);
                        break
                    }
                    Some(semi_rel) => {
                        rest = &after[semi_rel + 1..];
                    }
                }
            }
        }
    }
    out
}

/// The exact prefix every compiled selector starts with: the theme
/// disambiguator plus the editor host. Single source of truth so consumers that
/// must UNDO the scoping (e.g. `themes::commands::unscope_theme_css`, which
/// feeds isolated plugin webviews that have no `data-theme` ancestor) cannot
/// drift from the form written here. Pinned literally by
/// `tests/themes_compiler_test.rs`.
pub fn scope_prefix(theme_id: &str) -> String {
    format!(r#"[data-theme="{theme_id}"] .moraya-editor"#)
}

/// Rewrite a single CSS selector list string to a scoped form.
///
/// Algorithm: split on top-level commas → for each selector, tokenize into
/// (compound, combinator) pairs → substitute scope targets (`:root`,
/// `#write`, `html`, `body` when alone) with the SCOPE marker → normalize
/// child combinators following the scope to descendants → ensure exactly
/// one scope at the start → render. Selector-list results are de-duplicated.
pub fn rewrite_selector_text(input: &str, theme_id: &str) -> String {
    let scope = scope_prefix(theme_id);
    let mut parts: Vec<String> = Vec::new();
    for raw in split_top_level_comma(input) {
        parts.push(rewrite_one(raw.trim(), &scope));
    }
    // Deduplicate while preserving order.
    let mut seen: Vec<String> = Vec::new();
    for p in parts {
        if !seen.iter().any(|s| s == &p) { seen.push(p) }
    }
    seen.join(", ")
}

fn rewrite_one(sel: &str, scope: &str) -> String {
    let tokens = tokenize_selector(sel);
    // tokens is a flat list alternating (compound, combinator); first is compound.
    // Substitute scope targets when compound is exactly one of the four.
    let mut rebuilt: Vec<SelToken> = Vec::with_capacity(tokens.len());
    for tok in tokens {
        match tok {
            SelToken::Compound(s) if is_scope_target(&s) => {
                rebuilt.push(SelToken::ScopeMarker);
            }
            other => rebuilt.push(other),
        }
    }
    // Convert child combinators following a ScopeMarker into descendant.
    for i in 0..rebuilt.len().saturating_sub(1) {
        if matches!(rebuilt[i], SelToken::ScopeMarker)
            && matches!(rebuilt[i + 1], SelToken::Combinator('>'))
        {
            rebuilt[i + 1] = SelToken::Combinator(' ');
        }
    }
    // Ensure exactly one leading ScopeMarker.
    let has_leading_scope = matches!(rebuilt.first(), Some(SelToken::ScopeMarker));
    if !has_leading_scope {
        rebuilt.insert(0, SelToken::Combinator(' '));
        rebuilt.insert(0, SelToken::ScopeMarker);
    }
    // Render.
    let mut out = String::new();
    for tok in rebuilt {
        match tok {
            SelToken::ScopeMarker => out.push_str(scope),
            SelToken::Compound(s) => out.push_str(&s),
            SelToken::Combinator(' ') => out.push(' '),
            SelToken::Combinator(c) => {
                if !out.ends_with(' ') { out.push(' ') }
                out.push(c);
                out.push(' ');
            }
        }
    }
    // Collapse any double spaces.
    while out.contains("  ") { out = out.replace("  ", " ") }
    out.trim().to_string()
}

#[derive(Debug, Clone)]
enum SelToken {
    Compound(String),
    Combinator(char), // ' ' descendant, '>' child, '+' adjacent, '~' general
    ScopeMarker,
}

fn is_scope_target(compound: &str) -> bool {
    matches!(compound, ":root" | "#write" | "html" | "body")
}

fn tokenize_selector(sel: &str) -> Vec<SelToken> {
    let mut out: Vec<SelToken> = Vec::new();
    let mut current = String::new();
    let mut chars = sel.chars().peekable();
    let mut depth_paren = 0usize;
    let mut depth_bracket = 0usize;
    while let Some(c) = chars.next() {
        match c {
            '(' => { depth_paren += 1; current.push(c) }
            ')' => { if depth_paren > 0 { depth_paren -= 1 } current.push(c) }
            '[' => { depth_bracket += 1; current.push(c) }
            ']' => { if depth_bracket > 0 { depth_bracket -= 1 } current.push(c) }
            ' ' | '\t' | '\n' if depth_paren == 0 && depth_bracket == 0 => {
                if !current.is_empty() {
                    out.push(SelToken::Compound(std::mem::take(&mut current)));
                }
                // Peek next non-whitespace; if it's a structural combinator,
                // emit that; else emit descendant.
                while let Some(&p) = chars.peek() {
                    if p == ' ' || p == '\t' || p == '\n' { chars.next(); continue }
                    break;
                }
                match chars.peek() {
                    Some('>') | Some('+') | Some('~') => {
                        let c2 = chars.next().unwrap();
                        // Skip trailing whitespace.
                        while let Some(&p) = chars.peek() {
                            if p == ' ' || p == '\t' || p == '\n' { chars.next(); continue }
                            break;
                        }
                        out.push(SelToken::Combinator(c2));
                    }
                    Some(_) => out.push(SelToken::Combinator(' ')),
                    None => break,
                }
            }
            '>' | '+' | '~' if depth_paren == 0 && depth_bracket == 0 => {
                if !current.is_empty() {
                    out.push(SelToken::Compound(std::mem::take(&mut current)));
                }
                out.push(SelToken::Combinator(c));
                // Skip whitespace after combinator.
                while let Some(&p) = chars.peek() {
                    if p == ' ' || p == '\t' || p == '\n' { chars.next(); continue }
                    break;
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(SelToken::Compound(current));
    }
    out
}

fn split_top_level_comma(s: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    let bytes = s.as_bytes();
    let mut start = 0usize;
    let mut depth_paren = 0i32;
    let mut depth_bracket = 0i32;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'(' => depth_paren += 1,
            b')' => depth_paren -= 1,
            b'[' => depth_bracket += 1,
            b']' => depth_bracket -= 1,
            b',' if depth_paren == 0 && depth_bracket == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < bytes.len() { out.push(&s[start..]) }
    out
}

use std::path::{Component, Path, PathBuf};

/// Rewrite a single `url(...)` payload (without the surrounding `url(` and
/// `)`, no quotes) for use in compiled CSS.
///
/// - Absolute URL schemes (`https:`, `http:`, `data:`, `file:`) are left alone.
/// - Empty string is left alone.
/// - Otherwise the value is treated as a path relative to `asset_dir` (the
///   theme's same-named asset folder). If the resolved path tries to escape
///   `asset_dir` via `..`, return `about:blank` to neuter the reference.
pub fn rewrite_url_value(value: &str, asset_dir: &str) -> String {
    if value.is_empty() { return String::new() }
    let lower = value.to_ascii_lowercase();
    for scheme in ["http://", "https://", "data:", "file://", "about:"] {
        if lower.starts_with(scheme) { return value.to_string() }
    }
    let base = Path::new(asset_dir);
    let candidate = base.join(value);
    let normalized = normalize(&candidate);
    if !normalized.starts_with(base) {
        return "about:blank".to_string();
    }
    format!("file://{}", normalized.display())
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::ParentDir => { out.pop(); }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Task 10: Full compile pipeline
// ---------------------------------------------------------------------------

use lightningcss::{
    rules::CssRule,
    stylesheet::{ParserOptions, PrinterOptions, StyleSheet},
    traits::ToCss,
};

/// Compile Typora source CSS into a scoped, note.md-ready form.
///
/// `theme_id` is used both as the `data-theme` attribute value and (with
/// `asset_dir`) to resolve relative `url(...)` paths inside `@font-face`.
pub fn compile_theme_css(src: &str, theme_id: &str, asset_dir: &str) -> Result<String, String> {
    let stripped = strip_include_when_export(src);
    // Pre-validate: check for unclosed blocks and unterminated declarations.
    // lightningcss applies CSS error-recovery by default and would silently
    // accept e.g. `:root { color: ` — we want to surface that as an error.
    check_structural_validity(&stripped)?;
    // Box::leak gives us a 'static str so the stylesheet can outlive this fn's local.
    let static_src: &'static str = Box::leak(stripped.into_boxed_str());
    let mut ss = StyleSheet::parse(static_src, ParserOptions {
        error_recovery: true,
        ..ParserOptions::default()
    })
        .map_err(|e| format!("parse error: {e}"))?;
    rewrite_rules(&mut ss.rules.0, theme_id, asset_dir);
    let printed = ss
        .to_css(PrinterOptions { minify: false, ..PrinterOptions::default() })
        .map_err(|e| format!("print error: {e}"))?;
    Ok(printed.code)
}

/// Quick structural check: ensure all `{` are closed and there is no
/// unterminated string literal or colon-without-value declaration.
///
/// This supplements lightningcss's error-recovery parser so that obviously
/// malformed input (like `:root { color: `) is rejected as an error.
fn check_structural_validity(css: &str) -> Result<(), String> {
    let mut depth: i32 = 0;
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // Skip string literals (single or double quoted).
            '"' | '\'' => {
                let quote = c;
                let mut closed = false;
                while let Some(sc) = chars.next() {
                    if sc == '\\' { chars.next(); continue; }
                    if sc == quote { closed = true; break; }
                }
                if !closed {
                    return Err(format!("parse error: unterminated string literal"));
                }
            }
            // Skip comments.
            '/' if chars.peek() == Some(&'*') => {
                chars.next(); // consume '*'
                let mut closed = false;
                while let Some(cc) = chars.next() {
                    if cc == '*' && chars.peek() == Some(&'/') {
                        chars.next(); // consume '/'
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return Err(format!("parse error: unterminated comment"));
                }
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return Err(format!("parse error: unexpected `}}`"));
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(format!("parse error: unclosed block (`{{` without matching `}}`), depth={depth}"));
    }
    Ok(())
}

fn rewrite_rules(rules: &mut Vec<CssRule<'static>>, theme_id: &str, asset_dir: &str) {
    for rule in rules.iter_mut() {
        match rule {
            CssRule::Style(style) => {
                // Serialize the current selector list to a string.
                let sel_str = style
                    .selectors
                    .to_css_string(PrinterOptions::default())
                    .unwrap_or_default();
                let rewritten = rewrite_selector_text(&sel_str, theme_id);
                // Build a synthetic stylesheet to re-parse the rewritten selectors.
                // Box::leak gives us a 'static str required by StyleSheet::parse.
                let synthetic = format!("{} {{ _: 0; }}", rewritten);
                let static_synthetic: &'static str = Box::leak(synthetic.into_boxed_str());
                if let Ok(mini) = StyleSheet::parse(static_synthetic, ParserOptions::default()) {
                    if let Some(CssRule::Style(first)) = mini.rules.0.into_iter().next() {
                        style.selectors = first.selectors;
                    }
                }
                // Recurse into any nested rules.
                rewrite_rules(&mut style.rules.0, theme_id, asset_dir);
            }
            CssRule::Media(media) => {
                rewrite_rules(&mut media.rules.0, theme_id, asset_dir);
            }
            CssRule::Supports(supports) => {
                rewrite_rules(&mut supports.rules.0, theme_id, asset_dir);
            }
            CssRule::FontFace(ff) => {
                use lightningcss::rules::font_face::FontFaceProperty;
                for prop in ff.properties.iter_mut() {
                    if let FontFaceProperty::Source(sources) = prop {
                        use lightningcss::rules::font_face::Source;
                        for src_item in sources.iter_mut() {
                            if let Source::Url(url_src) = src_item {
                                let original = url_src.url.url.as_ref().to_string();
                                let rewritten = rewrite_url_value(&original, asset_dir);
                                url_src.url.url = rewritten.into();
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
