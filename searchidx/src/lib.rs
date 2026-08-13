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

pub mod attention;
pub mod block;
pub mod chunk;
pub mod frontmatter;
pub mod globs;
pub mod links;
pub mod norm;
pub mod origin;
pub mod outline;
pub mod paths;
pub mod plain;
pub mod prose;
pub mod query;
/// Rename/move detection's pure pairing half. `pub(crate)` throughout: the
/// two call sites (the sweep and the watcher batch) are both inside this
/// crate, and the decision is not something a caller should be able to
/// second-guess from outside.
pub(crate) mod rename;
pub mod scan;
pub mod store;
pub mod tokenize;
pub mod transcript;
pub mod watch;

pub use block::{Block, BlockLevel, FileMeta, Link};
pub use origin::Origin;
pub use query::{Abort, Answer, Hit, Limits, Query, Route};
pub use scan::{
    BatchOutcome, IndexOutcome, Phase, Progress, ProgressFn, ScanOptions, ScanStats, SkippedFile,
};

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
    /// `globs_stamp` is `SourceGlobs::stamp()` for the vault's currently
    /// configured source-glob patterns — the *same* `SourceGlobs` that ends
    /// up in `ScanOptions.source_globs`, not an independently resolved
    /// value. This crate has no `ScanOptions` in hand at `open` time (the
    /// caller opens the index before it necessarily has one built), so it
    /// cannot enforce that itself; both of the two `SearchIndex::open`
    /// callers — `search::mod::open_vault` and `cli::search::run` — compute
    /// `globs_stamp` as `opts.source_globs.stamp()`, off the very
    /// `ScanOptions` value returned by `search::options::for_vault` (the
    /// self-declared single construction point for `ScanOptions`), rather
    /// than resolving the vault's settings a second time. Precedent this
    /// deliberately does NOT follow: the retired `sync_dir` version of this
    /// parameter genuinely was sourced independently, because `sync_dir`
    /// was never a `ScanOptions` field. `source_globs` *is* one — computing
    /// `globs_stamp` any other way here would let it drift from the value
    /// `origin::derive` actually used, silently neutering this whole
    /// invalidation mechanism the moment the two values diverge. Stamped
    /// into the index and compared on every open so a changed setting
    /// invalidates every stored row instead of leaving it silently stale.
    /// See `store::open`'s doc comment.
    pub fn open(vault_root: &Path, globs_stamp: &str) -> Result<Self, String> {
        let db = paths::index_db_path(vault_root).ok_or("no local app data directory")?;
        Self::open_at(vault_root, &db, globs_stamp)
    }

    pub fn open_at(vault_root: &Path, db_path: &Path, globs_stamp: &str) -> Result<Self, String> {
        // Must be the *same* normalization `paths::vault_key` uses, or two
        // spellings of one vault share a database while disagreeing about the
        // stamp — see `paths::normalized_vault_root`.
        let root = paths::normalized_vault_root(vault_root);
        let conn = store::open(db_path, &root, globs_stamp).map_err(|e| e.to_string())?;
        Ok(SearchIndex {
            conn,
            vault_root: vault_root.to_path_buf(),
            db_path: db_path.to_path_buf(),
        })
    }

    /// Build if the index is empty, otherwise leave it alone. Callers that want
    /// freshness call [`Self::sweep`].
    pub fn ensure_built(&mut self, opts: &ScanOptions) -> Result<ScanStats, String> {
        self.ensure_built_with_progress(opts, None)
    }

    /// Same as [`Self::ensure_built`], but reports progress for the build it
    /// may run. The cold-start build is the *longest* scan this crate ever
    /// performs (a full vault, from an empty database) and it is the one the
    /// user is most likely to be sitting in front of waiting — so a caller
    /// that has a progress channel must be able to drive it here, not only
    /// from an explicit [`Self::rebuild`]. Without this the host's startup
    /// open reported nothing at all for minutes and its settings page could
    /// only say "still building" with no numbers behind it.
    ///
    /// The no-op case (index already has rows) reports nothing, on purpose:
    /// there is no work to report on, and a lone phase event with no follow-up
    /// would leave a progress bar on screen that never moves.
    pub fn ensure_built_with_progress(
        &mut self,
        opts: &ScanOptions,
        progress: Option<ProgressFn>,
    ) -> Result<ScanStats, String> {
        let files: i64 = self
            .conn
            .query_row("SELECT count(*) FROM files", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if files > 0 {
            return Ok(ScanStats::default());
        }
        self.rebuild_with_progress(opts, progress)
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

    /// Apply one watcher batch. Prefer this over a loop of [`Self::index_one`]
    /// when the paths arrived together: only a batch can see both ends of a
    /// rename, and only then can the rename be taken at the cost of an
    /// UPDATE instead of a full rebuild. `index_one`'s single-file contract
    /// is unchanged — a rename is a cross-file judgement and cannot be made
    /// one file at a time (spec §6).
    ///
    /// `String` error, like every other method here rather than the
    /// `rusqlite::Result` the plan sketched: this facade's callers all
    /// degrade rather than propagate, and one method leaking the storage
    /// crate's error type into the watcher would be the only exception.
    pub fn apply_batch(&mut self, rels: &[String], opts: &ScanOptions) -> Result<BatchOutcome, String> {
        scan::apply_batch(&mut self.conn, &self.vault_root, rels, opts).map_err(|e| e.to_string())
    }

    pub fn search(&self, raw: &str, limit: usize) -> Result<(Vec<Hit>, Route), String> {
        let q = query::parse(raw);
        query::search(&self.conn, &q, limit, &today()).map_err(|e| e.to_string())
    }

    /// Same retrieval, under a caller-supplied budget: an interactive caller
    /// keeps live typing off the expensive fallback ([`Limits::deep`]) and
    /// abandons a query the moment the user has moved on ([`Limits::abort`]).
    ///
    /// Ranks with [`query::Weights::default`] — the shipped constants,
    /// unconditionally. Kept that way on purpose rather than reading a
    /// settings-page value itself: this signature is called from places
    /// (tests, any future caller that just wants "the shipped ranking")
    /// that must not have to learn about `Weights` just to keep compiling.
    /// A caller that wants a configured value — `search::options::
    /// weights_for_vault` / `cli::search::weights_for`'s the GUI and CLI
    /// (C-T8) — calls [`Self::search_with_weights`] instead.
    pub fn search_with(&self, raw: &str, limit: usize, limits: &Limits) -> Result<Answer, String> {
        self.search_with_weights(raw, limit, limits, &query::Weights::default())
    }

    /// [`Self::search_with`], but ranked with a caller-supplied [`query::
    /// Weights`] instead of the shipped constants. A sibling method rather
    /// than a new parameter on `search_with` itself — C-T7's doc comment on
    /// `query::Weights` (predicting this exact follow-up) ruled out changing
    /// that facade's signature, since every existing caller of `search_with`
    /// would otherwise have to start passing weights just to keep compiling.
    /// `search_with` is now defined in terms of this method with
    /// `Weights::default()`, so the two can never silently diverge in
    /// anything but the weights argument.
    pub fn search_with_weights(
        &self,
        raw: &str,
        limit: usize,
        limits: &Limits,
        weights: &query::Weights,
    ) -> Result<Answer, String> {
        self.search_ranked(raw, limit, limits, weights, &query::Conventions::default())
    }

    /// [`Self::search_with_weights`], plus what the ranking needs to know
    /// about this vault's conventions — today just `wikipageDir`, which is
    /// what lets the page a `[[…]]` link would create be pinned above
    /// everything else when the query names it exactly (wikipage priority
    /// spec §4).
    ///
    /// A fourth sibling method rather than a parameter on `search_with_
    /// weights`, for the same reason `search_with_weights` was a sibling of
    /// `search_with`: callers who have no vault settings in hand — tests, and
    /// anything that just wants "the shipped behaviour" — must not have to
    /// learn about `Conventions` to keep compiling. `Conventions::default()`
    /// (no directory, so no pinning) is the honest answer for them: a caller
    /// that cannot know the user's directory name must not guess one.
    pub fn search_ranked(
        &self,
        raw: &str,
        limit: usize,
        limits: &Limits,
        weights: &query::Weights,
        conventions: &query::Conventions,
    ) -> Result<Answer, String> {
        let q = query::parse(raw);
        query::search_with(&self.conn, &q, limit, &today(), limits, weights, conventions)
            .map_err(|e| e.to_string())
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
            origin_counts: origin_counts(&self.conn)?,
            type_counts: type_counts(&self.conn)?,
            attention_files: store::attention_file_count(&self.conn).unwrap_or(0),
            attention_as_of: store::meta_get(&self.conn, "attention_as_of"),
        })
    }

    pub fn vault_root(&self) -> &Path {
        &self.vault_root
    }

    /// 重新摄取 vault 的注意力数据,返回写入的文件数。
    ///
    /// **全量重算**,不是增量:当天的 analytics 文件整天都在被重写,任何
    /// 「读到哪儿了」的水位方案算错时都是静默的(分数偏高,无症状)。重算是
    /// 无状态的,因此也是幂等的 —— 连调两次结果完全相同。
    ///
    /// `links` 由调用方从 `.notemd/mirrors/` 读出(`MirrorMeta` 的格式归
    /// `src-tauri` 所有,见 `attention::MirrorLink` 的文档)。传空切片是
    /// 合法的:那只意味着 vault 外源文件的阅读时长不参与,vault 内的照常。
    pub fn refresh_attention(
        &mut self,
        links: &[attention::MirrorLink],
    ) -> Result<usize, String> {
        let as_of = today();
        let files = attention::collect(&self.vault_root, &as_of);
        let folded = attention::fold(&files, links, &as_of);
        let n = store::replace_attention(&self.conn, &as_of, &folded).map_err(|e| e.to_string())?;
        // `doc_attention` 的行数在零结果时无法反推「跑没跑过」—— 一个空表和一个
        // 从未摄取过的库看起来完全一样。单独把这一轮跑过的事实盖进 `meta`,
        // `IndexStats::attention_as_of` 读的是这个键,不是 `doc_attention` 本身。
        store::meta_set(&self.conn, "attention_as_of", &as_of).map_err(|e| e.to_string())?;
        Ok(n)
    }
}

/// Design spec §6/§9/§7.4: the settings page's per-tier file counts, one
/// `GROUP BY` over the real `files.origin` column. Every stored value is one
/// of `origin::Origin::as_str()`'s strings by construction (`origin` is
/// `NOT NULL` and has exactly one writer, `store::replace_file`) — but this
/// still routes each row through `store::origin_of` rather than matching the
/// literals directly, so a row that somehow holds anything else still lands
/// in a real bucket (falling back to `Derived`, `origin_of`'s own documented
/// default) instead of silently vanishing from every count.
///
/// C-T11 added the fourth `unlabeled` field (B-T8 originally shipped this as
/// a three-tier shape with a documented, TODO-marked undercount — see the
/// removed `Origin::Unlabeled => {}` no-op arm this replaced, and the git
/// history of this function for that TODO's full reasoning). `unlabeled` is
/// real and load-bearing for the settings page: it is the actionable exit
/// the design spec's §7.4 clickable row exists for (`origin:unlabeled`),
/// and — for anyone auditing `human + derived + source + unlabeled ==
/// files` — it is now the whole story, not `files - 1`.
fn origin_counts(conn: &rusqlite::Connection) -> Result<OriginCounts, String> {
    let mut stmt = conn
        .prepare("SELECT origin, count(*) FROM files GROUP BY origin")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut counts = OriginCounts::default();
    for row in rows {
        let (raw, n) = row.map_err(|e| e.to_string())?;
        match store::origin_of(raw.as_deref()) {
            Origin::Human => counts.human += n,
            Origin::Derived => counts.derived += n,
            Origin::Source => counts.source += n,
            Origin::Unlabeled => counts.unlabeled += n,
        }
    }
    Ok(counts)
}

/// Review round 1 finding: the bound on how many rows `type_counts` can
/// return is NOT `CONCEPT_TYPE`'s registry size, which is what the original
/// version of this module was implicitly sized against. Rule 7
/// (`origin::derive`) stores whatever string sits in an unregistered `type:`
/// straight through — so every distinct *free-text* value gets its own
/// settings-page row, registered or not. A single ebook-import run that
/// stamps a fresh `type: Chapter N Summary` per chapter, or one agent that
/// free-types a `type:` per document, produces one row per document, not
/// one per registered type. 10 is deliberately small: this section is a
/// compact overview panel (unlike the settings page's skipped-files list or
/// the 500-file-milestone rebuild log, which are diagnostic and expected to
/// scroll), and a real vault's derived content is expected to cluster into a
/// handful of dominant types (`Book Summary`, `Answer`, …) — the tail this
/// cap drops is exactly the "one-off or mistyped `type:`" case that belongs
/// folded into a remainder, not itemized by name.
const TYPE_COUNTS_CAP: i64 = 10;

/// Design spec §6: `derived`'s distribution by `concept_type`, for the
/// settings page's tier breakdown. Scoped to `origin = 'derived'` AND a
/// non-null `concept_type` — untyped derived files (rule 7's "has
/// frontmatter but an unregistered/absent type") are deliberately excluded
/// rather than stashed under a sentinel key, matching the panel's own
/// grouping convention (`grouping.ts`'s `derivedOther` group is a computed
/// remainder, not a named type).
///
/// Capped to the top [`TYPE_COUNTS_CAP`] types by file count (ties broken by
/// name, for determinism) — see that constant's doc comment for why this is
/// necessary and how the bound was picked. The overflow is not lost: it is
/// simply not itemized. `origin_counts.derived` (a separate, uncapped query)
/// stays the true total, and the frontend's existing "untyped derived"
/// remainder — `origin_counts.derived - sum(type_counts.values())` — folds
/// BOTH the genuinely untyped files AND any capped-off named types into the
/// same "Other" bucket it already renders, with no wire-shape change needed.
///
/// A `BTreeMap` gives deterministic key order for the DTO and its tests; the
/// frontend only reads by key, never assumes insertion order (it re-sorts by
/// count for display).
fn type_counts(conn: &rusqlite::Connection) -> Result<std::collections::BTreeMap<String, i64>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT concept_type, count(*) FROM files \
             WHERE origin = 'derived' AND concept_type IS NOT NULL \
             GROUP BY concept_type \
             ORDER BY count(*) DESC, concept_type ASC \
             LIMIT ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([TYPE_COUNTS_CAP], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut out = std::collections::BTreeMap::new();
    for row in rows {
        let (t, n) = row.map_err(|e| e.to_string())?;
        out.insert(t, n);
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OriginCounts {
    pub human: i64,
    pub derived: i64,
    pub source: i64,
    /// Added C-T11 — see `origin_counts`'s doc comment. Files with no
    /// frontmatter and no matching source-glob pattern (`origin::derive`
    /// rule 6′); the settings page's dedicated, clickable exit for this tier
    /// runs `origin:unlabeled` (design spec §7.4).
    pub unlabeled: i64,
}

#[derive(Debug, Clone)]
pub struct IndexStats {
    pub files: i64,
    pub blocks: i64,
    pub db_bytes: u64,
    pub built_at: Option<String>,
    pub tokenizer_id: String,
    pub origin_counts: OriginCounts,
    pub type_counts: std::collections::BTreeMap<String, i64>,
    /// 有注意力数据、**且仍在索引里**的文件数。与 `files` 一起构成设置页的
    /// 覆盖率行 ——「摄取根本没跑起来」在别处没有任何可见症状,这是唯一的
    /// 发现途径。
    ///
    /// 口径是交集而不是 `doc_attention` 的行数,理由(以及那个「60 / 1」的
    /// 实测)见 `store::attention_file_count`:这个数字与 `files` 一起显示,
    /// 分子大于分母就等于没有意义。
    pub attention_files: i64,
    /// 上一轮 `refresh_attention` 用的 `as_of`(存在 `meta.attention_as_of`
    /// 键里,不是从 `doc_attention` 的行反推出来的)。
    ///
    /// `None` **必须**只表示「摄取从未在这个索引上跑过」。「跑过但零结果」是
    /// 一个不同的状态,表现为 `Some(day)` 且 `attention_files == 0` —— 用户
    /// 一次没开过 Reading Insights 和开了但恰好没有任何有效数据,是两件设置页
    /// 需要分开说清的事;如果这个字段只看 `doc_attention` 是否有行,零结果的
    /// 那一轮会被误判成「从未跑过」,两种状态就再也分不开了。
    pub attention_as_of: Option<String>,
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

        let first = SearchIndex::open_at(&bare, &db, "sync").unwrap();
        let stamped = store::meta_get(&first.conn, "vault_root").unwrap();
        assert_eq!(stamped, paths::normalized_vault_root(&bare));
        drop(first);

        // Stand in for the GUI mid-rebuild: a held write transaction. The
        // second open has nothing to write, so it must not wait on it.
        let mut holder = store::open(&db, &stamped, "sync").unwrap();
        let tx = holder
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        store::meta_set(&tx, "probe", "1").unwrap();

        let started = std::time::Instant::now();
        let second = SearchIndex::open_at(&slashed, &db, "sync").unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(4),
            "the trailing-slash spelling must not re-stamp, and so must not wait on a writer"
        );
        assert_eq!(store::meta_get(&second.conn, "vault_root").as_deref(), Some(stamped.as_str()));
        tx.commit().unwrap();
    }

    /// Task B-T8 (design spec §6/§9): the settings page's per-tier
    /// statistics come from `IndexStats::origin_counts` /
    /// `IndexStats::type_counts`, both SQL `GROUP BY`s over the real
    /// `files` table — not a re-derivation in Rust. Eight files exercise
    /// every tier plus the "derived but untyped" case that must NOT show up
    /// in `type_counts` (grouping.ts's convention: untyped derived hits are
    /// a computed "other" bucket, not a named type — see
    /// `SettingsDialog.svelte`'s tier section, which derives that bucket as
    /// `derived - sum(type_counts.values())` rather than the backend
    /// stashing it under a sentinel key).
    #[test]
    fn stats_reports_origin_and_type_counts_via_group_by() {
        let d = tempfile::tempdir().unwrap();
        let vault = d.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();

        // Human: rule 1 (`.note.md`) + rule 3 (`verified.by: human:`).
        std::fs::write(vault.join("a.note.md"), "my own words\n").unwrap();
        std::fs::write(
            vault.join("verified.md"),
            "---\nverified:\n  by: human:bruce\n---\nsigned off\n",
        )
        .unwrap();

        // Derived: two `Book Summary`, one `Answer`, one untyped (rule 7).
        std::fs::write(vault.join("summary1.md"), "---\ntype: Book Summary\n---\nbody\n").unwrap();
        std::fs::write(vault.join("summary2.md"), "---\ntype: Book Summary\n---\nbody\n").unwrap();
        std::fs::write(vault.join("answer1.md"), "---\ntype: Answer\n---\nbody\n").unwrap();
        std::fs::write(vault.join("untyped.md"), "---\ntitle: no type here\n---\nbody\n").unwrap();

        // Source: mapped `type: Book` (rule 4).
        std::fs::write(vault.join("book1.md"), "---\ntype: Book\n---\nbody\n").unwrap();
        // Unlabeled: no frontmatter at all and no configured source-glob
        // pattern to match it either (rule 6′) — `ScanOptions::default()`
        // below carries an empty `source_globs` (matches nothing), so this
        // is the only way to reach rule 6′ through the real pipeline today.
        std::fs::write(vault.join("raw.md"), "no frontmatter, no claim\n").unwrap();

        let db = d.path().join("index.db");
        let mut idx = SearchIndex::open_at(&vault, &db, "sync").unwrap();
        idx.rebuild(&ScanOptions::default()).unwrap();

        let s = idx.stats().unwrap();
        assert_eq!(s.files, 8, "raw.md is still indexed and counted in the total");
        assert_eq!(s.origin_counts.human, 2, "a.note.md + verified.md");
        assert_eq!(s.origin_counts.derived, 4, "2x Book Summary + Answer + untyped");
        assert_eq!(s.origin_counts.source, 1, "Book only — raw.md now classifies Unlabeled, not Source (rule 6′)");
        // C-T11: `OriginCounts` grew a fourth `unlabeled` field (was the
        // known, documented `files - 1` undercount from B-T8 through C-T10 —
        // see this test's git history). raw.md is the one file that reaches
        // rule 6′, so it must land here now instead of nowhere.
        assert_eq!(s.origin_counts.unlabeled, 1, "raw.md — no frontmatter, no matching source-glob pattern");
        assert_eq!(
            s.origin_counts.human + s.origin_counts.derived + s.origin_counts.source + s.origin_counts.unlabeled,
            s.files,
            "all four tiers together must now account for every indexed file, no undercount left"
        );

        assert_eq!(s.type_counts.get("Book Summary").copied(), Some(2));
        assert_eq!(s.type_counts.get("Answer").copied(), Some(1));
        // Untyped derived and every `source`/`human` file must be absent —
        // `type_counts` is scoped to `derived` with a non-null `concept_type`.
        assert_eq!(s.type_counts.len(), 2, "no untyped/source/human entries leaked in: {:?}", s.type_counts);
    }

    /// Review round 1 finding: `type_counts` has no bound on the number of
    /// distinct `concept_type` strings it can return — and the bound is NOT
    /// `CONCEPT_TYPE`'s registry size, because rule 7 stores *unregistered*
    /// free-text types verbatim too (`TYPE_COUNTS_CAP`'s doc comment has the
    /// full scenario: per-chapter ebook-import type stamping, one row per
    /// document). 12 distinct types with strictly decreasing counts (12, 11,
    /// …, 1) — writing straight into `files` via SQL, bypassing frontmatter
    /// parsing entirely, since this test is about the `GROUP BY`/`LIMIT`
    /// behavior, not `origin::derive`, which the fixture-based test above
    /// already covers.
    #[test]
    fn type_counts_caps_at_the_top_n_by_count_without_shrinking_the_derived_total() {
        let d = tempfile::tempdir().unwrap();
        let vault = d.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        let db = d.path().join("index.db");
        let idx = SearchIndex::open_at(&vault, &db, "sync").unwrap();

        let mut expected_derived_total = 0i64;
        for i in 1..=12 {
            let count = 13 - i; // Type01: 12 files, … Type12: 1 file
            expected_derived_total += count;
            for n in 0..count {
                idx.conn
                    .execute(
                        "INSERT INTO files(path, ext, origin, concept_type) VALUES (?1, 'md', 'derived', ?2)",
                        rusqlite::params![format!("t{i}-{n}.md"), format!("Type{i:02}")],
                    )
                    .unwrap();
            }
        }

        let s = idx.stats().unwrap();
        assert_eq!(
            s.origin_counts.derived, expected_derived_total,
            "the true derived total (a separate, uncapped query) must not shrink because of the display cap"
        );
        assert_eq!(s.type_counts.len(), TYPE_COUNTS_CAP as usize, "only the top {TYPE_COUNTS_CAP} types are itemized");
        // Type01 (12 files) .. Type10 (3 files) are the top 10 by count.
        for i in 1..=10 {
            assert_eq!(
                s.type_counts.get(&format!("Type{i:02}")).copied(),
                Some(13 - i),
                "Type{i:02} (rank {i}) should be in the top {TYPE_COUNTS_CAP}"
            );
        }
        // Type11 (2 files) and Type12 (1 file) are the tail — dropped from
        // `type_counts`, but still present in `origin_counts.derived` above,
        // and the frontend folds them into its "Other" remainder.
        assert!(s.type_counts.get("Type11").is_none(), "the tail must be capped off, not zero-valued");
        assert!(s.type_counts.get("Type12").is_none());
    }

    fn vault(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (rel, body) in files {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        d
    }
    fn tmp() -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("index.db");
        (d, p)
    }

    /// 一次重命名的删除与新增落在同一批次里 → 走配对,不重算块。
    #[test]
    fn a_batch_containing_both_ends_of_a_rename_takes_the_fast_path() {
        let v = vault(&[("old.md", "# H\n\nalpha\n")]);
        let (_d, p) = tmp();
        // 注意参数顺序:open_at(vault_root, db_path, globs_stamp)
        let mut idx = SearchIndex::open_at(v.path(), &p, "").unwrap();
        let opts = ScanOptions::default();
        idx.ensure_built(&opts).unwrap();
        std::fs::rename(v.path().join("old.md"), v.path().join("new.md")).unwrap();
        let out = idx.apply_batch(&["old.md".into(), "new.md".into()], &opts).unwrap();
        assert_eq!(out.renamed, 1);
        assert_eq!(out.reindexed, 0);
        assert_eq!(out.removed, 0);
    }

    /// 只看得到一半(跨 debounce 窗口)→ 退回逐个处理,即今天的行为。
    /// 这是降级,不是错误(spec §6)。
    #[test]
    fn a_batch_with_only_the_new_half_falls_back_to_a_reindex() {
        let v = vault(&[("old.md", "# H\n\nalpha\n")]);
        let (_d, p) = tmp();
        // 注意参数顺序:open_at(vault_root, db_path, globs_stamp)
        let mut idx = SearchIndex::open_at(v.path(), &p, "").unwrap();
        let opts = ScanOptions::default();
        idx.ensure_built(&opts).unwrap();
        std::fs::rename(v.path().join("old.md"), v.path().join("new.md")).unwrap();
        let out = idx.apply_batch(&["new.md".into()], &opts).unwrap();
        assert_eq!(out.renamed, 0);
        assert_eq!(out.reindexed, 1);
    }

    /// The cold-start build — the one every launch runs, and the longest scan
    /// this crate performs — must be reportable. It was not: `ensure_built`
    /// had no progress variant at all, so the host's `open_vault` could only
    /// leave its settings page saying "the index is still building" with no
    /// phase, no counts and no elapsed time for the whole (minutes-long, on a
    /// real vault) window.
    #[test]
    fn ensure_built_reports_progress_for_the_build_it_runs() {
        use std::sync::Mutex;
        let d = tempfile::tempdir().unwrap();
        let vault = d.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        for i in 0..5 {
            std::fs::write(vault.join(format!("n{i}.md")), "body\n").unwrap();
        }
        let db = d.path().join("index.db");
        let mut idx = SearchIndex::open_at(&vault, &db, "sync").unwrap();

        let seen: Mutex<Vec<Phase>> = Mutex::new(Vec::new());
        let cb = |p: &Progress| seen.lock().unwrap().push(p.phase);
        idx.ensure_built_with_progress(&ScanOptions::default(), Some(&cb)).unwrap();

        let phases = seen.lock().unwrap().clone();
        assert!(!phases.is_empty(), "a cold build must report something");
        assert_eq!(phases.last(), Some(&Phase::Done), "the terminal phase drives the UI back out of its progress state");

        // Warm path: nothing to do, so nothing to report — a lone phase event
        // with no follow-up would leave a progress bar on screen that never
        // moves.
        seen.lock().unwrap().clear();
        idx.ensure_built_with_progress(&ScanOptions::default(), Some(&cb)).unwrap();
        assert!(seen.lock().unwrap().is_empty(), "an index that already has rows runs no scan and reports none");
    }
}
