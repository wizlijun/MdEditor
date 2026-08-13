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

/// 采集全部检查。本任务先返回空表,后续任务逐组填充。
fn collect(_args: &DoctorArgs) -> Vec<Check> {
    Vec::new()
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
}
