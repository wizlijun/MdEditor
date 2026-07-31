use super::*;
use std::io::Write;
use zip::write::{SimpleFileOptions, ZipWriter};

/// Builds an in-memory-constructed HTMLZ zip on disk at `path` from
/// `(entry_name, contents)` pairs -- no binary fixture files needed, per
/// the brief.
fn write_htmlz(path: &Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    for (name, contents) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(contents).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn cleans_calibre_markers() {
    let md = "Title{.calibre1}\n[x](#calibre_link-12)\n::: div\n42\nfoo\nbar .ct}\n\n\n\nend";
    let out = clean_calibre_markers(md);
    assert!(!out.contains("{.calibre"));
    assert!(!out.contains("#calibre_link"));
    assert!(!out.contains(":::"));
    assert!(!out.lines().any(|l| l.trim() == "42")); // 纯数字行删
    assert!(!out.contains(".ct}"));
    assert!(out.contains("foo"));
    assert!(!out.contains("\n\n\n")); // ≥3 空行折叠
}

#[test]
fn clean_calibre_markers_drops_cn_suffixed_lines_and_normalizes_whitespace() {
    let md = "kept\ncaption remnant .cn}\n\u{feff}BOM\u{a0}here";
    let out = clean_calibre_markers(md);
    assert!(out.contains("kept"));
    assert!(!out.contains(".cn}"));
    assert!(!out.contains('\u{feff}'));
    assert!(out.contains("BOM here"));
}

#[test]
fn extract_reads_opf_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let htmlz_path = tmp.path().join("book.htmlz");
    let opf = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns:dc="http://purl.org/dc/elements/1.1/">
  <metadata>
    <dc:title>七力</dc:title>
    <dc:creator>H</dc:creator>
    <dc:language>zh</dc:language>
  </metadata>
</package>"#;
    write_htmlz(
        &htmlz_path,
        &[
            ("index.html", b"<html><body><h1>Hi</h1></body></html>"),
            ("images/a.png", &[0x89, 0x50, 0x4e, 0x47]),
            ("metadata.opf", opf.as_bytes()),
        ],
    );

    let work = tmp.path().join("work");
    let extracted = extract(&htmlz_path, &work).expect("extract should succeed");

    assert!(extracted.html.ends_with("index.html"));
    assert_eq!(extracted.meta.title.as_deref(), Some("七力"));
    assert_eq!(extracted.meta.creator.as_deref(), Some("H"));
    assert_eq!(extracted.meta.language.as_deref(), Some("zh"));
    let images_dir = extracted.images_dir.expect("images dir should be found");
    assert_eq!(images_dir.file_name().unwrap(), "images");
    assert!(images_dir.join("a.png").is_file());
}

#[test]
fn extract_falls_back_without_opf_or_images_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let htmlz_path = tmp.path().join("book.htmlz");
    write_htmlz(
        &htmlz_path,
        &[("content.htm", b"<html><body><p>Hello</p></body></html>")],
    );

    let work = tmp.path().join("work");
    let extracted = extract(&htmlz_path, &work).expect("extract should succeed");

    assert!(extracted.html.ends_with("content.htm"));
    assert!(extracted.images_dir.is_none());
    assert!(extracted.meta.title.is_none());
    assert!(extracted.meta.creator.is_none());
    assert!(extracted.meta.publisher.is_none());
    assert!(extracted.meta.language.is_none());
}

#[test]
fn html_to_markdown_strips_calibre_markers_and_skipped_tags() {
    let html = "<html><body>\
        <script>evil()</script>\
        <h1>Title{.calibre1}</h1>\
        <p>Body text.</p>\
        </body></html>";
    let md = html_to_markdown(html).expect("conversion should succeed");
    assert!(!md.contains("evil()"));
    assert!(!md.contains("{.calibre"));
    assert!(md.contains("Body text."));
}
