//! roots 握手 —— 把错配检测从君子协定升级成服务端判定。
//!
//! 上游 spec 的握手靠 agent 自己 Read `.notemd/vault-id`、自己比对、自己降级;
//! agent 忘了比对,错配就静默发生。探针实测 Cowork 声明 `roots.listChanged`
//! 并主动推送变更 —— 于是 server 能反过来问「你挂载了哪些目录」,自己比对。

use serde_json::{json, Value};
use std::path::PathBuf;
use crate::sotvault::vault_id;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountStatus { Matched, Mismatched, Unknown }

impl MountStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MountStatus::Matched => "matched",
            MountStatus::Mismatched => "mismatched",
            MountStatus::Unknown => "unknown",
        }
    }
}

/// 把 percent-encoded 的字节序列解码为字节。`%20` → 0x20, 其他保持不变。
/// 路径是 OsStr,底层是字节序列,不假定 UTF-8 边界。
fn percent_decode_bytes(encoded: &str) -> Vec<u8> {
    let mut result = Vec::new();
    let bytes = encoded.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    result.push(byte);
                    i += 3;
                    continue;
                }
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    result
}

/// `file:///a/b` → `/a/b`, `file://localhost/a/b` → `/a/b`。
/// A root whose scheme is not `file:`, or whose authority is neither empty
/// nor `localhost`, is not this server's business — skip it and let the
/// caller keep looking at the remaining roots.
/// Percent-encoding (e.g. a space as `%20`) is decoded before matching.
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let after_scheme = uri.strip_prefix("file://")?;

    // `file://[authority]/path`. An empty authority (`file:///path`) and
    // `localhost` both mean "this machine"; anything else is a remote host
    // this server has no business resolving, so it is skipped rather than
    // treated as local.
    const LOCALHOST_PREFIX: &str = "localhost/";
    let path_part = if after_scheme.starts_with('/') {
        after_scheme
    } else if after_scheme.starts_with(LOCALHOST_PREFIX) {
        // Keep the leading '/' that terminates the authority, matching the
        // `file:///path` branch above — both arms hand `path_part` a string
        // starting with '/'.
        &after_scheme[LOCALHOST_PREFIX.len() - 1..]
    } else if let Some(slash_pos) = after_scheme.find('/') {
        let authority = &after_scheme[..slash_pos];
        if !authority.is_empty() {
            return None; // non-localhost authority: remote, not ours
        }
        &after_scheme[slash_pos..]
    } else {
        return None; // no path component at all: malformed
    };

    let decoded_bytes = percent_decode_bytes(path_part);

    // Convert decoded bytes to OsStr in a platform-specific way:
    // - Unix: paths are byte sequences; preserve arbitrary bytes (even non-UTF-8) faithfully
    // - Windows: paths are UTF-16; cannot represent arbitrary byte sequences. Use lossy UTF-8
    //   conversion. A mangled path will simply not match any vault-id and fall through to
    //   the existing skip behaviour, which is the safe outcome.
    #[cfg(unix)]
    {
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(&decoded_bytes)))
    }
    #[cfg(windows)]
    {
        let string = String::from_utf8_lossy(&decoded_bytes).into_owned();
        // `file:///C:/Users/me/vault` decodes above to `/C:/Users/me/vault` —
        // a leading slash in front of a drive letter, which the Windows path
        // parser reads as "rooted but driveless" and never resolves to the
        // real `C:\Users\me\vault`. Strip that one leading slash when what
        // follows is a drive letter (`^[A-Za-z]:`); leave everything else
        // (UNC-shaped or otherwise) alone.
        let stripped = string
            .strip_prefix('/')
            .filter(|rest| {
                let b = rest.as_bytes();
                b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
            })
            .unwrap_or(&string);
        Some(PathBuf::from(stripped))
    }
}

/// `None` 表示 client 未声明 roots 能力 ⇒ `Unknown`,回落 agent 自查协议。
/// 空切片表示「声明了,但没挂任何目录」⇒ 同样无从判断,也是 `Unknown`。
pub fn classify(roots: Option<&[String]>, our_id: &str) -> (MountStatus, Option<String>) {
    let Some(roots) = roots else { return (MountStatus::Unknown, None) };
    if roots.is_empty() { return (MountStatus::Unknown, None) }
    for uri in roots {
        let Some(p) = uri_to_path(uri) else { continue };
        let vault_id_file = vault_id::vault_id_path(&p);
        let Ok(raw) = std::fs::read_to_string(vault_id_file) else { continue };
        if raw.trim() == our_id {
            return (MountStatus::Matched, Some(uri.clone()));
        }
    }
    (MountStatus::Mismatched, None)
}

/// `mismatched` 时**照常返回检索结果**,只是让错配无法被误解。
///
/// 危险的从来不是结果本身(对 server 的 vault 永远是对的),而是 agent 拿
/// `/dailynote/2026/x.note.md` 去自己的挂载点解析、读到同路径的别的文件。
/// 拒绝服务会误伤一类正当用法:agent 只想知道你笔记里有什么,并不打算读原文。
pub fn to_json(status: MountStatus, matched_root: Option<String>) -> Value {
    let advice = match status {
        MountStatus::Matched =>
            "Paths in this response resolve against your mounted vault.",
        MountStatus::Mismatched =>
            "Your mounted folders are NOT this vault — do not resolve these paths against them; \
             a same-named file there is a different file. Use the returned text and breadcrumb, \
             or ask the user to mount the vault.",
        MountStatus::Unknown =>
            "Mount could not be determined. Before resolving paths, read .notemd/vault-id in \
             your mounted folder and compare it with vault_id above.",
    };
    json!({ "status": status.as_str(), "matched_root": matched_root, "advice": advice })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_with(id: &str) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".notemd")).unwrap();
        std::fs::write(d.path().join(".notemd/vault-id"), format!("{id}\n")).unwrap();
        d
    }
    const ID: &str = "3f2504e0-4f89-41d3-9a0c-0305e82c3301";
    const OTHER: &str = "11111111-4111-8111-8111-111111111111";

    /// The URI's path part, always with the leading `/` `uri_to_path` expects
    /// right after the (empty or `localhost`) authority — on unix this is
    /// just `path.display()` (already absolute, already starts with `/`); on
    /// Windows `path.display()` starts with a drive letter (`C:\Users\...`),
    /// so a `/` is prepended and backslashes are normalized to `/`, giving
    /// the well-formed `file:///C:/Users/...` shape a real Windows MCP client
    /// sends (this is the cfg-aware builder the drive-letter fix needs: the
    /// old `format!("file://{}", path.display())` produced the malformed
    /// `file://C:\Users\...` on Windows, which every test here used to build).
    fn uri_path(path: &std::path::Path) -> String {
        #[cfg(unix)]
        {
            path.display().to_string()
        }
        #[cfg(windows)]
        {
            format!("/{}", path.display().to_string().replace('\\', "/"))
        }
    }

    fn file_uri(path: &std::path::Path) -> String {
        format!("file://{}", uri_path(path))
    }

    #[test]
    fn matching_root_is_matched() {
        let d = vault_with(ID);
        let uri = file_uri(d.path());
        let (st, matched) = classify(Some(&[uri.clone()]), ID);
        assert_eq!(st, MountStatus::Matched);
        assert_eq!(matched.as_deref(), Some(uri.as_str()));
    }

    #[test]
    fn non_matching_roots_are_mismatched() {
        let d = vault_with(OTHER);
        let uri = file_uri(d.path());
        let (st, matched) = classify(Some(&[uri]), ID);
        assert_eq!(st, MountStatus::Mismatched);
        assert_eq!(matched, None);
    }

    /// client 没声明 roots ⇒ unknown,回落到 agent 自查协议。
    /// 绝不能因此拒绝服务。
    #[test]
    fn absent_roots_are_unknown() {
        let (st, _) = classify(None, ID);
        assert_eq!(st, MountStatus::Unknown);
    }

    /// 有 roots 但都读不到 vault-id ⇒ 仍是 mismatched,不是 unknown。
    /// unknown 的含义是「无从判断」,这里是「判断了,不匹配」。
    #[test]
    fn roots_without_vault_id_are_mismatched() {
        let d = tempfile::tempdir().unwrap();
        let uri = file_uri(d.path());
        let (st, _) = classify(Some(&[uri]), ID);
        assert_eq!(st, MountStatus::Mismatched);
    }

    /// Windows drive-letter regression: `file:///C:/Users/me/vault` must
    /// resolve to `C:\Users\me\vault`, not the rooted-but-driveless
    /// `\C:\Users\me\vault` the naive "strip one leading slash" parse used to
    /// produce (that path never opens, so a correctly-mounted Windows vault
    /// was reported `Mismatched`). Windows-only: the parsing branch under
    /// test only exists under `cfg(windows)`, and this repo cannot cross-
    /// compile/run for Windows (see the task report) — this pins the
    /// behaviour for whenever it does run there.
    #[cfg(windows)]
    #[test]
    fn windows_drive_letter_uri_is_matched() {
        let d = vault_with(ID);
        let uri = format!("file:///{}", d.path().display().to_string().replace('\\', "/"));
        assert!(uri.starts_with("file:///") && uri[8..].as_bytes()[1] == b':', "fixture must actually look like file:///C:/...: {uri}");
        let (st, matched) = classify(Some(&[uri.clone()]), ID);
        assert_eq!(st, MountStatus::Matched, "a correctly mounted Windows vault must not be Mismatched");
        assert_eq!(matched.as_deref(), Some(uri.as_str()));
    }

    #[test]
    fn mismatched_json_carries_actionable_advice() {
        let v = to_json(MountStatus::Mismatched, None);
        assert_eq!(v["status"], "mismatched");
        let advice = v["advice"].as_str().unwrap();
        assert!(advice.contains("do not"), "必须明确告诉 agent 别去解析路径");
    }

    /// 空切片表示「声明了,但没挂任何目录」⇒ `Unknown`。
    #[test]
    fn empty_roots_slice_is_unknown() {
        let (st, matched) = classify(Some(&[]), ID);
        assert_eq!(st, MountStatus::Unknown);
        assert_eq!(matched, None);
    }

    /// Percent-encoded 路径(例如空格)能正确解析并匹配。
    #[test]
    fn percent_encoded_path_with_space_matches() {
        let _d = vault_with(ID);
        // 建一个带空格的目录进行测试
        let space_parent = tempfile::tempdir().unwrap();
        let space_dir = space_parent.path().join("vault with spaces");
        std::fs::create_dir(&space_dir).unwrap();
        std::fs::create_dir_all(space_dir.join(".notemd")).unwrap();
        std::fs::write(space_dir.join(".notemd/vault-id"), format!("{ID}\n")).unwrap();

        // file:// URI 中空格编码为 %20
        let uri_with_encoded_space = format!("file://{}", uri_path(&space_dir).replace(' ', "%20"));
        let (st, matched) = classify(Some(&[uri_with_encoded_space.clone()]), ID);
        assert_eq!(st, MountStatus::Matched, "percent-encoded path 应该能正确解析并匹配");
        assert_eq!(matched.as_deref(), Some(uri_with_encoded_space.as_str()));
    }

    /// file://localhost/path 形式也应该被正确解析为本地路径。
    #[test]
    fn file_localhost_uri_is_recognized() {
        let d = vault_with(ID);
        let uri = format!("file://localhost{}", uri_path(d.path()));
        let (st, matched) = classify(Some(&[uri.clone()]), ID);
        assert_eq!(st, MountStatus::Matched);
        assert_eq!(matched.as_deref(), Some(uri.as_str()));
    }

    /// 非 localhost 的 authority 应该被跳过(保守地视为远程)。
    #[test]
    fn non_localhost_authority_is_skipped() {
        let d = vault_with(ID);
        // 构造 file://example.com/path
        let uri = format!("file://example.com{}", uri_path(d.path()));
        // 这个 URI 会被跳过,由于没有其他根,结果应该是 Mismatched
        let (st, _) = classify(Some(&[uri]), ID);
        assert_eq!(st, MountStatus::Mismatched, "remote authority 应该被跳过");
    }

    /// 第一个根无法读取,但第二个根匹配 —— 验证 loop 继续而非提前返回。
    #[test]
    fn malformed_first_root_does_not_prevent_later_match() {
        let good = vault_with(ID);
        let bad = tempfile::tempdir().unwrap(); // 无 .notemd/vault-id

        let bad_uri = file_uri(bad.path());
        let good_uri = file_uri(good.path());

        let (st, matched) = classify(Some(&[bad_uri, good_uri.clone()]), ID);
        assert_eq!(st, MountStatus::Matched, "第二个可用的根应该被找到");
        assert_eq!(matched.as_deref(), Some(good_uri.as_str()), "matched_root 应该指向匹配的那个");
    }

    /// 非 file: scheme 的根也应该被跳过,不导致整体失败。
    #[test]
    fn non_file_scheme_does_not_prevent_later_match() {
        let good = vault_with(ID);
        let good_uri = file_uri(good.path());

        // 一个非 file: scheme 的 URI
        let http_uri = "https://example.com/vault".to_string();

        let (st, matched) = classify(Some(&[http_uri, good_uri.clone()]), ID);
        assert_eq!(st, MountStatus::Matched, "非 file: scheme 应该被跳过,继续检查后续根");
        assert_eq!(matched.as_deref(), Some(good_uri.as_str()));
    }
}
