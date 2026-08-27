//! Ordinary tasks are mutually exclusive. A safely scoped task can share the
//! legacy task gate while taking an exclusive lock for one concrete target.
//! The kernel `flock` is the authority; JSON only identifies holders for UI.
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
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
    files: Vec<File>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        // Keep the inode in place. Unlinking a flocked path lets another
        // process create and lock a second inode before this guard is dropped,
        // which would put two owners behind the same pathname.
        for file in self.files.iter().rev() {
            let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        }
    }
}

pub fn lock_path(task_run_dir: &Path) -> PathBuf {
    task_run_dir.join("lock")
}

fn scoped_locks_dir(task_run_dir: &Path) -> PathBuf {
    task_run_dir.join("locks")
}

/// Stable, filename-safe identity for one scoped target. A collision only
/// serializes two unrelated targets; it can never allow the same target twice.
fn scope_name(scope: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in scope.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}.lock")
}

fn scoped_lock_path(task_run_dir: &Path, scope: &str) -> PathBuf {
    scoped_locks_dir(task_run_dir).join(scope_name(scope))
}

/// The only task allowed to run concurrently today. The returned key is the
/// canonical vault-relative source path, so every provider derives the same
/// lock for the same book without trusting caller-supplied lock metadata.
pub fn scoped_target(task_id: &str, vault: &Path, target: Option<&str>) -> Option<String> {
    if task_id != "ai-read-ebook" {
        return None;
    }
    let root = vault.canonicalize().ok()?;
    let target = Path::new(target?);
    let absolute = if target.is_absolute() {
        target.canonicalize().ok()?
    } else {
        root.join(target).canonicalize().ok()?
    };
    let rel = absolute.strip_prefix(&root).ok()?;
    let key = rel.to_string_lossy().replace('\\', "/");
    (!key.is_empty()).then_some(key)
}

pub fn acquire_with(
    task_run_dir: &Path,
    info: LockInfo,
    _alive: impl Fn(i32) -> bool,
) -> Result<Guard, Busy> {
    let p = lock_path(task_run_dir);
    let _ = std::fs::create_dir_all(task_run_dir);
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&p)
        .map_err(|_| Busy(info.clone()))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(Busy(read_info(&mut file).unwrap_or(LockInfo {
            pid: 0,
            run_id: "unknown".into(),
            started_at: String::new(),
        })));
    }

    file.set_len(0).map_err(|_| Busy(info.clone()))?;
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.write_all(serde_json::to_string(&info).unwrap().as_bytes()))
        .and_then(|_| file.sync_data())
        .map_err(|_| Busy(info))?;
    Ok(Guard { files: vec![file] })
}

pub fn acquire(task_run_dir: &Path, info: LockInfo) -> Result<Guard, Busy> {
    acquire_with(task_run_dir, info, pid_alive)
}

/// Share the legacy task gate, then exclusively own one target. Sharing the
/// old `lock` inode is intentional: an older provider still taking `LOCK_EX`
/// remains mutually exclusive with new scoped runs during rolling upgrades.
pub fn acquire_scoped(task_run_dir: &Path, scope: &str, info: LockInfo) -> Result<Guard, Busy> {
    let _ = std::fs::create_dir_all(scoped_locks_dir(task_run_dir));
    let mut gate = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path(task_run_dir))
        .map_err(|_| Busy(info.clone()))?;
    if unsafe { libc::flock(gate.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } != 0 {
        return Err(Busy(read_info(&mut gate).unwrap_or(LockInfo {
            pid: 0,
            run_id: "unknown".into(),
            started_at: String::new(),
        })));
    }

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(scoped_lock_path(task_run_dir, scope))
        .map_err(|_| Busy(info.clone()))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(Busy(read_info(&mut file).unwrap_or(LockInfo {
            pid: 0,
            run_id: "unknown".into(),
            started_at: String::new(),
        })));
    }
    file.set_len(0).map_err(|_| Busy(info.clone()))?;
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.write_all(serde_json::to_string(&info).unwrap().as_bytes()))
        .and_then(|_| file.sync_data())
        .map_err(|_| Busy(info))?;
    Ok(Guard {
        files: vec![gate, file],
    })
}

pub fn acquire_for_run(
    task_run_dir: &Path,
    task_id: &str,
    vault: &Path,
    target: Option<&str>,
    info: LockInfo,
) -> Result<Guard, Busy> {
    match scoped_target(task_id, vault, target) {
        Some(scope) => acquire_scoped(task_run_dir, &scope, info),
        None => acquire(task_run_dir, info),
    }
}

/// Who currently holds this task's lock, if anyone. Reading the lock file
/// rather than an in-memory map is what lets the window see runs started by a
/// DETACHED CLI process — those live in a different process entirely.
pub fn current(task_run_dir: &Path) -> Option<LockInfo> {
    current_all(task_run_dir).into_iter().next()
}

pub fn current_with(task_run_dir: &Path, alive: impl Fn(i32) -> bool) -> Option<LockInfo> {
    current_all_with(task_run_dir, &alive).into_iter().next()
}

pub fn current_all(task_run_dir: &Path) -> Vec<LockInfo> {
    current_all_with(task_run_dir, &pid_alive)
}

pub fn current_for_run(task_run_dir: &Path, run_id: &str) -> Option<LockInfo> {
    current_all(task_run_dir)
        .into_iter()
        .find(|info| info.run_id == run_id)
}

fn current_all_with(task_run_dir: &Path, alive: &impl Fn(i32) -> bool) -> Vec<LockInfo> {
    let mut out = Vec::new();
    if let Some(info) = current_exclusive_with(task_run_dir, alive) {
        out.push(info);
    }
    let Ok(entries) = std::fs::read_dir(scoped_locks_dir(task_run_dir)) else {
        return out;
    };
    for entry in entries.flatten() {
        if let Some(info) = current_file_with(&entry.path(), libc::LOCK_EX, alive) {
            out.push(info);
        }
    }
    out.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    out
}

/// Probe with SH, not EX: new scoped runs hold the legacy gate in shared mode
/// and must not make stale JSON in that file look like a live exclusive run.
fn current_exclusive_with(task_run_dir: &Path, alive: &impl Fn(i32) -> bool) -> Option<LockInfo> {
    current_file_with(&lock_path(task_run_dir), libc::LOCK_SH, alive)
}

fn current_file_with(path: &Path, probe: i32, alive: &impl Fn(i32) -> bool) -> Option<LockInfo> {
    let mut file = OpenOptions::new().read(true).write(true).open(path).ok()?;
    if unsafe { libc::flock(file.as_raw_fd(), probe | libc::LOCK_NB) } == 0 {
        let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
        return None;
    }
    let info = read_info(&mut file)?;
    alive(info.pid).then_some(info)
}

fn read_info(file: &mut File) -> Option<LockInfo> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut s = String::new();
    file.read_to_string(&mut s).ok()?;
    serde_json::from_str(&s).ok()
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
        assert!(lock_path(d.path()).exists());
        assert!(current_with(d.path(), |_| true).is_none());
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
    fn simultaneous_clean_acquires_have_exactly_one_winner() {
        use std::sync::{Arc, Barrier};

        let d = tempfile::tempdir().unwrap();
        let path = Arc::new(d.path().to_path_buf());
        let start = Arc::new(Barrier::new(16));
        let attempted = Arc::new(Barrier::new(16));
        let mut threads = Vec::new();
        for pid in 1..=16 {
            let path = path.clone();
            let start = start.clone();
            let attempted = attempted.clone();
            threads.push(std::thread::spawn(move || {
                start.wait();
                let guard = acquire_with(&path, info(pid), |_| true).ok();
                attempted.wait();
                guard.is_some()
            }));
        }
        let winners = threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .filter(|won| *won)
            .count();
        assert_eq!(winners, 1);
    }

    #[test]
    fn different_scopes_share_the_task_gate_but_the_same_scope_is_busy() {
        let d = tempfile::tempdir().unwrap();
        let a = acquire_scoped(d.path(), "books/a/book.md", info(1)).unwrap();
        let mut second = info(2);
        second.run_id = "r2".into();
        second.started_at = "2026-07-30T00:00:01Z".into();
        let b = acquire_scoped(d.path(), "books/b/book.md", second).unwrap();
        assert_eq!(current_all_with(d.path(), &|_| true).len(), 2);
        assert_eq!(current_with(d.path(), |_| true).unwrap().pid, 1);
        assert_eq!(
            current_all_with(d.path(), &|_| true)
                .into_iter()
                .find(|run| run.run_id == "r1")
                .unwrap()
                .pid,
            1
        );
        match acquire_scoped(d.path(), "books/a/book.md", info(3)) {
            Err(Busy(cur)) => assert_eq!(cur.pid, 1),
            Ok(_) => panic!("the same scoped target must stay exclusive"),
        }
        drop((a, b));
        assert!(current_all_with(d.path(), &|_| true).is_empty());
    }

    #[test]
    fn ordinary_exclusive_and_scoped_runs_block_each_other() {
        let d = tempfile::tempdir().unwrap();
        let exclusive = acquire_with(d.path(), info(1), |_| true).unwrap();
        assert!(acquire_scoped(d.path(), "books/a", info(2)).is_err());
        drop(exclusive);

        let scoped = acquire_scoped(d.path(), "books/a", info(2)).unwrap();
        assert!(acquire_with(d.path(), info(1), |_| true).is_err());
        drop(scoped);
        assert!(acquire_with(d.path(), info(3), |_| true).is_ok());
    }

    #[test]
    fn scoped_target_is_only_enabled_for_a_real_ai_read_file_inside_the_vault() {
        let vault = tempfile::tempdir().unwrap();
        let book = vault.path().join("books/a/book.md");
        std::fs::create_dir_all(book.parent().unwrap()).unwrap();
        std::fs::write(&book, "book").unwrap();
        assert_eq!(
            scoped_target("ai-read-ebook", vault.path(), book.to_str()),
            Some("books/a/book.md".into())
        );
        assert_eq!(scoped_target("other", vault.path(), book.to_str()), None);
        assert_eq!(
            scoped_target("ai-read-ebook", vault.path(), Some("../outside.md")),
            None
        );
    }

    #[test]
    fn pid_alive_says_yes_for_our_own_process() {
        assert!(pid_alive(std::process::id() as i32));
    }
}
