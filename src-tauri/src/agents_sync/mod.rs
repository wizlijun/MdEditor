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

/// Read `<root>/AGENTS.md` if — and only if — it exists and has content
/// beyond whitespace. `None` covers a missing file, a 0-byte file, and a
/// whitespace-only file identically: none of the three is a document there is
/// anything sensible to append *to*. A `touch AGENTS.md`, an editor saving an
/// empty buffer, or an interrupted write can all produce the blank case, and
/// treating it as "empty string, append away" would write a frontmatter-less
/// fragment straight into `## Searching this vault` — exactly the OKF-invalid
/// document the missing-file guard exists to prevent, reached through a
/// different door. Bootstrapping a real AGENTS.md is `edit_agents_md`'s job
/// (writes the full `TEMPLATE`, which already includes this section), reached
/// from the tray, not this.
fn read_nonblank_agents_md(root: &Path) -> Result<Option<String>, String> {
    let path = root.join(watcher::AGENTS_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let existing = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    Ok((!existing.trim().is_empty()).then_some(existing))
}

/// Fs-level logic for `notemd_agents_search_section_missing`, testable
/// without a `tauri::AppHandle`.
fn agents_search_section_missing_at(root: &Path) -> Result<bool, String> {
    Ok(match read_nonblank_agents_md(root)? {
        Some(existing) => logic::search_section_missing(&existing),
        None => false,
    })
}

/// Fs-level logic for `notemd_agents_append_search_section`, testable without
/// a `tauri::AppHandle`.
fn agents_append_search_section_at(root: &Path) -> Result<bool, String> {
    let Some(existing) = read_nonblank_agents_md(root)? else {
        return Ok(false);
    };
    if !logic::search_section_missing(&existing) {
        return Ok(false);
    }
    let path = root.join(watcher::AGENTS_FILE);
    std::fs::write(&path, logic::append_search_section(&existing)).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Read-only check for the Settings UI: does this vault have an AGENTS.md
/// that's missing the search convention? Never writes anything — the
/// write-capable command below is separate so a status poll can never
/// accidentally trigger a file change.
#[tauri::command]
pub fn notemd_agents_search_section_missing(app: tauri::AppHandle) -> Result<bool, String> {
    let root = crate::sotvault::resolve_vault_root(&app).ok_or("Vault not configured")?;
    agents_search_section_missing_at(&root)
}

/// Append the search convention to the vault's AGENTS.md. Returns `false`
/// when it was already there, or when AGENTS.md is missing or blank (nothing
/// was written either way — see `read_nonblank_agents_md`). The GUI calls
/// this only after the user explicitly confirms — this function does not
/// ask, and nothing else may call it: a silent rewrite of the user's own file
/// is the one failure mode this feature exists to avoid.
#[tauri::command]
pub fn notemd_agents_append_search_section(app: tauri::AppHandle) -> Result<bool, String> {
    let root = crate::sotvault::resolve_vault_root(&app).ok_or("Vault not configured")?;
    agents_append_search_section_at(&root)
}

fn open_agents_in_editor(app: &tauri::AppHandle, root: &Path) {
    let agents = root.join(watcher::AGENTS_FILE);
    let _ = create_default_agents_md(root);
    // Creates/repairs the CLAUDE.md symlink and ensures the .gitignore line.
    run_check(app, root);
    crate::show_main_window(app);
    if let Some(p) = agents.to_str() {
        crate::emit_open_file_delayed(app, p);
    }
}

/// Create the built-in conventions only when AGENTS.md is genuinely absent.
/// `create_new` makes that promise atomic and refuses to follow an existing or
/// dangling symlink, so opening the editor can never replace user instructions.
fn create_default_agents_md(root: &Path) -> std::io::Result<bool> {
    use std::io::Write;

    let path = root.join(watcher::AGENTS_FILE);
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(TEMPLATE.as_bytes())?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error),
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

    #[test]
    fn creates_the_default_agents_template_only_when_missing() {
        let d = tempfile::tempdir().unwrap();
        assert!(create_default_agents_md(d.path()).unwrap());
        assert_eq!(
            fs::read_to_string(d.path().join("AGENTS.md")).unwrap(),
            TEMPLATE
        );
        assert!(!create_default_agents_md(d.path()).unwrap());
    }

    #[test]
    fn default_template_creation_never_replaces_existing_instructions() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("AGENTS.md");
        for existing in ["", "  \n", "# My vault\n\nKeep this exactly.\n"] {
            fs::write(&path, existing).unwrap();
            assert!(!create_default_agents_md(d.path()).unwrap());
            assert_eq!(fs::read_to_string(&path).unwrap(), existing);
            fs::remove_file(&path).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn default_template_creation_does_not_follow_a_dangling_symlink() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("outside.md");
        std::os::unix::fs::symlink(&target, d.path().join("AGENTS.md")).unwrap();
        assert!(!create_default_agents_md(d.path()).unwrap());
        assert!(!target.exists());
    }

    #[test]
    fn template_mermaid_contract_matches_the_frontend_dependency() {
        let package_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../package.json");
        let package: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(package_path).expect("root package.json must be readable"),
        )
        .expect("root package.json must be valid JSON");
        let version = package["dependencies"]["mermaid"]
            .as_str()
            .expect("package.json must pin Mermaid directly");

        assert!(
            TEMPLATE.contains(&format!("Mermaid {version}")),
            "Vault AGENTS.md must document the exact bundled Mermaid version"
        );
        for grammar in [
            "quadrantChart",
            "architecture-beta",
            "eventmodeling",
            "cynefin-beta",
            "swimlane-beta",
            "railroad-beta",
        ] {
            assert!(
                TEMPLATE.contains(grammar),
                "Vault AGENTS.md is missing `{grammar}`"
            );
        }
        for rule in [
            "Comments start with `%%`",
            "short ASCII identifiers",
            "does not auto-wrap",
        ] {
            assert!(
                TEMPLATE.contains(rule),
                "Vault AGENTS.md is missing Mermaid rule: {rule}"
            );
        }
    }

    // ---- notemd_agents_search_section_missing / notemd_agents_append_search_section ----
    // Exercised through the `_at(&Path)` fs-level functions, which the
    // `#[tauri::command]` wrappers call after resolving an AppHandle to a
    // vault root — same split as `reconcile` above.

    #[test]
    fn missing_status_false_when_agents_md_does_not_exist() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(agents_search_section_missing_at(d.path()), Ok(false));
    }

    #[test]
    fn missing_status_false_for_a_0_byte_agents_md() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("AGENTS.md"), "").unwrap();
        assert_eq!(agents_search_section_missing_at(d.path()), Ok(false));
    }

    #[test]
    fn missing_status_false_for_a_whitespace_only_agents_md() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("AGENTS.md"), "  \n\n\t\n  ").unwrap();
        assert_eq!(agents_search_section_missing_at(d.path()), Ok(false));
    }

    #[test]
    fn missing_status_true_for_real_content_without_the_section() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("AGENTS.md"), "# Vault\n\nMy own conventions.\n").unwrap();
        assert_eq!(agents_search_section_missing_at(d.path()), Ok(true));
    }

    #[test]
    fn append_writes_nothing_when_agents_md_does_not_exist() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(agents_append_search_section_at(d.path()), Ok(false));
        assert!(!d.path().join("AGENTS.md").exists(), "must not create the file");
    }

    /// The regression this whole guard exists for: a 0-byte AGENTS.md must
    /// come out of the append path exactly as blank as it went in, never
    /// topped up with a bare, frontmatter-less fragment.
    #[test]
    fn append_leaves_a_0_byte_agents_md_untouched() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("AGENTS.md");
        fs::write(&path, "").unwrap();
        assert_eq!(agents_append_search_section_at(d.path()), Ok(false));
        assert_eq!(fs::read_to_string(&path).unwrap(), "", "0-byte file must stay 0-byte");
    }

    /// Same guarantee, whitespace-only case — a file with just a newline is
    /// no more a real document than an empty one.
    #[test]
    fn append_leaves_a_whitespace_only_agents_md_untouched() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("AGENTS.md");
        fs::write(&path, "  \n\n\t\n  ").unwrap();
        assert_eq!(agents_append_search_section_at(d.path()), Ok(false));
        assert_eq!(fs::read_to_string(&path).unwrap(), "  \n\n\t\n  ", "whitespace-only file must be untouched");
    }

    #[test]
    fn append_writes_the_section_into_real_content() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("AGENTS.md");
        fs::write(&path, "# Vault\n\nMy own conventions.\n").unwrap();
        assert_eq!(agents_append_search_section_at(d.path()), Ok(true));
        let after = fs::read_to_string(&path).unwrap();
        assert!(after.starts_with("# Vault\n\nMy own conventions.\n"));
        assert!(after.contains("## Searching this vault"));
    }

    /// The template and the append path must teach agents the same wording.
    /// `SEARCH_SECTION` is hand-duplicated into `templates/AGENTS.md` today;
    /// without this test the two could drift apart silently — a new vault
    /// (template) and an upgraded vault (append) would then document the
    /// search command differently depending on which path created its
    /// AGENTS.md, and nobody would notice until two agents on two machines
    /// disagreed about how `notemd search` works.
    #[test]
    fn template_contains_the_same_search_section_the_append_path_writes() {
        assert!(
            TEMPLATE.contains(logic::SEARCH_SECTION.trim()),
            "templates/AGENTS.md has drifted from agents_sync::logic::SEARCH_SECTION"
        );
    }

    /// Task 11 review: 6 public docs enumerate `--json`'s extra fields in
    /// their own prose (README ×2, docs/FEATURES ×2, website/public/llms.txt,
    /// llms-full.txt) and none of them had a drift tripwire — adding
    /// `attention_minutes` to `print_json` and to this crate's own
    /// `SEARCH_SECTION` silently missed all six the first time around.
    ///
    /// This does not attempt to pin all six: a full-text `.contains(...)`
    /// check (the `TEMPLATE`/`SEARCH_SECTION` pattern above) would demand
    /// byte-identical wording, and each doc's context differs enough
    /// (overview vs. feature list vs. dense agent-facing convention) that
    /// forcing identical prose would fight their own editorial purpose —
    /// see the human-facing commit for how the six diverge in phrasing on
    /// purpose. What's cheap to hold structurally, without becoming a
    /// second copy of the wording itself, is field *names*: whichever field
    /// list is easiest for an outside agent to reach without cloning this
    /// repo (`llms.txt` — CLAUDE.md names it "给 agent 的公共约定") must at
    /// least mention every field this crate's own `SEARCH_SECTION` teaches.
    /// Read at test time via `CARGO_MANIFEST_DIR`, not `include_str!`, so a
    /// missing `website/` checkout (e.g. a stripped packaging context) fails
    /// this test rather than the ordinary build.
    #[test]
    fn llms_txt_names_every_field_search_section_teaches() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../website/public/llms.txt");
        let body = std::fs::read_to_string(path).expect("website/public/llms.txt must be readable from src-tauri/");
        for field in ["source_ref", "origin", "provenance", "attention_minutes"] {
            assert!(
                logic::SEARCH_SECTION.contains(field),
                "test bug: {field} isn't even in SEARCH_SECTION, nothing to compare against"
            );
            assert!(body.contains(field), "website/public/llms.txt is missing --json field `{field}` that SEARCH_SECTION documents");
        }
    }
}
