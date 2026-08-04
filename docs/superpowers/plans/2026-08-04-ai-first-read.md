# AI 先读(ebook 摘要 agent 任务 + host.agent/notify + 托盘全局提醒)实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ebook-import 队列中 done 的书籍行上加"AI 先读"按钮,异步经 claude-agent 生成 `YYYY-MM-DD-summary.md` 写到书目录,行内显示状态+耗时,完成后通过新的托盘全局提醒子系统通知,点击提醒直接打开摘要。

**Architecture:** 三层:①宿主新增 capability 门控的 `host.agent.run/status`(转发到 notemd.claude-agent 的新 `run-task`/既有 `run-status` 命令)与 `host.notify`(推入全局提醒注册表);②claude-agent 新增内置任务模板 `ai-read-ebook`;③ebook-import 后端维护 FIFO 串行队列 + 2s 轮询,窗口关掉后仍收尾并发提醒。托盘提醒 = `static` 注册表 + `tokio::sync::Notify` 信号 + Wry 守望任务重建菜单(菜单 API 须主线程,走 `run_on_main_thread`,仿 `rebuild_menu` lib.rs:1729)。

**Tech Stack:** Rust (Tauri 2 / tokio)、Svelte 5、vitest、serde_json。

**Spec:** `docs/superpowers/specs/2026-08-04-ai-first-read-design.md`

## Global Constraints

- **共享 worktree**:提交只精确 `git add <文件>`,绝不 `git add -A`。
- **托盘提醒态**:用 `tray.set_title(Some("N"))` 数字角标(有提醒时),不新增图标资源——与 spec §4 的"提醒态图标"偏离,已在 spec 中注记;图标四态逻辑不动。
- **进程通道约束**:`HostSink` 是同步闭包,在 tokio 读循环(process.rs:123)内被调;插件侧 `Host::request` 不能在 handler 里内联 await(SDK lib.rs:94-103,必须 `tokio::spawn`)。
- **i18n**:ebook-import 新文案进 `strings.ts` 四语目录(en/zh/ja/de),`strings.test.ts` 会强制:zh/ja/de 每键非空、无多余键、占位符一致、不得照抄英文;托盘菜单文案进 `menu_label` 四语表(lib.rs:1379)。
- **OKF**:摘要 frontmatter `type: Book Summary`,在 `src/lib/okf/concept.ts` 的 `CONCEPT_TYPE` 登记;`generated` actor 用 `<producer>/<version>` 形式,不得用 `human:`。
- **协议**:不改 `plugin-protocol` 结构体(capability 是裸 `Vec<String>`,新 host 方法参数用 `Value`),因此无需 `pnpm check:protocol` 再生成。
- **版本**:ebook-import `1.0.7→1.0.8` 且 `engines.notemd` 提到 `">=6.804.1"`(新 host API 门槛;宿主当日发 6.804.x);claude-agent `1.0.4→1.0.5`(engines 不动,新命令随包自带)。
- **验证命令**:宿主 `cargo test --manifest-path src-tauri/Cargo.toml`;插件后端各自 `cargo test --manifest-path plugins-src/<p>/backend/Cargo.toml`;前端 `pnpm -C plugins-src/ebook-import test`;主前端 `pnpm check && pnpm test`。
- **GUI**:不做 UI 自动化;dev 实机验证由用户执行(最后一个任务给手动步骤)。

---

### Task 1: 托盘提醒注册表(纯逻辑模块 + 全局静态)

**Files:**
- Create: `src-tauri/src/reminders.rs`
- Modify: `src-tauri/src/lib.rs`(仅 `mod reminders;` 一行,与现有 `mod` 声明并列)

**Interfaces:**
- Produces(后续任务依赖的精确签名):
  - `pub enum ReminderAction { OpenPath { path: String }, OpenPluginWindow { plugin_id: String, window: String } }`(serde tag=`kind`, snake_case)
  - `pub struct Reminder { pub id: u64, pub title: String, pub action: ReminderAction }`
  - `pub fn push(title: String, action: ReminderAction) -> u64` / `pub fn take(id: u64) -> Option<Reminder>` / `pub fn clear_all()` / `pub fn count() -> usize` / `pub fn snapshot() -> Vec<Reminder>`(全部作用于全局 `REGISTRY`,变更后 `DIRTY.notify_one()`)
  - `pub static DIRTY: LazyLock<tokio::sync::Notify>`(Task 2 的守望任务 await 它)
  - `pub fn parse_notify_params(v: &serde_json::Value) -> Result<(String, ReminderAction), String>`

- [ ] **Step 1: 写失败测试**

在新文件 `src-tauri/src/reminders.rs` 底部写测试(实现先只写空骨架让编译不过→测试红):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_assigns_increasing_ids_and_take_removes() {
        let mut r = Registry::default();
        let a = r.push("A".into(), ReminderAction::OpenPath { path: "/x/a.md".into() });
        let b = r.push("B".into(), ReminderAction::OpenPath { path: "/x/b.md".into() });
        assert!(b > a);
        assert_eq!(r.len(), 2);
        let got = r.take(a).expect("a exists");
        assert_eq!(got.title, "A");
        assert_eq!(r.len(), 1);
        assert!(r.take(a).is_none(), "second take is None");
    }

    #[test]
    fn clear_empties() {
        let mut r = Registry::default();
        r.push("A".into(), ReminderAction::OpenPath { path: "/x".into() });
        r.clear();
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn parse_open_path() {
        let (title, action) = parse_notify_params(&serde_json::json!({
            "title": "《书》AI 摘要已生成",
            "action": { "kind": "open_path", "path": "/v/ssot/ebooks/2026-08/书/2026-08-04-summary.md" }
        }))
        .unwrap();
        assert_eq!(title, "《书》AI 摘要已生成");
        assert_eq!(action, ReminderAction::OpenPath { path: "/v/ssot/ebooks/2026-08/书/2026-08-04-summary.md".into() });
    }

    #[test]
    fn parse_open_plugin_window() {
        let (_, action) = parse_notify_params(&serde_json::json!({
            "title": "失败",
            "action": { "kind": "open_plugin_window", "plugin_id": "notemd.claude-agent", "window": "main" }
        }))
        .unwrap();
        assert_eq!(action, ReminderAction::OpenPluginWindow { plugin_id: "notemd.claude-agent".into(), window: "main".into() });
    }

    #[test]
    fn parse_rejects_missing_title_or_bad_action() {
        assert!(parse_notify_params(&serde_json::json!({ "action": { "kind": "open_path", "path": "/x" } })).is_err());
        assert!(parse_notify_params(&serde_json::json!({ "title": "t" })).is_err());
        assert!(parse_notify_params(&serde_json::json!({ "title": "t", "action": { "kind": "nope" } })).is_err());
    }
}
```

- [ ] **Step 2: 实现模块**

```rust
//! 托盘全局提醒注册表。任何插件经 `host.notify`(capability `notify`)推入一条
//! 提醒;托盘出现「🔔 N」子菜单与数字角标,点击执行 action 并消掉该条。
//! 注册表是进程级全局(仿 plugin_runtime::STATE):HostServices 是泛型
//! `R: Runtime`,碰不了 Wry 专用的托盘刷新,所以写方只改数据 + 敲 DIRTY,
//! 由 lib.rs setup 里持有 Wry handle 的守望任务负责重建菜单。
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReminderAction {
    /// 绝对路径:聚焦主窗口并在编辑器打开该文件。
    OpenPath { path: String },
    /// 打开某插件贡献的窗口(失败提醒指向 claude-agent 的运行日志)。
    OpenPluginWindow { plugin_id: String, window: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reminder {
    pub id: u64,
    pub title: String,
    pub action: ReminderAction,
}

#[derive(Debug, Default)]
pub struct Registry {
    next_id: u64,
    items: Vec<Reminder>,
}

impl Registry {
    pub fn push(&mut self, title: String, action: ReminderAction) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Reminder { id, title, action });
        id
    }
    pub fn take(&mut self, id: u64) -> Option<Reminder> {
        let i = self.items.iter().position(|r| r.id == id)?;
        Some(self.items.remove(i))
    }
    pub fn clear(&mut self) {
        self.items.clear();
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn items(&self) -> &[Reminder] {
        &self.items
    }
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(Registry::default()));
/// 注册表变更信号。notify_one 在无等待者时存 permit,守望任务不会漏事件。
pub static DIRTY: LazyLock<tokio::sync::Notify> = LazyLock::new(tokio::sync::Notify::new);

pub fn push(title: String, action: ReminderAction) -> u64 {
    let id = REGISTRY.lock().unwrap().push(title, action);
    DIRTY.notify_one();
    id
}
pub fn take(id: u64) -> Option<Reminder> {
    let r = REGISTRY.lock().unwrap().take(id);
    if r.is_some() {
        DIRTY.notify_one();
    }
    r
}
pub fn clear_all() {
    REGISTRY.lock().unwrap().clear();
    DIRTY.notify_one();
}
pub fn count() -> usize {
    REGISTRY.lock().unwrap().len()
}
pub fn snapshot() -> Vec<Reminder> {
    REGISTRY.lock().unwrap().items().to_vec()
}

/// `host.notify` 参数 → (title, action)。
pub fn parse_notify_params(v: &serde_json::Value) -> Result<(String, ReminderAction), String> {
    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("host.notify needs a non-empty 'title'")?;
    let action = v.get("action").cloned().ok_or("host.notify needs an 'action'")?;
    let action: ReminderAction = serde_json::from_value(action).map_err(|e| format!("bad action: {e}"))?;
    Ok((title.to_string(), action))
}
```

在 `src-tauri/src/lib.rs` 现有 `mod` 声明区(`mod agents_sync;` 附近)加模块声明,并与相邻桌面端模块一样加 iOS 门控(消费方 `plugin_runtime` 也是同款门控):

```rust
#[cfg(not(target_os = "ios"))]
pub mod reminders;
```

- [ ] **Step 3: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml reminders`
Expected: 上述 5 个测试 PASS。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/reminders.rs src-tauri/src/lib.rs
git commit -m "feat(tray): 全局提醒注册表(纯逻辑 + static + DIRTY 信号)"
```

---

### Task 2: 托盘菜单/角标/点击动作接入

**Files:**
- Modify: `src-tauri/src/lib.rs`(`menu_label` ~1448、`build_tray_menu` ~1606-1665、`refresh_tray_status` ~788、`on_menu_event` ~1180-1213、setup 守望任务 ~1097 附近)

**Interfaces:**
- Consumes: Task 1 的 `reminders::{snapshot, take, clear_all, count, DIRTY, ReminderAction}`。
- Produces: `pub(crate) fn refresh_tray_reminders(app: &tauri::AppHandle)`——重建托盘菜单+刷角标,内部 `run_on_main_thread`,任何线程可调(Task 4 的 `host.notify` 落地不直接用它,守望任务用)。

- [ ] **Step 1: `menu_label` 加两个 key**(lib.rs:1456 `tray.editAgents` 行后):

```rust
"tray.reminders.title" => ("🔔 {n} notification(s)", "🔔 {n} 条提醒", "🔔 通知 {n} 件", "🔔 {n} Erinnerung(en)"),
"tray.reminders.clear" => ("Clear All Notifications", "清除全部提醒", "通知をすべてクリア", "Alle Erinnerungen löschen"),
```

- [ ] **Step 2: `build_tray_menu` 插入提醒子菜单**

在 `large_submenu` 构造(~lib.rs:1626)之后加:

```rust
    // 全局提醒子菜单(仅有提醒时出现)。大文件子菜单的同款样式。
    let reminder_items = crate::reminders::snapshot();
    let reminders_submenu = if reminder_items.is_empty() {
        None
    } else {
        let title = menu_label(locale, "tray.reminders.title")
            .replace("{n}", &reminder_items.len().to_string());
        let mut sub = SubmenuBuilder::with_id(app, "tray-reminders", &title);
        for r in &reminder_items {
            let it = MenuItem::with_id(app, format!("tray-reminder:{}", r.id), &r.title, true, None::<&str>)?;
            sub = sub.item(&it);
        }
        let clear = MenuItem::with_id(
            app, "tray-reminder-clear",
            menu_label(locale, "tray.reminders.clear"), true, None::<&str>,
        )?;
        sub = sub.separator().item(&clear);
        Some(sub.build()?)
    };
```

并在装配处(`if let Some(ref sm) = large_submenu { b2 = b2.item(sm); }` 之后)加:

```rust
    if let Some(ref rm) = reminders_submenu {
        b2 = b2.item(rm);
    }
```

- [ ] **Step 3: `refresh_tray_status` 角标**

把 lib.rs:788 的 `let _ = tray.set_title(None::<&str>);` 替换为:

```rust
        // 有未读提醒时在图标旁挂数字角标;否则保持纯图标。
        let n_reminders = crate::reminders::count();
        if n_reminders > 0 {
            let _ = tray.set_title(Some(n_reminders.to_string()));
        } else {
            let _ = tray.set_title(None::<&str>);
        }
```

- [ ] **Step 4: `refresh_tray_reminders` 助手 + setup 守望任务**

在 `refresh_tray_status` 后新增(注意仿 lib.rs:807-819 的重建块,菜单 API 必须主线程,守望任务在 tokio 线程,故整体裹 `run_on_main_thread`):

```rust
/// 提醒集变更后的托盘刷新:重建下拉菜单(让 🔔 子菜单进出)+ 刷新角标。
/// 任意线程可调;真正的菜单操作 hop 到主线程(macOS 菜单 API 要求,同
/// rebuild_menu 的做法)。
#[cfg(not(target_os = "ios"))]
pub(crate) fn refresh_tray_reminders(app: &tauri::AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let locale = read_saved_locale(&handle);
        if let Some(tray) = handle.tray_by_id("main") {
            if let Ok((menu, repo_item, status_item, sync_now_item)) = build_tray_menu(&handle, &locale) {
                *handle.state::<TrayRepoItem>().0.lock().unwrap() = Some(repo_item);
                *handle.state::<TrayStatusItem>().0.lock().unwrap() = Some(status_item);
                *handle.state::<TraySyncNowItem>().0.lock().unwrap() = Some(sync_now_item);
                let _ = tray.set_menu(Some(menu));
            }
        }
        refresh_tray_status(&handle);
    });
}
```

setup 里(`app.manage(vault_mgr);` lib.rs:1097 之后、托盘构建之后任意点)加守望任务:

```rust
            // 提醒注册表守望:任何 reminders::push/take/clear 敲响 DIRTY 后刷新托盘。
            #[cfg(not(target_os = "ios"))]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        crate::reminders::DIRTY.notified().await;
                        refresh_tray_reminders(&handle);
                    }
                });
            }
```

- [ ] **Step 5: `on_menu_event` 两个 arm**

在 tray 的 `on_menu_event`(lib.rs:1180)`"tray-quit"` arm 之后加:

```rust
                            "tray-reminder-clear" => {
                                crate::reminders::clear_all();
                                // DIRTY 守望会刷新;这里无需手动调用。
                            }
                            id if id.starts_with("tray-reminder:") => {
                                let r = id.strip_prefix("tray-reminder:")
                                    .and_then(|s| s.parse::<u64>().ok())
                                    .and_then(crate::reminders::take);
                                if let Some(r) = r {
                                    match r.action {
                                        crate::reminders::ReminderAction::OpenPath { path } => {
                                            emit_open_file_delayed(app, &path);
                                            show_main_window(app);
                                        }
                                        crate::reminders::ReminderAction::OpenPluginWindow { plugin_id, window } => {
                                            let _ = crate::plugin_runtime::windows::open_plugin_window(app, &plugin_id, &window);
                                        }
                                    }
                                }
                            }
```

(`emit_open_file_delayed` + `show_main_window` 即 `TauriServices::open_in_editor` ui_rpc.rs:690-696 的同款组合。)

- [ ] **Step 6: 编译 + 全量宿主测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 全部 PASS(本任务无新单测,菜单逻辑靠 Task 9 的 GUI 实机验证)。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(tray): 提醒子菜单 + 数字角标 + 点击动作 + DIRTY 守望刷新"
```

---

### Task 3: `host.agent.*` / `host.notify` 宿主方法(capability 门控 + 双通道接线)

**Files:**
- Modify: `src-tauri/src/plugin_runtime/host_api.rs`(`method_capability` :32、`make_sink` services 分支 :175、表测试 :488)
- Modify: `src-tauri/src/plugin_runtime/ui_rpc.rs`(`HostServices` trait :147、`dispatch_with` match :305、既有 ServicesStub 们)

**Interfaces:**
- Produces(Task 4/7 依赖):
  - capability 名:`agent`(host.agent.run/status)、`notify`(host.notify)。
  - `HostServices` 新增默认方法:
    `fn agent_execute(&self, _command: &str, _context: serde_json::Value) -> Result<serde_json::Value, String> { Err("agent_unavailable: no relay on this channel".into()) }`
    `fn notify_user(&self, _params: &serde_json::Value) -> Result<serde_json::Value, String> { Err("notify not supported here".into()) }`
  - 方法→命令映射:`host.agent.run` → `agent_execute("run-task", params)`;`host.agent.status` → `agent_execute("run-status", params)`;`host.notify` → `notify_user(&params)`。参数原样透传,结果原样返回。

- [ ] **Step 1: 写失败测试**

host_api.rs 表测试(`method_capability_table` :488)追加断言:

```rust
        assert_eq!(method_capability("host.agent.run"), Some("agent"));
        assert_eq!(method_capability("host.agent.status"), Some("agent"));
        assert_eq!(method_capability("host.notify"), Some("notify"));
```

host_api.rs tests 里给 `ServicesStub`(:667)加记录型覆写,并新增进程通道测试:

```rust
    // ServicesStub 增加字段与覆写(结构体改为:)
    struct ServicesStub(std::path::PathBuf, Arc<Mutex<Vec<(String, serde_json::Value)>>>);
    // 原有 trait 方法保持;新增:
    impl crate::plugin_runtime::ui_rpc::HostServices for ServicesStub {
        // …原有 pick_paths/pick_save/vault_root/wiki_daily_dirs/clipboard_write 不变…
        fn agent_execute(&self, command: &str, context: serde_json::Value) -> Result<serde_json::Value, String> {
            self.1.lock().unwrap().push((command.to_string(), context));
            Ok(serde_json::json!({ "run_id": "r-test" }))
        }
        fn notify_user(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
            self.1.lock().unwrap().push(("notify".into(), params.clone()));
            Ok(serde_json::json!({ "ok": true, "id": 1 }))
        }
    }

    #[test]
    fn agent_run_on_process_channel_relays_run_task_with_capability() {
        let log_dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = make_sink(
            "pub.test".into(),
            vec!["agent".into()],
            log_dir.path().to_path_buf(),
            recording_emitter().0,
            noop_poster(),
            Some(Arc::new(ServicesStub(std::env::temp_dir(), seen.clone()))),
        );
        let resp = sink(req(
            "host.agent.run",
            Some(1),
            serde_json::json!({"task": "ai-read-ebook", "prompt": "p", "note_path": "/v/b/book.md"}),
        ))
        .unwrap();
        assert_eq!(resp.result.unwrap()["run_id"], "r-test");
        let calls = seen.lock().unwrap();
        assert_eq!(calls[0].0, "run-task");
        assert_eq!(calls[0].1["task"], "ai-read-ebook");
    }

    #[test]
    fn agent_and_notify_without_capability_are_denied() {
        let log_dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = make_sink(
            "pub.test".into(),
            vec![], // 无 agent/notify
            log_dir.path().to_path_buf(),
            recording_emitter().0,
            noop_poster(),
            Some(Arc::new(ServicesStub(std::env::temp_dir(), seen.clone()))),
        );
        let resp = sink(req("host.agent.run", Some(1), serde_json::json!({}))).unwrap();
        assert_eq!(resp.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);
        let resp = sink(req("host.notify", Some(2), serde_json::json!({}))).unwrap();
        assert_eq!(resp.error.unwrap().code, proto::ERR_CAPABILITY_DENIED);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[test]
    fn agent_status_relays_run_status() {
        let log_dir = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = make_sink(
            "pub.test".into(),
            vec!["agent".into(), "notify".into()],
            log_dir.path().to_path_buf(),
            recording_emitter().0,
            noop_poster(),
            Some(Arc::new(ServicesStub(std::env::temp_dir(), seen.clone()))),
        );
        sink(req("host.agent.status", Some(1), serde_json::json!({"task": "ai-read-ebook", "run_id": "r1"})));
        sink(req("host.notify", Some(2), serde_json::json!({"title": "t", "action": {"kind": "open_path", "path": "/x"}})));
        let calls = seen.lock().unwrap();
        assert_eq!(calls[0].0, "run-status");
        assert_eq!(calls[1].0, "notify");
    }
```

注意:既有 `ServicesStub` 的两处使用(`vault_round_trip_…` :703、:745)构造参数要跟着补第二个字段。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml host_api`
Expected: FAIL(未知方法 -32601 / 断言不匹配)。

- [ ] **Step 3: 实现**

host_api.rs `method_capability`(:48 `host.location.get` 后):

```rust
        // AI agent 中转(转发到 notemd.claude-agent)与托盘全局提醒。
        "host.agent.run" | "host.agent.status" => Some("agent"),
        "host.notify" => Some("notify"),
```

host_api.rs `make_sink` 的 services 分支 match(:175-183,`host.location.get` 行后)加:

```rust
                                "host.agent.run" => Some(s.agent_execute("run-task", req.params.clone())),
                                "host.agent.status" => Some(s.agent_execute("run-status", req.params.clone())),
                                "host.notify" => Some(s.notify_user(&req.params)),
```

ui_rpc.rs `HostServices` trait(:147,`open_in_editor` 后)加上文 Interfaces 里的两个默认方法(带注释:默认不可用,生产实现只在 `TauriServices`)。

ui_rpc.rs `dispatch_with` match(:305,`host.editor.open` 行后)加:

```rust
        "host.agent.run"    => services.agent_execute("run-task", req.params.clone()),
        "host.agent.status" => services.agent_execute("run-status", req.params.clone()),
        "host.notify"       => services.notify_user(&req.params),
```

ui_rpc.rs 自己的测试 stub(搜 `impl` + `HostServices` 的测试实现)无需覆写新方法(有默认);若其 capability 表测试(:812-822)硬编码了全表,补 `agent`/`notify` 两行。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml plugin_runtime`
Expected: 新增 3 测试 + 存量全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/plugin_runtime/host_api.rs src-tauri/src/plugin_runtime/ui_rpc.rs
git commit -m "feat(plugin-host): host.agent.run/status(capability agent)与 host.notify(capability notify)"
```

---

### Task 4: `TauriServices` 生产实现(agent 中转 + 提醒落地)

**Files:**
- Modify: `src-tauri/src/plugin_runtime/ui_rpc.rs`(`impl HostServices for TauriServices` :607-698 附近)

**Interfaces:**
- Consumes: Task 1 `reminders::{parse_notify_params, push}`;Task 3 trait 方法;既有 `commands::get_or_register`(commands.rs:412)、`lifecycle::Trigger::Command`、`lc.ensure_active`/`lc.execute`(lifecycle.rs:251)。
- Produces: 无新接口(trait 的生产实现)。

- [ ] **Step 1: 实现两个方法**

`impl<R: tauri::Runtime> HostServices for TauriServices<R>` 里(`open_in_editor` 后)加:

```rust
    /// host.agent.* 中转:同步 trait 方法,但 lifecycle 是 async——spawn 到
    /// tauri 异步运行时,std channel 等结果。会阻塞调用线程(进程通道 = 该
    /// 插件的协议读循环;UI 通道 = 一个 tokio worker)最多 30s;claude-agent
    /// 的 run-task/run-status 都是"登记即返回",实际耗时毫秒级,30s 只兜
    /// 冷启动(ensure_active 首次拉起插件进程)。
    fn agent_execute(&self, command: &str, context: serde_json::Value) -> Result<serde_json::Value, String> {
        const AGENT_PLUGIN: &str = "notemd.claude-agent";
        let app = self.app.clone();
        let command = command.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        tauri::async_runtime::spawn(async move {
            let out = async {
                let lc = super::commands::get_or_register(&app, AGENT_PLUGIN)
                    .map_err(|e| format!("agent_unavailable: {e}"))?;
                lc.ensure_active(&super::lifecycle::Trigger::Command(command.clone()))
                    .await
                    .map_err(|e| format!("agent_unavailable: {e}"))?;
                lc.execute(plugin_protocol::ExecuteCommandParams { command, context }).await
            }
            .await;
            let _ = tx.send(out);
        });
        rx.recv_timeout(std::time::Duration::from_secs(30))
            .unwrap_or_else(|_| Err("agent relay timeout".into()))
    }

    fn notify_user(&self, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        let (title, action) = crate::reminders::parse_notify_params(params)?;
        let id = crate::reminders::push(title, action);
        Ok(serde_json::json!({ "ok": true, "id": id }))
    }
```

(`reminders::push` 只碰全局注册表 + 敲 DIRTY——泛型 `R` 下可编译;托盘刷新由 Task 2 的守望任务做。claude-agent 未安装时 `get_or_register` 报 `unknown v2 plugin`,带上 `agent_unavailable:` 前缀返回给调用方。)

- [ ] **Step 2: 编译 + 全量测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 全部 PASS(Task 3 的 stub 测试已覆盖分发;本实现为 Tauri 粘合,单测不可行)。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/plugin_runtime/ui_rpc.rs
git commit -m "feat(plugin-host): TauriServices 实现 agent 中转与提醒落地"
```

---

### Task 5: claude-agent 新命令 `run-task`

**Files:**
- Modify: `plugins-src/claude-agent/backend/src/plugin.rs`(`execute_command` :232、新函数、测试区)
- Modify: `plugins-src/claude-agent/manifest.v2.json`(activation.events、version)

**Interfaces:**
- Consumes: 既有 `self.start(host, params, trigger)`(plugin.rs:359,params 含 `task`/`prompt`/`use_context`/`note_path`,返回 `{run_id}`)。
- Produces: 命令 `run-task`,context `{task: String, prompt?: String, note_path?: String}` → `{run_id: String}`。`note_path` 非空时经既有 scoped-policy 机制限权。

- [ ] **Step 1: 写失败测试**

plugin.rs 测试区(参考 :683 附近的既有测试风格)加:

```rust
    #[tokio::test]
    async fn run_task_requires_a_task_id() {
        let (mut p, host, _tmp) = plugin_with_vault().await; // 复用本文件既有的测试构造 helper;
        // 若无现成 helper,则仿 :683 附近测试的搭建方式:临时 vault + activate。
        let err = p
            .execute_command(&host, &proto::ExecuteCommandParams {
                command: "run-task".into(),
                context: serde_json::json!({ "prompt": "x" }),
            })
            .unwrap_err();
        assert!(err.contains("'task'"), "err: {err}");
    }
```

(以文件里既有测试基建为准:如果现有测试直接构造 `ClaudeAgentPlugin` + 假 host,就照抄那套;断言点只有一个——缺 `task` 报错文案含 `'task'`。)

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml run_task`
Expected: FAIL with `unknown command 'run-task'`。

- [ ] **Step 3: 实现**

`execute_command` match(:246 `"run-note"` 行后)加:

```rust
            // 宿主 host.agent.run 中转:任意任务 + 调用方拼好的定位 prompt。
            "run-task" => self.run_task(host, &params.context),
```

`run_note` 后新函数:

```rust
    /// Run any task with a caller-composed prompt — the host relays
    /// `host.agent.run` here. `note_path` (optional) scopes permissions to
    /// that one file via the task's settings.scoped.json, same as run-note.
    fn run_task(&mut self, host: &sdk::Host, context: &Value) -> Result<Value, String> {
        let task_id = context
            .get("task")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("run-task needs a 'task'")?;
        let prompt = context.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let note_path = context.get("note_path").and_then(|v| v.as_str()).unwrap_or("");
        host.log_info(&format!("run-task {task_id}"));
        self.start(
            host,
            json!({
                "task": task_id,
                "prompt": prompt,
                "use_context": false,
                "note_path": note_path,
            }),
            "relay",
        )
    }
```

manifest.v2.json:`activation.events` 加 `"onCommand:run-task"`;`"version": "1.0.4"` → `"1.0.5"`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml`
Expected: 全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add plugins-src/claude-agent/backend/src/plugin.rs plugins-src/claude-agent/manifest.v2.json
git commit -m "feat(claude-agent): run-task 命令(宿主 host.agent.run 的落点)"
```

---

### Task 6: 内置任务模板 `ai-read-ebook` + OKF type 登记

**Files:**
- Create: `plugins-src/claude-agent/backend/templates/ai-read-ebook/{task.json,CLAUDE.md,settings.json,settings.scoped.json}`
- Modify: `plugins-src/claude-agent/backend/src/task.rs`(`BUILTIN` :73、测试 :264/:270/:320/:368)、`plugins-src/claude-agent/backend/src/plugin.rs`(:688 id 断言)
- Modify: `src/lib/okf/concept.ts`(`CONCEPT_TYPE` :12)

**Interfaces:**
- Produces: 任务 id `ai-read-ebook`(Task 7 的 `TASK_ID` 常量必须一致);摘要 frontmatter `type: Book Summary`。

- [ ] **Step 1: 更新既有断言(先红)**

- task.rs:264 `assert_eq!(wrote.len(), 8, …)` → `12`,注释改 `// 3 files each + answer-note-question's precheck + ai-read-ebook's 4 files.`
- task.rs:270、task.rs:320 → `assert_eq!(ids, vec!["ai-read-ebook", "answer-note-question", "selfcheck"]);`
- task.rs:368 `assert_eq!(tasks.len(), 2)` → `3`
- plugin.rs:688 → `assert_eq!(ids, vec!["ai-read-ebook", NOTE_TASK, "selfcheck"]);`

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml`
Expected: FAIL(模板还不存在)。

- [ ] **Step 2: 写四个模板文件**

`templates/ai-read-ebook/task.json`:

```json
{
  "name": "AI 阅读电子书",
  "description": "Read an imported book's book.md and write a date-stamped outline digest beside it.",
  "prompt": "根据 book.md,生成全书的大纲摘要,让我一眼能看到讲的是什么,按照我可能关注的方式,推荐优先阅读,突出核心观点和洞察,突出反常识的信息,生成简要的 md,以方便我后续追问阅读。输出位置、文件名与 frontmatter 严格遵循 CLAUDE.md 的协议。",
  "max_turns": 80,
  "timeout_seconds": 1800
}
```

`templates/ai-read-ebook/CLAUDE.md`:

```markdown
# 任务:AI 阅读电子书

你在 note.md 的 Claude Agent 插件里以 headless 模式运行,vault 根在 `${VAULT}`。

调用方会在追加的 prompt 里指明本次要读的书(`<书目录>/book.md`)和摘要目标文件
(`<书目录>/<YYYY-MM-DD>-summary.md`)。只读这本书,只写这一个摘要文件。

## 协议(逐条遵守)

1. 通读 book.md。大部头分段读完整本 —— 不要只读开头就下结论。
2. 摘要的目标是让读者一眼看清这本书讲什么,并能据此追问:
   - 全书大纲:按书的结构给出各部分在讲什么;
   - 「推荐优先阅读」:按读者可能关注的方式,点出最值得先读的章节/段落及理由;
   - 「核心观点与洞察」:提炼立得住的主张,注明出处章节;
   - 「反常识」:单列一节,收录与直觉相悖、颠覆常见认知的信息;
   - 简要,不复述细节;语言跟随书的语言。
3. 写到调用方指定的目标文件;同名文件已存在则直接覆盖。
4. 摘要文件必须以 OKF frontmatter 开头(YAML 必须可解析,标题含冒号等要双引号):

   ```
   ---
   type: Book Summary
   title: "<书名> — 摘要"
   generated:
     - by: claude-code/<你的模型名,如 opus-5>
       at: <目标文件名里的日期,YYYY-MM-DD>
   sources:
     - resource: book.md
   ---
   ```

   `by` 必须是 `<producer>/<version>` 形式(裸名不合规);这是 AI 生成内容(✦),
   不得使用 `human:` 前缀。
5. 除该摘要文件外,不要创建或修改任何文件;不要改 book.md。
```

`templates/ai-read-ebook/settings.json`(兜底:无 note_path 时,如用户从 claude-agent 窗口手动跑):

```json
{
  "permissions": {
    "allow": [
      "Read(${VAULT}/**)",
      "Write(${VAULT}/**/*-summary.md)",
      "Edit(${VAULT}/**/*-summary.md)"
    ],
    "deny": ["Bash", "Task", "WebSearch", "WebFetch"]
  }
}
```

`templates/ai-read-ebook/settings.scoped.json`(ebook-import 传 note_path=book.md 时生效;`${NOTE}`/`${SOURCE}` 都会替换成 book.md 的绝对路径,见 settings.rs:24-32):

```json
{
  "permissions": {
    "allow": [
      "Read(${NOTE})",
      "Read(${SOURCE})",
      "Write(${VAULT}/**/*-summary.md)",
      "Edit(${VAULT}/**/*-summary.md)"
    ],
    "deny": ["Bash", "Grep", "Glob", "Task", "WebSearch", "WebFetch"]
  }
}
```

- [ ] **Step 3: 登记 `BUILTIN`**

task.rs `BUILTIN`(:73)追加第三个元组(磁盘上源文件是平铺的,种子目标带 `.claude/` 前缀——同 answer-note-question 的写法):

```rust
    (
        "ai-read-ebook",
        &[
            ("task.json", include_str!("../templates/ai-read-ebook/task.json")),
            ("CLAUDE.md", include_str!("../templates/ai-read-ebook/CLAUDE.md")),
            (
                ".claude/settings.json",
                include_str!("../templates/ai-read-ebook/settings.json"),
            ),
            (
                ".claude/settings.scoped.json",
                include_str!("../templates/ai-read-ebook/settings.scoped.json"),
            ),
        ],
    ),
```

- [ ] **Step 4: `concept.ts` 登记**

`CONCEPT_TYPE`(concept.ts:12)在 `book: 'Book'` 行后加:

```ts
  /** AI 先读:电子书摘要 `YYYY-MM-DD-summary.md`(claude-agent ai-read-ebook 任务产出) */
  bookSummary: 'Book Summary',
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml && pnpm check`
Expected: Rust 全 PASS;`pnpm check` 无 TS 错误。

- [ ] **Step 6: Commit**

```bash
git add plugins-src/claude-agent/backend/templates/ai-read-ebook plugins-src/claude-agent/backend/src/task.rs plugins-src/claude-agent/backend/src/plugin.rs src/lib/okf/concept.ts
git commit -m "feat(claude-agent): 内置任务 ai-read-ebook(电子书大纲摘要)+ OKF type 登记"
```

---

### Task 7: ebook-import 后端:AI 阅读 FIFO + 轮询 + 提醒

**Files:**
- Create: `plugins-src/ebook-import/backend/src/airead.rs`
- Modify: `plugins-src/ebook-import/backend/src/main.rs` 或 `lib.rs`(`mod airead;`——以现有 mod 声明位置为准)
- Modify: `plugins-src/ebook-import/backend/src/plugin.rs`(Inner :33、initialize、on_ui_request :462、新函数)
- Modify: `plugins-src/ebook-import/manifest.v2.json`(capabilities、engines、version)

**Interfaces:**
- Consumes: `host.request("host.agent.run", {task, prompt, note_path}) → {run_id}`;`host.request("host.agent.status", {task, run_id}) → {state, record?/started_at?}`;`host.request("host.notify", {title, action}) → {ok, id}`(Task 3/4);任务 id `"ai-read-ebook"`(Task 6)。
- Produces: 窗口 RPC `plugin.ai_read_start {job_id: u64, dest_rel: String, name: String} → {queued: bool}`;host→UI 推送 `{type:"ai_read", job_id, event:"started"|"done"|"failed", started_at?, summary_rel?, error?}`(Task 8/9 消费)。

- [ ] **Step 1: 写 airead.rs(测试先行,同文件)**

```rust
//! "AI 先读"队列:done 的书逐本转给 claude-agent(任务锁是 per-task 的,
//! 只能串行),轮询 run 到收尾,经 host.notify 推托盘提醒。
//! 本模块只放可单测的纯逻辑;拉起 tokio 任务的粘合在 plugin.rs。
use std::collections::VecDeque;

pub const TASK_ID: &str = "ai-read-ebook";

#[derive(Debug, Clone, PartialEq)]
pub struct AiJob {
    pub job_id: u64,
    pub dest_rel: String,
    pub name: String,
}

/// FIFO + 单 worker 标志。所有方法都要在 Inner 的锁内调用,保证原子。
#[derive(Debug, Default)]
pub struct AiQueue {
    q: VecDeque<AiJob>,
    running: bool,
}

impl AiQueue {
    /// 入队;同 job_id 已在队中(重复点击)返回 false。
    pub fn enqueue(&mut self, job: AiJob) -> bool {
        if self.q.iter().any(|j| j.job_id == job.job_id) {
            return false;
        }
        self.q.push_back(job);
        true
    }
    /// 入队后是否要拉起 worker(已有 worker 在跑则不拉)。
    pub fn claim_worker(&mut self) -> bool {
        if self.running {
            return false;
        }
        self.running = true;
        true
    }
    /// worker 取下一本;队空时放下 running 标志并返回 None(worker 退出)。
    pub fn next(&mut self) -> Option<AiJob> {
        let j = self.q.pop_front();
        self.running = j.is_some();
        j
    }
}

pub fn summary_name(date: chrono::NaiveDate) -> String {
    format!("{}-summary.md", date.format("%Y-%m-%d"))
}

/// 附加给 run-task 的定位 prompt(任务模板自带总 prompt,这里只给坐标)。
pub fn run_prompt(dest_rel: &str, summary_rel: &str) -> String {
    format!(
        "本次只读这一本书:`{dest_rel}/book.md`。\n\
         摘要写到 `{summary_rel}`(同名文件已存在则直接覆盖)。\n\
         不要读、不要改 vault 里的其它文件 —— 权限也已按此限定。"
    )
}

/// 一次 host.agent.status 应答的解读。
#[derive(Debug, PartialEq)]
pub enum RunPoll {
    Running,
    Succeeded,
    Failed(String),
}

pub fn interpret_status(v: &serde_json::Value) -> RunPoll {
    match v.get("state").and_then(|s| s.as_str()) {
        Some("running") => RunPoll::Running,
        Some("done") => {
            let rec = v.get("record");
            let status = rec
                .and_then(|r| r.get("status"))
                .and_then(|s| s.as_str())
                .unwrap_or("error");
            if status == "success" {
                RunPoll::Succeeded
            } else {
                let tail = rec
                    .and_then(|r| r.get("result"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                RunPoll::Failed(format!("{status}: {tail}"))
            }
        }
        // 无 record 也无锁:进程死了,或任务锁被别处(窗口/CLI)抢占后我们的
        // run 没跑成 —— 都按失败重试处理。
        Some("lost") => RunPoll::Failed("run lost".into()),
        _ => RunPoll::Failed("unrecognized run status".into()),
    }
}

/// 托盘提醒标题;locale 来自 $initialize(InitializeParams.locale)。
pub fn reminder_title(locale: &str, name: &str, ok: bool) -> String {
    let (done, fail) = match locale.split('-').next().unwrap_or("en") {
        "zh" => (format!("《{name}》AI 摘要已生成"), format!("《{name}》AI 阅读失败")),
        "ja" => (
            format!("『{name}』AI 要約ができました"),
            format!("『{name}』AI リーディングに失敗しました"),
        ),
        "de" => (
            format!("KI-Zusammenfassung für „{name}“ ist fertig"),
            format!("KI-Lektüre von „{name}“ fehlgeschlagen"),
        ),
        _ => (
            format!("AI digest ready for \u{201c}{name}\u{201d}"),
            format!("AI reading failed for \u{201c}{name}\u{201d}"),
        ),
    };
    if ok { done } else { fail }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: u64) -> AiJob {
        AiJob { job_id: id, dest_rel: format!("ssot/ebooks/2026-08/b{id}"), name: format!("b{id}") }
    }

    #[test]
    fn enqueue_dedups_by_job_id() {
        let mut q = AiQueue::default();
        assert!(q.enqueue(job(1)));
        assert!(!q.enqueue(job(1)), "duplicate click must not double-queue");
        assert!(q.enqueue(job(2)));
    }

    #[test]
    fn claim_worker_only_once_until_drained() {
        let mut q = AiQueue::default();
        q.enqueue(job(1));
        assert!(q.claim_worker());
        assert!(!q.claim_worker(), "second start while running must not spawn");
        assert_eq!(q.next(), Some(job(1)));
        assert_eq!(q.next(), None); // 队空 → running 落下
        assert!(q.claim_worker(), "after drain a new worker may start");
    }

    #[test]
    fn summary_name_is_date_stamped() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 8, 4).unwrap();
        assert_eq!(summary_name(d), "2026-08-04-summary.md");
    }

    #[test]
    fn interpret_status_variants() {
        assert_eq!(interpret_status(&serde_json::json!({"state": "running", "steps": 3})), RunPoll::Running);
        assert_eq!(
            interpret_status(&serde_json::json!({"state": "done", "record": {"status": "success", "result": "ok"}})),
            RunPoll::Succeeded
        );
        assert!(matches!(
            interpret_status(&serde_json::json!({"state": "done", "record": {"status": "timeout", "result": "x"}})),
            RunPoll::Failed(e) if e.starts_with("timeout")
        ));
        assert!(matches!(interpret_status(&serde_json::json!({"state": "lost"})), RunPoll::Failed(_)));
        assert!(matches!(interpret_status(&serde_json::json!({})), RunPoll::Failed(_)));
    }

    #[test]
    fn reminder_titles_are_localized_and_distinct() {
        for ok in [true, false] {
            let all: Vec<String> = ["en", "zh", "ja", "de"]
                .iter()
                .map(|l| reminder_title(l, "深度工作", ok))
                .collect();
            for t in &all {
                assert!(t.contains("深度工作"));
            }
            let uniq: std::collections::HashSet<_> = all.iter().collect();
            assert_eq!(uniq.len(), 4, "each locale must differ: {all:?}");
        }
        assert_eq!(reminder_title("zh-CN", "x", true), reminder_title("zh", "x", true));
    }
}
```

- [ ] **Step 2: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/ebook-import/backend/Cargo.toml airead`
Expected: 6 测试 PASS(记得在 crate 根加 `mod airead;`)。

- [ ] **Step 3: plugin.rs 粘合**

Inner(:33)加字段:

```rust
    /// "AI 先读"串行队列(claude-agent 的任务锁是 per-task 的)。
    ai: crate::airead::AiQueue,
    /// $initialize 下发的宿主 locale,提醒标题本地化用。
    locale: String,
```

`impl sdk::NotemdPlugin for EbookImportPlugin` 里覆写 initialize(activate 前):

```rust
    fn initialize(&mut self, _host: &sdk::Host, params: &proto::InitializeParams) {
        self.inner.lock().unwrap().locale = params.locale.clone();
    }
```

`on_ui_request`(:462)match 加:

```rust
            "ai_read_start" => self.ai_read_start(host, &params),
```

新函数(impl EbookImportPlugin 内):

```rust
    /// "AI 先读":入队并(必要时)拉起串行 worker。同步返回,不等 agent。
    fn ai_read_start(&mut self, host: &sdk::Host, params: &Value) -> Result<Value, String> {
        let job_id = params
            .get("job_id")
            .and_then(|v| v.as_u64())
            .ok_or("ai_read_start needs 'job_id'")?;
        let dest_rel = params
            .get("dest_rel")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_end_matches('/'))
            .filter(|s| !s.is_empty())
            .ok_or("ai_read_start needs 'dest_rel'")?
            .to_string();
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&dest_rel)
            .to_string();
        let (vault, queued, spawn) = {
            let mut g = self.inner.lock().unwrap();
            let vault = g.vault.clone().ok_or(NO_VAULT)?;
            let queued = g.ai.enqueue(crate::airead::AiJob { job_id, dest_rel, name });
            let spawn = queued && g.ai.claim_worker();
            (vault, queued, spawn)
        };
        if spawn {
            spawn_ai_worker(host.clone(), self.inner.clone(), vault);
        }
        Ok(json!({ "queued": queued }))
    }
```

文件级新函数(`run_job` 附近,同其自由函数风格):

```rust
/// AI 阅读 worker:逐本处理直到队空。跑在 tokio 任务里(Host::request 绝不能
/// 在协议读循环上内联 await),窗口关闭不影响 —— 收尾与提醒照常。
fn spawn_ai_worker(host: sdk::Host, inner: Arc<Mutex<Inner>>, vault: PathBuf) {
    tokio::spawn(async move {
        loop {
            let job = { inner.lock().unwrap().ai.next() };
            let Some(job) = job else { break };
            let locale = { inner.lock().unwrap().locale.clone() };
            run_ai_job(&host, &vault, &locale, job).await;
        }
    });
}

async fn run_ai_job(host: &sdk::Host, vault: &Path, locale: &str, job: crate::airead::AiJob) {
    use crate::airead::{self, RunPoll};
    let summary_rel = format!(
        "{}/{}",
        job.dest_rel,
        airead::summary_name(chrono::Local::now().date_naive())
    );
    let started_at = chrono::Utc::now().to_rfc3339();
    host.ui_post(
        WINDOW,
        json!({ "type": "ai_read", "job_id": job.job_id, "event": "started",
                "started_at": started_at, "summary_rel": summary_rel }),
    );

    let fail = |err: String| async {
        host.log_warn(&format!("ai-read failed for {}: {err}", job.dest_rel));
        host.ui_post(
            WINDOW,
            json!({ "type": "ai_read", "job_id": job.job_id, "event": "failed", "error": err }),
        );
        // 失败提醒指向 claude-agent 窗口(那里有完整运行日志)。
        let _ = host
            .request(
                "host.notify",
                json!({
                    "title": airead::reminder_title(locale, &job.name, false),
                    "action": { "kind": "open_plugin_window",
                                "plugin_id": "notemd.claude-agent", "window": "main" }
                }),
            )
            .await;
    };

    let book_abs = vault.join(&job.dest_rel).join("book.md");
    let run = host
        .request(
            "host.agent.run",
            json!({
                "task": airead::TASK_ID,
                "prompt": airead::run_prompt(&job.dest_rel, &summary_rel),
                "note_path": book_abs.to_string_lossy(),
            }),
        )
        .await;
    let run_id = match run {
        Ok(v) => match v.get("run_id").and_then(|r| r.as_str()).map(str::to_string) {
            Some(id) => id,
            None => return fail(format!("host.agent.run returned no run_id: {v}")).await,
        },
        Err(e) => return fail(e).await,
    };

    // 2s 轮询到收尾;2h 是防呆上限(任务自身 timeout_seconds=1800 会先到)。
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2 * 3600);
    let mut strikes = 0u32;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if tokio::time::Instant::now() > deadline {
            return fail("polling deadline exceeded".into()).await;
        }
        let status = host
            .request(
                "host.agent.status",
                json!({ "task": airead::TASK_ID, "run_id": run_id }),
            )
            .await;
        let v = match status {
            Ok(v) => {
                strikes = 0;
                v
            }
            Err(e) => {
                // 瞬时中转失败(如 claude-agent 进程重启)容忍几次。
                strikes += 1;
                if strikes >= 5 {
                    return fail(format!("run-status failed {strikes} times: {e}")).await;
                }
                continue;
            }
        };
        match airead::interpret_status(&v) {
            RunPoll::Running => continue,
            RunPoll::Failed(e) => return fail(e).await,
            RunPoll::Succeeded => {
                // record 成功还不算数:约定的摘要文件必须真的在。
                if !vault.join(&summary_rel).is_file() {
                    return fail(format!("run succeeded but {summary_rel} is missing")).await;
                }
                host.ui_post(
                    WINDOW,
                    json!({ "type": "ai_read", "job_id": job.job_id, "event": "done",
                            "summary_rel": summary_rel }),
                );
                let _ = host
                    .request(
                        "host.notify",
                        json!({
                            "title": airead::reminder_title(locale, &job.name, true),
                            "action": { "kind": "open_path",
                                        "path": vault.join(&summary_rel).to_string_lossy() }
                        }),
                    )
                    .await;
                return;
            }
        }
    }
}
```

- [ ] **Step 4: manifest.v2.json**

`capabilities` → `["ui", "toast", "dialog", "vault.read", "vault.write", "editor.open", "agent", "notify"]`;`engines.notemd` → `">=6.804.1"`;`version` → `"1.0.8"`。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/ebook-import/backend/Cargo.toml`
Expected: 全部 PASS(worker 是 IO 粘合,靠 airead 纯逻辑测试 + GUI 实机)。

- [ ] **Step 6: Commit**

```bash
git add plugins-src/ebook-import/backend/src/airead.rs plugins-src/ebook-import/backend/src/plugin.rs plugins-src/ebook-import/backend/src/main.rs plugins-src/ebook-import/manifest.v2.json
git commit -m "feat(ebook-import): AI 先读后端——FIFO 串行队列 + agent 轮询 + 托盘提醒"
```

(若 mod 声明在 lib.rs 则把 main.rs 换成实际文件。)

---

### Task 8: ebook-import 前端 reducer(queue.ts)

**Files:**
- Modify: `plugins-src/ebook-import/src/lib/queue.ts`(QueueItem :15、新类型、新 reducer)
- Test: `plugins-src/ebook-import/src/lib/queue.test.ts`

**Interfaces:**
- Consumes: Task 7 的推送 payload `{type:"ai_read", job_id, event, started_at?, summary_rel?, error?}`。
- Produces: `QueueItem` 新可选字段 `aiStatus?: AiStatus; aiStartedAt?: string; aiSummaryRel?: string; aiError?: string`;`export type AiStatus = 'queued' | 'running' | 'done' | 'failed'`;`export interface AiEvent { event: 'queued' | 'started' | 'done' | 'failed'; started_at?: string; summary_rel?: string; error?: string }`;`export function onAiEvent(q: Queue, jobId: number, ev: AiEvent): Queue`(按 `item.jobId` 匹配,找不到原样返回)。

- [ ] **Step 1: 写失败测试**(queue.test.ts 追加)

```ts
describe('onAiEvent', () => {
  function doneItem(id: number, jobId: number): Queue {
    let q: Queue = { items: [], activeId: null }
    q = addPaths(q, [`/tmp/book${id}.epub`])
    const item = { ...q.items[0], status: 'done' as const, jobId, destRel: `ssot/ebooks/2026-08/b${id}` }
    return { ...q, items: [item] }
  }

  it('started marks running with timestamps and target', () => {
    const q = onAiEvent(doneItem(1, 7), 7, {
      event: 'started',
      started_at: '2026-08-04T03:00:00Z',
      summary_rel: 'ssot/ebooks/2026-08/b1/2026-08-04-summary.md',
    })
    expect(q.items[0].aiStatus).toBe('running')
    expect(q.items[0].aiStartedAt).toBe('2026-08-04T03:00:00Z')
    expect(q.items[0].aiSummaryRel).toBe('ssot/ebooks/2026-08/b1/2026-08-04-summary.md')
  })

  it('queued then done keeps summary target and clears error', () => {
    let q = onAiEvent(doneItem(1, 7), 7, { event: 'queued' })
    expect(q.items[0].aiStatus).toBe('queued')
    q = onAiEvent(q, 7, { event: 'done', summary_rel: 'x/2026-08-04-summary.md' })
    expect(q.items[0].aiStatus).toBe('done')
    expect(q.items[0].aiSummaryRel).toBe('x/2026-08-04-summary.md')
  })

  it('failed records the error; retry via queued clears it', () => {
    let q = onAiEvent(doneItem(1, 7), 7, { event: 'failed', error: 'run lost' })
    expect(q.items[0].aiStatus).toBe('failed')
    expect(q.items[0].aiError).toBe('run lost')
    q = onAiEvent(q, 7, { event: 'queued' })
    expect(q.items[0].aiStatus).toBe('queued')
    expect(q.items[0].aiError).toBeUndefined()
  })

  it('unknown jobId is a no-op', () => {
    const q = doneItem(1, 7)
    expect(onAiEvent(q, 99, { event: 'started' })).toBe(q)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm -C plugins-src/ebook-import test`
Expected: FAIL(`onAiEvent` 未导出)。

- [ ] **Step 3: 实现**

QueueItem(:15,`logs: string[]` 前)加:

```ts
  /** "AI 先读"侧线状态(与导入 status 正交,仅 done 的行会有)。 */
  aiStatus?: AiStatus
  aiStartedAt?: string
  aiSummaryRel?: string
  aiError?: string
```

文件尾部加:

```ts
export type AiStatus = 'queued' | 'running' | 'done' | 'failed'

/** Payload shape of a host `type:"ai_read"` push (backend run_ai_job);
 * `queued` is applied locally by the window right after plugin.ai_read_start. */
export interface AiEvent {
  event: 'queued' | 'started' | 'done' | 'failed'
  started_at?: string
  summary_rel?: string
  error?: string
}

export function onAiEvent(q: Queue, jobId: number, ev: AiEvent): Queue {
  const idx = q.items.findIndex((i) => i.jobId === jobId)
  if (idx === -1) return q
  const item = q.items[idx]
  let next: QueueItem
  switch (ev.event) {
    case 'queued':
      next = { ...item, aiStatus: 'queued', aiError: undefined }
      break
    case 'started':
      next = {
        ...item,
        aiStatus: 'running',
        aiStartedAt: ev.started_at,
        aiSummaryRel: ev.summary_rel,
        aiError: undefined,
      }
      break
    case 'done':
      next = { ...item, aiStatus: 'done', aiSummaryRel: ev.summary_rel ?? item.aiSummaryRel }
      break
    case 'failed':
      next = { ...item, aiStatus: 'failed', aiError: ev.error }
      break
    default:
      return q
  }
  const items = [...q.items]
  items[idx] = next
  return { ...q, items }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm -C plugins-src/ebook-import test`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add plugins-src/ebook-import/src/lib/queue.ts plugins-src/ebook-import/src/lib/queue.test.ts
git commit -m "feat(ebook-import): queue 增加 AI 先读侧线状态与 onAiEvent reducer"
```

---

### Task 9: ebook-import 前端 UI + i18n

**Files:**
- Modify: `plugins-src/ebook-import/src/App.svelte`(:21 push 类型、:157 onMessage、:246 动作函数、:433 done 分支)
- Modify: `plugins-src/ebook-import/src/lib/strings.ts`(MessageKey union、四个 catalog)
- Test: `plugins-src/ebook-import/src/lib/strings.test.ts`(既有套件自动覆盖新键,无需新增用例)

**Interfaces:**
- Consumes: Task 8 `onAiEvent/AiEvent`;Task 7 RPC `plugin.ai_read_start`。

- [ ] **Step 1: strings 四语新键**

MessageKey union 加:`'action.aiRead' | 'action.viewSummary' | 'ai.queued' | 'ai.running' | 'ai.failed'`。各 catalog(en :79 / zh :149 / ja :219 / de :289)对应加:

```ts
  // en
  'action.aiRead': 'AI read first',
  'action.viewSummary': 'View digest',
  'ai.queued': 'Waiting for AI…',
  'ai.running': 'AI reading… {elapsed}',
  'ai.failed': 'AI reading failed',
  // zh
  'action.aiRead': 'AI 先读',
  'action.viewSummary': '查看摘要',
  'ai.queued': '排队等待 AI 阅读…',
  'ai.running': 'AI 阅读中… {elapsed}',
  'ai.failed': 'AI 阅读失败',
  // ja
  'action.aiRead': 'AI に先に読ませる',
  'action.viewSummary': '要約を見る',
  'ai.queued': 'AI リーディング待機中…',
  'ai.running': 'AI リーディング中… {elapsed}',
  'ai.failed': 'AI リーディングに失敗しました',
  // de
  'action.aiRead': 'Zuerst KI lesen lassen',
  'action.viewSummary': 'Zusammenfassung öffnen',
  'ai.queued': 'Wartet auf KI…',
  'ai.running': 'KI liest… {elapsed}',
  'ai.failed': 'KI-Lektüre fehlgeschlagen',
```

Run: `pnpm -C plugins-src/ebook-import test` → strings 套件 PASS(它会验完整性/占位符/非英文)。

- [ ] **Step 2: App.svelte 类型与 onMessage**

push 类型(:21 附近)加:

```ts
  type AiPush = {
    type: 'ai_read'
    job_id: number
    event: 'started' | 'done' | 'failed'
    started_at?: string
    summary_rel?: string
    error?: string
  }
```

`HostPush` union 加 `AiPush`;import 里补 `onAiEvent`(from `./lib/queue`)。onMessage(:157)的 `else if (m.type === 'job')` 后加:

```ts
    } else if (m.type === 'ai_read') {
      const a = m as AiPush
      q = onAiEvent(q, a.job_id, {
        event: a.event,
        started_at: a.started_at,
        summary_rel: a.summary_rel,
        error: a.error,
      })
    }
```

- [ ] **Step 3: 动作函数 + 耗时 ticker**

`openInEditor`(:255)旁加:

```ts
  async function aiRead(item: QueueItem) {
    if (!item.destRel || item.jobId == null) return
    try {
      await bridge().request('plugin.ai_read_start', {
        job_id: item.jobId,
        dest_rel: item.destRel,
        name: item.name,
      })
      q = onAiEvent(q, item.jobId, { event: 'queued' })
    } catch (e) {
      globalError = message(e)
    }
  }

  async function openSummary(item: QueueItem) {
    if (!item.aiSummaryRel) return
    try {
      await bridge().request('host.editor.open', { path: item.aiSummaryRel })
    } catch (e) {
      globalError = message(e)
    }
  }

  // 「AI 阅读中… 3m12s」的秒针。effect 只读 q(anyRunning),interval 写
  // nowMs —— nowMs 不在 effect 依赖里,不会自失效死循环($effect 纪律)。
  let nowMs = $state(Date.now())
  $effect(() => {
    if (!q.items.some((i) => i.aiStatus === 'running')) return
    const t = setInterval(() => {
      nowMs = Date.now()
    }, 1000)
    return () => clearInterval(t)
  })
  function aiElapsed(item: QueueItem): string {
    if (!item.aiStartedAt) return ''
    const s = Math.max(0, Math.floor((nowMs - Date.parse(item.aiStartedAt)) / 1000))
    const m = Math.floor(s / 60)
    return m > 0 ? `${m}m${s % 60}s` : `${s}s`
  }
```

- [ ] **Step 4: done 分支 UI**

行内(:433 `{#if item.status === 'done'}` 块)替换为:

```svelte
            {#if item.status === 'done'}
              <button class="link" onclick={() => openInEditor(item)}>{t('action.openInEditor')}</button>
              {#if !item.aiStatus || item.aiStatus === 'failed'}
                <button class="link" onclick={() => aiRead(item)}>{t('action.aiRead')}</button>
              {:else if item.aiStatus === 'queued'}
                <span class="stage">{t('ai.queued')}</span>
              {:else if item.aiStatus === 'running'}
                <span class="stage">{t('ai.running', { elapsed: aiElapsed(item) })}</span>
              {:else if item.aiStatus === 'done'}
                <button class="link" onclick={() => openSummary(item)}>{t('action.viewSummary')}</button>
              {/if}
            {/if}
```

失败详情行(`{#if item.status === 'failed' && …}` 错误段之后、log 展开之前)加:

```svelte
          {#if item.aiStatus === 'failed' && item.aiError}
            <p class="error">{t('ai.failed')} <span class="detail">{item.aiError}</span></p>
          {/if}
```

- [ ] **Step 5: 全量前端验证**

Run: `pnpm -C plugins-src/ebook-import test && pnpm check`
Expected: 全 PASS、无 TS/Svelte 错误。

- [ ] **Step 6: Commit**

```bash
git add plugins-src/ebook-import/src/App.svelte plugins-src/ebook-import/src/lib/strings.ts
git commit -m "feat(ebook-import): AI 先读按钮/状态/查看摘要 UI 与四语文案"
```

---

### Task 10: 全量回归 + spec 注记 + dev GUI 手动验证 + 发布

**Files:**
- Modify: `docs/superpowers/specs/2026-08-04-ai-first-read-design.md`(§4 图标一句改为 title 角标)

- [ ] **Step 1: spec 注记**

spec §4 中「图标切"提醒态"新资源;图标优先级……」改为:「有提醒时托盘图标旁挂数字角标(`tray.set_title(N)`),图标四态不动——比新图标资源多传达条数,且无需新增美术资产」。

- [ ] **Step 2: 全量回归**

Run(依次,全须绿):

```bash
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path plugins-src/claude-agent/backend/Cargo.toml
cargo test --manifest-path plugins-src/ebook-import/backend/Cargo.toml
pnpm -C plugins-src/ebook-import test
pnpm check && pnpm test
```

- [ ] **Step 3: Commit + 汇报,等用户 GUI 验证**

```bash
git add docs/superpowers/specs/2026-08-04-ai-first-read-design.md
git commit -m "docs(spec): AI 先读——托盘提醒态改为数字角标注记"
```

然后**停在这里**,给用户手动验证步骤(不做 UI 自动化):

1. 构建并本地安装两插件的 dev 产物(按现有 dist-plugins dev 流程),`pnpm tauri dev` 起宿主;
2. 导入一本小书 → done 行出现「AI 先读」;点击 → 变「排队等待/AI 阅读中… Ns」;
3. 关掉导入窗口等完成 → 托盘图标旁出现数字角标 + 「🔔 1 条提醒」子菜单;
4. 点提醒 → 主窗口聚焦并打开 `YYYY-MM-DD-summary.md`,frontmatter `type: Book Summary`;提醒消失、角标清零;
5. 制造一次失败(如断开 claude 登录)→ 行内失败可重试;失败提醒点击打开 claude-agent 窗口;
6. 「清除全部提醒」可清空;语言切换后托盘文案/按钮文案跟随。

- [ ] **Step 4: 用户确认后发布(按既有惯例)**

- 宿主:独立 worktree + `.env.release` + `release.sh`(日期版本自动推导,≥6.804.1);
- 插件:`release-plugins.sh` 发 notemd.ebook-import 1.0.8 与 notemd.claude-agent 1.0.5;注意 gen-plugin-index 的本地 dist-plugins 旧版回扫坑(发布前清理本地 dist-plugins 里的历史版本);
- 发布前确认 gh 活跃账号是 wizlijun。
