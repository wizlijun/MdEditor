//! The NotemdPlugin implementation: the window's UI methods plus the menu and
//! CLI commands.
//!
//! The SDK calls `on_ui_request` and `activate` synchronously ON the protocol
//! read loop, and a `host.*` response can only be routed BY that loop — so
//! nothing here may await a host round-trip. `run.start` spawns a tokio task and
//! returns the run id immediately; blocking would wedge the whole plugin.
use crate::{composition, discover, engine, policy, runner, task};
use agent_run_core::scaffold::RunMeta;
use agent_run_core::task::check_task_id;
use agent_run_core::{harness, lock, prompt, record};
use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const WINDOW: &str = "main";
const NO_VAULT: &str = "no vault configured";
/// The task the main window's Agent workspace runs.
const NOTE_TASK: &str = "answer-note-question";
use crate::SELF_PLUGIN_ID;
/// The harness behind this plugin, as the window names it.
const HARNESS_NAME: &str = "DeepSeek Harness";
/// A version probe runs while the window waits, so it is bounded tightly. A
/// checkout launcher goes through pnpm, which is slower to start than a bin.
const VERSION_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// The tray reminder a caller wants pushed when its run reaches a terminal
/// state. Sent from HERE rather than by the caller: a plugin with an open window
/// is torn down the moment that window closes, which would kill the caller's
/// polling task and its reminder with it.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
struct NotifySpec {
    title_ok: String,
    title_fail: String,
    /// ABSOLUTE path the success reminder opens. The host re-checks that it is
    /// inside the vault before accepting it.
    open_path: String,
    /// ABSOLUTE path that must exist for the run to count as a success — a
    /// `success` record with no file on disk is a failure to the user.
    expect_file: String,
}

fn run_delivered(spec: &NotifySpec, rec: Option<&record::RunRecord>) -> bool {
    rec.map(|r| r.status) == Some(record::Status::Success)
        && Path::new(&spec.expect_file).is_file()
}

fn notify_params(spec: &NotifySpec, delivered: bool) -> Value {
    if delivered {
        json!({
            "title": spec.title_ok,
            "action": { "kind": "open_path", "path": spec.open_path },
        })
    } else {
        json!({
            "title": spec.title_fail,
            "action": { "kind": "open_plugin_window",
                        "plugin_id": SELF_PLUGIN_ID, "window": WINDOW },
            "severity": "warn",
        })
    }
}

/// MUST be called from a spawned task, never from the protocol read loop.
async fn notify_outcome(host: &sdk::Host, spec: &NotifySpec, rec: Option<&record::RunRecord>) {
    let params = notify_params(spec, run_delivered(spec, rec));
    if let Err(e) = host.request("host.notify", params).await {
        host.log_warn(&format!("host.notify failed: {e}"));
    }
}

/// A note's path relative to the vault. Absolute paths outside the vault (and
/// traversal) return None — a task must not be pointed at a file the vault
/// doesn't own.
fn note_relative_to_vault(vault: &Path, note_path: &str) -> Option<String> {
    let root = vault.canonicalize().ok()?;
    let p = Path::new(note_path);
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
    /// Whether the plugin window is open — the difference between an `ask`
    /// policy having somebody to ask and failing closed.
    window_open: bool,
    /// run_id → cancel channel
    running: HashMap<String, mpsc::Sender<()>>,
}

pub struct DeepseekAgentPlugin {
    inner: Arc<Mutex<Inner>>,
    tab_context: Option<Value>,
}

impl DeepseekAgentPlugin {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            tab_context: None,
        }
    }
}

impl Default for DeepseekAgentPlugin {
    fn default() -> Self {
        Self::new()
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
    /// The sandbox mode this task runs under, so the window can show what it is
    /// allowed to do before you press Run.
    permission_mode: String,
    policy_rationale: String,
}

fn overview(vault: &Path) -> Vec<TaskOverview> {
    task::discover(vault)
        .into_iter()
        .map(|def| {
            let run_dir = task::runs_root(vault).join(&def.id);
            let held = lock::current(&run_dir);
            let p = policy::Policy::load(&task::task_dir(vault, &def.id)).unwrap_or_default();
            TaskOverview {
                running: held.is_some(),
                running_since: held.map(|h| h.started_at),
                last_run: record::recent(&run_dir, 1).into_iter().next(),
                permission_mode: p.permission_mode.as_env().to_string(),
                policy_rationale: p.rationale,
                def,
            }
        })
        .collect()
}

/// The vault root. The host is authoritative (`host.vault.info`), but it can
/// answer with nothing — during startup before vault_sync has initialised — so
/// retry, then fall back to the config file the host itself falls back to.
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
    Some(PathBuf::from(home).join("Library/Application Support/net.notemd.app/shared.json"))
}

fn shared_config_vault() -> Option<PathBuf> {
    shared_config_vault_at(&shared_config_path()?)
}

fn shared_config_vault_at(path: &Path) -> Option<PathBuf> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let s = v.get("sotvault")?.as_str()?;
    (!s.is_empty()).then(|| PathBuf::from(s))
}

/// Everything a vault needs before a run. All pure filesystem work, so it is
/// safe to call synchronously on the protocol read loop — and it has to be,
/// because a command can arrive the instant activation returns.
fn prepare_vault(host: &sdk::Host, root: &Path) {
    let wrote = task::seed_builtin_templates(root);
    if !wrote.is_empty() {
        host.log_info(&format!("seeded task templates: {}", wrote.join(", ")));
    }
    match composition::ensure_config(root) {
        Ok(true) => host.log_info("wrote the dsh composition to .notemd/dsh/cordis.yml"),
        Ok(false) => {}
        Err(e) => host.log_warn(&format!("could not write the dsh composition: {e}")),
    }
    task::ensure_gitignore(root);
}

/// Load a task, rebuilding a built-in whose directory has gone missing.
fn load_task(vault: &Path, id: &str) -> Option<task::TaskDef> {
    let dir = task::task_dir(vault, id);
    if let Some(t) = task::read_task(&dir) {
        return Some(t);
    }
    task::seed_builtin_templates(vault);
    task::read_task(&dir)
}

impl sdk::NotemdPlugin for DeepseekAgentPlugin {
    fn activate(&mut self, host: &sdk::Host, _p: &proto::ActivateParams) -> Result<(), String> {
        let inner = self.inner.clone();
        let host = host.clone();

        // Seed the vault SYNCHRONOUSLY from the shared config — a plain file
        // read, no host round-trip. The host activates the plugin and runs the
        // command immediately afterwards, so anything that waits for the host's
        // answer would have the first command race it.
        let seeded = shared_config_vault();
        if let Some(root) = &seeded {
            inner.lock().unwrap().vault = Some(root.clone());
            prepare_vault(&host, root);
        }

        // MUST be spawned, never awaited inline: `$activate` runs ON the
        // protocol read loop, and the response to `host.vault.info` can only be
        // routed BY that loop. Awaiting here deadlocks the plugin until the
        // host's request timeout — which the user sees as an empty task list and
        // a dead Run button.
        tokio::spawn(async move {
            let root = vault_from_host(&host).await.or(seeded);
            if let Some(root) = &root {
                prepare_vault(&host, root);
                host.log_info(&format!("deepseek-agent ready (vault={})", root.display()));
                match discover::discover(None) {
                    Some(l) => host.log_info(&format!("ACP server: {}", l.origin)),
                    None => host.log_warn(&discover::NOT_FOUND.replace('\n', " ")),
                }
            } else {
                host.log_warn("no vault configured; deepseek-agent needs one");
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
        // don't leave orphaned harness processes behind.
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
            "open" => {
                self.tab_context = params.context.get("tab").cloned();
                self.inner.lock().unwrap().window_open = true;
                Ok(json!({ "success": true }))
            }
            // CLI: notemd dsagent <task> [-p …] [--wait]
            "run" => self.cli_run(host, &params.context),
            // The main window's Agent workspace: answer the open questions in
            // ONE note, rather than sweeping the whole vault.
            "run-note" => self.run_note(host, &params.context),
            // 宿主 host.agent.run 中转:任意任务 + 调用方拼好的定位 prompt。
            "run-task" => self.run_task(host, &params.context),
            "run-status" => self.run_status(&params.context),
            // What the window shows above everything else: is the ACP server
            // there, which dsh is it, which model will it use, and did the last
            // run die of something that will kill the next one too.
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
                    Some(v) => Ok(json!({
                        "tasks": overview(&v),
                        "ready": true,
                        "harness": discover::discover(None).map(|l| l.origin),
                    })),
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
                    Some(t) if !t.is_empty() => {
                        check_task_id(t)?;
                        record::recent(&root.join(t), 30)
                    }
                    _ => record::recent_all(&root, 30),
                };
                Ok(json!({ "runs": runs }))
            }
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
            "history.clear" => {
                let vault = self.vault()?;
                let root = task::runs_root(&vault);
                let n = match params.get("task").and_then(|v| v.as_str()) {
                    Some(t) if !t.is_empty() => {
                        check_task_id(t)?;
                        record::clear(&root.join(t))
                    }
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

impl DeepseekAgentPlugin {
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
        // A malformed spec is an error rather than a silent None: the caller
        // would otherwise wait forever for a reminder that can never come.
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

        // The permission gate first: a task whose policy will not parse must not
        // run, whatever else is or isn't installed.
        let policy = policy::Policy::load(&task_dir)?;
        let launcher = discover::discover(None).ok_or(discover::NOT_FOUND)?;
        // Put the ACP bridge in our profile the first time it is needed. A
        // no-op afterwards (read from the profile manifest), so this is not a
        // package-manager round trip per run.
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
        discover::ensure_acp(&launcher.program, &home, &discover::runtime_path())?;
        let config = composition::resolve_config(&vault, None)?;

        let target = params
            .get("note_path")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ctx = if use_ctx { self.tab_ctx() } else { None };
        // The source-document paragraph is generated per run, not written into a
        // template: which documents are mirrors is per-vault state.
        let metas = agent_run_core::mirror::read_metas(&vault);
        let scope = target
            .as_deref()
            .map(|t| agent_run_core::Scope::for_note(&vault, Path::new(t), &metas));
        let full = prompt::with_source_context(
            &prompt::compose(&def.prompt, &user_prompt, ctx.as_ref()),
            &vault,
            scope.as_ref(),
        );
        let run_id = record::new_run_id(chrono::Utc::now(), std::process::id());
        let window_open = self.inner.lock().unwrap().window_open;

        let spec = engine::RunSpec {
            prompt: full,
            meta: RunMeta {
                vault: vault.clone(),
                task: def,
                task_dir,
                task_run_dir: task::runs_root(&vault).join(&task_id),
                run_id: run_id.clone(),
                trigger: trigger.to_string(),
                harness: SELF_PLUGIN_ID.to_string(),
                target,
                deliverable: notify.as_ref().map(|n| PathBuf::from(&n.expect_file)),
            },
            launcher,
            config,
            env_path: None,
            sessions_dir: composition::sessions_dir(&vault),
            policy,
            window_open,
            api_key: std::env::var("DEEPSEEK_API_KEY").ok(),
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
            let pump = {
                let h = h.clone();
                let rid = rid.clone();
                tokio::spawn(async move {
                    let mut last: Option<record::RunRecord> = None;
                    while let Some(step) = rx.recv().await {
                        match step {
                            agent_run_core::Step::Event(e) => h.ui_post(
                                WINDOW,
                                json!({ "kind": "event", "run_id": rid, "event": e }),
                            ),
                            agent_run_core::Step::Done(r) => {
                                h.ui_post(
                                    WINDOW,
                                    json!({ "kind": "done", "run_id": rid, "record": r }),
                                );
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
                    json!({ "kind": "busy", "run_id": rid, "holder": busy.run_id }),
                );
                h.toast("warn", "That task is already running", Some(&busy.run_id));
            }
            let rec = pump.await.ok().flatten();
            inner.lock().unwrap().running.remove(&rid);
            // Exactly one reminder per run that asked for one. We are inside a
            // spawned task here, the only place `host.request` may be awaited.
            if let Some(n) = notify {
                notify_outcome(&h, &n, rec.as_ref()).await;
            }
        });
        Ok(json!({ "run_id": run_id }))
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

    /// Answer the open questions in ONE sidecar note.
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
            }),
            "note",
        )
    }

    /// Run any task with a caller-composed prompt — the host relays
    /// `host.agent.run` here.
    fn run_task(&mut self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let task_id = context
            .get("task")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("run-task needs a 'task'")?;
        let prompt = context.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let note_path = context
            .get("note_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // Same guard as run-note: a caller-named note MUST resolve inside the
        // vault, or this relay becomes a way to point a run at any file on disk.
        if !note_path.is_empty() {
            let vault = self.vault()?;
            note_relative_to_vault(&vault, note_path)
                .ok_or_else(|| format!("note is outside the vault: {note_path}"))?;
        }
        host.log_info(&format!("run-task {task_id}"));
        self.start(
            host,
            json!({
                "task": task_id,
                "prompt": prompt,
                "use_context": false,
                "note_path": note_path,
                "notify": context.get("notify").cloned().unwrap_or(Value::Null),
            }),
            "relay",
        )
    }

    /// Where a run stands. Reads the lock, the progress snapshot and the record —
    /// all on disk, so a run started by another process reports just as
    /// accurately as one we started.
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
        let run_dir = task::runs_root(&vault).join(task_id);

        if let Some(rec) = record::find(&run_dir, run_id) {
            return Ok(json!({ "state": "done", "record": rec }));
        }
        if let Some(h) = lock::current(&run_dir).filter(|h| h.run_id == run_id) {
            let p = record::read_progress(&run_dir).filter(|p| p.run_id == run_id);
            return Ok(json!({
                "state": "running",
                "started_at": h.started_at,
                "steps": p.as_ref().map(|p| p.steps).unwrap_or(0),
                "last": p.map(|p| p.last).unwrap_or_default(),
            }));
        }
        // No record and no live lock: the process died without writing one.
        Ok(json!({ "state": "lost" }))
    }

    /// Is DeepSeek Harness usable, and which one is it?
    ///
    /// The version comes from the launcher itself (`dsh-acp-demo --version`, or
    /// `pnpm --dir <repo> run demo:acp --version` for a checkout). The model is
    /// read out of the composition this vault will ACTUALLY boot — not a
    /// constant here — so a user who edited their `cordis.yml` sees their own
    /// choice rather than ours.
    fn harness_status(&self) -> Result<Value, String> {
        let Some(launcher) = discover::discover(None) else {
            return Ok(serde_json::to_value(agent_run_core::HarnessStatus::missing(
                HARNESS_NAME,
                discover::NOT_FOUND,
            ))
            .unwrap());
        };
        let vault = self.vault().ok();
        let default_model = vault
            .as_deref()
            .and_then(|v| composition::resolve_config(v, None).ok())
            .and_then(|c| composition::default_model(&c));
        // Scoped to OUR runs: both agent plugins share one runs root, so an
        // unfiltered read showed Claude's expired OAuth in this window as though
        // DeepSeek were the broken one.
        let warning = vault
            .as_deref()
            .and_then(|v| harness::recent_environment_warning(&task::runs_root(v), SELF_PLUGIN_ID));
        // A checkout launcher runs through pnpm, which fails loudly on a version
        // mismatch. Present but unusable is NOT ready — reporting that failure as
        // a version put "[ERROR] This project is configured to use 11.7.0 of
        // pnpm…" where the version belongs and called the harness good to go.
        // `dsh --version` answers instantly (no profile boot, no ACP server), so
        // this is a plain probe again.
        let (ok, version, hint) =
            match harness::probe_version(&launcher.program, &[], VERSION_PROBE_TIMEOUT) {
                harness::Probe::Version(v) => (true, Some(v), None),
                harness::Probe::Failed(why) => (false, None, Some(why)),
                harness::Probe::Unavailable => (true, None, None),
            };
        Ok(serde_json::to_value(agent_run_core::HarnessStatus {
            harness: HARNESS_NAME.to_string(),
            ok,
            version,
            origin: launcher.origin,
            default_model,
            hint,
            warning,
        })
        .unwrap())
    }

    /// The CLI entry point. Detached by default; `--wait` runs inline.
    fn cli_run(&mut self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let task_id = cli_str(context, "task")
            .ok_or("usage: notemd dsagent <task> [-p PROMPT] [--wait]")?;
        let p = cli_str(context, "prompt").unwrap_or_default();
        if cli_flag(context, "wait") {
            self.start(
                host,
                json!({ "task": task_id, "prompt": p, "use_context": false }),
                "cli",
            )
        } else {
            runner::spawn_detached(&self.vault()?, &task_id, &p)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// NOTEMD_SHARED_CONFIG is process-global, so the tests that set it take turns.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn seed_config(vault: &Path) -> PathBuf {
        let cfg = std::env::temp_dir().join(format!(
            "deepseek-agent-test-{}-{}.json",
            std::process::id(),
            vault.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(
            &cfg,
            format!(r#"{{"version":1,"sotvault":"{}"}}"#, vault.display()),
        )
        .unwrap();
        cfg
    }

    fn serve() -> (tokio::io::DuplexStream, tokio::io::DuplexStream) {
        let (to_plugin, plugin_stdin) = tokio::io::duplex(16 * 1024);
        let (plugin_stdout, from_plugin) = tokio::io::duplex(16 * 1024);
        // Its OWN runtime on its OWN thread: a regression that wedges the read
        // loop should still fail this test in seconds rather than hang CI.
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(sdk::serve_io(
                    DeepseekAgentPlugin::new(),
                    plugin_stdin,
                    plugin_stdout,
                ));
        });
        (to_plugin, from_plugin)
    }

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

    /// The protocol loop must stay responsive while the vault lookup is in
    /// flight. `$activate` is dispatched synchronously ON the read loop, and a
    /// `host.*` response can only be routed BY that loop — so awaiting one
    /// inside activate deadlocks the plugin for the host's whole request
    /// timeout, which the user sees as an empty task list and a dead Run button.
    /// Here the host deliberately NEVER answers `host.vault.info`.
    ///
    /// Copied deliberately from claude-agent: same trap, same guard.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn activate_never_blocks_the_protocol_loop() {
        let _env = env_guard();
        std::env::set_var("NOTEMD_SHARED_CONFIG", "/nonexistent/deepseek-agent-test.json");
        let (mut to_plugin, from_plugin) = serve();
        to_plugin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$activate\",\"params\":{\"event\":\"onCommand:open\"}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ui.request\",\"params\":{\"method\":\"tasks.list\",\"params\":{}}}\n",
            )
            .await
            .unwrap();

        let answered = await_response(
            from_plugin,
            |v| v.get("id").and_then(|i| i.as_u64()) == Some(2) && v.get("result").is_some(),
            "tasks.list",
        )
        .await;
        // Vault unresolved (we never answered), so: no tasks, and not ready.
        assert_eq!(answered["result"]["ready"], false);
        assert_eq!(answered["result"]["tasks"].as_array().unwrap().len(), 0);
        std::env::remove_var("NOTEMD_SHARED_CONFIG");
    }

    /// The host activates the plugin and runs the command right after, so a
    /// command must NOT have to wait for the host's vault answer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_command_right_after_activation_already_has_a_vault() {
        let _env = env_guard();
        let vault = tempfile::tempdir().unwrap();
        let cfg = seed_config(vault.path());
        std::env::set_var("NOTEMD_SHARED_CONFIG", &cfg);

        let (mut to_plugin, from_plugin) = serve();
        to_plugin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$activate\",\"params\":{\"event\":\"onCommand:run-status\"}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"command.execute\",\"params\":{\"command\":\"run-status\",\"context\":{\"run_id\":\"R1\"}}}\n",
            )
            .await
            .unwrap();

        let answered = await_response(
            from_plugin,
            |v| v.get("id").and_then(|i| i.as_u64()) == Some(2),
            "the first command",
        )
        .await;
        assert!(
            answered.get("error").is_none(),
            "a command right after activation failed: {answered}"
        );
        assert_eq!(answered["result"]["state"], "lost");

        // The templates AND the composition are on disk already, not queued
        // behind the host's answer.
        let ids: Vec<String> = task::discover(vault.path())
            .into_iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, vec![NOTE_TASK, "selfcheck"]);
        assert!(composition::config_path(vault.path()).is_file());
        std::env::remove_var("NOTEMD_SHARED_CONFIG");
        let _ = std::fs::remove_file(&cfg);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_task_requires_a_task_id() {
        let _env = env_guard();
        let vault = tempfile::tempdir().unwrap();
        let cfg = seed_config(vault.path());
        std::env::set_var("NOTEMD_SHARED_CONFIG", &cfg);

        let (mut to_plugin, from_plugin) = serve();
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
        let _ = std::fs::remove_file(&cfg);
    }

    /// `host.agent.run` is open to any plugin holding the `agent` capability, so
    /// the relay is the reachable-from-another-plugin door. A traversing id
    /// would point the run's policy at a directory the caller planted.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn run_task_refuses_a_traversing_task_id() {
        let _env = env_guard();
        let vault = tempfile::tempdir().unwrap();
        let cfg = seed_config(vault.path());
        std::env::set_var("NOTEMD_SHARED_CONFIG", &cfg);

        let (mut to_plugin, from_plugin) = serve();
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
        let _ = std::fs::remove_file(&cfg);
    }

    #[test]
    fn a_note_inside_the_vault_resolves_and_one_outside_is_refused() {
        let v = tempfile::tempdir().unwrap();
        let note = v.path().join("docs/a.note.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, "x").unwrap();
        assert_eq!(
            note_relative_to_vault(v.path(), note.to_str().unwrap()).as_deref(),
            Some("docs/a.note.md")
        );

        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.note.md");
        std::fs::write(&secret, "x").unwrap();
        assert_eq!(note_relative_to_vault(v.path(), secret.to_str().unwrap()), None);
        assert_eq!(note_relative_to_vault(v.path(), "../escape.note.md"), None);
        assert_eq!(note_relative_to_vault(v.path(), "gone.note.md"), None);
    }

    #[test]
    fn run_status_reports_running_then_done() {
        let v = tempfile::tempdir().unwrap();
        let p = DeepseekAgentPlugin::new();
        p.inner.lock().unwrap().vault = Some(v.path().to_path_buf());
        let run_dir = task::runs_root(v.path()).join(NOTE_TASK);
        let ctx = json!({ "task": NOTE_TASK, "run_id": "R1" });

        assert_eq!(p.run_status(&ctx).unwrap()["state"], "lost");

        let _held = lock::acquire(
            &run_dir,
            lock::LockInfo {
                pid: std::process::id() as i32,
                run_id: "R1".into(),
                started_at: "2026-08-17T00:00:00Z".into(),
            },
        )
        .unwrap();
        record::write_progress(
            &run_dir,
            &record::Progress {
                run_id: "R1".into(),
                steps: 3,
                last: "答到第三段".into(),
                updated_at: "2026-08-17T00:00:03Z".into(),
            },
        );
        let running = p.run_status(&ctx).unwrap();
        assert_eq!(running["state"], "running");
        assert_eq!(running["steps"], 3);
        assert_eq!(running["last"], "答到第三段");

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
                num_turns: None,
                session_id: Some("dsh-1".into()),
                result: "答了 2 个".into(),
                stderr_tail: String::new(),
                artifacts: vec!["answers/a.md".into()],
                harness: Some(SELF_PLUGIN_ID.into()),
            },
        )
        .unwrap();
        let done = p.run_status(&ctx).unwrap();
        assert_eq!(done["state"], "done");
        assert_eq!(done["record"]["result"], "答了 2 个");
        assert_eq!(done["record"]["session_id"], "dsh-1");
    }

    #[test]
    fn run_status_needs_a_run_id_and_refuses_a_traversing_task() {
        let v = tempfile::tempdir().unwrap();
        let p = DeepseekAgentPlugin::new();
        p.inner.lock().unwrap().vault = Some(v.path().to_path_buf());
        assert!(p.run_status(&json!({ "task": NOTE_TASK })).is_err());
        let e = p
            .run_status(&json!({ "task": "../../evil", "run_id": "R1" }))
            .unwrap_err();
        assert!(e.contains("invalid task id"), "err: {e}");
        let e = p
            .runs_root_and_task(&json!({ "task": "../../evil" }))
            .unwrap_err();
        assert!(e.contains("invalid task id"), "err: {e}");
    }

    /// The overview is what the window shows BEFORE you press Run, so it has to
    /// tell you what the task is allowed to do.
    #[test]
    fn the_overview_reports_each_tasks_sandbox_mode() {
        let v = tempfile::tempdir().unwrap();
        task::seed_builtin_templates(v.path());
        let got = overview(v.path());
        assert_eq!(got.len(), 2);
        for t in &got {
            assert_eq!(t.permission_mode, "workspace-write");
            assert!(!t.policy_rationale.is_empty(), "{}", t.def.id);
            assert!(!t.running);
        }
        let json = serde_json::to_value(&got).unwrap();
        assert_eq!(json[0]["id"], NOTE_TASK);
        assert_eq!(json[0]["permission_mode"], "workspace-write");
    }

    #[test]
    fn overview_reports_status_from_disk_not_from_memory() {
        let v = tempfile::tempdir().unwrap();
        task::seed_builtin_templates(v.path());
        let runs = task::runs_root(v.path()).join(NOTE_TASK);
        let _held = lock::acquire(
            &runs,
            lock::LockInfo {
                pid: std::process::id() as i32,
                run_id: "R2".into(),
                started_at: "2026-08-17T00:00:02Z".into(),
            },
        )
        .unwrap();
        let got = overview(v.path());
        let note = got.iter().find(|t| t.def.id == NOTE_TASK).unwrap();
        assert!(note.running, "a detached run holds the lock, not a memory map");
        assert_eq!(note.running_since.as_deref(), Some("2026-08-17T00:00:02Z"));
    }

    #[test]
    fn a_deleted_builtin_task_rebuilds_itself_on_demand() {
        let v = tempfile::tempdir().unwrap();
        task::seed_builtin_templates(v.path());
        std::fs::remove_dir_all(task::task_dir(v.path(), NOTE_TASK)).unwrap();
        let got = load_task(v.path(), NOTE_TASK).expect("a built-in rebuilds itself");
        assert!(!got.prompt.is_empty());
        assert!(task::task_dir(v.path(), NOTE_TASK).join("AGENTS.md").exists());
        assert_eq!(load_task(v.path(), "no-such-task"), None);
    }

    #[test]
    fn reads_cli_args_from_every_shape_the_host_has_used() {
        let nested = json!({ "cli": { "args": { "task": "selfcheck" },
                                      "flags": { "prompt": "go", "wait": true } } });
        assert_eq!(cli_str(&nested, "task").as_deref(), Some("selfcheck"));
        assert_eq!(cli_str(&nested, "prompt").as_deref(), Some("go"));
        assert!(cli_flag(&nested, "wait"));

        let flat = json!({ "task": "sweep", "prompt": "go", "wait": false });
        assert_eq!(cli_str(&flat, "task").as_deref(), Some("sweep"));
        assert!(!cli_flag(&flat, "wait"));

        let empty = json!({ "cli": { "args": { "task": "" } } });
        assert_eq!(cli_str(&empty, "task"), None);
        assert!(!cli_flag(&empty, "wait"));
    }

    #[test]
    fn reads_the_vault_out_of_the_shared_config() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"version":1,"sotvault":"/Users/x/git/sotvault"}"#).unwrap();
        assert_eq!(
            shared_config_vault_at(&p),
            Some(PathBuf::from("/Users/x/git/sotvault"))
        );
        for (name, body) in [
            ("empty.json", r#"{"version":1,"sotvault":""}"#),
            ("absent.json", r#"{"version":1}"#),
            ("broken.json", "{not json"),
        ] {
            let p = d.path().join(name);
            std::fs::write(&p, body).unwrap();
            assert_eq!(shared_config_vault_at(&p), None, "{name}");
        }
        assert_eq!(shared_config_vault_at(&d.path().join("nope.json")), None);
    }

    #[test]
    fn a_reminder_is_a_success_only_when_the_promised_file_is_really_there() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("2026-08-17-summary.md");
        let spec = NotifySpec {
            title_ok: "ok".into(),
            title_fail: "fail".into(),
            open_path: f.to_string_lossy().to_string(),
            expect_file: f.to_string_lossy().to_string(),
        };
        let rec = |s| record::RunRecord {
            run_id: "R1".into(),
            task: "t".into(),
            trigger: "relay".into(),
            started_at: "s".into(),
            ended_at: "e".into(),
            status: s,
            exit_code: Some(0),
            num_turns: None,
            session_id: None,
            result: String::new(),
            stderr_tail: String::new(),
            artifacts: Vec::new(),
            harness: Some(SELF_PLUGIN_ID.into()),
        };
        assert!(!run_delivered(&spec, Some(&rec(record::Status::Success))));
        std::fs::write(&f, "# x").unwrap();
        assert!(run_delivered(&spec, Some(&rec(record::Status::Success))));
        assert!(!run_delivered(&spec, Some(&rec(record::Status::Error))));
        assert!(!run_delivered(&spec, None));

        let ok = notify_params(&spec, true);
        assert_eq!(ok["action"]["kind"], "open_path");
        assert!(ok.get("severity").is_none(), "success is Info, not Warn");
        let bad = notify_params(&spec, false);
        assert_eq!(bad["action"]["plugin_id"], SELF_PLUGIN_ID);
        assert_eq!(bad["severity"], "warn");
    }

    #[test]
    fn tab_context_needs_a_path_to_be_usable() {
        let mut p = DeepseekAgentPlugin::new();
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
