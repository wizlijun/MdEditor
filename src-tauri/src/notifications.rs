//! 托盘全局提醒注册表。任何插件经 `host.notify`(capability `notify`)推入一条
//! 提醒;托盘出现「● N」蓝点子菜单与数字角标,点击执行 action 并消掉该条。
//! 注册表是进程级全局(仿 plugin_runtime::STATE):HostServices 是泛型
//! `R: Runtime`,碰不了 Wry 专用的托盘刷新,所以写方只改数据 + 敲 DIRTY,
//! 由 lib.rs setup 里持有 Wry handle 的守望任务负责重建菜单。
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotificationAction {
    /// 绝对路径:聚焦主窗口并在编辑器打开该文件。
    OpenPath { path: String },
    /// 打开某插件贡献的窗口(失败提醒指向 claude-agent 的运行日志)。
    OpenPluginWindow { plugin_id: String, window: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub id: u64,
    pub title: String,
    pub action: NotificationAction,
}

#[derive(Debug, Default)]
pub struct Registry {
    next_id: u64,
    items: Vec<Notification>,
}

impl Registry {
    pub fn push(&mut self, title: String, action: NotificationAction) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Notification { id, title, action });
        id
    }
    pub fn take(&mut self, id: u64) -> Option<Notification> {
        let i = self.items.iter().position(|r| r.id == id)?;
        Some(self.items.remove(i))
    }
    pub fn clear(&mut self) {
        self.items.clear();
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

pub fn push(title: String, action: NotificationAction) -> u64 {
    let id = REGISTRY.lock().unwrap().push(title, action);
    DIRTY.notify_one();
    id
}
pub fn take(id: u64) -> Option<Notification> {
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
pub fn snapshot() -> Vec<Notification> {
    REGISTRY.lock().unwrap().items().to_vec()
}

/// `host.notify` 参数 → (title, action)。
pub fn parse_notify_params(v: &serde_json::Value) -> Result<(String, NotificationAction), String> {
    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("host.notify needs a non-empty 'title'")?;
    let action = v.get("action").cloned().ok_or("host.notify needs an 'action'")?;
    let action: NotificationAction = serde_json::from_value(action).map_err(|e| format!("bad action: {e}"))?;
    Ok((title.to_string(), action))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_assigns_increasing_ids_and_take_removes() {
        let mut r = Registry::default();
        let a = r.push("A".into(), NotificationAction::OpenPath { path: "/x/a.md".into() });
        let b = r.push("B".into(), NotificationAction::OpenPath { path: "/x/b.md".into() });
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
        r.push("A".into(), NotificationAction::OpenPath { path: "/x".into() });
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
        assert_eq!(action, NotificationAction::OpenPath { path: "/v/ssot/ebooks/2026-08/书/2026-08-04-summary.md".into() });
    }

    #[test]
    fn parse_open_plugin_window() {
        let (_, action) = parse_notify_params(&serde_json::json!({
            "title": "失败",
            "action": { "kind": "open_plugin_window", "plugin_id": "notemd.claude-agent", "window": "main" }
        }))
        .unwrap();
        assert_eq!(action, NotificationAction::OpenPluginWindow { plugin_id: "notemd.claude-agent".into(), window: "main".into() });
    }

    #[test]
    fn parse_rejects_missing_title_or_bad_action() {
        assert!(parse_notify_params(&serde_json::json!({ "action": { "kind": "open_path", "path": "/x" } })).is_err());
        assert!(parse_notify_params(&serde_json::json!({ "title": "t" })).is_err());
        assert!(parse_notify_params(&serde_json::json!({ "title": "t", "action": { "kind": "nope" } })).is_err());
    }
}
