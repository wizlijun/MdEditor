//! Plugin registry client (子项目③ Task 2): the read side of the marketplace.
//!
//! - [`RegistryIndex`] / [`RegistryEntry`] mirror the CF Worker's
//!   `GET /api/index.json` payload (and `gen-plugin-index.mjs` output).
//! - [`parse_index`] is a pure, testable JSON decode — the network layer
//!   ([`fetch_index`]) is a thin reqwest wrapper on top so tests never touch
//!   the network.
//! - [`download`] pulls a `.notemdpkg`, capping the read at [`MAX_PKG_BYTES`]
//!   so a hostile or misconfigured registry can't exhaust memory.
//! - [`report_install`] is fire-and-forget install telemetry (all errors
//!   swallowed — a stats POST must never break an install).
//!
//! Base URL is [`DEFAULT_REGISTRY`], overridable via settings.json
//! `plugins_v2.registry_url` (read exactly like `read_saved_locale`).
//!
//! Signature verification uses [`PLUGIN_REGISTRY_PUBKEY`]; the install path in
//! commands.rs feeds it to `installer::verify_and_stage`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Registry base URL when settings.json has no `plugins_v2.registry_url`.
pub const DEFAULT_REGISTRY: &str = "https://plugins.notemd.net";

/// Hard ceiling on a downloaded package (50 MiB). Reads beyond this abort with
/// an error rather than buffering unbounded bytes from an untrusted server.
pub const MAX_PKG_BYTES: u64 = 50 * 1024 * 1024;

/// Whole-request timeout for the small JSON calls (index fetch, install ping).
const NET_TIMEOUT_SECS: u64 = 10;

/// Connect timeout for a package download — an unreachable registry still
/// fails fast.
const DOWNLOAD_CONNECT_TIMEOUT_SECS: u64 = 10;

/// Idle timeout *between body chunks* for a package download. A whole-request
/// timeout must NOT be used here: it kills a healthy but slow transfer partway
/// through the body, which reqwest surfaces as the opaque "error decoding
/// response body". Multi-megabyte packages routinely take longer than any fixed
/// request budget on a modest connection (or through a proxy), so the bound is
/// on *stalling*, not on total duration: as long as bytes keep arriving the
/// download runs to completion however long it takes.
const DOWNLOAD_IDLE_TIMEOUT_SECS: u64 = 60;

/// Production plugin-registry pubkey (minisign key id 2BAFE555935FE0A9). This
/// is the base64 line (no `untrusted comment:` prefix) of
/// `~/.tauri/notemd-plugins.pub`; the matching private key signs every
/// `.notemdpkg` published by scripts/release-plugins.sh and is NOT in the
/// repo. `installer::verify_and_stage` accepts exactly this form.
pub const PLUGIN_REGISTRY_PUBKEY: &str =
    "RWSp4F+TVeWvKxkXXQIfd9pceHoU1UGBbDCC2BYOtOjeUdtf2X+YG2WT";

/// The full registry index (`GET /api/index.json`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistryIndex {
    pub plugins: Vec<RegistryEntry>,
}

/// One publishable plugin version. Field set matches `gen-plugin-index.mjs`
/// (Task 5) and the CF Worker (Task 4). `sha256`/`download` are keyed by arch
/// (e.g. `aarch64-apple-darwin`) so a multi-arch package resolves per host.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistryEntry {
    pub id: String,
    pub version: String,
    pub min_host: String,
    pub archs: Vec<String>,
    pub size: u64,
    /// arch → lowercase hex sha256 of that arch's `.notemdpkg`.
    pub sha256: BTreeMap<String, String>,
    pub name: String,
    /// Stable capability group derived from the manifest's primary menu
    /// `submenu`. Old registry entries omit it and fall back to Other.
    #[serde(default)]
    pub category: Option<String>,
    pub description: Option<String>,
    pub i18n: Option<serde_json::Value>,
    pub icon_url: Option<String>,
    pub changelog_url: Option<String>,
    /// arch → download URL of that arch's `.notemdpkg`.
    pub download: BTreeMap<String, String>,
}

/// Pure decode of an index JSON body. Kept separate from the network so the
/// happy/invalid paths are unit-testable without a live registry.
pub fn parse_index(bytes: &[u8]) -> Result<RegistryIndex, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("invalid registry index: {e}"))
}

/// Flatten an error and its `source()` chain into one line.
///
/// `reqwest::Error`'s own Display stops at "error sending request for url (…)"
/// — the part that says *what actually went wrong* (connection refused, dns
/// failure, TLS) lives in the source chain. Reporting only the head turns every
/// network fault into the same unactionable sentence.
fn error_chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = e.to_string();
    let mut cursor = e.source();
    while let Some(src) = cursor {
        let text = src.to_string();
        // Skip links that only restate what the caller already printed.
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        cursor = src.source();
    }
    out
}

/// A note about the proxy, when one is configured.
///
/// reqwest routes through `HTTPS_PROXY` / `HTTP_PROXY` silently, so a stale
/// value fails exactly like a dead registry: same message, completely different
/// fix. Naming the variable and its value turns a bug report into a one-line
/// correction. Checked in reqwest's own precedence order for an https URL.
fn proxy_hint() -> String {
    for key in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"] {
        match std::env::var(key) {
            Ok(value) if !value.trim().is_empty() => return format!(" (proxied via {key}={value})"),
            _ => continue,
        }
    }
    String::new()
}

/// Network errors, reported so the reader can act on them.
fn net_err(what: &str, e: &(dyn std::error::Error + 'static)) -> String {
    format!("{what}: {}{}", error_chain(e), proxy_hint())
}

/// `GET {base}/api/index.json` → [`parse_index`]. 10s timeout.
pub async fn fetch_index(base_url: &str) -> Result<RegistryIndex, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(NET_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    fetch_index_via(&client, base_url).await
}

/// [`fetch_index`] with the client injected — same rationale as
/// [`download_via`]: a caller that wants to probe a loopback test server
/// (`cli::doctor`'s registry health check, in tests) needs a client with
/// `.no_proxy()`, since reqwest otherwise honours the system proxy by
/// default and a stale/misbehaving one would turn a "server unreachable"
/// test into a flaky pass or a hang.
pub async fn fetch_index_via(client: &reqwest::Client, base_url: &str) -> Result<RegistryIndex, String> {
    let url = format!("{}/api/index.json", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| net_err("fetch index", &e))?;
    if !resp.status().is_success() {
        return Err(format!("registry returned {} for {url}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| net_err("read index body", &e))?;
    parse_index(&bytes)
}

/// `GET url` → package bytes, aborting if the body exceeds [`MAX_PKG_BYTES`].
/// Reads chunk-by-chunk via [`reqwest::Response::chunk`] so an oversized or
/// `Content-Length`-lying server can't force us to buffer more than the cap
/// (the check runs on each chunk, before appending it).
///
/// Bounded by connect + per-chunk idle timeouts rather than a whole-request
/// one — see [`DOWNLOAD_IDLE_TIMEOUT_SECS`].
pub async fn download(url: &str) -> Result<Vec<u8>, String> {
    download_reporting(url, |_, _| {}).await
}

/// [`download`], calling `on_progress(received, total)` after every chunk so a
/// caller can drive a progress bar. `total` is the advertised `Content-Length`
/// and is `None` when the server doesn't send one (a chunked response), in
/// which case only the running byte count is known.
pub async fn download_reporting<F: FnMut(u64, Option<u64>)>(
    url: &str,
    on_progress: F,
) -> Result<Vec<u8>, String> {
    download_with(
        url,
        std::time::Duration::from_secs(DOWNLOAD_CONNECT_TIMEOUT_SECS),
        std::time::Duration::from_secs(DOWNLOAD_IDLE_TIMEOUT_SECS),
        on_progress,
    )
    .await
}

/// [`download`] with injectable timeouts, so the idle-vs-total semantics are
/// testable in milliseconds instead of tens of seconds.
pub async fn download_with<F: FnMut(u64, Option<u64>)>(
    url: &str,
    connect_timeout: std::time::Duration,
    idle_timeout: std::time::Duration,
    on_progress: F,
) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .read_timeout(idle_timeout)
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    download_via(&client, url, on_progress).await
}

/// The streaming read itself, with the client injected. Split out so tests can
/// supply a client that bypasses the system proxy (reqwest honours it by
/// default, which is right in production and fatal for a loopback test server).
pub async fn download_via<F: FnMut(u64, Option<u64>)>(
    client: &reqwest::Client,
    url: &str,
    mut on_progress: F,
) -> Result<Vec<u8>, String> {
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| net_err("download", &e))?;
    if !resp.status().is_success() {
        return Err(format!("download returned {} for {url}", resp.status()));
    }
    // Fast reject on an advertised size over the cap (cheap early-out).
    if let Some(len) = resp.content_length() {
        if len > MAX_PKG_BYTES {
            return Err(format!(
                "package too large: {len} bytes exceeds {MAX_PKG_BYTES} cap"
            ));
        }
    }
    let total = resp.content_length();
    let mut out: Vec<u8> = Vec::new();
    on_progress(0, total);
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| net_err("download stream", &e))?
    {
        if out.len() as u64 + chunk.len() as u64 > MAX_PKG_BYTES {
            return Err(format!(
                "package exceeds {MAX_PKG_BYTES} byte cap while streaming"
            ));
        }
        out.extend_from_slice(&chunk);
        on_progress(out.len() as u64, total);
    }
    Ok(out)
}

/// `POST {base}/api/stats/install {id,version}` — fire-and-forget. Every error
/// (build/send/status) is swallowed: install telemetry must never surface to
/// or block the user.
pub async fn report_install(base_url: &str, id: &str, version: &str) {
    let url = format!("{}/api/stats/install", base_url.trim_end_matches('/'));
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(NET_TIMEOUT_SECS))
        .build()
    else {
        return;
    };
    let _ = client
        .post(&url)
        .json(&serde_json::json!({ "id": id, "version": version }))
        .send()
        .await;
}

/// Pure resolver for the registry base URL against an explicit config dir:
/// settings.json `plugins_v2.registry_url` override, else [`DEFAULT_REGISTRY`].
/// Read exactly like `read_saved_locale` (fails closed to the default on any
/// read/parse error). The CLI (no AppHandle) calls this with
/// `cli::resolve_config_dir()`; the AppHandle version wraps it.
pub fn registry_base_url_at(config_dir: &std::path::Path) -> String {
    let Ok(text) = std::fs::read_to_string(config_dir.join("settings.json")) else {
        return DEFAULT_REGISTRY.to_string();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return DEFAULT_REGISTRY.to_string();
    };
    json.get("plugins_v2.registry_url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| DEFAULT_REGISTRY.to_string())
}

/// AppHandle wrapper over [`registry_base_url_at`]: resolves the app config dir,
/// then delegates. On resolution failure returns [`DEFAULT_REGISTRY`].
pub fn registry_base_url<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> String {
    use tauri::Manager;
    let Ok(dir) = app.path().app_config_dir() else {
        return DEFAULT_REGISTRY.to_string();
    };
    registry_base_url_at(&dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two-level error whose head says nothing useful — the shape
    /// `reqwest::Error` has.
    #[derive(Debug)]
    struct Layered(&'static str, Option<Box<Layered>>);
    impl std::fmt::Display for Layered {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }
    impl std::error::Error for Layered {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1.as_deref().map(|e| e as &(dyn std::error::Error + 'static))
        }
    }

    #[test]
    fn error_chain_reaches_the_actual_cause() {
        let e = Layered(
            "error sending request for url (https://plugins.notemd.net/api/index.json)",
            Some(Box::new(Layered(
                "tcp connect error",
                Some(Box::new(Layered("connection refused (os error 10061)", None))),
            ))),
        );
        let s = error_chain(&e);
        assert!(s.contains("error sending request"), "{s}");
        assert!(s.contains("tcp connect error"), "{s}");
        // The whole point: the reason a user can act on must survive.
        assert!(s.contains("os error 10061"), "{s}");
    }

    #[test]
    fn error_chain_does_not_repeat_a_link_already_shown() {
        let e = Layered("same text", Some(Box::new(Layered("same text", None))));
        assert_eq!(error_chain(&e), "same text");
    }

    #[test]
    fn error_chain_handles_a_lone_error() {
        assert_eq!(error_chain(&Layered("alone", None)), "alone");
    }

    /// `proxy_hint` reads process-wide env, so these two cases share one test —
    /// splitting them would let a parallel run observe the other's mutation.
    #[test]
    fn proxy_hint_names_the_variable_when_set() {
        let keys = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"];
        let saved: Vec<_> = keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        for k in keys {
            std::env::remove_var(k);
        }

        assert_eq!(proxy_hint(), "", "no proxy set → no hint");

        std::env::set_var("HTTPS_PROXY", "http://127.0.0.1:1087");
        let hint = proxy_hint();
        assert!(hint.contains("HTTPS_PROXY"), "{hint}");
        assert!(hint.contains("127.0.0.1:1087"), "{hint}");

        // Blank must count as unset, not as "proxied via ''".
        std::env::set_var("HTTPS_PROXY", "   ");
        assert_eq!(proxy_hint(), "");

        for (k, v) in saved {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }

    fn valid_index_json() -> &'static str {
        r#"{
          "plugins": [
            {
              "id": "notemd.md2pdf",
              "version": "1.2.0",
              "min_host": ">=0.1.0",
              "archs": ["aarch64-apple-darwin", "x86_64-apple-darwin"],
              "size": 1048576,
              "sha256": {
                "aarch64-apple-darwin": "aa",
                "x86_64-apple-darwin": "bb"
              },
              "name": "Export to PDF",
              "category": "import-export",
              "description": "Render the current note to PDF.",
              "i18n": { "zh": { "name": "导出 PDF" } },
              "icon_url": "https://plugins.notemd.net/icons/md2pdf.png",
              "changelog_url": null,
              "download": {
                "aarch64-apple-darwin": "https://plugins.notemd.net/api/download/notemd.md2pdf/1.2.0/aarch64-apple-darwin",
                "x86_64-apple-darwin": "https://plugins.notemd.net/api/download/notemd.md2pdf/1.2.0/x86_64-apple-darwin"
              }
            }
          ]
        }"#
    }

    #[test]
    fn parse_index_accepts_valid_payload() {
        let idx = parse_index(valid_index_json().as_bytes()).expect("valid index");
        assert_eq!(idx.plugins.len(), 1);
        let e = &idx.plugins[0];
        assert_eq!(e.id, "notemd.md2pdf");
        assert_eq!(e.version, "1.2.0");
        assert_eq!(e.min_host, ">=0.1.0");
        assert_eq!(e.archs.len(), 2);
        assert_eq!(e.sha256.get("aarch64-apple-darwin").unwrap(), "aa");
        assert_eq!(e.name, "Export to PDF");
        assert_eq!(e.category.as_deref(), Some("import-export"));
        assert!(e.description.is_some());
        assert!(e.i18n.is_some());
        assert!(e.icon_url.is_some());
        assert!(e.changelog_url.is_none());
        assert!(e.download.contains_key("x86_64-apple-darwin"));
    }

    #[test]
    fn parse_index_minimal_optional_fields_omitted() {
        // Only the required fields; every Option field absent.
        let json = r#"{
          "plugins": [
            { "id": "x.y", "version": "0.1.0", "min_host": ">=0.0.0",
              "archs": [], "size": 0, "sha256": {}, "name": "Y", "download": {} }
          ]
        }"#;
        let idx = parse_index(json.as_bytes()).expect("minimal index");
        let e = &idx.plugins[0];
        assert_eq!(e.id, "x.y");
        assert!(e.category.is_none());
        assert!(e.description.is_none());
        assert!(e.i18n.is_none());
        assert!(e.icon_url.is_none());
        assert!(e.changelog_url.is_none());
    }

    #[test]
    fn parse_index_rejects_malformed_json() {
        let err = parse_index(b"{ not json").unwrap_err();
        assert!(err.contains("invalid registry index"), "got {err}");
    }

    #[test]
    fn parse_index_rejects_missing_required_field() {
        // No `plugins` key.
        let err = parse_index(br#"{ "other": 1 }"#).unwrap_err();
        assert!(err.contains("invalid registry index"), "got {err}");

        // An entry missing `version`.
        let err = parse_index(
            br#"{ "plugins": [ { "id": "a.b", "min_host": ">=0", "archs": [], "size": 0, "sha256": {}, "name": "n", "download": {} } ] }"#,
        )
        .unwrap_err();
        assert!(err.contains("invalid registry index"), "got {err}");
    }

    #[test]
    fn registry_base_url_at_reads_override_or_defaults() {
        let dir = tempfile::tempdir().unwrap();
        // No settings.json ⇒ default.
        assert_eq!(registry_base_url_at(dir.path()), DEFAULT_REGISTRY);
        // Override present (trailing slash trimmed).
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{ "plugins_v2.registry_url": "https://mirror.example.com/" }"#,
        )
        .unwrap();
        assert_eq!(registry_base_url_at(dir.path()), "https://mirror.example.com");
        // Empty override falls back to the default.
        std::fs::write(
            dir.path().join("settings.json"),
            r#"{ "plugins_v2.registry_url": "" }"#,
        )
        .unwrap();
        assert_eq!(registry_base_url_at(dir.path()), DEFAULT_REGISTRY);
        // Malformed settings.json ⇒ default (fail closed).
        std::fs::write(dir.path().join("settings.json"), "{ not json").unwrap();
        assert_eq!(registry_base_url_at(dir.path()), DEFAULT_REGISTRY);
    }

    #[test]
    fn entry_round_trips_through_serialize() {
        // We return RegistryEntry-derived JSON to the frontend, so it must
        // serialize back out cleanly.
        let idx = parse_index(valid_index_json().as_bytes()).unwrap();
        let v = serde_json::to_value(&idx).unwrap();
        assert_eq!(v["plugins"][0]["id"], "notemd.md2pdf");
        assert_eq!(v["plugins"][0]["download"]["aarch64-apple-darwin"].is_string(), true);
    }

    /// Serves one chunked response, sleeping `gap` before each chunk, then
    /// returns the bound port. Chunked encoding is what lets the client see
    /// (and time) the body arriving piecemeal, like a real registry download.
    fn serve_chunked(chunks: usize, chunk_len: usize, gap: std::time::Duration) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                  Transfer-Encoding: chunked\r\n\r\n",
            );
            for _ in 0..chunks {
                std::thread::sleep(gap);
                let body = vec![b'x'; chunk_len];
                if sock
                    .write_all(format!("{:x}\r\n", chunk_len).as_bytes())
                    .is_err()
                    || sock.write_all(&body).is_err()
                    || sock.write_all(b"\r\n").is_err()
                {
                    return; // client hung up (an idle-timeout test)
                }
                let _ = sock.flush();
            }
            let _ = sock.write_all(b"0\r\n\r\n");
        });
        port
    }

    /// A client aimed at the loopback test server: same timeout shape as
    /// production, minus the system proxy (which would otherwise swallow the
    /// request — reqwest honours it by default).
    fn loopback_client(idle: std::time::Duration) -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_secs(5))
            .read_timeout(idle)
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn download_survives_a_transfer_longer_than_the_idle_timeout() {
        // The regression this guards: a whole-request timeout kills a healthy
        // but slow multi-megabyte download partway through the body, which
        // reqwest reports as the opaque "error decoding response body". Here
        // the transfer takes ~4x the idle bound in total while never stalling
        // for longer than it — that must succeed.
        let port = serve_chunked(8, 64, std::time::Duration::from_millis(120));
        let client = loopback_client(std::time::Duration::from_millis(250));
        let out = download_via(&client, &format!("http://127.0.0.1:{port}/pkg"), |_, _| {})
            .await
            .expect("slow-but-steady transfer must not time out");
        assert_eq!(out.len(), 8 * 64);
    }

    #[tokio::test]
    async fn download_reports_progress_as_bytes_land() {
        // The UI's progress bar is only useful if it advances DURING the
        // transfer, not once at the end — assert one callback per chunk with a
        // monotonically rising byte count.
        let port = serve_chunked(4, 64, std::time::Duration::from_millis(10));
        let client = loopback_client(std::time::Duration::from_secs(2));
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(u64, Option<u64>)>::new()));
        let sink = seen.clone();
        let out = download_via(&client, &format!("http://127.0.0.1:{port}/pkg"), move |r, t| {
            sink.lock().unwrap().push((r, t));
        })
        .await
        .expect("download");
        assert_eq!(out.len(), 4 * 64);

        let seen = seen.lock().unwrap();
        // One priming call at 0 plus one per chunk.
        assert_eq!(seen.first().map(|(r, _)| *r), Some(0), "primes at zero: {seen:?}");
        assert_eq!(seen.last().map(|(r, _)| *r), Some(256), "ends at the full size: {seen:?}");
        assert!(seen.len() >= 3, "advances during the transfer: {seen:?}");
        assert!(
            seen.windows(2).all(|w| w[0].0 <= w[1].0),
            "byte count never goes backwards: {seen:?}"
        );
    }

    #[tokio::test]
    async fn download_gives_up_when_the_body_stalls() {
        // The other half of the contract: a server that opens the response and
        // then goes quiet is still bounded, so a wedged registry can't hang the
        // install forever.
        let port = serve_chunked(2, 64, std::time::Duration::from_millis(600));
        let client = loopback_client(std::time::Duration::from_millis(150));
        let err = download_via(&client, &format!("http://127.0.0.1:{port}/pkg"), |_, _| {})
            .await
            .expect_err("a stalled body must abort");
        assert!(err.starts_with("download"), "unexpected error: {err}");
    }
}
