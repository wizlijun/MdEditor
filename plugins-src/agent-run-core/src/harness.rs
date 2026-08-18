//! What the user needs to know about the harness behind an agent, before they
//! trust a run to it: is it there, which version, which model, and did the last
//! run fail for a reason that will just happen again.
//!
//! That last part is why this is not merely `--version`. A harness whose
//! credentials expired reports its version perfectly happily and then fails
//! every run with "OAuth session expired". Version alone therefore answers the
//! wrong question — "is it installed" — while the user is asking "will it work".
//! So the status carries the last run's failure when that failure looks like an
//! environment problem rather than a task problem.
//!
//! Deliberately NOT here: an active credential probe. Verifying auth means
//! making a real model call — slow, possibly billed, on every window open. We
//! report what we observed rather than what we provoked.
use crate::record;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One agent's harness, as the window shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HarnessStatus {
    /// Display name of the harness itself: "Claude Code", "DeepSeek Harness".
    pub harness: String,
    /// Is the executable there? Everything else is decoration if this is false.
    pub ok: bool,
    /// Version string as the harness reports it, or `None` when it could not be asked.
    pub version: Option<String>,
    /// Where it was resolved from — a path, or "monorepo checkout at …".
    pub origin: String,
    /// The model a run uses when its task does not pin one.
    pub default_model: Option<String>,
    /// What to do when `ok` is false.
    pub hint: Option<String>,
    /// An environment-level problem observed in the most recent run — expired
    /// credentials, rate limits. Distinct from a task that merely failed.
    pub warning: Option<String>,
}

impl HarnessStatus {
    /// The harness could not be found at all.
    pub fn missing(harness: &str, hint: &str) -> Self {
        Self {
            harness: harness.to_string(),
            ok: false,
            version: None,
            origin: String::new(),
            default_model: None,
            hint: Some(hint.to_string()),
            warning: None,
        }
    }
}

/// What asking for a version told us.
#[derive(Debug, Clone, PartialEq)]
pub enum Probe {
    /// It answered. The first non-empty line — harnesses print `name x.y.z
    /// (extra)`, and the whole line beats guessing which token is the number.
    Version(String),
    /// It ran and FAILED. The launcher is present but not usable, and what it
    /// printed says why ("your pnpm is too old", "cannot find module"). Reporting
    /// this as a version would put an error message where a version belongs and
    /// call a broken harness ready.
    Failed(String),
    /// Could not be asked at all: missing, not executable, or it hung.
    Unavailable,
}

/// Ask an executable for its version, bounded so a hung binary cannot wedge the
/// window.
///
/// `path` is the `PATH` the child gets, and it is not optional in practice.
/// Both harnesses are `#!/usr/bin/env node` shims, and a GUI-launched host
/// inherits a `PATH` with no node in it — so probing without one fails with
/// `env: node: No such file or directory` and reports a perfectly healthy
/// harness as broken. The RUN path always set this; the probe did not, and a
/// probe that says `ok: false` disables the Run button, so the omission blocked
/// everything. (Missed because a probe run from a terminal inherits a full
/// PATH: it was tested in the one context where the bug cannot appear.)
///
/// Exit status is not decoration either: a launcher that fails and prints its
/// complaint to stdout would otherwise have that complaint shown AS the version,
/// with the harness marked ready.
pub fn probe_version(
    program: &Path,
    args: &[String],
    path: &str,
    timeout: std::time::Duration,
) -> Probe {
    let mut cmd = std::process::Command::new(program);
    cmd.env("PATH", path)
        .args(args)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let Ok(mut child) = cmd.spawn() else {
        return Probe::Unavailable;
    };

    // No portable "wait with timeout" in std; poll, which is fine at this
    // granularity and keeps the dependency list unchanged.
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            // Timed out, or the wait itself failed: do not leave the child behind.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Probe::Unavailable;
            }
        }
    }
    let Ok(out) = child.wait_with_output() else {
        return Probe::Unavailable;
    };
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    // Some tools version themselves on stderr.
    let text = if stdout.trim().is_empty() { &stderr } else { &stdout };
    let line = first_line(text);
    match (out.status.success(), line) {
        (true, Some(v)) => Probe::Version(v),
        (true, None) => Probe::Unavailable,
        // It failed: prefer whichever stream actually explains why.
        (false, _) => match first_line(&stderr).or_else(|| first_line(&stdout)) {
            Some(why) => Probe::Failed(why),
            None => Probe::Unavailable,
        },
    }
}

/// The first non-empty line, trimmed and capped.
pub fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(120).collect())
}

/// Environment-level failures — the run would fail again for the same reason,
/// whatever task it was. Matched case-insensitively on substrings that appear in
/// the harness's own message.
const ENVIRONMENT_FAILURES: [&str; 9] = [
    "oauth",
    "authenticate",
    "unauthorized",
    // Both spellings: harnesses name the variable (`DEEPSEEK_API_KEY`) as often
    // as they name the concept ("no API key").
    "api key",
    "api_key",
    "credential",
    "rate limit",
    "quota",
    "not logged in",
];

/// Does this failure look like the environment rather than the task?
pub fn is_environment_failure(message: &str) -> bool {
    let m = message.to_lowercase();
    ENVIRONMENT_FAILURES.iter().any(|k| m.contains(k))
}

/// The most recent environment-level failure among THIS harness's own runs.
///
/// `harness` is the caller's plugin id, and filtering on it is the whole point:
/// both agent plugins share one runs root, so an unfiltered read shows Claude's
/// expired OAuth in the DeepSeek window as though DeepSeek were broken. Records
/// written before the field existed carry no harness and are skipped — unknown
/// provenance is not "mine".
///
/// Only the newest of this harness's runs is considered: a credential that has
/// since been fixed must stop being reported the moment a run succeeds.
pub fn recent_environment_warning(runs_root: &Path, harness: &str) -> Option<String> {
    // Enough rows that the other harness's runs cannot bury ours, still bounded.
    let last = record::recent_all(runs_root, 60)
        .into_iter()
        .find(|r| r.harness.as_deref() == Some(harness))?;
    if last.status != record::Status::Error {
        return None;
    }
    let message = if last.result.trim().is_empty() {
        last.stderr_tail.clone()
    } else {
        last.result.clone()
    };
    is_environment_failure(&message).then(|| first_line(&message))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fake(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    const QUICK: std::time::Duration = std::time::Duration::from_secs(5);
    /// Enough for `/bin/sh` fakes; the point of the parameter is that callers
    /// must pass one at all.
    const PATH: &str = "/usr/bin:/bin";

    #[test]
    fn reads_a_version_off_stdout() {
        let d = tempfile::tempdir().unwrap();
        let p = fake(d.path(), "v", "echo '2.1.226 (Claude Code)'");
        assert_eq!(
            probe_version(&p, &[], PATH, QUICK),
            Probe::Version("2.1.226 (Claude Code)".into())
        );
    }

    /// Some tools version themselves on stderr; a blank answer would read as
    /// "not installed" and send the user chasing an install that is already there.
    #[test]
    fn falls_back_to_stderr_when_stdout_is_empty() {
        let d = tempfile::tempdir().unwrap();
        let p = fake(d.path(), "v", "echo '0.1.0-rc.6' >&2");
        assert_eq!(probe_version(&p, &[], PATH, QUICK), Probe::Version("0.1.0-rc.6".into()));
    }

    #[test]
    fn passes_the_launcher_args_before_version() {
        let d = tempfile::tempdir().unwrap();
        let p = fake(d.path(), "v", r#"echo "$*""#);
        assert_eq!(
            probe_version(&p, &["--dir".into(), "/repo".into()], PATH, QUICK),
            Probe::Version("--dir /repo --version".into())
        );
    }

    /// A hung binary must not wedge the window that asked.
    #[test]
    fn a_hung_binary_times_out_instead_of_blocking() {
        let d = tempfile::tempdir().unwrap();
        let p = fake(d.path(), "hang", "sleep 30");
        let started = std::time::Instant::now();
        assert_eq!(
            probe_version(&p, &[], PATH, std::time::Duration::from_millis(300)),
            Probe::Unavailable
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "took {:?}",
            started.elapsed()
        );
    }

    /// The bug this exists for. Both harnesses are `#!/usr/bin/env node` shims;
    /// a GUI-launched host has no node on its PATH, so a probe run without one
    /// dies with `env: node: No such file or directory` and reports a working
    /// harness as broken — which disables the Run button.
    #[test]
    fn the_child_gets_the_path_it_was_given_not_the_hosts() {
        let d = tempfile::tempdir().unwrap();
        // An interpreter that exists only where we choose to name.
        let bin_dir = d.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        fake(&bin_dir, "notemd-fake-node", r#"echo "9.9.9""#);

        // A shim that finds its interpreter through PATH, exactly like `dsh`.
        use std::os::unix::fs::PermissionsExt;
        let shim = d.path().join("shim");
        std::fs::write(&shim, "#!/usr/bin/env notemd-fake-node\n").unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Without that directory on PATH the shim cannot start at all…
        assert_ne!(
            probe_version(&shim, &[], "/usr/bin:/bin", QUICK),
            Probe::Version("9.9.9".into()),
            "a PATH without the interpreter must not look like a working harness"
        );
        // …and with it, the same binary answers.
        let with = format!("{}:/usr/bin:/bin", bin_dir.display());
        assert_eq!(
            probe_version(&shim, &[], &with, QUICK),
            Probe::Version("9.9.9".into())
        );
    }

    #[test]
    fn a_missing_binary_reads_as_no_version() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(probe_version(&d.path().join("nope"), &[], PATH, QUICK), Probe::Unavailable);
    }

    #[test]
    fn a_silent_binary_reads_as_no_version() {
        let d = tempfile::tempdir().unwrap();
        let p = fake(d.path(), "quiet", "exit 0");
        assert_eq!(probe_version(&p, &[], PATH, QUICK), Probe::Unavailable);
    }

    /// Caught live: a monorepo launcher goes through pnpm, and a pnpm version
    /// mismatch prints its complaint to STDOUT and exits non-zero. Taking the
    /// first line regardless put "[ERROR] This project is configured to use
    /// 11.7.0 of pnpm…" in the version field and reported the harness as ready.
    #[test]
    fn a_launcher_that_fails_is_reported_as_failed_not_as_a_version() {
        let d = tempfile::tempdir().unwrap();
        let p = fake(
            d.path(),
            "pnpm-ish",
            "echo '[ERROR] This project is configured to use 11.7.0 of pnpm. Your current pnpm is v11.0.9'\nexit 1",
        );
        assert_eq!(
            probe_version(&p, &[], PATH, QUICK),
            Probe::Failed(
                "[ERROR] This project is configured to use 11.7.0 of pnpm. Your current pnpm is v11.0.9".into()
            )
        );
    }

    /// When it fails, whichever stream explains why is what the user needs.
    #[test]
    fn a_failure_prefers_the_stream_that_explains_itself() {
        let d = tempfile::tempdir().unwrap();
        let p = fake(d.path(), "err", "echo 'cannot find module' >&2\nexit 2");
        assert_eq!(
            probe_version(&p, &[], PATH, QUICK),
            Probe::Failed("cannot find module".into())
        );
    }

    /// The distinction that makes the warning worth showing: an environment
    /// failure repeats no matter what you ask it to do.
    #[test]
    fn recognizes_environment_failures_and_leaves_task_failures_alone() {
        for env in [
            "Failed to authenticate: OAuth session expired and could not be refreshed",
            "401 Unauthorized",
            "DEEPSEEK_API_KEY is not set",
            "rate limit exceeded, retry later",
            "You have exceeded your quota",
            "invalid credential",
        ] {
            assert!(is_environment_failure(env), "should be environmental: {env}");
        }
        for task in [
            "这篇手记里没有待答的问题",
            "the model refused the request",
            "turn failed: tool call errored",
            "spawn failed: No such file or directory",
        ] {
            assert!(!is_environment_failure(task), "should NOT be environmental: {task}");
        }
    }

    fn write_run(root: &Path, task: &str, id: &str, status: record::Status, result: &str) {
        write_run_as(root, task, id, status, result, MINE)
    }

    const MINE: &str = "notemd.deepseek-agent";
    const THEIRS: &str = "notemd.claude-agent";

    fn write_run_as(
        root: &Path,
        task: &str,
        id: &str,
        status: record::Status,
        result: &str,
        harness: &str,
    ) {
        record::write(
            &root.join(task),
            &record::RunRecord {
                run_id: id.into(),
                task: task.into(),
                trigger: "relay".into(),
                started_at: "s".into(),
                ended_at: "e".into(),
                status,
                exit_code: Some(1),
                num_turns: None,
                session_id: None,
                result: result.into(),
                stderr_tail: String::new(),
                artifacts: Vec::new(),
                harness: Some(harness.to_string()),
            },
        )
        .unwrap();
    }

    #[test]
    fn surfaces_an_expired_credential_from_the_newest_run() {
        let d = tempfile::tempdir().unwrap();
        write_run(
            d.path(),
            "idea-proof",
            "20260818T022534Z-a",
            record::Status::Error,
            "Failed to authenticate: OAuth session expired and could not be refreshed",
        );
        assert_eq!(
            recent_environment_warning(d.path(), MINE).as_deref(),
            Some("Failed to authenticate: OAuth session expired and could not be refreshed")
        );
    }

    /// Once a run succeeds, the stale warning must stop being shown — otherwise
    /// re-authenticating never visibly fixes anything.
    #[test]
    fn a_later_success_clears_the_warning() {
        let d = tempfile::tempdir().unwrap();
        write_run(d.path(), "t", "20260818T000001Z-a", record::Status::Error, "OAuth expired");
        write_run(d.path(), "t", "20260818T000002Z-b", record::Status::Success, "done");
        assert_eq!(recent_environment_warning(d.path(), MINE), None);
    }

    #[test]
    fn an_ordinary_task_failure_is_not_reported_as_an_environment_problem() {
        let d = tempfile::tempdir().unwrap();
        write_run(
            d.path(),
            "t",
            "20260818T000001Z-a",
            record::Status::Error,
            "the model refused the request",
        );
        assert_eq!(recent_environment_warning(d.path(), MINE), None);
    }

    #[test]
    fn a_vault_that_never_ran_anything_has_no_warning() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(recent_environment_warning(d.path(), MINE), None);
        assert_eq!(recent_environment_warning(&d.path().join("nope"), MINE), None);
    }

    /// Caught live: the DeepSeek window showed "OAuth session expired" — a
    /// CLAUDE failure — because both plugins share one runs root and the read
    /// was unfiltered. A harness must never claim the other's problem.
    #[test]
    fn the_other_harnesss_failure_is_not_claimed_as_ours() {
        let d = tempfile::tempdir().unwrap();
        write_run_as(
            d.path(),
            "idea-proof",
            "20260818T022534Z-a",
            record::Status::Error,
            "Failed to authenticate: OAuth session expired and could not be refreshed",
            THEIRS,
        );
        assert_eq!(recent_environment_warning(d.path(), MINE), None);
        assert!(recent_environment_warning(d.path(), THEIRS).is_some());
    }

    /// Ours must still surface when the other harness ran more recently.
    #[test]
    fn our_own_failure_survives_a_newer_run_from_the_other_harness() {
        let d = tempfile::tempdir().unwrap();
        write_run_as(d.path(), "t", "20260818T000001Z-a", record::Status::Error, "no API key", MINE);
        write_run_as(d.path(), "t", "20260818T000009Z-z", record::Status::Success, "done", THEIRS);
        assert_eq!(recent_environment_warning(d.path(), MINE).as_deref(), Some("no API key"));
    }

    /// A record from before the field existed has unknown provenance — which is
    /// not the same as "mine".
    #[test]
    fn a_record_without_provenance_is_not_claimed() {
        let d = tempfile::tempdir().unwrap();
        record::write(
            &d.path().join("t"),
            &record::RunRecord {
                run_id: "20260818T000001Z-a".into(),
                task: "t".into(),
                trigger: "relay".into(),
                started_at: "s".into(),
                ended_at: "e".into(),
                status: record::Status::Error,
                exit_code: Some(1),
                num_turns: None,
                session_id: None,
                result: "OAuth session expired".into(),
                stderr_tail: String::new(),
                artifacts: Vec::new(),
                harness: None,
            },
        )
        .unwrap();
        assert_eq!(recent_environment_warning(d.path(), MINE), None);
    }

    #[test]
    fn a_missing_harness_reports_what_to_do_about_it() {
        let s = HarnessStatus::missing("Claude Code", "install it with npm i -g …");
        assert!(!s.ok);
        assert!(s.hint.unwrap().contains("npm i -g"));
        assert_eq!(s.version, None);
    }
}
