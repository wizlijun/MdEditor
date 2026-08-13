//! Built-in subcommands: help, version, plugin
//! {list,enable,disable,info,install,update,remove}.
//!
//! These run entirely in Rust without spinning up a Tauri webview.

use crate::plugin_host::PluginManifest;
use super::args::Parsed;
use super::router::Builtin;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

const PLUGIN_API_VERSION: &str = "v1";

pub fn run(b: Builtin, parsed: &Parsed) -> ExitCode {
    let (manifests, enabled) = current_scan();
    let manifests_only: Vec<PluginManifest> =
        manifests.into_iter().map(|(m, _)| m).collect();
    match b {
        Builtin::Help { topic, all } => {
            println!("{}", render_help(topic.as_deref(), all, &manifests_only, &enabled));
            ExitCode::from(0)
        }
        Builtin::Version => {
            println!("{}", render_version(parsed.globals.json));
            ExitCode::from(0)
        }
        Builtin::PluginList => {
            println!("{}", render_plugin_list(parsed.globals.json, &manifests_only, &enabled));
            ExitCode::from(0)
        }
        Builtin::PluginEnable(id) => plugin_set_enabled(&id, true, parsed),
        Builtin::PluginDisable(id) => plugin_set_enabled(&id, false, parsed),
        Builtin::PluginInfo(id) => {
            let m = match manifests_only.iter().find(|m| m.id == id) {
                Some(m) => m,
                None => {
                    eprintln!("notemd: unknown plugin id '{id}'");
                    return ExitCode::from(2);
                }
            };
            println!("{}", render_plugin_info(m, &enabled));
            ExitCode::from(0)
        }
        Builtin::PluginInstall(id, version) => market::run_install(&id, version.as_deref(), parsed),
        Builtin::PluginUpdate(id) => market::run_update(id.as_deref(), parsed),
        Builtin::PluginRemove(id, keep_data) => market::run_remove(&id, keep_data, parsed),
        Builtin::Search(args) => super::search::run(args.with_global_json(parsed.globals.json)),
    }
}

/// `plugin enable`/`plugin disable`. The enabled flag lives in the runtime's
/// state.json, which also covers re-enabling a *disabled* plugin — one that
/// `current_scan` filters out of `manifests` entirely (discovery only returns
/// enabled ones). An id absent from state.json is simply not installed.
fn plugin_set_enabled(id: &str, enabled: bool, parsed: &Parsed) -> ExitCode {
    match market::set_v2_enabled(id, enabled) {
        Some(res) => report_toggle(id, enabled, res, parsed),
        None => {
            eprintln!("notemd: unknown plugin id '{id}'");
            ExitCode::from(2)
        }
    }
}

fn report_toggle(id: &str, enabled: bool, res: Result<(), String>, parsed: &Parsed) -> ExitCode {
    let (verb, past) = if enabled { ("enable", "enabled") } else { ("disable", "disabled") };
    match res {
        Ok(()) => {
            if !parsed.globals.quiet {
                eprintln!("✓ plugin '{id}' {past}");
            }
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("notemd: failed to {verb} plugin: {e}");
            ExitCode::from(1)
        }
    }
}

pub fn render_version(as_json: bool) -> String {
    let version = env!("CARGO_PKG_VERSION");
    if as_json {
        json!({
            "ok": true,
            "data": { "version": version, "plugin_api": PLUGIN_API_VERSION }
        }).to_string()
    } else {
        format!("notemd {version} (plugin API {PLUGIN_API_VERSION})")
    }
}

pub fn render_help(
    topic: Option<&str>,
    all: bool,
    manifests: &[PluginManifest],
    enabled: &HashMap<String, bool>,
) -> String {
    if let Some(t) = topic {
        return render_help_topic(t, manifests, enabled);
    }
    let version = env!("CARGO_PKG_VERSION");
    let mut out = String::new();
    out.push_str("notemd — note.md command-line interface\n");
    out.push_str(&format!("Version: {version} (plugin API {PLUGIN_API_VERSION})\n\n"));
    out.push_str("USAGE:\n");
    out.push_str("  notemd [global options] <command> [args...]\n");
    for m in manifests {
        let is_on = super::is_enabled(m, enabled);
        if !is_on { continue }
        for entry in &m.cli {
            if let Some(short) = entry.aliases.iter().find(|a| a.starts_with('-') && a.len() == 2) {
                out.push_str(&format!(
                    "  notemd {short} <file>                  (alias for: notemd {} <file>)\n",
                    entry.subcommand,
                ));
            }
        }
    }
    out.push_str("\nCORE COMMANDS:\n");
    out.push_str("  help          Show this help (aliases: -h, --help)\n");
    out.push_str("  version       Print version (aliases: -v, --version)\n");
    out.push_str("  plugin        Manage plugins (list, enable, disable, info, install, update, remove)\n");
    out.push_str("  share         Render and publish file as a shareable URL (alias: --share)\n");
    out.push_str("  search        Full-text search over the Vault (--vault, --json, --limit, --stats)\n");
    out.push_str("  reading-insights [report]   Generate a reading digest from the Vault (--vault, --date, --stdout)\n");

    let mut shown_header = false;
    for m in manifests {
        // Core stubs are hardcoded in CORE COMMANDS above; never re-list them
        // as plugins, even if a caller passes the injected stub manifests.
        if crate::cli::runner::is_core_cli_stub(m) { continue }
        let is_on = super::is_enabled(m, enabled);
        if !is_on { continue }
        for entry in &m.cli {
            if !shown_header {
                out.push_str("\nPLUGIN COMMANDS:\n");
                shown_header = true;
            }
            out.push_str(&format!(
                "  {:<13} {:<60} [{}]\n",
                entry.subcommand, entry.summary, m.name,
            ));
        }
    }

    if all {
        let mut shown = false;
        for m in manifests {
            if crate::cli::runner::is_core_cli_stub(m) { continue }
            let is_on = super::is_enabled(m, enabled);
            if is_on { continue }
            for entry in &m.cli {
                if !shown {
                    out.push_str("\nDISABLED COMMANDS:\n");
                    shown = true;
                }
                out.push_str(&format!(
                    "  {:<13} (provided by '{}' plugin — disabled)\n                Enable: notemd plugin enable {}\n",
                    entry.subcommand, m.name, m.id,
                ));
            }
        }
    }

    out.push_str("\nGLOBAL OPTIONS:\n");
    out.push_str("  --json              Emit machine-readable JSON instead of text\n");
    out.push_str("  -q, --quiet         Suppress non-essential status output\n");
    out.push_str("  -y, --yes           Assume 'yes' for confirmation prompts\n");
    out.push_str("  --no-clipboard      Don't copy the result to the clipboard (default: copy)\n");

    out.push_str("\nEXIT CODES:\n");
    out.push_str("  0    Success\n");
    out.push_str("  2    File or argument error\n");
    out.push_str("  4    Network or server error\n");
    out.push_str("  5    Plugin package failed verification (signature / hash)\n");
    out.push_str("  127  Unknown command\n");

    out.push_str("\nRun 'notemd help <command>' for details on a specific command.\n");
    out.push_str("Run 'notemd help --all' to see disabled / unavailable commands too.\n");
    out
}

fn render_help_topic(
    topic: &str,
    manifests: &[PluginManifest],
    enabled: &HashMap<String, bool>,
) -> String {
    if let Some(core) = render_core_topic(topic) {
        return core;
    }
    for m in manifests {
        for entry in &m.cli {
            if entry.subcommand == topic || entry.aliases.iter().any(|a| a == topic) {
                let on = super::is_enabled(m, enabled);
                let mut out = String::new();
                out.push_str(&format!(
                    "notemd {} — {}\n",
                    entry.subcommand, entry.summary,
                ));
                out.push_str(&format!("Provided by: {} plugin (v{})", m.name, m.version));
                if !on { out.push_str(" [DISABLED]"); }
                out.push('\n');
                out.push_str("\nUSAGE:\n");
                let args_sig = entry.args.iter()
                    .map(|a| if a.required { format!("<{}>", a.name) } else { format!("[{}]", a.name) })
                    .collect::<Vec<_>>().join(" ");
                out.push_str(&format!("  notemd {} {}\n", entry.subcommand, args_sig));
                for a in &entry.aliases {
                    out.push_str(&format!("  notemd {} {}                  (alias)\n", a, args_sig));
                }
                if !entry.args.is_empty() {
                    out.push_str("\nARGUMENTS:\n");
                    for a in &entry.args {
                        out.push_str(&format!("  <{:<8}> {}\n",
                            a.name, a.help.as_deref().unwrap_or("")));
                    }
                }
                if !entry.flags.is_empty() {
                    out.push_str("\nFLAGS:\n");
                    for f in &entry.flags {
                        let flag = match &f.short {
                            Some(s) => format!("{}, {}", s, f.long),
                            None => f.long.clone(),
                        };
                        out.push_str(&format!("  {:<25} {}\n",
                            flag, f.help.as_deref().unwrap_or("")));
                    }
                }
                out.push_str("\nEXIT CODES:\n");
                out.push_str("  0    Success\n");
                out.push_str("  2    File or argument error\n");
                out.push_str("  4    Network or server error\n");
                return out;
            }
        }
    }
    format!("notemd: unknown topic '{topic}'. Run 'notemd help' to see commands.\n")
}

/// Detailed help for the built-in core commands.
fn render_core_topic(topic: &str) -> Option<String> {
    let body = match topic {
        "help" | "-h" | "--help" => "\
notemd help — Show help for notemd and its commands

USAGE:
  notemd help [command]
  notemd help --all

DESCRIPTION:
  With no argument, lists every available command. Pass a command name to see
  its arguments, flags, and exit codes. Add --all to also list commands that
  are provided by disabled plugins.

ALIASES:
  -h, --help
",
        "version" | "-v" | "--version" => "\
notemd version — Print the notemd version and plugin API level

USAGE:
  notemd version [--json]

ALIASES:
  -v, --version
",
        "plugin" => "\
notemd plugin — Manage plugins

USAGE:
  notemd plugin list                      List installed plugins and their state
  notemd plugin enable  <plugin-id>       Enable a plugin
  notemd plugin disable <plugin-id>       Disable a plugin
  notemd plugin info    <plugin-id>       Show details for a single plugin
  notemd plugin install <id>[@version]    Download, verify, and install a plugin
  notemd plugin update  [<plugin-id>]     Update one plugin, or all if omitted
  notemd plugin remove  <plugin-id>       Uninstall a plugin (alias: uninstall)

FLAGS:
  --keep-data    (remove) Keep the plugin's data dir on disk

NOTES:
  Use 'notemd plugin list' to discover plugin ids. Enable/disable persist to
  the app's settings and affect both the CLI and the desktop app.
  Without @version, install picks the NEWEST version this notemd can run (same
  choice the in-app plugin market makes); update never moves you to a version
  that needs a newer notemd. Pass @version to override that deliberately.
  install/update download from the plugin registry and verify every package's
  minisign signature + sha256 before it touches disk; a running app picks up the
  change on its next launch.
",
        "share" | "--share" => "\
notemd share — Render and publish file as a shareable URL

USAGE:
  notemd share <file>
  notemd --share <file>                  (alias)

ARGUMENTS:
  <file>           Markdown or image file to share

FLAGS:
  --update         Force update existing share (default if already shared)
  --copy-link      Print previously-shared URL instead of re-publishing
  --unshare        Remove share for this file

Shares are published to the configured share server and the URL is copied to
the clipboard (disable with --no-clipboard). Files outside the Vault are
homed into the Vault first.
",
        "search" => "\
notemd search — Full-text search over the Vault

USAGE:
  notemd search <query...> [--vault <path>] [--json] [--limit <n>] [--context <n>]
  notemd search --stats --vault <path>

DESCRIPTION:
  Grep-shaped on purpose: default output is `path:line:text`, one hit per
  line. `rg`/`grep` keep working and are never wrong to use — this is an
  accelerator, not a gatekeeper. Filter flags are sugar for the same query
  grammar the Vault search panel understands (`tag:x`, `type:x`, `path:x`,
  `ext:x`, `after:YYYY-MM-DD`, `before:YYYY-MM-DD`, `page:[[X]]`,
  `origin:human|derived|source|unlabeled`) — e.g. `--tag x` is exactly `tag:x`
  appended to the query; `page:[[X]]` (a wikilink target) and `origin:` have no
  dedicated flag, type them directly into the query. `origin:unlabeled` finds
  files with no frontmatter and no source-glob match — ranked lowest by
  default (×0.3), so they can be missing from plain results entirely; this is
  how to find them anyway. Quote a phrase (`\"exact phrase\"`) for an
  exact-match instead of a bag of terms.

FLAGS:
  --vault <path>    Vault root (default: the configured Vault)
  --json            Emit {query, route, took_ms, total, hits: [...]}; each hit
                     adds score, breadcrumb, source_ref (path#Lline), origin
                     (\"human\"|\"derived\"|\"source\"|\"unlabeled\") and provenance
                     ({agent_by, human_verified}) beyond the plain
                     path/line/text. A hit with provenance.agent_by set was
                     written by a model — follow its sources to the primary
                     document before relying on it. attention_minutes is the
                     user's own reading/editing attention on that document in
                     minutes, decayed to today with a 30-day half-life (0 = no
                     data); ranking already factors it in and the field is
                     exposed so you can explain the order. That data is
                     ingested by the desktop app — this CLI reads it but never
                     ingests — so on a machine where the GUI has never opened
                     this vault it reads 0 for every hit and ranking is
                     unaffected.
  --limit <n>       Max hits (default: 20)
  --context <n>     Print N lines of context around each hit
  --tag <t>         Filter: tag:<t>
  --type <t>        Filter: type:<t>
  --path <p>        Filter: path:<p> (substring match)
  --ext <e>         Filter: ext:<e>
  --after <date>    Filter: doc_date >= date (YYYY-MM-DD)
  --before <date>   Filter: doc_date <= date (YYYY-MM-DD)
  --stats           Report index size/freshness instead of searching
  --rebuild         Force a full rebuild before searching
  --no-sweep        Skip the bounded freshness sweep (answer from what's indexed)

Retrieval never fails because the index is unhappy: an unusable index, or a
freshness sweep that runs past its 2s budget, degrades to a direct file scan
(or an answer from the existing index) with a one-line warning on stderr
instead of an error.

EXIT CODES:
  0    Output was printed — one or more hits (or --stats/--rebuild ran)
  1    No hits — not an error, nothing to branch on but 'try something else'
  2    No Vault configured/found, or a missing query
",
        "reading-insights" => "\
notemd reading-insights — Reading Insights (engagement) report

USAGE:
  notemd reading-insights [report] --vault <path> [--date <preset>] [--stdout]
  notemd reading-insights [report] --vault <path> --from YYYY-MM-DD --to YYYY-MM-DD

  The `report` subcommand is the default and may be omitted.

FLAGS:
  --vault <path>   Vault root. Reads <vault>/.notemd/analytics/
  --date <preset>  today | yesterday (default) | 7d | 30d | month
  --from --to      Explicit YYYY-MM-DD range (overrides --date)
  --stdout         Print to stdout instead of writing <vault>/stat/*.md

Owner engagement only (read/edit time, edit bursts, marks). Audience (online
reading) stats are shown in the in-app Reading Insights window.
",
        _ => return None,
    };

    let mut out = body.to_string();
    // Most core topics share this generic 0/1/2 footer, but a topic whose own
    // exit codes mean something more specific (`search`'s 1 is "no hits", not
    // a runtime error) writes its own `EXIT CODES:` section instead — appending
    // the generic one on top would tell an agent two contradictory things
    // about the exit code it will hit most often. `contains` rather than a
    // per-topic flag so this stays correct automatically if another topic
    // later grows its own accurate block.
    if !body.contains("EXIT CODES:") {
        out.push_str("\nEXIT CODES:\n");
        out.push_str("  0    Success\n");
        out.push_str("  1    Runtime error\n");
        out.push_str("  2    File or argument error\n");
    }
    Some(out)
}

pub fn render_plugin_list(
    as_json: bool,
    manifests: &[PluginManifest],
    enabled: &HashMap<String, bool>,
) -> String {
    if as_json {
        let arr: Vec<_> = manifests.iter().map(|m| {
            let is_on = super::is_enabled(m, enabled);
            json!({
                "id": m.id,
                "name": m.name,
                "version": m.version,
                "status": if is_on { "enabled" } else { "disabled" },
                "cli": m.cli.iter().map(|c| json!({
                    "subcommand": c.subcommand,
                    "aliases": c.aliases,
                    "summary": c.summary,
                })).collect::<Vec<_>>(),
            })
        }).collect();
        return json!({ "ok": true, "data": arr }).to_string();
    }
    let mut out = String::new();
    out.push_str(&format!("{:<10} {:<12} {:<8} {:<10} {}\n",
        "ID", "NAME", "VERSION", "STATUS", "CLI"));
    for m in manifests {
        let is_on = super::is_enabled(m, enabled);
        let cli = m.cli.iter().map(|c| {
            let aliases = if c.aliases.is_empty() {
                String::new()
            } else {
                format!(" ({})", c.aliases.join(", "))
            };
            format!("{}{aliases}", c.subcommand)
        }).collect::<Vec<_>>().join(", ");
        out.push_str(&format!("{:<10} {:<12} {:<8} {:<10} {}\n",
            m.id, m.name, m.version,
            if is_on { "enabled" } else { "disabled" },
            cli,
        ));
    }
    out
}

pub fn render_plugin_info(
    m: &PluginManifest,
    enabled: &HashMap<String, bool>,
) -> String {
    let is_on = super::is_enabled(m, enabled);
    let mut out = String::new();
    out.push_str(&format!("{} ({})  v{}\n", m.name, m.id, m.version));
    out.push_str(&format!("Status: {}\n", if is_on { "enabled" } else { "disabled" }));
    if let Some(d) = &m.description {
        out.push_str(&format!("Description: {d}\n"));
    }
    if !m.cli.is_empty() {
        out.push_str("\nCLI commands:\n");
        for c in &m.cli {
            out.push_str(&format!("  - {}: {}\n", c.subcommand, c.summary));
            for a in &c.aliases {
                out.push_str(&format!("    alias: {a}\n"));
            }
        }
    }
    if !m.menus.is_empty() {
        out.push_str("\nMenu items:\n");
        for me in &m.menus {
            out.push_str(&format!("  - [{}] {} ({})\n", me.location, me.label, me.command));
        }
    }
    out
}

/// Collects the installed plugins (adapted to the `PluginManifest` view-model
/// shape) so they appear in `plugin list`, `plugin info`, and `help` — the same
/// scan router.rs/runner.rs use for routing.
///
/// Deliberately does NOT inject the core CLI stubs (`runner::
/// core_cli_stub_manifests()`), unlike runner.rs's current_scan: the stubs
/// exist only so routing/arg-parsing can match core subcommands. Injecting
/// them here would double-list `share` in `notemd help` (core row + PLUGIN
/// COMMANDS row) and pollute `notemd plugin list` with pseudo-plugins.
fn current_scan() -> (Vec<(PluginManifest, PathBuf)>, HashMap<String, bool>) {
    let mut manifests = Vec::new();
    let mut enabled = HashMap::new();
    super::runner::append_v2_manifests(&mut manifests, &mut enabled);
    (manifests, enabled)
}

/// v2 marketplace subcommands driven from the CLI (子项目③ Task 3):
/// `plugin install/update/remove`. These reuse the same pure installer +
/// registry-client layers the GUI market commands use (`plugin_runtime::
/// {installer,market,state,discovery}`), so a package the CLI installs is
/// byte-identically verified to one installed from the window.
///
/// The CLI has no `AppHandle` and no ambient tokio runtime (the plugin builtins
/// run synchronously from `main`), so each command builds a small current-thread
/// [`tokio::runtime::Runtime`] and `block_on`s the async network + verify work.
/// The install tree root is derived from `dirs::data_dir()` + `BUNDLE_ID`,
/// matching `runner::v2_plugins_root` so GUI and CLI share one install tree.
mod market {
    use super::*;
    use crate::plugin_runtime::market as mkt;
    use crate::plugin_runtime::{discovery, installer, state};

    /// Exit codes (mirroring the documented CLI scheme, plus a signature-specific
    /// one so scripts can distinguish an untrusted package from other failures):
    /// 4 = network/registry/other runtime error, 5 = signature/hash verification
    /// failure (package rejected as untrusted/corrupt), 2 = argument error.
    const EXIT_VERIFY: u8 = 5;
    const EXIT_RUNTIME: u8 = 4;

    /// CLI equivalent of the Tauri app-data plugins root — same derivation as
    /// `runner::v2_plugins_root`, so both entry points scan/write one tree.
    fn plugins_root() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join(crate::app_dirs::BUNDLE_ID).join("plugins"))
    }

    /// App-data dir (parent of `plugins/` and `plugin_data/`), used as the
    /// `data_root` uninstall passes to `installer::uninstall`.
    fn app_data_root() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join(crate::app_dirs::BUNDLE_ID))
    }

    /// `plugin enable`/`disable`: flip the plugin's `enabled` flag in state.json
    /// (the single source of truth). Returns `None` when `id` is not an
    /// installed plugin, `Some(result)` once handled. A running app reconciles
    /// on its next launch (same as install/update).
    pub(super) fn set_v2_enabled(id: &str, enabled: bool) -> Option<Result<(), String>> {
        set_v2_enabled_at(&plugins_root()?, id, enabled)
    }

    /// Pure core of [`set_v2_enabled`] — testable against an explicit root.
    fn set_v2_enabled_at(
        root: &std::path::Path,
        id: &str,
        enabled: bool,
    ) -> Option<Result<(), String>> {
        let mut install = state::load(root);
        let entry = install.installed.get_mut(id)?;
        entry.enabled = enabled;
        Some(state::save(root, &install))
    }

    fn registry_base() -> String {
        mkt::registry_base_url_at(&super::super::resolve_config_dir())
    }

    /// A short-lived current-thread runtime for a single command's async work.
    fn runtime() -> Result<tokio::runtime::Runtime, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("failed to start async runtime: {e}"))
    }

    // ── Pure, unit-testable helpers ──────────────────────────────────────────

    /// The running host version, used for every `min_host` decision here.
    fn host_version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Strictly-newer test used to ORDER registry entries, to evaluate
    /// `min_host` bounds, AND to gate `plugin update`'s final "is this
    /// actually newer than what is installed" decision. Deliberately a port
    /// of `isNewerVersion` in `src/lib/market/types.ts`: dotted components
    /// compared left-to-right as numbers (so `1.10.0` > `1.9.0`, which string
    /// comparison gets backwards), missing components read as 0, a
    /// non-numeric component reads as 0.
    ///
    /// This is the ONLY comparator `pick_update_entry` uses — both to order
    /// candidates and to gate the decision — so it is byte-for-byte the same
    /// function as `pickUpdateTo` (`select.ts`) for every version string,
    /// including ones a strict `major.minor.patch` parser would reject. A
    /// second, stricter comparator here would let the CLI silently disagree
    /// with the market window on exactly the registry entries where it
    /// matters most: one comparator says "up-to-date", the other offers an
    /// update.
    fn version_gt(candidate: &str, current: &str) -> bool {
        let a = version_parts(candidate);
        let b = version_parts(current);
        for i in 0..a.len().max(b.len()) {
            let x = a.get(i).copied().unwrap_or(0);
            let y = b.get(i).copied().unwrap_or(0);
            if x != y {
                return x > y;
            }
        }
        false
    }

    /// `"1.10.0"` → `[1, 10, 0]`. Mirrors the TS `parseInt(p, 10) || 0`:
    /// leading digits win, anything else reads as 0.
    fn version_parts(v: &str) -> Vec<u64> {
        v.split('.')
            .map(|p| {
                let digits: String = p.trim_start().chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse().unwrap_or(0)
            })
            .collect()
    }

    /// True when `host` satisfies a registry `min_host` range like `">=6.803.0"`
    /// — a comma-separated list of `>=` `>` `<=` `<` `=` comparators over dotted
    /// numeric versions, the subset the registry actually ships.
    ///
    /// Port of `minHostSatisfied` in `src/lib/market/select.ts`, *including its
    /// fail-open*: a token this parser doesn't understand reads as satisfied.
    /// Selection must never hide a version the installer might accept — the
    /// installer re-checks the package's own `engines.notemd` authoritatively
    /// (full semver `VersionReq`) after signature + hash verification.
    fn min_host_satisfied(range: &str, host: &str) -> bool {
        for token in range.split(',') {
            let part = token.trim();
            if part.is_empty() || part == "*" {
                continue;
            }
            let Some((op, bound)) = split_comparator(part) else {
                return true; // unrecognized syntax — fail open
            };
            let gt = version_gt(host, bound);
            let lt = version_gt(bound, host);
            let ok = match op {
                ">=" => !lt,
                ">" => gt,
                "<=" => !gt,
                "<" => lt,
                _ => !gt && !lt, // '='
            };
            if !ok {
                return false;
            }
        }
        true
    }

    /// The Rust equivalent of the TS regex `^(>=|<=|>|<|=)\s*(\d[\d.]*)$`:
    /// splits a trimmed comparator token into operator + bound, or `None` when
    /// it doesn't match (which the caller turns into a fail-open `true`).
    fn split_comparator(part: &str) -> Option<(&str, &str)> {
        let (op, rest) = if let Some(r) = part.strip_prefix(">=") {
            (">=", r)
        } else if let Some(r) = part.strip_prefix("<=") {
            ("<=", r)
        } else if let Some(r) = part.strip_prefix('>') {
            (">", r)
        } else if let Some(r) = part.strip_prefix('<') {
            ("<", r)
        } else if let Some(r) = part.strip_prefix('=') {
            ("=", r)
        } else {
            return None;
        };
        let bound = rest.trim_start();
        if !bound.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        if !bound.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return None;
        }
        Some((op, bound))
    }

    /// Newest of `candidates` by [`version_gt`]; with `compatible_only`, skips
    /// entries whose `min_host` this host does not satisfy. Port of `newest` in
    /// `src/lib/market/select.ts`.
    fn newest<'a>(
        candidates: &[&'a mkt::RegistryEntry],
        host: &str,
        compatible_only: bool,
    ) -> Option<&'a mkt::RegistryEntry> {
        let mut best: Option<&'a mkt::RegistryEntry> = None;
        for e in candidates {
            if compatible_only && !min_host_satisfied(&e.min_host, host) {
                continue;
            }
            if best.is_none_or(|b| version_gt(&e.version, &b.version)) {
                best = Some(e);
            }
        }
        best
    }

    /// Resolve which version to install for `id`.
    ///
    /// * An explicit `@version` is honored verbatim, *including* one this host
    ///   cannot run: that is a deliberate user override, and the installer still
    ///   refuses it authoritatively after verification.
    /// * Without a version: the newest version this host satisfies — the same
    ///   choice `pickAvailable` (`src/lib/market/select.ts`) makes for the
    ///   market window, so the CLI and the window never disagree. The registry
    ///   index carries ONE ENTRY PER PUBLISHED VERSION, sorted by id then
    ///   version ascending, so taking the first match (the old bug) installed
    ///   the OLDEST version on record.
    /// * When the host satisfies none: the same entry `pickAvailable` would show
    ///   (the newest overall), but reported as an error up front instead of
    ///   downloading a package the installer is certain to reject — the message
    ///   is the honest "requires notemd X" one, so it reads as a stale app
    ///   rather than a broken registry.
    ///
    /// Arch availability is deliberately NOT part of this choice (`select.ts`
    /// ignores it too): a newest version that ships no package for this arch
    /// errors out in [`select_download`] instead of silently installing an older
    /// one behind the user's back. `install <id>@<older>` remains the escape.
    ///
    /// Kept pure (takes a slice + the host version) so tests don't hit the
    /// network.
    fn resolve_entry(
        plugins: &[mkt::RegistryEntry],
        id: &str,
        requested: Option<&str>,
        host: &str,
    ) -> Result<mkt::RegistryEntry, String> {
        if let Some(v) = requested {
            return plugins
                .iter()
                .find(|e| e.id == id && e.version == v)
                .cloned()
                .ok_or_else(|| format!("plugin '{id}' version '{v}' not found in registry"));
        }
        let group: Vec<&mkt::RegistryEntry> = plugins.iter().filter(|e| e.id == id).collect();
        if group.is_empty() {
            return Err(format!("plugin '{id}' not found in registry"));
        }
        if let Some(e) = newest(&group, host, true) {
            return Ok(e.clone());
        }
        let overall = newest(&group, host, false).expect("group is non-empty");
        Err(format!(
            "plugin '{id}' {} requires notemd {}, host is {host} — update note.md, \
             or pick an older version explicitly: notemd plugin install {id}@<version>",
            overall.version, overall.min_host,
        ))
    }

    /// Which entry `plugin update` should move `id` to.
    /// Port of `pickUpdateTo` (`src/lib/market/select.ts`):
    /// the newest HOST-COMPATIBLE version, offered only when it is strictly
    /// newer than the installed one — a newer-but-incompatible version is never
    /// offered, since that update could only fail.
    ///
    /// Returns `Err(note)` for the "nothing to do" cases so the caller can print
    /// why: not published, already current, or held back by `min_host`.
    fn pick_update_entry<'a>(
        plugins: &'a [mkt::RegistryEntry],
        id: &str,
        installed: &str,
        host: &str,
    ) -> Result<&'a mkt::RegistryEntry, String> {
        let group: Vec<&'a mkt::RegistryEntry> = plugins.iter().filter(|e| e.id == id).collect();
        if group.is_empty() {
            return Err("not in registry".into());
        }
        if let Some(best) = newest(&group, host, true) {
            if version_gt(&best.version, installed) {
                return Ok(best);
            }
        }
        // Nothing compatible is newer. Say so explicitly when a newer version
        // does exist but this host is too old, so `update` never looks like it
        // silently ignored a release the market window advertises.
        let overall = newest(&group, host, false).expect("group is non-empty");
        if version_gt(&overall.version, installed) {
            return Err(format!(
                "up-to-date ({} needs notemd {})",
                overall.version, overall.min_host
            ));
        }
        Err("up-to-date".into())
    }

    /// Pick this host arch's download URL + expected sha256 from an index entry.
    /// Mirrors `commands::resolve_download`. UI-only plugins publish under the
    /// `universal` key, so we prefer the host triple then fall back to it;
    /// errors only when neither is present.
    fn select_download(entry: &mkt::RegistryEntry) -> Result<(String, String), String> {
        let triple = discovery::current_arch_triple()
            .ok_or_else(|| format!("unsupported host arch '{}'", std::env::consts::ARCH))?;
        let url = entry
            .download
            .get(triple)
            .or_else(|| entry.download.get("universal"))
            .ok_or_else(|| format!("plugin '{}' has no download for arch '{triple}'", entry.id))?;
        let sha = entry
            .sha256
            .get(triple)
            .or_else(|| entry.sha256.get("universal"))
            .ok_or_else(|| format!("plugin '{}' has no sha256 for arch '{triple}'", entry.id))?;
        Ok((url.clone(), sha.clone()))
    }

    /// Signature URL convention (shared with `commands.rs` and honored by the
    /// registry worker in Task 4/5): the detached `.minisig` is served as the
    /// package URL with `.minisig` appended. There is no separate `sig` field in
    /// the index — deriving it here keeps the index lean and the convention in
    /// exactly one place per consumer.
    fn sig_url_for(pkg_url: &str) -> String {
        format!("{pkg_url}.minisig")
    }

    // ── Command entry points ─────────────────────────────────────────────────

    pub fn run_install(id: &str, version: Option<&str>, parsed: &Parsed) -> ExitCode {
        let Some(root) = plugins_root() else {
            return fail(parsed, EXIT_RUNTIME, "cannot resolve app data dir");
        };
        let base = registry_base();

        let rt = match runtime() {
            Ok(rt) => rt,
            Err(e) => return fail(parsed, EXIT_RUNTIME, &e),
        };

        let result: Result<(String, String), (u8, String)> = rt.block_on(async {
            let index = mkt::fetch_index(&base)
                .await
                .map_err(|e| (EXIT_RUNTIME, e))?;
            let entry = resolve_entry(&index.plugins, id, version, host_version())
                .map_err(|e| (EXIT_RUNTIME, e))?;
            let (url, sha) = select_download(&entry).map_err(|e| (EXIT_RUNTIME, e))?;
            let sig_url = sig_url_for(&url);

            let pkg = mkt::download(&url).await.map_err(|e| (EXIT_RUNTIME, e))?;
            let sig = String::from_utf8(mkt::download(&sig_url).await.map_err(|e| (EXIT_RUNTIME, e))?)
                .map_err(|e| (EXIT_RUNTIME, format!("signature is not valid utf-8: {e}")))?;

            let host_version = env!("CARGO_PKG_VERSION");
            let tmp = tempfile::tempdir()
                .map_err(|e| (EXIT_RUNTIME, format!("tempdir: {e}")))?;
            // Verification failures (bad sig / hash mismatch) are the untrusted-
            // package case → EXIT_VERIFY; everything else (unpack, manifest,
            // id mismatch, io) is a plain runtime failure → EXIT_RUNTIME.
            installer::verify_and_stage(
                &pkg,
                &sig,
                &sha,
                mkt::PLUGIN_REGISTRY_PUBKEY,
                id,
                host_version,
                tmp.path(),
            )
            .map_err(|e| (exit_for_install_err(&e), e.to_string()))?;

            installer::commit_install(&root, id, &entry.version, tmp.path())
                .map_err(|e| (EXIT_RUNTIME, e.to_string()))?;

            // Record installed + enabled in state.json.
            let mut install = state::load(&root);
            install.installed.insert(
                id.to_string(),
                state::InstalledPlugin { version: entry.version.clone(), enabled: true },
            );
            state::save(&root, &install).map_err(|e| (EXIT_RUNTIME, e))?;

            // Fire-and-forget telemetry (never affects the exit code).
            mkt::report_install(&base, id, &entry.version).await;

            Ok((id.to_string(), entry.version.clone()))
        });

        match result {
            Ok((id, version)) => {
                emit_install_ok(parsed, &id, &version);
                ExitCode::from(0)
            }
            Err((code, msg)) => fail(parsed, code, &msg),
        }
    }

    pub fn run_update(id: Option<&str>, parsed: &Parsed) -> ExitCode {
        let Some(root) = plugins_root() else {
            return fail(parsed, EXIT_RUNTIME, "cannot resolve app data dir");
        };
        let base = registry_base();

        let rt = match runtime() {
            Ok(rt) => rt,
            Err(e) => return fail(parsed, EXIT_RUNTIME, &e),
        };

        // Which installed plugins are candidates for update.
        let installed = state::load(&root).installed;
        let targets: Vec<(String, String)> = match id {
            Some(one) => match installed.get(one) {
                Some(p) => vec![(one.to_string(), p.version.clone())],
                None => return fail(parsed, EXIT_RUNTIME, &format!("plugin '{one}' is not installed")),
            },
            None => installed
                .iter()
                .map(|(k, v)| (k.clone(), v.version.clone()))
                .collect(),
        };

        if targets.is_empty() {
            emit_update_summary(parsed, &[]);
            return ExitCode::from(0);
        }

        let outcomes: Result<Vec<UpdateOutcome>, (u8, String)> = rt.block_on(async {
            let index = mkt::fetch_index(&base).await.map_err(|e| (EXIT_RUNTIME, e))?;
            let mut out = Vec::with_capacity(targets.len());
            for (id, installed_ver) in &targets {
                let entry = match pick_update_entry(&index.plugins, id, installed_ver, host_version()) {
                    Ok(e) => e.clone(),
                    Err(note) => {
                        out.push(UpdateOutcome { id: id.clone(), from: installed_ver.clone(), to: None, note });
                        continue;
                    }
                };
                // Newer version available → install it (same verify pipeline).
                let (url, sha) = select_download(&entry).map_err(|e| (EXIT_RUNTIME, e))?;
                let sig_url = sig_url_for(&url);
                let pkg = mkt::download(&url).await.map_err(|e| (EXIT_RUNTIME, e))?;
                let sig = String::from_utf8(mkt::download(&sig_url).await.map_err(|e| (EXIT_RUNTIME, e))?)
                    .map_err(|e| (EXIT_RUNTIME, format!("signature is not valid utf-8: {e}")))?;
                let host_version = env!("CARGO_PKG_VERSION");
                let tmp = tempfile::tempdir().map_err(|e| (EXIT_RUNTIME, format!("tempdir: {e}")))?;
                installer::verify_and_stage(&pkg, &sig, &sha, mkt::PLUGIN_REGISTRY_PUBKEY, id, host_version, tmp.path())
                    .map_err(|e| (exit_for_install_err(&e), e.to_string()))?;
                installer::commit_install(&root, id, &entry.version, tmp.path())
                    .map_err(|e| (EXIT_RUNTIME, e.to_string()))?;
                let mut install = state::load(&root);
                let enabled = install.installed.get(id).map(|p| p.enabled).unwrap_or(true);
                install.installed.insert(id.clone(), state::InstalledPlugin { version: entry.version.clone(), enabled });
                state::save(&root, &install).map_err(|e| (EXIT_RUNTIME, e))?;
                mkt::report_install(&base, id, &entry.version).await;
                out.push(UpdateOutcome { id: id.clone(), from: installed_ver.clone(), to: Some(entry.version.clone()), note: "updated".into() });
            }
            Ok(out)
        });

        match outcomes {
            Ok(list) => {
                emit_update_summary(parsed, &list);
                ExitCode::from(0)
            }
            Err((code, msg)) => fail(parsed, code, &msg),
        }
    }

    pub fn run_remove(id: &str, keep_data: bool, parsed: &Parsed) -> ExitCode {
        let (Some(root), Some(data_root)) = (plugins_root(), app_data_root()) else {
            return fail(parsed, EXIT_RUNTIME, "cannot resolve app data dir");
        };

        // Refuse to "remove" something not installed, so the user gets a clear
        // message instead of a silent success.
        let mut install = state::load(&root);
        if !install.installed.contains_key(id) {
            return fail(parsed, EXIT_RUNTIME, &format!("plugin '{id}' is not installed"));
        }

        if let Err(e) = installer::uninstall(&root, id, keep_data, &data_root) {
            return fail(parsed, EXIT_RUNTIME, &e.to_string());
        }
        install.installed.remove(id);
        if let Err(e) = state::save(&root, &install) {
            return fail(parsed, EXIT_RUNTIME, &e);
        }

        emit_remove_ok(parsed, id, keep_data);
        ExitCode::from(0)
    }

    // ── Output + error plumbing ──────────────────────────────────────────────

    struct UpdateOutcome {
        id: String,
        from: String,
        to: Option<String>,
        note: String,
    }

    /// Map an installer error to the CLI exit code: verification failures
    /// (untrusted/corrupt package) are EXIT_VERIFY; everything else EXIT_RUNTIME.
    fn exit_for_install_err(e: &installer::InstallError) -> u8 {
        match e {
            installer::InstallError::Hash | installer::InstallError::Signature => EXIT_VERIFY,
            _ => EXIT_RUNTIME,
        }
    }

    /// A CLI process cannot reconcile a *running* GUI instance's live runtime;
    /// the note next to a successful install says so.
    const RESTART_NOTE: &str =
        "note.md picks this up on its next launch (a running instance needs a restart).";

    fn emit_install_ok(parsed: &Parsed, id: &str, version: &str) {
        if parsed.globals.json {
            println!("{}", json!({ "ok": true, "data": { "id": id, "version": version } }));
        } else {
            if !parsed.globals.quiet {
                eprintln!("✓ installed '{id}' {version}");
                eprintln!("{RESTART_NOTE}");
            }
        }
    }

    fn emit_remove_ok(parsed: &Parsed, id: &str, keep_data: bool) {
        if parsed.globals.json {
            println!("{}", json!({ "ok": true, "data": { "id": id, "removed": true, "kept_data": keep_data } }));
        } else if !parsed.globals.quiet {
            eprintln!("✓ removed '{id}'{}", if keep_data { " (kept plugin data)" } else { "" });
            eprintln!("{RESTART_NOTE}");
        }
    }

    fn emit_update_summary(parsed: &Parsed, outcomes: &[UpdateOutcome]) {
        if parsed.globals.json {
            let arr: Vec<_> = outcomes
                .iter()
                .map(|o| json!({ "id": o.id, "from": o.from, "to": o.to, "status": o.note }))
                .collect();
            println!("{}", json!({ "ok": true, "data": arr }));
            return;
        }
        if parsed.globals.quiet {
            return;
        }
        if outcomes.is_empty() {
            eprintln!("No plugins installed — nothing to update.");
            return;
        }
        let mut any_updated = false;
        for o in outcomes {
            match &o.to {
                Some(to) => {
                    any_updated = true;
                    eprintln!("✓ {} {} → {}", o.id, o.from, to);
                }
                None => eprintln!("• {} {} ({})", o.id, o.from, o.note),
            }
        }
        if any_updated {
            eprintln!("{RESTART_NOTE}");
        }
    }

    /// Print an error (JSON or text) and return the given exit code.
    fn fail(parsed: &Parsed, code: u8, msg: &str) -> ExitCode {
        if parsed.globals.json {
            println!("{}", json!({ "ok": false, "error": msg }));
        } else {
            eprintln!("notemd: {msg}");
        }
        ExitCode::from(code)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::plugin_runtime::market::RegistryEntry;
        use std::collections::BTreeMap;

        fn entry(id: &str, version: &str) -> RegistryEntry {
            entry_min_host(id, version, ">=0.0.0")
        }

        fn entry_min_host(id: &str, version: &str, min_host: &str) -> RegistryEntry {
            let mut sha = BTreeMap::new();
            sha.insert("aarch64-apple-darwin".to_string(), "aa".to_string());
            sha.insert("x86_64-apple-darwin".to_string(), "bb".to_string());
            let mut dl = BTreeMap::new();
            dl.insert(
                "aarch64-apple-darwin".to_string(),
                format!("https://plugins.notemd.net/api/download/{id}/{version}/aarch64-apple-darwin"),
            );
            dl.insert(
                "x86_64-apple-darwin".to_string(),
                format!("https://plugins.notemd.net/api/download/{id}/{version}/x86_64-apple-darwin"),
            );
            RegistryEntry {
                id: id.to_string(),
                version: version.to_string(),
                min_host: min_host.to_string(),
                archs: vec!["aarch64-apple-darwin".into(), "x86_64-apple-darwin".into()],
                size: 1,
                sha256: sha,
                name: id.to_string(),
                description: None,
                i18n: None,
                icon_url: None,
                changelog_url: None,
                download: dl,
            }
        }

        #[test]
        fn resolve_entry_uses_requested_version() {
            let plugins = vec![entry("x", "1.0.0"), entry("x", "2.0.0"), entry("y", "1.0.0")];
            let e = resolve_entry(&plugins, "x", Some("2.0.0"), "6.804.1").unwrap();
            assert_eq!(e.version, "2.0.0");
        }

        /// An explicit `@version` this host cannot run is a DELIBERATE user
        /// override: selection honors it and the installer refuses it
        /// authoritatively ("requires notemd X") after verification. Pinned so
        /// the min_host pre-filter never leaks into the explicit path.
        #[test]
        fn resolve_entry_requested_version_wins_even_when_incompatible() {
            let plugins = vec![
                entry_min_host("x", "1.0.0", ">=6.716.7"),
                entry_min_host("x", "2.0.0", ">=99.0.0"),
            ];
            let e = resolve_entry(&plugins, "x", Some("2.0.0"), "6.804.1").unwrap();
            assert_eq!(e.version, "2.0.0");
        }

        #[test]
        fn resolve_entry_requested_missing_errors() {
            let plugins = vec![entry("x", "1.0.0")];
            let err = resolve_entry(&plugins, "x", Some("9.9.9"), "6.804.1").unwrap_err();
            assert!(err.contains("version '9.9.9' not found"), "got {err}");
        }

        #[test]
        fn resolve_entry_no_version_picks_advertised() {
            let plugins = vec![entry("x", "1.4.0")];
            let e = resolve_entry(&plugins, "x", None, "6.804.1").unwrap();
            assert_eq!(e.version, "1.4.0");
        }

        #[test]
        fn resolve_entry_unknown_id_errors() {
            let plugins = vec![entry("x", "1.0.0")];
            let err = resolve_entry(&plugins, "nope", None, "6.804.1").unwrap_err();
            assert!(err.contains("not found in registry"), "got {err}");
        }

        /// THE BUG: the index carries one entry per published version, so
        /// `find(|e| e.id == id)` returned whichever came first — the oldest, in
        /// the registry's id+version-ascending order. Fed out of order here on
        /// purpose: the answer must come from comparing versions, never from the
        /// index's own sort.
        #[test]
        fn resolve_entry_no_version_picks_newest_compatible() {
            let plugins = vec![
                entry_min_host("notemd.roam-import", "1.1.0", ">=6.803.0"),
                entry_min_host("notemd.roam-import", "1.0.4", ">=6.716.7"),
                entry_min_host("notemd.roam-import", "1.2.0", ">=6.803.0"),
                entry_min_host("other.plugin", "9.9.9", ">=0.0.0"),
            ];
            let e = resolve_entry(&plugins, "notemd.roam-import", None, "6.804.1").unwrap();
            assert_eq!(e.version, "1.2.0");
        }

        /// Versions where string comparison gets the order backwards.
        #[test]
        fn resolve_entry_orders_versions_numerically_not_lexically() {
            let plugins = vec![entry("x", "1.9.0"), entry("x", "1.10.0"), entry("x", "1.2.0")];
            let e = resolve_entry(&plugins, "x", None, "6.804.1").unwrap();
            assert_eq!(e.version, "1.10.0", "'1.10.0' > '1.9.0' numerically");
        }

        #[test]
        fn resolve_entry_skips_versions_this_host_cannot_run() {
            let plugins = vec![
                entry_min_host("x", "1.0.0", ">=6.716.7"),
                entry_min_host("x", "1.1.0", ">=6.716.7"),
                entry_min_host("x", "2.0.0", ">=6.900.0"),
            ];
            let e = resolve_entry(&plugins, "x", None, "6.804.1").unwrap();
            assert_eq!(e.version, "1.1.0", "newest COMPATIBLE, not newest overall");
        }

        /// Host-side ordering that string comparison gets backwards: host
        /// 6.10.0 satisfies `>=6.9.0`.
        #[test]
        fn resolve_entry_compares_host_numerically() {
            let plugins = vec![entry_min_host("x", "1.0.0", ">=6.9.0")];
            assert!(resolve_entry(&plugins, "x", None, "6.10.0").is_ok());
            assert!(resolve_entry(&plugins, "x", None, "6.8.9").is_err());
        }

        /// When the host satisfies NO published version, selection agrees with
        /// `pickAvailable` (the newest overall is the one that would be shown)
        /// but install refuses up front with the installer's honest wording,
        /// instead of downloading a package that is certain to be rejected.
        #[test]
        fn resolve_entry_errors_when_no_version_is_compatible() {
            let plugins = vec![
                entry_min_host("x", "1.0.0", ">=6.900.0"),
                entry_min_host("x", "2.0.0", ">=7.000.0"),
            ];
            let err = resolve_entry(&plugins, "x", None, "6.804.1").unwrap_err();
            assert!(err.contains("requires notemd >=7.000.0"), "got {err}");
            assert!(err.contains("2.0.0"), "must name the newest version: {err}");
            assert!(err.contains("host is 6.804.1"), "got {err}");
            assert!(err.contains("install x@<version>"), "must offer the override: {err}");
        }

        /// A `min_host` this parser doesn't understand reads as SATISFIED, so
        /// selection never hides a version the installer might accept (it
        /// re-checks `engines.notemd` with full semver anyway). Same fail-open
        /// as `minHostSatisfied` in select.ts.
        #[test]
        fn min_host_unrecognized_syntax_fails_open() {
            assert!(min_host_satisfied("^1.2.3", "0.0.1"));
            assert!(min_host_satisfied("~6.8", "0.0.1"));
            assert!(min_host_satisfied(">=1.0.0-beta", "0.0.1"));
            assert!(min_host_satisfied("*", "0.0.1"));
            assert!(min_host_satisfied("", "0.0.1"));
            let plugins = vec![entry_min_host("x", "3.0.0", "^9.9.9")];
            assert_eq!(resolve_entry(&plugins, "x", None, "6.804.1").unwrap().version, "3.0.0");
        }

        /// The comparator subset the registry ships, matched against
        /// select.test.ts case for case.
        #[test]
        fn min_host_matches_select_ts_semantics() {
            assert!(min_host_satisfied(">=6.716.7", "6.716.7"));
            assert!(min_host_satisfied(">=6.716.7", "6.720.0"));
            assert!(!min_host_satisfied(">=6.716.7", "6.716.6"));
            assert!(min_host_satisfied(">=6.9.0", "6.10.0"));
            assert!(min_host_satisfied(">=1.0.0, <2.0.0", "1.5.0"));
            assert!(!min_host_satisfied(">=1.0.0, <2.0.0", "2.0.0"));
            assert!(min_host_satisfied("<=2.0.0", "2.0.0"));
            assert!(!min_host_satisfied(">2.0.0", "2.0.0"));
            assert!(min_host_satisfied("=2.0.0", "2.0.0"));
            assert!(!min_host_satisfied("=2.0.0", "2.0.1"));
        }

        #[test]
        fn version_gt_is_numeric_and_component_wise() {
            assert!(version_gt("1.10.0", "1.9.0"));
            assert!(!version_gt("1.9.0", "1.10.0"));
            assert!(version_gt("6.10.0", "6.9.0"));
            assert!(!version_gt("1.0.0", "1.0.0"));
            assert!(version_gt("1.0.1", "1.0"));
            assert!(!version_gt("1.0", "1.0.0"));
        }

        /// Arch availability is NOT part of version selection (select.ts ignores
        /// it too): a newest version with no package for this arch produces an
        /// honest error rather than a silent downgrade to an older one. The user
        /// can still ask for the older one by name.
        #[test]
        fn resolve_entry_ignores_arch_availability_and_select_download_reports_it() {
            let mut newest_no_arch = entry("x", "2.0.0");
            newest_no_arch.download.clear();
            newest_no_arch.sha256.clear();
            newest_no_arch.archs.clear();
            let plugins = vec![entry("x", "1.0.0"), newest_no_arch];

            let e = resolve_entry(&plugins, "x", None, "6.804.1").unwrap();
            assert_eq!(e.version, "2.0.0", "no silent downgrade to the arch-complete 1.0.0");
            let err = select_download(&e).unwrap_err();
            assert!(err.contains("no download for arch"), "got {err}");

            // The explicit escape hatch still installs cleanly.
            let older = resolve_entry(&plugins, "x", Some("1.0.0"), "6.804.1").unwrap();
            assert!(select_download(&older).is_ok());
        }

        // ── plugin update ────────────────────────────────────────────────────

        /// `plugin update` had the same first-match defect: with the live index
        /// (1.0.4, 1.1.0, 1.2.0) it compared the installed version against the
        /// OLDEST entry and reported "up-to-date" forever.
        #[test]
        fn pick_update_entry_targets_newest_compatible() {
            let plugins = vec![
                entry_min_host("r", "1.1.0", ">=6.803.0"),
                entry_min_host("r", "1.0.4", ">=6.716.7"),
                entry_min_host("r", "1.2.0", ">=6.803.0"),
            ];
            let e = pick_update_entry(&plugins, "r", "1.0.4", "6.804.1").unwrap();
            assert_eq!(e.version, "1.2.0");
        }

        #[test]
        fn pick_update_entry_never_offers_an_incompatible_newer_version() {
            let plugins = vec![
                entry_min_host("r", "1.0.4", ">=6.716.7"),
                entry_min_host("r", "1.2.0", ">=6.900.0"),
            ];
            // Held back, and the note says why instead of a bare "up-to-date".
            let note = pick_update_entry(&plugins, "r", "1.0.4", "6.804.1").unwrap_err();
            assert!(note.contains("1.2.0"), "got {note}");
            assert!(note.contains("needs notemd >=6.900.0"), "got {note}");
        }

        #[test]
        fn pick_update_entry_notes_up_to_date_and_unknown() {
            let plugins = vec![entry("r", "1.0.4"), entry("r", "1.2.0")];
            assert_eq!(pick_update_entry(&plugins, "r", "1.2.0", "6.804.1").unwrap_err(), "up-to-date");
            assert_eq!(pick_update_entry(&plugins, "nope", "1.0.0", "6.804.1").unwrap_err(), "not in registry");
        }

        #[test]
        fn pick_update_entry_orders_numerically() {
            let plugins = vec![entry("r", "1.9.0"), entry("r", "1.10.0")];
            let e = pick_update_entry(&plugins, "r", "1.9.0", "6.804.1").unwrap();
            assert_eq!(e.version, "1.10.0");
        }

        /// A two-component registry version parses fine under `version_gt` (the
        /// TS-matching comparator that orders candidates) but not under strict
        /// `semver::Version`. Before the fix, `pick_update_entry` ordered by
        /// `version_gt` but gated the final decision with a *different*,
        /// semver-only comparator that rejects "1.2" outright — so this update
        /// was silently swallowed as "up-to-date". One comparator must decide.
        #[test]
        fn pick_update_entry_offers_two_component_newer_version() {
            let plugins = vec![entry("r", "1.2")];
            let e = pick_update_entry(&plugins, "r", "1.1.9", "6.804.1").unwrap();
            assert_eq!(e.version, "1.2");
        }

        /// An INSTALLED version that strict semver cannot parse (four dotted
        /// components here) must not freeze updates forever. Before the fix,
        /// the decision gate tried `semver::Version::parse` on the installed
        /// string, failed, and always answered "not newer" — hiding a
        /// genuinely newer, well-formed candidate.
        #[test]
        fn pick_update_entry_offers_update_past_unparseable_installed_version() {
            let plugins = vec![entry("r", "2.0.0")];
            let e = pick_update_entry(&plugins, "r", "1.2.0.9", "6.804.1").unwrap();
            assert_eq!(e.version, "2.0.0");
        }

        /// An older candidate must never be offered, regardless of how odd its
        /// (or the installed version's) shape is — the fix must not turn the
        /// gate into an accidental "always update" rubber stamp.
        #[test]
        fn pick_update_entry_no_update_when_candidate_older_regardless_of_parse_shape() {
            let plugins = vec![entry("r", "1.5")];
            let err = pick_update_entry(&plugins, "r", "2.0.0", "6.804.1").unwrap_err();
            assert_eq!(err, "up-to-date");
        }

        /// Equal versions are never "an update", even when the shared shape is
        /// one strict semver would reject.
        #[test]
        fn pick_update_entry_no_update_when_candidate_equal_to_installed() {
            let plugins = vec![entry("r", "1.2")];
            let err = pick_update_entry(&plugins, "r", "1.2", "6.804.1").unwrap_err();
            assert_eq!(err, "up-to-date");
        }

        /// The decision and the ordering must be the SAME function: whatever
        /// `pick_update_entry` returns is exactly what `newest` already picked
        /// among host-compatible candidates — never a different entry reached
        /// by a second, disagreeing comparator. Before the fix, the newest
        /// compatible entry ("1.1", two-component) was rejected by the strict
        /// gate, and the fallback "overall newest" (1.2.0, incompatible) was
        /// then found "newer" by the strict gate too — so the old code
        /// answered with the WRONG reason (incompatibility) for what should
        /// have been a clean update to the two-component compatible version.
        #[test]
        fn pick_update_entry_decision_agrees_with_newest_among_compatible() {
            let plugins = vec![
                entry_min_host("r", "1.0.4", ">=6.716.7"),
                entry_min_host("r", "1.1", ">=6.716.7"),
                entry_min_host("r", "1.2.0", ">=6.900.0"), // incompatible with host below
            ];
            let group: Vec<&RegistryEntry> = plugins.iter().filter(|e| e.id == "r").collect();
            let expected = newest(&group, "6.804.1", true).unwrap();
            let got = pick_update_entry(&plugins, "r", "1.0.4", "6.804.1").unwrap();
            assert_eq!(got.version, expected.version, "decision must agree with the ordering");
            assert_eq!(got.version, "1.1", "must pick the newest COMPATIBLE entry — 1.2.0 is incompatible");
        }

        /// Pin: for today's live registry, every version is a well-formed
        /// x.y.z, so the two comparators always agreed before this refactor.
        /// The fix must not change this answer.
        #[test]
        fn pick_update_entry_well_formed_versions_unchanged() {
            let plugins = vec![
                entry_min_host("r", "1.0.4", ">=6.716.7"),
                entry_min_host("r", "1.1.0", ">=6.803.0"),
                entry_min_host("r", "1.2.0", ">=6.803.0"),
            ];
            let e = pick_update_entry(&plugins, "r", "1.0.4", "6.804.1").unwrap();
            assert_eq!(e.version, "1.2.0");
        }

        #[test]
        fn select_download_picks_current_arch() {
            let triple = discovery::current_arch_triple().expect("supported arch");
            let (url, sha) = select_download(&entry("x", "1.0.0")).unwrap();
            assert!(url.ends_with(triple), "url {url} must target host arch {triple}");
            assert!(!sha.is_empty());
        }

        /// A ui-only plugin (roam-import) publishes only under `universal`; the
        /// resolver must fall back to it on any supported host arch (FIX-1).
        fn universal_entry(id: &str, version: &str) -> RegistryEntry {
            let mut sha = BTreeMap::new();
            sha.insert("universal".to_string(), "uu".to_string());
            let mut dl = BTreeMap::new();
            dl.insert(
                "universal".to_string(),
                format!("https://plugins.notemd.net/api/download/{id}/{version}/universal"),
            );
            RegistryEntry {
                id: id.to_string(),
                version: version.to_string(),
                min_host: ">=0.0.0".to_string(),
                archs: vec!["universal".into()],
                size: 1,
                sha256: sha,
                name: id.to_string(),
                description: None,
                i18n: None,
                icon_url: None,
                changelog_url: None,
                download: dl,
            }
        }

        #[test]
        fn select_download_falls_back_to_universal() {
            let (url, sha) = select_download(&universal_entry("roam", "1.0.0")).unwrap();
            assert!(url.ends_with("universal"), "url {url} must resolve to the universal package");
            assert_eq!(sha, "uu");
        }

        #[test]
        fn select_download_errors_when_neither_triple_nor_universal() {
            let mut e = entry("x", "1.0.0");
            e.download.clear();
            e.sha256.clear();
            let err = select_download(&e).unwrap_err();
            assert!(err.contains("no download for arch"), "got {err}");
        }

        #[test]
        fn sig_url_is_pkg_plus_minisig() {
            assert_eq!(
                sig_url_for("https://h/api/download/x/1.0.0/aarch64-apple-darwin"),
                "https://h/api/download/x/1.0.0/aarch64-apple-darwin.minisig"
            );
        }

        #[test]
        fn install_err_maps_to_exit_code() {
            assert_eq!(exit_for_install_err(&installer::InstallError::Hash), EXIT_VERIFY);
            assert_eq!(exit_for_install_err(&installer::InstallError::Signature), EXIT_VERIFY);
            assert_eq!(exit_for_install_err(&installer::InstallError::IdMismatch), EXIT_RUNTIME);
            assert_eq!(exit_for_install_err(&installer::InstallError::Unpack("x".into())), EXIT_RUNTIME);
            assert_eq!(exit_for_install_err(&installer::InstallError::Io("x".into())), EXIT_RUNTIME);
        }

        /// The CLI plugins root must derive identically to `runner::
        /// v2_plugins_root` so GUI and CLI operate on the same install tree.
        #[test]
        fn plugins_root_matches_runner_derivation() {
            let expected = dirs::data_dir()
                .map(|d| d.join(crate::app_dirs::BUNDLE_ID).join("plugins"));
            assert_eq!(plugins_root(), expected);
        }

        fn seed_state(root: &std::path::Path, id: &str, enabled: bool) {
            let mut s = state::InstallState::default();
            s.installed.insert(
                id.to_string(),
                state::InstalledPlugin { version: "1.0.0".into(), enabled },
            );
            state::save(root, &s).unwrap();
        }

        /// Disabling then re-enabling a v2 plugin must flip the state.json flag
        /// both ways — re-enable especially, since a disabled plugin is absent
        /// from `current_scan`'s manifest list and can only be found here.
        #[test]
        fn set_v2_enabled_at_toggles_state_json_both_ways() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            seed_state(root, "notemd.md2pdf", true);

            assert!(set_v2_enabled_at(root, "notemd.md2pdf", false).unwrap().is_ok());
            assert!(!state::load(root).installed["notemd.md2pdf"].enabled);

            assert!(set_v2_enabled_at(root, "notemd.md2pdf", true).unwrap().is_ok());
            assert!(state::load(root).installed["notemd.md2pdf"].enabled);
        }

        /// A non-installed id returns None, which the caller reports as an
        /// unknown plugin id.
        #[test]
        fn set_v2_enabled_at_unknown_id_is_none() {
            let dir = tempfile::tempdir().unwrap();
            seed_state(dir.path(), "notemd.md2pdf", true);
            assert!(set_v2_enabled_at(dir.path(), "base", true).is_none());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_host::{PluginManifest, CliEntry};
    use std::collections::HashMap;

    fn share_manifest() -> PluginManifest {
        PluginManifest {
            id: "share".to_string(),
            name: "Share".to_string(),
            version: "0.1.0".to_string(),
            description: Some("Publish current file as a shareable web page".to_string()),
            kind: crate::plugin_host::PluginKind::External,
            binary: Some("bin".to_string()),
            default_enabled: None,
            menus: vec![],
            context_menus: vec![],
            custom_editors: vec![],
            settings: None,
            host_capabilities: vec![],
            timeout_seconds: 30,
            i18n: HashMap::new(),
            manifest_version: None,
            open_windows: None,
            cli: vec![CliEntry {
                subcommand: "share".to_string(),
                aliases: vec!["--share".to_string()],
                command: "publish".to_string(),
                summary: "Render and publish file as a shareable URL".to_string(),
                args: vec![],
                flags: vec![],
                requires_tab_context: true,
            }],
        }
    }

    #[test] fn help_includes_share_when_enabled() {
        let mut enabled = HashMap::new();
        enabled.insert("share".to_string(), true);
        let out = render_help(None, false, &[share_manifest()], &enabled);
        assert!(out.contains("PLUGIN COMMANDS:"));
        assert!(out.contains("share"));
        assert!(out.contains("[Share]"));
        assert!(out.contains("Render and publish"));
    }
    #[test] fn help_all_includes_disabled_section() {
        let mut enabled = HashMap::new();
        enabled.insert("share".to_string(), false);
        let out = render_help(None, true, &[share_manifest()], &enabled);
        assert!(out.contains("DISABLED COMMANDS:"));
        assert!(out.contains("notemd plugin enable share"));
    }
    #[test] fn help_lists_global_options() {
        let out = render_help(None, false, &[], &HashMap::new());
        assert!(out.contains("GLOBAL OPTIONS:"));
        assert!(out.contains("--json"));
        assert!(out.contains("-q, --quiet"));
        assert!(out.contains("-y, --yes"));
        assert!(out.contains("--no-clipboard"));
    }
    #[test] fn help_topic_resolves_core_commands() {
        for topic in ["help", "version", "plugin", "share", "reading-insights"] {
            let out = render_help(Some(topic), false, &[], &HashMap::new());
            assert!(out.contains(&format!("notemd {topic}")), "topic {topic} not documented");
            assert!(!out.contains("unknown topic"), "topic {topic} rendered as unknown");
        }
    }
    #[test] fn help_topic_share_is_core_no_manifest_needed() {
        // share 已 core 化：无任何 manifest 时 help share / help --share 都必须解析。
        for topic in ["share", "--share"] {
            let out = render_help(Some(topic), false, &[], &HashMap::new());
            assert!(out.contains("notemd share"), "topic {topic} missing header");
            assert!(out.contains("Render and publish"), "topic {topic} missing summary");
            assert!(out.contains("--unshare"), "topic {topic} missing flags");
            assert!(out.contains("EXIT CODES:"), "topic {topic} missing exit codes");
        }
    }
    #[test] fn help_root_with_core_stubs_lists_share_exactly_once() {
        // builtin 的 current_scan 故意不注入 core stub；这里直接把 stub 传给
        // render_help，钉死不变量：share 只出现一次（CORE COMMANDS 行），
        // 绝不在 PLUGIN COMMANDS 里重复。
        let stubs = crate::cli::runner::core_cli_stub_manifests();
        let mut enabled = HashMap::new();
        for m in &stubs { enabled.insert(m.id.clone(), true); }
        let out = render_help(None, false, &stubs, &enabled);
        let share_rows = out.lines()
            .filter(|l| l.trim_start().starts_with("share "))
            .count();
        assert_eq!(share_rows, 1, "share must appear exactly once, got:\n{out}");
        assert!(!out.contains("PLUGIN COMMANDS:"),
            "core stubs must never render a PLUGIN COMMANDS section:\n{out}");
    }
    /// Task 18: the CLI topic is the other place (besides AGENTS.md's
    /// "Searching this vault" section) an agent learns this command's
    /// grammar. Pin that it actually documents the `page:` filter (it has no
    /// `--page` flag — the only way to discover it is this text) and the
    /// extra `--json` fields, so an agent reading `notemd help search` gets
    /// the same picture `--json` output and the query grammar actually give it.
    #[test]
    fn help_search_topic_documents_page_filter_and_json_fields() {
        let out = render_help(Some("search"), false, &[], &HashMap::new());
        assert!(out.contains("page:[[X]]"), "must document the page: filter:\n{out}");
        assert!(out.contains("source_ref"), "must document --json's source_ref field:\n{out}");
        assert!(out.contains("provenance"), "must document --json's provenance field:\n{out}");
        assert!(out.contains("agent_by"), "must explain what provenance.agent_by means:\n{out}");
    }
    /// `--json` 的字段是 agent 的公共约定,加了字段就得写进帮助,
    /// 否则只有读源码的人知道它存在。
    #[test]
    fn search_help_documents_attention_minutes() {
        let out = render_help(Some("search"), false, &[], &HashMap::new());
        assert!(out.contains("attention_minutes"), "必须记录 --json 的注意力字段:\n{out}");
    }
    /// Review round 1, Important #1: `render_core_topic` used to append a
    /// generic "1 Runtime error" footer underneath every topic's own body,
    /// including `search`'s, which already states "1 = no hits (not an
    /// error)" — the one document written specifically so an agent can branch
    /// on exit codes contradicted itself about the exit code it hits most
    /// often. Pin that `search` states its exit codes exactly once, and that
    /// the generic/contradictory wording is nowhere in the topic.
    #[test]
    fn help_search_topic_states_exit_codes_once_and_without_the_generic_contradiction() {
        let out = render_help(Some("search"), false, &[], &HashMap::new());
        assert_eq!(
            out.matches("EXIT CODES:").count(), 1,
            "exit codes must appear exactly once, not doubled by the generic footer:\n{out}"
        );
        assert!(out.contains("No hits"), "exit 1 must be documented as 'no hits':\n{out}");
        assert!(!out.contains("Runtime error"), "the generic footer's contradictory wording must not appear:\n{out}");
    }
    /// The other core topics must keep getting the generic footer — this is
    /// what proves the `body.contains("EXIT CODES:")` guard is scoped to
    /// `search` alone, not a regression that silently dropped it everywhere.
    #[test]
    fn other_core_topics_still_get_the_generic_exit_codes_footer() {
        for topic in ["help", "version", "plugin", "share"] {
            let out = render_help(Some(topic), false, &[], &HashMap::new());
            assert_eq!(
                out.matches("EXIT CODES:").count(), 1,
                "topic {topic} must show exit codes exactly once:\n{out}"
            );
            assert!(out.contains("Runtime error"), "topic {topic} must keep the generic footer:\n{out}");
        }
    }
    #[test] fn help_share_topic_documents_every_stub_flag() {
        // 契约对齐：share stub 的 cli entry 声明的每个 flag 长名，都必须出现在
        // `notemd help share` 的 core topic 文本里（stub 与 help 文案同步演进）。
        let stubs = crate::cli::runner::core_cli_stub_manifests();
        let share = stubs.iter().find(|m| m.id == "share").expect("share stub exists");
        let topic = render_help(Some("share"), false, &[], &HashMap::new());
        for entry in &share.cli {
            for f in &entry.flags {
                assert!(topic.contains(&f.long),
                    "help share topic missing flag {}", f.long);
            }
        }
    }
    #[test] fn help_root_lists_share_as_core_command() {
        let out = render_help(None, false, &[], &HashMap::new());
        assert!(out.contains("CORE COMMANDS:"));
        assert!(out.contains("share"));
        assert!(out.contains("Render and publish file as a shareable URL"));
        // core 化后不该出现插件小节（无 manifest 时）。
        assert!(!out.contains("PLUGIN COMMANDS:"));
    }
    #[test] fn help_topic_shows_per_subcommand_detail() {
        // 用非 core 子命令测 manifest 主题路径（share 现在被 core topic 遮蔽）。
        let mut m = share_manifest();
        m.id = "demo".to_string();
        m.name = "Demo".to_string();
        m.cli[0].subcommand = "demo".to_string();
        m.cli[0].aliases = vec!["--demo".to_string()];
        let mut enabled = HashMap::new();
        enabled.insert("demo".to_string(), true);
        let out = render_help(Some("demo"), false, &[m], &enabled);
        assert!(out.contains("notemd demo"));
        assert!(out.contains("Render and publish"));
        assert!(out.contains("Provided by: Demo plugin"));
        assert!(out.contains("EXIT CODES:"));
    }
    #[test] fn version_string_includes_plugin_api() {
        let v = render_version(false);
        assert!(v.contains("notemd"));
        assert!(v.contains("plugin API v1"));
    }
    #[test] fn version_json_is_parsable() {
        let v = render_version(true);
        let _: serde_json::Value = serde_json::from_str(&v).expect("valid JSON");
    }
    #[test] fn plugin_list_rows_enabled_and_disabled() {
        let mut enabled = HashMap::new();
        enabled.insert("share".to_string(), false);
        let out = render_plugin_list(false, &[share_manifest()], &enabled);
        assert!(out.contains("share"));
        assert!(out.contains("disabled"));
    }
    #[test] fn plugin_list_json_array() {
        let mut enabled = HashMap::new();
        enabled.insert("share".to_string(), true);
        let out = render_plugin_list(true, &[share_manifest()], &enabled);
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let arr = v["data"].as_array().expect("data is array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["status"], "enabled");
    }
}
