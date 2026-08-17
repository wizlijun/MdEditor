//! notemd.deepseek-agent entry point. Two modes:
//!  - `--runner <runDir>`: the detached process a CLI-triggered run hands off to
//!    (the host's CLI path runs in a throwaway headless app instance that would
//!    otherwise kill the child at exit — see runner.rs).
//!  - no args: the SDK serve loop, as a host-managed resident plugin process.
//!
//! Everything a run IS — locks, records, progress, artifacts, OKF stamping —
//! lives in the shared `agent-run-core` crate, so this plugin and claude-agent
//! cannot drift on the on-disk format the host reads.
mod acp;
mod composition;
mod discover;
mod engine;
mod plugin;
mod policy;
mod runner;
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
    rt.block_on(notemd_plugin_sdk::serve(plugin::DeepseekAgentPlugin::new()));
}
