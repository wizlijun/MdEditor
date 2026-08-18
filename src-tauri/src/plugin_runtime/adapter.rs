//! Adapter: project a plugin's `ManifestV2` into the [`PluginManifest`] view
//! model that the frontend and the CLI consume (menus, context menus, CLI
//! entries, settings blocks, custom editors, windows). `to_v1` keeps its
//! historical name — the shape it produces predates the v2 manifest format and
//! outlived it as the host's presentation layer.

use crate::plugin_host::PluginManifest;

pub fn to_v1(m: &plugin_protocol::ManifestV2) -> Result<PluginManifest, String> {
    // 经 serde_json 转换：PluginManifest 派生 Deserialize，未知字段忽略。
    let mut v = serde_json::json!({
        "id": m.id, "name": m.name, "version": m.version,
        "kind": "external", "binary": "",
        "host_capabilities": m.capabilities,
        "menus": m.contributes.menus,
        "context_menus": m.contributes.context_menus,
        "cli": m.contributes.cli,
        // custom_editors (子项目④) rides through untouched so the frontend can
        // build its ext→editor registry from the adapted manifest.
        "custom_editors": m.contributes.custom_editors,
        "manifest_version": 2,
        // Whether this plugin can serve the agent slot. COMPUTED here, not
        // re-derived by the frontend: the rule (declares the three agent
        // commands) lives in `agent_provider` alone, so the TS side cannot
        // drift from it. The events themselves are deliberately NOT projected —
        // the view model carries conclusions, not raw manifest internals.
        "agent_provider": super::agent_provider::is_provider(m),
    });
    if let Some(d) = &m.description { v["description"] = serde_json::json!(d); }
    if let Some(s) = &m.contributes.settings { v["settings"] = s.clone(); }
    if let Some(i) = &m.i18n { v["i18n"] = i.clone(); }

    // Windows with an `open_command` become an `open_command → window_id` map so
    // the frontend can route that command to `plugin_v2_open_window` instead of
    // `plugin_v2_execute`. Windows without an open_command are not exposed here.
    let open_windows: std::collections::HashMap<String, String> = m
        .contributes
        .windows
        .iter()
        .filter_map(|w| w.open_command.clone().map(|cmd| (cmd, w.id.clone())))
        .collect();
    if !open_windows.is_empty() {
        v["open_windows"] = serde_json::json!(open_windows);
    }

    serde_json::from_value(v).map_err(|e| format!("contributes could not be adapted: {e}"))
}

/// Every discovered plugin in view-model shape, in STATE's id order.
/// Plugins whose contributes block fails the conversion are skipped with an
/// eprintln — they do not crash the host.
pub fn adapted_v2_manifests() -> Vec<PluginManifest> {
    super::STATE
        .read()
        .unwrap()
        .plugins
        .values()
        .filter_map(|(m, _)| match to_v1(m) {
            Ok(v1) => Some(v1),
            Err(e) => {
                eprintln!("[plugin_runtime] {}: contributes could not be adapted: {e}", m.id);
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::PluginKind;

    /// Every shipped manifest must survive adaptation. When one doesn't,
    /// `adapted_v2_manifests` drops that plugin with nothing but an eprintln —
    /// so the whole plugin, menus and windows included, silently vanishes. A
    /// missing `summary`, or `"ty"` where the view model renames it to `"type"`,
    /// is enough to do it. Catch that here instead of in the running app.
    #[test]
    fn shipped_plugin_manifests_survive_adaptation() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins-src");
        let mut checked = 0;
        for entry in std::fs::read_dir(&root).expect("plugins-src is readable") {
            let path = entry.unwrap().path().join("manifest.v2.json");
            if !path.exists() {
                continue;
            }
            let body = std::fs::read_to_string(&path).unwrap();
            let m: plugin_protocol::ManifestV2 = serde_json::from_str(&body)
                .unwrap_or_else(|e| panic!("{}: does not parse as a v2 manifest: {e}", path.display()));
            to_v1(&m).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            checked += 1;
        }
        assert!(checked >= 4, "expected several plugin manifests, found {checked}");
    }

    /// Two menu entries sharing a command collapse to ONE native menu id
    /// (`plugin:<id>:<command>`, menu-registry.ts), so a plugin must not
    /// contribute the same command twice.
    #[test]
    fn shipped_plugins_do_not_contribute_a_command_twice() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins-src");
        for entry in std::fs::read_dir(&root).expect("plugins-src is readable") {
            let path = entry.unwrap().path().join("manifest.v2.json");
            if !path.exists() {
                continue;
            }
            let m: plugin_protocol::ManifestV2 =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut seen = std::collections::HashSet::new();
            for menu in &m.contributes.menus {
                let cmd = menu.get("command").and_then(|c| c.as_str()).unwrap_or("");
                assert!(
                    seen.insert(cmd.to_string()),
                    "{}: command '{cmd}' is contributed by two menu entries",
                    path.display()
                );
            }
        }
    }

    /// Full-featured sample in the md2pdf shape (plan Task 11 Step 3) plus
    /// context_menus / settings / i18n to exercise every passthrough.
    fn sample() -> plugin_protocol::ManifestV2 {
        serde_json::from_value(serde_json::json!({
            "manifest_version": 2,
            "id": "notemd.md2pdf",
            "name": "Export to PDF",
            "version": "1.0.0",
            "kind": "native",
            "engines": { "notemd": ">=6.716.7" },
            "description": "Export the current tab to PDF",
            "binary": { "aarch64-apple-darwin": "bin/md2pdf-v2", "x86_64-apple-darwin": "bin/md2pdf-v2" },
            "activation": { "events": ["onCommand:export", "onCli:pdf2"] },
            "contributes": {
                "menus": [{ "location": "file", "label": "Export to PDF (v2)…", "command": "export",
                            "enabled_when": "currentTab.kind == 'markdown'",
                            "prompt": { "kind": "save-dialog", "default_filename": "{stem}.pdf",
                                        "filters": [{ "name": "PDF", "extensions": ["pdf"] }] } }],
                "context_menus": [{ "location": "tab", "label": "Export tab…", "command": "export" }],
                "settings": { "tab_label": "PDF Export",
                              "schema": [{ "key": "paper", "type": "select", "label": "Paper",
                                            "options": ["A4"], "default": "A4" }] },
                "cli": [{ "subcommand": "pdf2", "command": "export",
                          "summary": "Export Markdown or HTML file to PDF (v2 runtime)",
                          "args": [{ "name": "file", "type": "path", "required": true, "help": "File to export" }],
                          "flags": [{ "long": "--output", "short": "-o", "type": "string", "help": "Output PDF path" }],
                          "requires_tab_context": true }]
            },
            "capabilities": ["renderer.html", "toast"],
            "i18n": { "zh": { "menus": { "export": "导出 PDF（v2）…" } } },
            "request_timeout_seconds": 60,
            "idle_shutdown_seconds": 120
        }))
        .unwrap()
    }

    #[test]
    fn maps_core_fields_marks_v2_and_external_with_empty_binary() {
        let v1 = to_v1(&sample()).unwrap();
        assert_eq!(v1.id, "notemd.md2pdf");
        assert_eq!(v1.name, "Export to PDF");
        assert_eq!(v1.version, "1.0.0");
        assert_eq!(v1.manifest_version, Some(2));
        assert_eq!(v1.kind, PluginKind::External);
        assert_eq!(v1.binary.as_deref(), Some(""));
        assert_eq!(v1.description.as_deref(), Some("Export the current tab to PDF"));
        // capabilities → host_capabilities, order preserved.
        assert_eq!(v1.host_capabilities, vec!["renderer.html".to_string(), "toast".to_string()]);
    }

    #[test]
    fn passes_menus_context_menus_cli_settings_i18n_through() {
        let v1 = to_v1(&sample()).unwrap();

        assert_eq!(v1.menus.len(), 1);
        let me = &v1.menus[0];
        assert_eq!(me.location, "file");
        assert_eq!(me.command, "export");
        assert_eq!(me.label, "Export to PDF (v2)…");
        assert_eq!(me.enabled_when.as_deref(), Some("currentTab.kind == 'markdown'"));
        let prompt = me.prompt.as_ref().expect("prompt passthrough");
        assert_eq!(prompt.kind, "save-dialog");
        assert_eq!(prompt.default_filename, "{stem}.pdf");
        assert_eq!(prompt.filters[0].name, "PDF");
        assert_eq!(prompt.filters[0].extensions, vec!["pdf"]);

        assert_eq!(v1.context_menus.len(), 1);
        assert_eq!(v1.context_menus[0].location, "tab");
        assert_eq!(v1.context_menus[0].command, "export");

        assert_eq!(v1.cli.len(), 1);
        let cli = &v1.cli[0];
        assert_eq!(cli.subcommand, "pdf2");
        assert_eq!(cli.command, "export");
        assert!(cli.requires_tab_context);
        assert_eq!(cli.args[0].name, "file");
        assert_eq!(cli.args[0].ty, "path");
        assert!(cli.args[0].required);
        assert_eq!(cli.flags[0].long, "--output");
        assert_eq!(cli.flags[0].short.as_deref(), Some("-o"));

        let settings = v1.settings.as_ref().expect("settings passthrough");
        assert_eq!(settings.tab_label, "PDF Export");
        assert_eq!(settings.schema.len(), 1);
        assert_eq!(settings.schema[0]["key"], "paper");

        // i18n rides through in the exact PluginI18n shape, so the existing
        // per-locale menu label resolution works on adapted manifests.
        let zh = v1.i18n.get("zh").expect("zh i18n passthrough");
        assert_eq!(zh.menus.get("export").map(String::as_str), Some("导出 PDF（v2）…"));
    }

    /// A ManifestV2 whose contributes.menus entry lacks both label AND command
    /// should fail the conversion (MenuEntry requires label + command).
    #[test]
    fn to_v1_returns_err_when_menu_entry_lacks_label_and_command() {
        let m: plugin_protocol::ManifestV2 = serde_json::from_value(serde_json::json!({
            "manifest_version": 2,
            "id": "pub.bad",
            "name": "Bad",
            "version": "0.1.0",
            "kind": "native",
            "engines": { "notemd": ">=0.0.0" },
            "activation": { "events": ["*"] },
            "capabilities": [],
            "contributes": {
                "menus": [{ "location": "file" }]
            }
        }))
        .unwrap();
        let result = to_v1(&m);
        assert!(result.is_err(), "expected Err for menu entry missing label/command");
    }

    /// adapted_v2_manifests-style skip: a bad manifest is silently dropped,
    /// while a good sibling still produces a valid manifest.
    #[test]
    fn to_v1_bad_skipped_good_survives() {
        let bad: plugin_protocol::ManifestV2 = serde_json::from_value(serde_json::json!({
            "manifest_version": 2,
            "id": "pub.bad2",
            "name": "Bad2",
            "version": "0.1.0",
            "kind": "native",
            "engines": { "notemd": ">=0.0.0" },
            "activation": { "events": ["*"] },
            "capabilities": [],
            "contributes": { "menus": [{ "location": "file" }] }
        }))
        .unwrap();
        let good: plugin_protocol::ManifestV2 = serde_json::from_value(serde_json::json!({
            "manifest_version": 2,
            "id": "pub.good",
            "name": "Good",
            "version": "0.1.0",
            "kind": "native",
            "engines": { "notemd": ">=0.0.0" },
            "activation": { "events": ["*"] },
            "capabilities": []
        }))
        .unwrap();
        // Mirror adapted_v2_manifests skip logic
        let results: Vec<_> = [&bad, &good]
            .iter()
            .filter_map(|m| match to_v1(m) {
                Ok(v1) => Some(v1),
                Err(e) => {
                    eprintln!("[plugin_runtime] {}: contributes could not be adapted: {e}", m.id);
                    None
                }
            })
            .collect();
        assert_eq!(results.len(), 1, "bad manifest should be skipped, good survives");
        assert_eq!(results[0].id, "pub.good");
    }

    /// Windows carrying an `open_command` become an `open_command → window_id`
    /// map on the adapted manifest; a window without one is absent.
    #[test]
    fn open_windows_maps_only_windows_with_open_command() {
        let m: plugin_protocol::ManifestV2 = serde_json::from_value(serde_json::json!({
            "manifest_version": 2,
            "id": "notemd.roam-import",
            "name": "Roam Import",
            "version": "1.0.0",
            "kind": "native",
            "engines": { "notemd": ">=0.0.0" },
            "ui": "ui/",
            "activation": { "events": ["onCommand:open"] },
            "capabilities": [],
            "contributes": {
                "windows": [
                    { "id": "main", "entry": "index.html", "width": 680.0, "height": 620.0,
                      "open_command": "open" },
                    { "id": "aux", "entry": "aux.html", "width": 400.0, "height": 300.0 }
                ]
            }
        }))
        .unwrap();
        let v1 = to_v1(&m).unwrap();
        let ow = v1.open_windows.as_ref().expect("open_windows present");
        assert_eq!(ow.get("open").map(String::as_str), Some("main"));
        // The window with no open_command contributes nothing → single entry.
        assert_eq!(ow.len(), 1, "only the open-command window is exposed");
    }

    /// No windows (or none with an open_command) → the field stays None.
    #[test]
    fn open_windows_absent_when_no_open_command() {
        // sample() has no windows at all.
        assert!(to_v1(&sample()).unwrap().open_windows.is_none());
    }

    /// custom_editors (子项目④) rides through the adapter untouched so the
    /// frontend can build its ext→editor registry from the adapted manifest.
    #[test]
    fn custom_editors_pass_through() {
        let m: plugin_protocol::ManifestV2 = serde_json::from_value(serde_json::json!({
            "manifest_version": 2,
            "id": "notemd.base",
            "name": "Base",
            "version": "1.0.0",
            "kind": "native",
            "engines": { "notemd": ">=0.0.0" },
            "ui": "ui/",
            "activation": { "events": ["onFileType:base"] },
            "capabilities": [],
            "contributes": {
                "custom_editors": [
                    { "id": "base-table", "file_extensions": [".base"], "entry": "editor.html" }
                ]
            }
        }))
        .unwrap();
        let v1 = to_v1(&m).unwrap();
        assert_eq!(v1.custom_editors.len(), 1);
        assert_eq!(v1.custom_editors[0]["id"], "base-table");
        assert_eq!(v1.custom_editors[0]["file_extensions"][0], ".base");
        assert_eq!(v1.custom_editors[0]["entry"], "editor.html");
    }

    #[test]
    fn minimal_manifest_gets_view_model_defaults() {
        let m: plugin_protocol::ManifestV2 = serde_json::from_value(serde_json::json!({
            "manifest_version": 2,
            "id": "pub.min",
            "name": "Min",
            "version": "0.1.0",
            "kind": "native",
            "engines": { "notemd": ">=0.0.0" },
            "activation": { "events": ["*"] },
            "capabilities": []
        }))
        .unwrap();
        let v1 = to_v1(&m).unwrap();
        assert_eq!(v1.manifest_version, Some(2));
        assert_eq!(v1.kind, PluginKind::External);
        assert!(v1.description.is_none());
        assert!(v1.menus.is_empty());
        assert!(v1.context_menus.is_empty());
        assert!(v1.custom_editors.is_empty());
        assert!(v1.cli.is_empty());
        assert!(v1.settings.is_none());
        assert!(v1.i18n.is_empty());
        assert!(v1.host_capabilities.is_empty());
        assert!(v1.open_windows.is_none());
        assert_eq!(v1.timeout_seconds, 30, "serde default applies");
    }
}

#[cfg(test)]
mod agent_provider_projection {
    use super::*;

    fn manifest(events: &[&str]) -> plugin_protocol::ManifestV2 {
        serde_json::from_value(serde_json::json!({
            "manifest_version": 2,
            "id": "notemd.x",
            "name": "X",
            "version": "1.0.0",
            "kind": "native",
            "engines": { "notemd": ">=6.0.0" },
            "activation": { "events": events },
            "capabilities": [],
        }))
        .unwrap()
    }

    /// The regression this exists for: the sidecar note's Agent area asks the
    /// VIEW MODEL whether an agent is installed. `activation` is NOT projected
    /// (the view model carries conclusions, not manifest internals), so the
    /// answer has to travel as its own field — deriving it frontend-side from
    /// `activation.events` silently yields "no agents installed" and the whole
    /// Agent area disappears. That shipped in v6.817.4.
    #[test]
    fn the_view_model_says_whether_a_plugin_can_serve_the_agent_slot() {
        let agent = to_v1(&manifest(&[
            "onCommand:run-task",
            "onCommand:run-note",
            "onCommand:run-status",
        ]))
        .unwrap();
        assert!(agent.agent_provider);

        let not_agent = to_v1(&manifest(&["onCommand:export"])).unwrap();
        assert!(!not_agent.agent_provider);
    }

    /// It has to survive serialization: the frontend reads JSON, not the struct.
    #[test]
    fn the_flag_reaches_the_frontend_as_json() {
        let v = serde_json::to_value(
            to_v1(&manifest(&[
                "onCommand:run-task",
                "onCommand:run-note",
                "onCommand:run-status",
            ]))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(v["agent_provider"], true);
        assert!(
            v.get("activation").is_none(),
            "the view model carries conclusions, not raw manifest internals"
        );
    }

    /// Both shipped agent plugins, end to end through the real adapter.
    #[test]
    fn both_shipped_agent_plugins_project_as_providers() {
        for path in [
            "../plugins-src/claude-agent/manifest.v2.json",
            "../plugins-src/deepseek-agent/manifest.v2.json",
        ] {
            let body = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
            let m: plugin_protocol::ManifestV2 = serde_json::from_str(&body).unwrap();
            assert!(to_v1(&m).unwrap().agent_provider, "{path}");
        }
    }
}
