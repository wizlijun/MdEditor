use std::path::Path;

use crate::platform::command;

use super::SyncReport;

pub type GitResult<T> = Result<T, String>;

/// Returns the `git --version` string when the executable is present and
/// runnable, otherwise `None`. Used to surface "git unavailable" prominently
/// instead of silently reporting a healthy sync.
pub fn version() -> Option<String> {
    let output = command("git").arg("--version").output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Accepted proxy URL, normalized, or an explanation of why it is not one.
/// An empty/whitespace value means "no proxy" and yields `Ok(None)`.
///
/// The scheme set is what git itself understands for `http.proxy`
/// (libcurl): plain HTTP CONNECT proxies and SOCKS. `socks5h` is the variant
/// that resolves DNS at the proxy, which is usually what a user behind a
/// filtered resolver actually wants — so it is accepted rather than "fixed".
///
/// Note this covers HTTPS remotes only. An `ssh://` / `git@host:` remote does
/// not read `http.proxy` at all; it would need `core.sshCommand` with a
/// ProxyCommand. The vault flow configures HTTPS remotes with a PAT, so that
/// is the case worth supporting — and saying so beats silently doing nothing.
pub fn validate_proxy_url(raw: &str) -> Result<Option<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = url::Url::parse(trimmed)
        .map_err(|e| format!("not a URL: {e} — expected e.g. http://127.0.0.1:1080"))?;
    match parsed.scheme() {
        "http" | "https" | "socks5" | "socks5h" => {}
        other => {
            return Err(format!(
                "unsupported proxy scheme '{other}' — use http, https, socks5 or socks5h"
            ))
        }
    }
    if parsed.host_str().unwrap_or("").is_empty() {
        return Err("proxy URL has no host — expected e.g. http://127.0.0.1:1080".into());
    }
    Ok(Some(trimmed.to_string()))
}

/// The configured git proxy, or `None`.
///
/// Read per invocation rather than cached: the file is a few hundred bytes and
/// a git spawn dominates it by orders of magnitude, and reading fresh means a
/// settings change takes effect on the next sync tick instead of after a
/// restart. An invalid stored value is ignored here rather than failing the
/// sync: the settings command rejects bad input at write time, so a bad value
/// can only come from a hand-edited file, and refusing to sync at all is a
/// worse outcome than attempting a direct connection.
fn configured_proxy() -> Option<String> {
    let path = crate::shared_config::config_path().ok()?;
    let cfg = crate::shared_config::read(&path).ok()?;
    validate_proxy_url(cfg.git_proxy.as_deref().unwrap_or(""))
        .ok()
        .flatten()
}

pub fn run_git(repo: &Path, args: &[&str]) -> GitResult<String> {
    run_git_with_proxy(repo, args, configured_proxy().as_deref())
}

/// [`run_git`] with the proxy supplied rather than read from disk, so the
/// injection is testable against a real repo without touching the user's config.
fn run_git_with_proxy(repo: &Path, args: &[&str], proxy: Option<&str>) -> GitResult<String> {
    let mut cmd = command("git");
    // `-c` overrides go BEFORE the subcommand, and are scoped to this single
    // invocation — nothing is written to the user's repo or global config, so
    // clearing the setting genuinely stops proxying instead of leaving a stale
    // `http.proxy` behind in .git/config for every other tool to inherit.
    if let Some(proxy) = proxy {
        cmd.arg("-c").arg(format!("http.proxy={proxy}"));
        cmd.arg("-c").arg(format!("https.proxy={proxy}"));
    }
    let output = cmd
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("git spawn: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

pub fn has_changes(repo: &Path) -> GitResult<bool> {
    let out = run_git(repo, &["status", "--porcelain"])?;
    Ok(!out.trim().is_empty())
}

pub fn fetch(repo: &Path, remote: &str, branch: &str) -> GitResult<()> {
    run_git(repo, &["fetch", remote, branch])?;
    Ok(())
}

/// git add -A,然后把超阈值文件撤出暂存(仍留在工作区),返回被排除清单。
fn stage_except_oversized(repo: &Path) -> GitResult<Vec<String>> {
    let oversized = super::large_files::detect_oversized(repo)?;
    run_git(repo, &["add", "-A"])?;
    for f in &oversized {
        let _ = run_git(repo, &["reset", "--", f]);
    }
    Ok(oversized)
}

/// 暂存区是否有待提交内容(用于提交守卫)。
fn has_staged(repo: &Path) -> bool {
    run_git(repo, &["diff", "--cached", "--quiet"]).is_err()
}

/// 本地 HEAD 是否领先 {remote}/{branch}(存在已 commit 未 push 的提交)。
/// 远端跟踪引用缺失(首推或从未 fetch 成功)按领先处理,交给 push 判定;
/// 空仓库(HEAD 未诞生)无可推,按不领先处理。
fn is_ahead(repo: &Path, remote: &str, branch: &str) -> bool {
    if run_git(repo, &["rev-parse", "--verify", "HEAD"]).is_err() {
        return false;
    }
    match run_git(
        repo,
        &["rev-list", "--count", &format!("{remote}/{branch}..HEAD")],
    ) {
        Ok(n) => n.trim().parse::<u64>().map(|c| c > 0).unwrap_or(true),
        Err(_) => true,
    }
}

/// 远端是否有本地还没有的提交。只有为真时才允许动工作区 —— 单设备连续编辑
/// 时这一步恒为 false,同步全程不碰用户正在编辑的文件。
/// 远端跟踪引用缺失(首推)或空仓库按「无新提交」处理。
fn remote_ahead(repo: &Path, remote: &str, branch: &str) -> bool {
    if run_git(repo, &["rev-parse", "--verify", "HEAD"]).is_err() {
        return false;
    }
    match run_git(
        repo,
        &["rev-list", "--count", &format!("HEAD..{remote}/{branch}")],
    ) {
        Ok(n) => n.trim().parse::<u64>().map(|c| c > 0).unwrap_or(false),
        Err(_) => false,
    }
}

/// 一轮同步。
///
/// **不变式:本地改动先落成提交,工作区在同步期间绝不被按回 HEAD。**
///
/// 旧实现走 `stash push → rebase → stash pop`,靠把本地改动挪开来腾出干净的树。
/// 但 `git stash push` 会把工作区重置到 HEAD —— 在真实 vault(14k 文件)上实测
/// 有 ~200ms 的窗口,用户刚敲下的字从磁盘上消失。编辑器的文件监听撞进这个窗口
/// 就弹「已被其他应用修改」,而横幅缓存的 `pendingExternal` 正是那份旧内容,点
/// 「从磁盘重新加载」直接覆盖掉用户的编辑(真实丢过一条批注)。
///
/// 改成 commit-first:提交之后工作区自然干净,合并远端不再需要挪动任何未提交
/// 内容。代价是远端分叉时留下合并提交而非线性历史 —— vault 是自动同步的私有
/// 仓库,历史形态远不如「不丢字」重要。
pub fn sync(repo: &Path, remote: &str, branch: &str) -> GitResult<SyncReport> {
    let has_remote = run_git(repo, &["remote", "get-url", remote]).is_ok();
    let mut skipped_large: Vec<String> = Vec::new();

    {
        let writer = crate::memory_control::v2::RepositoryWriter::new(repo);
        let _memory_transaction = writer
            .begin()
            .map_err(|error| format!("Memory v2 sync lock failed: {error}"))?;
        // Memory source snapshot, derived projections, staging and commit share
        // one lock boundary, so the commit cannot knowingly contain an older
        // projection than its authoritative YAML collection.
        reconcile_memory_v2(repo)?;

        // ① 本地改动先入库。此后工作区干净,后续步骤无须触碰用户正在编辑的文件。
        if has_changes(repo)? {
            skipped_large = stage_except_oversized(repo)?;
            if has_staged(repo) {
                let ts = chrono_now();
                run_git(repo, &["commit", "-m", &format!("vault: auto-sync {ts}")])?;
            }
        }
    }

    if !has_remote {
        return Ok(SyncReport { skipped_large });
    }

    let fetch_ok = fetch(repo, remote, branch).is_ok();

    // ② 只有远端确实有新提交时才合并。单设备场景下这里恒为 no-op,磁盘上的
    //    文件内容自始至终是用户自己那份。
    if fetch_ok && remote_ahead(repo, remote, branch) {
        let writer = crate::memory_control::v2::RepositoryWriter::new(repo);
        let _memory_transaction = writer
            .begin()
            .map_err(|error| format!("Memory v2 sync lock failed: {error}"))?;
        // A Memory RPC may have completed while fetch was in flight. Commit
        // that lock-consistent snapshot before touching the worktree by merge.
        reconcile_memory_v2(repo)?;
        if has_changes(repo)? {
            let more = stage_except_oversized(repo)?;
            for file in more {
                if !skipped_large.contains(&file) {
                    skipped_large.push(file);
                }
            }
            if has_staged(repo) {
                run_git(
                    repo,
                    &["commit", "-m", "vault: auto-sync before remote merge"],
                )?;
            }
        }
        let upstream = format!("{remote}/{branch}");
        let can_fast_forward =
            run_git(repo, &["merge-base", "--is-ancestor", "HEAD", &upstream]).is_ok();
        if can_fast_forward {
            validate_memory_v2_revision(repo, &upstream)?;
            run_git(repo, &["merge", "--ff-only", &upstream])?;
        } else {
            // 真分叉:合并而非 rebase。rebase 会先把工作区 checkout 回上游再逐个
            // 重放本地提交,等于把 ① 要消灭的回滚窗口原样请回来。
            if run_git(repo, &["merge", "--no-commit", "--no-edit", &upstream]).is_err() {
                if !repo.join(".git").join("MERGE_HEAD").exists() {
                    let _ = run_git(repo, &["merge", "--abort"]);
                    return Err("merge failed, skipping cycle".into());
                }
                // Immutable authoritative Memory assets must never use the
                // ordinary ours/theirs fallback. A same-path collision means
                // tampering, an ID collision, or protocol damage.
                let conflicted = conflicted_paths(repo)?;
                if conflicted
                    .iter()
                    .any(|path| path.starts_with(".notemd/memory/"))
                {
                    let detail = conflicted
                        .iter()
                        .filter(|path| path.starts_with(".notemd/memory/"))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    let _ = run_git(repo, &["merge", "--abort"]);
                    return Err(format!(
                        "MEMORY_SOURCE_CONFLICT: authoritative Memory assets conflicted: {detail}"
                    ));
                }
                // 冲突:留一份含双方内容的副本,工作区取本地版本(用户刚写的字优先)。
                super::conflict::handle_conflicts(repo, "--ours")?;
            }
        }

        // Git merges immutable records; the reducer decides semantic heads and
        // rewrites the disposable projections from the merged collection.
        if let Err(error) = reconcile_memory_v2(repo) {
            if repo.join(".git").join("MERGE_HEAD").exists() {
                let _ = run_git(repo, &["merge", "--abort"]);
            }
            return Err(error);
        }
        if has_changes(repo)? {
            let more = stage_except_oversized(repo)?;
            for file in more {
                if !skipped_large.contains(&file) {
                    skipped_large.push(file);
                }
            }
        }
        let merging = repo.join(".git").join("MERGE_HEAD").exists();
        let staged = has_staged(repo);
        if staged || merging {
            let mut args = vec!["commit"];
            if merging && !staged {
                // Divergent histories can have identical trees (for example,
                // each device made an empty metadata commit). `status
                // --porcelain` is empty in that state even though MERGE_HEAD
                // still requires a merge commit before push can converge.
                args.push("--allow-empty");
            }
            args.extend(["-m", "vault: reconcile Memory v2 projections"]);
            run_git(repo, &args)?;
        }
    }

    // ③ 树干净≠已同步:上轮 commit 成功但 push 失败会留下滞留提交,
    //    不补推就 return Ok 会把失败盖成"Sync completed"且永不重试。
    if is_ahead(repo, remote, branch) {
        run_git(repo, &["push", remote, branch])
            .map_err(|e| format!("push failed (will retry): {e}"))?;
    }

    Ok(SyncReport { skipped_large })
}

fn conflicted_paths(repo: &Path) -> GitResult<Vec<String>> {
    let out = run_git(repo, &["diff", "--name-only", "--diff-filter=U"])?;
    Ok(out
        .lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

fn reconcile_memory_v2(repo: &Path) -> GitResult<()> {
    use crate::memory_control::v2::{rebuild_projections_unlocked, RepositoryMode, V2Repository};

    let snapshot = V2Repository::new(repo)
        .load()
        .map_err(|error| format!("Memory v2 reconcile failed: {error}"))?;
    match snapshot.mode {
        RepositoryMode::V2Active => rebuild_projections_unlocked(repo)
            .map(|_| ())
            .map_err(|error| format!("Memory v2 reconcile failed: {error}")),
        RepositoryMode::V2Incomplete => Err(
            "MEMORY_PROTOCOL_INCOMPLETE: Memory v2 assets exist without a valid activation".into(),
        ),
        RepositoryMode::Absent => Ok(()),
    }
}

fn validate_memory_v2_revision(repo: &Path, revision: &str) -> GitResult<()> {
    let temp = tempfile::tempdir().map_err(|error| format!("Memory v2 validation: {error}"))?;
    let worktree = temp.path().join("candidate");
    run_git(
        repo,
        &[
            "worktree",
            "add",
            "--detach",
            "--force",
            worktree
                .to_str()
                .ok_or("Memory v2 validation path is not UTF-8")?,
            revision,
        ],
    )?;
    let validation = reconcile_memory_v2(&worktree);
    let cleanup = run_git(
        repo,
        &[
            "worktree",
            "remove",
            "--force",
            worktree
                .to_str()
                .ok_or("Memory v2 validation path is not UTF-8")?,
        ],
    );
    validation?;
    cleanup.map(|_| ())
}

fn chrono_now() -> String {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{secs}")
}

#[cfg(test)]
mod proxy_tests {
    use super::validate_proxy_url;

    #[test]
    fn blank_means_no_proxy() {
        for raw in ["", "   ", "\t\n"] {
            assert_eq!(validate_proxy_url(raw), Ok(None), "{raw:?}");
        }
    }

    #[test]
    fn accepts_the_schemes_git_understands() {
        for raw in [
            "http://127.0.0.1:1080",
            "https://proxy.corp:8443",
            "socks5://127.0.0.1:1080",
            "socks5h://127.0.0.1:1080",
            "http://user:pass@proxy.corp:3128",
        ] {
            assert_eq!(validate_proxy_url(raw), Ok(Some(raw.to_string())), "{raw}");
        }
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(
            validate_proxy_url("  http://127.0.0.1:1080  "),
            Ok(Some("http://127.0.0.1:1080".to_string()))
        );
    }

    #[test]
    fn rejects_a_bare_host_port() {
        // The commonest mistake: pasting what a proxy app shows in its UI.
        let err = validate_proxy_url("127.0.0.1:1080").unwrap_err();
        assert!(err.contains("http://127.0.0.1:1080"), "{err}");
    }

    #[test]
    fn rejects_a_scheme_git_cannot_use() {
        let err = validate_proxy_url("ftp://127.0.0.1:1080").unwrap_err();
        assert!(err.contains("unsupported proxy scheme"), "{err}");
        assert!(err.contains("ftp"), "{err}");
    }

    #[test]
    fn rejects_a_url_without_a_host() {
        assert!(validate_proxy_url("http://").is_err());
    }

    /// Ask a real git what it sees. This is the assertion that matters: it
    /// proves the override actually reaches the subprocess (and that `-c`
    /// lands before the subcommand, which is the one way to get this wrong).
    #[test]
    fn git_receives_the_proxy_override() {
        let dir = tempfile::TempDir::new().unwrap();
        super::super::git_ops::run_git_with_proxy(dir.path(), &["init", "-q"], None).unwrap();

        let got = super::super::git_ops::run_git_with_proxy(
            dir.path(),
            &["config", "--get", "http.proxy"],
            Some("http://127.0.0.1:1080"),
        )
        .unwrap();
        assert_eq!(got.trim(), "http://127.0.0.1:1080");

        let https = super::super::git_ops::run_git_with_proxy(
            dir.path(),
            &["config", "--get", "https.proxy"],
            Some("http://127.0.0.1:1080"),
        )
        .unwrap();
        assert_eq!(https.trim(), "http://127.0.0.1:1080");
    }

    /// …and that nothing was persisted: with no proxy passed, git must report
    /// none. A `-c` that leaked into .git/config would show up here.
    #[test]
    fn the_override_is_not_written_to_the_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        super::super::git_ops::run_git_with_proxy(dir.path(), &["init", "-q"], None).unwrap();
        super::super::git_ops::run_git_with_proxy(
            dir.path(),
            &["config", "--list"],
            Some("http://127.0.0.1:1080"),
        )
        .unwrap();

        // `config --get` exits non-zero when the key is unset.
        let after = super::super::git_ops::run_git_with_proxy(
            dir.path(),
            &["config", "--get", "http.proxy"],
            None,
        );
        assert!(
            after.is_err(),
            "http.proxy leaked into the repo config: {after:?}"
        );
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &std::path::Path, args: &[&str]) {
        assert!(Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success());
    }

    fn init_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "t@t"]);
        git(dir.path(), &["config", "user.name", "t"]);
        dir
    }

    #[test]
    fn stage_except_oversized_leaves_big_file_unstaged() {
        let dir = init_repo();
        std::fs::write(dir.path().join("small.md"), "hi").unwrap();
        std::fs::write(dir.path().join("big.bin"), vec![b'x'; 11 * 1024 * 1024]).unwrap();
        let skipped = stage_except_oversized(dir.path()).unwrap();
        assert_eq!(skipped, vec!["big.bin".to_string()]);
        let staged = run_git(dir.path(), &["diff", "--cached", "--name-only"]).unwrap();
        assert!(staged.contains("small.md"));
        assert!(!staged.contains("big.bin"));
    }

    #[test]
    fn sync_no_remote_skips_big_and_commits_rest() {
        let dir = init_repo();
        std::fs::write(dir.path().join("note.md"), "content").unwrap();
        std::fs::write(dir.path().join("huge.bin"), vec![b'x'; 11 * 1024 * 1024]).unwrap();
        let report = sync(dir.path(), "origin", "main").unwrap();
        assert_eq!(report.skipped_large, vec!["huge.bin".to_string()]);
        let tree = run_git(dir.path(), &["ls-tree", "-r", "--name-only", "HEAD"]).unwrap();
        assert!(tree.contains("note.md"));
        assert!(!tree.contains("huge.bin"));
    }

    #[test]
    fn sync_fails_closed_when_memory_v2_is_partially_activated() {
        let dir = init_repo();
        std::fs::create_dir_all(dir.path().join(".notemd/memory/claims/aa")).unwrap();

        let error = sync(dir.path(), "origin", "main").unwrap_err();

        assert!(error.contains("MEMORY_PROTOCOL_INCOMPLETE"), "{error}");
        assert!(
            run_git(dir.path(), &["rev-parse", "--verify", "HEAD"]).is_err(),
            "an incomplete authority tree must not be committed"
        );
    }

    /// bare 远端 + 已推首个提交的工作仓库,返回 (work, bare)。
    fn init_remote_pair(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let bare = root.join("remote.git");
        // `-b main` explicitly: without it the bare repo's HEAD follows the
        // machine's `init.defaultBranch` (still `master` by default), so a
        // later `clone` lands on a nonexistent branch and `push origin main`
        // dies with "src refspec main does not match any". The work tree below
        // already pins `-b main`; the remote must agree.
        git(root, &["init", "--bare", "-q", "-b", "main", "remote.git"]);
        let work = root.join("work");
        std::fs::create_dir(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        git(&work, &["config", "user.email", "t@t"]);
        git(&work, &["config", "user.name", "t"]);
        // These tests assert file contents byte-for-byte. A machine with the
        // Windows Git default `core.autocrlf=true` would rewrite LF to CRLF on
        // checkout and every such assertion would fail for reasons unrelated to
        // sync. Pin it per-repo rather than depending on the host's global.
        git(&work, &["config", "core.autocrlf", "false"]);
        git(&work, &["remote", "add", "origin", bare.to_str().unwrap()]);
        std::fs::write(work.join("note.md"), "base\n").unwrap();
        git(&work, &["add", "note.md"]);
        git(&work, &["commit", "-q", "-m", "seed"]);
        git(&work, &["push", "-q", "origin", "main"]);
        (work, bare)
    }

    #[test]
    fn push_failure_then_clean_cycle_retries_push() {
        let root = TempDir::new().unwrap();
        let (work, bare) = init_remote_pair(root.path());

        // 远端不可达的一轮:commit 落盘、push 失败
        let missing = root.path().join("missing.git");
        git(
            &work,
            &["remote", "set-url", "origin", missing.to_str().unwrap()],
        );
        std::fs::write(work.join("note.md"), "stranded\n").unwrap();
        let err = sync(&work, "origin", "main").unwrap_err();
        assert!(err.contains("push failed"), "unexpected error: {err}");

        // 远端恢复后的下一轮:工作区已干净,必须补推滞留提交
        git(
            &work,
            &["remote", "set-url", "origin", bare.to_str().unwrap()],
        );
        sync(&work, "origin", "main").unwrap();

        let local = run_git(&work, &["rev-parse", "HEAD"]).unwrap();
        let remote_head = run_git(&bare, &["rev-parse", "main"]).unwrap();
        assert_eq!(local.trim(), remote_head.trim(), "干净树周期应补推滞留提交");

        // 已同步的干净树再跑一轮仍应成功
        sync(&work, "origin", "main").unwrap();
    }

    /// 另一设备推进一个改动 `note.md` 的提交,返回该 clone 的路径。
    fn push_from_other_device(root: &std::path::Path, file: &str, content: &str) {
        let other = root.join("other");
        if !other.exists() {
            git(root, &["clone", "-q", "remote.git", "other"]);
            git(&other, &["config", "user.email", "o@o"]);
            git(&other, &["config", "user.name", "o"]);
            git(&other, &["config", "core.autocrlf", "false"]);
        }
        std::fs::write(other.join(file), content).unwrap();
        git(&other, &["add", "-A"]);
        git(&other, &["commit", "-q", "-m", "theirs"]);
        git(&other, &["push", "-q", "origin", "main"]);
    }

    /// 在 `f` 执行期间以最高频率读 `path`,返回「读到过的、不含 `marker` 的内容」
    /// (`None` = 从没读到过)。
    ///
    /// 返回内容而不是 bool:一次「不含标记」的读有两种可能 —— 工作区真的被按回
    /// 了 HEAD,或者读者撞上了 git 的非原子写(open→truncate→write)读到半截。
    /// 前者是回归,后者是采样噪声,只有把实际内容打出来才分得清。空串一律按撕裂
    /// 读丢弃:任何一个真实版本的 note.md 都不是空的。
    fn watch_for_revert<R>(
        path: &std::path::Path,
        marker: &str,
        f: impl FnOnce() -> R,
    ) -> (R, Option<String>) {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{Arc, Mutex};

        let stop = Arc::new(AtomicBool::new(false));
        let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let sampler = {
            let (stop, seen) = (stop.clone(), seen.clone());
            let (path, marker) = (path.to_path_buf(), marker.to_string());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Ok(s) = std::fs::read_to_string(&path) {
                        if !s.is_empty() && !s.contains(&marker) {
                            *seen.lock().unwrap() = Some(s);
                        }
                    }
                }
            })
        };
        let out = f();
        stop.store(true, Ordering::Relaxed);
        sampler.join().unwrap();
        let seen = seen.lock().unwrap().clone();
        (out, seen)
    }

    /// 回归(2026-08-06):同步绝不能把工作区按回 HEAD。
    ///
    /// 旧实现走 `git add -A → stash push → rebase → stash pop`,而 `stash push`
    /// 会把工作区重置到 HEAD —— 在 14k 文件的真实 vault 上实测有 ~200ms 的窗口,
    /// 用户刚敲下的字从磁盘上消失。编辑器的文件监听正好在这个窗口读盘,于是每轮
    /// 同步都弹一次「已被其他应用修改」,而横幅缓存的 `pendingExternal` 是那份旧
    /// 内容 —— 点「从磁盘重新加载」就把用户的编辑覆盖掉(真实丢过一条批注)。
    #[test]
    fn sync_never_reverts_working_tree() {
        let root = TempDir::new().unwrap();
        let (work, _bare) = init_remote_pair(root.path());

        let note = work.join("note.md");
        std::fs::write(&note, "base\nUSER-EDIT\n").unwrap();

        let (res, reverted) =
            watch_for_revert(&note, "USER-EDIT", || sync(&work, "origin", "main"));
        res.unwrap();

        assert_eq!(reverted, None, "同步期间用户的编辑从磁盘上消失过,读到的是");
        let reflog = run_git(&work, &["reflog", "--date=iso"]).unwrap();
        assert!(
            !reflog.contains("reset: moving to HEAD"),
            "工作区被 reset 回 HEAD:\n{reflog}"
        );
        assert!(
            run_git(&work, &["stash", "list"])
                .unwrap()
                .trim()
                .is_empty(),
            "不应残留 stash"
        );
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "base\nUSER-EDIT\n");
    }

    /// 远端也有新提交时同样不许回滚工作区:先提交本地,再合并远端。
    #[test]
    fn diverged_sync_merges_without_reverting() {
        let root = TempDir::new().unwrap();
        let (work, _bare) = init_remote_pair(root.path());
        push_from_other_device(root.path(), "theirs.md", "from other device\n");

        let note = work.join("note.md");
        std::fs::write(&note, "base\nUSER-EDIT\n").unwrap();

        let (res, reverted) =
            watch_for_revert(&note, "USER-EDIT", || sync(&work, "origin", "main"));
        res.unwrap();

        assert_eq!(
            reverted, None,
            "分叉合并期间用户的编辑从磁盘上消失过,读到的是"
        );
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "base\nUSER-EDIT\n");
        assert_eq!(
            std::fs::read_to_string(work.join("theirs.md")).unwrap(),
            "from other device\n",
            "远端的改动应被合并进来"
        );
        assert!(
            run_git(&work, &["status", "--porcelain"])
                .unwrap()
                .trim()
                .is_empty(),
            "同步后工作区应干净"
        );
    }

    #[test]
    fn divergent_empty_commits_are_closed_by_an_empty_merge_commit() {
        let root = TempDir::new().unwrap();
        let (work, bare) = init_remote_pair(root.path());
        let other = root.path().join("other-empty");
        git(root.path(), &["clone", "-q", "remote.git", "other-empty"]);
        git(&other, &["config", "user.email", "o@o"]);
        git(&other, &["config", "user.name", "o"]);

        git(
            &work,
            &["commit", "--allow-empty", "-q", "-m", "local empty"],
        );
        git(
            &other,
            &["commit", "--allow-empty", "-q", "-m", "remote empty"],
        );
        git(&other, &["push", "-q", "origin", "main"]);

        sync(&work, "origin", "main").unwrap();

        assert!(
            !work.join(".git/MERGE_HEAD").exists(),
            "sync must never return with an unfinished merge"
        );
        let head = run_git(&work, &["rev-parse", "HEAD"]).unwrap();
        let remote_head = run_git(&bare, &["rev-parse", "main"]).unwrap();
        assert_eq!(head.trim(), remote_head.trim());
        let parents = run_git(&work, &["rev-list", "--parents", "-n", "1", "HEAD"]).unwrap();
        assert_eq!(
            parents.split_whitespace().count(),
            3,
            "the converged head must have both divergent commits as parents"
        );
    }

    /// 两边改同一文件:本地内容留在工作区,完整的冲突全文另存一份副本。
    #[test]
    fn conflicting_edit_keeps_local_and_saves_conflict_copy() {
        let root = TempDir::new().unwrap();
        let (work, _bare) = init_remote_pair(root.path());
        push_from_other_device(root.path(), "note.md", "theirs\n");

        let note = work.join("note.md");
        std::fs::write(&note, "base\nUSER-EDIT\n").unwrap();

        sync(&work, "origin", "main").unwrap();

        assert_eq!(
            std::fs::read_to_string(&note).unwrap(),
            "base\nUSER-EDIT\n",
            "冲突时本地编辑必须留在工作区"
        );
        let copies: Vec<_> = std::fs::read_dir(&work)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with("note.conflict.") && n.ends_with(".md"))
            .collect();
        assert_eq!(copies.len(), 1, "应留下一份冲突副本, got {copies:?}");
        let copy = std::fs::read_to_string(work.join(&copies[0])).unwrap();
        assert!(
            copy.contains("USER-EDIT") && copy.contains("theirs"),
            "副本应含双方内容:\n{copy}"
        );
        assert!(
            !work.join(".git/MERGE_HEAD").exists(),
            "不应停留在合并中间态"
        );
    }

    #[test]
    fn sync_only_big_file_makes_no_commit() {
        let dir = init_repo();
        std::fs::write(dir.path().join("seed.md"), "seed").unwrap();
        git(dir.path(), &["add", "seed.md"]);
        git(dir.path(), &["commit", "-q", "-m", "seed"]);
        let head_before = run_git(dir.path(), &["rev-parse", "HEAD"]).unwrap();
        std::fs::write(dir.path().join("only.bin"), vec![b'x'; 11 * 1024 * 1024]).unwrap();
        let report = sync(dir.path(), "origin", "main").unwrap();
        assert_eq!(report.skipped_large, vec!["only.bin".to_string()]);
        let head_after = run_git(dir.path(), &["rev-parse", "HEAD"]).unwrap();
        assert_eq!(head_before, head_after, "不应产生空 commit");
    }
}
