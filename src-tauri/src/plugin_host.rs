//! The manifest view model the frontend and the CLI consume, plus the native
//! menu items plugins contribute.
//!
//! Plugins ship a `manifest.v2.json` and run on the resident runtime in
//! [`crate::plugin_runtime`]. That runtime adapts every installed manifest into
//! the [`PluginManifest`] shape defined here (`plugin_runtime::adapter::to_v1`),
//! which is what `get_plugin_manifests` serves to the webviews and what the CLI
//! router matches subcommands against. Nothing in this module reads the disk or
//! spawns anything — it is the presentation layer over the runtime's state.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    #[default]
    External,
    Builtin,
}

/// Per-locale overrides for a plugin's user-facing strings. Keys mirror the
/// stable identifiers in the manifest (menu/context command, settings field
/// key) so translations don't depend on array order.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginI18n {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub menus: HashMap<String, String>,
    #[serde(default)]
    pub context_menus: HashMap<String, String>,
    #[serde(rename = "settings.tab_label", default)]
    pub settings_tab_label: Option<String>,
    #[serde(rename = "settings.fields", default)]
    pub settings_fields: HashMap<String, String>,
}

/// A plugin as the frontend and the CLI see it. Produced from a
/// `plugin_protocol::ManifestV2` by `plugin_runtime::adapter::to_v1`; it is a
/// view model, not an on-disk format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    /// locale code -> string overrides (English base lives in the fields above).
    #[serde(default)]
    pub i18n: HashMap<String, PluginI18n>,
    /// Always [`PluginKind::External`] on adapted manifests; kept so the shape
    /// stays stable for consumers that already read it.
    #[serde(default)]
    pub kind: PluginKind,
    /// Always `Some("")` on adapted manifests — the runtime resolves a plugin's
    /// executable from its install tree, so no path travels in the view model.
    #[serde(default)]
    pub binary: Option<String>,
    /// Legacy field, kept for shape stability. Whether a plugin is active is
    /// decided by the runtime's `state.json` before its manifest ever reaches
    /// a consumer, so nothing reads this.
    #[serde(default)]
    pub default_enabled: Option<bool>,
    #[serde(default)]
    pub menus: Vec<MenuEntry>,
    #[serde(default)]
    pub context_menus: Vec<ContextMenuEntry>,
    /// Custom-editor contributions (子项目④), passed through from the v2
    /// manifest by `plugin_runtime::adapter` so the frontend can build its
    /// ext→editor registry. Opaque here — the host never interprets it; it
    /// rides to the frontend via `get_plugin_manifests`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_editors: Vec<serde_json::Value>,
    #[serde(default)]
    pub settings: Option<SettingsBlock>,
    pub host_capabilities: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub cli: Vec<CliEntry>,
    /// The manifest generation this was adapted from — always `Some(2)` for a
    /// plugin, absent on the CLI's injected core stubs.
    #[serde(default)]
    pub manifest_version: Option<u32>,
    /// `open_command → window_id` for plugins whose window contributions declare
    /// an `open_command` (`plugin_runtime::adapter`). The frontend routes those
    /// commands to `plugin_v2_open_window` instead of `plugin_v2_execute`.
    /// `None` when the plugin has no openable windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_windows: Option<HashMap<String, String>>,
}

fn default_timeout() -> u64 { 30 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptFilter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptSpec {
    pub kind: String,                           // "save-dialog" is the only kind
    pub default_filename: String,
    pub filters: Vec<PromptFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MenuEntry {
    pub location: String,
    /// Optional named sub-menu under `location` (e.g. "import" nests the item
    /// under File ▸ Import). None keeps the item flat in the top-level menu.
    #[serde(default)]
    pub submenu: Option<String>,
    pub label: String,
    #[serde(default)]
    pub shortcut: Option<String>,
    pub command: String,
    #[serde(default)]
    pub enabled_when: Option<String>,
    #[serde(default)]
    pub prompt: Option<PromptSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMenuEntry {
    pub location: String,
    pub label: String,
    pub command: String,
    #[serde(default)]
    pub enabled_when: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliArg {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,             // "path" | "string" | "integer"
    pub required: bool,
    #[serde(default)]
    pub help: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliFlag {
    pub long: String,
    #[serde(default)]
    pub short: Option<String>,
    #[serde(rename = "type")]
    pub ty: String,             // "boolean" | "string"
    #[serde(default)]
    pub help: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliEntry {
    pub subcommand: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub command: String,
    pub summary: String,
    #[serde(default)]
    pub args: Vec<CliArg>,
    #[serde(default)]
    pub flags: Vec<CliFlag>,
    #[serde(default)]
    pub requires_tab_context: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsBlock {
    pub tab_label: String,
    pub schema: Vec<serde_json::Value>,
}

/// Sorted by id: this order reaches the user as the order of the Plugins menu
/// and of the frontend's plugin lists, so it must not depend on map iteration
/// or discovery order — a list that reshuffles between launches reads as a glitch.
fn by_id(mut v: Vec<PluginManifest>) -> Vec<PluginManifest> {
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

/// Every installed, enabled plugin in view-model shape. Drives the frontend's
/// menu model, dispatch table, settings tabs and custom-editor registry.
#[tauri::command]
pub fn get_plugin_manifests() -> Vec<PluginManifest> {
    by_id(crate::plugin_runtime::adapter::adapted_v2_manifests())
}

pub struct LocatedMenuItem {
    pub id: String,
    pub label: String,
    pub shortcut: Option<String>,
    pub location: String,
    pub submenu: Option<String>,
}

/// Returns menu entries flattened across all active plugins, with ids encoded
/// as `plugin:<id>:<command>`. Menu labels are resolved for `locale` (falling
/// back to the manifest's English label per missing key; a plugin's i18n block
/// is passed through by the adapter).
pub fn collect_top_menu_items(locale: &str) -> Vec<LocatedMenuItem> {
    let manifests = get_plugin_manifests();
    let mut out = Vec::new();
    for m in manifests.iter() {
        for me in m.menus.iter() {
            let label = m
                .i18n
                .get(locale)
                .and_then(|t| t.menus.get(&me.command))
                .cloned()
                .unwrap_or_else(|| me.label.clone());
            out.push(LocatedMenuItem {
                id: format!("plugin:{}:{}", m.id, me.command),
                label,
                shortcut: me.shortcut.clone(),
                location: me.location.clone(),
                submenu: me.submenu.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_with_cli_round_trips() {
        let json = r#"{
            "id": "demo",
            "name": "Demo",
            "version": "0.1.0",
            "binary": "bin",
            "host_capabilities": [],
            "cli": [{
                "subcommand": "demo",
                "command": "noop",
                "summary": "s",
                "args": [{"name": "f", "type": "path", "required": true}]
            }]
        }"#;
        let m: PluginManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.cli.len(), 1);
        assert_eq!(m.cli[0].subcommand, "demo");
        assert_eq!(m.cli[0].args.len(), 1);
        assert_eq!(m.cli[0].args[0].ty, "path");
    }

    #[test]
    fn manifest_without_cli_defaults_to_empty() {
        let json = r#"{
            "id": "old",
            "name": "Old",
            "version": "0.1.0",
            "binary": "bin",
            "host_capabilities": []
        }"#;
        let m: PluginManifest = serde_json::from_str(json).unwrap();
        assert!(m.cli.is_empty());
    }

    #[test]
    fn manifest_defaults_to_external_kind() {
        let json = r#"{
            "id": "share", "name": "Share", "version": "1.0.0",
            "binary": "bin", "host_capabilities": ["toast"]
        }"#;
        let m: PluginManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.kind, PluginKind::External);
    }

    #[test]
    fn manifest_parses_builtin_kind() {
        let json = r#"{
            "id": "openclaw-chat", "name": "OpenClaw Chat", "version": "0.1.0",
            "kind": "builtin", "host_capabilities": []
        }"#;
        let m: PluginManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.kind, PluginKind::Builtin);
        assert!(m.binary.is_none());
    }
}
