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
    wiki_dir: String,
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
            .join("Library/Application Support/net.notemd.app/shared.json"),
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

/// Reads a boolean CLI flag (e.g. `--dry-run`) out of `context` at any of the
/// same pointers `cli_str` checks. Ported from `ebook-import/backend/src/
/// plugin.rs`'s `cli_flag`. The host's clap-style parser (`src-tauri/src/cli/
/// runner.rs::parse_subcommand_args`) only inserts the key when the flag was
/// actually typed, as `Bool(true)` — but this also accepts a truthy string,
/// matching `cli_str`'s tolerance for the shape having varied.
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

/// A folder name read out of `Inner` that has not been resolved yet reads as
/// `""` — `Inner::default()` starts both `daily_dir` and `wiki_dir` empty,
/// and a CLI invocation dispatches `command.execute` the instant `$activate`
/// returns, which can race the async `host.vault.info` round-trip that fills
/// them in (see `shared_config_path`'s doc comment above). `sync::
/// sync_requested_day` shields the single-day path from that race by
/// defaulting an empty `daily_dir` itself; [`incremental::sync_since`] does
/// not do the same — an empty dir there is `is_safe_rel_dir`'s job to
/// *reject*, not default, because at that layer "empty" and "a host that
/// genuinely configured no subfolder" cannot be told apart. So this wiring
/// layer supplies the same default `activate` would have, rather than pass
/// `""` through and turn a startup race into a hard "invalid wiki folder"
/// error on the first CLI run after every plugin restart.
fn dir_or_default(dir: &str, default: &str) -> String {
    if dir.is_empty() { default.to_string() } else { dir.to_string() }
}

/// The whole report as a JSON value, plus an explicit `ok` flag.
///
/// `ok` is `errors.is_empty()`, **not** `failed == 0`.
/// [`incremental::SyncReport::errors`] is how `sync_since` says "this run was
/// not clean" — an unreadable ledger, a rename it refused to perform, a page
/// that failed — and `errors` is the authority, independent of `failed`:
/// `failed` counts only the one page that stopped the run outright, while a
/// problem like the unreadable-ledger case reports `failed == 0` and a
/// non-empty `errors` on purpose (see `incremental.rs`'s own doc comment and
/// tests). A run with problems is not a clean run and must never be presented
/// as one.
///
/// This is what the **window** gets, in both cases. It used to get a rejected
/// promise carrying a flattened summary string, which threw the whole report
/// away: `renamed` was not even in it, so the case where a rename matters most
/// — a page moved *and* something else went wrong — was exactly the case where
/// the user was never told which file had moved, and the `[[wikilink]]`s now
/// pointing at nothing went unmentioned. A run that synced forty pages and
/// then hit one problem rendered as a red banner and no statistics at all.
/// The window renders the counts, the page list and the renames *alongside*
/// the errors now; `ok` is what tells it which banner to use.
fn sync_report_value(
    report: &notemd_roam_import::incremental::SyncReport,
) -> Result<Value, String> {
    let mut value = serde_json::to_value(report).map_err(|e| e.to_string())?;
    if let Some(map) = value.as_object_mut() {
        map.insert("ok".into(), Value::Bool(report.errors.is_empty()));
    }
    Ok(value)
}

/// The errors-mean-not-clean contract, for the **CLI**.
///
/// The host's CLI layer is generic (`src/lib/cli/CliRunner.svelte`): a
/// resolved plugin command is exit 0 with `{"ok":true,…}`, a rejected one is
/// exit 4 with a `plugin_failed` envelope, and there is no third answer to
/// return. So "this run was not clean" has to be an `Err`, or a cron job
/// reading the exit code reports success while the sync silently
/// under-scans — the failure this contract exists to prevent.
///
/// What changed is that the `Err` no longer *discards* the run. The message
/// leads with a human summary that now includes `renamed`, names each moved
/// file on its own line, and ends with the complete serialized report — so
/// nothing the run computed is lost to the error path, and `--json`'s
/// `error.message` still carries every count, path and rename.
///
/// Pure — takes the already-computed report, no `Host` — so this decision is
/// exercised without a real vault, roam CLI or clock. The caller
/// (`cli_sync_changed`) additionally logs the report and every error through
/// the `Host`; that side effect can't live here.
fn cli_sync_outcome(
    report: &notemd_roam_import::incremental::SyncReport,
) -> Result<Value, String> {
    if report.errors.is_empty() {
        return sync_report_value(report);
    }
    let mut message = format!(
        "roam-sync finished with {} problem(s) and is NOT clean (scanned={}, synced={}, \
         skipped={}, failed={}, renamed={}): {}",
        report.errors.len(),
        report.scanned,
        report.synced,
        report.skipped,
        report.failed,
        report.renamed.len(),
        report.errors.join(" | "),
    );
    // Named one per line rather than folded into a count: a rename is the one
    // thing this sync does that the user did not ask for file by file, and
    // the `[[wikilink]]`s pointing at the old name are now broken.
    for r in &report.renamed {
        message.push_str(&format!("\n  moved {} -> {}", r.from, r.to));
    }
    // And the whole thing, machine-readable, so the error path loses nothing.
    if let Ok(json) = serde_json::to_string(report) {
        message.push_str(&format!("\n  report: {json}"));
    }
    Err(message)
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
        // A separate line, and only when there is something to say: adopting is
        // this sync *restructuring a note an earlier import wrote* — stamping
        // `id::` onto blocks that had none — not a change Roam made. It happens
        // once per note, but the first pass over a vault built by the JSON
        // importer touches every daily note in it, so it must be findable in
        // the log afterwards rather than inferred from a diff.
        if outcome.adopted > 0 {
            host.log_info(&format!(
                "sync {} -> {}: adopted {} block(s) written by an earlier import \
                 (stamped their Roam id:: in place; no content changed)",
                outcome.date, outcome.path, outcome.adopted,
            ));
        }
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

    /// Adapter only: supply [`incremental::sync_since`] with the vault, the
    /// two folder names, and the two impure edges it refuses to reach for
    /// itself — `discover` (the two datalog change-discovery queries, merged)
    /// and `fetch` (one recursive page pull). Both the window's `sync_since`
    /// UI method and the CLI's `sync-changed` command call this and nothing
    /// else, so there is exactly one orchestration for "sync everything
    /// changed since the watermark" — mirroring [`Self::sync_day`] for the
    /// single-day path.
    fn sync_changed(
        &self,
        params: &Value,
    ) -> Result<notemd_roam_import::incremental::SyncReport, String> {
        use notemd_roam_import::{changed, discover, incremental, roam_cli, roam_page};

        let (vault, daily_dir, wiki_dir) = {
            let g = self.inner.lock().unwrap();
            (g.vault.clone(), g.daily_dir.clone(), g.wiki_dir.clone())
        };
        let vault = vault.ok_or("no vault configured")?;
        let daily_dir = dir_or_default(&daily_dir, "dailynote");
        let wiki_dir = dir_or_default(&wiki_dir, "wikipage");
        let roam_path = params.get("roam_path").and_then(|s| s.as_str());
        let graph = params.get("graph").and_then(|s| s.as_str());
        let since = params.get("since").and_then(|s| s.as_str());
        let dry_run = params.get("dry_run").and_then(|b| b.as_bool()).unwrap_or(false);
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let today = chrono::Local::now().date_naive();

        // Discovered once, up front, and shared by both closures below — one
        // "roam CLI not found" error for the whole run rather than one per
        // page fetched.
        let exe = discover::discover(roam_path)
            .ok_or("the roam CLI was not found — install @roam-research/roam-cli")?;

        incremental::sync_since(
            &vault,
            (&wiki_dir, &daily_dir),
            since,
            // The ledger records which graph this vault is bound to, and
            // refuses a run against a different one — see `check_graph`.
            graph,
            today,
            &now,
            dry_run,
            |since_ms| {
                let (blocks, pages) = roam_cli::fetch_changed(&exe, graph, since_ms)?;
                changed::merge_changed(&blocks, &pages)
            },
            |uid| roam_page::parse_day_result(&roam_cli::fetch_day(&exe, graph, uid)?),
        )
    }

    /// Run [`Self::sync_changed`] and log the outcome — a summary line always,
    /// and every entry of `errors` as its own warning, so a problem this run
    /// hit is findable in the log whatever the caller then does with it.
    ///
    /// Returns the report itself. The two callers differ *only* in how they
    /// present a run that was not clean, which is the whole of I4: the window
    /// ([`sync_report_value`]) gets the statistics, the page list and the
    /// renames alongside an `ok: false`, while the CLI
    /// ([`cli_sync_outcome`]) must turn it into an `Err` because exit 4 is the
    /// only "not clean" the host's generic CLI layer can express.
    fn sync_changed_and_log(
        &self,
        host: &sdk::Host,
        params: &Value,
    ) -> Result<notemd_roam_import::incremental::SyncReport, String> {
        let report = self.sync_changed(params)?;
        host.log_info(&format!(
            "roam-sync {}..{} scanned={} synced={} skipped={} failed={} renamed={} \
             errors={} dry_run={}",
            report.from.as_deref().unwrap_or("?"),
            report.to.as_deref().unwrap_or("?"),
            report.scanned, report.synced, report.skipped, report.failed,
            report.renamed.len(), report.errors.len(), report.dry_run,
        ));
        for e in &report.errors {
            host.log_warn(&format!("roam-sync: {e}"));
        }
        Ok(report)
    }

    /// UI method `sync_status`: ledger-only, no fetch — just what the last
    /// run left behind, for the window to show without triggering a sync.
    fn sync_status(&self) -> Result<Value, String> {
        let vault = self.inner.lock().unwrap().vault.clone();
        let vault = vault.ok_or("no vault configured")?;
        let ledger = notemd_roam_import::ledger::Ledger::load(&vault).ledger;
        Ok(json!({ "last_synced_at": ledger.last_synced_at }))
    }

    /// UI method `sync_since`: the report, always — including when the run was
    /// not clean, where `ok` is `false` and `errors` carries the reasons. The
    /// window shows both halves; see [`sync_report_value`].
    fn ui_sync_changed(&self, host: &sdk::Host, params: &Value) -> Result<Value, String> {
        sync_report_value(&self.sync_changed_and_log(host, params)?)
    }

    /// CLI entry point: `notemd roam-sync [--since …] [--graph …]
    /// [--dry-run]`. Reads flags out of `context` (the host parses argv, not
    /// the plugin) and reuses the exact same `sync_changed` →
    /// `incremental::sync_since` path the window drives — differing only in
    /// [`cli_sync_outcome`]'s exit-code contract.
    fn cli_sync_changed(&self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let report = self.sync_changed_and_log(host, &cli_sync_changed_params(context))?;
        cli_sync_outcome(&report)
    }
}

/// Pure half of [`RoamImportPlugin::cli_sync_changed`]: reads `--since`,
/// `--graph` and `--dry-run` out of the host-injected CLI context into the
/// `sync_changed`/`sync_since` UI-method params shape. Split out so each flag
/// reaching this function is a plain unit test with no `Host`, no vault and
/// no `roam` process — see the tests below, each of which asserts on a value
/// that is *not* that flag's default (an empty/absent `--since` or `--graph`,
/// or an absent `--dry-run`), the same gap that let a flag go unwired and
/// unnoticed once before in this repo.
fn cli_sync_changed_params(context: &Value) -> Value {
    json!({
        "since": cli_str(context, "since"),
        "graph": cli_str(context, "graph"),
        "dry_run": cli_flag(context, "dry-run"),
    })
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
            let wiki_dir = info.as_ref()
                .and_then(|v| v.get("wiki_dir")).and_then(|d| d.as_str())
                .filter(|s| !s.is_empty()).unwrap_or("wikipage").to_string();
            let mut g = inner.lock().unwrap();
            // Never clobber a working seed (or a previously-resolved root)
            // with None.
            if root.is_some() {
                g.vault = root;
            }
            g.daily_dir = daily_dir;
            g.wiki_dir = wiki_dir;
            g.vault_checked = true;
        });
        Ok(())
    }

    fn deactivate(&mut self, _host: &sdk::Host) {}

    fn execute_command(&mut self, host: &sdk::Host, params: &proto::ExecuteCommandParams)
        -> Result<Value, String> {
        match params.command.as_str() {
            "sync-day" => self.cli_sync_day(host, &params.context),
            "sync-changed" => self.cli_sync_changed(host, &params.context),
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
            "sync_since" => self.ui_sync_changed(host, &params),
            "sync_status" => self.sync_status(),
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

    // ── cli_flag: reading a boolean flag out of the CLI's injected context ──

    #[test]
    fn cli_flag_reads_true_at_any_of_the_hosts_known_pointers() {
        assert!(cli_flag(&json!({"cli": {"flags": {"dry-run": true}}}), "dry-run"));
        assert!(cli_flag(&json!({"cli": {"dry-run": true}}), "dry-run"));
        assert!(cli_flag(&json!({"dry-run": true}), "dry-run"));
    }

    #[test]
    fn cli_flag_is_false_when_absent_or_explicitly_false() {
        assert!(!cli_flag(&json!({}), "dry-run"));
        assert!(!cli_flag(&json!({"cli": {"flags": {"dry-run": false}}}), "dry-run"));
    }

    // ── cli_sync_changed_params: --since/--graph/--dry-run each reach the
    // sync_changed params shape. Every assertion below uses a value that
    // could not be mistaken for that flag's default (empty/absent for the
    // strings, false/absent for the boolean) — the same shape of bug that
    // once let a CLI flag go completely unwired in this repo without a
    // failing test, because the value under test happened to equal the
    // default. ───────────────────────────────────────────────────────────

    #[test]
    fn since_flag_reaches_the_params() {
        let ctx = json!({"cli": {"flags": {"since": "2020-01-01"}}});
        assert_eq!(cli_sync_changed_params(&ctx)["since"], json!("2020-01-01"));
    }

    #[test]
    fn graph_flag_reaches_the_params() {
        let ctx = json!({"cli": {"flags": {"graph": "not-the-default-graph"}}});
        assert_eq!(cli_sync_changed_params(&ctx)["graph"], json!("not-the-default-graph"));
    }

    #[test]
    fn dry_run_flag_reaches_the_params() {
        let ctx = json!({"cli": {"flags": {"dry-run": true}}});
        assert_eq!(cli_sync_changed_params(&ctx)["dry_run"], json!(true));
    }

    #[test]
    fn absent_flags_read_as_none_and_false_not_as_someones_default() {
        let params = cli_sync_changed_params(&json!({}));
        assert_eq!(params["since"], json!(null));
        assert_eq!(params["graph"], json!(null));
        assert_eq!(params["dry_run"], json!(false));
    }

    // ── dir_or_default: shields sync_changed from the activate() race ───

    #[test]
    fn dir_or_default_keeps_a_resolved_dir() {
        assert_eq!(dir_or_default("journal", "dailynote"), "journal");
    }

    #[test]
    fn dir_or_default_falls_back_when_not_yet_resolved() {
        assert_eq!(dir_or_default("", "dailynote"), "dailynote");
        assert_eq!(dir_or_default("", "wikipage"), "wikipage");
    }

    // ── the two presentations of one report ─────────────────────────────

    use notemd_roam_import::incremental::{Planned, Renamed, SyncReport};

    fn clean_report() -> SyncReport {
        SyncReport {
            from: Some("2026-08-01T00:00:00.000Z".into()),
            to: Some("2026-08-02T00:00:00.000Z".into()),
            scanned: 2, synced: 2, skipped: 0, failed: 0,
            pages: vec![], renamed: vec![], errors: vec![], dry_run: false,
        }
    }

    /// A report that did real work *and* hit a problem — the shape I4 is
    /// about. Both are true at once, and neither may be dropped.
    fn busy_but_not_clean() -> SyncReport {
        let mut report = clean_report();
        report.scanned = 40;
        report.synced = 39;
        report.pages = vec![Planned {
            uid: "u".into(), title: "新名".into(),
            rel: "wikipage/新名.note.md".into(), wrote: true,
        }];
        report.renamed = vec![Renamed {
            uid: "u".into(),
            from: "wikipage/旧名.note.md".into(),
            to: "wikipage/新名.note.md".into(),
        }];
        report.errors = vec!["unreadable last sync time '<<<<<<< HEAD' in the ledger".into()];
        report
    }

    #[test]
    fn a_clean_report_becomes_ok_with_the_report_as_json() {
        let v = cli_sync_outcome(&clean_report()).expect("a clean report must be Ok");
        assert_eq!(v["scanned"], json!(2));
        assert_eq!(v["synced"], json!(2));
        assert_eq!(v["errors"], json!([]));
        assert_eq!(v["ok"], json!(true));
    }

    #[test]
    fn a_failed_page_is_reported_as_an_error_not_a_success() {
        let mut report = clean_report();
        report.failed = 1;
        report.errors = vec!["u: network went away".into()];
        let err = cli_sync_outcome(&report).unwrap_err();
        assert!(err.contains("1 problem"), "{err}");
        assert!(err.contains("network went away"), "{err}");
    }

    /// The case the whole contract exists for: `failed == 0` — no page
    /// stopped the run — but `errors` is non-empty (an unreadable ledger, or
    /// a rename refused because the destination already exists). This must
    /// still surface as an error, not a quiet success, or a cron job reading
    /// only the exit code silently under-scans forever.
    #[test]
    fn a_clean_failed_count_with_non_empty_errors_is_still_not_success() {
        let mut report = clean_report();
        report.failed = 0;
        report.errors = vec!["unreadable last sync time '<<<<<<< HEAD' in the ledger".into()];
        let err = cli_sync_outcome(&report).unwrap_err();
        assert!(err.contains("failed=0"), "{err}");
        assert!(err.contains("unreadable last sync time"), "{err}");
    }

    #[test]
    fn multiple_errors_are_all_visible_in_the_message() {
        let mut report = clean_report();
        report.errors = vec!["first problem".into(), "second problem".into()];
        let err = cli_sync_outcome(&report).unwrap_err();
        assert!(err.contains("first problem"), "{err}");
        assert!(err.contains("second problem"), "{err}");
        assert!(err.contains("2 problem"), "{err}");
    }

    /// I4, CLI half. The exit-4 contract stays, but the error must no longer
    /// throw the run away: the summary counts `renamed`, every move is named
    /// (those `[[wikilink]]`s are broken now, and this is the only place the
    /// user hears about it), and the full report rides along so `--json`'s
    /// `error.message` still carries every count, path and rename.
    #[test]
    fn a_not_clean_run_still_reports_everything_it_did() {
        let report = busy_but_not_clean();
        let err = cli_sync_outcome(&report).unwrap_err();
        assert!(err.contains("scanned=40") && err.contains("synced=39"), "{err}");
        assert!(err.contains("renamed=1"), "{err}");
        assert!(err.contains("moved wikipage/旧名.note.md -> wikipage/新名.note.md"), "{err}");

        let json = err.split("report: ").nth(1).unwrap_or_else(|| panic!("no report in: {err}"));
        let back: Value = serde_json::from_str(json.trim()).unwrap();
        assert_eq!(back["synced"], json!(39));
        assert_eq!(back["pages"][0]["rel"], json!("wikipage/新名.note.md"));
        assert_eq!(back["renamed"][0]["to"], json!("wikipage/新名.note.md"));
    }

    /// I4, window half. The window used to get nothing but a string here: a
    /// red banner, no statistics, and no word of the file that moved — for a
    /// run that synced 39 pages. It gets the whole report now, with `ok`
    /// telling it the run was not clean.
    #[test]
    fn the_window_gets_the_statistics_alongside_the_errors() {
        let v = sync_report_value(&busy_but_not_clean()).unwrap();
        assert_eq!(v["ok"], json!(false), "a run with errors is not a clean run");
        assert_eq!(v["scanned"], json!(40));
        assert_eq!(v["synced"], json!(39));
        assert_eq!(v["failed"], json!(0), "no page failed, and yet the run is not clean");
        assert_eq!(v["renamed"][0]["from"], json!("wikipage/旧名.note.md"));
        assert_eq!(v["pages"][0]["rel"], json!("wikipage/新名.note.md"));
        assert_eq!(v["errors"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_clean_run_is_marked_ok_for_the_window_too() {
        let v = sync_report_value(&clean_report()).unwrap();
        assert_eq!(v["ok"], json!(true));
    }
}
