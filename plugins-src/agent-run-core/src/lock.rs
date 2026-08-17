//! Same task mutually exclusive, different tasks in parallel. The lock is a
//! JSON file in the task's run dir; a lock left behind by a crashed process is
//! reclaimed by checking whether its pid is still alive (otherwise one crash
//! would wedge a task forever).
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: i32,
    pub run_id: String,
    pub started_at: String,
}

#[derive(Debug)]
pub struct Busy(pub LockInfo);

/// Held for the duration of a run; dropping it releases the lock, so no error
/// path has to remember to clean up.
pub struct Guard {
    path: PathBuf,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn lock_path(task_run_dir: &Path) -> PathBuf {
    task_run_dir.join("lock")
}

pub fn acquire_with(
    task_run_dir: &Path,
    info: LockInfo,
    alive: impl Fn(i32) -> bool,
) -> Result<Guard, Busy> {
    let p = lock_path(task_run_dir);
    if let Ok(s) = std::fs::read_to_string(&p) {
        if let Ok(cur) = serde_json::from_str::<LockInfo>(&s) {
            if alive(cur.pid) {
                return Err(Busy(cur));
            }
            // Stale lock: the holder is gone, take it over.
        }
    }
    let _ = std::fs::create_dir_all(task_run_dir);
    let _ = std::fs::write(&p, serde_json::to_string(&info).unwrap());
    Ok(Guard { path: p })
}

pub fn acquire(task_run_dir: &Path, info: LockInfo) -> Result<Guard, Busy> {
    acquire_with(task_run_dir, info, pid_alive)
}

/// Who currently holds this task's lock, if anyone. Reading the lock file
/// rather than an in-memory map is what lets the window see runs started by a
/// DETACHED CLI process — those live in a different process entirely.
pub fn current(task_run_dir: &Path) -> Option<LockInfo> {
    current_with(task_run_dir, pid_alive)
}

pub fn current_with(task_run_dir: &Path, alive: impl Fn(i32) -> bool) -> Option<LockInfo> {
    let s = std::fs::read_to_string(lock_path(task_run_dir)).ok()?;
    let info: LockInfo = serde_json::from_str(&s).ok()?;
    alive(info.pid).then_some(info)
}

/// `kill(pid, 0)` is the POSIX liveness probe: sends nothing, just checks that
/// the process exists and is signalable.
pub fn pid_alive(pid: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pid: i32) -> LockInfo {
        LockInfo {
            pid,
            run_id: "r1".into(),
            started_at: "2026-07-30T00:00:00Z".into(),
        }
    }

    #[test]
    fn acquires_on_a_clean_dir_and_releases_on_drop() {
        let d = tempfile::tempdir().unwrap();
        let g = acquire_with(d.path(), info(1), |_| true).unwrap();
        assert!(lock_path(d.path()).exists());
        drop(g);
        assert!(!lock_path(d.path()).exists());
    }

    #[test]
    fn refuses_when_the_holder_is_still_alive() {
        let d = tempfile::tempdir().unwrap();
        let _g = acquire_with(d.path(), info(4242), |_| true).unwrap();
        match acquire_with(d.path(), info(9999), |_| true) {
            Err(Busy(cur)) => assert_eq!(cur.pid, 4242),
            Ok(_) => panic!("expected the second acquire to be refused"),
        }
    }

    #[test]
    fn reclaims_a_stale_lock_whose_holder_died() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path()).unwrap();
        std::fs::write(
            lock_path(d.path()),
            serde_json::to_string(&info(4242)).unwrap(),
        )
        .unwrap();
        let _g = acquire_with(d.path(), info(9999), |_| false).unwrap();
        let cur: LockInfo =
            serde_json::from_str(&std::fs::read_to_string(lock_path(d.path())).unwrap()).unwrap();
        assert_eq!(cur.pid, 9999);
    }

    #[test]
    fn treats_a_corrupt_lock_file_as_reclaimable() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(lock_path(d.path()), "{garbage").unwrap();
        assert!(acquire_with(d.path(), info(1), |_| true).is_ok());
    }

    #[test]
    fn creates_the_run_dir_when_it_does_not_exist_yet() {
        let d = tempfile::tempdir().unwrap();
        let nested = d.path().join("a/b/c");
        let _g = acquire_with(&nested, info(1), |_| true).unwrap();
        assert!(lock_path(&nested).exists());
    }

    #[test]
    fn current_reports_the_live_holder() {
        let d = tempfile::tempdir().unwrap();
        assert!(current_with(d.path(), |_| true).is_none());
        let _g = acquire_with(d.path(), info(4242), |_| true).unwrap();
        assert_eq!(current_with(d.path(), |_| true).unwrap().pid, 4242);
    }

    #[test]
    fn current_reads_a_stale_lock_as_free() {
        let d = tempfile::tempdir().unwrap();
        let _g = acquire_with(d.path(), info(4242), |_| true).unwrap();
        assert!(current_with(d.path(), |_| false).is_none());
    }

    #[test]
    fn pid_alive_says_yes_for_our_own_process() {
        assert!(pid_alive(std::process::id() as i32));
    }
}
