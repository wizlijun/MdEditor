use std::path::PathBuf;

#[test]
fn vault_path_under_documents() {
    let base = PathBuf::from("/tmp/foo");
    let p = crate::vault_ios::path::resolve_vault_path(&base);
    assert_eq!(p, PathBuf::from("/tmp/foo/Vault"));
}

#[test]
fn vault_path_handles_trailing_slash() {
    let base = PathBuf::from("/tmp/foo/");
    let p = crate::vault_ios::path::resolve_vault_path(&base);
    assert_eq!(p, PathBuf::from("/tmp/foo/Vault"));
}

use std::fs;
use tempfile::tempdir;

#[test]
fn list_dir_filters_by_whitelist_and_hides_dotgit() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    fs::write(root.join("readme.md"), "x").unwrap();
    fs::write(root.join("board.canvas"), r#"{"nodes":[],"edges":[]}"#).unwrap();
    fs::write(root.join("a.txt"), "y").unwrap();
    fs::write(root.join("photo.png"), &[0u8; 8]).unwrap();
    fs::create_dir(root.join("subdir")).unwrap();
    fs::create_dir(root.join(".git")).unwrap();
    fs::write(root.join("ignore.pdf"), "z").unwrap();
    fs::write(root.join(".DS_Store"), "").unwrap();

    let entries = crate::vault_ios::list_dir::list(root, "").unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

    assert!(names.contains(&"readme.md"));
    assert!(names.contains(&"board.canvas"));
    assert!(names.contains(&"a.txt"));
    assert!(names.contains(&"photo.png"));
    assert!(names.contains(&"subdir"));
    assert!(!names.contains(&".git"));
    assert!(!names.contains(&"ignore.pdf"));
    assert!(!names.contains(&".DS_Store"));
}

#[test]
fn list_dir_lists_mdx_documents() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("guide.mdx"), "x").unwrap();

    let entries = crate::vault_ios::list_dir::list(root, "").unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();

    assert!(names.contains(&"guide.mdx"));
}

#[test]
fn list_dir_returns_kind_and_size() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("foo.md"), "hello world").unwrap();
    fs::create_dir(root.join("sub")).unwrap();

    let entries = crate::vault_ios::list_dir::list(root, "").unwrap();
    let md = entries.iter().find(|e| e.name == "foo.md").unwrap();
    assert_eq!(md.kind, "file");
    assert_eq!(md.size, Some(11));
    assert_eq!(md.ext.as_deref(), Some("md"));

    let sub = entries.iter().find(|e| e.name == "sub").unwrap();
    assert_eq!(sub.kind, "dir");
    assert_eq!(sub.size, None);
}

#[test]
fn list_dir_relative_path_navigates() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("notes")).unwrap();
    fs::write(root.join("notes/today.md"), "x").unwrap();

    let entries = crate::vault_ios::list_dir::list(root, "notes").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "today.md");
}

#[test]
fn list_dir_rejects_path_traversal() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let result = crate::vault_ios::list_dir::list(root, "../etc");
    assert!(result.is_err());
}

#[test]
fn keychain_stub_roundtrip() {
    let dir = tempdir().unwrap();
    std::env::set_var("MDEDITOR_KEYCHAIN_STUB_DIR", dir.path());
    crate::vault_ios::keychain::stub::set("pat", "secret-token").unwrap();
    let got = crate::vault_ios::keychain::stub::get("pat").unwrap();
    assert_eq!(got.as_deref(), Some("secret-token"));
    crate::vault_ios::keychain::stub::delete("pat").unwrap();
    let gone = crate::vault_ios::keychain::stub::get("pat").unwrap();
    assert_eq!(gone, None);
    std::env::remove_var("MDEDITOR_KEYCHAIN_STUB_DIR");
}

#[test]
fn sig_uses_configured_name_and_email() {
    let mgr = crate::vault_ios::VaultIosManager::new();
    *mgr.author_name.lock().unwrap() = "Alice".into();
    *mgr.author_email.lock().unwrap() = "a@example.com".into();
    let sig = crate::vault_ios::sig::author_sig(&mgr).unwrap();
    assert_eq!(sig.name(), Some("Alice"));
    assert_eq!(sig.email(), Some("a@example.com"));
}

#[test]
fn sig_falls_back_when_email_empty() {
    let mgr = crate::vault_ios::VaultIosManager::new();
    *mgr.author_name.lock().unwrap() = "Bob".into();
    // email left empty
    let sig = crate::vault_ios::sig::author_sig(&mgr).unwrap();
    assert_eq!(sig.email(), Some("noreply@mdeditor.local"));
}

use std::process::Command;

fn make_bare_remote() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let remote = dir.path().join("remote.git");
    // `-b main` explicitly: otherwise the bare repo's HEAD follows the machine's
    // `init.defaultBranch` (still `master` by default), the clone below lands on
    // a nonexistent branch, and every "main"-based assertion downstream fails.
    Command::new("git")
        .args(["init", "--bare", "-b", "main", remote.to_str().unwrap()])
        .output()
        .unwrap();

    let work = dir.path().join("work");
    Command::new("git").args(["clone", remote.to_str().unwrap(), work.to_str().unwrap()]).output().unwrap();
    std::fs::write(work.join("README.md"), "hello").unwrap();
    Command::new("git").args(["-C", work.to_str().unwrap(), "config", "user.email", "t@t.test"]).output().unwrap();
    Command::new("git").args(["-C", work.to_str().unwrap(), "config", "user.name", "t"]).output().unwrap();
    Command::new("git").args(["-C", work.to_str().unwrap(), "add", "."]).output().unwrap();
    Command::new("git").args(["-C", work.to_str().unwrap(), "commit", "-m", "init"]).output().unwrap();
    Command::new("git").args(["-C", work.to_str().unwrap(), "push", "origin", "HEAD:main"]).output().unwrap();

    (dir, remote)
}

#[test]
fn clone_local_bare_repo_succeeds() {
    let (_keep, remote) = make_bare_remote();
    let dest = tempdir().unwrap();
    let vault_dir = dest.path().join("Vault");

    let res = crate::vault_ios::clone::clone_repo(
        remote.to_str().unwrap(),
        "main",
        "fake-pat-not-checked-for-local-path",
        &vault_dir,
        |_progress| {},
    );
    assert!(res.is_ok(), "clone failed: {res:?}");
    assert!(vault_dir.join("README.md").exists());
    assert!(vault_dir.join(".git").exists());
}

fn clone_with_local_path(remote: &std::path::Path) -> tempfile::TempDir {
    let dest = tempdir().unwrap();
    let vault = dest.path().join("Vault");
    crate::vault_ios::clone::clone_repo(
        remote.to_str().unwrap(),
        "main",
        "n/a",
        &vault,
        |_| {},
    ).unwrap();
    dest
}

#[test]
fn sync_clean_workdir_fast_forwards() {
    let (_keep, remote) = make_bare_remote();
    let local = clone_with_local_path(&remote);
    let vault = local.path().join("Vault");

    // Make a remote commit through a separate working clone.
    let work = tempdir().unwrap();
    Command::new("git").args(["clone", remote.to_str().unwrap(), work.path().to_str().unwrap()]).output().unwrap();
    std::fs::write(work.path().join("new.md"), "hi").unwrap();
    Command::new("git").args(["-C", work.path().to_str().unwrap(), "add", "."]).output().unwrap();
    Command::new("git").args(["-C", work.path().to_str().unwrap(), "config", "user.email", "t@t"]).output().unwrap();
    Command::new("git").args(["-C", work.path().to_str().unwrap(), "config", "user.name", "t"]).output().unwrap();
    Command::new("git").args(["-C", work.path().to_str().unwrap(), "commit", "-m", "remote"]).output().unwrap();
    Command::new("git").args(["-C", work.path().to_str().unwrap(), "push"]).output().unwrap();

    let mgr = crate::vault_ios::VaultIosManager::new();
    *mgr.author_name.lock().unwrap() = "T".into();
    *mgr.author_email.lock().unwrap() = "t@t".into();

    let outcome = crate::vault_ios::sync::sync_once(&mgr, &vault, "main", remote.to_str().unwrap(), "fake-pat").unwrap();
    assert!(matches!(outcome, crate::vault_ios::sync::SyncOutcome::PullOnly));
    assert!(vault.join("new.md").exists());
}

#[test]
fn sync_dirty_workdir_commits_and_pushes() {
    let (_keep, remote) = make_bare_remote();
    let local = clone_with_local_path(&remote);
    let vault = local.path().join("Vault");

    std::fs::write(vault.join("README.md"), "edited").unwrap();

    let mgr = crate::vault_ios::VaultIosManager::new();
    *mgr.author_name.lock().unwrap() = "T".into();
    *mgr.author_email.lock().unwrap() = "t@t".into();

    let outcome = crate::vault_ios::sync::sync_once(&mgr, &vault, "main", remote.to_str().unwrap(), "fake-pat").unwrap();
    assert!(matches!(outcome, crate::vault_ios::sync::SyncOutcome::Pushed { .. }));

    // Verify remote received the push.
    let verify = tempdir().unwrap();
    Command::new("git").args(["clone", remote.to_str().unwrap(), verify.path().to_str().unwrap()]).output().unwrap();
    assert_eq!(std::fs::read_to_string(verify.path().join("README.md")).unwrap(), "edited");
}

#[test]
fn sync_conflict_keeps_local_as_conflict_file() {
    let (_keep, remote) = make_bare_remote();
    let local = clone_with_local_path(&remote);
    let vault = local.path().join("Vault");

    // Remote changes README.md
    let work = tempdir().unwrap();
    Command::new("git").args(["clone", remote.to_str().unwrap(), work.path().to_str().unwrap()]).output().unwrap();
    std::fs::write(work.path().join("README.md"), "remote-version").unwrap();
    Command::new("git").args(["-C", work.path().to_str().unwrap(), "add", "."]).output().unwrap();
    Command::new("git").args(["-C", work.path().to_str().unwrap(), "config", "user.email", "t@t"]).output().unwrap();
    Command::new("git").args(["-C", work.path().to_str().unwrap(), "config", "user.name", "t"]).output().unwrap();
    Command::new("git").args(["-C", work.path().to_str().unwrap(), "commit", "-m", "remote edit"]).output().unwrap();
    Command::new("git").args(["-C", work.path().to_str().unwrap(), "push"]).output().unwrap();

    // Local also changes README.md (different content)
    std::fs::write(vault.join("README.md"), "local-version").unwrap();

    let mgr = crate::vault_ios::VaultIosManager::new();
    *mgr.author_name.lock().unwrap() = "T".into();
    *mgr.author_email.lock().unwrap() = "t@t".into();

    let outcome = crate::vault_ios::sync::sync_once(&mgr, &vault, "main", remote.to_str().unwrap(), "fake-pat").unwrap();
    match outcome {
        crate::vault_ios::sync::SyncOutcome::Pushed { conflicts } => {
            assert!(!conflicts.is_empty(), "expected conflict log entries");
        }
        other => panic!("expected Pushed with conflicts, got {other:?}"),
    }

    // The .conflict.<ts>.md file should exist alongside README.md
    let entries: Vec<_> = std::fs::read_dir(&vault).unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(entries.iter().any(|n| n.starts_with("README.conflict.") && n.ends_with(".md")),
        "no conflict backup file found in {entries:?}");

    // The README.md itself should be the remote version (theirs).
    assert_eq!(std::fs::read_to_string(vault.join("README.md")).unwrap(), "remote-version");
}
