use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use super::{git_ops, watcher, SyncState, VaultSyncManager};

/// vault 内连续这么久没有文件变动,才认为这一阵编辑告一段落、可以同步。
const QUIET_PERIOD: Duration = Duration::from_secs(10);
/// 一直不停笔时的兜底:距首个事件超过这么久就无条件同步一次。
const MAX_DEFER: Duration = Duration::from_secs(120);

pub fn start(app: &AppHandle) -> Result<(), String> {
    let mgr = app.state::<Arc<VaultSyncManager>>();
    let repo_path = mgr.repo_path.lock().unwrap().clone()
        .ok_or("Vault sync not configured: no repo_path")?;
    let repo = PathBuf::from(&repo_path);

    if !repo.join(".git").exists() {
        return Err(format!("Not a git repo: {repo_path}"));
    }

    {
        let mut stop = mgr.stop_flag.lock().unwrap();
        *stop = false;
    }
    set_state(app, SyncState::Running);
    mgr.logs.push("INFO", "Sync started");

    let app_handle = app.clone();
    let remote = mgr.remote.clone();
    let branch = mgr.branch.clone();

    std::thread::spawn(move || {
        run_loop(app_handle, repo, remote, branch);
    });

    Ok(())
}

pub fn stop(app: &AppHandle) -> Result<(), String> {
    let mgr = app.state::<Arc<VaultSyncManager>>();
    {
        let mut stop = mgr.stop_flag.lock().unwrap();
        *stop = true;
    }
    set_state(app, SyncState::Stopped);
    mgr.logs.push("INFO", "Sync stopped");
    Ok(())
}

pub fn sync_once(app: &AppHandle) -> Result<(), String> {
    let mgr = app.state::<Arc<VaultSyncManager>>();
    let repo_path = mgr.repo_path.lock().unwrap().clone()
        .ok_or("Not configured")?;
    let repo = PathBuf::from(&repo_path);
    let remote = mgr.remote.clone();
    let branch = mgr.branch.clone();

    match mgr.sync_gate.try_lock() {
        Ok(_guard) => do_sync(app, &repo, &remote, &branch),
        Err(_) => mgr.logs.push("INFO", "sync already in progress, skipped"),
    }
    Ok(())
}

fn run_loop(app: AppHandle, repo: PathBuf, remote: String, branch: String) {
    let (tx, rx) = std::sync::mpsc::channel::<()>();

    let _watcher = match watcher::start(&repo, tx.clone()) {
        Ok(w) => w,
        Err(e) => {
            let mgr = app.state::<Arc<VaultSyncManager>>();
            mgr.logs.push("ERROR", &format!("Watcher failed: {e}"));
            set_state(&app, SyncState::Error);
            return;
        }
    };

    // Initial sync immediately on start
    {
        let mgr = app.state::<Arc<VaultSyncManager>>();
        let _guard = mgr.sync_gate.lock().unwrap();
        do_sync(&app, &repo, &remote, &branch);
    }

    let tx_periodic = tx.clone();
    let app_for_periodic = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let mgr = app_for_periodic.state::<Arc<VaultSyncManager>>();
            if *mgr.stop_flag.lock().unwrap() {
                break;
            }
            let _ = tx_periodic.send(());
        }
    });

    loop {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(_) => {
                // 等编辑停下来再同步:每来一个文件事件就把窗口顺延,连续 QUIET_PERIOD
                // 没有动静才跑一轮。创作和大批量修改基本只发生在单台设备上,上传晚
                // 几十秒无所谓,但每 15 秒抢一轮 git 会在用户正打字时反复扰动仓库。
                // MAX_DEFER 兜底:长时间不停笔也要定期落一个检查点,不能无限推迟。
                let start = std::time::Instant::now();
                loop {
                    match rx.recv_timeout(QUIET_PERIOD) {
                        // 又有改动 —— 顺延,除非已经拖到上限。
                        Ok(_) if start.elapsed() < MAX_DEFER => continue,
                        _ => break,
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                let mgr = app.state::<Arc<VaultSyncManager>>();
                if *mgr.stop_flag.lock().unwrap() {
                    break;
                }
                continue;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        let mgr = app.state::<Arc<VaultSyncManager>>();
        if *mgr.stop_flag.lock().unwrap() {
            break;
        }

        {
            let mgr = app.state::<Arc<VaultSyncManager>>();
            let _guard = mgr.sync_gate.lock().unwrap();
            do_sync(&app, &repo, &remote, &branch);
        }
    }
}

fn do_sync(app: &AppHandle, repo: &PathBuf, remote: &str, branch: &str) {
    let mgr = app.state::<Arc<VaultSyncManager>>();

    // Guard: if `git` itself cannot be run, never report a healthy sync.
    match git_ops::version() {
        Some(ver) => {
            let was_unavailable = !*mgr.git_available.lock().unwrap();
            *mgr.git_available.lock().unwrap() = true;
            if was_unavailable {
                mgr.logs.push("INFO", &format!("git is available again: {ver}"));
            }
        }
        None => {
            *mgr.git_available.lock().unwrap() = false;
            let msg = "git executable not found on PATH — sync is paused";
            *mgr.error_msg.lock().unwrap() = Some(msg.to_string());
            set_state(app, SyncState::GitUnavailable);
            mgr.logs.push("ERROR", msg);
            let _ = app.emit("vault-sync-log", ());
            return;
        }
    }

    set_state(app, SyncState::Syncing);
    mgr.logs.push("INFO", "Syncing...");

    let head_before = git_ops::run_git(repo, &["rev-parse", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string());

    match git_ops::sync(repo, remote, branch) {
        Ok(report) => {
            *mgr.skipped_large_files.lock().unwrap() = report.skipped_large.clone();
            if !report.skipped_large.is_empty() {
                mgr.logs.push(
                    "WARN",
                    &format!("{} file(s) over the size limit were left out of sync: {}",
                        report.skipped_large.len(),
                        report.skipped_large.join(", ")),
                );
            }
            let ts = format!("{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default().as_secs());
            *mgr.last_sync.lock().unwrap() = Some(ts);
            *mgr.error_msg.lock().unwrap() = None;
            set_state(app, SyncState::Running);
            mgr.logs.push("INFO", "Sync completed");

            // If this sync changed any per-device recents file, tell the UI to refresh the menu.
            let head_after = git_ops::run_git(repo, &["rev-parse", "HEAD"])
                .ok()
                .map(|s| s.trim().to_string());
            if let (Some(before), Some(after)) = (head_before.as_ref(), head_after.as_ref()) {
                if before != after {
                    if let Ok(diff) = git_ops::run_git(repo, &["diff", "--name-only", before, after]) {
                        if diff.lines().any(|l| l.trim().starts_with(".notemd/recents/")) {
                            let _ = app.emit("editor://recents-synced", ());
                        }
                    }
                }
            }
        }
        Err(e) => {
            if e.contains("conflict") || e.contains("Conflict") {
                set_state(app, SyncState::Conflict);
                mgr.logs.push("WARN", &format!("Conflict: {e}"));
            } else {
                *mgr.error_msg.lock().unwrap() = Some(e.clone());
                set_state(app, SyncState::Error);
                mgr.logs.push("ERROR", &e);
            }
        }
    }

    let _ = app.emit("vault-sync-log", ());
}

fn set_state(app: &AppHandle, state: SyncState) {
    let mgr = app.state::<Arc<VaultSyncManager>>();
    *mgr.state.lock().unwrap() = state;
    let _ = app.emit("vault-sync-state-changed", state);
    #[cfg(not(target_os = "ios"))]
    crate::refresh_tray_status(app);
}
