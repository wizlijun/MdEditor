//! `.notemd/vault-id` —— vault 身份的唯一读写点。
//!
//! 一次生成永不改变;**重建索引不换 ID**。与 `.notemd/settings.json` 同待遇:
//! 随 git 同步、不按 deviceId 分区 —— 同一个 vault 在多台机器上就是同一个身份,
//! 这正是 MCP 握手能判定「agent 挂载的是不是我这个 vault」的前提。
//!
//! 写这个文件不会引起索引抖动:`search::watch::should_forward` 已排除
//! `.notemd/`(仅放行 `.notemd/analytics/`)。
//!
//! 不引入 `uuid` crate:仓库已有 `rand 0.8`,v4 就是 16 字节随机数打两个标记位。

use std::io;
use std::path::{Path, PathBuf};

fn path_of(vault_root: &Path) -> PathBuf {
    vault_root.join(".notemd").join("vault-id")
}

/// 形如 `3f2504e0-4f89-41d3-9a0c-0305e82c3301` 才算数:长度、连字符位置、
/// 版本位(第 15 个字符)、变体位(第 20 个字符)全部校验。宽松一点就等于
/// 让一次误写永久污染 vault 身份。
fn is_uuid_v4(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 { return false; }
    for (i, c) in b.iter().enumerate() {
        let ok = match i {
            8 | 13 | 18 | 23 => *c == b'-',
            14 => *c == b'4',
            19 => matches!(*c, b'8' | b'9' | b'a' | b'b' | b'A' | b'B'),
            _ => c.is_ascii_hexdigit(),
        };
        if !ok { return false; }
    }
    true
}

fn generate() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10
    let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// 读取,不存在或不合法则创建。幂等。
pub fn ensure(vault_root: &Path) -> io::Result<String> {
    let p = path_of(vault_root);
    if let Ok(raw) = std::fs::read_to_string(&p) {
        let trimmed = raw.trim();
        if is_uuid_v4(trimmed) {
            return Ok(trimmed.to_string());
        }
    }
    let id = generate();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, format!("{id}\n"))?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_once_and_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let a = ensure(dir.path()).unwrap();
        let b = ensure(dir.path()).unwrap();
        assert_eq!(a, b, "vault-id 一次生成永不改变");
        assert_eq!(a.len(), 36, "UUID v4 规范形式");
        assert_eq!(&a[14..15], "4", "版本号必须是 4");
    }

    #[test]
    fn replaces_unparseable_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(dir.path().join(".notemd/vault-id"), "garbage").unwrap();
        let id = ensure(dir.path()).unwrap();
        assert_ne!(id, "garbage");
        assert_eq!(id.len(), 36);
    }

    #[test]
    fn tolerates_surrounding_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        let written = "3f2504e0-4f89-41d3-9a0c-0305e82c3301";
        std::fs::write(
            dir.path().join(".notemd/vault-id"),
            format!("  {written}\n"),
        )
        .unwrap();
        assert_eq!(ensure(dir.path()).unwrap(), written);
    }
}
