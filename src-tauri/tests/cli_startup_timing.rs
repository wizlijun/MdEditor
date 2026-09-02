//! Regression test: CLI mode dispatch must be fast.
//!
//! Asserts that `notemd help` returns under 500 ms on developer hardware
//! (release builds) or under 2500 ms in debug. The release budget reflects
//! the real-world goal — a user-perceptible `notemd help` invocation. Debug
//! builds carry significant extra overhead (no opt, large dylib graph,
//! cargo test harness fork) so we allow more headroom there while still
//! catching catastrophic regressions (e.g., the dispatch path accidentally
//! initializing Tauri / the webview / the plugin runtime).

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

#[cfg(debug_assertions)]
const BUDGET_MS: u128 = 2500;
#[cfg(not(debug_assertions))]
const BUDGET_MS: u128 = 500;

/// Put `cmd` into CLI mode.
///
/// unix fakes `argv[0]` (that is the real dispatch signal there — a bin-dir
/// symlink named `notemd`). Windows has no `arg0`, and the GUI executable is
/// literally `notemd.exe`, so argv[0] cannot disambiguate: `is_cli_mode` keys
/// off an explicit `--cli` flag there (see cli/mod.rs and the NSIS PATH shim in
/// docs/2026-08-08-pc-port-refactor-plan.md §5.1). Both paths reach the same
/// dispatch code, which is what this test measures.
fn cli_mode(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.arg0("notemd");
    }
    #[cfg(windows)]
    {
        cmd.arg("--cli");
    }
}

#[test]
fn cli_help_returns_quickly() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_notemd"));
    let home = std::env::temp_dir().join(format!(
        "notemd-timing-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir_all(&home).unwrap();

    // Warm up: first invocation pays cold dyld / linker / codesign cost.
    // We're measuring dispatch overhead, not page-fault-in-the-fs cost.
    {
        let mut warm = Command::new(&bin);
        cli_mode(&mut warm);
        warm.arg("help");
        warm.env("HOME", home.to_str().unwrap());
        let _ = warm.output();
    }

    let start = Instant::now();
    let mut cmd = Command::new(bin);
    cli_mode(&mut cmd);
    cmd.arg("help");
    cmd.env("HOME", home.to_str().unwrap());
    let output = cmd.output().expect("spawn");
    let elapsed = start.elapsed();

    let _ = std::fs::remove_dir_all(&home);
    assert!(
        output.status.success(),
        "help should exit 0, got {:?}",
        output.status
    );
    assert!(
        elapsed.as_millis() < BUDGET_MS,
        "notemd help took {} ms (budget {})",
        elapsed.as_millis(),
        BUDGET_MS,
    );
}

/// Two budgets, because they measure different things: an ASCII query never
/// touches the Chinese dictionary, a CJK one pays to decompress and parse it
/// exactly once per process. Conflating them would either hide an ASCII
/// regression or fail spuriously on a cold dictionary.
#[cfg(debug_assertions)]
const SEARCH_ASCII_MS: u128 = 4000;
#[cfg(debug_assertions)]
const SEARCH_CJK_MS: u128 = 6000;
#[cfg(not(debug_assertions))]
const SEARCH_ASCII_MS: u128 = 800;
#[cfg(not(debug_assertions))]
const SEARCH_CJK_MS: u128 = 1200;

#[test]
fn search_meets_both_startup_budgets() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_notemd"));
    // A throwaway HOME so the index lands in a scratch app-data dir instead of
    // the developer's real one (same isolation as `cli_help_returns_quickly`
    // above).
    let home = std::env::temp_dir().join(format!(
        "notemd-search-timing-home-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir_all(&home).unwrap();

    let vault = std::env::temp_dir().join(format!("notemd-search-timing-{}", std::process::id()));
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(vault.join("a.md"), "brownfox 全文检索\n").unwrap();

    let run = |q: &str| -> u128 {
        let mut cmd = Command::new(&bin);
        cli_mode(&mut cmd);
        cmd.args(["search", q, "--vault"]).arg(&vault);
        cmd.env("HOME", &home);
        let t = Instant::now();
        let out = cmd.output().expect("spawn");
        assert!(
            out.status.success(),
            "search {q:?} should exit 0, got {:?}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        t.elapsed().as_millis()
    };
    // Warm up disk/dyld caches with a query that actually hits — "warmup" is
    // not in the fixture, and an empty-result run exits 1 (no hits, not an
    // error), which would trip the `run` closure's success assertion. Each
    // invocation is its own fresh process, so this pays only OS-level cache
    // costs, never the CJK dictionary load the timed run below measures.
    let _ = run("brownfox");
    let ascii = run("brownfox");
    let cjk = run("全文检索");
    let _ = std::fs::remove_dir_all(&vault);
    let _ = std::fs::remove_dir_all(&home);
    assert!(
        ascii < SEARCH_ASCII_MS,
        "ascii search took {ascii} ms (budget {SEARCH_ASCII_MS})"
    );
    assert!(
        cjk < SEARCH_CJK_MS,
        "cjk search took {cjk} ms (budget {SEARCH_CJK_MS})"
    );
}
