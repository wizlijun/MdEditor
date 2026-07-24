# CLAUDE.md Symlink Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `CLAUDE.md` a relative symlink to `AGENTS.md` (gitignored, sync-service-owned) instead of a hash-synced copy, removing the cross-machine conflict prompt.

**Architecture:** Rewrite `agents_sync` around on-disk state classification. A pure `decide(agents_exists, ClaudeKind) -> SyncAction` drives filesystem actions in `mod.rs` (create / backup-then-symlink / repoint / remove-dangling) plus idempotent `.gitignore` maintenance. Delete the baseline, hashing, and conflict dialog.

**Tech Stack:** Rust, Tauri, `notify` (existing watcher), `tempfile` (tests). No new crate deps — date stamp via a pure UTC algorithm.

**Spec:** `docs/superpowers/specs/2026-07-25-claude-md-symlink-design.md`

---

## File Structure

- `src-tauri/src/agents_sync/logic.rs` — **rewrite**: `ClaudeKind`, `SyncAction`, pure `decide`, pure `ymd_from_unix_secs`, pure `pick_backup_name`. Unit tests.
- `src-tauri/src/agents_sync/mod.rs` — **rewrite `run_check` + helpers**: classify state, perform action with rollback, ensure gitignore. Delete `prompt_conflict`, `backup_to_temp`, `prompting`, baseline usage, hashing.
- `src-tauri/src/agents_sync/baseline.rs` — **delete**.
- `src-tauri/src/agents_sync/watcher.rs` — unchanged.
- `src-tauri/templates/AGENTS.md` — reword line 4.

---

## Task 1: Rewrite pure decision logic

**Files:**
- Modify: `src-tauri/src/agents_sync/logic.rs` (full rewrite)

- [ ] **Step 1: Replace file contents with new enums + pure functions + tests**

```rust
//! Pure decision logic for the CLAUDE.md → AGENTS.md symlink.

/// What `CLAUDE.md` currently is on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeKind {
    Missing,
    /// Symlink whose target is exactly `AGENTS.md` (same-dir, relative).
    CorrectSymlink,
    /// Symlink to anything else (absolute, or a different file).
    WrongSymlink,
    /// A regular file (or any non-symlink entry).
    RegularFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAction {
    None,
    CreateSymlink,
    BackupThenSymlink,
    RepointSymlink,
    RemoveDangling,
}

pub fn decide(agents_exists: bool, claude: ClaudeKind) -> SyncAction {
    use ClaudeKind::*;
    use SyncAction::*;
    if agents_exists {
        match claude {
            Missing => CreateSymlink,
            CorrectSymlink => None,
            WrongSymlink => RepointSymlink,
            RegularFile => BackupThenSymlink,
        }
    } else {
        match claude {
            // A relative link to a now-missing AGENTS.md is dangling; sync owns it.
            CorrectSymlink => RemoveDangling,
            Missing | WrongSymlink | RegularFile => None,
        }
    }
}

/// Civil date (year, month, day) from a Unix timestamp in seconds, UTC.
/// Howard Hinnant's `civil_from_days`.
pub fn ymd_from_unix_secs(secs: i64) -> (i64, u32, u32) {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

/// Pick a non-colliding backup filename for a given `YYYYMMDD` stamp.
/// `exists` reports whether a candidate name is already taken.
pub fn pick_backup_name(stamp: &str, exists: impl Fn(&str) -> bool) -> String {
    let base = format!("CLAUDE.{stamp}.md");
    if !exists(&base) {
        return base;
    }
    let mut n = 2;
    loop {
        let candidate = format!("CLAUDE.{stamp}-{n}.md");
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ClaudeKind::*;
    use SyncAction::*;

    #[test]
    fn agents_present_actions() {
        assert_eq!(decide(true, Missing), CreateSymlink);
        assert_eq!(decide(true, CorrectSymlink), None);
        assert_eq!(decide(true, WrongSymlink), RepointSymlink);
        assert_eq!(decide(true, RegularFile), BackupThenSymlink);
    }

    #[test]
    fn agents_absent_actions() {
        assert_eq!(decide(false, CorrectSymlink), RemoveDangling);
        assert_eq!(decide(false, WrongSymlink), None);
        assert_eq!(decide(false, RegularFile), None);
        assert_eq!(decide(false, Missing), None);
    }

    #[test]
    fn ymd_epoch_and_known_dates() {
        assert_eq!(ymd_from_unix_secs(0), (1970, 1, 1));
        // 2026-07-25T00:00:00Z == 1774396800
        assert_eq!(ymd_from_unix_secs(1_774_396_800), (2026, 7, 25));
        // last second of 1999
        assert_eq!(ymd_from_unix_secs(946_684_799), (1999, 12, 31));
    }

    #[test]
    fn backup_name_no_collision() {
        assert_eq!(pick_backup_name("20260725", |_| false), "CLAUDE.20260725.md");
    }

    #[test]
    fn backup_name_suffixes_on_collision() {
        let taken = |n: &str| n == "CLAUDE.20260725.md" || n == "CLAUDE.20260725-2.md";
        assert_eq!(pick_backup_name("20260725", taken), "CLAUDE.20260725-3.md");
    }
}
```

- [ ] **Step 2: Verify tests fail-then-pass by compiling the crate later** (logic compiles standalone once `mod.rs` stops importing `PairState`). Run in Task 4.

- [ ] **Step 3: Commit** (bundled with Task 2–3; logic.rs alone won't compile against old mod.rs).

---

## Task 2: Delete baseline module

**Files:**
- Delete: `src-tauri/src/agents_sync/baseline.rs`

- [ ] **Step 1: Remove the file**

```bash
git rm src-tauri/src/agents_sync/baseline.rs
```

(The `pub mod baseline;` line is removed in Task 3.)

---

## Task 3: Rewrite mod.rs actions

**Files:**
- Modify: `src-tauri/src/agents_sync/mod.rs`

- [ ] **Step 1: Replace the module docs + imports + `AgentsSyncState`**

Header block becomes:

```rust
//! CLAUDE.md as a relative symlink to AGENTS.md. The sync service owns
//! CLAUDE.md's lifecycle: create the symlink when missing, back up and replace
//! a foreign regular file, repoint a wrong symlink, and remove a dangling one.
//! CLAUDE.md is gitignored. See
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
        Self { generation: AtomicU64::new(0) }
    }
}
```

- [ ] **Step 2: Keep `vault_root`, delete `baseline_path` / `hash_of` / `current_state`; add classify + fs helpers**

Delete `baseline_path`, `hash_of`, `current_state`. Keep `vault_root` unchanged. Add:

```rust
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
        Ok(meta) if meta.file_type().is_symlink() => {
            match std::fs::read_link(&p) {
                Ok(target) if target == Path::new(watcher::AGENTS_FILE) => {
                    ClaudeKind::CorrectSymlink
                }
                _ => ClaudeKind::WrongSymlink,
            }
        }
        Ok(_) => ClaudeKind::RegularFile,
    }
}

/// Create the relative symlink `CLAUDE.md -> AGENTS.md`. Fails on Windows
/// without privilege; caller treats an error as "skip".
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
```

- [ ] **Step 3: Replace `run_check` and delete `prompt_conflict` + `backup_to_temp`**

```rust
fn run_check(app: &tauri::AppHandle, root: &Path) {
    ensure_gitignore(root);
    let action = logic::decide(agents_exists(root), classify_claude(root));
    crate::dlog(&format!("agents_sync run_check → {action:?}"));
    match action {
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
                // Best-effort restore of the previous link.
                if let Some(t) = old {
                    #[cfg(unix)]
                    let _ = std::os::unix::fs::symlink(&t, &claude);
                    #[cfg(windows)]
                    let _ = std::os::windows::fs::symlink_file(&t, &claude);
                }
                crate::dlog(&format!("agents_sync repoint failed, restored: {e}"));
            }
        }
    }
}
```

Delete `prompt_conflict` and `backup_to_temp` entirely. Keep `edit_agents_md` and `open_agents_in_editor` unchanged (they already write the template then call `run_check`).

- [ ] **Step 4: Verify `init` / `start` still reference only surviving symbols** — `init` calls `app.manage(AgentsSyncState::new())` and `start`; `start` calls `run_check`. The `prompting` early-out in the old `run_check` is gone; nothing else referenced it.

---

## Task 4: Compile + run Rust tests

- [ ] **Step 1: Build and test the crate**

Run: `cd src-tauri && cargo test agents_sync`
Expected: PASS (logic + any fs tests). Fix compile errors (stale `PairState`/`baseline`/`prompting` references) until green.

- [ ] **Step 2: Full check**

Run: `cd src-tauri && cargo build`
Expected: builds clean.

---

## Task 5: Filesystem integration tests

**Files:**
- Modify: `src-tauri/src/agents_sync/mod.rs` (add `#[cfg(test)] mod fs_tests` at end)

Note: `run_check` needs an `AppHandle`. Extract the fs core into a testable
free function `reconcile(root: &Path)` that does everything `run_check` does
except `crate::dlog`/AppHandle, and have `run_check` call `reconcile(root)`
after logging. Test `reconcile` directly.

- [ ] **Step 1: Refactor — add `reconcile(root)` and make `run_check` delegate**

```rust
/// Filesystem core of run_check, AppHandle-free for testing.
fn reconcile(root: &Path) {
    ensure_gitignore(root);
    match logic::decide(agents_exists(root), classify_claude(root)) {
        // ... move the match arms here (identical bodies, drop the dlog line) ...
    }
}
```

`run_check` becomes:

```rust
fn run_check(_app: &tauri::AppHandle, root: &Path) {
    let action = logic::decide(agents_exists(root), classify_claude(root));
    crate::dlog(&format!("agents_sync run_check → {action:?}"));
    reconcile(root);
}
```

(Recomputing `decide` once for the log line is fine — cheap stat calls.)

- [ ] **Step 2: Add fs tests**

```rust
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
```

- [ ] **Step 3: Run**

Run: `cd src-tauri && cargo test agents_sync`
Expected: PASS.

---

## Task 6: Reword template

**Files:**
- Modify: `src-tauri/templates/AGENTS.md:3-4`

- [ ] **Step 1: Change the description line**

Replace:

```
Guidance for AI agents working in this vault. This file is the source of
truth; CLAUDE.md is an auto-generated copy — edit AGENTS.md only.
```

with:

```
Guidance for AI agents working in this vault. This file is the source of
truth; CLAUDE.md is a symlink to this file — edit AGENTS.md only.
```

- [ ] **Step 2: Commit everything**

```bash
git add src-tauri/src/agents_sync/ src-tauri/templates/AGENTS.md docs/superpowers
git commit -m "feat(agents-sync): CLAUDE.md as gitignored symlink to AGENTS.md"
```

---

## Self-Review

- **Spec coverage:** decision table → Task 1 `decide` + Task 5 fs tests; relative symlink → `make_symlink`; dated backup + collision → `pick_backup_name` + Task 5; Windows skip + rollback → `run_check`/`reconcile` arms; `.gitignore` → `ensure_gitignore`; delete baseline/hash/dialog → Tasks 2–3; template reword → Task 6. All covered.
- **Placeholders:** none — every step has full code.
- **Type consistency:** `ClaudeKind`/`SyncAction` variants match between `logic.rs` and `mod.rs`; `reconcile`/`run_check`/`make_symlink`/`ensure_gitignore`/`date_stamp`/`pick_backup_name`/`ymd_from_unix_secs` names are consistent across tasks.
