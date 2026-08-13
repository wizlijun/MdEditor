//! Rename/move detection: the pairing decision, and nothing else.
//!
//! Renames are already *correct* today — the sweep walks the vault, and any
//! row it didn't walk past gets deleted — but they are expensive: a renamed
//! file is seen as "one deletion plus one brand-new file", so it is re-read,
//! re-chunked, re-tokenized and re-written into FTS even though not one byte
//! changed. Renaming a directory holding 500 transcripts is 500 full rebuilds.
//!
//! Everything needed to recognise a rename is already in the index:
//! `files.content_hash`, `mtime` and `size` are loaded into memory by
//! `store::all_file_rows` on every sweep anyway. See
//! `docs/superpowers/specs/2026-08-13-rename-detection-design.md`.
//!
//! This module holds only the **pure** half of that decision — no file reads,
//! no database — so it can be unit-tested away from both. The hash
//! confirmation that turns a candidate pair into an actual rename lives with
//! its caller in `scan.rs` (`confirm_and_apply`), because it needs the vault
//! root and a transaction.

use std::collections::{HashMap, HashSet};

use crate::scan::chunker_class;

/// A row in `files` that this round did not walk past: the index believes the
/// file exists, the disk says otherwise. Either it was deleted, or it moved.
#[derive(Debug, Clone)]
pub(crate) struct Orphan {
    pub path: String,
    pub size: i64,
    pub mtime: i64,
    pub content_hash: String,
}

/// A path on disk that `files` has no row for. Either it is genuinely new, or
/// it is the far end of a move.
#[derive(Debug, Clone)]
pub(crate) struct NewPath {
    pub rel: String,
    pub size: i64,
    pub mtime: i64,
}

/// Pair new paths against orphans, returning `(index into `news`, index into
/// `orphans`)` for every pair worth confirming.
///
/// **Pre-screen only.** `(size, mtime)` equality costs no I/O — POSIX
/// `rename(2)` and a Finder move both preserve both values — and it lets a
/// genuinely-new file fall through to the existing full-index path without
/// any extra read, since that path was going to read the file regardless.
/// Two distinct files *can* share a size and an mtime (unpack an archive, run
/// a checkout), and mispairing would not merely be slow: it would put "that
/// file's content" under "this file's path", and retrieval hands out
/// `path#Lnnn` anchors off exactly that. So the caller MUST confirm by
/// hashing the bytes; this function deliberately cannot (spec §3.2).
///
/// Two rules are enforced here rather than by the caller:
///
/// - **Same chunker class.** `a.md` and `a.note.md` can be byte-identical and
///   still have to be chunked differently, so a class change is not a rename
///   at all — it is a rebuild (spec §4.2).
/// - **One orphan is claimed at most once.** Two byte-identical new files
///   cannot both come from the same old row; the second takes the full path.
///
/// What is *not* enforced here: a target path already occupied in `files`
/// (two files swapping names). That is invisible to this function — it sees
/// two sets, not the database — and the caller falls back on `rename_file`
/// returning `false` (spec §4.2).
pub(crate) fn pair_candidates(news: &[NewPath], orphans: &[Orphan]) -> Vec<(usize, usize)> {
    let mut by_stat: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, o) in orphans.iter().enumerate() {
        by_stat.entry((o.size, o.mtime)).or_default().push(i);
    }
    let mut claimed: HashSet<usize> = HashSet::new();
    let mut pairs = Vec::new();
    for (ni, n) in news.iter().enumerate() {
        let Some(bucket) = by_stat.get(&(n.size, n.mtime)) else { continue };
        let hit = bucket
            .iter()
            .copied()
            .find(|oi| !claimed.contains(oi) && chunker_class(&orphans[*oi].path) == chunker_class(&n.rel));
        if let Some(oi) = hit {
            claimed.insert(oi);
            pairs.push((ni, oi));
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(rel: &str, size: i64, mtime: i64) -> NewPath {
        NewPath { rel: rel.into(), size, mtime }
    }
    fn o(path: &str, size: i64, mtime: i64, h: &str) -> Orphan {
        Orphan { path: path.into(), size, mtime, content_hash: h.into() }
    }

    #[test]
    fn size_and_mtime_both_must_match() {
        let news = [n("new/a.md", 10, 100)];
        assert_eq!(pair_candidates(&news, &[o("old/a.md", 10, 100, "h")]), vec![(0, 0)]);
        assert!(pair_candidates(&news, &[o("old/a.md", 11, 100, "h")]).is_empty());
        assert!(pair_candidates(&news, &[o("old/a.md", 10, 101, "h")]).is_empty());
    }

    /// 分块器类别不同就不能走快路径 —— 块必须重算(spec §4.2)。
    #[test]
    fn a_chunker_class_change_is_not_a_rename() {
        let news = [n("a.note.md", 10, 100)];
        assert!(pair_candidates(&news, &[o("a.md", 10, 100, "h")]).is_empty());
    }

    /// 一个孤儿只能被认领一次。两个内容相同的新文件里,第二个走全量路径。
    #[test]
    fn one_orphan_is_claimed_at_most_once() {
        let news = [n("x.md", 10, 100), n("y.md", 10, 100)];
        let pairs = pair_candidates(&news, &[o("old.md", 10, 100, "h")]);
        assert_eq!(pairs.len(), 1);
    }

    /// 目标路径被占用的情形不在这里挡 —— 这个函数只看两个集合,不看库。
    /// 互换名字时两条配对都会产出,由调用方按 UNIQUE 约束退回(spec §4.2)。
    #[test]
    fn a_swap_produces_both_pairs_and_is_the_callers_problem() {
        let news = [n("a.md", 10, 100), n("b.md", 20, 200)];
        let orphans = [o("b.md", 10, 100, "h1"), o("a.md", 20, 200, "h2")];
        assert_eq!(pair_candidates(&news, &orphans).len(), 2);
    }

    #[test]
    fn empty_inputs_yield_no_pairs() {
        assert!(pair_candidates(&[], &[o("a.md", 1, 1, "h")]).is_empty());
        assert!(pair_candidates(&[n("a.md", 1, 1)], &[]).is_empty());
    }
}
