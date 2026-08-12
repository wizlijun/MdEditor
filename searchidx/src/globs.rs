//! User-designated glob patterns that decide which vault files count as raw
//! source material (spec: `.superpowers/sdd/2026-08-12-source-globs-and-
//! transcript-indexing/`, §4.1). Two consumers share this matcher: it
//! replaces the old "no frontmatter ⇒ source" proxy for `.md` files, and it
//! is also what decides whether a non-markdown text file (`.srt`/`.vtt`/
//! `.txt`) gets indexed at all.
//!
//! The syntax is deliberately tiny — `**`, `*`, literal segments; no brace
//! expansion, no regex — because the pattern string has to be
//! human-readable and reverse-engineerable from a sample path in a settings
//! UI. That is also why this hand-rolls the matcher instead of pulling in a
//! glob crate: a general-purpose library would need to be trimmed down to
//! this same tiny surface anyway, and `searchidx` must not grow its
//! dependency budget (nor gain a `tauri` dependency) for it.
//!
//! Matching is case-literal by design, not an oversight: this codebase has
//! shipped incidents where a vault directory's case was silently flipped,
//! and case-insensitive matching would paper over exactly that class of
//! bug. The settings UI is expected to warn when a pattern matches zero
//! files instead.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceGlobs {
    /// Normalized patterns, each pre-split on `/` into segments, ready for
    /// `matches_segments`. Empty when the raw pattern list was empty or
    /// every entry was unparseable (blank/whitespace-only).
    patterns: Vec<Vec<String>>,
    /// Precomputed by `parse` so `stamp()` is a cheap clone, not a re-sort
    /// on every call — callers may stamp on every index open.
    stamp: String,
}

/// Parses raw pattern strings (as typed into the settings UI, one per line)
/// into a `SourceGlobs`. Unparseable entries — blank or whitespace-only —
/// are dropped rather than making the whole list invalid, per the
/// consumer-side tolerance obligation the rest of this crate follows
/// (OKF §11 in spirit: don't reject on a partially-bad input).
pub fn parse(patterns: &[String]) -> SourceGlobs {
    let mut normalized: Vec<String> = patterns
        .iter()
        .map(|p| p.trim().trim_matches('/').to_string())
        .filter(|p| !p.is_empty())
        .collect();
    // Sorting here — not just at stamp time — means `patterns` and `stamp`
    // are derived from the exact same normalized list, so they can never
    // drift out of sync with each other.
    normalized.sort();

    let stamp = normalized.join("\n");
    let patterns = normalized
        .iter()
        .map(|p| p.split('/').map(String::from).collect())
        .collect();

    SourceGlobs { patterns, stamp }
}

impl SourceGlobs {
    /// `rel` is vault-relative and `/`-separated; the crate normalizes
    /// separators upstream, so this never sees `\`.
    pub fn matches(&self, rel: &str) -> bool {
        let path_segments: Vec<&str> = rel.split('/').collect();
        self.patterns
            .iter()
            .any(|pat| matches_segments(pat, &path_segments))
    }

    /// Canonical form for the index's `meta` table. Two pattern lists that
    /// differ only in order, surrounding whitespace, or leading/trailing
    /// slashes MUST produce the same stamp — a mismatch on index open
    /// triggers a full rebuild, and re-saving the settings page without
    /// changing anything semantically must not pay that cost.
    pub fn stamp(&self) -> String {
        self.stamp.clone()
    }

    /// An empty pattern set — the state on upgrade, before the user has
    /// configured anything — matches nothing. See `matches`'s doc: it is
    /// not "matches everything", which would reclassify an entire vault as
    /// raw source material on first run.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }
}

/// Segment-by-segment matcher. `**` consumes any number of path segments
/// (including zero); a bare pattern segment (possibly containing `*`) must
/// consume exactly one path segment. Recursing on slices this way — rather
/// than comparing prefixes as strings — is what makes `ebook/**` not match
/// `ebook2/...`: `"ebook"` as a *segment* only ever compares against the
/// whole first path segment, never against a prefix of a longer one. This
/// is the same `/`-boundary problem `scan.rs`'s exclude-dir matcher and
/// `origin.rs`'s old mirror-dir check solved with `format!("{dir}/")`; here
/// it falls out of the segment representation for free.
fn matches_segments(pat: &[String], path: &[&str]) -> bool {
    match pat.first().map(String::as_str) {
        None => path.is_empty(),
        Some("**") => (0..=path.len()).any(|i| matches_segments(&pat[1..], &path[i..])),
        Some(p) => match path.first() {
            Some(seg) if segment_matches(p, seg) => matches_segments(&pat[1..], &path[1..]),
            _ => false,
        },
    }
}

/// Matches `*` within a single segment (never crosses `/` — that is what
/// `matches_segments` already guarantees by construction, since each
/// element of `pat`/`path` is a segment, not a full path). Works on `char`s
/// rather than bytes so a `*` never lands mid-codepoint on non-ASCII path
/// segments (e.g. `三体`).
fn segment_matches(pat: &str, seg: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let s: Vec<char> = seg.chars().collect();

    fn go(p: &[char], s: &[char]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some('*') => (0..=s.len()).any(|i| go(&p[1..], &s[i..])),
            Some(c) => match s.first() {
                Some(sc) if sc == c => go(&p[1..], &s[1..]),
                _ => false,
            },
        }
    }

    go(&p, &s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(p: &[&str]) -> SourceGlobs {
        parse(&p.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_double_star_crosses_directory_levels() {
        let s = g(&["ebook/**"]);
        assert!(s.matches("ebook/a.md"));
        assert!(s.matches("ebook/三体/第一部/x.srt"));
        assert!(!s.matches("other/a.md"));
    }

    /// 前缀相似不得误命中 —— `ebook2/` 不是 `ebook/` 的子目录。
    /// 这是既有 sync_dir 匹配器踩过的同一个坑,那里靠显式补 `/` 边界解决。
    #[test]
    fn a_lookalike_prefix_does_not_match() {
        let s = g(&["ebook/**"]);
        assert!(!s.matches("ebook2/a.md"));
        assert!(!s.matches("my-ebook/a.md"));
        assert!(!s.matches("x/ebook/a.md"));
    }

    #[test]
    fn a_single_star_does_not_cross_a_level() {
        let s = g(&["clips/*.txt"]);
        assert!(s.matches("clips/a.txt"));
        assert!(!s.matches("clips/sub/a.txt"));
    }

    #[test]
    fn a_bare_double_star_matches_everything() {
        assert!(g(&["**/*.srt"]).matches("any/where/deep/a.srt"));
        assert!(!g(&["**/*.srt"]).matches("any/where/a.txt"));
    }

    /// 空列表匹配零个,不是匹配一切 —— 反过来会让首次升级时全库变原始资料。
    #[test]
    fn an_empty_set_matches_nothing() {
        assert!(!g(&[]).matches("a.md"));
        assert!(g(&[]).is_empty());
    }

    /// 无法解析的模式被丢弃而不是让整份列表失效(容忍义务)。
    #[test]
    fn an_unparseable_pattern_is_dropped_not_fatal() {
        let s = g(&["", "   ", "ebook/**"]);
        assert!(s.matches("ebook/a.md"), "合法的那条仍须生效");
    }

    /// 规范化:顺序、空白、首尾斜杠不同但语义相同的两份列表必须产出同一个
    /// 串。否则每次保存设置都会触发一次无谓的全库重建。
    #[test]
    fn the_stamp_is_order_and_whitespace_insensitive() {
        assert_eq!(g(&["b/**", "a/**"]).stamp(), g(&["  a/** ", "/b/**/"]).stamp());
        assert_ne!(g(&["a/**"]).stamp(), g(&["a/*"]).stamp());
    }
}
