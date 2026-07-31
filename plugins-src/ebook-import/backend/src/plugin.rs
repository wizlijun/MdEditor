use notemd_plugin_sdk as sdk;
use serde_json::{json, Value};

pub struct EbookImportPlugin { pub data_dir: std::path::PathBuf }
impl EbookImportPlugin { pub fn new() -> Self { Self { data_dir: std::env::temp_dir() } } }

impl sdk::NotemdPlugin for EbookImportPlugin {
    fn initialize(&mut self, _h: &sdk::Host, p: &sdk::InitializeParams) {
        self.data_dir = std::path::PathBuf::from(&p.data_dir);
    }
    fn activate(&mut self, _h: &sdk::Host, _p: &sdk::plugin_protocol::ActivateParams) -> Result<(), String> { Ok(()) }
    fn deactivate(&mut self, _h: &sdk::Host) {}
    fn execute_command(&mut self, _h: &sdk::Host, p: &sdk::ExecuteCommandParams) -> Result<Value, String> {
        Err(format!("unknown command '{}'", p.command))
    }
    fn on_ui_request(&mut self, _h: &sdk::Host, m: &str, _p: Value) -> Result<Value, String> {
        Err(format!("unknown ui method '{m}'"))
    }
}
