mod convert; mod dates; mod discover; mod merge; mod outline; mod plugin; mod procutil; mod roam_cli; mod roam_page; mod syntax;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2).enable_all().build().expect("tokio runtime");
    rt.block_on(notemd_plugin_sdk::serve(plugin::RoamImportPlugin::new()));
}
