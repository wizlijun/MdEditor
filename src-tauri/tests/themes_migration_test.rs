use notemd_lib::themes::migration::copy_built_ins_if_missing;
use std::fs;
use tempfile::tempdir;

#[test]
fn copies_all_built_ins_when_themes_dir_empty() {
    let res = tempdir().unwrap();
    let themes = tempdir().unwrap();
    fs::write(res.path().join("default.css"), "/* d */").unwrap();
    fs::write(res.path().join("effie.css"), "/* e */").unwrap();
    let n = copy_built_ins_if_missing(res.path(), themes.path(), &["default", "effie"]).unwrap();
    assert_eq!(n, 2);
    assert!(themes.path().join("default.css").exists());
    assert!(themes.path().join("effie.css").exists());
}

#[test]
fn does_not_overwrite_existing() {
    let res = tempdir().unwrap();
    let themes = tempdir().unwrap();
    fs::write(res.path().join("default.css"), "/* new */").unwrap();
    fs::write(themes.path().join("default.css"), "/* user-edited */").unwrap();
    let n = copy_built_ins_if_missing(res.path(), themes.path(), &["default"]).unwrap();
    assert_eq!(n, 0);
    let body = fs::read_to_string(themes.path().join("default.css")).unwrap();
    assert_eq!(body, "/* user-edited */");
}

#[test]
fn missing_resource_is_warning_not_error() {
    let res = tempdir().unwrap();
    let themes = tempdir().unwrap();
    let n = copy_built_ins_if_missing(res.path(), themes.path(), &["default"]).unwrap();
    assert_eq!(n, 0);
    assert!(!themes.path().join("default.css").exists());
}
