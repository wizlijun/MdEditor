//! One JSON file per run. The full event stream is deliberately NOT persisted —
//! it exists for the window to watch live; keeping it would only add noise to
//! the vault.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const RESULT_LIMIT: usize = 8 * 1024;
pub const STDERR_LIMIT: usize = 2 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Success,
    /// The precheck said there was nothing to do; no model ran.
    Skipped,
    Error,
    Timeout,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub task: String,
    /// "window" | "cli"
    pub trigger: String,
    pub started_at: String,
    pub ended_at: String,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub num_turns: Option<u64>,
    pub session_id: Option<String>,
    pub result: String,
    pub stderr_tail: String,
    /// Vault-relative markdown a run produced, for `host.editor.open`. Default
    /// keeps records written before this field readable.
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// WHICH agent performed this run — `notemd.claude-agent`,
    /// `notemd.deepseek-agent`.
    ///
    /// Both agent plugins share one tasks root AND one runs root on purpose (a
    /// task is a job description, not a binding to one model), which means every
    /// window lists every run. Without this field a claude run shows up in the
    /// DeepSeek Agent window looking exactly like a deepseek run — which is how
    /// an expired *Claude* credential got read as a DeepSeek failure. `None` on
    /// records written before the field existed: unknown, not "mine".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
}

pub fn runs_dir(task_run_dir: &Path) -> PathBuf {
    task_run_dir.join("runs")
}

/// A live snapshot, rewritten as the run progresses. It exists because progress
/// has to be visible ACROSS processes: the main window polls it, and the run it
/// is watching may belong to a detached CLI runner it has no channel to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Progress {
    pub run_id: String,
    /// Assistant turns seen so far — what the run has actually done.
    pub steps: u64,
    /// Newest activity, e.g. "Read a.note.md".
    pub last: String,
    pub updated_at: String,
}

pub fn progress_path(task_run_dir: &Path) -> PathBuf {
    task_run_dir.join("progress.json")
}

pub fn write_progress(task_run_dir: &Path, p: &Progress) {
    let _ = std::fs::create_dir_all(task_run_dir);
    if let Ok(s) = serde_json::to_string(p) {
        let _ = std::fs::write(progress_path(task_run_dir), s);
    }
}

pub fn read_progress(task_run_dir: &Path) -> Option<Progress> {
    serde_json::from_str(&std::fs::read_to_string(progress_path(task_run_dir)).ok()?).ok()
}

pub fn clear_progress(task_run_dir: &Path) {
    let _ = std::fs::remove_file(progress_path(task_run_dir));
}

/// Cap a single run's log. Enough to read what happened, small enough that a
/// runaway task can't fill the vault.
pub const LOG_LIMIT: usize = 256 * 1024;

pub fn log_path(task_run_dir: &Path, run_id: &str) -> PathBuf {
    runs_dir(task_run_dir).join(format!("{run_id}.log"))
}

/// Append one line to a run's log, stopping once it hits the cap.
pub fn append_log(task_run_dir: &Path, run_id: &str, line: &str) {
    let p = log_path(task_run_dir, run_id);
    if std::fs::metadata(&p).map(|m| m.len() as usize).unwrap_or(0) >= LOG_LIMIT {
        return;
    }
    let _ = std::fs::create_dir_all(runs_dir(task_run_dir));
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = writeln!(f, "{line}");
    }
}

pub fn read_log(task_run_dir: &Path, run_id: &str) -> Option<String> {
    std::fs::read_to_string(log_path(task_run_dir, run_id)).ok()
}

/// Forget one run entirely: its record and its log.
pub fn delete(task_run_dir: &Path, run_id: &str) -> bool {
    let rec = runs_dir(task_run_dir).join(format!("{run_id}.json"));
    let gone = std::fs::remove_file(&rec).is_ok();
    let _ = std::fs::remove_file(log_path(task_run_dir, run_id));
    gone
}

/// Forget every run of a task. Returns how many records were removed.
pub fn clear(task_run_dir: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(runs_dir(task_run_dir)) else {
        return 0;
    };
    let mut n = 0;
    for e in rd.flatten() {
        let p = e.path();
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext == "json" || ext == "log" {
            if std::fs::remove_file(&p).is_ok() && ext == "json" {
                n += 1;
            }
        }
    }
    n
}

/// Find one run's record by id, whichever task it belongs to.
pub fn find(task_run_dir: &Path, run_id: &str) -> Option<RunRecord> {
    let p = runs_dir(task_run_dir).join(format!("{run_id}.json"));
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

/// Keep the END of the string — that's where the failure reason lives. Snaps
/// forward to a char boundary so multi-byte characters never get sliced apart.
pub fn tail(s: &str, limit: usize) -> String {
    if s.len() <= limit {
        return s.to_string();
    }
    let mut start = s.len() - limit;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}

pub fn write(task_run_dir: &Path, rec: &RunRecord) -> std::io::Result<PathBuf> {
    let d = runs_dir(task_run_dir);
    std::fs::create_dir_all(&d)?;
    let p = d.join(format!("{}.json", rec.run_id));
    std::fs::write(&p, serde_json::to_string_pretty(rec).unwrap() + "\n")?;
    Ok(p)
}

/// The most recent N, newest first (a run id starts with a UTC timestamp, so
/// lexical order is chronological order).
pub fn recent(task_run_dir: &Path, n: usize) -> Vec<RunRecord> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(runs_dir(task_run_dir))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files.reverse();
    files
        .into_iter()
        .take(n)
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect()
}

/// The most recent N across EVERY task under `runs_root`, newest first. Each
/// record carries its own `task`, so the caller can label the rows.
pub fn recent_all(runs_root: &std::path::Path, n: usize) -> Vec<RunRecord> {
    let Ok(rd) = std::fs::read_dir(runs_root) else {
        return Vec::new();
    };
    let mut all: Vec<RunRecord> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .flat_map(|e| recent(&e.path(), n))
        .collect();
    // run_id starts with a UTC timestamp, so this is a chronological sort.
    all.sort_by(|a, b| b.run_id.cmp(&a.run_id));
    all.truncate(n);
    all
}

/// `20260730T104233Z-<low 24 bits of the pid, hex>`: lexical order is time
/// order, and two processes in the same second don't collide.
pub fn new_run_id(now: chrono::DateTime<chrono::Utc>, pid: u32) -> String {
    format!("{}-{:06x}", now.format("%Y%m%dT%H%M%SZ"), pid & 0xff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str) -> RunRecord {
        RunRecord {
            run_id: id.into(),
            task: "t".into(),
            trigger: "window".into(),
            started_at: "a".into(),
            ended_at: "b".into(),
            status: Status::Success,
            exit_code: Some(0),
            num_turns: Some(3),
            session_id: None,
            result: "ok".into(),
            stderr_tail: String::new(),
            artifacts: Vec::new(),
            harness: Some("notemd.claude-agent".into()),
        }
    }

    #[test]
    fn round_trips_a_record_through_disk() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &rec("20260730T000000Z-000001")).unwrap();
        let got = recent(d.path(), 10);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].status, Status::Success);
        assert_eq!(got[0].result, "ok");
    }

    #[test]
    fn recent_returns_newest_first_and_respects_the_limit() {
        let d = tempfile::tempdir().unwrap();
        for id in [
            "20260730T000001Z-a",
            "20260730T000002Z-b",
            "20260730T000003Z-c",
        ] {
            write(d.path(), &rec(id)).unwrap();
        }
        let got = recent(d.path(), 2);
        assert_eq!(
            got.iter().map(|r| r.run_id.as_str()).collect::<Vec<_>>(),
            vec!["20260730T000003Z-c", "20260730T000002Z-b"]
        );
    }

    #[test]
    fn tail_keeps_the_end_and_never_splits_a_utf8_char() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("abc", 10), "abc");
        let s = "问题问题问题"; // 3 bytes per char
        let got = tail(s, 7);
        assert!(got.len() <= 7);
        assert!(s.ends_with(&got));
        assert_eq!(got, "问题");
    }

    #[test]
    fn run_ids_sort_chronologically() {
        use chrono::TimeZone;
        let a = new_run_id(chrono::Utc.with_ymd_and_hms(2026, 7, 30, 1, 0, 0).unwrap(), 1);
        let b = new_run_id(chrono::Utc.with_ymd_and_hms(2026, 7, 30, 2, 0, 0).unwrap(), 1);
        assert!(a < b);
    }

    #[test]
    fn recent_is_empty_when_nothing_ran_yet() {
        let d = tempfile::tempdir().unwrap();
        assert!(recent(d.path(), 5).is_empty());
    }

    #[test]
    fn a_run_log_appends_and_reads_back() {
        let d = tempfile::tempdir().unwrap();
        append_log(d.path(), "R1", "Read a.md");
        append_log(d.path(), "R1", "Write answers/a.md");
        assert_eq!(read_log(d.path(), "R1").unwrap(), "Read a.md\nWrite answers/a.md\n");
        assert_eq!(read_log(d.path(), "R2"), None);
    }

    #[test]
    fn a_run_log_stops_growing_at_the_cap() {
        let d = tempfile::tempdir().unwrap();
        let chunk = "x".repeat(8 * 1024);
        for _ in 0..64 {
            append_log(d.path(), "R1", &chunk);
        }
        let len = read_log(d.path(), "R1").unwrap().len();
        assert!(len >= LOG_LIMIT, "should fill up to the cap, got {len}");
        assert!(len < LOG_LIMIT + 9 * 1024, "should stop just past it, got {len}");
    }

    #[test]
    fn deleting_a_run_takes_its_log_with_it() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &rec("R1")).unwrap();
        append_log(d.path(), "R1", "Read a.md");
        assert!(delete(d.path(), "R1"));
        assert!(recent(d.path(), 5).is_empty());
        assert_eq!(read_log(d.path(), "R1"), None);
        // Deleting what isn't there is a no-op, not an error.
        assert!(!delete(d.path(), "R1"));
    }

    #[test]
    fn clearing_removes_every_run_of_a_task() {
        let d = tempfile::tempdir().unwrap();
        for id in ["R1", "R2", "R3"] {
            write(d.path(), &rec(id)).unwrap();
            append_log(d.path(), id, "line");
        }
        assert_eq!(clear(d.path()), 3);
        assert!(recent(d.path(), 10).is_empty());
        assert_eq!(read_log(d.path(), "R1"), None);
        assert_eq!(clear(d.path()), 0);
    }

    #[test]
    fn recent_all_merges_every_task_newest_first() {
        let root = tempfile::tempdir().unwrap();
        let mut a = rec("20260730T000001Z-a");
        a.task = "alpha".into();
        let mut b = rec("20260730T000003Z-b");
        b.task = "beta".into();
        let mut c = rec("20260730T000002Z-c");
        c.task = "alpha".into();
        write(&root.path().join("alpha"), &a).unwrap();
        write(&root.path().join("beta"), &b).unwrap();
        write(&root.path().join("alpha"), &c).unwrap();

        let got = recent_all(root.path(), 10);
        assert_eq!(
            got.iter().map(|r| r.task.as_str()).collect::<Vec<_>>(),
            vec!["beta", "alpha", "alpha"]
        );
    }

    #[test]
    fn recent_all_respects_the_limit_across_tasks() {
        let root = tempfile::tempdir().unwrap();
        for (i, task) in ["alpha", "beta", "gamma"].iter().enumerate() {
            let mut r = rec(&format!("20260730T00000{i}Z-x"));
            r.task = (*task).into();
            write(&root.path().join(task), &r).unwrap();
        }
        assert_eq!(recent_all(root.path(), 2).len(), 2);
    }

    #[test]
    fn recent_all_is_empty_when_no_task_has_ever_run() {
        let root = tempfile::tempdir().unwrap();
        assert!(recent_all(root.path(), 5).is_empty());
        assert!(recent_all(&root.path().join("nope"), 5).is_empty());
    }

    #[test]
    fn recent_skips_a_corrupt_record_instead_of_failing() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &rec("20260730T000001Z-a")).unwrap();
        std::fs::write(runs_dir(d.path()).join("20260730T000002Z-b.json"), "{oops").unwrap();
        assert_eq!(recent(d.path(), 10).len(), 1);
    }
}
