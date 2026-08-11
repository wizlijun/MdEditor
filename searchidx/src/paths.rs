//! Where the index lives.
//!
//! Outside the vault, in the machine's LOCAL app data — `dirs::data_local_dir()`,
//! never `data_dir()`. On macOS the two happen to be the same directory, which is
//! precisely why the distinction is easy to get wrong: on Windows `data_dir()` is
//! Roaming AppData, so a domain user's index would follow them to another machine
//! and describe files that are not there. The index belongs to a machine.
//!
//! Both the GUI and the CLI call THIS function. Not "two implementations that
//! agree" — one implementation, so there is nothing to drift.

use std::path::{Path, PathBuf};

pub const BUNDLE_ID: &str = "net.notemd.app";

/// The one spelling of a vault path: `/`-separated, no trailing slash.
///
/// Everything that turns a vault root into a *string* must come through here.
/// Two near-copies of this normalization already drifted once: `vault_key`
/// trimmed the trailing slash and `SearchIndex::open_at`'s `meta.vault_root`
/// stamp did not, so `~/vault` and `~/vault/` — the second is what both bash
/// and zsh produce when you tab-complete a directory into `--vault` — resolved
/// to the *same* `index.db` while stamping *different* strings. The equality
/// check that makes that stamp conditional was then permanently false, so
/// every open wrote, alternating forever: a five-second `busy_timeout` stall
/// on every read against a live writer, which is precisely the cost the
/// conditional exists to avoid.
pub fn normalized_vault_root(vault_root: &Path) -> String {
    let norm = vault_root.to_string_lossy().replace('\\', "/");
    norm.trim_end_matches('/').to_string()
}

/// Stable per-vault cache key: first 16 hex chars of the SHA-256 of the
/// normalized vault path.
pub fn vault_key(vault_root: &Path) -> String {
    crate::norm::content_hash(normalized_vault_root(vault_root).as_bytes())[..16].to_string()
}

/// `<local app data>/net.notemd.app/search/<vault_key>/index.db`.
/// `None` only when the platform has no local data directory at all.
pub fn index_db_path(vault_root: &Path) -> Option<PathBuf> {
    Some(
        dirs::data_local_dir()?
            .join(BUNDLE_ID)
            .join("search")
            .join(vault_key(vault_root))
            .join("index.db"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 同一个 vault 必须永远算出同一个 key,且 key 只依赖规范化后的路径字符串
    /// —— 否则 GUI 与 CLI 会各开一个库,CLI 永远查不到 GUI 刚索引的内容。
    #[test]
    fn vault_key_is_stable_and_slash_normalized() {
        let a = vault_key(Path::new("/Users/x/vault"));
        assert_eq!(a, vault_key(Path::new("/Users/x/vault/")));
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, vault_key(Path::new("/Users/x/other")));
    }

    /// The normalizer is the single source both the cache key and the
    /// `meta.vault_root` stamp read from, so its contract is asserted directly
    /// rather than only through them.
    #[test]
    fn normalized_vault_root_is_slash_separated_and_slash_free_at_the_end() {
        assert_eq!(normalized_vault_root(Path::new("/Users/x/vault")), "/Users/x/vault");
        assert_eq!(normalized_vault_root(Path::new("/Users/x/vault/")), "/Users/x/vault");
        assert_eq!(normalized_vault_root(Path::new("/Users/x/vault///")), "/Users/x/vault");
        assert_eq!(normalized_vault_root(Path::new(r"C:\Users\x\vault\")), "C:/Users/x/vault");
        // A root path is all trailing slash; trimming it to "" is fine (it is
        // only ever hashed or compared), but it must not panic.
        assert_eq!(normalized_vault_root(Path::new("/")), "");
    }

    #[test]
    fn db_path_is_under_the_local_app_data_dir_for_this_bundle() {
        let p = index_db_path(Path::new("/Users/x/vault")).unwrap();
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(s.contains(BUNDLE_ID), "{s}");
        assert!(s.ends_with(&format!("search/{}/index.db", vault_key(Path::new("/Users/x/vault")))), "{s}");
        assert!(s.starts_with(&dirs::data_local_dir().unwrap().to_string_lossy().replace('\\', "/")), "{s}");
    }

    /// Windows 上索引必须落在 Local,不是 Roaming:索引属于机器,漫游到另一台
    /// 机器上的是一份指向不存在文件的陈旧库。macOS 上两者恰好同路径,正是这一点
    /// 长期掩盖了这个坑,所以这条断言只在 Windows 上才有意义 —— 也只在那里跑。
    #[cfg(windows)]
    #[test]
    fn on_windows_the_index_lives_in_local_appdata_not_roaming() {
        let p = index_db_path(Path::new(r"C:\vault")).unwrap().to_string_lossy().to_lowercase();
        assert!(p.contains(r"\local\"), "{p}");
        let roaming = dirs::data_dir().unwrap().to_string_lossy().to_lowercase();
        assert!(!p.starts_with(&roaming), "index must not roam: {p}");
    }
}
