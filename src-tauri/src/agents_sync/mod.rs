//! CLAUDE.md as a relative symlink to AGENTS.md. The sync service owns
//! CLAUDE.md's lifecycle: create the symlink when missing, back up and replace
//! a foreign regular file, repoint a wrong symlink, and remove a dangling one.
//! CLAUDE.md is gitignored. Lifecycle is independent of vault_sync's git loop —
//! active whenever a vault path is configured. See
//! docs/superpowers/specs/2026-07-25-claude-md-symlink-design.md.

pub mod logic;
pub mod watcher;

use logic::{ClaudeKind, SyncAction};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tauri::Manager;

pub const TEMPLATE: &str = include_str!("../../templates/AGENTS.md");

pub struct AgentsSyncState {
    /// Bumped on every (re)start; stale watcher threads exit when they notice.
    generation: AtomicU64,
}

impl AgentsSyncState {
    fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
        }
    }
}

fn vault_root(app: &tauri::AppHandle) -> Option<PathBuf> {
    let mgr = app.state::<Arc<crate::vault_sync::VaultSyncManager>>();
    let guard = mgr.repo_path.lock().unwrap();
    guard.as_deref().map(PathBuf::from)
}

fn claude_path(root: &Path) -> PathBuf {
    root.join(watcher::CLAUDE_FILE)
}

fn agents_exists(root: &Path) -> bool {
    root.join(watcher::AGENTS_FILE).exists()
}

fn classify_claude(root: &Path) -> ClaudeKind {
    let p = claude_path(root);
    match std::fs::symlink_metadata(&p) {
        Err(_) => ClaudeKind::Missing,
        Ok(meta) if meta.file_type().is_symlink() => match std::fs::read_link(&p) {
            Ok(target) if target == Path::new(watcher::AGENTS_FILE) => ClaudeKind::CorrectSymlink,
            _ => ClaudeKind::WrongSymlink,
        },
        Ok(_) => ClaudeKind::RegularFile,
    }
}

/// Create the relative symlink `CLAUDE.md -> AGENTS.md`. Fails on Windows
/// without privilege; callers treat an error as "skip".
fn make_symlink(root: &Path) -> std::io::Result<()> {
    let link = claude_path(root);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(watcher::AGENTS_FILE, &link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(watcher::AGENTS_FILE, &link)
    }
}

/// Best-effort recreate of a previous symlink target (rollback path).
fn restore_symlink(link: &Path, target: &Path) {
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let _ = std::os::windows::fs::symlink_file(target, link);
}

fn date_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = logic::ymd_from_unix_secs(secs);
    format!("{y:04}{m:02}{d:02}")
}

/// Ensure `<vault>/.gitignore` contains a `CLAUDE.md` line (idempotent).
fn ensure_gitignore(root: &Path) {
    let gi = root.join(".gitignore");
    let existing = std::fs::read_to_string(&gi).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == "CLAUDE.md") {
        return;
    }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str("CLAUDE.md\n");
    let _ = std::fs::write(&gi, next);
}

/// Filesystem core of `run_check`, AppHandle-free for testing.
fn reconcile(root: &Path) {
    ensure_gitignore(root);
    match logic::decide(agents_exists(root), classify_claude(root)) {
        SyncAction::None => {}
        SyncAction::RemoveDangling => {
            let _ = std::fs::remove_file(claude_path(root));
        }
        SyncAction::CreateSymlink => {
            if let Err(e) = make_symlink(root) {
                crate::dlog(&format!("agents_sync symlink create failed (skip): {e}"));
            }
        }
        SyncAction::BackupThenSymlink => {
            let claude = claude_path(root);
            let stamp = date_stamp();
            let name = logic::pick_backup_name(&stamp, |n| root.join(n).exists());
            let backup = root.join(&name);
            if std::fs::rename(&claude, &backup).is_err() {
                crate::dlog("agents_sync backup rename failed (skip)");
                return;
            }
            if let Err(e) = make_symlink(root) {
                // Roll back so the user is not left without CLAUDE.md.
                let _ = std::fs::rename(&backup, &claude);
                crate::dlog(&format!("agents_sync symlink failed, rolled back: {e}"));
            }
        }
        SyncAction::RepointSymlink => {
            let claude = claude_path(root);
            let old = std::fs::read_link(&claude).ok();
            if std::fs::remove_file(&claude).is_err() {
                crate::dlog("agents_sync repoint remove failed (skip)");
                return;
            }
            if let Err(e) = make_symlink(root) {
                if let Some(t) = old {
                    restore_symlink(&claude, &t);
                }
                crate::dlog(&format!("agents_sync repoint failed, restored: {e}"));
            }
        }
    }
}

/// Call once at app setup, after vault_sync::init.
pub fn init(app: &tauri::AppHandle) {
    app.manage(AgentsSyncState::new());
    if let Some(root) = vault_root(app) {
        start(app, &root);
    }
}

/// Call when the vault folder changes (tray picker).
pub fn restart(app: &tauri::AppHandle, root: &str) {
    start(app, Path::new(root));
}

fn start(app: &tauri::AppHandle, root: &Path) {
    let state = app.state::<AgentsSyncState>();
    let my_gen = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, rx) = mpsc::channel::<()>();
    let root = root.to_path_buf();
    let app = app.clone();
    std::thread::spawn(move || {
        // The watcher lives in this thread; dropping it on exit stops events.
        let _watcher = match watcher::start(&root, tx) {
            Ok(w) => w,
            Err(_) => return,
        };
        // Startup check catches state that changed while the app was closed.
        run_check(&app, &root);
        loop {
            let stale = || {
                app.state::<AgentsSyncState>().generation.load(Ordering::SeqCst) != my_gen
            };
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(()) => {
                    // Debounce: swallow the burst, then act once.
                    while rx.recv_timeout(Duration::from_millis(500)).is_ok() {}
                    if stale() {
                        return;
                    }
                    run_check(&app, &root);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if stale() {
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    });
}

fn run_check(_app: &tauri::AppHandle, root: &Path) {
    let action = logic::decide(agents_exists(root), classify_claude(root));
    crate::dlog(&format!("agents_sync run_check → {action:?}"));
    reconcile(root);
}

/// Tray entry point: ensure AGENTS.md exists (template on first use), sync,
/// and open it in the main window.
pub fn edit_agents_md(app: &tauri::AppHandle) {
    crate::dlog("agents_sync edit_agents_md invoked");
    if let Some(root) = vault_root(app) {
        open_agents_in_editor(app, &root);
    } else {
        // No vault yet: reuse the folder picker, then open. The picker's
        // shared path (pick_sync_folder_inner) also calls restart() for us.
        let app_for_open = app.clone();
        crate::pick_sync_folder_inner(app, move |path| {
            open_agents_in_editor(&app_for_open, Path::new(&path));
        });
    }
}

/// Read-only check for the Settings UI: does this vault have an AGENTS.md
/// that's missing the search convention? Never writes anything — the
/// write-capable command below is separate so a status poll can never
/// accidentally trigger a file change.
///
/// `false` when AGENTS.md does not exist at all, not just when it already has
/// the section: append-only means there is nothing to append *to* yet, and
/// the empty string would otherwise read as "missing the section", which
/// would tell the GUI to offer a button whose write path (below) produces a
/// frontmatter-less fragment — a document OKF v0.2 would reject. Bootstrapping
/// a new AGENTS.md is `edit_agents_md`'s job (writes the full `TEMPLATE`,
/// which already includes this section), reached from the tray, not this.
#[tauri::command]
pub fn notemd_agents_search_section_missing(app: tauri::AppHandle) -> Result<bool, String> {
    let root = crate::sotvault::resolve_vault_root(&app).ok_or("Vault not configured")?;
    let path = root.join(watcher::AGENTS_FILE);
    if !path.is_file() {
        return Ok(false);
    }
    let existing = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok(logic::search_section_missing(&existing))
}

/// Append the search convention to the vault's AGENTS.md. Returns `false`
/// when it was already there, or when AGENTS.md does not exist yet (nothing
/// was written either way — see `notemd_agents_search_section_missing` for
/// why a missing file is not "treat as empty and append"). The GUI calls this
/// only after the user explicitly confirms — this function does not ask, and
/// nothing else may call it: a silent rewrite of the user's own file is the
/// one failure mode this feature exists to avoid.
#[tauri::command]
pub fn notemd_agents_append_search_section(app: tauri::AppHandle) -> Result<bool, String> {
    let root = crate::sotvault::resolve_vault_root(&app).ok_or("Vault not configured")?;
    let path = root.join(watcher::AGENTS_FILE);
    if !path.is_file() {
        return Ok(false);
    }
    let existing = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if !logic::search_section_missing(&existing) {
        return Ok(false);
    }
    std::fs::write(&path, logic::append_search_section(&existing)).map_err(|e| e.to_string())?;
    Ok(true)
}

fn open_agents_in_editor(app: &tauri::AppHandle, root: &Path) {
    let agents = root.join(watcher::AGENTS_FILE);
    if !agents.exists() {
        let _ = std::fs::write(&agents, TEMPLATE);
    }
    // Creates/repairs the CLAUDE.md symlink and ensures the .gitignore line.
    run_check(app, root);
    crate::show_main_window(app);
    if let Some(p) = agents.to_str() {
        crate::emit_open_file_delayed(app, p);
    }
}

#[cfg(test)]
mod fs_tests {
    use super::*;
    use std::fs;

    fn vault() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("AGENTS.md"), "hello\n").unwrap();
        d
    }

    #[cfg(unix)]
    #[test]
    fn creates_symlink_when_missing() {
        let d = vault();
        reconcile(d.path());
        let link = d.path().join("CLAUDE.md");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(&link).unwrap(), Path::new("AGENTS.md"));
        assert_eq!(fs::read_to_string(&link).unwrap(), "hello\n");
    }

    #[cfg(unix)]
    #[test]
    fn noop_on_correct_symlink() {
        let d = vault();
        reconcile(d.path());
        reconcile(d.path()); // second run must not error or change anything
        let link = d.path().join("CLAUDE.md");
        assert_eq!(fs::read_link(&link).unwrap(), Path::new("AGENTS.md"));
    }

    #[cfg(unix)]
    #[test]
    fn backs_up_regular_file_then_symlinks() {
        let d = vault();
        fs::write(d.path().join("CLAUDE.md"), "old-real\n").unwrap();
        reconcile(d.path());
        let link = d.path().join("CLAUDE.md");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        let stamp = super::date_stamp();
        let backup = d.path().join(format!("CLAUDE.{stamp}.md"));
        assert_eq!(fs::read_to_string(&backup).unwrap(), "old-real\n");
    }

    #[cfg(unix)]
    #[test]
    fn repoints_wrong_symlink() {
        let d = vault();
        fs::write(d.path().join("OTHER.md"), "other\n").unwrap();
        std::os::unix::fs::symlink("OTHER.md", d.path().join("CLAUDE.md")).unwrap();
        reconcile(d.path());
        assert_eq!(
            fs::read_link(d.path().join("CLAUDE.md")).unwrap(),
            Path::new("AGENTS.md")
        );
    }

    #[cfg(unix)]
    #[test]
    fn removes_dangling_when_agents_gone() {
        let d = vault();
        std::os::unix::fs::symlink("AGENTS.md", d.path().join("CLAUDE.md")).unwrap();
        fs::remove_file(d.path().join("AGENTS.md")).unwrap();
        reconcile(d.path());
        assert!(fs::symlink_metadata(d.path().join("CLAUDE.md")).is_err());
    }

    #[test]
    fn gitignore_appends_once() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("AGENTS.md"), "x\n").unwrap();
        ensure_gitignore(d.path());
        ensure_gitignore(d.path());
        let gi = fs::read_to_string(d.path().join(".gitignore")).unwrap();
        assert_eq!(gi.matches("CLAUDE.md").count(), 1);
    }

    #[test]
    fn gitignore_preserves_existing_lines() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join(".gitignore"), "node_modules\n").unwrap();
        ensure_gitignore(d.path());
        let gi = fs::read_to_string(d.path().join(".gitignore")).unwrap();
        assert!(gi.contains("node_modules"));
        assert!(gi.contains("CLAUDE.md"));
    }
}
