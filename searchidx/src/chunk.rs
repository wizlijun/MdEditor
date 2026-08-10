//! The single dispatch point: bytes on disk → (file metadata, blocks, links).
//!
//! Everything upstream of the store goes through here, so there is exactly one
//! place where "how is a file turned into rows" is decided — which is what makes
//! `rebuild == incremental update` true by construction.

use crate::block::{Block, FileMeta, Link};
use crate::{frontmatter, links, norm, outline, prose};

pub struct Parsed {
    pub meta: FileMeta,
    pub blocks: Vec<Block>,
    pub links: Vec<Link>,
}

/// `rel_path` is the vault-relative, `/`-separated path. `mtime_secs` is the
/// last-modified time, used only as the final fallback for `doc_date`.
pub fn parse_file(rel_path: &str, raw: &str, mtime_secs: i64) -> Parsed {
    let text = norm::strip_cr(raw);
    let (fm_raw, body, body_line) = frontmatter::split(&text);
    let fm = fm_raw.map(frontmatter::parse).unwrap_or_default();

    let blocks = if rel_path.ends_with(".note.md") {
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

fn stem(rel_path: &str) -> Option<String> {
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
        let p = parse_file("a.note.md", "- alpha\n  - beta\n", MTIME);
        assert!(p.blocks.iter().any(|b| b.text == "beta" && b.breadcrumb == "alpha"));
    }

    #[test]
    fn plain_md_files_go_through_the_prose_chunker() {
        let p = parse_file("a.md", "# T\n\npara\n", MTIME);
        assert!(p.blocks.iter().any(|b| b.text == "para"));
    }

    /// spec §3.5 的降级链,顺序不能反:文件名 → frontmatter → mtime。
    #[test]
    fn doc_date_prefers_the_filename_prefix() {
        let p = parse_file("2026-01-02-thing.md", "---\ncreated: 2020-05-05\n---\nx\n", MTIME);
        assert_eq!(p.meta.doc_date.as_deref(), Some("2026-01-02"));
        assert!(!p.meta.date_inferred);
    }

    #[test]
    fn doc_date_falls_back_to_frontmatter_then_to_mtime() {
        let p = parse_file("thing.md", "---\ncreated: 2020-05-05\n---\nx\n", MTIME);
        assert_eq!(p.meta.doc_date.as_deref(), Some("2020-05-05"));
        assert!(!p.meta.date_inferred);

        let p = parse_file("thing.md", "x\n", MTIME);
        assert_eq!(p.meta.doc_date.as_deref(), Some("2026-08-10"));
        assert!(p.meta.date_inferred, "mtime-derived dates must be flagged inferred");
    }

    #[test]
    fn title_falls_back_to_the_first_h1_then_to_the_stem() {
        assert_eq!(parse_file("a.md", "---\ntitle: FM\n---\n# H\n", MTIME).meta.title.as_deref(), Some("FM"));
        assert_eq!(parse_file("a.md", "# H\n", MTIME).meta.title.as_deref(), Some("H"));
        assert_eq!(parse_file("dir/my-note.md", "text\n", MTIME).meta.title.as_deref(), Some("my-note"));
    }

    /// CRLF 文件必须和 LF 文件产出逐字相同的块 —— 跨平台规约②。
    #[test]
    fn crlf_input_produces_identical_blocks_to_lf() {
        let lf = parse_file("a.md", "# T\n\npara\n", MTIME);
        let crlf = parse_file("a.md", "# T\r\n\r\npara\r\n", MTIME);
        let f = |p: &Parsed| p.blocks.iter().map(|b| (b.line_start, b.line_end, b.text.clone())).collect::<Vec<_>>();
        assert_eq!(f(&lf), f(&crlf));
    }

    /// 宽容义务:frontmatter 坏掉不影响正文进索引。
    #[test]
    fn a_broken_frontmatter_still_indexes_the_body() {
        let p = parse_file("a.md", "---\n[[[\n---\nbody text\n", MTIME);
        assert!(p.blocks.iter().any(|b| b.text.contains("body text")));
    }

    /// `links` rows must actually flow through the dispatch point, not just be
    /// computed and discarded.
    #[test]
    fn links_found_in_the_body_show_up_on_parsed() {
        let p = parse_file("a.md", "see [[Target]]\n", MTIME);
        assert_eq!(p.links.len(), 1);
        assert_eq!(p.links[0].target, "Target");
    }

    /// Task 11's public wrapper around the internal unix→YYYY-MM-DD helper.
    #[test]
    fn ymd_from_unix_public_matches_the_internal_helper() {
        assert_eq!(ymd_from_unix_public(MTIME), "2026-08-10");
    }
}
