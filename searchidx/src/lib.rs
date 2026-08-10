//! Platform-independent full-text index for note.md vaults.
//!
//! The index is a **pure function of the vault's files** (plus the tokenizer
//! version) stored outside the vault, in the machine's local app cache. It is
//! disposable: any inconsistency is resolved by deleting and rebuilding, never
//! by repair logic. Nothing here ever writes into the vault.
//!
//! Everything that decides *what gets indexed and what ranks first* lives in
//! this crate so that the Tauri command layer, the `notemd search` CLI and the
//! file watcher are three thin adapters over one algorithm. That is the whole
//! reason the crate exists — see docs/2026-08-10-vault-search-index-design.md §2.

pub mod block;
pub mod chunk;
pub mod frontmatter;
pub mod links;
pub mod norm;
pub mod outline;
pub mod paths;
pub mod prose;
pub mod query;
pub mod scan;
pub mod store;
pub mod tokenize;
pub mod watch;

pub use block::{Block, BlockLevel, FileMeta, Link};
pub use query::{Hit, Query, Route};
pub use scan::{IndexOutcome, ScanOptions, ScanStats};

use std::path::{Path, PathBuf};
use std::time::Duration;

/// One open index over one vault.
///
/// Errors are `String` on purpose: every caller (Tauri command, CLI, watcher)
/// turns them into a degradation, never a failure the user must act on. See the
/// degradation matrix in the design spec §9.
pub struct SearchIndex {
    conn: rusqlite::Connection,
    vault_root: PathBuf,
    db_path: PathBuf,
}

impl SearchIndex {
    pub fn open(vault_root: &Path) -> Result<Self, String> {
        let db = paths::index_db_path(vault_root).ok_or("no local app data directory")?;
        Self::open_at(vault_root, &db)
    }

    pub fn open_at(vault_root: &Path, db_path: &Path) -> Result<Self, String> {
        let root = vault_root.to_string_lossy().replace('\\', "/");
        let conn = store::open(db_path, &root).map_err(|e| e.to_string())?;
        Ok(SearchIndex {
            conn,
            vault_root: vault_root.to_path_buf(),
            db_path: db_path.to_path_buf(),
        })
    }

    /// Build if the index is empty, otherwise leave it alone. Callers that want
    /// freshness call [`Self::sweep`].
    pub fn ensure_built(&mut self, opts: &ScanOptions) -> Result<ScanStats, String> {
        let files: i64 = self
            .conn
            .query_row("SELECT count(*) FROM files", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if files > 0 {
            return Ok(ScanStats::default());
        }
        self.rebuild(opts)
    }

    pub fn rebuild(&mut self, opts: &ScanOptions) -> Result<ScanStats, String> {
        scan::build_full(&mut self.conn, &self.vault_root, opts).map_err(|e| e.to_string())
    }

    pub fn sweep(&mut self, opts: &ScanOptions, deadline: Option<Duration>) -> Result<ScanStats, String> {
        scan::sweep(&mut self.conn, &self.vault_root, opts, deadline).map_err(|e| e.to_string())
    }

    /// Re-index one file. Returns [`IndexOutcome`] rather than a bare bool so
    /// a caller (the file watcher) can log *why* a file left the index —
    /// gone, oversized, or no longer indexable are distinct situations, not
    /// one failure.
    pub fn index_one(&mut self, rel: &str, opts: &ScanOptions) -> Result<IndexOutcome, String> {
        scan::index_one(&mut self.conn, &self.vault_root, rel, opts).map_err(|e| e.to_string())
    }

    pub fn search(&self, raw: &str, limit: usize) -> Result<(Vec<Hit>, Route), String> {
        let q = query::parse(raw);
        query::search(&self.conn, &q, limit, &today()).map_err(|e| e.to_string())
    }

    pub fn stats(&self) -> Result<IndexStats, String> {
        let files = self.conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        let blocks = self.conn.query_row("SELECT count(*) FROM blocks", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        Ok(IndexStats {
            files,
            blocks,
            db_bytes: std::fs::metadata(&self.db_path).map(|m| m.len()).unwrap_or(0),
            built_at: store::meta_get(&self.conn, "built_at"),
            tokenizer_id: store::meta_get(&self.conn, "tokenizer_id").unwrap_or_default(),
        })
    }

    pub fn vault_root(&self) -> &Path {
        &self.vault_root
    }
}

#[derive(Debug, Clone)]
pub struct IndexStats {
    pub files: i64,
    pub blocks: i64,
    pub db_bytes: u64,
    pub built_at: Option<String>,
    pub tokenizer_id: String,
}

fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    chunk::ymd_from_unix_public(secs)
}
