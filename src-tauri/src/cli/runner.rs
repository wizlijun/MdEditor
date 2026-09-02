//! Plugin-subcommand path: build CliPayload, launch headless Tauri, wait
//! for cli_finish from the frontend, exit.

use crate::cli::args::Parsed;
use crate::cli::router::PluginRoute;
use crate::cli::state::{CliPayload, CliState, GlobalFlags};
use crate::plugin_host::PluginManifest;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;

pub fn run(p: PluginRoute, parsed: Parsed) -> ExitCode {
    let (manifests, _enabled) = current_scan();
    let manifest = match manifests.iter().find(|(m, _)| m.id == p.plugin_id) {
        Some((m, _)) => m.clone(),
        None => {
            emit_cli_error(
                parsed.globals.json,
                "internal_error",
                &format!(
                    "plugin '{}' vanished between routing and execution",
                    p.plugin_id
                ),
            );
            return ExitCode::from(1);
        }
    };
    let cli_entry = match manifest.cli.iter().find(|c| c.subcommand == p.subcommand) {
        Some(e) => e.clone(),
        None => {
            emit_cli_error(
                parsed.globals.json,
                "internal_error",
                &format!("subcommand '{}' missing in '{}'", p.subcommand, p.plugin_id),
            );
            return ExitCode::from(1);
        }
    };

    // Parse remaining argv against the cli entry's spec.
    let (mut args, flags) = match parse_subcommand_args(&p.remaining, &cli_entry) {
        Ok(v) => v,
        Err(msg) => {
            emit_cli_error(parsed.globals.json, "invalid_arguments", &msg);
            return ExitCode::from(2);
        }
    };

    // Decide plugin_command via flags (--unshare/--copy-link/--update).
    let plugin_command = match decide_plugin_command(&flags, &cli_entry.command) {
        Ok(s) => s,
        Err(msg) => {
            emit_cli_error(parsed.globals.json, "invalid_arguments", &msg);
            return ExitCode::from(2);
        }
    };

    // Resolve only arguments declared as `path`; ordinary strings such as an
    // agent task id must remain byte-for-byte unchanged. Share's record-only
    // operations intentionally accept a deleted source path.
    let allow_missing_paths =
        p.plugin_id == "share" && matches!(plugin_command.as_str(), "copy-link" | "unpublish");
    if let Err(msg) = resolve_path_args(&mut args, &cli_entry, allow_missing_paths) {
        emit_cli_error(parsed.globals.json, "invalid_arguments", &msg);
        return ExitCode::from(2);
    }

    let timeout = watchdog_timeout(&manifest, &flags);
    let payload = CliPayload {
        subcommand: p.subcommand.clone(),
        plugin_id: p.plugin_id.clone(),
        plugin_command,
        args,
        flags,
        global: GlobalFlags {
            json: parsed.globals.json,
            quiet: parsed.globals.quiet,
            clipboard: parsed.globals.clipboard,
        },
    };
    let (tx, rx) = oneshot::channel();
    let state = CliState::new(payload, tx);

    let exit_code = launch_tauri_headless(state, rx, timeout, parsed.globals.json);
    ExitCode::from(exit_code as u8)
}

fn emit_cli_error(json: bool, code: &str, message: &str) {
    let message = message.strip_prefix("notemd: ").unwrap_or(message);
    if json {
        println!("{}", cli_error_value(code, message));
        let _ = std::io::stdout().flush();
    } else {
        eprintln!("notemd: {message}");
        let _ = std::io::stderr().flush();
    }
}

fn cli_error_value(code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "error": { "code": code, "message": message },
    })
}

fn current_scan() -> (
    Vec<(PluginManifest, PathBuf)>,
    std::collections::HashMap<String, bool>,
) {
    let mut manifests = Vec::new();
    let mut enabled = std::collections::HashMap::new();
    append_core_cli_stubs(&mut manifests, &mut enabled);
    append_v2_manifests(&mut manifests, &mut enabled);
    (manifests, enabled)
}

/// CLI equivalent of Tauri's `app_data_dir()` plugins root: on macOS both
/// resolve to `~/Library/Application Support/<BUNDLE_ID>`. The equivalence
/// assumption is documented by `data_dir_matches_tauri_app_data_dir` below.
pub(crate) fn v2_plugins_root() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(crate::app_dirs::BUNDLE_ID).join("plugins"))
}

/// Merge the installed plugins (adapted to the `PluginManifest` view-model
/// shape) into the CLI scan so router/runner matching sees them alongside the
/// core stubs. Skips ids already present (same de-dup guard as the core stubs);
/// every plugin appended here is enabled — enable/disable lives in the runtime's
/// state.json, which discovery already honors.
pub(crate) fn append_v2_manifests(
    manifests: &mut Vec<(PluginManifest, PathBuf)>,
    enabled: &mut std::collections::HashMap<String, bool>,
) {
    let Some(root) = v2_plugins_root() else {
        return;
    };
    let host_version = env!("CARGO_PKG_VERSION");
    for entry in crate::plugin_runtime::discovery::scan_root_inventory(&root, host_version) {
        if !entry.enabled {
            continue;
        }
        let Ok(m) = entry.manifest else {
            continue;
        };
        let id = entry.id;
        let install_dir = entry.current_dir;
        if manifests.iter().any(|(existing, _)| existing.id == id) {
            continue;
        }
        if let Ok(v1) = crate::plugin_runtime::adapter::to_v1(&m) {
            enabled.insert(id, true);
            manifests.push((v1, install_dir));
        }
    }
}

/// Core-ized 功能的 CLI stub：share 与 reading-insights 的子命令属于核心，
/// 不再有磁盘 manifest；注入扫描结果供 router/runner 统一匹配。
pub fn core_cli_stub_manifests() -> Vec<PluginManifest> {
    let share = serde_json::from_value(serde_json::json!({
        "id": "share", "name": "Share", "version": "core", "binary": "",
        "host_capabilities": ["renderer.html", "settings.read", "settings.write:share.records", "clipboard.write", "toast", "dialog"],
        "cli": [{
            "subcommand": "share", "aliases": ["--share"], "command": "publish",
            "summary": "Render and publish file as a shareable URL",
            "args": [{ "name": "file", "type": "path", "required": true, "help": "Markdown or image file to share" }],
            "flags": [
                { "long": "--update", "type": "boolean", "help": "Force update existing share (default if already shared)" },
                { "long": "--copy-link", "type": "boolean", "help": "Print previously-shared URL instead of re-publishing" },
                { "long": "--unshare", "type": "boolean", "help": "Remove share for this file" }
            ],
            "requires_tab_context": true
        }]
    })).expect("share cli stub");
    let insights = serde_json::from_value(serde_json::json!({
        "id": "reading-insights", "name": "Reading Insights", "version": "core", "binary": "",
        "host_capabilities": [],
        "cli": [{
            "subcommand": "report", "command": "report",
            "summary": "Generate a reading engagement report (owner + online audience) from the Vault",
            "args": [],
            "flags": [
                { "long": "--vault", "type": "string", "help": "Vault root (defaults to the configured Vault)" },
                { "long": "--date", "type": "string", "help": "today | yesterday (default) | 7d | 30d | month" },
                { "long": "--from", "type": "string", "help": "YYYY-MM-DD (requires --to; cannot be combined with --date)" },
                { "long": "--to", "type": "string", "help": "YYYY-MM-DD" },
                { "long": "--stdout", "type": "boolean", "help": "Print to stdout instead of writing <vault>/stat/*.md" }
            ]
        }]
    })).expect("insights cli stub");
    vec![share, insights]
}

/// True when this manifest is one of the injected core CLI stubs (see
/// [`core_cli_stub_manifests`]) rather than a real on-disk plugin. Stubs are
/// distinguishable by their sentinel shape: version "core" + empty binary.
/// builtin.rs uses this to keep core commands out of the PLUGIN COMMANDS help
/// section even if stubs are ever passed to the renderers.
pub fn is_core_cli_stub(m: &PluginManifest) -> bool {
    m.version == "core" && m.binary.as_deref() == Some("")
}

/// 把 core stub 追加进扫描结果。同 id 已在扫描结果里（例如某插件占用了同名 id）
/// 则不追加，保持该插件行为；追加时强制 enabled=true —— core 命令不可禁用。
pub(crate) fn append_core_cli_stubs(
    manifests: &mut Vec<(PluginManifest, PathBuf)>,
    enabled: &mut std::collections::HashMap<String, bool>,
) {
    for stub in core_cli_stub_manifests() {
        if manifests.iter().any(|(m, _)| m.id == stub.id) {
            continue;
        }
        enabled.insert(stub.id.clone(), true);
        manifests.push((stub, PathBuf::new()));
    }
}

fn parse_subcommand_args(
    remaining: &[String],
    entry: &crate::plugin_host::CliEntry,
) -> Result<
    (
        serde_json::Map<String, serde_json::Value>,
        serde_json::Map<String, serde_json::Value>,
    ),
    String,
> {
    let mut args = serde_json::Map::new();
    let mut flags = serde_json::Map::new();
    let mut positional_index = 0;
    let mut positional_only = false;
    let mut i = 0;
    while i < remaining.len() {
        let tok = &remaining[i];
        if !positional_only && tok == "--" {
            positional_only = true;
        } else if let Some(flag) = if positional_only {
            None
        } else {
            entry
                .flags
                .iter()
                .find(|f| f.long == *tok || f.short.as_deref() == Some(tok.as_str()))
        } {
            let key = flag.long.trim_start_matches('-').to_string();
            if flags.contains_key(&key) {
                return Err(format!("notemd: flag {} may only be specified once", flag.long));
            }
            match flag.ty.as_str() {
                "boolean" => {
                    flags.insert(key, serde_json::Value::Bool(true));
                }
                "string" => {
                    if i + 1 >= remaining.len() {
                        return Err(format!("notemd: flag {} requires a value", flag.long));
                    }
                    if remaining[i + 1].starts_with('-') {
                        return Err(format!(
                            "notemd: flag {} requires a value before '{}'",
                            flag.long, remaining[i + 1]
                        ));
                    }
                    flags.insert(key, serde_json::Value::String(remaining[i + 1].clone()));
                    i += 1;
                }
                _ => return Err(format!("notemd: internal: unknown flag type '{}'", flag.ty)),
            }
        } else if !positional_only && tok.starts_with('-') {
            return Err(format!("notemd: unknown flag '{tok}'"));
        } else {
            let spec = entry
                .args
                .get(positional_index)
                .ok_or_else(|| format!("notemd: unexpected argument '{tok}'"))?;
            let value = match spec.ty.as_str() {
                "path" | "string" => serde_json::Value::String(tok.clone()),
                "integer" => {
                    let parsed = tok.parse::<i64>().map_err(|_| {
                        format!("notemd: argument <{}> must be an integer", spec.name)
                    })?;
                    serde_json::Value::Number(parsed.into())
                }
                other => {
                    return Err(format!(
                        "notemd: internal: unknown argument type '{other}' for <{}>",
                        spec.name
                    ));
                }
            };
            args.insert(spec.name.clone(), value);
            positional_index += 1;
        }
        i += 1;
    }
    for required in entry.args.iter().filter(|a| a.required) {
        if !args.contains_key(&required.name) {
            return Err(format!(
                "notemd: missing required argument '<{}>'",
                required.name
            ));
        }
    }
    Ok((args, flags))
}

fn resolve_path_args(
    args: &mut serde_json::Map<String, serde_json::Value>,
    entry: &crate::plugin_host::CliEntry,
    allow_missing: bool,
) -> Result<(), String> {
    for spec in entry.args.iter().filter(|a| a.ty == "path") {
        let Some(raw) = args.get(&spec.name).and_then(|v| v.as_str()) else {
            continue;
        };
        let resolved = if allow_missing {
            absolute_path_without_requiring_target(Path::new(raw))
                .map_err(|e| format!("notemd: cannot resolve '{raw}': {e}"))?
        } else {
            Path::new(raw)
                .canonicalize()
                .map_err(|_| format!("notemd: cannot read '{raw}': No such file or directory"))?
        };
        args.insert(
            spec.name.clone(),
            serde_json::Value::String(resolved.to_string_lossy().into_owned()),
        );
    }
    Ok(())
}

/// Return a stable absolute spelling without requiring the final path to
/// exist. Canonicalising the parent when possible keeps share-record keys
/// consistent with the normal publish path (including symlinked parents).
fn absolute_path_without_requiring_target(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let Some(name) = absolute.file_name() else {
        return Ok(absolute);
    };
    match absolute.parent().and_then(|p| p.canonicalize().ok()) {
        Some(parent) => Ok(parent.join(name)),
        None => Ok(absolute),
    }
}

/// The hidden WebView must never keep a shell command alive forever. Normal
/// commands get the manifest timeout plus startup/IPC headroom; explicit
/// long-running `--wait` agent commands get the documented 300 seconds plus
/// startup/IPC headroom.
fn watchdog_timeout(
    manifest: &PluginManifest,
    flags: &serde_json::Map<String, serde_json::Value>,
) -> Duration {
    if flags.get("wait").and_then(|v| v.as_bool()) == Some(true) {
        Duration::from_secs(330)
    } else {
        Duration::from_secs(manifest.timeout_seconds.clamp(30, 300) + 30)
    }
}

/// Mutually-exclusive flag fan-out: --update, --copy-link, --unshare map to
/// the right plugin command. Default is the manifest entry's declared command.
fn decide_plugin_command(
    flags: &serde_json::Map<String, serde_json::Value>,
    default_cmd: &str,
) -> Result<String, String> {
    let truthy = |k: &str| flags.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let exclusive: Vec<&str> = ["update", "copy-link", "unshare"]
        .into_iter()
        .filter(|k| truthy(k))
        .collect();
    if exclusive.len() > 1 {
        return Err(format!(
            "notemd: flags --{} are mutually exclusive",
            exclusive.join(" --")
        ));
    }
    Ok(if truthy("unshare") {
        "unpublish".to_string()
    } else if truthy("copy-link") {
        "copy-link".to_string()
    } else {
        default_cmd.to_string()
    })
}

fn launch_tauri_headless(
    state: CliState,
    rx: oneshot::Receiver<crate::cli::state::CliResult>,
    watchdog_after: Duration,
    json: bool,
) -> i32 {
    let result_arc = std::sync::Arc::new(std::sync::Mutex::new(None::<i32>));
    let result_arc_clone = result_arc.clone();

    // Start the guard before Tauri is built: a WebView/runtime startup hang is
    // exactly as harmful to a shell caller as a command that never finishes.
    let finished = std::sync::Arc::new(AtomicBool::new(false));
    let watchdog_finished = finished.clone();
    std::thread::spawn(move || {
        std::thread::sleep(watchdog_after);
        if !watchdog_finished.load(Ordering::Acquire) {
            emit_cli_error(
                json,
                "timeout",
                &format!(
                    "CLI command timed out after {} seconds",
                    watchdog_after.as_secs()
                ),
            );
            std::process::exit(1);
        }
    });

    let init_script = "window.__M_CLI_MODE__ = true;";

    let app = match tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            crate::cli::state::cli_payload,
            crate::cli::state::cli_finish,
            // get_plugin_manifests serves the adapted v2 manifests from
            // plugin_runtime::STATE (populated by init_for_cli below);
            // CliRunner executes the matched command via plugin_v2_execute.
            crate::plugin_host::get_plugin_manifests,
            crate::plugin_runtime::commands::plugin_v2_execute_cli,
            crate::themes::commands::theme_load_compiled,
            // sotvault: needed by `notemd share` — refreshSotvault + prepareShareSrc
            // resolve the vault root, and an outside-vault file is homed in first
            // via sotvault_sync_to_vault. resolve_vault_root falls back to the
            // shared config, so no VaultSyncManager needs to be managed here.
            crate::sotvault::sotvault_vault_root,
            crate::sotvault::sotvault_vault_debug,
            crate::sotvault::sotvault_records,
            crate::sotvault::sotvault_sync_to_vault,
        ])
        .setup(move |app| {
            // Populate plugin_runtime::STATE so get_plugin_manifests /
            // plugin_v2_execute see the plugins the Rust-side scan
            // (append_v2_manifests) routed here.
            crate::plugin_runtime::init_for_cli(&app.handle());
            let _ = tauri::WebviewWindowBuilder::new(
                app,
                "cli",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .visible(false)
            .skip_taskbar(true)
            .initialization_script(init_script)
            .build()?;
            Ok(())
        })
        .manage(state)
        .build(crate::tauri_context())
    {
        Ok(app) => app,
        Err(error) => {
            finished.store(true, Ordering::Release);
            emit_cli_error(json, "startup_failed", &format!("CLI runtime failed to start: {error}"));
            return 1;
        }
    };

    tauri::async_runtime::spawn(async move {
        if let Ok(res) = rx.await {
            *result_arc_clone.lock().unwrap() = Some(res.exit_code);
        }
    });

    app.run(|_app, _event| {});
    finished.store(true, Ordering::Release);

    let code = result_arc.lock().unwrap().unwrap_or(1);
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::{CliArg, CliEntry, CliFlag};

    fn entry_with_file_and_flags() -> CliEntry {
        CliEntry {
            subcommand: "share".to_string(),
            aliases: vec![],
            command: "publish".to_string(),
            summary: "s".to_string(),
            args: vec![CliArg {
                name: "file".to_string(),
                ty: "path".to_string(),
                required: true,
                help: None,
            }],
            flags: vec![
                CliFlag {
                    long: "--update".to_string(),
                    short: None,
                    ty: "boolean".to_string(),
                    help: None,
                },
                CliFlag {
                    long: "--copy-link".to_string(),
                    short: None,
                    ty: "boolean".to_string(),
                    help: None,
                },
                CliFlag {
                    long: "--unshare".to_string(),
                    short: None,
                    ty: "boolean".to_string(),
                    help: None,
                },
            ],
            requires_tab_context: true,
        }
    }

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_just_file_succeeds() {
        let (args, flags) =
            parse_subcommand_args(&s(&["draft.md"]), &entry_with_file_and_flags()).unwrap();
        assert_eq!(args.get("file").and_then(|v| v.as_str()), Some("draft.md"));
        assert!(flags.is_empty());
    }
    #[test]
    fn parse_file_with_flag() {
        let (args, flags) =
            parse_subcommand_args(&s(&["draft.md", "--update"]), &entry_with_file_and_flags())
                .unwrap();
        assert_eq!(args.get("file").and_then(|v| v.as_str()), Some("draft.md"));
        assert_eq!(flags.get("update").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn parse_preserves_all_declared_names_and_types() {
        let entry = CliEntry {
            subcommand: "run".into(),
            aliases: vec![],
            command: "run".into(),
            summary: "s".into(),
            args: vec![
                CliArg {
                    name: "task".into(),
                    ty: "string".into(),
                    required: true,
                    help: None,
                },
                CliArg {
                    name: "count".into(),
                    ty: "integer".into(),
                    required: true,
                    help: None,
                },
                CliArg {
                    name: "source".into(),
                    ty: "path".into(),
                    required: false,
                    help: None,
                },
            ],
            flags: vec![],
            requires_tab_context: false,
        };
        let (args, _) =
            parse_subcommand_args(&s(&["selfcheck", "3", "notes/a.md"]), &entry).unwrap();
        assert_eq!(args.get("task").and_then(|v| v.as_str()), Some("selfcheck"));
        assert_eq!(args.get("count").and_then(|v| v.as_i64()), Some(3));
        assert_eq!(
            args.get("source").and_then(|v| v.as_str()),
            Some("notes/a.md")
        );
    }

    #[test]
    fn parse_double_dash_allows_dash_prefixed_string() {
        let mut entry = entry_with_file_and_flags();
        entry.args[0].ty = "string".into();
        entry.args[0].name = "task".into();
        let (args, _) = parse_subcommand_args(&s(&["--", "--selfcheck"]), &entry).unwrap();
        assert_eq!(
            args.get("task").and_then(|v| v.as_str()),
            Some("--selfcheck")
        );
    }

    #[test]
    fn parse_rejects_invalid_integer() {
        let mut entry = entry_with_file_and_flags();
        entry.args[0].ty = "integer".into();
        entry.args[0].name = "count".into();
        let err = parse_subcommand_args(&s(&["many"]), &entry).unwrap_err();
        assert!(err.contains("must be an integer"));
    }
    #[test]
    fn parse_missing_required_arg() {
        let r = parse_subcommand_args(&s(&[]), &entry_with_file_and_flags());
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("missing required"));
    }
    #[test]
    fn parse_unknown_flag() {
        let r = parse_subcommand_args(&s(&["draft.md", "--bogus"]), &entry_with_file_and_flags());
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("unknown flag"));
    }

    #[test]
    fn parse_rejects_duplicate_flags_and_a_flag_used_as_a_string_value() {
        let mut entry = entry_with_file_and_flags();
        entry.flags.push(CliFlag {
            long: "--output".into(),
            short: Some("-o".into()),
            ty: "string".into(),
            help: None,
        });
        let duplicate = parse_subcommand_args(
            &s(&["draft.md", "--output", "a", "-o", "b"]),
            &entry,
        )
        .unwrap_err();
        assert!(duplicate.contains("may only be specified once"));

        let missing = parse_subcommand_args(
            &s(&["draft.md", "--output", "--update"]),
            &entry,
        )
        .unwrap_err();
        assert!(missing.contains("requires a value"));
    }

    #[test]
    fn allow_missing_path_makes_deleted_share_target_absolute() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("deleted.md");
        let mut args = serde_json::Map::from_iter([(
            "file".into(),
            serde_json::Value::String(missing.to_string_lossy().into_owned()),
        )]);
        resolve_path_args(&mut args, &entry_with_file_and_flags(), true).unwrap();
        let expected = tmp.path().canonicalize().unwrap().join("deleted.md");
        assert_eq!(
            args["file"].as_str(),
            Some(expected.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn normal_path_resolution_still_rejects_missing_input() {
        let mut args = serde_json::Map::from_iter([(
            "file".into(),
            serde_json::Value::String("definitely-missing-notemd-file.md".into()),
        )]);
        let err = resolve_path_args(&mut args, &entry_with_file_and_flags(), false).unwrap_err();
        assert!(err.contains("cannot read"));
    }

    #[test]
    fn watchdog_is_bounded_and_wait_gets_long_window() {
        let manifest = core_cli_stub_manifests().remove(0);
        assert_eq!(
            watchdog_timeout(&manifest, &serde_json::Map::new()),
            Duration::from_secs(60)
        );
        let flags = serde_json::Map::from_iter([("wait".into(), serde_json::Value::Bool(true))]);
        assert_eq!(
            watchdog_timeout(&manifest, &flags),
            Duration::from_secs(330)
        );
    }

    #[test]
    fn json_cli_error_is_a_stable_envelope() {
        let value = cli_error_value("invalid_arguments", "bad");
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "invalid_arguments");
    }

    #[test]
    fn decide_default_command() {
        let r = decide_plugin_command(&serde_json::Map::new(), "publish").unwrap();
        assert_eq!(r, "publish");
    }
    #[test]
    fn decide_unshare_maps_to_unpublish() {
        let mut f = serde_json::Map::new();
        f.insert("unshare".to_string(), serde_json::Value::Bool(true));
        assert_eq!(decide_plugin_command(&f, "publish").unwrap(), "unpublish");
    }
    #[test]
    fn decide_copy_link_maps_to_copy_link() {
        let mut f = serde_json::Map::new();
        f.insert("copy-link".to_string(), serde_json::Value::Bool(true));
        assert_eq!(decide_plugin_command(&f, "publish").unwrap(), "copy-link");
    }
    #[test]
    fn decide_mutually_exclusive() {
        let mut f = serde_json::Map::new();
        f.insert("update".to_string(), serde_json::Value::Bool(true));
        f.insert("unshare".to_string(), serde_json::Value::Bool(true));
        let r = decide_plugin_command(&f, "publish");
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("mutually exclusive"));
    }

    /// Documents the v2_plugins_root assumption: the CLI has no AppHandle, so
    /// it derives the v2 plugins root from `dirs::data_dir()` + BUNDLE_ID.
    /// Tauri's app_data_dir() resolves to the same place on macOS
    /// (`~/Library/Application Support/net.notemd.app`) — if this ever drifts,
    /// GUI and CLI would scan different v2 install roots.
    #[cfg(target_os = "macos")]
    #[test]
    fn data_dir_matches_tauri_app_data_dir() {
        let root = dirs::data_dir().unwrap().join(crate::app_dirs::BUNDLE_ID);
        assert!(
            root.ends_with("Application Support/net.notemd.app"),
            "unexpected v2 root base: {}",
            root.display()
        );
    }

    /// Version equivalence pin: the Cargo crate version (embedded at compile
    /// time via env!("CARGO_PKG_VERSION")) must equal the "version" field in
    /// the root package.json. A drift would mean the CLI reports a different
    /// version than the npm/tauri build system thinks it is.
    #[test]
    fn cargo_version_matches_package_json_version() {
        let pkg_json_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../package.json");
        let bytes = std::fs::read(pkg_json_path)
            .expect("root package.json must be readable from src-tauri/");
        let v: serde_json::Value =
            serde_json::from_slice(&bytes).expect("root package.json must be valid JSON");
        let pkg_version = v
            .get("version")
            .and_then(|v| v.as_str())
            .expect("package.json must have a string 'version' field");
        assert_eq!(
            env!("CARGO_PKG_VERSION"),
            pkg_version,
            "Cargo version ({}) differs from package.json version ({})",
            env!("CARGO_PKG_VERSION"),
            pkg_version,
        );
    }
}
