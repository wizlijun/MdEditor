//! `host.power_mode.*` —— Power Mode 的配置读写通道。
//!
//! 为什么需要它:特效引擎跑在宿主里(主窗口编辑器 + 下发给插件窗口的 Editor
//! Kit),而配置由 power-mode 插件的窗口编辑。插件窗口是隔离 webview,既没有
//! Tauri IPC 也够不到 settings.json,只能经这两条 RPC 走。
//!
//! 读侧返回的是**生效后**的值:插件没装/被停用就直接 `null`,所以不需要在卸载
//! 路径上补「清理残留配置」的钩子 —— 卸了就不炸。

use serde_json::{json, Value};

/// 本插件的 id。settings.json 里的键、生效面清单的排除项都用它。
pub const PLUGIN_ID: &str = "notemd.power-mode";

/// 一个已加载插件的最小画像,只取判定生效面需要的字段。
#[derive(Debug, Clone)]
pub struct PluginBrief {
    pub id: String,
    pub name: String,
    pub capabilities: Vec<String>,
    /// manifest 的 `i18n` 原样透传(形如 `{"zh": {"name": "…"}}`)。
    pub i18n: Option<Value>,
}

/// 从 settings.json 的整份 JSON 里取出 `plugins.<PLUGIN_ID>.config`。
pub fn config_from_settings(settings: &Value) -> Option<Value> {
    settings.get("plugins")?.get(PLUGIN_ID)?.get("config").cloned()
}

/// 生效后的配置。
///
/// - 插件没装/停用 → `null`(前端据此整体关闭)
/// - 装了但从没配过 → `{}`(前端用默认值:Idea Spark 开、主窗口关)
pub fn effective(installed: bool, settings: &Value) -> Value {
    if !installed {
        return Value::Null;
    }
    config_from_settings(settings).unwrap_or_else(|| json!({}))
}

/// 可作为生效面的插件:已加载、声明了 `editor.kit`、且不是 power-mode 自己
/// (它自己的窗口是实操区,不受生效面开关管)。
///
/// `names` 是 manifest `i18n.<locale>.name` 的映射;插件 UI 按自己的 locale 挑。
pub fn surfaces(plugins: &[PluginBrief]) -> Vec<Value> {
    let mut out: Vec<Value> = plugins
        .iter()
        .filter(|p| p.id != PLUGIN_ID)
        .filter(|p| p.capabilities.iter().any(|c| c == "editor.kit"))
        .map(|p| {
            let mut names = serde_json::Map::new();
            if let Some(Value::Object(map)) = p.i18n.as_ref() {
                for (locale, entry) in map {
                    if let Some(Value::String(n)) = entry.get("name") {
                        names.insert(locale.clone(), Value::String(n.clone()));
                    }
                }
            }
            json!({ "id": p.id, "name": p.name, "names": Value::Object(names) })
        })
        .collect();
    out.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief(id: &str, caps: &[&str], i18n: Option<Value>) -> PluginBrief {
        PluginBrief {
            id: id.into(),
            name: id.into(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
            i18n,
        }
    }

    #[test]
    fn effective_is_null_when_the_plugin_is_not_loaded() {
        let settings = json!({ "plugins": { PLUGIN_ID: { "config": { "surfaces": { "main": true } } } } });
        assert_eq!(effective(false, &settings), Value::Null);
    }

    #[test]
    fn effective_is_empty_object_when_installed_but_never_configured() {
        assert_eq!(effective(true, &json!({})), json!({}));
        assert_eq!(effective(true, &json!({ "plugins": {} })), json!({}));
    }

    #[test]
    fn effective_returns_the_stored_config_verbatim() {
        let settings = json!({ "plugins": { PLUGIN_ID: { "config": { "surfaces": { "main": true } } } } });
        assert_eq!(effective(true, &settings), json!({ "surfaces": { "main": true } }));
    }

    #[test]
    fn surfaces_keeps_only_editor_kit_plugins_and_drops_power_mode_itself() {
        let list = vec![
            brief("notemd.idea-spark", &["editor.kit", "vault.read"], None),
            brief("notemd.roam-import", &["vault.read"], None),
            brief(PLUGIN_ID, &["editor.kit", "power-mode"], None),
        ];
        let out = surfaces(&list);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], "notemd.idea-spark");
    }

    #[test]
    fn surfaces_carries_the_localized_names() {
        let list = vec![brief(
            "notemd.idea-spark",
            &["editor.kit"],
            Some(json!({ "zh": { "name": "奇思妙想" }, "ja": { "name": "アイデアスパーク" } })),
        )];
        let out = surfaces(&list);
        assert_eq!(out[0]["names"]["zh"], "奇思妙想");
        assert_eq!(out[0]["names"]["ja"], "アイデアスパーク");
        assert_eq!(out[0]["name"], "notemd.idea-spark");
    }

    #[test]
    fn surfaces_is_sorted_by_id_for_a_stable_ui_order() {
        let list = vec![
            brief("notemd.zeta", &["editor.kit"], None),
            brief("notemd.alpha", &["editor.kit"], None),
        ];
        let out = surfaces(&list);
        assert_eq!(out[0]["id"], "notemd.alpha");
        assert_eq!(out[1]["id"], "notemd.zeta");
    }
}
