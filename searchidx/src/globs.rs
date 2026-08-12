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

use std::collections::HashMap;

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
    let mut normalized: Vec<Vec<String>> = patterns
        .iter()
        .map(|p| p.trim().trim_matches('/').to_string())
        .filter(|p| !p.is_empty())
        .map(|p| collapse_double_star(p.split('/').map(String::from).collect()))
        .collect();
    // Sorting *and deduping* here — not just at stamp time — means
    // `patterns` and `stamp` are derived from the exact same normalized
    // list, so they can never drift out of sync with each other.
    //
    // Dedup matters because `matches` is unaffected by a repeated entry —
    // `["a/**", "a/**"]` behaves identically to `["a/**"]` — but without
    // it those two lists produced different stamps, and a stamp mismatch
    // on index open triggers a full rebuild. A pasted duplicate line (or a
    // UI round-trip that duplicates an entry) must not cost the user a
    // rebuild for a no-op edit. `dedup()` only removes *consecutive*
    // duplicates, which is why it runs after `sort()`.
    normalized.sort();
    normalized.dedup();

    let stamp = normalized
        .iter()
        .map(|segs| segs.join("/"))
        .collect::<Vec<_>>()
        .join("\n");

    SourceGlobs { patterns: normalized, stamp }
}

/// Collapses consecutive `**` segments into one. `a/**/**/b.md` and
/// `a/**/b.md` match exactly the same set of paths — both mean "zero or
/// more segments here" — so they must normalize to the same stamp too
/// (same class of bug as the duplicate-pattern case above). Collapsing
/// also shrinks the search space `matches_segments` has to explore for
/// patterns with several `**`s.
fn collapse_double_star(segs: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(segs.len());
    for seg in segs {
        if seg == "**" && out.last().map(String::as_str) == Some("**") {
            continue;
        }
        out.push(seg);
    }
    out
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
///
/// Memoized on `(pattern_index, path_index)`: the predicate at each pair
/// is pure and depends on nothing outside that pair, so caching it turns
/// what would otherwise be super-linear backtracking — a pattern with
/// several `**`s retrying every split point independently — into
/// `O(pattern_segments × path_segments)`. Without this, a legal (if
/// unusual) hand-typed pattern with a handful of non-adjacent `**`s
/// against a moderately deep path takes multiple *seconds* per file, and
/// nothing in `parse` rejects that pattern as invalid — it would silently
/// turn a routine scan into minutes of CPU. See
/// `a_pathological_multi_double_star_pattern_does_not_blow_up`.
fn matches_segments(pat: &[String], path: &[&str]) -> bool {
    fn go(
        pat: &[String],
        path: &[&str],
        pi: usize,
        si: usize,
        memo: &mut HashMap<(usize, usize), bool>,
    ) -> bool {
        if let Some(&cached) = memo.get(&(pi, si)) {
            return cached;
        }
        let result = match pat.get(pi).map(String::as_str) {
            None => si == path.len(),
            Some("**") => (si..=path.len()).any(|i| go(pat, path, pi + 1, i, memo)),
            Some(p) => match path.get(si) {
                Some(seg) if segment_matches(p, seg) => go(pat, path, pi + 1, si + 1, memo),
                _ => false,
            },
        };
        memo.insert((pi, si), result);
        result
    }

    let mut memo = HashMap::new();
    go(pat, path, 0, 0, &mut memo)
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

    /// review round 1: a pasted/round-tripped duplicate line is the same
    /// list semantically (`matches` is identical either way) but was
    /// producing a different stamp — costing the user a full index rebuild
    /// for a no-op edit.
    #[test]
    fn duplicate_patterns_collapse_to_one_stamp_entry() {
        assert_eq!(g(&["a/**"]).stamp(), g(&["a/**", "a/**"]).stamp());
    }

    /// review round 1: `a/**/**/b.md` and `a/**/b.md` match identically on
    /// every path, so they must stamp identically too — same
    /// stamp-instability class as the dedup gap above.
    #[test]
    fn consecutive_double_stars_collapse_in_the_stamp() {
        assert_eq!(g(&["a/**/b.md"]).stamp(), g(&["a/**/**/b.md"]).stamp());
        assert!(g(&["a/**/**/b.md"]).matches("a/x/y/b.md"));
    }

    /// review round 1: unmemoized `**` recursion is super-linear in path
    /// depth for patterns with several *non-adjacent* `**` segments — the
    /// reviewer measured a single `matches()` call taking 15+ seconds at
    /// depth=30, stars=8, and that pattern is syntactically legal (nothing
    /// in `parse` rejects it). This pins that a pathological-but-legal
    /// pattern against a deep path completes fast.
    ///
    /// Every path segment is the same repeated literal (`x`) and every
    /// literal pattern segment between `**`s is also `x`, so *every* split
    /// point is a locally-valid match — that ambiguity is what makes naive
    /// backtracking explore combinatorially many (pattern-position,
    /// path-position) pairs redundantly. The pattern ends in `zzz`, which
    /// never occurs, so the whole match ultimately fails only after that
    /// full exhaustive search — the true worst case. (An earlier version
    /// of this test used a literal that never occurred anywhere in the
    /// pattern, which let every `**` branch fail in O(1) with no
    /// backtracking at all — not pathological, and it stayed fast even
    /// against the unmemoized implementation.)
    #[test]
    fn a_pathological_multi_double_star_pattern_does_not_blow_up() {
        let pattern = format!("x{}/zzz", "/**/x".repeat(8));
        let s = g(&[pattern.as_str()]);
        let path = vec!["x"; 30].join("/");

        let start = std::time::Instant::now();
        assert!(!s.matches(&path));
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "pathological pattern took {elapsed:?} — matches_segments must be memoized"
        );
    }
}
