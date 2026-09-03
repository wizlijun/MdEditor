use notemd_meetings::{MigrationMode, MigrationOptions, MigrationReport, MigrationService};
use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const WINDOW: &str = "main";

#[derive(Default)]
struct Inner {
    vault: Option<PathBuf>,
    vault_checked: bool,
    jobs: HashMap<u64, Arc<AtomicBool>>,
    next_job: u64,
}

pub struct MeetingsPlugin {
    data_dir: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

impl MeetingsPlugin {
    pub fn new() -> Self {
        Self {
            data_dir: std::env::temp_dir(),
            inner: Arc::new(Mutex::new(Inner {
                next_job: 1,
                ..Default::default()
            })),
        }
    }

    fn service(&self) -> Result<MigrationService, String> {
        let vault = self
            .inner
            .lock()
            .unwrap()
            .vault
            .clone()
            .ok_or("no vault configured")?;
        Ok(MigrationService::new(vault, self.data_dir.clone()))
    }

    fn detect(&self, params: &Value) -> Result<Value, String> {
        let source = required_str(params, "source")?;
        serde_json::to_value(self.service()?.detect(Path::new(source))?)
            .map_err(|error| error.to_string())
    }

    fn plan(&self, params: &Value) -> Result<Value, String> {
        let options = options_from_params(params)?;
        serde_json::to_value(self.service()?.plan(&options)?).map_err(|error| error.to_string())
    }

    fn apply_start(&self, host: &sdk::Host, params: &Value) -> Result<Value, String> {
        let options = options_from_params(params)?;
        let expected_plan = params
            .get("expected_plan")
            .filter(|value| !value.is_null())
            .map(|value| serde_json::from_value::<MigrationReport>(value.clone()))
            .transpose()
            .map_err(|error| format!("invalid expected_plan: {error}"))?;
        let service = self.service()?;
        // Fail fast on bad source/user/timezone before returning a job id.
        let _ = service.plan(&options)?;
        let (job_id, cancelled) = {
            let mut inner = self.inner.lock().unwrap();
            let job_id = inner.next_job;
            inner.next_job += 1;
            let cancelled = Arc::new(AtomicBool::new(false));
            inner.jobs.insert(job_id, cancelled.clone());
            (job_id, cancelled)
        };
        let host = host.clone();
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let progress_host = host.clone();
            let result = service.apply(
                &options,
                expected_plan.as_ref(),
                &cancelled,
                |committed, total, item| {
                    progress_host.ui_post(
                        WINDOW,
                        json!({
                            "type": "hemory-migration",
                            "job_id": job_id,
                            "event": "progress",
                            "committed": committed,
                            "total": total,
                            "item": item,
                        }),
                    );
                },
            );
            match result {
                Ok(report) => host.ui_post(
                    WINDOW,
                    json!({
                        "type": "hemory-migration",
                        "job_id": job_id,
                        "event": "done",
                        "committed": report.committed,
                        "report": report,
                    }),
                ),
                Err(error) => host.ui_post(
                    WINDOW,
                    json!({
                        "type": "hemory-migration",
                        "job_id": job_id,
                        "event": "failed",
                        "error": error,
                    }),
                ),
            }
            inner.lock().unwrap().jobs.remove(&job_id);
        });
        Ok(json!({"job_id": job_id}))
    }

    fn cancel(&self, params: &Value) -> Result<Value, String> {
        let job_id = params
            .get("job_id")
            .and_then(Value::as_u64)
            .ok_or("job_id is required")?;
        let cancelled = self.inner.lock().unwrap().jobs.get(&job_id).cloned();
        if let Some(cancelled) = cancelled {
            cancelled.store(true, Ordering::Relaxed);
            Ok(json!({"cancelled": true, "job_id": job_id}))
        } else {
            Ok(json!({"cancelled": false, "job_id": job_id}))
        }
    }

    fn library_list(&self) -> Result<Value, String> {
        Ok(json!({"meetings": self.service()?.library_list()?}))
    }

    fn cli_import(&self, context: &Value) -> Result<Value, String> {
        let source = cli_str(context, "source").ok_or(
            "usage: notemd meetings-import-hemory <source> [--dry-run] [--full] [--user ID] [--timezone IANA]",
        )?;
        let options = MigrationOptions {
            source: PathBuf::from(source),
            user: cli_str(context, "user"),
            timezone: cli_str(context, "timezone"),
            mode: if cli_flag(context, "full") {
                MigrationMode::Full
            } else {
                MigrationMode::Incremental
            },
        };
        let service = self.service()?;
        let report = if cli_flag(context, "dry-run") {
            service.plan(&options)?
        } else {
            service.apply(&options, None, &AtomicBool::new(false), |_, _, _| {})?
        };
        cli_report_outcome(&report)
    }
}

fn cli_report_outcome(report: &MigrationReport) -> Result<Value, String> {
    let value = serde_json::to_value(report).map_err(|error| error.to_string())?;
    let clean = report.is_clean();
    Ok(json!({
        "__notemd_cli_result": {
            "exit_code": if clean { 0 } else { 4 },
            "message": if clean {
                "Hemory migration completed"
            } else {
                "Hemory migration completed with conflicts or blocked items"
            },
            "data": value,
        }
    }))
}

async fn vault_from_host(host: &sdk::Host) -> Option<PathBuf> {
    for attempt in 1..=3 {
        match host.request("host.vault.info", json!({})).await {
            Ok(value) => {
                if let Some(root) = value
                    .get("root")
                    .and_then(Value::as_str)
                    .filter(|root| !root.is_empty())
                {
                    return Some(PathBuf::from(root));
                }
                host.log_warn(&format!("host.vault.info has no root (try {attempt})"));
            }
            Err(error) => {
                host.log_warn(&format!("host.vault.info failed (try {attempt}): {error}"))
            }
        }
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
    None
}

fn shared_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("NOTEMD_SHARED_CONFIG") {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join("Library/Application Support/net.notemd.app/shared.json"))
}

fn shared_config_vault() -> Option<PathBuf> {
    let value: Value = serde_json::from_slice(&std::fs::read(shared_config_path()?).ok()?).ok()?;
    value
        .get("sotvault")
        .and_then(Value::as_str)
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{key} is required"))
}

fn options_from_params(params: &Value) -> Result<MigrationOptions, String> {
    let mode = match params
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("incremental")
    {
        "incremental" => MigrationMode::Incremental,
        "full" => MigrationMode::Full,
        other => return Err(format!("unknown migration mode '{other}'")),
    };
    Ok(MigrationOptions {
        source: PathBuf::from(required_str(params, "source")?),
        user: params
            .get("user")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        timezone: params
            .get("timezone")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        mode,
    })
}

fn cli_str(context: &Value, key: &str) -> Option<String> {
    for pointer in [
        format!("/cli/args/{key}"),
        format!("/cli/flags/{key}"),
        format!("/cli/{key}"),
        format!("/{key}"),
    ] {
        if let Some(value) = context
            .pointer(&pointer)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

fn cli_flag(context: &Value, key: &str) -> bool {
    for pointer in [
        format!("/cli/flags/{key}"),
        format!("/cli/{key}"),
        format!("/{key}"),
    ] {
        match context.pointer(&pointer) {
            Some(Value::Bool(value)) => return *value,
            Some(Value::String(value)) => return !value.is_empty() && value != "false",
            _ => {}
        }
    }
    false
}

impl sdk::NotemdPlugin for MeetingsPlugin {
    fn initialize(&mut self, _host: &sdk::Host, params: &proto::InitializeParams) {
        self.data_dir = PathBuf::from(&params.data_dir);
    }

    fn activate(
        &mut self,
        host: &sdk::Host,
        _params: &proto::ActivateParams,
    ) -> Result<(), String> {
        let seeded = shared_config_vault();
        if let Some(vault) = &seeded {
            self.inner.lock().unwrap().vault = Some(vault.clone());
        }
        let inner = self.inner.clone();
        let host = host.clone();
        tokio::spawn(async move {
            let resolved = vault_from_host(&host).await.or(seeded);
            let mut state = inner.lock().unwrap();
            if resolved.is_some() {
                state.vault = resolved;
            }
            state.vault_checked = true;
        });
        Ok(())
    }

    fn deactivate(&mut self, _host: &sdk::Host) {
        for cancelled in self.inner.lock().unwrap().jobs.values() {
            cancelled.store(true, Ordering::Relaxed);
        }
    }

    fn execute_command(
        &mut self,
        _host: &sdk::Host,
        params: &proto::ExecuteCommandParams,
    ) -> Result<Value, String> {
        match params.command.as_str() {
            "meetings-import-hemory" | "import-hemory" => self.cli_import(&params.context),
            other => Err(format!("unknown command '{other}'")),
        }
    }

    fn on_ui_request(
        &mut self,
        host: &sdk::Host,
        method: &str,
        params: Value,
    ) -> Result<Value, String> {
        match method.strip_prefix("plugin.").unwrap_or(method) {
            "library_list" => self.library_list(),
            "hemory_detect" => self.detect(&params),
            "hemory_plan" => self.plan(&params),
            "hemory_apply_start" => self.apply_start(host, &params),
            "hemory_cancel" => self.cancel(&params),
            other => Err(format!("unknown ui method '{other}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_and_cli_params_are_mapped_without_defaulting_full() {
        let params = json!({
            "source": "/tmp/source",
            "user": "alice",
            "timezone": "Asia/Taipei",
            "mode": "full"
        });
        let options = options_from_params(&params).unwrap();
        assert_eq!(options.mode, MigrationMode::Full);
        assert_eq!(options.user.as_deref(), Some("alice"));
        assert!(cli_flag(&json!({"cli":{"flags":{"full":true}}}), "full"));
        assert!(!cli_flag(&json!({}), "full"));
        assert_eq!(
            cli_str(&json!({"cli":{"args":{"source":"/tmp/x"}}}), "source").as_deref(),
            Some("/tmp/x")
        );
    }

    #[test]
    fn cli_non_clean_result_keeps_the_complete_structured_report() {
        let mut report = MigrationReport::new(MigrationMode::Incremental, true, "alice".into());
        report.planned_at = "2026-09-03T10:00:00Z".into();
        report.blocked = 1;
        report.errors.push("bad transcript".into());
        let result = cli_report_outcome(&report).unwrap();
        let envelope = &result["__notemd_cli_result"];
        assert_eq!(envelope["exit_code"], 4);
        let recovered: MigrationReport = serde_json::from_value(envelope["data"].clone()).unwrap();
        assert_eq!(recovered.source_user, "alice");
        assert_eq!(recovered.errors, vec!["bad transcript"]);
        assert_eq!(recovered.blocked, 1);
    }

    #[test]
    fn cli_clean_result_uses_the_same_structured_envelope() {
        let report = MigrationReport::new(MigrationMode::Incremental, true, "alice".into());
        let result = cli_report_outcome(&report).unwrap();
        let envelope = &result["__notemd_cli_result"];
        assert_eq!(envelope["exit_code"], 0);
        assert_eq!(envelope["data"]["source_user"], "alice");
    }
}
