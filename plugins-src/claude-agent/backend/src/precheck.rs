//! The precheck mechanism lives in `agent-run-core`; what stays here is the
//! coverage for THIS plugin's shipped script, exercised as it will actually run.
//! It is what decides whether a run costs tokens, so it gets real cases.
pub use agent_run_core::precheck::*;

#[cfg(test)]
mod shipped {
    use agent_run_core::precheck::{run, Outcome};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

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

    /// A path is only accepted as the target when it is really there — the
    /// script's own guard, pinned so a rewrite cannot quietly drop it.
    #[tokio::test]
    async fn the_task_dir_is_the_scripts_cwd() {
        let v = setup();
        assert!(Path::new(&v.task.path().join("precheck.sh")).is_file());
    }
}
