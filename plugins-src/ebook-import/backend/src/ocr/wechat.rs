//! WeChat OCR [`OcrEngine`]. Ports `01_ocr_to_md.py`'s page-at-a-time flow:
//! render every page to a PNG, POST each PNG to a self-hosted WeChat-OCR
//! HTTP endpoint as multipart `file` data, expect back
//! `{"success": bool, "content": "<markdown>"}`, write the per-page
//! markdown to `pageNNNN.md`, then merge every `pageNNNN.md` present (in
//! filename order) into one document.
//!
//! Two things make long OCR runs (hundreds of pages, a flaky OCR box) not a
//! disaster: pages whose `pageNNNN.md` already exists are skipped rather
//! than re-sent (so an interrupted run resumes for free), and a page that
//! fails is merely recorded and left out of the merge rather than aborting
//! the whole book -- only a *total* wipeout (zero pages ever produced
//! content) is an error.

#[cfg(test)]
mod tests;

use crate::ocr::{OcrEngine, OcrProgress, PageRenderer};
use std::path::Path;
use std::time::Duration;

pub struct WeChatOcr {
    pub url: String,
    pub renderer: Box<dyn PageRenderer>,
    pub timeout: Duration,
}

impl OcrEngine for WeChatOcr {
    fn ocr_pdf(
        &self,
        pdf: &Path,
        work: &Path,
        on: &mut dyn FnMut(OcrProgress),
    ) -> Result<String, String> {
        let images_dir = work.join("ocr_images");
        std::fs::create_dir_all(&images_dir)
            .map_err(|e| format!("create {}: {e}", images_dir.display()))?;
        let pages = self.renderer.render_pages(pdf, &images_dir)?;
        let total = pages.len();

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()
            .map_err(|e| format!("build http client: {e}"))?;

        let mut succeeded = 0usize;
        let mut failed_pages: Vec<usize> = Vec::new();

        for (idx, image_path) in pages.iter().enumerate() {
            let page_no = idx + 1;
            let page_md = work.join(format!("page{page_no:04}.md"));

            if page_md.exists() {
                // Resume support: a prior run already produced this page.
                succeeded += 1;
            } else {
                match ocr_one_page(&client, &self.url, image_path) {
                    Ok(content) => {
                        std::fs::write(&page_md, content)
                            .map_err(|e| format!("write {}: {e}", page_md.display()))?;
                        succeeded += 1;
                    }
                    Err(_) => failed_pages.push(page_no),
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

/// POSTs one rendered page image to the WeChat-OCR endpoint and returns its
/// markdown `content` on `{"success": true, ...}`. Any transport error,
/// unparsable body, or `success != true` collapses to a single `Err` --
/// callers only need to know whether the page produced content, not why it
/// didn't.
fn ocr_one_page(
    client: &reqwest::blocking::Client,
    url: &str,
    image_path: &Path,
) -> Result<String, String> {
    let bytes = std::fs::read(image_path).map_err(|e| format!("read {}: {e}", image_path.display()))?;
    let file_name = image_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("page.png")
        .to_string();
    let part = reqwest::blocking::multipart::Part::bytes(bytes)
        .file_name(file_name)
        .mime_str("image/png")
        .map_err(|e| e.to_string())?;
    let form = reqwest::blocking::multipart::Form::new().part("file", part);

    let resp = client
        .post(url)
        .multipart(form)
        .send()
        .map_err(|e| format!("POST {url}: {e}"))?;
    let json: serde_json::Value = resp
        .json()
        .map_err(|e| format!("parse OCR response json: {e}"))?;

    let success = json.get("success").and_then(|v| v.as_bool()).unwrap_or(false);
    if !success {
        return Err("OCR endpoint reported success=false".to_string());
    }
    json.get("content")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "OCR response missing `content`".to_string())
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
