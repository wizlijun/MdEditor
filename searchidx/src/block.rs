//! The data model every chunker produces and the store consumes.
//!
//! Three resolutions per file (design spec §3.3): a `Line` block for "find me
//! that exact sentence", a `Section` block for "what does this section argue",
//! a `File` block for "what is this document about". Matching the granularity
//! of the question is what makes retrieval both fast and precise.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockLevel {
    File,
    Section,
    Line,
}

impl BlockLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockLevel::File => "file",
            BlockLevel::Section => "section",
            BlockLevel::Line => "line",
        }
    }
    pub fn from_str(s: &str) -> BlockLevel {
        match s {
            "file" => BlockLevel::File,
            "section" => BlockLevel::Section,
            _ => BlockLevel::Line,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    /// 1-based, inclusive.
    pub line_start: u32,
    pub line_end: u32,
    /// Ancestor chain derived at index time. Never written back to the file —
    /// we take the self-containment benefit without polluting the vault.
    pub breadcrumb: String,
    pub text: String,
    pub level: BlockLevel,
    /// `type:: annotation` or `type:: question` on an outline node.
    pub is_annotation: bool,
    /// The `by::` value when it is NOT a `human:` actor — i.e. an AI author.
    pub agent_by: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FileMeta {
    pub title: Option<String>,
    pub concept_type: Option<String>,
    pub tags: Vec<String>,
    /// `YYYY-MM-DD`.
    pub doc_date: Option<String>,
    /// True when `doc_date` came from mtime rather than the name/frontmatter.
    pub date_inferred: bool,
    pub human_verified: bool,
    /// Provenance tier from `origin::derive` (spec §3) — index-side only,
    /// never written back to the file. See `crate::origin` for the rule
    /// table and `store::replace_file`/`store::origin_of` for how it is
    /// persisted and read back.
    pub origin: crate::origin::Origin,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// `"wiki"` or `"md"`.
    pub kind: String,
    pub target: String,
    pub line: u32,
}

/// Join breadcrumb levels, truncating each to 40 chars (spec §3.6).
pub fn breadcrumb_of(levels: &[String]) -> String {
    levels
        .iter()
        .map(|l| l.chars().take(40).collect::<String>())
        .collect::<Vec<_>>()
        .join(" > ")
}
