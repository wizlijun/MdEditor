//! The NotemdPlugin implementation: five window RPC methods plus the menu and
//! CLI commands.
//!
//! The SDK calls `on_ui_request` synchronously on the protocol read loop, so
//! `run.start` only spawns a tokio task and returns the run id immediately —
//! blocking here would wedge the whole plugin. Events reach the window from
//! that task via `host.ui_post`.
use crate::{discover, engine, prompt, record, runner, task};
use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const WINDOW: &str = "main";

#[derive(Default)]
struct Inner {
    vault: Option<PathBuf>,
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

/// The vault root has to come from the host (`host.vault.info`); everything
/// after that is plain filesystem work in our own process.
async fn vault_root(host: &sdk::Host) -> Option<PathBuf> {
    let v = host.request("host.vault.info", json!({})).await.ok()?;
    v.get("root")?.as_str().map(PathBuf::from)
}

impl sdk::NotemdPlugin for ClaudeAgentPlugin {
    fn activate(&mut self, host: &sdk::Host, _p: &proto::ActivateParams) -> Result<(), String> {
        let inner = self.inner.clone();
        let host2 = host.clone();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                if let Some(root) = vault_root(&host2).await {
                    let wrote = task::seed_builtin_templates(&root);
                    if !wrote.is_empty() {
                        host2.log_info(&format!("seeded task templates: {}", wrote.join(", ")));
                    }
                    task::ensure_gitignore(&root);
                    inner.lock().unwrap().vault = Some(root);
                } else {
                    host2.log_warn("no vault configured; claude-agent needs one");
                }
            })
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
            "tasks.list" => {
                let vault = self.vault()?;
                Ok(json!({ "tasks": task::discover(&vault) }))
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
            "history.list" => {
                let vault = self.vault()?;
                let t = params.get("task").and_then(|v| v.as_str()).unwrap_or_default();
                Ok(json!({ "runs": record::recent(&task::runs_root(&vault).join(t), 20) }))
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
            .ok_or_else(|| "no vault configured".to_string())
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
