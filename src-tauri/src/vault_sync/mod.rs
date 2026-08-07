pub mod conflict;
pub mod git_ops;
pub mod large_files;
pub mod log_buffer;
pub mod service;
pub mod watcher;

use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

use log_buffer::LogBuffer;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    NotConfigured,
    Stopped,
    Running,
    Syncing,
    Conflict,
    Error,
    /// The `git` executable could not be found or failed to run.
    GitUnavailable,
}

impl SyncState {
    /// A short human-readable label for menus, tooltips and log headers.
    pub fn label(self) -> &'static str {
        match self {
            SyncState::NotConfigured => "Not configured",
            SyncState::Stopped => "Stopped",
            SyncState::Running => "Running",
            SyncState::Syncing => "Syncing…",
            SyncState::Conflict => "Conflict — needs attention",
            SyncState::Error => "Error",
            SyncState::GitUnavailable => "Git unavailable",
        }
    }

    /// True when the state represents a problem the user should notice.
    pub fn is_problem(self) -> bool {
        matches!(
            self,
            SyncState::Conflict | SyncState::Error | SyncState::GitUnavailable
        )
    }
}

/// 一轮同步的结果摘要。目前只带被门禁排除的大文件清单(相对路径)。
#[derive(Debug, Default, Clone)]
pub struct SyncReport {
    pub skipped_large: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VaultSyncStatus {
    pub state: SyncState,
    pub repo_path: Option<String>,
    pub last_sync: Option<String>,
    pub error_message: Option<String>,
    pub git_available: bool,
    pub skipped_large_files: Vec<String>,
}

pub struct VaultSyncManager {
    pub state: Mutex<SyncState>,
    pub repo_path: Mutex<Option<String>>,
    pub remote: String,
    pub branch: String,
    pub logs: LogBuffer,
    pub last_sync: Mutex<Option<String>>,
    pub error_msg: Mutex<Option<String>>,
    pub git_available: Mutex<bool>,
    pub stop_flag: Mutex<bool>,
    /// 串行化 do_sync:后台循环阻塞持有,手动 sync_once 用 try_lock。
    pub sync_gate: Mutex<()>,
    /// 最近一轮被门禁排除的大文件(相对 repo 根路径)。正交于 SyncState。
    pub skipped_large_files: Mutex<Vec<String>>,
}

impl VaultSyncManager {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(SyncState::NotConfigured),
            repo_path: Mutex::new(None),
            remote: "origin".into(),
            branch: "main".into(),
            logs: LogBuffer::new(1000),
            last_sync: Mutex::new(None),
            error_msg: Mutex::new(None),
            git_available: Mutex::new(true),
            stop_flag: Mutex::new(false),
            sync_gate: Mutex::new(()),
            skipped_large_files: Mutex::new(Vec::new()),
        }
    }
}

#[tauri::command]
pub fn vault_sync_start(app: AppHandle) -> Result<(), String> {
    service::start(&app)
}

#[tauri::command]
pub fn vault_sync_stop(app: AppHandle) -> Result<(), String> {
    service::stop(&app)
}

#[tauri::command]
pub fn vault_sync_now(app: AppHandle) -> Result<(), String> {
    service::sync_once(&app)
}

#[tauri::command]
pub fn vault_sync_status(app: AppHandle) -> VaultSyncStatus {
    let mgr = app.state::<Arc<VaultSyncManager>>();
    let state = *mgr.state.lock().unwrap();
    let repo_path = mgr.repo_path.lock().unwrap().clone();
    let last_sync = mgr.last_sync.lock().unwrap().clone();
    let error_message = mgr.error_msg.lock().unwrap().clone();
    let git_available = *mgr.git_available.lock().unwrap();
    let skipped_large_files = mgr.skipped_large_files.lock().unwrap().clone();
    VaultSyncStatus { state, repo_path, last_sync, error_message, git_available, skipped_large_files }
}

#[tauri::command]
pub fn vault_sync_logs(app: AppHandle) -> Vec<log_buffer::LogEntry> {
    let mgr = app.state::<Arc<VaultSyncManager>>();
    mgr.logs.entries()
}

pub fn init(app: &AppHandle) {
    let repo_path = crate::shared_config::config_path()
        .ok()
        .and_then(|p| crate::shared_config::read(&p).ok())
        .and_then(|cfg| cfg.sotvault)
        .filter(|s| !s.is_empty());

    if let Some(ref path) = repo_path {
        let mgr = app.state::<Arc<VaultSyncManager>>();
        *mgr.repo_path.lock().unwrap() = Some(path.clone());

        *mgr.state.lock().unwrap() = SyncState::Stopped;
        let app_clone = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = service::start(&app_clone);
            crate::refresh_tray_status(&app_clone);
        });
    }
}
