use super::store::Record;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// SHA-256 hex digest of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Outcome of checking whether an opened vault copy needs updating from source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateOutcome {
    NotTracked,
    SourceMissing,
    UpToDate,
    OriginUpdated,
    Conflict,
}

/// Pure decision: compare current on-disk hashes against the record's
/// last-sync fingerprints. One-directional + conflict-aware:
/// - source unchanged  -> UpToDate (regardless of vault side)
/// - source changed, vault untouched -> OriginUpdated
/// - source changed, vault also changed -> Conflict
pub fn decide_update(record: &Record, source_now: &str, vault_now: &str) -> UpdateOutcome {
    if source_now == record.source_hash {
        return UpdateOutcome::UpToDate;
    }
    if vault_now == record.vault_hash {
        UpdateOutcome::OriginUpdated
    } else {
        UpdateOutcome::Conflict
    }
}

/// Read source + vault files and decide. Missing source -> SourceMissing.
pub fn check_update_io(record: &Record, source: &Path, vault: &Path) -> Result<UpdateOutcome, String> {
    if !source.exists() {
        return Ok(UpdateOutcome::SourceMissing);
    }
    let src = std::fs::read(source).map_err(|e| e.to_string())?;
    let vlt = std::fs::read(vault).map_err(|e| e.to_string())?;
    Ok(decide_update(record, &sha256_hex(&src), &sha256_hex(&vlt)))
}

/// Pick a non-colliding destination inside `dir` for `basename`. If
/// `dir/basename` is free, return it; otherwise append `-2`, `-3`, ... before
/// the extension. `exists` decides occupancy (injected for testability).
pub fn dedup_target(dir: &Path, basename: &str, exists: &dyn Fn(&Path) -> bool) -> PathBuf {
    let first = dir.join(basename);
    if !exists(&first) {
        return first;
    }
    let (stem, ext) = split_ext(basename);
    let mut n = 2;
    loop {
        let candidate = match &ext {
            Some(e) => dir.join(format!("{stem}-{n}.{e}")),
            None => dir.join(format!("{stem}-{n}")),
        };
        if !exists(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Choose where to write a sync copy. When `existing` is a tracked vault copy
/// that still exists, reuse it (in-place update — no proliferating `-2` copies);
/// otherwise fall back to a fresh non-colliding `dedup_target`.
pub fn sync_target(
    existing: Option<PathBuf>,
    dir: &Path,
    basename: &str,
    exists: &dyn Fn(&Path) -> bool,
) -> PathBuf {
    match existing {
        Some(p) if exists(&p) => p,
        _ => dedup_target(dir, basename, exists),
    }
}

fn split_ext(name: &str) -> (String, Option<String>) {
    match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), Some(name[i + 1..].to_string())),
        _ => (name.to_string(), None),
    }
}

/// True when `file` is inside `vault_root`.
pub fn is_under_vault(vault_root: &Path, file: &Path) -> bool {
    file.starts_with(vault_root)
}

/// True when `name`'s extension is `md` or `mdx` (case-insensitive).
fn is_markdown(name: &str) -> bool {
    match name.rfind('.') {
        Some(i) if i + 1 < name.len() => {
            let ext = &name[i + 1..];
            ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("mdx")
        }
        _ => false,
    }
}

/// True when `name` already starts with a `yyyy-MM-dd-` prefix.
fn has_date_prefix(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() < 11 {
        return false;
    }
    let d = |i: usize| b[i].is_ascii_digit();
    d(0) && d(1) && d(2) && d(3)
        && b[4] == b'-'
        && d(5) && d(6)
        && b[7] == b'-'
        && d(8) && d(9)
        && b[10] == b'-'
}

/// For Markdown files lacking a `yyyy-MM-dd-` prefix, prepend `<date_prefix>-`.
/// Non-Markdown files and already-dated names are returned unchanged.
pub fn dated_basename(basename: &str, date_prefix: &str) -> String {
    if !is_markdown(basename) || has_date_prefix(basename) {
        return basename.to_string();
    }
    format!("{date_prefix}-{basename}")
}

/// Image file extensions (lowercase), mirroring paste-resources.ts.
const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico", "tiff", "tif", "avif",
];

/// True when `name`'s extension (case-insensitive) is a known image type.
pub fn is_image_ext(name: &str) -> bool {
    match name.rfind('.') {
        Some(i) if i + 1 < name.len() => {
            let ext = name[i + 1..].to_ascii_lowercase();
            IMAGE_EXTENSIONS.contains(&ext.as_str())
        }
        _ => false,
    }
}

/// The per-md assets directory name derived from the vault md file stem.
pub fn assets_dir_name(stem: &str) -> String {
    format!("{stem}.assets")
}

/// Companion outline-note filename (`foo.md` → `foo.note.md`) for a markdown
/// basename. None for note files themselves and for non-markdown names —
/// mirrors `companionPathFor` in src/lib/outline/store.svelte.ts.
pub fn companion_note_name(basename: &str) -> Option<String> {
    let lower = basename.to_lowercase();
    if lower.ends_with(".note.md") || lower.ends_with(".notes.md") {
        return None;
    }
    for ext in [".md", ".markdown", ".mdown", ".mkd"] {
        if lower.ends_with(ext) {
            let stem = &basename[..basename.len() - ext.len()];
            return Some(format!("{stem}.note.md"));
        }
    }
    None
}

/// Scan markdown for inline image links `![alt](target)` and return each raw
/// `target` string (the text between the parentheses), in document order.
/// v1: no nested `]`/`)`, no reference-style, no HTML.
pub fn scan_image_link_targets(md: &str) -> Vec<String> {
    let b = md.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < b.len() {
        if b[i] == b'!' && b[i + 1] == b'[' {
            if let Some(close) = find_byte(b, i + 2, b']') {
                if close + 1 < b.len() && b[close + 1] == b'(' {
                    if let Some(rparen) = find_byte(b, close + 2, b')') {
                        out.push(md[close + 2..rparen].to_string());
                        i = rparen + 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    out
}

fn find_byte(b: &[u8], from: usize, needle: u8) -> Option<usize> {
    (from..b.len()).find(|&j| b[j] == needle)
}

/// Extract the path portion from a raw link target, stripping an optional
/// `"title"` and unwrapping `<...>` angle brackets.
pub fn extract_link_path(raw: &str) -> String {
    let t = raw.trim();
    if let Some(stripped) = t.strip_prefix('<') {
        return stripped.split('>').next().unwrap_or("").to_string();
    }
    // Path runs until the first ASCII whitespace (a title, if any, follows).
    match t.find(char::is_whitespace) {
        Some(i) => t[..i].to_string(),
        None => t.to_string(),
    }
}

/// True when `p` is a relative local path (not a URL, not absolute, not data:).
pub fn is_relative_local(p: &str) -> bool {
    let t = p.trim();
    if t.is_empty() || t.starts_with('/') || t.starts_with('#') {
        return false;
    }
    if t.starts_with("data:") || t.contains("://") {
        return false;
    }
    // Windows drive-absolute, e.g. C:\...
    let b = t.as_bytes();
    if b.len() >= 2 && b[1] == b':' {
        return false;
    }
    true
}

/// Rebuild a raw link target with `new_path` swapped in, preserving an optional
/// `"title"` and `<...>` angle brackets.
pub fn rewrite_link_target(raw: &str, new_path: &str) -> String {
    let t = raw.trim();
    if t.starts_with('<') {
        // <path>rest  ->  <new_path>rest
        let after = match t.find('>') {
            Some(i) => &t[i + 1..],
            None => "",
        };
        return format!("<{new_path}>{after}");
    }
    match t.find(char::is_whitespace) {
        Some(i) => format!("{}{}", new_path, &t[i..]),
        None => new_path.to_string(),
    }
}

/// One file to copy into the assets dir: absolute source -> dest filename.
#[derive(Debug, Clone, PartialEq)]
pub struct CopyOp {
    pub src_abs: PathBuf,
    pub dest_filename: String,
}

/// One planned link rewrite. Applied by the caller only after the matching
/// CopyOp copies successfully (keeps md refs consistent with copied files).
/// The caller applies rewrites via `str::replace` on `original`, which replaces all identical occurrences at once.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedRef {
    /// The exact `(target)` token in the source md, including parentheses.
    pub original: String,
    /// The replacement `(target)` token pointing into the assets dir.
    pub rewritten: String,
    pub dest_filename: String,
}

/// Pick a non-colliding filename within an assets dir given already-used names.
fn dedup_name(name: &str, used: &HashSet<String>) -> String {
    if !used.contains(name) {
        return name.to_string();
    }
    let (stem, ext) = split_ext(name);
    let mut n = 2;
    loop {
        let candidate = match &ext {
            Some(e) => format!("{stem}-{n}.{e}"),
            None => format!("{stem}-{n}"),
        };
        if !used.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Scan `md` for inline images with relative-local image paths that exist under
/// `source_dir`, and plan how to bundle them into `{stem}.assets/`.
/// Returns (rewrite plan, copy plan). Pure: `exists` decides file presence.
pub fn plan_image_assets(
    md: &str,
    source_dir: &Path,
    stem: &str,
    exists: &dyn Fn(&Path) -> bool,
) -> (Vec<PlannedRef>, Vec<CopyOp>) {
    let mut used_names: HashSet<String> = HashSet::new();
    let mut abs_to_dest: HashMap<PathBuf, String> = HashMap::new();
    let mut seen_originals: HashSet<String> = HashSet::new();
    let mut copies: Vec<CopyOp> = Vec::new();
    let mut refs: Vec<PlannedRef> = Vec::new();

    for raw in scan_image_link_targets(md) {
        let path = extract_link_path(&raw);
        if !is_relative_local(&path) || !is_image_ext(&path) {
            continue;
        }
        let abs = source_dir.join(&path);
        if !exists(&abs) {
            continue;
        }
        let original = format!("({raw})");
        if !seen_originals.insert(original.clone()) {
            continue; // identical token already planned
        }
        let dest = match abs_to_dest.get(&abs) {
            Some(d) => d.clone(),
            None => {
                let basename = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&path);
                let name = dedup_name(basename, &used_names);
                used_names.insert(name.clone());
                abs_to_dest.insert(abs.clone(), name.clone());
                copies.push(CopyOp { src_abs: abs.clone(), dest_filename: name.clone() });
                name
            }
        };
        let rewritten = format!("({})", rewrite_link_target(&raw, &format!("{stem}.assets/{dest}")));
        refs.push(PlannedRef { original, rewritten, dest_filename: dest });
    }
    (refs, copies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn rec(source_hash: &str, vault_hash: &str) -> Record {
        Record {
            vault_path: "/vault/a.md".into(),
            source_path: "/src/a.md".into(),
            synced_at: 1,
            source_hash: source_hash.into(),
            vault_hash: vault_hash.into(),
            note_merge_base: None,
        }
    }

    #[test]
    fn sha256_is_stable() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn source_unchanged_is_up_to_date() {
        let r = rec("S", "V");
        assert_eq!(decide_update(&r, "S", "V"), UpdateOutcome::UpToDate);
        // vault drift alone never prompts
        assert_eq!(decide_update(&r, "S", "V2"), UpdateOutcome::UpToDate);
    }

    #[test]
    fn source_changed_vault_intact_is_origin_updated() {
        let r = rec("S", "V");
        assert_eq!(decide_update(&r, "S2", "V"), UpdateOutcome::OriginUpdated);
    }

    #[test]
    fn both_changed_is_conflict() {
        let r = rec("S", "V");
        assert_eq!(decide_update(&r, "S2", "V2"), UpdateOutcome::Conflict);
    }

    #[test]
    fn check_update_io_reports_source_missing() {
        let tmp = TempDir::new().unwrap();
        let vault = tmp.path().join("a.md");
        std::fs::write(&vault, b"x").unwrap();
        let mut r = rec("S", "V");
        r.source_path = tmp.path().join("missing.md").to_string_lossy().into();
        r.vault_path = vault.to_string_lossy().into();
        let out = check_update_io(&r, Path::new(&r.source_path), &vault).unwrap();
        assert_eq!(out, UpdateOutcome::SourceMissing);
    }

    #[test]
    fn check_update_io_detects_origin_update() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("src.md");
        let vault = tmp.path().join("vault.md");
        std::fs::write(&source, b"NEW").unwrap();
        std::fs::write(&vault, b"OLD").unwrap();
        let r = Record {
            vault_path: vault.to_string_lossy().into(),
            source_path: source.to_string_lossy().into(),
            synced_at: 1,
            source_hash: sha256_hex(b"OLD"),
            vault_hash: sha256_hex(b"OLD"),
            note_merge_base: None,
        };
        let out = check_update_io(&r, &source, &vault).unwrap();
        assert_eq!(out, UpdateOutcome::OriginUpdated);
    }

    #[test]
    fn dedup_returns_basename_when_free() {
        let got = dedup_target(Path::new("/v"), "a.md", &|_p| false);
        assert_eq!(got, PathBuf::from("/v/a.md"));
    }

    #[test]
    fn dedup_appends_suffix_on_collision() {
        // /v/a.md and /v/a-2.md taken; expect /v/a-3.md
        // Compare via PathBuf so the predicate matches on Windows too: the
        // candidates are built with `join`, whose separator is native.
        let taken = [PathBuf::from("/v/a.md"), PathBuf::from("/v/a-2.md")];
        let exists = |p: &Path| taken.iter().any(|t| t == p);
        let got = dedup_target(Path::new("/v"), "a.md", &exists);
        assert_eq!(got, PathBuf::from("/v/a-3.md"));
    }

    #[test]
    fn dedup_handles_no_extension() {
        let exists = |p: &Path| p == PathBuf::from("/v/README");
        let got = dedup_target(Path::new("/v"), "README", &exists);
        assert_eq!(got, PathBuf::from("/v/README-2"));
    }

    #[test]
    fn sync_target_reuses_existing_copy_when_present() {
        // A tracked copy exists → reuse it, even though the basename slot is taken.
        let existing = PathBuf::from("/v/Sync/a.md");
        let exists = |p: &Path| p == Path::new("/v/Sync/a.md");
        let got = sync_target(Some(existing.clone()), Path::new("/v/Sync"), "a.md", &exists);
        assert_eq!(got, existing);
    }

    #[test]
    fn sync_target_dedups_when_no_existing_copy() {
        // No tracked copy → fresh non-colliding target (basename slot free).
        let got = sync_target(None, Path::new("/v/Sync"), "a.md", &|_p| false);
        assert_eq!(got, PathBuf::from("/v/Sync/a.md"));
    }

    #[test]
    fn sync_target_dedups_when_tracked_copy_vanished() {
        // Record points to a copy the user deleted → don't reuse a missing path;
        // dedup around whatever else occupies the dir.
        let taken = [PathBuf::from("/v/Sync/a.md")];
        let exists = |p: &Path| taken.iter().any(|t| t == p);
        let got = sync_target(
            Some(PathBuf::from("/v/Sync/gone.md")),
            Path::new("/v/Sync"),
            "a.md",
            &exists,
        );
        assert_eq!(got, PathBuf::from("/v/Sync/a-2.md"));
    }

    #[test]
    fn is_under_vault_prefix() {
        assert!(is_under_vault(Path::new("/Users/b/Vault"), Path::new("/Users/b/Vault/Imported/a.md")));
        assert!(!is_under_vault(Path::new("/Users/b/Vault"), Path::new("/Users/b/work/a.md")));
    }

    #[test]
    fn dated_basename_prefixes_undated_md() {
        assert_eq!(dated_basename("notes.md", "2026-06-18"), "2026-06-18-notes.md");
        assert_eq!(dated_basename("NOTES.MD", "2026-06-18"), "2026-06-18-NOTES.MD");
    }

    #[test]
    fn dated_basename_prefixes_mdx_like_md() {
        // mdx is a document too — syncing foo.md and foo.mdx into the vault
        // should not produce inconsistent naming.
        assert_eq!(dated_basename("guide.mdx", "2026-06-18"), "2026-06-18-guide.mdx");
    }

    #[test]
    fn dated_basename_leaves_already_dated_md() {
        assert_eq!(dated_basename("2026-01-02-notes.md", "2026-06-18"), "2026-01-02-notes.md");
    }

    #[test]
    fn dated_basename_ignores_non_md() {
        assert_eq!(dated_basename("photo.png", "2026-06-18"), "photo.png");
        assert_eq!(dated_basename("README", "2026-06-18"), "README");
    }

    #[test]
    fn dated_basename_prefixes_when_existing_prefix_is_not_strict_format() {
        // single-digit month/day is not a yyyy-MM-dd- prefix
        assert_eq!(dated_basename("2026-1-2-notes.md", "2026-06-18"), "2026-06-18-2026-1-2-notes.md");
    }

    #[test]
    fn is_image_ext_matches_known_extensions() {
        assert!(is_image_ext("a/b/pic.PNG"));
        assert!(is_image_ext("x.jpeg"));
        assert!(is_image_ext("x.svg"));
        assert!(!is_image_ext("x.pdf"));
        assert!(!is_image_ext("noext"));
        assert!(!is_image_ext("trailing."));
    }

    #[test]
    fn assets_dir_name_appends_suffix() {
        assert_eq!(assets_dir_name("2026-07-03-notes"), "2026-07-03-notes.assets");
    }

    #[test]
    fn scan_finds_inline_image_targets_only() {
        let md = "text ![a](assets/x.png) more [not img](y.md) ![b](<z.png>) end";
        let got = scan_image_link_targets(md);
        assert_eq!(got, vec!["assets/x.png".to_string(), "<z.png>".to_string()]);
    }

    #[test]
    fn extract_link_path_handles_title_and_angles() {
        assert_eq!(extract_link_path("assets/x.png"), "assets/x.png");
        assert_eq!(extract_link_path("  assets/x.png  \"a title\""), "assets/x.png");
        assert_eq!(extract_link_path("<assets/my file.png>"), "assets/my file.png");
    }

    #[test]
    fn is_relative_local_rejects_absolute_and_urls() {
        assert!(is_relative_local("assets/x.png"));
        assert!(is_relative_local("./images/x.png"));
        assert!(!is_relative_local("/abs/x.png"));
        assert!(!is_relative_local("https://h/x.png"));
        assert!(!is_relative_local("http://h/x.png"));
        assert!(!is_relative_local("data:image/png;base64,AAAA"));
        assert!(!is_relative_local("C:\\win\\x.png"));
        assert!(!is_relative_local(""));
    }

    #[test]
    fn rewrite_link_target_preserves_title_and_angles() {
        assert_eq!(rewrite_link_target("assets/x.png", "d.assets/x.png"), "d.assets/x.png");
        assert_eq!(
            rewrite_link_target("assets/x.png \"t\"", "d.assets/x.png"),
            "d.assets/x.png \"t\""
        );
        assert_eq!(
            rewrite_link_target("<assets/x.png>", "d.assets/x.png"),
            "<d.assets/x.png>"
        );
    }

    fn always(_p: &std::path::Path) -> bool { true }

    #[test]
    fn plan_no_images_returns_empty() {
        let (refs, copies) = plan_image_assets("# hi\n[link](a.md)", Path::new("/s"), "d", &always);
        assert!(refs.is_empty());
        assert!(copies.is_empty());
    }

    #[test]
    fn plan_single_image_rewrites_and_copies() {
        let md = "![a](assets/x.png)";
        let (refs, copies) = plan_image_assets(md, Path::new("/s"), "d", &always);
        assert_eq!(copies.len(), 1);
        assert_eq!(copies[0].src_abs, PathBuf::from("/s/assets/x.png"));
        assert_eq!(copies[0].dest_filename, "x.png");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].original, "(assets/x.png)");
        assert_eq!(refs[0].rewritten, "(d.assets/x.png)");
        assert_eq!(refs[0].dest_filename, "x.png");
    }

    #[test]
    fn plan_skips_urls_absolute_and_non_images() {
        let md = "![a](https://h/x.png) ![b](/abs/y.png) ![c](note.md)";
        let (refs, copies) = plan_image_assets(md, Path::new("/s"), "d", &always);
        assert!(refs.is_empty());
        assert!(copies.is_empty());
    }

    #[test]
    fn plan_skips_missing_source_files() {
        let md = "![a](assets/x.png)";
        let none = |_p: &Path| false;
        let (refs, copies) = plan_image_assets(md, Path::new("/s"), "d", &none);
        assert!(refs.is_empty());
        assert!(copies.is_empty());
    }

    #[test]
    fn plan_dedups_same_basename_from_different_dirs() {
        let md = "![a](one/img.png) ![b](two/img.png)";
        let (refs, copies) = plan_image_assets(md, Path::new("/s"), "d", &always);
        assert_eq!(copies.len(), 2);
        assert_eq!(copies[0].dest_filename, "img.png");
        assert_eq!(copies[1].dest_filename, "img-2.png");
        assert_eq!(refs[0].rewritten, "(d.assets/img.png)");
        assert_eq!(refs[1].rewritten, "(d.assets/img-2.png)");
    }

    #[test]
    fn plan_copies_same_file_once() {
        let md = "![a](assets/x.png) then again ![a](assets/x.png)";
        let (refs, copies) = plan_image_assets(md, Path::new("/s"), "d", &always);
        assert_eq!(copies.len(), 1);
        assert_eq!(refs.len(), 1); // one unique original token
        assert_eq!(refs[0].rewritten, "(d.assets/x.png)");
    }

    #[test]
    fn plan_preserves_title_in_rewrite() {
        let md = "![a](assets/x.png \"cap\")";
        let (refs, _copies) = plan_image_assets(md, Path::new("/s"), "d", &always);
        assert_eq!(refs[0].original, "(assets/x.png \"cap\")");
        assert_eq!(refs[0].rewritten, "(d.assets/x.png \"cap\")");
    }

    #[test]
    fn companion_note_name_for_md_files() {
        assert_eq!(companion_note_name("foo.md").as_deref(), Some("foo.note.md"));
        assert_eq!(
            companion_note_name("2026-07-10-foo.md").as_deref(),
            Some("2026-07-10-foo.note.md")
        );
        assert_eq!(companion_note_name("Foo.MD").as_deref(), Some("Foo.note.md"));
        assert_eq!(companion_note_name("a.markdown").as_deref(), Some("a.note.md"));
    }

    #[test]
    fn companion_note_name_skips_notes_and_non_md() {
        assert_eq!(companion_note_name("foo.note.md"), None);
        assert_eq!(companion_note_name("foo.notes.md"), None);
        assert_eq!(companion_note_name("foo.txt"), None);
        assert_eq!(companion_note_name("foo"), None);
    }

}

