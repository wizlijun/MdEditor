use super::*;
use crate::ocr::OcrProgress;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

/// Fake [`PageRenderer`] used in place of [`crate::ocr::pdfium::PdfiumRenderer`]:
/// writes `count` tiny 1x1 PNGs (via the `image` crate, so they're real,
/// decodable PNG files) into `out_dir` and returns them in page order,
/// without touching a real PDF or the pdfium dylib.
struct FakeRenderer {
    count: usize,
}

impl PageRenderer for FakeRenderer {
    fn render_pages(&self, _pdf: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String> {
        std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
        let mut paths = Vec::with_capacity(self.count);
        for i in 1..=self.count {
            let path = out_dir.join(format!("page_{i:04}.png"));
            image::DynamicImage::ImageRgb8(image::RgbImage::new(1, 1))
                .save_with_format(&path, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            paths.push(path);
        }
        Ok(paths)
    }
}

/// Starts a mock OCR server on an ephemeral `127.0.0.1` port that hands out
/// `responses` -- one canned JSON body per accepted connection, in order --
/// then closes each connection. Returns the server's base URL and a shared
/// counter of accepted connections so tests can assert exactly how many
/// HTTP requests were made (the resume test relies on this to prove a
/// cached page never hits the network).
fn start_mock_server(responses: Vec<String>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock OCR server");
    let addr = listener.local_addr().unwrap();
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
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });

    (format!("http://{addr}"), counter)
}

/// Reads (and discards) one HTTP request off `stream`: headers first up to
/// the blank line, then exactly `Content-Length` more bytes if that header
/// is present. This is a mock, not a real HTTP server -- the multipart
/// bodies these tests send are tiny (a 1x1 PNG), so a byte-at-a-time header
/// scan followed by one `read_exact` is plenty robust.
fn drain_request(stream: &mut TcpStream) {
    let mut header_bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => return, // peer half-closed before finishing headers
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

fn engine(url: String, page_count: usize) -> WeChatOcr {
    WeChatOcr {
        url,
        renderer: Box::new(FakeRenderer { count: page_count }),
        timeout: Duration::from_secs(5),
        cancelled: Arc::new(AtomicBool::new(false)),
    }
}

/// Builds the canned JSON body a mock page response would send:
/// `{"success":true,"content":"<content>"}`.
fn ok_body(content: &str) -> String {
    format!("{{\"success\":true,\"content\":\"{content}\"}}")
}

/// The canned JSON body for a page the OCR endpoint reports as failed.
fn fail_body() -> String {
    "{\"success\":false}".to_string()
}

#[test]
fn all_pages_succeed_merge_in_order_and_report_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();

    let (url, counter) = start_mock_server(vec![ok_body("# p1"), ok_body("# p2"), ok_body("# p3")]);

    let mut pages_seen = Vec::new();
    let result = engine(url, 3)
        .ocr_pdf(Path::new("fake.pdf"), &work, &mut |p| {
            if let OcrProgress::Page { done, total } = p {
                pages_seen.push((done, total));
            }
        })
        .expect("all pages succeeding must be Ok");

    assert_eq!(result, "# p1\n\n# p2\n\n# p3");
    assert_eq!(counter.load(Ordering::SeqCst), 3, "one request per page");
    assert_eq!(pages_seen, vec![(1, 3), (2, 3), (3, 3)]);
}

#[test]
fn preexisting_page_md_is_skipped_and_never_requested() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    // Simulates resuming after an earlier run that already produced page 2.
    std::fs::write(work.join("page0002.md"), "cached").unwrap();

    let (url, counter) = start_mock_server(vec![ok_body("# p1"), ok_body("# p3")]);

    let result = engine(url, 3)
        .ocr_pdf(Path::new("fake.pdf"), &work, &mut |_| {})
        .expect("resume run must be Ok");

    assert_eq!(
        counter.load(Ordering::SeqCst),
        2,
        "the cached page must not trigger an HTTP request"
    );
    assert_eq!(result, "# p1\n\ncached\n\n# p3");
}

#[test]
fn a_failed_page_is_recorded_and_left_out_of_the_merge() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();

    let (url, _counter) = start_mock_server(vec![ok_body("# p1"), fail_body(), ok_body("# p3")]);

    let mut statuses = Vec::new();
    let result = engine(url, 3)
        .ocr_pdf(Path::new("fake.pdf"), &work, &mut |p| {
            if let OcrProgress::Status(s) = p {
                statuses.push(s);
            }
        })
        .expect("a partial failure must still be Ok");

    assert_eq!(result, "# p1\n\n# p3");
    assert!(
        !work.join("page0002.md").exists(),
        "a failed page must not leave a pageNNNN.md behind"
    );
    assert!(
        statuses.iter().any(|s| s.contains("[2]")),
        "expected a failed-pages status naming page 2, got {statuses:?}"
    );
}

#[test]
fn every_page_failing_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();

    let (url, _counter) = start_mock_server(vec![fail_body(), fail_body()]);

    let result = engine(url, 2).ocr_pdf(Path::new("fake.pdf"), &work, &mut |_| {});

    assert!(result.is_err(), "zero successful pages must be an Err");
}

#[test]
fn a_precancelled_run_returns_err_without_making_any_request() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();

    // No responses queued -- if `ocr_pdf` made even one request, `accept()`
    // would hang until this thread's mock server loop exits, not panic; the
    // `counter == 0` assertion below is what actually pins "no request".
    let (url, counter) = start_mock_server(vec![]);

    let mut e = engine(url, 3);
    e.cancelled = Arc::new(AtomicBool::new(true));

    let result = e.ocr_pdf(Path::new("fake.pdf"), &work, &mut |_| {});

    assert_eq!(result, Err("cancelled".to_string()));
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "a pre-cancelled run must not make any OCR request"
    );
}

#[test]
fn a_cancel_flag_set_mid_run_stops_before_the_next_page() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();

    let (url, counter) = start_mock_server(vec![ok_body("# p1"), ok_body("# p2"), ok_body("# p3")]);

    let mut e = engine(url, 3);
    let cancel_flag = Arc::new(AtomicBool::new(false));
    e.cancelled = cancel_flag.clone();

    // Cancel right after page 1 reports progress -- the loop must notice
    // at the top of the *next* iteration, before page 2's request.
    let result = e.ocr_pdf(Path::new("fake.pdf"), &work, &mut move |p| {
        if let OcrProgress::Page { done: 1, .. } = p {
            cancel_flag.store(true, Ordering::Relaxed);
        }
    });

    assert_eq!(result, Err("cancelled".to_string()));
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "must stop after page 1's request, before page 2's"
    );
}
