//! Pure decision logic for the CLAUDE.md → AGENTS.md symlink.

/// The convention block that teaches any harness how to search this vault.
///
/// Windows and macOS spell the command identically on purpose (the installer
/// puts a `notemd` shim on PATH) — one instruction, not a platform matrix.
pub const SEARCH_SECTION: &str = r#"## Searching this vault

This vault has a local full-text index. Prefer it over a raw `rg` sweep: it is
faster, it knows Chinese word boundaries, and it ranks the notes you have
actually annotated above machine-generated summaries of them.

```
notemd search <query...>            # path:line:text, ranked, exit 1 = no match
notemd search "exact phrase"        # phrase match
notemd search x tag:y type:z        # filters: tag: type: path: ext: after: before: page:[[X]] origin:
notemd search origin:human          # only what a human wrote/signed (vs derived|source|unlabeled)
notemd search origin:unlabeled      # files with no frontmatter and no source-glob match — fix these
notemd search x --json              # adds score, breadcrumb, source_ref, provenance, origin, attention_minutes
notemd search x --context 2         # surrounding lines
notemd search x --all               # every hit — default cap is 20 (--limit N adjusts, 0 = no cap)
```

`rg` and `grep` keep working and are never wrong to use — the index is an
accelerator, not a gatekeeper. When a result's `provenance.agent_by` is set, the
text was written by a model: follow its `sources` to the primary document before
relying on it. `origin` classifies a whole file into `human` (you wrote or
signed it) / `derived` (a model generated it) / `source` (raw material a model
still has to read) / `unlabeled` (nobody has claimed it — no frontmatter, no
source-glob match) — filter to `origin:human` to see only what a human
actually judged, or `origin:unlabeled` to find files worth labeling one way or
the other, or read the field in `--json` output to weigh a hit accordingly.
Unlabeled files are ranked lowest by default (×0.3) and can fall out of the
top results entirely, so `origin:unlabeled` is the way to find them anyway.
"#;

/// True when `agents_md` does not already contain the search convention block.
pub fn search_section_missing(agents_md: &str) -> bool {
    !agents_md.contains("## Searching this vault")
}

/// Append-only. Never rewrites, reorders or reformats what is already there:
/// AGENTS.md is the user's file, and a tool that edits it silently is a tool
/// they stop trusting.
pub fn append_search_section(agents_md: &str) -> String {
    if !search_section_missing(agents_md) {
        return agents_md.to_string();
    }
    let mut out = agents_md.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(SEARCH_SECTION);
    out
}

/// What `CLAUDE.md` currently is on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeKind {
    Missing,
    /// Symlink whose target is exactly `AGENTS.md` (same-dir, relative).
    CorrectSymlink,
    /// Symlink to anything else (absolute, or a different file).
    WrongSymlink,
    /// A regular file (or any non-symlink entry).
    RegularFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    None,
    CreateSymlink,
    BackupThenSymlink,
    RepointSymlink,
    RemoveDangling,
}

pub fn decide(agents_exists: bool, claude: ClaudeKind) -> SyncAction {
    use ClaudeKind::*;
    use SyncAction::*;
    if agents_exists {
        match claude {
            Missing => CreateSymlink,
            CorrectSymlink => None,
            WrongSymlink => RepointSymlink,
            RegularFile => BackupThenSymlink,
        }
    } else {
        match claude {
            // A relative link to a now-missing AGENTS.md is dangling; sync owns it.
            CorrectSymlink => RemoveDangling,
            Missing | WrongSymlink | RegularFile => None,
        }
    }
}

/// Civil date (year, month, day) from a Unix timestamp in seconds, UTC.
/// Howard Hinnant's `civil_from_days`.
pub fn ymd_from_unix_secs(secs: i64) -> (i64, u32, u32) {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

/// Pick a non-colliding backup filename for a given `YYYYMMDD` stamp.
/// `exists` reports whether a candidate name is already taken.
pub fn pick_backup_name(stamp: &str, exists: impl Fn(&str) -> bool) -> String {
    let base = format!("CLAUDE.{stamp}.md");
    if !exists(&base) {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("CLAUDE.{stamp}-{n}.md");
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ClaudeKind::*;
    use SyncAction::*;

    #[test]
    fn agents_present_actions() {
        assert_eq!(decide(true, Missing), CreateSymlink);
        assert_eq!(decide(true, CorrectSymlink), None);
        assert_eq!(decide(true, WrongSymlink), RepointSymlink);
        assert_eq!(decide(true, RegularFile), BackupThenSymlink);
    }

    #[test]
    fn agents_absent_actions() {
        assert_eq!(decide(false, CorrectSymlink), RemoveDangling);
        assert_eq!(decide(false, WrongSymlink), None);
        assert_eq!(decide(false, RegularFile), None);
        assert_eq!(decide(false, Missing), None);
    }

    #[test]
    fn ymd_epoch_and_known_dates() {
        assert_eq!(ymd_from_unix_secs(0), (1970, 1, 1));
        // 2026-07-25T00:00:00Z == 1784937600
        assert_eq!(ymd_from_unix_secs(1_784_937_600), (2026, 7, 25));
        // last second of 1999
        assert_eq!(ymd_from_unix_secs(946_684_799), (1999, 12, 31));
    }

    #[test]
    fn backup_name_no_collision() {
        assert_eq!(pick_backup_name("20260725", |_| false), "CLAUDE.20260725.md");
    }

    #[test]
    fn backup_name_suffixes_on_collision() {
        let taken = |n: &str| n == "CLAUDE.20260725.md" || n == "CLAUDE.20260725-2.md";
        assert_eq!(pick_backup_name("20260725", taken), "CLAUDE.20260725-3.md");
    }

    #[test]
    fn detects_whether_the_search_section_is_present() {
        assert!(search_section_missing("# Vault\n\nnotes\n"));
        assert!(!search_section_missing(&append_search_section("# Vault\n")));
    }

    /// 一键追加必须是**追加**:用户既有内容一个字节都不能动。这条测试就是
    /// 「绝不静默改写」的机器表达。
    #[test]
    fn appending_leaves_existing_content_byte_identical() {
        let before = "# Vault\n\nMy own conventions.\n";
        let after = append_search_section(before);
        assert!(after.starts_with(before), "existing content must be untouched");
        assert!(after.contains("## Searching this vault"));
    }

    #[test]
    fn appending_twice_does_not_duplicate_the_section() {
        let once = append_search_section("# Vault\n");
        assert_eq!(append_search_section(&once), once);
    }

    /// 文件不以换行结尾时不能把新标题粘到最后一行后面。
    #[test]
    fn appending_normalizes_a_missing_trailing_newline() {
        let after = append_search_section("# Vault");
        assert!(after.contains("# Vault\n\n## Searching this vault"), "{after}");
    }

    /// A regression a same-content-check test would miss: appending must not
    /// just leave `before`'s *content* somewhere in the output, it must leave
    /// it as an exact, untouched prefix. This pins byte offsets, not just substrings.
    #[test]
    fn appended_section_starts_immediately_after_existing_content_plus_separator() {
        let before = "# Vault\n\nfirst\nsecond\n";
        let after = append_search_section(before);
        let suffix = &after[before.len()..];
        assert!(suffix.starts_with('\n'), "expected exactly one blank-line separator, got {suffix:?}");
        assert!(suffix[1..].starts_with(SEARCH_SECTION));
    }
}
