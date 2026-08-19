use notemd_lib::themes::zip_safety::{extract_zip_safely, ExtractError, ExtractLimits};
use std::fs;
use std::io::Write;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

fn make_zip(dir: &std::path::Path, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
    let path = dir.join("test.zip");
    let f = fs::File::create(&path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    for (name, body) in entries {
        zw.start_file(*name, SimpleFileOptions::default()).unwrap();
        zw.write_all(body).unwrap();
    }
    zw.finish().unwrap();
    path
}

fn small_limits() -> ExtractLimits {
    ExtractLimits { max_entry_bytes: 5 * 1024 * 1024, max_total_bytes: 20 * 1024 * 1024 }
}

#[test]
fn extracts_valid_zip() {
    let scratch = tempdir().unwrap();
    let target = tempdir().unwrap();
    let zip = make_zip(scratch.path(), &[
        ("claude-like.css", b":root {}"),
        ("claude-like/fonts/x.txt", b"font-data"),
    ]);
    let report = extract_zip_safely(&zip, target.path(), small_limits()).unwrap();
    assert_eq!(report.entries_extracted, 2);
    assert!(target.path().join("claude-like.css").exists());
    assert!(target.path().join("claude-like/fonts/x.txt").exists());
}

#[test]
fn rejects_path_traversal() {
    let scratch = tempdir().unwrap();
    let target = tempdir().unwrap();
    let zip = make_zip(scratch.path(), &[
        ("../escape.css", b"bad"),
    ]);
    let err = extract_zip_safely(&zip, target.path(), small_limits()).unwrap_err();
    assert!(matches!(err, ExtractError::PathTraversal(_)));
}

#[test]
fn rejects_absolute_paths() {
    let scratch = tempdir().unwrap();
    let target = tempdir().unwrap();
    let zip = make_zip(scratch.path(), &[
        ("/etc/passwd", b"bad"),
    ]);
    let err = extract_zip_safely(&zip, target.path(), small_limits()).unwrap_err();
    assert!(matches!(err, ExtractError::PathTraversal(_)));
}

#[test]
fn rejects_per_entry_overflow() {
    let scratch = tempdir().unwrap();
    let target = tempdir().unwrap();
    let big: Vec<u8> = vec![b'x'; 6 * 1024 * 1024];
    let zip = make_zip(scratch.path(), &[("huge.css", &big)]);
    let err = extract_zip_safely(&zip, target.path(), small_limits()).unwrap_err();
    assert!(matches!(err, ExtractError::EntryTooLarge { .. }));
}

#[test]
fn rejects_total_overflow() {
    let scratch = tempdir().unwrap();
    let target = tempdir().unwrap();
    let chunk: Vec<u8> = vec![b'x'; 4 * 1024 * 1024];
    let zip = make_zip(scratch.path(), &[
        ("a.css", &chunk),
        ("b.css", &chunk),
        ("c.css", &chunk),
        ("d.css", &chunk),
        ("e.css", &chunk),
        ("f.css", &chunk),  // total 24 MB > 20 MB cap
    ]);
    let err = extract_zip_safely(&zip, target.path(), small_limits()).unwrap_err();
    assert!(matches!(err, ExtractError::TotalTooLarge { .. }));
}

#[test]
fn corrupt_zip_returns_err() {
    let scratch = tempdir().unwrap();
    let target = tempdir().unwrap();
    let zip = scratch.path().join("bad.zip");
    fs::write(&zip, b"not a zip").unwrap();
    let err = extract_zip_safely(&zip, target.path(), small_limits()).unwrap_err();
    assert!(matches!(err, ExtractError::Corrupt(_)));
}
