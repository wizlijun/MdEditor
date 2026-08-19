use notemd_lib::themes::registry::{scan_themes_dir, ThemeMeta};
use std::fs;
use tempfile::tempdir;

fn write(dir: &std::path::Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).unwrap();
}

#[test]
fn empty_dir_returns_empty_vec() {
    let d = tempdir().unwrap();
    let list = scan_themes_dir(d.path(), &["default", "effie"]).unwrap();
    assert!(list.is_empty());
}

#[test]
fn picks_up_css_with_header() {
    let d = tempdir().unwrap();
    write(d.path(), "claude-like.css",
        "/*\n * Theme Name: Claude-Like\n * Appearance: light\n */\n:root {}");
    let list = scan_themes_dir(d.path(), &["default"]).unwrap();
    assert_eq!(list.len(), 1);
    let m: &ThemeMeta = &list[0];
    assert_eq!(m.id, "claude-like");
    assert_eq!(m.name, "Claude-Like");
    assert_eq!(m.appearance.as_str(), "light");
    assert!(!m.built_in);
}

#[test]
fn no_header_uses_filename_heuristic_and_title_case() {
    let d = tempdir().unwrap();
    write(d.path(), "claude-like-dark.css", ":root {}");
    let list = scan_themes_dir(d.path(), &["default"]).unwrap();
    let m = &list[0];
    assert_eq!(m.id, "claude-like-dark");
    assert_eq!(m.name, "Claude-Like Dark");
    assert_eq!(m.appearance.as_str(), "dark");
}

#[test]
fn built_in_flag_is_set_for_known_ids() {
    let d = tempdir().unwrap();
    write(d.path(), "default.css", ":root {}");
    write(d.path(), "custom.css", ":root {}");
    let list = scan_themes_dir(d.path(), &["default", "effie"]).unwrap();
    let by_id = |id: &str| list.iter().find(|m| m.id == id).unwrap().built_in;
    assert!(by_id("default"));
    assert!(!by_id("custom"));
}

#[test]
fn skips_non_css_files_and_subdirs_quietly() {
    let d = tempdir().unwrap();
    write(d.path(), "valid.css", ":root {}");
    write(d.path(), "README.md", "hi");
    write(d.path(), "screenshot.png", "");
    fs::create_dir(d.path().join("valid")).unwrap();
    fs::create_dir(d.path().join(".compiled")).unwrap();
    let list = scan_themes_dir(d.path(), &["default"]).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "valid");
}

#[test]
fn skips_invalid_ids_with_warning() {
    let d = tempdir().unwrap();
    write(d.path(), "Bad Name.css", ":root {}");
    write(d.path(), "ok.css", ":root {}");
    let list = scan_themes_dir(d.path(), &["default"]).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "ok");
}

#[test]
fn list_is_sorted_by_display_name() {
    let d = tempdir().unwrap();
    write(d.path(), "zebra.css", ":root {}");
    write(d.path(), "apple.css", ":root {}");
    let list = scan_themes_dir(d.path(), &["default"]).unwrap();
    let names: Vec<&str> = list.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["Apple", "Zebra"]);
}

#[test]
fn missing_dir_returns_empty_not_error() {
    let d = tempdir().unwrap();
    let missing = d.path().join("does-not-exist");
    let list = scan_themes_dir(&missing, &["default"]).unwrap();
    assert!(list.is_empty());
}
