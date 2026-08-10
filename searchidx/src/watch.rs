//! Debounce and flood-degradation decisions for the file watcher.
//!
//! Pure and platform-free so it can be tested without a filesystem: macOS
//! (FSEvents) and Windows (ReadDirectoryChangesW) deliver different event
//! sequences for the same rename or delete, and the *policy* — collapse repeats,
//! give up on per-file updates past a threshold — must not depend on which.

use std::collections::HashSet;

/// Matches the backlink layer's existing debounce. Long enough to collapse an
/// editor's save burst, short enough to stay under the 500 ms
/// save-to-searchable budget.
pub const DEBOUNCE_MS: u64 = 300;

/// Past this many distinct files in one window, a full sweep is cheaper than
/// per-file updates — and this is what a `git checkout` or a vault sync looks
/// like.
pub const FLOOD_THRESHOLD: usize = 500;

#[derive(Debug)]
pub enum Batch {
    Files(Vec<String>),
    FullSweep,
}

#[derive(Debug, Default)]
pub struct Pending {
    paths: HashSet<String>,
    flooded: bool,
}

impl Pending {
    pub fn note(&mut self, rel: String) {
        if self.flooded {
            return;
        }
        self.paths.insert(rel);
        if self.paths.len() > FLOOD_THRESHOLD {
            self.flooded = true;
            self.paths.clear();
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.flooded && self.paths.is_empty()
    }

    /// Drain, resetting to the empty state — including the flood flag, so one
    /// burst does not condemn every later batch to a full sweep.
    pub fn take(&mut self) -> Batch {
        if std::mem::take(&mut self.flooded) {
            self.paths.clear();
            return Batch::FullSweep;
        }
        Batch::Files(std::mem::take(&mut self.paths).into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_paths_accumulate_and_drain_once() {
        let mut p = Pending::default();
        p.note("a.md".into());
        p.note("b.md".into());
        match p.take() {
            Batch::Files(mut v) => { v.sort(); assert_eq!(v, vec!["a.md", "b.md"]); }
            other => panic!("{other:?}"),
        }
        assert!(matches!(p.take(), Batch::Files(v) if v.is_empty()));
    }

    /// 同一个文件在去抖窗口里被写十次,只重索引一次 —— 编辑器的自动保存就是这样。
    #[test]
    fn repeated_writes_to_one_file_collapse() {
        let mut p = Pending::default();
        for _ in 0..10 { p.note("a.md".into()); }
        assert!(matches!(p.take(), Batch::Files(v) if v.len() == 1));
    }

    /// 洪峰(git checkout、批量同步)时逐文件更新比全量还慢,直接降级。
    #[test]
    fn a_flood_degrades_to_a_full_sweep() {
        let mut p = Pending::default();
        for i in 0..(FLOOD_THRESHOLD + 1) { p.note(format!("f{i}.md")); }
        assert!(matches!(p.take(), Batch::FullSweep));
        // 降级后状态必须复位,否则下一批永远是 FullSweep。
        p.note("a.md".into());
        assert!(matches!(p.take(), Batch::Files(v) if v == vec!["a.md"]));
    }

    #[test]
    fn the_debounce_window_is_300ms() {
        assert_eq!(DEBOUNCE_MS, 300);
    }
}
