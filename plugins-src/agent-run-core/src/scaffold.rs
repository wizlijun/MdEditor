//! The shape every run has, minus the transport.
//!
//! Every agent engine does the same five things around its different middle:
//! take the task lock, ask the precheck whether this is worth starting, track
//! progress across processes, collect what was written, and land a record. Those
//! five live here so providers cannot drift apart on what a "run" means
//! — the window and the host read one on-disk shape regardless of who produced it.
//!
//! What is NOT here is the middle: spawning the harness and turning its output
//! into [`crate::event::Event`]s. That is the part that genuinely differs.
use crate::{artifacts, lock, okf, precheck, record, task::TaskDef};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Everything the scaffold needs to know about a run. The harness-specific
/// engine holds this plus its own transport fields.
pub struct RunMeta {
    pub vault: PathBuf,
    pub task: TaskDef,
    pub task_dir: PathBuf,
    pub task_run_dir: PathBuf,
    pub run_id: String,
    pub trigger: String,
    /// The plugin id performing this run (`notemd.claude-agent`,
    /// `notemd.codex-agent`, `notemd.deepseek-agent`). Lands in the record so a shared runs root
    /// cannot make one harness's failure look like another's.
    pub harness: String,
    /// The one file this run is about, if any — handed to the precheck script as
    /// `NOTEMD_NOTE` so it can answer "is there anything to do?" locally.
    pub target: Option<String>,
    /// The file this run was asked to PRODUCE (absolute), if the caller named
    /// one. It usually lives outside `output/`/`answers/`, so it has to be
    /// declared to be collected as an artifact and OKF-stamped at all.
    pub deliverable: Option<PathBuf>,
}

/// The run holds this for its whole life; dropping it releases the task lock.
pub struct Started {
    pub guard: lock::Guard,
    /// Wall-clock start, for telling this run's output from an older run's.
    /// Taken before the lock so a file written the instant the harness starts
    /// still counts.
    pub started_at: SystemTime,
    pub started: chrono::DateTime<chrono::Utc>,
}

/// Why a run never got as far as starting the harness.
pub enum Blocked {
    /// The same task is already running, and by whom.
    Busy(lock::LockInfo),
    /// The precheck said there is nothing to do, with its reason.
    Skip(String),
}

/// Take the lock and run the precheck. `Ok` means the caller may now spawn.
///
/// The lock is acquired here and handed back inside [`Started`], so no error
/// path has to remember to release it.
pub async fn preflight(meta: &RunMeta) -> Result<Started, Blocked> {
    let started = chrono::Utc::now();
    let started_at = SystemTime::now();
    let guard = lock::acquire_for_run(
        &meta.task_run_dir,
        &meta.task.id,
        &meta.vault,
        meta.target.as_deref(),
        lock::LockInfo {
            pid: std::process::id() as i32,
            run_id: meta.run_id.clone(),
            started_at: started.to_rfc3339(),
        },
    )
    .map_err(|b| Blocked::Busy(b.0))?;

    if let precheck::Outcome::Skip(reason) = precheck::run(
        &meta.task_dir,
        meta.task.precheck.as_deref(),
        &meta.vault,
        meta.target.as_deref(),
    )
    .await
    {
        return Err(Blocked::Skip(reason));
    }
    Ok(Started {
        guard,
        started_at,
        started,
    })
}

/// The cross-process progress snapshot. It exists because progress has to be
/// visible ACROSS processes: the main window polls it, and the run it is
/// watching may belong to a detached CLI runner it has no channel to.
pub struct ProgressTracker {
    dir: PathBuf,
    run_id: String,
    inner: record::Progress,
}

impl ProgressTracker {
    pub fn start(task_run_dir: &Path, run_id: &str, at: chrono::DateTime<chrono::Utc>) -> Self {
        let inner = record::Progress {
            run_id: run_id.to_string(),
            steps: 0,
            last: String::new(),
            updated_at: at.to_rfc3339(),
        };
        record::write_progress(task_run_dir, &inner);
        Self {
            dir: task_run_dir.to_path_buf(),
            run_id: run_id.to_string(),
            inner,
        }
    }

    /// One thing happened. `line` goes verbatim into the run log; `label` is the
    /// short form the window shows as "what it's doing now".
    pub fn step(&mut self, label: &str, line: &str) {
        self.inner.steps += 1;
        self.inner.last = label.chars().take(80).collect();
        record::append_log(&self.dir, &self.run_id, line);
        self.inner.updated_at = chrono::Utc::now().to_rfc3339();
        record::write_progress(&self.dir, &self.inner);
    }

    pub fn steps(&self) -> u64 {
        self.inner.steps
    }
}

/// Land the run: collect artifacts, stamp any that the model left without OKF
/// front-matter, write the record, clear the progress snapshot.
///
/// `by` is the OKF §7 actor (`<producer>/<version>`) — `claude-agent/1.0.11`,
/// `codex/gpt-5`, `deepseek-harness/deepseek-v4-pro`. Never a `human:` prefix.
#[allow(clippy::too_many_arguments)]
pub fn finalize(
    meta: &RunMeta,
    started: &Started,
    status: record::Status,
    exit_code: Option<i32>,
    result: Option<crate::event::RunResult>,
    fallback_err: String,
    stderr_tail: String,
    by: &str,
) -> record::RunRecord {
    finalize_scoped(
        meta,
        started,
        status,
        exit_code,
        result,
        fallback_err,
        stderr_tail,
        by,
        true,
    )
}

/// Finalize with control over the legacy global `answers/` mtime scan. New
/// concurrent providers should pass `false` and use task-local `output/` or an
/// explicit deliverable, which can be attributed to one run without guessing.
#[allow(clippy::too_many_arguments)]
pub fn finalize_scoped(
    meta: &RunMeta,
    started: &Started,
    status: record::Status,
    exit_code: Option<i32>,
    result: Option<crate::event::RunResult>,
    fallback_err: String,
    stderr_tail: String,
    by: &str,
    include_vault_answers: bool,
) -> record::RunRecord {
    let mut found = artifacts::collect_with_answers(
        &meta.vault,
        &meta.task_dir,
        started.started_at,
        meta.deliverable.as_deref(),
        include_vault_answers,
    );
    // Concurrent AI reads share the task template/output directories. Only the
    // explicitly declared summary can be attributed to this run without an
    // mtime race against another book finishing at the same time.
    if lock::scoped_target(&meta.task.id, &meta.vault, meta.target.as_deref()).is_some() {
        let declared = meta
            .deliverable
            .as_deref()
            .and_then(|path| artifacts::vault_relative(&meta.vault, path));
        found.retain(|path| Some(path) == declared.as_ref());
    }
    // 提示词要求 agent 自己写 OKF 头,但那是约束不是保证:漏写就地补上,免得
    // vault 里多一份没有 `type` 的文档(OKF §4.1)。已有 frontmatter 的不碰。
    let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let target_rel = meta
        .deliverable
        .as_deref()
        .and_then(|d| artifacts::vault_relative(&meta.vault, d));
    let mut stamped = 0;
    if let Some(rel) = &target_rel {
        stamped += okf::stamp_vault_docs(
            &meta.vault,
            std::slice::from_ref(rel),
            meta.task.okf_type.as_deref().unwrap_or(okf::DEFAULT_TYPE),
            by,
            &at,
        );
    }
    let rest: Vec<String> = found
        .iter()
        .filter(|r| Some(*r) != target_rel.as_ref())
        .cloned()
        .collect();
    stamped += okf::stamp_vault_docs(&meta.vault, &rest, okf::DEFAULT_TYPE, by, &at);
    if stamped > 0 {
        eprintln!("[agent-run-core] stamped OKF front-matter on {stamped} file(s)");
    }

    let persistence_error = result.as_ref().and_then(|result| {
        record::write_terminal_result(&meta.task_run_dir, &meta.run_id, &result.result)
            .err()
            .map(|error| format!("persist complete terminal result: {error}"))
    });
    let mut rec = build_record(
        meta,
        started.started,
        status,
        exit_code,
        result,
        fallback_err,
        stderr_tail,
        found,
    );
    if let Some(error) = persistence_error {
        rec.status = record::Status::Error;
        rec.stderr_tail = record::tail(&error, record::STDERR_LIMIT);
    }
    let _ = record::write(&meta.task_run_dir, &rec);
    // The record is the answer from here on; a leftover snapshot would read as
    // a run still in flight.
    record::clear_progress_for(&meta.task_run_dir, &meta.run_id);
    rec
}

/// A terminal record for a run that never started (a failed spawn, a precheck
/// skip). Same landing as [`finalize`] minus the artifact sweep — there is
/// nothing to collect when nothing ran.
pub fn finalize_without_run(
    meta: &RunMeta,
    started: chrono::DateTime<chrono::Utc>,
    status: record::Status,
    message: String,
) -> record::RunRecord {
    let rec = build_record(
        meta,
        started,
        status,
        None,
        None,
        message,
        String::new(),
        Vec::new(),
    );
    let _ = record::write(&meta.task_run_dir, &rec);
    record::clear_progress_for(&meta.task_run_dir, &meta.run_id);
    rec
}

#[allow(clippy::too_many_arguments)]
fn build_record(
    meta: &RunMeta,
    started: chrono::DateTime<chrono::Utc>,
    status: record::Status,
    exit_code: Option<i32>,
    result: Option<crate::event::RunResult>,
    fallback_err: String,
    stderr_tail: String,
    artifacts: Vec<String>,
) -> record::RunRecord {
    // A spawn failure has no stream and no stderr — carry its message in both
    // fields so neither the window nor the record comes up blank.
    let stderr_tail = if stderr_tail.is_empty() && !fallback_err.is_empty() {
        fallback_err.clone()
    } else {
        stderr_tail
    };
    let usage = result.as_ref().and_then(|r| r.usage.clone());
    record::RunRecord {
        run_id: meta.run_id.clone(),
        task: meta.task.id.clone(),
        trigger: meta.trigger.clone(),
        started_at: started.to_rfc3339(),
        ended_at: chrono::Utc::now().to_rfc3339(),
        status,
        exit_code,
        num_turns: result.as_ref().and_then(|r| r.num_turns),
        session_id: result.as_ref().and_then(|r| r.session_id.clone()),
        result: record::tail(
            &result.map(|r| r.result).unwrap_or(fallback_err),
            record::RESULT_LIMIT,
        ),
        stderr_tail,
        artifacts,
        harness: Some(meta.harness.clone()),
        usage,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::RunResult;

    fn meta(dir: &Path) -> RunMeta {
        let task_dir = dir.join("task");
        std::fs::create_dir_all(&task_dir).unwrap();
        RunMeta {
            vault: dir.to_path_buf(),
            task: TaskDef {
                id: "t".into(),
                name: "T".into(),
                description: String::new(),
                prompt: "p".into(),
                max_turns: None,
                timeout_seconds: 30,
                model: None,
                precheck: None,
                okf_type: None,
                directive: Vec::new(),
                source_plugin: None,
            },
            task_dir,
            task_run_dir: dir.join("runs-t"),
            run_id: "20260817T000000Z-000001".into(),
            trigger: "window".into(),
            harness: "notemd.test-agent".into(),
            target: None,
            deliverable: None,
        }
    }

    #[tokio::test]
    async fn finalize_keeps_a_small_record_summary_and_an_exact_machine_result() {
        let dir = tempfile::tempdir().unwrap();
        let meta = meta(dir.path());
        let started = preflight(&meta).await.ok().unwrap();
        let full = format!("begin-{}-end", "x".repeat(record::RESULT_LIMIT * 2));
        let rec = finalize_scoped(
            &meta,
            &started,
            record::Status::Success,
            Some(0),
            Some(RunResult {
                is_error: false,
                result: full.clone(),
                session_id: None,
                num_turns: Some(1),
                usage: None,
            }),
            String::new(),
            String::new(),
            "test/1",
            false,
        );
        assert_eq!(rec.status, record::Status::Success);
        assert_eq!(rec.result.len(), record::RESULT_LIMIT);
        assert_eq!(
            record::read_terminal_result(&meta.task_run_dir, &meta.run_id)
                .unwrap()
                .as_deref(),
            Some(full.as_str())
        );
    }

    #[tokio::test]
    async fn finalize_fails_closed_when_the_complete_result_exceeds_its_limit() {
        let dir = tempfile::tempdir().unwrap();
        let meta = meta(dir.path());
        let started = preflight(&meta).await.ok().unwrap();
        let rec = finalize_scoped(
            &meta,
            &started,
            record::Status::Success,
            Some(0),
            Some(RunResult {
                is_error: false,
                result: "x".repeat(record::TERMINAL_RESULT_LIMIT + 1),
                session_id: None,
                num_turns: Some(1),
                usage: None,
            }),
            String::new(),
            String::new(),
            "test/1",
            false,
        );
        assert_eq!(rec.status, record::Status::Error);
        assert!(rec.stderr_tail.contains("persist complete terminal result"));
        assert!(
            record::read_terminal_result(&meta.task_run_dir, &meta.run_id)
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn preflight_takes_the_lock_and_lets_a_task_with_no_precheck_through() {
        let d = tempfile::tempdir().unwrap();
        let m = meta(d.path());
        let started = match preflight(&m).await {
            Ok(s) => s,
            Err(_) => panic!("a task with no precheck must be allowed to start"),
        };
        assert!(lock::current(&m.task_run_dir).is_some());
        drop(started);
        assert!(lock::current(&m.task_run_dir).is_none());
    }

    #[tokio::test]
    async fn the_same_task_cannot_preflight_twice_at_once() {
        let d = tempfile::tempdir().unwrap();
        let m = meta(d.path());
        let _first = preflight(&m).await.ok().expect("first must win");
        match preflight(&meta(d.path())).await {
            Err(Blocked::Busy(who)) => assert_eq!(who.run_id, m.run_id),
            _ => panic!("the second preflight must be refused as busy"),
        }
    }

    #[tokio::test]
    async fn ai_reads_of_different_books_preflight_together_but_one_book_does_not() {
        let d = tempfile::tempdir().unwrap();
        let make = |name: &str, run_id: &str| {
            let mut m = meta(d.path());
            m.task.id = "ai-read-ebook".into();
            m.run_id = run_id.into();
            let book = d.path().join(format!("books/{name}/book.md"));
            std::fs::create_dir_all(book.parent().unwrap()).unwrap();
            std::fs::write(&book, name).unwrap();
            m.target = Some(book.to_string_lossy().into_owned());
            m
        };
        let a = make("a", "run-a");
        let b = make("b", "run-b");
        let same_a = make("a", "run-a-again");
        let first = preflight(&a).await.ok().expect("first book must start");
        let second = preflight(&b).await.ok().expect("another book may run");
        match preflight(&same_a).await {
            Err(Blocked::Busy(who)) => assert_eq!(who.run_id, "run-a"),
            _ => panic!("the same book must stay exclusive across runs"),
        }
        assert_eq!(lock::current_all(&a.task_run_dir).len(), 2);
        drop((first, second));
    }

    /// The whole point of a precheck is spending no tokens — and it must not
    /// hold the lock afterwards either.
    #[tokio::test]
    async fn a_failing_precheck_blocks_the_run_and_releases_the_lock() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let mut m = meta(d.path());
        let check = m.task_dir.join("precheck.sh");
        std::fs::write(&check, "#!/bin/sh\necho '没有待答的问题'\nexit 1\n").unwrap();
        std::fs::set_permissions(&check, std::fs::Permissions::from_mode(0o755)).unwrap();
        m.task.precheck = Some("precheck.sh".into());

        match preflight(&m).await {
            Err(Blocked::Skip(reason)) => assert_eq!(reason, "没有待答的问题"),
            _ => panic!("expected a skip"),
        }
        assert!(
            lock::current(&m.task_run_dir).is_none(),
            "a skipped run must not leave the task wedged"
        );
    }

    #[tokio::test]
    async fn finalize_collects_artifacts_stamps_okf_and_lands_a_record() {
        let d = tempfile::tempdir().unwrap();
        let mut m = meta(d.path());
        m.task.okf_type = Some("Book Summary".into());
        let summary = d.path().join("ssot/b/summary.md");
        m.deliverable = Some(summary.clone());
        let started = preflight(&m).await.ok().unwrap();

        std::fs::create_dir_all(summary.parent().unwrap()).unwrap();
        std::fs::write(&summary, "# 深度工作 — 摘要\n").unwrap();
        std::fs::create_dir_all(d.path().join("answers")).unwrap();
        std::fs::write(d.path().join("answers/long.md"), "# long\n").unwrap();

        let rec = finalize(
            &m,
            &started,
            record::Status::Success,
            Some(0),
            Some(RunResult {
                is_error: false,
                result: "done".into(),
                session_id: Some("s1".into()),
                num_turns: Some(2),
                usage: None,
            }),
            String::new(),
            String::new(),
            "deepseek-harness/deepseek-v4-pro",
        );

        assert_eq!(rec.status, record::Status::Success);
        assert_eq!(rec.result, "done");
        assert_eq!(rec.session_id.as_deref(), Some("s1"));
        assert_eq!(rec.artifacts, vec!["answers/long.md", "ssot/b/summary.md"]);
        // The declared deliverable gets the TASK's type, everything else Answer.
        let got = std::fs::read_to_string(&summary).unwrap();
        assert!(
            got.starts_with("---\ntype: Book Summary\ntitle: \"深度工作 — 摘要\"\ngenerated: { by: deepseek-harness/deepseek-v4-pro,"),
            "got: {got}"
        );
        assert!(std::fs::read_to_string(d.path().join("answers/long.md"))
            .unwrap()
            .starts_with("---\ntype: Answer\n"));
        // Landed on disk, and the live snapshot is gone.
        assert_eq!(record::recent(&m.task_run_dir, 5).len(), 1);
        assert!(record::read_progress_for(&m.task_run_dir, &m.run_id).is_none());
    }

    #[tokio::test]
    async fn a_scoped_ai_read_only_claims_its_declared_deliverable() {
        let d = tempfile::tempdir().unwrap();
        let mut m = meta(d.path());
        m.task.id = "ai-read-ebook".into();
        let book = d.path().join("books/a/book.md");
        let summary = d.path().join("books/a/summary.md");
        std::fs::create_dir_all(book.parent().unwrap()).unwrap();
        std::fs::write(&book, "book").unwrap();
        m.target = Some(book.to_string_lossy().into_owned());
        m.deliverable = Some(summary.clone());
        let started = preflight(&m).await.ok().unwrap();

        std::fs::write(&summary, "# mine").unwrap();
        std::fs::create_dir_all(m.task_dir.join("output")).unwrap();
        std::fs::write(m.task_dir.join("output/other.md"), "# other").unwrap();
        let rec = finalize_scoped(
            &m,
            &started,
            record::Status::Success,
            Some(0),
            None,
            String::new(),
            String::new(),
            "test/1",
            false,
        );
        assert_eq!(rec.artifacts, vec!["books/a/summary.md"]);
    }

    #[tokio::test]
    async fn a_document_that_already_has_front_matter_is_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let m = meta(d.path());
        let started = preflight(&m).await.ok().unwrap();
        let body = "---\ntype: Answer\ntitle: \"x\"\n---\n# x\n";
        std::fs::create_dir_all(d.path().join("answers")).unwrap();
        std::fs::write(d.path().join("answers/a.md"), body).unwrap();

        finalize(
            &m,
            &started,
            record::Status::Success,
            Some(0),
            None,
            String::new(),
            String::new(),
            "deepseek-harness/x",
        );
        assert_eq!(
            std::fs::read_to_string(d.path().join("answers/a.md")).unwrap(),
            body
        );
    }

    #[test]
    fn a_run_that_never_started_still_lands_a_record() {
        let d = tempfile::tempdir().unwrap();
        let m = meta(d.path());
        let rec = finalize_without_run(
            &m,
            chrono::Utc::now(),
            record::Status::Skipped,
            "这篇手记里没有待答的问题".into(),
        );
        assert_eq!(rec.status, record::Status::Skipped);
        assert_eq!(rec.result, "这篇手记里没有待答的问题");
        // A spawn failure has no stream: the message has to reach both fields.
        assert_eq!(rec.stderr_tail, "这篇手记里没有待答的问题");
        assert_eq!(record::recent(&m.task_run_dir, 5).len(), 1);
    }

    #[test]
    fn the_progress_tracker_is_readable_from_another_process() {
        let d = tempfile::tempdir().unwrap();
        let m = meta(d.path());
        let mut p = ProgressTracker::start(&m.task_run_dir, &m.run_id, chrono::Utc::now());
        p.step("Read a.note.md", "Read a.note.md");
        p.step("answered it", "answered it");
        assert_eq!(p.steps(), 2);

        let seen = record::read_progress_for(&m.task_run_dir, &m.run_id).unwrap();
        assert_eq!(seen.run_id, m.run_id);
        assert_eq!(seen.steps, 2);
        assert_eq!(seen.last, "answered it");
        assert_eq!(
            record::read_log(&m.task_run_dir, &m.run_id).unwrap(),
            "Read a.note.md\nanswered it\n"
        );
    }

    #[test]
    fn a_long_progress_label_is_truncated_but_the_log_keeps_everything() {
        let d = tempfile::tempdir().unwrap();
        let m = meta(d.path());
        let mut p = ProgressTracker::start(&m.task_run_dir, &m.run_id, chrono::Utc::now());
        let long = "字".repeat(200);
        p.step(&long, &long);
        assert_eq!(
            record::read_progress_for(&m.task_run_dir, &m.run_id)
                .unwrap()
                .last
                .chars()
                .count(),
            80
        );
        assert!(record::read_log(&m.task_run_dir, &m.run_id)
            .unwrap()
            .contains(&long));
    }
}
