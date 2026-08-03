//! Binary entry point. All the logic lives in the library crate
//! (`notemd_roam_import`) so integration tests can reach it; this file and
//! `plugin.rs` are the only parts the tests do not see.
mod plugin;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2).enable_all().build().expect("tokio runtime");
    rt.block_on(notemd_plugin_sdk::serve(plugin::RoamImportPlugin::new()));
}
