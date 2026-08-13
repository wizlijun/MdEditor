# CLI doctor 自检 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 `notemd` CLI 增加一个纯 Rust 的 `doctor` 子命令,一次性自检环境 / vault / 搜索索引 / 插件系统 / 网络五组能力,只诊断不修复。

**Architecture:** 新模块 `src-tauri/src/cli/doctor.rs`。每项检查是一个**吃显式参数、不摸全局状态**的纯函数,返回统一的 `Check` 记录;`run()` 只负责采集真实环境(读文件、开索引、发网络请求)、把值喂给纯函数、渲染、算退出码。所有「什么算健康」的判断都调用各子系统已有的权威实现,不复刻。

**Tech Stack:** Rust,`serde_json`(输出信封)、`tokio` current-thread runtime + `reqwest`(网络组,均已是现有依赖)、`searchidx`(索引)、`tempfile`(测试)。

**Spec:** `docs/superpowers/specs/2026-08-13-cli-doctor-design.md`

## Global Constraints

- **不引入第二份健康判断。** 复用 `install::status`、`git_ops::version`、`git_ops::validate_proxy_url`、`discovery` 的校验链、`vault_settings::validate_search_weights`、`SearchIndex::open_at` + `stats()`。
- **不走 fail-soft 读取。** `shared_config::read`、`vault_settings::read`、`state::load` 三者都把「缺失」和「损坏」吞成默认值;doctor 必须区分这两态,所以这三处**自己读文件 + `serde_json::from_str`**。枚举已安装插件仍可用 `state::load`。
- **不做全量首建。** 索引 DB 文件不存在时不 open、不 build,记 warn。仅当 DB 文件已存在时才开库并跑 2s 预算增量 sweep。
- **索引 stamp 只能来自 `super::search::scan_options_for(root).source_globs.stamp()`。** 换任何别的算法都会把好索引误判为失效。
- **纯 builtin,不起 webview。** 路由注册必须在 `match_against_manifests` 之前,与 `search` 同款。
- **子进程一律走 `crate::platform::command()`**,不得 `std::process::Command::new`。
- **退出码:** 0 = 无 fail(warn/skip 不影响);1 = 至少一项 fail;2 = 参数错误。
- **流纪律:** 报告整体是结果,走 **stdout**;`--json` 用 `{"ok":…,"data":…}` 标准信封,字段 snake_case。
- 版本号:`env!("CARGO_PKG_VERSION")` 作为 host_version 喂给插件校验。

---

### Task 1: 走通骨架 —— 类型、渲染、退出码、命令注册

先让 `notemd doctor` 端到端跑通(检查列表为空),后续每个任务只往里加一组检查。

**Files:**
- Create: `src-tauri/src/cli/doctor.rs`
- Modify: `src-tauri/src/cli/mod.rs:17`(模块声明)
- Modify: `src-tauri/src/cli/router.rs:41`(`Builtin` 枚举)、`src-tauri/src/cli/router.rs:97`(路由分支)
- Modify: `src-tauri/src/cli/builtin.rs:48`(分发臂)、`builtin.rs:127`(CORE COMMANDS)、`builtin.rs:390`(`render_core_topic` 新 topic)
- Test: `src-tauri/src/cli/doctor.rs` 内 `mod tests`;`src-tauri/tests/cli_builtin_integration.rs` 追加

**Interfaces:**
- Produces:
  - `pub enum Status { Pass, Warn, Fail, Skip }`
  - `pub struct Check { pub id: String, pub status: Status, pub detail: String, pub hint: Option<String> }`
  - `Check::pass(id, detail)` / `Check::warn(id, detail, hint)` / `Check::fail(id, detail, hint)` / `Check::skip(id, detail)`
  - `pub fn group_of(id: &str) -> &str`
  - `pub struct DoctorArgs { pub offline: bool, pub vault: Option<String>, pub json: bool }`,含 `pub fn with_global_json(self, global: bool) -> Self`
  - `pub fn parse_args(rest: &[String], json_global: bool) -> DoctorArgs`
  - `pub fn exit_code_for(checks: &[Check]) -> u8`
  - `pub fn render_plain(checks: &[Check]) -> String`
  - `pub fn render_json(checks: &[Check]) -> String`
  - `pub fn run(args: DoctorArgs) -> ExitCode`
- Consumes: 无(第一个任务)

- [ ] **Step 1: 写失败的测试**

在新建的 `src-tauri/src/cli/doctor.rs` 末尾:

```rust
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
```

在 `src-tauri/src/cli/router.rs` 的 `mod tests` 内追加:

```rust
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
```

在 `src-tauri/tests/cli_builtin_integration.rs` 末尾追加:

```rust
#[test]
fn doctor_offline_json_has_envelope_and_skips_network() {
    let home = temp_home();
    let (code, stdout, _) = run_cli(&["doctor", "--offline", "--json"], &home);
    let _ = std::fs::remove_dir_all(&home);
    // 0 或 1 都是合法结果：本机是否装了 git、是否有 CLI 软链会左右 fail 数。
    // 退出码的精确契约由 doctor.rs 的 exit_code_for 单测钉住，这里只验接线与形状。
    assert!(code == 0 || code == 1, "code={code} stdout={stdout}");
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect(&stdout);
    assert!(v["data"]["checks"].is_array(), "{stdout}");
    let checks = v["data"]["checks"].as_array().unwrap();
    assert!(!checks.is_empty(), "{stdout}");
    // --offline 下网络组必须整组 skip，绝不发请求。
    for ch in checks.iter().filter(|c| c["group"] == "net") {
        assert_eq!(ch["status"], "skip", "{ch}");
    }
}

#[test]
fn doctor_help_topic_documents_its_own_exit_codes() {
    let home = temp_home();
    let (code, stdout, _) = run_cli(&["help", "doctor"], &home);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(code, 0);
    assert!(stdout.contains("EXIT CODES:"), "{stdout}");
    assert!(stdout.contains("--offline"), "{stdout}");
}

#[test]
fn help_lists_doctor_as_a_core_command() {
    let home = temp_home();
    let (code, stdout, _) = run_cli(&["help"], &home);
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(code, 0);
    assert!(stdout.contains("doctor"), "{stdout}");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib cli::doctor 2>&1 | tail -20`
Expected: 编译失败 —— `unresolved module 'doctor'` / `cannot find type Check`。

- [ ] **Step 3: 写最小实现**

`src-tauri/src/cli/doctor.rs` 顶部(测试模块之前)写入:

```rust
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
```

`src-tauri/src/cli/mod.rs` 第 17 行 `pub mod state;` 之前(保持字母序附近)加:

```rust
pub mod doctor;
```

`src-tauri/src/cli/router.rs` 的 `enum Builtin` 内,`Search` 之后加:

```rust
    /// `doctor` — self-check every local capability. Core, never disabled.
    Doctor(super::doctor::DoctorArgs),
```

同文件,`if first == "search" { … }` 那个 `if` 块之后、`if first == "plugin"` 之前加:

```rust
    // Core, never disabled: a broken plugin state is exactly when doctor is
    // needed most, so it must not be routable through plugin matching.
    if first == "doctor" {
        return Route::Builtin(Builtin::Doctor(super::doctor::parse_args(&rest[1..], false)));
    }
```

`Builtin` 加了非 `Debug` 字段会破 `#[derive(Debug)]`,所以 `DoctorArgs` 上必须有 `#[derive(Debug, Clone, Default)]`(上面已给)。

`src-tauri/src/cli/builtin.rs` 的 `run` 内,`Builtin::Search(...)` 那一臂之后加:

```rust
        Builtin::Doctor(args) => super::doctor::run(args.with_global_json(parsed.globals.json)),
```

同文件 `render_help` 的 CORE COMMANDS 段,`search` 那行之后加:

```rust
    out.push_str("  doctor        Self-check every local capability (--offline, --vault, --json)\n");
```

同文件 `render_core_topic` 的 `match topic` 内,`"reading-insights" => …` 之前加:

```rust
        "doctor" => "\
notemd doctor — Self-check notemd's local setup

USAGE:
  notemd doctor [--offline] [--vault <path>] [--json]

DESCRIPTION:
  Runs every local health check and prints a grouped report: environment (CLI
  symlink, git, proxy), configuration and Vault, the search index, installed
  plugins, and — unless --offline — reachability of the plugin registry and the
  update endpoint.

  Diagnose-only. doctor never writes settings, never installs anything, and
  never builds a search index from scratch; each finding carries a one-line
  suggestion for what to run next.

FLAGS:
  --offline         Skip the two network probes (registry, updater)
  --vault <path>    Vault root to check (default: the configured Vault)
  --json            Emit {ok, data: {checks: [...], summary: {...}}}

NOTES:
  Warnings never change the exit code: an uninstalled CLI symlink, a Vault that
  is not a git repository, and an unreachable network are all legitimate states,
  so `notemd doctor && ...` is safe to script.

EXIT CODES:
  0    No failures (warnings and skipped checks are fine)
  1    At least one check failed
  2    Argument error
",
```

- [ ] **Step 4: 跑测试确认通过**

```bash
cd src-tauri && cargo test --lib cli::doctor && cargo test --lib cli::router && cargo test --test cli_builtin_integration doctor
```
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/cli/doctor.rs src-tauri/src/cli/mod.rs src-tauri/src/cli/router.rs \
        src-tauri/src/cli/builtin.rs src-tauri/tests/cli_builtin_integration.rs
git commit -m "feat(cli): doctor 命令骨架 —— 报告类型、渲染、退出码与路由注册"
```

---

### Task 2: 环境组(env)

**Files:**
- Modify: `src-tauri/src/cli/doctor.rs`
- Test: `src-tauri/src/cli/doctor.rs` 内 `mod tests`

**Interfaces:**
- Consumes: Task 1 的 `Check` / `Status` / `collect`
- Produces:
  - `fn check_cli_link(installed: bool, path: Option<&str>, target_exists: Option<bool>) -> Check`
  - `fn check_git(version: Option<&str>) -> Check`
  - `fn check_git_proxy(raw: Option<&str>) -> Check`
  - `fn env_checks() -> Vec<Check>`(采集真实环境后调上面三个纯函数)

- [ ] **Step 1: 写失败的测试**

追加进 `doctor.rs` 的 `mod tests`:

```rust
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib cli::doctor 2>&1 | tail -20`
Expected: FAIL —— `cannot find function 'check_cli_link' in this scope`。

- [ ] **Step 3: 写最小实现**

`doctor.rs` 内 `collect` 之前加:

```rust
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
```

把 `collect` 改成:

```rust
fn collect(_args: &DoctorArgs) -> Vec<Check> {
    env_checks(None)
}
```

(`cfg` 参数在 Task 3 接上真实值。)

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib cli::doctor`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/cli/doctor.rs
git commit -m "feat(cli): doctor 环境组 —— CLI 软链、git 可用性、代理合法性"
```

---

### Task 3: 配置与 vault 组(vault)

**Files:**
- Modify: `src-tauri/src/cli/doctor.rs`
- Modify: `src-tauri/src/sotvault/vault_settings.rs:254`(`validate_search_weights` 提为 `pub(crate)`)
- Test: `src-tauri/src/cli/doctor.rs` 内 `mod tests`

**Interfaces:**
- Consumes: Task 2 的 `env_checks(cfg)`
- Produces:
  - `fn check_shared_config(path: &Path) -> (Check, Option<SharedConfig>)`
  - `fn check_vault_root(explicit: Option<&str>, cfg: Option<&SharedConfig>) -> (Check, Option<PathBuf>)`
  - `fn check_git_repo(root: &Path) -> Check`
  - `fn check_vault_settings(root: &Path) -> Check`
  - `fn vault_checks(args: &DoctorArgs) -> (Vec<Check>, Option<SharedConfig>, Option<PathBuf>)`

- [ ] **Step 1: 写失败的测试**

先把 `vault_settings.rs:254` 的签名从 `fn validate_search_weights` 改成:

```rust
pub(crate) fn validate_search_weights(w: &SearchWeights) -> Result<(), String> {
```

再追加进 `doctor.rs` 的 `mod tests`:

```rust
    use std::path::Path;

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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib cli::doctor 2>&1 | tail -20`
Expected: FAIL —— `cannot find function 'check_shared_config'`。

- [ ] **Step 3: 写最小实现**

`doctor.rs` 顶部 `use` 增加 `use std::path::{Path, PathBuf};`,并在环境组之后加:

```rust
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
```

把 `collect` 改成:

```rust
fn collect(args: &DoctorArgs) -> Vec<Check> {
    let (vault, cfg, _root) = vault_checks(args);
    let mut out = env_checks(cfg.as_ref());
    out.extend(vault);
    out
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib cli::doctor && cargo test --lib vault_settings`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/cli/doctor.rs src-tauri/src/sotvault/vault_settings.rs
git commit -m "feat(cli): doctor 配置与 vault 组 —— 区分缺失与损坏，未配置不连坐"
```

---

### Task 4: 搜索索引组(search)

**Files:**
- Modify: `src-tauri/src/cli/doctor.rs`
- Test: `src-tauri/src/cli/doctor.rs` 内 `mod tests`

**Interfaces:**
- Consumes: Task 3 的 `vault_checks` 返回的 `Option<PathBuf>` vault 根
- Produces: `fn search_checks(root: Option<&Path>) -> Vec<Check>`、`fn search_checks_at(root: &Path, db_path: Option<&Path>) -> Vec<Check>`

- [ ] **Step 1: 写失败的测试**

追加进 `doctor.rs` 的 `mod tests`:

```rust
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib cli::doctor 2>&1 | tail -20`
Expected: FAIL —— `cannot find function 'search_checks'`。

- [ ] **Step 3: 写最小实现**

`doctor.rs` 内加(vault 组之后):

```rust
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
```

把 `collect` 改成:

```rust
fn collect(args: &DoctorArgs) -> Vec<Check> {
    let (vault, cfg, root) = vault_checks(args);
    let mut out = env_checks(cfg.as_ref());
    out.extend(vault);
    out.extend(search_checks(root.as_deref()));
    out
}
```

(`searchidx/src/lib.rs:21` 已有 `pub mod paths;`,`index_db_path` 已是 `pub fn` —— 无需改动 searchidx。)

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib cli::doctor`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/cli/doctor.rs
git commit -m "feat(cli): doctor 搜索索引组 —— 只读探测，不替用户全量首建"
```

---

### Task 5: 插件系统组(plugin)

**Files:**
- Modify: `src-tauri/src/plugin_runtime/discovery.rs:52`(`load_validated` 提为公开的 `validate_installed`)
- Modify: `src-tauri/src/cli/runner.rs:100`(`v2_plugins_root` 提为 `pub(crate)`)
- Modify: `src-tauri/src/cli/doctor.rs`
- Test: `src-tauri/src/cli/doctor.rs` 内 `mod tests`

**Interfaces:**
- Consumes: `crate::plugin_runtime::discovery::validate_installed`、`crate::cli::runner::v2_plugins_root`
- Produces: `fn plugin_checks(root: Option<&Path>, host_version: &str) -> Vec<Check>`

- [ ] **Step 1: 写失败的测试**

先做两处可见性调整。`discovery.rs`:把

```rust
fn load_validated(current: &Path, dir_id: &str, host_version: &str) -> Result<ManifestV2, String> {
```

改为

```rust
/// 校验一个已安装插件的 `current/` 目录:manifest 可读可解析、通过
/// `validate_manifest`、id 与安装目录一致、当前架构有二进制。
///
/// 公开是刻意的:`notemd doctor` 必须调用**这一个**实现来判断插件是否健康。
/// 若它在 doctor 里复刻一遍这串判断,两份必然漂移 —— 那正是这个项目反复踩过的
/// 「第二真相来源」坑。
pub fn validate_installed(current: &Path, dir_id: &str, host_version: &str) -> Result<ManifestV2, String> {
```

并把 `scan_root` 内的 `load_validated(&current, id, host_version)` 改成 `validate_installed(&current, id, host_version)`。

`runner.rs:100`:把 `fn v2_plugins_root()` 改成 `pub(crate) fn v2_plugins_root()`(doctor 直接调它,而不是新增第三份 `dirs::data_dir().join(BUNDLE_ID).join("plugins")` 副本 —— 现有两份已由 `builtin.rs` 的 `plugins_root_matches_runner_derivation` 合同测试钉住,再加一份就要再加一条合同测试)。

追加进 `doctor.rs` 的 `mod tests`:

```rust
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib cli::doctor 2>&1 | tail -20`
Expected: FAIL —— `cannot find function 'plugin_checks'`。

- [ ] **Step 3: 写最小实现**

`doctor.rs` 内加(搜索组之后):

```rust
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
        return vec![Check::pass("plugin.root", "no plugins installed")];
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
```

把 `collect` 改成:

```rust
fn collect(args: &DoctorArgs) -> Vec<Check> {
    let (vault, cfg, root) = vault_checks(args);
    let mut out = env_checks(cfg.as_ref());
    out.extend(vault);
    out.extend(search_checks(root.as_deref()));
    out.extend(plugin_checks(
        super::runner::v2_plugins_root().as_deref(),
        env!("CARGO_PKG_VERSION"),
    ));
    out
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test --lib cli::doctor && cargo test --lib plugin_runtime::discovery`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/cli/doctor.rs src-tauri/src/cli/runner.rs src-tauri/src/plugin_runtime/discovery.rs
git commit -m "feat(cli): doctor 插件组 —— 复用 discovery 校验链，逐插件报原因"
```

---

### Task 6: 网络组(net)与最终校验

**Files:**
- Modify: `src-tauri/src/cli/doctor.rs`
- Test: `src-tauri/src/cli/doctor.rs` 内 `mod tests`

**Interfaces:**
- Consumes: `crate::plugin_runtime::market::{fetch_index, registry_base_url_at}`
- Produces: `fn net_checks(offline: bool) -> Vec<Check>`、`const UPDATER_ENDPOINT: &str`

- [ ] **Step 1: 写失败的测试**

追加进 `doctor.rs` 的 `mod tests`:

```rust
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
    /// 不该让 `notemd doctor && ...` 在飞机上失败（设计文档 §4.5）。
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --lib cli::doctor 2>&1 | tail -20`
Expected: FAIL —— `cannot find function 'net_checks'`。

- [ ] **Step 3: 写最小实现**

`doctor.rs` 内加(插件组之后):

```rust
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
            tokio::join!(registry_probe(&base), updater_probe(UPDATER_ENDPOINT));
        vec![registry, updater]
    })
}

async fn registry_probe(base: &str) -> Check {
    match crate::plugin_runtime::market::fetch_index(base).await {
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

async fn updater_probe(url: &str) -> Check {
    let client = match reqwest::Client::builder().timeout(NET_TIMEOUT).build() {
        Ok(c) => c,
        Err(e) => {
            return Check::warn("net.updater", format!("http client: {e}"), "Retry")
        }
    };
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

/// 同步外壳,让上面两个 async 探针在单测里可直接调用。
#[cfg(test)]
fn probe_registry_at(base: &str) -> Check {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(registry_probe(base))
}

#[cfg(test)]
fn probe_updater_at(url: &str) -> Check {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(updater_probe(url))
}
```

把 `collect` 改成最终形态:

```rust
fn collect(args: &DoctorArgs) -> Vec<Check> {
    let (vault, cfg, root) = vault_checks(args);
    let mut out = env_checks(cfg.as_ref());
    out.extend(vault);
    out.extend(search_checks(root.as_deref()));
    out.extend(plugin_checks(
        super::runner::v2_plugins_root().as_deref(),
        env!("CARGO_PKG_VERSION"),
    ));
    out.extend(net_checks(args.offline));
    out
}
```

`RegistryIndex` 的字段名若不是 `plugins`,按 `src-tauri/src/plugin_runtime/market.rs` 的实际定义改 `index.plugins.len()` 那一处。

- [ ] **Step 4: 跑测试确认通过**

```bash
cd src-tauri && cargo test --lib cli::doctor && cargo test --test cli_builtin_integration doctor
```
Expected: PASS。

- [ ] **Step 5: mutation 验证(必需)**

两条,逐条做完必须改回来:

1. 把 `exit_code_for` 的实现临时改成 `0`(恒不失败),跑 `cargo test --lib cli::doctor`。
   Expected: `any_failure_yields_one` 和 `json_ok_is_false_when_something_failed` **变红**。改回。
2. 把 `check_shared_config` 临时改成走 fail-soft 的 `crate::shared_config::read(path)`,缺失与损坏都返回 `Check::pass`,跑同一条命令。
   Expected: `missing_shared_config_warns_and_corrupt_one_fails` **变红**。改回。

若任一条没变红,说明那条测试没钉住任何东西,先修测试再继续。

- [ ] **Step 6: 全量校验**

```bash
cd src-tauri && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && pnpm check
```
Expected: 全绿。

- [ ] **Step 7: 真实联网手工验证**

```bash
cd src-tauri && cargo run --bin notemd -- --cli doctor
```
(`--cli` 是必需的:`cargo run` 的 argv[0] 落在 `target/` 下,`is_cli_mode` 会判成 GUI。)
确认:五组都打印出来、`net.registry` 与 `net.updater` 在有网时 pass、整体耗时在数秒内、退出码符合报告内容。

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src/cli/doctor.rs
git commit -m "feat(cli): doctor 网络组 —— 市场与 updater 端点并发探测，失败只记 warning"
```

---

### Task 7: 文档

**Files:**
- Modify: `AGENTS.md`(CLI 命令清单段)
- Modify: `README.md` / `README.zh-CN.md`(若其中列了 CLI 命令)
- Modify: `website/public/llms.txt`、`website/public/llms-full.txt`(agent 公共约定)

**Interfaces:**
- Consumes: Task 6 完成的命令与帮助文本
- Produces: 无代码接口

- [ ] **Step 1: 找到所有列了 CLI 命令的地方**

```bash
cd /Users/bruce/git/mdeditor && grep -rln "notemd search" \
  AGENTS.md README.md README.zh-CN.md docs/FEATURES*.md website/public/llms*.txt 2>/dev/null
```

- [ ] **Step 2: 逐个文件补上 doctor**

在每处 `notemd search` 所在的命令清单里,按该文件既有的行格式补一行。统一措辞:

- 中文:`notemd doctor —— 自检本地环境、Vault、搜索索引、插件与网络连通性(--offline 跳过联网,--json 机器可读)`
- English: `notemd doctor — self-check the local setup: environment, Vault, search index, plugins, and network reachability (--offline, --json)`

不要新增章节,只在已有清单里插行 —— 这些文件的结构由别处的规范约束。

- [ ] **Step 3: 校验**

```bash
cd /Users/bruce/git/mdeditor && pnpm check
```
Expected: 通过(纯文档改动不应触发任何检查失败)。

- [ ] **Step 4: 提交**

```bash
git add AGENTS.md README.md README.zh-CN.md website/public/llms.txt website/public/llms-full.txt
git commit -m "docs(cli): doctor 写进 README/AGENTS/llms.txt 命令清单"
```

---

## Self-Review

**Spec coverage:**

| Spec 章节 | 落到哪个任务 |
| --- | --- |
| §3 命令与路由(builtin、不可遮蔽、`--offline`/`--vault`、help) | Task 1 |
| §4.1 环境组三项 | Task 2 |
| §4.2 配置与 vault 四项 + 未配置不连坐 | Task 3 |
| §4.3 搜索索引三项 + 不做全量首建 | Task 4 |
| §4.4 插件系统三类 + 复用 discovery | Task 5 |
| §4.5 网络两项并发 + warn 语义 | Task 6 |
| §5 输出、JSON 信封、退出码 | Task 1(渲染/退出码)、Task 6(集成验证) |
| §6 结构:检查函数与渲染分离、吃显式参数 | 全程(每个 `check_*` 都是纯函数,`*_checks_at` 提供可测核心) |
| §7 测试(单元 1–5、集成 6–8、mutation) | Task 1–6,mutation 在 Task 6 Step 5 |

**规范外的现实约束(读代码后发现,已写进计划):**

1. `discovery::load_validated` 是私有的 → Task 5 提为 `pub fn validate_installed`,`scan_root` 改调它。这是 §4.4「必须调用同一实现」的落地方式。
2. `vault_settings::validate_search_weights` 是私有的 → Task 3 提为 `pub(crate)`。
3. `v2_plugins_root` 在 `runner.rs` 与 `builtin.rs::market` 已有两份副本(由合同测试钉住)→ Task 5 提 `runner` 那份为 `pub(crate)` 给 doctor 用,**不新增第三份**。
4. `install::status` 的 `target_valid` 语义是「是否指向当前进程的二进制」,对 doctor 过严(dev 构建、多安装都会 false)→ Task 2 改为自己判断 target 是否存在,只把真 dangling 记 fail。这也让集成测试在开发机上不会假红。
5. `SearchIndex::open` 会**创建** DB → Task 4 必须先 `db.is_file()` 再决定开不开,否则「不做全量首建」这条会被 open 本身破掉。
6. 集成测试无法假设开发机没装 git/软链 → 退出码的精确契约交给 `exit_code_for` 单测,集成测试只验接线与 JSON 形状。

**Placeholder scan:** 无 TBD / TODO;每个代码步骤都给了可直接粘贴的完整实现与测试。

**Type consistency:** `Check` / `Status` / `DoctorArgs` 的字段与构造函数在 Task 1 定义,后续任务只调用不改签名;`check_*` 一律返回 `Check`,`*_checks` 一律返回 `Vec<Check>`;`vault_checks` 返回三元组 `(Vec<Check>, Option<SharedConfig>, Option<PathBuf>)`,Task 3 定义、Task 4/5/6 的 `collect` 按同一形状消费。
