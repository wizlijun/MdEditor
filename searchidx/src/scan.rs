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

use crate::globs::SourceGlobs;
use crate::norm::{content_hash, rel_path};
use crate::store;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub large_file_threshold_mb: u32,
    /// Vault-relative directory prefixes to skip, `/`-separated.
    pub exclude_dirs: Vec<String>,
    /// The vault's configured source-glob patterns (spec `.superpowers/sdd/
    /// 2026-08-12-source-globs-and-transcript-indexing/`, §4.1). Two jobs
    /// share this one field: forwarded verbatim to `origin::derive` (rule
    /// 5′, replacing the retired sync-mirror-directory special case, rule
    /// 5), and consulted by `is_indexable` below to decide whether a
    /// `.srt`/`.vtt`/`.txt` file is in scope at all. `ScanOptions::default()`
    /// carries an empty `SourceGlobs`, which matches nothing — so on
    /// upgrade, before a user has configured anything, no transcript file
    /// is indexed and no `.md` is reclassified as `Source` by this rule.
    ///
    /// `search::options::for_vault` (the single construction point, see
    /// `search_scan_options_contract.rs`) fills this from the vault's real
    /// `searchSourceGlobs` setting as of C-T8 — absent seeds `<syncDir>/**`
    /// from the resolved sync directory, an explicit empty list is
    /// respected and never re-seeded. It is no longer a
    /// `SourceGlobs::default()` stopgap.
    pub source_globs: SourceGlobs,
}

impl Default for ScanOptions {
    fn default() -> Self {
        // 10 MB matches the vault's git large-file gate. NOT the backlink
        // layer's 1 MB: measured against a real vault, that would drop 46% of
        // the corpus — a guardrail for a different job.
        ScanOptions { large_file_threshold_mb: 10, exclude_dirs: Vec::new(), source_globs: SourceGlobs::default() }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    pub files_indexed: usize,
    pub files_removed: usize,
    pub files_skipped_large: Vec<SkippedFile>,
    pub took_ms: u128,
    pub timed_out: bool,
}

/// A file the walk skipped for exceeding `ScanOptions.large_file_threshold_mb`,
/// carrying the actual on-disk size that put it over — not just the path.
/// Callers (`notemd search --stats`, the settings page's skipped-files list)
/// need the size to explain *why* a specific file is invisible to search
/// without making the user go stat it themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    /// Vault-relative, `/`-separated path.
    pub path: String,
    /// On-disk size in bytes at the moment it was skipped.
    pub size: u64,
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
///
/// The extension gate: `.md` is always in scope, unconditionally — it is
/// this product's native format and always has been, glob-configured or
/// not, and its case is never relaxed: a `.md` file is produced in-app or by
/// an agent following this vault's own conventions, so its exact case is
/// ours to keep. `.srt`/`.vtt`/`.txt` (raw transcript formats, C-T4/C-T5)
/// are only in scope *inside* a configured source glob — `opts.source_globs`
/// is "matches nothing" by default (`ScanOptions::default()`), so an
/// unconfigured vault indexes none of them, matching the
/// pre-transcript-support behavior — and their case IS relaxed
/// (`ends_with_ascii_ci`): those three formats arrive from external tooling
/// (subtitle rippers, export utilities) this product does not control, and
/// an uppercase `.SRT` off a ripper is common enough that silently never
/// indexing it — with no diagnostic anywhere, since the settings page's
/// "pattern matches N files" count is satisfied by the `.md` files sitting
/// next to it — would be a real, undiagnosable gap. This asymmetry is a
/// decision, pinned by `uppercase_transcript_extensions_are_indexed_case_
/// insensitively` and `uppercase_md_is_still_not_indexed`, not an oversight.
/// Every other extension is never indexed. The dot-segment and
/// `exclude_dirs` checks below run after the extension gate but exclusion
/// still wins overall — it is the last check, so nothing above it can
/// short-circuit past it.
pub fn is_indexable(rel: &str, opts: &ScanOptions) -> bool {
    let in_scope = if rel.ends_with(".md") {
        true
    } else if ends_with_ascii_ci(rel, ".srt") || ends_with_ascii_ci(rel, ".vtt") || ends_with_ascii_ci(rel, ".txt") {
        opts.source_globs.matches(rel)
    } else {
        false
    };
    if !in_scope {
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

/// ASCII case-insensitive suffix check, used only for the three
/// externally-authored transcript extensions (see `is_indexable`'s doc
/// comment for why `.md` does not go through this). Byte-indexed rather
/// than `to_ascii_lowercase()`-and-compare so it never allocates per call —
/// this runs on every candidate in a vault walk. Slicing `s.as_bytes()` by a
/// fixed byte count from the end cannot land mid-codepoint-and-panic the way
/// slicing a `&str` could, because it produces a `&[u8]`, not a `&str`; a
/// tail that happens to split a multi-byte UTF-8 character just fails
/// `eq_ignore_ascii_case` instead (that comparison only special-cases the
/// ASCII range, so any non-ASCII byte is compared for exact equality and a
/// split sequence's bytes will not equal `suffix`'s ASCII bytes).
pub(crate) fn ends_with_ascii_ci(s: &str, suffix: &str) -> bool {
    let s = s.as_bytes();
    let suffix = suffix.as_bytes();
    s.len() >= suffix.len() && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// The `files.ext` value for a vault-relative path (spec §5.1): `"note.md"`,
/// `"srt"`, `"vtt"`, `"txt"`, or `"md"` (the fallback, matching every file
/// `is_indexable` admits that isn't one of the other four shapes). This is
/// the single place that decision is made — `index_into` calls it rather
/// than recomputing it, so a rename's fast path (which only needs to redo
/// this, not re-chunk) has exactly one place to call too. Case-insensitive
/// for `.srt`/`.vtt`/`.txt` and case-sensitive for `.note.md`/`.md`, via the
/// same `ends_with_ascii_ci` helper `is_indexable` uses — see that function's
/// doc comment for why the asymmetry is deliberate.
pub(crate) fn ext_of(rel: &str) -> &'static str {
    if rel.ends_with(".note.md") {
        "note.md"
    } else if ends_with_ascii_ci(rel, ".srt") {
        "srt"
    } else if ends_with_ascii_ci(rel, ".vtt") {
        "vtt"
    } else if ends_with_ascii_ci(rel, ".txt") {
        "txt"
    } else {
        "md"
    }
}

/// Which chunker `chunk::parse_file` will send a path through. Kept
/// deliberately separate from `ext_of` above (a `.srt` and a `.vtt` share no
/// `ext_of` value but do share a `ChunkerClass`) and used by the rename fast
/// path to decide whether a renamed file's blocks can be kept as-is or must
/// be recomputed: same class, same content ⇒ same blocks; different class ⇒
/// re-chunk even if the bytes on disk didn't change, because `a.md` and
/// `a.note.md` can be byte-identical and still parse completely differently.
///
/// MUST mirror `chunk::parse_file`'s format dispatch exactly, including
/// order — `.note.md` is checked before the `.md` fallback, or every sidecar
/// note would be classified as prose. If that dispatch changes, this must
/// change with it (and vice versa); each carries a comment pointing at the
/// other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChunkerClass {
    /// `.note.md` → `outline::chunk`.
    Outline,
    /// `.srt` / `.vtt` → `transcript::chunk`.
    Transcript,
    /// `.txt` → `plain::chunk`.
    Plain,
    /// Everything else (`.md`) → `prose::chunk`.
    Prose,
}

pub(crate) fn chunker_class(rel: &str) -> ChunkerClass {
    if ends_with_ascii_ci(rel, ".srt") || ends_with_ascii_ci(rel, ".vtt") {
        ChunkerClass::Transcript
    } else if ends_with_ascii_ci(rel, ".txt") {
        ChunkerClass::Plain
    } else if rel.ends_with(".note.md") {
        ChunkerClass::Outline
    } else {
        ChunkerClass::Prose
    }
}

struct Candidate {
    rel: String,
    mtime: i64,
    size: i64,
}

/// The stage of a scan. The UI uses it to decide what to show; the order
/// here is the order phases actually execute in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Walking the vault; the total is not yet known.
    Walking,
    /// Indexing candidates one at a time.
    Indexing,
    /// Removing rows for files that have disappeared (`sweep` only).
    Removing,
    Done,
}

/// A progress snapshot. `current` is a vault-relative path, `/`-separated
/// per this crate's cross-platform convention.
#[derive(Debug, Clone)]
pub struct Progress {
    pub phase: Phase,
    pub done: usize,
    /// Zero during `Walking` — the count is not known until the walk
    /// completes.
    pub total: usize,
    pub current: Option<String>,
    pub elapsed_ms: u128,
}

pub type ProgressFn<'a> = &'a (dyn Fn(&Progress) + Send + Sync);

/// Throttles progress callbacks. A per-file callback on a real vault (8,826
/// files) turns into 8,826 cross-IPC emits and drowns the host's event
/// loop, so callers are notified at most every `every_n` files or `every`
/// duration, whichever comes first — except a phase transition, which is
/// always forced through (`force`), because phases drive the UI's state
/// machine and a dropped one leaves it stuck showing the wrong phase.
struct Throttle {
    every_n: usize,
    every: Duration,
    last_at: Instant,
    last_n: usize,
}

impl Throttle {
    fn new() -> Self {
        Throttle { every_n: 25, every: Duration::from_millis(200), last_at: Instant::now(), last_n: 0 }
    }
    fn should_emit(&mut self, done: usize, force: bool) -> bool {
        if force || done >= self.last_n + self.every_n || self.last_at.elapsed() >= self.every {
            self.last_at = Instant::now();
            self.last_n = done;
            return true;
        }
        false
    }
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

fn walk(vault_root: &Path, opts: &ScanOptions) -> (Vec<Candidate>, Vec<SkippedFile>) {
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
            skipped.push(SkippedFile { path: rel, size: meta.len() });
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
    progress: Option<ProgressFn>,
) -> rusqlite::Result<ScanStats> {
    let started = Instant::now();
    let mut throttle = Throttle::new();
    let report = |phase: Phase, done: usize, total: usize, current: Option<&str>| {
        if let Some(f) = progress {
            f(&Progress {
                phase,
                done,
                total,
                current: current.map(|s| s.to_string()),
                elapsed_ms: started.elapsed().as_millis(),
            });
        }
    };

    report(Phase::Walking, 0, 0, None);
    let (candidates, skipped) = walk(vault_root, opts);
    let total = candidates.len();
    let mut stats = ScanStats { files_skipped_large: skipped, ..Default::default() };

    // One transaction for the whole build: thousands of small commits is what
    // makes naive SQLite indexers slow, not the parsing.
    let tx = conn.transaction()?;
    tx.execute_batch("DELETE FROM blocks_fts; DELETE FROM blocks; DELETE FROM links; DELETE FROM files;")?;
    for (i, c) in candidates.iter().enumerate() {
        if index_into(&tx, vault_root, c, opts)? {
            stats.files_indexed += 1;
        }
        // Force the first callback through so the UI can leave `Walking`
        // for `Indexing` (and learn `total`) immediately, not after the
        // first throttle window elapses.
        if throttle.should_emit(i + 1, i == 0) {
            report(Phase::Indexing, i + 1, total, Some(&c.rel));
        }
    }
    tx.commit()?;
    store::meta_set(conn, "built_at", &format!("{}", now_secs()))?;
    // Only here, not in `sweep`: a full rebuild is the one transaction big
    // enough to leave a WAL the size of the whole index, and it already costs
    // minutes so one checkpoint is noise. Incremental sweeps stay inside
    // SQLite's default `wal_autocheckpoint` (1000 pages ≈ 4 MB), where a
    // TRUNCATE per watcher batch would be pure overhead for no space back.
    store::checkpoint_truncate(conn);
    stats.took_ms = started.elapsed().as_millis();
    report(Phase::Done, total, total, None);
    Ok(stats)
}

pub fn sweep(
    conn: &mut Connection,
    vault_root: &Path,
    opts: &ScanOptions,
    deadline: Option<Duration>,
    progress: Option<ProgressFn>,
) -> rusqlite::Result<ScanStats> {
    let started = Instant::now();
    let mut stats = sweep_with_budget(
        conn,
        vault_root,
        opts,
        || deadline.is_some_and(|d| started.elapsed() >= d),
        progress,
    )?;
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
    progress: Option<ProgressFn>,
) -> rusqlite::Result<ScanStats> {
    let started = Instant::now();
    let mut throttle = Throttle::new();
    let report = |phase: Phase, done: usize, total: usize, current: Option<&str>| {
        if let Some(f) = progress {
            f(&Progress {
                phase,
                done,
                total,
                current: current.map(|s| s.to_string()),
                elapsed_ms: started.elapsed().as_millis(),
            });
        }
    };

    report(Phase::Walking, 0, 0, None);
    let known = store::all_file_rows(conn)?;
    let (candidates, skipped) = walk(vault_root, opts);
    let total = candidates.len();
    let mut stats = ScanStats { files_skipped_large: skipped, ..Default::default() };

    // Announce entering Indexing unconditionally (not throttled, not
    // gated by the budget check below) whenever there is work queued. An
    // already-expired deadline can trip `over_budget()` on the very first
    // candidate, `break`-ing before that iteration's own throttled report
    // ever runs — without this line the whole run would go straight from
    // Walking to Done with no sign that Indexing was ever entered or that
    // `total` candidates were queued for it. This report always carries
    // `done: 0`, so it never overlaps in meaning with the per-candidate
    // reports below, which report only after a candidate is actually
    // considered.
    if total > 0 {
        report(Phase::Indexing, 0, total, None);
    }

    let tx = conn.transaction()?;
    // Owned strings, not `&c.rel` borrows: `candidates` and `tx` are both
    // alive at once, and `tx` needs `&mut` access inside the loop below, so
    // a borrow of `candidates` held across the loop would not compile.
    let mut seen: HashSet<String> = HashSet::with_capacity(candidates.len());
    for (i, c) in candidates.iter().enumerate() {
        seen.insert(c.rel.clone());
        if over_budget() {
            stats.timed_out = true;
            break;
        }
        let known_row = known.get(&c.rel);
        let stat_matches = known_row.is_some_and(|row| row.mtime == c.mtime && row.size == c.size);
        if !stat_matches {
            // stat says "maybe"; the hash decides. Editors that preserve
            // mtime (and same-length edits) would otherwise slip through
            // unnoticed. Only reads the file when there is a known row to
            // compare against — a brand-new file falls straight through to
            // `index_into` below, matching the `stat_matches` fast path.
            let hash_matches = known_row.is_some_and(|row| {
                std::fs::read(vault_root.join(&c.rel))
                    .map(|bytes| content_hash(&bytes) == row.content_hash)
                    .unwrap_or(false)
            });
            if hash_matches {
                // Content is unchanged but the stat metadata drifted (a
                // `touch`, a checkout, a sync that rewrites timestamps).
                // Reconcile the stored mtime/size so the *next* sweep hits
                // the cheap stat fast-path instead of re-reading and
                // re-hashing this file forever — a file that never gets its
                // stat updated here would permanently cost a full
                // read+hash on every future sweep. This only touches
                // `files` columns that are not derived from content
                // (blocks/links/fts are untouched), so it does not disturb
                // the file-scoped-replacement convergence property between
                // the two writer processes.
                tx.execute(
                    "UPDATE files SET mtime=?1, size=?2 WHERE path=?3",
                    rusqlite::params![c.mtime, c.size, c.rel],
                )?;
            } else if index_into(&tx, vault_root, c, opts)? {
                stats.files_indexed += 1;
            }
        }
        // Force the first callback through so the UI can leave `Walking`
        // for `Indexing` (and learn `total`) immediately.
        if throttle.should_emit(i + 1, i == 0) {
            report(Phase::Indexing, i + 1, total, Some(&c.rel));
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
        let to_remove: Vec<&String> = known.keys().filter(|p| !seen.contains(p.as_str())).collect();
        let remove_total = to_remove.len();
        let mut remove_throttle = Throttle::new();
        for (i, path) in to_remove.into_iter().enumerate() {
            store::remove_file(&tx, path)?;
            stats.files_removed += 1;
            // `done` is the count removed so far, not a position in
            // `known` — that is what a progress bar for "deleting stale
            // rows" needs to show.
            if remove_throttle.should_emit(i + 1, i == 0) {
                report(Phase::Removing, i + 1, remove_total, Some(path.as_str()));
            }
        }
    }
    tx.commit()?;
    report(Phase::Done, total, total, None);
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
            if index_into(&tx, vault_root, &candidate, opts)? {
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
    opts: &ScanOptions,
) -> rusqlite::Result<bool> {
    let Ok(bytes) = std::fs::read(vault_root.join(&c.rel)) else { return Ok(false) };
    // Lossy on purpose: a file with a stray non-UTF-8 byte still gets indexed
    // rather than silently vanishing from search.
    let raw = String::from_utf8_lossy(&bytes);
    // `opts.source_globs` reaches `origin::derive` for real (rule 5′) — see
    // `ScanOptions.source_globs`'s doc comment. `search::options::for_vault`
    // (the single construction point) fills this from the vault's real
    // `searchSourceGlobs` setting as of C-T8, so this is real, observed
    // GUI/CLI behavior on a real vault, not a stopgap.
    let parsed = crate::chunk::parse_file(&c.rel, &raw, c.mtime, &opts.source_globs);
    // spec §5.1: `files.ext` must carry the file's real extension, not
    // always "md" — a `.srt`/`.vtt`/`.txt` file only ever reaches this point
    // inside a matching source glob (`is_indexable`'s gate), but once it
    // does, it is a real query-language-observable fact (`ext:srt`) and must
    // not be indistinguishable from a `.md` file. `ext_of` is the one place
    // that decision is encoded, reused here rather than re-decided.
    let ext = ext_of(&c.rel);
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
        crate::store::open(&v.join(".idx.db"), &v.to_string_lossy(), "sync").unwrap()
    }
    fn count(c: &Connection) -> i64 {
        c.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap()
    }

    use std::sync::{Arc, Mutex};

    fn recording() -> (Arc<Mutex<Vec<(Phase, usize, usize)>>>, impl Fn(&Progress) + Send + Sync) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let l = log.clone();
        (log, move |p: &Progress| l.lock().unwrap().push((p.phase, p.done, p.total)))
    }

    /// 阶段切换必须逐个报告 —— UI 靠它决定显示什么,漏一个就会卡在上一阶段。
    #[test]
    fn every_phase_transition_is_reported() {
        let v = vault(&[("a.md", "x\n"), ("b.md", "y\n")]);
        let mut c = conn_for(v.path());
        let (log, cb) = recording();
        build_full(&mut c, v.path(), &ScanOptions::default(), Some(&cb)).unwrap();
        let phases: Vec<Phase> = log.lock().unwrap().iter().map(|e| e.0).collect();
        assert!(phases.first() == Some(&Phase::Walking), "{phases:?}");
        assert!(phases.contains(&Phase::Indexing), "{phases:?}");
        assert_eq!(phases.last(), Some(&Phase::Done), "{phases:?}");
    }

    /// `total` 在 Walking 阶段还不知道(0),扫描完成后必须被填上真实值 ——
    /// 否则进度条永远停在不确定态。
    #[test]
    fn total_is_unknown_while_walking_and_filled_in_afterwards() {
        let v = vault(&[("a.md", "x\n"), ("b.md", "y\n"), ("c.md", "z\n")]);
        let mut c = conn_for(v.path());
        let (log, cb) = recording();
        build_full(&mut c, v.path(), &ScanOptions::default(), Some(&cb)).unwrap();
        let entries = log.lock().unwrap().clone();
        assert_eq!(entries[0], (Phase::Walking, 0, 0));
        assert!(entries.iter().any(|e| e.0 == Phase::Indexing && e.2 == 3), "{entries:?}");
    }

    /// 节流:60 个文件不能产生 60 次回调。8,826 个文件逐个跨 IPC emit 会把
    /// 主线程淹掉,这条测试是那个约束的机器表达。
    #[test]
    fn indexing_callbacks_are_throttled_not_per_file() {
        let files: Vec<(String, String)> =
            (0..60).map(|i| (format!("f{i}.md"), format!("body {i}\n"))).collect();
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let v = vault(&refs);
        let mut c = conn_for(v.path());
        let (log, cb) = recording();
        build_full(&mut c, v.path(), &ScanOptions::default(), Some(&cb)).unwrap();
        let indexing = log.lock().unwrap().iter().filter(|e| e.0 == Phase::Indexing).count();
        assert!(indexing <= 6, "60 个文件产生了 {indexing} 次 Indexing 回调,节流没生效");
        assert!(indexing >= 2, "一次都没节流出来也不对: {indexing}");
    }

    /// 不传回调时行为必须与从前逐字一致(既有调用点全部传 None)。
    #[test]
    fn a_none_callback_changes_nothing() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        let s = build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_indexed, 1);
    }

    /// `sweep` 走三段阶段(Walking → Indexing → Removing → Done)—— 与
    /// `build_full` 同构,但多一个删除阶段;`Removing` 的 `done` 必须是已删除
    /// 的计数,不是遍历位置。
    #[test]
    fn sweep_reports_removing_with_a_running_removed_count() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n"), ("c.md", "gamma\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        fs::remove_file(v.path().join("b.md")).unwrap();
        fs::remove_file(v.path().join("c.md")).unwrap();

        let (log, cb) = recording();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None, Some(&cb)).unwrap();
        assert_eq!(s.files_removed, 2);

        let entries = log.lock().unwrap().clone();
        let removing: Vec<(usize, usize)> =
            entries.iter().filter(|e| e.0 == Phase::Removing).map(|e| (e.1, e.2)).collect();
        assert!(!removing.is_empty(), "{entries:?}");
        // `done` counts must be monotonically increasing and never exceed
        // the true removed count — a walk-position index (as opposed to a
        // removed-so-far count) would let `done` run ahead of `total`.
        // Throttling means the very last removal is not guaranteed to get
        // its own callback (same as `Indexing`'s tail) — the authoritative
        // final count is `ScanStats::files_removed`, asserted above.
        for w in removing.windows(2) {
            assert!(w[1].0 >= w[0].0, "{removing:?}");
        }
        assert!(removing.iter().all(|(done, total)| *done <= *total && *total == 2), "{removing:?}");
        let phases: Vec<Phase> = entries.iter().map(|e| e.0).collect();
        assert!(phases.contains(&Phase::Walking), "{phases:?}");
        assert!(phases.contains(&Phase::Removing), "{phases:?}");
        assert_eq!(phases.last(), Some(&Phase::Done), "{phases:?}");
    }

    /// `sweep` 的旧签名(不传回调)必须继续可用,逐字行为不变。
    #[test]
    fn sweep_with_a_none_callback_changes_nothing() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        fs::write(v.path().join("a.md"), "alpha changed\n").unwrap();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None, None).unwrap();
        assert_eq!(s.files_indexed, 1);
    }

    /// review round 1: an already-expired budget must not let the run jump
    /// straight from `Walking` to `Done` — the caller still needs to learn
    /// that `Indexing` was entered (and that `total` candidates were
    /// queued for it) even though the very first candidate trips
    /// `over_budget()` before any per-candidate work or throttled report
    /// runs.
    #[test]
    fn an_immediately_expired_budget_still_announces_indexing_before_done() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        let (log, cb) = recording();
        let s = sweep_with_budget(&mut c, v.path(), &ScanOptions::default(), || true, Some(&cb)).unwrap();
        assert!(s.timed_out);
        assert_eq!(s.files_indexed, 0, "over_budget trips before any candidate is processed");

        let entries = log.lock().unwrap().clone();
        let phases: Vec<Phase> = entries.iter().map(|e| e.0).collect();
        assert_eq!(phases, vec![Phase::Walking, Phase::Indexing, Phase::Done], "{phases:?}");
        let indexing = entries.iter().find(|e| e.0 == Phase::Indexing).unwrap();
        assert_eq!(*indexing, (Phase::Indexing, 0, 2), "announces the phase with 0 done and the real total, not a per-candidate count");
    }

    #[test]
    fn build_full_indexes_markdown_and_note_files_only() {
        let v = vault(&[("a.md", "alpha\n"), ("b.note.md", "- beta\n"), ("c.txt", "gamma\n"), ("d.png", "x")]);
        let mut c = conn_for(v.path());
        let s = build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_indexed, 2);
        assert_eq!(count(&c), 2);
    }

    /// `.` 开头的目录不进索引:`.git` 是几万个对象,`.notemd` 是配置。
    #[test]
    fn dot_directories_are_skipped() {
        let v = vault(&[("a.md", "x\n"), (".git/x.md", "y\n"), (".notemd/z.md", "y\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(count(&c), 1);
    }

    /// 护栏是 10MB 而不是反链层的 1MB —— 后者会砍掉真实 vault 里 46% 的语料。
    #[test]
    fn files_over_the_threshold_are_skipped_and_reported() {
        let big = "x".repeat(2 * 1024 * 1024);
        let v = vault(&[("a.md", "small\n"), ("big.md", &big)]);
        let mut c = conn_for(v.path());
        let opts = ScanOptions { large_file_threshold_mb: 1, ..Default::default() };
        let s = build_full(&mut c, v.path(), &opts, None).unwrap();
        assert_eq!(s.files_indexed, 1);
        assert_eq!(
            s.files_skipped_large,
            vec![SkippedFile { path: "big.md".to_string(), size: 2 * 1024 * 1024 }]
        );
    }

    /// The skipped report must carry the file's *actual* size, not a rounded
    /// or guessed one — settings-page callers show it verbatim to explain why
    /// a specific file is missing from search. A size that doesn't land on a
    /// round MB boundary pins that nothing along the way truncates/rounds it.
    #[test]
    fn the_skipped_size_is_the_exact_byte_count_not_a_rounded_one() {
        let odd_size = 1_500_037usize; // deliberately not a round MB multiple
        let big = "x".repeat(odd_size);
        let v = vault(&[("big.md", &big)]);
        let mut c = conn_for(v.path());
        let opts = ScanOptions { large_file_threshold_mb: 1, ..Default::default() };
        let s = build_full(&mut c, v.path(), &opts, None).unwrap();
        assert_eq!(s.files_skipped_large.len(), 1);
        assert_eq!(s.files_skipped_large[0].path, "big.md");
        assert_eq!(s.files_skipped_large[0].size, odd_size as u64);
    }

    #[test]
    fn excluded_directories_are_not_indexed() {
        let v = vault(&[("a.md", "x\n"), ("sessions/b.md", "y\n")]);
        let mut c = conn_for(v.path());
        let opts = ScanOptions { exclude_dirs: vec!["sessions".into()], ..Default::default() };
        build_full(&mut c, v.path(), &opts, None).unwrap();
        assert_eq!(count(&c), 1);
    }

    #[test]
    fn sweep_reindexes_only_what_changed() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None, None).unwrap();
        assert_eq!(s.files_indexed, 0, "an unchanged vault must be a no-op");

        fs::write(v.path().join("a.md"), "alpha changed\n").unwrap();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None, None).unwrap();
        assert_eq!(s.files_indexed, 1);
    }

    #[test]
    fn sweep_removes_rows_for_deleted_files() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        fs::remove_file(v.path().join("b.md")).unwrap();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None, None).unwrap();
        assert_eq!(s.files_removed, 1);
        assert_eq!(count(&c), 1);
    }

    /// 同样的 mtime/size 但内容变了(编辑器保留时间戳)也要被抓到:快路径之后
    /// 还有 hash 复核。
    #[test]
    fn sweep_falls_back_to_hashing_when_stat_looks_unchanged() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        // same length, same mtime restored
        let meta = fs::metadata(v.path().join("a.md")).unwrap();
        fs::write(v.path().join("a.md"), "alphaX\n").unwrap();
        filetime_set(&v.path().join("a.md"), &meta);
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None, None).unwrap();
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
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();

        // Touch the file (advance mtime, same size, same content) without a
        // real edit. std::fs::File::set_modified pushes the timestamp
        // forward by a full second so it is guaranteed distinct even on
        // coarse-grained filesystems.
        let meta = fs::metadata(v.path().join("a.md")).unwrap();
        let touched = meta.modified().unwrap() + std::time::Duration::from_secs(1);
        let f = fs::OpenOptions::new().write(true).open(v.path().join("a.md")).unwrap();
        f.set_modified(touched).unwrap();

        let s1 = sweep(&mut c, v.path(), &ScanOptions::default(), None, None).unwrap();
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
        let s2 = sweep(&mut c, v.path(), &ScanOptions::default(), None, None).unwrap();
        assert_eq!(s2.files_indexed, 0, "second sweep must be a pure stat no-op");
    }

    /// 超时是降级,不是错误:返回现有索引能给的答案。
    #[test]
    fn sweep_reports_a_timeout_instead_of_failing() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), Some(Duration::from_nanos(1)), None).unwrap();
        assert!(s.timed_out);
    }

    /// A timed-out sweep must still commit whatever partial indexing work it
    /// did — a hard deadline degrades the answer, it does not throw the work
    /// away.
    #[test]
    fn a_timed_out_sweep_still_commits_and_skips_the_deletion_pass() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        fs::remove_file(v.path().join("b.md")).unwrap();
        // A deadline of zero always reads as "over budget" on the very first
        // check, before any file is even considered.
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), Some(Duration::from_secs(0)), None).unwrap();
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
        let mut c1 = crate::store::open(&v.path().join(".i1.db"), "v", "sync").unwrap();
        build_full(&mut c1, v.path(), &ScanOptions::default(), None).unwrap();
        let mut c2 = crate::store::open(&v.path().join(".i2.db"), "v", "sync").unwrap();
        build_full(&mut c2, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(dump(&c1), dump(&c2));
    }

    #[test]
    fn index_one_reindexes_a_single_file() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
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
        build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
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
        build_full(&mut c, v.path(), &opts, None).unwrap();
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
        build_full(&mut c, v.path(), &opts, None).unwrap();
        assert_eq!(count(&c), 1, "a.md starts out small enough to be indexed");

        let big = "x".repeat(2 * 1024 * 1024);
        fs::write(v.path().join("a.md"), &big).unwrap();
        let s = sweep(&mut c, v.path(), &opts, None, None).unwrap();
        assert_eq!(count(&c), 0, "a file that grows past the guardrail must leave the index");
        assert_eq!(
            s.files_skipped_large,
            vec![SkippedFile { path: "a.md".to_string(), size: 2 * 1024 * 1024 }],
            "its absence must be explained by the skip report"
        );
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
        }, None)
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

    #[test]
    fn md_is_indexed_regardless_of_the_globs() {
        let opts = ScanOptions::default();
        assert!(is_indexable("anywhere/a.md", &opts));
        assert!(is_indexable("b.note.md", &opts));
    }

    #[test]
    fn transcripts_are_indexed_only_inside_a_source_glob() {
        let mut opts = ScanOptions::default();
        opts.source_globs = crate::globs::parse(&["media/**".to_string()]);
        assert!(is_indexable("media/talk.srt", &opts));
        assert!(is_indexable("media/talk.vtt", &opts));
        assert!(is_indexable("media/notes.txt", &opts));
        assert!(!is_indexable("elsewhere/talk.srt", &opts), "模式外的转写不得进索引");
    }

    #[test]
    fn an_unlisted_extension_is_never_indexed_even_inside_a_glob() {
        let mut opts = ScanOptions::default();
        opts.source_globs = crate::globs::parse(&["media/**".to_string()]);
        assert!(!is_indexable("media/a.pdf", &opts));
        assert!(!is_indexable("media/a.mp4", &opts));
    }

    /// `.srt`/`.vtt`/`.txt` arrive from external tooling (subtitle rippers,
    /// export utilities) this product does not control, and uppercase
    /// extensions are common there — a user should not have to rename files
    /// off a ripper just to get them indexed. `.md`, in the next test, is
    /// the deliberate opposite: it is produced in-app or by agents following
    /// this vault's own conventions, so its case is ours to keep exact.
    #[test]
    fn uppercase_transcript_extensions_are_indexed_case_insensitively() {
        let mut opts = ScanOptions::default();
        opts.source_globs = crate::globs::parse(&["media/**".to_string()]);
        assert!(is_indexable("media/Lecture01.SRT", &opts));
        assert!(is_indexable("media/Lecture01.Vtt", &opts));
        assert!(is_indexable("media/notes.TXT", &opts));
    }

    /// Final fix wave, Blocker 3 — THE COMPOSITION of this gate with the
    /// glob matcher, which the test above does not reach: it uses a
    /// directory-shaped `media/**`, where the pattern never looks at the
    /// extension at all, so it is blind to whether an extension FILTER
    /// (`*.srt` — the shape `suggestGlobs` emits as rung 2, and the shape a
    /// user types when a directory holds more than transcripts) agrees with
    /// this gate about case. Before the fix it did not: this gate admitted
    /// `B.SRT` while `media/**/*.srt` refused to designate it, so the
    /// directory indexed 1 of 3 files and §7.2's zero-match warning could
    /// not fire (the count was 1, not 0) — reopening precisely the
    /// undiagnosable gap `ends_with_ascii_ci` exists to close.
    #[test]
    fn an_extension_filter_glob_and_this_gate_agree_about_case() {
        let mut opts = ScanOptions::default();
        for pattern in ["media/**/*.srt", "media/**/*.SRT"] {
            opts.source_globs = crate::globs::parse(&[pattern.to_string()]);
            for path in ["media/s1/a.srt", "media/s1/B.SRT", "media/s1/c.Srt"] {
                assert!(is_indexable(path, &opts), "{pattern} 必须收录 {path}");
            }
            assert!(!is_indexable("media/s1/a.txt", &opts), "{pattern} 只框住 .srt");
        }
    }

    /// The asymmetry with the test above is a decision, not an oversight:
    /// `.md` case is never relaxed.
    #[test]
    fn uppercase_md_is_still_not_indexed() {
        let opts = ScanOptions::default();
        assert!(!is_indexable("a.MD", &opts));
        assert!(!is_indexable("a.Md", &opts));
    }

    /// 排除优先于收录。
    #[test]
    fn exclude_dirs_win_over_a_source_glob() {
        let mut opts = ScanOptions::default();
        opts.source_globs = crate::globs::parse(&["media/**".to_string()]);
        opts.exclude_dirs = vec!["media/raw".to_string()];
        assert!(!is_indexable("media/raw/a.srt", &opts));
    }

    /// A `.srt` indexed under a wide glob must lose its row, not be
    /// stranded, once the user narrows the pattern so it no longer matches
    /// — the same "no partial file must masquerade as a real one" property
    /// `sweep_removes_rows_for_deleted_files` and
    /// `sweep_removes_rows_for_a_file_that_grows_past_the_threshold` pin for
    /// the other two ways a file can stop being indexable. `walk()` filters
    /// candidates through `is_indexable` before they ever reach `seen`, so
    /// the deletion pass below (`known.keys().filter(|p| !seen.contains(..))`)
    /// removes it through the same generic path as a deleted file — this
    /// test exists to prove that generic path actually reaches this new
    /// third case, not just the two it was written against.
    #[test]
    fn sweep_removes_a_transcript_that_falls_outside_a_narrowed_glob() {
        let v = vault(&[("media/talk.srt", "1\n00:00:00,000 --> 00:00:01,000\nhello\n")]);
        let mut c = conn_for(v.path());
        let wide = ScanOptions { source_globs: crate::globs::parse(&["media/**".to_string()]), ..Default::default() };
        build_full(&mut c, v.path(), &wide, None).unwrap();
        assert_eq!(count(&c), 1, "the transcript must be indexed under the wide glob");

        let narrow =
            ScanOptions { source_globs: crate::globs::parse(&["media/other/**".to_string()]), ..Default::default() };
        let s = sweep(&mut c, v.path(), &narrow, None, None).unwrap();
        assert_eq!(s.files_removed, 1, "narrowing the glob must remove the now-out-of-scope row");
        assert_eq!(count(&c), 0, "no stranded row may remain");
    }

    /// 测试辅助:把 mtime 设回去。用 std 的 File::set_modified,不引入 filetime。
    fn filetime_set(p: &Path, meta: &std::fs::Metadata) {
        let f = fs::OpenOptions::new().write(true).open(p).unwrap();
        f.set_modified(meta.modified().unwrap()).unwrap();
    }

    #[test]
    fn ext_of_covers_every_indexable_shape() {
        assert_eq!(ext_of("a/b.note.md"), "note.md");
        assert_eq!(ext_of("a/b.md"), "md");
        assert_eq!(ext_of("a/b.SRT"), "srt", "大小写不敏感,与 is_indexable 同一决定");
        assert_eq!(ext_of("a/b.vtt"), "vtt");
        assert_eq!(ext_of("a/b.TXT"), "txt");
    }

    /// 快路径的前提。`a.md` 与 `a.note.md` 内容可以一字不差,但走的是不同
    /// 的分块器 —— 块必须重算,不能只换路径。
    #[test]
    fn chunker_class_separates_the_four_dispatch_targets() {
        assert_eq!(chunker_class("a.note.md"), ChunkerClass::Outline);
        assert_eq!(chunker_class("a.md"), ChunkerClass::Prose);
        assert_eq!(chunker_class("a.srt"), ChunkerClass::Transcript);
        assert_eq!(chunker_class("a.VTT"), ChunkerClass::Transcript);
        assert_eq!(chunker_class("a.txt"), ChunkerClass::Plain);
        assert_ne!(chunker_class("a.md"), chunker_class("a.note.md"));
    }
}
