//! The single dispatch point: bytes on disk → (file metadata, blocks, links).
//!
//! Everything upstream of the store goes through here, so there is exactly one
//! place where "how is a file turned into rows" is decided — which is what makes
//! `rebuild == incremental update` true by construction.

use crate::block::{Block, FileMeta, Link};
use crate::globs::SourceGlobs;
use crate::scan::ends_with_ascii_ci;
use crate::{frontmatter, links, norm, origin, outline, plain, prose, transcript};

pub struct Parsed {
    pub meta: FileMeta,
    pub blocks: Vec<Block>,
    pub links: Vec<Link>,
}

/// `rel_path` is the vault-relative, `/`-separated path. `mtime_secs` is the
/// last-modified time, used only as the final fallback for `doc_date`.
/// `globs` is the vault's configured source-glob patterns, forwarded
/// verbatim to `origin::derive` (spec §3, rule 5′) — this used to be a
/// `sync_dir: &str` (the sync-mirror special case rule 5′ replaced).
pub fn parse_file(rel_path: &str, raw: &str, mtime_secs: i64, globs: &SourceGlobs) -> Parsed {
    let text = norm::strip_cr(raw);

    // Format dispatch is decided FIRST, because it also decides whether the
    // bytes are allowed to be read as frontmatter at all. Only markdown has
    // frontmatter: spec §4.2 defines `.srt`/`.vtt` as "cue number / timecode
    // / text / blank line" and §4.3 defines `.txt` as "split into blocks on
    // blank lines", neither with a frontmatter step. Running
    // `frontmatter::split` unconditionally (as this did until the final fix
    // wave) silently DROPPED content: a `.txt` whose first line happens to
    // be `---` had everything up to the next `---` removed from the body
    // before `plain::chunk` ever saw it — unsearchable, with no diagnostic —
    // and a `type:` line inside that accidental block reclassified the file
    // through rule 4, overriding the source-glob designation rule 5′ is
    // supposed to give it. `---` is ordinary punctuation in transcripts and
    // plain text (a scene divider, a rule under a heading); it is a header
    // only in markdown.
    let is_transcript = ends_with_ascii_ci(rel_path, ".srt") || ends_with_ascii_ci(rel_path, ".vtt");
    let is_plain_text = ends_with_ascii_ci(rel_path, ".txt");
    let is_markdown = !is_transcript && !is_plain_text;

    let (fm_raw, body, body_line) =
        if is_markdown { frontmatter::split(&text) } else { (None, text.as_ref(), 1) };
    // `fm_raw.is_some()` must be captured BEFORE it is collapsed below —
    // `fm_raw.map(parse).unwrap_or_default()` produces the exact same
    // `Frontmatter::default()`-shaped value for "no `---` block at all" and
    // "a `---` block with nothing `derive` cares about", but `origin::derive`
    // treats those two cases differently (rule 6 vs. rule 7 — see the doc
    // comment on `derive` and `some_default_frontmatter_is_not_the_same_as_none`
    // in `origin.rs`). Passing the collapsed `Some(&fm)` unconditionally would
    // silently invert spec §3.2's deliberate misclassification for every
    // frontmatter-less file in the vault.
    let fm_present = fm_raw.is_some();
    let fm = fm_raw.map(frontmatter::parse).unwrap_or_default();

    // Chunker dispatch, from the same three booleans computed above so it can
    // never disagree with the frontmatter decision. `.note.md` is checked
    // with an exact, case-sensitive `ends_with` (it is this vault's own
    // in-app convention — see the asymmetry `is_indexable` documents in
    // `scan.rs`); the three externally-authored formats go through the SAME
    // `ends_with_ascii_ci` helper `is_indexable` uses to decide these files
    // are in scope at all, so dispatch can never disagree with the scan gate
    // about what an upper-cased `Lecture.SRT` is. `.md` (anything left over)
    // is the fallback, matching every file `is_indexable` admits that isn't
    // one of the other three shapes.
    //
    // `scan::chunker_class` mirrors this exact dispatch (same order) for the
    // rename fast path, which needs to know "same chunker or not" without
    // re-chunking. If this dispatch changes, that function must change with
    // it.
    let blocks = if is_transcript {
        transcript::chunk(body, body_line)
    } else if is_plain_text {
        plain::chunk(body, body_line)
    } else if rel_path.ends_with(".note.md") {
        outline::chunk(body, body_line)
    } else {
        prose::chunk(body, body_line)
    };

    let (doc_date, date_inferred) = resolve_doc_date(rel_path, &fm, mtime_secs);
    let meta = FileMeta {
        title: fm.title.clone().or_else(|| first_h1(body)).or_else(|| stem(rel_path)),
        concept_type: fm.concept_type.clone(),
        tags: fm.tags.clone(),
        doc_date,
        date_inferred,
        human_verified: fm.human_verified,
        origin: origin::derive(rel_path, fm_present.then_some(&fm), globs),
    };
    Parsed { meta, blocks, links: links::extract(body, body_line) }
}

/// Degradation chain from spec §3.5: filename prefix → frontmatter → mtime.
/// The filename wins because a dated filename is this vault's dominant
/// convention and is what the author actually meant by "when".
fn resolve_doc_date(rel_path: &str, fm: &frontmatter::Frontmatter, mtime_secs: i64) -> (Option<String>, bool) {
    if let Some(d) = filename_date(rel_path) {
        return (Some(d), false);
    }
    for candidate in [&fm.created, &fm.date, &fm.generated_at] {
        if let Some(v) = candidate.as_deref().and_then(ymd_prefix) {
            return (Some(v), false);
        }
    }
    (Some(ymd_from_unix(mtime_secs)), true)
}

fn filename_date(rel_path: &str) -> Option<String> {
    let name = rel_path.rsplit('/').next()?;
    ymd_prefix(name)
}

/// Accepts a leading `YYYY-MM-DD`, which covers both `2026-08-10-thing.md` and
/// an ISO timestamp like `2026-08-01T10:00:00Z`.
fn ymd_prefix(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() < 10 {
        return None;
    }
    let ok = b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit);
    ok.then(|| s[..10].to_string())
}

/// Civil-from-days (Howard Hinnant's algorithm:
/// http://howardhinnant.github.io/date_algorithms.html#civil_from_days). No
/// chrono dependency for one date conversion — the crate's dependency list is
/// part of the binary size budget. Verified by hand against the epoch, a leap
/// day, a year boundary, and pre-1970 dates (see chunk.rs test module).
fn ymd_from_unix(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Public wrapper around [`ymd_from_unix`] — Task 11 (the `SearchIndex` facade)
/// needs the same unix→`YYYY-MM-DD` conversion for surfacing freshness info,
/// and this keeps that a one-line call instead of a second implementation.
pub fn ymd_from_unix_public(secs: i64) -> String {
    ymd_from_unix(secs)
}

fn first_h1(body: &str) -> Option<String> {
    body.lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// The filename without its `.md` / `.note.md` suffix. `pub(crate)` because
/// `store::title_tokens` needs the same answer this module's `title` fallback
/// chain uses — two spellings of "what is this file called" would drift.
pub(crate) fn stem(rel_path: &str) -> Option<String> {
    let name = rel_path.rsplit('/').next()?;
    let stem = name.strip_suffix(".note.md").or_else(|| name.strip_suffix(".md")).unwrap_or(name);
    (!stem.is_empty()).then(|| stem.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-08-10T00:00:00Z. NOTE: the task brief's original constant
    // (1_754_784_000) is 2025-08-10T00:00:00Z, not 2026 — corrected here.
    const MTIME: i64 = 1_786_320_000;

    #[test]
    fn note_md_files_go_through_the_outline_chunker() {
        let p = parse_file("a.note.md", "- alpha\n  - beta\n", MTIME, &no_globs());
        assert!(p.blocks.iter().any(|b| b.text == "beta" && b.breadcrumb == "alpha"));
    }

    #[test]
    fn plain_md_files_go_through_the_prose_chunker() {
        let p = parse_file("a.md", "# T\n\npara\n", MTIME, &no_globs());
        assert!(p.blocks.iter().any(|b| b.text == "para"));
    }

    /// The dispatch itself, pinned through `parse_file` — not just that
    /// `transcript::chunk` behaves correctly in isolation (that's
    /// `transcript.rs`'s own test module's job). Distinguishing signal: a
    /// misroute to `prose::chunk` would fold the timecode line into the same
    /// paragraph as the text (no blank line separates them here), so the
    /// exact single-block-with-only-"hello world"-text shape only comes out
    /// of `transcript::chunk`.
    #[test]
    fn srt_and_vtt_files_go_through_the_transcript_chunker() {
        let p = parse_file("a.srt", "1\n00:00:01,000 --> 00:00:02,000\nhello world\n", MTIME, &no_globs());
        let texts: Vec<&str> = p.blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, vec!["hello world"], "{:?}", p.blocks);

        let p = parse_file("a.vtt", "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nhi\n", MTIME, &no_globs());
        let texts: Vec<&str> = p.blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, vec!["hi"], "{:?}", p.blocks);
    }

    /// Same reasoning as the transcript dispatch test above, for `.txt` →
    /// `plain::chunk`. Distinguishing signal: `prose::chunk` always appends
    /// a trailing `BlockLevel::File` rollup block even for a headingless
    /// body (see `prose.rs`'s `a_file_without_headings_still_gets_a_file_
    /// block`), so a misroute to `prose` would produce three blocks here,
    /// not two.
    #[test]
    fn txt_files_go_through_the_plain_chunker() {
        let p = parse_file("a.txt", "para one\n\npara two\n", MTIME, &no_globs());
        let texts: Vec<&str> = p.blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(texts, vec!["para one", "para two"], "{:?}", p.blocks);
    }

    /// Bullet 2 of the task: dispatch must be case-insensitive for the three
    /// externally-authored extensions (an uppercase `.SRT`/`.TXT` off a
    /// ripper or export tool), reusing the exact same helper
    /// `is_indexable` in `scan.rs` uses to admit them — see that function's
    /// doc comment for why the asymmetry with `.md` (checked next) exists.
    #[test]
    fn dispatch_is_case_insensitive_for_transcripts_and_txt_but_not_for_md() {
        let p = parse_file("Lecture.SRT", "1\n00:00:01,000 --> 00:00:02,000\nhi\n", MTIME, &no_globs());
        assert_eq!(p.blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), vec!["hi"], "{:?}", p.blocks);

        let p = parse_file("notes.TXT", "para one\n\npara two\n", MTIME, &no_globs());
        assert_eq!(
            p.blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>(),
            vec!["para one", "para two"],
            "{:?}",
            p.blocks
        );
    }

    /// spec §3.5 的降级链,顺序不能反:文件名 → frontmatter → mtime。
    #[test]
    fn doc_date_prefers_the_filename_prefix() {
        let p = parse_file("2026-01-02-thing.md", "---\ncreated: 2020-05-05\n---\nx\n", MTIME, &no_globs());
        assert_eq!(p.meta.doc_date.as_deref(), Some("2026-01-02"));
        assert!(!p.meta.date_inferred);
    }

    #[test]
    fn doc_date_falls_back_to_frontmatter_then_to_mtime() {
        let p = parse_file("thing.md", "---\ncreated: 2020-05-05\n---\nx\n", MTIME, &no_globs());
        assert_eq!(p.meta.doc_date.as_deref(), Some("2020-05-05"));
        assert!(!p.meta.date_inferred);

        let p = parse_file("thing.md", "x\n", MTIME, &no_globs());
        assert_eq!(p.meta.doc_date.as_deref(), Some("2026-08-10"));
        assert!(p.meta.date_inferred, "mtime-derived dates must be flagged inferred");
    }

    #[test]
    fn title_falls_back_to_the_first_h1_then_to_the_stem() {
        assert_eq!(parse_file("a.md", "---\ntitle: FM\n---\n# H\n", MTIME, &no_globs()).meta.title.as_deref(), Some("FM"));
        assert_eq!(parse_file("a.md", "# H\n", MTIME, &no_globs()).meta.title.as_deref(), Some("H"));
        assert_eq!(parse_file("dir/my-note.md", "text\n", MTIME, &no_globs()).meta.title.as_deref(), Some("my-note"));
    }

    /// CRLF 文件必须和 LF 文件产出逐字相同的块 —— 跨平台规约②。
    #[test]
    fn crlf_input_produces_identical_blocks_to_lf() {
        let lf = parse_file("a.md", "# T\n\npara\n", MTIME, &no_globs());
        let crlf = parse_file("a.md", "# T\r\n\r\npara\r\n", MTIME, &no_globs());
        let f = |p: &Parsed| p.blocks.iter().map(|b| (b.line_start, b.line_end, b.text.clone())).collect::<Vec<_>>();
        assert_eq!(f(&lf), f(&crlf));
    }

    /// 宽容义务:frontmatter 坏掉不影响正文进索引。
    #[test]
    fn a_broken_frontmatter_still_indexes_the_body() {
        let p = parse_file("a.md", "---\n[[[\n---\nbody text\n", MTIME, &no_globs());
        assert!(p.blocks.iter().any(|b| b.text.contains("body text")));
    }

    /// `links` rows must actually flow through the dispatch point, not just be
    /// computed and discarded.
    #[test]
    fn links_found_in_the_body_show_up_on_parsed() {
        let p = parse_file("a.md", "see [[Target]]\n", MTIME, &no_globs());
        assert_eq!(p.links.len(), 1);
        assert_eq!(p.links[0].target, "Target");
    }

    /// Task 11's public wrapper around the internal unix→YYYY-MM-DD helper.
    #[test]
    fn ymd_from_unix_public_matches_the_internal_helper() {
        assert_eq!(ymd_from_unix_public(MTIME), "2026-08-10");
    }

    /// Pins the edge cases the module comment on `ymd_from_unix` claims are
    /// covered: epoch, a leap day, both sides of a year boundary, and a
    /// pre-1970 (negative) timestamp — the case `div_euclid`/`rem_euclid` was
    /// chosen for. Each value was independently cross-checked against
    /// Python's `datetime.utcfromtimestamp` before this test was written (see
    /// the task-6 report's "independent verification" section).
    #[test]
    fn ymd_from_unix_handles_epoch_leap_day_year_boundary_and_negative_timestamps() {
        assert_eq!(ymd_from_unix(0), "1970-01-01"); // epoch
        assert_eq!(ymd_from_unix(1_709_164_800), "2024-02-29"); // leap day
        assert_eq!(ymd_from_unix(1_767_139_200), "2025-12-31"); // year boundary, before
        assert_eq!(ymd_from_unix(1_767_225_600), "2026-01-01"); // year boundary, after
        assert_eq!(ymd_from_unix(-86_400), "1969-12-31"); // pre-1970, negative input
    }

    /// The middle of the degradation chain (spec §3.5: filename → created →
    /// date → generated.at → mtime) is easy to silently reorder. Isolate each
    /// non-filename, non-mtime rung by omitting the higher-priority keys, and
    /// confirm `date_inferred` stays false for all of them — only the mtime
    /// branch sets it.
    #[test]
    fn doc_date_falls_back_through_date_then_generated_at_before_mtime() {
        let p = parse_file("thing.md", "---\ndate: 2021-06-07\n---\nx\n", MTIME, &no_globs());
        assert_eq!(p.meta.doc_date.as_deref(), Some("2021-06-07"));
        assert!(!p.meta.date_inferred);

        let p = parse_file(
            "thing.md",
            "---\ngenerated:\n  by: claude/1\n  at: 2022-09-10T00:00:00Z\n---\nx\n",
            MTIME,
            &no_globs(),
        );
        assert_eq!(p.meta.doc_date.as_deref(), Some("2022-09-10"));
        assert!(!p.meta.date_inferred);
    }

    /// The trap this task was built around (see the doc comment on
    /// `origin::derive` and `origin::tests::some_default_frontmatter_is_not_the_same_as_none`):
    /// `parse_file` collapses "no `---` block at all" and "an empty/irrelevant
    /// `---` block" into the same `Frontmatter::default()`-shaped value before
    /// anything downstream sees it. If that collapsed value were forwarded to
    /// `origin::derive` as `Some(&fm)` unconditionally, every frontmatter-less
    /// `.md` — the bulk of ebook exports, transcripts, and hand-written notes
    /// with no frontmatter — would misclassify as `Derived` instead of the
    /// spec's deliberate `Unlabeled` (§3.2 rule 6′, 2026-08-12 design),
    /// inverting the whole point of the tier for exactly the files it exists
    /// to protect. This test proves the real dispatch point (`parse_file`,
    /// not `origin::derive` called directly) preserves the absent-vs-empty
    /// distinction end-to-end.
    #[test]
    fn a_file_with_no_frontmatter_block_at_all_classifies_as_unlabeled() {
        let p = parse_file("plain.md", "just a paragraph, no frontmatter\n", MTIME, &no_globs());
        assert_eq!(p.meta.origin, crate::origin::Origin::Unlabeled);
    }

    /// The sibling of the test above, using the SAME input shape
    /// (`Frontmatter::default()`) reached via a genuinely present-but-empty
    /// `---` block instead of an absent one — pinning that these two inputs,
    /// which look identical after `frontmatter::parse`, still diverge in
    /// `origin` because `parse_file` keeps the "was there a block at all"
    /// fact alive past the collapse.
    #[test]
    fn a_file_with_an_empty_frontmatter_block_does_not_classify_as_unlabeled() {
        let p = parse_file("plain.md", "---\n---\njust a paragraph\n", MTIME, &no_globs());
        assert_ne!(p.meta.origin, crate::origin::Origin::Unlabeled);
    }

    /// A matched source-glob path with no frontmatter must still win rule 5′
    /// over rule 6′ through the real dispatch point, not just in
    /// `origin::derive` unit tests — see
    /// `origin::tests::a_matched_path_without_frontmatter_is_source_not_unlabeled`
    /// for the narrower pin of the same priority.
    #[test]
    fn a_matched_glob_with_no_frontmatter_classifies_as_source_through_parse_file() {
        let globs = crate::globs::parse(&["ebook/**".to_string()]);
        let p = parse_file("ebook/a.md", "no frontmatter here\n", MTIME, &globs);
        assert_eq!(p.meta.origin, crate::origin::Origin::Source);
    }

    /// The interaction of two tasks (C-T4/C-T5's transcript dispatch and
    /// C-T2's rule 5′/6′ origin priority): a transcript has no frontmatter
    /// block at all, so `fm_present` is `false` exactly like the plain `.md`
    /// case `a_file_with_no_frontmatter_block_at_all_classifies_as_
    /// unlabeled` pins as `Unlabeled` above — the ONLY reason this one
    /// doesn't fall into that same tier is that rule 5′ (its path matches a
    /// configured source glob) wins the priority race before rule 6′
    /// (absent frontmatter) is ever consulted. Worth pinning on its own
    /// rather than trusting the two tasks' unit tests to compose correctly.
    #[test]
    fn a_transcript_is_source_never_unlabeled() {
        let g = crate::globs::parse(&["media/**".to_string()]);
        let p = parse_file("media/a.srt", "1\n00:00:01,000 --> 00:00:02,000\n话\n", 0, &g);
        assert_eq!(p.meta.origin, crate::Origin::Source);
    }

    /// Final fix wave, Blocker 4 — CONTENT LOSS. `frontmatter::split` used to
    /// run unconditionally, before the format dispatch, so a `.txt` (or
    /// `.srt`/`.vtt`) whose first line is `---` had everything up to the next
    /// `---` amputated from the body: those words never reached
    /// `plain::chunk`, never became a block, and never entered FTS — the
    /// query below returned zero hits against a file that plainly contains
    /// the word. Nothing reported it. Spec §4.3 defines `.txt` as "split into
    /// blocks on blank lines" with no frontmatter step at all; `---` is
    /// ordinary punctuation outside markdown.
    #[test]
    fn a_leading_dash_block_in_a_txt_file_is_content_not_frontmatter() {
        let p = parse_file("raw/b.txt", "---\nscene alphaword one\n---\nscene betaword two\n", MTIME, &no_globs());
        let joined = p.blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("alphaword"), "leading block was dropped: {:?}", p.blocks);
        assert!(joined.contains("betaword"), "{:?}", p.blocks);
    }

    /// The same defect's other half — a TIER FLIP, not just lost text. A
    /// `type:` line inside that accidental frontmatter block reached rule 4
    /// (registered type → mapped tier) and beat rule 5′, so a `.txt` sitting
    /// squarely inside a source-glob pattern classified `Derived` instead of
    /// `Source`. A non-markdown file has no frontmatter by construction, so
    /// its tier must be derived from `None` — which is exactly what rules
    /// 5′/6′ expect (spec §3: "`.srt`/`.txt`/`.vtt` 永不进「未标注」" holds
    /// only because rule 5′ necessarily fires first).
    #[test]
    fn a_type_line_inside_a_txt_file_does_not_override_the_source_glob_tier() {
        let globs = crate::globs::parse(&["raw/**".to_string()]);
        let p = parse_file("raw/a.txt", "---\ntype: Book Summary\n---\nhello alpha world\n", MTIME, &globs);
        assert_eq!(p.meta.origin, crate::origin::Origin::Source);
        assert_eq!(p.meta.concept_type, None, "a .txt has no frontmatter to take a concept type from");
    }

    /// Same pin for the transcript branch (spec §4.2 likewise has no
    /// frontmatter step). The input has to START with `---` for the old
    /// unconditional `frontmatter::split` to bite at all —
    /// `frontmatter::split` only recognises a delimiter at byte 0 — so a
    /// `---` merely sitting inside a cue proves nothing; this is a divider
    /// line prepended by an export tool, which used to swallow every cue up
    /// to the next `---`.
    #[test]
    fn a_leading_dash_block_in_a_transcript_is_content_not_frontmatter() {
        let p = parse_file(
            "media/a.srt",
            "---\n1\n00:00:01,000 --> 00:00:02,000\ngammaword\n---\n\n2\n00:00:03,000 --> 00:00:04,000\ndeltaword\n",
            MTIME,
            &no_globs(),
        );
        let joined = p.blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("gammaword"), "leading cue was dropped: {:?}", p.blocks);
        assert!(joined.contains("deltaword"), "{:?}", p.blocks);
    }

    fn no_globs() -> SourceGlobs {
        SourceGlobs::default()
    }
}
