//! Routing: argv → Route. Step order matches spec §3 exactly.

use crate::plugin_host::PluginManifest;
use super::args::Parsed;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Route {
    Builtin(Builtin),
    Plugin(PluginRoute),
    /// A subcommand whose owning plugin the scan reported as disabled.
    ///
    /// **Production cannot currently reach this**: `current_scan` only ever
    /// inserts `true` (core stubs are always on, and discovery drops plugins
    /// whose `state.json` entry is disabled — they arrive absent, not disabled).
    /// The arm is kept because `resolve_with` is a pure function over a caller-
    /// supplied enabled map, and honoring an explicit `false` is the behavior a
    /// future "list installed-but-disabled plugins" scan would want.
    Disabled { plugin_id: String, subcommand: String },
    Unknown(String),
}

#[derive(Debug)]
pub enum Builtin {
    Help { topic: Option<String>, all: bool },
    Version,
    PluginList,
    PluginEnable(String),
    PluginDisable(String),
    PluginInfo(String),
    /// `plugin install <id>[@version]` — install (or reinstall) a v2 plugin.
    /// `None` version ⇒ install the version the registry index advertises.
    PluginInstall(String, Option<String>),
    /// `plugin update [<id>]` — update one plugin, or (no id) every installed
    /// plugin that the registry advertises a newer version for.
    PluginUpdate(Option<String>),
    /// `plugin remove <id>` (alias `uninstall`). `keep_data` from `--keep-data`.
    PluginRemove(String, bool),
    /// `search <query...>` — full-text search over the configured vault.
    Search(super::search::SearchArgs),
    /// `doctor` — self-check every local capability. Core, never disabled.
    Doctor(super::doctor::DoctorArgs),
    /// `mcp` —— MCP server 的 stdio 外壳。Core,never disabled:
    /// agent 的检索入口不能取决于插件状态。
    Mcp,
}

/// Split an `id[@version]` token on the LAST `@`, so plugin ids that themselves
/// contain `@` (scoped-style) keep working. Returns `(id, Some(version))` when
/// a non-empty version follows a `@`, else `(whole, None)`.
pub(crate) fn split_id_version(token: &str) -> (String, Option<String>) {
    match token.rfind('@') {
        Some(i) if i > 0 && i + 1 < token.len() => {
            (token[..i].to_string(), Some(token[i + 1..].to_string()))
        }
        _ => (token.to_string(), None),
    }
}

#[derive(Debug)]
pub struct PluginRoute {
    pub plugin_id: String,
    pub subcommand: String,
    pub remaining: Vec<String>,
}

/// Resolves against the live filesystem.
pub fn resolve(parsed: &Parsed) -> Route {
    let (manifests, enabled) = current_scan();
    resolve_with(&parsed.rest, &manifests, &enabled)
}

/// Pure resolver — takes pre-scanned data. Used by tests.
pub fn resolve_with(
    rest: &[String],
    manifests: &[(PluginManifest, PathBuf)],
    enabled: &HashMap<String, bool>,
) -> Route {
    let first = match rest.first() {
        Some(s) => s.clone(),
        None => return Route::Builtin(Builtin::Help { topic: None, all: false }),
    };

    if matches!(first.as_str(), "help" | "-h" | "--help") {
        let mut topic: Option<String> = None;
        let mut all = false;
        for a in rest.iter().skip(1) {
            if a == "--all" { all = true; }
            else if topic.is_none() { topic = Some(a.clone()); }
        }
        return Route::Builtin(Builtin::Help { topic, all });
    }

    if matches!(first.as_str(), "version" | "-v" | "--version") {
        return Route::Builtin(Builtin::Version);
    }

    // Core, never disabled: an agent's search must not depend on plugin state.
    if first == "search" {
        return Route::Builtin(Builtin::Search(super::search::parse_args(&rest[1..], false)));
    }

    // Core, never disabled: a broken plugin state is exactly when doctor is
    // needed most, so it must not be routable through plugin matching.
    if first == "doctor" {
        return Route::Builtin(Builtin::Doctor(super::doctor::parse_args(&rest[1..], false)));
    }

    // Core, never disabled: an agent's retrieval path must not depend on
    // plugin state — same reasoning as `search`/`doctor` above.
    if first == "mcp" {
        return Route::Builtin(Builtin::Mcp);
    }

    if first == "plugin" {
        return match rest.get(1).map(|s| s.as_str()) {
            Some("list") => Route::Builtin(Builtin::PluginList),
            Some("enable") => match rest.get(2) {
                Some(id) => Route::Builtin(Builtin::PluginEnable(id.clone())),
                None => Route::Unknown("plugin enable (missing id)".to_string()),
            },
            Some("disable") => match rest.get(2) {
                Some(id) => Route::Builtin(Builtin::PluginDisable(id.clone())),
                None => Route::Unknown("plugin disable (missing id)".to_string()),
            },
            Some("info") => match rest.get(2) {
                Some(id) => Route::Builtin(Builtin::PluginInfo(id.clone())),
                None => Route::Unknown("plugin info (missing id)".to_string()),
            },
            Some("install") => match rest.get(2) {
                Some(spec) => {
                    let (id, version) = split_id_version(spec);
                    Route::Builtin(Builtin::PluginInstall(id, version))
                }
                None => Route::Unknown("plugin install (missing id)".to_string()),
            },
            Some("update") | Some("upgrade") => {
                // Optional id positional; ignore trailing flags for now.
                let id = rest.iter().skip(2).find(|a| !a.starts_with('-')).cloned();
                Route::Builtin(Builtin::PluginUpdate(id))
            }
            Some("remove") | Some("uninstall") => {
                let keep_data = rest.iter().any(|a| a == "--keep-data");
                match rest.iter().skip(2).find(|a| !a.starts_with('-')) {
                    Some(id) => Route::Builtin(Builtin::PluginRemove(id.clone(), keep_data)),
                    None => Route::Unknown("plugin remove (missing id)".to_string()),
                }
            }
            _ => Route::Unknown(format!("plugin {}", rest.get(1).cloned().unwrap_or_default())),
        };
    }

    // reading-insights uses the two-level `notemd reading-insights report` form
    // and is handled through the webview runner (reusing the in-app report logic,
    // incl. online audience). Core-ized: no plugin process, and the enabled map
    // is deliberately not consulted — core commands cannot be disabled.
    if first == "reading-insights" {
        let skip = match rest.get(1).map(|s| s.as_str()) {
            Some("report") => 2,
            Some(s) if s.starts_with('-') => 1, // flags → implicit `report`
            None => 1,
            Some(other) => return Route::Unknown(format!("reading-insights {}", other)),
        };
        let remaining: Vec<String> = rest.iter().skip(skip).cloned().collect();
        return Route::Plugin(PluginRoute {
            plugin_id: "reading-insights".to_string(),
            subcommand: "report".to_string(),
            remaining,
        });
    }

    let resolved = match_against_manifests(manifests, &first, enabled);
    match resolved {
        Some((plugin_id, subcommand, is_enabled)) => {
            if is_enabled {
                Route::Plugin(PluginRoute {
                    plugin_id,
                    subcommand,
                    remaining: rest.iter().skip(1).cloned().collect(),
                })
            } else {
                Route::Disabled { plugin_id, subcommand }
            }
        }
        None => Route::Unknown(first),
    }
}

fn match_against_manifests(
    manifests: &[(PluginManifest, PathBuf)],
    token: &str,
    enabled: &HashMap<String, bool>,
) -> Option<(String, String, bool)> {
    for (m, _dir) in manifests {
        for entry in &m.cli {
            if entry.subcommand == token || entry.aliases.iter().any(|a| a == token) {
                let is_enabled = super::is_enabled(m, enabled);
                return Some((m.id.clone(), entry.subcommand.clone(), is_enabled));
            }
        }
    }
    None
}

fn current_scan() -> (Vec<(PluginManifest, PathBuf)>, HashMap<String, bool>) {
    let mut manifests = Vec::new();
    let mut enabled = HashMap::new();
    // core 化的 share / reading-insights 无磁盘 manifest，注入 stub 参与匹配。
    super::runner::append_core_cli_stubs(&mut manifests, &mut enabled);
    // 安装的插件（adapter 转成前端/CLI 视图模型形状），泛型匹配直接吃 cli 条目。
    super::runner::append_v2_manifests(&mut manifests, &mut enabled);
    (manifests, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::{PluginManifest, CliEntry};
    use std::path::PathBuf;

    fn manifest_with_cli(id: &str, sub: &str, aliases: &[&str]) -> PluginManifest {
        PluginManifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "0.1.0".to_string(),
            description: None,
            kind: crate::plugin_host::PluginKind::External,
            binary: Some("bin".to_string()),
            // A CLI stub is not an agent provider; the field exists so the
            // shape matches what the adapter produces.
            agent_provider: false,
            default_enabled: None,
            menus: vec![],
            context_menus: vec![],
            custom_editors: vec![],
            settings: None,
            host_capabilities: vec![],
            timeout_seconds: 30,
            i18n: std::collections::HashMap::new(),
            manifest_version: None,
            open_windows: None,
            cli: vec![CliEntry {
                subcommand: sub.to_string(),
                aliases: aliases.iter().map(|s| s.to_string()).collect(),
                command: "noop".to_string(),
                summary: "s".to_string(),
                args: vec![],
                flags: vec![],
                requires_tab_context: false,
            }],
        }
    }

    fn route_with(
        rest: &[&str],
        manifests: Vec<(PluginManifest, PathBuf)>,
        enabled: std::collections::HashMap<String, bool>,
    ) -> Route {
        resolve_with(
            &rest.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            &manifests,
            &enabled,
        )
    }

    #[test] fn no_args_is_help() {
        let r = route_with(&[], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::Help { .. })));
    }
    #[test] fn help_subcommand_routes_to_help() {
        let r = route_with(&["help"], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::Help { topic: None, all: false })));
    }
    #[test] fn help_with_topic_carries_topic() {
        let r = route_with(&["help", "share"], vec![], Default::default());
        let Route::Builtin(Builtin::Help { topic, all }) = r else { panic!() };
        assert_eq!(topic.as_deref(), Some("share"));
        assert!(!all);
    }
    #[test] fn dash_h_routes_to_help() {
        let r = route_with(&["-h"], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::Help { .. })));
    }
    #[test] fn version_routes() {
        let r = route_with(&["version"], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::Version)));
    }
    #[test] fn plugin_list_routes() {
        let r = route_with(&["plugin", "list"], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::PluginList)));
    }
    #[test] fn plugin_enable_with_id_routes() {
        let r = route_with(&["plugin", "enable", "share"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginEnable(id)) = r else { panic!() };
        assert_eq!(id, "share");
    }

    #[test] fn plugin_install_with_version_routes() {
        let r = route_with(&["plugin", "install", "x@1.2.0"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginInstall(id, ver)) = r else { panic!() };
        assert_eq!(id, "x");
        assert_eq!(ver.as_deref(), Some("1.2.0"));
    }
    #[test] fn plugin_install_without_version_routes() {
        let r = route_with(&["plugin", "install", "x"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginInstall(id, ver)) = r else { panic!() };
        assert_eq!(id, "x");
        assert_eq!(ver, None);
    }
    #[test] fn plugin_install_id_with_at_splits_on_last() {
        // A scoped-style id keeps its own '@'; only the last '@version' splits.
        let r = route_with(&["plugin", "install", "@scope/pkg@2.0.0"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginInstall(id, ver)) = r else { panic!() };
        assert_eq!(id, "@scope/pkg");
        assert_eq!(ver.as_deref(), Some("2.0.0"));
    }
    #[test] fn plugin_install_missing_id_is_unknown() {
        let r = route_with(&["plugin", "install"], vec![], Default::default());
        assert!(matches!(r, Route::Unknown(_)));
    }
    #[test] fn plugin_update_all_and_one() {
        let r = route_with(&["plugin", "update"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginUpdate(id)) = r else { panic!() };
        assert_eq!(id, None);
        let r = route_with(&["plugin", "update", "x"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginUpdate(id)) = r else { panic!() };
        assert_eq!(id.as_deref(), Some("x"));
    }
    #[test] fn plugin_remove_and_uninstall_with_keep_data() {
        let r = route_with(&["plugin", "remove", "x"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginRemove(id, keep)) = r else { panic!() };
        assert_eq!(id, "x");
        assert!(!keep);
        // `uninstall` alias + --keep-data (flag before id).
        let r = route_with(&["plugin", "uninstall", "--keep-data", "x"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginRemove(id, keep)) = r else { panic!() };
        assert_eq!(id, "x");
        assert!(keep);
    }
    #[test] fn plugin_remove_missing_id_is_unknown() {
        let r = route_with(&["plugin", "remove", "--keep-data"], vec![], Default::default());
        assert!(matches!(r, Route::Unknown(_)));
    }
    #[test] fn split_id_version_cases() {
        assert_eq!(split_id_version("x"), ("x".to_string(), None));
        assert_eq!(split_id_version("x@1.0.0"), ("x".to_string(), Some("1.0.0".to_string())));
        assert_eq!(split_id_version("a@b@1.0.0"), ("a@b".to_string(), Some("1.0.0".to_string())));
        // Trailing '@' with no version ⇒ no version.
        assert_eq!(split_id_version("x@"), ("x@".to_string(), None));
        // Leading '@' with no later '@' ⇒ whole thing is the id.
        assert_eq!(split_id_version("@scope/x"), ("@scope/x".to_string(), None));
    }
    #[test] fn enabled_plugin_subcommand_routes_to_plugin() {
        let m = manifest_with_cli("share", "share", &["-s"]);
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("share".to_string(), true);
        let r = route_with(&["share", "draft.md"], vec![(m, PathBuf::from("/tmp"))], enabled);
        let Route::Plugin(p) = r else { panic!() };
        assert_eq!(p.plugin_id, "share");
        assert_eq!(p.subcommand, "share");
        assert_eq!(p.remaining, vec!["draft.md".to_string()]);
    }
    #[test] fn enabled_plugin_alias_resolves_to_subcommand() {
        let m = manifest_with_cli("share", "share", &["-s"]);
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("share".to_string(), true);
        let r = route_with(&["-s", "draft.md"], vec![(m, PathBuf::from("/tmp"))], enabled);
        let Route::Plugin(p) = r else { panic!() };
        assert_eq!(p.subcommand, "share");
        assert_eq!(p.remaining, vec!["draft.md".to_string()]);
    }
    #[test] fn disabled_plugin_yields_disabled_route() {
        let m = manifest_with_cli("share", "share", &["-s"]);
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("share".to_string(), false);
        let r = route_with(&["-s", "x.md"], vec![(m, PathBuf::from("/tmp"))], enabled);
        let Route::Disabled { plugin_id, subcommand } = r else { panic!() };
        assert_eq!(plugin_id, "share");
        assert_eq!(subcommand, "share");
    }
    #[test] fn unknown_command_yields_unknown() {
        let r = route_with(&["nope"], vec![], Default::default());
        let Route::Unknown(name) = r else { panic!() };
        assert_eq!(name, "nope");
    }

    /// `mcp` 是 core:agent 的检索入口不能被插件遮蔽,也不能被禁用。
    #[test]
    fn mcp_routes_as_builtin() {
        let r = route_with(&["mcp"], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::Mcp)), "got {r:?}");
    }

    #[test]
    fn mcp_is_not_shadowed_by_a_plugin() {
        let m = manifest_with_cli("evil", "mcp", &[]);
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("evil".to_string(), true);
        let r = route_with(&["mcp"], vec![(m, PathBuf::from("/tmp"))], enabled);
        assert!(matches!(r, Route::Builtin(Builtin::Mcp)), "got {r:?}");
    }

    #[test]
    fn doctor_routes_as_builtin() {
        let r = route_with(&["doctor", "--offline"], vec![], Default::default());
        let Route::Builtin(Builtin::Doctor(args)) = r else { panic!("expected doctor builtin") };
        assert!(args.offline);
    }

    /// doctor 是 core：即便某个插件声明了同名 cli 子命令，也绝不能被遮蔽。
    #[test]
    fn doctor_is_not_shadowed_by_a_plugin() {
        let m = manifest_with_cli("evil", "doctor", &[]);
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("evil".to_string(), true);
        let r = route_with(&["doctor"], vec![(m, PathBuf::from("/tmp"))], enabled);
        assert!(matches!(r, Route::Builtin(Builtin::Doctor(_))), "got {r:?}");
    }

    #[test]
    fn share_routes_without_manifest() {
        // share 是 core：无 manifest 时也必须路由成功（core stub 由 current_scan 注入，
        // 纯函数层直接喂 stub 验证匹配逻辑）。
        let stubs = crate::cli::runner::core_cli_stub_manifests();
        let pairs: Vec<(PluginManifest, PathBuf)> =
            stubs.into_iter().map(|m| (m, PathBuf::new())).collect();
        let r = resolve_with(
            &vec!["share".into(), "/tmp/a.md".into()],
            &pairs,
            &HashMap::new(),
        );
        match r {
            Route::Plugin(p) => assert_eq!(p.plugin_id, "share"),
            other => panic!("expected share plugin route, got {:?}", other),
        }
    }

    #[test]
    fn share_alias_routes_via_stub() {
        // `--share` 别名也必须由 stub 覆盖（原 manifest 声明的 aliases）。
        let stubs = crate::cli::runner::core_cli_stub_manifests();
        let pairs: Vec<(PluginManifest, PathBuf)> =
            stubs.into_iter().map(|m| (m, PathBuf::new())).collect();
        let r = resolve_with(
            &vec!["--share".into(), "/tmp/a.md".into()],
            &pairs,
            &HashMap::new(),
        );
        match r {
            Route::Plugin(p) => {
                assert_eq!(p.plugin_id, "share");
                assert_eq!(p.subcommand, "share");
            }
            other => panic!("expected share plugin route, got {:?}", other),
        }
    }

    #[test]
    fn reading_insights_never_disabled() {
        let r = resolve_with(
            &vec!["reading-insights".into(), "report".into()],
            &[],
            &HashMap::from([("reading-insights".to_string(), false)]),
        );
        assert!(matches!(r, Route::Plugin(_)), "core-ized: enabled map must be ignored");
    }

    /// Composition test for the CLI merge: an install tree scanned by
    /// `discovery::scan_root` and adapted via `adapter::to_v1` must route its
    /// cli subcommand. Uses a fixture id so it stays independent of any real
    /// plugin, and exercises the scan→adapt→route pipeline without touching
    /// current_scan's real dirs.
    #[test]
    fn adapted_manifest_routes_subcommand() {
        use crate::plugin_runtime::state::{InstallState, InstalledPlugin};
        use crate::plugin_runtime::{adapter, discovery, state};

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // state.json marking the fixture enabled.
        let mut st = InstallState::default();
        st.installed.insert(
            "notemd.fixture".to_string(),
            InstalledPlugin { version: "1.0.0".into(), enabled: true },
        );
        state::save(root, &st).unwrap();

        // <root>/notemd.fixture/current/: manifest.json + dummy binary.
        let triple = discovery::current_arch_triple().expect("supported arch");
        let current = root.join("notemd.fixture").join("current");
        std::fs::create_dir_all(current.join("bin")).unwrap();
        std::fs::write(current.join("bin/fixture"), b"#!/bin/sh\nexit 0\n").unwrap();
        let manifest = serde_json::json!({
            "manifest_version": 2,
            "id": "notemd.fixture",
            "name": "Fixture",
            "version": "1.0.0",
            "kind": "native",
            "engines": { "notemd": ">=0.0.0" },
            "binary": { triple: "bin/fixture" },
            "activation": { "events": ["onCli:pdf2"] },
            "contributes": {
                "cli": [{ "subcommand": "pdf2", "command": "export",
                          "summary": "x", "args": [], "flags": [] }]
            },
            "capabilities": []
        });
        std::fs::write(current.join("manifest.json"), manifest.to_string()).unwrap();

        let scanned = discovery::scan_root(root, "1.0.0");
        assert_eq!(scanned.len(), 1);
        let pairs: Vec<(PluginManifest, PathBuf)> = scanned
            .into_iter()
            .filter_map(|(id, (m, install_dir))| match adapter::to_v1(&m) {
                Ok(v1) => Some((v1, install_dir)),
                Err(e) => {
                    eprintln!("[test] {id}: contributes could not be adapted: {e}");
                    None
                }
            })
            .collect();
        let enabled = HashMap::from([("notemd.fixture".to_string(), true)]);

        let r = resolve_with(&vec!["pdf2".into(), "x.md".into()], &pairs, &enabled);
        let Route::Plugin(p) = r else { panic!("expected v2 plugin route, got {r:?}") };
        assert_eq!(p.plugin_id, "notemd.fixture");
        assert_eq!(p.subcommand, "pdf2");
        assert_eq!(p.remaining, vec!["x.md".to_string()]);
    }
}
