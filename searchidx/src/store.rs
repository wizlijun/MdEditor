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
use crate::tokenize::{tokenize, TOKENIZER_ID};

pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA_SQL: &str = r#"
CREATE TABLE files(
  id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL,
  ext TEXT NOT NULL, mtime INTEGER, size INTEGER, content_hash TEXT,
  title TEXT, concept_type TEXT, tags_json TEXT,
  doc_date TEXT, date_inferred INTEGER,
  human_verified INTEGER DEFAULT 0);
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

/// Open (creating if needed) the index at `db_path`. Anything unexpected —
/// unreadable file, wrong schema version, wrong tokenizer — is resolved by
/// deleting the file and starting over. There is deliberately no repair
/// path: the index is disposable derived data, and rebuild is always
/// correct while repair logic never fully is.
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
pub fn open(db_path: &Path, vault_root: &str) -> rusqlite::Result<Connection> {
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match try_open(db_path, vault_root) {
        Ok(Opened::Ready(conn)) => return Ok(conn),
        Ok(Opened::Stale) | Err(_) => {}
    }
    if !wipe(db_path) {
        return Err(rusqlite::Error::InvalidPath(db_path.to_path_buf()));
    }
    create_fresh(db_path, vault_root)
}

/// The outcome of a single open-and-inspect attempt against an existing (or
/// newly created) file.
enum Opened {
    /// A connection whose `meta` table (freshly created or pre-existing)
    /// matches the current `SCHEMA_VERSION`/`TOKENIZER_ID` — safe to use.
    Ready(Connection),
    /// The file opened, but its `meta` table disagrees with the current
    /// schema/tokenizer. The caller must wipe the file and start over; this
    /// function does not do that itself, so it never needs to re-enter.
    Stale,
}

/// Open `db_path` and classify what was found. Never recurses and never
/// wipes anything itself — wiping is the caller's job, done at most once,
/// in `open`.
fn try_open(db_path: &Path, vault_root: &str) -> rusqlite::Result<Opened> {
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
        stamp_fresh_schema(&conn, vault_root)?;
        return Ok(Opened::Ready(conn));
    }

    let ok = meta_get(&conn, "schema_version").as_deref() == Some(&SCHEMA_VERSION.to_string())
        && meta_get(&conn, "tokenizer_id").as_deref() == Some(TOKENIZER_ID);
    if !ok {
        drop(conn);
        return Ok(Opened::Stale);
    }
    // vault_root can change if the same cache slot is reused; stamp it.
    meta_set(&conn, "vault_root", vault_root)?;
    Ok(Opened::Ready(conn))
}

/// Open a file that is known to not exist yet (just created, or just
/// wiped) and build the schema on it. Never called on a file that might
/// already have a `meta` table — `try_open` owns that check.
fn create_fresh(db_path: &Path, vault_root: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path)?;
    set_pragmas(&conn)?;
    stamp_fresh_schema(&conn, vault_root)?;
    Ok(conn)
}

fn stamp_fresh_schema(conn: &Connection, vault_root: &str) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA_SQL)?;
    meta_set(conn, "schema_version", &SCHEMA_VERSION.to_string())?;
    meta_set(conn, "tokenizer_id", TOKENIZER_ID)?;
    meta_set(conn, "vault_root", vault_root)?;
    Ok(())
}

fn set_pragmas(conn: &Connection) -> rusqlite::Result<()> {
    // `PRAGMA journal_mode=WAL` returns a row with the resulting mode, so it
    // cannot go through `pragma_update` (which errors on statements that
    // yield results) — read it back via `query_row` instead, which also
    // proves WAL genuinely took effect rather than merely not-erroring.
    let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    debug_assert_eq!(mode.to_lowercase(), "wal");
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

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
        "INSERT INTO files(path,ext,mtime,size,content_hash,title,concept_type,tags_json,doc_date,date_inferred,human_verified)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            rel, ext, mtime, size, hash,
            parsed.meta.title, parsed.meta.concept_type, tags_json,
            parsed.meta.doc_date, parsed.meta.date_inferred as i64,
            parsed.meta.human_verified as i64
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

    fn write(conn: &mut Connection, rel: &str, text: &str) {
        let parsed = parse_file(rel, text, MTIME);
        let tx = conn.transaction().unwrap();
        replace_file(&tx, rel, "md", 1, text.len() as i64, "h1", &parsed).unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn open_creates_the_schema_and_stamps_meta() {
        let (_d, p) = tmp();
        let conn = open(&p, "/v").unwrap();
        assert_eq!(meta_get(&conn, "schema_version").as_deref(), Some(SCHEMA_VERSION.to_string().as_str()));
        assert_eq!(meta_get(&conn, "tokenizer_id").as_deref(), Some(crate::tokenize::TOKENIZER_ID));
        assert_eq!(meta_get(&conn, "vault_root").as_deref(), Some("/v"));
    }

    /// 索引是可弃派生物:版本不符不修,直接扔掉重建。自愈最简、没有半修好的库。
    #[test]
    fn a_stale_tokenizer_id_wipes_the_database() {
        let (_d, p) = tmp();
        {
            let mut conn = open(&p, "/v").unwrap();
            write(&mut conn, "a.md", "hello\n");
            meta_set(&conn, "tokenizer_id", "v0+something-else").unwrap();
        }
        let conn = open(&p, "/v").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "a tokenizer change must invalidate every stored token");
        assert_eq!(meta_get(&conn, "tokenizer_id").as_deref(), Some(crate::tokenize::TOKENIZER_ID));
    }

    #[test]
    fn a_stale_schema_version_wipes_the_database() {
        let (_d, p) = tmp();
        {
            let mut conn = open(&p, "/v").unwrap();
            write(&mut conn, "a.md", "hello\n");
            meta_set(&conn, "schema_version", "0").unwrap();
        }
        let conn = open(&p, "/v").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn a_corrupt_database_file_is_replaced_not_reported() {
        let (_d, p) = tmp();
        std::fs::write(&p, b"this is not a sqlite file at all").unwrap();
        let conn = open(&p, "/v").unwrap();
        assert_eq!(meta_get(&conn, "schema_version").as_deref(), Some("1"));
    }

    /// 免 IPC 收敛的数学前提:同一文件重复写入必须收敛到同一状态。
    #[test]
    fn replacing_a_file_is_idempotent() {
        let (_d, p) = tmp();
        let mut conn = open(&p, "/v").unwrap();
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
        let mut conn = open(&p, "/v").unwrap();
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
        let mut conn = open(&p, "/v").unwrap();
        write(&mut conn, "a.md", "x\n");
        let rows = all_file_rows(&conn).unwrap();
        assert_eq!(rows.get("a.md").unwrap().content_hash, "h1");
    }

    #[test]
    fn wal_and_busy_timeout_are_enabled_for_two_process_access() {
        let (_d, p) = tmp();
        let conn = open(&p, "/v").unwrap();
        let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
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
        assert!(open(&bogus, "/v").is_err());
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
        assert!(open(&p, "/v").is_err());
    }
}
