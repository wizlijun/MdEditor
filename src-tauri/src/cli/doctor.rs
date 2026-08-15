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
    /// Tokens `parse_args` did not recognize as one of doctor's own flags —
    /// global flags (`--json`/`-q`/`--cli`/…) are stripped by `args::parse`
    /// before `rest` ever reaches here, so anything landing in `unknown` is
    /// genuinely a typo or an argument doctor does not support. `--vault`
    /// with no following value also lands here: silently falling back to the
    /// *configured* vault when the caller explicitly asked to check a
    /// different one would report on the wrong directory. `run()` must check
    /// this before doing any work and exit 2 — see `help doctor`'s EXIT CODES
    /// section, which promises exactly that contract.
    pub unknown: Vec<String>,
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
            "--vault" => match rest.get(i + 1) {
                Some(v) => {
                    a.vault = Some(v.clone());
                    i += 1;
                }
                None => a.unknown.push("--vault".to_string()),
            },
            other => a.unknown.push(other.to_string()),
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
///
/// `target_valid == false` while the target *does* exist is a third,
/// distinct state (neither "broken" nor "fine"): the symlink resolves to
/// some *other* build — an old download-directory copy, a dev build — and
/// `notemd` in a terminal silently runs that instead of this one. Not a
/// `fail` (the command works), but worth a `warn` with the actual target so
/// the user can tell at a glance whether that is expected.
fn check_cli_link(
    installed: bool,
    path: Option<&str>,
    target_exists: Option<bool>,
    target_valid: bool,
    target: Option<&str>,
) -> Check {
    // `install.rs` self-describes as "macOS-only in v1": `candidate_dirs()`
    // is all macOS paths and the Windows PATH shim is laid down by the NSIS
    // installer, never through this check chain. Reporting "not installed"
    // here on Windows would point users at Preferences → General → Command
    // line, a menu item that only exists on macOS.
    if !cfg!(target_os = "macos") {
        return Check::skip("env.cli_link", "managed by the installer on this platform");
    }
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
    if target_exists == Some(true) && !target_valid {
        let t = target.unwrap_or("(unknown target)");
        return Check::warn(
            "env.cli_link",
            format!("{p} points at another build: {t}"),
            "If unexpected, reinstall it in Preferences → General → Command line to point at this build",
        );
    }
    Check::pass("env.cli_link", p)
}

/// Whether a symlink's target exists, resolving a **relative** target
/// against the directory the link itself lives in — not the process's
/// current working directory. `std::fs::read_link` returns the target
/// exactly as stored (e.g. `../../Applications/note.md.app/…`), and
/// `Path::exists` on that raw value resolves relative to cwd; a perfectly
/// working relative symlink checked from any other directory would then
/// read as dangling. Returns `None` when `p` is not a symlink at all
/// (permissive — a Windows shim or a copied binary should not be flagged).
fn symlink_target_exists(p: &Path) -> Option<bool> {
    let target = std::fs::read_link(p).ok()?;
    let resolved = if target.is_relative() {
        p.parent().unwrap_or(Path::new(".")).join(&target)
    } else {
        target
    };
    Some(resolved.exists())
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
    // 自己解析软链目标是否存在(见 `symlink_target_exists` 的文档注释:相对
    // 路径必须按链接*所在目录*解析,不是当前工作目录)。读不出目标(不是
    // 软链)⇒ None ⇒ 宽容按通过处理。目标的原始文本另外读一次,只用于
    // "points at another build" 的展示,不参与判断。
    let link_path = st.path.as_deref().map(Path::new);
    let target_exists = link_path.and_then(symlink_target_exists);
    let target_display =
        link_path.and_then(|p| std::fs::read_link(p).ok()).map(|t| t.display().to_string());
    vec![
        check_cli_link(
            st.installed,
            st.path.as_deref(),
            target_exists,
            st.target_valid,
            target_display.as_deref(),
        ),
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
            "Out-of-range weights fall back to the shipped default at query time; fix them in Preferences → Search",
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
            // 同 search.index_open / plugin.root 在数据目录解析不出时的判法
            // 统一成 warn:这三处都是"平台标准目录 API 返回空"这一类情况,在
            // 实践中不可达,也不是用户能修的东西(不是配置错误,是平台层面
            // 的异常)——不该让 doctor 因为这个退出 1。
            out.push(Check::warn(
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

fn search_checks(root: Option<&Path>, vault_flag: Option<&str>) -> Vec<Check> {
    let Some(root) = root else {
        return vec![
            Check::skip("search.index_open", "no Vault to check"),
            Check::skip("search.stats", "no Vault to check"),
            Check::skip("search.skipped_large", "no Vault to check"),
        ];
    };
    let db = searchidx::paths::index_db_path(root);
    search_checks_at(root, db.as_deref(), vault_flag)
}

/// [`search_checks`] 的可测核心:索引 DB 路径显式传入。生产路径固定用
/// [`SWEEP_DEADLINE`];[`search_checks_at_with_deadline`] 把预算也做成参数,
/// 让"sweep 超时"这个分支能在单测里确定性触发,不用真的等 2 秒或跟机器速度
/// 赛跑。
fn search_checks_at(root: &Path, db_path: Option<&Path>, vault_flag: Option<&str>) -> Vec<Check> {
    search_checks_at_with_deadline(root, db_path, vault_flag, SWEEP_DEADLINE)
}

/// hint 里建议的 `notemd search` 命令要不要带 `--vault`:当这次检查的 vault
/// 根来自 `--vault` 参数(而不是已配置的那个)时,不带 `--vault` 的命令会让
/// 读者复制粘贴出一条解析到**另一个**目录的命令。`--stats` 而非 `<any
/// query>`:`search.rs` 的 `run()` 在要求 query 之前就为 `--stats` return 了
/// (search.rs:199-201),所以 `--rebuild --stats` 同样有效且不需要用户凭空
/// 编一个查询词。
fn search_rebuild_hint(vault_flag: Option<&str>) -> String {
    match vault_flag {
        Some(v) => format!("notemd search --rebuild --stats --vault {v}"),
        None => "notemd search --rebuild --stats".to_string(),
    }
}

fn search_stats_hint(vault_flag: Option<&str>) -> String {
    match vault_flag {
        Some(v) => format!("notemd search --stats --vault {v}"),
        None => "notemd search --stats".to_string(),
    }
}

fn search_checks_at_with_deadline(
    root: &Path,
    db_path: Option<&Path>,
    vault_flag: Option<&str>,
    deadline: std::time::Duration,
) -> Vec<Check> {
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
                format!("Build it with: {}", search_stats_hint(vault_flag)),
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
                    format!("Rebuild it with: {}", search_rebuild_hint(vault_flag)),
                ),
                Check::skip("search.stats", "index unavailable"),
                Check::skip("search.skipped_large", "index unavailable"),
            ]
        }
    };
    let mut out = vec![Check::pass("search.index_open", db.display().to_string())];

    // 增量 sweep(不是全量首建)—— 与 `notemd search` 每次调用都做的派生数据
    // 维护完全一致,并且共用同一个预算(生产路径是 `SWEEP_DEADLINE`)。
    let swept = idx.sweep(&opts, Some(deadline));

    // Important 2(终审):这条 note 曾经只拼在 `search.skipped_large` 的
    // pass 分支上("none"里)——真正需要它的是 warn 分支(列出的大文件清单
    // 可能因为超时而不全),之前那条分支反而永远走不到 warn。提到 match 之
    // 前,两个 Ok 分支各自拼一次,而不是各写一份。
    let timeout_note = match &swept {
        Ok(s) if s.timed_out => " (freshness sweep hit its 2s budget; list may be partial)",
        _ => "",
    };

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
            // M1(终审): `files == 0` alone is not evidence of a problem — a
            // freshly built index over a genuinely empty (of indexable
            // content) vault also reports zero, and "run --rebuild" changes
            // nothing there. Only warn when the *sweep itself* couldn't
            // rule that out: it timed out before finishing, or it actually
            // indexed something this round (meaning the DB's zero predates
            // real work that just happened, i.e. something's inconsistent).
            let sweep_found_nothing_to_explain_the_zero =
                matches!(&swept, Ok(sw) if !sw.timed_out && sw.files_indexed == 0);
            if s.files == 0 && !sweep_found_nothing_to_explain_the_zero {
                Check::warn(
                    "search.stats",
                    detail,
                    format!("Nothing is indexed — run: {}", search_rebuild_hint(vault_flag)),
                )
            } else if s.files == 0 {
                Check::pass("search.stats", format!("{detail}, no indexable files"))
            } else {
                Check::pass("search.stats", detail)
            }
        }
        Err(e) => Check::warn("search.stats", e, format!("Rebuild with: {}", search_rebuild_hint(vault_flag))),
    });

    out.push(match &swept {
        Ok(s) if s.files_skipped_large.is_empty() => {
            Check::pass("search.skipped_large", format!("none{timeout_note}"))
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
                format!("invisible to search: {list}{timeout_note}"),
                "Raise searchLargeFileThresholdMb in <vault>/.notemd/settings.json, or keep using rg for these",
            )
        }
        Err(e) => Check::warn(
            "search.skipped_large",
            format!("freshness sweep failed: {e}"),
            format!("Rebuild with: {}", search_rebuild_hint(vault_flag)),
        ),
    });

    out
}

// ── 插件系统组 ────────────────────────────────────────────────────────────

fn plugin_checks(root: Option<&Path>, host_version: &str) -> Vec<Check> {
    let Some(root) = root else {
        return vec![Check::warn(
            "plugin.root",
            "cannot resolve the app data directory",
            "Report this — notemd cannot find where plugins are installed",
        )];
    };
    if !root.exists() {
        // M3(终审):`plugin.state` must still appear here, pass, rather than
        // vanish — a JSON consumer diffing two runs (one before any plugin
        // was ever installed, one after `state.json` exists but is empty)
        // must see a stable check-id set, not one that grows a `plugin.state`
        // row the moment the directory is created for the first time.
        return vec![
            Check::pass("plugin.root", "no plugins installed"),
            Check::pass("plugin.state", "no plugins installed"),
        ];
    }
    let mut out = vec![Check::pass("plugin.root", root.display().to_string())];

    // 同 shared.json / settings.json:`state::load` 把损坏当成空表,而空表和
    // 「所有插件都读不出来了」是天差地别的两件事。
    let state_path = root.join("state.json");
    match std::fs::read_to_string(&state_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            out.push(Check::pass("plugin.state", "no plugins installed"));
            return out;
        }
        Err(e) => {
            out.push(Check::fail(
                "plugin.state",
                format!("{}: {e}", state_path.display()),
                "Check the file's permissions",
            ));
            return out;
        }
        Ok(text) => match serde_json::from_str::<crate::plugin_runtime::state::InstallState>(&text) {
            Err(e) => {
                out.push(Check::fail(
                    "plugin.state",
                    format!("{} is not valid JSON: {e}", state_path.display()),
                    "Reinstall the affected plugins with: notemd plugin install <id>",
                ));
                return out;
            }
            Ok(state) => {
                out.push(Check::pass(
                    "plugin.state",
                    format!("{} installed", state.installed.len()),
                ));
                for (id, entry) in &state.installed {
                    let check_id = format!("plugin.{id}");
                    if !entry.enabled {
                        out.push(Check::skip(&check_id, format!("{} (disabled)", entry.version)));
                        continue;
                    }
                    let current = root.join(id).join("current");
                    // 同一个实现,不复刻:manifest 解析 + validate_manifest +
                    // id 一致 + 本机架构二进制存在,全在这一个函数里。
                    match crate::plugin_runtime::discovery::validate_installed(&current, id, host_version) {
                        Ok(m) => out.push(Check::pass(&check_id, m.version)),
                        Err(e) => out.push(Check::fail(
                            &check_id,
                            e,
                            format!("Reinstall it with: notemd plugin install {id}"),
                        )),
                    }
                }
            }
        },
    }
    out
}

// ── 网络组 ────────────────────────────────────────────────────────────────

/// 与 `tauri.conf.json` 的 `plugins.updater.endpoints[0]` 必须一致 —— 由
/// `updater_endpoint_matches_tauri_conf` 单测钉住。运行时解析 tauri.conf.json
/// 会引入一份只为诊断而存在的配置读取路径,常量 + 防漂移测试更便宜。
const UPDATER_ENDPOINT: &str =
    "https://github.com/wizlijun/note.md/releases/latest/download/latest.json";

const NET_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

fn net_checks(offline: bool) -> Vec<Check> {
    if offline {
        return vec![
            Check::skip("net.registry", "skipped (--offline)"),
            Check::skip("net.updater", "skipped (--offline)"),
        ];
    }
    let base = crate::plugin_runtime::market::registry_base_url_at(&super::resolve_config_dir());
    let client = match reqwest::Client::builder().timeout(NET_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("http client: {e}");
            return vec![
                Check::warn("net.registry", msg.clone(), "Retry; report this if it persists"),
                Check::warn("net.updater", msg, "Retry; report this if it persists"),
            ];
        }
    };
    // 两项并发发起,所以整组的耗时上界是单项超时(10s),不是两者相加。
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            let msg = format!("cannot start an async runtime: {e}");
            return vec![
                Check::warn("net.registry", msg.clone(), "Retry; report this if it persists"),
                Check::warn("net.updater", msg, "Retry; report this if it persists"),
            ];
        }
    };
    rt.block_on(async {
        let (registry, updater) =
            tokio::join!(registry_probe(&client, &base), updater_probe(&client, UPDATER_ENDPOINT));
        vec![registry, updater]
    })
}

/// `client` is injected: production builds one plain client above; tests
/// supply one with `.no_proxy()` so the loopback probes below cannot be
/// redirected through a system proxy — reqwest honours `HTTP(S)_PROXY` by
/// default, and a developer machine with one configured would otherwise turn
/// "connection refused" into whatever the proxy answers with instead,
/// silently flipping these tests' expected outcome. Same pattern as
/// `market::download_via`'s `client` parameter, for the same reason.
async fn registry_probe(client: &reqwest::Client, base: &str) -> Check {
    match crate::plugin_runtime::market::fetch_index_via(client, base).await {
        Ok(index) => Check::pass(
            "net.registry",
            format!("{base} ({} plugins)", index.plugins.len()),
        ),
        Err(e) => Check::warn(
            "net.registry",
            format!("{base}: {e}"),
            "The plugin market needs this; everything else works offline",
        ),
    }
}

async fn updater_probe(client: &reqwest::Client, url: &str) -> Check {
    match client.get(url).send().await {
        Ok(r) if r.status().is_success() => Check::pass("net.updater", "reachable"),
        Ok(r) => Check::warn(
            "net.updater",
            format!("{url} returned {}", r.status()),
            "Automatic updates will not find a release until this resolves",
        ),
        Err(e) => Check::warn(
            "net.updater",
            format!("{url}: {e}"),
            "Automatic updates need this; everything else works offline",
        ),
    }
}

/// 同步外壳,让上面两个 async 探针在单测里可直接调用。用 `.no_proxy()` 的
/// client(见 `registry_probe` 的文档注释)——保留端口 0 的 loopback 请求必须
/// 真正连接失败,不能被系统代理接住。
#[cfg(test)]
fn test_client() -> reqwest::Client {
    reqwest::Client::builder().timeout(NET_TIMEOUT).no_proxy().build().unwrap()
}

#[cfg(test)]
fn probe_registry_at(base: &str) -> Check {
    let client = test_client();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(registry_probe(&client, base))
}

#[cfg(test)]
fn probe_updater_at(url: &str) -> Check {
    let client = test_client();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(updater_probe(&client, url))
}

/// 采集全部检查。
fn collect(args: &DoctorArgs) -> Vec<Check> {
    let (vault, cfg, root) = vault_checks(args);
    let mut out = env_checks(cfg.as_ref());
    out.extend(vault);
    out.extend(search_checks(root.as_deref(), args.vault.as_deref()));
    out.extend(plugin_checks(
        super::runner::v2_plugins_root().as_deref(),
        env!("CARGO_PKG_VERSION"),
    ));
    out.extend(net_checks(args.offline));
    out
}

pub fn run(args: DoctorArgs) -> ExitCode {
    // Argument error: return before `collect()` runs anything, including the
    // two network probes — a typo'd `--offline` (e.g. `--ofline`) must not
    // silently fall through to "checked everything, made two requests,
    // exited 0" (see the `unknown` field's doc comment).
    if !args.unknown.is_empty() {
        for x in &args.unknown {
            eprintln!("notemd: unknown option '{x}' — see: notemd help doctor");
        }
        return ExitCode::from(2);
    }
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
        assert!(a.unknown.is_empty(), "{:?}", a.unknown);
    }

    /// Important 1(终审):未识别的 flag 必须落进 `unknown`,而不是被 `_ => {}`
    /// 静默吞掉 —— 否则 `notemd doctor --ofline` 这种拼错会照常跑完全部检查、
    /// 发两次网络请求、退出 0,脚本据此误判"离线自检通过"。
    #[test]
    fn parse_args_collects_unrecognized_tokens() {
        let rest: Vec<String> =
            ["--ofline", "--json", "--nope"].iter().map(|s| s.to_string()).collect();
        let a = parse_args(&rest, false);
        assert_eq!(a.unknown, vec!["--ofline".to_string(), "--nope".to_string()]);
        assert!(a.json, "recognized flags between the unknown ones must still be parsed");
    }

    /// `--vault` with no following value must not silently fall back to the
    /// *configured* vault — the caller explicitly asked to check a different
    /// one, and a silent fallback would report on the wrong directory.
    #[test]
    fn parse_args_treats_a_valueless_vault_flag_as_unknown() {
        let rest: Vec<String> = ["--vault"].iter().map(|s| s.to_string()).collect();
        let a = parse_args(&rest, false);
        assert_eq!(a.unknown, vec!["--vault".to_string()]);
        assert!(a.vault.is_none());
    }

    #[test]
    fn global_json_flag_reaches_doctor_args() {
        let a = parse_args(&[], false).with_global_json(true);
        assert!(a.json);
    }

    #[test]
    fn cli_link_absent_is_a_warning_not_a_failure() {
        // GUI 用户不装软链是完全正常的，不能因此让 doctor 退出 1。
        let c = check_cli_link(false, None, None, false, None);
        assert_eq!(c.status, Status::Warn);
        assert!(c.hint.is_some());
    }

    #[test]
    fn cli_link_present_and_resolvable_passes() {
        let c = check_cli_link(true, Some("/usr/local/bin/notemd"), Some(true), true, None);
        assert_eq!(c.status, Status::Pass);
        assert!(c.detail.contains("/usr/local/bin/notemd"), "{}", c.detail);
    }

    #[test]
    fn cli_link_pointing_at_a_missing_target_fails() {
        // dangling 软链 = 命令存在但一跑就报 "no such file"，必须是 fail。
        let c = check_cli_link(true, Some("/usr/local/bin/notemd"), Some(false), false, None);
        assert_eq!(c.status, Status::Fail);
    }

    #[test]
    fn cli_link_that_is_not_a_symlink_passes() {
        // 非软链（Windows shim、拷贝的二进制）读不出 target；宽容处理，不误报。
        let c = check_cli_link(true, Some("/usr/local/bin/notemd"), None, false, None);
        assert_eq!(c.status, Status::Pass);
    }

    /// M9(终审):target 存在但不是本进程的二进制 —— 另一个安装、dev 构建
    /// 在 PATH 上顶替了正版。曾经把 `target_valid` 整个丢掉,等于放弃这个
    /// 真实事故类型;现在必须 warn 并把实际目标路径带出来。
    #[test]
    fn cli_link_pointing_at_a_different_existing_build_warns() {
        let c = check_cli_link(
            true,
            Some("/usr/local/bin/notemd"),
            Some(true),
            false,
            Some("/tmp/dev-build/notemd"),
        );
        assert_eq!(c.status, Status::Warn);
        assert!(c.detail.contains("/tmp/dev-build/notemd"), "{}", c.detail);
    }

    /// M8(终审):`install.rs` 自述 macOS-only in v1;在其它平台上给出的
    /// "Preferences → General → Command line" 建议指向一个不存在的偏好项,
    /// 必须整条 skip 而不是 warn。
    #[test]
    #[cfg(not(target_os = "macos"))]
    fn cli_link_is_skipped_on_non_macos_platforms() {
        let c = check_cli_link(true, Some("C:/tools/notemd.exe"), Some(true), true, None);
        assert_eq!(c.status, Status::Skip);
    }

    /// Important 3(终审):相对软链的目标必须按链接*所在目录*解析,不是当前
    /// 工作目录 —— 否则 `~/.local/bin/notemd -> ../../Applications/…` 这种
    /// 完全可用的相对软链,在别的 cwd 下跑 doctor 会被判成 dangling(假
    /// fail,doctor 的核心卖点里最伤的一种)。
    #[test]
    fn relative_symlink_target_resolves_against_the_links_own_directory() {
        let dir = tempfile::tempdir().unwrap();
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::write(bin_dir.join("notemd-real"), b"").unwrap();

        let link_dir = dir.path().join("link");
        std::fs::create_dir_all(&link_dir).unwrap();

        let ok_link = link_dir.join("notemd");
        std::os::unix::fs::symlink("../bin/notemd-real", &ok_link).unwrap();
        assert_eq!(symlink_target_exists(&ok_link), Some(true), "a working relative symlink must not read as dangling");

        let dangling_link = link_dir.join("dangling");
        std::os::unix::fs::symlink("../bin/does-not-exist", &dangling_link).unwrap();
        assert_eq!(symlink_target_exists(&dangling_link), Some(false));
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

    /// M13(终审):`check_vault_root` deliberately re-implements
    /// `search::resolve_vault_root`'s resolution order (spec §4.2 — doctor
    /// needs to distinguish "not configured" from "configured but missing",
    /// which `resolve_vault_root`'s `Option<PathBuf>` collapses into one
    /// `None`). Two independent implementations of the same order drift
    /// silently the day either one gains a step; pin that they agree, for
    /// both the `--vault` and the configured-`sotvault` branch.
    #[test]
    fn vault_root_resolution_agrees_with_search_resolve_vault_root() {
        let dir = tempfile::tempdir().unwrap();
        let path_str = dir.path().to_str().unwrap();

        // --vault 显式参数分支:两边都必须是 "explicit wins, unconditionally".
        let cfg = crate::shared_config::SharedConfig {
            version: 1,
            sotvault: Some("/definitely/not/here".into()),
            ..Default::default()
        };
        let (_, doctor_root) = check_vault_root(Some(path_str), Some(&cfg));
        let search_root = crate::cli::search::resolve_vault_root(Some(path_str));
        assert_eq!(doctor_root, search_root, "explicit --vault 分支必须一致");

        // 未显式指定时落回配置里的 sotvault,且同样过滤空字符串
        // (`filter(|s| !s.is_empty())`)——这条钉住两边共享同一条过滤规则,
        // 不是各写一份、日后各自漂移。
        let configured = crate::shared_config::SharedConfig {
            version: 1,
            sotvault: Some(path_str.to_string()),
            ..Default::default()
        };
        let (_, doctor_root2) = check_vault_root(None, Some(&configured));
        let expected = configured.sotvault.clone().filter(|s| !s.is_empty()).map(PathBuf::from);
        assert_eq!(doctor_root2, expected, "配置分支的过滤规则必须与 search::resolve_vault_root 相同");
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

    /// M2(终审):config dir / data dir(search 组)/ data dir(plugin 组)—— 同
    /// 一类"平台标准目录 API 解析不出来"的情况,曾经给了三种 status
    /// (fail/warn/warn)。统一成 warn:这类情况实践中不可达,也不是用户能
    /// 修的东西,不该让 doctor 因此退出 1。
    #[test]
    fn no_config_directory_on_this_platform_is_a_warning_not_a_failure() {
        let args = DoctorArgs { offline: true, vault: None, ..Default::default() };
        let (checks, cfg, root) = vault_checks_from(&args, None);
        assert!(cfg.is_none());
        assert!(root.is_none());
        let c = checks.iter().find(|c| c.id == "vault.shared_config").unwrap();
        assert_eq!(c.status, Status::Warn, "{c:?}");
    }

    #[test]
    fn no_vault_skips_the_whole_search_group() {
        let checks = search_checks(None, None);
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

        let checks = search_checks_at(vault.path(), Some(&db), None);

        assert!(!db.exists(), "doctor 绝不能建库");
        let open = checks.iter().find(|c| c.id == "search.index_open").unwrap();
        assert_eq!(open.status, Status::Warn);
        assert!(open.hint.as_deref().unwrap().contains("notemd search"), "{open:?}");
        // 打不开就没有统计可言 —— 后两项记 skip。
        assert!(checks.iter().any(|c| c.id == "search.stats" && c.status == Status::Skip));
    }

    /// M7(终审):vault 根若来自 `--vault` 参数,hint 里建议的 `notemd
    /// search …` 必须原样带上 `--vault <path>` —— 不带的话读者复制粘贴出的
    /// 命令解析的是**已配置**的那个 vault,而不是刚被诊断的这一个。
    #[test]
    fn hint_includes_the_vault_flag_when_the_root_came_from_it() {
        let vault = tempfile::tempdir().unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("index.db");

        let checks = search_checks_at(vault.path(), Some(&db), Some("/explicit/vault"));
        let open = checks.iter().find(|c| c.id == "search.index_open").unwrap();
        assert!(
            open.hint.as_deref().unwrap().contains("--vault /explicit/vault"),
            "{open:?}"
        );
        // 没有显式 --vault 时不该凭空出现。
        let checks_no_flag = search_checks_at(vault.path(), Some(&db), None);
        let open_no_flag = checks_no_flag.iter().find(|c| c.id == "search.index_open").unwrap();
        assert!(!open_no_flag.hint.as_deref().unwrap().contains("--vault"), "{open_no_flag:?}");
    }

    /// M7(终审):`<any query>` 逼用户凭空编一个查询词;`--stats` 同样有效
    /// (search.rs 的 run() 在要求 query 之前就为 --stats return 了)且更自然。
    #[test]
    fn rebuild_hints_use_stats_not_a_placeholder_query() {
        assert_eq!(search_rebuild_hint(None), "notemd search --rebuild --stats");
        assert_eq!(
            search_rebuild_hint(Some("/v")),
            "notemd search --rebuild --stats --vault /v"
        );
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

        let checks = search_checks_at(vault.path(), Some(&db), None);
        let open = checks.iter().find(|c| c.id == "search.index_open").unwrap();
        assert_eq!(open.status, Status::Pass, "{open:?}");
        let stats = checks.iter().find(|c| c.id == "search.stats").unwrap();
        assert_eq!(stats.status, Status::Pass, "{stats:?}");
        // M5(终审):弱断言 `contains("1 file")` 连 "1 files"（英语误用复数）
        // 都会通过；钉住渲染实际产出的逗号，才真正验证了单复数分支。
        assert!(stats.detail.contains("1 file,"), "{}", stats.detail);
    }

    /// M1(终审):`files == 0` 本身不是问题的证据 —— 全新索引照在一个真的
    /// 没有可索引内容的 vault 上跑一遍,统计也是 0,而 `--rebuild` 对此毫无
    /// 意义。sweep 刚跑完、没超时、也没索引到任何东西时记 pass,不该恒 warn。
    #[test]
    fn an_index_over_an_empty_vault_passes_with_no_indexable_files() {
        let vault = tempfile::tempdir().unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("index.db");
        let opts = crate::cli::search::scan_options_for(vault.path());
        let stamp = opts.source_globs.stamp();
        let mut idx = searchidx::SearchIndex::open_at(vault.path(), &db, &stamp).unwrap();
        idx.ensure_built(&opts).unwrap();
        drop(idx);

        let checks = search_checks_at(vault.path(), Some(&db), None);
        let stats = checks.iter().find(|c| c.id == "search.stats").unwrap();
        assert_eq!(stats.status, Status::Pass, "{stats:?}");
        assert!(stats.detail.contains("no indexable files"), "{}", stats.detail);
    }

    /// M1(终审)的对照面:sweep 因为超时而没能确认 vault 是真的空 —— 这时
    /// `files == 0` 仍然可能只是"还没来得及看",必须继续 warn,不能误判成
    /// pass。用零预算强制第一次 over_budget() 检查就命中,确定性触发超时
    /// (`sweep_with_budget` 在遇到第一个候选文件时才检查预算 —— 见
    /// `searchidx::scan::sweep_with_budget`)。
    #[test]
    fn stats_still_warn_when_the_sweep_timed_out_before_confirming_emptiness() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::write(vault.path().join("a.md"), "content").unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("index.db");
        {
            let opts = crate::cli::search::scan_options_for(vault.path());
            let stamp = opts.source_globs.stamp();
            let _ = searchidx::SearchIndex::open_at(vault.path(), &db, &stamp).unwrap();
        }

        let checks =
            search_checks_at_with_deadline(vault.path(), Some(&db), None, std::time::Duration::from_secs(0));
        let stats = checks.iter().find(|c| c.id == "search.stats").unwrap();
        assert_eq!(stats.status, Status::Warn, "{stats:?}");
    }

    /// Important 2(终审):sweep 超时的"list may be partial"提示曾经只挂在
    /// `files_skipped_large` 为空的 pass 分支上 —— 恰好是不需要它的那条,
    /// 真正列出大文件清单的 warn 分支反而永远走不到。大文件清单非空 +
    /// 超时同时发生时,warn 分支的 detail 也必须带上这句提示。
    #[test]
    fn skipped_large_warning_carries_the_timeout_note_when_the_list_may_be_partial() {
        let vault = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(vault.path().join(".notemd")).unwrap();
        std::fs::write(
            vault.path().join(".notemd/settings.json"),
            r#"{"searchLargeFileThresholdMb":1}"#,
        )
        .unwrap();
        // 2MB，超过刚设的 1MB 阈值 —— walk() 无条件收集 skipped_large，不受
        // deadline 影响，所以即使零预算这个文件也一定会出现在列表里。
        std::fs::write(vault.path().join("big.md"), vec![b'x'; 2 * 1024 * 1024]).unwrap();
        // 一个正常大小的文件，好让候选队列非空 —— `over_budget()` 只在
        // 索引循环的每次迭代里检查(见 `searchidx::scan::sweep_with_budget`),
        // 候选队列为空时循环压根不跑一次，`timed_out` 就永远不会被置位。
        std::fs::write(vault.path().join("small.md"), "hello").unwrap();
        let dbdir = tempfile::tempdir().unwrap();
        let db = dbdir.path().join("index.db");
        {
            let opts = crate::cli::search::scan_options_for(vault.path());
            let stamp = opts.source_globs.stamp();
            let _ = searchidx::SearchIndex::open_at(vault.path(), &db, &stamp).unwrap();
        }

        let checks =
            search_checks_at_with_deadline(vault.path(), Some(&db), None, std::time::Duration::from_secs(0));
        let skipped = checks.iter().find(|c| c.id == "search.skipped_large").unwrap();
        assert_eq!(skipped.status, Status::Warn, "{skipped:?}");
        assert!(skipped.detail.contains("big.md"), "{}", skipped.detail);
        assert!(
            skipped.detail.contains("list may be partial"),
            "warn 分支必须也带上超时提示: {}",
            skipped.detail
        );
    }

    use crate::plugin_runtime::state::{InstallState, InstalledPlugin};

    fn write_plugin_state(root: &Path, entries: &[(&str, bool)]) {
        let mut s = InstallState::default();
        for (id, enabled) in entries {
            s.installed.insert(
                (*id).to_string(),
                InstalledPlugin { version: "1.0.0".into(), enabled: *enabled },
            );
        }
        crate::plugin_runtime::state::save(root, &s).unwrap();
    }

    /// 与 discovery 的测试同款:最小可用 manifest,binary 键就是当前架构三元组。
    fn fixture_manifest(id: &str, binary_key: &str) -> String {
        serde_json::json!({
            "manifest_version": 2,
            "id": id,
            "name": "Fixture",
            "version": "1.0.0",
            "kind": "native",
            "engines": { "notemd": ">=0.0.0" },
            "binary": { binary_key: "bin/fixture" },
            "activation": { "events": ["onCli:fixture"] },
            "capabilities": []
        })
        .to_string()
    }

    fn install_fixture(root: &Path, dir_id: &str, manifest: &str, with_binary: bool) {
        let current = root.join(dir_id).join("current");
        std::fs::create_dir_all(current.join("bin")).unwrap();
        std::fs::write(current.join("manifest.json"), manifest).unwrap();
        if with_binary {
            std::fs::write(current.join("bin/fixture"), b"#!/bin/sh\nexit 0\n").unwrap();
        }
    }

    fn triple() -> &'static str {
        crate::plugin_runtime::discovery::current_arch_triple().expect("supported arch")
    }

    #[test]
    fn no_plugins_installed_is_not_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let checks = plugin_checks(Some(&dir.path().join("plugins")), "1.0.0");
        assert!(checks.iter().all(|c| c.status == Status::Pass), "{checks:?}");
        // M3(终审):`plugin.state` must not vanish just because the plugins
        // root doesn't exist yet — a JSON consumer diffing two runs needs a
        // stable check-id set regardless of whether anything is installed.
        assert!(checks.iter().any(|c| c.id == "plugin.state"), "{checks:?}");
    }

    /// state.json 是插件系统的唯一真相源;它坏了 = 插件全体不可信,必须 fail。
    /// 同时钉住「不许改用 fail-soft 的 state::load()」—— 那个函数把损坏文件
    /// 当成空表,这条断言会立刻变红。
    #[test]
    fn corrupt_plugin_state_fails() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("state.json"), "{ not json").unwrap();
        let c = plugin_checks(Some(root), "1.0.0")
            .into_iter()
            .find(|c| c.id == "plugin.state")
            .unwrap();
        assert_eq!(c.status, Status::Fail);
    }

    #[test]
    fn a_healthy_plugin_passes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_plugin_state(root, &[("notemd.fixture", true)]);
        install_fixture(root, "notemd.fixture", &fixture_manifest("notemd.fixture", triple()), true);

        let c = plugin_checks(Some(root), "1.0.0")
            .into_iter()
            .find(|c| c.id == "plugin.notemd.fixture")
            .unwrap();
        assert_eq!(c.status, Status::Pass, "{c:?}");
        assert_eq!(group_of(&c.id), "plugin");
    }

    /// 「装了却没反应」的最常见根因:包里没有本机架构的二进制。
    /// detail 必须原样带上 discovery 的原因串,而不是一句笼统的 "invalid"。
    #[test]
    fn a_plugin_without_a_binary_for_this_arch_fails_with_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_plugin_state(root, &[("notemd.fixture", true)]);
        install_fixture(
            root,
            "notemd.fixture",
            &fixture_manifest("notemd.fixture", "wasm32-unknown-unknown"),
            true,
        );

        let c = plugin_checks(Some(root), "1.0.0")
            .into_iter()
            .find(|c| c.id == "plugin.notemd.fixture")
            .unwrap();
        assert_eq!(c.status, Status::Fail);
        assert!(c.detail.contains("no binary for host arch"), "{}", c.detail);
    }

    #[test]
    fn a_disabled_plugin_is_reported_as_skipped_not_broken() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_plugin_state(root, &[("notemd.fixture", false)]);
        install_fixture(root, "notemd.fixture", &fixture_manifest("notemd.fixture", triple()), true);

        let c = plugin_checks(Some(root), "1.0.0")
            .into_iter()
            .find(|c| c.id == "plugin.notemd.fixture")
            .unwrap();
        assert_eq!(c.status, Status::Skip);
        assert!(c.detail.contains("disabled"), "{}", c.detail);
    }

    #[test]
    fn offline_skips_both_network_probes_without_touching_the_network() {
        let checks = net_checks(true);
        assert_eq!(checks.len(), 2);
        assert!(checks.iter().all(|c| c.status == Status::Skip), "{checks:?}");
        assert!(checks.iter().any(|c| c.id == "net.registry"));
        assert!(checks.iter().any(|c| c.id == "net.updater"));
    }

    /// updater 端点必须与 tauri.conf.json 里真正生效的那个是同一个 URL。
    /// 这条测试就是防漂移的锁:改了配置没改常量,它立刻变红。
    #[test]
    fn updater_endpoint_matches_tauri_conf() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let endpoints = conf["plugins"]["updater"]["endpoints"].as_array().unwrap();
        assert_eq!(endpoints[0].as_str().unwrap(), UPDATER_ENDPOINT);
    }

    /// 网络失败是 warn 不是 fail：断网、公司代理、GitHub 抽风都不是「安装损坏」，
    /// 不该让 `notemd doctor && ...` 在飞机上失败(设计文档 §4.5)。
    #[test]
    fn an_unreachable_registry_is_only_a_warning() {
        // 保留端口 0 不可能连通，且不会真的打到任何服务器上。
        let c = probe_registry_at("http://127.0.0.1:0");
        assert_eq!(c.status, Status::Warn, "{c:?}");
        assert_eq!(c.id, "net.registry");
    }

    #[test]
    fn an_unreachable_updater_endpoint_is_only_a_warning() {
        let c = probe_updater_at("http://127.0.0.1:0/latest.json");
        assert_eq!(c.status, Status::Warn, "{c:?}");
        assert_eq!(c.id, "net.updater");
    }
}
