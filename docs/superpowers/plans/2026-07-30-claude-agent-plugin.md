# Claude Headless Agent 插件 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `notemd.claude-agent` 插件 —— 在 vault 内的任务模板目录里跑 `claude -p`,窗口实时看流、随时中断,CLI 可 detach 触发。

**Architecture:** 后端 + 前端形态。Rust 后端(`notemd-plugin-sdk`)持有任务发现、锁、claude 子进程与 stream-json 解析,经 `host.ui.post` 把事件推给隔离 webview 窗口;同一个二进制带 `--runner` 模式,供 CLI detach 后独立跑完长任务。前端是 Svelte 独立窗口(照 `plugins-src/openclaw`)。

**Tech Stack:** Rust(tokio / serde_json / libc)、`notemd-plugin-sdk`、Svelte 5 + Vite 6、vitest。

**设计文档:** `docs/superpowers/specs/2026-07-30-claude-headless-agent-plugin-design.md`
**技术准绳:** `docs/2026-07-30-claude-headless-automation-implementation-plan.md`

---

## 背景速读(实现者必读)

三条你不知道就会踩的约束:

1. **插件 UI 是隔离 webview,没有 Tauri IPC。** 窗口只能用宿主注入的 `window.notemd.request(method, params)`;方法名以 `plugin.` 开头时宿主转发给本插件后端进程的 `ui.request`。反方向是后端 `host.ui_post("main", payload)` → 窗口 `onMessage`。样板:`plugins-src/openclaw/src/lib/bridge.ts:1-11`。
2. **SDK 的 `on_ui_request` / `execute_command` 是同步的**(`notemd-plugin-sdk/src/lib.rs:17-30`),跑在协议读循环上。长任务必须 `tokio::spawn` 出去,立即返回,否则整个插件协议卡死。openclaw 用 `tokio::task::block_in_place` + `Handle::current().block_on` 处理短异步(`plugins-src/openclaw/backend/src/lib.rs:127-139`),照抄这个模式即可。
3. **CLI 子命令另起无头 app 实例**(`src-tauri/src/cli/runner.rs:82`),单次 invoke 上限 300 秒,实例退出会收走子进程。所以 CLI 默认 detach 到 `--runner` 模式。

Rust 测试按本仓库惯例**写在同文件的 `#[cfg(test)] mod tests`**(参见 `src-tauri/src/plugin_runtime/host_api.rs`)。

---

## File Structure

**后端** `plugins-src/claude-agent/backend/`:

| 文件 | 职责 |
|---|---|
| `Cargo.toml` | crate 定义,依赖 sdk / tokio / serde / serde_json / libc |
| `src/main.rs` | 入口:`--runner <dir>` 走 runner 模式,否则 SDK serve 循环 |
| `src/discover.rs` | 找 `claude` 可执行文件(GUI PATH 精简是头号坑) |
| `src/prompt.rs` | 三段 prompt 拼接 + argv 组装 |
| `src/stream.rs` | stream-json 逐行解析 → `Event`;抽取最终 `RunResult` |
| `src/task.rs` | `TaskDef`(task.json)、任务发现、内置模板落盘、`.gitignore` 幂等 |
| `src/settings.rs` | `${VAULT}` 替换 → `.claude/settings.local.json` |
| `src/lock.rs` | 任务锁:获取 / 冲突 / 陈旧 pid 回收 |
| `src/record.rs` | `RunRecord` 模型、截断、写盘、读最近 N 条 |
| `src/engine.rs` | 运行引擎:spawn claude、泵事件、超时、取消(两条链路共用) |
| `src/runner.rs` | `--runner` 模式:读 request.json → 跑引擎 → 写记录 |
| `src/plugin.rs` | `NotemdPlugin` 实现 + `ui.request` 分发 |
| `templates/` | 内置模板源文件(`include_str!` 嵌入二进制) |

**前端** `plugins-src/claude-agent/`:

| 文件 | 职责 |
|---|---|
| `manifest.v2.json` | 插件声明 |
| `package.json` / `vite.config.ts` / `tsconfig.json` / `vitest.config.ts` / `index.html` | 独立 Vite 工程(照 `plugins-src/weekly-review`) |
| `src/main.ts` / `src/App.svelte` | 挂载 + 三栏布局 |
| `src/lib/bridge.ts` | `window.notemd` 类型化封装(照抄 openclaw) |
| `src/lib/strings.ts` | 插件自带 i18n(zh/en/ja/de) |
| `src/lib/events.ts` | 事件流 → 视图模型 reducer(纯函数,vitest 覆盖) |
| `src/components/TaskList.svelte` | 左栏任务列表 |
| `src/components/RunStream.svelte` | 中栏流式事件区 |
| `src/components/HistoryList.svelte` | 历史运行记录 |

---

## Task 1: 后端 crate 骨架 + manifest + 装得进去

**Files:**
- Create: `plugins-src/claude-agent/backend/Cargo.toml`
- Create: `plugins-src/claude-agent/backend/src/main.rs`
- Create: `plugins-src/claude-agent/backend/src/plugin.rs`
- Create: `plugins-src/claude-agent/manifest.v2.json`

- [ ] **Step 1: 写 Cargo.toml**

```toml
[package]
name = "notemd-claude-agent"
version = "1.0.0"
edition = "2021"

[[bin]]
name = "notemd-claude-agent"
path = "src/main.rs"

[dependencies]
notemd-plugin-sdk = { path = "../../../notemd-plugin-sdk" }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "io-util", "process", "sync"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = "0.4"
libc = "0.2"

[dev-dependencies]
tempfile = "3"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = true
```

注意:**不要**设 `panic = "abort"`(pos-log 设了),本 crate 的测试需要 unwind。

- [ ] **Step 2: 写 main.rs**

```rust
//! notemd.claude-agent 入口。两种模式:
//!  - `--runner <runDir>`:CLI detach 后的独立跑批进程(见 runner.rs)
//!  - 无参数:SDK serve 循环,作为宿主管理的常驻插件进程
mod discover;
mod engine;
mod lock;
mod plugin;
mod prompt;
mod record;
mod runner;
mod settings;
mod stream;
mod task;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("tokio runtime");
    if let Some(i) = args.iter().position(|a| a == "--runner") {
        let dir = args.get(i + 1).cloned().unwrap_or_default();
        std::process::exit(rt.block_on(runner::run(std::path::PathBuf::from(dir))));
    }
    rt.block_on(notemd_plugin_sdk::serve(plugin::ClaudeAgentPlugin::new()));
}
```

- [ ] **Step 3: 写 plugin.rs 的最小骨架(后续任务往里填)**

```rust
//! NotemdPlugin 实现:窗口 RPC 分发 + 命令处理。
use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde_json::{json, Value};

pub struct ClaudeAgentPlugin {
    /// 开窗那一刻宿主给的 tab 上下文(v1 形状的 context.tab)。
    pub tab_context: Option<Value>,
}

impl ClaudeAgentPlugin {
    pub fn new() -> Self { Self { tab_context: None } }
}

impl sdk::NotemdPlugin for ClaudeAgentPlugin {
    fn activate(&mut self, host: &sdk::Host, _p: &proto::ActivateParams) -> Result<(), String> {
        host.log_info("claude-agent activated");
        Ok(())
    }

    fn deactivate(&mut self, _host: &sdk::Host) {}

    fn execute_command(&mut self, _host: &sdk::Host, params: &proto::ExecuteCommandParams)
        -> Result<Value, String> {
        match params.command.as_str() {
            "open" => {
                self.tab_context = params.context.get("tab").cloned();
                Ok(json!({ "success": true }))
            }
            other => Err(format!("unknown command '{other}'")),
        }
    }

    fn on_ui_request(&mut self, _host: &sdk::Host, method: &str, _params: Value)
        -> Result<Value, String> {
        match method {
            "context.get" => Ok(json!({ "tab": self.tab_context })),
            other => Err(format!("unknown ui method '{other}'")),
        }
    }
}
```

此刻其余 mod 还不存在,`main.rs` 的 `mod` 行会编译失败 —— **先建空文件占位**:`discover.rs` / `engine.rs` / `lock.rs` / `prompt.rs` / `record.rs` / `runner.rs` / `settings.rs` / `stream.rs` / `task.rs` 各写一行注释即可。`runner::run` 先写桩:

```rust
//! CLI detach 后的独立跑批进程(Task 12 填实)。
pub async fn run(_dir: std::path::PathBuf) -> i32 { 0 }
```

- [ ] **Step 4: 写 manifest.v2.json**

```json
{
  "manifest_version": 2,
  "id": "notemd.claude-agent",
  "name": "Claude Agent",
  "version": "1.0.0",
  "kind": "native",
  "engines": { "notemd": ">=6.720.4" },
  "description": "Run Claude Code headless against your vault: task templates live in .notemd/agent-tasks, stream live in a window, or fire detached from the CLI.",
  "binary": {
    "aarch64-apple-darwin": "bin/notemd-claude-agent",
    "x86_64-apple-darwin": "bin/notemd-claude-agent"
  },
  "ui": "ui/",
  "activation": { "events": ["onCommand:open", "onCommand:run", "onCli:agent"] },
  "contributes": {
    "menus": [
      { "location": "window", "label": "Claude Agent…", "command": "open" }
    ],
    "windows": [
      {
        "id": "main",
        "entry": "index.html",
        "title": "Claude Agent",
        "width": 900,
        "height": 640,
        "min_width": 640,
        "min_height": 420,
        "open_command": "open"
      }
    ],
    "cli": [
      {
        "subcommand": "agent",
        "command": "run",
        "args": [{ "name": "task", "ty": "string", "required": true }],
        "flags": [
          { "long": "--prompt", "short": "-p" },
          { "long": "--wait" }
        ]
      }
    ]
  },
  "capabilities": ["ui", "toast", "vault.read"],
  "request_timeout_seconds": 300,
  "idle_shutdown_seconds": 0
}
```

`idle_shutdown_seconds: 0` = 不空闲自杀(有任务在跑时被回收会杀掉子进程)。

- [ ] **Step 5: 编译**

Run: `cargo build --manifest-path plugins-src/claude-agent/backend/Cargo.toml`
Expected: 编译通过(warnings about unused modules 可忽略)

- [ ] **Step 6: Commit**

```bash
git add plugins-src/claude-agent/backend/Cargo.toml plugins-src/claude-agent/backend/Cargo.lock \
        plugins-src/claude-agent/backend/src plugins-src/claude-agent/manifest.v2.json
git commit -m "feat(claude-agent): scaffold the backend crate and manifest"
```

---

## Task 2: 找到 claude 可执行文件

GUI 应用的 PATH 是精简的(不含 `~/.local/bin` 等),这是这类插件的头号坑。

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/discover.rs`

- [ ] **Step 1: 写失败测试**

```rust
//! 定位 `claude` 可执行文件。GUI 进程的 PATH 不含用户 shell 的补充路径,
//! 所以显式路径 → 登录 shell 查询 → 常见安装位置,三级回退。
use std::path::{Path, PathBuf};

/// 候选安装位置,按优先级。`~` 由调用方展开。
pub fn candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".claude/local/claude"),
        home.join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
    ]
}

/// 纯逻辑版本,便于测试:`shell_lookup` 模拟登录 shell 的 `command -v claude`,
/// `is_exec` 模拟"存在且可执行"。
pub fn discover_with(
    explicit: Option<&str>,
    home: &Path,
    shell_lookup: impl Fn() -> Option<PathBuf>,
    is_exec: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if let Some(p) = explicit.filter(|s| !s.is_empty()) {
        let p = PathBuf::from(p);
        if is_exec(&p) { return Some(p); }
    }
    if let Some(p) = shell_lookup() {
        if is_exec(&p) { return Some(p); }
    }
    candidates(home).into_iter().find(|c| is_exec(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_path_wins() {
        let got = discover_with(Some("/custom/claude"), Path::new("/home/u"),
            || Some(PathBuf::from("/shell/claude")), |_| true);
        assert_eq!(got, Some(PathBuf::from("/custom/claude")));
    }

    #[test]
    fn falls_back_to_shell_lookup_when_explicit_missing() {
        let got = discover_with(Some("/gone/claude"), Path::new("/home/u"),
            || Some(PathBuf::from("/shell/claude")),
            |p| p != Path::new("/gone/claude"));
        assert_eq!(got, Some(PathBuf::from("/shell/claude")));
    }

    #[test]
    fn falls_back_to_candidates_when_shell_finds_nothing() {
        let home = Path::new("/home/u");
        let want = home.join(".local/bin/claude");
        let w = want.clone();
        let got = discover_with(None, home, || None, move |p| p == w);
        assert_eq!(got, Some(want));
    }

    #[test]
    fn returns_none_when_nothing_is_executable() {
        assert_eq!(discover_with(None, Path::new("/home/u"), || None, |_| false), None);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml discover`
Expected: 先失败(文件刚写时若你先只贴测试),贴完实现后转 PASS

- [ ] **Step 3: 加生产入口(真实 shell 查询 + 可执行判定)**

```rust
/// 生产入口。登录 shell 查询用 `/bin/zsh -lic 'command -v claude'` —— 必须带
/// `-l`(读 profile)和 `-i`(读 rc),否则拿不到用户装在 ~/.local/bin 的 claude。
pub fn discover(explicit: Option<&str>) -> Option<PathBuf> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    discover_with(explicit, &home, shell_lookup, is_executable)
}

fn shell_lookup() -> Option<PathBuf> {
    let out = std::process::Command::new("/bin/zsh")
        .args(["-lic", "command -v claude"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(PathBuf::from(s)) }
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml discover`
Expected: 4 passed

- [ ] **Step 5: Commit**

```bash
git add plugins-src/claude-agent/backend/src/discover.rs
git commit -m "feat(claude-agent): find the claude binary despite a lean GUI PATH"
```

---

## Task 3: prompt 三段拼接 + argv 组装

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/prompt.rs`

- [ ] **Step 1: 写测试 + 实现**

```rust
//! 三段 prompt 拼接与 claude argv 组装。顺序固定并写进模板作者文档:
//! ①模板 prompt ②本次输入 ③当前文档上下文。
use crate::task::TaskDef;

/// 开窗那一刻的 tab 上下文(取自 v1 形状 context.tab 的两个字段)。
#[derive(Debug, Clone, PartialEq)]
pub struct TabContext {
    pub path: String,
    pub selection: String,
}

pub fn compose(task_prompt: &str, user_prompt: &str, ctx: Option<&TabContext>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !task_prompt.trim().is_empty() { parts.push(task_prompt.trim().to_string()); }
    if !user_prompt.trim().is_empty() { parts.push(user_prompt.trim().to_string()); }
    if let Some(c) = ctx {
        let mut b = format!("## 当前文档\n路径:{}", c.path);
        if !c.selection.trim().is_empty() {
            b.push_str(&format!("\n选中内容:\n{}", c.selection.trim()));
        }
        parts.push(b);
    }
    parts.join("\n\n")
}

/// claude 的命令行参数(不含可执行文件本身)。
/// 刻意不传 `--bare`:那会跳过 CLAUDE.md / skills / .mcp.json 的自动发现。
pub fn build_argv(task: &TaskDef, prompt: &str) -> Vec<String> {
    let mut v = vec![
        "-p".to_string(), prompt.to_string(),
        "--output-format".to_string(), "stream-json".to_string(),
        "--verbose".to_string(),
    ];
    if let Some(t) = task.max_turns { v.push("--max-turns".into()); v.push(t.to_string()); }
    if let Some(m) = task.model.as_deref().filter(|s| !s.is_empty()) {
        v.push("--model".into()); v.push(m.to_string());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> TaskDef {
        TaskDef { id: "t".into(), name: "T".into(), description: String::new(),
                  prompt: "P".into(), max_turns: Some(50), timeout_seconds: 1800, model: None }
    }

    #[test]
    fn joins_three_parts_in_fixed_order() {
        let ctx = TabContext { path: "/v/a.md".into(), selection: "sel".into() };
        let got = compose("TASK", "USER", Some(&ctx));
        assert_eq!(got, "TASK\n\nUSER\n\n## 当前文档\n路径:/v/a.md\n选中内容:\nsel");
    }

    #[test]
    fn omits_empty_parts() {
        assert_eq!(compose("TASK", "   ", None), "TASK");
    }

    #[test]
    fn context_without_selection_keeps_only_the_path() {
        let ctx = TabContext { path: "/v/a.md".into(), selection: "  ".into() };
        assert_eq!(compose("", "", Some(&ctx)), "## 当前文档\n路径:/v/a.md");
    }

    #[test]
    fn argv_is_stream_json_verbose_and_never_bare() {
        let got = build_argv(&task(), "hi");
        assert_eq!(got, vec!["-p", "hi", "--output-format", "stream-json", "--verbose",
                             "--max-turns", "50"]);
        assert!(!got.iter().any(|a| a == "--bare"));
    }

    #[test]
    fn argv_passes_model_through_when_set() {
        let mut t = task();
        t.model = Some("claude-opus-5".into());
        let got = build_argv(&t, "hi");
        assert!(got.windows(2).any(|w| w == ["--model", "claude-opus-5"]));
    }
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml prompt`
Expected: 5 passed(需 Task 5 的 `TaskDef` 先存在 —— 若先做本任务,在 `task.rs` 里先放 `TaskDef` 结构体定义)

- [ ] **Step 3: Commit**

```bash
git add plugins-src/claude-agent/backend/src/prompt.rs plugins-src/claude-agent/backend/src/task.rs
git commit -m "feat(claude-agent): compose the three-part prompt and claude argv"
```

---

## Task 4: stream-json 解析

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/stream.rs`

- [ ] **Step 1: 写测试 + 实现**

```rust
//! `--output-format stream-json --verbose` 的逐行解析。宿主侧只关心四类事件:
//! 系统初始化、助手文本、工具调用、最终结果。其余行(含非 JSON 噪声)丢弃。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    System { subtype: String },
    Text { text: String },
    ToolUse { name: String, brief: String },
    Result(RunResult),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResult {
    pub is_error: bool,
    pub result: String,
    pub session_id: Option<String>,
    pub num_turns: Option<u64>,
}

/// 解析一行。返回 `None` 表示这行不产生事件(噪声/不关心的类型)。
pub fn parse_line(line: &str) -> Option<Event> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    match v.get("type")?.as_str()? {
        "system" => Some(Event::System {
            subtype: v.get("subtype").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        }),
        "assistant" => {
            let blocks = v.pointer("/message/content")?.as_array()?;
            for b in blocks {
                match b.get("type").and_then(|t| t.as_str()) {
                    Some("tool_use") => {
                        let name = b.get("name").and_then(|n| n.as_str()).unwrap_or("tool");
                        return Some(Event::ToolUse { name: name.to_string(), brief: tool_brief(b) });
                    }
                    Some("text") => {
                        let t = b.get("text").and_then(|t| t.as_str()).unwrap_or("");
                        if !t.is_empty() { return Some(Event::Text { text: t.to_string() }); }
                    }
                    _ => {}
                }
            }
            None
        }
        "result" => Some(Event::Result(RunResult {
            is_error: v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false),
            result: v.get("result").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            session_id: v.get("session_id").and_then(|s| s.as_str()).map(str::to_string),
            num_turns: v.get("num_turns").and_then(|n| n.as_u64()),
        })),
        _ => None,
    }
}

/// 工具调用的一行摘要:优先文件路径,其次命令,再次模式串。
fn tool_brief(block: &serde_json::Value) -> String {
    let i = match block.get("input") { Some(i) => i, None => return String::new() };
    for k in ["file_path", "path", "command", "pattern", "url"] {
        if let Some(s) = i.get(k).and_then(|v| v.as_str()) {
            return s.chars().take(120).collect();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_result_line() {
        let l = r#"{"type":"result","subtype":"success","result":"done","session_id":"s1","num_turns":12,"is_error":false}"#;
        assert_eq!(parse_line(l), Some(Event::Result(RunResult {
            is_error: false, result: "done".into(),
            session_id: Some("s1".into()), num_turns: Some(12),
        })));
    }

    #[test]
    fn parses_a_tool_use_with_a_file_brief() {
        let l = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"src/a.rs"}}]}}"#;
        assert_eq!(parse_line(l), Some(Event::ToolUse { name: "Read".into(), brief: "src/a.rs".into() }));
    }

    #[test]
    fn parses_assistant_text() {
        let l = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello"}]}}"#;
        assert_eq!(parse_line(l), Some(Event::Text { text: "hello".into() }));
    }

    #[test]
    fn drops_noise_lines() {
        assert_eq!(parse_line("not json at all"), None);
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line(r#"{"type":"user","message":{}}"#), None);
        assert_eq!(parse_line(r#"{"no_type":1}"#), None);
    }

    #[test]
    fn treats_is_error_true_as_a_failed_result() {
        let l = r#"{"type":"result","subtype":"error_max_turns","result":"hit limit","is_error":true}"#;
        match parse_line(l) {
            Some(Event::Result(r)) => { assert!(r.is_error); assert_eq!(r.result, "hit limit"); }
            other => panic!("expected a result event, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml stream`
Expected: 5 passed

- [ ] **Step 3: Commit**

```bash
git add plugins-src/claude-agent/backend/src/stream.rs
git commit -m "feat(claude-agent): parse the stream-json event lines"
```

---

## Task 5: 任务发现与 task.json

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/task.rs`

- [ ] **Step 1: 写测试 + 实现**

```rust
//! 任务模板:`<vault>/.notemd/agent-tasks/<id>/task.json` + CLAUDE.md + .claude/。
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskDef {
    /// 目录名。序列化给窗口用;从磁盘读时由目录名填充。
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub max_turns: Option<u64>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub model: Option<String>,
}

fn default_timeout() -> u64 { 1800 }

pub fn tasks_root(vault: &Path) -> PathBuf { vault.join(".notemd/agent-tasks") }
pub fn runs_root(vault: &Path) -> PathBuf { vault.join(".notemd/agent-runs") }
pub fn task_dir(vault: &Path, id: &str) -> PathBuf { tasks_root(vault).join(id) }

/// 扫描任务目录。跳过读不出 task.json 的目录(坏模板不该让整个列表瞎掉),
/// 按 id 排序保证窗口列表稳定。
pub fn discover(vault: &Path) -> Vec<TaskDef> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(tasks_root(vault)) else { return out };
    for e in rd.flatten() {
        if !e.path().is_dir() { continue }
        let id = e.file_name().to_string_lossy().to_string();
        if let Some(mut t) = read_task(&e.path()) { t.id = id; out.push(t); }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

pub fn read_task(dir: &Path) -> Option<TaskDef> {
    let s = std::fs::read_to_string(dir.join("task.json")).ok()?;
    serde_json::from_str(&s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_task(root: &Path, id: &str, json: &str) {
        let d = tasks_root(root).join(id);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("task.json"), json).unwrap();
    }

    #[test]
    fn discovers_tasks_sorted_by_id_with_the_dir_name_as_id() {
        let v = tempfile::tempdir().unwrap();
        write_task(v.path(), "zeta", r#"{"name":"Z"}"#);
        write_task(v.path(), "alpha", r#"{"name":"A","description":"d","prompt":"p"}"#);
        let got = discover(v.path());
        assert_eq!(got.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["alpha", "zeta"]);
        assert_eq!(got[0].name, "A");
        assert_eq!(got[0].prompt, "p");
    }

    #[test]
    fn defaults_timeout_to_half_an_hour() {
        let v = tempfile::tempdir().unwrap();
        write_task(v.path(), "t", r#"{"name":"T"}"#);
        assert_eq!(discover(v.path())[0].timeout_seconds, 1800);
    }

    #[test]
    fn skips_dirs_whose_task_json_is_broken_or_missing() {
        let v = tempfile::tempdir().unwrap();
        write_task(v.path(), "good", r#"{"name":"G"}"#);
        write_task(v.path(), "broken", "{not json");
        std::fs::create_dir_all(tasks_root(v.path()).join("empty")).unwrap();
        assert_eq!(discover(v.path()).len(), 1);
    }

    #[test]
    fn returns_empty_when_the_vault_has_no_tasks_dir() {
        let v = tempfile::tempdir().unwrap();
        assert!(discover(v.path()).is_empty());
    }
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml task::`
Expected: 4 passed

- [ ] **Step 3: Commit**

```bash
git add plugins-src/claude-agent/backend/src/task.rs
git commit -m "feat(claude-agent): discover task templates from the vault"
```

---

## Task 6: 内置模板落盘(不覆盖)+ .gitignore 幂等

**Files:**
- Create: `plugins-src/claude-agent/backend/templates/selfcheck/task.json`
- Create: `plugins-src/claude-agent/backend/templates/selfcheck/CLAUDE.md`
- Create: `plugins-src/claude-agent/backend/templates/selfcheck/settings.json`
- Create: `plugins-src/claude-agent/backend/templates/annotation-sweep/task.json`
- Create: `plugins-src/claude-agent/backend/templates/annotation-sweep/CLAUDE.md`
- Create: `plugins-src/claude-agent/backend/templates/annotation-sweep/settings.json`
- Modify: `plugins-src/claude-agent/backend/src/task.rs`

- [ ] **Step 1: 写 selfcheck 模板三件套**

`templates/selfcheck/task.json`:
```json
{
  "name": "Self-check",
  "description": "Report the environment: which CLAUDE.md files loaded, which skills are available, and the vault root.",
  "prompt": "报告你当前的运行环境,逐项列出:\n1. 你加载了哪几份 CLAUDE.md(给出完整路径)\n2. 你能发现哪些 skills\n3. 你的工作目录与 vault 根路径\n4. 你当前被授予的工具权限\n把这份报告同时写入 output/selfcheck.md,然后用一段话总结环境是否就绪。",
  "max_turns": 10,
  "timeout_seconds": 300
}
```

`templates/selfcheck/CLAUDE.md`:
```markdown
# 任务:环境自检

你在 note.md 的 Claude Agent 插件里以 headless 模式运行。

- 工作目录是本任务模板目录,vault 根在 `${VAULT}`。
- 交付物写入本目录下的 `output/`。
- 只读不写 vault 里的笔记 —— 自检不许改用户的任何文件。
```

`templates/selfcheck/settings.json`:
```json
{
  "permissions": {
    "allow": ["Read(${VAULT}/**)", "Write(output/**)", "Edit(output/**)"]
  }
}
```

- [ ] **Step 2: 写 annotation-sweep 模板三件套**

协议原文见 `docs/superpowers/specs/2026-07-27-annotation-qa-loop-design.md` §3。

`templates/annotation-sweep/task.json`:
```json
{
  "name": "Annotation sweep",
  "description": "Answer open questions captured in sidecar .note.md files.",
  "prompt": "执行一次批注问答 sweep,严格按 CLAUDE.md 的协议作答。若没有 status:: open 的问题,直接报告「无待答问题」并结束。",
  "max_turns": 60,
  "timeout_seconds": 1800
}
```

`templates/annotation-sweep/CLAUDE.md`:
```markdown
# 任务:批注问答 sweep

你在 note.md 的 Claude Agent 插件里以 headless 模式运行,vault 根在 `${VAULT}`。

## 协议(逐条遵守)

1. 扫描 vault 中 `type:: question` 且 `status:: open` 的 `.note.md` 节点。
2. 结合节点的 `line::` 定位伴生源文件的原文上下文再作答。
3. 短答案:在该问题节点下追加一个 `✦` 前缀的子节点,并带上 `answered::`(ISO 8601)与 `by:: claude-code`。
4. 长答案:写入 `${VAULT}/answers/yyyy-MM-dd-<slug>.md`,问题节点下只留一行 `✦` 摘要 + 指向该文件的链接。
5. 把该节点的 `status::` 置为 `answered`。

## 硬约束

- **绝不**把 `status::` 置为 `closed` —— 只有人能关闭问题。
- **绝不**修改源 `.md` 文件,只写 `.note.md` 与 `answers/`。
- **绝不**改动 `●` 开头的内容,那是人写的。
- 拿不准的问题就跳过,并在最终答复里列出跳过原因。
```

`templates/annotation-sweep/settings.json`:
```json
{
  "permissions": {
    "allow": [
      "Read(${VAULT}/**)",
      "Write(${VAULT}/**/*.note.md)",
      "Edit(${VAULT}/**/*.note.md)",
      "Write(${VAULT}/answers/**)",
      "Edit(${VAULT}/answers/**)"
    ]
  }
}
```

白名单里**不给**源 `.md` 写权限 —— 让协议约束落到权限层,而不是只靠 prompt 自觉。

- [ ] **Step 3: 写落盘测试 + 实现(追加进 task.rs)**

```rust
/// 内置模板,编译进二进制。首次运行时写进 vault;已存在的文件一律不覆盖。
const BUILTIN: &[(&str, &[(&str, &str)])] = &[
    ("selfcheck", &[
        ("task.json", include_str!("../templates/selfcheck/task.json")),
        ("CLAUDE.md", include_str!("../templates/selfcheck/CLAUDE.md")),
        (".claude/settings.json", include_str!("../templates/selfcheck/settings.json")),
    ]),
    ("annotation-sweep", &[
        ("task.json", include_str!("../templates/annotation-sweep/task.json")),
        ("CLAUDE.md", include_str!("../templates/annotation-sweep/CLAUDE.md")),
        (".claude/settings.json", include_str!("../templates/annotation-sweep/settings.json")),
    ]),
];

/// 幂等:缺什么补什么,已有文件绝不覆盖(用户改过的模板归用户)。
/// 返回实际写出的相对路径,便于日志。
pub fn seed_builtin_templates(vault: &Path) -> Vec<String> {
    let mut wrote = Vec::new();
    for (id, files) in BUILTIN {
        for (rel, body) in *files {
            let p = task_dir(vault, id).join(rel);
            if p.exists() { continue }
            if let Some(parent) = p.parent() { let _ = std::fs::create_dir_all(parent); }
            if std::fs::write(&p, body).is_ok() { wrote.push(format!("{id}/{rel}")); }
        }
    }
    wrote
}

/// 确保 vault 的 .gitignore 忽略派生数据。写法照 agents_sync::ensure_gitignore:
/// 逐行比对后 append,幂等。
pub fn ensure_gitignore(vault: &Path) {
    const LINES: [&str; 2] = [
        ".notemd/agent-runs/",
        ".notemd/agent-tasks/*/.claude/settings.local.json",
    ];
    let gi = vault.join(".gitignore");
    let cur = std::fs::read_to_string(&gi).unwrap_or_default();
    let missing: Vec<&str> = LINES.iter().copied()
        .filter(|l| !cur.lines().any(|e| e.trim() == *l)).collect();
    if missing.is_empty() { return }
    let mut next = cur;
    if !next.is_empty() && !next.ends_with('\n') { next.push('\n'); }
    for l in missing { next.push_str(l); next.push('\n'); }
    let _ = std::fs::write(&gi, next);
}
```

测试(追加进 `task.rs` 的 `mod tests`):

```rust
    #[test]
    fn seeds_both_builtin_templates_on_a_fresh_vault() {
        let v = tempfile::tempdir().unwrap();
        let wrote = seed_builtin_templates(v.path());
        assert_eq!(wrote.len(), 6);
        assert!(task_dir(v.path(), "selfcheck").join("CLAUDE.md").exists());
        assert!(task_dir(v.path(), "annotation-sweep").join(".claude/settings.json").exists());
        let ids: Vec<String> = discover(v.path()).into_iter().map(|t| t.id).collect();
        assert_eq!(ids, vec!["annotation-sweep", "selfcheck"]);
    }

    #[test]
    fn never_overwrites_a_template_the_user_edited() {
        let v = tempfile::tempdir().unwrap();
        seed_builtin_templates(v.path());
        let mine = task_dir(v.path(), "selfcheck").join("CLAUDE.md");
        std::fs::write(&mine, "MINE").unwrap();
        let wrote = seed_builtin_templates(v.path());
        assert!(wrote.is_empty());
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), "MINE");
    }

    #[test]
    fn gitignore_appends_once_and_preserves_existing_lines() {
        let v = tempfile::tempdir().unwrap();
        std::fs::write(v.path().join(".gitignore"), "node_modules\n").unwrap();
        ensure_gitignore(v.path());
        ensure_gitignore(v.path());
        let gi = std::fs::read_to_string(v.path().join(".gitignore")).unwrap();
        assert!(gi.starts_with("node_modules\n"));
        assert_eq!(gi.matches(".notemd/agent-runs/").count(), 1);
        assert_eq!(gi.matches("settings.local.json").count(), 1);
    }
```

- [ ] **Step 4: 跑测试**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml task::`
Expected: 7 passed

- [ ] **Step 5: Commit**

```bash
git add plugins-src/claude-agent/backend/templates plugins-src/claude-agent/backend/src/task.rs
git commit -m "feat(claude-agent): ship the selfcheck and annotation-sweep templates"
```

---

## Task 7: `${VAULT}` 替换 → settings.local.json

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/settings.rs`

- [ ] **Step 1: 写测试 + 实现**

```rust
//! 模板的 .claude/settings.json 是可移植的(用 ${VAULT} 占位)。每次运行前把占位
//! 替换成真实 vault 绝对路径,生成 .claude/settings.local.json —— Claude Code
//! 原生的本地覆盖层,已在 vault .gitignore 里。
use std::path::Path;

/// 生成 settings.local.json。模板不存在时静默跳过(任务可以不带权限文件)。
pub fn materialize(task_dir: &Path, vault: &Path) -> std::io::Result<()> {
    let src = task_dir.join(".claude/settings.json");
    let Ok(body) = std::fs::read_to_string(&src) else { return Ok(()) };
    let out = substitute(&body, vault);
    let dst = task_dir.join(".claude/settings.local.json");
    if let Some(p) = dst.parent() { std::fs::create_dir_all(p)?; }
    std::fs::write(dst, out)
}

pub fn substitute(body: &str, vault: &Path) -> String {
    body.replace("${VAULT}", &vault.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_every_vault_placeholder() {
        let got = substitute(r#"["Read(${VAULT}/**)","Write(${VAULT}/a)"]"#, Path::new("/v/notes"));
        assert_eq!(got, r#"["Read(/v/notes/**)","Write(/v/notes/a)"]"#);
    }

    #[test]
    fn writes_a_local_override_next_to_the_template() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".claude")).unwrap();
        std::fs::write(d.path().join(".claude/settings.json"),
            r#"{"permissions":{"allow":["Read(${VAULT}/**)"]}}"#).unwrap();
        materialize(d.path(), Path::new("/v/notes")).unwrap();
        let got = std::fs::read_to_string(d.path().join(".claude/settings.local.json")).unwrap();
        assert!(got.contains("Read(/v/notes/**)"));
        assert!(!got.contains("${VAULT}"));
        // 模板本身不能被改写 —— 它要保持可移植。
        let tpl = std::fs::read_to_string(d.path().join(".claude/settings.json")).unwrap();
        assert!(tpl.contains("${VAULT}"));
    }

    #[test]
    fn is_a_no_op_when_the_task_has_no_settings_template() {
        let d = tempfile::tempdir().unwrap();
        materialize(d.path(), Path::new("/v")).unwrap();
        assert!(!d.path().join(".claude/settings.local.json").exists());
    }
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml settings`
Expected: 3 passed

- [ ] **Step 3: Commit**

```bash
git add plugins-src/claude-agent/backend/src/settings.rs
git commit -m "feat(claude-agent): materialize \${VAULT} into settings.local.json"
```

---

## Task 8: 任务锁

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/lock.rs`

- [ ] **Step 1: 写测试 + 实现**

```rust
//! 同任务互斥、跨任务并行。锁是任务运行目录下的一个 JSON 文件;进程崩溃留下的
//! 陈旧锁按 pid 存活判定自动回收(否则一次崩溃就永久锁死一个任务)。
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: i32,
    pub run_id: String,
    pub started_at: String,
}

#[derive(Debug)]
pub struct Busy(pub LockInfo);

/// 持有期间存在;drop 即释放(把锁交给 RAII,避免每条错误分支都要记得删)。
pub struct Guard { path: PathBuf }

impl Drop for Guard {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.path); }
}

pub fn lock_path(task_run_dir: &Path) -> PathBuf { task_run_dir.join("lock") }

pub fn acquire_with(
    task_run_dir: &Path,
    info: LockInfo,
    alive: impl Fn(i32) -> bool,
) -> Result<Guard, Busy> {
    let p = lock_path(task_run_dir);
    if let Ok(s) = std::fs::read_to_string(&p) {
        if let Ok(cur) = serde_json::from_str::<LockInfo>(&s) {
            if alive(cur.pid) { return Err(Busy(cur)); }
            // 陈旧锁:持有者已死,回收。
        }
    }
    let _ = std::fs::create_dir_all(task_run_dir);
    let _ = std::fs::write(&p, serde_json::to_string(&info).unwrap());
    Ok(Guard { path: p })
}

pub fn acquire(task_run_dir: &Path, info: LockInfo) -> Result<Guard, Busy> {
    acquire_with(task_run_dir, info, pid_alive)
}

/// `kill(pid, 0)` 是 POSIX 判定进程存活的标准做法:不发信号,只做权限与存在性检查。
pub fn pid_alive(pid: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pid: i32) -> LockInfo {
        LockInfo { pid, run_id: "r1".into(), started_at: "2026-07-30T00:00:00Z".into() }
    }

    #[test]
    fn acquires_on_a_clean_dir() {
        let d = tempfile::tempdir().unwrap();
        let g = acquire_with(d.path(), info(1), |_| true).unwrap();
        assert!(lock_path(d.path()).exists());
        drop(g);
        assert!(!lock_path(d.path()).exists());
    }

    #[test]
    fn refuses_when_the_holder_is_still_alive() {
        let d = tempfile::tempdir().unwrap();
        let _g = acquire_with(d.path(), info(4242), |_| true).unwrap();
        match acquire_with(d.path(), info(9999), |_| true) {
            Err(Busy(cur)) => assert_eq!(cur.pid, 4242),
            Ok(_) => panic!("expected the second acquire to be refused"),
        }
    }

    #[test]
    fn reclaims_a_stale_lock_whose_holder_died() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(lock_path(d.path()),
            serde_json::to_string(&info(4242)).unwrap()).unwrap();
        let _g = acquire_with(d.path(), info(9999), |_| false).unwrap();
        let cur: LockInfo = serde_json::from_str(
            &std::fs::read_to_string(lock_path(d.path())).unwrap()).unwrap();
        assert_eq!(cur.pid, 9999);
    }

    #[test]
    fn treats_a_corrupt_lock_file_as_reclaimable() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(lock_path(d.path()), "{garbage").unwrap();
        assert!(acquire_with(d.path(), info(1), |_| true).is_ok());
    }

    #[test]
    fn pid_alive_says_yes_for_our_own_process() {
        assert!(pid_alive(std::process::id() as i32));
    }
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml lock`
Expected: 5 passed

- [ ] **Step 3: Commit**

```bash
git add plugins-src/claude-agent/backend/src/lock.rs
git commit -m "feat(claude-agent): lock a task per run and reclaim stale locks"
```

---

## Task 9: 运行记录

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/record.rs`

- [ ] **Step 1: 写测试 + 实现**

```rust
//! 运行记录:每次运行一个 JSON 文件。全量事件流刻意不落盘 —— 那是给窗口看的,
//! 存下来只会给 vault 添噪音。
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const RESULT_LIMIT: usize = 8 * 1024;
pub const STDERR_LIMIT: usize = 2 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status { Success, Error, Timeout, Cancelled }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub task: String,
    pub trigger: String,               // "window" | "cli"
    pub started_at: String,
    pub ended_at: String,
    pub status: Status,
    pub exit_code: Option<i32>,
    pub num_turns: Option<u64>,
    pub session_id: Option<String>,
    pub result: String,
    pub stderr_tail: String,
}

pub fn runs_dir(task_run_dir: &Path) -> PathBuf { task_run_dir.join("runs") }

/// 保留尾部:失败原因通常在最后。按 char 边界截断,避免切碎 UTF-8。
pub fn tail(s: &str, limit: usize) -> String {
    if s.len() <= limit { return s.to_string() }
    let start = s.char_indices().rev()
        .take_while(|(i, _)| s.len() - i <= limit)
        .last().map(|(i, _)| i).unwrap_or(0);
    s[start..].to_string()
}

pub fn write(task_run_dir: &Path, rec: &RunRecord) -> std::io::Result<PathBuf> {
    let d = runs_dir(task_run_dir);
    std::fs::create_dir_all(&d)?;
    let p = d.join(format!("{}.json", rec.run_id));
    std::fs::write(&p, serde_json::to_string_pretty(rec).unwrap() + "\n")?;
    Ok(p)
}

/// 最近 N 条,新的在前(run_id 以 UTC 时间戳打头,字典序即时间序)。
pub fn recent(task_run_dir: &Path, n: usize) -> Vec<RunRecord> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(runs_dir(task_run_dir))
        .map(|rd| rd.flatten().map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json")).collect())
        .unwrap_or_default();
    files.sort();
    files.reverse();
    files.into_iter().take(n)
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect()
}

/// `20260730T104233Z-<pid 后 6 位十六进制>`:字典序 = 时间序,同秒不撞。
pub fn new_run_id(now: chrono::DateTime<chrono::Utc>, pid: u32) -> String {
    format!("{}-{:06x}", now.format("%Y%m%dT%H%M%SZ"), pid & 0xff_ffff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: &str) -> RunRecord {
        RunRecord {
            run_id: id.into(), task: "t".into(), trigger: "window".into(),
            started_at: "a".into(), ended_at: "b".into(), status: Status::Success,
            exit_code: Some(0), num_turns: Some(3), session_id: None,
            result: "ok".into(), stderr_tail: String::new(),
        }
    }

    #[test]
    fn round_trips_a_record_through_disk() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &rec("20260730T000000Z-000001")).unwrap();
        let got = recent(d.path(), 10);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].status, Status::Success);
        assert_eq!(got[0].result, "ok");
    }

    #[test]
    fn recent_returns_newest_first_and_respects_the_limit() {
        let d = tempfile::tempdir().unwrap();
        for id in ["20260730T000001Z-a", "20260730T000002Z-b", "20260730T000003Z-c"] {
            write(d.path(), &rec(id)).unwrap();
        }
        let got = recent(d.path(), 2);
        assert_eq!(got.iter().map(|r| r.run_id.as_str()).collect::<Vec<_>>(),
                   vec!["20260730T000003Z-c", "20260730T000002Z-b"]);
    }

    #[test]
    fn tail_keeps_the_end_and_never_splits_a_utf8_char() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("abc", 10), "abc");
        let s = "问题问题问题";           // 每字 3 字节
        let got = tail(s, 7);
        assert!(got.len() <= 7);
        assert!(s.ends_with(&got));       // 是合法的尾部切片,没切碎字符
    }

    #[test]
    fn run_ids_sort_chronologically() {
        use chrono::TimeZone;
        let a = new_run_id(chrono::Utc.with_ymd_and_hms(2026, 7, 30, 1, 0, 0).unwrap(), 1);
        let b = new_run_id(chrono::Utc.with_ymd_and_hms(2026, 7, 30, 2, 0, 0).unwrap(), 1);
        assert!(a < b);
    }

    #[test]
    fn recent_is_empty_when_nothing_ran_yet() {
        let d = tempfile::tempdir().unwrap();
        assert!(recent(d.path(), 5).is_empty());
    }
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml record`
Expected: 5 passed

- [ ] **Step 3: Commit**

```bash
git add plugins-src/claude-agent/backend/src/record.rs
git commit -m "feat(claude-agent): persist one JSON record per run"
```

---

## Task 10: 运行引擎(spawn / 泵事件 / 超时 / 取消)

两条链路共用。用假 `claude` 桩脚本做端到端。

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/engine.rs`

- [ ] **Step 1: 写引擎实现**

```rust
//! 运行引擎:起 claude、把 stream-json 泵成事件、处理超时与取消。窗口链路与
//! detached runner 共用它,差异只在谁持有子进程。
use crate::{lock, prompt, record, settings, stream, task::TaskDef};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

pub struct RunSpec {
    pub vault: PathBuf,
    pub task: TaskDef,
    pub task_dir: PathBuf,
    pub task_run_dir: PathBuf,
    pub claude: PathBuf,
    pub prompt: String,
    pub trigger: String,
    pub run_id: String,
    pub oauth_token: Option<String>,
}

/// 引擎向外发的每一步。窗口链路把它转成 `host.ui.post`;runner 模式只用终态。
#[derive(Debug)]
pub enum Step {
    Event(stream::Event),
    Done(record::RunRecord),
}

/// 跑一次。`cancel` 收到任意消息即终止子进程组。
/// 锁在函数内获取并持有到结束 —— 调用方不必管释放。
pub async fn run(
    spec: RunSpec,
    tx: mpsc::UnboundedSender<Step>,
    mut cancel: mpsc::Receiver<()>,
) -> Result<(), lock::Busy> {
    let started = chrono::Utc::now();
    let _guard = lock::acquire(&spec.task_run_dir, lock::LockInfo {
        pid: std::process::id() as i32,
        run_id: spec.run_id.clone(),
        started_at: started.to_rfc3339(),
    })?;

    let _ = settings::materialize(&spec.task_dir, &spec.vault);
    let argv = prompt::build_argv(&spec.task, &spec.prompt);

    let mut cmd = tokio::process::Command::new(&spec.claude);
    cmd.args(&argv)
        .current_dir(&spec.task_dir)          // cwd = 任务模板目录(见设计 §2.1)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(t) = &spec.oauth_token { cmd.env("CLAUDE_CODE_OAUTH_TOKEN", t); }
    // 自成进程组,这样超时/取消能一次干掉 claude 及它派生的所有子进程。
    unsafe { cmd.pre_exec(|| { libc::setsid(); Ok(()) }); }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(Step::Done(finish(&spec, started, record::Status::Error,
                None, None, String::new(), format!("spawn failed: {e}"))));
            return Ok(());
        }
    };
    let pgid = child.id().unwrap_or(0) as i32;

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let mut lines = BufReader::new(stdout).lines();

    // stderr 只留尾巴,不逐行转发(它是 claude 的诊断噪声,不是给用户看的)。
    let err_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let eb = err_buf.clone();
    tokio::spawn(async move {
        let mut el = BufReader::new(stderr).lines();
        while let Ok(Some(l)) = el.next_line().await {
            let mut g = eb.lock().unwrap();
            g.push_str(&l); g.push('\n');
            let t = record::tail(&g, record::STDERR_LIMIT * 2);
            *g = t;
        }
    });

    let mut final_result: Option<stream::RunResult> = None;
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(spec.task.timeout_seconds));
    tokio::pin!(deadline);
    let status = loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(l)) => {
                    if let Some(ev) = stream::parse_line(&l) {
                        if let stream::Event::Result(r) = &ev { final_result = Some(r.clone()); }
                        let _ = tx.send(Step::Event(ev));
                    }
                }
                _ => break None,                       // EOF 或读错:等 wait 拿退出码
            },
            _ = &mut deadline => { kill_group(pgid); break Some(record::Status::Timeout) }
            _ = cancel.recv() => { kill_group(pgid); break Some(record::Status::Cancelled) }
        }
    };

    let exit = child.wait().await.ok().and_then(|s| s.code());
    let stderr_tail = record::tail(&err_buf.lock().unwrap(), record::STDERR_LIMIT);
    let st = status.unwrap_or_else(|| match (&final_result, exit) {
        (Some(r), _) if r.is_error => record::Status::Error,
        (_, Some(0)) => record::Status::Success,
        _ => record::Status::Error,
    });
    let rec = finish(&spec, started, st, exit, final_result, stderr_tail, String::new());
    let _ = record::write(&spec.task_run_dir, &rec);
    let _ = tx.send(Step::Done(rec));
    Ok(())
}

fn kill_group(pgid: i32) {
    if pgid > 0 { unsafe { libc::killpg(pgid, libc::SIGTERM); } }
}

#[allow(clippy::too_many_arguments)]
fn finish(
    spec: &RunSpec,
    started: chrono::DateTime<chrono::Utc>,
    status: record::Status,
    exit_code: Option<i32>,
    result: Option<stream::RunResult>,
    stderr_tail: String,
    fallback_err: String,
) -> record::RunRecord {
    record::RunRecord {
        run_id: spec.run_id.clone(),
        task: spec.task.id.clone(),
        trigger: spec.trigger.clone(),
        started_at: started.to_rfc3339(),
        ended_at: chrono::Utc::now().to_rfc3339(),
        status,
        exit_code,
        num_turns: result.as_ref().and_then(|r| r.num_turns),
        session_id: result.as_ref().and_then(|r| r.session_id.clone()),
        result: record::tail(
            &result.map(|r| r.result).unwrap_or(fallback_err), record::RESULT_LIMIT),
        stderr_tail,
    }
}
```

- [ ] **Step 2: 写端到端测试(假 claude 桩)**

追加到 `engine.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskDef;

    /// 造一个可执行的假 claude。`body` 是 shell 脚本正文。
    fn fake_claude(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("fake-claude");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    fn spec(dir: &Path, claude: PathBuf, timeout: u64) -> RunSpec {
        let task_dir = dir.join("task");
        std::fs::create_dir_all(&task_dir).unwrap();
        RunSpec {
            vault: dir.to_path_buf(),
            task: TaskDef { id: "t".into(), name: "T".into(), description: String::new(),
                            prompt: "p".into(), max_turns: None, timeout_seconds: timeout,
                            model: None },
            task_dir,
            task_run_dir: dir.join("runs-t"),
            claude,
            prompt: "hi".into(),
            trigger: "window".into(),
            run_id: "20260730T000000Z-000001".into(),
            oauth_token: None,
        }
    }

    async fn drive(s: RunSpec) -> (Vec<stream::Event>, record::RunRecord) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_ctx, crx) = mpsc::channel(1);
        run(s, tx, crx).await.unwrap();
        let (mut evs, mut done) = (Vec::new(), None);
        while let Ok(step) = rx.try_recv() {
            match step { Step::Event(e) => evs.push(e), Step::Done(r) => done = Some(r) }
        }
        (evs, done.expect("engine must always emit Done"))
    }

    #[tokio::test]
    async fn streams_events_and_records_success() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(d.path(), concat!(
            r#"echo '{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"a.md"}}]}}'"#, "\n",
            r#"echo '{"type":"result","subtype":"success","result":"done","session_id":"s1","num_turns":2,"is_error":false}'"#));
        let (evs, rec) = drive(spec(d.path(), c, 30)).await;
        assert!(matches!(evs[0], stream::Event::ToolUse { .. }));
        assert_eq!(rec.status, record::Status::Success);
        assert_eq!(rec.result, "done");
        assert_eq!(rec.session_id.as_deref(), Some("s1"));
    }

    #[tokio::test]
    async fn is_error_true_records_a_failure() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(d.path(),
            r#"echo '{"type":"result","result":"nope","is_error":true}'"#);
        let (_e, rec) = drive(spec(d.path(), c, 30)).await;
        assert_eq!(rec.status, record::Status::Error);
        assert_eq!(rec.result, "nope");
    }

    #[tokio::test]
    async fn a_hung_claude_hits_the_timeout_and_is_killed() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(d.path(), "sleep 30");
        let (_e, rec) = drive(spec(d.path(), c, 1)).await;
        assert_eq!(rec.status, record::Status::Timeout);
    }

    #[tokio::test]
    async fn cancel_stops_a_running_task() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(d.path(), "sleep 30");
        let s = spec(d.path(), c, 60);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (ctx, crx) = mpsc::channel(1);
        let h = tokio::spawn(run(s, tx, crx));
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        ctx.send(()).await.unwrap();
        h.await.unwrap().unwrap();
        let mut done = None;
        while let Ok(step) = rx.try_recv() { if let Step::Done(r) = step { done = Some(r) } }
        assert_eq!(done.unwrap().status, record::Status::Cancelled);
    }

    #[tokio::test]
    async fn a_missing_claude_binary_records_an_error_instead_of_panicking() {
        let d = tempfile::tempdir().unwrap();
        let (_e, rec) = drive(spec(d.path(), d.path().join("nope"), 30)).await;
        assert_eq!(rec.status, record::Status::Error);
        assert!(rec.stderr_tail.contains("spawn failed"));
    }

    #[tokio::test]
    async fn the_same_task_cannot_run_twice_at_once() {
        let d = tempfile::tempdir().unwrap();
        let c = fake_claude(d.path(), "sleep 5");
        let s1 = spec(d.path(), c.clone(), 60);
        let (tx1, _rx1) = mpsc::unbounded_channel();
        let (_c1, cr1) = mpsc::channel(1);
        let h = tokio::spawn(run(s1, tx1, cr1));
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let (tx2, _rx2) = mpsc::unbounded_channel();
        let (_c2, cr2) = mpsc::channel(1);
        assert!(run(spec(d.path(), c, 60), tx2, cr2).await.is_err());
        h.abort();
    }
}
```

- [ ] **Step 3: 跑测试**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml engine`
Expected: 6 passed(超时那条约 1 秒,取消那条约 0.2 秒)

- [ ] **Step 4: Commit**

```bash
git add plugins-src/claude-agent/backend/src/engine.rs
git commit -m "feat(claude-agent): run claude with streaming, timeout and cancel"
```

---

## Task 11: 后端接线(窗口 RPC + 命令)

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/plugin.rs`

- [ ] **Step 1: 实现完整的 plugin.rs**

```rust
//! NotemdPlugin 实现。窗口 RPC 五个方法 + 菜单/CLI 命令。
//!
//! SDK 的 on_ui_request 是同步的、跑在协议读循环上,所以 `run.start` 只负责
//! 起 tokio 任务并立刻返回 run_id;事件由该任务经 host.ui_post 推给窗口。
use crate::{discover, engine, prompt, record, task};
use notemd_plugin_sdk as sdk;
use sdk::plugin_protocol as proto;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const WINDOW: &str = "main";

#[derive(Default)]
struct Inner {
    vault: Option<PathBuf>,
    /// run_id → 取消通道
    running: HashMap<String, mpsc::Sender<()>>,
}

pub struct ClaudeAgentPlugin {
    inner: Arc<Mutex<Inner>>,
    tab_context: Option<Value>,
}

impl ClaudeAgentPlugin {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(Inner::default())), tab_context: None }
    }
}

/// vault 根只能问宿主要(host.vault.info),之后一切文件操作走插件自己的 fs。
async fn vault_root(host: &sdk::Host) -> Option<PathBuf> {
    let v = host.request("host.vault.info", json!({})).await.ok()?;
    v.get("root")?.as_str().map(PathBuf::from)
}

impl sdk::NotemdPlugin for ClaudeAgentPlugin {
    fn activate(&mut self, host: &sdk::Host, _p: &proto::ActivateParams) -> Result<(), String> {
        let inner = self.inner.clone();
        let host2 = host.clone();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                if let Some(root) = vault_root(&host2).await {
                    let wrote = task::seed_builtin_templates(&root);
                    if !wrote.is_empty() {
                        host2.log_info(&format!("seeded task templates: {}", wrote.join(", ")));
                    }
                    task::ensure_gitignore(&root);
                    inner.lock().unwrap().vault = Some(root);
                } else {
                    host2.log_warn("no vault configured; claude-agent needs one");
                }
            })
        });
        Ok(())
    }

    fn deactivate(&mut self, _host: &sdk::Host) {
        // 进程要走了:先招呼所有在跑的任务取消,免得留下孤儿 claude。
        let running: Vec<_> = self.inner.lock().unwrap().running.drain().collect();
        for (_id, tx) in running { let _ = tx.try_send(()); }
    }

    fn execute_command(&mut self, host: &sdk::Host, params: &proto::ExecuteCommandParams)
        -> Result<Value, String> {
        match params.command.as_str() {
            // 开窗:记住此刻的 tab,窗口稍后用 context.get 取。
            "open" => {
                self.tab_context = params.context.get("tab").cloned();
                Ok(json!({ "success": true }))
            }
            // CLI:notemd agent <task> [-p …] [--wait]
            "run" => self.cli_run(host, &params.context),
            other => Err(format!("unknown command '{other}'")),
        }
    }

    fn on_ui_request(&mut self, host: &sdk::Host, method: &str, params: Value)
        -> Result<Value, String> {
        match method {
            "tasks.list" => {
                let vault = self.vault()?;
                Ok(json!({ "tasks": task::discover(&vault) }))
            }
            "context.get" => Ok(json!({ "tab": self.tab_context })),
            "run.start" => self.start(host, params, "window"),
            "run.cancel" => {
                let id = params.get("run_id").and_then(|v| v.as_str()).unwrap_or_default();
                let tx = self.inner.lock().unwrap().running.get(id).cloned();
                match tx { Some(t) => { let _ = t.try_send(()); Ok(json!({"ok": true})) }
                           None => Err(format!("run '{id}' is not running")) }
            }
            "history.list" => {
                let vault = self.vault()?;
                let t = params.get("task").and_then(|v| v.as_str()).unwrap_or_default();
                Ok(json!({ "runs": record::recent(&task::runs_root(&vault).join(t), 20) }))
            }
            other => Err(format!("unknown ui method '{other}'")),
        }
    }
}

impl ClaudeAgentPlugin {
    fn vault(&self) -> Result<PathBuf, String> {
        self.inner.lock().unwrap().vault.clone()
            .ok_or_else(|| "no vault configured".to_string())
    }

    /// 组装 RunSpec 并起后台任务;立即返回 run_id。
    fn start(&mut self, host: &sdk::Host, params: Value, trigger: &str) -> Result<Value, String> {
        let vault = self.vault()?;
        let task_id = params.get("task").and_then(|v| v.as_str())
            .ok_or("missing 'task'")?.to_string();
        let user_prompt = params.get("prompt").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let use_ctx = params.get("use_context").and_then(|v| v.as_bool()).unwrap_or(false);

        let task_dir = task::task_dir(&vault, &task_id);
        let mut def = task::read_task(&task_dir).ok_or(format!("unknown task '{task_id}'"))?;
        def.id = task_id.clone();

        let claude = discover::discover(std::env::var("NOTEMD_CLAUDE_BIN").ok().as_deref())
            .ok_or("claude executable not found — install Claude Code, or set NOTEMD_CLAUDE_BIN")?;

        let ctx = if use_ctx { self.tab_ctx() } else { None };
        let full = prompt::compose(&def.prompt, &user_prompt, ctx.as_ref());
        let run_id = record::new_run_id(chrono::Utc::now(), std::process::id());

        let spec = engine::RunSpec {
            vault: vault.clone(),
            task: def,
            task_dir,
            task_run_dir: task::runs_root(&vault).join(&task_id),
            claude,
            prompt: full,
            trigger: trigger.to_string(),
            run_id: run_id.clone(),
            oauth_token: std::env::var("CLAUDE_CODE_OAUTH_TOKEN").ok(),
        };

        let (tx, mut rx) = mpsc::unbounded_channel();
        let (ctx_tx, ctx_rx) = mpsc::channel(1);
        self.inner.lock().unwrap().running.insert(run_id.clone(), ctx_tx);

        let h = host.clone();
        let inner = self.inner.clone();
        let rid = run_id.clone();
        tokio::spawn(async move {
            let pump = {
                let h = h.clone();
                let rid = rid.clone();
                tokio::spawn(async move {
                    while let Some(step) = rx.recv().await {
                        match step {
                            engine::Step::Event(e) => h.ui_post(WINDOW,
                                json!({"kind": "event", "run_id": rid, "event": e})),
                            engine::Step::Done(r) => h.ui_post(WINDOW,
                                json!({"kind": "done", "run_id": rid, "record": r})),
                        }
                    }
                })
            };
            if let Err(busy) = engine::run(spec, tx, ctx_rx).await {
                h.ui_post(WINDOW, json!({"kind": "busy", "run_id": rid, "holder": busy.0}));
                h.toast("warn", "Task already running", Some(&busy.0.run_id));
            }
            let _ = pump.await;
            inner.lock().unwrap().running.remove(&rid);
        });
        Ok(json!({ "run_id": run_id }))
    }

    fn tab_ctx(&self) -> Option<prompt::TabContext> {
        let t = self.tab_context.as_ref()?;
        Some(prompt::TabContext {
            path: t.get("path").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
            selection: t.get("selection").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
        })
    }

    /// CLI 入口。默认 detach(见 Task 12);`--wait` 走同步路径。
    fn cli_run(&mut self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let task_id = context.pointer("/cli/args/task").and_then(|v| v.as_str())
            .ok_or("usage: notemd agent <task> [-p PROMPT] [--wait]")?.to_string();
        let p = context.pointer("/cli/flags/prompt").and_then(|v| v.as_str())
            .unwrap_or("").to_string();
        let wait = context.pointer("/cli/flags/wait").and_then(|v| v.as_bool()).unwrap_or(false);
        let params = json!({ "task": task_id, "prompt": p, "use_context": false });
        if wait {
            self.start(host, params, "cli")
        } else {
            crate::runner::spawn_detached(&self.vault()?, &task_id, &p)
        }
    }
}
```

**注意**:`context` 里 CLI 参数的确切形状要在实机验证时打日志确认(`host.log_info(&context.to_string())`),若与 `/cli/args/...` 不同则按实际路径调整 —— 这是宿主前端解析后注入的,不同版本可能是 `context.cli.args` 也可能扁平化。

- [ ] **Step 2: 编译**

Run: `cargo build --manifest-path plugins-src/claude-agent/backend/Cargo.toml`
Expected: 通过

- [ ] **Step 3: 跑全部后端测试**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml`
Expected: 全绿

- [ ] **Step 4: Commit**

```bash
git add plugins-src/claude-agent/backend/src/plugin.rs
git commit -m "feat(claude-agent): wire the window RPC and menu/CLI commands"
```

---

## Task 12: runner 模式(CLI detach)

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/runner.rs`

- [ ] **Step 1: 实现 runner**

```rust
//! CLI detach 路径。宿主的 CLI 子命令跑在一个**另起的无头 app 实例**里
//! (src-tauri/src/cli/runner.rs:82),单次 invoke 上限 300 秒,且实例退出会
//! 收走子进程 —— 对 sweep 这类长任务不够用。
//!
//! 所以默认把活儿交给自身二进制的 runner 模式:setsid 脱离进程组独立跑,
//! 插件立刻返回 run_id,无头实例可以干净退出。
use crate::{discover, engine, prompt, record, task};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub vault: PathBuf,
    pub task_id: String,
    pub prompt: String,
    pub run_id: String,
}

/// 起一个 detached runner。返回给 CLI 的 `{run_id, status:"started"}`。
pub fn spawn_detached(vault: &Path, task_id: &str, user_prompt: &str)
    -> Result<serde_json::Value, String> {
    let run_id = record::new_run_id(chrono::Utc::now(), std::process::id());
    let run_dir = task::runs_root(vault).join(task_id).join("pending").join(&run_id);
    std::fs::create_dir_all(&run_dir).map_err(|e| e.to_string())?;
    let req = Request {
        vault: vault.to_path_buf(), task_id: task_id.into(),
        prompt: user_prompt.into(), run_id: run_id.clone(),
    };
    std::fs::write(run_dir.join("request.json"),
        serde_json::to_string(&req).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--runner").arg(&run_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| { libc::setsid(); Ok(()) });
    }
    cmd.spawn().map_err(|e| format!("failed to start runner: {e}"))?;
    Ok(serde_json::json!({ "run_id": run_id, "status": "started" }))
}

/// runner 进程主体。返回进程退出码。
pub async fn run(run_dir: PathBuf) -> i32 {
    let Ok(body) = std::fs::read_to_string(run_dir.join("request.json")) else { return 2 };
    let Ok(req) = serde_json::from_str::<Request>(&body) else { return 2 };

    let task_dir = task::task_dir(&req.vault, &req.task_id);
    let Some(mut def) = task::read_task(&task_dir) else { return 2 };
    def.id = req.task_id.clone();
    let Some(claude) = discover::discover(std::env::var("NOTEMD_CLAUDE_BIN").ok().as_deref())
        else { return 3 };

    let spec = engine::RunSpec {
        vault: req.vault.clone(),
        prompt: prompt::compose(&def.prompt, &req.prompt, None),
        task: def,
        task_dir,
        task_run_dir: task::runs_root(&req.vault).join(&req.task_id),
        claude,
        trigger: "cli".into(),
        run_id: req.run_id.clone(),
        oauth_token: std::env::var("CLAUDE_CODE_OAUTH_TOKEN").ok(),
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_cancel_tx, cancel_rx) = mpsc::channel(1);
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let code = match engine::run(spec, tx, cancel_rx).await {
        Ok(()) => 0,
        Err(_busy) => 4,        // 同任务已在跑
    };
    let _ = drain.await;
    let _ = std::fs::remove_dir_all(&run_dir);
    code
}
```

- [ ] **Step 2: 写测试**

追加到 `runner.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_detached_writes_a_request_the_runner_can_read_back() {
        let v = tempfile::tempdir().unwrap();
        let out = spawn_detached(v.path(), "selfcheck", "extra").unwrap();
        let run_id = out["run_id"].as_str().unwrap();
        assert_eq!(out["status"], "started");
        let p = task::runs_root(v.path()).join("selfcheck").join("pending")
            .join(run_id).join("request.json");
        let req: Request = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        assert_eq!(req.task_id, "selfcheck");
        assert_eq!(req.prompt, "extra");
        assert_eq!(req.run_id, run_id);
    }

    #[tokio::test]
    async fn runner_exits_with_a_code_when_the_request_is_unreadable() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(run(d.path().to_path_buf()).await, 2);
    }

    #[tokio::test]
    async fn runner_refuses_an_unknown_task() {
        let d = tempfile::tempdir().unwrap();
        let v = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("request.json"), serde_json::to_string(&Request {
            vault: v.path().to_path_buf(), task_id: "nope".into(),
            prompt: String::new(), run_id: "r".into(),
        }).unwrap()).unwrap();
        assert_eq!(run(d.path().to_path_buf()).await, 2);
    }
}
```

注意第一个测试会真的 spawn 一个 runner 进程,它读到 `selfcheck` 任务不存在会立刻退出(码 2),无副作用。

- [ ] **Step 3: 跑测试**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml runner`
Expected: 3 passed

- [ ] **Step 4: Commit**

```bash
git add plugins-src/claude-agent/backend/src/runner.rs
git commit -m "feat(claude-agent): detach long CLI runs into a runner process"
```

---

## Task 13: 前端脚手架 + bridge + i18n

**Files:**
- Create: `plugins-src/claude-agent/package.json`
- Create: `plugins-src/claude-agent/vite.config.ts`
- Create: `plugins-src/claude-agent/tsconfig.json`
- Create: `plugins-src/claude-agent/vitest.config.ts`
- Create: `plugins-src/claude-agent/index.html`
- Create: `plugins-src/claude-agent/src/main.ts`
- Create: `plugins-src/claude-agent/src/lib/bridge.ts`
- Create: `plugins-src/claude-agent/src/lib/strings.ts`

- [ ] **Step 1: package.json**

```json
{
  "name": "claude-agent",
  "version": "1.0.0",
  "type": "module",
  "private": true,
  "scripts": {
    "build": "vite build",
    "check": "svelte-check --tsconfig ./tsconfig.json",
    "test": "vitest run",
    "preview": "vite preview"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^5",
    "svelte": "^5",
    "svelte-check": "^4",
    "typescript": "^5",
    "vite": "^6",
    "vitest": "^4",
    "jsdom": "^25"
  }
}
```

- [ ] **Step 2: vite.config.ts / vitest.config.ts / tsconfig.json / index.html**

`vite.config.ts` —— `base: './'` 是硬要求(宿主用 `plugin://<id>/…` 提供资源,绝对路径会挂):

```ts
import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

export default defineConfig({
  plugins: [svelte()],
  base: './',
  build: {
    target: 'safari15',
    minify: 'esbuild',
    sourcemap: false,
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: { input: { index: 'index.html' } },
  },
})
```

`vitest.config.ts`:

```ts
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: { environment: 'jsdom', include: ['src/**/*.test.ts'] },
})
```

`tsconfig.json`:照抄 `plugins-src/weekly-review/tsconfig.json`。

`index.html`:

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="color-scheme" content="light dark" />
    <title>Claude Agent</title>
  </head>
  <body>
    <div id="claude-agent-app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

`color-scheme: light dark` 是必须的:独立 Tauri 窗口不继承主程序 app.css,不声明会在深色模式下卡成浅色。

- [ ] **Step 3: bridge.ts(照抄 openclaw,改方法前缀注释)**

```ts
// 宿主注入的 window.notemd 桥。插件窗口没有 Tauri IPC:
//  1. UI → 后端进程:request('plugin.<method>')(宿主转成 ui.request)
//  2. 后端 → UI:host.ui_post 的 payload,经 onMessage 收
export interface NotemdBridge {
  pluginId: string
  locale: string
  theme: string
  request(method: string, params?: unknown): Promise<any>
  onMessage(cb: (payload: unknown) => void): void
}

declare global {
  interface Window { notemd: NotemdBridge }
}

export function bridge(): NotemdBridge {
  const b = window.notemd
  if (!b) throw new Error('window.notemd bridge missing (not in a plugin window)')
  return b
}

/** 调后端方法。`plugin.` 前缀让宿主路由到本插件进程。 */
export function request(method: string, params?: unknown): Promise<any> {
  return bridge().request('plugin.' + method, params)
}

export function onMessage(cb: (m: any) => void): void {
  bridge().onMessage((p) => cb(p))
}
```

- [ ] **Step 4: strings.ts(zh/en/ja/de)**

```ts
type Dict = Record<string, string>

const en: Dict = {
  'tasks.title': 'Tasks',
  'tasks.empty': 'No task templates yet.',
  'run.start': 'Run',
  'run.stop': 'Stop',
  'run.running': 'Running',
  'run.prompt.placeholder': 'What should Claude do this run? (optional)',
  'ctx.label': 'Context',
  'ctx.selection': '{n} chars selected',
  'history.title': 'Recent runs',
  'status.success': 'Success',
  'status.error': 'Failed',
  'status.timeout': 'Timed out',
  'status.cancelled': 'Stopped',
  'err.noClaude': 'Claude Code not found. Install it, then reopen this window.',
  'err.busy': 'That task is already running.',
}

const zh: Dict = {
  'tasks.title': '任务',
  'tasks.empty': '还没有任务模板。',
  'run.start': '运行',
  'run.stop': '停止',
  'run.running': '运行中',
  'run.prompt.placeholder': '这次要 Claude 做什么?(可留空)',
  'ctx.label': '上下文',
  'ctx.selection': '选中 {n} 字',
  'history.title': '最近运行',
  'status.success': '成功',
  'status.error': '失败',
  'status.timeout': '超时',
  'status.cancelled': '已停止',
  'err.noClaude': '没找到 Claude Code。装好之后重新打开这个窗口。',
  'err.busy': '这个任务已经在跑了。',
}

const ja: Dict = { /* 同键,日文;缺键自动回退 en */ }
const de: Dict = { /* 同键,德文;缺键自动回退 en */ }

const TABLES: Record<string, Dict> = { en, zh, ja, de }

export function t(locale: string, key: string, vars?: Record<string, string | number>): string {
  const table = TABLES[locale?.slice(0, 2)] ?? en
  let s = table[key] ?? en[key] ?? key
  if (vars) for (const [k, v] of Object.entries(vars)) s = s.replace(`{${k}}`, String(v))
  return s
}
```

ja/de 表按同样的键补全译文(不要留空对象 —— 缺键会静默回退英文,那样 UI 会中英混排)。

- [ ] **Step 5: main.ts**

```ts
import { mount } from 'svelte'
import App from './App.svelte'

mount(App, { target: document.getElementById('claude-agent-app')! })
```

- [ ] **Step 6: 装依赖并确认能构建(App.svelte 先写占位)**

Run: `pnpm install && pnpm --filter claude-agent build`
Expected: `dist/index.html` 生成

- [ ] **Step 7: Commit**

```bash
git add plugins-src/claude-agent/package.json plugins-src/claude-agent/vite.config.ts \
        plugins-src/claude-agent/vitest.config.ts plugins-src/claude-agent/tsconfig.json \
        plugins-src/claude-agent/index.html plugins-src/claude-agent/src pnpm-lock.yaml
git commit -m "feat(claude-agent): scaffold the plugin window"
```

---

## Task 14: 事件流 reducer(纯函数 + vitest)

**Files:**
- Create: `plugins-src/claude-agent/src/lib/events.ts`
- Create: `plugins-src/claude-agent/src/lib/events.test.ts`

- [ ] **Step 1: 写失败测试**

```ts
import { describe, it, expect } from 'vitest'
import { emptyView, reduce, type RunView } from './events'

describe('run view reducer', () => {
  it('starts idle', () => {
    expect(emptyView().status).toBe('idle')
    expect(emptyView().items).toEqual([])
  })

  it('appends tool calls as their own rows', () => {
    const v = reduce(emptyView(), {
      kind: 'event', run_id: 'r1',
      event: { kind: 'tool_use', name: 'Read', brief: 'a.md' },
    })
    expect(v.items).toEqual([{ type: 'tool', name: 'Read', brief: 'a.md' }])
  })

  it('merges consecutive text events into one row', () => {
    let v = reduce(emptyView(), { kind: 'event', run_id: 'r1', event: { kind: 'text', text: 'he' } })
    v = reduce(v, { kind: 'event', run_id: 'r1', event: { kind: 'text', text: 'llo' } })
    expect(v.items).toEqual([{ type: 'text', text: 'hello' }])
  })

  it('starts a new text row after a tool call', () => {
    let v = reduce(emptyView(), { kind: 'event', run_id: 'r1', event: { kind: 'text', text: 'a' } })
    v = reduce(v, { kind: 'event', run_id: 'r1', event: { kind: 'tool_use', name: 'Read', brief: '' } })
    v = reduce(v, { kind: 'event', run_id: 'r1', event: { kind: 'text', text: 'b' } })
    expect(v.items.map((i) => i.type)).toEqual(['text', 'tool', 'text'])
  })

  it('goes terminal on done and records the turn count', () => {
    const v = reduce({ ...emptyView(), status: 'running' }, {
      kind: 'done', run_id: 'r1',
      record: { status: 'success', num_turns: 7, result: 'done', run_id: 'r1' },
    })
    expect(v.status).toBe('success')
    expect(v.turns).toBe(7)
    expect(v.result).toBe('done')
  })

  it('ignores messages from a different run', () => {
    const before: RunView = { ...emptyView(), runId: 'r1', status: 'running' }
    const after = reduce(before, {
      kind: 'event', run_id: 'OTHER', event: { kind: 'text', text: 'stray' },
    })
    expect(after).toBe(before)
  })

  it('surfaces a busy rejection', () => {
    const v = reduce({ ...emptyView(), runId: 'r1', status: 'running' }, {
      kind: 'busy', run_id: 'r1', holder: { run_id: 'r0', pid: 1, started_at: 'x' },
    })
    expect(v.status).toBe('busy')
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm --filter claude-agent test`
Expected: FAIL —— `Cannot find module './events'`

- [ ] **Step 3: 实现 events.ts**

```ts
// 后端事件流 → 视图模型。纯函数,便于测试;Svelte 侧只负责渲染。
export type Item =
  | { type: 'text'; text: string }
  | { type: 'tool'; name: string; brief: string }

export type Status = 'idle' | 'running' | 'success' | 'error' | 'timeout' | 'cancelled' | 'busy'

export interface RunView {
  runId: string | null
  status: Status
  items: Item[]
  turns?: number
  result?: string
}

export function emptyView(): RunView {
  return { runId: null, status: 'idle', items: [] }
}

/** 后端 host.ui_post 推来的信封。 */
export type HostMessage =
  | { kind: 'event'; run_id: string; event: BackendEvent }
  | { kind: 'done'; run_id: string; record: { status: Status; num_turns?: number; result?: string } }
  | { kind: 'busy'; run_id: string; holder: unknown }

type BackendEvent =
  | { kind: 'text'; text: string }
  | { kind: 'tool_use'; name: string; brief: string }
  | { kind: 'system'; subtype: string }
  | { kind: 'result'; [k: string]: unknown }

export function reduce(view: RunView, msg: HostMessage): RunView {
  // 上一次运行的残留消息不该污染当前视图。
  if (view.runId && msg.run_id !== view.runId) return view

  if (msg.kind === 'busy') return { ...view, status: 'busy' }

  if (msg.kind === 'done') {
    return { ...view, status: msg.record.status,
             turns: msg.record.num_turns, result: msg.record.result }
  }

  const e = msg.event
  if (e.kind === 'text') {
    const last = view.items[view.items.length - 1]
    // 连续文本合成一行,否则流式输出会碎成几十条。
    const items = last?.type === 'text'
      ? [...view.items.slice(0, -1), { type: 'text' as const, text: last.text + e.text }]
      : [...view.items, { type: 'text' as const, text: e.text }]
    return { ...view, items }
  }
  if (e.kind === 'tool_use') {
    return { ...view, items: [...view.items, { type: 'tool', name: e.name, brief: e.brief }] }
  }
  return view
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm --filter claude-agent test`
Expected: 7 passed

- [ ] **Step 5: Commit**

```bash
git add plugins-src/claude-agent/src/lib/events.ts plugins-src/claude-agent/src/lib/events.test.ts
git commit -m "feat(claude-agent): reduce the backend event stream into a view model"
```

---

## Task 15: 窗口 UI

**Files:**
- Create: `plugins-src/claude-agent/src/App.svelte`
- Create: `plugins-src/claude-agent/src/components/TaskList.svelte`
- Create: `plugins-src/claude-agent/src/components/RunStream.svelte`
- Create: `plugins-src/claude-agent/src/components/HistoryList.svelte`

- [ ] **Step 1: TaskList.svelte**

```svelte
<script lang="ts">
  interface Task { id: string; name: string; description: string }
  let { tasks, selected, onselect }:
    { tasks: Task[]; selected: string | null; onselect: (id: string) => void } = $props()
</script>

<ul class="tasks">
  {#each tasks as t (t.id)}
    <li>
      <button class:active={t.id === selected} onclick={() => onselect(t.id)}>
        <span class="name">{t.name}</span>
        <span class="desc">{t.description}</span>
      </button>
    </li>
  {/each}
</ul>

<style>
  .tasks { list-style: none; margin: 0; padding: 0; }
  button {
    /* button 不继承 font-size/family —— 必须显式声明,否则在大字号下错位 */
    font: inherit; font-size: 13px;
    display: block; width: 100%; text-align: left; padding: 8px 10px;
    background: none; border: 0; border-radius: 6px; color: inherit; cursor: pointer;
  }
  button:hover { background: color-mix(in srgb, currentColor 8%, transparent); }
  button.active { background: color-mix(in srgb, currentColor 14%, transparent); }
  .name { display: block; font-weight: 600; }
  .desc { display: block; opacity: 0.65; font-size: 12px; }
</style>
```

- [ ] **Step 2: RunStream.svelte**

```svelte
<script lang="ts">
  import type { Item } from '../lib/events'
  let { items }: { items: Item[] } = $props()
  let box: HTMLDivElement | undefined = $state()
  // 新事件进来就贴着底,除非用户手动往上翻了。
  $effect(() => {
    void items.length
    if (box && box.scrollHeight - box.scrollTop - box.clientHeight < 80) {
      box.scrollTop = box.scrollHeight
    }
  })
</script>

<div class="stream" bind:this={box}>
  {#each items as it, i (i)}
    {#if it.type === 'tool'}
      <div class="tool"><span class="tname">{it.name}</span> <span class="brief">{it.brief}</span></div>
    {:else}
      <div class="text">{it.text}</div>
    {/if}
  {/each}
</div>

<style>
  .stream { flex: 1; overflow: auto; padding: 10px; font-size: 13px; line-height: 1.5; }
  .tool { font-family: ui-monospace, monospace; font-size: 12px; opacity: 0.8; padding: 2px 0; }
  .tname { font-weight: 600; }
  .brief { opacity: 0.7; }
  .text { white-space: pre-wrap; padding: 4px 0; }
</style>
```

- [ ] **Step 3: HistoryList.svelte**

```svelte
<script lang="ts">
  interface Rec { run_id: string; status: string; started_at: string; result: string }
  let { runs, label }: { runs: Rec[]; label: (k: string) => string } = $props()
</script>

<ul class="history">
  {#each runs as r (r.run_id)}
    <li>
      <span class="s s-{r.status}">{label('status.' + r.status)}</span>
      <span class="when">{r.started_at.slice(0, 16).replace('T', ' ')}</span>
      <span class="sum">{r.result.slice(0, 80)}</span>
    </li>
  {/each}
</ul>

<style>
  .history { list-style: none; margin: 0; padding: 0; font-size: 12px; }
  li { display: flex; gap: 8px; padding: 3px 0; align-items: baseline; }
  .s { font-weight: 600; }
  .s-error, .s-timeout { color: #d33; }
  .when { opacity: 0.6; }
  .sum { opacity: 0.75; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
```

- [ ] **Step 4: App.svelte(三栏 + 接线)**

```svelte
<script lang="ts">
  import { bridge, request, onMessage } from './lib/bridge'
  import { t } from './lib/strings'
  import { emptyView, reduce, type RunView, type HostMessage } from './lib/events'
  import TaskList from './components/TaskList.svelte'
  import RunStream from './components/RunStream.svelte'
  import HistoryList from './components/HistoryList.svelte'

  interface Task { id: string; name: string; description: string }

  const locale = bridge().locale
  const tr = (k: string, v?: Record<string, string | number>) => t(locale, k, v)

  let tasks: Task[] = $state([])
  let selected: string | null = $state(null)
  let userPrompt = $state('')
  let ctx: { path: string; selection: string } | null = $state(null)
  let useCtx = $state(true)
  let view: RunView = $state(emptyView())
  let history: any[] = $state([])
  let error = $state('')

  const running = $derived(view.status === 'running')

  onMessage((m: HostMessage) => { view = reduce(view, m) })

  async function load() {
    try {
      tasks = (await request('tasks.list')).tasks
      if (!selected && tasks.length) selected = tasks[0].id
      const c = await request('context.get')
      ctx = c.tab ? { path: c.tab.path ?? '', selection: c.tab.selection ?? '' } : null
      await loadHistory()
    } catch (e) { error = String(e) }
  }

  async function loadHistory() {
    if (!selected) return
    history = (await request('history.list', { task: selected })).runs
  }

  async function start() {
    if (!selected) return
    error = ''
    view = { ...emptyView(), status: 'running' }
    try {
      const r = await request('run.start',
        { task: selected, prompt: userPrompt, use_context: useCtx && !!ctx })
      view = { ...view, runId: r.run_id }
    } catch (e) { error = String(e); view = emptyView() }
  }

  async function stop() {
    if (view.runId) await request('run.cancel', { run_id: view.runId })
  }

  $effect(() => { void selected; loadHistory() })
  load()
</script>

<main>
  <aside>
    <h2>{tr('tasks.title')}</h2>
    {#if tasks.length === 0}<p class="empty">{tr('tasks.empty')}</p>{/if}
    <TaskList {tasks} {selected} onselect={(id) => (selected = id)} />
    <h2>{tr('history.title')}</h2>
    <HistoryList runs={history} label={tr} />
  </aside>

  <section>
    <header>
      <textarea bind:value={userPrompt} placeholder={tr('run.prompt.placeholder')}></textarea>
      {#if ctx}
        <label class="ctx">
          <input type="checkbox" bind:checked={useCtx} />
          {tr('ctx.label')}: {ctx.path.split('/').pop()}
          {#if ctx.selection}({tr('ctx.selection', { n: ctx.selection.length })}){/if}
        </label>
      {/if}
    </header>

    <RunStream items={view.items} />

    <footer>
      {#if error}<span class="err">{error}</span>{/if}
      {#if view.status !== 'idle' && view.status !== 'running'}
        <span class="st">{tr('status.' + view.status)}</span>
      {/if}
      {#if view.turns != null}<span class="turns">{view.turns} turns</span>{/if}
      {#if running}
        <button onclick={stop}>{tr('run.stop')}</button>
      {:else}
        <button class="primary" onclick={start} disabled={!selected}>{tr('run.start')}</button>
      {/if}
    </footer>
  </section>
</main>

<style>
  :global(body) { margin: 0; font-family: -apple-system, system-ui, sans-serif; }
  main { display: flex; height: 100vh; }
  aside { width: 240px; padding: 12px; overflow: auto;
          border-right: 1px solid color-mix(in srgb, currentColor 15%, transparent); }
  h2 { font-size: 11px; text-transform: uppercase; opacity: 0.55; margin: 12px 0 6px; }
  .empty { font-size: 12px; opacity: 0.6; }
  section { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  header { padding: 10px; border-bottom: 1px solid color-mix(in srgb, currentColor 12%, transparent); }
  textarea { width: 100%; box-sizing: border-box; min-height: 52px; resize: vertical;
             font: inherit; font-size: 13px; padding: 6px; border-radius: 6px;
             border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
             background: transparent; color: inherit; }
  .ctx { display: block; margin-top: 6px; font-size: 12px; opacity: 0.8; }
  footer { display: flex; align-items: center; gap: 10px; padding: 8px 10px;
           border-top: 1px solid color-mix(in srgb, currentColor 12%, transparent); }
  footer button { margin-left: auto; font: inherit; font-size: 13px; padding: 5px 14px;
                  border-radius: 6px; border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
                  background: transparent; color: inherit; cursor: pointer; }
  footer button.primary { background: color-mix(in srgb, currentColor 12%, transparent); font-weight: 600; }
  .err { color: #d33; font-size: 12px; }
  .st, .turns { font-size: 12px; opacity: 0.7; }
</style>
```

- [ ] **Step 5: 构建 + 类型检查**

Run: `pnpm --filter claude-agent build && pnpm --filter claude-agent check`
Expected: 构建成功;`check` 无 error

- [ ] **Step 6: Commit**

```bash
git add plugins-src/claude-agent/src
git commit -m "feat(claude-agent): build the three-pane run window"
```

---

## Task 16: dev-install 接线

**Files:**
- Modify: `scripts/dev-install-plugin.sh`

- [ ] **Step 1: 加 claude-agent 分支**

在参数白名单里加 `claude-agent`(两处 `case`/`for` 都要改),并在 `pos-log` 分支后追加:

```bash
elif [[ "$PLUGIN" == "claude-agent" ]]; then
  SRC="plugins-src/claude-agent"
  # 后端(当前架构)+ 独立 UI bundle。
  cargo build $([ "$PROFILE" = release ] && echo --release) \
    --manifest-path "$SRC/backend/Cargo.toml" --bin notemd-claude-agent
  pnpm --filter claude-agent build
  VERSION=$(node -e "console.log(require('./$SRC/manifest.v2.json').version)")
  DEST="$ROOT/notemd.claude-agent/$VERSION"
  rm -rf "$DEST"
  mkdir -p "$DEST/bin" "$DEST/ui"
  cp "$SRC/backend/target/$PROFILE/notemd-claude-agent" "$DEST/bin/notemd-claude-agent"
  cp -R "$SRC/dist/." "$DEST/ui/"
  cp "$SRC/manifest.v2.json" "$DEST/manifest.json"
  ln -sfn "$VERSION" "$ROOT/notemd.claude-agent/current"
  mark_installed "notemd.claude-agent" "$VERSION"
  echo "✓ installed notemd.claude-agent@$VERSION ($PROFILE, $(uname -m), backend + ui) → $DEST"
  echo "  open it:  Window menu ▸ \"Claude Agent…\""
```

- [ ] **Step 2: 在脚本尾部追加手动 E2E 清单**

```bash
# ---------------------------------------------------------------------------
# Manual E2E walkthrough — claude-agent:
#   1. scripts/dev-install-plugin.sh claude-agent
#   2. NOTEMD_PLUGINS_V2=1 pnpm tauri dev   (with a Vault configured)
#   3. Window ▸ "Claude Agent…" → 窗口打开,左栏有 selfcheck / annotation-sweep
#      两个任务(首次启动写进 <vault>/.notemd/agent-tasks/)。
#   4. 选 selfcheck → Run → 事件实时滚动(工具调用 + 文本),底部转终态。
#      <vault>/.notemd/agent-runs/selfcheck/runs/*.json 出现一条记录。
#   5. 选 annotation-sweep → Run → 跑到一半点 Stop → 状态变「已停止」,
#      `pgrep -f claude` 确认子进程已被收走。
#   6. CLI: `notemd agent selfcheck` → 立刻返回 run_id(detach);
#      几十秒后 runs/ 里出现 trigger:"cli" 的记录。
#   7. CLI: `notemd agent selfcheck --wait` → 同步等待并打印结果。
#   8. 两个任务同时跑 → 互不干扰;同一任务连点两次 Run → 第二次报「已在运行」。
# ---------------------------------------------------------------------------
```

- [ ] **Step 3: 跑一次装机**

Run: `scripts/dev-install-plugin.sh claude-agent`
Expected: `✓ installed notemd.claude-agent@1.0.0 …`

- [ ] **Step 4: Commit**

```bash
git add scripts/dev-install-plugin.sh
git commit -m "chore(claude-agent): dev-install the plugin"
```

---

## Task 17: 插件 README + 使用边界

**Files:**
- Create: `plugins-src/claude-agent/README.md`

- [ ] **Step 1: 写 README**

内容必须覆盖:

1. 这插件是什么(通用 headless 运行器,不是聊天)
2. 前置条件:装 Claude Code、本机登录订阅账号;找不到 `claude` 时用 `NOTEMD_CLAUDE_BIN` 指路
3. 任务模板怎么写:目录结构、`task.json` 全字段表、`${VAULT}` 占位、**三段 prompt 的固定拼接顺序**、cwd 定在任务目录(所以 vault 根 CLAUDE.md 与任务 CLAUDE.md 都会加载)
4. CLI 用法:`notemd agent <task> [-p …] [--wait]`,默认 detach 的原因
5. 并发规则:同任务互斥、跨任务并行
6. **使用边界**(照 `docs/2026-07-30-claude-headless-automation-implementation-plan.md` §3):`claude -p` 走订阅额度是官方允许的用法,仅供账号本人的自动化,不得包装成多用户服务

- [ ] **Step 2: Commit**

```bash
git add plugins-src/claude-agent/README.md
git commit -m "docs(claude-agent): document task templates and the usage boundary"
```

---

## Task 18: 全量验证

- [ ] **Step 1: 后端全测**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml`
Expected: 全绿(约 35 个测试)

- [ ] **Step 2: 前端测试 + 类型检查**

Run: `pnpm --filter claude-agent test && pnpm --filter claude-agent check`
Expected: 7 passed;check 无 error

- [ ] **Step 3: 仓库整体检查未被打破**

Run: `pnpm test && pnpm check`
Expected: 与本分支起点一致(新插件不应影响主程序测试)

- [ ] **Step 4: 手动 GUI 验收**

按 Task 16 Step 2 写进脚本的 8 条清单逐条走。**这一步必须由用户在真机上做** —— 本仓库约定 GUI 验证不做自动化。

- [ ] **Step 5: Commit(如有修复)**

---

## Self-Review 记录

对照 spec 逐节核查:

| Spec 章节 | 覆盖任务 |
|---|---|
| §1 形态与进程模型 | Task 1(manifest/骨架)、Task 11(窗口 RPC)、Task 12(CLI detach) |
| §2 vault 目录约定 | Task 5(发现)、Task 6(模板 + gitignore) |
| §2.1 cwd 定在任务目录 | Task 10(`current_dir(&spec.task_dir)`) |
| §2.2 `${VAULT}` 与 settings.local.json | Task 7 |
| §3 Prompt 组装 | Task 3 |
| §4 窗口 UI | Task 13、14、15 |
| §5 失败面 | Task 2(找不到 claude)、Task 8(锁/陈旧锁)、Task 10(超时/取消/spawn 失败/非零退出)、Task 11(deactivate 收子进程) |
| §6 内置模板 | Task 6 |
| §7 测试 | Task 2-10、14 的单测;Task 10 的假 claude 端到端;Task 18 的人工验收 |
| §8 使用边界 | Task 17 |

未覆盖项:限流(rate limit)只在 `result` 文本里原样呈现,不做特判 —— spec §5 明确"记进运行记录并提示,不自动重试",记录里已含完整 result 文本,窗口照常显示,无需额外代码。
