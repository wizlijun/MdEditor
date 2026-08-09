//! File-level git history for vault files. Reuses `vault_sync::git_ops` to run
//! `git log`/`git show` against the vault repo. Desktop-only in practice
//! (commands are registered only in the non-iOS invoke handler).

use std::path::Path;

use crate::vault_sync::git_ops;

/// One commit that touched a given file. `timestamp` is Unix seconds (author date).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct GitCommit {
    pub hash: String,
    pub short: String,
    pub author: String,
    pub timestamp: i64,
    pub subject: String,
}

/// The field separator we embed in the `git log --format` string (ASCII Unit
/// Separator, 0x1f — will never appear in a one-line subject).
const FS: char = '\u{1f}';

/// `git log --format` string. The field separators are literal U+001F bytes and
/// MUST match `FS` above so `parse_log` splits correctly.
const LOG_FORMAT: &str = "--format=%H\u{1f}%h\u{1f}%an\u{1f}%at\u{1f}%s";

/// Parse the `git log --format=%H<FS>%h<FS>%an<FS>%at<FS>%s` output (one commit
/// per line) into structured commits. Blank lines and malformed lines are skipped.
pub fn parse_log(stdout: &str) -> Vec<GitCommit> {
    stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(5, FS);
            let hash = parts.next()?.trim().to_string();
            if hash.is_empty() {
                return None;
            }
            let short = parts.next()?.trim().to_string();
            let author = parts.next()?.trim().to_string();
            let timestamp = parts.next()?.trim().parse::<i64>().ok()?;
            let subject = parts.next().unwrap_or("").trim().to_string();
            Some(GitCommit { hash, short, author, timestamp, subject })
        })
        .collect()
}

/// The repo-relative, forward-slashed path of `abs` under `repo`. Errors when
/// `abs` is not under `repo`.
pub fn rel_path(repo: &Path, abs: &Path) -> Result<String, String> {
    let rel = abs
        .strip_prefix(repo)
        .map_err(|_| "file is not under the vault repo".to_string())?;
    Ok(rel.to_string_lossy().replace('\\', "/"))
}

/// Commit history for a single file. Returns an empty list when the file has no
/// history; returns `Err("git-unavailable")` when git isn't runnable so the UI
/// can show a distinct empty state.
#[tauri::command]
pub fn git_file_log(repo: String, abs_path: String) -> Result<Vec<GitCommit>, String> {
    if git_ops::version().is_none() {
        return Err("git-unavailable".to_string());
    }
    let repo_path = Path::new(&repo);
    let rel = rel_path(repo_path, Path::new(&abs_path))?;
    let out = git_ops::run_git(
        repo_path,
        &["log", "--follow", LOG_FORMAT, "--", &rel],
    )?;
    Ok(parse_log(&out))
}

/// Full `git show <rev>` diff limited to the file (includes the commit header).
#[tauri::command]
pub fn git_file_show(repo: String, rev: String, abs_path: String) -> Result<String, String> {
    let repo_path = Path::new(&repo);
    let rel = rel_path(repo_path, Path::new(&abs_path))?;
    git_ops::run_git(repo_path, &["show", &rev, "--", &rel])
}

/// File contents as of `<rev>` (`git show <rev>:<rel>`), for buffer restore.
#[tauri::command]
pub fn git_file_at(repo: String, rev: String, abs_path: String) -> Result<String, String> {
    let repo_path = Path::new(&repo);
    let rel = rel_path(repo_path, Path::new(&abs_path))?;
    let spec = format!("{rev}:{rel}");
    git_ops::run_git(repo_path, &["show", &spec])
}

/// Unified diff between the file as of `<rev>` and the caller-provided `current`
/// buffer (the live editor content, possibly unsaved). Runs `git diff
/// --no-index` on two temp files so it compares against the buffer regardless of
/// the file's on-disk/staged state. Returns an empty string when identical.
#[tauri::command]
pub fn git_diff_current(
    repo: String,
    rev: String,
    abs_path: String,
    current: String,
) -> Result<String, String> {
    let repo_path = Path::new(&repo);
    let rel = rel_path(repo_path, Path::new(&abs_path))?;
    let hist = git_ops::run_git(repo_path, &["show", &format!("{rev}:{rel}")])?;

    let dir = std::env::temp_dir();
    let uniq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let base = format!("notemd-diff-{}-{}", std::process::id(), uniq);
    let old_p = dir.join(format!("{base}.old"));
    let new_p = dir.join(format!("{base}.new"));
    std::fs::write(&old_p, hist.as_bytes()).map_err(|e| format!("write temp: {e}"))?;
    std::fs::write(&new_p, current.as_bytes()).map_err(|e| format!("write temp: {e}"))?;

    let out = crate::platform::command("git")
        .args([
            "diff",
            "--no-index",
            "--",
            &old_p.to_string_lossy(),
            &new_p.to_string_lossy(),
        ])
        .output();
    let _ = std::fs::remove_file(&old_p);
    let _ = std::fs::remove_file(&new_p);
    let out = out.map_err(|e| format!("git spawn: {e}"))?;

    // `git diff --no-index` exits 1 when there ARE differences (normal), 0 when
    // identical, >1 on real error.
    if out.status.code().unwrap_or(-1) > 1 {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }

    // Cosmetic: rewrite the temp-file paths in the diff header to friendly
    // labels (these are meta lines DiffView renders muted). Cover both the
    // leading-slash and git's prefix-stripped forms.
    let name = rel.rsplit('/').next().unwrap_or(&rel);
    let short: String = rev.chars().take(7).collect();
    let old_abs = old_p.to_string_lossy().to_string();
    let new_abs = new_p.to_string_lossy().to_string();
    let diff = String::from_utf8_lossy(&out.stdout)
        .replace(&old_abs, &format!("{name}@{short}"))
        .replace(old_abs.trim_start_matches('/'), &format!("{name}@{short}"))
        .replace(&new_abs, &format!("{name} (current)"))
        .replace(new_abs.trim_start_matches('/'), &format!("{name} (current)"));
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_log_reads_fields() {
        let fs = '\u{1f}';
        let line = format!("abcdef123456{fs}abcdef1{fs}Jane Doe{fs}1700000000{fs}fix: a thing");
        let out = parse_log(&format!("{line}\n"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].hash, "abcdef123456");
        assert_eq!(out[0].short, "abcdef1");
        assert_eq!(out[0].author, "Jane Doe");
        assert_eq!(out[0].timestamp, 1_700_000_000);
        assert_eq!(out[0].subject, "fix: a thing");
    }

    #[test]
    fn parse_log_skips_blank_and_malformed() {
        let fs = '\u{1f}';
        let good = format!("h{fs}s{fs}a{fs}123{fs}subj");
        let bad = format!("h{fs}s{fs}a{fs}notanumber{fs}subj");
        let out = parse_log(&format!("\n{good}\n{bad}\n\n"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].subject, "subj");
    }

    #[test]
    fn rel_path_forward_slashes_under_repo() {
        let repo = PathBuf::from("/vault");
        let abs = PathBuf::from("/vault/Sync/note.md");
        assert_eq!(rel_path(&repo, &abs).unwrap(), "Sync/note.md");
    }

    #[test]
    fn rel_path_rejects_outside() {
        let repo = PathBuf::from("/vault");
        let abs = PathBuf::from("/elsewhere/note.md");
        assert!(rel_path(&repo, &abs).is_err());
    }

    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .status()
            .unwrap();
        assert!(status.success(), "git {:?} failed", args);
    }

    #[test]
    fn log_show_at_roundtrip_in_temp_repo() {
        let dir = std::env::temp_dir().join(format!("mdeditor-gh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        let file = dir.join("note.md");
        std::fs::write(&file, "v1\n").unwrap();
        git(&dir, &["add", "note.md"]);
        git(&dir, &["commit", "-q", "-m", "first"]);
        std::fs::write(&file, "v2\n").unwrap();
        git(&dir, &["add", "note.md"]);
        git(&dir, &["commit", "-q", "-m", "second"]);

        let repo = dir.to_string_lossy().to_string();
        let abs = file.to_string_lossy().to_string();

        let log = git_file_log(repo.clone(), abs.clone()).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].subject, "second"); // newest first
        assert_eq!(log[1].subject, "first");

        let old = log[1].hash.clone();
        let content = git_file_at(repo.clone(), old.clone(), abs.clone()).unwrap();
        assert_eq!(content, "v1\n");

        let diff = git_file_show(repo.clone(), log[0].hash.clone(), abs.clone()).unwrap();
        assert!(diff.contains("-v1"));
        assert!(diff.contains("+v2"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_current_buffer_vs_rev() {
        let dir = std::env::temp_dir().join(format!("mdeditor-gh-dc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        let file = dir.join("note.md");
        std::fs::write(&file, "v1\n").unwrap();
        git(&dir, &["add", "note.md"]);
        git(&dir, &["commit", "-q", "-m", "first"]);

        let repo = dir.to_string_lossy().to_string();
        let abs = file.to_string_lossy().to_string();
        let rev = git_file_log(repo.clone(), abs.clone()).unwrap()[0].hash.clone();

        // committed "v1\n" vs an edited buffer "v2\n" → shows the change
        let diff = git_diff_current(repo.clone(), rev.clone(), abs.clone(), "v2\n".into()).unwrap();
        assert!(diff.contains("-v1"), "diff should show removed v1: {diff}");
        assert!(diff.contains("+v2"), "diff should show added v2: {diff}");

        // identical buffer → no differences → empty
        let same = git_diff_current(repo, rev, abs, "v1\n".into()).unwrap();
        assert!(same.trim().is_empty(), "identical buffer should yield empty diff: {same}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
