//! Routing: argv → Route. Step order matches spec §3 exactly.

use super::args::Parsed;
use crate::plugin_host::PluginManifest;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug)]
pub enum Route {
    Builtin(Builtin),
    Plugin(PluginRoute),
    /// A subcommand whose owning plugin is installed but disabled.
    Disabled {
        plugin_id: String,
        subcommand: String,
    },
    Unknown(String),
}

#[derive(Debug)]
pub enum Builtin {
    /// A recognized built-in command with an invalid argv shape. Keeping this
    /// distinct from `Unknown` gives scripts the documented exit code 2 rather
    /// than 127 and, critically, prevents destructive fallback behavior.
    ArgumentError(String),
    Help {
        topic: Option<String>,
        all: bool,
    },
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
    /// `memory` — controlled USER/MEMORY proposals, review and integrity.
    Memory(super::memory::MemoryArgs),
    /// `notemd .` / `notemd xxx.md` — open paths in the desktop app. Holds the
    /// raw tokens; `open::run` resolves them against the cwd and reports a
    /// missing file. Lowest precedence: only reached when nothing matched a
    /// command, so a command name never loses to a same-named file (write
    /// `./search` for that).
    Open(Vec<String>),
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
    if !parsed.errors.is_empty() {
        return argument_error(parsed.errors.join("; "));
    }
    let (manifests, enabled) = current_scan();
    let route = resolve_with(&parsed.rest, &manifests, &enabled);
    // Nothing matched a command → the tokens may still name files/directories
    // to open (`notemd .`). The probe needs the disk, which is exactly what
    // `resolve_with` is kept free of, so it happens out here.
    if matches!(route, Route::Unknown(_)) {
        if let Some(open) = super::open::route_unmatched(&parsed.rest) {
            return open;
        }
    }
    route
}

/// Pure resolver — takes pre-scanned data. Used by tests.
pub fn resolve_with(
    rest: &[String],
    manifests: &[(PluginManifest, PathBuf)],
    enabled: &HashMap<String, bool>,
) -> Route {
    let first = match rest.first() {
        Some(s) => s.clone(),
        None => {
            return Route::Builtin(Builtin::Help {
                topic: None,
                all: false,
            })
        }
    };

    if matches!(first.as_str(), "help" | "-h" | "--help") {
        let mut topic: Option<String> = None;
        let mut all = false;
        for a in rest.iter().skip(1) {
            let known_alias = matches!(a.as_str(), "-h" | "--help" | "-v" | "--version" | "--share")
                || manifests.iter().any(|(manifest, _)| {
                    manifest.cli.iter().any(|entry| entry.aliases.iter().any(|alias| alias == a))
                });
            if a == "--all" && !all {
                all = true;
            } else if a == "--all" {
                return argument_error("--all may only be specified once");
            } else if a.starts_with('-') && !known_alias {
                return argument_error(format!("unknown flag '{a}' for help"));
            } else if topic.is_none() {
                topic = Some(a.clone());
            } else {
                return argument_error("help accepts at most one command topic");
            }
        }
        return Route::Builtin(Builtin::Help { topic, all });
    }

    if matches!(first.as_str(), "version" | "-v" | "--version") {
        return if rest.len() == 1 {
            Route::Builtin(Builtin::Version)
        } else {
            argument_error("version does not accept arguments")
        };
    }

    // Core, never disabled: an agent's search must not depend on plugin state.
    if first == "search" {
        return Route::Builtin(Builtin::Search(super::search::parse_args(
            &rest[1..],
            false,
        )));
    }

    // Core, never disabled: a broken plugin state is exactly when doctor is
    // needed most, so it must not be routable through plugin matching.
    if first == "doctor" {
        return Route::Builtin(Builtin::Doctor(super::doctor::parse_args(
            &rest[1..],
            false,
        )));
    }

    // Core, never disabled: an agent's retrieval path must not depend on
    // plugin state — same reasoning as `search`/`doctor` above.
    if first == "mcp" {
        return if rest.len() == 1 {
            Route::Builtin(Builtin::Mcp)
        } else {
            argument_error("mcp does not accept arguments")
        };
    }

    // Core, never disabled: Agents must always be able to inspect and propose
    // memory changes even when the optional review window is disabled.
    if first == "memory" {
        return Route::Builtin(Builtin::Memory(super::memory::parse_args(
            &rest[1..],
            false,
        )));
    }

    if first == "plugin" {
        return match rest.get(1).map(|s| s.as_str()) {
            Some("list") => no_plugin_tail("list", &rest[2..], Builtin::PluginList),
            Some("enable") => one_plugin_id("enable", &rest[2..], Builtin::PluginEnable),
            Some("disable") => one_plugin_id("disable", &rest[2..], Builtin::PluginDisable),
            Some("info") => one_plugin_id("info", &rest[2..], Builtin::PluginInfo),
            Some("install") => match strict_optional_id("install", &rest[2..], true) {
                Ok(Some(spec)) => {
                    let (id, version) = split_id_version(&spec);
                    Route::Builtin(Builtin::PluginInstall(id, version))
                }
                Ok(None) => argument_error("plugin install requires <id>[@version]"),
                Err(e) => argument_error(e),
            },
            Some("update") | Some("upgrade") => {
                match strict_optional_id("update", &rest[2..], false) {
                    Ok(id) => Route::Builtin(Builtin::PluginUpdate(id)),
                    Err(e) => argument_error(e),
                }
            }
            Some("remove") | Some("uninstall") => parse_plugin_remove(&rest[2..]),
            Some(other) => argument_error(format!("unknown plugin subcommand '{other}'")),
            None => argument_error("plugin requires a subcommand"),
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
            Some(other) => {
                return argument_error(format!(
                    "unknown reading-insights subcommand '{other}'"
                ));
            }
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
                Route::Disabled {
                    plugin_id,
                    subcommand,
                }
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

fn argument_error(message: impl Into<String>) -> Route {
    Route::Builtin(Builtin::ArgumentError(message.into()))
}

fn no_plugin_tail(action: &str, tail: &[String], builtin: Builtin) -> Route {
    if tail.is_empty() {
        Route::Builtin(builtin)
    } else {
        argument_error(format!(
            "plugin {action} does not accept arguments: {}",
            tail.join(" ")
        ))
    }
}

fn one_plugin_id(action: &str, tail: &[String], make: fn(String) -> Builtin) -> Route {
    match strict_optional_id(action, tail, true) {
        Ok(Some(id)) => Route::Builtin(make(id)),
        Ok(None) => argument_error(format!("plugin {action} requires <plugin-id>")),
        Err(e) => argument_error(e),
    }
}

fn strict_optional_id(
    action: &str,
    tail: &[String],
    required: bool,
) -> Result<Option<String>, String> {
    match tail {
        [] if required => Ok(None),
        [] => Ok(None),
        [id] if !id.starts_with('-') => Ok(Some(id.clone())),
        [flag] if flag.starts_with('-') => {
            Err(format!("unknown flag '{flag}' for plugin {action}"))
        }
        _ => Err(format!("plugin {action} accepts at most one plugin id")),
    }
}

fn parse_plugin_remove(tail: &[String]) -> Route {
    let mut id: Option<String> = None;
    let mut keep_data = false;
    for token in tail {
        match token.as_str() {
            "--keep-data" if !keep_data => keep_data = true,
            "--keep-data" => return argument_error("--keep-data may only be specified once"),
            flag if flag.starts_with('-') => {
                return argument_error(format!("unknown flag '{flag}' for plugin remove"));
            }
            value if id.is_none() => id = Some(value.to_string()),
            _ => return argument_error("plugin remove accepts exactly one plugin id"),
        }
    }
    match id {
        Some(id) => Route::Builtin(Builtin::PluginRemove(id, keep_data)),
        None => argument_error("plugin remove requires <plugin-id>"),
    }
}

#[derive(Debug)]
pub(crate) struct CliInventoryIssue {
    pub id: String,
    pub version: String,
    pub enabled: bool,
    pub error: String,
}

/// Append every valid installed plugin, including disabled ones. Invalid
/// installations are returned as diagnostics so management commands can show
/// them instead of pretending they are not installed.
pub(crate) fn append_installed_cli_manifests(
    manifests: &mut Vec<(PluginManifest, PathBuf)>,
    enabled: &mut HashMap<String, bool>,
) -> Vec<CliInventoryIssue> {
    let Some(root) = super::runner::v2_plugins_root() else {
        return Vec::new();
    };
    append_installed_cli_manifests_at(&root, manifests, enabled)
}

fn append_installed_cli_manifests_at(
    root: &std::path::Path,
    manifests: &mut Vec<(PluginManifest, PathBuf)>,
    enabled: &mut HashMap<String, bool>,
) -> Vec<CliInventoryIssue> {
    let mut issues = Vec::new();
    for entry in
        crate::plugin_runtime::discovery::scan_root_inventory(root, env!("CARGO_PKG_VERSION"))
    {
        // Core providers win by id as well as by command token. Do not let a
        // state.json entry named `share` overwrite the core stub's enabled bit.
        if manifests
            .iter()
            .any(|(existing, _)| existing.id == entry.id)
        {
            issues.push(CliInventoryIssue {
                id: entry.id,
                version: entry.state_version,
                enabled: entry.enabled,
                error: "plugin id conflicts with a core command provider".into(),
            });
            continue;
        }
        enabled.insert(entry.id.clone(), entry.enabled);
        match entry.manifest {
            Ok(v2) => match crate::plugin_runtime::adapter::to_v1(&v2) {
                Ok(v1) => manifests.push((v1, entry.current_dir)),
                Err(error) => issues.push(CliInventoryIssue {
                    id: entry.id,
                    version: entry.state_version,
                    enabled: entry.enabled,
                    error,
                }),
            },
            Err(error) => issues.push(CliInventoryIssue {
                id: entry.id,
                version: entry.state_version,
                enabled: entry.enabled,
                error,
            }),
        }
    }
    issues
}

/// Reject every dynamic CLI entry that uses a core-reserved token or shares a
/// token with another plugin entry. Dropping all owners of an ambiguous token
/// is deterministic and safer than whichever manifest happened to scan first.
#[derive(Debug)]
pub(crate) struct CliNameConflict {
    pub plugin_id: String,
    pub entry: String,
    pub reasons: Vec<String>,
}

pub(crate) fn reject_cli_name_conflicts(
    manifests: &mut [(PluginManifest, PathBuf)],
) -> Vec<CliNameConflict> {
    use std::collections::{BTreeMap, BTreeSet};

    const RESERVED: &[&str] = &[
        "help", "-h", "--help", "version", "-v", "--version", "plugin", "search",
        "doctor", "mcp", "memory", "open", "reading-insights", "share", "--share",
        "--json", "-q", "--quiet", "--no-clipboard", "--cli", "-y", "--yes",
    ];
    let mut owners: BTreeMap<String, BTreeSet<(usize, usize)>> = BTreeMap::new();
    let mut rejected: BTreeMap<(usize, usize), BTreeSet<String>> = BTreeMap::new();

    for (manifest_idx, (manifest, _)) in manifests.iter().enumerate() {
        if crate::cli::runner::is_core_cli_stub(manifest) {
            continue;
        }
        for (entry_idx, entry) in manifest.cli.iter().enumerate() {
            for token in std::iter::once(&entry.subcommand).chain(entry.aliases.iter()) {
                owners
                    .entry(token.clone())
                    .or_default()
                    .insert((manifest_idx, entry_idx));
                if RESERVED.contains(&token.as_str()) {
                    rejected
                        .entry((manifest_idx, entry_idx))
                        .or_default()
                        .insert(format!("reserved CLI name '{token}'"));
                }
            }
        }
    }
    for (token, token_owners) in owners {
        if token_owners.len() > 1 {
            for owner in token_owners {
                rejected
                    .entry(owner)
                    .or_default()
                    .insert(format!("ambiguous CLI name '{token}'"));
            }
        }
    }

    let mut conflicts = Vec::new();
    for (manifest_idx, (manifest, _)) in manifests.iter_mut().enumerate() {
        let mut entry_idx = 0usize;
        manifest.cli.retain(|entry| {
            let reasons = rejected.get(&(manifest_idx, entry_idx));
            entry_idx += 1;
            if let Some(reasons) = reasons {
                conflicts.push(CliNameConflict {
                    plugin_id: manifest.id.clone(),
                    entry: entry.subcommand.clone(),
                    reasons: reasons.iter().cloned().collect(),
                });
                false
            } else {
                true
            }
        });
    }
    conflicts
}

fn current_scan() -> (Vec<(PluginManifest, PathBuf)>, HashMap<String, bool>) {
    let mut manifests = Vec::new();
    let mut enabled = HashMap::new();
    // core 化的 share / reading-insights 无磁盘 manifest，注入 stub 参与匹配。
    super::runner::append_core_cli_stubs(&mut manifests, &mut enabled);
    // Management-aware inventory keeps valid disabled manifests routable to
    // Route::Disabled. Runtime execution still uses its enabled-only scan.
    // Routing must not leak unrelated installation diagnostics into every
    // command's stderr (especially a machine-readable `--json` invocation).
    // `plugin list/info` and `doctor` surface these issues deliberately.
    let _ = append_installed_cli_manifests(&mut manifests, &mut enabled);
    let _ = reject_cli_name_conflicts(&mut manifests);
    (manifests, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::{CliEntry, PluginManifest};
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

    #[test]
    fn no_args_is_help() {
        let r = route_with(&[], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::Help { .. })));
    }
    #[test]
    fn help_subcommand_routes_to_help() {
        let r = route_with(&["help"], vec![], Default::default());
        assert!(matches!(
            r,
            Route::Builtin(Builtin::Help {
                topic: None,
                all: false
            })
        ));
    }
    #[test]
    fn help_with_topic_carries_topic() {
        let r = route_with(&["help", "share"], vec![], Default::default());
        let Route::Builtin(Builtin::Help { topic, all }) = r else {
            panic!()
        };
        assert_eq!(topic.as_deref(), Some("share"));
        assert!(!all);
    }
    #[test]
    fn help_rejects_unknown_flags_and_multiple_topics() {
        for args in [
            vec!["help", "--bogus"],
            vec!["help", "search", "doctor"],
            vec!["help", "--all", "--all"],
        ] {
            let route = route_with(&args, vec![], Default::default());
            assert!(
                matches!(route, Route::Builtin(Builtin::ArgumentError(_))),
                "got {route:?}"
            );
        }
        let route = route_with(&["help", "--share"], vec![], Default::default());
        assert!(matches!(
            route,
            Route::Builtin(Builtin::Help { topic: Some(_), .. })
        ));
        for alias in ["-h", "--help", "-v", "--version"] {
            let route = route_with(&["help", alias], vec![], Default::default());
            assert!(matches!(
                route,
                Route::Builtin(Builtin::Help { topic: Some(_), .. })
            ), "help topic alias {alias} must remain reachable");
        }
        let plugin = manifest_with_cli("demo.plugin", "demo", &["-d"]);
        let route = route_with(&["help", "-d"], vec![(plugin, PathBuf::new())], Default::default());
        assert!(matches!(
            route,
            Route::Builtin(Builtin::Help { topic: Some(_), .. })
        ));
    }
    #[test]
    fn dash_h_routes_to_help() {
        let r = route_with(&["-h"], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::Help { .. })));
    }
    #[test]
    fn version_routes() {
        let r = route_with(&["version"], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::Version)));
    }
    #[test]
    fn version_and_mcp_reject_extra_arguments() {
        for args in [vec!["version", "extra"], vec!["mcp", "--bogus"]] {
            let route = route_with(&args, vec![], Default::default());
            assert!(
                matches!(route, Route::Builtin(Builtin::ArgumentError(_))),
                "got {route:?}"
            );
        }
    }
    #[test]
    fn plugin_list_routes() {
        let r = route_with(&["plugin", "list"], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::PluginList)));
    }
    #[test]
    fn plugin_enable_with_id_routes() {
        let r = route_with(&["plugin", "enable", "share"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginEnable(id)) = r else {
            panic!()
        };
        assert_eq!(id, "share");
    }

    #[test]
    fn plugin_install_with_version_routes() {
        let r = route_with(
            &["plugin", "install", "x@1.2.0"],
            vec![],
            Default::default(),
        );
        let Route::Builtin(Builtin::PluginInstall(id, ver)) = r else {
            panic!()
        };
        assert_eq!(id, "x");
        assert_eq!(ver.as_deref(), Some("1.2.0"));
    }
    #[test]
    fn plugin_install_without_version_routes() {
        let r = route_with(&["plugin", "install", "x"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginInstall(id, ver)) = r else {
            panic!()
        };
        assert_eq!(id, "x");
        assert_eq!(ver, None);
    }
    #[test]
    fn plugin_install_id_with_at_splits_on_last() {
        // A scoped-style id keeps its own '@'; only the last '@version' splits.
        let r = route_with(
            &["plugin", "install", "@scope/pkg@2.0.0"],
            vec![],
            Default::default(),
        );
        let Route::Builtin(Builtin::PluginInstall(id, ver)) = r else {
            panic!()
        };
        assert_eq!(id, "@scope/pkg");
        assert_eq!(ver.as_deref(), Some("2.0.0"));
    }
    #[test]
    fn plugin_install_missing_id_is_argument_error() {
        let r = route_with(&["plugin", "install"], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::ArgumentError(_))));
    }
    #[test]
    fn plugin_update_all_and_one() {
        let r = route_with(&["plugin", "update"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginUpdate(id)) = r else {
            panic!()
        };
        assert_eq!(id, None);
        let r = route_with(&["plugin", "update", "x"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginUpdate(id)) = r else {
            panic!()
        };
        assert_eq!(id.as_deref(), Some("x"));
    }
    #[test]
    fn plugin_remove_and_uninstall_with_keep_data() {
        let r = route_with(&["plugin", "remove", "x"], vec![], Default::default());
        let Route::Builtin(Builtin::PluginRemove(id, keep)) = r else {
            panic!()
        };
        assert_eq!(id, "x");
        assert!(!keep);
        // `uninstall` alias + --keep-data (flag before id).
        let r = route_with(
            &["plugin", "uninstall", "--keep-data", "x"],
            vec![],
            Default::default(),
        );
        let Route::Builtin(Builtin::PluginRemove(id, keep)) = r else {
            panic!()
        };
        assert_eq!(id, "x");
        assert!(keep);
    }
    #[test]
    fn plugin_remove_missing_id_is_argument_error() {
        let r = route_with(
            &["plugin", "remove", "--keep-data"],
            vec![],
            Default::default(),
        );
        assert!(matches!(r, Route::Builtin(Builtin::ArgumentError(_))));
    }
    #[test]
    fn plugin_management_rejects_unknown_flags_and_extra_positionals() {
        for args in [
            vec!["plugin", "list", "--bogus"],
            vec!["plugin", "enable", "x", "extra"],
            vec!["plugin", "disable", "--bogus"],
            vec!["plugin", "info", "x", "extra"],
            vec!["plugin", "install", "x", "extra"],
            vec!["plugin", "update", "--bogus"],
            vec!["plugin", "upgrade", "x", "extra"],
            vec!["plugin", "remove", "x", "--keep-dtaa"],
            vec!["plugin", "remove", "x", "y"],
        ] {
            let route = route_with(&args, vec![], Default::default());
            assert!(
                matches!(route, Route::Builtin(Builtin::ArgumentError(_))),
                "expected argument error for {args:?}, got {route:?}",
            );
        }
    }
    #[test]
    fn recognized_command_with_unknown_subcommand_is_an_argument_error() {
        for args in [
            vec!["plugin", "wat"],
            vec!["reading-insights", "wat"],
        ] {
            let route = route_with(&args, vec![], Default::default());
            assert!(matches!(route, Route::Builtin(Builtin::ArgumentError(_))), "got {route:?}");
        }
    }
    #[test]
    fn plugin_update_unknown_flag_can_never_mean_update_all() {
        let route = route_with(&["plugin", "update", "--bogus"], vec![], Default::default());
        assert!(!matches!(
            route,
            Route::Builtin(Builtin::PluginUpdate(None))
        ));
        assert!(matches!(route, Route::Builtin(Builtin::ArgumentError(_))));
    }
    #[test]
    fn split_id_version_cases() {
        assert_eq!(split_id_version("x"), ("x".to_string(), None));
        assert_eq!(
            split_id_version("x@1.0.0"),
            ("x".to_string(), Some("1.0.0".to_string()))
        );
        assert_eq!(
            split_id_version("a@b@1.0.0"),
            ("a@b".to_string(), Some("1.0.0".to_string()))
        );
        // Trailing '@' with no version ⇒ no version.
        assert_eq!(split_id_version("x@"), ("x@".to_string(), None));
        // Leading '@' with no later '@' ⇒ whole thing is the id.
        assert_eq!(split_id_version("@scope/x"), ("@scope/x".to_string(), None));
    }
    #[test]
    fn enabled_plugin_subcommand_routes_to_plugin() {
        let m = manifest_with_cli("share", "share", &["-s"]);
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("share".to_string(), true);
        let r = route_with(
            &["share", "draft.md"],
            vec![(m, PathBuf::from("/tmp"))],
            enabled,
        );
        let Route::Plugin(p) = r else { panic!() };
        assert_eq!(p.plugin_id, "share");
        assert_eq!(p.subcommand, "share");
        assert_eq!(p.remaining, vec!["draft.md".to_string()]);
    }
    #[test]
    fn enabled_plugin_alias_resolves_to_subcommand() {
        let m = manifest_with_cli("share", "share", &["-s"]);
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("share".to_string(), true);
        let r = route_with(
            &["-s", "draft.md"],
            vec![(m, PathBuf::from("/tmp"))],
            enabled,
        );
        let Route::Plugin(p) = r else { panic!() };
        assert_eq!(p.subcommand, "share");
        assert_eq!(p.remaining, vec!["draft.md".to_string()]);
    }
    #[test]
    fn disabled_plugin_yields_disabled_route() {
        let m = manifest_with_cli("share", "share", &["-s"]);
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("share".to_string(), false);
        let r = route_with(&["-s", "x.md"], vec![(m, PathBuf::from("/tmp"))], enabled);
        let Route::Disabled {
            plugin_id,
            subcommand,
        } = r
        else {
            panic!()
        };
        assert_eq!(plugin_id, "share");
        assert_eq!(subcommand, "share");
    }
    #[test]
    fn unknown_command_yields_unknown() {
        let r = route_with(&["nope"], vec![], Default::default());
        let Route::Unknown(name) = r else { panic!() };
        assert_eq!(name, "nope");
    }
    #[test]
    fn conflicting_plugin_cli_names_are_removed_from_all_owners() {
        let first = manifest_with_cli("one", "duplicate", &[]);
        let second = manifest_with_cli("two", "other", &["duplicate"]);
        let mut manifests = vec![
            (first, PathBuf::from("/one")),
            (second, PathBuf::from("/two")),
        ];
        reject_cli_name_conflicts(&mut manifests);
        assert!(manifests[0].0.cli.is_empty());
        assert!(manifests[1].0.cli.is_empty());
    }
    #[test]
    fn core_reserved_cli_names_are_rejected() {
        for reserved in [
            "search",
            "open",
            "--json",
            "-q",
            "--quiet",
            "--no-clipboard",
            "--cli",
            "-y",
            "--yes",
        ] {
            let plugin = manifest_with_cli("evil", reserved, &[]);
            let mut manifests = vec![(plugin, PathBuf::from("/evil"))];
            let conflicts = reject_cli_name_conflicts(&mut manifests);
            assert!(manifests[0].0.cli.is_empty(), "reserved token survived: {reserved}");
            assert_eq!(conflicts.len(), 1);
            assert!(conflicts[0].reasons[0].contains(reserved));
        }
    }
    #[test]
    fn installed_core_id_cannot_disable_the_core_provider() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = crate::plugin_runtime::state::InstallState::default();
        state.installed.insert(
            "share".into(),
            crate::plugin_runtime::state::InstalledPlugin {
                version: "1.0.0".into(),
                enabled: false,
            },
        );
        crate::plugin_runtime::state::save(dir.path(), &state).unwrap();

        let mut manifests = crate::cli::runner::core_cli_stub_manifests()
            .into_iter()
            .map(|manifest| (manifest, PathBuf::new()))
            .collect::<Vec<_>>();
        let mut enabled = HashMap::from([
            ("share".to_string(), true),
            ("reading-insights".to_string(), true),
        ]);
        let issues = append_installed_cli_manifests_at(dir.path(), &mut manifests, &mut enabled);
        assert_eq!(enabled.get("share"), Some(&true));
        assert_eq!(issues.len(), 1);
        assert!(issues[0].error.contains("core command provider"));
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
    fn memory_routes_as_core_and_preserves_control_flags() {
        let r = route_with(
            &["memory", "propose", "--operation", "create"],
            vec![],
            Default::default(),
        );
        let Route::Builtin(Builtin::Memory(args)) = r else {
            panic!("expected memory builtin")
        };
        assert_eq!(args.action, "propose");
        assert_eq!(
            args.flags.get("operation").map(String::as_str),
            Some("create")
        );
    }

    #[test]
    fn doctor_routes_as_builtin() {
        let r = route_with(&["doctor", "--offline"], vec![], Default::default());
        let Route::Builtin(Builtin::Doctor(args)) = r else {
            panic!("expected doctor builtin")
        };
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
        assert!(
            matches!(r, Route::Plugin(_)),
            "core-ized: enabled map must be ignored"
        );
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
            InstalledPlugin {
                version: "1.0.0".into(),
                enabled: true,
            },
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
        let Route::Plugin(p) = r else {
            panic!("expected v2 plugin route, got {r:?}")
        };
        assert_eq!(p.plugin_id, "notemd.fixture");
        assert_eq!(p.subcommand, "pdf2");
        assert_eq!(p.remaining, vec!["x.md".to_string()]);
    }
}
