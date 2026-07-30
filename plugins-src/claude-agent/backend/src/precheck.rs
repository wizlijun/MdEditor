//! A task may declare a precheck script: a cheap, local answer to "is there
//! anything to do?" before a model ever starts. Answering a note with no open
//! questions costs real tokens and tells the user nothing.
//!
//! The script lives in the task template, so it is readable and editable like
//! everything else there. Exit 0 means run; any other code means skip, and
//! whatever it printed becomes the reason shown to the user.
use std::path::Path;
use std::process::Stdio;

/// A hung script must not wedge a run.
pub const TIMEOUT_SECS: u64 = 15;

#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Nothing declared, or the script said go.
    Run,
    /// The script says there is nothing to do, with its reason.
    Skip(String),
}

/// Run `task_dir/<script>` with the task directory as cwd.
///
/// `vault` and `target` reach the script as `NOTEMD_VAULT` / `NOTEMD_NOTE`,
/// which is how it knows WHICH file to look at. A script that cannot be run at
/// all (missing, not executable) is treated as "no opinion" — a broken check
/// should not block the work.
pub async fn run(
    task_dir: &Path,
    script: Option<&str>,
    vault: &Path,
    target: Option<&str>,
) -> Outcome {
    let Some(script) = script.filter(|s| !s.trim().is_empty()) else {
        return Outcome::Run;
    };
    let path = task_dir.join(script);
    if !path.is_file() {
        return Outcome::Run;
    }

    let mut cmd = tokio::process::Command::new(&path);
    cmd.current_dir(task_dir)
        .env("NOTEMD_VAULT", vault)
        .env("NOTEMD_NOTE", target.unwrap_or(""))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let out = tokio::time::timeout(
        std::time::Duration::from_secs(TIMEOUT_SECS),
        cmd.output(),
    )
    .await;

    match out {
        Ok(Ok(o)) if o.status.success() => Outcome::Run,
        Ok(Ok(o)) => {
            let said = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            let reason = if !said.is_empty() {
                said
            } else if !err.is_empty() {
                err
            } else {
                format!("precheck exited {}", o.status.code().unwrap_or(-1))
            };
            Outcome::Skip(reason)
        }
        // Could not be executed, or took too long: don't let the check itself
        // become the thing that stops the work.
        Ok(Err(_)) | Err(_) => Outcome::Run,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> String {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        name.to_string()
    }

    #[tokio::test]
    async fn no_script_declared_means_run() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(run(d.path(), None, d.path(), None).await, Outcome::Run);
        assert_eq!(run(d.path(), Some("  "), d.path(), None).await, Outcome::Run);
    }

    #[tokio::test]
    async fn exit_zero_means_run() {
        let d = tempfile::tempdir().unwrap();
        let s = script(d.path(), "ok.sh", "exit 0");
        assert_eq!(run(d.path(), Some(&s), d.path(), None).await, Outcome::Run);
    }

    #[tokio::test]
    async fn a_nonzero_exit_skips_with_what_it_printed() {
        let d = tempfile::tempdir().unwrap();
        let s = script(d.path(), "no.sh", "echo '没有待答的问题'\nexit 1");
        assert_eq!(
            run(d.path(), Some(&s), d.path(), None).await,
            Outcome::Skip("没有待答的问题".into())
        );
    }

    #[tokio::test]
    async fn a_silent_failure_still_explains_itself() {
        let d = tempfile::tempdir().unwrap();
        let s = script(d.path(), "quiet.sh", "exit 3");
        assert_eq!(
            run(d.path(), Some(&s), d.path(), None).await,
            Outcome::Skip("precheck exited 3".into())
        );
    }

    #[tokio::test]
    async fn stderr_is_used_when_nothing_went_to_stdout() {
        let d = tempfile::tempdir().unwrap();
        let s = script(d.path(), "err.sh", "echo 'file missing' >&2\nexit 1");
        assert_eq!(
            run(d.path(), Some(&s), d.path(), None).await,
            Outcome::Skip("file missing".into())
        );
    }

    #[tokio::test]
    async fn the_script_is_told_which_note_and_vault() {
        let d = tempfile::tempdir().unwrap();
        let s = script(
            d.path(),
            "env.sh",
            "[ \"$NOTEMD_NOTE\" = /v/a.note.md ] && [ -n \"$NOTEMD_VAULT\" ] && exit 0\nexit 1",
        );
        assert_eq!(
            run(d.path(), Some(&s), Path::new("/v"), Some("/v/a.note.md")).await,
            Outcome::Run
        );
    }

    /// The shipped script, exercised as it will actually run. It is what
    /// decides whether a run costs tokens, so it gets real cases.
    mod shipped {
        use super::*;

        const SCRIPT: &str = include_str!("../templates/answer-note-question/precheck.sh");

        struct Vault {
            dir: tempfile::TempDir,
            task: tempfile::TempDir,
        }

        fn setup() -> Vault {
            let task = tempfile::tempdir().unwrap();
            let p = task.path().join("precheck.sh");
            std::fs::write(&p, SCRIPT).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            Vault {
                dir: tempfile::tempdir().unwrap(),
                task,
            }
        }

        fn note(v: &Vault, name: &str, body: &str) -> String {
            let p = v.dir.path().join(name);
            std::fs::write(&p, body).unwrap();
            p.to_string_lossy().to_string()
        }

        async fn check(v: &Vault, target: Option<&str>) -> Outcome {
            run(v.task.path(), Some("precheck.sh"), v.dir.path(), target).await
        }

        #[tokio::test]
        async fn runs_when_the_note_has_an_open_question() {
            let v = setup();
            let n = note(&v, "a.note.md", "- ?为什么?\n  type:: question\n  status:: open\n");
            assert_eq!(check(&v, Some(&n)).await, Outcome::Run);
        }

        #[tokio::test]
        async fn skips_when_every_question_is_already_answered() {
            let v = setup();
            let n = note(
                &v,
                "a.note.md",
                "- ?为什么?\n  type:: question\n  status:: answered\n",
            );
            assert_eq!(
                check(&v, Some(&n)).await,
                Outcome::Skip("这篇手记里没有待答的问题".into())
            );
        }

        #[tokio::test]
        async fn skips_a_note_with_no_questions_at_all() {
            let v = setup();
            let n = note(&v, "a.note.md", "- 一条普通笔记\n");
            assert_eq!(
                check(&v, Some(&n)).await,
                Outcome::Skip("这篇手记里没有问题".into())
            );
        }

        #[tokio::test]
        async fn skips_when_the_note_does_not_exist() {
            let v = setup();
            let missing = v.dir.path().join("gone.note.md").to_string_lossy().to_string();
            match check(&v, Some(&missing)).await {
                Outcome::Skip(r) => assert!(r.contains("不存在"), "got {r}"),
                other => panic!("expected a skip, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn a_whole_vault_pass_runs_only_when_something_is_open() {
            let v = setup();
            note(&v, "quiet.note.md", "- 一条普通笔记\n");
            assert_eq!(
                check(&v, None).await,
                Outcome::Skip("vault 里没有待答的问题".into())
            );

            note(&v, "loud.note.md", "- ?为什么?\n  type:: question\n  status:: open\n");
            assert_eq!(check(&v, None).await, Outcome::Run);
        }
    }

    #[tokio::test]
    async fn a_broken_check_does_not_block_the_work() {
        let d = tempfile::tempdir().unwrap();
        // Declared but absent.
        assert_eq!(run(d.path(), Some("gone.sh"), d.path(), None).await, Outcome::Run);
        // Present but not executable.
        std::fs::write(d.path().join("noexec.sh"), "#!/bin/sh\nexit 1\n").unwrap();
        assert_eq!(
            run(d.path(), Some("noexec.sh"), d.path(), None).await,
            Outcome::Run
        );
    }
}
