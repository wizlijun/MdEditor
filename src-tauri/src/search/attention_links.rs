//! `MirrorMeta` → `searchidx::attention::MirrorLink` 的唯一适配器。
//!
//! `searchidx` 刻意不认识 `MirrorMeta`:那个格式归 `sotvault` 所有,两个
//! crate 各解析一遍意味着格式一改就有一边静默错掉(而错的方向是「注意力
//! 归错文件」,没有任何症状)。所以镜像记录由命令层读出后传进去。

use std::path::Path;

use searchidx::attention::MirrorLink;

use crate::sotvault::mirror_meta::{self, MirrorMeta};

pub fn links_for_vault(vault_root: &Path) -> Vec<MirrorLink> {
    to_links(mirror_meta::read_all(vault_root))
}

fn to_links(metas: Vec<MirrorMeta>) -> Vec<MirrorLink> {
    metas
        .into_iter()
        .map(|m| MirrorLink { device_id: m.device_id, source: m.source, mirror: m.mirror })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `MirrorMeta` 的三个字段原样落到 `MirrorLink`。这个适配器是格式知识
    /// 的唯一跨 crate 出口 —— `searchidx` 刻意不认识 `MirrorMeta`,免得
    /// 格式一改就有一边静默错掉。
    #[test]
    fn mirror_metas_map_field_for_field() {
        let metas = vec![crate::sotvault::mirror_meta::MirrorMeta {
            mirror: "sync/x.md".into(),
            device_id: "DEV-1".into(),
            device_name: "mac".into(),
            source: "/Users/bruce/x.md".into(),
            synced_at: 0,
            checksum: "sha256:0".into(),
        }];
        let links = to_links(metas);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].mirror, "sync/x.md");
        assert_eq!(links[0].device_id, "DEV-1");
        assert_eq!(links[0].source, "/Users/bruce/x.md");
    }

    /// 没有 .notemd/mirrors 的 vault 给空列表,不报错。
    #[test]
    fn a_vault_without_mirrors_yields_no_links() {
        let d = tempfile::tempdir().unwrap();
        assert!(links_for_vault(d.path()).is_empty());
    }
}
