//! The NotemdPlugin implementation: five window RPC methods plus the menu and
//! CLI commands.
//!
//! The SDK calls `on_ui_request` synchronously on the protocol read loop, so
//! `run.start` only spawns a tokio task and returns the run id immediately —
//! blocking here would wedge the whole plugin. Events reach the window from
//! that task via `host.ui_post`.
use crate::{discover, engine, lock, prompt, record, runner, task};
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
            .join("Library/Application Support/com.laobu.mdeditor-shared/config.json"),
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
                // Rename before seeding, or the old and new names would both
                // end up in the task list.
                let moved = task::migrate_renamed_tasks(root);
                if !moved.is_empty() {
                    host.log_info(&format!("migrated renamed tasks: {}", moved.join(", ")));
                }
                let wrote = task::seed_builtin_templates(root);
                if !wrote.is_empty() {
                    host.log_info(&format!("seeded task templates: {}", wrote.join(", ")));
                }
                task::ensure_gitignore(root);
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
            "run-status" => self.run_status(&params.context),
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
            other => Err(format!("unknown ui method '{other}'")),
        }
    }
}

impl ClaudeAgentPlugin {
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
        let user_prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let use_ctx = params
            .get("use_context")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let task_dir = task::task_dir(&vault, &task_id);
        let mut def = task::read_task(&task_dir).ok_or(format!("unknown task '{task_id}'"))?;
        def.id = task_id.clone();

        let claude = discover::discover(std::env::var("NOTEMD_CLAUDE_BIN").ok().as_deref())
            .ok_or("claude executable not found — install Claude Code, or point NOTEMD_CLAUDE_BIN at it")?;

        let ctx = if use_ctx { self.tab_ctx() } else { None };
        let full = prompt::compose(&def.prompt, &user_prompt, ctx.as_ref());
        let run_id = record::new_run_id(chrono::Utc::now(), std::process::id());

        let spec = engine::RunSpec {
            vault: vault.clone(),
            task: def,
            task_dir,
            task_run_dir: task::runs_root(&vault).join(&task_id),
            claude,
            prompt: full,
            trigger: trigger.to_string(),
            run_id: run_id.clone(),
            oauth_token: std::env::var("CLAUDE_CODE_OAUTH_TOKEN").ok(),
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
                    while let Some(step) = rx.recv().await {
                        match step {
                            engine::Step::Event(e) => h.ui_post(
                                WINDOW,
                                json!({ "kind": "event", "run_id": rid, "event": e }),
                            ),
                            engine::Step::Done(r) => h.ui_post(
                                WINDOW,
                                json!({ "kind": "done", "run_id": rid, "record": r }),
                            ),
                        }
                    }
                })
            };
            if let Err(busy) = engine::run(spec, tx, cancel_rx).await {
                h.ui_post(
                    WINDOW,
                    json!({ "kind": "busy", "run_id": rid, "holder": busy.0 }),
                );
                h.toast("warn", "That task is already running", Some(&busy.0.run_id));
            }
            let _ = pump.await;
            inner.lock().unwrap().running.remove(&rid);
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
            "本次只处理这一个文件:`{rel}`。\n\
             只回答该文件里 `status:: open` 的问题,不要扫描 vault 中的其它文件。\n\
             该文件里没有待答问题时,直接报告「无待答问题」并结束。"
        );
        host.log_info(&format!("run-note {task_id} on {rel}"));
        self.start(
            host,
            json!({ "task": task_id, "prompt": prompt, "use_context": false }),
            "note",
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
        let run_dir = task::runs_root(&vault).join(task_id);

        if let Some(rec) = record::find(&run_dir, run_id) {
            return Ok(json!({ "state": "done", "record": rec }));
        }
        let held = lock::current(&run_dir).filter(|h| h.run_id == run_id);
        if let Some(h) = held {
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

    /// The CLI entry point. Detached by default; `--wait` runs inline.
    fn cli_run(&mut self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let task_id = cli_str(context, "task")
            .ok_or("usage: notemd agent <task> [-p PROMPT] [--wait]")?;
        let p = cli_str(context, "prompt").unwrap_or_default();
        let wait = cli_flag(context, "wait");
        if wait {
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
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"command.execute\",\"params\":{\"command\":\"run-status\",\"context\":{\"run_id\":\"R1\"}}}\n",
            )
            .await
            .unwrap();

        let answered = await_response(
            from_plugin,
            |v| v.get("id").and_then(|i| i.as_u64()) == Some(2),
            "run-status",
        )
        .await;
        assert!(
            answered.get("error").is_none(),
            "run-status failed right after activation: {answered}"
        );
        // No such run yet — but the vault resolved, which is the point.
        assert_eq!(answered["result"]["state"], "lost");
        std::env::remove_var("NOTEMD_SHARED_CONFIG");
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
        assert_eq!(first["id"], "answer-note-question");
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
            },
        )
        .unwrap();
        let done = p.run_status(&ctx).unwrap();
        assert_eq!(done["state"], "done");
        assert_eq!(done["record"]["result"], "answered 2");
        assert_eq!(done["record"]["artifacts"][0], "answers/a.md");
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
