//! `notemd doctor` —— 一条命令自检 notemd 的全部本地能力。
//!
//! 只诊断,不修复:每项检查读状态、给判断、附一条可执行的下一步,绝不改动
//! 任何文件。判断逻辑全部复用各子系统已有的权威实现(`install::status`、
//! `git_ops`、`discovery` 的校验链、`vault_settings` 的权重校验、`searchidx`),
//! 因为 doctor 自带一份判断的话,两份必然漂移 —— 见设计文档 §1。

use serde::Serialize;
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

/// 采集全部检查。本任务先接入环境组,后续任务逐组填充其余分组。
fn collect(_args: &DoctorArgs) -> Vec<Check> {
    env_checks(None)
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
}
