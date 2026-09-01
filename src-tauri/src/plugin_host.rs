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
    /// Can this plugin serve the host's agent slot? Computed by
    /// `plugin_runtime::adapter` from the manifest's activation events (see
    /// `plugin_runtime::agent_provider`), so every consumer — the sidecar
    /// note's Agent area, the CLI — applies one rule rather than its own copy.
    #[serde(default)]
    pub agent_provider: bool,
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
    /// Stable one-level capability group under Plugins (e.g. `agents`).
    /// Unknown or missing values are presented under Other.
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

/// Stable one-level capability groups shown under the native Plugins menu.
/// `menus[].submenu` carries one of these language-neutral keys. Keeping the
/// field open means an older Host still loads newer plugin manifests; this
/// Host normalizes unknown/missing keys to Other instead of hiding an action.
pub const PLUGIN_MENU_GROUP_ORDER: [&str; 7] = [
    "record",
    "reading",
    "inspiration",
    "advance",
    "reflect",
    "create",
    "other",
];

pub fn normalize_plugin_menu_group(group: Option<&str>) -> &'static str {
    match group {
        Some("record") | Some("capture") | Some("capture-import") => "record",
        Some("reading") => "reading",
        Some("inspiration") => "inspiration",
        Some("advance") | Some("agents") => "advance",
        Some("reflect") | Some("thinking") | Some("thinking-review") => "reflect",
        Some("create")
        | Some("import-export")
        | Some("publish-export")
        | Some("editing")
        | Some("editor-extensions") => "create",
        _ => "other",
    }
}

/// First-party plugins keep their curated cognitive category even while an
/// older installed manifest still carries a legacy capability key.
pub fn plugin_menu_group_for_plugin(plugin_id: &str, group: Option<&str>) -> &'static str {
    match plugin_id {
        "notemd.pos-log" | "notemd.roam-import" => "record",
        "notemd.ebook-import" | "notemd.trace-source" => "reading",
        "notemd.idea-spark" => "inspiration",
        "notemd.next"
        | "notemd.claude-agent"
        | "notemd.codex-agent"
        | "notemd.deepseek-agent"
        | "notemd.openclaw-chat" => "advance",
        "notemd.decision-log" | "notemd.weekly-review" => "reflect",
        "notemd.md2pdf" | "notemd.power-mode" => "create",
        _ => normalize_plugin_menu_group(group),
    }
}

fn plugin_id_from_menu_item_id(id: &str) -> Option<&str> {
    id.strip_prefix("plugin:")?.split_once(':').map(|(plugin_id, _)| plugin_id)
}

pub struct PluginMenuGroup<'a> {
    pub key: &'static str,
    pub items: Vec<&'a LocatedMenuItem>,
}

/// Group plugin actions in the fixed capability order while preserving the
/// existing stable manifest/id order inside each group.
pub fn group_plugin_menu_items(items: &[LocatedMenuItem]) -> Vec<PluginMenuGroup<'_>> {
    let mut groups: Vec<PluginMenuGroup<'_>> = PLUGIN_MENU_GROUP_ORDER
        .iter()
        .map(|key| PluginMenuGroup { key, items: Vec::new() })
        .collect();
    for item in items {
        let key = plugin_id_from_menu_item_id(&item.id)
            .map(|id| plugin_menu_group_for_plugin(id, item.submenu.as_deref()))
            .unwrap_or_else(|| normalize_plugin_menu_group(item.submenu.as_deref()));
        groups
            .iter_mut()
            .find(|group| group.key == key)
            .expect("normalized plugin group is in PLUGIN_MENU_GROUP_ORDER")
            .items
            .push(item);
    }
    groups.retain(|group| !group.items.is_empty());
    groups
}

/// Returns menu entries across all active plugins, with ids encoded
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

    fn menu_item(id: &str, group: Option<&str>) -> LocatedMenuItem {
        LocatedMenuItem {
            id: id.to_string(),
            label: id.to_string(),
            shortcut: None,
            location: "plugins".to_string(),
            submenu: group.map(str::to_string),
        }
    }

    #[test]
    fn plugin_menu_group_order_matches_the_documented_taxonomy() {
        assert_eq!(PLUGIN_MENU_GROUP_ORDER, [
            "record",
            "reading",
            "inspiration",
            "advance",
            "reflect",
            "create",
            "other",
        ]);
    }

    #[test]
    fn plugin_menu_groups_follow_capability_order_and_keep_item_order() {
        let items = vec![
            menu_item("reading-1", Some("reading")),
            menu_item("advance-1", Some("advance")),
            menu_item("advance-2", Some("advance")),
            menu_item("record-1", Some("record")),
            menu_item("create-1", Some("create")),
        ];
        let groups = group_plugin_menu_items(&items);
        assert_eq!(groups.iter().map(|g| g.key).collect::<Vec<_>>(), vec![
            "record", "reading", "advance", "create",
        ]);
        assert_eq!(groups[2].items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec![
            "advance-1", "advance-2",
        ]);
    }

    #[test]
    fn unknown_or_missing_plugin_menu_group_falls_back_to_other() {
        let items = vec![
            menu_item("known", Some("thinking")),
            menu_item("unknown", Some("future-category")),
            menu_item("missing", None),
        ];
        let groups = group_plugin_menu_items(&items);
        assert_eq!(groups.iter().map(|g| g.key).collect::<Vec<_>>(), vec![
            "reflect", "other",
        ]);
        assert_eq!(groups[1].items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), vec![
            "unknown", "missing",
        ]);
    }

    #[test]
    fn previous_plugin_menu_group_keys_map_to_current_groups() {
        assert_eq!(normalize_plugin_menu_group(Some("agents")), "advance");
        assert_eq!(normalize_plugin_menu_group(Some("capture-import")), "record");
        assert_eq!(normalize_plugin_menu_group(Some("thinking-review")), "reflect");
        assert_eq!(normalize_plugin_menu_group(Some("publish-export")), "create");
        assert_eq!(normalize_plugin_menu_group(Some("editor-extensions")), "create");
    }

    #[test]
    fn first_party_plugins_override_ambiguous_legacy_groups() {
        assert_eq!(plugin_menu_group_for_plugin("notemd.idea-spark", Some("thinking")), "inspiration");
        assert_eq!(plugin_menu_group_for_plugin("notemd.trace-source", Some("capture")), "reading");
        assert_eq!(plugin_menu_group_for_plugin("third.party", Some("thinking")), "reflect");
    }
}
