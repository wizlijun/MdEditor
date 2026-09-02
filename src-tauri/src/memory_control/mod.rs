//! Memory Protocol v2 authority, reducer, projections and trusted RPC adapter.
pub mod v2;

use serde_json::Value;
use std::path::Path;

/// Host RPC adapter used only by the official Memory plugin window.
pub fn dispatch(root: &Path, method: &str, params: &Value) -> Result<Value, String> {
    v2::dispatch_rpc(root, method, params)
}
