# Roam CLI 当日同步 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 `notemd.roam-import` 插件加一条「用 Roam 官方 CLI 把某一天的日记同步进 vault 当日 dailynote」的路径,窗口和 `notemd roam-day` 子命令共用同一份实现。

**Architecture:** 插件从「纯前端」升为「后端 + 前端」(骨架照抄 `plugins-src/ebook-import/`)。新增的 Rust backend 端到端负责:发现 `roam` 可执行 → `roam datalog-query` 取当日页面树 → 转成 outline → 与 vault 里已有 `.note.md` 按块 uid 合并 → 写盘。窗口 UI 与 CLI 都只是这个 backend 的客户端。现有 JSON 导出导入路径(TS)一行不动。

**Tech Stack:** Rust(`notemd-plugin-sdk`、tokio、serde_json、chrono、regex)、Svelte 5 + Vite(插件 UI)、vitest(前端/宿主单测)、cargo test(后端单测)。

设计依据:`docs/superpowers/specs/2026-08-03-roam-cli-daily-sync-design.md`。

## Global Constraints

- 插件 id `notemd.roam-import`;新增 backend crate 名与二进制名均为 `notemd-roam-import`。
- `manifest.v2.json` 用 `#[serde(deny_unknown_fields)]` 解析,**多写一个字段就加载失败**;~~`engines.notemd` 保持 `">=6.716.7"`~~。
  > **事后更正(final review)**:这条 Global Constraint 是在还不知道要改宿主之前写下的,因此定错了。Task 10 发现 `CliRunner` 无条件要求 `payload.file`,`notemd roam-day` 没有文件参数 —— 于是宿主必须一起改(见 §6 的「唯一改动」)。任何**已发布**的宿主(截至 6.801.5)都跑不通这个子命令,所以 `engines.notemd` 必须是 `">=6.803.0"`,且**插件上架前宿主必须先发到该版本或更高**。
- 后端与宿主的通道是 stdin/stdout 上的 NDJSON JSON-RPC;**任何 `println!` 到 stdout 都会污染协议**,调试输出一律走 `host.log_info/warn/error`。
- `$activate` 在协议读循环上同步派发:在 `activate()` 里 `await` `host.request(...)` 会把插件卡死到宿主超时。异步查询必须 spawn(照抄 `ebook-import` 的 `vault_from_host`)。
- vault 文件读写用 `std::fs`(后端已知 vault 绝对路径);`host.vault.*` 只用来问 `host.vault.info`。
- 日记文件路径固定 `<vault>/<daily_dir>/<yyyy>/<yyyy-MM-dd>.note.md`,`daily_dir` 来自 `host.vault.info` 的 `daily_dir`,缺省 `dailynote`。
- 时间戳格式与 TS 的 `new Date(ms).toISOString()` 一致:UTC、毫秒精度、`Z` 结尾(chrono `to_rfc3339_opts(SecondsFormat::Millis, true)`)。
- 所有用户可见文案四语言齐全:`en` / `zh` / `ja` / `de`,并有 `strings.test.ts` 断言键集一致。
- 不做 UI 自动化;GUI 由用户实机验证。
- 共享 worktree:提交时**只精确 `git add` 目标文件**,绝不 `git add -A`。

---

### Task 1: 后端骨架 + `roam` 可执行发现 + 状态探测

**Files:**
- Create: `plugins-src/roam-import/backend/Cargo.toml`
- Create: `plugins-src/roam-import/backend/src/main.rs`
- Create: `plugins-src/roam-import/backend/src/discover.rs`
- Create: `plugins-src/roam-import/backend/src/roam_cli.rs`
- Create: `plugins-src/roam-import/backend/src/plugin.rs`
- Modify: `plugins-src/roam-import/manifest.v2.json`
- Modify: `scripts/dev-install-plugin.sh:68-81`

**Interfaces:**
- Consumes: `notemd_plugin_sdk::{serve, Host, NotemdPlugin}`,`plugin_protocol::{InitializeParams, ActivateParams, ExecuteCommandParams}`。
- Produces:
  - `discover::discover_with(explicit: Option<&str>, home: &Path, shell_lookup: impl Fn() -> Option<PathBuf>, is_exec: impl Fn(&Path) -> bool) -> Option<PathBuf>`
  - `discover::discover(explicit: Option<&str>) -> Option<PathBuf>`
  - `roam_cli::Probe { pub found: Option<String>, pub version: Option<String>, pub graphs: Vec<String>, pub state: ProbeState }`
  - `roam_cli::ProbeState`(enum:`Missing` / `NotConnected` / `Ready`)
  - `roam_cli::parse_version(stdout: &str) -> Option<String>`
  - `roam_cli::graphs_from_list(stdout: &str) -> Result<Vec<String>, String>`
  - `plugin::RoamImportPlugin::new() -> Self`

- [ ] **Step 1: 写 Cargo.toml**

```toml
[package]
name = "notemd-roam-import"
version = "1.1.0"
edition = "2021"

[[bin]]
name = "notemd-roam-import"
path = "src/main.rs"

[dependencies]
notemd-plugin-sdk = { path = "../../../notemd-plugin-sdk" }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "io-util", "sync"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = "0.4"
regex = "1"

[dev-dependencies]
tempfile = "3"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = true
```

- [ ] **Step 2: 写 discover 的失败测试**

`plugins-src/roam-import/backend/src/discover.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn home() -> PathBuf { PathBuf::from("/Users/x") }

    #[test]
    fn explicit_path_wins_when_executable() {
        let got = discover_with(Some("/opt/roam"), &home(), || None, |p| p == Path::new("/opt/roam"));
        assert_eq!(got, Some(PathBuf::from("/opt/roam")));
    }

    #[test]
    fn explicit_path_ignored_when_not_executable() {
        let got = discover_with(
            Some("/opt/roam"), &home(),
            || Some(PathBuf::from("/usr/local/bin/roam")),
            |p| p == Path::new("/usr/local/bin/roam"),
        );
        assert_eq!(got, Some(PathBuf::from("/usr/local/bin/roam")));
    }

    #[test]
    fn falls_back_to_well_known_locations() {
        let got = discover_with(None, &home(), || None, |p| p == Path::new("/opt/homebrew/bin/roam"));
        assert_eq!(got, Some(PathBuf::from("/opt/homebrew/bin/roam")));
    }

    #[test]
    fn returns_none_when_nothing_is_executable() {
        assert_eq!(discover_with(None, &home(), || None, |_| false), None);
    }
}
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml discover`
Expected: 编译失败,`cannot find function discover_with`

- [ ] **Step 4: 实现 discover.rs**

三级发现,与 `plugins-src/claude-agent/backend/src/discover.rs` 同构(GUI 进程 PATH 很瘦,不做登录 shell 查找就找不到 npm 全局装的 `roam`):

```rust
//! Locate the `roam` executable (@roam-research/roam-cli). A GUI-spawned
//! process inherits a lean PATH with none of the user's shell additions, so a
//! plain `Command::new("roam")` fails for most installs. Three tiers:
//! explicit override → login-shell lookup → well-known install locations.
use std::path::{Path, PathBuf};
use std::process::Command;

/// Well-known install locations, in priority order.
pub fn candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/opt/homebrew/bin/roam"),
        PathBuf::from("/usr/local/bin/roam"),
        home.join(".local/bin/roam"),
        home.join(".npm-global/bin/roam"),
        home.join(".volta/bin/roam"),
    ]
}

/// Pure core, injectable for tests.
pub fn discover_with(
    explicit: Option<&str>,
    home: &Path,
    shell_lookup: impl Fn() -> Option<PathBuf>,
    is_exec: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if let Some(p) = explicit.filter(|s| !s.is_empty()) {
        let p = PathBuf::from(p);
        if is_exec(&p) {
            return Some(p);
        }
    }
    if let Some(p) = shell_lookup() {
        if is_exec(&p) {
            return Some(p);
        }
    }
    candidates(home).into_iter().find(|c| is_exec(c))
}

/// Production entry. `-l -i` are both needed: a login shell alone misses rc-file
/// PATH additions (nvm/volta live there).
pub fn discover(explicit: Option<&str>) -> Option<PathBuf> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    discover_with(explicit, &home, shell_lookup, is_executable)
}

fn shell_lookup() -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = Command::new(shell).args(["-l", "-i", "-c", "command -v roam"]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml discover`
Expected: 4 passed

- [ ] **Step 6: 写 probe 解析的失败测试**

`plugins-src/roam-import/backend/src/roam_cli.rs` 末尾。真实输出样本:`roam --version` → `0.9.2\n`;`roam list-graphs` 未连接时输出 `{"error":{"code":"CONFIG_NOT_FOUND","message":"No graphs configured. …"}}`;已连接时输出一个 JSON 数组。

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_version() {
        assert_eq!(parse_version("0.9.2\n"), Some("0.9.2".to_string()));
    }

    #[test]
    fn parses_version_with_prefix() {
        assert_eq!(parse_version("roam-cli 1.0.0\n"), Some("1.0.0".to_string()));
    }

    #[test]
    fn version_of_garbage_is_none() {
        assert_eq!(parse_version("command not found"), None);
    }

    #[test]
    fn config_not_found_is_an_error() {
        let out = r#"{"error":{"code":"CONFIG_NOT_FOUND","message":"No graphs configured."}}"#;
        assert_eq!(graphs_from_list(out), Err("CONFIG_NOT_FOUND".to_string()));
    }

    #[test]
    fn reads_graph_names_from_array() {
        let out = r#"[{"graph":"bruce","nickname":"bruce"},{"graph":"work","nickname":"w"}]"#;
        assert_eq!(graphs_from_list(out), Ok(vec!["bruce".to_string(), "work".to_string()]));
    }

    #[test]
    fn reads_graph_names_from_wrapped_object() {
        let out = r#"{"graphs":[{"graph":"bruce"}]}"#;
        assert_eq!(graphs_from_list(out), Ok(vec!["bruce".to_string()]));
    }
}
```

- [ ] **Step 7: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml roam_cli`
Expected: 编译失败,`cannot find function parse_version`

- [ ] **Step 8: 实现 roam_cli.rs 的探测部分**

```rust
//! Thin wrapper over the `roam` CLI (@roam-research/roam-cli). Every argument
//! is program-constructed — nothing is ever handed to a shell.
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeState {
    /// No `roam` executable anywhere.
    Missing,
    /// Executable found, but `roam connect` has never been run on this machine.
    NotConnected,
    /// Executable found and at least one graph is configured.
    Ready,
}

#[derive(Debug, Clone, Serialize)]
pub struct Probe {
    pub state: ProbeState,
    /// Absolute path of the executable we found, when we found one.
    pub found: Option<String>,
    pub version: Option<String>,
    pub graphs: Vec<String>,
}

/// First semver-looking token in `roam --version` output.
pub fn parse_version(stdout: &str) -> Option<String> {
    regex::Regex::new(r"\d+\.\d+\.\d+")
        .ok()?
        .find(stdout)
        .map(|m| m.as_str().to_string())
}

/// Graph names from `roam list-graphs`. The CLI answers with an error envelope
/// (not a non-zero exit) when no graph is connected, so the JSON must be read.
pub fn graphs_from_list(stdout: &str) -> Result<Vec<String>, String> {
    let v: serde_json::Value =
        serde_json::from_str(stdout).map_err(|e| format!("unreadable list-graphs output: {e}"))?;
    if let Some(code) = v.pointer("/error/code").and_then(|c| c.as_str()) {
        return Err(code.to_string());
    }
    let arr = v
        .as_array()
        .or_else(|| v.get("graphs").and_then(|g| g.as_array()))
        .ok_or_else(|| "list-graphs did not return an array".to_string())?;
    Ok(arr
        .iter()
        .filter_map(|g| {
            g.get("graph")
                .or_else(|| g.get("nickname"))
                .and_then(|s| s.as_str())
                .or_else(|| g.as_str())
        })
        .map(|s| s.to_string())
        .collect())
}

/// Run `roam <args>` with a hard timeout, returning stdout. stderr is folded
/// into the error so an authorization failure is visible to the user.
pub fn run(exe: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(exe)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))?;
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(_) => break,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                return Err(format!("roam timed out after {}s", timeout.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { stdout } else { err });
    }
    Ok(stdout)
}

/// Full status for the UI's three-state banner.
pub fn probe(explicit: Option<&str>) -> Probe {
    let Some(exe) = crate::discover::discover(explicit) else {
        return Probe { state: ProbeState::Missing, found: None, version: None, graphs: vec![] };
    };
    let version = run(&exe, &["--version"], Duration::from_secs(20))
        .ok()
        .and_then(|s| parse_version(&s));
    let graphs = run(&exe, &["list-graphs"], Duration::from_secs(20))
        .ok()
        .and_then(|s| graphs_from_list(&s).ok())
        .unwrap_or_default();
    let state = if graphs.is_empty() { ProbeState::NotConnected } else { ProbeState::Ready };
    Probe { state, found: Some(exe.display().to_string()), version, graphs }
}
```

`Command::new(exe)` 起的是 Node 脚本;`roam` 的 shebang 会自己找 node。若日后发现 npm 全局装的 `roam` 依赖 PATH 里的 `node`,再补 `.env("PATH", …)` —— 现在不预判。

- [ ] **Step 9: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml roam_cli`
Expected: 6 passed

- [ ] **Step 10: 写 main.rs 与最小 plugin.rs**

`main.rs`:

```rust
mod convert; mod dates; mod discover; mod merge; mod outline; mod plugin; mod roam_cli; mod roam_page; mod syntax;

fn main() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2).enable_all().build().expect("tokio runtime");
    rt.block_on(notemd_plugin_sdk::serve(plugin::RoamImportPlugin::new()));
}
```

Task 1 只落地 `discover` / `roam_cli` / `plugin`,其余模块先建空文件(`// filled in by Task N`)让 `mod` 声明成立。

`plugin.rs`(vault 解析照抄 `plugins-src/ebook-import/backend/src/plugin.rs:60-115` 的 `vault_from_host`,**必须 spawn 不能 inline await**):

```rust
use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Inner {
    vault: Option<PathBuf>,
    daily_dir: String,
    vault_checked: bool,
}

pub struct RoamImportPlugin {
    pub data_dir: PathBuf,
    inner: Arc<Mutex<Inner>>,
}

impl RoamImportPlugin {
    pub fn new() -> Self {
        Self { data_dir: std::env::temp_dir(), inner: Arc::new(Mutex::new(Inner::default())) }
    }
}

impl sdk::NotemdPlugin for RoamImportPlugin {
    fn initialize(&mut self, _host: &sdk::Host, params: &proto::InitializeParams) {
        self.data_dir = PathBuf::from(&params.data_dir);
    }

    fn activate(&mut self, host: &sdk::Host, _p: &proto::ActivateParams) -> Result<(), String> {
        let inner = self.inner.clone();
        let host = host.clone();
        tokio::spawn(async move {
            let info = host.request("host.vault.info", json!({})).await.ok();
            let mut g = inner.lock().unwrap();
            g.vault = info.as_ref()
                .and_then(|v| v.get("root")).and_then(|r| r.as_str())
                .filter(|s| !s.is_empty()).map(PathBuf::from);
            g.daily_dir = info.as_ref()
                .and_then(|v| v.get("daily_dir")).and_then(|d| d.as_str())
                .filter(|s| !s.is_empty()).unwrap_or("dailynote").to_string();
            g.vault_checked = true;
        });
        Ok(())
    }

    fn deactivate(&mut self, _host: &sdk::Host) {}

    fn execute_command(&mut self, _host: &sdk::Host, params: &proto::ExecuteCommandParams)
        -> Result<Value, String> {
        Err(format!("unknown command '{}'", params.command))
    }

    fn on_ui_request(&mut self, _host: &sdk::Host, method: &str, params: Value)
        -> Result<Value, String> {
        match method {
            "probe" => {
                let explicit = params.get("roam_path").and_then(|s| s.as_str());
                serde_json::to_value(crate::roam_cli::probe(explicit)).map_err(|e| e.to_string())
            }
            other => Err(format!("unknown ui method '{other}'")),
        }
    }
}
```

- [ ] **Step 11: 编译**

Run: `cargo build --manifest-path plugins-src/roam-import/backend/Cargo.toml`
Expected: 成功

- [ ] **Step 12: 给 manifest 加 binary**

`plugins-src/roam-import/manifest.v2.json`:`version` 改 `"1.1.0"`;在 `"kind"` 之后插入

```json
  "binary": {
    "aarch64-apple-darwin": "bin/notemd-roam-import",
    "x86_64-apple-darwin": "bin/notemd-roam-import"
  },
```

`capabilities` 数组补一个 `"ui"`(UI↔后端 RPC 需要)。

- [ ] **Step 13: 改 dev-install 脚本**

把 `scripts/dev-install-plugin.sh:68-81` 的 `roam-import` 分支整段替换为(照 `ebook-import` 分支的写法,同时装 bin/ 与 ui/):

```bash
elif [[ "$PLUGIN" == "roam-import" ]]; then
  SRC="plugins-src/roam-import"
  cargo build $([ "$PROFILE" = release ] && echo --release) \
    --manifest-path "$SRC/backend/Cargo.toml" --bin notemd-roam-import
  pnpm --filter roam-import-plugin build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.roam-import/$VERSION"
  rm -rf "$DEST"; mkdir -p "$DEST/bin" "$DEST/ui"
  cp "$SRC/backend/target/$PROFILE/notemd-roam-import" "$DEST/bin/"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.roam-import/current"
  mark_installed "notemd.roam-import" "$VERSION"
  echo "✓ installed notemd.roam-import@$VERSION ($PROFILE, $(uname -m), backend + ui) → $DEST"
```

同时把文件头注释里 `roam-import → …(no binary: pure UI)` 那两行改成 backend + ui 的描述。

- [ ] **Step 14: 提交**

```bash
git add plugins-src/roam-import/backend plugins-src/roam-import/manifest.v2.json scripts/dev-install-plugin.sh
git commit -m "feat(roam-import): add a Rust backend that probes the Roam CLI"
```

---

### Task 2: 日期解析 + datalog 查询构造 + 页面树解析

**Files:**
- Create: `plugins-src/roam-import/backend/src/dates.rs`
- Create: `plugins-src/roam-import/backend/src/roam_page.rs`
- Modify: `plugins-src/roam-import/backend/src/roam_cli.rs`(加 `day_query` / `fetch_day`)

**Interfaces:**
- Consumes: Task 1 的 `roam_cli::run`。
- Produces:
  - `dates::resolve_date(input: Option<&str>, today: chrono::NaiveDate) -> Result<String, String>` — 返回 `yyyy-MM-dd`
  - `dates::to_roam_uid(date: &str) -> Option<String>` — `yyyy-MM-dd` → `MM-DD-YYYY`
  - `roam_cli::day_query(uid: &str) -> String`
  - `roam_cli::fetch_day(exe: &Path, graph: Option<&str>, uid: &str) -> Result<serde_json::Value, String>`
  - `roam_page::RoamPage { pub title: String, pub uid: Option<String>, pub create_time: Option<i64>, pub edit_time: Option<i64>, pub children: Vec<RoamBlock> }`
  - `roam_page::RoamBlock { pub uid: Option<String>, pub string: String, pub heading: Option<u8>, pub create_time: Option<i64>, pub edit_time: Option<i64>, pub children: Vec<RoamBlock> }`
  - `roam_page::parse_day_result(v: &serde_json::Value) -> Result<Option<RoamPage>, String>`

- [ ] **Step 1: 写日期解析的失败测试**

`dates.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn today() -> NaiveDate { NaiveDate::from_ymd_opt(2026, 8, 3).unwrap() }

    #[test]
    fn defaults_to_yesterday() {
        assert_eq!(resolve_date(None, today()), Ok("2026-08-02".to_string()));
    }

    #[test]
    fn resolves_relative_words() {
        assert_eq!(resolve_date(Some("today"), today()), Ok("2026-08-03".to_string()));
        assert_eq!(resolve_date(Some("Yesterday"), today()), Ok("2026-08-02".to_string()));
    }

    #[test]
    fn passes_through_iso_dates() {
        assert_eq!(resolve_date(Some("2026-01-09"), today()), Ok("2026-01-09".to_string()));
    }

    #[test]
    fn rejects_garbage_and_impossible_dates() {
        assert!(resolve_date(Some("08/02/2026"), today()).is_err());
        assert!(resolve_date(Some("2026-13-40"), today()).is_err());
    }

    #[test]
    fn converts_iso_to_roam_uid() {
        assert_eq!(to_roam_uid("2026-08-02"), Some("08-02-2026".to_string()));
        assert_eq!(to_roam_uid("2026-8-2"), None);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml dates`
Expected: 编译失败,`cannot find function resolve_date`

- [ ] **Step 3: 实现 dates.rs**

```rust
//! Date handling. Roam's daily-note page uid is `MM-DD-YYYY`; note.md's daily
//! note file is `yyyy-MM-dd`. `today`/`yesterday` are OUR convenience — the
//! Roam read API has no relative-date vocabulary — so they resolve here,
//! against the caller-supplied local calendar date (injectable for tests).
use chrono::{Duration, NaiveDate};

pub fn resolve_date(input: Option<&str>, today: NaiveDate) -> Result<String, String> {
    let raw = input.unwrap_or("yesterday").trim().to_lowercase();
    let d = match raw.as_str() {
        "today" => today,
        "yesterday" => today - Duration::days(1),
        "tomorrow" => today + Duration::days(1),
        other => NaiveDate::parse_from_str(other, "%Y-%m-%d")
            .map_err(|_| format!("invalid --date '{other}': expected yyyy-MM-dd, today or yesterday"))?,
    };
    Ok(d.format("%Y-%m-%d").to_string())
}

/// `yyyy-MM-dd` → Roam's daily-note page uid `MM-DD-YYYY`. Shape-strict: a
/// non-zero-padded input is rejected rather than silently reformatted.
pub fn to_roam_uid(date: &str) -> Option<String> {
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    if d.format("%Y-%m-%d").to_string() != date { return None; }
    Some(d.format("%m-%d-%Y").to_string())
}
```

`chrono` 的 `%Y-%m-%d` 会接受 `2026-8-2`,所以要回写比对一次才能拒掉非补零输入。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml dates`
Expected: 5 passed

- [ ] **Step 5: 写页面树解析的失败测试**

`roam_page.rs`。样本是本机 `roam datalog-query` 真实返回(外层是 `[[page]]`,`order` 未必有序,`:create/time` 与 `:edit/time` 靠 `:as` 别名区分):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_result_means_no_page_that_day() {
        assert_eq!(parse_day_result(&json!([])).unwrap(), None);
    }

    #[test]
    fn reads_title_uid_and_times() {
        let v = json!([[{
            "title": "August 2nd, 2026", "uid": "08-02-2026",
            "create-time": 1785600005019i64, "edit-time": 1785704684051i64
        }]]);
        let p = parse_day_result(&v).unwrap().unwrap();
        assert_eq!(p.title, "August 2nd, 2026");
        assert_eq!(p.uid.as_deref(), Some("08-02-2026"));
        assert_eq!(p.create_time, Some(1785600005019));
        assert!(p.children.is_empty());
    }

    #[test]
    fn sorts_children_by_order_at_every_level() {
        let v = json!([[{
            "title": "August 2nd, 2026", "uid": "08-02-2026",
            "children": [
                { "uid": "b", "string": "second", "order": 1,
                  "children": [ { "uid": "b2", "string": "y", "order": 1 },
                                { "uid": "b1", "string": "x", "order": 0 } ] },
                { "uid": "a", "string": "first", "order": 0 }
            ]
        }]]);
        let p = parse_day_result(&v).unwrap().unwrap();
        assert_eq!(p.children.iter().map(|c| c.string.as_str()).collect::<Vec<_>>(), vec!["first", "second"]);
        assert_eq!(p.children[1].children.iter().map(|c| c.string.as_str()).collect::<Vec<_>>(), vec!["x", "y"]);
    }

    #[test]
    fn missing_string_and_order_default_instead_of_failing() {
        let v = json!([[{ "title": "t", "uid": "u", "children": [ { "uid": "a" } ] }]]);
        let p = parse_day_result(&v).unwrap().unwrap();
        assert_eq!(p.children[0].string, "");
    }

    #[test]
    fn keeps_heading_level() {
        let v = json!([[{ "title": "t", "uid": "u",
                          "children": [ { "uid": "a", "string": "H", "heading": 2 } ] }]]);
        assert_eq!(parse_day_result(&v).unwrap().unwrap().children[0].heading, Some(2));
    }
}
```

- [ ] **Step 6: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml roam_page`
Expected: 编译失败,`cannot find function parse_day_result`

- [ ] **Step 7: 实现 roam_page.rs**

```rust
//! The shape `roam datalog-query` returns for a recursive page pull. It is
//! deliberately the same shape as Roam's JSON export (`RoamPage`/`RoamBlock` in
//! the TS importer) with ONE difference: datalog does not guarantee child
//! order, so `order` must be read and sorted on.
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RoamBlock {
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub string: String,
    #[serde(default)]
    pub order: i64,
    #[serde(default)]
    pub heading: Option<u8>,
    #[serde(default, rename = "create-time")]
    pub create_time: Option<i64>,
    #[serde(default, rename = "edit-time")]
    pub edit_time: Option<i64>,
    #[serde(default)]
    pub children: Vec<RoamBlock>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RoamPage {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default, rename = "create-time")]
    pub create_time: Option<i64>,
    #[serde(default, rename = "edit-time")]
    pub edit_time: Option<i64>,
    #[serde(default)]
    pub children: Vec<RoamBlock>,
}

fn sort_tree(blocks: &mut Vec<RoamBlock>) {
    blocks.sort_by_key(|b| b.order);
    for b in blocks.iter_mut() {
        sort_tree(&mut b.children);
    }
}

/// `[[page]]` → the page, with every level order-sorted. An empty relation
/// means Roam has no daily page for that day (NOT an error).
pub fn parse_day_result(v: &serde_json::Value) -> Result<Option<RoamPage>, String> {
    let Some(first) = v.as_array().and_then(|rows| rows.first()) else { return Ok(None) };
    let obj = match first {
        serde_json::Value::Array(cols) => match cols.first() {
            Some(o) => o,
            None => return Ok(None),
        },
        other => other,
    };
    if obj.is_null() { return Ok(None); }
    let mut page: RoamPage =
        serde_json::from_value(obj.clone()).map_err(|e| format!("unreadable page: {e}"))?;
    sort_tree(&mut page.children);
    Ok(Some(page))
}
```

- [ ] **Step 8: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml roam_page`
Expected: 5 passed

- [ ] **Step 9: 写查询构造的失败测试**

追加到 `roam_cli.rs` 的 `mod tests`:

```rust
    #[test]
    fn day_query_embeds_the_uid_and_aliases_both_timestamps() {
        let q = day_query("08-02-2026");
        assert!(q.contains(r#"[?e :block/uid "08-02-2026"]"#));
        // Without :as both :create/time and :edit/time collapse onto one "time" key.
        assert!(q.contains(r#"[:create/time :as "create-time"]"#));
        assert!(q.contains(r#"[:edit/time :as "edit-time"]"#));
        // Unbounded recursion: a fixed-depth pattern silently truncates deep outlines.
        assert!(q.contains("{:block/children ...}"));
    }
```

- [ ] **Step 10: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml day_query`
Expected: 编译失败,`cannot find function day_query`

- [ ] **Step 11: 实现 day_query 与 fetch_day**

追加到 `roam_cli.rs`:

```rust
/// The recursive pull that returns a whole daily page in Roam-export shape.
pub fn day_query(uid: &str) -> String {
    format!(
        r#"[:find (pull ?e [:node/title :block/uid :block/string :block/order :block/heading [:create/time :as "create-time"] [:edit/time :as "edit-time"] {{:block/children ...}}]) :where [?e :block/uid "{uid}"]]"#
    )
}

/// Fetch one daily page. `graph` is optional — the CLI auto-selects when only
/// one graph is configured.
pub fn fetch_day(exe: &Path, graph: Option<&str>, uid: &str) -> Result<serde_json::Value, String> {
    let query = day_query(uid);
    let mut args: Vec<&str> = vec!["datalog-query", "--query", &query];
    if let Some(g) = graph.filter(|g| !g.is_empty()) {
        args.push("--graph");
        args.push(g);
    }
    let out = run(exe, &args, Duration::from_secs(60))?;
    serde_json::from_str(&out).map_err(|e| format!("unreadable datalog output: {e}"))
}
```

- [ ] **Step 12: 跑全部后端测试**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml`
Expected: 全部 passed

- [ ] **Step 13: 提交**

```bash
git add plugins-src/roam-import/backend/src
git commit -m "feat(roam-import): fetch and parse one Roam daily page over datalog"
```

---

### Task 3: Roam 行内语法转换(Rust 移植)

**Files:**
- Create: `plugins-src/roam-import/backend/src/syntax.rs`

**Interfaces:**
- Consumes: 无。
- Produces:
  - `syntax::convert_inline(s: &str) -> String`
  - `syntax::to_iso_date(target: &str) -> Option<String>`
  - `syntax::normalize_date_links(s: &str) -> String`
  - `syntax::escape_reserved_props(s: &str) -> String`

移植源:`plugins-src/roam-import/src/lib/roam-import/syntax.ts`。行为必须逐条对齐(`rewriteLinks` 不移植 —— 全图改名只发生在整图导入路径)。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_and_done_become_checkboxes() {
        assert_eq!(convert_inline("{{[[TODO]]}} buy milk"), "[ ] buy milk");
        assert_eq!(convert_inline("{{DONE}} shipped"), "[x] shipped");
    }

    #[test]
    fn embeds_collapse_to_block_refs() {
        assert_eq!(convert_inline("{{[[embed]]: ((abc123))}}"), "((abc123))");
        assert_eq!(convert_inline("{{embed: ((abc123))}}"), "((abc123))");
    }

    #[test]
    fn roam_underscore_italics_become_asterisks() {
        assert_eq!(convert_inline("__slanted__"), "*slanted*");
    }

    #[test]
    fn hashtag_page_links_become_plain_wikilinks() {
        assert_eq!(convert_inline("#[[Hemory]] note"), "[[Hemory]] note");
    }

    #[test]
    fn code_spans_are_left_alone() {
        assert_eq!(convert_inline("`__x__` and __y__"), "`__x__` and *y*");
        assert_eq!(convert_inline("```\n{{TODO}}\n```"), "```\n{{TODO}}\n```");
    }

    #[test]
    fn english_daily_titles_become_iso() {
        assert_eq!(to_iso_date("August 15th, 2022").as_deref(), Some("2022-08-15"));
        assert_eq!(to_iso_date("March 1st, 2026").as_deref(), Some("2026-03-01"));
        assert_eq!(to_iso_date("Hemory"), None);
    }

    #[test]
    fn date_links_are_normalized_but_other_links_are_not() {
        assert_eq!(
            normalize_date_links("see [[August 15th, 2022]] and [[Hemory]]"),
            "see [[2022-08-15]] and [[Hemory]]"
        );
    }

    #[test]
    fn continuation_lines_that_look_like_props_get_escaped() {
        // A second line reading `id:: x` would be eaten as a node property by
        // parse_outline; one leading space makes it content again.
        assert_eq!(escape_reserved_props("head\nid:: x"), "head\n id:: x");
        assert_eq!(escape_reserved_props("id:: x"), "id:: x");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml syntax`
Expected: 编译失败,`cannot find function convert_inline`

- [ ] **Step 3: 实现 syntax.rs**

```rust
//! Roam inline syntax → note.md markdown. Ported from
//! `plugins-src/roam-import/src/lib/roam-import/syntax.ts`; keep the two in
//! step — the shared golden fixture (Task 7) is what catches drift.
use regex::{Captures, Regex};
use std::sync::OnceLock;

/// Split on code (``` fences and `inline` spans): even indices are prose,
/// odd indices are code and must pass through untouched.
fn map_non_code(s: &str, f: impl Fn(&str) -> String) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?s)(```.*?```|`[^`\n]*`)").unwrap());
    let mut out = String::new();
    let mut last = 0;
    for m in re.find_iter(s) {
        out.push_str(&f(&s[last..m.start()]));
        out.push_str(m.as_str());
        last = m.end();
    }
    out.push_str(&f(&s[last..]));
    out
}

fn re(p: &str) -> Regex { Regex::new(p).unwrap() }

pub fn convert_inline(s: &str) -> String {
    map_non_code(s, |seg| {
        let seg = re(r"\{\{\[\[embed\]\]:\s*\(\(([a-zA-Z0-9_-]+)\)\)\s*\}\}").replace_all(seg, "(($1))");
        let seg = re(r"\{\{embed:\s*\(\(([a-zA-Z0-9_-]+)\)\)\s*\}\}").replace_all(&seg, "(($1))");
        let seg = seg.replace("{{[[TODO]]}}", "[ ]").replace("{{[[DONE]]}}", "[x]")
                     .replace("{{TODO}}", "[ ]").replace("{{DONE}}", "[x]");
        let seg = re(r"__([^_\n](?:[^\n]*?[^_\n])?)__").replace_all(&seg, "*$1*");
        re(r"#\[\[([^\]\n]+)\]\]").replace_all(&seg, "[[$1]]").into_owned()
    })
}

const MONTHS: [(&str, &str); 12] = [
    ("january", "01"), ("february", "02"), ("march", "03"), ("april", "04"),
    ("may", "05"), ("june", "06"), ("july", "07"), ("august", "08"),
    ("september", "09"), ("october", "10"), ("november", "11"), ("december", "12"),
];

/// Roam's daily title ("August 15th, 2022") → "2022-08-15"; anything else None.
pub fn to_iso_date(target: &str) -> Option<String> {
    let caps = re(r"^([A-Za-z]+) (\d{1,2})(?:st|nd|rd|th), (\d{4})$").captures(target)?;
    let mo = MONTHS.iter().find(|(n, _)| *n == caps[1].to_lowercase())?.1;
    let dd: u32 = caps[2].parse().ok()?;
    if !(1..=31).contains(&dd) { return None; }
    Some(format!("{}-{}-{:02}", &caps[3], mo, dd))
}

/// note.md only resolves ISO date links (`[[yyyy-MM-dd]]`), so English daily
/// titles must be rewritten or the link points at nothing.
pub fn normalize_date_links(s: &str) -> String {
    map_non_code(s, |seg| {
        re(r"\[\[([^\]\n]+)\]\]")
            .replace_all(seg, |c: &Captures| match to_iso_date(&c[1]) {
                Some(iso) => format!("[[{iso}]]"),
                None => c[0].to_string(),
            })
            .into_owned()
    })
}

/// A continuation line shaped like a node property would be swallowed by
/// parse_outline. One leading space keeps it content (renders the same).
pub fn escape_reserved_props(s: &str) -> String {
    let prop = re(r"^(type|line|id|collapsed|created|updated|status|answered|by):: ");
    s.split('\n')
        .enumerate()
        .map(|(i, ln)| if i > 0 && prop.is_match(ln) { format!(" {ln}") } else { ln.to_string() })
        .collect::<Vec<_>>()
        .join("\n")
}
```

保留字集合比 TS 版多了 `status|answered|by` —— 宿主 `parseOutline` 认这三个键(`src/lib/outline/markdown.ts:4`),TS 版漏了。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml syntax`
Expected: 8 passed

- [ ] **Step 5: 提交**

```bash
git add plugins-src/roam-import/backend/src/syntax.rs
git commit -m "feat(roam-import): port the Roam inline-syntax conversion to Rust"
```

---

### Task 4: outline 模型 / 解析 / 序列化 / front-matter

**Files:**
- Create: `plugins-src/roam-import/backend/src/outline.rs`

**Interfaces:**
- Consumes: 无。
- Produces:
  - `outline::Node { pub id: String, pub parent: Option<String>, pub order: i64, pub content: String, pub collapsed: bool, pub source: String, pub anchor_line: Option<i64>, pub status: Option<String>, pub answered_at: Option<String>, pub answered_by: Option<String>, pub created_at: Option<String>, pub updated_at: Option<String>, pub persist_id: bool }`
  - `outline::Tree { pub frontmatter: Option<String>, pub nodes: Vec<Node> }`
  - `Tree::children_of(&self, parent: Option<&str>) -> Vec<&Node>`(按 order 升序)
  - `outline::parse_outline(text: &str) -> Tree`
  - `outline::serialize_outline(tree: &Tree) -> String`
  - `outline::touch_frontmatter(raw: Option<&str>, title: &str, created: &str, now: &str) -> String`

**移植源与必须对齐的行为**(`src/lib/outline/markdown.ts`):属性行白名单 `type|line|id|collapsed|created|updated|status|answered|by`;缩进两空格一级;属性行缩进 = 节点缩进 + 2;序列化顺序 `type → line → status → created → updated → answered → by → id → collapsed`;` ``` ` 围栏进入 raw 模式(逐行原样收,不识别 bullet/属性),闭合围栏长度 ≥ 开启长度才退出;无法归类的行降级成根层节点(不丢内容)。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_plain_outline() {
        let text = "---\ntitle: 2026-08-02\n---\n- a\n  - b\n- c\n";
        assert_eq!(serialize_outline(&parse_outline(text)), text);
    }

    #[test]
    fn round_trips_properties_in_canonical_order() {
        let text = "- hello\n  created:: 2026-08-02T00:00:00.000Z\n  updated:: 2026-08-02T01:00:00.000Z\n  id:: abc\n  collapsed:: true\n";
        assert_eq!(serialize_outline(&parse_outline(text)), text);
    }

    #[test]
    fn keeps_id_and_marks_it_persisted() {
        let t = parse_outline("- hello\n  id:: abc\n");
        assert_eq!(t.nodes[0].id, "abc");
        assert!(t.nodes[0].persist_id);
    }

    #[test]
    fn a_node_without_id_is_not_persisted() {
        let t = parse_outline("- hello\n");
        assert!(!t.nodes[0].persist_id);
        assert_eq!(serialize_outline(&t), "- hello\n");
    }

    #[test]
    fn multi_line_content_survives() {
        let text = "- first\n  second\n";
        let t = parse_outline(text);
        assert_eq!(t.nodes[0].content, "first\nsecond");
        assert_eq!(serialize_outline(&t), text);
    }

    #[test]
    fn answer_fences_are_taken_raw() {
        let text = "- ```\n  type:: answer\n  still inside the fence\n  ```\n";
        let t = parse_outline(text);
        assert_eq!(t.nodes.len(), 1);
        assert!(t.nodes[0].content.contains("type:: answer"));
        assert_eq!(t.nodes[0].source, "manual");
    }

    #[test]
    fn typed_nodes_round_trip() {
        let text = "- ask me\n  type:: question\n  status:: open\n";
        assert_eq!(serialize_outline(&parse_outline(text)), text);
    }

    #[test]
    fn children_are_ordered() {
        let t = parse_outline("- a\n- b\n- c\n");
        let kids: Vec<&str> = t.children_of(None).iter().map(|n| n.content.as_str()).collect();
        assert_eq!(kids, vec!["a", "b", "c"]);
    }

    #[test]
    fn frontmatter_touch_fills_and_refreshes() {
        let fm = touch_frontmatter(None, "2026-08-02", "2026-08-02T00:00:00.000Z", "2026-08-03T09:00:00.000Z");
        assert!(fm.contains("title: 2026-08-02"));
        assert!(fm.contains("created: 2026-08-02T00:00:00.000Z"));
        assert!(fm.contains("updated: 2026-08-03T09:00:00.000Z"));
    }

    #[test]
    fn frontmatter_touch_keeps_unknown_keys_and_only_moves_updated() {
        let raw = "title: 2026-08-02\nroam-uid: 08-02-2026\nupdated: 2026-01-01T00:00:00.000Z";
        let fm = touch_frontmatter(Some(raw), "2026-08-02", "x", "2026-08-03T09:00:00.000Z");
        assert!(fm.contains("roam-uid: 08-02-2026"));
        assert!(fm.contains("updated: 2026-08-03T09:00:00.000Z"));
        assert!(!fm.contains("2026-01-01"));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml outline`
Expected: 编译失败,`cannot find function parse_outline`

- [ ] **Step 3: 实现 outline.rs**

按 `src/lib/outline/markdown.ts` 的 `parseOutline`/`serializeOutline` 逐条移植。要点:

- 节点 id:文件里有 `id::` 就用它并置 `persist_id = true`;没有则生成一个进程内唯一的占位 id(`format!("local-{n}")`,`n` 为解析计数器),**不写回文件**。合并只按位置处理这些节点,占位 id 不出现在输出里。
- `order`:每层 `+100` 递增(与 TS 的 `nextOrder` 一致)。
- 围栏 raw 模式:节点首行以 `` ``` `` 开头时进入,续行剥掉 `节点缩进 + 2` 个空格后原样追加,遇到长度 ≥ 开启长度的闭合行退出。
- `serialize_outline` 里空续行写成空串而不是缩进空白(`markdown.ts:39`)。
- `touch_frontmatter` 行级实现,**不引 YAML 库**:按行扫,`title:`/`created:` 缺失才补(追加在末尾),`updated:` 存在就替换该行、不存在就追加。保留其它所有行与顺序。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml outline`
Expected: 10 passed

- [ ] **Step 5: 提交**

```bash
git add plugins-src/roam-import/backend/src/outline.rs
git commit -m "feat(roam-import): port .note.md outline parse/serialize to Rust"
```

---

### Task 5: RoamPage → outline Tree

**Files:**
- Create: `plugins-src/roam-import/backend/src/convert.rs`

**Interfaces:**
- Consumes: `roam_page::{RoamPage, RoamBlock}`、`syntax::*`、`outline::{Node, Tree}`。
- Produces: `convert::convert_page(page: &RoamPage, date: &str) -> outline::Tree`

规则(对齐 `plugins-src/roam-import/src/lib/roam-import/convert.ts`,**除了 id 落盘**):

- 节点 `id` = block uid;uid 缺失时用 `format!("roam-{i}")` 占位。
- **`persist_id` 恒为 `true`** —— 这是与整图导入路径的关键差异:不写 `id::` 就没法下次按 uid 对位,合并会退化成整文件覆盖。
- 内容 = `escape_reserved_props(normalize_date_links(convert_inline(string)))`,`heading` 1..=3 时前置对应个数的 `#`。
- `created_at` / `updated_at` = `create-time` / `edit-time` 的 ISO 8601(UTC、毫秒、Z)。
- front-matter `title` 恒为 `date`(`yyyy-MM-dd`),不用 Roam 的英文标题。
- 页面无子块时产出空 `nodes`(合并阶段自己处理,不在这里塞占位空节点)。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::roam_page::{RoamBlock, RoamPage};

    fn block(uid: &str, s: &str) -> RoamBlock {
        RoamBlock { uid: Some(uid.into()), string: s.into(), order: 0, heading: None,
                    create_time: None, edit_time: None, children: vec![] }
    }

    fn page(children: Vec<RoamBlock>) -> RoamPage {
        RoamPage { title: "August 2nd, 2026".into(), uid: Some("08-02-2026".into()),
                   create_time: Some(1785600005019), edit_time: None, children }
    }

    #[test]
    fn node_id_is_the_roam_uid_and_is_always_persisted() {
        let t = convert_page(&page(vec![block("hCIv7Y63h", "hi")]), "2026-08-02");
        assert_eq!(t.nodes[0].id, "hCIv7Y63h");
        assert!(t.nodes[0].persist_id, "every Roam block must carry id:: or the next merge cannot align");
    }

    #[test]
    fn inline_syntax_is_converted() {
        let t = convert_page(&page(vec![block("a", "{{[[TODO]]}} __x__")]), "2026-08-02");
        assert_eq!(t.nodes[0].content, "[ ] *x*");
    }

    #[test]
    fn heading_level_becomes_hashes() {
        let mut b = block("a", "Title");
        b.heading = Some(2);
        let t = convert_page(&page(vec![b]), "2026-08-02");
        assert_eq!(t.nodes[0].content, "## Title");
    }

    #[test]
    fn timestamps_match_the_ts_iso_format() {
        let mut b = block("a", "x");
        b.create_time = Some(1785600005019);
        let t = convert_page(&page(vec![b]), "2026-08-02");
        assert_eq!(t.nodes[0].created_at.as_deref(), Some("2026-08-02T04:00:05.019Z"));
    }

    #[test]
    fn frontmatter_title_is_the_iso_date_not_the_roam_title() {
        let t = convert_page(&page(vec![]), "2026-08-02");
        assert!(t.frontmatter.as_ref().unwrap().contains("title: 2026-08-02"));
        assert!(!t.frontmatter.as_ref().unwrap().contains("August"));
    }

    #[test]
    fn children_become_nested_nodes() {
        let mut parent = block("p", "parent");
        parent.children = vec![block("c", "child")];
        let t = convert_page(&page(vec![parent]), "2026-08-02");
        let child = t.nodes.iter().find(|n| n.id == "c").unwrap();
        assert_eq!(child.parent.as_deref(), Some("p"));
    }
}
```

`1785600005019` 对应的 UTC 时刻请在实现后用一次 `cargo test` 的实际输出核对,若与断言不符,**改断言不改实现**(实现只负责调用 chrono)。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml convert`
Expected: 编译失败,`cannot find function convert_page`

- [ ] **Step 3: 实现 convert.rs**

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml convert`
Expected: 6 passed

- [ ] **Step 5: 提交**

```bash
git add plugins-src/roam-import/backend/src/convert.rs
git commit -m "feat(roam-import): convert a Roam daily page into an outline tree"
```

---

### Task 6: 按块 uid 合并

**Files:**
- Create: `plugins-src/roam-import/backend/src/merge.rs`

**Interfaces:**
- Consumes: `outline::{Node, Tree}`。
- Produces:
  - `merge::MergeStats { pub created: usize, pub updated: usize, pub kept_local: usize, pub roam_gone_kept: usize }`
  - `merge::merge(local: &outline::Tree, roam: &outline::Tree) -> (outline::Tree, MergeStats)`

**算法(逐条实现,顺序即优先级):**

1. `roam_uids` = roam 树全部节点 id 的集合;`local_by_id` = local 树按 id 索引。
2. 从根层开始递归。某一层的输出列表:
   a. 先放该层全部 roam 子节点,按 roam 的 order。
   b. 再按 local 原顺序处理该层「本地块」(id ∉ `roam_uids`),两分支:向前找最近的、**已经落在输出列表里**的同级前驱(可能是存活的 roam 块,也可能是刚插入的另一个本地块)→ 插到它后面;否则一律插到列表**头部**。连续的一串本地块因此保持原相对顺序(第一块落头部,其余锚在前一块之后)。(产品负责人裁定:无锚点时落头部,不落末尾 —— 你写的块不该因为 roam 往同一父节点新增内容就被挤到下面;测试 `local_children_of_a_roam_block_survive` 按此断言。)
3. 同 uid 节点:`content`/`created_at`/`updated_at` 取 roam 版;`collapsed` 取 local 版(折叠是本地视图状态,不该被 Roam 覆盖);`persist_id` = true。
4. 递归时 local 侧的父节点按 **id 全局查找**(块可能在 Roam 里被移到了别的父下)。
5. 保留的本地子树整棵复制,但**丢弃其中 id ∈ `roam_uids` 的节点及其整棵子树** —— 那个块已经在 roam 结构里输出过了,它自己的本地子节点会在递归它时被捡回,不会丢也不会重。
6. 输出层内 `order` 一律重编为 `0, 100, 200, …`。
7. 统计:roam 节点在 local 无对应 → `created`;有对应且内容不同 → `updated`;本地块 `persist_id == true` → `roam_gone_kept`(带 `id::` 的只可能是以前从 Roam 同步来的),否则 → `kept_local`。
8. front-matter 由调用方(Task 7)处理,`merge` 直接把 `local.frontmatter` 原样带出。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::{parse_outline, serialize_outline};

    /// Roam-side trees always carry id::, so they can be written as text.
    fn roam(text: &str) -> crate::outline::Tree { parse_outline(text) }

    #[test]
    fn empty_local_takes_the_roam_tree_whole() {
        let r = roam("- a\n  id:: u1\n- b\n  id:: u2\n");
        let (out, st) = merge(&parse_outline(""), &r);
        assert_eq!(serialize_outline(&out), "- a\n  id:: u1\n- b\n  id:: u2\n");
        assert_eq!(st.created, 2);
    }

    #[test]
    fn same_uid_takes_the_roam_content() {
        let local = parse_outline("- old text\n  id:: u1\n");
        let (out, st) = merge(&local, &roam("- new text\n  id:: u1\n"));
        assert_eq!(serialize_outline(&out), "- new text\n  id:: u1\n");
        assert_eq!(st.updated, 1);
        assert_eq!(st.created, 0);
    }

    #[test]
    fn a_local_block_keeps_its_place_between_roam_blocks() {
        let local = parse_outline("- a\n  id:: u1\n- mine\n- b\n  id:: u2\n");
        let (out, st) = merge(&local, &roam("- a\n  id:: u1\n- b\n  id:: u2\n"));
        assert_eq!(serialize_outline(&out), "- a\n  id:: u1\n- mine\n- b\n  id:: u2\n");
        assert_eq!(st.kept_local, 1);
    }

    #[test]
    fn a_local_block_before_every_roam_block_stays_at_the_top() {
        let local = parse_outline("- mine\n- a\n  id:: u1\n");
        let (out, _) = merge(&local, &roam("- a\n  id:: u1\n"));
        assert_eq!(serialize_outline(&out), "- mine\n- a\n  id:: u1\n");
    }

    #[test]
    fn a_block_deleted_in_roam_is_kept_locally() {
        let local = parse_outline("- a\n  id:: u1\n- gone\n  id:: u9\n");
        let (out, st) = merge(&local, &roam("- a\n  id:: u1\n"));
        assert_eq!(serialize_outline(&out), "- a\n  id:: u1\n- gone\n  id:: u9\n");
        assert_eq!(st.roam_gone_kept, 1);
        assert_eq!(st.kept_local, 0);
    }

    #[test]
    fn local_children_of_a_roam_block_survive() {
        let local = parse_outline("- a\n  id:: u1\n  - my note\n");
        let (out, _) = merge(&local, &roam("- a\n  id:: u1\n  - from roam\n    id:: u2\n"));
        assert_eq!(serialize_outline(&out), "- a\n  id:: u1\n  - my note\n  - from roam\n    id:: u2\n");
    }

    #[test]
    fn a_block_moved_to_another_parent_in_roam_takes_its_local_children_along() {
        let local = parse_outline("- p1\n  id:: u1\n  - moved\n    id:: u9\n    - my note\n- p2\n  id:: u2\n");
        let r = roam("- p1\n  id:: u1\n- p2\n  id:: u2\n  - moved\n    id:: u9\n");
        let (out, _) = merge(&local, &r);
        let text = serialize_outline(&out);
        assert_eq!(text, "- p1\n  id:: u1\n- p2\n  id:: u2\n  - moved\n    id:: u9\n    - my note\n");
        assert_eq!(text.matches("id:: u9").count(), 1, "the moved block must not be duplicated");
    }

    #[test]
    fn collapsed_is_a_local_view_state_and_survives_a_sync() {
        let local = parse_outline("- a\n  id:: u1\n  collapsed:: true\n");
        let (out, _) = merge(&local, &roam("- a\n  id:: u1\n"));
        assert!(serialize_outline(&out).contains("collapsed:: true"));
    }

    #[test]
    fn merging_twice_changes_nothing() {
        let local = parse_outline("- a\n  id:: u1\n- mine\n- gone\n  id:: u9\n");
        let r = roam("- a\n  id:: u1\n- b\n  id:: u2\n");
        let (once, _) = merge(&local, &r);
        let (twice, st) = merge(&once, &r);
        assert_eq!(serialize_outline(&once), serialize_outline(&twice));
        assert_eq!(st.created, 0);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml merge`
Expected: 编译失败,`cannot find function merge`

- [ ] **Step 3: 实现 merge.rs**

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml merge`
Expected: 9 passed

- [ ] **Step 5: 提交**

```bash
git add plugins-src/roam-import/backend/src/merge.rs
git commit -m "feat(roam-import): merge a Roam day into an existing note by block uid"
```

---

### Task 7: `sync_day` 编排 + golden fixture

**Files:**
- Modify: `plugins-src/roam-import/backend/src/plugin.rs`
- Create: `plugins-src/roam-import/backend/src/sync.rs`
- Create: `plugins-src/roam-import/backend/tests/fixtures/roam-day.json`
- Create: `plugins-src/roam-import/backend/tests/fixtures/local-before.note.md`
- Create: `plugins-src/roam-import/backend/tests/fixtures/daily.note.md`
- Create: `plugins-src/roam-import/backend/tests/golden.rs`
- Create: `src/lib/outline/roam-golden.test.ts`

**Interfaces:**
- Consumes: Task 2–6 全部。
- Produces:
  - `sync::SyncOutcome { pub date: String, pub path: String, pub created: usize, pub updated: usize, pub kept_local: usize, pub roam_gone_kept: usize, pub found: bool }`
  - `sync::daily_rel_path(daily_dir: &str, date: &str) -> String` — `<daily_dir>/<yyyy>/<date>.note.md`
  - `sync::sync_day(vault: &Path, daily_dir: &str, page: Option<&RoamPage>, date: &str, now: &str) -> Result<SyncOutcome, String>` — 纯 IO 编排,Roam 取数已在外层完成,便于用 tempfile 测
  - `plugin` 的 ui 方法 `sync_day`,参数 `{ date?, graph?, roam_path? }`

- [ ] **Step 1: 写 sync 的失败测试**

`sync.rs` 末尾,用 `tempfile::tempdir()`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::roam_page::{RoamBlock, RoamPage};

    const NOW: &str = "2026-08-03T09:00:00.000Z";

    fn page() -> RoamPage {
        RoamPage {
            title: "August 2nd, 2026".into(), uid: Some("08-02-2026".into()),
            create_time: Some(1785600005019), edit_time: None,
            children: vec![RoamBlock {
                uid: Some("u1".into()), string: "from roam".into(), order: 0, heading: None,
                create_time: None, edit_time: None, children: vec![],
            }],
        }
    }

    #[test]
    fn path_is_daily_dir_year_date() {
        assert_eq!(daily_rel_path("dailynote", "2026-08-02"), "dailynote/2026/2026-08-02.note.md");
    }

    #[test]
    fn writes_a_new_note_when_none_exists() {
        let dir = tempfile::tempdir().unwrap();
        let out = sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap();
        assert!(out.found);
        assert_eq!(out.created, 1);
        let text = std::fs::read_to_string(dir.path().join(&out.path)).unwrap();
        assert!(text.contains("- from roam"));
        assert!(text.contains("id:: u1"));
        assert!(text.contains("title: 2026-08-02"));
    }

    #[test]
    fn no_roam_page_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let out = sync_day(dir.path(), "dailynote", None, "2026-08-02", NOW).unwrap();
        assert!(!out.found);
        assert!(!dir.path().join(&out.path).exists());
    }

    #[test]
    fn a_second_sync_leaves_the_file_byte_identical() {
        let dir = tempfile::tempdir().unwrap();
        sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap();
        let first = std::fs::read_to_string(dir.path().join("dailynote/2026/2026-08-02.note.md")).unwrap();
        sync_day(dir.path(), "dailynote", Some(&page()), "2026-08-02", NOW).unwrap();
        let second = std::fs::read_to_string(dir.path().join("dailynote/2026/2026-08-02.note.md")).unwrap();
        assert_eq!(first, second);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml sync`
Expected: 编译失败,`cannot find function sync_day`

- [ ] **Step 3: 实现 sync.rs**

流程:`convert_page` → 读现有文件(不存在则空树)→ `parse_outline` → `merge` → `touch_frontmatter(local_fm, date, page.create_time 的 ISO 或 now, now)` → `serialize_outline` → `create_dir_all` + 写盘。`page` 为 `None` 时直接返回 `found: false`,不碰文件系统。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml sync`
Expected: 4 passed

- [ ] **Step 5: 造 golden fixture**

`tests/fixtures/roam-day.json` = 一次真实 `roam datalog-query` 输出的裁剪版(含:嵌套三层、一个 heading 块、一个含 `{{[[TODO]]}}` 与 `__斜体__` 的块、一个含 `[[August 15th, 2022]]` 的块)。
`tests/fixtures/local-before.note.md` = 已有当日笔记(含一个无 `id::` 的本地块、一个 `id:: gone-1` 的旧 Roam 块)。
`tests/fixtures/daily.note.md` = 合并后的期望输出。

先随便写一个占位期望文件,跑一次测试拿到实际输出,人工逐行核对合理后再写回 fixture(**核对内容,不要盲抄输出**)。

- [ ] **Step 6: 写 golden 测试**

`tests/golden.rs`:

```rust
//! Format-drift guard. The SAME fixture is asserted from the Rust side (this
//! file) and the TypeScript side (src/lib/outline/roam-golden.test.ts). If
//! either side's .note.md format moves, one of them goes red.
#[test]
fn merged_output_matches_the_golden_fixture() {
    let raw: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/roam-day.json")).unwrap();
    let page = notemd_roam_import::roam_page::parse_day_result(&raw).unwrap().unwrap();
    let local = notemd_roam_import::outline::parse_outline(include_str!("fixtures/local-before.note.md"));
    let roam = notemd_roam_import::convert::convert_page(&page, "2026-08-02");
    let (merged, _) = notemd_roam_import::merge::merge(&local, &roam);
    let mut merged = merged;
    merged.frontmatter = Some(notemd_roam_import::outline::touch_frontmatter(
        merged.frontmatter.as_deref(), "2026-08-02",
        "2026-08-02T04:00:05.019Z", "2026-08-03T09:00:00.000Z",
    ));
    assert_eq!(
        notemd_roam_import::outline::serialize_outline(&merged),
        include_str!("fixtures/daily.note.md")
    );
}
```

集成测试要求 crate 是 lib。在 `Cargo.toml` 里补:

```toml
[lib]
name = "notemd_roam_import"
path = "src/lib.rs"
```

并新建 `src/lib.rs`(`pub mod convert; pub mod dates; pub mod discover; pub mod merge; pub mod outline; pub mod roam_cli; pub mod roam_page; pub mod sync; pub mod syntax;`),`main.rs` 改为 `use notemd_roam_import::*;` + `mod plugin;`。

- [ ] **Step 7: 写 TS 侧同一 fixture 的往返测试**

`src/lib/outline/roam-golden.test.ts`:

```ts
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { parseOutline, serializeOutline } from './markdown'

/** Format-drift guard, TS half. The Rust backend writes this exact file
 *  (plugins-src/roam-import/backend/tests/golden.rs asserts it byte-for-byte);
 *  the host must read it back and re-serialize it unchanged. */
describe('roam-import golden daily note', () => {
  const path = 'plugins-src/roam-import/backend/tests/fixtures/daily.note.md'
  const text = readFileSync(path, 'utf8')

  it('round-trips through the host outline parser unchanged', () => {
    expect(serializeOutline(parseOutline(text))).toBe(text)
  })

  it('keeps every Roam block addressable by id', () => {
    const tree = parseOutline(text)
    const persisted = [...tree.nodes.values()].filter((n) => n.persistId === true)
    expect(persisted.length).toBeGreaterThan(0)
    for (const n of persisted) expect(n.id).not.toMatch(/^local-/)
  })
})
```

- [ ] **Step 8: 跑两侧测试**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml`
Expected: 全部 passed

Run: `pnpm test -- src/lib/outline/roam-golden.test.ts`
Expected: 2 passed

- [ ] **Step 9: 接上 plugin 的 ui 方法**

`plugin.rs` 的 `on_ui_request` 增加 `"sync_day"`:取 `date`/`graph`/`roam_path`,`discover` → `fetch_day` → `parse_day_result` → `sync_day`,vault 未就绪时返回 `Err("no vault configured")`。`now` 用 `chrono::Utc::now()`,`today` 用 `chrono::Local::now().date_naive()`。

- [ ] **Step 10: 编译**

Run: `cargo build --manifest-path plugins-src/roam-import/backend/Cargo.toml`
Expected: 成功

- [ ] **Step 11: 提交**

```bash
git add plugins-src/roam-import/backend src/lib/outline/roam-golden.test.ts
git commit -m "feat(roam-import): wire sync_day end to end, pinned by a shared golden fixture"
```

---

### Task 8: 窗口 UI —— 开关 / 状态 / 日期 / 同步

**Files:**
- Modify: `plugins-src/roam-import/src/lib/bridge.ts`
- Modify: `plugins-src/roam-import/src/App.svelte`
- Modify: `plugins-src/roam-import/src/lib/strings.ts`
- Modify: `plugins-src/roam-import/src/lib/strings.test.ts`

**Interfaces:**
- Consumes: 后端 ui 方法 `probe`(返回 `{ state, found, version, graphs }`)与 `sync_day`(返回 `SyncOutcome`)。
- Produces: `bridge.pluginRequest(method: string, params?: unknown): Promise<any>`。

- [ ] **Step 1: 加 bridge 方法**

`bridge.ts` 末尾:

```ts
/** Call this plugin's OWN backend (`on_ui_request`). The host strips the
 *  `plugin.` prefix before forwarding (ui_rpc.rs:258). */
export function pluginRequest(method: string, params?: unknown): Promise<any> {
  return bridge().request(`plugin.${method}`, params)
}

export type ProbeState = 'missing' | 'not_connected' | 'ready'
export interface RoamProbe {
  state: ProbeState
  found: string | null
  version: string | null
  graphs: string[]
}
export interface SyncOutcome {
  date: string; path: string; found: boolean
  created: number; updated: number; kept_local: number; roam_gone_kept: number
}
```

- [ ] **Step 2: 写文案键的失败测试**

`strings.test.ts` 追加(仓库既有断言是「四语言键集一致」,这里补新键存在性):

```ts
it('has the Roam CLI sync strings in every locale', () => {
  const keys = [
    'cli.toggle', 'cli.link', 'cli.state.missing', 'cli.state.notConnected',
    'cli.state.ready', 'cli.install', 'cli.connect', 'cli.date', 'cli.sync',
    'cli.syncing', 'cli.result', 'cli.noPage', 'cli.failed',
  ]
  for (const loc of ['en', 'zh', 'ja', 'de'] as const) {
    for (const k of keys) expect(catalogs[loc], `${loc}.${k}`).toHaveProperty(k)
  }
})
```

若 `strings.test.ts` 现有写法不导出 `catalogs`,按该文件既有的取字典方式改写这段,不要新造一套导出。

- [ ] **Step 3: 跑测试确认失败**

Run: `pnpm --filter roam-import-plugin test`
Expected: FAIL,缺 `cli.toggle` 等键

- [ ] **Step 4: 补四语言文案**

`strings.ts` 各语言目录加(zh 示例;en/ja/de 同键):

```
'cli.toggle': '使用 Roam CLI 同步',
'cli.link': 'roam-tools',
'cli.state.missing': '未检测到 roam 命令',
'cli.state.notConnected': '已安装 roam {version},但尚未连接图谱',
'cli.state.ready': 'roam {version} · 图谱 {graph}',
'cli.install': '安装:npm i -g @roam-research/roam-cli',
'cli.connect': '连接:在终端运行 roam connect',
'cli.date': '日期',
'cli.sync': '同步当日',
'cli.syncing': '正在同步…',
'cli.result': '新增 {created} 块 · 更新 {updated} 块 · 保留本地 {kept} 块',
'cli.noPage': 'Roam 里没有 {date} 的日记页,未写入任何文件',
'cli.failed': '同步失败:{error}',
```

- [ ] **Step 5: 跑测试确认通过**

Run: `pnpm --filter roam-import-plugin test`
Expected: PASS

- [ ] **Step 6: 加 UI**

`App.svelte`:`onMount` 里在 `vaultInfo()` 之后 `probe()` 一次;新增 `useCli` / `date` / `probeResult` / `syncing` / `syncResult` 状态。`useCli` 与 `date` 存 `localStorage`(纯 UI 偏好,不必进后端设置)。渲染顺序:标题 → CLI 区块 → 原有 hint + 选文件按钮。三态按 `probeResult.state` 分支,`missing` / `not_connected` 时禁用同步按钮并显示对应指引;链接用 `<a href="https://github.com/Roam-Research/roam-tools" target="_blank" rel="noopener">`(与 `ebook-import/src/App.svelte:362` 同写法)。日期 `<input type="date">`,默认昨天(本地时区)。

- [ ] **Step 7: 类型检查 + 构建**

Run: `pnpm --filter roam-import-plugin check && pnpm --filter roam-import-plugin build`
Expected: 无错误

- [ ] **Step 8: 提交**

```bash
git add plugins-src/roam-import/src
git commit -m "feat(roam-import): add the Roam CLI sync panel to the import window"
```

---

### Task 9: 放宽宿主 CLI 对文件参数的强制要求

**Files:**
- Modify: `src/lib/cli/CliRunner.svelte:111-114`
- Modify: `src/lib/cli/cli-runner.ts`
- Modify: `src/lib/cli/cli-runner.test.ts`

**Interfaces:**
- Produces: `cli-runner.ts` 导出 `requiresFileArg(entry: { args?: Array<{ type?: string; required?: boolean }> } | undefined): boolean`

现状:`CliRunner.svelte` 对所有插件子命令无条件要求 `payload.file`,任何没有文件参数的子命令(如 `notemd roam-day`)直接以退出码 2 失败。

- [ ] **Step 1: 写失败测试**

`cli-runner.test.ts` 追加:

```ts
import { requiresFileArg } from './cli-runner'

describe('requiresFileArg', () => {
  it('is true when the entry declares a required path arg', () => {
    expect(requiresFileArg({ args: [{ name: 'file', type: 'path', required: true }] })).toBe(true)
  })
  it('is false for a file-less subcommand', () => {
    expect(requiresFileArg({ args: [] })).toBe(false)
    expect(requiresFileArg(undefined)).toBe(false)
  })
  it('is false when the path arg is optional', () => {
    expect(requiresFileArg({ args: [{ name: 'file', type: 'path', required: false }] })).toBe(false)
  })
  it('ignores required args that are not paths', () => {
    expect(requiresFileArg({ args: [{ name: 'date', type: 'string', required: true }] })).toBe(false)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/cli/cli-runner.test.ts`
Expected: FAIL,`requiresFileArg is not a function`

- [ ] **Step 3: 实现并接线**

`cli-runner.ts` 追加:

```ts
/**
 * Does this CLI entry actually need a file argument? A subcommand that
 * declares no required `path` arg (e.g. `notemd roam-day --date …`) must not
 * be rejected for missing one.
 */
export function requiresFileArg(
  entry: { args?: Array<{ type?: string; required?: boolean }> } | undefined,
): boolean {
  return (entry?.args ?? []).some((a) => a.type === 'path' && a.required === true)
}
```

`CliRunner.svelte`:把 `const entry = …` 那行上移到 `if (!payload.file)` 之前,并把该判断改为

```ts
    const entry = (manifest.cli ?? []).find(c => c.subcommand === payload.subcommand)
    if (requiresFileArg(entry) && !payload.file) {
      await finish({ exit_code: 2, stderr: ['notemd: missing file argument'] })
      return
    }
```

`buildVirtualTab` / `renderTabAsInlineBody` / `outputPath` 三段都只在 `payload.file` 非空时才执行;`snap` 在无文件时用空壳(`path: ''`、`kind: 'markdown'`、`content: ''`),后端不读它。

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/cli/cli-runner.test.ts`
Expected: PASS

- [ ] **Step 5: 跑宿主全量测试确保没打断既有 CLI**

Run: `pnpm test && pnpm check`
Expected: 全绿

- [ ] **Step 6: 提交**

```bash
git add src/lib/cli/CliRunner.svelte src/lib/cli/cli-runner.ts src/lib/cli/cli-runner.test.ts
git commit -m "fix(cli): only demand a file argument when the subcommand declares one"
```

---

### Task 10: CLI 子命令接线 + 文档 + 端到端

**Files:**
- Modify: `plugins-src/roam-import/manifest.v2.json`
- Modify: `plugins-src/roam-import/backend/src/plugin.rs`
- Create: `plugins-src/roam-import/README.md`

**Interfaces:**
- Consumes: Task 7 的 `sync::sync_day`、Task 9 的宿主放宽。
- Produces: `notemd roam-day [--date …] [--graph …]`。

- [ ] **Step 1: 加 manifest 的 cli 条目**

`activation.events` 增 `"onCli:roam-day"`;`contributes` 增:

```json
    "cli": [
      {
        "subcommand": "roam-day",
        "command": "sync-day",
        "summary": "Sync one day's Roam daily note into the vault",
        "args": [],
        "flags": [
          { "long": "--date", "type": "string", "help": "yyyy-MM-dd, today or yesterday (default: yesterday)" },
          { "long": "--graph", "type": "string", "help": "Roam graph name (default: the CLI's own default)" }
        ],
        "requires_tab_context": false
      }
    ]
```

`i18n` 三个语言块各补 `"cli": { "roam-day": "…" }` 若该结构被宿主消费;若 manifest 的 i18n 只覆盖 `name`/`menus`,则不加(**先看 `plugin-protocol/src/lib.rs` 的 i18n 透传结构再决定,不要凭印象加字段** —— `deny_unknown_fields` 会让多余字段直接加载失败)。

- [ ] **Step 2: 实现 execute_command**

`plugin.rs`:

```rust
    fn execute_command(&mut self, host: &sdk::Host, params: &proto::ExecuteCommandParams)
        -> Result<Value, String> {
        match params.command.as_str() {
            "sync-day" => self.cli_sync_day(host, &params.context),
            other => Err(format!("unknown command '{other}'")),
        }
    }
```

`cli_sync_day` 用 `cli_str(context, "date")` / `cli_str(context, "graph")` 取 flag(照抄 `ebook-import/backend/src/plugin.rs:116-130` 的 `cli_str`,它按 `/cli/args/`、`/cli/flags/`、`/cli/`、`/` 四个指针依次找),然后与 ui 方法 `sync_day` 走同一个内部函数。返回值即 `SyncOutcome` 的 JSON。

- [ ] **Step 3: 构建 + 装 dev 插件**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml && bash scripts/dev-install-plugin.sh roam-import`
Expected: 测试全绿,输出 `✓ installed notemd.roam-import@1.1.0 (debug, arm64, backend + ui)`

- [ ] **Step 4: 端到端跑 CLI**

Run: `notemd roam-day --date 2026-08-02 --json`
Expected: `{"ok":true,"data":{"date":"2026-08-02","path":"dailynote/2026/2026-08-02.note.md","found":true,…}}`,且 vault 里出现该文件。

再跑一次同样命令,确认文件内容未变(幂等):

Run: `notemd roam-day --date 2026-08-02 --json`
Expected: `created` 与 `updated` 归零或与首次一致,文件 `git diff` 无变化。

未安装 roam 的路径无法在本机构造,改为断言错误分支的文案由单测覆盖(Task 1 的 `ProbeState::Missing`)。

- [ ] **Step 5: 写 README**

`plugins-src/roam-import/README.md`:两条路径(整图 JSON 导入 / CLI 当日同步)、前置条件(Roam 桌面版在运行 + `roam connect`)、合并语义三句话(Roam 为准、本地块保留、Roam 删除的块保留)、CLI 用法、`#.rm-hide` 不被过滤的已知取舍。

- [ ] **Step 6: 提交**

```bash
git add plugins-src/roam-import/manifest.v2.json plugins-src/roam-import/backend/src/plugin.rs plugins-src/roam-import/README.md
git commit -m "feat(roam-import): expose the daily sync as 'notemd roam-day'"
```

- [ ] **Step 7: 交给用户实机验证**

给出手动验证清单(不自己跑 GUI 自动化):

1. 重启 note.md → 文件菜单 ▸「从 Roam Research 导入…」开窗
2. 勾「使用 Roam CLI 同步」→ 状态行显示 `roam 0.9.2 · 图谱 bruce`
3. 选一个有 Roam 日记的日期 → 点「同步当日」→ 看统计与生成的笔记
4. 在该笔记里手写一个块 → 再同步一次 → 手写块仍在原位
5. 切到日/英/德界面语言各看一次该区块文案

---

## Self-Review

**Spec coverage**

| spec 章节 | 落在哪个任务 |
|---|---|
| §2 形态:后端 + 前端 | Task 1 |
| §3 取数(递归 pull、`:as` 别名、order 排序、空结果) | Task 2 |
| §3 `#.rm-hide` 取舍 | Task 10 README 记录 |
| §4 合并全部六条规则 + `id::` 强制落盘 | Task 5(id::)、Task 6(合并) |
| §4 目标路径 | Task 7 `daily_rel_path` |
| §5 界面三态 / 日期 / 开关 / 外链 / i18n | Task 8 |
| §6 CLI 子命令 + 宿主放宽 | Task 9、Task 10 |
| §7 单测 / golden fixture 双侧 | Task 3–7 |
| §8 验收 1–5 | Task 10 Step 4 与 Step 7 |

**Placeholder scan**:无 TBD;两处刻意的「先跑再定」都给了明确判据 —— Task 5 Step 1 的时间戳断言(以实际输出核对后改断言)、Task 7 Step 5 的 golden fixture(先占位再人工核对)。Task 10 Step 1 的 i18n 字段要求先读 `plugin-protocol/src/lib.rs` 再决定,理由是 `deny_unknown_fields`。

**Type consistency**:`Tree`/`Node` 字段在 Task 4 定义,Task 5/6/7 引用一致;`MergeStats` 四个字段与 `SyncOutcome` 同名同义;`ProbeState` 的 snake_case 序列化与 TS 侧 `'missing' | 'not_connected' | 'ready'` 对齐;`parse_day_result` 的 `Option<RoamPage>` 语义(None = 当天无页)在 Task 2/7 一致。
