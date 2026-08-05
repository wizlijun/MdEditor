# 统一通知基建 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把所有插件推送消息与宿主告警统一到一套「通知(Notification)」基建,托盘常驻子菜单展示、日志窗口存历史、角标实时跟随。

**Architecture:** 复用现有 `reminders.rs` 进程级注册表(重命名为 `notifications.rs`),给每条通知加 `source`(sticky key)+`severity`;托盘把动态子菜单改为常驻、并入大文件告警;每条通知镜像一行到 `log_bus`(category=`notification`),日志窗口新增该分类过滤即成历史。

**Tech Stack:** Rust(Tauri 2,tray/menu)、Svelte 5(logs 窗口)、自研 i18n(en/zh/ja/de)。

## Global Constraints

- 概念词 = **Notification**;四语言展示名(Apple 本地化):en `Notifications` / zh `通知` / ja `通知` / de `Mitteilungen`;`Clear All` = en `Clear All` / zh `全部清除` / ja `すべて消去` / de `Alle entfernen`。
- capability 名保持 `notify`(向后兼容,不新增 capability)。
- 现有插件对 `host.notify` 的旧调用(仅 title+action)必须零改动仍工作 = 瞬时 Info。
- 每个写 `.md` 无关;本任务不碰 vault 写入。
- 提交只精确 `git add` 目标文件,绝不 `git add -A`(主 worktree 常被兄弟会话共享)。
- Rust 测试:`cargo test`(在 `src-tauri`);前端:`pnpm check` + `pnpm test`。
- GUI/托盘实机验证由用户完成,本计划不做 UI 自动化;实现完成跑通 check+test 后提交,**不自动 release**(等用户 dev 验证)。

---

### Task 1: 重命名 reminders → notifications(纯机械,行为不变)

**Files:**
- Rename: `src-tauri/src/reminders.rs` → `src-tauri/src/notifications.rs`
- Modify: `src-tauri/src/lib.rs`(`pub mod reminders;`→`pub mod notifications;`;所有 `crate::reminders::` / `reminders::` → `notifications::`;托盘事件 id `tray-reminder*`→`tray-notification*`;`refresh_tray_reminders`→`refresh_tray_notifications`)
- Modify: `src-tauri/src/plugin_runtime/ui_rpc.rs`(`crate::reminders::` → `crate::notifications::`)

**Interfaces:**
- Produces(改名后,签名暂不变):`notifications::{push, take, clear_all, count, snapshot, parse_notify_params}`;类型 `Notification`、`NotificationAction`。

- [ ] **Step 1:** `git mv src-tauri/src/reminders.rs src-tauri/src/notifications.rs`
- [ ] **Step 2:** 在 `notifications.rs` 内把 `Reminder`→`Notification`、`ReminderAction`→`NotificationAction`(含 doc 注释「提醒」→「通知」);模块头注释同步。
- [ ] **Step 3:** `lib.rs`:`pub mod reminders;`→`pub mod notifications;`;全量替换 `crate::reminders::`→`crate::notifications::`、`reminders::`→`notifications::`;事件 id 字面量 `tray-reminder-clear`→`tray-notification-clear`、`tray-reminder:`→`tray-notification:`;函数名 `refresh_tray_reminders`→`refresh_tray_notifications`(含其调用点、DIRTY 守望任务)。
- [ ] **Step 4:** `ui_rpc.rs`:`crate::reminders::`→`crate::notifications::`。
- [ ] **Step 5:** `cd src-tauri && cargo test notifications 2>&1 | tail -20` — 期望原 5 个单测 PASS(改名后)。
- [ ] **Step 6:** `cd src-tauri && cargo build 2>&1 | tail -5` — 期望编译通过。
- [ ] **Step 7:** Commit:`git add src-tauri/src/notifications.rs src-tauri/src/lib.rs src-tauri/src/plugin_runtime/ui_rpc.rs && git commit -m "refactor(notifications): reminders→notifications 改名,行为不变"`

---

### Task 2: 数据模型扩展 —— source + severity + sticky 生命周期

**Files:**
- Modify: `src-tauri/src/notifications.rs`

**Interfaces:**
- Produces:
  - `enum Severity { Info, Warn }`(`Serialize/Deserialize`,`snake_case`,`Default = Info`)
  - `NotificationAction` 新增变体 `OpenLogs { filter: Option<String> }`(`rename_all="snake_case"` → kind `"open_logs"`),供同步异常打开日志窗口。
  - `struct Notification { id: u64, title: String, action: NotificationAction, source: Option<String>, severity: Severity }`
  - `Registry::push(&mut self, title, action, source: Option<String>, severity) -> (u64, bool)`(bool=是否有实质变化)
  - `Registry::dismiss_source(&mut self, key: &str) -> bool`
  - `Registry::clear_transient(&mut self)`(移除 `source.is_none()` 的项)
  - `Registry::max_severity(&self) -> Option<Severity>`
  - free fns:`push(title, action, source, severity) -> u64`、`dismiss_source(key)`、`clear_transient()`、`max_severity() -> Option<Severity>`;`parse_notify_params(v) -> (String, NotificationAction, Option<String>, Severity)`

- [ ] **Step 1: 写失败测试**(替换/新增到 `notifications.rs` 的 `mod tests`)

```rust
#[test]
fn sticky_source_is_idempotent_update() {
    let mut r = Registry::default();
    let (a, ch1) = r.push("旧".into(), oc("/x"), Some("k".into()), Severity::Warn);
    assert!(ch1);
    let (b, ch2) = r.push("新".into(), oc("/y"), Some("k".into()), Severity::Warn);
    assert_eq!(a, b, "同 source 保留同 id");
    assert!(ch2, "标题变了算变化");
    assert_eq!(r.len(), 1, "同 source 不新增");
    assert_eq!(r.items()[0].title, "新");
    let (_, ch3) = r.push("新".into(), oc("/y"), Some("k".into()), Severity::Warn);
    assert!(!ch3, "完全相同不算变化");
}

#[test]
fn dismiss_source_removes_only_matching() {
    let mut r = Registry::default();
    r.push("a".into(), oc("/a"), Some("k1".into()), Severity::Info);
    r.push("b".into(), oc("/b"), Some("k2".into()), Severity::Info);
    assert!(r.dismiss_source("k1"));
    assert!(!r.dismiss_source("k1"), "已无该 key");
    assert_eq!(r.len(), 1);
    assert_eq!(r.items()[0].source.as_deref(), Some("k2"));
}

#[test]
fn clear_transient_keeps_sticky() {
    let mut r = Registry::default();
    r.push("瞬时".into(), oc("/t"), None, Severity::Info);
    r.push("持续".into(), oc("/s"), Some("k".into()), Severity::Warn);
    r.clear_transient();
    assert_eq!(r.len(), 1);
    assert!(r.items()[0].source.is_some());
}

#[test]
fn max_severity_prefers_warn() {
    let mut r = Registry::default();
    assert_eq!(r.max_severity(), None);
    r.push("i".into(), oc("/i"), None, Severity::Info);
    assert_eq!(r.max_severity(), Some(Severity::Info));
    r.push("w".into(), oc("/w"), Some("k".into()), Severity::Warn);
    assert_eq!(r.max_severity(), Some(Severity::Warn));
}

#[test]
fn parse_reads_source_and_severity_with_defaults() {
    let (t, _a, src, sev) = parse_notify_params(&serde_json::json!({
        "title": "t", "action": { "kind": "open_path", "path": "/x" },
        "source": "vault.large_files", "severity": "warn"
    })).unwrap();
    assert_eq!(t, "t");
    assert_eq!(src.as_deref(), Some("vault.large_files"));
    assert_eq!(sev, Severity::Warn);
    // 缺省 = 瞬时 Info(旧插件零改动)
    let (_, _, src2, sev2) = parse_notify_params(&serde_json::json!({
        "title": "t", "action": { "kind": "open_path", "path": "/x" }
    })).unwrap();
    assert_eq!(src2, None);
    assert_eq!(sev2, Severity::Info);
}
```

测试辅助:在 `mod tests` 顶部加 `fn oc(p: &str) -> NotificationAction { NotificationAction::OpenPath { path: p.into() } }`。旧的 `push_assigns_increasing_ids_and_take_removes` / `parse_open_path` / `parse_open_plugin_window` 更新为新签名(push 多两参、parse 返回四元组;取 `.0`/`.1`)。

- [ ] **Step 2:** `cd src-tauri && cargo test notifications 2>&1 | tail -20` — 期望 FAIL(签名不匹配 / 方法不存在)。
- [ ] **Step 3: 实现**
  - 加 `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)] #[serde(rename_all="snake_case")] pub enum Severity { #[default] Info, Warn }`。
  - `Notification` 加 `pub source: Option<String>, pub severity: Severity`。
  - `Registry::push` 改签名,逻辑:
    ```rust
    pub fn push(&mut self, title: String, action: NotificationAction,
                source: Option<String>, severity: Severity) -> (u64, bool) {
        if let Some(ref key) = source {
            if let Some(it) = self.items.iter_mut().find(|i| i.source.as_deref() == Some(key.as_str())) {
                let changed = it.title != title || it.action != action || it.severity != severity;
                it.title = title; it.action = action; it.severity = severity;
                return (it.id, changed);
            }
        }
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Notification { id, title, action, source, severity });
        (id, true)
    }
    ```
  - `dismiss_source`:`if let Some(i)=self.items.iter().position(|x| x.source.as_deref()==Some(key)) { self.items.remove(i); true } else { false }`。
  - `clear_transient`:`self.items.retain(|x| x.source.is_some());`。
  - `max_severity`:`self.items.iter().map(|x| x.severity).max_by_key(|s| matches!(s, Severity::Warn) as u8)`(或手写:有 Warn→Warn,否则有项→Info,空→None)。
  - free `push` 转发并返回 `id`(丢弃 bool,Task 3 会用到 bool,故 free push 暂返回 `(u64,bool)` 或另开 `push_logged`;见下)。**决定**:free `push` 返回 `u64`;新增 `pub fn push_changed(...) -> (u64, bool)` 供 Task 3。free `dismiss_source`/`clear_transient`/`max_severity` 转发 + `DIRTY.notify_one()`(dismiss/clear 有实际移除时才敲)。
  - `NotificationAction` 加 `OpenLogs { filter: Option<String> }` 变体(serde `open_logs`)。
  - `parse_notify_params` 追加:`let source = v.get("source").and_then(|s| s.as_str()).filter(|s| !s.trim().is_empty()).map(String::from);` `let severity = match v.get("severity").and_then(|s| s.as_str()) { Some("warn") => Severity::Warn, _ => Severity::Info };` 返回四元组。
- [ ] **Step 4:** `cd src-tauri && cargo test notifications 2>&1 | tail -20` — 期望 PASS。
- [ ] **Step 5:** 修 `ui_rpc.rs:1086` 的 `notify_user`:`let (title, action, source, severity) = crate::notifications::parse_notify_params(params)?; let id = crate::notifications::push(title, action, source, severity);`。`cargo build` 通过。
- [ ] **Step 6:** Commit:`git add src-tauri/src/notifications.rs src-tauri/src/plugin_runtime/ui_rpc.rs && git commit -m "feat(notifications): source key 幂等 + severity + sticky 生命周期"`

---

### Task 3: 通知镜像进日志总线(历史)

**Files:**
- Modify: `src-tauri/src/notifications.rs`(free `push` 里镜像日志)

**Interfaces:**
- Consumes:`crate::log_bus::push_cat(category, source, level, message)`。
- Produces:每条有实质变化的通知 → 一行 `category="notification"` 日志。

- [ ] **Step 1: 写测试**(在 `mod tests`)

```rust
#[test]
fn push_mirrors_a_notification_log_line() {
    crate::log_bus::clear();
    // free push 走全局注册表:先清干净
    clear_all();
    push("《书》摘要已生成".into(), oc("/v/s.md"), None, Severity::Info);
    let last = crate::log_bus::snapshot().into_iter().rev()
        .find(|l| l.category == "notification");
    let last = last.expect("有一条 notification 日志");
    assert_eq!(last.message, "《书》摘要已生成");
    assert_eq!(last.level, "info");
    clear_all();
}
```

- [ ] **Step 2:** `cargo test notifications::tests::push_mirrors 2>&1 | tail -20` — FAIL。
- [ ] **Step 3: 实现** free `push`:
  ```rust
  pub fn push(title: String, action: NotificationAction, source: Option<String>, severity: Severity) -> u64 {
      let (id, changed) = REGISTRY.lock().unwrap().push(title.clone(), action, source, severity);
      if changed {
          let level = match severity { Severity::Warn => "warn", Severity::Info => "info" };
          crate::log_bus::push_cat("notification", "backend", level, title);
          DIRTY.notify_one();
      }
      id
  }
  ```
  (仅 `changed` 时敲 DIRTY + 记日志,避免 sticky 重复评估刷屏。)
- [ ] **Step 4:** `cargo test notifications 2>&1 | tail -20` — PASS。
- [ ] **Step 5:** Commit:`git add src-tauri/src/notifications.rs && git commit -m "feat(notifications): 每条通知镜像一行到日志总线(notification 分类)"`

---

### Task 4: 托盘常驻「通知」子菜单 + 大文件并入 + 角标

**Files:**
- Modify: `src-tauri/src/lib.rs`(`build_tray_menu` 组装段 ~1810-1890;`menu_label` 目录 ~1615;托盘事件 handler ~1318-1345;`refresh_tray_status` 角标 ~824)

**Interfaces:**
- Consumes:`notifications::{snapshot, count, max_severity, take, clear_transient}`;`Severity`。
- Produces:常驻子菜单 id `tray-notifications`;项 id `tray-notification:{id}`、`tray-notification-clear`、`tray-notification-history`。

- [ ] **Step 1:** `menu_label` 目录替换/新增(四语言,Apple 本地化):
  ```rust
  "tray.notifications.titleN" => ("{n} Notifications", "{n} 条通知", "通知 {n} 件", "{n} Mitteilungen"),
  "tray.notifications.titleEmpty" => ("Notifications", "通知", "通知", "Mitteilungen"),
  "tray.notifications.clear" => ("Clear All", "全部清除", "すべて消去", "Alle entfernen"),
  "tray.notifications.history" => ("Notification History…", "通知历史…", "通知履歴…", "Mitteilungsverlauf…"),
  ```
  删除旧 `tray.reminders.title` / `tray.reminders.clear`。
- [ ] **Step 2:** 重写子菜单构建段(替换 `reminders_submenu` 与 `large_submenu` 两块为一块常驻):
  ```rust
  let notif_items = crate::notifications::snapshot();
  let notif_submenu = {
      let dot = match crate::notifications::max_severity() {
          Some(crate::notifications::Severity::Warn) => DotColor::Yellow,
          Some(crate::notifications::Severity::Info) => DotColor::Blue,
          None => DotColor::Grey,
      };
      let title = if notif_items.is_empty() {
          menu_label(locale, "tray.notifications.titleEmpty")
      } else {
          menu_label(locale, "tray.notifications.titleN").replace("{n}", &notif_items.len().to_string())
      };
      let sub = Submenu::with_id_and_icon(app, "tray-notifications", &title, true, flat_dot(dot))?;
      for n in &notif_items {
          let it = MenuItem::with_id(app, format!("tray-notification:{}", n.id), &n.title, true, None::<&str>)?;
          sub.append(&it)?;
      }
      let has_transient = notif_items.iter().any(|n| n.source.is_none());
      let clear = MenuItem::with_id(app, "tray-notification-clear",
          menu_label(locale, "tray.notifications.clear"), has_transient, None::<&str>)?;
      let history = MenuItem::with_id(app, "tray-notification-history",
          menu_label(locale, "tray.notifications.history"), true, None::<&str>)?;
      if !notif_items.is_empty() { sub.append(&PredefinedMenuItem::separator(app)?)?; }
      sub.append(&clear)?;
      sub.append(&PredefinedMenuItem::separator(app)?)?;
      sub.append(&history)?;
      sub
  };
  ```
  删除 `large_submenu` 定义块。`status_item` 的 `has_large` 计算保留不动(状态行仍显示黄灯:`status_dot_image(state, has_large)`)。
- [ ] **Step 3:** 组装顺序(替换 b2 段):
  ```rust
  let mut b2 = b.item(&sync_repo_item).item(&status_item);
  b2 = b2.item(&notif_submenu);            // 常驻,状态行下、查看日志上
  let menu = b2.item(&sync_now_item).item(&sync_log_item).item(&edit_agents_item)
      .separator().item(&quit_item).build()?;
  ```
  删掉 `if let Some(ref sm)=large_submenu` 与 `if let Some(ref rm)=reminders_submenu` 两段。同时删除 `TrayShownLargeFiles` 触发的菜单重建(Task 5 改由通知刷新驱动)——保留 state 定义但其 diff-重建块删除(或保留无害;最小化改动:保留)。
- [ ] **Step 4:** 事件 handler(~1318):`"tray-reminder-clear"`→`"tray-notification-clear" => { crate::notifications::clear_transient(); crate::notifications::refresh... }`(clear_transient 内部敲 DIRTY 即触发重建);新增 `"tray-notification-history" => { open_logs_window(app, Some("notification")); }`;`id.starts_with("tray-reminder:")`→`"tray-notification:"`,取到后:瞬时(source none)执行 action 已含 `take`;**改**:先 `snapshot` 找到该项判断 source,执行 action,若 `source.is_none()` 才 `take(id)`。具体:
  ```rust
  id if id.starts_with("tray-notification:") => {
      if let Some(nid) = id.strip_prefix("tray-notification:").and_then(|s| s.parse::<u64>().ok()) {
          if let Some(n) = crate::notifications::snapshot().into_iter().find(|x| x.id == nid) {
              match &n.action { /* OpenPath / OpenPluginWindow 同原逻辑 */ }
              if n.source.is_none() { crate::notifications::take(nid); }
          }
      }
  }
  ```
  (原 `take`-then-match 改为 snapshot-match-then-conditional-take,保证 sticky 点击不消失。)
- [ ] **Step 5:** 角标:`refresh_tray_status` 中 `let n_reminders = crate::reminders::count();` → `let n = crate::notifications::count();`,`if n>0 { set_title(n) } else { set_title(None) }`(逻辑已在,仅改调用名)。
- [ ] **Step 6:** `cd src-tauri && cargo build 2>&1 | tail -15` — 通过(修所有残留引用)。
- [ ] **Step 7:** Commit:`git add src-tauri/src/lib.rs && git commit -m "feat(tray): 常驻「通知」子菜单,大文件并入,角标跟随活跃数"`

---

### Task 5: 内部生产者迁移 —— 大文件门禁 + 同步异常 → sticky 通知

**Files:**
- Modify: `src-tauri/src/lib.rs`(`refresh_tray_status` 内,读到 `skipped_large`/`state` 处)

**Interfaces:**
- Consumes:`notifications::{push, dismiss_source, Severity, NotificationAction}`;`vault_sync::SyncState`。
- Produces:sticky `source="vault.large_files"`、`source="vault.sync"`。

- [ ] **Step 1:** 在 `refresh_tray_status` 拿到 `skipped_large`(即 `mgr.skipped_large_files`)后加:
  ```rust
  if skipped_large.is_empty() {
      crate::notifications::dismiss_source("vault.large_files");
  } else {
      let title = menu_label(&locale_for_status, "notif.largeFiles")
          .replace("{n}", &skipped_large.len().to_string());
      // action:打开 vault 根(文件夹视图)。vault_root 从 mgr 取。
      crate::notifications::push(title,
          crate::notifications::NotificationAction::OpenPath { path: vault_root_string },
          Some("vault.large_files".into()), crate::notifications::Severity::Warn);
  }
  ```
  (`locale_for_status = read_saved_locale(app)`;`notif.largeFiles` 目录项:`("{n} large files not committed", "{n} 个大文件未进 commit", "{n} 件の大きなファイルは未コミット", "{n} große Dateien nicht committet")`。逐文件明细已由现有 `log_cat!("git-sync", ...)` 或此处补 `log_warn!` 记录——最小化:不新增逐文件日志,summary 通知本身进 notification 日志即够。)
- [ ] **Step 2:** 同步异常:在同函数拿到 `state` 后加(action 用新 `OpenLogs` 打开同步日志):
  ```rust
  if state.is_problem() {
      crate::notifications::push(
          menu_label(&locale_for_status, "notif.syncError"),
          crate::notifications::NotificationAction::OpenLogs { filter: Some("git-sync".into()) },
          Some("vault.sync".into()), crate::notifications::Severity::Warn);
  } else {
      crate::notifications::dismiss_source("vault.sync");
  }
  ```
  (`notif.syncError` 目录项:`("Vault sync failed", "Vault 同步失败", "Vault 同期に失敗", "Vault-Synchronisierung fehlgeschlagen")`。状态行红点仍并行显示当前态,与通知不冲突:状态行=当前态,通知=需处理的异常条目——honor 决策 B。)
  Task 4 事件 handler 需处理 `OpenLogs` 分支:`crate::notifications::NotificationAction::OpenLogs { filter } => { open_logs_window(app, filter.as_deref()); }`(sticky,点击不 take)。
- [ ] **Step 3:** 防递归:`push`/`dismiss_source` 敲 `DIRTY` → 触发 `refresh_tray_notifications` → 其末尾调 `refresh_tray_status` → 又 push。**必须幂等收敛**:`push` 同 source 同内容返回 `changed=false` 不敲 DIRTY;`dismiss_source` 无匹配返回 false 不敲 DIRTY。→ 稳态下第二轮不再触发,收敛。**验证点**:确保 `dismiss_source` 无匹配时不敲 DIRTY(Task 2 已如此)。
- [ ] **Step 4:** `cargo build` + `cargo test notifications` — 通过。
- [ ] **Step 5:** Commit:`git add src-tauri/src/lib.rs && git commit -m "feat(notifications): 大文件门禁改为 sticky 通知(vault.large_files)"`

---

### Task 6: host.dismissNotification 桥(notify capability 下)

**Files:**
- Modify: `src-tauri/src/plugin_runtime/host_api.rs`(`method_capability`、dispatch)、`src-tauri/src/plugin_runtime/ui_rpc.rs`(trait `HostServices` 加默认方法 + 真实 impl + notify_push/dispatch)

**Interfaces:**
- Consumes:`notifications::dismiss_source`。
- Produces:插件可调 `host.dismissNotification({ source })`。

- [ ] **Step 1: 测试**(host_api.rs tests):`assert_eq!(method_capability("host.dismissNotification"), Some("notify"));` 与一条:带 notify capability 时 `host.dismissNotification` 被派发到 `notify_user`-兄弟方法(用 RecordingServices 记录 `("dismiss", params)`)。
- [ ] **Step 2:** `cargo test host_api 2>&1 | tail` — FAIL。
- [ ] **Step 3: 实现**
  - `HostServices` trait 加:`fn dismiss_notification(&self, _params: &serde_json::Value) -> Result<serde_json::Value, String> { Ok(serde_json::json!({"ok": true})) }`。
  - 真实 impl(ui_rpc.rs:1086 同一 impl 块):`fn dismiss_notification(&self, params) { if let Some(s)=params.get("source").and_then(|v|v.as_str()) { crate::notifications::dismiss_source(s); } Ok(json!({"ok":true})) }`。
  - `method_capability`:`"host.dismissNotification" => Some("notify"),`。
  - dispatch(host_api.rs:204 与 ui_rpc.rs:413 两处 match):`"host.dismissNotification" => Some(services.dismiss_notification(&req.params)),` / `=> services.dismiss_notification(&req.params),`。
  - 测试 stub `RecordingServices` 加 `dismiss_notification` 记录。
- [ ] **Step 4:** `cargo test host_api 2>&1 | tail` — PASS。
- [ ] **Step 5:** Commit:`git add src-tauri/src/plugin_runtime/host_api.rs src-tauri/src/plugin_runtime/ui_rpc.rs && git commit -m "feat(host): host.dismissNotification(notify capability 下,撤 sticky)"`

---

### Task 7: 前端日志窗口新增「通知」分类 + i18n(4 语言)

**Files:**
- Modify: `src/logs-app.svelte`(分类下拉 option + `catClass`)
- Modify: `src/lib/i18n/{en,zh,ja,de}.ts`(`logs.categories.notification`)
- Modify: `src/lib/logs/logs-store.test.ts`(可选:分类过滤断言)

**Interfaces:**
- Consumes:`store.categoryFilter === 'notification'` 匹配 `line.category === 'notification'`(现成 `matchCategory` 逻辑已覆盖精确匹配)。

- [ ] **Step 1:** `logs-app.svelte` 分类 `<select>` 在 `git-sync` option 后加:
  ```svelte
  <option value="notification">{t('logs.categories.notification')}</option>
  ```
  `catClass(cat)`:`if (cat === 'notification') return 'cat-notif'`;并在 `<style>` 加 `.cat-notif { color: <蓝> }`(参照现有 `.cat-git`)。
- [ ] **Step 2:** 四语言 i18n(紧邻 `logs.categories.gitSync`):
  - en `'logs.categories.notification': 'Notifications',`
  - zh `'logs.categories.notification': '通知',`
  - ja `'logs.categories.notification': '通知',`
  - de `'logs.categories.notification': 'Mitteilungen',`
- [ ] **Step 3:** `pnpm check 2>&1 | tail -20` — 无类型/缺键错误(i18n 串测 `store.test.ts` 覆盖四语言键一致性)。
- [ ] **Step 4:** `pnpm test 2>&1 | tail -20` — PASS。
- [ ] **Step 5:** Commit:`git add src/logs-app.svelte src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts && git commit -m "feat(logs): 通知分类过滤 + 四语言"`

---

### Task 8: 全量校验

- [ ] **Step 1:** `cd src-tauri && cargo test 2>&1 | tail -25` — 全绿。
- [ ] **Step 2:** `cd src-tauri && cargo build 2>&1 | tail -5` — 通过。
- [ ] **Step 3:** `pnpm check && pnpm test 2>&1 | tail -25` — 全绿。
- [ ] **Step 4:** 交用户 dev 实机验证托盘(常驻「通知」项、空/非空标题、色点、角标随增删跟随、大文件告警进通知、通知历史打开日志筛到 notification)。**不自动 release。**

## Self-Review

- **Spec coverage:** 术语✓(T4/T7)、数据模型✓(T2)、生命周期 B✓(T2/T4)、常驻子菜单+位置✓(T4)、状态行保留✓(T4)、大文件并入✓(T5)、同步异常并入✓(T5,OpenLogs action)、日志集成✓(T3/T7)、角标✓(T4)、插件 API notify 扩展✓(T2 parse + T6 dismiss)、测试✓。与 spec 一致,无偏离。
- **Placeholder scan:** 无 TBD/TODO;每步给出实码。
- **Type consistency:** `push(title, action, source, severity)` 四参一致贯穿 T2/T4/T5;`Severity::{Info,Warn}`、`dismiss_source(&str)`、`clear_transient()`、`max_severity()->Option<Severity>` 命名前后一致;事件 id `tray-notification*` 一致。
