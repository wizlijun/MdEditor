//! Production [`PageRenderer`]: rasterizes every page of a PDF to a PNG via
//! the `pdfium-render` bindings to Google's pdfium C++ library. Pdfium
//! itself ships as a separate dylib (not something `cargo` can vendor), so
//! this module's only real job -- besides the render loop -- is locating
//! that dylib on disk. No unit tests live here: doing so would require a
//! real `libpdfium.dylib` on the test runner, so this is verified on-device
//! (see the task brief) rather than under `cargo test`.

use crate::ocr::PageRenderer;
use pdfium_render::prelude::*;
use std::path::{Path, PathBuf};

pub struct PdfiumRenderer {
    pdfium: Pdfium,
}

impl PdfiumRenderer {
    /// Locates and binds the pdfium dylib, preferring an explicit override
    /// (`NOTEMD_PDFIUM_PATH`, a full path to the dylib -- useful for
    /// development and for sandboxed test/CI environments) and otherwise
    /// expecting `libpdfium.dylib` to sit next to this binary, which is how
    /// the app bundle ships it alongside the plugin executable.
    pub fn new() -> Result<Self, String> {
        let path = dylib_path()?;
        let bindings = Pdfium::bind_to_library(&path)
            .map_err(|e| format!("bind pdfium library at {}: {e}", path.display()))?;
        Ok(Self {
            pdfium: Pdfium::new(bindings),
        })
    }
}

/// Resolves the pdfium dylib path: `NOTEMD_PDFIUM_PATH` env var if set,
/// otherwise `libpdfium.dylib` next to the running executable.
fn dylib_path() -> Result<PathBuf, String> {
    if let Ok(overridden) = std::env::var("NOTEMD_PDFIUM_PATH") {
        return Ok(PathBuf::from(overridden));
    }

    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| format!("executable path {} has no parent directory", exe.display()))?;
    Ok(dir.join("libpdfium.dylib"))
}

impl PageRenderer for PdfiumRenderer {
    fn render_pages(&self, pdf: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String> {
        std::fs::create_dir_all(out_dir)
            .map_err(|e| format!("create {}: {e}", out_dir.display()))?;

        let document = self
            .pdfium
            .load_pdf_from_file(pdf, None)
            .map_err(|e| format!("load {}: {e}", pdf.display()))?;

        // 2x zoom matches the python pipeline's rendering resolution --
        // enough detail for OCR without producing unnecessarily huge PNGs.
        let config = PdfRenderConfig::new().scale_page_by_factor(2.0);

        let mut out_paths = Vec::new();
        for (index, page) in document.pages().iter().enumerate() {
            let page_no = index + 1;
            let out_path = out_dir.join(format!("page_{page_no:04}.png"));

            let bitmap = page
                .render_with_config(&config)
                .map_err(|e| format!("render page {page_no}: {e}"))?;
            bitmap
                .as_image()
                .save_with_format(&out_path, image::ImageFormat::Png)
                .map_err(|e| format!("save {}: {e}", out_path.display()))?;

            out_paths.push(out_path);
        }

        Ok(out_paths)
    }
}
