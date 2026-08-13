//! Unified log bus: merges backend main-process + frontend webview + git-sync +
//! plugin sources into a single ring buffer, lands them in
//! `logs/app-YYYY-MM-DD.log`, and emits `log://line` to the "View Logs" window.
//! Only NEW log calls flow in — existing vault_sync/plugin stores are untouched,
//! they mirror one line here.
use serde::Serialize;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const MAX_LINES: usize = 3000;

/// How many days of archived logs survive. Older files are deleted at startup
/// and again on every day boundary the process happens to be running through.
const RETAIN_DAYS: i64 = 7;

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub ts: String,       // RFC3339 millis UTC, e.g. 2026-07-21T08:12:33.456Z
    pub source: String,   // "backend" | "frontend"
    pub category: String, // "core" | "git-sync" | "plugin:<id>" | "frontend"
    pub level: String,    // "debug" | "info" | "warn" | "error"
    pub message: String,
}

struct LogBus {
    buffer: Mutex<VecDeque<LogLine>>,
    app: OnceLock<AppHandle>,
    sink: Mutex<Option<Sink>>,
}

/// The open log file plus everything needed to notice that it has become
/// yesterday's. Held behind the bus's mutex, so `roll_if_new_day` runs under
/// the same lock as the write that follows it — the handle can never be
/// swapped out from under a line being written.
struct Sink {
    dir: PathBuf,
    /// Local calendar day this file is named after, `YYYY-MM-DD`.
    day: String,
    file: File,
    /// Unix second the day was last computed for. Within one second the answer
    /// cannot have changed, so a chatty log does not ask the system for its
    /// timezone once per line.
    checked_secs: i64,
}

impl Sink {
    fn roll_if_new_day(&mut self) {
        let secs = now_secs();
        if secs == self.checked_secs {
            return;
        }
        self.checked_secs = secs;
        let day = day_string(secs);
        if day == self.day {
            return;
        }
        // Only adopt the new day once its file is actually open: if the open
        // fails (disk full, permissions) keeping yesterday's handle loses the
        // day boundary but not the lines, which is the better half of a bad
        // trade — and the next line retries.
        if let Some(file) = open_day_file(&self.dir, &day) {
            self.file = file;
            self.day = day;
            prune_old_logs(&self.dir, &cutoff_day(secs));
        }
    }
}

impl LogBus {
    fn new() -> Self {
        LogBus {
            buffer: Mutex::new(VecDeque::with_capacity(MAX_LINES)),
            app: OnceLock::new(),
            sink: Mutex::new(None),
        }
    }

    fn record(&self, line: LogLine) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push_back(line.clone());
            while buf.len() > MAX_LINES {
                buf.pop_front();
            }
        }
        if let Ok(mut slot) = self.sink.lock() {
            if let Some(sink) = slot.as_mut() {
                sink.roll_if_new_day();
                let _ = writeln!(
                    sink.file,
                    "{} [{}/{}/{}] {}",
                    line.ts, line.source, line.category, line.level, line.message
                );
            }
        }
        if let Some(app) = self.app.get() {
            let _ = app.emit("log://line", &line);
        }
    }
}

fn bus() -> &'static LogBus {
    static BUS: OnceLock<LogBus> = OnceLock::new();
    BUS.get_or_init(LogBus::new)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// The calendar day a log file is named after — in **local** time, deliberately
/// not the UTC day its own timestamps carry.
///
/// The two are different questions. A line's `ts` stays UTC because it is
/// machine-read and compared across devices; a *file name* is read by a person
/// looking for "what happened yesterday", and in UTC+8 a UTC-dated file starts
/// at 08:00 local — every morning's logs would land in the previous day's file.
/// Falls back to the UTC day only where there is no local-time API at all
/// (neither unix nor windows), which is no platform this ships on.
fn day_string(secs: i64) -> String {
    local_day(secs).unwrap_or_else(|| format_rfc3339(secs, 0)[..10].to_string())
}

/// Files named for a day strictly before this one are deleted. Computed by
/// walking back `RETAIN_DAYS` of wall-clock seconds and asking for *that*
/// instant's local day, so it stays right across a DST shift (which moves the
/// clock by an hour, never across a date boundary from local noon).
fn cutoff_day(now: i64) -> String {
    day_string(now - RETAIN_DAYS * 86_400)
}

fn day_file_name(day: &str) -> String {
    format!("app-{day}.log")
}

fn open_day_file(dir: &Path, day: &str) -> Option<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(day_file_name(day)))
        .ok()
}

/// `app-2026-08-13.log` / `app-2026-08-13.legacy.log` → `2026-08-13`.
///
/// ISO dates sort lexicographically in time order, so callers compare the
/// returned string directly instead of parsing it into numbers. Anything that
/// does not match this exact shape returns `None` and is therefore never
/// deleted — the logs directory is somewhere a user may have dropped their own
/// files, and a retention sweep must not treat "unrecognized" as "expired".
fn log_file_day(name: &str) -> Option<&str> {
    if !name.ends_with(".log") {
        return None;
    }
    let day = name.strip_prefix("app-")?.get(..10)?;
    let shaped = day.len() == 10
        && day.as_bytes().iter().enumerate().all(|(i, b)| match i {
            4 | 7 => *b == b'-',
            _ => b.is_ascii_digit(),
        });
    shaped.then_some(day)
}

/// Delete archives older than `cutoff_day`. Best-effort throughout: a file that
/// will not delete (open elsewhere, permissions) is skipped, never retried in a
/// loop and never escalated — retention is housekeeping, not correctness.
fn prune_old_logs(dir: &Path, cutoff_day: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(day) = log_file_day(name) else { continue };
        if day < cutoff_day {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Migration for the single un-rotated `app.log` every build before this one
/// wrote — by then routinely tens of MB. Renaming it to an archive named after
/// its own last-modified day (rather than deleting it, or leaving it) puts it
/// under the same `RETAIN_DAYS` policy as everything else, so it disappears on
/// its own instead of sitting there forever as the one file nothing manages.
///
/// If that name is already taken the legacy file is left exactly where it is:
/// that means this day was already archived once, and a rename would silently
/// destroy a log rather than merely postpone tidying one up.
fn archive_legacy(dir: &Path) {
    let legacy = dir.join("app.log");
    let Ok(meta) = std::fs::metadata(&legacy) else {
        return;
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or_else(now_secs, |d| d.as_secs() as i64);
    let target = dir.join(format!("app-{}.legacy.log", day_string(modified)));
    if target.exists() {
        return;
    }
    let _ = std::fs::rename(&legacy, &target);
}

/// Local `YYYY-MM-DD` for a unix second. `localtime_r` is the reentrant form
/// and carries the platform's own DST/timezone rules — the same call the tray's
/// `local_parts` uses for its absolute "last synced HH:MM".
#[cfg(unix)]
fn local_day(secs: i64) -> Option<String> {
    let t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return None;
    }
    Some(format!(
        "{:04}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday
    ))
}

/// Windows: unix seconds → FILETIME (1601 epoch, 100 ns ticks) → UTC SYSTEMTIME
/// → local SYSTEMTIME. Via `SystemTimeToTzSpecificLocalTime` rather than
/// `FileTimeToLocalFileTime`, for the reason spelled out on `local_parts`: the
/// latter applies *today's* DST offset to a historical instant, which is an
/// hour wrong for half the year — and an hour wrong near midnight is a whole
/// day wrong here.
#[cfg(windows)]
fn local_day(secs: i64) -> Option<String> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{
        FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime, TIME_ZONE_INFORMATION,
    };

    let ticks = secs.checked_add(11_644_473_600)?.checked_mul(10_000_000)?;
    if ticks < 0 {
        return None;
    }
    let ticks = ticks as u64;
    let ft = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    if unsafe { FileTimeToSystemTime(&ft, &mut utc) } == 0 {
        return None;
    }
    let tz = std::ptr::null::<TIME_ZONE_INFORMATION>();
    if unsafe { SystemTimeToTzSpecificLocalTime(tz, &utc, &mut local) } == 0 {
        return None;
    }
    Some(format!(
        "{:04}-{:02}-{:02}",
        local.wYear, local.wMonth, local.wDay
    ))
}

#[cfg(all(not(unix), not(windows)))]
fn local_day(_secs: i64) -> Option<String> {
    None
}

/// RFC3339 millis UTC without pulling in `chrono` (src-tauri has no such dep).
/// Uses Howard Hinnant's days->civil algorithm.
fn now_rfc3339() -> String {
    let dur = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format_rfc3339(dur.as_secs() as i64, dur.subsec_millis())
}

fn format_rfc3339(epoch_secs: i64, millis: u32) -> String {
    let days = epoch_secs.div_euclid(86_400);
    let secs_of_day = epoch_secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (secs_of_day / 3600, (secs_of_day % 3600) / 60, secs_of_day % 60);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y, m, d, hh, mm, ss, millis
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn push(level: &str, message: String) {
    push_cat("core", "backend", level, message);
}

pub fn push_cat(category: &str, source: &str, level: &str, message: String) {
    eprintln!("[{category}] {message}"); // dev stderr mirror
    bus().record(LogLine {
        ts: now_rfc3339(),
        source: source.into(),
        category: category.into(),
        level: level.into(),
        message,
    });
}

pub fn snapshot() -> Vec<LogLine> {
    bus().buffer.lock().map(|b| b.iter().cloned().collect()).unwrap_or_default()
}

pub fn clear() {
    if let Ok(mut b) = bus().buffer.lock() {
        b.clear();
    }
}

pub fn init(app: AppHandle) {
    let _ = bus().app.set(app.clone());
    if let Ok(dir) = app.path().app_data_dir() {
        let logs_dir = dir.join("logs");
        let _ = std::fs::create_dir_all(&logs_dir);
        open_sink(&logs_dir);
    }
}

/// Point the bus at today's file and take out the trash. Split from `init` so
/// it is reachable from tests without an `AppHandle`.
fn open_sink(logs_dir: &Path) {
    archive_legacy(logs_dir);
    let secs = now_secs();
    let day = day_string(secs);
    let Some(file) = open_day_file(logs_dir, &day) else {
        // Could not open today's file: the directory is unusable (missing,
        // read-only, disk full), which is no state to be deleting other
        // people's files from. Leave the archives alone.
        return;
    };
    if let Ok(mut slot) = bus().sink.lock() {
        *slot = Some(Sink {
            dir: logs_dir.to_path_buf(),
            day,
            file,
            checked_secs: secs,
        });
    }
    prune_old_logs(logs_dir, &cutoff_day(secs));
}

#[tauri::command]
pub fn logs_append_frontend(level: String, message: String) {
    let level = if matches!(level.as_str(), "debug" | "info" | "warn" | "error") {
        level
    } else {
        "info".into()
    };
    bus().record(LogLine {
        ts: now_rfc3339(),
        source: "frontend".into(),
        category: "frontend".into(),
        level,
        message,
    });
}

#[tauri::command]
pub fn logs_get_snapshot() -> Vec<LogLine> {
    snapshot()
}

#[tauri::command]
pub fn logs_clear() {
    clear()
}

#[macro_export]
macro_rules! log_info {
    ($($a:tt)*) => { $crate::log_bus::push("info", format!($($a)*)) };
}
#[macro_export]
macro_rules! log_warn {
    ($($a:tt)*) => { $crate::log_bus::push("warn", format!($($a)*)) };
}
#[macro_export]
macro_rules! log_error {
    ($($a:tt)*) => { $crate::log_bus::push("error", format!($($a)*)) };
}
/// Category-tagged variant for git-sync / plugins. `$cat` and `$lvl` are string
/// literals; the rest is a `format!` payload.
#[macro_export]
macro_rules! log_cat {
    ($cat:expr, $lvl:expr, $($a:tt)*) => {
        $crate::log_bus::push_cat($cat, "backend", $lvl, format!($($a)*))
    };
}

// The process-global singleton bus is shared by every test that touches the log
// buffer — here AND in other modules (e.g. `notifications` mirrors a line here).
// They must all serialize on one lock, or `cargo test`'s parallelism lets a
// concurrent push/clear corrupt the ring-buffer assertions. Recover from poison
// so one panicking test doesn't cascade-fail the rest.
#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());
#[cfg(test)]
pub(crate) fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        super::test_guard()
    }

    #[test]
    fn ring_buffer_drops_oldest_and_keeps_newest() {
        let _g = guard();
        clear();
        for i in 0..(MAX_LINES + 5) {
            push_cat("core", "backend", "info", format!("line {i}"));
        }
        let snap = snapshot();
        assert_eq!(snap.len(), MAX_LINES);
        assert_eq!(snap.last().unwrap().message, format!("line {}", MAX_LINES + 4));
        clear();
    }

    #[test]
    fn clear_empties_buffer() {
        let _g = guard();
        clear();
        push_cat("core", "backend", "info", "hi".into());
        clear();
        assert!(snapshot().is_empty());
    }

    #[test]
    fn category_and_source_pass_through() {
        let _g = guard();
        clear();
        push_cat("git-sync", "backend", "warn", "conflict".into());
        let last = snapshot().pop().unwrap();
        assert_eq!(last.category, "git-sync");
        assert_eq!(last.source, "backend");
        assert_eq!(last.level, "warn");
        clear();
    }

    #[test]
    fn frontend_command_forces_category_and_defaults_bad_level() {
        let _g = guard();
        clear();
        logs_append_frontend("bogus".into(), "msg".into());
        let last = snapshot().pop().unwrap();
        assert_eq!(last.category, "frontend");
        assert_eq!(last.source, "frontend");
        assert_eq!(last.level, "info");
        clear();
    }

    #[test]
    fn rfc3339_matches_known_epoch() {
        // 2021-01-01T00:00:00.000Z == 1609459200 s
        assert_eq!(format_rfc3339(1_609_459_200, 0), "2021-01-01T00:00:00.000Z");
        assert_eq!(format_rfc3339(1_609_459_200, 456), "2021-01-01T00:00:00.456Z");
    }

    // ── daily rotation ────────────────────────────────────────────────────

    /// The retention sweep is a `remove_file` loop over a directory a user can
    /// put files in, so what it *refuses* to match matters more than what it
    /// matches: anything unrecognized must come back `None` and survive.
    #[test]
    fn only_dated_app_logs_are_recognized_for_retention() {
        assert_eq!(log_file_day("app-2026-08-13.log"), Some("2026-08-13"));
        assert_eq!(log_file_day("app-2026-08-13.legacy.log"), Some("2026-08-13"));
        // The pre-rotation file itself: managed by `archive_legacy`, never by
        // date comparison — it has no date to compare.
        assert_eq!(log_file_day("app.log"), None);
        assert_eq!(log_file_day("app-2026-08-13.txt"), None);
        assert_eq!(log_file_day("app-not-a-date.log"), None);
        assert_eq!(log_file_day("app-2026-8-13.log"), None);
        assert_eq!(log_file_day("notes.log"), None);
        assert_eq!(log_file_day("app-2026.log"), None);
    }

    #[test]
    fn prune_deletes_only_files_older_than_the_cutoff() {
        let dir = tempfile::tempdir().unwrap();
        let names = [
            "app-2026-08-01.log",       // older  → gone
            "app-2026-08-05.legacy.log", // older  → gone
            "app-2026-08-06.log",       // == cutoff → kept (strictly older only)
            "app-2026-08-13.log",       // newer  → kept
            "app.log",                  // unrecognized → kept
            "notes.txt",                // user's own → kept
        ];
        for n in names {
            std::fs::write(dir.path().join(n), b"x").unwrap();
        }
        prune_old_logs(dir.path(), "2026-08-06");

        let left: std::collections::BTreeSet<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        let expected: std::collections::BTreeSet<String> =
            ["app-2026-08-06.log", "app-2026-08-13.log", "app.log", "notes.txt"]
                .into_iter()
                .map(String::from)
                .collect();
        assert_eq!(left, expected);
    }

    /// `RETAIN_DAYS` worth of days back, expressed the same way file names are
    /// — so a file named for the cutoff day is exactly on the edge and stays.
    #[test]
    fn cutoff_is_retain_days_behind_today() {
        let now = now_secs();
        assert_eq!(cutoff_day(now), day_string(now - RETAIN_DAYS * 86_400));
        assert!(cutoff_day(now) < day_string(now));
    }

    /// The legacy single-file log becomes an archive named for its own mtime,
    /// which is what puts it under the retention policy.
    #[test]
    fn legacy_app_log_is_archived_under_a_dated_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.log"), b"old lines").unwrap();

        archive_legacy(dir.path());

        assert!(!dir.path().join("app.log").exists(), "legacy file moved");
        let archived: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(archived.len(), 1);
        assert!(archived[0].ends_with(".legacy.log"), "{archived:?}");
        assert!(log_file_day(&archived[0]).is_some(), "must be prunable: {archived:?}");
        assert_eq!(std::fs::read(dir.path().join(&archived[0])).unwrap(), b"old lines");
    }

    /// Same day, second run: the archive already exists. Overwriting it would
    /// destroy a log, so the legacy file is left alone instead.
    #[test]
    fn legacy_archive_never_overwrites_an_existing_one() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.log"), b"new").unwrap();
        let taken = dir.path().join(format!("app-{}.legacy.log", day_string(now_secs())));
        std::fs::write(&taken, b"already archived").unwrap();

        archive_legacy(dir.path());

        assert_eq!(std::fs::read(&taken).unwrap(), b"already archived");
        assert_eq!(std::fs::read(dir.path().join("app.log")).unwrap(), b"new");
    }

    /// A day that has rolled over gets a new file, and the write lands in it —
    /// the whole point of the feature, asserted end to end on a real `Sink`.
    #[test]
    fn sink_rolls_to_a_new_file_when_the_day_changes() {
        let dir = tempfile::tempdir().unwrap();
        let today = day_string(now_secs());
        let mut sink = Sink {
            dir: dir.path().to_path_buf(),
            day: "1999-12-31".into(), // pretend the process started last century
            file: open_day_file(dir.path(), "1999-12-31").unwrap(),
            checked_secs: 0,
        };

        sink.roll_if_new_day();

        assert_eq!(sink.day, today);
        writeln!(sink.file, "after midnight").unwrap();
        let landed = std::fs::read_to_string(dir.path().join(day_file_name(&today))).unwrap();
        assert!(landed.contains("after midnight"), "{landed:?}");
        // …and the 1999 file was old enough to be swept by the same roll.
        assert!(!dir.path().join("app-1999-12-31.log").exists());
    }

    /// Within one second the day cannot have changed, so the sink must not
    /// re-derive it — the fast path every log line takes.
    #[test]
    fn sink_skips_the_day_check_within_the_same_second() {
        let dir = tempfile::tempdir().unwrap();
        let mut sink = Sink {
            dir: dir.path().to_path_buf(),
            day: "1999-12-31".into(),
            file: open_day_file(dir.path(), "1999-12-31").unwrap(),
            checked_secs: now_secs(),
        };

        sink.roll_if_new_day();

        assert_eq!(sink.day, "1999-12-31", "no roll inside the cached second");
    }

    /// Local, not UTC — the reason the file name and the line timestamps use
    /// different clocks. Only assertable where the two genuinely differ, so
    /// this pins the shape everywhere and the value where the offset is known.
    #[test]
    fn day_string_is_the_local_calendar_day() {
        // 2026-01-01T00:30:00Z — already the 1st in UTC, still 2025 in any
        // timezone west of UTC, and the 1st in the east.
        let s = day_string(1_767_227_400);
        assert_eq!(s.len(), 10, "{s}");
        assert!(s.starts_with("202"), "{s}");
        if let Some(local) = local_day(1_767_227_400) {
            assert_eq!(s, local);
            assert!(
                ["2025-12-31", "2026-01-01", "2026-01-02"].contains(&s.as_str()),
                "local day must be within a day of the UTC one: {s}"
            );
        }
    }
}
