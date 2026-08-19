//! Integration tests for plugin_runtime::process (plugin-runtime-v2 Task 5):
//! spawn/handshake, NDJSON RPC round-trip, per-request timeout, fast failure
//! on process death, stderr logging, graceful shutdown — and for
//! plugin_runtime::lifecycle (Task 6): crash backoff circuit breaker, idle
//! shutdown + lazy re-activation, deactivate-is-not-a-crash, startup
//! activation — and for plugin_runtime::host_api (Task 7): capability-gated
//! `host.toast` dispatched through `make_sink` wired into a real process.

use notemd_lib::plugin_runtime::host_api::make_sink;
use notemd_lib::plugin_runtime::lifecycle::{
    startup_activation, PhaseKind, PluginLifecycle, SpawnCtx, Trigger,
};
use notemd_lib::plugin_runtime::process::{initialize_and_activate, HostSink, PluginProcess};
use plugin_protocol as proto;
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn fixture_script(name: &str) -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir).join("tests/fixtures/v2").join(name)
}

/// Path to a fixture, ready to hand to `PluginProcess::spawn`.
///
/// The fixtures stay POSIX `sh` scripts — one source, run natively on macOS and
/// Linux, no per-platform copies to drift apart.
#[cfg(not(windows))]
fn fixture(name: &str) -> PathBuf {
    fixture_script(name)
}

/// Windows cannot `CreateProcess` a `.sh`, so the fixture is reached through a
/// generated `.cmd` shim that hands the very same script to Git for Windows'
/// POSIX shell. The scripts themselves are untouched.
///
/// Git for Windows is not a new dependency: vault sync already shells out to
/// `git`, and the Windows story is "install Git for Windows" rather than
/// bundling MinGit (docs/2026-08-08-pc-port-refactor-plan.md §5.2/§11), so its
/// `sh.exe` is present on any machine that can run the app at all. It is also
/// what GitHub Actions runs for `shell: bash` on windows-latest.
#[cfg(windows)]
fn fixture(name: &str) -> PathBuf {
    let script = fixture_script(name);
    let shim = shim_dir().join(format!("{name}.cmd"));
    if !shim.exists() {
        let sh = posix_sh().unwrap_or_else(|| {
            panic!(
                "no POSIX shell found for the .sh fixtures. Install Git for Windows, \
                 or point NOTEMD_TEST_SH at an sh.exe."
            )
        });
        // CRLF: cmd.exe is the one consumer and it is the least surprising form.
        let body = format!("@echo off\r\n\"{}\" \"{}\"\r\n", sh.display(), script.display());
        std::fs::write(&shim, body).expect("write fixture shim");
    }
    shim
}

/// Process-lifetime scratch dir for the generated shims.
#[cfg(windows)]
fn shim_dir() -> PathBuf {
    use std::sync::OnceLock;
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let d = std::env::temp_dir().join(format!("notemd-fixture-shims-{}", std::process::id()));
        std::fs::create_dir_all(&d).expect("create shim dir");
        d
    })
    .clone()
}

/// Locate a POSIX shell: `$NOTEMD_TEST_SH`, else the one shipped beside the
/// `git` already on PATH (`<git root>/bin/sh.exe`, `<git root>/usr/bin/sh.exe`).
#[cfg(windows)]
fn posix_sh() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("NOTEMD_TEST_SH") {
        let p = PathBuf::from(explicit);
        if p.is_file() {
            return Some(p);
        }
    }
    // git.exe lives in `<root>/cmd/` or `<root>/bin/`; the shell is under
    // `<root>/bin/` or `<root>/usr/bin/`.
    let from_path = std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|d| d.join("git.exe"))
                .find(|c| c.is_file())
        })
        .and_then(|git| Some(git.parent()?.parent()?.to_path_buf()));

    // Well-known install roots as a fallback. Needed because a test run started
    // from Git Bash inherits an MSYS-form PATH (`/c/Program Files/Git/cmd`),
    // which means nothing to a native process — the same trap that makes
    // `pnpm install` fail from Git Bash. Looking here as well keeps the suite
    // runnable from either shell.
    let well_known = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .flat_map(|base| {
            let base = PathBuf::from(base);
            [base.join("Git"), base.join("Programs/Git")]
        });

    from_path
        .into_iter()
        .chain(well_known)
        .flat_map(|root| [root.join("bin/sh.exe"), root.join("usr/bin/sh.exe")])
        .find(|p| p.is_file())
}

/// HostSink that records every plugin→host request/notification it sees.
fn recording_sink() -> (HostSink, Arc<Mutex<Vec<proto::RpcRequest>>>) {
    let seen: Arc<Mutex<Vec<proto::RpcRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_in_sink = seen.clone();
    let sink: HostSink = Arc::new(move |req| {
        seen_in_sink.lock().unwrap().push(req);
        None
    });
    (sink, seen)
}

fn init_params(root: &std::path::Path) -> proto::InitializeParams {
    proto::InitializeParams {
        protocol_version: proto::PROTOCOL_VERSION,
        host_version: "6.716.7".into(),
        locale: "en".into(),
        theme: "light".into(),
        plugin_root: root.display().to_string(),
        data_dir: root.display().to_string(),
    }
}

/// Poll until `pred` holds, up to ~2s. `host.*` messages are handed to the sink
/// on its own queue thread (so a slow sink can never stall response routing —
/// see `PluginProcess::spawn`), which means "the response came back" no longer
/// implies "the sink already ran".
async fn wait_until(mut pred: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if pred() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// Poll `has_exited` until it reports an exit code (reaping can lag the pipe
/// EOF slightly).
async fn wait_exit_code(proc: &PluginProcess) -> Option<i32> {
    for _ in 0..50 {
        if let Some(code) = proc.has_exited().await {
            return Some(code);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
}

// ── ① ok.sh: full round-trip + host.toast notification + graceful shutdown ──

#[tokio::test]
async fn ok_round_trip_toast_and_graceful_shutdown() {
    let log_dir = tempfile::tempdir().unwrap();
    let (sink, seen) = recording_sink();
    let proc = PluginProcess::spawn(&fixture("ok.sh"), "test.ok", log_dir.path(), 5, sink)
        .await
        .unwrap();
    initialize_and_activate(&proc, &init_params(log_dir.path()), "onStartupFinished")
        .await
        .unwrap();

    let out = proc
        .request("command.execute", json!({ "command": "noop", "context": {} }))
        .await
        .unwrap();
    assert_eq!(out, json!({ "echo": true }));

    // ok.sh emits the toast notification *before* the execute response on the
    // same pipe; the reader hands it to the sink queue, which runs off-loop.
    assert!(
        wait_until(|| !seen.lock().unwrap().is_empty()).await,
        "the toast notification never reached the sink"
    );
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "expected exactly one host.* message");
        assert_eq!(seen[0].method, "host.toast");
        assert_eq!(seen[0].id, None, "toast is a notification");
        assert_eq!(seen[0].params, json!({ "level": "success", "message": "hi" }));
    }

    assert!(proc.shutdown().await, "expected graceful shutdown");
    assert_eq!(proc.has_exited().await, Some(0));
}

// ── ①b echo-env.sh: spawned plugin does NOT inherit host secrets ──
//
// The host clears the child environment and re-adds only a safe allowlist
// before spawning, so a secret-bearing var set in the parent process must not
// leak into the plugin. The fixture reports whether SECRET_TEST_VAR reached it.
#[tokio::test]
async fn spawned_plugin_does_not_inherit_host_secret_env() {
    // SAFETY: single-threaded test setup before any spawn reads the env.
    std::env::set_var("SECRET_TEST_VAR", "leaked-api-key");
    let log_dir = tempfile::tempdir().unwrap();
    let (sink, _seen) = recording_sink();
    let proc = PluginProcess::spawn(&fixture("echo-env.sh"), "test.env", log_dir.path(), 5, sink)
        .await
        .unwrap();
    initialize_and_activate(&proc, &init_params(log_dir.path()), "onStartupFinished")
        .await
        .unwrap();
    let out = proc
        .request("command.execute", json!({ "command": "noop", "context": {} }))
        .await
        .unwrap();
    std::env::remove_var("SECRET_TEST_VAR");
    assert_eq!(
        out,
        json!({ "secret_present": false }),
        "host must clear the environment: the plugin should NOT see SECRET_TEST_VAR"
    );
    proc.shutdown().await;
}

// ── ② slow.sh: per-request timeout fails the request only (spec §12) ──

#[tokio::test]
async fn slow_execute_times_out_without_killing_process() {
    let log_dir = tempfile::tempdir().unwrap();
    let (sink, _seen) = recording_sink();
    let proc = PluginProcess::spawn(&fixture("slow.sh"), "test.slow", log_dir.path(), 1, sink)
        .await
        .unwrap();
    // The 1s per-request timeout under test also governs the handshake, which
    // can flake on a cold/loaded machine. Retrying is safe: the fixture drops
    // nothing, and a late response to a timed-out id is discarded by the host.
    let mut handshake = Err(String::new());
    for _ in 0..3 {
        handshake = initialize_and_activate(&proc, &init_params(log_dir.path()), "onCommand:x").await;
        if handshake.is_ok() {
            break;
        }
    }
    handshake.expect("handshake should succeed within retries");

    let err = proc.request("command.execute", json!({})).await.unwrap_err();
    assert!(err.contains("timeout:1"), "got: {err}");

    // Process must survive a per-request timeout…
    assert_eq!(proc.has_exited().await, None, "process should still be alive");
    // …and still serve later requests: $deactivate succeeds → graceful.
    assert!(proc.shutdown().await, "expected graceful shutdown after timeout");
}

// ── ②b chatty.sh: the timeout is silence, not elapsed time ──
//
// v6.804.5 shipped the opposite: a fixed relay ceiling failed an AI read that
// was streaming happily and went on to succeed. A plugin that keeps producing
// output is working, and must not be declared timed out for taking long.

#[tokio::test]
async fn a_chatty_plugin_is_not_timed_out_while_it_keeps_talking() {
    let log_dir = tempfile::tempdir().unwrap();
    let (sink, seen) = recording_sink();
    // 1s silence budget vs ~2s of chatter: an elapsed-time timeout fails at 1s.
    let proc = PluginProcess::spawn(&fixture("chatty.sh"), "test.chatty", log_dir.path(), 1, sink)
        .await
        .unwrap();
    initialize_and_activate(&proc, &init_params(log_dir.path()), "onCommand:x")
        .await
        .unwrap();

    let started = Instant::now();
    let err = proc.request("command.execute", json!({})).await.unwrap_err();
    let waited = started.elapsed();

    // It DID eventually time out — the fixture never answers, and silence after
    // the chatter stops is exactly what a timeout is for.
    assert!(err.contains("timeout:1"), "got: {err}");
    // …but only after the chatter ran out, i.e. the clock restarted on output.
    assert!(
        waited >= Duration::from_secs(2),
        "gave up after {waited:?}: output must restart the silence clock"
    );
    assert!(
        seen.lock().unwrap().len() >= 5,
        "expected the chatter to have been routed to the sink meanwhile"
    );
    assert_eq!(proc.has_exited().await, None, "process should still be alive");
}

// ── ②c a blocking host_sink must not stall response routing ──
//
// The root cause behind the v6.804.5 report: `host.agent.*` relays through the
// sink and blocks until the OTHER plugin answers. With the sink running inline
// on the read loop, that stalls this plugin's own responses — the plugin has
// long since replied and the caller still times out.

#[tokio::test]
async fn a_blocking_host_sink_does_not_delay_the_plugins_response() {
    let log_dir = tempfile::tempdir().unwrap();
    // A sink that takes 2s per message, like a relay waiting on another plugin.
    let sink: HostSink = Arc::new(|_req| {
        std::thread::sleep(Duration::from_secs(2));
        None
    });
    let proc = PluginProcess::spawn(&fixture("ok.sh"), "test.blocking", log_dir.path(), 30, sink)
        .await
        .unwrap();
    initialize_and_activate(&proc, &init_params(log_dir.path()), "onStartupFinished")
        .await
        .unwrap();

    // ok.sh emits a host.toast notification and THEN the execute response.
    let started = Instant::now();
    let out = proc
        .request("command.execute", json!({ "command": "noop", "context": {} }))
        .await
        .unwrap();
    assert_eq!(out, json!({ "echo": true }));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "the response waited out the blocking sink ({:?})",
        started.elapsed()
    );
}

// ── ③ crash-activate.sh: activation failure surfaces + exit code observed ──

#[tokio::test]
async fn crash_on_activate_reports_error_and_exit_code() {
    let log_dir = tempfile::tempdir().unwrap();
    let (sink, _seen) = recording_sink();
    let proc = PluginProcess::spawn(
        &fixture("crash-activate.sh"),
        "test.crash",
        log_dir.path(),
        5,
        sink,
    )
    .await
    .unwrap();

    let err = initialize_and_activate(&proc, &init_params(log_dir.path()), "onCommand:x")
        .await
        .unwrap_err();
    assert!(err.starts_with("$activate:"), "got: {err}");
    assert_eq!(wait_exit_code(&proc).await, Some(1));
}

// ── ④ die-mid-request.sh: pins the reader-exit pending drain + stderr log ──

#[tokio::test]
async fn process_death_mid_request_fails_pending_fast_and_logs_stderr() {
    let log_dir = tempfile::tempdir().unwrap();
    let (sink, _seen) = recording_sink();
    // 30s request timeout: only the drain-on-reader-exit behavior makes the
    // in-flight request return promptly instead of hanging out the timeout.
    let proc = PluginProcess::spawn(
        &fixture("die-mid-request.sh"),
        "test.die",
        log_dir.path(),
        30,
        sink,
    )
    .await
    .unwrap();
    initialize_and_activate(&proc, &init_params(log_dir.path()), "onCommand:x")
        .await
        .unwrap();

    let t0 = Instant::now();
    let err = proc.request("command.execute", json!({})).await.unwrap_err();
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "request should fail fast, took {:?}",
        t0.elapsed()
    );
    assert!(err.contains("channel closed"), "got: {err}");
    assert_eq!(wait_exit_code(&proc).await, Some(0));

    // The fixture's stderr line landed in <log_dir>/<plugin_id>.log with the
    // "stderr" level tag (poll: the capture task writes asynchronously).
    let log_path = log_dir.path().join("test.die.log");
    let mut content = String::new();
    for _ in 0..50 {
        content = std::fs::read_to_string(&log_path).unwrap_or_default();
        if !content.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        content.contains("[stderr] dying mid-request"),
        "log content: {content:?}"
    );
}

// ── shutdown on an already-dead process: false, and promptly ──

#[tokio::test]
async fn shutdown_after_process_death_is_not_graceful_and_returns_promptly() {
    let log_dir = tempfile::tempdir().unwrap();
    let (sink, _seen) = recording_sink();
    let proc = PluginProcess::spawn(
        &fixture("die-mid-request.sh"),
        "test.die2",
        log_dir.path(),
        5,
        sink,
    )
    .await
    .unwrap();
    initialize_and_activate(&proc, &init_params(log_dir.path()), "onCommand:x")
        .await
        .unwrap();
    let _ = proc.request("command.execute", json!({})).await; // kills the fixture
    assert_eq!(wait_exit_code(&proc).await, Some(0));

    let t0 = Instant::now();
    let graceful = proc.shutdown().await;
    assert!(!graceful, "dead process must not report a graceful shutdown");
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "shutdown of a dead process should not hang, took {:?}",
        t0.elapsed()
    );
}

// ═══ Task 6: lifecycle state machine ═══

fn v2_manifest(id: &str, events: &[&str], idle_shutdown_seconds: Option<u64>) -> proto::ManifestV2 {
    let mut m = json!({
        "manifest_version": 2,
        "id": id,
        "name": "Lifecycle Fixture",
        "version": "1.0.0",
        "kind": "native",
        "engines": { "notemd": ">=0.0.0" },
        "activation": { "events": events },
        "capabilities": []
    });
    if let Some(n) = idle_shutdown_seconds {
        m["idle_shutdown_seconds"] = json!(n);
    }
    serde_json::from_value(m).unwrap()
}

/// Lifecycle over a fixture script: fast test-friendly poll intervals are set
/// by each test on the returned (not-yet-Arc'd) value.
fn make_lifecycle(
    fixture_name: &str,
    id: &str,
    events: &[&str],
    idle_shutdown_seconds: Option<u64>,
    dir: &std::path::Path,
) -> PluginLifecycle {
    let (sink, _seen) = recording_sink();
    let ctx = SpawnCtx {
        binary: fixture(fixture_name),
        log_dir: dir.to_path_buf(),
        host_sink: sink,
        host_version: "6.716.7".into(),
        locale: "en".into(),
        app_data: dir.to_path_buf(),
    };
    PluginLifecycle::new(
        v2_manifest(id, events, idle_shutdown_seconds),
        dir.to_path_buf(),
        ctx,
    )
}

async fn wait_for_phase(lc: &PluginLifecycle, want: impl Fn(&PhaseKind) -> bool) -> PhaseKind {
    let mut kind = lc.phase_kind().await;
    for _ in 0..100 {
        // generous: up to 10s
        if want(&kind) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        kind = lc.phase_kind().await;
    }
    kind
}

// ── ② crash-loop.sh: backoff restarts converge to Disabled("crash-loop") ──

#[tokio::test]
async fn crash_loop_restarts_then_trips_breaker_to_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let mut lc = make_lifecycle("crash-loop.sh", "test.crashloop", &["*"], None, dir.path());
    lc.backoff_secs = vec![0, 0, 0];
    lc.crash_poll = Duration::from_millis(100);
    let lc = Arc::new(lc);

    // Activation itself SUCCEEDS (the fixture answers the full handshake
    // before dying) — the breaker is a supervision concern, not a handshake one.
    lc.ensure_active(&Trigger::Startup).await.expect("first activation succeeds");

    // Crash #1 → restart, crash #2 → restart, crash #3 → Disabled.
    let kind = wait_for_phase(&lc, |k| matches!(k, PhaseKind::Disabled(_))).await;
    assert_eq!(kind, PhaseKind::Disabled("crash-loop".into()));
    assert_eq!(
        lc.crash_times.lock().unwrap().len(),
        3,
        "exactly 3 crashes recorded in the window"
    );

    // Disabled rejects further activation with the reason (spec §4.2).
    let err = match lc.ensure_active(&Trigger::Startup).await {
        Ok(_) => panic!("ensure_active must fail once disabled"),
        Err(e) => e,
    };
    assert!(err.contains("crash-loop"), "got: {err}");

    // The breaker trip landed in the plugin log.
    let log = std::fs::read_to_string(dir.path().join("test.crashloop.log")).unwrap_or_default();
    assert!(log.contains("[crash-loop]"), "log content: {log:?}");
}

// ── ③ ok.sh + idle_shutdown_seconds=1: idle reap, then lazy re-activation ──

#[tokio::test]
async fn idle_shutdown_deactivates_then_next_trigger_reactivates() {
    let dir = tempfile::tempdir().unwrap();
    let mut lc = make_lifecycle("ok.sh", "test.idle", &["*"], Some(1), dir.path());
    lc.idle_poll = Duration::from_millis(200);
    let lc = Arc::new(lc);

    lc.ensure_active(&Trigger::Startup).await.unwrap();
    assert_eq!(lc.phase_kind().await, PhaseKind::Active);

    // idle_shutdown_seconds(1s) + poll(200ms) with generous margin.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        lc.phase_kind().await,
        PhaseKind::Inactive,
        "idle watcher should have deactivated"
    );
    assert!(
        lc.crash_times.lock().unwrap().is_empty(),
        "idle shutdown must not be recorded as a crash"
    );

    // Lazy re-activation on the next trigger; execute round-trips.
    lc.ensure_active(&Trigger::Command("noop".into())).await.unwrap();
    assert_eq!(lc.phase_kind().await, PhaseKind::Active);
    let out = lc
        .execute(proto::ExecuteCommandParams { command: "noop".into(), context: json!({}) })
        .await
        .unwrap();
    assert_eq!(out, json!({ "echo": true }));
    lc.deactivate().await;
}

// ── ④ deactivate is not a crash ──

#[tokio::test]
async fn deactivate_is_not_recorded_as_a_crash() {
    let dir = tempfile::tempdir().unwrap();
    let mut lc = make_lifecycle("ok.sh", "test.deact", &["*"], None, dir.path());
    lc.crash_poll = Duration::from_millis(100);
    let lc = Arc::new(lc);

    lc.ensure_active(&Trigger::Startup).await.unwrap();
    lc.deactivate().await;
    assert_eq!(lc.phase_kind().await, PhaseKind::Inactive);

    // Give the crash watcher several ticks to (wrongly) react.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(
        lc.phase_kind().await,
        PhaseKind::Inactive,
        "phase must stay Inactive, not Disabled"
    );
    assert!(
        lc.crash_times.lock().unwrap().is_empty(),
        "deactivate must not be recorded as a crash"
    );
}

// ── startup_activation: only Startup-matching plugins get activated ──

#[tokio::test]
async fn startup_activation_activates_only_matching_plugins() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let on_startup = Arc::new(make_lifecycle(
        "ok.sh",
        "test.startup",
        &["onStartupFinished"],
        None,
        dir_a.path(),
    ));
    let on_command = Arc::new(make_lifecycle(
        "ok.sh",
        "test.oncommand",
        &["onCommand:export"],
        None,
        dir_b.path(),
    ));

    startup_activation(vec![on_startup.clone(), on_command.clone()]);

    let kind = wait_for_phase(&on_startup, |k| matches!(k, PhaseKind::Active)).await;
    assert_eq!(kind, PhaseKind::Active, "onStartupFinished plugin should activate");
    assert_eq!(
        on_command.phase_kind().await,
        PhaseKind::Inactive,
        "onCommand-only plugin must stay inactive"
    );
    on_startup.deactivate().await;
}

// ═══ Task 7: host_api — make_sink wired through a real process ═══

/// Build a `ToastEmitter` that records every payload it sees.
fn recording_toast_emitter()
-> (notemd_lib::plugin_runtime::host_api::ToastEmitter, Arc<Mutex<Vec<serde_json::Value>>>)
{
    let seen: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_in = seen.clone();
    let emitter = Arc::new(move |v: serde_json::Value| {
        seen_in.lock().unwrap().push(v);
    });
    (emitter, seen)
}

// ── Task 7 integration: make_sink(capability=["toast"]) + ok.sh ──────────────
//
// Wires a real `make_sink` (with recording emitter, capability ["toast"])
// through `PluginProcess::spawn` running ok.sh.  After a `command.execute`
// the sink should have received a toast notification whose payload contains
// `plugin_id`, `level`, and `message` fields.

#[tokio::test]
async fn host_api_make_sink_dispatches_toast_through_real_process() {
    let log_dir = tempfile::tempdir().unwrap();
    let (emitter, emitted) = recording_toast_emitter();

    // Build a real capability-gated sink for plugin id "test.ok" with "toast" cap.
    // 子项目②b added a `UiPoster` arg (host.ui.post); this test doesn't exercise
    // it, so pass a no-op poster.
    let ui_poster: notemd_lib::plugin_runtime::host_api::UiPoster =
        Arc::new(|_window_id: &str, _payload: &serde_json::Value| {});
    let sink: HostSink = make_sink(
        "test.ok".into(),
        vec!["toast".into()],
        log_dir.path().to_path_buf(),
        emitter,
        ui_poster,
        None, // no host.vault.* HostServices needed for this toast round-trip
    );

    let proc = PluginProcess::spawn(&fixture("ok.sh"), "test.ok", log_dir.path(), 5, sink)
        .await
        .unwrap();
    initialize_and_activate(&proc, &init_params(log_dir.path()), "onStartupFinished")
        .await
        .unwrap();

    // ok.sh emits a host.toast notification before its execute response.
    let out = proc
        .request("command.execute", json!({ "command": "noop", "context": {} }))
        .await
        .unwrap();
    assert_eq!(out, json!({ "echo": true }));

    // The make_sink should have fired the emitter with the toast payload (on the
    // sink's own queue thread, so poll rather than assume it already ran).
    assert!(
        wait_until(|| !emitted.lock().unwrap().is_empty()).await,
        "the toast never reached the emitter"
    );
    let payloads = emitted.lock().unwrap();
    assert_eq!(payloads.len(), 1, "expected exactly one emitted toast");
    assert_eq!(payloads[0]["plugin_id"], "test.ok");
    assert_eq!(payloads[0]["level"], "success");
    assert_eq!(payloads[0]["message"], "hi");

    assert!(proc.shutdown().await, "expected graceful shutdown");
}

// ═══ 子项目②b Task 3: streaming fixture — bidirectional window channel ═══
//
// `stream.sh` is a real plugin process exercising BOTH directions of the v2
// window channel (子项目②b) end-to-end through `PluginProcess` + `make_sink`:
//   1. UI → process (`ui.request`): the host sends
//      `ui.request {method:"echo", params}` → the fixture's on_ui_request echoes
//      the inner params back as the result (and `ping` → `"pong"`).
//   2. process → UI (`host.ui.post` streaming push): after `$activate`, the
//      fixture streams three `host.ui.post` notifications {seq:1,2,3} on window
//      "main" (~50ms apart) followed by a final {done:true}. `make_sink`'s
//      capability-gated `host.ui.post` branch routes each to the injected
//      `UiPoster`, which records `(window_id, payload)` — standing in for
//      `windows::push_to_window` without a Tauri runtime.
//
// This is the integration counterpart to the host_api unit coverage of
// `host.ui.post` (capability gate, poster invocation) and the SDK duplex unit
// coverage of `on_ui_request` / `ui_post`: here the two meet across a real pipe.

/// Build a `UiPoster` that records every `(window_id, payload)` it receives.
#[allow(clippy::type_complexity)]
fn recording_ui_poster() -> (
    notemd_lib::plugin_runtime::host_api::UiPoster,
    Arc<Mutex<Vec<(String, serde_json::Value)>>>,
) {
    let seen: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_in = seen.clone();
    let poster = Arc::new(move |window_id: &str, payload: &serde_json::Value| {
        seen_in.lock().unwrap().push((window_id.to_string(), payload.clone()));
    });
    (poster, seen)
}

#[tokio::test]
async fn stream_fixture_ui_request_round_trip_and_host_ui_post_push() {
    let log_dir = tempfile::tempdir().unwrap();
    let (emitter, _emitted) = recording_toast_emitter();
    let (poster, posted) = recording_ui_poster();

    // A capability-gated sink holding both `ui` (for host.ui.post) and `toast`,
    // wired to a recording poster (host.ui.post → posted) for "test.stream".
    let sink: HostSink = make_sink(
        "test.stream".into(),
        vec!["ui".into(), "toast".into()],
        log_dir.path().to_path_buf(),
        emitter,
        poster,
        None, // no host.vault.* HostServices needed for this streaming round-trip
    );

    let proc = PluginProcess::spawn(&fixture("stream.sh"), "test.stream", log_dir.path(), 5, sink)
        .await
        .unwrap();
    // $activate kicks off the background streamer inside the fixture.
    initialize_and_activate(&proc, &init_params(log_dir.path()), "onCommand:open")
        .await
        .unwrap();

    // ── Direction 1: UI → process (ui.request echo round-trip) ─────────────
    // The host forwards a window RPC as `ui.request {method, params}`; the
    // fixture's on_ui_request echoes the INNER params back as the result.
    let echoed = proc
        .request("ui.request", json!({ "method": "echo", "params": { "x": 1 } }))
        .await
        .unwrap();
    assert_eq!(echoed, json!({ "x": 1 }), "ui.request echo must round-trip params");

    // `ping` → "pong" proves method dispatch (not a blind echo).
    let pong = proc
        .request("ui.request", json!({ "method": "ping", "params": {} }))
        .await
        .unwrap();
    assert_eq!(pong, json!("pong"), "ui.request ping must dispatch to pong");

    // ── Direction 2: process → UI (host.ui.post streaming push) ────────────
    // Poll until the recording poster has captured the 3 seq frames (streamed
    // ~50ms apart from a background subshell; give a generous margin).
    let seqs: Vec<i64> = {
        let mut got = Vec::new();
        for _ in 0..100 {
            {
                let posted = posted.lock().unwrap();
                got = posted
                    .iter()
                    .filter(|(win, _)| win == "main")
                    .filter_map(|(_, payload)| payload.get("seq").and_then(|s| s.as_i64()))
                    .collect();
            }
            if got.len() >= 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        got
    };
    assert!(
        seqs.len() >= 3,
        "expected ≥3 host.ui.post seq frames, got {seqs:?} (all posts: {:?})",
        *posted.lock().unwrap()
    );
    assert_eq!(&seqs[..3], &[1, 2, 3], "seq frames must arrive in order 1,2,3");

    // Every recorded push targeted the plugin's "main" window.
    for (win, _) in posted.lock().unwrap().iter() {
        assert_eq!(win, "main", "host.ui.post must target window 'main'");
    }

    assert!(proc.shutdown().await, "expected graceful shutdown");
}
