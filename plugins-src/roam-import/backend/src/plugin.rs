use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Inner {
    vault: Option<PathBuf>,
    daily_dir: String,
    vault_checked: bool,
}

pub struct RoamImportPlugin {
    pub data_dir: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

impl RoamImportPlugin {
    pub fn new() -> Self {
        Self { data_dir: std::env::temp_dir(), inner: Arc::new(Mutex::new(Inner::default())) }
    }
}

impl sdk::NotemdPlugin for RoamImportPlugin {
    fn initialize(&mut self, _host: &sdk::Host, params: &proto::InitializeParams) {
        self.data_dir = PathBuf::from(&params.data_dir);
    }

    fn activate(&mut self, host: &sdk::Host, _p: &proto::ActivateParams) -> Result<(), String> {
        let inner = self.inner.clone();
        let host = host.clone();
        tokio::spawn(async move {
            let info = host.request("host.vault.info", json!({})).await.ok();
            let mut g = inner.lock().unwrap();
            g.vault = info.as_ref()
                .and_then(|v| v.get("root")).and_then(|r| r.as_str())
                .filter(|s| !s.is_empty()).map(PathBuf::from);
            g.daily_dir = info.as_ref()
                .and_then(|v| v.get("daily_dir")).and_then(|d| d.as_str())
                .filter(|s| !s.is_empty()).unwrap_or("dailynote").to_string();
            g.vault_checked = true;
        });
        Ok(())
    }

    fn deactivate(&mut self, _host: &sdk::Host) {}

    fn execute_command(&mut self, _host: &sdk::Host, params: &proto::ExecuteCommandParams)
        -> Result<Value, String> {
        Err(format!("unknown command '{}'", params.command))
    }

    fn on_ui_request(&mut self, _host: &sdk::Host, method: &str, params: Value)
        -> Result<Value, String> {
        match method {
            "probe" => {
                let explicit = params.get("roam_path").and_then(|s| s.as_str());
                serde_json::to_value(crate::roam_cli::probe(explicit)).map_err(|e| e.to_string())
            }
            other => Err(format!("unknown ui method '{other}'")),
        }
    }
}
