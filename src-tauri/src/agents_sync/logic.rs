//! Pure decision logic for the CLAUDE.md → AGENTS.md symlink.

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
}
