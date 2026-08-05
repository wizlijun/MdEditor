# 统一通知基建(Unified Notifications)设计

日期:2026-08-05
状态:设计定稿,待实现
相关代码:`src-tauri/src/reminders.rs`、`src-tauri/src/lib.rs`(托盘)、`src-tauri/src/log_bus.rs`、`src/logs-app.svelte`、`src/lib/logs/logs-store.svelte.ts`、`src/lib/i18n/{en,zh,ja,de}.ts`

## 目标

把「所有插件推送的消息 + 宿主告警」统一到**一套通知基建**里管理与显示:

1. 托盘菜单里有一个**固定常驻**的「通知」子菜单项(空也不隐藏),位于「查看日志」正上方。
2. 不做独立通知框——**历史复用「查看日志」窗口**,新增一个 `notification` 分类过滤;子菜单底部「通知历史…」点击即打开日志窗口并预设到该类。
3. 托盘图标角标数字实时跟随真实活跃通知数,归零立即隐藏。

现状:基建雏形已存在(`reminders.rs` 注册表 + `host.notify` capability + 托盘蓝点动态子菜单),但英文/德文用词不一致、子菜单「有未读才进出」、大文件告警与同步状态各走独立通道。本设计将其收敛为一套。

## 术语(锁定 —— 采用 Apple 各语言官方本地化)

规范概念词:**Notification**。capability 名保持 `notify`(向后兼容)。

| key | en | zh(简) | ja | de |
|---|---|---|---|---|
| 概念/常驻标题 | Notifications | 通知 | 通知 | Mitteilungen |
| 标题(N 条) | {n} Notifications | {n} 条通知 | 通知 {n} 件 | {n} Mitteilungen |
| 空态 | No Notifications | 无通知 | 通知はありません | Keine Mitteilungen |
| 清除全部 | Clear All | 全部清除 | すべて消去 | Alle entfernen |
| 通知历史 | Notification History… | 通知历史… | 通知履歴… | Mitteilungsverlauf… |
| 日志分类 | Notifications | 通知 | 通知 | Mitteilungen |

依据:Apple macOS/iOS 本地化 —— `Notifications` 德语作 **Mitteilungen**(通知中心 = Mitteilungszentrale),`Clear All` 德语 **Alle entfernen** / 简中 **全部清除** / 日 **すべて消去**。中文界面词取「通知」与 macOS 通知中心对齐。

## 数据模型

`src-tauri/src/reminders.rs` → 重命名为 **`notifications.rs`**;类型 `Reminder`→`Notification`、`ReminderAction`→`NotificationAction`;`crate::reminders::*` 全量机械改名。托盘事件 id 前缀 `tray-reminder*` → `tray-notification*`。

```rust
pub enum Severity { Info, Warn }   // Info=蓝点, Warn=黄点

pub struct Notification {
    pub id: u64,                       // 自增
    pub title: String,
    pub action: NotificationAction,    // OpenPath | OpenPluginWindow(不变)
    pub source: Option<String>,        // 新增:稳定 key。Some=持续告警;None=瞬时事件
    pub severity: Severity,            // 新增:默认 Info
}
```

## 生命周期(方案 B:source key + 幂等 + 显式撤下)

- `push(title, action, source, severity) -> u64`
  - `source = Some(key)` 且注册表已有同 key 项 → **原地更新**(标题/动作/严重度),保留其 id,不新增。
  - 否则追加新项(自增 id)。
- `dismiss_source(key)`:按 key 移除持续告警(产生方在根因消失时调用)。
- `take(id)`:移除单条(瞬时项点击后调用),不变。
- 点击行为(托盘 handler):
  - **瞬时**(`source = None`):执行 action + `take(id)` 移除。
  - **持续**(`source = Some`):执行 action,**不移除**(根因仍在)。
- **「全部清除」语义**:只移除**瞬时**项(`source = None`);持续告警保留。防止用户一键抹掉仍成立的真实告警。实现:`clear_transient()` 遍历保留 `source.is_some()` 的项。

`DIRTY` 通知机制不变;任意线程写数据后敲 `DIRTY`,主线程守望任务重建菜单 + 刷角标。

## 托盘结构

「通知」子菜单**永远存在**(空也不隐藏),插入到 `sync_log_item`(查看日志)**正上方**。当前顺序:
`… sync_repo_item → status_item → [large_submenu?] → [reminders_submenu?] → sync_now_item → sync_log_item → …`
改为:
`… sync_repo_item → status_item → **notifications_submenu(常驻)** → sync_now_item → sync_log_item → …`

- 子菜单标题:
  - N>0 → `{n} 条通知`,色点 = 活跃项中的**最高严重度**(有 Warn 则黄点,否则蓝点)。
  - N=0 → `通知`,灰点。
- 子菜单内容:
  - 活跃通知项逐条(各带 action)。
  - 分隔线。
  - `全部清除`(N=0 或无瞬时项时禁用)。
  - **`通知历史…`**(永远在)→ `open_logs_window(app, Some("notification"))`。
- 色点:Info/Warn 复用现有 `flat_dot(DotColor::Blue/Yellow)`,与状态行 `status_dot_image` **同源同 36px 尺寸**,视觉一致。

**状态行(`tray-sync-status`)保留(决策 a)**:它显示「同步态 · 上次同步 Xm 前」+ 健康色点,是"当前持续状态"而非"一条待处理消息",与通知语义不同,继续做被动健康显示。大文件独立黄灯子菜单 `tray-large-files` **撤掉**,并入通知。

### 角标

托盘图标角标 = 活跃通知数(瞬时 + 持续),来自 `notifications::count()`。现有 `refresh_tray_status` 已实现 `count()>0 → set_title(n)`、`else → set_title(None)`;`refresh_tray_reminders` 末尾已调 `refresh_tray_status`。→ 通知增删经 `DIRTY` → 重建菜单 + 刷角标,**实时跟随、归零立即隐藏**。本设计只需保证 `count()` 统计统一后的注册表。

## 日志集成(历史)

- 每条通知 `push` 时,同步写日志总线:`log_bus::push_cat("notification", "backend", level, title)`,`level` 随 `severity`(Info→info / Warn→warn)。历史永久留在 `logs/app.log` 与日志窗口 ring buffer(现有 3000 行上限)。
- `src/logs-app.svelte`:分类下拉 `<select bind:value={store.categoryFilter}>` 增 `<option value="notification">{t('logs.categories.notification')}</option>`;`catClass()` 增一个 `cat-notification` 配色。
- 「通知历史…」复用现成的 `open_logs_window(app, Some("notification"))` → 发 `nav://logs-filter` 事件预设 `categoryFilter`。
- **不新增独立通知窗口。**

## 插件 API(向后兼容)

- `host.notify` 参数扩两个可选字段:
  - `source?: string` —— 提供即为持续告警(幂等)。
  - `severity?: "info" | "warn"` —— 缺省 `info`。
  - 缺省行为 = 现状(瞬时 Info),现有插件零改动。
- 新增桥接方法 `host.dismissNotification({ source })` —— 供插件撤下自己的 sticky 通知(对应后端 `dismiss_source`)。
- 后端 `parse_notify_params` 扩展解析 `source`/`severity`,未知/缺省宽容降级为瞬时 Info。

## 内部生产者迁移

- **大文件门禁**(`vault_sync` skipped_large_files):有超限 → `push`/更新 sticky,`source = "vault.large_files"`,`Severity::Warn`,title「N 个大文件未进 commit」,action 打开 vault 文件夹视图;都删除/低于阈值 → `dismiss_source("vault.large_files")`。逐文件明细进日志(不再逐条塞菜单)。撤掉 `tray-large-files` 子菜单及 `TrayShownLargeFiles` 相关重建逻辑(改由通知刷新驱动)。
- **同步异常**:进入 `SyncState::Error`/问题态 → sticky `source = "vault.sync"`(Warn),action 打开同步日志;恢复正常 → `dismiss_source("vault.sync")`。(状态行仍并行显示健康态,二者不冲突:状态行=当前态,通知=需处理的异常。)

## 测试

- `notifications.rs` 单测:
  - 同 `source` 幂等更新(不新增、id 保留、字段被更新)。
  - `dismiss_source` 只移除匹配项。
  - `clear_transient` 保留 `source.is_some()` 项、移除 `None` 项。
  - `parse_notify_params` 解析 `source`/`severity`,缺省降级 Info/瞬时,坏值宽容。
  - 最高严重度取值(有 Warn → Warn)。
- `logs-store` / `logs-app`:通知分类过滤项存在且 `matchCategory('notification')` 正确。
- `menu_label` 四语言串补齐(见术语表)并纳入既有 i18n 串测(en/zh/ja/de 无缺键)。

## 非目标(YAGNI)

- 不做通知的独立窗口/面板。
- 不做「已读/未读」细分状态(角标 = 活跃数即可)。
- 不做通知分组、优先级排序、系统级(macOS Notification Center)推送。
- 不迁移历史数据。
