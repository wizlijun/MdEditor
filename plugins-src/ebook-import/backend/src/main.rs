mod airead; mod bookconf; mod calibre; mod htmlz; mod library; mod ocr; mod pipeline; mod plugin; mod settings;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2).enable_all().build().expect("tokio runtime");
    rt.block_on(notemd_plugin_sdk::serve(plugin::EbookImportPlugin::new()));
}
