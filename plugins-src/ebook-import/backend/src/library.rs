//! The book library: everything already imported into the vault, not just what
//! this window's queue did. Pure filesystem read, no state.
use std::path::Path;

#[cfg(test)]
mod tests;

/// One imported book: the directory `<ebooks_root>/<YYYY-MM>/<Title>/`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct BookEntry {
    /// Vault-relative, POSIX-separated directory path — the same shape
    /// `dest_rel` has, so it feeds `ai_read_start` unchanged.
    pub rel: String,
    pub name: String,
    pub month: String,
    /// Stable logical category. Legacy books may not have one yet.
    pub topic_id: Option<String>,
    /// Current user-facing label, absent for legacy/unknown topic ids.
    pub topic_label: Option<String>,
    /// `YYYY-MM-DD-summary.md` file names, newest first.
    pub summaries: Vec<String>,
}

/// Whether `name` is a `YYYY-MM-DD-summary.md` produced by an AI read. Written
/// out by hand rather than with a regex so the shape is obvious at the call
/// site: `summary_name` in `airead.rs` is the only thing that makes these, and
/// this predicate has to keep matching it.
fn is_summary(name: &str) -> bool {
    let Some(date) = name.strip_suffix("-summary.md") else {
        return false;
    };
    let b = date.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Directory entries of `dir` that are themselves directories, sorted by name.
/// An unreadable directory yields nothing rather than failing the whole scan —
/// one bad month must not blank the library.
fn sorted_subdirs(dir: &Path) -> Vec<std::fs::DirEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<_> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    out.sort_by_key(|e| e.file_name());
    out
}

/// The authoritative time this book joined the vault. The importer writes one
/// exact top-level scalar (`added_at: YYYY-MM-DDTHH:MM:SSZ`); tolerate future
/// extra metadata lines, but only accept the strict UTC shape promised by the
/// file contract. Missing or hand-edited invalid metadata is a compatibility
/// case, not a reason to hide an otherwise readable book.
fn read_added_at(dir: &Path) -> Option<chrono::DateTime<chrono::Utc>> {
    let text = std::fs::read_to_string(dir.join("meta.yml")).ok()?;
    let raw = text.lines().find_map(|line| line.strip_prefix("added_at: "))?;
    if raw.len() != 20 || !raw.ends_with('Z') {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

/// Every book under `<vault>/<ebooks_root>/<YYYY-MM>/<Title>/`, newest
/// `meta.yml` `added_at` first. Equal timestamps fall back to newest month then
/// alphabetical name. Books with missing or invalid metadata remain visible
/// after all timestamped books and use that same legacy month/name order. A
/// directory only counts as a book if it holds a `book.md` — every row the
/// window draws offers "AI read", and that job would have nothing to read
/// otherwise.
///
/// `ebooks_root` is validated here even though `apply_vault_patch` already
/// rejects an escaping value at save time: `.notemd/ebook-import.json` is
/// hand-editable (and agent-editable), and listing `../..` would walk the
/// user's home directory into the window. Same defense-in-depth reasoning as
/// `pipeline::run_import`.
pub fn scan(vault: &Path, ebooks_root: &str) -> Vec<BookEntry> {
    if crate::settings::validate_ebooks_root(ebooks_root).is_err() {
        return Vec::new();
    }
    let root = vault.join(ebooks_root);
    let catalog = crate::topics::read_catalog(&root).ok();
    let mut out = Vec::new();
    for month in sorted_subdirs(&root) {
        let month_name = month.file_name().to_string_lossy().to_string();
        for book in sorted_subdirs(&month.path()) {
            let dir = book.path();
            if !dir.join("book.md").is_file() {
                continue;
            }
            let name = book.file_name().to_string_lossy().to_string();
            let topic_id = crate::topics::read_book_topic(&dir.join("meta.yml"))
                .ok()
                .flatten();
            let topic_label = topic_id.as_deref().and_then(|id| {
                catalog
                    .as_ref()
                    .and_then(|catalog| catalog.topic(id))
                    .map(|topic| topic.label.clone())
            });
            let mut summaries: Vec<String> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| is_summary(n))
                .collect();
            // Lexicographic on `YYYY-MM-DD-…` is chronological; reversed, it is
            // newest first.
            summaries.sort_unstable_by(|a, b| b.cmp(a));
            out.push((
                read_added_at(&dir),
                BookEntry {
                    rel: format!("{ebooks_root}/{month_name}/{name}"),
                    name,
                    month: month_name.clone(),
                    topic_id,
                    topic_label,
                    summaries,
                },
            ));
        }
    }
    out.sort_by(|(a_added, a), (b_added, b)| {
        b_added
            .cmp(a_added)
            .then_with(|| b.month.cmp(&a.month))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.rel.cmp(&b.rel))
    });
    out.into_iter().map(|(_, book)| book).collect()
}
