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

/// `yyyy-MM-dd` → Roam's daily-note page uid `MM-DD-YYYY`. Shape-strict: a
/// non-zero-padded input is rejected rather than silently reformatted.
pub fn to_roam_uid(date: &str) -> Option<String> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    if d.format("%Y-%m-%d").to_string() != date { return None; }
    Some(d.format("%m-%d-%Y").to_string())
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
    fn converts_iso_to_roam_uid() {
        assert_eq!(to_roam_uid("2026-08-02"), Some("08-02-2026".to_string()));
        assert_eq!(to_roam_uid("2026-8-2"), None);
    }
}
