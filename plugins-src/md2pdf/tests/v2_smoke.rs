//! End-to-end smoke test for the v2 service binary: drives the NDJSON
//! JSON-RPC loop over real pipes and asserts a real PDF is produced by the
//! sibling v1 renderer (macOS WKWebView pipeline).
//!
//! Both binaries are copied into one tempdir so the v2 sibling lookup
//! (`current_exe().parent()/md2pdf`) resolves exactly like an installed
//! plugin package's `bin/` directory.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Lines the plugin writes to stdout, pumped by a reader thread so every
/// receive can carry a timeout.
struct Client {
    rx: mpsc::Receiver<String>,
    notifications: Vec<Value>,
}

impl Client {
    /// Next JSON-RPC *response*; notifications encountered on the way are
    /// recorded in `self.notifications`.
    fn next_response(&mut self, timeout: Duration) -> Value {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("timed out waiting for a response");
            let line = self
                .rx
                .recv_timeout(remaining)
                .expect("plugin stdout closed or timed out while awaiting response");
            let v: Value =
                serde_json::from_str(&line).unwrap_or_else(|e| panic!("non-JSON line: {line} ({e})"));
            if v.get("method").is_some() {
                self.notifications.push(v);
                continue;
            }
            return v;
        }
    }

    fn toast_count(&self) -> usize {
        self.notifications
            .iter()
            .filter(|n| n["method"] == json!("host.toast"))
            .count()
    }
}

fn send(stdin: &mut impl Write, v: Value) {
    let mut buf = serde_json::to_vec(&v).unwrap();
    buf.push(b'\n');
    stdin.write_all(&buf).unwrap();
    stdin.flush().unwrap();
}

#[test]
fn v2_service_exports_pdf_via_sibling_v1_renderer() {
    // ── Stage both binaries as siblings in a tempdir ────────────────────
    let dir = tempfile::tempdir().unwrap();
    let v2_path = dir.path().join("md2pdf-v2");
    let v1_path = dir.path().join("md2pdf");
    std::fs::copy(env!("CARGO_BIN_EXE_md2pdf-v2"), &v2_path).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_md2pdf"), &v1_path).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [&v2_path, &v1_path] {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    let out_path = dir.path().join("out.pdf");
    let out_path_str = out_path.to_str().unwrap().to_string();

    // ── Spawn the v2 service ────────────────────────────────────────────
    let mut child = Command::new(&v2_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn md2pdf-v2");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let (tx, rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let mut client = Client { rx, notifications: Vec::new() };

    // ── $initialize ─────────────────────────────────────────────────────
    send(&mut stdin, json!({
        "jsonrpc": "2.0", "id": 1, "method": "$initialize",
        "params": {
            "protocol_version": 2, "host_version": "6.717.0", "locale": "en",
            "theme": "light",
            "plugin_root": dir.path().to_str().unwrap(),
            "data_dir": dir.path().to_str().unwrap(),
        }
    }));
    let resp = client.next_response(Duration::from_secs(10));
    assert_eq!(resp["id"], json!(1));
    assert_eq!(resp["result"], json!({"ok": true}), "initialize failed: {resp}");

    // ── $activate ───────────────────────────────────────────────────────
    send(&mut stdin, json!({
        "jsonrpc": "2.0", "id": 2, "method": "$activate",
        "params": { "event": "onCommand:export" }
    }));
    let resp = client.next_response(Duration::from_secs(10));
    assert_eq!(resp["id"], json!(2));
    assert_eq!(resp["result"], json!({"ok": true}), "activate failed: {resp}");

    // ── command.execute → real render via sibling v1 ────────────────────
    send(&mut stdin, json!({
        "jsonrpc": "2.0", "id": 3, "method": "command.execute",
        "params": {
            "command": "export",
            "context": {
                "tab": { "path": "/tmp/x.md", "filename": "x.md", "title": "X" },
                "rendered_html": "<h1>X</h1><p>hello from v2</p>",
                "output_path": out_path_str,
            }
        }
    }));
    let resp = client.next_response(Duration::from_secs(120));
    assert_eq!(resp["id"], json!(3));
    assert!(resp["error"].is_null(), "execute errored: {resp}");
    assert_eq!(
        resp["result"]["path"],
        json!(out_path_str),
        "execute result must carry the output path: {resp}"
    );

    // Exactly one success toast, emitted before the execute response.
    assert_eq!(client.toast_count(), 1, "notifications: {:?}", client.notifications);
    let toast = client
        .notifications
        .iter()
        .find(|n| n["method"] == json!("host.toast"))
        .unwrap();
    assert_eq!(toast["id"], Value::Null, "toast must be a notification (no id)");
    assert_eq!(toast["params"]["level"], json!("success"));
    assert!(
        toast["params"]["message"].as_str().unwrap().contains(&out_path_str),
        "toast should mention the output path: {toast}"
    );

    // Real PDF on disk.
    let bytes = std::fs::read(&out_path).expect("output PDF exists");
    assert!(
        bytes.starts_with(b"%PDF"),
        "expected PDF magic bytes, got: {:?}",
        &bytes[..bytes.len().min(8)],
    );

    // ── $deactivate → clean exit 0 ──────────────────────────────────────
    send(&mut stdin, json!({
        "jsonrpc": "2.0", "id": 4, "method": "$deactivate", "params": {}
    }));
    let resp = client.next_response(Duration::from_secs(10));
    assert_eq!(resp["id"], json!(4));
    assert_eq!(resp["result"], json!({"ok": true}), "deactivate failed: {resp}");

    // Stdout reaches EOF (reader thread finishes) and the process exits 0.
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < deadline, "md2pdf-v2 did not exit after $deactivate");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "md2pdf-v2 exited non-zero: {status}");
    reader.join().unwrap();
}
