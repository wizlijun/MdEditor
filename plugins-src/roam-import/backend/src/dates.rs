//! Date handling. Roam's daily-note page uid is `MM-DD-YYYY`; note.md's daily
//! note file is `yyyy-MM-dd`. `today`/`yesterday` are OUR convenience — the
//! Roam read API has no relative-date vocabulary — so they resolve here,
//! against the caller-supplied local calendar date (injectable for tests).
use chrono::{Duration, NaiveDate};

pub fn resolve_date(input: Option<&str>, today: NaiveDate) -> Result<String, String> {
    let raw = input.unwrap_or("yesterday").trim().to_lowercase();
    let d = match raw.as_str() {
        "today" => today,
        "yesterday" => today - Duration::days(1),
        "tomorrow" => today + Duration::days(1),
        other => NaiveDate::parse_from_str(other, "%Y-%m-%d")
            .map_err(|_| format!("invalid --date '{other}': expected yyyy-MM-dd, today or yesterday"))?,
    };
    Ok(d.format("%Y-%m-%d").to_string())
}

/// Strict `yyyy-MM-dd`: a real calendar date, zero-padded, and nothing else —
/// a non-zero-padded or otherwise reformatted input is rejected rather than
/// silently accepted.
fn parse_iso(date: &str) -> Option<NaiveDate> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    (d.format("%Y-%m-%d").to_string() == date).then_some(d)
}

/// Is this exactly a `yyyy-MM-dd` calendar date? `sync` asks before joining a
/// date into a vault path — that path is the only thing standing between a
/// bad `--date` and a write outside the daily-note folder.
pub fn is_iso_date(date: &str) -> bool {
    parse_iso(date).is_some()
}

/// `yyyy-MM-dd` → Roam's daily-note page uid `MM-DD-YYYY`.
pub fn to_roam_uid(date: &str) -> Option<String> {
    Some(parse_iso(date)?.format("%m-%d-%Y").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn today() -> NaiveDate { NaiveDate::from_ymd_opt(2026, 8, 3).unwrap() }

    #[test]
    fn defaults_to_yesterday() {
        assert_eq!(resolve_date(None, today()), Ok("2026-08-02".to_string()));
    }

    #[test]
    fn resolves_relative_words() {
        assert_eq!(resolve_date(Some("today"), today()), Ok("2026-08-03".to_string()));
        assert_eq!(resolve_date(Some("Yesterday"), today()), Ok("2026-08-02".to_string()));
    }

    #[test]
    fn passes_through_iso_dates() {
        assert_eq!(resolve_date(Some("2026-01-09"), today()), Ok("2026-01-09".to_string()));
    }

    #[test]
    fn rejects_garbage_and_impossible_dates() {
        assert!(resolve_date(Some("08/02/2026"), today()).is_err());
        assert!(resolve_date(Some("2026-13-40"), today()).is_err());
    }

    #[test]
    fn only_a_padded_calendar_date_is_an_iso_date() {
        assert!(is_iso_date("2026-08-02"));
        assert!(!is_iso_date("2026-8-2"));
        assert!(!is_iso_date("2026-02-30"), "a date that does not exist");
        assert!(!is_iso_date("../../etc/passwd"), "the case sync cares about");
    }

    #[test]
    fn converts_iso_to_roam_uid() {
        assert_eq!(to_roam_uid("2026-08-02"), Some("08-02-2026".to_string()));
        assert_eq!(to_roam_uid("2026-8-2"), None);
    }
}
