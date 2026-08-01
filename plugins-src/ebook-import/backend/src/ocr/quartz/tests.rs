use super::QuartzRenderer;
use crate::ocr::PageRenderer;
use std::path::Path;

/// Builds a minimal, valid PDF (raw bytes, hand-assembled -- no external
/// tooling) with one page per `(width_pt, height_pt, rect)` entry. When
/// `rect` is `Some((x, y, w, h))` that page's content stream fills a solid
/// black rectangle at that box (PDF units, origin bottom-left); `None`
/// leaves the page blank (white once rasterized).
///
/// This is deliberately the simplest structure CoreGraphics' PDF parser
/// will accept: a Catalog, a Pages node, N page objects and N content-stream
/// objects, wired together with a plain (uncompressed) xref table. Real
/// object byte offsets are recorded as they're written so the xref table is
/// accurate -- a PDF with a wrong xref table is exactly the kind of "invalid
/// PDF" the error-path test below exercises, so this one has to be right.
fn build_pdf(page_specs: &[(u32, u32, Option<(u32, u32, u32, u32)>)]) -> Vec<u8> {
    let n = page_specs.len();
    let total_objs = 2 + 2 * n; // 1 catalog + 1 pages node + n pages + n content streams
    let mut offsets = vec![0usize; total_objs + 1]; // 1-indexed; offsets[0] unused
    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n");

    offsets[1] = pdf.len();
    pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    let kids: String = (0..n)
        .map(|i| format!("{} 0 R ", 3 + i))
        .collect::<String>();
    offsets[2] = pdf.len();
    pdf.extend_from_slice(
        format!(
            "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {n} >>\nendobj\n",
            kids.trim_end()
        )
        .as_bytes(),
    );

    for (i, (w, h, _rect)) in page_specs.iter().enumerate() {
        let page_obj = 3 + i;
        let content_obj = 3 + n + i;
        offsets[page_obj] = pdf.len();
        pdf.extend_from_slice(
            format!(
                "{page_obj} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] \
                 /Contents {content_obj} 0 R /Resources << >> >>\nendobj\n"
            )
            .as_bytes(),
        );
    }

    for (i, (_w, _h, rect)) in page_specs.iter().enumerate() {
        let content_obj = 3 + n + i;
        let content = match rect {
            Some((rx, ry, rw, rh)) => format!("0 0 0 rg\n{rx} {ry} {rw} {rh} re f\n"),
            None => String::new(),
        };
        offsets[content_obj] = pdf.len();
        pdf.extend_from_slice(
            format!("{content_obj} 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes(),
        );
        pdf.extend_from_slice(content.as_bytes());
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
    }

    let xref_offset = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n", total_objs + 1).as_bytes());
    pdf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF",
            total_objs + 1
        )
        .as_bytes(),
    );

    pdf
}

fn is_black(p: &image::Rgba<u8>) -> bool {
    p.0[0] < 40 && p.0[1] < 40 && p.0[2] < 40
}

fn is_white(p: &image::Rgba<u8>) -> bool {
    p.0[0] > 200 && p.0[1] > 200 && p.0[2] > 200
}

/// The regression test that matters: a page with a black rectangle placed
/// away from every edge must rasterize to a clean, non-sheared image. The
/// bug this guards against (copying CoreGraphics' padded `bytes_per_row`
/// verbatim instead of the real `width * 4` per row) makes each row's data
/// drift by a growing offset -- so the black rectangle's left/right column
/// bounds would shift row-to-row instead of staying constant. That's what's
/// checked here, which sidesteps needing to know CoreGraphics' bitmap
/// vertical-flip convention: whichever way the rectangle lands, a correct
/// render keeps identical column bounds across every row it occupies.
#[test]
fn renders_pdf_without_shearing() {
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("fixture.pdf");
    let (page_w, page_h) = (200u32, 100u32);
    let (rx, ry, rw, rh) = (50u32, 25u32, 100u32, 50u32); // clear of every edge
    std::fs::write(&pdf_path, build_pdf(&[(page_w, page_h, Some((rx, ry, rw, rh)))])).unwrap();

    let out_dir = dir.path().join("out");
    let renderer = QuartzRenderer::new().unwrap();
    let pages = renderer.render_pages(&pdf_path, &out_dir).unwrap();
    assert_eq!(pages.len(), 1);
    assert!(pages[0].exists());
    assert_eq!(pages[0].file_name().unwrap(), "page_0001.png");

    let img = image::open(&pages[0]).unwrap().to_rgba8();
    let (w, h) = img.dimensions();
    // 2x scale, matching the pdfium renderer's prior contract.
    assert_eq!(w, page_w * 2);
    assert_eq!(h, page_h * 2);

    // Per row, find the leftmost/rightmost black pixel (if any).
    let mut black_rows: Vec<(u32, u32, u32)> = Vec::new();
    for y in 0..h {
        let mut first = None;
        let mut last = None;
        for x in 0..w {
            if is_black(img.get_pixel(x, y)) {
                first.get_or_insert(x);
                last = Some(x);
            }
        }
        if let (Some(lo), Some(hi)) = (first, last) {
            black_rows.push((y, lo, hi));
        }
    }

    assert!(
        !black_rows.is_empty(),
        "expected a solid black rectangle somewhere in the render"
    );
    let (_, expected_lo, expected_hi) = black_rows[0];
    for (y, lo, hi) in &black_rows {
        assert_eq!(*lo, expected_lo, "row {y}: left edge drifted -- shearing regression");
        assert_eq!(*hi, expected_hi, "row {y}: right edge drifted -- shearing regression");
    }

    // Rectangle was `rw x rh` PDF points -> `rw*2 x rh*2` device pixels
    // (small antialiasing slop at the hard edge is fine).
    let band_rows = black_rows.len();
    assert!(
        band_rows.abs_diff((rh * 2) as usize) <= 2,
        "black band height {band_rows} != expected ~{}",
        rh * 2
    );
    let band_cols = (expected_hi - expected_lo + 1) as usize;
    assert!(
        band_cols.abs_diff((rw * 2) as usize) <= 2,
        "black band width {band_cols} != expected ~{}",
        rw * 2
    );

    // Rows well clear of the rectangle's band (top and bottom edges of the
    // page) must be entirely white, not black -- i.e. no all-black backdrop.
    let band_start = black_rows.first().unwrap().0;
    let band_end = black_rows.last().unwrap().0;
    assert!(band_start > 4 && band_end < h - 4, "rectangle unexpectedly touches page edge");
    for &y in &[0, h - 1] {
        for x in 0..w {
            let p = img.get_pixel(x, y);
            assert!(is_white(p), "expected white at ({x},{y}) on an edge row, got {p:?}");
        }
    }
}

/// Multi-page smoke test: page count, per-page files, and dimensions (2x
/// the PDF's point size) for a real (if blank) 2-page PDF.
#[test]
fn renders_two_page_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("two-pages.pdf");
    let sizes = [(120u32, 80u32), (150u32, 90u32)];
    std::fs::write(
        &pdf_path,
        build_pdf(&sizes.map(|(w, h)| (w, h, None))),
    )
    .unwrap();

    let out_dir = dir.path().join("out");
    let renderer = QuartzRenderer::new().unwrap();
    let pages = renderer.render_pages(&pdf_path, &out_dir).unwrap();

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].file_name().unwrap(), "page_0001.png");
    assert_eq!(pages[1].file_name().unwrap(), "page_0002.png");

    for (path, (w, h)) in pages.iter().zip(sizes.iter()) {
        assert!(path.exists());
        let img = image::open(path).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (w * 2, h * 2));
        // Blank page (no content stream fill) should rasterize all white.
        assert!(img.pixels().all(is_white));
    }
}

#[test]
fn errors_on_nonexistent_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let renderer = QuartzRenderer::new().unwrap();
    let result = renderer.render_pages(
        Path::new("/nonexistent/does-not-exist-anywhere.pdf"),
        &dir.path().join("out"),
    );
    assert!(result.is_err());
}

#[test]
fn errors_on_invalid_pdf_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("bad.pdf");
    std::fs::write(&bad, b"this is not a pdf at all").unwrap();
    let renderer = QuartzRenderer::new().unwrap();
    let result = renderer.render_pages(&bad, &dir.path().join("out"));
    assert!(result.is_err(), "garbage bytes should not open as a PDF document");
}
