//! GUI 与 CLI 必须用同一份 ScanOptions。它们是两个进程写同一个索引库,
//! 阈值不一致就意味着同一个 vault 被两个口径索引 —— 直接违反「一个算法,
//! 三个 adapter」这条本功能赖以成立的前提。所以回落逻辑只能有一份实现,
//! 这个测试钉住「只有一份」。

use std::path::Path;

fn write_settings(root: &Path, json: &str) {
    std::fs::create_dir_all(root.join(".notemd")).unwrap();
    std::fs::write(root.join(".notemd/settings.json"), json).unwrap();
}

#[test]
fn the_index_threshold_falls_back_to_the_git_gate_when_unset() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{"largeFileThresholdMb": 25}"#);
    let opts = mdeditor_lib::search::options::for_vault(d.path());
    assert_eq!(opts.large_file_threshold_mb, 25, "未设索引阈值时应跟随 git 门禁");
}

#[test]
fn an_explicit_index_threshold_decouples_from_the_git_gate() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{"largeFileThresholdMb": 10, "searchLargeFileThresholdMb": 50}"#);
    let opts = mdeditor_lib::search::options::for_vault(d.path());
    assert_eq!(opts.large_file_threshold_mb, 50, "显式设过就不再跟随");
}

#[test]
fn both_unset_falls_back_to_the_default() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{}"#);
    assert_eq!(mdeditor_lib::search::options::for_vault(d.path()).large_file_threshold_mb, 10);
}

/// 两个 adapter 的构造必须是同一个函数,不是两份「碰巧一致」的实现。
#[test]
fn the_cli_and_the_gui_build_options_through_one_function() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{"searchLargeFileThresholdMb": 33, "searchExcludeDirs": ["a"], "syncDir": "box"}"#);
    let gui = mdeditor_lib::search::options::for_vault(d.path());
    let cli = mdeditor_lib::cli::search::scan_options_for(d.path());
    assert_eq!(gui.large_file_threshold_mb, cli.large_file_threshold_mb);
    assert_eq!(gui.exclude_dirs, cli.exclude_dirs);
    assert_eq!(gui.source_globs, cli.source_globs);
}

/// Task C-T8's other single construction point: the GUI and the CLI must
/// resolve the identical ranking `Weights` for the same vault, or the two
/// adapters rank the same query differently — the same "one algorithm,
/// three adapters" premise the `ScanOptions` test above exists to protect,
/// applied to ranking instead of scanning.
#[test]
fn the_cli_and_the_gui_resolve_the_same_weights() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{"searchWeights": {"human": 2.0, "source": 0.5}}"#);
    let gui = mdeditor_lib::search::options::weights_for_vault(d.path());
    let cli = mdeditor_lib::cli::search::weights_for(d.path());
    assert_eq!(gui, cli);
    // Not a vacuous comparison of two defaults: pin the actually-configured
    // value made it through, so a `weights_for_vault` that silently ignored
    // `search_weights` would still fail this test even though `gui == cli`
    // would trivially hold.
    assert_eq!(gui.human, 2.0);
    assert_eq!(gui.source, 0.5);
}

// `sync_dir` no longer lives on `ScanOptions` at all (C-T3) — the
// 2026-08-12 design (C-T2) had already retired rule 5 (the
// sync-mirror-directory special case) in favor of user-configured source
// globs (rule 5′), leaving `ScanOptions.sync_dir` with no live correctness
// reason to exist, and C-T3 deleted it. Neither call site resolves
// `sync_dir` for `SearchIndex::open` any more, either (review round 1 on
// C-T6 caught a stale version of this comment that still said they did):
// `store::open`'s staleness stamp was repointed at `SourceGlobs::stamp()`
// (C-T6), and both `search::mod::open_vault` and `cli::search::run` derive
// it as `opts.source_globs.stamp()` — off the very `ScanOptions` value
// `the_cli_and_the_gui_build_options_through_one_function` above already
// pins as identical between the two adapters — rather than resolving
// anything a second time. That is why there is no separate
// `ScanOptions`-shaped test for the glob stamp here: asserting
// `gui.source_globs == cli.source_globs` above is sufficient, since both
// callers' stamps are a pure function of that already-equal field.

// --- wikipage 检索优先级:第三个「唯一构造点」 ---------------------------
// spec `docs/superpowers/specs/2026-08-12-wikipage-search-priority-design.md`

/// `wikipageDir` 决定哪篇笔记会被硬置顶成第一条。GUI 和 CLI 若各读各的,
/// 同一个查询在两个入口会给出不同的第一条 —— 与 ScanOptions / Weights
/// 完全同类的问题,所以同样只能有一份实现。
#[test]
fn the_cli_and_the_gui_resolve_the_same_wikipage_conventions() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{"wikipageDir": "概念"}"#);
    let gui = mdeditor_lib::search::options::conventions_for_vault(d.path());
    let cli = mdeditor_lib::cli::search::conventions_for(d.path());
    assert_eq!(gui, cli);
    // 不是「两个默认值恰好相等」的空断言:钉住配置值真的被读到了。
    assert_eq!(gui.wikipage_dir.as_deref(), Some("概念"));
}

/// 没配过 = 用出厂目录名,而不是「不置顶」。绝大多数 vault 从没动过这个设置,
/// 对它们而言置顶必须开箱即用。
#[test]
fn an_unset_wikipage_dir_falls_back_to_the_shipped_default() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{}"#);
    let c = mdeditor_lib::search::options::conventions_for_vault(d.path());
    assert_eq!(c.wikipage_dir.as_deref(), Some("wikipage"));
}

/// 非法目录名(路径穿越)必须回落到默认,而不是原样交给排序去比对 ——
/// 与 `ui_rpc` 对同一个字段的处理保持一致(它也是 validate 失败即回落)。
#[test]
fn an_invalid_wikipage_dir_falls_back_instead_of_being_used_verbatim() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{"wikipageDir": "../escape"}"#);
    let c = mdeditor_lib::search::options::conventions_for_vault(d.path());
    assert_eq!(c.wikipage_dir.as_deref(), Some("wikipage"));
}
