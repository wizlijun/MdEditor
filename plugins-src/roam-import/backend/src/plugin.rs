use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

/// Resolve `host.vault.info`, retrying up to 3 times — the host can
/// legitimately answer with no root while vault_sync is still starting up
/// (ported from `plugins-src/ebook-import/backend/src/plugin.rs`'s
/// `vault_from_host`). Every failed/empty attempt is logged: a swallowed
/// error here reads to the user as "no vault configured" with no way to
/// tell why.
///
/// Generic over the fetch/log so this loop is unit-testable without a real
/// `Host` (which has no public constructor) or real sleeps — `discover_with`
/// uses the same injected-closure shape for the same reason.
async fn resolve_vault_info<Fetch, FetchFut, Log>(
    mut fetch: Fetch,
    log_warn: Log,
    retry_delay: Duration,
) -> Option<Value>
where
    Fetch: FnMut() -> FetchFut,
    FetchFut: std::future::Future<Output = Result<Value, String>>,
    Log: Fn(&str),
{
    for attempt in 1..=3 {
        match fetch().await {
            Ok(v) => {
                let has_root = v
                    .get("root")
                    .and_then(|r| r.as_str())
                    .filter(|s| !s.is_empty())
                    .is_some();
                if has_root {
                    return Some(v);
                }
                log_warn(&format!("host.vault.info has no root (try {attempt}): {v}"));
            }
            Err(e) => log_warn(&format!("host.vault.info failed (try {attempt}): {e}")),
        }
        tokio::time::sleep(retry_delay).await;
    }
    None
}

async fn vault_from_host(host: &sdk::Host) -> Option<Value> {
    resolve_vault_info(
        || host.request("host.vault.info", json!({})),
        |m| host.log_warn(m),
        Duration::from_millis(700),
    )
    .await
}

impl RoamImportPlugin {
    /// One day, end to end: resolve the date against the *local* calendar
    /// (a daily note is a human's day, not UTC's), ask Roam for that page,
    /// and merge it into the vault. Runs synchronously on the protocol read
    /// loop like `probe` does — `fetch_day` is bounded at 60s and the
    /// manifest raises this plugin's request timeout to 120s to cover it.
    fn sync_day(&self, params: &Value) -> Result<notemd_roam_import::sync::SyncOutcome, String> {
        use notemd_roam_import::{dates, discover, roam_cli, roam_page, sync};

        let (vault, daily_dir) = {
            let g = self.inner.lock().unwrap();
            (g.vault.clone(), g.daily_dir.clone())
        };
        // Also the answer while `activate`'s vault lookup is still in flight:
        // writing a day's notes somewhere other than the vault would be worse
        // than asking the user to try again.
        let vault = vault.ok_or("no vault configured")?;
        let daily_dir = if daily_dir.is_empty() { "dailynote".to_string() } else { daily_dir };

        let date = dates::resolve_date(
            params.get("date").and_then(|s| s.as_str()),
            chrono::Local::now().date_naive(),
        )?;
        let uid = dates::to_roam_uid(&date).ok_or_else(|| format!("invalid date '{date}'"))?;

        let roam_path = params.get("roam_path").and_then(|s| s.as_str());
        let exe = discover::discover(roam_path)
            .ok_or("the roam CLI was not found — install @roam-research/roam-cli")?;
        let raw = roam_cli::fetch_day(&exe, params.get("graph").and_then(|s| s.as_str()), &uid)?;
        let page = roam_page::parse_day_result(&raw)?;

        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        sync::sync_day(&vault, &daily_dir, page.as_ref(), &date, &now)
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
            let info = vault_from_host(&host).await;
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

    fn on_ui_request(&mut self, host: &sdk::Host, method: &str, params: Value)
        -> Result<Value, String> {
        match method {
            "probe" => {
                let explicit = params.get("roam_path").and_then(|s| s.as_str());
                serde_json::to_value(notemd_roam_import::roam_cli::probe(explicit))
                    .map_err(|e| e.to_string())
            }
            "sync_day" => {
                let outcome = self.sync_day(&params)?;
                host.log_info(&format!(
                    "sync {} -> {} (found={} created={} updated={} kept_local={} roam_gone_kept={})",
                    outcome.date, outcome.path, outcome.found, outcome.created,
                    outcome.updated, outcome.kept_local, outcome.roam_gone_kept,
                ));
                serde_json::to_value(outcome).map_err(|e| e.to_string())
            }
            other => Err(format!("unknown ui method '{other}'")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;

    // Sub-millisecond retry delay: these tests exercise the retry/logging
    // logic, not real backoff timing.
    const FAST: Duration = Duration::from_millis(1);

    #[tokio::test]
    async fn succeeds_on_first_attempt_when_root_present() {
        let calls = AtomicUsize::new(0);
        let warns: StdMutex<Vec<String>> = StdMutex::new(vec![]);
        let got = resolve_vault_info(
            || {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Ok(json!({"root": "/vault", "daily_dir": "daily"})) }
            },
            |m| warns.lock().unwrap().push(m.to_string()),
            FAST,
        )
        .await;
        assert_eq!(got.unwrap()["root"], json!("/vault"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(warns.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn empty_root_is_treated_as_absent_and_retried() {
        let call = AtomicUsize::new(0);
        let warns: StdMutex<Vec<String>> = StdMutex::new(vec![]);
        let got = resolve_vault_info(
            || {
                let n = call.fetch_add(1, Ordering::SeqCst);
                async move {
                    if n < 2 {
                        Ok(json!({"root": ""}))
                    } else {
                        Ok(json!({"root": "/vault"}))
                    }
                }
            },
            |m| warns.lock().unwrap().push(m.to_string()),
            FAST,
        )
        .await;
        assert_eq!(got.unwrap()["root"], json!("/vault"));
        assert_eq!(call.load(Ordering::SeqCst), 3);
        assert_eq!(warns.lock().unwrap().len(), 2, "one warn per empty-root attempt");
    }

    #[tokio::test]
    async fn gives_up_after_three_failed_attempts_and_logs_each() {
        let warns: StdMutex<Vec<String>> = StdMutex::new(vec![]);
        let got = resolve_vault_info(
            || async { Err::<Value, String>("boom".to_string()) },
            |m| warns.lock().unwrap().push(m.to_string()),
            FAST,
        )
        .await;
        assert!(got.is_none());
        let w = warns.lock().unwrap();
        assert_eq!(w.len(), 3, "one warn per failed attempt, including the last");
        assert!(w.iter().all(|m| m.contains("boom")));
    }
}
