//! Ebook topic taxonomy, per-book assignments, and generated Markdown indexes.
//!
//! `topics.yml` and each book's `meta.yml` are authoritative. Topic indexes are
//! deterministic projections and are only ever replaced/removed when they carry
//! this module's versioned generated marker.

use chrono::{DateTime, Utc};
use fs2::FileExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io::Write;
use std::io::{Read, Take};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

pub const TOPICS_FILE: &str = "topics.yml";
pub const LOCK_FILE: &str = ".ebook-topics.lock";
pub const MAX_TOPICS: usize = 8;
pub const GENERATED_MARKER: &str =
    "<!-- notemd:generated ebook-topic-index/v1; edit topics.yml or book meta.yml -->";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Serialize every canonical topic/meta/index mutation across UI and CLI
/// plugin processes. Callers hold this around the whole multi-file operation;
/// individual atomic writes remain independently crash-safe.
pub fn with_topic_lock<T>(
    ebooks_root: &Path,
    op: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    std::fs::create_dir_all(ebooks_root)
        .map_err(|e| format!("create {}: {e}", ebooks_root.display()))?;
    let path = ebooks_root.join(LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&path)
        .map_err(|e| format!("open topic lock {}: {e}", path.display()))?;
    file.lock_exclusive()
        .map_err(|e| format!("lock topic state {}: {e}", path.display()))?;
    let result = op();
    let unlock = file
        .unlock()
        .map_err(|e| format!("unlock topic state {}: {e}", path.display()));
    match (result, unlock) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopicCatalog {
    pub schema_version: u32,
    pub topics: Vec<Topic>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Topic {
    pub id: String,
    pub label: String,
    pub description: String,
    pub index_file: String,
    pub vocabulary: Vec<Vocabulary>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vocabulary {
    pub term: String,
    pub description: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl TopicCatalog {
    pub fn topic(&self, id: &str) -> Option<&Topic> {
        self.topics.iter().find(|topic| topic.id == id)
    }

    pub fn contains_topic(&self, id: &str) -> bool {
        self.topic(id).is_some()
    }
}

/// One committed book discovered below `<ebooks_root>/<YYYY-MM>/<Title>/`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScannedBook {
    /// POSIX path relative to `ebooks_root`, without the trailing `book.md`.
    pub rel: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_id: Option<String>,
}

/// Bounded, untrusted evidence supplied to the classification Agent. Source
/// paths are intentionally reduced to a format suffix so local absolute paths
/// from `sources[].resource` never leave the book file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_format: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_summary_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opening_excerpt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RebuildResult {
    pub index_paths: Vec<PathBuf>,
    pub removed_stale_indexes: Vec<PathBuf>,
    pub unclassified_books: Vec<String>,
    pub unknown_topic_books: Vec<String>,
}

pub fn validate_catalog(catalog: &TopicCatalog) -> Result<(), String> {
    if catalog.schema_version != 1 {
        return Err(format!(
            "unsupported topics.yml schema_version {}; expected 1",
            catalog.schema_version
        ));
    }
    if !(1..=MAX_TOPICS).contains(&catalog.topics.len()) {
        return Err(format!(
            "topics.yml must define 1–{MAX_TOPICS} topics; found {}",
            catalog.topics.len()
        ));
    }

    let mut ids = BTreeSet::new();
    let mut labels = BTreeSet::new();
    let mut index_files = BTreeSet::new();
    for (i, topic) in catalog.topics.iter().enumerate() {
        let at = format!("topics[{i}]");
        validate_topic_id(&topic.id).map_err(|e| format!("{at}.id: {e}"))?;
        if !ids.insert(topic.id.as_str()) {
            return Err(format!("{at}.id: duplicate topic id {:?}", topic.id));
        }
        require_non_empty(&topic.label, &format!("{at}.label"))?;
        if !labels.insert(topic.label.trim()) {
            return Err(format!("{at}.label: duplicate label {:?}", topic.label));
        }
        require_non_empty(&topic.description, &format!("{at}.description"))?;
        validate_index_file(&topic.index_file).map_err(|e| format!("{at}.index_file: {e}"))?;
        // The plugin targets macOS first, where the usual vault filesystem is
        // case-insensitive. Reject names that would alias there even though
        // their YAML spellings differ.
        if !index_files.insert(topic.index_file.to_lowercase()) {
            return Err(format!(
                "{at}.index_file: duplicate index file {:?}",
                topic.index_file
            ));
        }
        if topic.vocabulary.len() < 2 {
            return Err(format!("{at}.vocabulary: at least 2 entries are required"));
        }
        let mut terms = BTreeSet::new();
        for (j, item) in topic.vocabulary.iter().enumerate() {
            let item_at = format!("{at}.vocabulary[{j}]");
            require_non_empty(&item.term, &format!("{item_at}.term"))?;
            require_non_empty(&item.description, &format!("{item_at}.description"))?;
            if !terms.insert(item.term.trim()) {
                return Err(format!("{item_at}.term: duplicate term {:?}", item.term));
            }
        }
    }
    Ok(())
}

pub fn validate_topic_id(id: &str) -> Result<(), String> {
    let re = Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").expect("constant topic id regex");
    if re.is_match(id) {
        Ok(())
    } else {
        Err(format!("must match [a-z0-9]+(?:-[a-z0-9]+)*; got {id:?}"))
    }
}

pub fn validate_index_file(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.trim() != name
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.chars().any(char::is_control)
        || Path::new(name).file_name() != Some(OsStr::new(name))
    {
        return Err("must be one safe file name in the ebooks root".to_string());
    }
    if matches!(name.to_ascii_lowercase().as_str(), "index.md" | "log.md") {
        return Err(format!("reserved file name {name:?}"));
    }
    if !name.to_ascii_lowercase().ends_with(".index.md") {
        return Err("must end with .index.md".to_string());
    }
    Ok(())
}

fn require_non_empty(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{field}: must not be empty"))
    } else {
        Ok(())
    }
}

pub fn parse_catalog(yaml: &str) -> Result<TopicCatalog, String> {
    let catalog: TopicCatalog =
        serde_yaml::from_str(yaml).map_err(|e| format!("parse topics.yml: {e}"))?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

fn regular_file_state(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_file() => Ok(true),
        Ok(_) => Err(format!(
            "refusing non-regular or symlinked catalog {}",
            path.display()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("inspect {}: {error}", path.display())),
    }
}

pub fn read_catalog(ebooks_root: &Path) -> Result<TopicCatalog, String> {
    let path = ebooks_root.join(TOPICS_FILE);
    if !regular_file_state(&path)? {
        return Err(format!("read {}: file does not exist", path.display()));
    }
    let yaml =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    parse_catalog(&yaml)
}

pub fn catalog_revision(ebooks_root: &Path) -> Result<Option<String>, String> {
    let path = ebooks_root.join(TOPICS_FILE);
    if !regular_file_state(&path)? {
        return Ok(None);
    }
    match std::fs::read(&path) {
        Ok(bytes) => Ok(Some(format!("sha256:{:x}", Sha256::digest(bytes)))),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}

pub fn write_catalog(ebooks_root: &Path, catalog: &TopicCatalog) -> Result<PathBuf, String> {
    validate_catalog(catalog)?;
    std::fs::create_dir_all(ebooks_root)
        .map_err(|e| format!("create {}: {e}", ebooks_root.display()))?;
    let yaml = serde_yaml::to_string(catalog).map_err(|e| format!("serialize topics.yml: {e}"))?;
    let path = ebooks_root.join(TOPICS_FILE);
    let _ = regular_file_state(&path)?;
    atomic_write(&path, yaml.as_bytes())?;
    Ok(path)
}

/// Read an old or new `meta.yml`. A missing `topic_id` is a supported legacy
/// state; a present non-string value is malformed and fails closed.
pub fn read_book_topic(meta_path: &Path) -> Result<Option<String>, String> {
    let mapping = read_yaml_mapping(meta_path)?;
    let Some(value) = mapping.get(Value::String("topic_id".to_string())) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|s| Some(s.to_string()))
        .ok_or_else(|| format!("{}: topic_id must be a string", meta_path.display()))
}

/// Set `topic_id` while retaining `added_at` and every unknown metadata key.
/// The caller must additionally verify that the ID exists in the current
/// catalog; this function enforces only the stable ID syntax.
pub fn write_book_topic(meta_path: &Path, topic_id: &str) -> Result<(), String> {
    validate_topic_id(topic_id)?;
    let mut mapping = if meta_path.exists() {
        read_yaml_mapping(meta_path)?
    } else {
        Mapping::new()
    };
    mapping.insert(
        Value::String("topic_id".to_string()),
        Value::String(topic_id.to_string()),
    );
    let yaml = serde_yaml::to_string(&mapping)
        .map_err(|e| format!("serialize {}: {e}", meta_path.display()))?;
    atomic_write(meta_path, yaml.as_bytes())
}

/// Catalog-aware assignment entry point for UI/CLI integration. Unlike
/// [`write_book_topic`], this proves the reference exists before changing the
/// book metadata.
pub fn assign_book_topic(
    meta_path: &Path,
    catalog: &TopicCatalog,
    topic_id: &str,
) -> Result<(), String> {
    validate_catalog(catalog)?;
    if !catalog.contains_topic(topic_id) {
        return Err(format!("unknown topic id {topic_id:?}"));
    }
    write_book_topic(meta_path, topic_id)
}

/// Resolve one scanned book's metadata without accepting separators, symlinked
/// directories, or symlinked files. `scan_books` produces exactly two path
/// components; rechecking at mutation time closes the scan→write escape.
pub fn existing_book_meta(ebooks_root: &Path, rel: &str) -> Result<PathBuf, String> {
    let components: Vec<_> = Path::new(rel).components().collect();
    if components.len() != 2
        || components
            .iter()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(format!("unsafe library book path {rel:?}"));
    }
    let mut current = ebooks_root.to_path_buf();
    for component in components {
        let std::path::Component::Normal(name) = component else {
            unreachable!()
        };
        current.push(name);
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|e| format!("inspect {}: {e}", current.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "unsafe library book directory {}",
                current.display()
            ));
        }
    }
    if !crate::library::is_regular_file(&current.join("book.md")) {
        return Err(format!("missing book.md for {rel:?}"));
    }
    let meta = current.join("meta.yml");
    let metadata =
        std::fs::symlink_metadata(&meta).map_err(|e| format!("inspect {}: {e}", meta.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("unsafe book metadata {}", meta.display()));
    }
    Ok(meta)
}

fn read_yaml_mapping(path: &Path) -> Result<Mapping, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let value: Value =
        serde_yaml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
    value
        .as_mapping()
        .cloned()
        .ok_or_else(|| format!("{}: expected a YAML mapping", path.display()))
}

/// Scan committed books. Only a directory containing both `book.md` and
/// `meta.yml` participates; `meta.yml` is the import commit marker.
pub fn scan_books(ebooks_root: &Path) -> Result<Vec<ScannedBook>, String> {
    let mut books = Vec::new();
    let Ok(months) = std::fs::read_dir(ebooks_root) else {
        return Ok(books);
    };
    let mut months: Vec<_> = months
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .collect();
    months.sort_by_key(|e| e.file_name());
    for month in months {
        let month_name = month.file_name().to_string_lossy().to_string();
        let Ok(entries) = std::fs::read_dir(month.path()) else {
            continue;
        };
        let mut entries: Vec<_> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let dir = entry.path();
            let book_path = dir.join("book.md");
            let meta_path = dir.join("meta.yml");
            if !crate::library::is_regular_file(&book_path)
                || !crate::library::is_regular_file(&meta_path)
            {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();
            let frontmatter = read_book_frontmatter(&book_path);
            let (title, creator, publisher, language) = frontmatter
                .map(|frontmatter| {
                    (
                        frontmatter.title,
                        frontmatter.creator,
                        frontmatter.publisher,
                        frontmatter.language,
                    )
                })
                .unwrap_or_else(|| (dir_name.clone(), None, None, None));
            let meta = read_yaml_mapping(&meta_path)?;
            let added_at = optional_string(&meta, "added_at", &meta_path)?;
            let topic_id = optional_string(&meta, "topic_id", &meta_path)?;
            books.push(ScannedBook {
                rel: format!("{month_name}/{dir_name}"),
                title,
                creator,
                publisher,
                language,
                added_at,
                topic_id,
            });
        }
    }
    books.sort_by(compare_books);
    Ok(books)
}

fn optional_string(mapping: &Mapping, key: &str, path: &Path) -> Result<Option<String>, String> {
    let Some(value) = mapping.get(Value::String(key.to_string())) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(|s| Some(s.to_string()))
        .ok_or_else(|| format!("{}: {key} must be a string", path.display()))
}

#[derive(Debug, PartialEq, Eq)]
struct BookFrontmatter {
    title: String,
    creator: Option<String>,
    publisher: Option<String>,
    language: Option<String>,
    source_format: Option<String>,
}

const FRONTMATTER_READ_LIMIT: u64 = 64 * 1024;
const BOOK_CONTEXT_READ_LIMIT: u64 = 256 * 1024;
const SUMMARY_READ_LIMIT: u64 = 32 * 1024;

fn read_prefix(path: &Path, limit: u64) -> Result<String, String> {
    if !crate::library::is_regular_file(path) {
        return Err(format!("refusing non-regular file {}", path.display()));
    }
    let file =
        std::fs::File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    let mut reader: Take<std::fs::File> = file.take(limit);
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_book_frontmatter(path: &Path) -> Option<BookFrontmatter> {
    let text = read_prefix(path, FRONTMATTER_READ_LIMIT).ok()?;
    let body = text.strip_prefix("---\n")?;
    let end = body.find("\n---")?;
    let value: Value = serde_yaml::from_str(&body[..end]).ok()?;
    let map = value.as_mapping()?;
    let title = map
        .get(Value::String("title".to_string()))
        .and_then(Value::as_str)?
        .to_string();
    let creator = map
        .get(Value::String("creator".to_string()))
        .and_then(Value::as_str)
        .or_else(|| {
            map.get(Value::String("sources".to_string()))
                .and_then(Value::as_sequence)
                .and_then(|sources| sources.first())
                .and_then(Value::as_mapping)
                .and_then(|source| source.get(Value::String("author".to_string())))
                .and_then(Value::as_str)
        })
        .map(str::to_string);
    let publisher = map
        .get(Value::String("publisher".to_string()))
        .and_then(Value::as_str)
        .map(str::to_string);
    let language = map
        .get(Value::String("language".to_string()))
        .and_then(Value::as_str)
        .map(str::to_string);
    let source_format = map
        .get(Value::String("sources".to_string()))
        .and_then(Value::as_sequence)
        .and_then(|sources| sources.first())
        .and_then(Value::as_mapping)
        .and_then(|source| source.get(Value::String("resource".to_string())))
        .and_then(Value::as_str)
        .and_then(|resource| Path::new(resource).extension())
        .and_then(OsStr::to_str)
        .map(|extension| extension.to_ascii_lowercase());
    Some(BookFrontmatter {
        title: trim_utf8(&title, 512),
        creator: creator.map(|value| trim_utf8(&value, 256)),
        publisher: publisher.map(|value| trim_utf8(&value, 256)),
        language: language.map(|value| trim_utf8(&value, 64)),
        source_format: source_format.map(|value| trim_utf8(&value, 24)),
    })
}

fn trim_utf8(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].trim_end().to_string()
}

fn markdown_body(text: &str) -> &str {
    let Some(body) = text.strip_prefix("---\n") else {
        return text;
    };
    let Some(end) = body.find("\n---") else {
        return text;
    };
    body[end + 4..].trim_start_matches(['\r', '\n'])
}

fn prose_excerpt(text: &str, limit: usize) -> Option<String> {
    let mut out = String::new();
    let mut in_fence = false;
    for raw in markdown_body(text).lines() {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence
            || line.is_empty()
            || line.starts_with('#')
            || line.starts_with("![")
            || line == GENERATED_MARKER
        {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line);
        if out.len() >= limit {
            break;
        }
    }
    let out = trim_utf8(&out, limit);
    (!out.is_empty()).then_some(out)
}

fn markdown_headings(text: &str) -> Vec<String> {
    let mut headings = Vec::new();
    let mut bytes = 0usize;
    for line in markdown_body(text).lines() {
        let line = line.trim_start();
        let Some(title) = line
            .strip_prefix('#')
            .map(|title| title.trim_start_matches('#').trim())
        else {
            continue;
        };
        if title.is_empty() {
            continue;
        }
        let title = trim_utf8(title, 120);
        if bytes + title.len() > 900 || headings.len() >= 12 {
            break;
        }
        bytes += title.len();
        headings.push(title);
    }
    headings
}

/// Read the newest AI summary plus the beginning and heading structure of one
/// verified book. Every source is regular-file checked and byte bounded.
pub fn read_book_evidence(ebooks_root: &Path, rel: &str) -> Result<BookEvidence, String> {
    let meta = existing_book_meta(ebooks_root, rel)?;
    let dir = meta
        .parent()
        .ok_or_else(|| format!("book metadata has no parent: {}", meta.display()))?;
    let book_path = dir.join("book.md");
    let book_text = read_prefix(&book_path, BOOK_CONTEXT_READ_LIMIT)?;
    let source_format =
        read_book_frontmatter(&book_path).and_then(|frontmatter| frontmatter.source_format);

    let mut summaries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|error| format!("read {}: {error}", dir.display()))?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| crate::library::is_summary(&entry.file_name().to_string_lossy()))
        .collect();
    summaries.sort_by_key(|entry| entry.file_name());
    let (latest_summary_file, summary_excerpt) = match summaries.last() {
        Some(entry) => {
            let name = entry.file_name().to_string_lossy().to_string();
            let text = read_prefix(&entry.path(), SUMMARY_READ_LIMIT)?;
            (Some(name), prose_excerpt(&text, 1600))
        }
        None => (None, None),
    };

    Ok(BookEvidence {
        source_format,
        headings: markdown_headings(&book_text),
        latest_summary_file,
        summary_excerpt,
        opening_excerpt: prose_excerpt(&book_text, 1000),
    })
}

fn parsed_added_at(book: &ScannedBook) -> Option<DateTime<Utc>> {
    let raw = book.added_at.as_deref()?;
    if raw.len() != 20 || !raw.ends_with('Z') {
        return None;
    }
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn compare_books(a: &ScannedBook, b: &ScannedBook) -> Ordering {
    parsed_added_at(b)
        .cmp(&parsed_added_at(a))
        .then_with(|| a.title.cmp(&b.title))
        .then_with(|| a.rel.cmp(&b.rel))
}

pub fn render_index(topic: &Topic, books: &[ScannedBook]) -> String {
    let mut books: Vec<_> = books
        .iter()
        .filter(|book| book.topic_id.as_deref() == Some(topic.id.as_str()))
        .collect();
    books.sort_by(|a, b| compare_books(a, b));

    let mut out = String::new();
    out.push_str("---\n");
    out.push_str("type: Book Topic Index\n");
    out.push_str(&format!("title: {}\n", yaml_quote(&topic.label)));
    out.push_str(&format!(
        "description: {}\n",
        yaml_quote(&topic.description)
    ));
    out.push_str(&format!("tags: [ebooks, topic, {}]\n", topic.id));
    out.push_str("---\n");
    out.push_str(GENERATED_MARKER);
    out.push_str("\n\n# ");
    out.push_str(&topic.label);
    out.push_str("\n\n");
    out.push_str(&topic.description);
    out.push_str("\n\n## 相关词汇\n\n");
    for item in &topic.vocabulary {
        out.push_str("- **");
        out.push_str(&escape_markdown(&item.term));
        out.push_str("** — ");
        out.push_str(&item.description);
        out.push('\n');
    }
    out.push_str("\n## 书籍\n");
    for book in books {
        out.push_str("\n- [");
        out.push_str(&escape_markdown(&book.title));
        out.push_str("](<");
        out.push_str(&book.rel);
        out.push_str("/book.md>)");
        if book.creator.is_some() || book.added_at.is_some() {
            out.push_str(" — ");
            let mut details = Vec::new();
            if let Some(creator) = &book.creator {
                details.push(creator.clone());
            }
            if let Some(added_at) = &book.added_at {
                details.push(added_at.get(..10).unwrap_or(added_at).to_string());
            }
            out.push_str(&details.join(" · "));
        }
        out.push('\n');
    }
    out
}

fn yaml_quote(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string cannot fail")
}

fn escape_markdown(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

pub fn is_generated_index(text: &str) -> bool {
    text.lines().take(16).any(|line| line == GENERATED_MARKER)
}

/// Validate every desired destination before a canonical mutation. This is
/// intentionally side-effect free and is shared by import and catalog-save so
/// a handwritten collision cannot be discovered only after state was committed.
pub fn preflight_indexes(ebooks_root: &Path, catalog: &TopicCatalog) -> Result<(), String> {
    validate_catalog(catalog)?;
    // Also prove the current metadata set is readable before any write.
    let _ = scan_books(ebooks_root)?;
    let desired: BTreeSet<_> = catalog
        .topics
        .iter()
        .map(|topic| topic.index_file.to_ascii_lowercase())
        .collect();
    let entries = match std::fs::read_dir(ebooks_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("read {}: {error}", ebooks_root.display())),
    };
    for entry in entries {
        let entry = entry.map_err(|e| format!("read entry in {}: {e}", ebooks_root.display()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".index.md") {
            continue;
        }
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("inspect {}: {e}", path.display()))?;
        if !metadata.file_type().is_file() {
            return Err(format!(
                "refusing non-regular or symlinked topic index {}",
                path.display()
            ));
        }
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        if desired.contains(&lower) && !is_generated_index(&text) {
            return Err(format!(
                "topic index conflicts with a hand-written file: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

/// Rebuild every current topic index and remove obsolete generated v1
/// indexes. Before touching any file, all desired destinations are checked so
/// a hand-written same-name file stops the whole operation without partial
/// writes.
pub fn rebuild_indexes(
    ebooks_root: &Path,
    catalog: &TopicCatalog,
) -> Result<RebuildResult, String> {
    preflight_indexes(ebooks_root, catalog)?;
    let books = scan_books(ebooks_root)?;
    let desired: BTreeSet<_> = catalog
        .topics
        .iter()
        .map(|topic| topic.index_file.to_ascii_lowercase())
        .collect();

    std::fs::create_dir_all(ebooks_root)
        .map_err(|e| format!("create {}: {e}", ebooks_root.display()))?;
    let mut index_paths = Vec::new();
    for topic in &catalog.topics {
        let path = ebooks_root.join(&topic.index_file);
        atomic_write(&path, render_index(topic, &books).as_bytes())?;
        index_paths.push(PathBuf::from(&topic.index_file));
    }

    let mut removed_stale_indexes = Vec::new();
    for entry in std::fs::read_dir(ebooks_root)
        .map_err(|e| format!("read {}: {e}", ebooks_root.display()))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        if !lower.ends_with(".index.md") || desired.contains(&lower) {
            continue;
        }
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if is_generated_index(&text) {
            std::fs::remove_file(&path)
                .map_err(|e| format!("remove stale {}: {e}", path.display()))?;
            removed_stale_indexes.push(PathBuf::from(name));
        }
    }
    removed_stale_indexes.sort();

    let mut unclassified_books = Vec::new();
    let mut unknown_topic_books = Vec::new();
    for book in &books {
        match book.topic_id.as_deref() {
            None => unclassified_books.push(book.rel.clone()),
            Some(id) if !catalog.contains_topic(id) => unknown_topic_books.push(book.rel.clone()),
            Some(_) => {}
        }
    }

    Ok(RebuildResult {
        index_paths,
        removed_stale_indexes,
        unclassified_books,
        unknown_topic_books,
    })
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("invalid output path {}", path.display()))?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, AtomicOrdering::Relaxed);
    let tmp = parent.join(format!(
        ".{file_name}.{}.{sequence}.tmp",
        std::process::id()
    ));
    let result = (|| -> Result<(), String> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| format!("create {}: {e}", tmp.display()))?;
        file.write_all(bytes)
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        file.sync_all()
            .map_err(|e| format!("sync {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog_yaml(topic_count: usize) -> String {
        let mut out = String::from("schema_version: 1\nfuture_root:\n  enabled: true\ntopics:\n");
        for i in 0..topic_count {
            out.push_str(&format!(
                "  - id: topic-{i}\n    label: 主题{i}\n    description: 主题 {i} 的领域说明。\n    index_file: 主题{i}.index.md\n    future_topic: keep\n    vocabulary:\n      - term: 词{i}甲\n        description: 词甲描述\n        future_word: 1\n      - term: 词{i}乙\n        description: 词乙描述\n"
            ));
        }
        out
    }

    fn one_topic() -> TopicCatalog {
        parse_catalog(&catalog_yaml(1)).unwrap()
    }

    #[test]
    fn schema_accepts_one_through_eight_and_preserves_unknown_fields() {
        for count in 1..=MAX_TOPICS {
            assert_eq!(
                parse_catalog(&catalog_yaml(count)).unwrap().topics.len(),
                count
            );
        }
        assert!(parse_catalog(&catalog_yaml(0)).unwrap_err().contains("1–8"));
        assert!(parse_catalog(&catalog_yaml(MAX_TOPICS + 1))
            .unwrap_err()
            .contains("1–8"));

        let original = parse_catalog(&catalog_yaml(1)).unwrap();
        let serialized = serde_yaml::to_string(&original).unwrap();
        let reparsed = parse_catalog(&serialized).unwrap();
        assert_eq!(reparsed.extra, original.extra);
        assert_eq!(reparsed.topics[0].extra, original.topics[0].extra);
        assert_eq!(
            reparsed.topics[0].vocabulary[0].extra,
            original.topics[0].vocabulary[0].extra
        );
    }

    #[test]
    fn catalog_revision_changes_with_bytes_and_represents_absence() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(catalog_revision(tmp.path()).unwrap(), None);
        write_catalog(tmp.path(), &one_topic()).unwrap();
        let first = catalog_revision(tmp.path()).unwrap().unwrap();
        assert!(first.starts_with("sha256:"));
        let mut changed = one_topic();
        changed.topics[0].label = "另一个主题".into();
        write_catalog(tmp.path(), &changed).unwrap();
        assert_ne!(catalog_revision(tmp.path()).unwrap().unwrap(), first);
    }

    #[cfg(unix)]
    #[test]
    fn catalog_io_never_follows_a_symlinked_topics_file() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let external = outside.path().join("topics.yml");
        let original = serde_yaml::to_string(&one_topic()).unwrap();
        std::fs::write(&external, &original).unwrap();
        let link = root.path().join(TOPICS_FILE);
        symlink(&external, &link).unwrap();

        assert!(read_catalog(root.path())
            .unwrap_err()
            .contains("symlinked catalog"));
        assert!(catalog_revision(root.path())
            .unwrap_err()
            .contains("symlinked catalog"));
        assert!(write_catalog(root.path(), &one_topic())
            .unwrap_err()
            .contains("symlinked catalog"));
        assert_eq!(std::fs::read_to_string(&external).unwrap(), original);
        assert!(std::fs::symlink_metadata(link)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn schema_rejects_bad_ids_duplicates_unsafe_files_and_incomplete_vocabulary() {
        let mut catalog = one_topic();
        catalog.topics[0].id = "Bad ID".into();
        assert!(validate_catalog(&catalog)
            .unwrap_err()
            .contains("must match"));

        let mut catalog = parse_catalog(&catalog_yaml(2)).unwrap();
        catalog.topics[1].id = catalog.topics[0].id.clone();
        assert!(validate_catalog(&catalog)
            .unwrap_err()
            .contains("duplicate topic id"));
        catalog.topics[1].id = "second".into();
        catalog.topics[1].label = catalog.topics[0].label.clone();
        assert!(validate_catalog(&catalog)
            .unwrap_err()
            .contains("duplicate label"));
        catalog.topics[1].label = "另一个".into();
        catalog.topics[1].index_file = "../escape.index.md".into();
        assert!(validate_catalog(&catalog)
            .unwrap_err()
            .contains("safe file name"));

        catalog.topics[0].index_file = "Topic.index.md".into();
        catalog.topics[1].index_file = "topic.index.md".into();
        assert!(validate_catalog(&catalog)
            .unwrap_err()
            .contains("duplicate index file"));

        let mut catalog = one_topic();
        catalog.topics[0].vocabulary.truncate(1);
        assert!(validate_catalog(&catalog)
            .unwrap_err()
            .contains("at least 2"));
        let mut catalog = one_topic();
        catalog.topics[0].vocabulary[1].term = catalog.topics[0].vocabulary[0].term.clone();
        assert!(validate_catalog(&catalog)
            .unwrap_err()
            .contains("duplicate term"));
    }

    #[test]
    fn assigning_topic_preserves_added_at_and_unknown_nested_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("meta.yml");
        std::fs::write(
            &path,
            "added_at: 2026-08-27T06:40:15Z\nfuture:\n  score: 9\ntopic_id: old-topic\n",
        )
        .unwrap();
        write_book_topic(&path, "business-strategy").unwrap();
        assert_eq!(
            read_book_topic(&path).unwrap().as_deref(),
            Some("business-strategy")
        );
        let value: Value = serde_yaml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let map = value.as_mapping().unwrap();
        assert_eq!(
            map.get(Value::String("added_at".into()))
                .and_then(Value::as_str),
            Some("2026-08-27T06:40:15Z")
        );
        assert_eq!(
            map.get(Value::String("future".into()))
                .and_then(Value::as_mapping)
                .and_then(|m| m.get(Value::String("score".into())))
                .and_then(Value::as_i64),
            Some(9)
        );
    }

    #[test]
    fn catalog_aware_assignment_rejects_unknown_topic_without_mutating_meta() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("meta.yml");
        let original = "added_at: 2026-08-27T06:40:15Z\n";
        std::fs::write(&path, original).unwrap();
        let err = assign_book_topic(&path, &one_topic(), "unknown-topic").unwrap_err();
        assert!(err.contains("unknown topic id"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn inventory_metadata_reads_creator_publisher_and_language() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("book.md");
        std::fs::write(
            &path,
            "---\ntype: Book\ntitle: DDIA\ncreator: Martin Kleppmann\npublisher: O'Reilly\nlanguage: en\n---\n",
        )
        .unwrap();
        assert_eq!(
            read_book_frontmatter(&path),
            Some(BookFrontmatter {
                title: "DDIA".into(),
                creator: Some("Martin Kleppmann".into()),
                publisher: Some("O'Reilly".into()),
                language: Some("en".into()),
                source_format: None,
            })
        );
    }

    #[test]
    fn book_evidence_prefers_latest_summary_and_never_exposes_source_path() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("2026-09/Example");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("book.md"),
            concat!(
                "---\n",
                "type: Book\n",
                "title: Example\n",
                "sources:\n",
                "  - resource: /Users/private/secret.epub\n",
                "---\n",
                "# First chapter\n",
                "Opening evidence about distributed systems.\n",
            ),
        )
        .unwrap();
        std::fs::write(dir.join("meta.yml"), "added_at: 2026-09-01T00:00:00Z\n").unwrap();
        std::fs::write(dir.join("2026-09-01-summary.md"), "old summary").unwrap();
        std::fs::write(
            dir.join("2026-09-02-summary.md"),
            "---\ntype: Book Summary\n---\nnewest summary evidence",
        )
        .unwrap();

        let evidence = read_book_evidence(tmp.path(), "2026-09/Example").unwrap();
        assert_eq!(evidence.source_format.as_deref(), Some("epub"));
        assert_eq!(
            evidence.latest_summary_file.as_deref(),
            Some("2026-09-02-summary.md")
        );
        assert_eq!(
            evidence.summary_excerpt.as_deref(),
            Some("newest summary evidence")
        );
        assert_eq!(evidence.headings, ["First chapter"]);
        assert!(evidence
            .opening_excerpt
            .as_deref()
            .unwrap()
            .contains("distributed systems"));
        let serialized = serde_yaml::to_string(&evidence).unwrap();
        assert!(!serialized.contains("/Users/private"));
    }

    #[cfg(unix)]
    #[test]
    fn book_evidence_ignores_a_symlinked_newer_summary() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("2026-09/Example");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("book.md"), "# Book\nbody").unwrap();
        std::fs::write(dir.join("meta.yml"), "added_at: 2026-09-01T00:00:00Z\n").unwrap();
        std::fs::write(dir.join("2026-09-01-summary.md"), "inside").unwrap();
        let external = outside.path().join("secret.md");
        std::fs::write(&external, "outside secret").unwrap();
        symlink(&external, dir.join("2026-09-02-summary.md")).unwrap();

        let evidence = read_book_evidence(tmp.path(), "2026-09/Example").unwrap();
        assert_eq!(
            evidence.latest_summary_file.as_deref(),
            Some("2026-09-01-summary.md")
        );
        assert_eq!(evidence.summary_excerpt.as_deref(), Some("inside"));
    }

    fn write_book(
        root: &Path,
        month: &str,
        dir_name: &str,
        title: &str,
        creator: Option<&str>,
        added_at: Option<&str>,
        topic_id: Option<&str>,
    ) {
        let dir = root.join(month).join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        let author = creator
            .map(|v| format!("sources:\n  - author: {v}\n"))
            .unwrap_or_default();
        std::fs::write(
            dir.join("book.md"),
            format!("---\ntype: Book\ntitle: {title}\n{author}---\n\nBody\n"),
        )
        .unwrap();
        let mut meta = String::new();
        if let Some(value) = added_at {
            meta.push_str(&format!("added_at: {value}\n"));
        }
        if let Some(value) = topic_id {
            meta.push_str(&format!("topic_id: {value}\n"));
        }
        std::fs::write(dir.join("meta.yml"), meta).unwrap();
    }

    #[test]
    fn index_is_deterministic_and_sorts_newest_then_title_then_path() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_book(
            root,
            "2026-08",
            "Older",
            "Older",
            Some("Author A"),
            Some("2026-08-01T00:00:00Z"),
            Some("topic-0"),
        );
        write_book(
            root,
            "2026-07",
            "Newest",
            "A [Newest]",
            Some("Author B"),
            Some("2026-08-02T00:00:00Z"),
            Some("topic-0"),
        );
        let books = scan_books(root).unwrap();
        let rendered = render_index(&one_topic().topics[0], &books);
        assert_eq!(rendered, render_index(&one_topic().topics[0], &books));
        assert!(rendered.starts_with("---\ntype: Book Topic Index\n"));
        assert!(rendered.contains(GENERATED_MARKER));
        assert!(rendered.contains("- **词0甲** — 词甲描述"));
        let newest = rendered.find("A \\[Newest\\]").unwrap();
        let older = rendered.find("[Older]").unwrap();
        assert!(newest < older);
        assert!(rendered.contains("](<2026-07/Newest/book.md>) — Author B · 2026-08-02"));
    }

    #[test]
    fn rebuild_fails_before_writes_on_handwritten_conflict_and_cleans_only_generated_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let catalog = one_topic();
        let target = root.join(&catalog.topics[0].index_file);
        std::fs::write(&target, "# Mine\n").unwrap();
        let err = rebuild_indexes(root, &catalog).unwrap_err();
        assert!(err.contains("hand-written"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "# Mine\n");

        std::fs::write(&target, format!("{GENERATED_MARKER}\nold\n")).unwrap();
        let stale = root.join("旧主题.index.md");
        std::fs::write(&stale, format!("{GENERATED_MARKER}\nstale\n")).unwrap();
        let handwritten_stale = root.join("手写.index.md");
        std::fs::write(&handwritten_stale, "# Keep\n").unwrap();
        let result = rebuild_indexes(root, &catalog).unwrap();
        assert!(is_generated_index(
            &std::fs::read_to_string(&target).unwrap()
        ));
        assert_eq!(
            result.removed_stale_indexes,
            vec![PathBuf::from("旧主题.index.md")]
        );
        assert!(!stale.exists());
        assert!(handwritten_stale.exists());
    }

    #[test]
    fn stale_generated_index_cleanup_is_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let stale = tmp.path().join("Old.INDEX.MD");
        std::fs::write(&stale, format!("{GENERATED_MARKER}\nstale\n")).unwrap();
        rebuild_indexes(tmp.path(), &one_topic()).unwrap();
        assert!(!stale.exists());
    }

    #[cfg(unix)]
    #[test]
    fn preflight_rejects_desired_and_stale_symlink_indexes_before_writing() {
        use std::os::unix::fs::symlink;

        for desired_link in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let external = outside.path().join("outside.md");
            let original = format!("{GENERATED_MARKER}\noutside\n");
            std::fs::write(&external, &original).unwrap();
            let link_name = if desired_link {
                one_topic().topics[0].index_file.clone()
            } else {
                "Stale.index.md".to_string()
            };
            symlink(&external, root.path().join(link_name)).unwrap();

            let error = rebuild_indexes(root.path(), &one_topic()).unwrap_err();
            assert!(error.contains("symlinked topic index"), "{error}");
            assert_eq!(std::fs::read_to_string(&external).unwrap(), original);
            if !desired_link {
                assert!(
                    !root.path().join(&one_topic().topics[0].index_file).exists(),
                    "stale-index validation must happen before writing desired indexes"
                );
            }
        }
    }

    #[test]
    fn preflight_rejects_a_non_regular_stale_index_before_writing() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Stale.index.md")).unwrap();
        let error = rebuild_indexes(root.path(), &one_topic()).unwrap_err();
        assert!(error.contains("non-regular"), "{error}");
        assert!(!root.path().join(&one_topic().topics[0].index_file).exists());
    }

    #[cfg(unix)]
    #[test]
    fn scan_and_mutation_refuse_symlinked_book_directories() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_book = outside.path().join("Book");
        std::fs::create_dir_all(&outside_book).unwrap();
        std::fs::write(
            outside_book.join("book.md"),
            "---\ntype: Book\ntitle: X\n---\n",
        )
        .unwrap();
        std::fs::write(
            outside_book.join("meta.yml"),
            "added_at: 2026-09-01T00:00:00Z\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.path().join("2026-09")).unwrap();
        symlink(&outside_book, root.path().join("2026-09/Book")).unwrap();

        assert!(scan_books(root.path()).unwrap().is_empty());
        assert!(existing_book_meta(root.path(), "2026-09/Book").is_err());
        assert_eq!(
            std::fs::read_to_string(outside_book.join("meta.yml")).unwrap(),
            "added_at: 2026-09-01T00:00:00Z\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn inventory_scan_refuses_symlinked_book_or_meta_files() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let external_book = outside.path().join("book.md");
        let external_meta = outside.path().join("meta.yml");
        std::fs::write(&external_book, "---\ntitle: Outside\n---\n").unwrap();
        std::fs::write(&external_meta, "topic_id: topic-0\n").unwrap();

        let book_link = root.path().join("2026-09/Book Link");
        std::fs::create_dir_all(&book_link).unwrap();
        symlink(&external_book, book_link.join("book.md")).unwrap();
        std::fs::write(book_link.join("meta.yml"), "").unwrap();

        let meta_link = root.path().join("2026-09/Meta Link");
        std::fs::create_dir_all(&meta_link).unwrap();
        std::fs::write(meta_link.join("book.md"), "---\ntitle: Inside\n---\n").unwrap();
        symlink(&external_meta, meta_link.join("meta.yml")).unwrap();

        assert!(scan_books(root.path()).unwrap().is_empty());
        assert!(existing_book_meta(root.path(), "2026-09/Book Link").is_err());
        assert!(existing_book_meta(root.path(), "2026-09/Meta Link").is_err());
    }

    #[test]
    fn rebuild_reports_legacy_and_unknown_assignments_without_hiding_books() {
        let tmp = tempfile::tempdir().unwrap();
        write_book(
            tmp.path(),
            "2026-08",
            "Legacy",
            "Legacy",
            None,
            Some("2026-08-01T00:00:00Z"),
            None,
        );
        write_book(
            tmp.path(),
            "2026-08",
            "Unknown",
            "Unknown",
            None,
            Some("2026-08-01T00:00:00Z"),
            Some("deleted-topic"),
        );
        let result = rebuild_indexes(tmp.path(), &one_topic()).unwrap();
        assert_eq!(result.unclassified_books, vec!["2026-08/Legacy"]);
        assert_eq!(result.unknown_topic_books, vec!["2026-08/Unknown"]);
    }

    #[test]
    fn shared_topic_lock_prevents_lost_updates() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::sync::Arc::new(tmp.path().to_path_buf());
        let counter = root.join("counter");
        std::fs::write(&counter, "0").unwrap();
        let mut workers = Vec::new();
        for _ in 0..6 {
            let root = root.clone();
            let counter = counter.clone();
            workers.push(std::thread::spawn(move || {
                for _ in 0..25 {
                    with_topic_lock(&root, || {
                        let value: usize = std::fs::read_to_string(&counter)
                            .map_err(|e| e.to_string())?
                            .parse::<usize>()
                            .map_err(|e| e.to_string())?;
                        std::fs::write(&counter, (value + 1).to_string()).map_err(|e| e.to_string())
                    })
                    .unwrap();
                }
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(std::fs::read_to_string(counter).unwrap(), "150");
    }
}
