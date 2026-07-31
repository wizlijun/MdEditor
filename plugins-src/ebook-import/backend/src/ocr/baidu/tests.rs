use super::*;
use crate::ocr::OcrProgress;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

// ---------------------------------------------------------------------
// localize_images
// ---------------------------------------------------------------------

/// A minimal but real PNG magic-byte prefix, enough for `guess_image_ext`'s
/// sniffing branch and to look like plausible image bytes on disk.
const PNG_BYTES: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 1, 2, 3];

#[test]
fn localize_images_downloads_dedups_and_skips_failed_fetches() {
    let tmp = tempfile::tempdir().unwrap();
    let images_dir = tmp.path().join("images");

    let md = "See ![one](http://example.com/a/1.png) and ![two](https://example.com/2.png) \
              and ![broken](http://example.com/broken.png) and again ![dup](http://example.com/a/1.png).";

    let fetch = |url: &str| -> Result<Vec<u8>, String> {
        match url {
            "http://example.com/a/1.png" | "https://example.com/2.png" => Ok(PNG_BYTES.to_vec()),
            _ => Err(format!("fetch failed for {url}")),
        }
    };

    let result = localize_images(md, &fetch, &images_dir).expect("localize_images must succeed");

    assert_eq!(
        result,
        "See ![one](images/baidu_001.png) and ![two](images/baidu_002.png) \
         and ![broken](http://example.com/broken.png) and again ![dup](images/baidu_001.png)."
    );

    assert_eq!(
        std::fs::read(images_dir.join("baidu_001.png")).unwrap(),
        PNG_BYTES
    );
    assert_eq!(
        std::fs::read(images_dir.join("baidu_002.png")).unwrap(),
        PNG_BYTES
    );
    // The failed fetch must not have created a third file.
    assert!(!images_dir.join("baidu_003.png").exists());
    assert!(!images_dir.join("baidu_003.bin").exists());
}

#[test]
fn localize_images_with_no_remote_links_is_a_no_op_and_creates_no_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let images_dir = tmp.path().join("images");
    let md = "# Just text, no images here.";

    let fetch = |_: &str| -> Result<Vec<u8>, String> { panic!("fetch must not be called") };
    let result = localize_images(md, &fetch, &images_dir).unwrap();

    assert_eq!(result, md);
    assert!(!images_dir.exists());
}

// ---------------------------------------------------------------------
// precheck_size
// ---------------------------------------------------------------------

#[test]
fn precheck_size_allows_up_to_100mb_and_rejects_above() {
    assert!(precheck_size(50 * 1024 * 1024).is_ok());
    assert!(precheck_size(100 * 1024 * 1024).is_ok());
    assert!(precheck_size(100 * 1024 * 1024 + 1).is_err());
}

// ---------------------------------------------------------------------
// Full flow against a hand-rolled mock HTTP server.
// ---------------------------------------------------------------------

/// Starts a mock server on an ephemeral `127.0.0.1` port that hands out one
/// canned response body per accepted connection, in order, then closes each
/// connection. `build_responses` receives the server's own `http://host:port`
/// base URL so a response can embed a self-referential link (e.g. a
/// `markdown_url` pointing back at this same mock server). Adapted from
/// `crate::ocr::wechat::tests::start_mock_server`; this mock doesn't route
/// by path or method, it just answers connections in sequence, which is
/// enough since `BaiduOcr::ocr_pdf` always makes its requests in a fixed
/// order (oauth, submit, query*, markdown GET, image GET*).
fn start_mock_server(build_responses: impl FnOnce(&str) -> Vec<String>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock baidu server");
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");
    let responses = build_responses(&base_url);

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_for_thread = counter.clone();

    thread::spawn(move || {
        for body in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            counter_for_thread.fetch_add(1, Ordering::SeqCst);
            drain_request(&mut stream);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });

    (base_url, counter)
}

/// Reads (and discards) one HTTP request off `stream`: headers up to the
/// blank line, then exactly `Content-Length` more bytes if present. Good
/// enough for a mock -- these tests' request bodies are tiny form-encoded
/// strings or empty GETs.
fn drain_request(stream: &mut TcpStream) {
    let mut header_bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return,
            Ok(_) => {
                header_bytes.push(byte[0]);
                if header_bytes.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => return,
        }
    }

    let headers = String::from_utf8_lossy(&header_bytes).to_lowercase();
    let content_length: usize = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0);

    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        let _ = stream.read_exact(&mut body);
    }
}

fn engine(base_url: &str, poll_interval: Duration) -> BaiduOcr {
    BaiduOcr {
        api_key: "key".to_string(),
        secret_key: "secret".to_string(),
        oauth_url: format!("{base_url}/oauth"),
        submit_url: format!("{base_url}/submit"),
        query_url: format!("{base_url}/query"),
        poll_interval,
        token_cache: Mutex::new(None),
    }
}

fn write_fake_pdf(dir: &Path) -> PathBuf {
    let path = dir.join("book.pdf");
    std::fs::write(&path, b"%PDF-1.4 fake pdf contents").unwrap();
    path
}

#[test]
fn ocr_pdf_full_flow_oauth_submit_poll_then_downloads_markdown() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let pdf = write_fake_pdf(tmp.path());

    let (base_url, counter) = start_mock_server(|base| {
        vec![
            r#"{"access_token":"T","expires_in":2592000}"#.to_string(),
            r#"{"error_code":0,"result":{"task_id":"t1"}}"#.to_string(),
            r#"{"error_code":0,"result":{"status":"running"}}"#.to_string(),
            format!(
                r#"{{"error_code":0,"result":{{"status":"success","markdown_url":"{base}/md"}}}}"#
            ),
            "# book".to_string(),
        ]
    });

    let mut statuses = Vec::new();
    let result = engine(&base_url, Duration::from_millis(10))
        .ocr_pdf(&pdf, &work, &mut |p| {
            if let OcrProgress::Status(s) = p {
                statuses.push(s);
            }
        })
        .expect("full mock flow must succeed");

    assert_eq!(result, "# book");
    assert_eq!(counter.load(Ordering::SeqCst), 5, "oauth+submit+query*2+markdown GET");
    assert!(
        statuses.iter().any(|s| s.contains("running")),
        "expected a running-status progress event, got {statuses:?}"
    );
    assert!(
        statuses.iter().any(|s| s.contains("success")),
        "expected a success-status progress event, got {statuses:?}"
    );
}

#[test]
fn ocr_pdf_rejects_a_pdf_over_100mb_without_making_any_request() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let pdf = tmp.path().join("huge.pdf");

    // Sparse file: seek past the limit and write one byte, so the test
    // doesn't actually allocate/write 100+ MB of real data.
    {
        use std::io::Seek;
        let mut f = std::fs::File::create(&pdf).unwrap();
        f.seek(std::io::SeekFrom::Start(MAX_PDF_BYTES + 1)).unwrap();
        f.write_all(&[0u8]).unwrap();
    }

    let (base_url, counter) = start_mock_server(|_| vec![]);
    let result = engine(&base_url, Duration::from_millis(10)).ocr_pdf(&pdf, &work, &mut |_| {});

    assert!(result.is_err(), "a >100MB PDF must be rejected");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "an oversized PDF must not trigger any network request"
    );
}

#[test]
fn ocr_pdf_surfaces_baidu_error_code_and_message_on_submit_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let pdf = write_fake_pdf(tmp.path());

    let (base_url, _counter) = start_mock_server(|_| {
        vec![
            r#"{"access_token":"T","expires_in":2592000}"#.to_string(),
            r#"{"error_code":17,"error_msg":"daily quota exceeded"}"#.to_string(),
        ]
    });

    let result = engine(&base_url, Duration::from_millis(10)).ocr_pdf(&pdf, &work, &mut |_| {});

    let err = result.expect_err("non-zero error_code on submit must be Err");
    assert!(
        err.contains("daily quota exceeded"),
        "expected error_msg in the error, got: {err}"
    );
}
