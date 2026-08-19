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

/// Abort a held task handle. Harmless if it has already finished (the
/// dead-handle recovery path in `start_with`) or is genuinely still running
/// (the explicit `stop()` path) — both call this on their way out. This is
/// **not** paired with socket-file cleanup here; see `start_with` and
/// `stop()` for why those two paths deliberately disagree on that.
fn abort(handle: Option<tauri::async_runtime::JoinHandle<()>>) {
    if let Some(h) = handle {
        h.abort();
    }
}

/// Best-effort delete of the unix socket file at `path`. Windows named pipes
/// have no filesystem entry, so callers only invoke this under `#[cfg(unix)]`.
///
/// Takes the path as a parameter rather than resolving
/// `platform::ipc::endpoint()` internally, purely so a test can point this at
/// a scratch file instead of the machine's real, shared MCP socket — see
/// `tests::removes_the_file_it_is_given`. No test in this module may call
/// `platform::ipc::endpoint()` and pass its result here; that would delete
/// (or fight over) the one socket path every note.md instance on the machine
/// shares, including a real GUI that may be serving MCP while `cargo test`
/// runs.
#[cfg(unix)]
fn remove_socket_file(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

/// `start`'s core logic, with `spawn` parameterized out — so a test can hand
/// it an `AppHandle`-free stand-in listener and pin the recovery behavior
/// below without needing a real Tauri app.
///
/// **Idempotent, but "idempotent" is judged by liveness, not by "is there a
/// handle sitting in `TASK`".** `server::spawn_listener`'s task self-
/// terminates — logs and returns — when `platform::ipc::listen()` fails,
/// leaving a *finished* handle behind. The old version only checked
/// `is_some()`, so that case wedged MCP off forever: every later `start()`
/// saw `Some` and gave up, and the only way back was the user happening to
/// flip the setting off and on (which works only because `stop()`
/// unconditionally `take()`s — nobody would know that's what fixes it).
///
/// **The dead-handle branch deliberately does NOT remove the socket file —
/// unlike `stop()` below.** Read `platform::ipc::listen()` (unix): it
/// connect-probes the existing path *before* ever unlinking it. If the probe
/// succeeds, `listen()` returns `AddrInUse` and leaves the file alone (that
/// probe is the whole "don't evict a live listener" invariant, also covered
/// by `platform::ipc`'s `live_listener_is_not_evicted` test). If the probe
/// fails, the socket is genuinely stale and `listen()` unlinks it and binds
/// successfully — so that case never produces a dead handle at all; it
/// self-heals inside `listen()`. `spawn_listener` calls `listen()` exactly
/// once with no retry, so the *only* way this branch is ever reached is the
/// first case: a live sibling instance currently owns the socket. Removing
/// the file here would silently kill that peer's reachability (new clients
/// get `ENOENT` on a path nobody accepts on) while the peer keeps running,
/// unaware — exactly the harm the probe exists to prevent, reintroduced
/// through this module instead. `listen()` is the one place with the probe
/// to make that call correctly; this function does not duplicate the
/// decision without it. It only aborts the dead handle and lets the
/// subsequent `listen()` call (inside the freshly spawned task) sort out
/// stale-vs-live on its own, the same way it always does.
fn start_with(spawn: impl FnOnce() -> tauri::async_runtime::JoinHandle<()>) {
    let mut guard = TASK.lock().unwrap();
    if let Some(h) = guard.as_ref() {
        if is_still_running(h.inner().is_finished()) {
            return;
        }
        abort(guard.take());
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

/// Stop listening and **remove the socket file** — unlike the dead-handle
/// path in `start_with` above, this one always removes it, no probe needed.
/// An explicit `stop()` is the user asking *this machine's own* listener to
/// go away right now: there is no "might be a live sibling" ambiguity to
/// resolve, because we are the one who just tore it down. Leaving the file
/// behind here is exactly what makes the shell hang connecting to an
/// endpoint nobody accepts on (the shell has its own timeout as a backstop,
/// but both ends should behave).
pub fn stop() {
    abort(TASK.lock().unwrap().take());
    #[cfg(unix)]
    if let Ok(p) = crate::platform::ipc::endpoint() {
        remove_socket_file(&p);
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
    ///
    /// 这条测试也顺带钉住了 review round 2 的 Finding 1:`start_with` 的死
    /// handle 分支只调 `abort`,不碰任何 socket 文件(不像 `stop()`)—— 所以
    /// 这条测试从不触碰机器上真实的 MCP 端点,与 `removes_the_file_it_is_given`
    /// 依赖的隔离是同一件事的两面。
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

    /// Pins `remove_socket_file`'s one job — delete the file at the path
    /// it's given — against a scratch temp file, **never**
    /// `platform::ipc::endpoint()`'s real path (review round 2, Finding 2:
    /// a prior version of this test suite called the real endpoint-touching
    /// cleanup directly, which would delete the one MCP socket every
    /// note.md instance on the machine shares — including a real GUI
    /// serving MCP while `cargo test` runs). The parameterized signature is
    /// what makes that avoidable: production `stop()` still resolves the
    /// real endpoint, but the removal logic itself is tested in isolation.
    #[cfg(unix)]
    #[test]
    fn removes_the_file_it_is_given() {
        let path = std::env::temp_dir()
            .join(format!("notemd-gate-test-{}.sock", std::process::id()));
        std::fs::write(&path, b"").unwrap();
        assert!(path.exists());
        remove_socket_file(&path);
        assert!(!path.exists());
    }

    /// Same function, missing file: must not panic (mirrors "the listener
    /// was never started" / "someone already cleaned it up" cases).
    #[cfg(unix)]
    #[test]
    fn tolerates_a_missing_file() {
        let path = std::env::temp_dir()
            .join(format!("notemd-gate-test-missing-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        assert!(!path.exists());
        remove_socket_file(&path); // must not panic
    }
}
