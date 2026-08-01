//! pos-log v2 plugin entry point.
//! Location comes from the HOST via `host.location.get` (the host is a signed
//! bundle that can obtain macOS location authorization; a bare plugin binary
//! cannot — its `requestWhenInUseAuthorization` never prompts). So this process
//! just runs the SDK serve loop — no main-thread CoreLocation service anymore.
mod logbook;
mod plugin;
mod strings;

fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(notemd_plugin_sdk::serve(plugin::PosLogPlugin::new()));
}
