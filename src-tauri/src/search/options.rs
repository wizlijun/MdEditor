//! `ScanOptions` 的**唯一**构造点。
//!
//! GUI 与 CLI 是两个进程,写同一个索引库。它们的扫描口径必须逐字段一致 ——
//! 否则同一个 vault 会被两套阈值/排除规则索引,而这个功能的全部前提是
//! 「一个算法,三个 adapter」。所以这里只有一个函数,两边都调它;
//! `tests/search_scan_options_contract.rs` 钉住这一点。

use std::path::Path;

use searchidx::ScanOptions;

/// 未配置任何阈值时的默认值。与 git 大文件门禁的默认值相同,但这是巧合
/// 不是耦合 —— 两者语义不同,可以各自演化。
const DEFAULT_THRESHOLD_MB: u32 = 10;

pub fn for_vault(vault_root: &Path) -> ScanOptions {
    let vs = crate::sotvault::vault_settings::read(vault_root);
    ScanOptions {
        // 回落链:索引阈值 → git 门禁 → 默认。中间那一跳是一次性的善意,
        // 让既有用户的索引行为不因为这次拆分而改变。
        large_file_threshold_mb: vs
            .search_large_file_threshold_mb
            .or(vs.large_file_threshold_mb)
            .unwrap_or(DEFAULT_THRESHOLD_MB),
        exclude_dirs: vs.search_exclude_dirs.unwrap_or_default(),
    }
}
