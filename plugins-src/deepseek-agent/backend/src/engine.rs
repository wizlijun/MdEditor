//! The run engine: start an ACP server, walk the handshake, pump its
//! notifications into events, land a record. The window path and the detached
//! runner share it — the only difference is who holds the child process.
//!
//! The scaffold around this (lock, precheck, artifacts, OKF stamping, the
//! record) is `agent_run_core::scaffold`, shared with claude-agent. What is
//! genuinely ours is the middle: a bidirectional NDJSON JSON-RPC dialogue rather
//! than a one-way stream of lines.
//!
//! ```text
//! spawn dsh --profile notemd --patch <vault overlay>   cwd = the task dir
//!   → initialize                assert the protocol version (§acp::check_initialize)
//!   → session/new               cwd = the task dir; mcpServers = [] (non-empty is rejected)
//!   → session/prompt            the composed three-part prompt
//!   ← session/update*           committed assistant text → Event::Text
//!   ← session/request_permission → answered from policy.json → Event::Permission
//!   ← stopReason                → RunResult → record → terminate
//! ```
use crate::{acp, composition, policy};
use agent_run_core::event::{Event, Step};
use agent_run_core::record;
use agent_run_core::scaffold::{self, Blocked, ProgressTracker, RunMeta};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

pub struct RunSpec {
    pub meta: RunMeta,
    /// How to start the ACP server.
    pub launcher: crate::discover::Launcher,
    /// The composition the server boots from.
    pub config: PathBuf,
    /// `PATH` for the child. `None` asks the login shell — which is what
    /// production wants, and what a test wants to stay out of.
    pub env_path: Option<String>,
    /// Where the harness may write its session logs.
    pub sessions_dir: PathBuf,
    pub prompt: String,
    pub policy: policy::Policy,
    /// Whether a plugin window is open to put an `ask` decision to.
    pub window_open: bool,
    pub api_key: Option<String>,
}

/// DSH derives `workspace-write` from the ACP session's immutable cwd. A
/// caller-declared deliverable is the narrowest honest boundary; a note-scoped
/// run uses the note's directory. Unscoped tasks retain their task directory.
fn session_workspace(meta: &RunMeta) -> PathBuf {
    let root = meta.vault.canonicalize().ok();
    let inside_vault = |candidate: &Path| {
        let canonical = candidate.canonicalize().ok()?;
        canonical.starts_with(root.as_ref()?).then_some(canonical)
    };
    let scoped_parent = |path: &Path| {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            meta.vault.join(path)
        };
        inside_vault(absolute.parent()?)
    };

    if let Some(deliverable) = meta.deliverable.as_deref() {
        if let Some(dir) = scoped_parent(deliverable) {
            return dir;
        }
    }
    if let Some(target) = meta.target.as_deref().map(Path::new) {
        if let Some(dir) = scoped_parent(target) {
            return dir;
        }
    }
    meta.task_dir.clone()
}

/// Moving the ACP cwd to the output directory would otherwise make DSH stop
/// discovering the task's AGENTS.md/CLAUDE.md. Carry the harness-specific task
/// protocol into the direct prompt whenever the session leaves task_dir.
fn prompt_for_session(spec: &RunSpec) -> String {
    let workspace = session_workspace(&spec.meta);
    let same_dir = workspace.canonicalize().ok() == spec.meta.task_dir.canonicalize().ok();
    if same_dir {
        return spec.prompt.clone();
    }
    let instructions = ["AGENTS.md", "CLAUDE.md"]
        .iter()
        .find_map(|name| std::fs::read_to_string(spec.meta.task_dir.join(name)).ok())
        .filter(|body| !body.trim().is_empty());
    let mut parts = Vec::new();
    if let Some(body) = instructions {
        parts.push(format!(
            "## 任务协议（来自任务目录，必须遵守）\n提示中提到的 AGENTS.md 或 CLAUDE.md 均指以下协议：\n{}",
            body.trim()
        ));
    }
    let mut paths = format!(
        "## 本次运行路径\nVault 根目录：`{}`\n当前可写工作区：`{}`\n任务或用户提示中的 Vault 相对路径都以 Vault 根目录解析。",
        spec.meta.vault.display(),
        workspace.display()
    );
    if let Some(target) = spec.meta.target.as_deref() {
        paths.push_str(&format!("\n本次输入文件：`{target}`"));
    }
    if let Some(deliverable) = spec.meta.deliverable.as_deref() {
        paths.push_str(&format!("\n本次唯一约定产物：`{}`", deliverable.display()));
    }
    parts.push(paths);
    parts.push(spec.prompt.trim().to_string());
    parts.join("\n\n")
}

/// The actor written into `by::` / `generated.by` (OKF §7). Always the harness
/// and its model — never a `human:` prefix, whatever the run produced.
pub fn actor(model: Option<&str>) -> String {
    format!("deepseek-harness/{}", model.unwrap_or("deepseek-v4-pro"))
}

/// Run once. Any message on `cancel` cancels the session and terminates the
/// child process group. The task lock is taken here and held until the run ends.
pub async fn run(
    spec: RunSpec,
    tx: mpsc::UnboundedSender<Step>,
    mut cancel: mpsc::Receiver<()>,
) -> Result<(), record::RunRecord> {
    let started = match scaffold::preflight(&spec.meta).await {
        Ok(s) => s,
        Err(Blocked::Busy(who)) => {
            // The caller turns this into the "already running" toast; there is
            // deliberately no record, because this run never happened.
            return Err(record::RunRecord {
                run_id: who.run_id,
                task: spec.meta.task.id.clone(),
                trigger: spec.meta.trigger.clone(),
                started_at: who.started_at,
                ended_at: String::new(),
                status: record::Status::Error,
                exit_code: None,
                num_turns: None,
                session_id: None,
                result: String::new(),
                stderr_tail: String::new(),
                artifacts: Vec::new(),
                harness: Some(crate::SELF_PLUGIN_ID.to_string()),
                usage: None,
            });
        }
        Err(Blocked::Skip(reason)) => {
            let rec = scaffold::finalize_without_run(
                &spec.meta,
                chrono::Utc::now(),
                record::Status::Skipped,
                reason,
            );
            let _ = tx.send(Step::Done(rec));
            return Ok(());
        }
    };

    let mut cmd = tokio::process::Command::new(&spec.launcher.program);
    // `dsh --profile notemd --patch <vault overlay>`: the profile is the
    // composition (the bridge plus dsh-base's rows), the overlay is what note.md
    // changes about it.
    cmd.args(spec.launcher.args(&spec.config))
        // Process cwd stays at the task template so launch-time relative files
        // remain stable. The ACP session gets its separately scoped cwd during
        // the handshake; DSH uses THAT as the workspace-write boundary.
        .current_dir(&spec.meta.task_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // dsh is a Node program that spawns its own tooling; a GUI-launched host
        // inherits a PATH with no node in it at all.
        .env(
            "PATH",
            spec.env_path
                .clone()
                .unwrap_or_else(crate::discover::runtime_path),
        )
        // The one real permission boundary. `cordis.yml` reads it in two places:
        // the sandbox fence and the approval gate.
        .env("DSH_PERMISSION_MODE", spec.policy.permission_mode.as_env())
        .env("NOTEMD_DSH_SESSIONS", &spec.sessions_dir);
    if let Some(k) = &spec.api_key {
        cmd.env("DEEPSEEK_API_KEY", k);
    }
    // Own process group, so a timeout or cancel takes down the server AND every
    // process it spawned in one signal.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let rec = scaffold::finalize_without_run(
                &spec.meta,
                started.started,
                record::Status::Error,
                format!(
                    "spawn failed ({}): {e}",
                    spec.launcher.program.display()
                ),
            );
            let _ = tx.send(Step::Done(rec));
            return Ok(());
        }
    };
    let pgid = child.id().unwrap_or(0) as i32;
    let mut stdin = child.stdin.take().expect("piped");
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let mut lines = BufReader::new(stdout).lines();

    // stderr is the harness's diagnostic noise, not something to show the user —
    // keep only a tail for the failure record. stdout is reserved for protocol
    // frames, so anything human-readable lands here.
    let err_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let eb = err_buf.clone();
    let err_task = tokio::spawn(async move {
        let mut el = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = el.next_line().await {
            let mut g = eb.lock().unwrap();
            g.push_str(&l);
            g.push('\n');
            let t = record::tail(&g, record::STDERR_LIMIT * 2);
            *g = t;
        }
    });

    let mut progress =
        ProgressTracker::start(&spec.meta.task_run_dir, &spec.meta.run_id, started.started);
    let mut state = Dialogue::new();
    // Kick off the handshake. Everything after this is driven by what comes back.
    let _ = stdin
        .write_all(state.begin(acp::METHOD_INITIALIZE, acp::initialize_params()).as_bytes())
        .await;

    // `timeout_seconds` 是**静默上限**,不是总时长上限:每收到一帧就重新起算。
    // 一次深度阅读跑 40 分钟不是"卡住",而卡住的表现恰恰是不再有帧 —— 拿总时长
    // 砍一个还在出声的 run,只会在最后一刻把快写完的答案扔掉。
    let quiet_limit = std::time::Duration::from_secs(spec.meta.task.timeout_seconds);
    let deadline = tokio::time::sleep(quiet_limit);
    tokio::pin!(deadline);

    let mut collected = String::new();
    let mut noise = String::new();
    let mut outcome: Option<Outcome> = None;
    let forced = loop {
        if outcome.is_some() {
            break None;
        }
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(l)) => {
                    deadline.as_mut().reset(tokio::time::Instant::now() + quiet_limit);
                    let Some(frame) = acp::parse_incoming(&l) else {
                        // 非协议噪声(launcher/pnpm 的横幅与进度)。不是垃圾:当
                        // 服务端根本没起来时(2026-08-18:pnpm 静默补装依赖数分钟
                        // 后 exit 1,stderr 全程为空),这是仅存的现场证据。
                        noise.push_str(&l);
                        noise.push('\n');
                        noise = record::tail(&noise, record::STDERR_LIMIT * 2);
                        continue;
                    };
                    for act in state.step(frame, &spec) {
                        match act {
                            Act::Send(bytes) => { let _ = stdin.write_all(bytes.as_bytes()).await; }
                            Act::Emit(ev) => {
                                match &ev {
                                    Event::Text { text } => {
                                        collected.push_str(text);
                                        progress.step(text, text);
                                    }
                                    Event::Permission { tool, decision } => {
                                        let line = format!("permission {decision}: {tool}");
                                        progress.step(&line, &line);
                                    }
                                    _ => {}
                                }
                                let _ = tx.send(Step::Event(ev));
                            }
                            Act::Finish(o) => outcome = Some(o),
                        }
                    }
                }
                // EOF or a read error: the server went away without finishing.
                _ => break Some(record::Status::Error),
            },
            _ = &mut deadline => {
                // Ask nicely first — a cooperative agent flushes its persistence
                // on cancel — then take the group down regardless.
                if let Some(s) = state.session_id() {
                    let _ = stdin.write_all(
                        acp::notification_frame(acp::METHOD_SESSION_CANCEL, acp::cancel_params(s)).as_bytes()
                    ).await;
                }
                kill_group(pgid);
                break Some(record::Status::Timeout);
            }
            _ = cancel.recv() => {
                if let Some(s) = state.session_id() {
                    let _ = stdin.write_all(
                        acp::notification_frame(acp::METHOD_SESSION_CANCEL, acp::cancel_params(s)).as_bytes()
                    ).await;
                }
                kill_group(pgid);
                break Some(record::Status::Cancelled);
            }
        }
    };

    // Closing stdin is the harness's cue to quiesce and flush its session log;
    // the kill below is the backstop for one that will not.
    drop(stdin);
    let exit = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait())
        .await
        .ok()
        .and_then(|r| r.ok())
        .and_then(|s| s.code());
    if exit.is_none() {
        kill_group(pgid);
        let _ = child.kill().await;
    }
    let _ = err_task.await;
    let mut stderr_tail = record::tail(&err_buf.lock().unwrap(), record::STDERR_LIMIT);
    if stderr_tail.is_empty() {
        // stderr 规定放诊断,但 pnpm/launcher 把一切都打在 stdout —— 空的 stderr
        // 配上一条失败记录等于「窗口一片空白」。退而取 stdout 噪声的尾巴。
        stderr_tail = record::tail(&noise, record::STDERR_LIMIT);
    }

    let session = state.session_id().unwrap_or_default().to_string();
    let (mut status, mut result) = match (forced, outcome) {
        // 服务端一帧没答完就走了(EOF/读错):多半从未启动。空 result 会让窗口
        // 无话可说 —— 合成一句人话,细节由 stderr_tail(或噪声尾巴)兜着。
        (Some(record::Status::Error), _) => (
            record::Status::Error,
            Some(agent_run_core::event::RunResult {
                is_error: true,
                result: format!(
                    "the ACP server exited{} without completing the run — it probably never started; its last output is below",
                    exit.map(|c| format!(" (code {c})")).unwrap_or_default()
                ),
                session_id: (!session.is_empty()).then(|| session.clone()),
                num_turns: None,
                usage: None,
            }),
        ),
        (Some(s), _) => (s, None),
        (None, Some(Outcome::Stopped { stop, usage })) => {
            let r = acp::result_for_stop(&stop, &collected, &session, usage);
            let s = match stop.as_str() {
                "end_turn" => record::Status::Success,
                "cancelled" => record::Status::Cancelled,
                _ => record::Status::Error,
            };
            (s, Some(r))
        }
        (None, Some(Outcome::Failed(msg))) => (
            record::Status::Error,
            Some(agent_run_core::event::RunResult {
                is_error: true,
                result: msg,
                session_id: (!session.is_empty()).then(|| session.clone()),
                num_turns: None,
                usage: None,
            }),
        ),
        (None, None) => (record::Status::Error, None),
    };

    // `end_turn` only means the model stopped talking. When a caller declared
    // one concrete output, success also means that file now exists.
    if status == record::Status::Success {
        if let Some(path) = spec.meta.deliverable.as_deref().filter(|p| !p.is_file()) {
            status = record::Status::Error;
            let message = format!("promised deliverable was not created: {}", path.display());
            match &mut result {
                Some(r) => {
                    r.is_error = true;
                    if r.result.trim().is_empty() {
                        r.result = message;
                    } else {
                        r.result.push_str("\n\n[");
                        r.result.push_str(&message);
                        r.result.push(']');
                    }
                }
                None => {
                    result = Some(agent_run_core::event::RunResult {
                        is_error: true,
                        result: message,
                        session_id: (!session.is_empty()).then(|| session.clone()),
                        num_turns: None,
                        usage: None,
                    });
                }
            }
        }
    }

    let rec = scaffold::finalize(
        &spec.meta,
        &started,
        status,
        exit,
        result,
        // A protocol that died mid-handshake has no result text; whatever the
        // harness printed to stderr is the only explanation there is.
        String::new(),
        stderr_tail,
        &actor(spec.meta.task.model.as_deref()),
    );
    let _ = tx.send(Step::Done(rec));
    Ok(())
}

fn kill_group(pgid: i32) {
    if pgid > 0 {
        unsafe {
            libc::killpg(pgid, libc::SIGTERM);
        }
    }
}

/// How the dialogue ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// `session/prompt` answered with this `stopReason` and optional accounting.
    Stopped {
        stop: String,
        usage: Option<agent_run_core::usage::Usage>,
    },
    /// The handshake or the turn failed, with a reason worth showing.
    Failed(String),
}

/// One thing the dialogue wants done.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    Send(String),
    Emit(Event),
    Finish(Outcome),
}

/// The handshake state machine, kept free of I/O so every transition is
/// testable: frames in, actions out.
pub struct Dialogue {
    next_id: u64,
    /// Which of our requests is outstanding, and what it was.
    pending: Option<(u64, &'static str)>,
    session: Option<String>,
}

impl Default for Dialogue {
    fn default() -> Self {
        Self::new()
    }
}

impl Dialogue {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            pending: None,
            session: None,
        }
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session.as_deref()
    }

    /// Frame the first request and remember we are waiting on it.
    pub fn begin(&mut self, method: &'static str, params: Value) -> String {
        let id = self.next_id;
        self.next_id += 1;
        self.pending = Some((id, method));
        acp::request_frame(id, method, params)
    }

    /// Advance on one incoming frame.
    pub fn step(&mut self, frame: acp::Incoming, spec: &RunSpec) -> Vec<Act> {
        match frame {
            // The agent asks; we answer from policy. This is the ONLY thing it
            // ever asks us, and it tells us only an opaque tool-call id.
            acp::Incoming::Request { id, method, params }
                if method == acp::METHOD_REQUEST_PERMISSION =>
            {
                let tool = acp::permission_tool_id(&params);
                let options = acp::permission_options(&params);
                // `Ask` needs a window and a person; with neither, `decide` has
                // already folded it to `Reject` (fail-closed).
                let decision = match spec.policy.decide(spec.window_open) {
                    policy::Outcome::Allow => acp::Decision::Allow,
                    // An interactive prompt is Phase 2; until then a window does
                    // not change the answer, it only means we could have asked.
                    policy::Outcome::Reject | policy::Outcome::Ask => acp::Decision::Reject,
                };
                let result = acp::permission_result(&options, decision);
                // Report what we actually answered, not what we intended: if the
                // agent offered no option of the wanted kind, this is `cancelled`.
                let reported = match result.pointer("/outcome/outcome").and_then(|v| v.as_str()) {
                    Some("selected") => decision.label().to_string(),
                    _ => "cancelled".to_string(),
                };
                vec![
                    Act::Send(acp::response_frame(&id, result)),
                    Act::Emit(Event::Permission {
                        tool,
                        decision: reported,
                    }),
                ]
            }
            // Anything else it asks, we do not implement — but a JSON-RPC request
            // left unanswered wedges the agent's turn forever, so answer with an
            // empty result rather than staying silent.
            acp::Incoming::Request { id, .. } => {
                vec![Act::Send(acp::response_frame(&id, Value::Null))]
            }
            acp::Incoming::Notification { method, params }
                if method == acp::METHOD_SESSION_UPDATE =>
            {
                acp::update_to_event(&params).map(Act::Emit).into_iter().collect()
            }
            acp::Incoming::Notification { .. } => Vec::new(),
            acp::Incoming::Response { id, result } => {
                let Some((want, method)) = self.pending else {
                    return Vec::new();
                };
                if id != want {
                    // Not the answer we are waiting for. Dropping it is right:
                    // we only ever have one request in flight.
                    return Vec::new();
                }
                self.pending = None;
                let result = match result {
                    Ok(v) => v,
                    Err(e) => return vec![Act::Finish(Outcome::Failed(format!("{method}: {e}")))],
                };
                self.advance(method, result, spec)
            }
        }
    }

    fn advance(&mut self, method: &'static str, result: Value, spec: &RunSpec) -> Vec<Act> {
        match method {
            acp::METHOD_INITIALIZE => match acp::check_initialize(&result) {
                Ok(_) => vec![Act::Send(self.begin(
                    acp::METHOD_SESSION_NEW,
                    acp::new_session_params(&session_workspace(&spec.meta).to_string_lossy()),
                ))],
                Err(e) => vec![Act::Finish(Outcome::Failed(e))],
            },
            acp::METHOD_SESSION_NEW => match acp::session_id(&result) {
                Ok(s) => {
                    self.session = Some(s.clone());
                    vec![Act::Send(self.begin(
                        acp::METHOD_SESSION_PROMPT,
                        acp::prompt_params(&s, &prompt_for_session(spec)),
                    ))]
                }
                Err(e) => vec![Act::Finish(Outcome::Failed(e))],
            },
            acp::METHOD_SESSION_PROMPT => {
                let stop = result
                    .get("stopReason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if stop.is_empty() {
                    return vec![Act::Finish(Outcome::Failed(format!(
                        "session/prompt answered without a stopReason: {result}"
                    )))];
                }
                let fallback_model = composition::default_model(&spec.config);
                vec![Act::Finish(Outcome::Stopped {
                    stop,
                    usage: acp::prompt_usage_at(
                        &result,
                        fallback_model.as_deref(),
                        chrono::Utc::now(),
                    ),
                })]
            }
            other => vec![Act::Finish(Outcome::Failed(format!(
                "unexpected reply to {other}"
            )))],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_run_core::task::TaskDef;

    /// The scripted ACP server, built alongside this crate's own binary.
    fn stub() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target");
        p.push(if cfg!(debug_assertions) { "debug" } else { "release" });
        p.push("stub-acp");
        assert!(
            p.is_file(),
            "the stub ACP server is missing — run `cargo build --bin stub-acp` first ({})",
            p.display()
        );
        p
    }

    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(dir.path().join("task")).unwrap();
            std::fs::write(dir.path().join("cordis.patch.yml"), "# stub\n").unwrap();
            Self { dir }
        }

        fn spec(&self, timeout: u64) -> RunSpec {
            let v = self.dir.path();
            RunSpec {
                meta: RunMeta {
                    vault: v.to_path_buf(),
                    task: TaskDef {
                        id: "t".into(),
                        name: "T".into(),
                        description: String::new(),
                        prompt: "p".into(),
                        max_turns: None,
                        timeout_seconds: timeout,
                        model: Some("deepseek-v4-pro".into()),
                        precheck: None,
                        okf_type: None,
            directive: Vec::new(),
                    },
                    task_dir: v.join("task"),
                    task_run_dir: v.join("runs-t"),
                    run_id: "20260817T000000Z-000001".into(),
                    trigger: "window".into(),
                    harness: crate::SELF_PLUGIN_ID.to_string(),
                    target: None,
                    deliverable: None,
                },
                launcher: crate::discover::Launcher {
                    program: stub(),
                    origin: "test stub".into(),
                },
                config: v.join("cordis.patch.yml"),
                // Keep the login shell out of the tests entirely.
                env_path: Some("/usr/bin:/bin".into()),
                sessions_dir: v.join("sessions"),
                prompt: "答一下".into(),
                policy: crate::policy::Policy::default(),
                window_open: false,
                api_key: None,
            }
        }
    }

    async fn drive(spec: RunSpec) -> (Vec<Event>, record::RunRecord) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_ctx, crx) = mpsc::channel(1);
        run(spec, tx, crx).await.expect("the engine must not report busy");
        collect(&mut rx)
    }

    fn collect(rx: &mut mpsc::UnboundedReceiver<Step>) -> (Vec<Event>, record::RunRecord) {
        let (mut evs, mut done) = (Vec::new(), None);
        while let Ok(step) = rx.try_recv() {
            match step {
                Step::Event(e) => evs.push(e),
                Step::Done(r) => done = Some(r),
            }
        }
        (evs, done.expect("the engine must always emit Done"))
    }

    /// Scope an env var to one test. The stub is configured through the
    /// environment it inherits, and cargo runs tests in one process.
    struct EnvVar(&'static str);
    impl EnvVar {
        fn set(k: &'static str, v: &str) -> Self {
            std::env::set_var(k, v);
            EnvVar(k)
        }
    }
    impl Drop for EnvVar {
        fn drop(&mut self) {
            std::env::remove_var(self.0);
        }
    }

    /// The env is process-global, so the tests that script the stub take turns.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn scoped_workspace_stays_inside_the_vault_and_carries_task_instructions() {
        let f = Fixture::new();
        let mut spec = f.spec(30);
        std::fs::write(spec.meta.task_dir.join("AGENTS.md"), "ONLY WRITE THE SUMMARY").unwrap();
        std::fs::write(spec.meta.task_dir.join("CLAUDE.md"), "SHOULD NOT BE INJECTED").unwrap();
        let workspace = f.dir.path().join("ssot/books/b");
        std::fs::create_dir_all(&workspace).unwrap();
        let target = workspace.join("book.md");
        std::fs::write(&target, "book").unwrap();
        let deliverable = workspace.join("summary.md");
        spec.meta.target = Some(target.to_string_lossy().to_string());
        spec.meta.deliverable = Some(deliverable.clone());

        assert_eq!(session_workspace(&spec.meta), workspace.canonicalize().unwrap());
        let prompt = prompt_for_session(&spec);
        assert!(prompt.contains("ONLY WRITE THE SUMMARY"), "{prompt}");
        assert!(!prompt.contains("SHOULD NOT BE INJECTED"), "{prompt}");
        assert!(prompt.contains(&f.dir.path().to_string_lossy().to_string()), "{prompt}");
        assert!(prompt.contains(&deliverable.to_string_lossy().to_string()), "{prompt}");

        let outside = tempfile::tempdir().unwrap();
        spec.meta.deliverable = Some(outside.path().join("escape.md"));
        spec.meta.target = None;
        assert_eq!(
            session_workspace(&spec.meta).canonicalize().unwrap(),
            spec.meta.task_dir.canonicalize().unwrap(),
            "an out-of-vault deliverable must not move the sandbox boundary"
        );

        let link = f.dir.path().join("linked-outside");
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        spec.meta.deliverable = Some(link.join("escape-through-link.md"));
        assert_eq!(
            session_workspace(&spec.meta).canonicalize().unwrap(),
            spec.meta.task_dir.canonicalize().unwrap(),
            "a symlink out of the vault must not move the sandbox boundary"
        );
    }

    #[test]
    fn a_note_scoped_run_uses_the_notes_directory_when_no_deliverable_is_named() {
        let f = Fixture::new();
        let mut spec = f.spec(30);
        let notes = f.dir.path().join("notes/topic");
        std::fs::create_dir_all(&notes).unwrap();
        let note = notes.join("question.note.md");
        std::fs::write(&note, "- question").unwrap();
        spec.meta.target = Some("notes/topic/question.note.md".into());
        assert_eq!(session_workspace(&spec.meta), notes.canonicalize().unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn walks_the_handshake_and_records_a_success() {
        let _g = env_guard();
        let _t = EnvVar::set("STUB_TEXT", "答案是 42");
        let f = Fixture::new();
        let (evs, rec) = drive(f.spec(30)).await;

        assert_eq!(rec.status, record::Status::Success, "{rec:?}");
        assert_eq!(rec.result, "答案是 42");
        assert_eq!(rec.session_id.as_deref(), Some("stub-session-1"));
        // Committed text surfaces; the thought alongside it does not.
        assert_eq!(
            evs.iter().filter(|e| matches!(e, Event::Text { .. })).count(),
            1
        );
        assert!(
            !evs.iter().any(|e| matches!(e, Event::ToolUse { .. })),
            "ACP carries no tool events: {evs:?}"
        );
    }

    /// The child still starts in the task directory, but a scoped run's ACP
    /// session workspace is the promised deliverable's directory. DSH derives
    /// the `workspace-write` boundary from session/new.cwd, not process.cwd.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_deliverable_scopes_the_session_workspace_without_moving_the_child() {
        let _g = env_guard();
        let _e = EnvVar::set("STUB_ECHO_CWD", "1");
        let f = Fixture::new();
        let mut spec = f.spec(30);
        let task_dir = spec.meta.task_dir.canonicalize().unwrap();
        let workspace = f.dir.path().join("ssot/books/b");
        std::fs::create_dir_all(&workspace).unwrap();
        spec.meta.deliverable = Some(workspace.join("summary.md"));
        let (_evs, rec) = drive(spec).await;

        let mut lines = rec.result.lines();
        let process_cwd = PathBuf::from(lines.next().unwrap()).canonicalize().unwrap();
        let session_cwd = PathBuf::from(lines.next().unwrap()).canonicalize().unwrap();
        assert_eq!(process_cwd, task_dir, "the process must run in the task dir");
        assert_eq!(session_cwd, workspace.canonicalize().unwrap());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn success_without_the_promised_deliverable_is_an_error() {
        let _g = env_guard();
        let _t = EnvVar::set("STUB_TEXT", "写好了");
        let f = Fixture::new();
        let mut spec = f.spec(30);
        let workspace = f.dir.path().join("ssot/books/b");
        std::fs::create_dir_all(&workspace).unwrap();
        spec.meta.deliverable = Some(workspace.join("missing-summary.md"));

        let (_evs, rec) = drive(spec).await;
        assert_eq!(rec.status, record::Status::Error, "{rec:?}");
        assert!(rec.result.contains("promised deliverable"), "{}", rec.result);
        assert!(rec.result.contains("missing-summary.md"), "{}", rec.result);
    }

    /// The permission mode is the run's only real boundary; it reaches the child
    /// as an environment variable the composition reads.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_sandbox_mode_reaches_the_child_as_an_environment_variable() {
        let _g = env_guard();
        let _e = EnvVar::set("STUB_ECHO_ENV", "DSH_PERMISSION_MODE");
        let f = Fixture::new();
        let mut spec = f.spec(30);
        spec.policy.permission_mode = crate::policy::PermissionMode::ReadOnly;
        let (_evs, rec) = drive(spec).await;
        assert_eq!(rec.result, "read-only");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_profile_and_the_vault_overlay_reach_the_child() {
        let _g = env_guard();
        let f = Fixture::new();
        let argv = f.dir.path().join("argv.txt");
        let _a = EnvVar::set("STUB_ARGV_FILE", argv.to_str().unwrap());
        drive(f.spec(30)).await;
        let seen = std::fs::read_to_string(&argv).unwrap();
        // `dsh --profile notemd --patch <vault overlay>`: the profile is the
        // composition, the overlay is what note.md changes about it.
        assert!(seen.contains("--profile"), "argv: {seen}");
        assert!(seen.contains("notemd"), "argv: {seen}");
        assert!(seen.contains("--patch"), "argv: {seen}");
        assert!(seen.contains("cordis.patch.yml"), "argv: {seen}");
        assert!(!seen.contains("--config"), "the old flag is gone: {seen}");
    }

    /// A harness that prints a banner on stdout before the protocol starts must
    /// not take the run down.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn non_protocol_noise_on_stdout_is_survived() {
        let _g = env_guard();
        let _n = EnvVar::set("STUB_NOISE", "1");
        let _t = EnvVar::set("STUB_TEXT", "还是答出来了");
        let f = Fixture::new();
        let (_evs, rec) = drive(f.spec(30)).await;
        assert_eq!(rec.status, record::Status::Success);
        assert_eq!(rec.result, "还是答出来了");
        assert!(rec.stderr_tail.contains("stderr diagnostics"), "{rec:?}");
    }

    /// 2026-08-18 真实事故:checkout 依赖残缺,`pnpm run` 静默补装数分钟后
    /// exit 1;全部输出在 stdout,stderr 为空 —— 记录里 result/stderr_tail 双空,
    /// 窗口一片空白。修复后:噪声尾巴进 stderr_tail,EOF 无结果时合成一句解释。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_server_that_dies_before_the_protocol_leaves_an_explanation() {
        let _g = env_guard();
        let _d = EnvVar::set("STUB_DIE_EARLY", "1");
        let f = Fixture::new();
        let (_evs, rec) = drive(f.spec(30)).await;
        assert_eq!(rec.status, record::Status::Error);
        assert!(rec.result.contains("exited"), "must explain itself: {rec:?}");
        assert!(
            rec.stderr_tail.contains("Progress: resolved 925"),
            "stdout noise is the only evidence there is — keep its tail: {rec:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_protocol_version_we_do_not_speak_fails_loud() {
        let _g = env_guard();
        let _v = EnvVar::set("STUB_BAD_VERSION", "1");
        let f = Fixture::new();
        let (_evs, rec) = drive(f.spec(30)).await;
        assert_eq!(rec.status, record::Status::Error);
        assert!(rec.result.contains("protocol mismatch"), "{}", rec.result);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_session_new_without_an_id_is_an_error_not_a_hang() {
        let _g = env_guard();
        let _v = EnvVar::set("STUB_NO_SESSION_ID", "1");
        let f = Fixture::new();
        let (_evs, rec) = drive(f.spec(30)).await;
        assert_eq!(rec.status, record::Status::Error);
        assert!(rec.result.contains("no session id"), "{}", rec.result);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_json_rpc_error_keeps_the_servers_own_explanation() {
        let _g = env_guard();
        let _v = EnvVar::set("STUB_PROMPT_ERROR", "1");
        let f = Fixture::new();
        let (_evs, rec) = drive(f.spec(30)).await;
        assert_eq!(rec.status, record::Status::Error);
        assert!(rec.result.contains("the stub was told to fail"), "{}", rec.result);
    }

    /// Only `end_turn` is a clean finish; a token-limited half-answer is not.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_non_end_turn_stop_reason_is_recorded_as_a_failure() {
        let _g = env_guard();
        let _s = EnvVar::set("STUB_STOP", "max_tokens");
        let _t = EnvVar::set("STUB_TEXT", "写到一半");
        let f = Fixture::new();
        let (_evs, rec) = drive(f.spec(30)).await;
        assert_eq!(rec.status, record::Status::Error);
        assert!(rec.result.contains("写到一半"), "{}", rec.result);
        assert!(rec.result.contains("max_tokens"), "{}", rec.result);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_permission_request_is_answered_from_the_policy_and_logged() {
        let _g = env_guard();
        let _p = EnvVar::set("STUB_PERMISSION", "1");
        let _t = EnvVar::set("STUB_TEXT", "做完了");
        let f = Fixture::new();
        let mut spec = f.spec(30);
        spec.policy.on_permission_request = crate::policy::OnRequest::Allow;
        let (evs, rec) = drive(spec).await;

        assert_eq!(rec.status, record::Status::Success);
        assert!(rec.result.contains("selected:allow-once"), "{}", rec.result);
        let perm = evs
            .iter()
            .find_map(|e| match e {
                Event::Permission { tool, decision } => Some((tool.clone(), decision.clone())),
                _ => None,
            })
            .expect("the decision must reach the run log");
        assert_eq!(perm, ("call-7".to_string(), "allowed".to_string()));
    }

    /// The default. An unattended run must not be able to widen itself.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_default_policy_rejects_a_permission_request() {
        let _g = env_guard();
        let _p = EnvVar::set("STUB_PERMISSION", "1");
        let f = Fixture::new();
        let (evs, rec) = drive(f.spec(30)).await;
        assert!(rec.result.contains("selected:reject-once"), "{}", rec.result);
        assert!(evs.iter().any(
            |e| matches!(e, Event::Permission { decision, .. } if decision == "rejected")
        ));
    }

    /// With no window there is nobody to ask, so `ask` must fall to reject —
    /// never to allow.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_ask_policy_without_a_window_fails_closed() {
        let _g = env_guard();
        let _p = EnvVar::set("STUB_PERMISSION", "1");
        let f = Fixture::new();
        let mut spec = f.spec(30);
        spec.policy.on_permission_request = crate::policy::OnRequest::Ask;
        spec.window_open = false;
        let (_evs, rec) = drive(spec).await;
        assert!(rec.result.contains("selected:reject-once"), "{}", rec.result);
    }

    /// Selecting an option of the wrong kind would approve what the policy
    /// refused, so `cancelled` is the only honest answer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_allow_with_no_allow_option_on_offer_cancels() {
        let _g = env_guard();
        let _p = EnvVar::set("STUB_PERMISSION", "1");
        let _n = EnvVar::set("STUB_NO_ALLOW", "1");
        let f = Fixture::new();
        let mut spec = f.spec(30);
        spec.policy.on_permission_request = crate::policy::OnRequest::Allow;
        let (evs, rec) = drive(spec).await;
        assert!(rec.result.contains("cancelled"), "{}", rec.result);
        assert!(
            evs.iter().any(
                |e| matches!(e, Event::Permission { decision, .. } if decision == "cancelled")
            ),
            "the log must say what was actually answered, not what was intended: {evs:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_hung_agent_hits_the_quiet_timeout_and_is_killed() {
        let _g = env_guard();
        let _h = EnvVar::set("STUB_HANG", "1");
        let f = Fixture::new();
        let (_evs, rec) = drive(f.spec(1)).await;
        assert_eq!(rec.status, record::Status::Timeout);
    }

    /// 静默上限 ≠ 总时长上限:只要还在出声,跑多久都不该被腰斩。
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_talkative_agent_outlives_the_quiet_limit() {
        let _g = env_guard();
        let _c = EnvVar::set("STUB_CHUNKS", "12");
        let _t = EnvVar::set("STUB_TEXT", "tick");
        let f = Fixture::new();
        // 12 chunks × 60 ms ≈ 0.7 s of streaming against a 1 s quiet limit: the
        // total outlives the limit while no single gap comes close to it.
        let (evs, rec) = drive(f.spec(1)).await;
        assert_eq!(
            rec.status,
            record::Status::Success,
            "a streaming run must not be timed out ({} events)",
            evs.len()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancel_stops_a_running_task() {
        let _g = env_guard();
        let _h = EnvVar::set("STUB_HANG", "1");
        let f = Fixture::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (ctx, crx) = mpsc::channel(1);
        let h = tokio::spawn(run(f.spec(60), tx, crx));
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        ctx.send(()).await.unwrap();
        h.await.unwrap().unwrap();
        let (_evs, rec) = collect(&mut rx);
        assert_eq!(rec.status, record::Status::Cancelled);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_missing_executable_records_an_error_instead_of_panicking() {
        let _g = env_guard();
        let f = Fixture::new();
        let mut spec = f.spec(30);
        spec.launcher.program = f.dir.path().join("nope");
        let (_evs, rec) = drive(spec).await;
        assert_eq!(rec.status, record::Status::Error);
        assert!(rec.stderr_tail.contains("spawn failed"), "{rec:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_same_task_cannot_run_twice_at_once() {
        let _g = env_guard();
        let _h = EnvVar::set("STUB_HANG", "1");
        let f = Fixture::new();
        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (_c1, cr1) = mpsc::channel(1);
        let h = tokio::spawn(run(f.spec(60), tx1, cr1));
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;

        let (tx2, _rx2) = mpsc::unbounded_channel();
        let (_c2, cr2) = mpsc::channel(1);
        let busy = run(f.spec(60), tx2, cr2).await.unwrap_err();
        assert_eq!(busy.run_id, "20260817T000000Z-000001");
        h.abort();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failing_precheck_skips_the_run_without_starting_the_agent() {
        use std::os::unix::fs::PermissionsExt;
        let _g = env_guard();
        let f = Fixture::new();
        let mut spec = f.spec(30);
        let check = spec.meta.task_dir.join("precheck.sh");
        std::fs::write(&check, "#!/bin/sh\necho '没有待答的问题'\nexit 1\n").unwrap();
        std::fs::set_permissions(&check, std::fs::Permissions::from_mode(0o755)).unwrap();
        spec.meta.task.precheck = Some("precheck.sh".into());
        // Point at a binary that would fail loudly if it were ever started.
        spec.launcher.program = f.dir.path().join("must-not-run");

        let (_evs, rec) = drive(spec).await;
        assert_eq!(rec.status, record::Status::Skipped);
        assert_eq!(rec.result, "没有待答的问题");
    }

    /// OKF §7: the actor is always the harness and its model. Never `human:`.
    #[test]
    fn the_actor_is_the_harness_and_never_a_human() {
        assert_eq!(actor(Some("deepseek-v4-flash")), "deepseek-harness/deepseek-v4-flash");
        assert_eq!(actor(None), "deepseek-harness/deepseek-v4-pro");
        assert!(!actor(Some("x")).starts_with("human:"));
    }

    /// A request we do not implement, left unanswered, wedges the agent's turn
    /// forever — so it gets an empty result rather than silence.
    #[test]
    fn an_unknown_agent_request_is_answered_rather_than_ignored() {
        let f = Fixture::new();
        let spec = f.spec(30);
        let mut d = Dialogue::new();
        let acts = d.step(
            crate::acp::Incoming::Request {
                id: serde_json::json!(9),
                method: "fs/read_text_file".into(),
                params: serde_json::json!({}),
            },
            &spec,
        );
        assert_eq!(acts.len(), 1);
        match &acts[0] {
            Act::Send(s) => {
                let v: serde_json::Value = serde_json::from_str(s.trim()).unwrap();
                assert_eq!(v["id"], 9);
                assert!(v.get("result").is_some(), "must be a response: {v}");
            }
            other => panic!("expected a response, got {other:?}"),
        }
    }

    #[test]
    fn a_response_to_a_request_we_are_not_waiting_on_is_dropped() {
        let f = Fixture::new();
        let spec = f.spec(30);
        let mut d = Dialogue::new();
        d.begin(crate::acp::METHOD_INITIALIZE, crate::acp::initialize_params());
        let acts = d.step(
            crate::acp::Incoming::Response {
                id: 999,
                result: Ok(serde_json::json!({ "protocolVersion": 1 })),
            },
            &spec,
        );
        assert!(acts.is_empty(), "{acts:?}");
    }

    #[test]
    fn a_prompt_reply_without_a_stop_reason_is_a_failure() {
        let f = Fixture::new();
        let spec = f.spec(30);
        let mut d = Dialogue::new();
        d.begin(crate::acp::METHOD_SESSION_PROMPT, serde_json::json!({}));
        let acts = d.step(
            crate::acp::Incoming::Response {
                id: 1,
                result: Ok(serde_json::json!({})),
            },
            &spec,
        );
        match &acts[0] {
            Act::Finish(Outcome::Failed(m)) => assert!(m.contains("stopReason"), "{m}"),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn a_prompt_reply_carries_usage_into_the_terminal_outcome() {
        let f = Fixture::new();
        let spec = f.spec(30);
        let mut d = Dialogue::new();
        d.begin(crate::acp::METHOD_SESSION_PROMPT, serde_json::json!({}));
        let acts = d.step(
            crate::acp::Incoming::Response {
                id: 1,
                result: Ok(serde_json::json!({
                    "stopReason": "end_turn",
                    "usage": {
                        "inputTokens": 7,
                        "cacheReadTokens": 2,
                        "outputTokens": 3
                    },
                    "totalCostUsd": 0.0004
                })),
            },
            &spec,
        );
        match &acts[0] {
            Act::Finish(Outcome::Stopped { stop, usage }) => {
                assert_eq!(stop, "end_turn");
                let usage = usage.as_ref().expect("usage");
                assert_eq!(usage.input_tokens, 7);
                assert_eq!(usage.cache_read_tokens, 2);
                assert_eq!(usage.output_tokens, 3);
                assert_eq!(
                    usage.cost.as_ref().map(|cost| cost.amount_usd),
                    Some(0.0004)
                );
            }
            other => panic!("expected a stopped outcome, got {other:?}"),
        }
    }
}
