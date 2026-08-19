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

/// 「held handle」≠「活着」的那一位判定。纯函数,只吃 `JoinHandle::
/// is_finished()` 的结果 —— 抽出来是为了不需要真的 `AppHandle` 就能单测
/// `start` 的这一步判断(见 `tests::finished_handle_is_not_considered_running`)。
fn is_still_running(finished: bool) -> bool {
    !finished
}

/// `stop()` 该做的那件事:abort(handle 已经跑完的话这是空操作)+ 删 socket
/// 文件。`start_with` 的「发现 handle 已死」分支与显式 `stop()` 共用这一个
/// 函数 —— 两条路径必须做同一件事,否则「死监听器留下的僵尸 socket」在
/// 重启路径上不会被清,新监听器起来前那个文件还在,复现的正是
/// `platform::ipc::listen()` 文档里写的 `AddrInUse` 那一类失败。
fn cleanup(handle: Option<tauri::async_runtime::JoinHandle<()>>) {
    if let Some(h) = handle {
        h.abort();
    }
    #[cfg(unix)]
    if let Ok(p) = crate::platform::ipc::endpoint() {
        let _ = std::fs::remove_file(p);
    }
}

/// `start` 的核心逻辑,`spawn` 参数化出去 —— 好让测试不需要真的 `AppHandle`
/// 也能喂一个「立刻跑完」的假监听器进来,钉住下面这条修复。
///
/// **幂等,但「幂等」判的是活着,不是「有没有个 handle 挂在那儿」。**
/// `server::spawn_listener` 起的任务会在 `platform::ipc::listen()` 失败时
/// (比如 socket 被占、或残留文件恰好把 `AddrInUse` 判成了「有人在跑」)自己
/// 打完日志就返回 —— 这时 `TASK` 里挂着的是一个**已经跑完**的 handle。旧版本
/// 只看 `is_some()`,于是这种情况下 MCP 永久哑火:往后每次 `start()` 都会
/// 因为「看到 Some」而直接放弃,用户把开关关了又开也救不回来(能救回来是
/// 因为 `stop()` 无条件 `take()`,但没人会知道要这么做)。这里改成先问
/// handle 是否还活着;死的就当成 `stop()` 已经发生过一样清理,再落子新的。
fn start_with(spawn: impl FnOnce() -> tauri::async_runtime::JoinHandle<()>) {
    let mut guard = TASK.lock().unwrap();
    if let Some(h) = guard.as_ref() {
        if is_still_running(h.inner().is_finished()) {
            return;
        }
        cleanup(guard.take());
    }
    *guard = Some(spawn());
}

/// 启动监听。**幂等**:已在跑就什么也不做,不会开出第二个监听器 —— 但见
/// `start_with` 的注释:「在跑」现在是真的问了 handle 还活不活着,不是单看
/// `TASK` 里有没有东西。
pub fn start(app: &AppHandle) {
    let app = app.clone();
    start_with(|| crate::mcp::server::spawn_listener(app));
}

/// 停止监听并**删掉 socket 文件** —— 留着的话外壳会连上一个不再有人 accept 的
/// 端点然后挂住(外壳侧另有超时兜底,但两边都做才对)。
pub fn stop() {
    cleanup(TASK.lock().unwrap().take());
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

    /// `dead_handle_is_replaced_not_treated_as_running` / `live_handle_is_left_alone`
    /// both drive the module-global `TASK` static directly (there's no other way to
    /// unit-test `start_with`'s recovery branch without a real `AppHandle` — this
    /// codebase has never enabled the `tauri::test` feature). Serializes just those
    /// two against each other, mirroring `platform::ipc`'s `IPC_TEST_LOCK` for the
    /// same reason: two tests racing on one shared static/socket path is a false
    /// failure waiting to happen under `cargo test`'s default parallelism.
    static TASK_TEST_LOCK: Mutex<()> = Mutex::new(());
    fn task_test_guard() -> std::sync::MutexGuard<'static, ()> {
        TASK_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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

    /// `is_still_running` 是整个修复的判断核心,单独钉住:跑完了就不算在跑。
    #[test]
    fn finished_handle_is_not_considered_running() {
        assert!(!is_still_running(true));
        assert!(is_still_running(false));
    }

    /// 核心回归用例(review round 1 发现):`start_with` 手上的 handle 一旦已经
    /// 跑完 —— 模拟 `spawn_listener` 因 `listen()` 失败自己提前退出 —— 必须当成
    /// 需要重新起,而不是看见 `Some` 就直接放弃。旧版本只判 `is_some()`,这个
    /// 场景下 MCP 会永久哑火,直到用户手动把设置关了再开。
    ///
    /// 用 `start_with` 而不是 `start`,是因为这个仓库从没启用过 `tauri::test`
    /// feature,拿不到真的 `AppHandle` 去调 `server::spawn_listener` —— 把
    /// spawn 步骤参数化出去,才能不依赖真实 GUI 状态单测这条恢复路径。
    #[test]
    fn dead_handle_is_replaced_not_treated_as_running() {
        let _guard = task_test_guard();
        // 造一个「已经跑完」的 handle:一个立刻返回的任务。轮询等它真正跑完,
        // 不能假设 spawn 后马上就 is_finished() —— 任务是在别的线程上跑的。
        let dead = tauri::async_runtime::spawn(async {});
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !dead.inner().is_finished() {
            assert!(
                std::time::Instant::now() < deadline,
                "spawned no-op task never finished — can't set up the test fixture"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        *TASK.lock().unwrap() = Some(dead);

        let mut spawn_called = false;
        start_with(|| {
            spawn_called = true;
            // 代表一个真的还在跑的监听器:故意不返回。
            tauri::async_runtime::spawn(async {
                std::future::pending::<()>().await;
            })
        });

        assert!(
            spawn_called,
            "a finished handle must not make start_with no-op — it must respawn"
        );
        assert!(
            !TASK.lock().unwrap().as_ref().unwrap().inner().is_finished(),
            "TASK must now hold the freshly spawned (still-running) handle"
        );

        // 不要把一个 pending-forever 的任务泄漏给同进程里的其它测试。
        if let Some(h) = TASK.lock().unwrap().take() {
            h.abort();
        }
    }

    /// 反过来:handle 还活着的时候,`start_with` 必须真的什么都不做 ——
    /// 不能因为这次修复把「幂等」本身改坏了。
    #[test]
    fn live_handle_is_left_alone() {
        let _guard = task_test_guard();
        let live = tauri::async_runtime::spawn(async {
            std::future::pending::<()>().await;
        });
        *TASK.lock().unwrap() = Some(live);

        let mut spawn_called = false;
        start_with(|| {
            spawn_called = true;
            tauri::async_runtime::spawn(async {})
        });

        assert!(!spawn_called, "a live handle must make start_with a no-op");

        if let Some(h) = TASK.lock().unwrap().take() {
            h.abort();
        }
    }
}
