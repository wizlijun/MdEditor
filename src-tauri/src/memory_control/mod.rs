//! Controlled USER.md / MEMORY.md workflow.
//!
//! Immutable proposal files and decision events are the audit history; the two
//! root Markdown files are read-only projections. Both the core CLI and the
//! official Memory plugin call this module so their semantics cannot drift.

mod document;
pub mod model;
mod store;
pub mod v2;

use serde_json::{json, Value};
use std::path::Path;

pub use model::*;

pub fn list(root: &Path) -> Result<Snapshot, String> {
    store::list(root)
}
pub fn suggest(root: &Path) -> Result<Value, String> {
    store::suggest(root)
}
pub fn propose(root: &Path, input: ProposeInput) -> Result<Proposal, String> {
    store::propose(root, input)
}
pub fn decide(root: &Path, input: DecideInput) -> Result<Value, String> {
    store::decide(root, input)
}
pub fn migrate(root: &Path) -> Result<Value, String> {
    store::migrate(root)
}

/// Host RPC adapter used only by the official Memory plugin window.
pub fn dispatch(root: &Path, method: &str, params: &Value) -> Result<Value, String> {
    if method.starts_with("host.memory.v2.") {
        return v2::dispatch_rpc(root, method, params);
    }
    let mode = v2::V2Repository::new(root)
        .load()
        .map_err(|error| error.to_string())?
        .mode;
    if matches!(mode, v2::RepositoryMode::V2Active | v2::RepositoryMode::V2Incomplete) {
        return Err(
            "MEMORY_PROTOCOL_V2_WRITE_FENCE: upgrade the Memory plugin; v1 RPC is disabled"
                .into(),
        );
    }
    match method {
        "host.memory.list" => serde_json::to_value(list(root)?).map_err(|e| e.to_string()),
        "host.memory.suggest" => suggest(root),
        "host.memory.propose" => {
            let input: ProposeInput = serde_json::from_value(params.clone())
                .map_err(|e| format!("memory: invalid propose params: {e}"))?;
            serde_json::to_value(propose(root, input)?).map_err(|e| e.to_string())
        }
        "host.memory.decide" => {
            let input: DecideInput = serde_json::from_value(params.clone())
                .map_err(|e| format!("memory: invalid decide params: {e}"))?;
            decide(root, input)
        }
        "host.memory.migrate" => migrate(root),
        "host.memory.check" => {
            let snapshot = list(root)?;
            Ok(json!({ "ok": !snapshot.integrity.drift, "integrity": snapshot.integrity }))
        }
        _ => Err(format!("memory: unknown method {method}")),
    }
}
