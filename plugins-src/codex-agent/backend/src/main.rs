//! note.md Codex Agent: resident plugin process and detached runner entry point.
pub const SELF_PLUGIN_ID: &str = "notemd.codex-agent";

mod argv;
mod discover;
mod engine;
mod plugin;
mod policy;
mod runner;
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
    rt.block_on(notemd_plugin_sdk::serve(plugin::CodexAgentPlugin::new()));
}
