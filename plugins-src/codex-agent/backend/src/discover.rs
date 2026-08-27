//! Locate Codex CLI and cheaply inspect its local authentication state.
//!
//! Discovery is shared with the other agent plugins: an explicit setting (or
//! `NOTEMD_CODEX_BIN`) wins, then the user's login shell, then well-known
//! install locations.  Authentication is deliberately probed with
//! `codex login status`, never with a model call, so opening the window cannot
//! consume quota.
use agent_run_core::{discover as core, harness};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

pub const BIN: &str = "codex";

pub fn candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/bin").join(BIN),
        PathBuf::from("/opt/homebrew/bin").join(BIN),
        PathBuf::from("/usr/local/bin").join(BIN),
        home.join(".npm-global/bin").join(BIN),
    ]
}

/// Injectable discovery core. `explicit` represents either a plugin setting
/// or the already-resolved environment override.
pub fn discover_with(
    explicit: Option<&str>,
    home: &Path,
    shell_lookup: impl Fn(&str) -> Option<PathBuf>,
    is_exec: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    core::discover_with(explicit, &candidates(home), || shell_lookup(BIN), &is_exec)
}

pub fn discover(explicit: Option<&str>) -> Option<PathBuf> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let explicit = explicit
        .map(str::to_string)
        .or_else(|| std::env::var("NOTEMD_CODEX_BIN").ok());
    discover_with(
        explicit.as_deref(),
        &home,
        |bin| core::probe(bin).0,
        core::is_executable,
    )
}

pub fn runtime_path() -> String {
    core::runtime_path(BIN)
}

/// Ask Codex itself for the effective model at `cwd`.
///
/// `config/read` applies Codex's user and trusted-project configuration layers;
/// `configRequirements/read` contributes managed new-thread defaults. When
/// neither pins a model, `model/list` exposes the catalog default. This local
/// app-server probe makes no model call and is bounded so a broken CLI cannot
/// wedge the note.md protocol loop.
pub fn effective_model(
    program: &Path,
    path: &str,
    cwd: &Path,
    timeout: std::time::Duration,
) -> Option<String> {
    let mut command = std::process::Command::new(program);
    command
        .args(["app-server", "--listen", "stdio://"])
        .current_dir(cwd)
        .env("PATH", path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn().ok()?;
    let pgid = child.id() as i32;

    let mut stdin = child.stdin.take().expect("piped app-server stdin");
    let stdout = child.stdout.take().expect("piped app-server stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = std::time::Instant::now() + timeout;
    let initialize = serde_json::json!({
        "id": 1,
        "method": "initialize",
        "params": { "clientInfo": { "name": "notemd-codex-agent", "version": "1" } }
    });
    if writeln!(stdin, "{initialize}")
        .and_then(|_| stdin.flush())
        .is_err()
        || !wait_for_initialize(&rx, deadline)
    {
        stop_probe(&mut child, pgid, reader);
        return None;
    }

    let requests = [
        serde_json::json!({ "method": "initialized", "params": {} }),
        serde_json::json!({
            "id": 2,
            "method": "config/read",
            "params": { "cwd": cwd.to_string_lossy(), "includeLayers": false }
        }),
        serde_json::json!({
            "id": 3,
            "method": "model/list",
            "params": { "includeHidden": true, "limit": 1000 }
        }),
        serde_json::json!({
            "id": 4,
            "method": "configRequirements/read",
            "params": {}
        }),
    ];
    if requests
        .iter()
        .try_for_each(|request| writeln!(stdin, "{request}"))
        .and_then(|_| stdin.flush())
        .is_err()
    {
        stop_probe(&mut child, pgid, reader);
        return None;
    }

    let mut config_seen = false;
    let mut requirements_seen = false;
    let mut managed = None;
    let mut configured = None;
    let mut catalog = None;
    loop {
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            break;
        };
        let Ok(line) = rx.recv_timeout(remaining) else {
            break;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        match value.get("id").and_then(|id| id.as_u64()) {
            Some(2) if value.get("result").is_some() => {
                config_seen = true;
                configured = value
                    .pointer("/result/config/model")
                    .and_then(|model| model.as_str())
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .map(str::to_string);
            }
            Some(3) => {
                catalog = value
                    .pointer("/result/data")
                    .and_then(|data| data.as_array())
                    .and_then(|models| {
                        models
                            .iter()
                            .find(|model| {
                                model.get("isDefault").and_then(|v| v.as_bool()) == Some(true)
                            })
                            .or_else(|| models.first())
                            .and_then(|model| model.get("model"))
                            .and_then(|model| model.as_str())
                            .map(str::to_string)
                    });
            }
            Some(4) => {
                requirements_seen = true;
                managed = value
                    .pointer("/result/requirements/models/newThread/model")
                    .and_then(|model| model.as_str())
                    .map(str::trim)
                    .filter(|model| !model.is_empty())
                    .map(str::to_string);
            }
            _ => {}
        }
        if managed.is_some()
            || (requirements_seen && configured.is_some())
            || (requirements_seen && config_seen && catalog.is_some())
        {
            break;
        }
    }

    stop_probe(&mut child, pgid, reader);
    if managed.is_some() {
        return managed;
    }
    if !requirements_seen || !config_seen {
        return None;
    }
    configured.or(catalog)
}

fn wait_for_initialize(
    rx: &std::sync::mpsc::Receiver<String>,
    deadline: std::time::Instant,
) -> bool {
    loop {
        let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
            return false;
        };
        let Ok(line) = rx.recv_timeout(remaining) else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if value.get("id").and_then(|id| id.as_u64()) == Some(1) {
            return value.get("result").is_some();
        }
    }
}

fn stop_probe(child: &mut std::process::Child, pgid: i32, reader: std::thread::JoinHandle<()>) {
    unsafe {
        libc::killpg(pgid, libc::SIGKILL);
    }
    let _ = child.wait();
    let _ = reader.join();
}

/// A task-local model is an intentional override. Otherwise follow the model
/// Codex resolves for this Vault exactly as a CLI launched there would.
pub fn resolve_model(
    task_model: Option<&str>,
    program: &Path,
    path: &str,
    vault: &Path,
    timeout: std::time::Duration,
) -> Option<String> {
    task_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
        .or_else(|| effective_model(program, path, vault, timeout))
}

pub const NOT_FOUND: &str =
    "codex executable not found — install Codex CLI, or point NOTEMD_CODEX_BIN at it";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthProbe {
    /// A stored login or an invocation-scoped credential is available.
    Authenticated(String),
    /// Codex answered truthfully that no stored login exists.
    NotLoggedIn,
    /// The command ran but failed for a reason other than being signed out.
    Failed(String),
    /// The status command could not be started or did not finish in time.
    Unavailable,
}

/// An invocation-scoped credential makes `codex exec` usable even when
/// `codex login status` reports no stored account. Never return the value.
pub fn environment_auth() -> Option<&'static str> {
    environment_auth_with(|name| std::env::var(name).ok())
}

fn environment_auth_with(mut get: impl FnMut(&str) -> Option<String>) -> Option<&'static str> {
    get("CODEX_API_KEY")
        .is_some_and(|v| !v.trim().is_empty())
        .then_some("CODEX_API_KEY")
}

/// Run the free, local auth-status command with a deadline. This only inspects
/// Codex's credential store; it does not validate the token over the network,
/// so real 401/rate-limit failures are still reported from the latest run via
/// `agent_run_core::harness::recent_environment_warning`.
pub fn probe_auth(program: &Path, path: &str, timeout: std::time::Duration) -> AuthProbe {
    probe_auth_with(program, path, timeout, environment_auth())
}

fn probe_auth_with(
    program: &Path,
    path: &str,
    timeout: std::time::Duration,
    environment_auth: Option<&str>,
) -> AuthProbe {
    if let Some(name) = environment_auth {
        return AuthProbe::Authenticated(format!("environment variable {name}"));
    }

    let mut cmd = std::process::Command::new(program);
    cmd.args(["login", "status"])
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(mut child) = cmd.spawn() else {
        return AuthProbe::Unavailable;
    };

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return AuthProbe::Unavailable;
            }
        }
    }

    let Ok(out) = child.wait_with_output() else {
        return AuthProbe::Unavailable;
    };
    // Codex intentionally prints login status to stderr. Keep stdout as a
    // compatibility fallback in case that changes in a later CLI.
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = harness::first_line(&stderr).or_else(|| harness::first_line(&stdout));
    if out.status.success() {
        return line
            .map(AuthProbe::Authenticated)
            .unwrap_or(AuthProbe::Unavailable);
    }
    match line {
        Some(s) if s.to_ascii_lowercase().contains("not logged in") => AuthProbe::NotLoggedIn,
        Some(s) => AuthProbe::Failed(s),
        None => AuthProbe::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const NONE: fn(&str) -> Option<PathBuf> = |_| None;
    const PATH: &str = "/usr/bin:/bin";
    const QUICK: std::time::Duration = std::time::Duration::from_secs(5);

    fn fake(dir: &Path, body: &str) -> PathBuf {
        let p = dir.join("codex");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn explicit_then_shell_then_candidates() {
        let got = discover_with(
            Some("/custom/codex"),
            Path::new("/home/u"),
            |_| Some(PathBuf::from("/shell/codex")),
            |_| true,
        )
        .unwrap();
        assert_eq!(got, PathBuf::from("/custom/codex"));

        let got = discover_with(
            Some("/gone/codex"),
            Path::new("/home/u"),
            |_| Some(PathBuf::from("/shell/codex")),
            |p| p != Path::new("/gone/codex"),
        )
        .unwrap();
        assert_eq!(got, PathBuf::from("/shell/codex"));

        let home = Path::new("/home/u");
        let want = home.join(".local/bin/codex");
        let w = want.clone();
        let got = discover_with(None, home, NONE, move |p| p == w).unwrap();
        assert_eq!(got, want);
    }

    #[test]
    fn every_candidate_is_an_absolute_codex_path() {
        for p in candidates(Path::new("/home/u")) {
            assert!(p.is_absolute(), "{p:?}");
            assert_eq!(p.file_name().unwrap(), BIN);
        }
    }

    #[test]
    fn returns_none_when_codex_is_missing() {
        assert_eq!(
            discover_with(None, Path::new("/home/u"), NONE, |_| false),
            None
        );
    }

    #[test]
    fn login_status_reads_codexs_stderr_contract() {
        let d = tempfile::tempdir().unwrap();
        let yes = fake(d.path(), "echo 'Logged in using ChatGPT' >&2; exit 0");
        assert_eq!(
            probe_auth_with(&yes, PATH, QUICK, None),
            AuthProbe::Authenticated("Logged in using ChatGPT".into())
        );

        let no = fake(d.path(), "echo 'Not logged in' >&2; exit 1");
        assert_eq!(
            probe_auth_with(&no, PATH, QUICK, None),
            AuthProbe::NotLoggedIn
        );
    }

    #[test]
    fn invocation_scoped_auth_avoids_a_false_signed_out_result() {
        let d = tempfile::tempdir().unwrap();
        let must_not_run = fake(d.path(), "exit 99");
        assert_eq!(
            probe_auth_with(&must_not_run, PATH, QUICK, Some("CODEX_API_KEY")),
            AuthProbe::Authenticated("environment variable CODEX_API_KEY".into())
        );
        assert_eq!(
            environment_auth_with(|name| (name == "CODEX_ACCESS_TOKEN").then(|| "token".into())),
            None
        );
        assert_eq!(
            environment_auth_with(|name| (name == "CODEX_API_KEY").then(|| "key".into())),
            Some("CODEX_API_KEY")
        );
    }

    #[test]
    fn effective_model_prefers_the_vaults_resolved_config() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(
            d.path(),
            "echo '{\"id\":1,\"result\":{}}'\n\
             echo '{\"id\":3,\"result\":{\"data\":[{\"model\":\"gpt-catalog\",\"isDefault\":true}]}}'\n\
             echo '{\"id\":2,\"result\":{\"config\":{\"model\":\"gpt-vault\"}}}'\n\
             echo '{\"id\":4,\"result\":{\"requirements\":null}}'\n\
             while read _; do :; done",
        );
        assert_eq!(
            effective_model(&bin, PATH, d.path(), QUICK).as_deref(),
            Some("gpt-vault")
        );
    }

    #[test]
    fn effective_model_uses_the_codex_catalog_default_when_config_is_unset() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(
            d.path(),
            "echo '{\"id\":1,\"result\":{}}'\n\
             echo '{\"id\":2,\"result\":{\"config\":{\"model\":null}}}'\n\
             echo '{\"id\":3,\"result\":{\"data\":[{\"model\":\"gpt-other\",\"isDefault\":false},{\"model\":\"gpt-default\",\"isDefault\":true}]}}'\n\
             echo '{\"id\":4,\"result\":{\"requirements\":null}}'\n\
             while read _; do :; done",
        );
        assert_eq!(
            effective_model(&bin, PATH, d.path(), QUICK).as_deref(),
            Some("gpt-default")
        );
    }

    #[test]
    fn managed_new_thread_model_wins_over_vault_and_catalog_defaults() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(
            d.path(),
            "echo '{\"id\":1,\"result\":{}}'\n\
             echo '{\"id\":2,\"result\":{\"config\":{\"model\":\"gpt-vault\"}}}'\n\
             echo '{\"id\":3,\"result\":{\"data\":[{\"model\":\"gpt-catalog\",\"isDefault\":true}]}}'\n\
             echo '{\"id\":4,\"result\":{\"requirements\":{\"models\":{\"newThread\":{\"model\":\"gpt-managed\"}}}}}'\n\
             while read _; do :; done",
        );
        assert_eq!(
            effective_model(&bin, PATH, d.path(), QUICK).as_deref(),
            Some("gpt-managed")
        );
    }

    #[test]
    fn catalog_falls_back_to_its_first_model_when_none_is_marked_default() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake(
            d.path(),
            "echo '{\"id\":1,\"result\":{}}'\n\
             echo '{\"id\":4,\"result\":{\"requirements\":null}}'\n\
             echo '{\"id\":2,\"result\":{\"config\":{\"model\":null}}}'\n\
             echo '{\"id\":3,\"result\":{\"data\":[{\"model\":\"gpt-first\",\"isDefault\":false},{\"model\":\"gpt-second\",\"isDefault\":false}]}}'\n\
             while read _; do :; done",
        );
        assert_eq!(
            effective_model(&bin, PATH, d.path(), QUICK).as_deref(),
            Some("gpt-first")
        );
    }

    #[test]
    fn a_task_model_wins_without_starting_a_probe() {
        assert_eq!(
            resolve_model(
                Some(" gpt-task "),
                Path::new("/no/such/codex"),
                PATH,
                Path::new("/no/such/vault"),
                QUICK,
            )
            .as_deref(),
            Some("gpt-task")
        );
    }

    #[test]
    fn a_broken_or_hung_status_probe_is_not_authenticated() {
        assert_eq!(
            probe_auth_with(Path::new("/no/such/codex"), PATH, QUICK, None),
            AuthProbe::Unavailable
        );

        let d = tempfile::tempdir().unwrap();
        let broken = fake(d.path(), "echo 'broken config' >&2; exit 2");
        assert_eq!(
            probe_auth_with(&broken, PATH, QUICK, None),
            AuthProbe::Failed("broken config".into())
        );
    }

    #[test]
    fn hint_names_the_supported_override() {
        assert!(NOT_FOUND.contains("NOTEMD_CODEX_BIN"));
        assert!(NOT_FOUND.contains("Codex CLI"));
    }
}
