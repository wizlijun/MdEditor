//! Production [`PageRenderer`]: rasterizes every page of a PDF to a PNG via
//! macOS's built-in Quartz/CoreGraphics PDF support -- a system framework,
//! not a bundled dependency. This replaces the earlier `pdfium-render`
//! binding, which required shipping an 8MB `libpdfium.dylib` alongside the
//! plugin binary; CoreGraphics ships with every macOS install, so there is
//! nothing to fetch, vendor, or codesign.
//!
//! The `core-graphics` crate has no PDF module, so the handful of
//! CoreGraphics C functions this needs are declared directly below in an
//! `unsafe extern "C"` block linked against the `CoreGraphics` framework
//! (see the `#[link(...)]` attribute). This was validated against a real
//! multi-page PDF before being wired in here (see the task's probe).
//!
//! Two pitfalls worth calling out for the next reader:
//! - A PDF page has no inherent backdrop; the bitmap context must be filled
//!   white before drawing, or transparent regions of the page rasterize as
//!   black -- which would corrupt OCR on any page with unpainted margins.
//! - CoreGraphics pads each bitmap row to its own alignment, so
//!   `CGContext::bytes_per_row()` is `>= width * 4` (4 bytes/pixel, RGBA8).
//!   Copying `ctx.data()` straight into `image::RgbaImage::from_raw` treats
//!   that padding as pixel data and SHEARS the image diagonally. The fix is
//!   to copy row by row using the real stride, not `width * 4`, as the
//!   source row length (see [`copy_rows_removing_padding`]).

use crate::ocr::PageRenderer;
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use core_foundation::url::{kCFURLPOSIXPathStyle, CFURL};
use core_graphics::base::kCGImageAlphaNoneSkipLast;
use core_graphics::color_space::CGColorSpace;
use core_graphics::context::CGContext;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use foreign_types_shared::ForeignType;
use std::os::raw::c_void;
use std::path::{Path, PathBuf};

type CGPDFDocumentRef = *mut c_void;
type CGPDFPageRef = *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPDFDocumentCreateWithURL(url: *const c_void) -> CGPDFDocumentRef;
    fn CGPDFDocumentGetNumberOfPages(doc: CGPDFDocumentRef) -> usize;
    fn CGPDFDocumentGetPage(doc: CGPDFDocumentRef, page: usize) -> CGPDFPageRef;
    fn CGPDFDocumentRelease(doc: CGPDFDocumentRef);
    fn CGPDFPageGetBoxRect(page: CGPDFPageRef, box_kind: i32) -> CGRect;
    fn CGContextDrawPDFPage(ctx: *mut c_void, page: CGPDFPageRef);
}

/// `kCGPDFMediaBox` -- the box CoreGraphics uses to report a PDF page's full
/// physical size (as opposed to the crop/bleed/trim/art boxes).
const K_CGPDF_MEDIA_BOX: i32 = 1;

/// Zoom factor applied to every rendered page. Matches the resolution the
/// prior pdfium-based renderer used: enough detail for OCR without
/// producing unnecessarily huge PNGs.
const RENDER_SCALE: f64 = 2.0;

/// A thin RAII wrapper around a `CGPDFDocumentRef` so early-return error
/// paths (zero pages, a page that fails to open) can't leak the document --
/// there is no `Drop` on the raw pointer type itself.
struct PdfDocument(CGPDFDocumentRef);

impl Drop for PdfDocument {
    fn drop(&mut self) {
        unsafe { CGPDFDocumentRelease(self.0) };
    }
}

pub struct QuartzRenderer;

impl QuartzRenderer {
    /// No setup needed -- CoreGraphics is always present on macOS. The
    /// `Result`-returning shape is kept (rather than a bare constructor) so
    /// the call site in `plugin.rs` doesn't need to change when swapping
    /// renderers.
    pub fn new() -> Result<Self, String> {
        Ok(Self)
    }
}

impl PageRenderer for QuartzRenderer {
    fn render_pages(&self, pdf: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String> {
        std::fs::create_dir_all(out_dir)
            .map_err(|e| format!("create {}: {e}", out_dir.display()))?;

        let pdf_str = pdf
            .to_str()
            .ok_or_else(|| format!("non-utf8 path: {}", pdf.display()))?;
        let url = CFURL::from_file_system_path(
            CFString::new(pdf_str),
            kCFURLPOSIXPathStyle,
            false,
        );
        let doc_ref =
            unsafe { CGPDFDocumentCreateWithURL(url.as_CFTypeRef() as *const c_void) };
        if doc_ref.is_null() {
            return Err(format!("could not open PDF: {}", pdf.display()));
        }
        let doc = PdfDocument(doc_ref);

        let page_count = unsafe { CGPDFDocumentGetNumberOfPages(doc.0) };
        if page_count == 0 {
            return Err(format!("PDF has zero pages: {}", pdf.display()));
        }

        let mut out_paths = Vec::with_capacity(page_count);
        for page_no in 1..=page_count {
            let out_path = out_dir.join(format!("page_{page_no:04}.png"));
            render_one_page(doc.0, page_no, &out_path)
                .map_err(|e| format!("render page {page_no}: {e}"))?;
            out_paths.push(out_path);
        }

        Ok(out_paths)
    }
}

/// Renders a single 1-indexed page of an already-open document to `out_path`
/// at [`RENDER_SCALE`].
fn render_one_page(doc: CGPDFDocumentRef, page_no: usize, out_path: &Path) -> Result<(), String> {
    let page = unsafe { CGPDFDocumentGetPage(doc, page_no) };
    if page.is_null() {
        return Err(format!("page {page_no} not found"));
    }

    let media_box = unsafe { CGPDFPageGetBoxRect(page, K_CGPDF_MEDIA_BOX) };
    let width = (media_box.size.width * RENDER_SCALE).round() as usize;
    let height = (media_box.size.height * RENDER_SCALE).round() as usize;
    if width == 0 || height == 0 {
        return Err(format!(
            "degenerate page size {}x{} (before scale: {}x{})",
            width, height, media_box.size.width, media_box.size.height
        ));
    }

    let mut ctx = CGContext::create_bitmap_context(
        None,
        width,
        height,
        8,
        0,
        &CGColorSpace::create_device_rgb(),
        kCGImageAlphaNoneSkipLast,
    );

    // A PDF page has no inherent backdrop -- fill white first so unpainted
    // regions don't rasterize as black (see module doc).
    ctx.set_rgb_fill_color(1.0, 1.0, 1.0, 1.0);
    ctx.fill_rect(CGRect::new(
        &CGPoint::new(0.0, 0.0),
        &CGSize::new(width as f64, height as f64),
    ));
    ctx.scale(RENDER_SCALE, RENDER_SCALE);
    ctx.translate(-media_box.origin.x, -media_box.origin.y);
    unsafe { CGContextDrawPDFPage(ctx.as_ptr() as *mut c_void, page) };

    let packed = copy_rows_removing_padding(&mut ctx, width, height);
    let img = image::RgbaImage::from_raw(width as u32, height as u32, packed)
        .ok_or_else(|| "packed buffer size mismatch".to_string())?;
    img.save(out_path)
        .map_err(|e| format!("save {}: {e}", out_path.display()))?;

    Ok(())
}

/// CoreGraphics pads each bitmap row up to its own alignment, so
/// `ctx.bytes_per_row()` is `>= width * 4`. This copies just the real
/// `width * 4` pixel bytes out of each row, dropping the padding -- without
/// it, `image::RgbaImage::from_raw` reads the padding as pixel data and the
/// resulting image comes out sheared (each row progressively offset).
fn copy_rows_removing_padding(ctx: &mut CGContext, width: usize, height: usize) -> Vec<u8> {
    let stride = ctx.bytes_per_row();
    let row_bytes = width * 4;
    let src = ctx.data();
    let mut packed = Vec::with_capacity(row_bytes * height);
    for row in 0..height {
        let start = row * stride;
        packed.extend_from_slice(&src[start..start + row_bytes]);
    }
    packed
}

#[cfg(test)]
mod tests;
