//! WeChat OCR [`OcrEngine`]. Ports `01_ocr_to_md.py`'s page-at-a-time flow:
//! render every page to a PNG, POST each PNG to a self-hosted WeChat-OCR
//! HTTP endpoint as multipart `file` data, expect back
//! `{"success": bool, "content": "<markdown>"}`, write the per-page
//! markdown to `pageNNNN.md`, then merge every `pageNNNN.md` present (in
//! filename order) into one document.
//!
//! Three things make long OCR runs (hundreds of pages, a flaky OCR box, an
//! unreachable OCR box) not a disaster: pages whose `pageNNNN.md` already
//! exists are skipped rather than re-sent (so an interrupted run resumes for
//! free); a page that fails is merely recorded and left out of the merge
//! rather than aborting the whole book -- only a *total* wipeout (zero pages
//! ever produced content) is an error; and 3 *consecutive* transport-level
//! failures (as opposed to a server-answered rejection) abort the whole book
//! immediately rather than burning the per-page timeout hundreds of times
//! over against a service that's simply unreachable.

#[cfg(test)]
mod tests;

use crate::ocr::{OcrEngine, OcrProgress, PageRenderer};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// TCP connect timeout for the OCR HTTP client, distinct from (and much
/// shorter than) `WeChatOcr::timeout` (the whole-request timeout, which also
/// has to cover slow OCR processing on a reachable server). Without this,
/// `reqwest`'s default has no connect-specific bound, so an unreachable host
/// (dropped packets, not just "connection refused") would burn the full
/// per-request timeout just failing to connect.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Consecutive transport-level failures (the HTTP request itself erroring —
/// connect refused/timed out, DNS failure, etc., as opposed to a
/// server-answered `success:false`) before `ocr_pdf` gives up on the whole
/// book rather than retrying page after page against an unreachable
/// service. See the circuit-breaker comment at its call site.
const MAX_CONSECUTIVE_TRANSPORT_FAILURES: usize = 3;

pub struct WeChatOcr {
    pub url: String,
    pub renderer: Box<dyn PageRenderer>,
    pub timeout: Duration,
    /// Checked at the top of `ocr_pdf` and again before every page. Wired to
    /// a job's cancel flag by `plugin.rs::build_engine` (and to `deactivate`
    /// cancelling every live job) -- without this, a cancelled/shutting-down
    /// plugin process's job thread would keep POSTing pages to the OCR
    /// endpoint indefinitely, holding its `Host` clone alive and blocking
    /// `serve_io` from ever completing after `$deactivate`.
    pub cancelled: Arc<AtomicBool>,
}

impl OcrEngine for WeChatOcr {
    fn ocr_pdf(
        &self,
        pdf: &Path,
        work: &Path,
        on: &mut dyn FnMut(OcrProgress),
    ) -> Result<String, String> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }

        let images_dir = work.join("ocr_images");
        std::fs::create_dir_all(&images_dir)
            .map_err(|e| format!("create {}: {e}", images_dir.display()))?;
        let pages = self.renderer.render_pages(pdf, &images_dir)?;
        let total = pages.len();

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|e| format!("build http client: {e}"))?;

        let mut succeeded = 0usize;
        let mut failed_pages: Vec<usize> = Vec::new();
        // Circuit breaker: the default URL is an intranet address, so
        // off-network every page would otherwise hang for the full
        // (120s) timeout before failing -- a 300-page book would then take
        // ~10 hours just to discover the service is unreachable. A
        // *transport*-level failure (the request itself errored: connect
        // refused/timed out, DNS failure, etc.) three times in a row means
        // the service isn't reachable at all, so abort the whole book
        // rather than repeat that wait per page. A server-answered
        // `success:false` is a normal per-page OCR failure, not a transport
        // problem, and resets this counter same as a success.
        let mut consecutive_transport_failures = 0usize;

        for (idx, image_path) in pages.iter().enumerate() {
            if self.cancelled.load(Ordering::Relaxed) {
                return Err("cancelled".to_string());
            }

            let page_no = idx + 1;
            let page_md = work.join(format!("page{page_no:04}.md"));

            if page_md.exists() {
                // Resume support: a prior run already produced this page.
                succeeded += 1;
                consecutive_transport_failures = 0;
            } else {
                match ocr_one_page(&client, &self.url, image_path) {
                    Ok(content) => {
                        std::fs::write(&page_md, content)
                            .map_err(|e| format!("write {}: {e}", page_md.display()))?;
                        succeeded += 1;
                        consecutive_transport_failures = 0;
                    }
                    Err(OcrPageError::Transport(e)) => {
                        failed_pages.push(page_no);
                        consecutive_transport_failures += 1;
                        if consecutive_transport_failures >= MAX_CONSECUTIVE_TRANSPORT_FAILURES {
                            return Err(format!("ocr service unreachable: {e}"));
                        }
                    }
                    Err(OcrPageError::Rejected(reason)) => {
                        on(OcrProgress::Status(format!("page {page_no} failed: {reason}")));
                        failed_pages.push(page_no);
                        consecutive_transport_failures = 0;
                    }
                }
            }

            on(OcrProgress::Page {
                done: page_no,
                total,
            });
        }

        if succeeded == 0 {
            return Err(format!("OCR produced no content for any of {total} page(s)"));
        }

        if !failed_pages.is_empty() {
            on(OcrProgress::Status(format!(
                "failed pages: {failed_pages:?}"
            )));
        }

        merge_pages(work)
    }
}

/// A single page's OCR failure, distinguishing *why* it failed so the
/// circuit breaker in `ocr_pdf` can tell "the OCR service is unreachable"
/// (transport) from "this one page's document/image was rejected" (a
/// server-answered `success:false`, or a body it couldn't parse) apart --
/// only the former should count toward aborting the whole book.
enum OcrPageError {
    /// The HTTP request itself failed: connect refused/timed out, DNS
    /// failure, TLS error, etc. -- the server never got a chance to answer.
    Transport(String),
    /// The server answered, but rejected the page (`success:false`, an
    /// unparsable/incomplete JSON body). A per-page problem, not evidence
    /// the service is down.
    Rejected(String),
}

/// POSTs one rendered page image to the WeChat-OCR endpoint and returns its
/// markdown `content` on `{"success": true, ...}`. Distinguishes a
/// transport-level failure (see [`OcrPageError`]) from a server-answered
/// rejection or unparsable body -- callers use that split to drive the
/// unreachable-service circuit breaker; either way the page itself is
/// simply not produced.
fn ocr_one_page(
    client: &reqwest::blocking::Client,
    url: &str,
    image_path: &Path,
) -> Result<String, OcrPageError> {
    let bytes = std::fs::read(image_path)
        .map_err(|e| OcrPageError::Rejected(format!("read {}: {e}", image_path.display())))?;
    let file_name = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("page.png")
        .to_string();
    let part = reqwest::blocking::multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str("image/png")
        .map_err(|e| OcrPageError::Rejected(e.to_string()))?;
    let form = reqwest::blocking::multipart::Form::new().part("file", part);

    let resp = client
        .post(url)
        .multipart(form)
        .send()
        .map_err(|e| OcrPageError::Transport(format!("POST {url}: {e}")))?;
    let json: serde_json::Value = resp
        .json()
        .map_err(|e| OcrPageError::Rejected(format!("parse OCR response json: {e}")))?;

    let success = json.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    if !success {
        return Err(OcrPageError::Rejected(
            "OCR endpoint reported success=false".to_string(),
        ));
    }
    json.get("content")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| OcrPageError::Rejected("OCR response missing `content`".to_string()))
}

/// Merges every `pageNNNN.md` present in `work` (freshly OCR'd pages plus
/// any pre-existing/cached ones from a prior interrupted run), sorted by
/// file name so page order is preserved regardless of write order, joined
/// with a blank line -- porting `01_ocr_to_md.py`'s merge step. A page that
/// never got a `pageNNNN.md` (failed and not cached) is silently absent.
fn merge_pages(work: &Path) -> Result<String, String> {
    let mut paths: Vec<_> = std::fs::read_dir(work)
        .map_err(|e| format!("read {}: {e}", work.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("page") && n.ends_with(".md"))
        })
        .collect();
    paths.sort();

    let mut parts = Vec::with_capacity(paths.len());
    for path in paths {
        parts.push(
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?,
        );
    }
    Ok(parts.join("\n\n"))
}
