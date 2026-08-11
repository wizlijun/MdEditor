//! Cross-platform normalization primitives.
//!
//! These four functions ARE the determinism contract from the design spec §2:
//! the same vault indexed on macOS and on Windows must yield byte-identical
//! `path` values, identical line numbers, and identical content hashes. Every
//! other module goes through here rather than touching `Path` / `\r` directly.

use std::borrow::Cow;
use std::path::Path;

/// Vault-relative, `/`-separated path. `None` when `abs` is not strictly below
/// `vault_root`.
///
/// `Path::strip_prefix` gives us a relative `Path` whose separator is still the
/// platform's, so we re-join the components explicitly. `to_string_lossy` is
/// deliberate: a filename that is not valid UTF-8 still gets indexed under a
/// best-effort name rather than being dropped — the index is a search aid, not
/// an authority on bytes.
pub fn rel_path(vault_root: &Path, abs: &Path) -> Option<String> {
    let rel = abs.strip_prefix(vault_root).ok()?;
    let mut out = String::new();
    for comp in rel.components() {
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(&comp.as_os_str().to_string_lossy());
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Remove every `\r`.
///
/// Not "strip a trailing \r per line": the TypeScript outline parser does a
/// blanket `text.replace(/\r/g, '')` (src/lib/outline/markdown.ts), and the two
/// parsers must agree line-for-line. One rule, both languages, nothing to
/// reason about.
pub fn strip_cr(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        Cow::Owned(text.replace('\r', ""))
    } else {
        Cow::Borrowed(text)
    }
}

/// SHA-256 of the raw file bytes, lowercase hex.
pub fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Byte offset of the start of each line (line 1 first). Text must already have
/// been through [`strip_cr`].
pub fn line_starts(text: &str) -> Vec<usize> {
    let mut v = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

/// 1-based line number for a byte offset. Out-of-range offsets clamp to the
/// last line — callers get a usable anchor instead of a panic.
pub fn line_of(line_starts: &[usize], byte_offset: usize) -> u32 {
    match line_starts.binary_search(&byte_offset) {
        Ok(i) => (i + 1) as u32,
        Err(i) => i.max(1) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rel_path_is_vault_relative_and_slash_separated() {
        let root = Path::new("/Users/x/vault");
        assert_eq!(
            rel_path(root, Path::new("/Users/x/vault/docs/a.md")).as_deref(),
            Some("docs/a.md")
        );
        // 单段
        assert_eq!(rel_path(root, Path::new("/Users/x/vault/a.md")).as_deref(), Some("a.md"));
        // root 本身没有相对路径
        assert_eq!(rel_path(root, root), None);
        // vault 外
        assert_eq!(rel_path(root, Path::new("/Users/x/other/a.md")), None);
    }

    /// 跨平台不变式:Windows 上产生的分隔符必须被规范化成 `/`,否则同一批
    /// fixtures 在两平台索引出的 `path` 不同,`source_ref` 给 agent 的锚也不同。
    #[cfg(windows)]
    #[test]
    fn rel_path_normalizes_backslashes_on_windows() {
        let root = Path::new(r"C:\Users\x\vault");
        assert_eq!(
            rel_path(root, Path::new(r"C:\Users\x\vault\docs\a.md")).as_deref(),
            Some("docs/a.md")
        );
    }

    #[test]
    fn strip_cr_removes_every_carriage_return() {
        assert_eq!(strip_cr("a\r\nb\r\n").as_ref(), "a\nb\n");
        // 孤立的 \r 也剥:与 TS stripCarriageReturns 逐字一致(见 outline/markdown.ts)
        assert_eq!(strip_cr("a\rb").as_ref(), "ab");
        // 无 \r 时零拷贝
        assert!(matches!(strip_cr("plain"), std::borrow::Cow::Borrowed(_)));
    }

    /// hash 对**原始字节**算,不是剥 \r 之后的文本:换行风格变化必须被视为
    /// 文件变化,否则 CRLF↔LF 的改写会让增量索引漏掉这个文件。
    #[test]
    fn content_hash_is_over_raw_bytes() {
        assert_ne!(content_hash(b"a\r\nb"), content_hash(b"a\nb"));
        assert_eq!(content_hash(b"abc").len(), 64);
        assert_eq!(
            content_hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn line_numbers_are_one_based_and_count_by_lf() {
        let text = "one\ntwo\nthree";
        let starts = line_starts(text);
        assert_eq!(line_of(&starts, 0), 1);
        assert_eq!(line_of(&starts, 3), 1);   // 行尾的 \n 之前仍属第 1 行
        assert_eq!(line_of(&starts, 4), 2);
        assert_eq!(line_of(&starts, text.len() - 1), 3);
        // 越界偏移收敛到最后一行,不 panic
        assert_eq!(line_of(&starts, 9999), 3);
    }
}
