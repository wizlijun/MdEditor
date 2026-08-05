//! Unified log bus: merges backend main-process + frontend webview + git-sync +
//! plugin sources into a single ring buffer, lands them in `logs/app.log`, and
//! emits `log://line` to the "View Logs" window. Only NEW log calls flow in —
//! existing vault_sync/plugin stores are untouched, they mirror one line here.
use serde::Serialize;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const MAX_LINES: usize = 3000;

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
    file: Mutex<Option<File>>,
}

impl LogBus {
    fn new() -> Self {
        LogBus {
            buffer: Mutex::new(VecDeque::with_capacity(MAX_LINES)),
            app: OnceLock::new(),
            file: Mutex::new(None),
        }
    }

    fn record(&self, line: LogLine) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push_back(line.clone());
            while buf.len() > MAX_LINES {
                buf.pop_front();
            }
        }
        if let Ok(mut f) = self.file.lock() {
            if let Some(file) = f.as_mut() {
                let _ = writeln!(
                    file,
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
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(logs_dir.join("app.log")) {
            if let Ok(mut slot) = bus().file.lock() {
                *slot = Some(file);
            }
        }
    }
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
}
