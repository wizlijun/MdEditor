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
pub mod origin;
pub mod outline;
pub mod paths;
pub mod prose;
pub mod query;
pub mod scan;
pub mod store;
pub mod tokenize;
pub mod watch;

pub use block::{Block, BlockLevel, FileMeta, Link};
pub use origin::Origin;
pub use query::{Hit, Query, Route};
pub use scan::{IndexOutcome, Phase, Progress, ProgressFn, ScanOptions, ScanStats, SkippedFile};

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
        // Must be the *same* normalization `paths::vault_key` uses, or two
        // spellings of one vault share a database while disagreeing about the
        // stamp — see `paths::normalized_vault_root`.
        let root = paths::normalized_vault_root(vault_root);
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
        self.rebuild_with_progress(opts, None)
    }

    /// Same as [`Self::rebuild`], but calls `progress` (throttled — every 25
    /// files or 200ms, always on a phase transition) as the build proceeds.
    /// `searchidx` has no dependency on `tauri`, so it is up to the caller
    /// to turn this into events; this crate only promises the callback
    /// contract.
    pub fn rebuild_with_progress(
        &mut self,
        opts: &ScanOptions,
        progress: Option<ProgressFn>,
    ) -> Result<ScanStats, String> {
        scan::build_full(&mut self.conn, &self.vault_root, opts, progress).map_err(|e| e.to_string())
    }

    pub fn sweep(&mut self, opts: &ScanOptions, deadline: Option<Duration>) -> Result<ScanStats, String> {
        self.sweep_with_progress(opts, deadline, None)
    }

    /// Same as [`Self::sweep`], but calls `progress` as the sweep proceeds.
    pub fn sweep_with_progress(
        &mut self,
        opts: &ScanOptions,
        deadline: Option<Duration>,
        progress: Option<ProgressFn>,
    ) -> Result<ScanStats, String> {
        scan::sweep(&mut self.conn, &self.vault_root, opts, deadline, progress).map_err(|e| e.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `~/vault` and `~/vault/` are the same vault — and both are ordinary to
    /// type, since bash and zsh both append the slash when you tab-complete a
    /// directory into `notemd search --vault`. They resolve to one `index.db`
    /// (`vault_key` trims the slash), so they must also stamp one
    /// `meta.vault_root`. When they did not, the equality check guarding that
    /// stamp was permanently false and *every* open — including read-only
    /// `notemd search` — wrote. The timing assertion is the symptom that made
    /// that expensive: a write against another process's open write
    /// transaction waits out the full 5s `busy_timeout` before giving up.
    #[test]
    fn a_trailing_slash_is_the_same_vault_and_stamps_the_same_root() {
        let d = tempfile::tempdir().unwrap();
        let db = d.path().join("index.db");
        let bare = d.path().join("vault");
        let slashed = std::path::PathBuf::from(format!("{}/", bare.to_string_lossy()));
        std::fs::create_dir_all(&bare).unwrap();

        assert_eq!(paths::vault_key(&bare), paths::vault_key(&slashed));

        let first = SearchIndex::open_at(&bare, &db).unwrap();
        let stamped = store::meta_get(&first.conn, "vault_root").unwrap();
        assert_eq!(stamped, paths::normalized_vault_root(&bare));
        drop(first);

        // Stand in for the GUI mid-rebuild: a held write transaction. The
        // second open has nothing to write, so it must not wait on it.
        let mut holder = store::open(&db, &stamped).unwrap();
        let tx = holder
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        store::meta_set(&tx, "probe", "1").unwrap();

        let started = std::time::Instant::now();
        let second = SearchIndex::open_at(&slashed, &db).unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(4),
            "the trailing-slash spelling must not re-stamp, and so must not wait on a writer"
        );
        assert_eq!(store::meta_get(&second.conn, "vault_root").as_deref(), Some(stamped.as_str()));
        tx.commit().unwrap();
    }
}
