//! roots 握手 —— 把错配检测从君子协定升级成服务端判定。
//!
//! 上游 spec 的握手靠 agent 自己 Read `.notemd/vault-id`、自己比对、自己降级;
//! agent 忘了比对,错配就静默发生。探针实测 Cowork 声明 `roots.listChanged`
//! 并主动推送变更 —— 于是 server 能反过来问「你挂载了哪些目录」,自己比对。

use serde_json::{json, Value};
use std::path::PathBuf;

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

/// `file:///a/b` → `/a/b`。非 file: 的 root(极少见)直接跳过。
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    uri.strip_prefix("file://").map(PathBuf::from)
}

/// `None` 表示 client 未声明 roots 能力 ⇒ `Unknown`,回落 agent 自查协议。
/// 空切片表示「声明了,但没挂任何目录」⇒ 同样无从判断,也是 `Unknown`。
pub fn classify(roots: Option<&[String]>, our_id: &str) -> (MountStatus, Option<String>) {
    let Some(roots) = roots else { return (MountStatus::Unknown, None) };
    if roots.is_empty() { return (MountStatus::Unknown, None) }
    for uri in roots {
        let Some(p) = uri_to_path(uri) else { continue };
        let Ok(raw) = std::fs::read_to_string(p.join(".notemd").join("vault-id")) else { continue };
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

    #[test]
    fn matching_root_is_matched() {
        let d = vault_with(ID);
        let uri = format!("file://{}", d.path().display());
        let (st, matched) = classify(Some(&[uri.clone()]), ID);
        assert_eq!(st, MountStatus::Matched);
        assert_eq!(matched.as_deref(), Some(uri.as_str()));
    }

    #[test]
    fn non_matching_roots_are_mismatched() {
        let d = vault_with(OTHER);
        let uri = format!("file://{}", d.path().display());
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
        let uri = format!("file://{}", d.path().display());
        let (st, _) = classify(Some(&[uri]), ID);
        assert_eq!(st, MountStatus::Mismatched);
    }

    #[test]
    fn mismatched_json_carries_actionable_advice() {
        let v = to_json(MountStatus::Mismatched, None);
        assert_eq!(v["status"], "mismatched");
        let advice = v["advice"].as_str().unwrap();
        assert!(advice.contains("do not"), "必须明确告诉 agent 别去解析路径");
    }
}
