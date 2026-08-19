//! MCP 监听的开关。设置项 `mcpServer.enabled`,**默认开**。
//!
//! 住在应用级 `settings.json`(与前端 `Store.load('settings.json')` 同一个文件),
//! 不是 `.notemd/settings.json` —— 后者随 git 同步,而「这台机器要不要对外提供
//! MCP」是每台机器各自的事。

use std::sync::Mutex;
use tauri::AppHandle;

static TASK: Mutex<Option<tauri::async_runtime::JoinHandle<()>>> = Mutex::new(None);

/// 从已读出的 settings JSON 判定。**键缺失 = 开**;非布尔的损坏值也回落到开 ——
/// 一次误写不该把功能静默关掉。
pub fn enabled_from_value(v: &serde_json::Value) -> bool {
    v.get("mcpServer")
        .and_then(|m| m.get("enabled"))
        .and_then(|e| e.as_bool())
        .unwrap_or(true)
}

pub fn enabled_from_settings(app: &AppHandle) -> bool {
    use tauri_plugin_store::StoreExt;
    let Ok(store) = app.store("settings.json") else { return true };
    let v = store.get("mcpServer").unwrap_or(serde_json::Value::Null);
    enabled_from_value(&serde_json::json!({ "mcpServer": v }))
}

/// 启动监听。**幂等**:已在跑就什么也不做,不会开出第二个监听器。
pub fn start(app: &AppHandle) {
    let mut guard = TASK.lock().unwrap();
    if guard.is_some() {
        return;
    }
    *guard = Some(crate::mcp::server::spawn_listener(app.clone()));
}

/// 停止监听并**删掉 socket 文件** —— 留着的话外壳会连上一个不再有人 accept 的
/// 端点然后挂住(外壳侧另有超时兜底,但两边都做才对)。
pub fn stop() {
    if let Some(h) = TASK.lock().unwrap().take() {
        h.abort();
    }
    #[cfg(unix)]
    if let Ok(p) = crate::platform::ipc::endpoint() {
        let _ = std::fs::remove_file(p);
    }
}

#[tauri::command]
pub fn set_mcp_enabled(app: AppHandle, enabled: bool) {
    if enabled {
        start(&app);
    } else {
        stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// **键缺失即视为开** —— 老用户升级上来不需要做任何事。
    #[test]
    fn absent_key_defaults_to_enabled() {
        assert!(enabled_from_value(&json!({})));
        assert!(enabled_from_value(&json!({ "autoSave": true })));
        assert!(enabled_from_value(&json!({ "mcpServer": {} })));
    }

    #[test]
    fn explicit_false_disables() {
        assert!(!enabled_from_value(&json!({ "mcpServer": { "enabled": false } })));
    }

    #[test]
    fn explicit_true_enables() {
        assert!(enabled_from_value(&json!({ "mcpServer": { "enabled": true } })));
    }

    /// 损坏的值不能把功能意外关掉:非布尔一律回落默认(开)。
    #[test]
    fn malformed_value_falls_back_to_enabled() {
        assert!(enabled_from_value(&json!({ "mcpServer": { "enabled": "no" } })));
        assert!(enabled_from_value(&json!({ "mcpServer": 42 })));
    }
}
