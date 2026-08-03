use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
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

/// Ported from `ebook-import/backend/src/plugin.rs`: the shared config file
/// the host itself reads its `sotvault` from. Read synchronously, in
/// `activate()`, before the `host.vault.info` round-trip even starts — a CLI
/// invocation dispatches `command.execute` the instant `$activate`'s
/// response comes back, which races the plugin's own async vault-fetch task
/// for a turn of the single-consumer stdin loop they both depend on. The
/// long-lived GUI window can afford to wait out that race; a one-shot CLI
/// command cannot, so it needs a vault the moment activation returns.
fn shared_config_path() -> Option<PathBuf> {
    // Overridable so a test never reads — and then seeds behavior from — the
    // real shared config of whoever is running the suite.
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
fn shared_config_vault_at(path: &Path) -> Option<PathBuf> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let s = v.get("sotvault")?.as_str()?;
    (!s.is_empty()).then(|| PathBuf::from(s))
}

/// The host's frontend parses CLI args and injects them into `context`; the
/// exact shape has varied, so look in every place it has lived. Ported from
/// `ebook-import/backend/src/plugin.rs`'s `cli_str`.
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

impl RoamImportPlugin {
    /// Adapter only: supply `sync::sync_requested_day` with the two things it
    /// refuses to reach for itself — the local calendar (a daily note is a
    /// human's day, not UTC's) and a fetcher that discovers and runs the
    /// `roam` CLI. All the decisions live in `sync.rs`, where they are
    /// testable; nothing that can be tested belongs in this bin crate.
    ///
    /// Runs synchronously on the protocol read loop like `probe` does —
    /// `fetch_day` is bounded at 60s and the manifest raises this plugin's
    /// request timeout to 120s to cover it.
    fn sync_day(&self, params: &Value) -> Result<notemd_roam_import::sync::SyncOutcome, String> {
        use notemd_roam_import::{discover, roam_cli, roam_page, sync};

        let (vault, daily_dir) = {
            let g = self.inner.lock().unwrap();
            (g.vault.clone(), g.daily_dir.clone())
        };
        let roam_path = params.get("roam_path").and_then(|s| s.as_str());
        let graph = params.get("graph").and_then(|s| s.as_str());
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        sync::sync_requested_day(
            vault.as_deref(),
            &daily_dir,
            params.get("date").and_then(|s| s.as_str()),
            chrono::Local::now().date_naive(),
            &now,
            |uid| {
                let exe = discover::discover(roam_path)
                    .ok_or("the roam CLI was not found — install @roam-research/roam-cli")?;
                roam_page::parse_day_result(&roam_cli::fetch_day(&exe, graph, uid)?)
            },
        )
    }

    /// Shared by the window's `sync_day` UI method and the CLI's `sync-day`
    /// command: run [`Self::sync_day`], log the outcome, and hand back its
    /// JSON — so the two callers cannot drift into different logging or
    /// response shapes.
    fn sync_day_and_report(&self, host: &sdk::Host, params: &Value) -> Result<Value, String> {
        let outcome = self.sync_day(params)?;
        host.log_info(&format!(
            "sync {} -> {} (found={} created={} updated={} kept_local={} roam_gone_kept={})",
            outcome.date, outcome.path, outcome.found, outcome.created,
            outcome.updated, outcome.kept_local, outcome.roam_gone_kept,
        ));
        serde_json::to_value(outcome).map_err(|e| e.to_string())
    }

    /// CLI entry point: `notemd roam-day [--date …] [--graph …]`. Reads flags
    /// out of `context` (the host parses argv, not the plugin) and reuses the
    /// exact same `sync_day` → `sync::sync_requested_day` path the window
    /// drives, so there is exactly one orchestration for "sync one day."
    fn cli_sync_day(&self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let params = json!({
            "date": cli_str(context, "date"),
            "graph": cli_str(context, "graph"),
        });
        self.sync_day_and_report(host, &params)
    }
}

impl sdk::NotemdPlugin for RoamImportPlugin {
    fn initialize(&mut self, _host: &sdk::Host, params: &proto::InitializeParams) {
        self.data_dir = PathBuf::from(&params.data_dir);
    }

    fn activate(&mut self, host: &sdk::Host, _p: &proto::ActivateParams) -> Result<(), String> {
        let inner = self.inner.clone();
        let host = host.clone();

        // Seed the vault SYNCHRONOUSLY from the shared config — a plain file
        // read, no host round-trip — so a CLI command that arrives the
        // instant activation returns already has a vault to work with (see
        // `shared_config_path`'s doc comment for why this matters here and
        // not just in ebook-import, where it was ported from).
        let seeded = shared_config_vault();
        if let Some(root) = &seeded {
            self.inner.lock().unwrap().vault = Some(root.clone());
        }

        // MUST be spawned, never awaited inline: `$activate` is dispatched
        // synchronously on the protocol read loop, and the response to
        // `host.vault.info` can only be routed BY that loop.
        tokio::spawn(async move {
            let info = vault_from_host(&host).await;
            let root = info.as_ref()
                .and_then(|v| v.get("root")).and_then(|r| r.as_str())
                .filter(|s| !s.is_empty()).map(PathBuf::from)
                .or(seeded);
            let daily_dir = info.as_ref()
                .and_then(|v| v.get("daily_dir")).and_then(|d| d.as_str())
                .filter(|s| !s.is_empty()).unwrap_or("dailynote").to_string();
            let mut g = inner.lock().unwrap();
            // Never clobber a working seed (or a previously-resolved root)
            // with None.
            if root.is_some() {
                g.vault = root;
            }
            g.daily_dir = daily_dir;
            g.vault_checked = true;
        });
        Ok(())
    }

    fn deactivate(&mut self, _host: &sdk::Host) {}

    fn execute_command(&mut self, host: &sdk::Host, params: &proto::ExecuteCommandParams)
        -> Result<Value, String> {
        match params.command.as_str() {
            "sync-day" => self.cli_sync_day(host, &params.context),
            other => Err(format!("unknown command '{other}'")),
        }
    }

    fn on_ui_request(&mut self, host: &sdk::Host, method: &str, params: Value)
        -> Result<Value, String> {
        match method {
            "probe" => {
                let explicit = params.get("roam_path").and_then(|s| s.as_str());
                serde_json::to_value(notemd_roam_import::roam_cli::probe(explicit))
                    .map_err(|e| e.to_string())
            }
            "sync_day" => self.sync_day_and_report(host, &params),
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

    // ── shared_config_vault_at: the synchronous CLI-path seed ───────────
    //
    // Pure-function tests only (no NOTEMD_SHARED_CONFIG env mutation, unlike
    // ebook-import's equivalent) — `shared_config_vault_at` takes its path as
    // an argument, so there is nothing process-global to guard here.

    #[test]
    fn shared_config_vault_at_reads_the_sotvault_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, r#"{"version":1,"sotvault":"/Users/x/git/sotvault"}"#).unwrap();
        assert_eq!(
            shared_config_vault_at(&path),
            Some(PathBuf::from("/Users/x/git/sotvault"))
        );
    }

    #[test]
    fn shared_config_without_a_usable_vault_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.json");
        assert_eq!(shared_config_vault_at(&missing), None);
        let empty = dir.path().join("empty.json");
        std::fs::write(&empty, r#"{"version":1,"sotvault":""}"#).unwrap();
        assert_eq!(shared_config_vault_at(&empty), None);
        let malformed = dir.path().join("bad.json");
        std::fs::write(&malformed, "not json").unwrap();
        assert_eq!(shared_config_vault_at(&malformed), None);
    }

    // ── cli_str: reading a flag out of the CLI's injected context ───────

    #[test]
    fn cli_str_finds_a_flag_at_any_of_the_hosts_known_pointers() {
        assert_eq!(
            cli_str(&json!({"cli": {"flags": {"date": "2026-08-02"}}}), "date"),
            Some("2026-08-02".to_string())
        );
        assert_eq!(
            cli_str(&json!({"cli": {"date": "2026-08-02"}}), "date"),
            Some("2026-08-02".to_string())
        );
        assert_eq!(
            cli_str(&json!({"date": "2026-08-02"}), "date"),
            Some("2026-08-02".to_string())
        );
    }

    #[test]
    fn cli_str_treats_absent_or_empty_as_none() {
        assert_eq!(cli_str(&json!({}), "date"), None);
        assert_eq!(cli_str(&json!({"cli": {"flags": {"date": ""}}}), "date"), None);
    }
}
