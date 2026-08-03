//! Where does a Roam page land in the vault? Pure decision logic, no IO:
//! classify a page (daily vs. wiki), sanitise its file name, and decide
//! whether this sync is a rename or a fresh path — all by consulting the
//! [`Ledger`](crate::ledger::Ledger), never the filesystem.
//!
//! The one rule worth stating up front: **the ledger's existing record for
//! THIS uid wins over a freshly recomputed name.** A page that landed at
//! `PKM (2).note.md` last time (because another uid already held
//! `PKM.note.md`) must stay there — recomputing the base name from scratch
//! and ignoring the ledger would fight the other page for `PKM.note.md`
//! forever; blindly re-suffixing because the base is taken would climb to
//! `(3)`, `(4)`, ... on every sync. So before doing anything else, this module
//! checks whether the uid's current ledger path is already the base name (or
//! a `(N)` suffix variant of it) and, if so, leaves it alone.
use crate::ledger::Ledger;
use crate::outline::{CONCEPT_TYPE_DAILY_NOTE, CONCEPT_TYPE_WIKI_PAGE};
use crate::sync::daily_rel_path;
use chrono::NaiveDate;
use regex::Regex;
use std::sync::OnceLock;

/// Where a Roam page lands: the vault-relative path, its front-matter title,
/// its OKF `type`, and — if the ledger already knew this uid under a
/// different path — the path it is being renamed from.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub rel: String,
    pub title: String,
    pub concept_type: &'static str,
    pub rename_from: Option<String>,
}

/// `sanitizeFileName` in `src/lib/outline/slug.ts`, ported verbatim: illegal
/// filesystem characters (macOS/Windows union) become `-`, then leading/
/// trailing whitespace, leading dots and leading/trailing dashes are
/// stripped, in that order — an empty result falls back to `untitled`. This
/// is the file-name half of the wikilink-text === file-name contract; it
/// must not drift from the TypeScript rules even in edge cases (e.g. a
/// newline in the title is *not* replaced, because slug.ts does not replace
/// it either).
pub fn sanitize_file_name(raw: &str) -> String {
    let replaced = illegal_char_pattern().replace_all(raw, "-");
    let trimmed = replaced.trim();
    let no_leading_dots = trimmed.trim_start_matches('.');
    let no_dashes = no_leading_dots.trim_start_matches('-').trim_end_matches('-');
    let result = no_dashes.trim();
    if result.is_empty() { "untitled".to_string() } else { result.to_string() }
}

fn illegal_char_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"[/\\:*?"<>|]"#).unwrap())
}

/// Roam's daily-page uid: exactly `MM-DD-YYYY`, zero-padded.
fn daily_uid_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{2}-\d{2}-\d{4}$").unwrap())
}

/// `(N)` suffix on a path stem, e.g. `wikipage/PKM (2)` captures
/// (`wikipage/PKM`, `2`). Applied to a path with its `.note.md` extension
/// already stripped.
fn suffix_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(.*) \(\d+\)$").unwrap())
}

/// If `uid` is shaped like a Roam daily-page uid *and* names a real calendar
/// date, its `yyyy-MM-dd` form — otherwise `None`. A uid that merely looks
/// like a date (`13-45-2026`) is not one; it falls through to the wiki
/// routing below rather than panicking or silently misrouting.
fn daily_iso_date(uid: &str) -> Option<String> {
    if !daily_uid_pattern().is_match(uid) {
        return None;
    }
    let d = NaiveDate::parse_from_str(uid, "%m-%d-%Y").ok()?;
    Some(d.format("%Y-%m-%d").to_string())
}

/// Does `existing` (a `.note.md`-stripped path) name the same page as `base`
/// (also stripped) — either exactly, or as `base (N)` for some `N`? This is
/// the check that keeps a page's suffix stable across syncs: it is asked
/// only about the path the ledger already recorded for *this* uid, never
/// used to compare two different uids against each other.
fn is_variant_of(existing: &str, base: &str) -> bool {
    if existing == base {
        return true;
    }
    match suffix_pattern().captures(existing) {
        Some(caps) => &caps[1] == base,
        None => false,
    }
}

/// Strip the `.note.md` extension a `Target::rel` always carries.
fn strip_ext(path: &str) -> &str {
    path.strip_suffix(".note.md").unwrap_or(path)
}

/// Find the first of `base`, `base (2)`, `base (3)`, ... not claimed in the
/// ledger by a *different* uid. Bounded by however many colliding suffixes
/// actually exist in the ledger — realistic vault sizes mean this is never
/// more than a handful of iterations, and it walks forward from `base`
/// exactly once rather than rescanning from scratch on each attempt.
fn resolve_path(base_rel: &str, uid: &str, ledger: &Ledger) -> String {
    let base_no_ext = strip_ext(base_rel).to_string();
    let mut n: u32 = 1;
    loop {
        let candidate = if n == 1 { base_rel.to_string() } else { format!("{base_no_ext} ({n}).note.md") };
        match ledger.uid_at(&candidate) {
            Some(other) if other != uid => n += 1,
            _ => return candidate,
        }
    }
}

/// Classify and place a Roam page: which vault file it belongs at, under
/// what title and OKF `type`, and whether landing it there is a rename of a
/// path the ledger already knows for this uid. `dirs` is
/// `(wiki_dir, daily_dir)` — the host's configured folder names.
pub fn route_page(uid: &str, roam_title: &str, dirs: (&str, &str), ledger: &Ledger) -> Target {
    let (wiki_dir, daily_dir) = dirs;

    let (title, concept_type, base_rel) = match daily_iso_date(uid) {
        Some(date) => {
            let rel = daily_rel_path(daily_dir, &date);
            (date, CONCEPT_TYPE_DAILY_NOTE, rel)
        }
        None => {
            let sanitized = sanitize_file_name(roam_title);
            let rel = format!("{wiki_dir}/{sanitized}.note.md");
            (roam_title.to_string(), CONCEPT_TYPE_WIKI_PAGE, rel)
        }
    };

    let existing = ledger.path_of(uid).map(|s| s.to_string());
    let base_no_ext = strip_ext(&base_rel).to_string();

    let (rel, rename_from) = match &existing {
        Some(old) if is_variant_of(strip_ext(old), &base_no_ext) => (old.clone(), None),
        _ => {
            let resolved = resolve_path(&base_rel, uid, ledger);
            let rename_from = existing.filter(|old| *old != resolved);
            (resolved, rename_from)
        }
    };

    Target { rel, title, concept_type, rename_from }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::Ledger;

    const DIRS: (&str, &str) = ("wikipage", "dailynote");

    #[test]
    fn sanitize_matches_the_typescript_rules() {
        assert_eq!(sanitize_file_name("回顾系统"), "回顾系统");
        assert_eq!(sanitize_file_name("a/b:c*d"), "a-b-c-d");
        assert_eq!(sanitize_file_name("  ..hidden  "), "hidden");
        assert_eq!(sanitize_file_name("///"), "untitled");
        assert_eq!(sanitize_file_name(""), "untitled");
    }

    #[test]
    fn a_daily_uid_routes_to_the_daily_folder_with_the_iso_date_as_title() {
        let t = route_page("08-02-2026", "August 2nd, 2026", DIRS, &Ledger::default());
        assert_eq!(t.rel, "dailynote/2026/2026-08-02.note.md");
        assert_eq!(t.title, "2026-08-02", "a daily note's title is the ISO date, never Roam's English one");
        assert_eq!(t.concept_type, crate::outline::CONCEPT_TYPE_DAILY_NOTE);
        assert!(t.rename_from.is_none());
    }

    #[test]
    fn any_other_uid_routes_to_the_wiki_folder_under_its_sanitised_title() {
        let t = route_page("8IFJWtnad", "回顾/系统", DIRS, &Ledger::default());
        assert_eq!(t.rel, "wikipage/回顾-系统.note.md");
        assert_eq!(t.title, "回顾/系统", "the front-matter title keeps the real Roam title");
        assert_eq!(t.concept_type, crate::outline::CONCEPT_TYPE_WIKI_PAGE);
    }

    #[test]
    fn a_title_change_since_the_last_sync_is_reported_as_a_rename() {
        let mut l = Ledger::default();
        l.claim("u", "wikipage/旧名.note.md", "旧名");
        let t = route_page("u", "新名", DIRS, &l);
        assert_eq!(t.rel, "wikipage/新名.note.md");
        assert_eq!(t.rename_from.as_deref(), Some("wikipage/旧名.note.md"));
    }

    #[test]
    fn an_unchanged_path_is_not_a_rename() {
        let mut l = Ledger::default();
        l.claim("u", "wikipage/名.note.md", "名");
        assert!(route_page("u", "名", DIRS, &l).rename_from.is_none());
    }

    #[test]
    fn a_path_held_by_another_uid_gets_a_numeric_suffix() {
        let mut l = Ledger::default();
        l.claim("other", "wikipage/PKM.note.md", "PKM");
        let t = route_page("mine", "PKM", DIRS, &l);
        assert_eq!(t.rel, "wikipage/PKM (2).note.md");
    }

    #[test]
    fn suffixes_keep_climbing_while_the_path_is_taken() {
        let mut l = Ledger::default();
        l.claim("a", "wikipage/PKM.note.md", "PKM");
        l.claim("b", "wikipage/PKM (2).note.md", "PKM");
        assert_eq!(route_page("c", "PKM", DIRS, &l).rel, "wikipage/PKM (3).note.md");
    }

    #[test]
    fn a_path_held_by_the_same_uid_gets_no_suffix() {
        let mut l = Ledger::default();
        l.claim("u", "wikipage/PKM (2).note.md", "PKM");
        // Re-routing the same page must not climb to (3) every sync.
        let t = route_page("u", "PKM", DIRS, &l);
        assert_eq!(t.rel, "wikipage/PKM (2).note.md");
        assert!(t.rename_from.is_none());
    }
}
