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
/// response body". Multi-megabyte packages (a plugin bundling a native dylib
/// runs to several MB) routinely take longer than any fixed request budget on
/// a modest connection, so the bound is on *stalling*, not on total duration.
const DOWNLOAD_IDLE_TIMEOUT_SECS: u64 = 30;

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

/// `GET {base}/api/index.json` → [`parse_index`]. 10s timeout.
pub async fn fetch_index(base_url: &str) -> Result<RegistryIndex, String> {
    let url = format!("{}/api/index.json", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(NET_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("fetch index: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("registry returned {} for {url}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("read index body: {e}"))?;
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
    download_with(
        url,
        std::time::Duration::from_secs(DOWNLOAD_CONNECT_TIMEOUT_SECS),
        std::time::Duration::from_secs(DOWNLOAD_IDLE_TIMEOUT_SECS),
    )
    .await
}

/// [`download`] with injectable timeouts, so the idle-vs-total semantics are
/// testable in milliseconds instead of tens of seconds.
pub async fn download_with(
    url: &str,
    connect_timeout: std::time::Duration,
    idle_timeout: std::time::Duration,
) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .read_timeout(idle_timeout)
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    download_via(&client, url).await
}

/// The streaming read itself, with the client injected. Split out so tests can
/// supply a client that bypasses the system proxy (reqwest honours it by
/// default, which is right in production and fatal for a loopback test server).
pub async fn download_via(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download: {e}"))?;
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
    let mut out: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("download stream: {e}"))?
    {
        if out.len() as u64 + chunk.len() as u64 > MAX_PKG_BYTES {
            return Err(format!(
                "package exceeds {MAX_PKG_BYTES} byte cap while streaming"
            ));
        }
        out.extend_from_slice(&chunk);
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
/// `cli::resolve_config_dir()`; the AppHandle version wraps it — mirrors the
/// `v2_flag_enabled_at` / `v2_flag_enabled` split.
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
        let out = download_via(&client, &format!("http://127.0.0.1:{port}/pkg"))
            .await
            .expect("slow-but-steady transfer must not time out");
        assert_eq!(out.len(), 8 * 64);
    }

    #[tokio::test]
    async fn download_gives_up_when_the_body_stalls() {
        // The other half of the contract: a server that opens the response and
        // then goes quiet is still bounded, so a wedged registry can't hang the
        // install forever.
        let port = serve_chunked(2, 64, std::time::Duration::from_millis(600));
        let client = loopback_client(std::time::Duration::from_millis(150));
        let err = download_via(&client, &format!("http://127.0.0.1:{port}/pkg"))
            .await
            .expect_err("a stalled body must abort");
        assert!(err.starts_with("download"), "unexpected error: {err}");
    }
}
