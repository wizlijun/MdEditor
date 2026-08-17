//! notemd.claude-agent entry point. Two modes:
//!  - `--runner <runDir>`: the detached process a CLI-triggered run hands off to
//!    (the host's CLI path runs in a throwaway headless app instance that would
//!    otherwise kill the child at exit — see runner.rs).
//!  - no args: the SDK serve loop, as a host-managed resident plugin process.
// Everything a run IS — locks, records, progress, artifacts, OKF stamping —
// lives in the shared `agent-run-core` crate now, so claude-agent and
// deepseek-agent cannot drift on the on-disk format the host reads. Re-exported
// at the crate root so the rest of this binary still says `crate::record::…`:
// the modules moved, the call sites did not have to.
pub use agent_run_core::{artifacts, lock, mirror, okf, record};

mod discover;
mod engine;
mod plugin;
mod precheck;
mod prompt;
mod runner;
mod settings;
mod stream;
mod task;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");
    if let Some(i) = args.iter().position(|a| a == "--runner") {
        let dir = args.get(i + 1).cloned().unwrap_or_default();
        std::process::exit(rt.block_on(runner::run(std::path::PathBuf::from(dir))));
    }
    rt.block_on(notemd_plugin_sdk::serve(plugin::ClaudeAgentPlugin::new()));
}
