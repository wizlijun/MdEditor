//! Spawn the built binary, hand it a Request, assert it writes a non-empty
//! file to `output_path` and reports success.
//!
//! Task 10 verifies stdin/stdout plumbing only — the file content is the
//! wrapped HTML stub. Task 11 will tighten the assertion to require %PDF
//! magic bytes once the real pipeline lands.

use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn happy_path_writes_a_file() {
    let bin = env!("CARGO_BIN_EXE_md2pdf");
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let out_path = tmp.path().to_str().unwrap().to_string();
    drop(tmp); // we want the path to NOT exist when md2pdf runs

    let req = serde_json::json!({
        "command": "export",
        "context": {
            "tab": { "path": "/tmp/x.md", "filename": "x.md", "title": "X" },
            "rendered_html": "<h1>X</h1><p>hello</p>",
            "output_path": out_path,
        }
    });

    let mut child = Command::new(bin)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn md2pdf");
    {
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(req.to_string().as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "md2pdf exit non-zero; stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| panic!("response not JSON: {stdout}"));
    assert_eq!(v["success"], true);

    let bytes = std::fs::read(&out_path).expect("output file exists");
    assert!(
        bytes.len() > 1024,
        "PDF should be ≥ 1 KB once produced; got {} bytes",
        bytes.len(),
    );
    assert!(
        bytes.starts_with(b"%PDF"),
        "expected PDF magic bytes, got: {:?}",
        &bytes[..bytes.len().min(8)],
    );

    let _ = std::fs::remove_file(&out_path);
}
