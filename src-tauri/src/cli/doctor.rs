//! `notemd doctor` —— 一条命令自检 notemd 的全部本地能力。
//!
//! 只诊断,不修复:每项检查读状态、给判断、附一条可执行的下一步,绝不改动
//! 任何文件。判断逻辑全部复用各子系统已有的权威实现(`install::status`、
//! `git_ops`、`discovery` 的校验链、`vault_settings` 的权重校验、`searchidx`),
//! 因为 doctor 自带一份判断的话,两份必然漂移 —— 见设计文档 §1。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
    Skip,
}

impl Status {
    fn symbol(self) -> char {
        match self {
            Status::Pass => '✓',
            Status::Warn => '⚠',
            Status::Fail => '✗',
            Status::Skip => '-',
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: String,
    pub status: Status,
    pub detail: String,
    pub hint: Option<String>,
}

impl Check {
    pub fn pass(id: &str, detail: impl Into<String>) -> Self {
        Self { id: id.into(), status: Status::Pass, detail: detail.into(), hint: None }
    }
    pub fn warn(id: &str, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self { id: id.into(), status: Status::Warn, detail: detail.into(), hint: Some(hint.into()) }
    }
    pub fn fail(id: &str, detail: impl Into<String>, hint: impl Into<String>) -> Self {
        Self { id: id.into(), status: Status::Fail, detail: detail.into(), hint: Some(hint.into()) }
    }
    pub fn skip(id: &str, detail: impl Into<String>) -> Self {
        Self { id: id.into(), status: Status::Skip, detail: detail.into(), hint: None }
    }
}

/// 分组 = id 的第一个点号段。插件检查的 id 是 `plugin.<插件id>`,而插件 id 自带
/// 点号(`notemd.md2pdf`),所以只能取第一段,不能按最后一个点切。
pub fn group_of(id: &str) -> &str {
    id.split('.').next().unwrap_or(id)
}

/// 分组的展示顺序;不在表里的分组按首次出现顺序排在最后。
const GROUP_ORDER: [&str; 5] = ["env", "vault", "search", "plugin", "net"];

#[derive(Debug, Clone, Default)]
pub struct DoctorArgs {
    pub offline: bool,
    pub vault: Option<String>,
    pub json: bool,
}

impl DoctorArgs {
    pub fn with_global_json(mut self, global: bool) -> Self {
        self.json = self.json || global;
        self
    }
}

pub fn parse_args(rest: &[String], json_global: bool) -> DoctorArgs {
    let mut a = DoctorArgs { json: json_global, ..Default::default() };
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "--offline" => a.offline = true,
            "--json" => a.json = true,
            "--vault" => {
                if let Some(v) = rest.get(i + 1) {
                    a.vault = Some(v.clone());
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    a
}

/// warn / skip 不影响退出码 —— 未装软链、vault 非 git 仓库、断网都是合法运行
/// 态,doctor 返回 0 才能安全地写进 `notemd doctor && …`(设计文档 §5)。
pub fn exit_code_for(checks: &[Check]) -> u8 {
    if checks.iter().any(|c| c.status == Status::Fail) { 1 } else { 0 }
}

fn count(checks: &[Check], s: Status) -> usize {
    checks.iter().filter(|c| c.status == s).count()
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn ordered_groups(checks: &[Check]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in GROUP_ORDER {
        if checks.iter().any(|c| group_of(&c.id) == g) {
            out.push(g.to_string());
        }
    }
    for c in checks {
        let g = group_of(&c.id).to_string();
        if !out.contains(&g) {
            out.push(g);
        }
    }
    out
}

pub fn render_plain(checks: &[Check]) -> String {
    let mut out = String::new();
    for g in ordered_groups(checks) {
        out.push_str(&format!("{}\n", g.to_uppercase()));
        for c in checks.iter().filter(|c| group_of(&c.id) == g) {
            out.push_str(&format!("  {} {:<24} {}\n", c.status.symbol(), c.id, c.detail));
            if c.status != Status::Pass {
                if let Some(h) = &c.hint {
                    out.push_str(&format!("      → {h}\n"));
                }
            }
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "{}, {}, {}, {}\n",
        plural(count(checks, Status::Pass), "passed", "passed"),
        plural(count(checks, Status::Warn), "warning", "warnings"),
        plural(count(checks, Status::Fail), "failure", "failures"),
        plural(count(checks, Status::Skip), "skipped", "skipped"),
    ));
    out
}

pub fn render_json(checks: &[Check]) -> String {
    let arr: Vec<serde_json::Value> = checks
        .iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id,
                "group": group_of(&c.id),
                "status": c.status,
                "detail": c.detail,
                "hint": c.hint,
            })
        })
        .collect();
    serde_json::json!({
        "ok": exit_code_for(checks) == 0,
        "data": {
            "checks": arr,
            "summary": {
                "passed": count(checks, Status::Pass),
                "warnings": count(checks, Status::Warn),
                "failures": count(checks, Status::Fail),
                "skipped": count(checks, Status::Skip),
            }
        }
    })
    .to_string()
}

// ── 环境组 ────────────────────────────────────────────────────────────────

/// `install::status` 已经区分「没装」和「装了但 target 不是当前二进制」,但
/// 它的 `target_valid` 问的是「是否指向*本进程*的二进制」—— 那对 doctor 太严:
/// 多个安装、dev 构建都会让它为 false 而软链本身完全可用。真正的坏情况是
/// **指向一个不存在的文件**(dangling),所以 target 是否存在由调用方解析后传入。
fn check_cli_link(installed: bool, path: Option<&str>, target_exists: Option<bool>) -> Check {
    if !installed {
        return Check::warn(
            "env.cli_link",
            "not installed",
            "Install it in Preferences → General → Command line, so `notemd` works in a terminal",
        );
    }
    let p = path.unwrap_or("(unknown path)");
    if target_exists == Some(false) {
        return Check::fail(
            "env.cli_link",
            format!("{p} points at a target that no longer exists"),
            "Reinstall it in Preferences → General → Command line",
        );
    }
    Check::pass("env.cli_link", p)
}

fn check_git(version: Option<&str>) -> Check {
    match version {
        Some(v) => Check::pass("env.git", v),
        None => Check::fail(
            "env.git",
            "git not found on PATH",
            "Install git (on macOS: xcode-select --install) — Vault sync cannot run without it",
        ),
    }
}

fn check_git_proxy(raw: Option<&str>) -> Check {
    match crate::vault_sync::git_ops::validate_proxy_url(raw.unwrap_or("")) {
        Ok(None) => Check::pass("env.git_proxy", "not configured"),
        Ok(Some(url)) => Check::pass("env.git_proxy", url),
        Err(e) => Check::fail(
            "env.git_proxy",
            e,
            "Fix or clear the proxy in Preferences → Sync",
        ),
    }
}

fn env_checks(cfg: Option<&crate::shared_config::SharedConfig>) -> Vec<Check> {
    let st = super::install::status(None);
    // 自己解析软链目标:`InstallStatus` 只带链接路径,不带目标是否存在。
    // 读不出目标(不是软链)⇒ None ⇒ 宽容按通过处理。
    let target_exists = st
        .path
        .as_deref()
        .map(std::path::Path::new)
        .and_then(|p| std::fs::read_link(p).ok())
        .map(|t| t.exists());
    vec![
        check_cli_link(st.installed, st.path.as_deref(), target_exists),
        check_git(crate::vault_sync::git_ops::version().as_deref()),
        check_git_proxy(cfg.and_then(|c| c.git_proxy.as_deref())),
    ]
}

// ── 配置与 vault 组 ────────────────────────────────────────────────────────

/// **刻意不走 `shared_config::read()`**:那个函数是 fail-soft 的,文件缺失和
/// 内容损坏都返回同一个默认值,而这两者对用户意味着完全不同的事(全新安装 vs
/// 配置被写坏)。doctor 的整个价值就在于把它们分开说。
fn check_shared_config(path: &Path) -> (Check, Option<crate::shared_config::SharedConfig>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                Check::warn(
                    "vault.shared_config",
                    format!("not created yet: {}", path.display()),
                    "Normal on a fresh install — set a Vault in Preferences and it appears",
                ),
                None,
            )
        }
        Err(e) => {
            return (
                Check::fail(
                    "vault.shared_config",
                    format!("{}: {e}", path.display()),
                    "Check the file's permissions",
                ),
                None,
            )
        }
    };
    match serde_json::from_str::<crate::shared_config::SharedConfig>(&text) {
        Ok(cfg) => (Check::pass("vault.shared_config", path.display().to_string()), Some(cfg)),
        Err(e) => (
            Check::fail(
                "vault.shared_config",
                format!("{} is not valid JSON: {e}", path.display()),
                "Move it aside and re-pick your Vault in Preferences",
            ),
            None,
        ),
    }
}

fn check_vault_root(
    explicit: Option<&str>,
    cfg: Option<&crate::shared_config::SharedConfig>,
) -> (Check, Option<PathBuf>) {
    let (raw, source) = match explicit {
        Some(v) => (v.to_string(), "--vault"),
        None => match cfg.and_then(|c| c.sotvault.as_deref()).filter(|s| !s.is_empty()) {
            Some(v) => (v.to_string(), "configured"),
            None => {
                return (
                    Check::warn(
                        "vault.sotvault",
                        "no Vault configured",
                        "Pick one in Preferences, or pass --vault <path>",
                    ),
                    None,
                )
            }
        },
    };
    let root = PathBuf::from(&raw);
    if root.is_dir() {
        (Check::pass("vault.sotvault", format!("{raw} ({source})")), Some(root))
    } else {
        (
            Check::fail(
                "vault.sotvault",
                format!("{raw} does not exist ({source})"),
                "Re-pick the Vault in Preferences, or reconnect the volume it lives on",
            ),
            None,
        )
    }
}

fn check_git_repo(root: &Path) -> Check {
    // git worktree 的 `.git` 是文件而非目录,所以用 exists 而不是 is_dir。
    if root.join(".git").exists() {
        Check::pass("vault.git_repo", "git repository")
    } else {
        Check::warn(
            "vault.git_repo",
            "not a git repository",
            "Fine for local-only use; Vault sync and history need `git init` plus a remote",
        )
    }
}

/// 同 [`check_shared_config`]:`vault_settings::read` 把损坏文件吞成默认值,
/// 所以这里自己读自己解析,再把解出来的权重交给**已有的**校验函数。
fn check_vault_settings(root: &Path) -> Check {
    let path = root.join(".notemd").join("settings.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Check::pass("vault.settings", "using defaults (no .notemd/settings.json)")
        }
        Err(e) => {
            return Check::fail(
                "vault.settings",
                format!("{}: {e}", path.display()),
                "Check the file's permissions",
            )
        }
    };
    let settings: crate::sotvault::vault_settings::VaultSettings =
        match serde_json::from_str(&text) {
            Ok(s) => s,
            Err(e) => {
                return Check::fail(
                    "vault.settings",
                    format!("{} is not valid JSON: {e}", path.display()),
                    "Fix the JSON, or delete the file to fall back to defaults",
                )
            }
        };
    // `search_weights` 是 Option —— 没设过就没什么可校验的。
    match settings.search_weights.as_ref().map(crate::sotvault::vault_settings::validate_search_weights) {
        None | Some(Ok(())) => Check::pass("vault.settings", path.display().to_string()),
        Some(Err(e)) => Check::warn(
            "vault.settings",
            e,
            "Out-of-range weights are clamped at query time; fix them in Preferences → Search",
        ),
    }
}

fn vault_checks(args: &DoctorArgs) -> (Vec<Check>, Option<crate::shared_config::SharedConfig>, Option<PathBuf>) {
    let path = crate::shared_config::config_path().ok();
    vault_checks_from(args, path.as_deref())
}

/// [`vault_checks`] 的可测核心:配置文件路径显式传入。`None` 表示平台上根本
/// 解析不出配置目录。
fn vault_checks_from(
    args: &DoctorArgs,
    config_path: Option<&Path>,
) -> (Vec<Check>, Option<crate::shared_config::SharedConfig>, Option<PathBuf>) {
    let mut out = Vec::new();
    let cfg = match config_path {
        Some(p) => {
            let (c, cfg) = check_shared_config(p);
            out.push(c);
            cfg
        }
        None => {
            out.push(Check::fail(
                "vault.shared_config",
                "no config directory on this platform",
                "Report this — notemd cannot store settings here",
            ));
            None
        }
    };
    let (c, root) = check_vault_root(args.vault.as_deref(), cfg.as_ref());
    out.push(c);
    match root.as_deref() {
        Some(r) => {
            out.push(check_git_repo(r));
            out.push(check_vault_settings(r));
        }
        None => {
            // 没有 vault 就没有判断依据 —— 记 skip,不连坐报 fail。
            out.push(Check::skip("vault.git_repo", "no Vault to check"));
            out.push(Check::skip("vault.settings", "no Vault to check"));
        }
    }
    (out, cfg, root)
}

// ── 搜索索引组 ────────────────────────────────────────────────────────────

/// 与 `notemd search` 同一预算:诊断不许阻塞调用方。
const SWEEP_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);

fn search_checks(root: Option<&Path>) -> Vec<Check> {
    let Some(root) = root else {
        return vec![
            Check::skip("search.index_open", "no Vault to check"),
            Check::skip("search.stats", "no Vault to check"),
            Check::skip("search.skipped_large", "no Vault to check"),
        ];
    };
    let db = searchidx::paths::index_db_path(root);
    search_checks_at(root, db.as_deref())
}

/// [`search_checks`] 的可测核心:索引 DB 路径显式传入。
fn search_checks_at(root: &Path, db_path: Option<&Path>) -> Vec<Check> {
    let Some(db) = db_path else {
        return vec![
            Check::warn(
                "search.index_open",
                "no local data directory on this platform",
                "Search falls back to scanning files directly",
            ),
            Check::skip("search.stats", "no index"),
            Check::skip("search.skipped_large", "no index"),
        ];
    };
    // **开库之前**先看文件在不在:`SearchIndex::open` 会创建 DB,而 doctor 是
    // 只读命令 —— 索引没建过就该说「没建过」,不该顺手替用户建一个。
    if !db.is_file() {
        return vec![
            Check::warn(
                "search.index_open",
                format!("not built yet: {}", db.display()),
                "Build it with: notemd search --stats",
            ),
            Check::skip("search.stats", "no index"),
            Check::skip("search.skipped_large", "no index"),
        ];
    }

    // stamp 必须来自 `scan_options_for` 产出的 ScanOptions —— 独立重算会把
    // 完全健康的索引误判成失效(见 `SearchIndex::open` 的文档注释)。
    let opts = super::search::scan_options_for(root);
    let stamp = opts.source_globs.stamp();
    let mut idx = match searchidx::SearchIndex::open_at(root, db, &stamp) {
        Ok(i) => i,
        Err(e) => {
            return vec![
                Check::warn(
                    "search.index_open",
                    format!("cannot open {}: {e}", db.display()),
                    "Rebuild it with: notemd search --rebuild <any query>",
                ),
                Check::skip("search.stats", "index unavailable"),
                Check::skip("search.skipped_large", "index unavailable"),
            ]
        }
    };
    let mut out = vec![Check::pass("search.index_open", db.display().to_string())];

    // 增量 sweep(不是全量首建)—— 与 `notemd search` 每次调用都做的派生数据
    // 维护完全一致,并且共用同一个 2s 预算。
    let swept = idx.sweep(&opts, Some(SWEEP_DEADLINE));

    out.push(match idx.stats() {
        Ok(s) => {
            let detail = format!(
                "{} file{}, {} block{}, {:.1} MB, tokenizer {}{}",
                s.files,
                if s.files == 1 { "" } else { "s" },
                s.blocks,
                if s.blocks == 1 { "" } else { "s" },
                s.db_bytes as f64 / 1_048_576.0,
                s.tokenizer_id,
                s.built_at.as_deref().map(|b| format!(", built {b}")).unwrap_or_default(),
            );
            if s.files == 0 {
                Check::warn(
                    "search.stats",
                    detail,
                    "Nothing is indexed — run: notemd search --rebuild <any query>",
                )
            } else {
                Check::pass("search.stats", detail)
            }
        }
        Err(e) => Check::warn("search.stats", e, "Rebuild with: notemd search --rebuild <any query>"),
    });

    out.push(match &swept {
        Ok(s) if s.files_skipped_large.is_empty() => {
            let note = if s.timed_out { " (freshness sweep hit its 2s budget; list may be partial)" } else { "" };
            Check::pass("search.skipped_large", format!("none{note}"))
        }
        Ok(s) => {
            let list = s
                .files_skipped_large
                .iter()
                .map(|f| format!("{} ({:.1} MB)", f.path, f.size as f64 / 1_048_576.0))
                .collect::<Vec<_>>()
                .join(", ");
            Check::warn(
                "search.skipped_large",
                format!("invisible to search: {list}"),
                "Raise searchLargeFileThresholdMb in <vault>/.notemd/settings.json, or keep using rg for these",
            )
        }
        Err(e) => Check::warn(
            "search.skipped_large",
            format!("freshness sweep failed: {e}"),
            "Rebuild with: notemd search --rebuild <any query>",
        ),
    });

    out
}

/// 采集全部检查。本任务接入配置、vault 与搜索索引组,后续任务逐组填充其余分组。
fn collect(args: &DoctorArgs) -> Vec<Check> {
    let (vault, cfg, root) = vault_checks(args);
    let mut out = env_checks(cfg.as_ref());
    out.extend(vault);
    out.extend(search_checks(root.as_deref()));
    out
}

pub fn run(args: DoctorArgs) -> ExitCode {
    let checks = collect(&args);
    if args.json {
        println!("{}", render_json(&checks));
    } else {
        print!("{}", render_plain(&checks));
    }
    ExitCode::from(exit_code_for(&checks))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, status: Status) -> Check {
        Check { id: id.to_string(), status, detail: "d".into(), hint: None }
    }

    #[test]
    fn group_is_the_first_dot_segment() {
        assert_eq!(group_of("env.git"), "env");
        // 插件 id 自带点号，分组仍必须是 "plugin"。
        assert_eq!(group_of("plugin.notemd.md2pdf"), "plugin");
        assert_eq!(group_of("nodots"), "nodots");
    }

    #[test]
    fn exit_code_is_zero_when_all_pass() {
        assert_eq!(exit_code_for(&[c("a.x", Status::Pass), c("a.y", Status::Pass)]), 0);
    }

    #[test]
    fn warnings_and_skips_do_not_change_the_exit_code() {
        // 未装软链 / vault 非 git 仓库 / 断网都是合法运行态，doctor 必须仍返回 0，
        // 否则它无法安全地进脚本（spec §5）。
        let checks = [c("a.x", Status::Pass), c("a.y", Status::Warn), c("a.z", Status::Skip)];
        assert_eq!(exit_code_for(&checks), 0);
    }

    #[test]
    fn any_failure_yields_one() {
        let checks = [c("a.x", Status::Pass), c("a.y", Status::Fail), c("a.z", Status::Warn)];
        assert_eq!(exit_code_for(&checks), 1);
    }

    #[test]
    fn json_envelope_has_ok_checks_and_summary() {
        let checks = vec![
            Check::pass("env.git", "git version 2.39.3"),
            Check::warn("env.cli_link", "not installed", "Install it in Preferences"),
        ];
        let v: serde_json::Value = serde_json::from_str(&render_json(&checks)).unwrap();
        assert_eq!(v["ok"], serde_json::json!(true));
        assert_eq!(v["data"]["checks"][0]["id"], "env.git");
        assert_eq!(v["data"]["checks"][0]["group"], "env");
        assert_eq!(v["data"]["checks"][0]["status"], "pass");
        assert_eq!(v["data"]["checks"][1]["hint"], "Install it in Preferences");
        assert_eq!(v["data"]["summary"]["passed"], 1);
        assert_eq!(v["data"]["summary"]["warnings"], 1);
        assert_eq!(v["data"]["summary"]["failures"], 0);
        assert_eq!(v["data"]["summary"]["skipped"], 0);
    }

    #[test]
    fn json_ok_is_false_when_something_failed() {
        let checks = vec![Check::fail("env.git", "not found", "Install git")];
        let v: serde_json::Value = serde_json::from_str(&render_json(&checks)).unwrap();
        assert_eq!(v["ok"], serde_json::json!(false));
        assert_eq!(v["data"]["summary"]["failures"], 1);
    }

    #[test]
    fn plain_output_groups_and_summarizes() {
        let checks = vec![
            Check::pass("env.git", "git version 2.39.3"),
            Check::fail("vault.sotvault", "vault not found: /nope", "Set it in Preferences"),
        ];
        let out = render_plain(&checks);
        assert!(out.contains("ENV"), "{out}");
        assert!(out.contains("VAULT"), "{out}");
        assert!(out.contains("✓ env.git"), "{out}");
        assert!(out.contains("✗ vault.sotvault"), "{out}");
        // hint 只在非 pass 项出现，并且缩进成续行。
        assert!(out.contains("→ Set it in Preferences"), "{out}");
        assert!(!out.contains("→ git version"), "{out}");
        assert!(out.contains("1 passed, 0 warnings, 1 failure, 0 skipped"), "{out}");
    }

    #[test]
    fn parse_args_reads_offline_and_vault() {
        let rest: Vec<String> = ["--offline", "--vault", "/tmp/v"].iter().map(|s| s.to_string()).collect();
        let a = parse_args(&rest, false);
        assert!(a.offline);
        assert_eq!(a.vault.as_deref(), Some("/tmp/v"));
        assert!(!a.json);
    }

    #[test]
    fn global_json_flag_reaches_doctor_args() {
        let a = parse_args(&[], false).with_global_json(true);
        assert!(a.json);
    }

    #[test]
    fn cli_link_absent_is_a_warning_not_a_failure() {
        // GUI 用户不装软链是完全正常的，不能因此让 doctor 退出 1。
        let c = check_cli_link(false, None, None);
        assert_eq!(c.status, Status::Warn);
        assert!(c.hint.is_some());
    }

    #[test]
    fn cli_link_present_and_resolvable_passes() {
        let c = check_cli_link(true, Some("/usr/local/bin/notemd"), Some(true));
        assert_eq!(c.status, Status::Pass);
        assert!(c.detail.contains("/usr/local/bin/notemd"), "{}", c.detail);
    }

    #[test]
    fn cli_link_pointing_at_a_missing_target_fails() {
        // dangling 软链 = 命令存在但一跑就报 "no such file"，必须是 fail。
        let c = check_cli_link(true, Some("/usr/local/bin/notemd"), Some(false));
        assert_eq!(c.status, Status::Fail);
    }

    #[test]
    fn cli_link_that_is_not_a_symlink_passes() {
        // 非软链（Windows shim、拷贝的二进制）读不出 target；宽容处理，不误报。
        let c = check_cli_link(true, Some("/usr/local/bin/notemd"), None);
        assert_eq!(c.status, Status::Pass);
    }

    #[test]
    fn missing_git_is_a_failure() {
        let c = check_git(None);
        assert_eq!(c.status, Status::Fail);
        assert_eq!(c.id, "env.git");
    }

    #[test]
    fn present_git_reports_its_version() {
        let c = check_git(Some("git version 2.39.3"));
        assert_eq!(c.status, Status::Pass);
        assert!(c.detail.contains("2.39.3"), "{}", c.detail);
    }

    #[test]
    fn unset_proxy_passes() {
        assert_eq!(check_git_proxy(None).status, Status::Pass);
        assert_eq!(check_git_proxy(Some("  ")).status, Status::Pass);
    }

    #[test]
    fn valid_proxy_passes_and_invalid_one_fails() {
        assert_eq!(check_git_proxy(Some("socks5://127.0.0.1:1080")).status, Status::Pass);
        let c = check_git_proxy(Some("ftp://nope"));
        assert_eq!(c.status, Status::Fail);
        // 复用 git_ops::validate_proxy_url 的原话，不另写一套错误文案。
        assert!(c.detail.contains("unsupported proxy scheme"), "{}", c.detail);
    }

    /// 缺失和损坏必须分开报 —— 这条测试同时钉住「不许改用 shared_config::read()」:
    /// 那个函数把两种情况都吞成默认值,一旦有人图省事换过去,两条断言会同时变红。
    #[test]
    fn missing_shared_config_warns_and_corrupt_one_fails() {
        let dir = tempfile::tempdir().unwrap();

        let (c, cfg) = check_shared_config(&dir.path().join("shared.json"));
        assert_eq!(c.status, Status::Warn);
        assert!(cfg.is_none());

        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, "{ not json").unwrap();
        let (c, cfg) = check_shared_config(&bad);
        assert_eq!(c.status, Status::Fail);
        assert!(cfg.is_none());
    }

    #[test]
    fn well_formed_shared_config_passes_and_yields_the_config() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("shared.json");
        std::fs::write(&p, r#"{"version":1,"sotvault":"/tmp/v"}"#).unwrap();
        let (c, cfg) = check_shared_config(&p);
        assert_eq!(c.status, Status::Pass);
        assert_eq!(cfg.unwrap().sotvault.as_deref(), Some("/tmp/v"));
    }

    #[test]
    fn unconfigured_vault_warns_and_yields_no_root() {
        let (c, root) = check_vault_root(None, None);
        assert_eq!(c.status, Status::Warn);
        assert!(root.is_none());
    }

    #[test]
    fn configured_but_missing_vault_dir_fails() {
        let cfg = crate::shared_config::SharedConfig {
            version: 1,
            sotvault: Some("/definitely/not/here".into()),
            ..Default::default()
        };
        let (c, root) = check_vault_root(None, Some(&cfg));
        assert_eq!(c.status, Status::Fail);
        assert!(root.is_none(), "一个不存在的目录不能继续喂给后面的检查");
    }

    #[test]
    fn explicit_vault_flag_wins_over_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = crate::shared_config::SharedConfig {
            version: 1,
            sotvault: Some("/definitely/not/here".into()),
            ..Default::default()
        };
        let (c, root) = check_vault_root(Some(dir.path().to_str().unwrap()), Some(&cfg));
        assert_eq!(c.status, Status::Pass);
        assert_eq!(root.as_deref(), Some(dir.path()));
    }

    #[test]
    fn a_vault_without_git_is_only_a_warning() {
        // 「文件高于应用」下 vault 不必是 git 仓库,只是同步能力不可用。
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(check_git_repo(dir.path()).status, Status::Warn);
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        assert_eq!(check_git_repo(dir.path()).status, Status::Pass);
    }

    #[test]
    fn absent_vault_settings_passes_and_corrupt_one_fails() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(check_vault_settings(dir.path()).status, Status::Pass);

        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(dir.path().join(".notemd/settings.json"), "{ not json").unwrap();
        assert_eq!(check_vault_settings(dir.path()).status, Status::Fail);
    }

    #[test]
    fn out_of_range_search_weights_warn() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(
            dir.path().join(".notemd/settings.json"),
            r#"{"searchWeights":{"human":99.0}}"#,
        )
        .unwrap();
        let c = check_vault_settings(dir.path());
        assert_eq!(c.status, Status::Warn);
        assert!(c.detail.contains("human"), "{}", c.detail);
    }

    #[test]
    fn unconfigured_vault_skips_the_dependent_checks_instead_of_failing_them() {
        // 没配 vault 时,git_repo / settings 记 skip,不连坐报 fail(设计文档 §4.2)。
        let args = DoctorArgs { offline: true, vault: None, ..Default::default() };
        let checks = vault_checks_from(&args, None).0;
        let dependent: Vec<&Check> = checks
            .iter()
            .filter(|c| c.id == "vault.git_repo" || c.id == "vault.settings")
            .collect();
        assert_eq!(dependent.len(), 2);
        assert!(dependent.iter().all(|c| c.status == Status::Skip), "{dependent:?}");
    }

    #[test]
    fn no_vault_skips_the_whole_search_group() {
        let checks = search_checks(None);
        assert!(!checks.is_empty());
        assert!(checks.iter().all(|c| c.status == Status::Skip), "{checks:?}");
    }

    /// 索引还没建过 ⇒ warn + 提示怎么建,而**不是**就地建一个:
    /// 全量首建可能跑很久,而 doctor 必须是秒级的只读命令(设计文档 §4.3)。
    #[test]
    fn an_unbuilt_index_warns_and_does_not_create_the_db() {
        let vault = tempfile::tempdir().unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("index.db");

        let checks = search_checks_at(vault.path(), Some(&db));

        assert!(!db.exists(), "doctor 绝不能建库");
        let open = checks.iter().find(|c| c.id == "search.index_open").unwrap();
        assert_eq!(open.status, Status::Warn);
        assert!(open.hint.as_deref().unwrap().contains("notemd search"), "{open:?}");
        // 打不开就没有统计可言 —— 后两项记 skip。
        assert!(checks.iter().any(|c| c.id == "search.stats" && c.status == Status::Skip));
    }

    #[test]
    fn an_existing_index_reports_stats() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("a.md"), "# Title\n\nhello doctor\n").unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("index.db");

        // 先按 search 命令同款的方式真建一次索引，doctor 才有东西可看。
        let opts = crate::cli::search::scan_options_for(vault.path());
        let stamp = opts.source_globs.stamp();
        let mut idx = searchidx::SearchIndex::open_at(vault.path(), &db, &stamp).unwrap();
        idx.ensure_built(&opts).unwrap();
        drop(idx);

        let checks = search_checks_at(vault.path(), Some(&db));
        let open = checks.iter().find(|c| c.id == "search.index_open").unwrap();
        assert_eq!(open.status, Status::Pass, "{open:?}");
        let stats = checks.iter().find(|c| c.id == "search.stats").unwrap();
        assert_eq!(stats.status, Status::Pass, "{stats:?}");
        assert!(stats.detail.contains("1 file"), "{}", stats.detail);
    }

    #[test]
    fn an_index_over_an_empty_vault_warns_about_zero_files() {
        let vault = tempfile::tempdir().unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("index.db");
        let opts = crate::cli::search::scan_options_for(vault.path());
        let stamp = opts.source_globs.stamp();
        let mut idx = searchidx::SearchIndex::open_at(vault.path(), &db, &stamp).unwrap();
        idx.ensure_built(&opts).unwrap();
        drop(idx);

        let checks = search_checks_at(vault.path(), Some(&db));
        let stats = checks.iter().find(|c| c.id == "search.stats").unwrap();
        assert_eq!(stats.status, Status::Warn, "{stats:?}");
    }
}
