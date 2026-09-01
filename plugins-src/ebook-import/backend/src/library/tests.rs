use super::*;

/// Lays down `<vault>/<root>/<month>/<name>/` and the files named in `files`.
fn book(vault: &Path, root: &str, month: &str, name: &str, files: &[&str]) {
    let dir = vault.join(root).join(month).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    for f in files {
        std::fs::write(dir.join(f), "x").unwrap();
    }
}

fn book_with_added_at(vault: &Path, month: &str, name: &str, added_at: &str) {
    book(vault, "ssot/ebooks", month, name, &["book.md"]);
    std::fs::write(
        vault
            .join("ssot/ebooks")
            .join(month)
            .join(name)
            .join("meta.yml"),
        format!("added_at: {added_at}\n"),
    )
    .unwrap();
}

#[test]
fn books_are_sorted_by_added_at_newest_first_even_across_months() {
    let tmp = tempfile::tempdir().unwrap();
    let v = tmp.path();
    book_with_added_at(v, "2026-08", "Older In Newer Month", "2026-08-01T00:00:00Z");
    book_with_added_at(v, "2026-07", "Newest", "2026-08-02T00:00:00Z");
    book_with_added_at(v, "2026-06", "Oldest", "2026-07-31T23:59:59Z");

    let names: Vec<String> = scan(v, "ssot/ebooks").into_iter().map(|b| b.name).collect();

    assert_eq!(names, ["Newest", "Older In Newer Month", "Oldest"]);
}

#[test]
fn equal_added_at_uses_month_then_name_as_a_stable_tie_breaker() {
    let tmp = tempfile::tempdir().unwrap();
    let v = tmp.path();
    for (month, name) in [("2026-07", "Zeta"), ("2026-08", "Mu"), ("2026-08", "Alpha")] {
        book_with_added_at(v, month, name, "2026-08-01T05:42:00Z");
    }

    let names: Vec<String> = scan(v, "ssot/ebooks").into_iter().map(|b| b.name).collect();

    assert_eq!(names, ["Alpha", "Mu", "Zeta"]);
}

#[test]
fn missing_or_invalid_added_at_sorts_after_valid_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let v = tmp.path();
    book_with_added_at(v, "2026-01", "Valid", "2026-01-01T00:00:00Z");
    book_with_added_at(v, "2026-09", "Broken", "not-a-timestamp");
    book(v, "ssot/ebooks", "2026-10", "Missing", &["book.md"]);

    let names: Vec<String> = scan(v, "ssot/ebooks").into_iter().map(|b| b.name).collect();

    assert_eq!(names, ["Valid", "Missing", "Broken"]);
}

#[test]
fn scan_lists_every_book_dir_newest_month_first() {
    let tmp = tempfile::tempdir().unwrap();
    let v = tmp.path();
    book(v, "ssot/ebooks", "2026-07", "Old Book", &["book.md"]);
    book(v, "ssot/ebooks", "2026-08", "Seven Powers", &["book.md"]);

    let got = scan(v, "ssot/ebooks");

    assert_eq!(got.len(), 2);
    assert_eq!(got[0].name, "Seven Powers");
    assert_eq!(got[0].month, "2026-08");
    assert_eq!(got[0].rel, "ssot/ebooks/2026-08/Seven Powers");
    assert_eq!(got[1].name, "Old Book");
    assert_eq!(got[1].rel, "ssot/ebooks/2026-07/Old Book");
}

/// Same month: alphabetical, so the list doesn't reshuffle between refreshes
/// (readdir order is not stable across platforms or filesystems).
#[test]
fn books_in_one_month_are_sorted_by_name() {
    let tmp = tempfile::tempdir().unwrap();
    let v = tmp.path();
    for n in ["Zeta", "Alpha", "Mu"] {
        book(v, "ssot/ebooks", "2026-08", n, &["book.md"]);
    }
    let names: Vec<String> = scan(v, "ssot/ebooks").into_iter().map(|b| b.name).collect();
    assert_eq!(names, ["Alpha", "Mu", "Zeta"]);
}

/// A directory without `book.md` is not a book. `work/` scratch dirs, a
/// half-finished import, or anything the user filed here by hand must not show
/// up as a readable book — every row in the list offers "AI read", and that
/// job would have nothing to read.
#[test]
fn a_dir_without_book_md_is_not_a_book() {
    let tmp = tempfile::tempdir().unwrap();
    let v = tmp.path();
    book(v, "ssot/ebooks", "2026-08", "Real", &["book.md"]);
    book(v, "ssot/ebooks", "2026-08", "Not A Book", &["config.txt"]);
    // A stray file directly under the month dir is not a book either.
    std::fs::write(v.join("ssot/ebooks/2026-08/loose.md"), "x").unwrap();

    let names: Vec<String> = scan(v, "ssot/ebooks").into_iter().map(|b| b.name).collect();
    assert_eq!(names, ["Real"]);
}

/// Only `YYYY-MM-DD-summary.md` counts, newest first — the window shows the
/// latest one and offers to re-read.
#[test]
fn summaries_are_collected_newest_first() {
    let tmp = tempfile::tempdir().unwrap();
    let v = tmp.path();
    book(
        v,
        "ssot/ebooks",
        "2026-08",
        "Seven Powers",
        &[
            "book.md",
            "2026-08-04-summary.md",
            "2026-08-26-summary.md",
            "config.txt",
            "notes.md",
            "summary.md",
        ],
    );

    let got = scan(v, "ssot/ebooks");
    assert_eq!(
        got[0].summaries,
        ["2026-08-26-summary.md", "2026-08-04-summary.md"]
    );
}

#[test]
fn a_book_with_no_summary_reports_none() {
    let tmp = tempfile::tempdir().unwrap();
    let v = tmp.path();
    book(v, "ssot/ebooks", "2026-08", "Unread", &["book.md"]);
    assert!(scan(v, "ssot/ebooks")[0].summaries.is_empty());
}

/// Nothing imported yet: an empty library, not an error. The window shows this
/// on first open, before the vault has any books at all.
#[test]
fn a_missing_root_scans_to_an_empty_library() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(scan(tmp.path(), "ssot/ebooks").is_empty());
}

/// Defense in depth, mirroring `run_import`: `.notemd/ebook-import.json` is
/// hand-editable, so a root that climbs out of the vault can reach this
/// function even though `apply_vault_patch` rejects it at save time. Listing
/// `../..` would walk the user's home directory into the window.
#[test]
fn an_escaping_root_scans_to_nothing_rather_than_walking_out() {
    let tmp = tempfile::tempdir().unwrap();
    let v = tmp.path();
    book(v, "ssot/ebooks", "2026-08", "Real", &["book.md"]);
    assert!(scan(v, "../..").is_empty());
    assert!(scan(v, "/etc").is_empty());
    assert!(scan(v, "").is_empty());
}

#[cfg(unix)]
#[test]
fn a_symlinked_library_root_does_not_list_books_outside_the_vault() {
    use std::os::unix::fs::symlink;

    let vault = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    book(outside.path(), "", "2026-08", "Outside", &["book.md"]);
    std::fs::create_dir_all(vault.path().join("ssot")).unwrap();
    symlink(outside.path(), vault.path().join("ssot/ebooks")).unwrap();
    assert!(scan(vault.path(), "ssot/ebooks").is_empty());
}

#[cfg(unix)]
#[test]
fn a_symlinked_book_markdown_is_not_a_library_book() {
    use std::os::unix::fs::symlink;

    let vault = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let dir = vault.path().join("ssot/ebooks/2026-08/Symlink Book");
    std::fs::create_dir_all(&dir).unwrap();
    let outside_book = outside.path().join("secret.md");
    std::fs::write(&outside_book, "outside").unwrap();
    symlink(&outside_book, dir.join("book.md")).unwrap();

    assert!(scan(vault.path(), "ssot/ebooks").is_empty());
}

#[cfg(unix)]
#[test]
fn a_symlinked_meta_is_treated_as_missing() {
    use std::os::unix::fs::symlink;

    let vault = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    book(
        vault.path(),
        "ssot/ebooks",
        "2026-08",
        "Symlink Meta",
        &["book.md"],
    );
    book_with_added_at(vault.path(), "2026-01", "Valid", "2026-01-01T00:00:00Z");
    let external_meta = outside.path().join("meta.yml");
    std::fs::write(
        &external_meta,
        "added_at: 2099-01-01T00:00:00Z\ntopic_id: leaked-topic\n",
    )
    .unwrap();
    symlink(
        &external_meta,
        vault
            .path()
            .join("ssot/ebooks/2026-08/Symlink Meta/meta.yml"),
    )
    .unwrap();

    let got = scan(vault.path(), "ssot/ebooks");
    assert_eq!(
        got.iter()
            .map(|book| book.name.as_str())
            .collect::<Vec<_>>(),
        ["Valid", "Symlink Meta"]
    );
    assert_eq!(got[1].topic_id, None);
}

#[cfg(unix)]
#[test]
fn a_symlinked_summary_is_not_exposed_by_the_library() {
    use std::os::unix::fs::symlink;

    let vault = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let dir = vault.path().join("ssot/ebooks/2026-08/Book");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("book.md"), "book").unwrap();
    std::fs::write(dir.join("2026-08-01-summary.md"), "inside").unwrap();
    let external = outside.path().join("secret.md");
    std::fs::write(&external, "outside").unwrap();
    symlink(&external, dir.join("2026-08-02-summary.md")).unwrap();

    let got = scan(vault.path(), "ssot/ebooks");
    assert_eq!(got[0].summaries, ["2026-08-01-summary.md"]);
}

/// The nesting is exactly two levels. A book one level too deep is not a book
/// dir, and neither is the month dir itself.
#[test]
fn only_the_month_slash_book_nesting_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let v = tmp.path();
    // <root>/<month>/<book>/<sub>/book.md — too deep.
    let deep = v.join("ssot/ebooks/2026-08/Outer/Inner");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("book.md"), "x").unwrap();
    // <root>/<book>/book.md — too shallow.
    let shallow = v.join("ssot/ebooks/Shallow");
    std::fs::create_dir_all(&shallow).unwrap();
    std::fs::write(shallow.join("book.md"), "x").unwrap();

    assert!(scan(v, "ssot/ebooks").is_empty());
}
