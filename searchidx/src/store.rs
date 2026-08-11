//! Storage: schema, self-healing open, file-granular idempotent replacement.
//!
//! Two processes (the GUI app and the `notemd search` CLI) write this
//! database with no IPC between them. There is no lock protocol and no
//! leader: instead, every write is a *pure function of one file's bytes*
//! applied as a delete-then-insert of that file's rows. Any interleaving of
//! two such writes converges, because both are computing the same answer
//! from the same input. WAL plus a busy timeout is all the coordination
//! that is needed. Preserve that property — a write path that reads the
//! previous state and patches it would break it silently.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, Transaction};

use crate::block::BlockLevel;
use crate::chunk::Parsed;
use crate::origin::Origin;
use crate::tokenize::{tokenize, TOKENIZER_ID};

// v1 -> v2: added `files.origin` (provenance tiering, spec
// `docs/superpowers/specs/2026-08-11-md-origin-tiering-design.md` §3). No
// migration — see the module doc comment: the index is disposable derived
// data, so a version bump means `open` wipes and rebuilds rather than
// ALTERing an old database into shape.
pub const SCHEMA_VERSION: i64 = 2;

const SCHEMA_SQL: &str = r#"
CREATE TABLE files(
  id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL,
  ext TEXT NOT NULL, mtime INTEGER, size INTEGER, content_hash TEXT,
  title TEXT, concept_type TEXT, tags_json TEXT,
  doc_date TEXT, date_inferred INTEGER,
  human_verified INTEGER DEFAULT 0, origin TEXT NOT NULL);
CREATE TABLE blocks(
  id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL REFERENCES files(id),
  line_start INTEGER, line_end INTEGER,
  breadcrumb TEXT, text TEXT, level TEXT,
  is_annotation INTEGER DEFAULT 0, agent_by TEXT);
CREATE INDEX blocks_file ON blocks(file_id);
CREATE VIRTUAL TABLE blocks_fts USING fts5(tok_text, tok_breadcrumb);
CREATE TABLE links(file_id INTEGER, kind TEXT, target TEXT, line INTEGER);
CREATE INDEX links_file ON links(file_id);
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
"#;

pub struct FileRow {
    pub path: String,
    pub mtime: i64,
    pub size: i64,
    pub content_hash: String,
}

/// Open (creating if needed) the index at `db_path`. A wrong schema version,
/// a wrong tokenizer, a changed `sync_dir`, or a file that is *genuinely not
/// a database* is resolved by deleting the file and starting over. There is
/// deliberately no repair path: the index is disposable derived data, and
/// rebuild is always correct while repair logic never fully is.
///
/// What is **not** resolved that way is any *transient* failure — most
/// importantly `SQLITE_BUSY`/`SQLITE_LOCKED` from another process holding the
/// write transaction (a `build_full`, a flood sweep), and plain I/O errors.
/// Those are propagated to the caller, which degrades (the CLI scans files
/// directly; the GUI leaves `IndexHandle` empty and the panel says "not
/// ready"). Wiping on an opaque `Err` was a real data-loss bug: `notemd
/// search` run while the GUI rebuilt would delete `index.db`/`-wal`/`-shm`
/// out from under the live writer — silently, exit code 0 — leaving the
/// writer committing into an orphaned inode and the GUI serving a database
/// nobody else can see until the next relaunch. Deleting the `-wal`/`-shm`
/// of a live WAL connection is additionally SQLite's own documented
/// corruption hazard. See `is_corruption` and
/// `a_concurrent_writer_must_not_cause_open_to_wipe_the_database`.
///
/// Control flow is straight-line, not recursive: `try_open` is called
/// exactly once, and if it reports the file needs wiping (or fails to open
/// at all), at most one wipe and one fresh-schema creation follow — never a
/// retry loop. The wipe's *result* is verified (`wipe` reports whether the
/// file is actually gone) rather than assumed, because `remove_file` can
/// fail silently for reasons outside this process's control — most
/// realistically on Windows, where another process (the GUI or the CLI,
/// whichever isn't this one) holding the file open blocks deletion. If the
/// wipe didn't take, this returns an `Err` instead of looping or — worse —
/// silently continuing to use a database that is still stale: a stale
/// index answering queries under the wrong tokenizer is a wrong-results bug
/// dressed up as a working one, which is worse than failing loudly.
/// `sync_dir` is the vault's currently-configured sync mirror directory name
/// (see `ScanOptions::sync_dir`) — stamped into `meta` and compared exactly
/// like `tokenizer_id` below, because `origin::derive` (rule 5) is a function
/// of it and nothing else re-derives a stored `origin` when only the
/// *setting* changes (see `a_changed_sync_dir_wipes_the_database`).
pub fn open(db_path: &Path, vault_root: &str, sync_dir: &str) -> rusqlite::Result<Connection> {
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match try_open(db_path, vault_root, sync_dir) {
        Ok(Opened::Ready(conn)) => return Ok(conn),
        Ok(Opened::Stale) => {}
        Err(e) if is_corruption(&e) => {}
        // Busy / locked / I/O: transient or environmental, never a reason to
        // destroy the index. Let the caller degrade.
        Err(e) => return Err(e),
    }
    if !wipe(db_path) {
        return Err(rusqlite::Error::InvalidPath(db_path.to_path_buf()));
    }
    create_fresh(db_path, vault_root, sync_dir)
}

/// Is this error "the bytes on disk are not a usable database", as opposed to
/// "somebody else is using it right now" or "the filesystem said no"? Only
/// the former justifies deleting the file. Kept as a named predicate rather
/// than inlined in `open`'s match so the distinction is testable on its own
/// — the alternative (a `SQLITE_BUSY` reaching a wipe) is a data-loss bug,
/// not a degraded-results bug.
fn is_corruption(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(err, _)
            if matches!(
                err.code,
                rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt
            )
    )
}

/// The outcome of a single open-and-inspect attempt against an existing (or
/// newly created) file.
enum Opened {
    /// A connection whose `meta` table (freshly created or pre-existing)
    /// matches the current `SCHEMA_VERSION`/`TOKENIZER_ID`/`sync_dir` — safe
    /// to use.
    Ready(Connection),
    /// The file opened, but its `meta` table disagrees with the current
    /// schema/tokenizer/sync_dir. The caller must wipe the file and start
    /// over; this function does not do that itself, so it never needs to
    /// re-enter.
    Stale,
}

/// Open `db_path` and classify what was found. Never recurses and never
/// wipes anything itself — wiping is the caller's job, done at most once,
/// in `open`.
fn try_open(db_path: &Path, vault_root: &str, sync_dir: &str) -> rusqlite::Result<Opened> {
    let conn = Connection::open(db_path)?;
    set_pragmas(&conn)?;

    let has_meta: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='meta'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)?;

    if !has_meta {
        // No `meta` and nothing else either: a file we just created (or one
        // wiped down to zero bytes). Ours to build on.
        let empty: bool = conn
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0))
            .map(|n| n == 0)?;
        if empty {
            stamp_fresh_schema(&conn, vault_root, sync_dir)?;
            return Ok(Opened::Ready(conn));
        }
        // No `meta` but *some* tables: a half-created index (a crash, a kill,
        // a full disk between two of `SCHEMA_SQL`'s statements), or someone
        // else's database that happens to live at our path. Stamping our
        // schema onto it fails on the first `CREATE TABLE` that already
        // exists — `SQLITE_ERROR`, which is neither `NOTADB` nor `CORRUPT`,
        // so `open` would return `Err` on this and every subsequent attempt:
        // search permanently dead for that vault, with even the panel's
        // Rebuild button unable to recover it (it comes through here too).
        // The index is disposable derived data, so the answer is the same one
        // a stale schema gets: wipe and rebuild.
        drop(conn);
        return Ok(Opened::Stale);
    }

    // `sync_dir` joins `schema_version`/`tokenizer_id` in this check, not the
    // best-effort re-stamp `vault_root` gets below: `origin::derive` (rule 5)
    // is a function of `sync_dir`, and unlike `vault_root` — which nothing
    // reads for correctness — a stale `sync_dir` means every mirrored file's
    // stored `origin` is silently wrong until that file's bytes happen to
    // change. A mismatch here is exactly as disruptive as a tokenizer change
    // (every stored derived value is suspect), so it gets the same answer:
    // wipe and rebuild, not a quiet re-stamp.
    let ok = meta_get(&conn, "schema_version").as_deref() == Some(&SCHEMA_VERSION.to_string())
        && meta_get(&conn, "tokenizer_id").as_deref() == Some(TOKENIZER_ID)
        && meta_get(&conn, "sync_dir").as_deref() == Some(sync_dir);
    if !ok {
        drop(conn);
        return Ok(Opened::Stale);
    }
    // vault_root can change if the same cache slot is reused; stamp it —
    // but only when it actually differs, and best-effort, deliberately NOT
    // `?`. Every caller reaches this line, including read-only ones like
    // `notemd search`, so an unconditional write here is a write on the open
    // path: against a database another process is mid-write on it first
    // waits out the whole `busy_timeout` and then returns `SQLITE_BUSY` —
    // which used to make `open` wipe the live index (see `open`), and even
    // once that is fixed would still cost every reader a five-second stall
    // for a value nothing reads for correctness (`paths::vault_key` already
    // scopes the database per vault). Reads never block in WAL mode, so the
    // compare below is free.
    if meta_get(&conn, "vault_root").as_deref() != Some(vault_root) {
        let _ = meta_set(&conn, "vault_root", vault_root);
    }
    Ok(Opened::Ready(conn))
}

/// Open a file that is known to not exist yet (just created, or just
/// wiped) and build the schema on it. Never called on a file that might
/// already have a `meta` table — `try_open` owns that check.
fn create_fresh(db_path: &Path, vault_root: &str, sync_dir: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path)?;
    set_pragmas(&conn)?;
    stamp_fresh_schema(&conn, vault_root, sync_dir)?;
    Ok(conn)
}

/// Create the schema and stamp `meta`, **atomically**. SQLite runs DDL inside
/// transactions, so one transaction around the whole batch means the file on
/// disk only ever has zero tables or all of them — never the half-built state
/// (`files` created, `meta` not) that a crash, a kill, or a full disk between
/// two auto-committed `CREATE TABLE`s used to leave behind. `try_open` still
/// handles that state defensively, since databases created by older builds
/// are already out there.
fn stamp_fresh_schema(conn: &Connection, vault_root: &str, sync_dir: &str) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute_batch(SCHEMA_SQL)?;
    meta_set(&tx, "schema_version", &SCHEMA_VERSION.to_string())?;
    meta_set(&tx, "tokenizer_id", TOKENIZER_ID)?;
    meta_set(&tx, "vault_root", vault_root)?;
    meta_set(&tx, "sync_dir", sync_dir)?;
    tx.commit()
}

fn set_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    // busy_timeout FIRST, so that every statement on this connection — the
    // journal_mode conversion below included — runs with a grace period
    // rather than SQLite's default of zero. Measured caveat, recorded so
    // nobody re-derives it: SQLite does **not** invoke the busy handler for
    // a journal-mode conversion, so moving this line does not currently
    // change that one statement's behavior (verified: with the timeout set
    // first, converting a rollback-journal file to WAL against a held write
    // transaction still returns `SQLITE_BUSY` immediately). It is ordered
    // this way as defence in depth — any statement added to this function
    // later would otherwise silently inherit a zero timeout.
    conn.pragma_update(None, "busy_timeout", BUSY_TIMEOUT_MS)?;
    // `PRAGMA journal_mode=WAL` returns a row with the resulting mode, so it
    // cannot go through `pragma_update` (which errors on statements that
    // yield results) — read it back via `query_row` instead, which also
    // proves WAL genuinely took effect rather than merely not-erroring.
    let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    debug_assert_eq!(mode.to_lowercase(), "wal");
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

/// How long a statement waits for another process's write transaction before
/// giving up. Two writers (the GUI and `notemd search`) with no IPC between
/// them is the design, so contention is ordinary, not exceptional.
const BUSY_TIMEOUT_MS: i32 = 5000;

/// Delete the database file at `db_path` (best-effort for its `-wal`/`-shm`
/// sidecars — SQLite recreates those on demand, so leaving a stray one
/// behind is harmless) and report whether the *main* file is actually gone
/// afterward. `remove_file`'s own `Result` is not trusted on its own: the
/// caller needs to know the file is really gone, not just that the removal
/// call didn't error, so this re-checks with `Path::exists`.
fn wipe(db_path: &Path) -> bool {
    let _ = std::fs::remove_file(db_path);
    for suffix in ["-wal", "-shm"] {
        let mut p = db_path.as_os_str().to_os_string();
        p.push(suffix);
        let _ = std::fs::remove_file(Path::new(&p));
    }
    !db_path.exists()
}

pub fn meta_get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key=?1", params![key], |r| r.get(0)).ok()
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Delete every row belonging to `rel` and insert the freshly parsed ones.
/// This is the whole coordination protocol between the two writer
/// processes: neither reads the other's prior state, both compute the same
/// rows from the same file bytes, so interleaved delete-then-insert calls
/// converge regardless of order.
#[allow(clippy::too_many_arguments)]
pub fn replace_file(
    tx: &Transaction,
    rel: &str,
    ext: &str,
    mtime: i64,
    size: i64,
    hash: &str,
    parsed: &Parsed,
) -> rusqlite::Result<()> {
    remove_file(tx, rel)?;
    let tags_json = serde_json::to_string(&parsed.meta.tags).unwrap_or_else(|_| "[]".into());
    tx.execute(
        "INSERT INTO files(path,ext,mtime,size,content_hash,title,concept_type,tags_json,doc_date,date_inferred,human_verified,origin)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![
            rel, ext, mtime, size, hash,
            parsed.meta.title, parsed.meta.concept_type, tags_json,
            parsed.meta.doc_date, parsed.meta.date_inferred as i64,
            parsed.meta.human_verified as i64, parsed.meta.origin.as_str()
        ],
    )?;
    let file_id = tx.last_insert_rowid();

    {
        let mut ins_block = tx.prepare_cached(
            "INSERT INTO blocks(file_id,line_start,line_end,breadcrumb,text,level,is_annotation,agent_by)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        )?;
        let mut ins_fts = tx.prepare_cached(
            "INSERT INTO blocks_fts(rowid,tok_text,tok_breadcrumb) VALUES(?1,?2,?3)",
        )?;
        for b in &parsed.blocks {
            ins_block.execute(params![
                file_id, b.line_start, b.line_end, b.breadcrumb, b.text,
                b.level.as_str(), b.is_annotation as i64, b.agent_by
            ])?;
            let block_id = tx.last_insert_rowid();
            ins_fts.execute(params![block_id, tokenize(&b.text), tokenize(&b.breadcrumb)])?;
        }
    }

    {
        let mut ins_link = tx.prepare_cached(
            "INSERT INTO links(file_id,kind,target,line) VALUES(?1,?2,?3,?4)",
        )?;
        for l in &parsed.links {
            ins_link.execute(params![file_id, l.kind, l.target, l.line])?;
        }
    }
    Ok(())
}

pub fn remove_file(tx: &Transaction, rel: &str) -> rusqlite::Result<()> {
    // The FTS table is a standalone (not external-content) table, so its
    // rows must be deleted explicitly by rowid — blocks.id IS
    // blocks_fts.rowid — and that select must run BEFORE the blocks
    // delete below, since it joins through blocks to find the file's rows.
    tx.execute(
        "DELETE FROM blocks_fts WHERE rowid IN
           (SELECT b.id FROM blocks b JOIN files f ON f.id=b.file_id WHERE f.path=?1)",
        params![rel],
    )?;
    tx.execute(
        "DELETE FROM blocks WHERE file_id IN (SELECT id FROM files WHERE path=?1)",
        params![rel],
    )?;
    tx.execute(
        "DELETE FROM links WHERE file_id IN (SELECT id FROM files WHERE path=?1)",
        params![rel],
    )?;
    tx.execute("DELETE FROM files WHERE path=?1", params![rel])?;
    Ok(())
}

pub fn all_file_rows(conn: &Connection) -> rusqlite::Result<HashMap<String, FileRow>> {
    let mut stmt = conn.prepare("SELECT path,mtime,size,content_hash FROM files")?;
    let rows = stmt.query_map([], |r| {
        Ok(FileRow { path: r.get(0)?, mtime: r.get(1)?, size: r.get(2)?, content_hash: r.get(3)? })
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let row = row?;
        out.insert(row.path.clone(), row);
    }
    Ok(out)
}

/// `BlockLevel` round-trip helper used by the query layer (Task 10) to turn
/// a stored `blocks.level` string back into the enum.
pub fn level_of(s: &str) -> BlockLevel {
    BlockLevel::from_str(s)
}

/// Turn a stored `files.origin` value back into an [`Origin`] for a later
/// query/ranking layer. `s` is `None` for a NULL column (a row written by a
/// hypothetical future schema variant that permits it) or an unrecognized
/// string (hand-edited row, a downgrade from a build that wrote a different
/// vocabulary, or plain corruption) — `Origin::from_str` returns `None` for
/// both.
///
/// This is a deliberate, explicit choice, not an incidental `unwrap_or`: an
/// unreadable `origin` is not the kind of inconsistency this crate's
/// self-healing story covers (that story is "the whole store's shape is
/// wrong → wipe and rebuild the whole index", see the module doc comment and
/// `store::open`) — it is one row with one bad column, and neither wiping the
/// entire index nor propagating an error through every caller of `query`/
/// `stats` for that is proportionate. It resolves to `Origin::Derived`, the
/// same conservative middle tier rule 7 of `origin::derive` already assigns
/// to a *known-present but unrecognized* frontmatter `type`: never grant the
/// trust boost reserved for `Human`, and never demote to `Source`'s
/// special-cased raw-material tier. This is a read-side fallback only — the
/// row itself is never rewritten to "fix" it.
pub fn origin_of(s: Option<&str>) -> Origin {
    s.and_then(Origin::from_str).unwrap_or(Origin::Derived)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::parse_file;

    // 2026-08-10T00:00:00Z.
    const MTIME: i64 = 1_786_320_000;

    fn tmp() -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("index.db");
        (d, p)
    }

    #[cfg(unix)]
    fn ino(p: &Path) -> u64 {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p).unwrap().ino()
    }

    fn write(conn: &mut Connection, rel: &str, text: &str) {
        let parsed = parse_file(rel, text, MTIME, "sync");
        let tx = conn.transaction().unwrap();
        replace_file(&tx, rel, "md", 1, text.len() as i64, "h1", &parsed).unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn open_creates_the_schema_and_stamps_meta() {
        let (_d, p) = tmp();
        let conn = open(&p, "/v", "sync").unwrap();
        assert_eq!(meta_get(&conn, "schema_version").as_deref(), Some(SCHEMA_VERSION.to_string().as_str()));
        assert_eq!(meta_get(&conn, "tokenizer_id").as_deref(), Some(crate::tokenize::TOKENIZER_ID));
        assert_eq!(meta_get(&conn, "vault_root").as_deref(), Some("/v"));
    }

    /// 索引是可弃派生物:版本不符不修,直接扔掉重建。自愈最简、没有半修好的库。
    #[test]
    fn a_stale_tokenizer_id_wipes_the_database() {
        let (_d, p) = tmp();
        {
            let mut conn = open(&p, "/v", "sync").unwrap();
            write(&mut conn, "a.md", "hello\n");
            meta_set(&conn, "tokenizer_id", "v0+something-else").unwrap();
        }
        let conn = open(&p, "/v", "sync").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "a tokenizer change must invalidate every stored token");
        assert_eq!(meta_get(&conn, "tokenizer_id").as_deref(), Some(crate::tokenize::TOKENIZER_ID));
    }

    /// review round 1, Important #1: `origin` is a function of `sync_dir`
    /// (rule 5, `origin::derive`), but nothing re-derives it when the vault's
    /// `syncDir` setting changes — the sweep's stat/hash fast path only
    /// touches `mtime`/`size` on an unchanged file, so a stale `origin` would
    /// otherwise survive indefinitely. Mirrors `a_stale_tokenizer_id_wipes_
    /// the_database` exactly: stamp `sync_dir` into `meta`, same as
    /// `tokenizer_id`, and treat a mismatch as `Opened::Stale`.
    #[test]
    fn a_changed_sync_dir_wipes_the_database() {
        let (_d, p) = tmp();
        {
            let mut conn = open(&p, "/v", "sync").unwrap();
            write(&mut conn, "a.md", "hello\n");
        }
        let conn = open(&p, "/v", "box").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "a changed sync_dir must invalidate every stored origin");
        assert_eq!(meta_get(&conn, "sync_dir").as_deref(), Some("box"));
    }

    #[test]
    fn a_stale_schema_version_wipes_the_database() {
        let (_d, p) = tmp();
        {
            let mut conn = open(&p, "/v", "sync").unwrap();
            write(&mut conn, "a.md", "hello\n");
            meta_set(&conn, "schema_version", "0").unwrap();
        }
        let conn = open(&p, "/v", "sync").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    /// The specific transition this task's schema bump depends on: a real
    /// pre-bump (v1, no `origin` column) database must be wiped and rebuilt
    /// on open rather than used as-is with a missing column — see the module
    /// doc comment on `open` and the "Do NOT write a migration" constraint on
    /// this task. Simulated by building a v2 index and then hand-rewriting
    /// `meta.schema_version` back to "1", the way a real pre-bump database
    /// would read.
    #[test]
    fn a_version_1_database_is_wiped_on_open() {
        let (_d, p) = tmp();
        {
            let mut conn = open(&p, "/v", "sync").unwrap();
            write(&mut conn, "a.md", "hello\n");
            meta_set(&conn, "schema_version", "1").unwrap();
        }
        let conn = open(&p, "/v", "sync").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "a v1 (pre-origin-column) database must be wiped, not used with a missing column");
        assert_eq!(meta_get(&conn, "schema_version").as_deref(), Some(SCHEMA_VERSION.to_string().as_str()));
    }

    /// `files.origin` must survive a write/read round trip through the real
    /// column, not just through `Origin::as_str`/`from_str` in isolation.
    /// `a.note.md` derives `Origin::Human` via `origin::derive` rule 1
    /// regardless of frontmatter or `sync_dir`, so this exercises the real
    /// `chunk::parse_file` -> `replace_file` -> SQL round trip end to end.
    #[test]
    fn origin_round_trips_through_the_files_table() {
        let (_d, p) = tmp();
        let mut conn = open(&p, "/v", "sync").unwrap();
        write(&mut conn, "a.note.md", "- x\n");
        let stored: Option<String> = conn
            .query_row("SELECT origin FROM files WHERE path='a.note.md'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored.as_deref(), Some(Origin::Human.as_str()), "stored column must hold the literal tier string");
        assert_eq!(origin_of(stored.as_deref()), Origin::Human, "and it must read back as the same tier");
    }

    /// The read-side fallback for a row `origin::derive` never actually wrote
    /// this way (NULL, a hand-edited row, or a value from a vocabulary this
    /// build no longer recognizes) — see the deliberateness note on
    /// `origin_of` itself. Pinned here, at the store layer, because that
    /// choice only matters once there is a real column to read from.
    #[test]
    fn origin_of_falls_back_to_derived_for_null_or_unrecognized_values() {
        assert_eq!(origin_of(None), Origin::Derived);
        assert_eq!(origin_of(Some("not-a-real-tier")), Origin::Derived);
        assert_eq!(origin_of(Some("")), Origin::Derived);
    }

    /// review round 1, Minor #2: `origin` has exactly one writer
    /// (`replace_file`, which always supplies it), so `origin_of(None)` is
    /// unreachable through this crate's own code today — there is no
    /// migration path to protect, so `NOT NULL` is free and turns a *future*
    /// forgotten-column insert into a loud constraint error at the write
    /// site instead of a silent `Derived` at the read site. `origin_of`'s
    /// NULL-tolerant fallback stays as defense for hand-edited/foreign rows,
    /// per the reviewer's explicit instruction — this test only pins that
    /// the schema itself refuses to manufacture that case.
    #[test]
    fn the_origin_column_rejects_a_null_insert() {
        let (_d, p) = tmp();
        let conn = open(&p, "/v", "sync").unwrap();
        let err = conn
            .execute(
                "INSERT INTO files(path,ext,mtime,size,content_hash) VALUES('x.md','md',0,0,'h')",
                [],
            )
            .expect_err("an insert that omits `origin` (defaulting to NULL) must be rejected by the schema");
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)), "{err:?}");
    }

    #[test]
    fn a_corrupt_database_file_is_replaced_not_reported() {
        let (_d, p) = tmp();
        std::fs::write(&p, b"this is not a sqlite file at all").unwrap();
        let conn = open(&p, "/v", "sync").unwrap();
        assert_eq!(meta_get(&conn, "schema_version").as_deref(), Some(SCHEMA_VERSION.to_string().as_str()));
    }

    /// A database that opens fine, has some of our tables, and no `meta` — the
    /// state a crash between two of `SCHEMA_SQL`'s statements used to leave —
    /// must self-heal like any other unusable index. It briefly did not:
    /// `try_open` stamped the schema onto it, `CREATE TABLE files` failed with
    /// `SQLITE_ERROR` ("table files already exists"), and since that is
    /// neither `NOTADB` nor `CORRUPT` it was returned as an `Err` — on this
    /// open and every one after it. Search dead for that vault until a human
    /// deleted the file by hand; even Rebuild could not fix it, because
    /// Rebuild opens through here too.
    #[test]
    fn a_half_created_database_is_rebuilt_not_failed_forever() {
        let (_d, p) = tmp();
        {
            let conn = Connection::open(&p).unwrap();
            conn.execute_batch("CREATE TABLE files(id INTEGER PRIMARY KEY, path TEXT);").unwrap();
        }
        let conn = open(&p, "/v", "sync").expect("a half-created index must be rebuilt, not reported");
        assert_eq!(meta_get(&conn, "schema_version").as_deref(), Some(SCHEMA_VERSION.to_string().as_str()));
        assert_eq!(meta_get(&conn, "vault_root").as_deref(), Some("/v"));
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        // And it is a real index afterwards, not just an openable file.
        drop(conn);
        let mut conn = open(&p, "/v", "sync").unwrap();
        write(&mut conn, "a.md", "alpha\n");
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    /// The reason the state above should stop being producible in the first
    /// place: schema creation is one transaction, so an interruption can leave
    /// zero tables but never a subset of them.
    #[test]
    fn schema_creation_is_atomic() {
        let (_d, p) = tmp();
        {
            let conn = Connection::open(&p).unwrap();
            set_pragmas(&conn).unwrap();
            // Make the *last* statement of the batch fail, standing in for a
            // crash/ENOSPC part-way through: `meta` already exists, so
            // `CREATE TABLE meta` errors after `files`/`blocks`/… succeeded.
            conn.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);").unwrap();
            assert!(stamp_fresh_schema(&conn, "/v", "sync").is_err());
            let tables: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name='files'", [], |r| r.get(0))
                .unwrap();
            assert_eq!(tables, 0, "a failed schema creation must roll back entirely");
        }
    }

    /// 免 IPC 收敛的数学前提:同一文件重复写入必须收敛到同一状态。
    #[test]
    fn replacing_a_file_is_idempotent() {
        let (_d, p) = tmp();
        let mut conn = open(&p, "/v", "sync").unwrap();
        write(&mut conn, "a.md", "# T\n\nalpha\n");
        let count = |c: &Connection| -> i64 { c.query_row("SELECT count(*) FROM blocks", [], |r| r.get(0)).unwrap() };
        let first = count(&conn);
        write(&mut conn, "a.md", "# T\n\nalpha\n");
        assert_eq!(count(&conn), first, "re-indexing must replace, never append");
        let files: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(files, 1);
    }

    /// FTS 影子行必须跟着块一起走,否则删掉的内容还能被搜出来。
    #[test]
    fn removing_a_file_clears_its_blocks_and_fts_rows() {
        let (_d, p) = tmp();
        let mut conn = open(&p, "/v", "sync").unwrap();
        write(&mut conn, "a.md", "alpha unique-token\n");
        let tx = conn.transaction().unwrap();
        remove_file(&tx, "a.md").unwrap();
        tx.commit().unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM blocks_fts WHERE blocks_fts MATCH '\"unique-token\"'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn all_file_rows_returns_stat_data_for_sweeping() {
        let (_d, p) = tmp();
        let mut conn = open(&p, "/v", "sync").unwrap();
        write(&mut conn, "a.md", "x\n");
        let rows = all_file_rows(&conn).unwrap();
        assert_eq!(rows.get("a.md").unwrap().content_hash, "h1");
    }

    #[test]
    fn wal_and_busy_timeout_are_enabled_for_two_process_access() {
        let (_d, p) = tmp();
        let conn = open(&p, "/v", "sync").unwrap();
        let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        // The timeout is what makes two writers without IPC workable at all.
        // Its *ordering* within `set_pragmas` is not observable (see the note
        // there on journal-mode conversions ignoring the busy handler), so
        // only the value is asserted.
        let busy: i64 = conn.query_row("PRAGMA busy_timeout", [], |r| r.get(0)).unwrap();
        assert_eq!(busy, BUSY_TIMEOUT_MS as i64);
    }

    /// A path that can never become a valid sqlite file (its parent does not
    /// exist and cannot be created because a same-named file is in the way)
    /// must return an `Err`, not loop.
    #[test]
    fn an_unopenable_path_returns_an_error_instead_of_looping_forever() {
        let (_d, root) = tmp();
        // Make `root`'s would-be parent a plain file, so `db_path`'s parent
        // directory can never exist.
        std::fs::write(&root, b"not a directory").unwrap();
        let bogus = root.join("nested").join("index.db");
        assert!(open(&bogus, "/v", "sync").is_err());
    }

    /// `wipe` must report whether the file is actually gone, not merely
    /// whether `remove_file` failed to error — that distinction is what lets
    /// `open`'s stale-database branch be straight-line control flow instead
    /// of a retry loop that assumes deletion succeeded. A directory is a
    /// deterministic way to make `remove_file` fail on every platform,
    /// including as root (unlike a permission-denied/chmod case, which is a
    /// silent no-op when the test runner is root).
    #[test]
    fn wipe_reports_whether_the_file_is_actually_gone() {
        let d = tempfile::tempdir().unwrap();

        let regular = d.path().join("regular.db");
        std::fs::write(&regular, b"x").unwrap();
        assert!(wipe(&regular), "a plain file must be reported removed");
        assert!(!regular.exists());
        assert!(wipe(&regular), "wiping an already-gone path is still success");

        let blocked = d.path().join("blocked.db");
        std::fs::create_dir(&blocked).unwrap();
        assert!(!wipe(&blocked), "a directory cannot be removed by remove_file; wipe must say so");
    }

    /// THE data-loss case. Opening the index while another process holds the
    /// write transaction (an ongoing `build_full`, the watcher's flood sweep,
    /// the panel's Rebuild button) must leave the database exactly where it
    /// is. It used to not: `try_open` stamped `vault_root` on *every* open,
    /// that write returned `SQLITE_BUSY`, and `open`'s catch-all `Err(_)` arm
    /// wiped `index.db`/`-wal`/`-shm` out from under the live writer. Since
    /// `AGENTS.md` now tells every agent to run `notemd search` habitually,
    /// the collision is ordinary, not exotic.
    #[test]
    fn a_concurrent_writer_must_not_cause_open_to_wipe_the_database() {
        let (_d, p) = tmp();
        {
            let mut conn = open(&p, "/v", "sync").unwrap();
            write(&mut conn, "a.md", "alpha\n");
        }
        // Stand in for the other process: hold a real write transaction, the
        // way `build_full` does for the whole duration of a full scan.
        let mut holder = open(&p, "/v", "sync").unwrap();
        let tx = holder
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        meta_set(&tx, "probe", "1").unwrap();

        #[cfg(unix)]
        let ino_before = ino(&p);
        let started = std::time::Instant::now();
        let conn = open(&p, "/v", "sync").expect("a busy writer must not make open fail");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(4),
            "open must not even *wait* on the writer: it has no reason to write"
        );

        // File identity: a wipe replaces the inode, so the same path would
        // resolve to a different file afterwards. Checked where the platform
        // exposes it, alongside the platform-independent proof below.
        #[cfg(unix)]
        assert_eq!(ino(&p), ino_before, "index.db was replaced by a different file");

        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "the live index must survive an open by a second process");

        // And the writer's own transaction must still be able to commit —
        // it would not be, had its file been unlinked underneath it.
        tx.commit().unwrap();
        let n: i64 = holder.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    /// The other half of the same rule, end-to-end: when the open genuinely
    /// *does* fail with a lock error, `open` must hand that error back rather
    /// than "self-heal" by deleting a database that is perfectly fine and in
    /// active use. `locking_mode=EXCLUSIVE` is the deterministic way to force
    /// that error — a plain write transaction no longer blocks the (now
    /// read-only) open path at all, which is the point of the fix above.
    ///
    /// Costs one `busy_timeout` (5s) by construction: the second connection
    /// waits out its full grace period before reporting the failure, which is
    /// exactly the behavior we want in production.
    #[test]
    fn a_lock_error_is_reported_not_self_healed_by_deleting_the_index() {
        let (_d, p) = tmp();
        {
            let mut conn = open(&p, "/v", "sync").unwrap();
            write(&mut conn, "a.md", "alpha\n");
        }
        let holder = open(&p, "/v", "sync").unwrap();
        holder.pragma_update(None, "locking_mode", "EXCLUSIVE").unwrap();
        holder.execute("INSERT INTO meta(key,value) VALUES('probe','1')", []).unwrap();

        let err = open(&p, "/v", "sync").err().expect("an exclusively locked database must not open");
        assert!(!is_corruption(&err), "a lock error is not corruption: {err:?}");
        assert!(p.exists(), "the database file must still be there");

        // Release the lock and prove the rows were never destroyed.
        drop(holder);
        let conn = open(&p, "/v", "sync").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "open must not have wiped a database it merely could not lock");
    }

    /// The classifier behind the arm above. `SQLITE_BUSY`/`SQLITE_LOCKED` and
    /// I/O failures are transient or environmental; only "these bytes are not
    /// a database" earns a wipe.
    #[test]
    fn only_genuine_corruption_justifies_wiping_the_index() {
        let err = |code| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error { code, extended_code: 0 },
                None,
            )
        };
        assert!(is_corruption(&err(rusqlite::ErrorCode::NotADatabase)));
        assert!(is_corruption(&err(rusqlite::ErrorCode::DatabaseCorrupt)));
        assert!(!is_corruption(&err(rusqlite::ErrorCode::DatabaseBusy)));
        assert!(!is_corruption(&err(rusqlite::ErrorCode::DatabaseLocked)));
        assert!(!is_corruption(&err(rusqlite::ErrorCode::CannotOpen)));
        assert!(!is_corruption(&err(rusqlite::ErrorCode::SystemIoFailure)));
        assert!(!is_corruption(&rusqlite::Error::InvalidQuery));
    }

    /// End-to-end version of the same case: when the whole db path is a
    /// directory, `Connection::open` fails immediately, `open`'s
    /// wipe-and-recreate path kicks in, and `wipe` fails too (directories
    /// can't be removed by `remove_file`). `open` must surface that as an
    /// `Err` rather than looping against a target that's still there.
    #[test]
    fn open_returns_an_error_when_the_db_path_cannot_be_wiped() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("index.db");
        std::fs::create_dir(&p).unwrap();
        assert!(open(&p, "/v", "sync").is_err());
    }
}
