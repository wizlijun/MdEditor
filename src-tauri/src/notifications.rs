//! 托盘全局通知注册表。任何插件经 `host.notify`(capability `notify`)推入一条
//! 通知;托盘出现常驻「通知」子菜单与数字角标,点击执行 action。瞬时通知点击即
//! 消,持续告警(带稳定 `source` key)由产生方在根因消失时 `dismiss_source` 撤下。
//! 每条有实质变化的通知镜像一行到日志总线(category=`notification`),日志窗口即历史。
//! 注册表是进程级全局(仿 plugin_runtime::STATE):HostServices 是泛型 `R: Runtime`,
//! 碰不了 Wry 专用的托盘刷新,所以写方只改数据 + 敲 DIRTY,由 lib.rs setup 里持有
//! Wry handle 的守望任务负责重建菜单。
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotificationAction {
    /// 绝对路径:聚焦主窗口并在编辑器打开该文件。
    OpenPath { path: String },
    /// 打开某插件贡献的窗口(失败通知指向 claude-agent 的运行日志)。
    OpenPluginWindow { plugin_id: String, window: String },
    /// 打开「查看日志」窗口,可预设分类过滤(如同步异常 → `git-sync`)。
    OpenLogs { filter: Option<String> },
}

/// 通知严重度。驱动托盘子菜单标题的色点:Info=蓝、Warn=黄(与状态行同一套 flat_dot)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    #[default]
    Info,
    Warn,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub id: u64,
    pub title: String,
    pub action: NotificationAction,
    /// `Some(key)` = 持续告警(同 key 幂等更新、产生方撤下);`None` = 瞬时事件(点击即消)。
    pub source: Option<String>,
    pub severity: Severity,
}

#[derive(Debug, Default)]
pub struct Registry {
    next_id: u64,
    items: Vec<Notification>,
}

impl Registry {
    /// 推入或(按 `source`)幂等更新一条通知。返回 `(id, changed)`;`changed=false`
    /// 表示同 `source` 已存在且内容完全一致 —— 调用方据此避免重复记日志/刷托盘。
    pub fn push(
        &mut self,
        title: String,
        action: NotificationAction,
        source: Option<String>,
        severity: Severity,
    ) -> (u64, bool) {
        if let Some(ref key) = source {
            if let Some(it) = self
                .items
                .iter_mut()
                .find(|i| i.source.as_deref() == Some(key.as_str()))
            {
                let changed = it.title != title || it.action != action || it.severity != severity;
                it.title = title;
                it.action = action;
                it.severity = severity;
                return (it.id, changed);
            }
        }
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Notification { id, title, action, source, severity });
        (id, true)
    }
    pub fn take(&mut self, id: u64) -> Option<Notification> {
        let i = self.items.iter().position(|r| r.id == id)?;
        Some(self.items.remove(i))
    }
    /// 按 `source` key 移除持续告警。返回是否移除了某项。
    pub fn dismiss_source(&mut self, key: &str) -> bool {
        if let Some(i) = self.items.iter().position(|x| x.source.as_deref() == Some(key)) {
            self.items.remove(i);
            true
        } else {
            false
        }
    }
    /// 只清瞬时项(`source == None`);持续告警保留(不能靠点一下抹掉真实告警)。
    pub fn clear_transient(&mut self) {
        self.items.retain(|x| x.source.is_some());
    }
    /// 活跃项里的最高严重度(有 Warn → Warn,否则有项 → Info,空 → None)。
    pub fn max_severity(&self) -> Option<Severity> {
        if self.items.is_empty() {
            None
        } else if self.items.iter().any(|x| x.severity == Severity::Warn) {
            Some(Severity::Warn)
        } else {
            Some(Severity::Info)
        }
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn items(&self) -> &[Notification] {
        &self.items
    }
}

static REGISTRY: LazyLock<Mutex<Registry>> = LazyLock::new(|| Mutex::new(Registry::default()));
/// 注册表变更信号。notify_one 在无等待者时存 permit,守望任务不会漏事件。
pub static DIRTY: LazyLock<tokio::sync::Notify> = LazyLock::new(tokio::sync::Notify::new);

/// 推入/更新一条通知。仅当有实质变化时才镜像日志 + 敲 DIRTY —— 持续告警反复以同
/// 内容重评估时不刷屏、也不会触发托盘刷新自激。
pub fn push(
    title: String,
    action: NotificationAction,
    source: Option<String>,
    severity: Severity,
) -> u64 {
    let (id, changed) = REGISTRY.lock().unwrap().push(title.clone(), action, source, severity);
    if changed {
        let level = match severity {
            Severity::Warn => "warn",
            Severity::Info => "info",
        };
        crate::log_bus::push_cat("notification", "backend", level, title);
        DIRTY.notify_one();
    }
    id
}
pub fn take(id: u64) -> Option<Notification> {
    let r = REGISTRY.lock().unwrap().take(id);
    if r.is_some() {
        DIRTY.notify_one();
    }
    r
}
/// 按 source 撤下持续告警;仅在确有移除时敲 DIRTY(无匹配不触发托盘刷新,避免自激)。
pub fn dismiss_source(key: &str) {
    if REGISTRY.lock().unwrap().dismiss_source(key) {
        DIRTY.notify_one();
    }
}
/// 「全部清除」:只清瞬时项,持续告警保留。仅在确有移除时敲 DIRTY。
pub fn clear_transient() {
    let mut reg = REGISTRY.lock().unwrap();
    let before = reg.len();
    reg.clear_transient();
    let changed = reg.len() != before;
    drop(reg);
    if changed {
        DIRTY.notify_one();
    }
}
/// 全清(含 sticky)。内部/测试用途,与「全部清除」按钮的 `clear_transient` 区分。
pub fn clear_all() {
    REGISTRY.lock().unwrap().items.clear();
    DIRTY.notify_one();
}
pub fn count() -> usize {
    REGISTRY.lock().unwrap().len()
}
pub fn max_severity() -> Option<Severity> {
    REGISTRY.lock().unwrap().max_severity()
}
pub fn snapshot() -> Vec<Notification> {
    REGISTRY.lock().unwrap().items().to_vec()
}

/// `host.notify` 参数 → `(title, action, source, severity)`。`source`/`severity`
/// 可选:缺省 = 瞬时 Info(旧插件仅传 title+action 时零改动仍工作)。
pub fn parse_notify_params(
    v: &serde_json::Value,
) -> Result<(String, NotificationAction, Option<String>, Severity), String> {
    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("host.notify needs a non-empty 'title'")?;
    let action = v.get("action").cloned().ok_or("host.notify needs an 'action'")?;
    let action: NotificationAction = serde_json::from_value(action).map_err(|e| format!("bad action: {e}"))?;
    let source = v
        .get("source")
        .and_then(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(String::from);
    let severity = match v.get("severity").and_then(|s| s.as_str()) {
        Some("warn") => Severity::Warn,
        _ => Severity::Info,
    };
    Ok((title.to_string(), action, source, severity))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oc(p: &str) -> NotificationAction {
        NotificationAction::OpenPath { path: p.into() }
    }

    #[test]
    fn push_assigns_increasing_ids_and_take_removes() {
        let mut r = Registry::default();
        let (a, _) = r.push("A".into(), oc("/x/a.md"), None, Severity::Info);
        let (b, _) = r.push("B".into(), oc("/x/b.md"), None, Severity::Info);
        assert!(b > a);
        assert_eq!(r.len(), 2);
        let got = r.take(a).expect("a exists");
        assert_eq!(got.title, "A");
        assert_eq!(r.len(), 1);
        assert!(r.take(a).is_none(), "second take is None");
    }

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
    fn parse_open_path() {
        let (title, action, source, severity) = parse_notify_params(&serde_json::json!({
            "title": "《书》AI 摘要已生成",
            "action": { "kind": "open_path", "path": "/v/ssot/ebooks/2026-08/书/2026-08-04-summary.md" }
        }))
        .unwrap();
        assert_eq!(title, "《书》AI 摘要已生成");
        assert_eq!(action, NotificationAction::OpenPath { path: "/v/ssot/ebooks/2026-08/书/2026-08-04-summary.md".into() });
        assert_eq!(source, None);
        assert_eq!(severity, Severity::Info);
    }

    #[test]
    fn parse_open_plugin_window() {
        let (_, action, _, _) = parse_notify_params(&serde_json::json!({
            "title": "失败",
            "action": { "kind": "open_plugin_window", "plugin_id": "notemd.claude-agent", "window": "main" }
        }))
        .unwrap();
        assert_eq!(action, NotificationAction::OpenPluginWindow { plugin_id: "notemd.claude-agent".into(), window: "main".into() });
    }

    #[test]
    fn parse_reads_source_and_severity_with_defaults() {
        let (t, action, src, sev) = parse_notify_params(&serde_json::json!({
            "title": "t", "action": { "kind": "open_path", "path": "/x" },
            "source": "vault.large_files", "severity": "warn"
        }))
        .unwrap();
        assert_eq!(t, "t");
        assert_eq!(action, NotificationAction::OpenPath { path: "/x".into() });
        assert_eq!(src.as_deref(), Some("vault.large_files"));
        assert_eq!(sev, Severity::Warn);
    }

    #[test]
    fn parse_open_logs_action() {
        let (_, action, _, _) = parse_notify_params(&serde_json::json!({
            "title": "同步失败",
            "action": { "kind": "open_logs", "filter": "git-sync" }
        }))
        .unwrap();
        assert_eq!(action, NotificationAction::OpenLogs { filter: Some("git-sync".into()) });
    }

    #[test]
    fn parse_rejects_missing_title_or_bad_action() {
        assert!(parse_notify_params(&serde_json::json!({ "action": { "kind": "open_path", "path": "/x" } })).is_err());
        assert!(parse_notify_params(&serde_json::json!({ "title": "t" })).is_err());
        assert!(parse_notify_params(&serde_json::json!({ "title": "t", "action": { "kind": "nope" } })).is_err());
    }

    #[test]
    fn push_mirrors_a_notification_log_line() {
        clear_all();
        crate::log_bus::clear();
        push("《书》摘要已生成".into(), oc("/v/s.md"), None, Severity::Info);
        let last = crate::log_bus::snapshot()
            .into_iter()
            .rev()
            .find(|l| l.category == "notification")
            .expect("有一条 notification 日志");
        assert_eq!(last.message, "《书》摘要已生成");
        assert_eq!(last.level, "info");
        clear_all();
        crate::log_bus::clear();
    }
}
