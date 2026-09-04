//! The NotemdPlugin implementation: five window RPC methods plus the menu and
//! CLI commands.
//!
//! The SDK calls `on_ui_request` synchronously on the protocol read loop, so
//! `run.start` only spawns a tokio task and returns the run id immediately —
//! blocking here would wedge the whole plugin. Events reach the window from
//! that task via `host.ui_post`.
use crate::{discover, engine, lock, prompt, record, runner, task};
use agent_run_core::task::check_task_id;
use agent_run_core::{harness, InvocationModelRequest, ModelProfile};
use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const WINDOW: &str = "main";
const NO_VAULT: &str = "no vault configured";
/// The task the main window's Agent workspace runs.
const NOTE_TASK: &str = "answer-note-question";
/// Our own id, for a reminder that points back at this window's run log — and
/// the provenance stamped on every run record (both agent plugins share one
/// runs root, so a record without this reads as anyone's).
const SELF_PLUGIN_ID: &str = "notemd.claude-agent";
/// The harness behind this plugin, as the window names it.
const HARNESS_NAME: &str = "Claude Code";
/// Invocation profile used by latency-sensitive orchestration phases. Claude
/// Code resolves this stable alias to the concrete Haiku model available to
/// the installed CLI/account.
const FAST_MODEL: &str = "haiku";
/// A version probe runs while the window waits, so it is bounded tightly.
const VERSION_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The tray reminder a caller wants pushed when its run reaches a terminal
/// state. Handed to us with `run-task`, and sent from HERE rather than by the
/// caller, on purpose: a plugin with an open window is torn down the moment
/// that window closes (`plugin_runtime/windows.rs`, `WindowEvent::Destroyed` →
/// `deactivate()`), which kills its polling task and its reminder with it.
/// claude-agent normally has no window open, so its run outlives the caller's.
///
/// Known residual edge (accepted, not solved): if the user opens claude-agent's
/// OWN window and then closes it, this process is torn down the same way and
/// the run in flight is interrupted — no reminder for that run.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
struct NotifySpec {
    title_ok: String,
    title_fail: String,
    /// ABSOLUTE path the success reminder opens. The host re-checks that it is
    /// inside the vault before accepting it (`ui_rpc::notify_push`).
    open_path: String,
    /// ABSOLUTE path that must exist for the run to count as a success — a
    /// `success` record with no file on disk is a failure to the user.
    expect_file: String,
}

/// Where this invocation wants its terminal usage summary rendered. The
/// backend defaults to a toast so callers that predate the option still get the
/// completion hint; `result` leaves the data on the Done record for an embedded
/// result surface to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UsageDisplay {
    Tip,
    Result,
}

impl UsageDisplay {
    fn from_params(params: &Value) -> Result<Self, String> {
        match params.get("usage_display") {
            None | Some(Value::Null) => Ok(Self::Tip),
            Some(Value::String(value)) if value == "tip" => Ok(Self::Tip),
            Some(Value::String(value)) if value == "result" => Ok(Self::Result),
            _ => Err("bad 'usage_display': expected 'tip' or 'result'".into()),
        }
    }
}

fn usage_tip(display: UsageDisplay, rec: &record::RunRecord) -> Option<String> {
    (display == UsageDisplay::Tip)
        .then(|| agent_run_core::usage::compact_run(rec.status, rec.usage.as_ref()))
}

/// Apply the mutually-exclusive invocation-level model controls. An omitted
/// selector preserves a legacy task pin; an explicit `default` deliberately
/// removes that pin and hands model selection back to Claude Code settings.
fn apply_invocation_model(def: &mut task::TaskDef, params: &Value) -> Result<(), String> {
    let request =
        InvocationModelRequest::from_context(params).map_err(|error| error.to_string())?;
    if let Some(model) = request.model() {
        def.model = Some(model.to_string());
        return Ok(());
    }
    match request.model_profile() {
        None => {}
        Some(ModelProfile::Default) => def.model = None,
        Some(ModelProfile::Fast) => def.model = Some(FAST_MODEL.into()),
    }
    Ok(())
}

/// Preserve every invocation-scoped control while the host's `run-task`
/// command is relayed into the common `start` path.
fn relayed_start_params(context: &Value, task_id: &str, prompt: &str, note_path: &str) -> Value {
    json!({
        "task": task_id,
        "prompt": prompt,
        "use_context": false,
        "note_path": note_path,
        "notify": context.get("notify").cloned().unwrap_or(Value::Null),
        "usage_display": context
            .get("usage_display")
            .cloned()
            .unwrap_or(Value::Null),
        "model_profile": context
            .get("model_profile")
            .cloned()
            .unwrap_or(Value::Null),
        "model": context.get("model").cloned().unwrap_or(Value::Null),
        "invocation_id": context.get("invocation_id").cloned().unwrap_or(Value::Null),
        "input_hash": context.get("input_hash").cloned().unwrap_or(Value::Null),
    })
}

fn harness_capabilities(
    default_model: Option<String>,
    available: bool,
) -> agent_run_core::HarnessCapabilities {
    agent_run_core::HarnessCapabilities {
        tasks: vec![
            task::GOVERNED_DOCUMENT_REVIEW_TASK.into(),
            task::SEARCH_PLAN_TASK.into(),
            task::SEARCH_ANSWER_TASK.into(),
            task::SEARCH_SUMMARY_TASK.into(),
            task::VAULT_RESEARCH_TASK.into(),
        ],
        search_plan_schemas: vec![1],
        terminal_result: true,
        input_only_isolation: true,
        model_routing: agent_run_core::ModelRoutingCapabilities {
            invocation_override: true,
            profiles: agent_run_core::ModelRoutingProfiles {
                fast: agent_run_core::ModelProfileCapability {
                    model: Some(FAST_MODEL.into()),
                    available,
                },
                default_profile: agent_run_core::ModelProfileCapability {
                    model: default_model,
                    available,
                },
            },
            // Claude Code has no reliable local, credential-free model catalog.
            selectable_models: Vec::new(),
        },
    }
}

/// A `success` record is not enough: the file the caller expects has to be on
/// disk, or the reminder would open nothing.
fn run_delivered(spec: &NotifySpec, rec: Option<&record::RunRecord>) -> bool {
    rec.map(|r| r.status) == Some(record::Status::Success)
        && std::path::Path::new(&spec.expect_file).is_file()
}

/// `host.notify` params: the file on success, our own run log on failure.
fn notify_params(spec: &NotifySpec, delivered: bool) -> Value {
    if delivered {
        json!({
            "title": spec.title_ok,
            "action": { "kind": "open_path", "path": spec.open_path },
        })
    } else {
        // 失败是需要注意的告警:标为 Warn(托盘黄点)。成功走默认 Info(蓝点)。
        json!({
            "title": spec.title_fail,
            "action": { "kind": "open_plugin_window",
                        "plugin_id": SELF_PLUGIN_ID, "window": WINDOW },
            "severity": "warn",
        })
    }
}

/// Push the one reminder this run is owed. MUST be called from a spawned task,
/// never from the protocol read loop: a `host.*` response can only be routed BY
/// that loop, so awaiting one on it deadlocks the plugin.
async fn notify_outcome(host: &sdk::Host, spec: &NotifySpec, rec: Option<&record::RunRecord>) {
    let params = notify_params(spec, run_delivered(spec, rec));
    if let Err(e) = host.request("host.notify", params).await {
        host.log_warn(&format!("host.notify failed: {e}"));
    }
}

/// A note's path relative to the vault, for naming it in a prompt. Absolute
/// paths outside the vault (and traversal) return None — a task must not be
/// pointed at a file the vault doesn't own.
fn note_relative_to_vault(vault: &std::path::Path, note_path: &str) -> Option<String> {
    let root = vault.canonicalize().ok()?;
    let p = std::path::Path::new(note_path);
    let abs = if p.is_absolute() {
        p.canonicalize().ok()?
    } else {
        root.join(p).canonicalize().ok()?
    };
    let rel = abs.strip_prefix(&root).ok()?;
    let s = rel.to_string_lossy().to_string();
    (!s.is_empty()).then_some(s)
}

#[derive(Default)]
struct Inner {
    vault: Option<PathBuf>,
    /// Whether the vault lookup has finished. `vault: None` means "still
    /// resolving" before this flips, and "no vault configured" after.
    vault_checked: bool,
    /// run_id → cancel channel
    running: HashMap<String, mpsc::Sender<()>>,
    invocations: agent_run_core::invocation::InvocationRegistry,
}

pub struct ClaudeAgentPlugin {
    inner: Arc<Mutex<Inner>>,
    tab_context: Option<Value>,
}

impl ClaudeAgentPlugin {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            tab_context: None,
        }
    }
}

/// A task plus its live state, for the window's task list.
#[derive(Serialize)]
struct TaskOverview {
    #[serde(flatten)]
    def: task::TaskDef,
    /// Derived from the lock file, not from an in-memory map — that's the only
    /// way a run started by a DETACHED CLI process shows up here.
    running: bool,
    running_since: Option<String>,
    last_run: Option<record::RunRecord>,
}

fn overview(vault: &std::path::Path) -> Vec<TaskOverview> {
    task::discover(vault)
        .into_iter()
        .map(|def| {
            let run_dir = task::runs_root(vault).join(&def.id);
            let held = lock::current(&run_dir);
            TaskOverview {
                def,
                running: held.is_some(),
                running_since: held.map(|h| h.started_at),
                last_run: record::recent(&run_dir, 1).into_iter().next(),
            }
        })
        .collect()
}

/// The vault root. The host is authoritative (`host.vault.info`), but it can
/// answer with nothing — during startup before vault_sync has initialised, for
/// one — so retry, and then fall back to the very config file the host itself
/// falls back to (`sotvault/mod.rs:215-232`). Every failure is logged: a
/// swallowed error here reads to the user as "no vault configured" with no way
/// to tell why.
async fn vault_from_host(host: &sdk::Host) -> Option<PathBuf> {
    for attempt in 1..=3 {
        match host.request("host.vault.info", json!({})).await {
            Ok(v) => {
                if let Some(root) = v
                    .get("root")
                    .and_then(|r| r.as_str())
                    .filter(|s| !s.is_empty())
                {
                    return Some(PathBuf::from(root));
                }
                host.log_warn(&format!("host.vault.info has no root (try {attempt}): {v}"));
            }
            Err(e) => host.log_warn(&format!("host.vault.info failed (try {attempt}): {e}")),
        }
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    }
    None
}

fn shared_config_path() -> Option<PathBuf> {
    // Overridable so a test never reads — and then seeds templates into — the
    // real vault of whoever is running the suite.
    if let Ok(p) = std::env::var("NOTEMD_SHARED_CONFIG") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join("Library/Application Support/net.notemd.app/shared.json"),
    )
}

fn shared_config_vault() -> Option<PathBuf> {
    shared_config_vault_at(&shared_config_path()?)
}

/// `{"sotvault": "/path"}` out of the shared config — the same key and file the
/// host reads.
fn shared_config_vault_at(path: &std::path::Path) -> Option<PathBuf> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let s = v.get("sotvault")?.as_str()?;
    (!s.is_empty()).then(|| PathBuf::from(s))
}

/// Everything a vault needs before a run: the rename migration, the built-in
/// templates, the gitignore lines. All pure filesystem work, so it is safe to
/// call synchronously on the protocol read loop — and it has to be, because a
/// command can arrive the instant activation returns.
fn prepare_vault(host: &sdk::Host, root: &std::path::Path) {
    // Rename before seeding, or the old and new names would both end up in the
    // task list.
    let moved = task::migrate_renamed_tasks(root);
    if !moved.is_empty() {
        host.log_info(&format!("migrated renamed tasks: {}", moved.join(", ")));
    }
    let wrote = task::seed_builtin_templates(root);
    if !wrote.is_empty() {
        host.log_info(&format!("seeded task templates: {}", wrote.join(", ")));
    }
    // Seeding never overwrites, so a vault that predates the change keeps denying
    // tools the templates no longer deny. Only a rewrite can lift a project deny.
    let freed = task::retire_information_denies(root);
    if !freed.is_empty() {
        host.log_info(&format!("retired information denies in: {}", freed.join(", ")));
    }
    task::ensure_gitignore(root);
}

/// Load a task, rebuilding a built-in whose directory has gone missing. Someone
/// deleting `.notemd/agent-tasks/answer-note-question/` should not turn the
/// workspace button into a permanent error.
fn load_task(vault: &std::path::Path, id: &str) -> Option<task::TaskDef> {
    let dir = task::task_dir(vault, id);
    if let Some(t) = task::read_task(&dir) {
        return Some(t);
    }
    task::seed_builtin_templates(vault);
    task::read_task(&dir)
}

impl sdk::NotemdPlugin for ClaudeAgentPlugin {
    fn activate(&mut self, host: &sdk::Host, _p: &proto::ActivateParams) -> Result<(), String> {
        let inner = self.inner.clone();
        let host = host.clone();

        // Seed the vault SYNCHRONOUSLY from the shared config — a plain file
        // read, no host round-trip. `plugin_v2_execute` activates the plugin
        // and runs the command immediately afterwards, so anything that waits
        // for the host's answer would have the first command race it and fail
        // with "no vault configured".
        let seeded = shared_config_vault();
        if let Some(root) = &seeded {
            inner.lock().unwrap().vault = Some(root.clone());
            // Templates are pure filesystem work — do them NOW, not in the
            // spawned task. A command arriving in between would otherwise find
            // the vault but no task and fail with "unknown task".
            prepare_vault(&host, root);
        }

        // MUST be spawned, never awaited inline. The SDK runs activate
        // synchronously ON the protocol read loop, and the response to
        // `host.vault.info` can only be routed BY that loop — awaiting it here
        // deadlocks the plugin until the host's request timeout, which looks
        // like an empty task list and a dead Run button.
        tokio::spawn(async move {
            // The host is authoritative; the seed only has to hold until it
            // answers. If it answers with nothing, the seed stands.
            let root = vault_from_host(&host).await.or(seeded);
            if let Some(root) = &root {
                // Idempotent: only writes what the sync pass didn't, and covers
                // the case where the host names a different vault than the seed.
                prepare_vault(&host, root);
                host.log_info(&format!("claude-agent ready (vault={})", root.display()));
            } else {
                host.log_warn("no vault configured; claude-agent needs one");
            }
            let mut g = inner.lock().unwrap();
            // Never clobber a working seed with None.
            if root.is_some() {
                g.vault = root;
            }
            g.vault_checked = true;
        });
        Ok(())
    }

    fn deactivate(&mut self, _host: &sdk::Host) {
        // The process is going away: tell every in-flight run to cancel so we
        // don't leave orphaned claude processes behind.
        let running: Vec<_> = self.inner.lock().unwrap().running.drain().collect();
        for (_id, tx) in running {
            let _ = tx.try_send(());
        }
    }

    fn execute_command(
        &mut self,
        host: &sdk::Host,
        params: &proto::ExecuteCommandParams,
    ) -> Result<Value, String> {
        match params.command.as_str() {
            // Opening the window: remember the tab of that moment; the window
            // asks for it with `context.get`.
            "open" => {
                self.tab_context = params.context.get("tab").cloned();
                Ok(json!({ "success": true }))
            }
            // CLI: notemd agent <task> [-p …] [--wait]
            "run" => {
                host.log_info(&format!("cli context: {}", params.context));
                self.cli_run(host, &params.context)
            }
            // The main window's Agent workspace: answer the open questions in
            // ONE note, rather than sweeping the whole vault.
            "run-note" => self.run_note(host, &params.context),
            // 宿主 host.agent.run 中转:任意任务 + 调用方拼好的定位 prompt。
            "run-task" => self.run_task(host, &params.context),
            "run-status" => self.run_status(&params.context),
            "run-cancel" => self.run_cancel(&params.context),
            // What the window shows above everything else: is Claude Code
            // there, which version, and did the last run die of something that
            // will kill the next one too.
            "harness-status" => self.harness_status(),
            other => Err(format!("unknown command '{other}'")),
        }
    }

    fn on_ui_request(
        &mut self,
        host: &sdk::Host,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        match method {
            // `ready: false` means the vault lookup is still in flight — the
            // window retries rather than reporting "no tasks".
            "tasks.list" => {
                let (vault, checked) = {
                    let g = self.inner.lock().unwrap();
                    (g.vault.clone(), g.vault_checked)
                };
                match vault {
                    Some(v) => Ok(json!({ "tasks": overview(&v), "ready": true })),
                    None if !checked => Ok(json!({ "tasks": [], "ready": false })),
                    None => Err(NO_VAULT.to_string()),
                }
            }
            "context.get" => Ok(json!({ "tab": self.tab_context })),
            // Also a UI method, not only a command: the window asks over the
            // `ui.request` channel (`bridge.request('plugin.…')`), while the
            // host's relay and the menu use `command.execute`. Registering it in
            // one place only left the window's banner spinning forever.
            "harness.status" | "harness-status" => self.harness_status(),
            "run.start" => self.start(host, params, "window"),
            "run.cancel" => {
                let id = params
                    .get("run_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let tx = self.inner.lock().unwrap().running.get(id).cloned();
                match tx {
                    Some(t) => {
                        let _ = t.try_send(());
                        Ok(json!({ "ok": true }))
                    }
                    None => Err(format!("run '{id}' is not running")),
                }
            }
            // No `task` (or an empty one) means every task, merged.
            "history.list" => {
                let vault = self.vault()?;
                let root = task::runs_root(&vault);
                let runs = match params.get("task").and_then(|v| v.as_str()) {
                    Some(t) if !t.is_empty() => record::recent(&root.join(t), 30),
                    _ => record::recent_all(&root, 30),
                };
                Ok(json!({ "runs": runs }))
            }
            // What one past run actually did, line by line.
            "history.log" => {
                let (root, task_id) = self.runs_root_and_task(&params)?;
                let run_id = params
                    .get("run_id")
                    .and_then(|v| v.as_str())
                    .ok_or("history.log needs a 'run_id'")?;
                Ok(json!({
                    "log": record::read_log(&root.join(task_id), run_id).unwrap_or_default(),
                }))
            }
            "history.delete" => {
                let (root, task_id) = self.runs_root_and_task(&params)?;
                let run_id = params
                    .get("run_id")
                    .and_then(|v| v.as_str())
                    .ok_or("history.delete needs a 'run_id'")?;
                Ok(json!({ "deleted": record::delete(&root.join(task_id), run_id) }))
            }
            // No task named: clear every task's history.
            "history.clear" => {
                let vault = self.vault()?;
                let root = task::runs_root(&vault);
                let n = match params.get("task").and_then(|v| v.as_str()) {
                    Some(t) if !t.is_empty() => record::clear(&root.join(t)),
                    _ => task::discover(&vault)
                        .iter()
                        .map(|t| record::clear(&root.join(&t.id)))
                        .sum(),
                };
                Ok(json!({ "cleared": n }))
            }
            other => Err(format!("unknown ui method '{other}'")),
        }
    }
}

impl ClaudeAgentPlugin {
    /// A record names its own task, so the window can act on a row from the
    /// all-tasks view without tracking which task it came from.
    fn runs_root_and_task(&self, params: &Value) -> Result<(PathBuf, String), String> {
        let vault = self.vault()?;
        let task_id = params
            .get("task")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("this call needs a 'task'")?
            .to_string();
        check_task_id(&task_id)?;
        Ok((task::runs_root(&vault), task_id))
    }

    fn vault(&self) -> Result<PathBuf, String> {
        self.inner
            .lock()
            .unwrap()
            .vault
            .clone()
            .ok_or_else(|| NO_VAULT.to_string())
    }

    /// Assemble a RunSpec, start the background task, return the run id.
    fn start(&mut self, host: &sdk::Host, params: Value, trigger: &str) -> Result<Value, String> {
        let invocation = agent_run_core::invocation::InvocationIdentity::from_context(&params)?;
        let vault = self.vault()?;
        let task_id = params
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or("missing 'task'")?
            .to_string();
        // Every entry point (window, CLI, run-note, the host's run-task relay)
        // funnels through here, so one check covers them all.
        check_task_id(&task_id)?;
        let user_prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let use_ctx = params
            .get("use_context")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let usage_display = UsageDisplay::from_params(&params)?;
        // A malformed spec is an error rather than a silent None: the caller
        // would otherwise wait forever for a reminder that was never going to
        // come.
        let notify = match params.get("notify") {
            Some(v) if !v.is_null() => Some(
                serde_json::from_value::<NotifySpec>(v.clone())
                    .map_err(|e| format!("bad 'notify': {e}"))?,
            ),
            _ => None,
        };

        let task_dir = task::task_dir(&vault, &task_id);
        let mut def = load_task(&vault, &task_id).ok_or(format!("unknown task '{task_id}'"))?;
        def.id = task_id.clone();
        apply_invocation_model(&mut def, &params)?;
        // Claude Code may resolve its settings-driven default only after it
        // starts. Invocation/task models are exact because this value is also
        // the one passed to `--model` below.
        let resolved_model = def.model.clone();

        let claude = discover::discover(std::env::var("NOTEMD_CLAUDE_BIN").ok().as_deref())
            .ok_or("claude executable not found — install Claude Code, or point NOTEMD_CLAUDE_BIN at it")?;

        // The one file this run is about, if the caller named one. The precheck
        // script reads it as NOTEMD_NOTE.
        let target = params
            .get("note_path")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ctx = if use_ctx && !task::is_input_only_task(&task_id) {
            self.tab_ctx()
        } else {
            None
        };
        let full = prompt::compose(&def.prompt, &user_prompt, ctx.as_ref());
        let run_id = record::new_run_id(chrono::Utc::now(), std::process::id());

        {
            let mut inner = self.inner.lock().unwrap();
            if let Some(existing) = inner.invocations.reuse_or_insert(
                invocation.as_ref(), &task_id, &run_id,
            )? {
                return Ok(json!({
                    "run_id": existing,
                    "resolved_model": resolved_model,
                    "reused": true,
                }));
            }
        }

        let spec = engine::RunSpec {
            vault: vault.clone(),
            task: def,
            task_dir,
            task_run_dir: task::runs_root(&vault).join(&task_id),
            claude,
            env_path: None,
            prompt: full,
            trigger: trigger.to_string(),
            harness: SELF_PLUGIN_ID.to_string(),
            run_id: run_id.clone(),
            oauth_token: std::env::var("CLAUDE_CODE_OAUTH_TOKEN").ok(),
            target,
            // The file the caller says this run must produce. Doubles as the
            // OKF stamp target and an artifact — it usually lives outside
            // output/ and answers/, so nothing else would find it.
            deliverable: notify.as_ref().map(|n| PathBuf::from(&n.expect_file)),
        };

        let (tx, mut rx) = mpsc::unbounded_channel();
        let (cancel_tx, cancel_rx) = mpsc::channel(1);
        self.inner
            .lock()
            .unwrap()
            .running
            .insert(run_id.clone(), cancel_tx);

        let h = host.clone();
        let inner = self.inner.clone();
        let rid = run_id.clone();
        tokio::spawn(async move {
            // The pump hands back the terminal record, so the reminder below
            // reports what actually happened. `None` = the run never reached a
            // record (the task was already locked, say) — a failure.
            let pump = {
                let h = h.clone();
                let rid = rid.clone();
                tokio::spawn(async move {
                    let mut last: Option<record::RunRecord> = None;
                    while let Some(step) = rx.recv().await {
                        match step {
                            engine::Step::Event(e) => h.ui_post(
                                WINDOW,
                                json!({ "kind": "event", "run_id": rid, "event": e }),
                            ),
                            engine::Step::Done(r) => {
                                h.ui_post(
                                    WINDOW,
                                    json!({ "kind": "done", "run_id": rid, "record": r }),
                                );
                                if let Some(summary) = usage_tip(usage_display, &r) {
                                    h.toast("info", "Claude token usage", Some(&summary));
                                }
                                last = Some(r);
                            }
                        }
                    }
                    last
                })
            };
            if let Err(busy) = engine::run(spec, tx, cancel_rx).await {
                h.ui_post(
                    WINDOW,
                    json!({ "kind": "busy", "run_id": rid, "holder": busy.0 }),
                );
                h.toast("warn", "That task is already running", Some(&busy.0.run_id));
            }
            let rec = pump.await.ok().flatten();
            inner.lock().unwrap().running.remove(&rid);
            // Exactly one reminder per run that asked for one. We are inside a
            // spawned task here, which is the only place `host.request` may be
            // awaited (see `notify_outcome`).
            if let Some(n) = notify {
                notify_outcome(&h, &n, rec.as_ref()).await;
            }
        });
        Ok(json!({ "run_id": run_id, "resolved_model": resolved_model }))
    }

    fn tab_ctx(&self) -> Option<prompt::TabContext> {
        let t = self.tab_context.as_ref()?;
        let path = t
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if path.is_empty() {
            return None;
        }
        Some(prompt::TabContext {
            path,
            selection: t
                .get("selection")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        })
    }

    /// Answer the open questions in ONE sidecar note. The main window hands us
    /// the note's path; we scope the task to it with an extra prompt paragraph
    /// rather than a second template, so the protocol stays in one place.
    fn run_note(&mut self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let vault = self.vault()?;
        let note_path = context
            .get("note_path")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("run-note needs a 'note_path'")?;
        let task_id = context
            .get("task")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(NOTE_TASK);

        let rel = note_relative_to_vault(&vault, note_path)
            .ok_or_else(|| format!("note is outside the vault: {note_path}"))?;
        let prompt = format!(
            "本次只处理这一个文件:`{rel}`,以及它对应的源文档(可能在 vault 之外的原目录)。\n\
             不要搜索 vault —— 权限已按此限定,搜索类工具不可用。\n\
             只回答该文件里 `status:: open` 的问题;没有待答问题时直接报告「无待答问题」并结束。"
        );
        host.log_info(&format!("run-note {task_id} on {rel}"));
        self.start(
            host,
            json!({
                "task": task_id,
                "prompt": prompt,
                "use_context": false,
                "note_path": note_path,
                "usage_display": context
                    .get("usage_display")
                    .cloned()
                    .unwrap_or(Value::Null),
            }),
            "note",
        )
    }

    /// Run any task with a caller-composed prompt — the host relays
    /// `host.agent.run` here. `note_path` (optional) scopes permissions to
    /// that one file via the task's settings.scoped.json, same as run-note.
    /// `notify` (optional) is the tray reminder we push on this run's behalf
    /// when it ends — see [`NotifySpec`] for why the CALLER can't push it.
    fn run_task(&mut self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let task_id = context
            .get("task")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("run-task needs a 'task'")?;
        let prompt = context.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let note_path = context.get("note_path").and_then(|v| v.as_str()).unwrap_or("");
        // Same guard as run-note: a caller-named note MUST resolve inside the
        // vault, or a task with a scoped policy (settings.scoped.json) would
        // let this relay write an arbitrary absolute path into the Read
        // allowlist of .claude/settings.local.json.
        if !note_path.is_empty() {
            let vault = self.vault()?;
            note_relative_to_vault(&vault, note_path)
                .ok_or_else(|| format!("note is outside the vault: {note_path}"))?;
        }
        host.log_info(&format!("run-task {task_id}"));
        self.start(
            host,
            relayed_start_params(context, task_id, prompt, note_path),
            "relay",
        )
    }

    /// Where a run stands, for the window's progress display. Reads the lock,
    /// the progress snapshot and the record — all on disk, so a run started by
    /// another process reports just as accurately as one we started.
    fn run_status(&self, context: &Value) -> Result<Value, String> {
        let vault = self.vault()?;
        let task_id = context
            .get("task")
            .and_then(|v| v.as_str())
            .unwrap_or(NOTE_TASK);
        let run_id = context
            .get("run_id")
            .and_then(|v| v.as_str())
            .ok_or("run-status needs a 'run_id'")?;
        // Same fence as `start`: this id is joined onto the runs root.
        check_task_id(task_id)?;
        check_run_id(run_id)?;
        let run_dir = task::runs_root(&vault).join(task_id);

        if let Some(rec) = record::find(&run_dir, run_id) {
            let terminal_result = record::read_terminal_result(&run_dir, run_id)
                .map_err(|error| format!("read complete terminal result: {error}"))?
                .map(|content| json!({ "content": content, "complete": true }));
            return Ok(json!({
                "state": "done",
                "record": rec,
                "terminal_result": terminal_result,
            }));
        }
        if let Some(h) = lock::current_for_run(&run_dir, run_id) {
            let p = record::read_progress_for(&run_dir, run_id);
            return Ok(json!({
                "state": "running",
                "started_at": h.started_at,
                "steps": p.as_ref().map(|p| p.steps).unwrap_or(0),
                "last": p.map(|p| p.last).unwrap_or_default(),
            }));
        }
        if self.inner.lock().unwrap().running.contains_key(run_id) {
            return Ok(json!({
                "state": "running",
                "started_at": "",
                "steps": 0,
                "last": "starting",
            }));
        }
        // No record and no live lock: the process died without writing one.
        Ok(json!({ "state": "lost" }))
    }

    /// Is Claude Code usable, and which one is it?
    ///
    /// `--version` is cheap and truthful about presence. It says nothing about
    /// credentials — an expired OAuth session versions itself perfectly happily
    /// and then fails every run — so the last run's environment-level failure
    /// rides along. We report what we observed rather than provoking a billed
    /// model call to find out.
    fn harness_status(&self) -> Result<Value, String> {
        let Some(bin) = discover::discover(std::env::var("NOTEMD_CLAUDE_BIN").ok().as_deref())
        else {
            let mut status = agent_run_core::HarnessStatus::missing(
                HARNESS_NAME,
                "安装 Claude Code(`npm i -g @anthropic-ai/claude-code`),或用 NOTEMD_CLAUDE_BIN 指定路径。",
            );
            status.capabilities = Some(harness_capabilities(None, false));
            return Ok(serde_json::to_value(status).unwrap());
        };
        // Scoped to OUR runs: both agent plugins share one runs root, so an
        // unfiltered read would show the other harness's expired credential here.
        let warning = self
            .vault()
            .ok()
            .and_then(|v| harness::recent_environment_warning(&task::runs_root(&v), SELF_PLUGIN_ID));
        // A binary that answers `--version` with a failure is present but not
        // usable; calling it ready would send the user off to debug their task.
        // The same enriched PATH the run gets: `claude` is a node shim too, and a
        // GUI-launched host inherits a PATH with no node in it.
        let path_env = discover::runtime_path();
        let (ok, version, hint) =
            match harness::probe_version(&bin, &[], &path_env, VERSION_PROBE_TIMEOUT) {
            harness::Probe::Version(v) => (true, Some(v), None),
            harness::Probe::Failed(why) => (false, None, Some(why)),
            // It is on disk and executable but said nothing. Not evidence of a
            // problem — plenty of runs have started from exactly this state.
            harness::Probe::Unavailable => (true, None, None),
        };
        Ok(serde_json::to_value(agent_run_core::HarnessStatus {
            harness: HARNESS_NAME.to_string(),
            ok,
            version,
            origin: bin.to_string_lossy().into_owned(),
            // Claude Code resolves its own model from the user's settings unless
            // a task pins one, so naming a model here would be a guess.
            default_model: None,
            hint,
            warning,
            capabilities: Some(harness_capabilities(None, ok)),
        })
        .unwrap())
    }

    /// The CLI entry point. Detached by default; `--wait` runs inline.
    fn cli_run(&mut self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let task_id = cli_str(context, "task")
            .ok_or("usage: notemd agent <task> [-p PROMPT] [--wait]")?;
        let p = cli_str(context, "prompt").unwrap_or_default();
        let wait = cli_flag(context, "wait");
        if wait {
            self.start(
                host,
                json!({
                    "task": task_id,
                    "prompt": p,
                    "use_context": false,
                    "usage_display": context
                        .get("usage_display")
                        .cloned()
                        .unwrap_or(Value::Null),
                }),
                "cli",
            )
        } else {
            runner::spawn_detached(&self.vault()?, &task_id, &p)
        }
    }

    fn run_cancel(&self, context: &Value) -> Result<Value, String> {
        let vault = self.vault()?;
        let task_id = context.get("task").and_then(Value::as_str).unwrap_or(NOTE_TASK);
        let run_id = context.get("run_id").and_then(Value::as_str)
            .ok_or("run-cancel needs a 'run_id'")?;
        check_task_id(task_id)?;
        check_run_id(run_id)?;
        if let Some(sender) = self.inner.lock().unwrap().running.get(run_id).cloned() {
            let _ = sender.try_send(());
            return Ok(json!({ "ok": true, "state": "cancelling" }));
        }
        let run_dir = task::runs_root(&vault).join(task_id);
        match record::find(&run_dir, run_id) {
            Some(record) => Ok(json!({ "ok": true, "state": "done", "status": record.status })),
            None => Ok(json!({ "ok": true, "state": "lost" })),
        }
    }
}

/// The host's frontend parses CLI args and injects them into `context`; the
/// exact shape has varied, so look in every place it has lived.
fn cli_str(context: &Value, key: &str) -> Option<String> {
    for ptr in [
        format!("/cli/args/{key}"),
        format!("/cli/flags/{key}"),
        format!("/cli/{key}"),
        format!("/{key}"),
    ] {
        if let Some(s) = context.pointer(&ptr).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn cli_flag(context: &Value, key: &str) -> bool {
    for ptr in [
        format!("/cli/flags/{key}"),
        format!("/cli/{key}"),
        format!("/{key}"),
    ] {
        match context.pointer(&ptr) {
            Some(Value::Bool(b)) => return *b,
            Some(Value::String(s)) => return !s.is_empty() && s != "false",
            _ => {}
        }
    }
    false
}

fn check_run_id(id: &str) -> Result<(), String> {
    if !id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        Ok(())
    } else {
        Err(format!("invalid run id '{id}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_run_core::task::valid_task_id;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// NOTEMD_SHARED_CONFIG is process-global, so the tests that set it have to
    /// take turns.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Read a line from the plugin until one satisfies `want`, or time out.
    async fn await_response(
        from_plugin: tokio::io::DuplexStream,
        want: impl Fn(&Value) -> bool,
        whose: &str,
    ) -> Value {
        let mut lines = BufReader::new(from_plugin).lines();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if want(&v) {
                        return v;
                    }
                }
            }
            panic!("the plugin closed its stdout without answering {whose}");
        })
        .await
        .unwrap_or_else(|_| panic!("{whose} went unanswered"))
    }

    /// `plugin_v2_execute` activates the plugin and runs the command right
    /// after, so a command must NOT have to wait for the host's vault answer.
    /// Here the host never answers `host.vault.info`; `run-status` must still
    /// resolve the vault (from the shared config) instead of failing with
    /// "no vault configured".
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_command_right_after_activation_already_has_a_vault() {
        let _env = env_guard();
        let vault = tempfile::tempdir().unwrap();
        let cfg = tempfile::tempdir().unwrap().path().join("config.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(
            &cfg,
            format!(r#"{{"version":1,"sotvault":"{}"}}"#, vault.path().display()),
        )
        .unwrap();
        std::env::set_var("NOTEMD_SHARED_CONFIG", &cfg);

        let (mut to_plugin, plugin_stdin) = tokio::io::duplex(16 * 1024);
        let (plugin_stdout, from_plugin) = tokio::io::duplex(16 * 1024);
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(sdk::serve_io(
                    ClaudeAgentPlugin::new(),
                    plugin_stdin,
                    plugin_stdout,
                ));
        });

        to_plugin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$activate\",\"params\":{\"event\":\"onCommand:run-status\"}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"command.execute\",\"params\":{\"command\":\"run-status\",\"context\":{\"run_id\":\"R1\"}}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ui.request\",\"params\":{\"method\":\"tasks.list\",\"params\":{}}}\n",
            )
            .await
            .unwrap();

        let answered = await_response(
            from_plugin,
            |v| {
                let id = v.get("id").and_then(|i| i.as_u64());
                id == Some(2) || id == Some(3)
            },
            "the first command",
        )
        .await;
        assert!(
            answered.get("error").is_none(),
            "a command right after activation failed: {answered}"
        );
        if answered["id"] == 2 {
            // No such run yet — but the vault resolved, which is the point.
            assert_eq!(answered["result"]["state"], "lost");
        }

        // The templates are on disk already, not queued behind the host's
        // answer — otherwise `run-note` fails with "unknown task".
        let ids: Vec<String> = task::discover(vault.path())
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "ai-read-ebook",
                NOTE_TASK,
                "governed-document-review",
                "search-answer",
                "search-plan",
                "search-summary",
                "selfcheck",
                "vault-research"
            ]
        );
        std::env::remove_var("NOTEMD_SHARED_CONFIG");
    }

    /// The host relays `host.agent.run` straight into this command; the one
    /// hard requirement is a task id to run — same wording contract as
    /// `start`'s own 'task' check, so callers can match on it either way.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_task_requires_a_task_id() {
        let _env = env_guard();
        let vault = tempfile::tempdir().unwrap();
        let cfg = tempfile::tempdir().unwrap().path().join("config.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(
            &cfg,
            format!(r#"{{"version":1,"sotvault":"{}"}}"#, vault.path().display()),
        )
        .unwrap();
        std::env::set_var("NOTEMD_SHARED_CONFIG", &cfg);

        let (mut to_plugin, plugin_stdin) = tokio::io::duplex(16 * 1024);
        let (plugin_stdout, from_plugin) = tokio::io::duplex(16 * 1024);
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(sdk::serve_io(
                    ClaudeAgentPlugin::new(),
                    plugin_stdin,
                    plugin_stdout,
                ));
        });

        to_plugin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$activate\",\"params\":{\"event\":\"onCommand:run-task\"}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"command.execute\",\"params\":{\"command\":\"run-task\",\"context\":{\"prompt\":\"x\"}}}\n",
            )
            .await
            .unwrap();

        let answered = await_response(
            from_plugin,
            |v| v.get("id").and_then(|i| i.as_u64()) == Some(2),
            "run-task",
        )
        .await;
        let err = answered["error"]["message"].as_str().unwrap_or_default();
        assert!(err.contains("'task'"), "err: {err}");
        std::env::remove_var("NOTEMD_SHARED_CONFIG");
    }

    /// `host.agent.run` is capability-gated but open to any plugin declaring
    /// `agent` — a task with a scoped policy (settings.scoped.json, like
    /// answer-note-question) turns a caller-named `note_path` into a Read
    /// allowlist entry. Same guard as run-note: a path outside the vault must
    /// be refused before it ever reaches `start`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_task_refuses_a_note_path_outside_the_vault() {
        let _env = env_guard();
        let vault = tempfile::tempdir().unwrap();
        let cfg = tempfile::tempdir().unwrap().path().join("config.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(
            &cfg,
            format!(r#"{{"version":1,"sotvault":"{}"}}"#, vault.path().display()),
        )
        .unwrap();
        std::env::set_var("NOTEMD_SHARED_CONFIG", &cfg);

        // A real file that exists, but sits OUTSIDE the vault.
        let outside = tempfile::tempdir().unwrap();
        let note = outside.path().join("secret.note.md");
        std::fs::write(&note, "x").unwrap();

        let (mut to_plugin, plugin_stdin) = tokio::io::duplex(16 * 1024);
        let (plugin_stdout, from_plugin) = tokio::io::duplex(16 * 1024);
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(sdk::serve_io(
                    ClaudeAgentPlugin::new(),
                    plugin_stdin,
                    plugin_stdout,
                ));
        });

        let req = format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$activate\",\"params\":{{\"event\":\"onCommand:run-task\"}}}}\n\
             {{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"command.execute\",\"params\":{{\"command\":\"run-task\",\"context\":{{\"task\":\"selfcheck\",\"note_path\":{}}}}}}}\n",
            serde_json::to_string(&note.to_string_lossy().to_string()).unwrap(),
        );
        to_plugin.write_all(req.as_bytes()).await.unwrap();

        let answered = await_response(
            from_plugin,
            |v| v.get("id").and_then(|i| i.as_u64()) == Some(2),
            "run-task",
        )
        .await;
        let err = answered["error"]["message"].as_str().unwrap_or_default();
        assert!(
            err.contains("outside the vault"),
            "err: {err}"
        );
        std::env::remove_var("NOTEMD_SHARED_CONFIG");
    }

    #[test]
    fn a_deleted_builtin_task_rebuilds_itself_on_demand() {
        let v = tempfile::tempdir().unwrap();
        task::seed_builtin_templates(v.path());
        std::fs::remove_dir_all(task::task_dir(v.path(), NOTE_TASK)).unwrap();
        assert!(task::read_task(&task::task_dir(v.path(), NOTE_TASK)).is_none());

        let got = load_task(v.path(), NOTE_TASK).expect("a built-in rebuilds itself");
        assert!(!got.prompt.is_empty());
        assert!(task::task_dir(v.path(), NOTE_TASK).join("CLAUDE.md").exists());
    }

    #[test]
    fn load_task_still_reports_a_task_that_was_never_built_in() {
        let v = tempfile::tempdir().unwrap();
        assert_eq!(load_task(v.path(), "no-such-task"), None);
    }

    /// The protocol loop must stay responsive while the vault lookup is in
    /// flight. The SDK dispatches `$activate` synchronously ON the read loop,
    /// and a `host.*` response can only be routed BY that loop — so awaiting
    /// one inside activate deadlocks the plugin for the host's whole request
    /// timeout, which the user sees as an empty task list and a dead Run
    /// button. Here the host deliberately NEVER answers `host.vault.info`;
    /// `tasks.list` must still come back.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn activate_never_blocks_the_protocol_loop() {
        // This drives the REAL plugin, whose shared-config seed would otherwise
        // read the developer's own config and seed templates into their live
        // vault. `_env` serializes the tests that set this global.
        let _env = env_guard();
        std::env::set_var("NOTEMD_SHARED_CONFIG", "/nonexistent/claude-agent-test.json");

        let (mut to_plugin, plugin_stdin) = tokio::io::duplex(16 * 1024);
        let (plugin_stdout, from_plugin) = tokio::io::duplex(16 * 1024);
        // The plugin gets its OWN runtime on its OWN thread: a regression here
        // wedges that runtime entirely, and we still want this test to fail in
        // seconds rather than hang CI.
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(sdk::serve_io(
                    ClaudeAgentPlugin::new(),
                    plugin_stdin,
                    plugin_stdout,
                ));
        });

        to_plugin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$activate\",\"params\":{\"event\":\"onCommand:open\"}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ui.request\",\"params\":{\"method\":\"tasks.list\",\"params\":{}}}\n",
            )
            .await
            .unwrap();

        let mut lines = BufReader::new(from_plugin).lines();
        let answered = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Ok(Some(line)) = lines.next_line().await {
                let v: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if v.get("id").and_then(|i| i.as_u64()) == Some(2) && v.get("result").is_some() {
                    return v;
                }
            }
            panic!("the plugin closed its stdout without answering tasks.list");
        })
        .await
        .expect("tasks.list went unanswered — activate blocked the read loop");

        // Vault unresolved (we never answered), so: no tasks, and not ready.
        assert_eq!(answered["result"]["ready"], false);
        assert_eq!(answered["result"]["tasks"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn reads_cli_args_from_the_nested_shape() {
        let c = json!({ "cli": { "args": { "task": "selfcheck" },
                                 "flags": { "prompt": "go", "wait": true } } });
        assert_eq!(cli_str(&c, "task").as_deref(), Some("selfcheck"));
        assert_eq!(cli_str(&c, "prompt").as_deref(), Some("go"));
        assert!(cli_flag(&c, "wait"));
    }

    #[test]
    fn reads_cli_args_from_a_flattened_shape() {
        let c = json!({ "task": "sweep", "prompt": "go", "wait": false });
        assert_eq!(cli_str(&c, "task").as_deref(), Some("sweep"));
        assert!(!cli_flag(&c, "wait"));
    }

    #[test]
    fn missing_cli_args_read_as_absent_rather_than_empty_strings() {
        let c = json!({ "cli": { "args": { "task": "" } } });
        assert_eq!(cli_str(&c, "task"), None);
        assert!(!cli_flag(&c, "wait"));
    }

    #[test]
    fn overview_reports_status_from_disk_not_from_memory() {
        let v = tempfile::tempdir().unwrap();
        task::seed_builtin_templates(v.path());
        let sweep_runs = task::runs_root(v.path()).join("answer-note-question");

        // A run recorded by SOME process (a detached CLI runner, say).
        record::write(
            &sweep_runs,
            &record::RunRecord {
                run_id: "20260730T000001Z-a".into(),
                task: "answer-note-question".into(),
                trigger: "cli".into(),
                started_at: "s".into(),
                ended_at: "e".into(),
                status: record::Status::Success,
                exit_code: Some(0),
                num_turns: Some(1),
                session_id: None,
                result: "ok".into(),
                stderr_tail: String::new(),
                artifacts: Vec::new(),
                harness: Some(SELF_PLUGIN_ID.into()),
                usage: None,
            },
        )
        .unwrap();
        // …and a live lock held by a process that really exists (us).
        let _held = lock::acquire(
            &sweep_runs,
            lock::LockInfo {
                pid: std::process::id() as i32,
                run_id: "20260730T000002Z-b".into(),
                started_at: "2026-07-30T00:00:02Z".into(),
            },
        )
        .unwrap();

        let got = overview(v.path());
        let sweep = got.iter().find(|t| t.def.id == "answer-note-question").unwrap();
        assert!(sweep.running);
        assert_eq!(sweep.running_since.as_deref(), Some("2026-07-30T00:00:02Z"));
        assert_eq!(sweep.last_run.as_ref().unwrap().result, "ok");

        let idle = got.iter().find(|t| t.def.id == "selfcheck").unwrap();
        assert!(!idle.running);
        assert!(idle.last_run.is_none());
    }

    #[test]
    fn overview_serializes_the_task_fields_flat() {
        let v = tempfile::tempdir().unwrap();
        task::seed_builtin_templates(v.path());
        let json = serde_json::to_value(overview(v.path())).unwrap();
        let first = &json[0];
        assert_eq!(first["id"], "ai-read-ebook");
        assert!(first["name"].is_string());
        assert_eq!(first["running"], false);
    }

    #[test]
    fn a_note_inside_the_vault_resolves_to_a_relative_path() {
        let v = tempfile::tempdir().unwrap();
        let note = v.path().join("docs/a.note.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, "x").unwrap();
        assert_eq!(
            note_relative_to_vault(v.path(), note.to_str().unwrap()).as_deref(),
            Some("docs/a.note.md")
        );
        assert_eq!(
            note_relative_to_vault(v.path(), "docs/a.note.md").as_deref(),
            Some("docs/a.note.md")
        );
    }

    #[test]
    fn a_note_outside_the_vault_is_refused() {
        let v = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let note = outside.path().join("secret.note.md");
        std::fs::write(&note, "x").unwrap();
        assert_eq!(note_relative_to_vault(v.path(), note.to_str().unwrap()), None);
        assert_eq!(note_relative_to_vault(v.path(), "../escape.note.md"), None);
        assert_eq!(note_relative_to_vault(v.path(), "gone.note.md"), None);
    }

    #[test]
    fn run_status_reports_running_then_done() {
        let v = tempfile::tempdir().unwrap();
        let p = ClaudeAgentPlugin::new();
        p.inner.lock().unwrap().vault = Some(v.path().to_path_buf());
        let run_dir = task::runs_root(v.path()).join(NOTE_TASK);
        let ctx = json!({ "task": NOTE_TASK, "run_id": "R1" });

        // Nothing on disk yet: the run is unaccounted for, not "running".
        assert_eq!(p.run_status(&ctx).unwrap()["state"], "lost");

        // A live lock plus a snapshot: running, with what it's doing.
        let _held = lock::acquire(
            &run_dir,
            lock::LockInfo {
                pid: std::process::id() as i32,
                run_id: "R1".into(),
                started_at: "2026-07-31T00:00:00Z".into(),
            },
        )
        .unwrap();
        record::write_progress(
            &run_dir,
            &record::Progress {
                run_id: "R1".into(),
                steps: 3,
                last: "Read a.note.md".into(),
                updated_at: "2026-07-31T00:00:03Z".into(),
            },
        );
        let running = p.run_status(&ctx).unwrap();
        assert_eq!(running["state"], "running");
        assert_eq!(running["steps"], 3);
        assert_eq!(running["last"], "Read a.note.md");

        // Once the record lands it wins, lock or no lock.
        record::write_terminal_result(&run_dir, "R1", "complete machine result").unwrap();
        record::write(
            &run_dir,
            &record::RunRecord {
                run_id: "R1".into(),
                task: NOTE_TASK.into(),
                trigger: "note".into(),
                started_at: "s".into(),
                ended_at: "e".into(),
                status: record::Status::Success,
                exit_code: Some(0),
                num_turns: Some(4),
                session_id: None,
                result: "answered 2".into(),
                stderr_tail: String::new(),
                artifacts: vec!["answers/a.md".into()],
                harness: Some(SELF_PLUGIN_ID.into()),
                usage: None,
            },
        )
        .unwrap();
        let done = p.run_status(&ctx).unwrap();
        assert_eq!(done["state"], "done");
        assert_eq!(done["record"]["result"], "answered 2");
        assert_eq!(done["terminal_result"]["complete"], true);
        assert_eq!(done["terminal_result"]["content"], "complete machine result");
        assert_eq!(done["record"]["artifacts"][0], "answers/a.md");
        assert_eq!(p.run_cancel(&ctx).unwrap()["state"], "done");
        assert_eq!(p.run_cancel(&ctx).unwrap()["state"], "done");
    }

    #[test]
    fn run_status_needs_a_run_id() {
        let v = tempfile::tempdir().unwrap();
        let p = ClaudeAgentPlugin::new();
        p.inner.lock().unwrap().vault = Some(v.path().to_path_buf());
        assert!(p.run_status(&json!({ "task": NOTE_TASK })).is_err());
    }

    #[test]
    fn reads_the_vault_out_of_the_shared_config() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(
            &p,
            r#"{"version":1,"sotvault":"/Users/x/git/sotvault","rawvault":"/Users/x/git/rawvault"}"#,
        )
        .unwrap();
        assert_eq!(
            shared_config_vault_at(&p),
            Some(PathBuf::from("/Users/x/git/sotvault"))
        );
    }

    #[test]
    fn shared_config_without_a_usable_vault_reads_as_none() {
        let d = tempfile::tempdir().unwrap();
        let missing = d.path().join("nope.json");
        assert_eq!(shared_config_vault_at(&missing), None);

        let empty = d.path().join("empty.json");
        std::fs::write(&empty, r#"{"version":1,"sotvault":""}"#).unwrap();
        assert_eq!(shared_config_vault_at(&empty), None);

        let absent = d.path().join("absent.json");
        std::fs::write(&absent, r#"{"version":1}"#).unwrap();
        assert_eq!(shared_config_vault_at(&absent), None);

        let broken = d.path().join("broken.json");
        std::fs::write(&broken, "{not json").unwrap();
        assert_eq!(shared_config_vault_at(&broken), None);
    }

    /// A task directory is the run's permission policy. An id that can leave
    /// `.notemd/agent-tasks/` would let a caller (any plugin holding `agent`)
    /// point that policy at a directory it planted — `.claude/settings.json`
    /// there could allow `Bash`.
    #[test]
    fn a_task_id_may_only_name_one_directory() {
        for good in ["selfcheck", "ai-read-ebook", "答疑", "a.b", "a..b"] {
            assert!(valid_task_id(good), "{good} must be allowed");
        }
        for bad in [
            "",
            "..",
            ".",
            "../evil",
            "../../etc",
            "a/b",
            "/abs/path",
            "./a",
            "a\\b",
            "..\\evil",
        ] {
            assert!(!valid_task_id(bad), "{bad} must be refused");
        }
    }

    #[test]
    fn run_status_refuses_a_traversing_task_id() {
        let v = tempfile::tempdir().unwrap();
        let p = ClaudeAgentPlugin::new();
        p.inner.lock().unwrap().vault = Some(v.path().to_path_buf());
        let e = p
            .run_status(&json!({ "task": "../../evil", "run_id": "R1" }))
            .unwrap_err();
        assert!(e.contains("invalid task id"), "err: {e}");
        // history.* share the same joined-root shape.
        let e = p
            .runs_root_and_task(&json!({ "task": "../../evil" }))
            .unwrap_err();
        assert!(e.contains("invalid task id"), "err: {e}");
    }

    /// The host relay is the reachable-from-another-plugin door, so pin it
    /// end-to-end rather than only at the helper.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_task_refuses_a_traversing_task_id() {
        let _env = env_guard();
        let vault = tempfile::tempdir().unwrap();
        let cfg = tempfile::tempdir().unwrap().path().join("config.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(
            &cfg,
            format!(r#"{{"version":1,"sotvault":"{}"}}"#, vault.path().display()),
        )
        .unwrap();
        std::env::set_var("NOTEMD_SHARED_CONFIG", &cfg);

        let (mut to_plugin, plugin_stdin) = tokio::io::duplex(16 * 1024);
        let (plugin_stdout, from_plugin) = tokio::io::duplex(16 * 1024);
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(sdk::serve_io(
                    ClaudeAgentPlugin::new(),
                    plugin_stdin,
                    plugin_stdout,
                ));
        });

        to_plugin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$activate\",\"params\":{\"event\":\"onCommand:run-task\"}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"command.execute\",\"params\":{\"command\":\"run-task\",\"context\":{\"task\":\"../../evil\",\"prompt\":\"p\"}}}\n",
            )
            .await
            .unwrap();

        let answered = await_response(
            from_plugin,
            |v| v.get("id").and_then(|i| i.as_u64()) == Some(2),
            "run-task",
        )
        .await;
        let err = answered["error"]["message"].as_str().unwrap_or_default();
        assert!(err.contains("invalid task id"), "err: {err}");
        std::env::remove_var("NOTEMD_SHARED_CONFIG");
    }

    fn a_record(status: record::Status) -> record::RunRecord {
        record::RunRecord {
            run_id: "R1".into(),
            task: "ai-read-ebook".into(),
            trigger: "relay".into(),
            started_at: "s".into(),
            ended_at: "e".into(),
            status,
            exit_code: Some(0),
            num_turns: Some(1),
            session_id: None,
            result: String::new(),
            stderr_tail: String::new(),
            artifacts: Vec::new(),
            harness: Some(SELF_PLUGIN_ID.into()),
            usage: None,
        }
    }

    #[test]
    fn usage_display_defaults_to_tip_and_validates_explicit_values() {
        assert_eq!(UsageDisplay::from_params(&json!({})), Ok(UsageDisplay::Tip));
        assert_eq!(
            UsageDisplay::from_params(&json!({ "usage_display": "tip" })),
            Ok(UsageDisplay::Tip)
        );
        assert_eq!(
            UsageDisplay::from_params(&json!({ "usage_display": "result" })),
            Ok(UsageDisplay::Result)
        );
        assert!(UsageDisplay::from_params(&json!({ "usage_display": "elsewhere" })).is_err());
    }

    #[test]
    fn invocation_model_profiles_and_exact_models_resolve_for_claude() {
        let vault = tempfile::tempdir().unwrap();
        task::seed_builtin_templates(vault.path());
        let mut def = load_task(vault.path(), "selfcheck").unwrap();
        def.model = Some("task-pin".into());

        apply_invocation_model(&mut def, &json!({})).unwrap();
        assert_eq!(def.model.as_deref(), Some("task-pin"));
        apply_invocation_model(&mut def, &json!({ "model_profile": "default" })).unwrap();
        assert_eq!(
            def.model, None,
            "explicit default must bypass a task.json model pin"
        );
        apply_invocation_model(&mut def, &json!({ "model_profile": "fast" })).unwrap();
        assert_eq!(def.model.as_deref(), Some("haiku"));
        apply_invocation_model(&mut def, &json!({ "model": "  claude-opus-5  " })).unwrap();
        assert_eq!(def.model.as_deref(), Some("claude-opus-5"));

        assert!(apply_invocation_model(&mut def, &json!({
            "model_profile": "fast",
            "model": "claude-opus-5"
        }))
        .unwrap_err()
        .contains("mutually exclusive"));
        assert!(apply_invocation_model(&mut def, &json!({ "model_profile": "cheap" })).is_err());
    }

    #[test]
    fn run_task_relay_keeps_invocation_model_controls() {
        let context = json!({
            "model_profile": "fast",
            "usage_display": "result",
            "notify": { "marker": true }
        });
        let params = relayed_start_params(&context, "search-plan", "packet", "");
        assert_eq!(params["task"], "search-plan");
        assert_eq!(params["model_profile"], "fast");
        assert_eq!(params["model"], Value::Null);
        assert_eq!(params["usage_display"], "result");
        assert_eq!(params["notify"]["marker"], true);

        let exact = relayed_start_params(
            &json!({ "model": "claude-sonnet-5" }),
            "search-answer",
            "packet",
            "",
        );
        assert_eq!(exact["model"], "claude-sonnet-5");
        assert_eq!(exact["model_profile"], Value::Null);
    }

    #[test]
    fn search_capabilities_advertise_lookup_summary_research_and_legacy_answer() {
        let capabilities = harness_capabilities(None, true);
        assert_eq!(
            capabilities.tasks,
            vec![
                "governed-document-review",
                "search-plan",
                "search-answer",
                "search-summary",
                "vault-research",
            ]
        );
        assert_eq!(capabilities.search_plan_schemas, vec![1]);
        assert!(capabilities.terminal_result);
        assert!(capabilities.input_only_isolation);
        assert!(capabilities.model_routing.invocation_override);
        assert_eq!(
            capabilities.model_routing.profiles.fast.model.as_deref(),
            Some("haiku")
        );
        assert!(capabilities.model_routing.profiles.fast.available);
        assert_eq!(capabilities.model_routing.profiles.default_profile.model, None);
        assert!(capabilities.model_routing.selectable_models.is_empty());
    }

    #[test]
    fn tip_mode_formats_usage_and_result_mode_stays_silent() {
        let mut rec = a_record(record::Status::Success);
        rec.usage = Some(agent_run_core::usage::Usage {
            input_tokens: 10,
            output_tokens: 5,
            cost: Some(agent_run_core::usage::Cost {
                amount_usd: 0.001,
                kind: agent_run_core::usage::CostKind::ProviderReported,
                pricing_as_of: None,
            }),
            ..Default::default()
        });
        let tip = usage_tip(UsageDisplay::Tip, &rec).expect("tip mode should emit");
        assert!(tip.contains("15 tokens"), "{tip}");
        assert!(tip.contains("$0.001000"), "{tip}");
        assert_eq!(usage_tip(UsageDisplay::Result, &rec), None);
    }

    /// The reminder is only a success when the run said so AND the promised
    /// file is really there — that existence check used to live in the caller,
    /// which is exactly the code that dies when its window closes.
    #[test]
    fn a_run_only_delivered_when_the_expected_file_exists() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("2026-08-04-summary.md");
        let spec = NotifySpec {
            title_ok: "ok".into(),
            title_fail: "fail".into(),
            open_path: f.to_string_lossy().to_string(),
            expect_file: f.to_string_lossy().to_string(),
        };
        // success record, no file yet
        assert!(!run_delivered(&spec, Some(&a_record(record::Status::Success))));
        std::fs::write(&f, "# x").unwrap();
        assert!(run_delivered(&spec, Some(&a_record(record::Status::Success))));
        // file there, but the run failed / never produced a record
        assert!(!run_delivered(&spec, Some(&a_record(record::Status::Error))));
        assert!(!run_delivered(&spec, Some(&a_record(record::Status::Timeout))));
        assert!(!run_delivered(&spec, None));
    }

    #[test]
    fn notify_params_point_at_the_file_or_at_our_own_run_log() {
        let spec = NotifySpec {
            title_ok: "《书》AI 摘要已生成".into(),
            title_fail: "《书》AI 阅读失败".into(),
            open_path: "/v/ssot/ebooks/b/2026-08-04-summary.md".into(),
            expect_file: "/v/ssot/ebooks/b/2026-08-04-summary.md".into(),
        };
        let ok = notify_params(&spec, true);
        assert_eq!(ok["title"], "《书》AI 摘要已生成");
        assert_eq!(ok["action"]["kind"], "open_path");
        assert_eq!(ok["action"]["path"], "/v/ssot/ebooks/b/2026-08-04-summary.md");
        // 成功不带 severity(宿主默认 Info/蓝点)。
        assert!(ok.get("severity").is_none());

        let bad = notify_params(&spec, false);
        assert_eq!(bad["title"], "《书》AI 阅读失败");
        assert_eq!(bad["action"]["kind"], "open_plugin_window");
        assert_eq!(bad["action"]["plugin_id"], SELF_PLUGIN_ID);
        assert_eq!(bad["action"]["window"], WINDOW);
        // 失败标 Warn(宿主黄点)。
        assert_eq!(bad["severity"], "warn");
    }

    /// A caller that garbles the spec has to hear about it — silently dropping
    /// it would leave it waiting for a reminder that can never arrive.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_task_rejects_a_malformed_notify_spec() {
        let _env = env_guard();
        let vault = tempfile::tempdir().unwrap();
        let cfg = tempfile::tempdir().unwrap().path().join("config.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(
            &cfg,
            format!(r#"{{"version":1,"sotvault":"{}"}}"#, vault.path().display()),
        )
        .unwrap();
        std::env::set_var("NOTEMD_SHARED_CONFIG", &cfg);

        let (mut to_plugin, plugin_stdin) = tokio::io::duplex(16 * 1024);
        let (plugin_stdout, from_plugin) = tokio::io::duplex(16 * 1024);
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(sdk::serve_io(
                    ClaudeAgentPlugin::new(),
                    plugin_stdin,
                    plugin_stdout,
                ));
        });

        to_plugin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$activate\",\"params\":{\"event\":\"onCommand:run-task\"}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"command.execute\",\"params\":{\"command\":\"run-task\",\"context\":{\"task\":\"selfcheck\",\"notify\":{\"title_ok\":\"a\"}}}}\n",
            )
            .await
            .unwrap();

        let answered = await_response(
            from_plugin,
            |v| v.get("id").and_then(|i| i.as_u64()) == Some(2),
            "run-task",
        )
        .await;
        let err = answered["error"]["message"].as_str().unwrap_or_default();
        assert!(err.contains("notify"), "err: {err}");
        std::env::remove_var("NOTEMD_SHARED_CONFIG");
    }

    #[test]
    fn tab_context_needs_a_path_to_be_usable() {
        let mut p = ClaudeAgentPlugin::new();
        p.tab_context = Some(json!({ "selection": "sel" }));
        assert_eq!(p.tab_ctx(), None);
        p.tab_context = Some(json!({ "path": "/v/a.md", "selection": "sel" }));
        assert_eq!(
            p.tab_ctx(),
            Some(prompt::TabContext {
                path: "/v/a.md".into(),
                selection: "sel".into()
            })
        );
    }
}
