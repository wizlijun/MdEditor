//! The `NotemdPlugin` implementation: vault resolution, the `on_ui_request`
//! method table the import window drives, job orchestration (one
//! `std::thread` per running import, cancellable), and the `import` CLI
//! command.
//!
//! Vault resolution (`vault_from_host`/`shared_config_vault*`) and the CLI
//! context-reading helpers (`cli_str`/`cli_flag`) are ported verbatim from
//! `plugins-src/claude-agent/backend/src/plugin.rs` — same problem (the SDK
//! dispatches `$activate` synchronously on the protocol read loop, so
//! resolving the vault via `host.request` must be spawned, never awaited
//! inline, or the whole plugin wedges until the host's request timeout).

use crate::ocr::baidu::BaiduOcr;
use crate::ocr::quartz::QuartzRenderer;
use crate::ocr::wechat::WeChatOcr;
use crate::ocr::OcrEngine;
use crate::calibre;
use crate::pipeline::{self, PipelineCtx};
use crate::settings::{self, DeviceSettings, VaultSettings};
use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const WINDOW: &str = "main";
const NO_VAULT: &str = "no vault configured";

#[derive(Default)]
struct Inner {
    vault: Option<PathBuf>,
    /// Whether the vault lookup has finished. `vault: None` means "still
    /// resolving" before this flips, and "no vault configured" after.
    vault_checked: bool,
    /// job_id → cancel flag, polled by the pipeline between stages.
    jobs: HashMap<u64, Arc<AtomicBool>>,
    /// Next job id to hand out; starts at 1 (0 would look like "unset").
    next_job: u64,
}

pub struct EbookImportPlugin {
    pub data_dir: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

impl EbookImportPlugin {
    pub fn new() -> Self {
        Self {
            data_dir: std::env::temp_dir(),
            inner: Arc::new(Mutex::new(Inner {
                next_job: 1,
                ..Default::default()
            })),
        }
    }
}

// ── Vault resolution (ported from claude-agent/backend/src/plugin.rs) ──

/// The vault root. The host is authoritative (`host.vault.info`), but it can
/// answer with nothing — during startup before vault_sync has initialised, for
/// one — so retry, and then fall back to the very config file the host itself
/// falls back to. Every failure is logged: a swallowed error here reads to the
/// user as "no vault configured" with no way to tell why.
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
/// claude-agent/backend/src/plugin.rs.
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

// ── save_settings merge rules (pure, unit-tested without a host) ───────

/// Merges a `save_settings` `vault` patch into the existing `VaultSettings`.
/// Only present, non-empty string fields overwrite; anything else (absent
/// key, `null`, empty string) leaves the existing value alone — vault
/// settings have no "clear to empty" semantics, unlike the device secrets
/// below.
///
/// `ebooks_root` additionally goes through [`settings::validate_ebooks_root`]
/// (Finding 4): an absolute path, a path containing a `..` component, or an
/// empty string can escape the vault once `pipeline::run_import` joins it
/// onto `vault_root`, so a bad value here must be rejected before it's ever
/// persisted to `.notemd/ebook-import.json` rather than silently written and
/// only caught later.
fn apply_vault_patch(existing: &VaultSettings, patch: &Value) -> Result<VaultSettings, String> {
    let mut out = existing.clone();
    if let Some(s) = patch.get("ebooks_root").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            settings::validate_ebooks_root(s)?;
            out.ebooks_root = s.to_string();
        }
    }
    if let Some(s) = patch.get("provider").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            out.provider = s.to_string();
        }
    }
    if let Some(s) = patch.get("wechat_url").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            out.wechat_url = s.to_string();
        }
    }
    Ok(out)
}

/// A secret-like field's patch value: empty string = leave unchanged,
/// literal `"-"` = clear, anything else = the new value. Applies to
/// `baidu_api_key`/`baidu_secret_key`, which are never echoed back to the
/// UI (see `detect_env`), so `save_settings` is the *only* way to change or
/// clear them — a plain empty string can't mean "clear" or a user clearing
/// an input field (rather than intentionally typing something) would wipe
/// a saved key by accident.
fn apply_secret_patch(existing: &str, patch: &str) -> String {
    match patch {
        "" => existing.to_string(),
        "-" => String::new(),
        other => other.to_string(),
    }
}

/// Merges a `save_settings` `device` patch into the existing
/// `DeviceSettings`. `calibre_path`: absent/`null` = leave unchanged, `""` =
/// clear, other string = set. The two Baidu keys follow
/// [`apply_secret_patch`].
fn apply_device_patch(existing: &DeviceSettings, patch: &Value) -> DeviceSettings {
    let mut out = existing.clone();
    match patch.get("calibre_path") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) if s.is_empty() => out.calibre_path = None,
        Some(Value::String(s)) => out.calibre_path = Some(s.clone()),
        _ => {}
    }
    if let Some(v) = patch.get("baidu_api_key").and_then(|v| v.as_str()) {
        out.baidu_api_key = apply_secret_patch(&out.baidu_api_key, v);
    }
    if let Some(v) = patch.get("baidu_secret_key").and_then(|v| v.as_str()) {
        out.baidu_secret_key = apply_secret_patch(&out.baidu_secret_key, v);
    }
    out
}

/// Builds the `OcrEngine` for `provider`, wired to `cancelled` so a job's
/// cancel flag (or `deactivate`'s "cancel everything") actually reaches the
/// engine's network loop — see `WeChatOcr`/`BaiduOcr`'s own `cancelled`
/// field docs. Baidu needs both keys set (an early, clear error beats a
/// confusing failure deep in `ocr_pdf`); the WeChat path constructs a
/// `QuartzRenderer` (CoreGraphics is a system framework, so unlike the old
/// pdfium-dylib renderer its `new()` can't practically fail on macOS -- the
/// `Result` shape is kept anyway so this call site wouldn't need to change
/// if that ever stopped being true) — surfaced here as the same kind of
/// `Result` error, which the caller turns into a `failed` job event.
fn build_engine(
    provider: &str,
    vault_settings: &VaultSettings,
    device: &DeviceSettings,
    cancelled: &Arc<AtomicBool>,
) -> Result<Box<dyn OcrEngine>, String> {
    match provider {
        "baidu" => {
            if device.baidu_api_key.is_empty() || device.baidu_secret_key.is_empty() {
                return Err(
                    "Baidu OCR needs an API key and secret key — set them in settings"
                        .to_string(),
                );
            }
            Ok(Box::new(
                BaiduOcr::new(device.baidu_api_key.clone(), device.baidu_secret_key.clone())
                    .with_cancel(cancelled.clone()),
            ))
        }
        _ => {
            let renderer = QuartzRenderer::new()?;
            Ok(Box::new(WeChatOcr {
                url: vault_settings.wechat_url.clone(),
                renderer: Box::new(renderer),
                // Construction-time default per Task 6 review: the 120s
                // timeout lives here, not inside WeChatOcr's own logic.
                timeout: Duration::from_secs(120),
                cancelled: cancelled.clone(),
            }))
        }
    }
}

/// Best-effort absolute form of `p`: canonicalized if it exists on disk,
/// otherwise joined onto the current working directory (falling back to `p`
/// itself if even that fails). Only used to key [`work_dir_name`]'s hash on
/// something path-like rather than a bare relative string that could
/// collide across different working directories.
fn absolute_or_given(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|cwd| cwd.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    })
}

/// A stable, filesystem-safe scratch-dir name for `input_abs`: the file
/// stem plus an 8-hex-char hash of the input's *absolute* path. Two
/// different source files that happen to share a stem (e.g. two unrelated
/// `chapter1.pdf`s from different imports) get isolated `work` dirs instead
/// of silently contaminating each other's `images/`/`htmlz/`/`pageNNNN.md`;
/// the *same* file re-imported (resuming an interrupted OCR run, or a CLI
/// retry) maps to the same dir every time, on purpose — see
/// `pipeline::PipelineCtx::work`'s doc comment on OCR resume.
fn work_dir_name(input_abs: &Path) -> String {
    let stem = input_abs
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("book");
    format!(
        "{stem}_{}_temp",
        fnv1a_hex8(input_abs.to_string_lossy().as_bytes())
    )
}

/// FNV-1a, 32-bit, formatted as 8 lowercase hex chars. Not cryptographic —
/// this only needs to be a cheap, stable, low-collision fingerprint for a
/// handful of concurrently-importing files, not a security boundary.
fn fnv1a_hex8(bytes: &[u8]) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for &b in bytes {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("{hash:08x}")
}

/// A destination path relative to the vault, POSIX-separated, for the
/// `done` event's `dest_rel`. Falls back to the absolute path (still
/// stringified with forward slashes where possible) if `dest` somehow isn't
/// under `vault` — should not happen since `run_import` always writes under
/// `vault_root`, but a UI-facing value must never come back empty or panic.
fn dest_relative(vault: &Path, dest: &Path) -> String {
    let rel = dest.strip_prefix(vault).unwrap_or(dest);
    rel.to_string_lossy().replace('\\', "/")
}

/// Runs one import to completion off the protocol thread, pushing
/// `log`/`progress`/`done`/`failed` events to the window as it goes. Free
/// function (not a method) because it runs on its own `std::thread`, well
/// after `on_ui_request` has already returned `{job_id}`.
fn run_job(
    host: sdk::Host,
    vault: PathBuf,
    data_dir: PathBuf,
    job_id: u64,
    input: PathBuf,
    ocr: bool,
    provider_override: Option<String>,
    cancelled: Arc<AtomicBool>,
) {
    let vault_settings = settings::load_vault(&vault);
    let device = settings::load_device(&data_dir);
    let provider = provider_override.unwrap_or_else(|| vault_settings.provider.clone());

    let h = host.clone();
    let mut log = move |line: String| {
        h.log_info(&line);
        h.ui_post(
            WINDOW,
            json!({ "type": "job", "job_id": job_id, "event": "log", "line": line }),
        );
    };
    let h = host.clone();
    let mut progress = move |stage: &str, pt: Option<(usize, usize)>| {
        let mut payload = json!({
            "type": "job", "job_id": job_id, "event": "progress", "stage": stage,
        });
        if let Some((page, total)) = pt {
            payload["page"] = json!(page);
            payload["total"] = json!(total);
        }
        h.ui_post(WINDOW, payload);
    };

    let engine: Option<Box<dyn OcrEngine>> = if ocr {
        match build_engine(&provider, &vault_settings, &device, &cancelled) {
            Ok(e) => Some(e),
            Err(e) => {
                host.ui_post(
                    WINDOW,
                    json!({ "type": "job", "job_id": job_id, "event": "failed", "error": e }),
                );
                return;
            }
        }
    } else {
        None
    };
    let calibre_bin = calibre::detect(device.calibre_path.as_deref()).map(|d| d.path);

    let input_abs = absolute_or_given(&input);
    let work = data_dir.join("work").join(work_dir_name(&input_abs));

    let mut ctx = PipelineCtx {
        vault_root: &vault,
        ebooks_root: &vault_settings.ebooks_root,
        work: &work,
        log: &mut log,
        progress: &mut progress,
        cancelled: &cancelled,
    };

    match pipeline::run_import(&mut ctx, &input, ocr, engine, calibre_bin.as_deref()) {
        Ok(dest) => {
            let dest_rel = dest_relative(&vault, &dest);
            host.ui_post(
                WINDOW,
                json!({ "type": "job", "job_id": job_id, "event": "done", "dest_rel": dest_rel }),
            );
        }
        Err(e) => {
            host.ui_post(
                WINDOW,
                json!({ "type": "job", "job_id": job_id, "event": "failed", "error": e }),
            );
        }
    }
}

impl sdk::NotemdPlugin for EbookImportPlugin {
    fn initialize(&mut self, _host: &sdk::Host, params: &proto::InitializeParams) {
        self.data_dir = PathBuf::from(&params.data_dir);
    }

    fn activate(&mut self, host: &sdk::Host, _p: &proto::ActivateParams) -> Result<(), String> {
        let inner = self.inner.clone();
        let host = host.clone();

        // Seed the vault SYNCHRONOUSLY from the shared config — a plain file
        // read, no host round-trip — so a command that arrives the instant
        // activation returns already has a vault to work with.
        let seeded = shared_config_vault();
        if let Some(root) = &seeded {
            inner.lock().unwrap().vault = Some(root.clone());
        }

        // MUST be spawned, never awaited inline: `$activate` is dispatched
        // synchronously on the protocol read loop, and the response to
        // `host.vault.info` can only be routed BY that loop.
        tokio::spawn(async move {
            let root = vault_from_host(&host).await.or(seeded);
            if let Some(root) = &root {
                host.log_info(&format!("ebook-import ready (vault={})", root.display()));
            } else {
                host.log_warn("no vault configured; ebook-import needs one");
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
        // The process is going away: cancel every in-flight job so a
        // background thread notices on its next check-between-stages
        // instead of continuing (harmlessly, since nothing reads its
        // output anymore) after the plugin process is torn down.
        let jobs: Vec<_> = self.inner.lock().unwrap().jobs.values().cloned().collect();
        for flag in jobs {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn execute_command(
        &mut self,
        host: &sdk::Host,
        params: &proto::ExecuteCommandParams,
    ) -> Result<Value, String> {
        match params.command.as_str() {
            "import" => self.cli_import(host, &params.context),
            other => Err(format!("unknown command '{other}'")),
        }
    }

    fn on_ui_request(&mut self, host: &sdk::Host, method: &str, params: Value) -> Result<Value, String> {
        match method {
            "detect_env" => self.detect_env(),
            "save_settings" => self.save_settings(&params),
            "import_start" => self.import_start(host, &params),
            "import_cancel" => self.import_cancel(&params),
            other => Err(format!("unknown ui method '{other}'")),
        }
    }
}

impl EbookImportPlugin {
    fn vault(&self) -> Result<PathBuf, String> {
        self.inner
            .lock()
            .unwrap()
            .vault
            .clone()
            .ok_or_else(|| NO_VAULT.to_string())
    }

    /// `ready: false` means the vault lookup is still in flight — the window
    /// retries rather than reporting "no calibre / no vault".
    fn detect_env(&self) -> Result<Value, String> {
        let (vault, checked) = {
            let g = self.inner.lock().unwrap();
            (g.vault.clone(), g.vault_checked)
        };
        let device = settings::load_device(&self.data_dir);
        let calibre_detected = calibre::detect(device.calibre_path.as_deref());
        let vault_settings = vault
            .as_ref()
            .map(|v| settings::load_vault(v))
            .unwrap_or_default();

        Ok(json!({
            "calibre": calibre_detected.map(|d| json!({ "path": d.path, "version": d.version })),
            "vault": vault.as_ref().map(|v| json!({ "root": v.to_string_lossy() })),
            "settings": vault_settings,
            "device": {
                "calibre_path": device.calibre_path,
                "baidu_api_key_set": !device.baidu_api_key.is_empty(),
                "baidu_secret_key_set": !device.baidu_secret_key.is_empty(),
            },
            "ready": checked,
        }))
    }

    fn save_settings(&self, params: &Value) -> Result<Value, String> {
        if let Some(vpatch) = params.get("vault") {
            let vault = self.vault()?;
            let existing = settings::load_vault(&vault);
            let merged = apply_vault_patch(&existing, vpatch)?;
            settings::save_vault(&vault, &merged).map_err(|e| e.to_string())?;
        }
        if let Some(dpatch) = params.get("device") {
            let existing = settings::load_device(&self.data_dir);
            let merged = apply_device_patch(&existing, dpatch);
            settings::save_device(&self.data_dir, &merged).map_err(|e| e.to_string())?;
        }
        Ok(json!({ "ok": true }))
    }

    fn import_start(&mut self, host: &sdk::Host, params: &Value) -> Result<Value, String> {
        let vault = self.vault()?;
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("import_start needs a 'path'")?
            .to_string();
        let ocr = params.get("ocr").and_then(|v| v.as_bool()).unwrap_or(false);
        let provider_override = params
            .get("provider")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let job_id = {
            let mut g = self.inner.lock().unwrap();
            let id = g.next_job;
            g.next_job += 1;
            id
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        self.inner
            .lock()
            .unwrap()
            .jobs
            .insert(job_id, cancelled.clone());

        let host = host.clone();
        let data_dir = self.data_dir.clone();
        std::thread::spawn(move || {
            run_job(
                host,
                vault,
                data_dir,
                job_id,
                PathBuf::from(path),
                ocr,
                provider_override,
                cancelled,
            );
        });

        Ok(json!({ "job_id": job_id }))
    }

    /// Unknown job ids are still `{ok:true}` — cancelling a job that already
    /// finished (or that the UI never actually started, e.g. a stale button
    /// double-click) is not an error the window needs to react to.
    fn import_cancel(&self, params: &Value) -> Result<Value, String> {
        let job_id = params
            .get("job_id")
            .and_then(|v| v.as_u64())
            .ok_or("import_cancel needs a 'job_id'")?;
        if let Some(flag) = self.inner.lock().unwrap().jobs.get(&job_id) {
            flag.store(true, Ordering::Relaxed);
        }
        Ok(json!({ "ok": true }))
    }

    /// CLI entry point: `notemd ebook-import <file> [--ocr] [--ocr-provider p] [--root r]`.
    /// Blocks until the import finishes (the host budgets 300s for a CLI
    /// command, and a single-book import fits that) — but runs the actual
    /// work on a plain `std::thread`, joined here, rather than inline.
    ///
    /// `command.execute` is dispatched synchronously ON the tokio protocol
    /// read loop (see the module doc). Both OCR engines build a
    /// `reqwest::blocking::Client` inside `ocr_pdf`, and `reqwest::blocking`
    /// panics if constructed from within a tokio runtime under
    /// `debug_assertions` — and even without that panic, running the
    /// pipeline (which can take minutes, e.g. Calibre conversion or OCR)
    /// inline here would block `$deactivate` and every other host↔plugin
    /// message for the whole import. Spawning a `std::thread` and `join`ing
    /// it keeps `cli_import`'s own contract ("blocks until done") while
    /// keeping the actual blocking I/O off the async runtime.
    fn cli_import(&mut self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let vault = self.vault()?;
        let file = cli_str(context, "file")
            .ok_or("usage: notemd ebook-import <file> [--ocr] [--ocr-provider PROVIDER] [--root ROOT]")?;
        let ocr = cli_flag(context, "ocr");
        let provider_override = cli_str(context, "ocr-provider");
        let root_override = cli_str(context, "root");

        let host = host.clone();
        let data_dir = self.data_dir.clone();

        let handle = std::thread::spawn(move || -> Result<(PathBuf, Vec<String>), String> {
            let mut vault_settings = settings::load_vault(&vault);
            if let Some(root) = root_override {
                vault_settings.ebooks_root = root;
            }
            let device = settings::load_device(&data_dir);
            let provider = provider_override.unwrap_or_else(|| vault_settings.provider.clone());
            let cancelled = Arc::new(AtomicBool::new(false));

            let engine: Option<Box<dyn OcrEngine>> = if ocr {
                Some(build_engine(&provider, &vault_settings, &device, &cancelled)?)
            } else {
                None
            };
            let calibre_bin = calibre::detect(device.calibre_path.as_deref()).map(|d| d.path);

            let input = PathBuf::from(&file);
            let input_abs = absolute_or_given(&input);
            let work = data_dir.join("work").join(work_dir_name(&input_abs));

            let mut lines: Vec<String> = Vec::new();
            let h = host.clone();
            let mut log = |line: String| {
                h.log_info(&line);
                lines.push(line);
            };
            let mut progress = |_: &str, _: Option<(usize, usize)>| {};

            let mut ctx = PipelineCtx {
                vault_root: &vault,
                ebooks_root: &vault_settings.ebooks_root,
                work: &work,
                log: &mut log,
                progress: &mut progress,
                cancelled: &cancelled,
            };

            let dest = pipeline::run_import(&mut ctx, &input, ocr, engine, calibre_bin.as_deref())?;
            Ok((dest, lines))
        });

        let (dest, lines) = handle
            .join()
            .map_err(|_| "ebook-import worker thread panicked".to_string())??;
        Ok(json!({ "dest": dest.to_string_lossy(), "log": lines }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    /// NOTEMD_SHARED_CONFIG is process-global, so tests that set it take turns.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ── save_settings merge rules ───────────────────────────────────────

    #[test]
    fn vault_patch_overwrites_only_present_nonempty_fields() {
        let existing = VaultSettings {
            ebooks_root: "ssot/ebooks".into(),
            provider: "wechat".into(),
            wechat_url: "http://old".into(),
        };
        let patch = json!({ "provider": "baidu" });
        let merged = apply_vault_patch(&existing, &patch).expect("valid patch must not error");
        assert_eq!(merged.provider, "baidu");
        assert_eq!(merged.ebooks_root, "ssot/ebooks");
        assert_eq!(merged.wechat_url, "http://old");
    }

    #[test]
    fn vault_patch_empty_string_leaves_field_unchanged() {
        let existing = VaultSettings::default();
        let patch = json!({ "wechat_url": "" });
        let merged = apply_vault_patch(&existing, &patch).expect("valid patch must not error");
        assert_eq!(merged.wechat_url, existing.wechat_url);
    }

    // ── Finding 4: ebooks_root patches that could escape the vault ─────────

    #[test]
    fn vault_patch_rejects_an_absolute_ebooks_root() {
        let existing = VaultSettings::default();
        let patch = json!({ "ebooks_root": "/etc" });
        assert!(apply_vault_patch(&existing, &patch).is_err());
    }

    #[test]
    fn vault_patch_rejects_an_ebooks_root_with_a_parent_dir_component() {
        let existing = VaultSettings::default();
        let patch = json!({ "ebooks_root": "../escape" });
        assert!(apply_vault_patch(&existing, &patch).is_err());
    }

    #[test]
    fn vault_patch_accepts_a_vault_relative_ebooks_root() {
        let existing = VaultSettings::default();
        let patch = json!({ "ebooks_root": "books/sub" });
        let merged = apply_vault_patch(&existing, &patch).expect("vault-relative path must be accepted");
        assert_eq!(merged.ebooks_root, "books/sub");
    }

    #[test]
    fn secret_patch_rules() {
        assert_eq!(apply_secret_patch("old", ""), "old");
        assert_eq!(apply_secret_patch("old", "-"), "");
        assert_eq!(apply_secret_patch("old", "new"), "new");
    }

    #[test]
    fn device_patch_calibre_path_rules() {
        let existing = DeviceSettings {
            calibre_path: Some("/usr/bin/ebook-convert".into()),
            baidu_api_key: "k".into(),
            baidu_secret_key: "s".into(),
        };
        // missing key: unchanged
        assert_eq!(
            apply_device_patch(&existing, &json!({})).calibre_path,
            existing.calibre_path
        );
        // null: unchanged
        assert_eq!(
            apply_device_patch(&existing, &json!({ "calibre_path": null })).calibre_path,
            existing.calibre_path
        );
        // "": cleared
        assert_eq!(
            apply_device_patch(&existing, &json!({ "calibre_path": "" })).calibre_path,
            None
        );
        // other string: set
        assert_eq!(
            apply_device_patch(&existing, &json!({ "calibre_path": "/opt/ebook-convert" })).calibre_path,
            Some("/opt/ebook-convert".to_string())
        );
    }

    #[test]
    fn device_patch_secret_rules() {
        let existing = DeviceSettings {
            calibre_path: None,
            baidu_api_key: "k".into(),
            baidu_secret_key: "s".into(),
        };
        let merged = apply_device_patch(
            &existing,
            &json!({ "baidu_api_key": "-", "baidu_secret_key": "new" }),
        );
        assert_eq!(merged.baidu_api_key, "");
        assert_eq!(merged.baidu_secret_key, "new");
    }

    // ── CLI context helpers ──────────────────────────────────────────────

    #[test]
    fn reads_cli_args_from_the_nested_shape() {
        let c = json!({ "cli": { "args": { "file": "book.epub" },
                                 "flags": { "ocr": true } } });
        assert_eq!(cli_str(&c, "file").as_deref(), Some("book.epub"));
        assert!(cli_flag(&c, "ocr"));
    }

    #[test]
    fn reads_cli_args_from_a_flattened_shape() {
        let c = json!({ "file": "book.pdf", "ocr": false });
        assert_eq!(cli_str(&c, "file").as_deref(), Some("book.pdf"));
        assert!(!cli_flag(&c, "ocr"));
    }

    #[test]
    fn missing_cli_args_read_as_absent_rather_than_empty_strings() {
        let c = json!({ "cli": { "args": { "file": "" } } });
        assert_eq!(cli_str(&c, "file"), None);
        assert!(!cli_flag(&c, "ocr"));
    }

    // ── shared config vault fallback ─────────────────────────────────────

    #[test]
    fn reads_the_vault_out_of_the_shared_config() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.json");
        std::fs::write(&p, r#"{"version":1,"sotvault":"/Users/x/git/sotvault"}"#).unwrap();
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
    }

    // ── dest_relative ────────────────────────────────────────────────────

    #[test]
    fn dest_relative_strips_the_vault_prefix() {
        let vault = Path::new("/vault");
        let dest = Path::new("/vault/ssot/ebooks/2026-08/Title");
        assert_eq!(dest_relative(vault, dest), "ssot/ebooks/2026-08/Title");
    }

    // ── work_dir_name ────────────────────────────────────────────────────

    #[test]
    fn work_dir_name_is_stable_for_the_same_path_and_differs_across_different_ones() {
        let a1 = work_dir_name(Path::new("/tmp/x/chapter1.pdf"));
        let a2 = work_dir_name(Path::new("/tmp/x/chapter1.pdf"));
        let b = work_dir_name(Path::new("/tmp/y/chapter1.pdf"));
        assert_eq!(
            a1, a2,
            "the same absolute path must always hash to the same work-dir name"
        );
        assert_ne!(
            a1, b,
            "two different files that happen to share a stem must NOT share a work dir"
        );
        assert!(a1.starts_with("chapter1_"));
        assert!(a1.ends_with("_temp"));
    }

    // ── serve_io integration: activation must not block the protocol loop ─

    /// Mirrors claude-agent's `activate_never_blocks_the_protocol_loop`: the
    /// host here never answers `host.vault.info`, so `detect_env` must still
    /// come back promptly with `ready: false` rather than hanging.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn activate_never_blocks_the_protocol_loop() {
        let _env = env_guard();
        std::env::set_var("NOTEMD_SHARED_CONFIG", "/nonexistent/ebook-import-test.json");

        let (mut to_plugin, plugin_stdin) = tokio::io::duplex(16 * 1024);
        let (plugin_stdout, from_plugin) = tokio::io::duplex(16 * 1024);
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(sdk::serve_io(EbookImportPlugin::new(), plugin_stdin, plugin_stdout));
        });

        to_plugin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$activate\",\"params\":{\"event\":\"onCommand:open\"}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ui.request\",\"params\":{\"method\":\"detect_env\",\"params\":{}}}\n",
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
            panic!("the plugin closed its stdout without answering detect_env");
        })
        .await
        .expect("detect_env went unanswered — activate blocked the read loop");

        assert_eq!(answered["result"]["ready"], false);
        assert_eq!(answered["result"]["vault"], Value::Null);
        std::env::remove_var("NOTEMD_SHARED_CONFIG");
    }

    /// `plugin_v2_execute` activates the plugin and can run a command right
    /// after; here that command (`import_start`) must see the vault seeded
    /// from the shared config synchronously, not wait behind the host's
    /// (never-arriving, in this test) `host.vault.info` answer.
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
                .block_on(sdk::serve_io(EbookImportPlugin::new(), plugin_stdin, plugin_stdout));
        });

        to_plugin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$activate\",\"params\":{\"event\":\"onCommand:open\"}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ui.request\",\"params\":{\"method\":\"import_cancel\",\"params\":{\"job_id\":999}}}\n",
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
                if v.get("id").and_then(|i| i.as_u64()) == Some(2) {
                    return v;
                }
            }
            panic!("the plugin closed its stdout without answering import_cancel");
        })
        .await
        .expect("import_cancel went unanswered");

        assert_eq!(answered["result"]["ok"], true);
        std::env::remove_var("NOTEMD_SHARED_CONFIG");
    }

    /// Pins the exact `host.ui.post` job-event contract Task 9's UI depends
    /// on: `{"window_id":"main","payload":{"type":"job","job_id":N,
    /// "event":"failed","error":"…"}}`. Uses a path that doesn't exist (and
    /// no Calibre override), so the run fails deterministically regardless
    /// of whether this machine happens to have a real Calibre install —
    /// either "calibre not found" or a conversion error, either way a
    /// `failed` event with *some* string `error`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn import_start_pushes_a_failed_job_event_with_the_exact_contract_shape() {
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

        // A fresh, isolated data_dir via $initialize — the default
        // `EbookImportPlugin::new()` data_dir is the process-wide temp dir,
        // which must not leak a real device.json (e.g. a genuine
        // calibre_path) into this test.
        let data_dir = tempfile::tempdir().unwrap();

        let (mut to_plugin, plugin_stdin) = tokio::io::duplex(16 * 1024);
        let (plugin_stdout, from_plugin) = tokio::io::duplex(16 * 1024);
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(sdk::serve_io(EbookImportPlugin::new(), plugin_stdin, plugin_stdout));
        });

        let init = json!({
            "jsonrpc": "2.0", "id": 1, "method": "$initialize",
            "params": {
                "protocol_version": 2, "host_version": "1.0.0", "locale": "en",
                "theme": "light", "plugin_root": "/tmp/plugin",
                "data_dir": data_dir.path().to_string_lossy(),
            }
        });
        to_plugin
            .write_all(format!("{init}\n").as_bytes())
            .await
            .unwrap();
        to_plugin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"$activate\",\"params\":{\"event\":\"onCommand:open\"}}\n\
                  {\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ui.request\",\"params\":{\"method\":\"import_start\",\"params\":{\"path\":\"/nonexistent/should-not-exist.pdf\",\"ocr\":false}}}\n",
            )
            .await
            .unwrap();

        let mut lines = BufReader::new(from_plugin).lines();
        let failed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while let Ok(Some(line)) = lines.next_line().await {
                let v: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if v.get("method").and_then(|m| m.as_str()) == Some("host.ui.post")
                    && v["params"]["payload"]["event"] == "failed"
                {
                    return v;
                }
            }
            panic!("no failed job event was ever pushed");
        })
        .await
        .expect("timed out waiting for the failed job event");

        assert_eq!(failed["params"]["window_id"], "main");
        let payload = &failed["params"]["payload"];
        assert_eq!(payload["type"], "job");
        assert_eq!(payload["job_id"], 1);
        assert_eq!(payload["event"], "failed");
        assert!(
            payload.get("error").and_then(|e| e.as_str()).is_some(),
            "expected a string 'error', got: {payload}"
        );

        std::env::remove_var("NOTEMD_SHARED_CONFIG");
    }

    #[test]
    fn vault_guard_reports_no_vault_configured() {
        // A bare Host isn't constructible outside serve_io (its fields are
        // private), so the full ui.request round trip for "no vault" is
        // covered by the serve_io tests above; this unit test pins the
        // guard `on_ui_request` handlers all route through.
        let p = EbookImportPlugin::new();
        assert_eq!(p.vault().unwrap_err(), NO_VAULT);
    }
}
