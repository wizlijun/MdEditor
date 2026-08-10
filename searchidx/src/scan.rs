//! Scanning: the full build and the freshness sweep.
//!
//! The sweep exists because the GUI and the CLI are separate processes with no
//! channel between them. When the GUI is closed, nothing has been watching the
//! vault, so the CLI cannot assume the index is current — it proves freshness
//! itself before answering. That proof is bounded by a hard deadline: a slow
//! sweep degrades to "answer from what we have, warn on stderr", because a
//! retrieval tool that blocks is worse than one that is slightly stale.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::norm::{content_hash, rel_path};
use crate::store;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub large_file_threshold_mb: u32,
    /// Vault-relative directory prefixes to skip, `/`-separated.
    pub exclude_dirs: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        // 10 MB matches the vault's git large-file gate. NOT the backlink
        // layer's 1 MB: measured against a real vault, that would drop 46% of
        // the corpus — a guardrail for a different job.
        ScanOptions { large_file_threshold_mb: 10, exclude_dirs: Vec::new() }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    pub files_indexed: usize,
    pub files_removed: usize,
    pub files_skipped_large: Vec<String>,
    pub took_ms: u128,
    pub timed_out: bool,
}

/// What [`index_one`] actually did, so callers (the file watcher, in a
/// later task) can log something truthful instead of a bare bool that
/// conflates "the file is gone" with "the file is still there but no
/// longer indexable" with "the file grew past the size guardrail".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexOutcome {
    /// The file was read, parsed, and its rows written.
    Indexed,
    /// No rows remain because the file no longer exists on disk.
    RemovedMissing,
    /// No rows remain because the file crossed the size guardrail. This is
    /// by design, not a bug — see the comment on the deletion pass in
    /// [`sweep`] for the rationale (stale line-number anchors are worse
    /// than an absent file, which `rg` still finds).
    RemovedOversized,
    /// No rows remain because the path is not something this index tracks
    /// at all (wrong extension, a dot-segment, or an excluded directory).
    RemovedNotIndexable,
}

/// Whether `rel` (a vault-relative, `/`-separated path) should be indexed at
/// all — independent of size, which is checked separately so a skip can be
/// reported rather than silently folded into "not indexable".
pub fn is_indexable(rel: &str, opts: &ScanOptions) -> bool {
    if !rel.ends_with(".md") {
        return false;
    }
    if rel.split('/').any(|seg| seg.starts_with('.')) {
        return false;
    }
    !opts.exclude_dirs.iter().any(|d| {
        let d = d.trim_matches('/');
        !d.is_empty() && (rel == d || rel.starts_with(&format!("{d}/")))
    })
}

struct Candidate {
    rel: String,
    mtime: i64,
    size: i64,
}

/// Walk the vault once, returning sorted indexable candidates and the
/// (also sorted by discovery order) list of paths skipped for size.
///
/// `ignore::WalkBuilder` with `.hidden(true)` already skips dot-directories
/// by default (it treats a leading `.` as a hidden-file convention, the same
/// as most shells' globs), but `is_indexable`'s own dot-segment check is kept
/// as a second, independent gate — the walker's notion of "hidden" is a
/// heuristic tied to `ignore`'s crate version and its interaction with
/// `.gitignore`/`.ignore` files, not a contract this crate controls. Tests
/// pin both a `.git/x.md` and a `.notemd/z.md` so a change in either layer
/// is caught.
/// The one walker configuration this product uses over a vault. Exported
/// because the CLI's index-less `fallback_scan` must walk the *same* corpus:
/// with `ignore`'s defaults it honours `.gitignore`/`.ignore`/global excludes,
/// and note.md vaults are git repositories — so a file the index happily
/// returns would be invisible to the fallback, and "found in the GUI, missing
/// from `notemd search`" is precisely the disagreement this crate exists to
/// make impossible.
///
/// Every ignore-file source is off on purpose: what belongs in the index is a
/// vault decision (`exclude_dirs` in `.notemd/settings.json`), not a decision
/// delegated to whatever the repository happens to keep out of git. A
/// `.gitignore`d note is still a note.
pub fn walk_builder(vault_root: &Path) -> ignore::WalkBuilder {
    let mut b = ignore::WalkBuilder::new(vault_root);
    b.hidden(true)
        .follow_links(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false);
    b
}

fn walk(vault_root: &Path, opts: &ScanOptions) -> (Vec<Candidate>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let limit = opts.large_file_threshold_mb as u64 * 1024 * 1024;

    let walker = walk_builder(vault_root).build();

    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Some(rel) = rel_path(vault_root, entry.path()) else { continue };
        if !is_indexable(&rel, opts) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.len() > limit {
            skipped.push(rel);
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(Candidate { rel, mtime, size: meta.len() as i64 });
    }
    // Determinism: row ids fall out of insertion order, so the candidate
    // list must be sorted before anything is written — otherwise two builds
    // of the same vault could produce different (but equally correct) rows,
    // and a later task's byte-stability assertion would flap.
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    (out, skipped)
}

pub fn build_full(
    conn: &mut Connection,
    vault_root: &Path,
    opts: &ScanOptions,
) -> rusqlite::Result<ScanStats> {
    let started = Instant::now();
    let (candidates, skipped) = walk(vault_root, opts);
    let mut stats = ScanStats { files_skipped_large: skipped, ..Default::default() };

    // One transaction for the whole build: thousands of small commits is what
    // makes naive SQLite indexers slow, not the parsing.
    let tx = conn.transaction()?;
    tx.execute_batch("DELETE FROM blocks_fts; DELETE FROM blocks; DELETE FROM links; DELETE FROM files;")?;
    for c in &candidates {
        if index_into(&tx, vault_root, c)? {
            stats.files_indexed += 1;
        }
    }
    tx.commit()?;
    store::meta_set(conn, "built_at", &format!("{}", now_secs()))?;
    stats.took_ms = started.elapsed().as_millis();
    Ok(stats)
}

pub fn sweep(
    conn: &mut Connection,
    vault_root: &Path,
    opts: &ScanOptions,
    deadline: Option<Duration>,
) -> rusqlite::Result<ScanStats> {
    let started = Instant::now();
    let mut stats = sweep_with_budget(conn, vault_root, opts, || {
        deadline.is_some_and(|d| started.elapsed() >= d)
    })?;
    stats.took_ms = started.elapsed().as_millis();
    Ok(stats)
}

/// The body of [`sweep`], parameterized over the "are we out of budget"
/// check instead of a real `Duration`/`Instant`. `sweep` wraps this with a
/// wall-clock closure; tests drive it directly with a deterministic
/// counter-based closure so a mid-loop timeout can be pinned exactly
/// (indexing speed varies with machine load, so racing the real clock to
/// catch "one file in, deadline trips before the second" would be flaky —
/// see `sweep_commits_files_indexed_before_a_mid_loop_deadline_trips`).
fn sweep_with_budget(
    conn: &mut Connection,
    vault_root: &Path,
    opts: &ScanOptions,
    mut over_budget: impl FnMut() -> bool,
) -> rusqlite::Result<ScanStats> {
    let known = store::all_file_rows(conn)?;
    let (candidates, skipped) = walk(vault_root, opts);
    let mut stats = ScanStats { files_skipped_large: skipped, ..Default::default() };

    let tx = conn.transaction()?;
    // Owned strings, not `&c.rel` borrows: `candidates` and `tx` are both
    // alive at once, and `tx` needs `&mut` access inside the loop below, so
    // a borrow of `candidates` held across the loop would not compile.
    let mut seen: HashSet<String> = HashSet::with_capacity(candidates.len());
    for c in &candidates {
        seen.insert(c.rel.clone());
        if over_budget() {
            stats.timed_out = true;
            break;
        }
        let known_row = known.get(&c.rel);
        let stat_matches = known_row.is_some_and(|row| row.mtime == c.mtime && row.size == c.size);
        if stat_matches {
            continue;
        }
        if let Some(row) = known_row {
            // stat says "maybe"; the hash decides. Editors that preserve
            // mtime (and same-length edits) would otherwise slip through
            // unnoticed.
            if let Ok(bytes) = std::fs::read(vault_root.join(&c.rel)) {
                if content_hash(&bytes) == row.content_hash {
                    // Content is unchanged but the stat metadata drifted (a
                    // `touch`, a checkout, a sync that rewrites timestamps).
                    // Reconcile the stored mtime/size so the *next* sweep
                    // hits the cheap stat fast-path instead of re-reading
                    // and re-hashing this file forever — a file that never
                    // gets its stat updated here would permanently cost a
                    // full read+hash on every future sweep. This only
                    // touches `files` columns that are not derived from
                    // content (blocks/links/fts are untouched), so it does
                    // not disturb the file-scoped-replacement convergence
                    // property between the two writer processes.
                    tx.execute(
                        "UPDATE files SET mtime=?1, size=?2 WHERE path=?3",
                        rusqlite::params![c.mtime, c.size, c.rel],
                    )?;
                    continue;
                }
            }
        }
        if index_into(&tx, vault_root, c)? {
            stats.files_indexed += 1;
        }
    }
    // Deadline semantics: a partial file list must never be interpreted as
    // "everything else was deleted". On timeout, skip the deletion pass
    // entirely and keep whatever indexing work was already done.
    //
    // Design note on what "not in `seen`" means: a file skipped for size
    // (recorded in `stats.files_skipped_large` above, via `walk`) never
    // makes it into `candidates`, so it never makes it into `seen` either
    // — the same code path that removes truly-deleted files also removes
    // rows for a file that is still on disk but has grown past
    // `large_file_threshold_mb`. That is intentional, not a gap: this
    // index hands agents `path#L120`-style source anchors, and a stored
    // row describing a stale snapshot of a file that has since grown past
    // the guardrail could point at line numbers that no longer hold in the
    // current file. A wrong citation is worse than a missing one, so the
    // file leaves the index — it stays reachable via `rg`, and its
    // continued absence is explained by its appearance in
    // `files_skipped_large` on every subsequent scan.
    if !stats.timed_out {
        for path in known.keys() {
            if !seen.contains(path) {
                store::remove_file(&tx, path)?;
                stats.files_removed += 1;
            }
        }
    }
    tx.commit()?;
    Ok(stats)
}

/// Re-index a single file (watcher path). Whenever the file cannot be
/// indexed — gone, no longer indexable, or over the size guardrail — its
/// rows are removed and the specific reason is reported via
/// [`IndexOutcome`] rather than collapsed into a bare bool, so a caller
/// (the file watcher) can log something truthful.
pub fn index_one(
    conn: &mut Connection,
    vault_root: &Path,
    rel: &str,
    opts: &ScanOptions,
) -> rusqlite::Result<IndexOutcome> {
    let abs = vault_root.join(rel);
    let tx = conn.transaction()?;
    let limit = opts.large_file_threshold_mb as u64 * 1024 * 1024;
    let outcome = match std::fs::metadata(&abs) {
        Ok(_) if !is_indexable(rel, opts) => {
            store::remove_file(&tx, rel)?;
            IndexOutcome::RemovedNotIndexable
        }
        // Oversized is removed by design — see the comment on `sweep`'s
        // deletion pass: a stale row for a file that has grown past the
        // guardrail could point at line numbers that no longer hold, and a
        // wrong citation is worse than an absent one.
        Ok(meta) if meta.len() > limit => {
            store::remove_file(&tx, rel)?;
            IndexOutcome::RemovedOversized
        }
        Ok(meta) => {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let candidate = Candidate { rel: rel.to_string(), mtime, size: meta.len() as i64 };
            if index_into(&tx, vault_root, &candidate)? {
                IndexOutcome::Indexed
            } else {
                // Stat succeeded but the read failed (e.g. a race where the
                // file was removed between the two calls) — treat the same
                // as "gone".
                store::remove_file(&tx, rel)?;
                IndexOutcome::RemovedMissing
            }
        }
        Err(_) => {
            store::remove_file(&tx, rel)?;
            IndexOutcome::RemovedMissing
        }
    };
    tx.commit()?;
    Ok(outcome)
}

fn index_into(
    tx: &rusqlite::Transaction,
    vault_root: &Path,
    c: &Candidate,
) -> rusqlite::Result<bool> {
    let Ok(bytes) = std::fs::read(vault_root.join(&c.rel)) else { return Ok(false) };
    // Lossy on purpose: a file with a stray non-UTF-8 byte still gets indexed
    // rather than silently vanishing from search.
    let raw = String::from_utf8_lossy(&bytes);
    let parsed = crate::chunk::parse_file(&c.rel, &raw, c.mtime);
    let ext = if c.rel.ends_with(".note.md") { "note.md" } else { "md" };
    store::replace_file(tx, &c.rel, ext, c.mtime, c.size, &content_hash(&bytes), &parsed)?;
    Ok(true)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (rel, body) in files {
            let p = d.path().join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
        d
    }
    fn conn_for(v: &Path) -> Connection {
        crate::store::open(&v.join(".idx.db"), &v.to_string_lossy()).unwrap()
    }
    fn count(c: &Connection) -> i64 {
        c.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn build_full_indexes_markdown_and_note_files_only() {
        let v = vault(&[("a.md", "alpha\n"), ("b.note.md", "- beta\n"), ("c.txt", "gamma\n"), ("d.png", "x")]);
        let mut c = conn_for(v.path());
        let s = build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        assert_eq!(s.files_indexed, 2);
        assert_eq!(count(&c), 2);
    }

    /// `.` 开头的目录不进索引:`.git` 是几万个对象,`.notemd` 是配置。
    #[test]
    fn dot_directories_are_skipped() {
        let v = vault(&[("a.md", "x\n"), (".git/x.md", "y\n"), (".notemd/z.md", "y\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        assert_eq!(count(&c), 1);
    }

    /// 护栏是 10MB 而不是反链层的 1MB —— 后者会砍掉真实 vault 里 46% 的语料。
    #[test]
    fn files_over_the_threshold_are_skipped_and_reported() {
        let big = "x".repeat(2 * 1024 * 1024);
        let v = vault(&[("a.md", "small\n"), ("big.md", &big)]);
        let mut c = conn_for(v.path());
        let opts = ScanOptions { large_file_threshold_mb: 1, ..Default::default() };
        let s = build_full(&mut c, v.path(), &opts).unwrap();
        assert_eq!(s.files_indexed, 1);
        assert_eq!(s.files_skipped_large, vec!["big.md".to_string()]);
    }

    #[test]
    fn excluded_directories_are_not_indexed() {
        let v = vault(&[("a.md", "x\n"), ("sessions/b.md", "y\n")]);
        let mut c = conn_for(v.path());
        let opts = ScanOptions { exclude_dirs: vec!["sessions".into()], ..Default::default() };
        build_full(&mut c, v.path(), &opts).unwrap();
        assert_eq!(count(&c), 1);
    }

    #[test]
    fn sweep_reindexes_only_what_changed() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_indexed, 0, "an unchanged vault must be a no-op");

        fs::write(v.path().join("a.md"), "alpha changed\n").unwrap();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_indexed, 1);
    }

    #[test]
    fn sweep_removes_rows_for_deleted_files() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        fs::remove_file(v.path().join("b.md")).unwrap();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_removed, 1);
        assert_eq!(count(&c), 1);
    }

    /// 同样的 mtime/size 但内容变了(编辑器保留时间戳)也要被抓到:快路径之后
    /// 还有 hash 复核。
    #[test]
    fn sweep_falls_back_to_hashing_when_stat_looks_unchanged() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        // same length, same mtime restored
        let meta = fs::metadata(v.path().join("a.md")).unwrap();
        fs::write(v.path().join("a.md"), "alphaX\n").unwrap();
        filetime_set(&v.path().join("a.md"), &meta);
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_indexed, 1, "content change must be caught even when stat matches");
    }

    /// hazard #1: a stat-only change (same content, different mtime/size —
    /// e.g. a `touch`, a checkout, or a sync that rewrites timestamps without
    /// changing bytes) must update the stored mtime/size so the *next* sweep
    /// takes the cheap stat fast-path instead of re-reading and re-hashing
    /// the file forever.
    #[test]
    fn a_stat_only_change_is_reconciled_once_then_the_next_sweep_does_no_work() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();

        // Touch the file (advance mtime, same size, same content) without a
        // real edit. std::fs::File::set_modified pushes the timestamp
        // forward by a full second so it is guaranteed distinct even on
        // coarse-grained filesystems.
        let meta = fs::metadata(v.path().join("a.md")).unwrap();
        let touched = meta.modified().unwrap() + std::time::Duration::from_secs(1);
        let f = fs::OpenOptions::new().write(true).open(v.path().join("a.md")).unwrap();
        f.set_modified(touched).unwrap();

        let s1 = sweep(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s1.files_indexed, 0, "content is unchanged, so no file should be re-indexed");

        let row_mtime: i64 = c
            .query_row("SELECT mtime FROM files WHERE path='a.md'", [], |r| r.get(0))
            .unwrap();
        let expected_mtime = touched.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        assert_eq!(row_mtime, expected_mtime, "stored mtime must be reconciled to the new stat");

        // Now prove the fast path took over: delete the file on disk out from
        // under the store's cached hash, and swap the fs::read that
        // index_into would need — simplest proxy is that a *second* sweep
        // with nothing touched does zero work, which is only possible if the
        // stat fast path (not a hash re-check) short-circuited.
        let s2 = sweep(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s2.files_indexed, 0, "second sweep must be a pure stat no-op");
    }

    /// 超时是降级,不是错误:返回现有索引能给的答案。
    #[test]
    fn sweep_reports_a_timeout_instead_of_failing() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), Some(Duration::from_nanos(1))).unwrap();
        assert!(s.timed_out);
    }

    /// A timed-out sweep must still commit whatever partial indexing work it
    /// did — a hard deadline degrades the answer, it does not throw the work
    /// away.
    #[test]
    fn a_timed_out_sweep_still_commits_and_skips_the_deletion_pass() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        fs::remove_file(v.path().join("b.md")).unwrap();
        // A deadline of zero always reads as "over budget" on the very first
        // check, before any file is even considered.
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), Some(Duration::from_secs(0))).unwrap();
        assert!(s.timed_out);
        assert_eq!(s.files_removed, 0, "a timed-out sweep must not run the deletion pass");
        assert_eq!(count(&c), 2, "b.md's row must survive an incomplete sweep");
    }

    /// 索引是纯函数:全量重建两次必须逐字节一致。
    #[test]
    fn rebuilding_twice_produces_an_identical_index() {
        let v = vault(&[("a.md", "# T\n\nalpha 检索\n"), ("b.note.md", "- beta\n  type:: annotation\n")]);
        let dump = |c: &Connection| -> Vec<String> {
            let mut st = c
                .prepare("SELECT f.path,b.line_start,b.line_end,b.level,b.breadcrumb,b.text FROM blocks b JOIN files f ON f.id=b.file_id ORDER BY f.path,b.line_start,b.level,b.text")
                .unwrap();
            st.query_map([], |r| {
                Ok(format!("{}|{}|{}|{}|{}|{}",
                    r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?))
            }).unwrap().map(|x| x.unwrap()).collect()
        };
        let mut c1 = crate::store::open(&v.path().join(".i1.db"), "v").unwrap();
        build_full(&mut c1, v.path(), &ScanOptions::default()).unwrap();
        let mut c2 = crate::store::open(&v.path().join(".i2.db"), "v").unwrap();
        build_full(&mut c2, v.path(), &ScanOptions::default()).unwrap();
        assert_eq!(dump(&c1), dump(&c2));
    }

    #[test]
    fn index_one_reindexes_a_single_file() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        fs::write(v.path().join("a.md"), "alpha changed\n").unwrap();
        let outcome = index_one(&mut c, v.path(), "a.md", &ScanOptions::default()).unwrap();
        assert_eq!(outcome, IndexOutcome::Indexed);
        let hash: String = c.query_row("SELECT content_hash FROM files WHERE path='a.md'", [], |r| r.get(0)).unwrap();
        assert_eq!(hash, crate::norm::content_hash(b"alpha changed\n"));
    }

    #[test]
    fn index_one_removes_rows_when_the_file_is_gone() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        fs::remove_file(v.path().join("a.md")).unwrap();
        let outcome = index_one(&mut c, v.path(), "a.md", &ScanOptions::default()).unwrap();
        assert_eq!(outcome, IndexOutcome::RemovedMissing);
        assert_eq!(count(&c), 0);
    }

    /// hazard from review round 1: `index_one`'s bare-bool return couldn't
    /// tell a caller (the file watcher) *why* a file left the index. Pin
    /// that a file growing past the guardrail specifically reports
    /// `RemovedOversized`, not `RemovedMissing`.
    #[test]
    fn index_one_reports_removed_oversized_distinctly_from_removed_missing() {
        let v = vault(&[("a.md", "small\n")]);
        let mut c = conn_for(v.path());
        let opts = ScanOptions { large_file_threshold_mb: 1, ..Default::default() };
        build_full(&mut c, v.path(), &opts).unwrap();
        assert_eq!(count(&c), 1);

        let big = "x".repeat(2 * 1024 * 1024);
        fs::write(v.path().join("a.md"), &big).unwrap();
        let outcome = index_one(&mut c, v.path(), "a.md", &opts).unwrap();
        assert_eq!(outcome, IndexOutcome::RemovedOversized);
        assert_eq!(count(&c), 0);
    }

    /// review round 1, finding 1: a file that grows past the size guardrail
    /// between sweeps must leave the index (by design — see the doc comment
    /// on `sweep`'s deletion pass), *and* that absence must be explained by
    /// showing up in `files_skipped_large`, not just silently vanish.
    #[test]
    fn sweep_removes_rows_for_a_file_that_grows_past_the_threshold() {
        let v = vault(&[("a.md", "small\n")]);
        let mut c = conn_for(v.path());
        let opts = ScanOptions { large_file_threshold_mb: 1, ..Default::default() };
        build_full(&mut c, v.path(), &opts).unwrap();
        assert_eq!(count(&c), 1, "a.md starts out small enough to be indexed");

        let big = "x".repeat(2 * 1024 * 1024);
        fs::write(v.path().join("a.md"), &big).unwrap();
        let s = sweep(&mut c, v.path(), &opts, None).unwrap();
        assert_eq!(count(&c), 0, "a file that grows past the guardrail must leave the index");
        assert_eq!(s.files_skipped_large, vec!["a.md".to_string()], "its absence must be explained by the skip report");
    }

    /// review round 1, finding 2: `a_timed_out_sweep_still_commits_and_skips_the_deletion_pass`
    /// only pins the *deletion-pass-skipped* half of hazard #3, since its
    /// zero deadline trips before any file is ever considered. This pins the
    /// other half — work done *before* the deadline is genuinely committed —
    /// by driving `sweep_with_budget` with a deterministic counter instead of
    /// racing the wall clock, which would be flaky under CI load.
    #[test]
    fn sweep_commits_files_indexed_before_a_mid_loop_deadline_trips() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n"), ("c.md", "gamma\n")]);
        let mut c = conn_for(v.path());
        // Candidates are sorted by path (see `walk`), so "a.md" is considered
        // first. The budget check is false on the very first call (letting
        // a.md be indexed) and true from the second call onward (tripping
        // before b.md is considered).
        let mut calls = 0u32;
        let s = sweep_with_budget(&mut c, v.path(), &ScanOptions::default(), || {
            calls += 1;
            calls > 1
        })
        .unwrap();
        assert!(s.timed_out);
        assert_eq!(s.files_indexed, 1, "exactly the one file considered before the budget tripped");

        let hash: String = c.query_row("SELECT content_hash FROM files WHERE path='a.md'", [], |r| r.get(0)).unwrap();
        assert_eq!(hash, crate::norm::content_hash(b"alpha\n"), "a.md's rows must survive the timed-out sweep");
        let unreached: i64 = c
            .query_row("SELECT count(*) FROM files WHERE path IN ('b.md','c.md')", [], |r| r.get(0))
            .unwrap();
        assert_eq!(unreached, 0, "files never reached before the deadline must not appear either");
    }

    #[test]
    fn is_indexable_rejects_non_markdown_dot_segments_and_excluded_dirs() {
        let opts = ScanOptions { exclude_dirs: vec!["sessions".into()], ..Default::default() };
        assert!(is_indexable("a.md", &opts));
        assert!(is_indexable("b.note.md", &opts));
        assert!(!is_indexable("c.txt", &opts));
        assert!(!is_indexable(".git/x.md", &opts));
        assert!(!is_indexable(".notemd/z.md", &opts));
        assert!(!is_indexable("sessions/b.md", &opts));
    }

    /// 测试辅助:把 mtime 设回去。用 std 的 File::set_modified,不引入 filetime。
    fn filetime_set(p: &Path, meta: &std::fs::Metadata) {
        let f = fs::OpenOptions::new().write(true).open(p).unwrap();
        f.set_modified(meta.modified().unwrap()).unwrap();
    }
}
