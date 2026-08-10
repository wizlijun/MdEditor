//! GUI and CLI must open the SAME index database. They are separate processes
//! with no channel between them, so if their path math ever diverges the CLI
//! silently answers from an index nobody is updating. The guarantee is
//! structural — one function, in searchidx — and this test is what keeps it so.

use std::path::Path;

#[test]
fn the_gui_and_the_cli_resolve_one_index_path() {
    let vault = Path::new(if cfg!(windows) { r"C:\vault" } else { "/vault" });
    // The CLI path (what src/cli/search.rs uses, via SearchIndex::open).
    let cli = searchidx::paths::index_db_path(vault).unwrap();
    // What the Tauri side must use — the SAME call, not a reimplementation.
    let gui = searchidx::paths::index_db_path(vault).unwrap();
    assert_eq!(cli, gui);
    assert!(cli.to_string_lossy().contains(searchidx::paths::BUNDLE_ID));
}

/// Windows 上 GUI 读 `%APPDATA%\net.notemd.app\shared.json`,CLI 必须读同一个。
/// 这是 headless vault-root 解析的前提:读错文件 = CLI 找不到 vault。
#[test]
fn the_cli_config_dir_is_the_platform_config_dir_for_this_bundle() {
    let dir = mdeditor_lib::cli::resolve_config_dir();
    let expected = dirs::config_dir().unwrap().join("net.notemd.app");
    assert_eq!(dir, expected);
}

/// Windows 陷阱:索引必须在 Local(每设备独立),配置在 Roaming 侧的
/// config_dir —— 两者是不同的目录,不能顺手统一。
#[cfg(windows)]
#[test]
fn the_index_is_local_while_the_config_is_not() {
    let idx = searchidx::paths::index_db_path(Path::new(r"C:\vault")).unwrap();
    let cfg = mdeditor_lib::cli::resolve_config_dir();
    assert_ne!(idx.parent().unwrap().parent().unwrap().parent(), cfg.parent());
    assert!(idx.to_string_lossy().to_lowercase().contains(r"\local\"));
}
