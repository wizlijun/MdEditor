//! Baidu 文档解析 (Unlimited-OCR) [`OcrEngine`]: submits the *whole PDF* as
//! one async task (unlike [`crate::ocr::wechat::WeChatOcr`], which OCRs one
//! rendered page image at a time), polls until the task finishes, then
//! downloads the resulting markdown from a Baidu-hosted `markdown_url`.
//!
//! Protocol (per Baidu's public docs for `unlimited-ocr-parser`):
//! 1. OAuth: `client_id`/`client_secret` exchange for a bearer `access_token`
//!    (cached in-memory until it's close to expiry -- `expires_in` is ~30
//!    days, so re-fetching per call would be wasteful and rate-limit-risky).
//! 2. Submit: POST the whole PDF (base64'd, urlencoded, `application/x-www-form-urlencoded`)
//!    to `submit_url`, get back a `task_id`.
//! 3. Poll: POST `task_id` to `query_url` every `poll_interval` until
//!    `status` is `success` (yields `markdown_url`) or `failed`.
//! 4. Fetch: GET `markdown_url` for the markdown text.
//!
//! Baidu's `markdown_url` (and any image URLs embedded in the markdown it
//! returns) are presigned links that expire in roughly 30 days. An archived
//! book that still points at those links silently rots once they expire --
//! that violates file-over-app (the `.md` must be self-contained, readable
//! forever without phoning home). [`localize_images`] downloads every
//! remote image referenced in the returned markdown into `work/images/` and
//! rewrites the links to point at the local copies before `ocr_pdf` returns.

#[cfg(test)]
mod tests;

use crate::ocr::{OcrEngine, OcrProgress};
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Baidu rejects (or silently truncates) documents above this size; check it
/// ourselves before spending a network round-trip on a submission Baidu
/// would refuse anyway. Page-count limits are *not* checked here (that would
/// need pdfium to count pages, and the whole point of the Baidu path is to
/// work on machines without the pdfium dylib) -- an over-length PDF is left
/// for Baidu's own `error_code`/`error_msg` to report.
const MAX_PDF_BYTES: u64 = 100 * 1024 * 1024;

/// `OcrEngine` backed by Baidu's 文档解析 (Unlimited-OCR) task API. Construct
/// production instances with [`BaiduOcr::new`]; tests build the struct
/// literal directly (all fields but the token cache are `pub`) pointing
/// `oauth_url`/`submit_url`/`query_url` at a mock server.
pub struct BaiduOcr {
    pub api_key: String,
    pub secret_key: String,
    pub oauth_url: String,
    pub submit_url: String,
    pub query_url: String,
    pub poll_interval: Duration,
    /// `(token, expires_at)`, refreshed lazily by [`BaiduOcr::access_token`].
    /// Interior mutability because [`OcrEngine::ocr_pdf`] takes `&self`.
    token_cache: Mutex<Option<(String, Instant)>>,
}

impl BaiduOcr {
    /// Production instance pointed at Baidu's real endpoints, polling every
    /// 7s (the task is typically minutes long; Baidu doesn't want faster
    /// polling than that).
    pub fn new(api_key: String, secret_key: String) -> Self {
        Self {
            api_key,
            secret_key,
            oauth_url: "https://aip.baidubce.com/oauth/2.0/token".to_string(),
            submit_url: "https://aip.baidubce.com/rest/2.0/brain/online/v2/unlimited-ocr-parser/task"
                .to_string(),
            query_url:
                "https://aip.baidubce.com/rest/2.0/brain/online/v2/unlimited-ocr-parser/task/query"
                    .to_string(),
            poll_interval: Duration::from_secs(7),
            token_cache: Mutex::new(None),
        }
    }

    /// Returns a cached access token if it hasn't expired yet, otherwise
    /// exchanges `api_key`/`secret_key` for a fresh one and caches it.
    fn access_token(&self, client: &reqwest::blocking::Client) -> Result<String, String> {
        if let Some((token, expires_at)) = self.token_cache.lock().unwrap().as_ref() {
            if Instant::now() < *expires_at {
                return Ok(token.clone());
            }
        }

        let url = format!(
            "{}?grant_type=client_credentials&client_id={}&client_secret={}",
            self.oauth_url,
            percent_encode(&self.api_key),
            percent_encode(&self.secret_key)
        );
        let resp = client
            .post(&url)
            .timeout(Duration::from_secs(30))
            .send()
            .map_err(|e| format!("POST {url}: {e}"))?;
        let json: serde_json::Value = resp
            .json()
            .map_err(|e| format!("parse oauth response json: {e}"))?;
        let token = json
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("oauth response missing access_token: {json}"))?
            .to_string();
        // Trim a minute of safety margin off the advertised TTL so we never
        // hand out a token that expires mid-request.
        let expires_in = json
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(2_592_000);
        let ttl = Duration::from_secs(expires_in.saturating_sub(60));

        *self.token_cache.lock().unwrap() = Some((token.clone(), Instant::now() + ttl));
        Ok(token)
    }

    /// Submits the whole PDF as one document-parsing task, returning its
    /// `task_id`.
    fn submit_task(
        &self,
        client: &reqwest::blocking::Client,
        token: &str,
        pdf_bytes: &[u8],
        file_name: &str,
    ) -> Result<String, String> {
        use base64::Engine;
        let file_data = base64::engine::general_purpose::STANDARD.encode(pdf_bytes);
        let body = format!(
            "file_data={}&file_name={}",
            percent_encode(&file_data),
            percent_encode(file_name)
        );
        let url = format!("{}?access_token={}", self.submit_url, percent_encode(token));
        let resp = client
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .timeout(Duration::from_secs(30))
            .body(body)
            .send()
            .map_err(|e| format!("POST {url}: {e}"))?;
        let json: serde_json::Value = resp
            .json()
            .map_err(|e| format!("parse submit response json: {e}"))?;
        check_error_code(&json)?;
        json.get("result")
            .and_then(|r| r.get("task_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("submit response missing result.task_id: {json}"))
    }

    /// Polls the task status once. Returns the raw `status` string and, once
    /// `status == "success"`, the `markdown_url` to fetch.
    fn query_task(
        &self,
        client: &reqwest::blocking::Client,
        token: &str,
        task_id: &str,
    ) -> Result<QueryResult, String> {
        let url = format!("{}?access_token={}", self.query_url, percent_encode(token));
        let body = format!("task_id={}", percent_encode(task_id));
        let resp = client
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .timeout(Duration::from_secs(30))
            .body(body)
            .send()
            .map_err(|e| format!("POST {url}: {e}"))?;
        let json: serde_json::Value = resp
            .json()
            .map_err(|e| format!("parse query response json: {e}"))?;
        check_error_code(&json)?;
        let result = json
            .get("result")
            .ok_or_else(|| format!("query response missing result: {json}"))?;
        let status = result
            .get("status")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("query result missing status: {json}"))?
            .to_string();
        let markdown_url = result
            .get("markdown_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        Ok(QueryResult {
            status,
            markdown_url,
        })
    }
}

struct QueryResult {
    status: String,
    markdown_url: Option<String>,
}

/// `error_code != 0` on any Baidu response (oauth aside, which uses a
/// different error shape) means the request failed; surface `error_msg` when
/// present so the caller sees Baidu's own explanation.
fn check_error_code(json: &serde_json::Value) -> Result<(), String> {
    let code = json.get("error_code").and_then(|v| v.as_i64()).unwrap_or(0);
    if code != 0 {
        let msg = json
            .get("error_msg")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        return Err(format!("Baidu OCR API error {code}: {msg}"));
    }
    Ok(())
}

/// Rejects a PDF above [`MAX_PDF_BYTES`] before it's read into memory or
/// submitted -- factored out from [`BaiduOcr::ocr_pdf`] so the boundary is
/// unit-testable without writing a 100 MB fixture file.
fn precheck_size(len: u64) -> Result<(), String> {
    if len > MAX_PDF_BYTES {
        Err(format!(
            "PDF is {len} bytes, exceeding the {MAX_PDF_BYTES}-byte limit Baidu's \
             Unlimited-OCR task API enforces; not submitting"
        ))
    } else {
        Ok(())
    }
}

/// Percent-encodes `input` for use inside an
/// `application/x-www-form-urlencoded` body/query string. Conservative by
/// design (only `A-Z a-z 0-9 - _ . ~` pass through unescaped) so it's safe
/// for arbitrary bytes, including base64 output (`+`, `/`, `=`) and UTF-8
/// file names -- we don't pull in a urlencoding crate for this one helper.
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

impl OcrEngine for BaiduOcr {
    fn ocr_pdf(
        &self,
        pdf: &Path,
        work: &Path,
        on: &mut dyn FnMut(OcrProgress),
    ) -> Result<String, String> {
        let metadata =
            std::fs::metadata(pdf).map_err(|e| format!("stat {}: {e}", pdf.display()))?;
        precheck_size(metadata.len())?;
        let pdf_bytes = std::fs::read(pdf).map_err(|e| format!("read {}: {e}", pdf.display()))?;
        let file_name = pdf
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("book.pdf")
            .to_string();

        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|e| format!("build http client: {e}"))?;

        on(OcrProgress::Status(
            "baidu ocr: requesting access token".to_string(),
        ));
        let token = self.access_token(&client)?;

        on(OcrProgress::Status(
            "baidu ocr: submitting document".to_string(),
        ));
        let task_id = self.submit_task(&client, &token, &pdf_bytes, &file_name)?;

        let markdown_url = loop {
            let q = self.query_task(&client, &token, &task_id)?;
            on(OcrProgress::Status(format!("baidu ocr: {}", q.status)));
            match q.status.as_str() {
                "success" => {
                    break q.markdown_url.ok_or_else(|| {
                        "baidu ocr: task reported success without markdown_url".to_string()
                    })?;
                }
                "failed" => return Err(format!("baidu ocr: task {task_id} failed")),
                _ => std::thread::sleep(self.poll_interval),
            }
        };

        on(OcrProgress::Status(
            "baidu ocr: downloading markdown".to_string(),
        ));
        let markdown = client
            .get(&markdown_url)
            .timeout(Duration::from_secs(120))
            .send()
            .map_err(|e| format!("GET {markdown_url}: {e}"))?
            .text()
            .map_err(|e| format!("read markdown body: {e}"))?;

        let images_dir = work.join("images");
        let fetch = |url: &str| -> Result<Vec<u8>, String> {
            client
                .get(url)
                .timeout(Duration::from_secs(120))
                .send()
                .map_err(|e| format!("GET {url}: {e}"))?
                .bytes()
                .map(|b| b.to_vec())
                .map_err(|e| format!("read image body: {e}"))
        };
        localize_images(&markdown, &fetch, &images_dir)
    }
}

/// Matches a markdown image whose URL is `http://` or `https://`: group 1 is
/// the alt text, group 2 the URL. Local/relative image links (already
/// self-contained) are left alone.
fn remote_image_regex() -> Regex {
    Regex::new(r"!\[([^\]]*)\]\((https?://[^)\s]+)\)").expect("static regex must compile")
}

/// Downloads every remote (`http(s)://`) image referenced in `md` via
/// `fetch`, saves it under `images_dir` as `baidu_NNN.<ext>` (`NNN` = 1-based
/// order of first appearance), and rewrites the link to that relative path.
///
/// This is what keeps an OCR'd book self-contained after Baidu's presigned
/// image links expire (~30 days) -- see the module doc for why that matters.
/// A URL that fails to fetch keeps its original (eventually-dead) link
/// rather than aborting the whole document; the same URL appearing more
/// than once reuses the first download instead of re-fetching.
pub fn localize_images(
    md: &str,
    fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    images_dir: &Path,
) -> Result<String, String> {
    let re = remote_image_regex();
    let mut downloaded: HashMap<String, String> = HashMap::new();
    let mut next_index: usize = 1;
    let mut dir_created = false;

    let mut out = String::with_capacity(md.len());
    let mut last_end = 0;

    for caps in re.captures_iter(md) {
        let whole = caps.get(0).unwrap();
        let alt = caps.get(1).unwrap().as_str();
        let url = caps.get(2).unwrap().as_str();

        out.push_str(&md[last_end..whole.start()]);

        let local_path = if let Some(existing) = downloaded.get(url) {
            Some(existing.clone())
        } else {
            match fetch(url) {
                Ok(bytes) => {
                    if !dir_created {
                        std::fs::create_dir_all(images_dir)
                            .map_err(|e| format!("create {}: {e}", images_dir.display()))?;
                        dir_created = true;
                    }
                    let ext = guess_image_ext(url, &bytes);
                    let file_name = format!("baidu_{next_index:03}.{ext}");
                    let file_path = images_dir.join(&file_name);
                    std::fs::write(&file_path, &bytes)
                        .map_err(|e| format!("write {}: {e}", file_path.display()))?;
                    next_index += 1;
                    let rel = format!("images/{file_name}");
                    downloaded.insert(url.to_string(), rel.clone());
                    Some(rel)
                }
                Err(_) => None,
            }
        };

        match local_path {
            Some(rel) => out.push_str(&format!("![{alt}]({rel})")),
            None => out.push_str(whole.as_str()),
        }

        last_end = whole.end();
    }
    out.push_str(&md[last_end..]);

    Ok(out)
}

/// Picks a file extension for a downloaded image: trust the URL's own
/// extension when it's one of the common web image types, otherwise sniff
/// the file's magic bytes, falling back to `bin` for anything unrecognized.
fn guess_image_ext(url: &str, bytes: &[u8]) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    if let Some(dot) = path.rfind('.') {
        let candidate = path[dot + 1..].to_lowercase();
        if matches!(candidate.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
            return candidate;
        }
    }
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "png".to_string()
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "jpg".to_string()
    } else {
        "bin".to_string()
    }
}
