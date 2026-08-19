use notemd_lib::themes::import::{prepare_import, ImportReport};
use std::fs;
use std::io::Write;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

fn make_zip(dir: &std::path::Path, entries: &[(&str, &[u8])]) -> std::path::PathBuf {
    let path = dir.join("t.zip");
    let f = fs::File::create(&path).unwrap();
    let mut zw = zip::ZipWriter::new(f);
    for (name, body) in entries {
        zw.start_file(*name, SimpleFileOptions::default()).unwrap();
        zw.write_all(body).unwrap();
    }
    zw.finish().unwrap();
    path
}

#[test]
fn detects_three_themes_from_typora_zip() {
    let scratch = tempdir().unwrap();
    let zip = make_zip(scratch.path(), &[
        ("claude-like.css",      b"/*\n * Theme Name: Claude-Like\n * Appearance: light\n */\n:root {}"),
        ("claude-like-grey.css", b"/*\n * Theme Name: Claude-Like Grey\n */\n:root {}"),
        ("claude-like-dark.css", b"/*\n * Theme Name: Claude-Like Dark\n * Appearance: dark\n */\n:root {}"),
    ]);
    let existing_ids = vec!["default".to_string(), "effie".to_string()];
    let report: ImportReport = prepare_import(&zip, &existing_ids).unwrap();
    assert_eq!(report.themes.len(), 3);
    assert_eq!(report.themes[0].id, "claude-like");
    assert_eq!(report.themes[0].appearance.as_str(), "light");
    assert_eq!(report.themes[1].id, "claude-like-dark");
    assert_eq!(report.themes[1].appearance.as_str(), "dark");
    assert_eq!(report.themes[2].id, "claude-like-grey");
    assert!(report.themes.iter().all(|t| !t.conflict));
    assert!(report.asset_dirs.is_empty());
}

#[test]
fn flags_conflicts_with_existing_ids() {
    let scratch = tempdir().unwrap();
    let zip = make_zip(scratch.path(), &[
        ("default.css", b"/*\n * Theme Name: Default Replacement\n */\n:root {}"),
        ("brand-new.css", b":root {}"),
    ]);
    let existing = vec!["default".to_string()];
    let report = prepare_import(&zip, &existing).unwrap();
    let by_id = |id: &str| report.themes.iter().find(|t| t.id == id).unwrap();
    assert!(by_id("default").conflict);
    assert!(!by_id("brand-new").conflict);
}

#[test]
fn detects_same_name_asset_directories() {
    let scratch = tempdir().unwrap();
    let zip = make_zip(scratch.path(), &[
        ("claude-like.css", b":root {}"),
        ("claude-like/fonts/x.woff2", b"font-bytes"),
    ]);
    let report = prepare_import(&zip, &[]).unwrap();
    assert_eq!(report.themes.len(), 1);
    assert_eq!(report.asset_dirs, vec!["claude-like".to_string()]);
}

#[test]
fn ignores_non_css_root_files_silently() {
    let scratch = tempdir().unwrap();
    let zip = make_zip(scratch.path(), &[
        ("ok.css", b":root {}"),
        ("README.md", b"# readme"),
        ("screenshot.png", b""),
    ]);
    let report = prepare_import(&zip, &[]).unwrap();
    assert_eq!(report.themes.len(), 1);
    assert_eq!(report.themes[0].id, "ok");
}

#[test]
fn invalid_css_is_reported_and_excluded() {
    let scratch = tempdir().unwrap();
    let zip = make_zip(scratch.path(), &[
        ("ok.css", b":root {}"),
        ("broken.css", b":root { color:"),  // unterminated
    ]);
    let report = prepare_import(&zip, &[]).unwrap();
    let ids: Vec<&str> = report.themes.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["ok"]);
    assert_eq!(report.errors.len(), 1);
    assert!(report.errors[0].file == "broken.css");
}

#[test]
fn empty_zip_returns_empty_report_no_error() {
    let scratch = tempdir().unwrap();
    let zip = make_zip(scratch.path(), &[
        ("README.md", b"hi"),
    ]);
    let report = prepare_import(&zip, &[]).unwrap();
    assert!(report.themes.is_empty());
    assert!(report.errors.is_empty());
}

#[test]
fn returns_temp_path_for_install() {
    let scratch = tempdir().unwrap();
    let zip = make_zip(scratch.path(), &[("ok.css", b":root {}")]);
    let report = prepare_import(&zip, &[]).unwrap();
    assert!(report.staging_dir.exists());
    assert!(report.staging_dir.join("ok.css").exists());
}
