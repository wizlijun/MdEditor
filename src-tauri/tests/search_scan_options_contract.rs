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

// `sync_dir` no longer lives on `ScanOptions` at all (C-T3) — the
// 2026-08-12 design (C-T2) had already retired rule 5 (the
// sync-mirror-directory special case) in favor of user-configured source
// globs (rule 5′), leaving `ScanOptions.sync_dir` with no live correctness
// reason to exist, and C-T3 deleted it. The GUI (`search::mod::open_vault`)
// and the CLI (`cli::search::run`) each now resolve `sync_dir` directly
// from `vault_settings::resolve_sync_dir` for `SearchIndex::open` — still
// the one shared function, just no longer routed through `ScanOptions`.
// That resolution's default/override behavior is already pinned by
// `vault_settings::tests::resolve_sync_dir_*`; this file's job is only
// `ScanOptions`, which is why there is no `ScanOptions`-shaped test for it
// here any more.
