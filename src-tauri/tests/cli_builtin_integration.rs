//! End-to-end test for built-in CLI subcommands.
//!
//! Spawns the real `notemd` binary (mainBinaryName) with argv[0] forced to
//! "notemd" so the CLI mode path triggers. Asserts stdout / stderr / exit code
//! for the happy paths. HOME points at a temp dir so the run never sees the
//! developer's real settings or installed plugins.

use std::path::PathBuf;
use std::process::Command;

fn binary_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_notemd"))
}

/// A throwaway HOME so `resolve_config_dir` / the plugin install tree resolve
/// somewhere empty rather than to the developer's account.
fn temp_home() -> PathBuf {
    std::env::temp_dir().join(format!(
        "notemd-cli-int-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
    ))
}

fn run_cli(args: &[&str], home: &PathBuf) -> (i32, String, String) {
    use std::os::unix::process::CommandExt;
    std::fs::create_dir_all(home).unwrap();
    let mut cmd = Command::new(binary_path());
    cmd.arg0("notemd");          // force CLI mode via argv[0] basename
    cmd.args(args);
    cmd.env_remove("HOME");
    cmd.env("HOME", home.to_str().unwrap());
    let out = cmd.output().expect("spawn binary");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn help_lists_core_commands() {
    let home = temp_home();
    let (code, stdout, _) = run_cli(&["help"], &home);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(code, 0);
    assert!(stdout.contains("CORE COMMANDS:"), "stdout was: {stdout}");
    assert!(stdout.contains("share"));
    assert!(stdout.contains("reading-insights"));
}

#[test]
fn version_prints_and_exits_zero() {
    let home = temp_home();
    let (code, stdout, _) = run_cli(&["version"], &home);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(code, 0);
    assert!(stdout.contains("notemd"));
    assert!(stdout.contains("plugin API v1"));
}

#[test]
fn plugin_list_exits_zero() {
    let home = temp_home();
    let (code, stdout, _) = run_cli(&["plugin", "list"], &home);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(code, 0);
    // No plugins installed under the temp HOME → header row only.
    assert!(stdout.contains("STATUS"), "stdout was: {stdout}");
}

#[test]
fn plugin_enable_unknown_id_exits_2() {
    let home = temp_home();
    let (code, _, stderr) = run_cli(&["plugin", "enable", "nope"], &home);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(code, 2);
    assert!(stderr.contains("unknown plugin id"), "stderr was: {stderr}");
}

#[test]
fn unknown_subcommand_exits_127() {
    let home = temp_home();
    let (code, _, stderr) = run_cli(&["nope"], &home);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(code, 127);
    assert!(stderr.contains("unknown command"));
}

#[test]
fn legacy_mdedit_argv0_still_enters_cli_mode() {
    // Pre-rename `mdedit` symlinks must keep working after the note.md rename.
    use std::os::unix::process::CommandExt;
    let home = temp_home();
    std::fs::create_dir_all(&home).unwrap();
    let mut cmd = Command::new(binary_path());
    cmd.arg0("mdedit");
    cmd.arg("version");
    cmd.env("HOME", home.to_str().unwrap());
    let out = cmd.output().expect("spawn binary");
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("notemd"));
}

#[test]
fn core_share_alias_routes_not_unknown() {
    // `--share` is a core stub alias: it must resolve to the share command
    // (which then fails on the missing file), never to "unknown command".
    let home = temp_home();
    let (code, _, _) = run_cli(&["--share", "/nonexistent/anything.md"], &home);
    let _ = std::fs::remove_dir_all(&home);
    assert_ne!(code, 127);
}
