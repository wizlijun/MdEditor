use super::*;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn detect_with_candidates_skips_nonexistent_paths_without_spawning() {
    let start = Instant::now();
    let result = detect_with_candidates(&[PathBuf::from("/no/such/ebook-convert-xyz")], Duration::from_secs(5));
    assert!(result.is_none());
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "nonexistent candidate should be skipped without spawning a process"
    );
}

#[test]
fn detect_with_candidates_finds_ok_fixture_and_captures_version() {
    let candidates = vec![
        PathBuf::from("/no/such/ebook-convert-xyz"),
        fixture("ebook-convert-ok.sh"),
    ];
    let detected = detect_with_candidates(&candidates, Duration::from_secs(5))
        .expect("ok fixture should be detected");
    assert!(
        detected.version.contains("calibre 7.0"),
        "unexpected version string: {}",
        detected.version
    );
    assert!(detected.path.ends_with("ebook-convert-ok.sh"));
}

#[test]
fn detect_with_candidates_treats_hang_as_timeout_and_returns_none() {
    let candidates = vec![fixture("ebook-convert-hang.sh")];
    let start = Instant::now();
    let result = detect_with_candidates(&candidates, Duration::from_secs(2));
    assert!(result.is_none(), "a wedged candidate must not be detected");
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "timeout handling should cut the hang off close to the requested timeout"
    );
}

#[test]
fn detect_finds_override_candidate_via_the_public_shell() {
    let detected = detect(Some(fixture("ebook-convert-ok.sh").to_str().unwrap()))
        .expect("override candidate should be detected through the public detect() shell");
    assert!(detected.version.contains("calibre 7.0"));
}

#[test]
fn candidates_places_device_override_first() {
    let list = candidates(Some("/tmp/custom-ebook-convert"));
    assert_eq!(list[0], PathBuf::from("/tmp/custom-ebook-convert"));
}

#[test]
fn candidates_without_override_still_includes_well_known_paths() {
    let list = candidates(None);
    assert!(list.contains(&PathBuf::from(
        "/Applications/calibre.app/Contents/MacOS/ebook-convert"
    )));
    assert!(list.contains(&PathBuf::from("/usr/local/bin/ebook-convert")));
    assert!(list.contains(&PathBuf::from("/opt/homebrew/bin/ebook-convert")));
    assert!(list.contains(&PathBuf::from("/usr/bin/ebook-convert")));
}

#[test]
fn resolve_calibre_path_value_appends_binary_when_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let resolved = resolve_calibre_path_value(tmp.path().to_str().unwrap());
    assert_eq!(resolved, tmp.path().join("ebook-convert"));
}

#[test]
fn resolve_calibre_path_value_keeps_file_path_as_is() {
    let resolved = resolve_calibre_path_value("/usr/local/bin/ebook-convert");
    assert_eq!(resolved, PathBuf::from("/usr/local/bin/ebook-convert"));
}

#[test]
fn convert_to_htmlz_ok_fixture_produces_output_file() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("book.epub");
    std::fs::write(&input, b"fake epub bytes").unwrap();
    let out = tmp.path().join("book.htmlz");

    let result = convert_to_htmlz(
        fixture("ebook-convert-ok.sh").to_str().unwrap(),
        &input,
        &out,
    );

    assert!(result.is_ok(), "unexpected error: {:?}", result.err());
    assert!(out.exists(), "ok fixture should have produced the output file");
}

#[test]
fn convert_to_htmlz_reports_err_on_process_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("book.epub");
    std::fs::write(&input, b"fake epub bytes").unwrap();
    let out = tmp.path().join("book.htmlz");

    let result = convert_to_htmlz("/usr/bin/false", &input, &out);

    assert!(result.is_err(), "a failing ebook-convert must surface as Err");
}

#[test]
fn tail_excerpt_keeps_only_the_last_max_chars() {
    let text = "a".repeat(10) + &"b".repeat(5);
    let excerpt = tail_excerpt(&text, 5);
    assert_eq!(excerpt, "bbbbb");
}

#[test]
fn tail_excerpt_reports_placeholder_for_empty_input() {
    assert_eq!(tail_excerpt("   \n", 100), "(no output)");
}
