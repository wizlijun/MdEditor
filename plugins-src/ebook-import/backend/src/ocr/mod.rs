//! OCR abstraction shared by the ebook-import pipeline: a page renderer
//! turns a PDF into per-page PNGs, an [`OcrEngine`] turns those PNGs into
//! markdown. This module only defines the shapes (traits + progress enum);
//! [`wechat`] provides the first concrete `OcrEngine` (ported from
//! `01_ocr_to_md.py`) and [`pdfium`] the production `PageRenderer`.
//!
//! Splitting engine from renderer keeps the network/resume logic (easy to
//! unit-test with a mock HTTP server and a fake renderer) separate from the
//! pdfium FFI binding (which needs a real dylib on disk and is only
//! exercised on-device).

pub mod pdfium;
pub mod wechat;

use std::path::{Path, PathBuf};

/// Progress events an [`OcrEngine`] reports back to its caller while
/// `ocr_pdf` runs. `Page` fires once per page (whether the page was
/// actually sent to the OCR service or skipped because a cached
/// `pageNNNN.md` already existed -- callers only care about overall
/// completion, not which). `Status` carries free-form human-readable
/// notices, e.g. a final summary of pages that failed.
pub enum OcrProgress {
    Page { done: usize, total: usize },
    Status(String),
}

/// Turns a PDF into markdown via OCR. `work` is a scratch directory the
/// engine may use for resumable intermediate state (rendered page images,
/// per-page markdown files); a caller that reruns `ocr_pdf` with the same
/// `work` dir after a crash/interrupt should skip pages already completed.
pub trait OcrEngine {
    fn ocr_pdf(
        &self,
        pdf: &Path,
        work: &Path,
        on: &mut dyn FnMut(OcrProgress),
    ) -> Result<String, String>;
}

/// Renders every page of a PDF to a PNG file under `out_dir`, returning the
/// produced file paths in page order. The production implementation is
/// [`pdfium::PdfiumRenderer`]; tests use a fake that writes tiny placeholder
/// PNGs instead of touching the real pdfium dylib.
pub trait PageRenderer {
    fn render_pages(&self, pdf: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String>;
}
