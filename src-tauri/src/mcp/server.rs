//! GUI 侧监听器。复用主程序已经热的 `search::IndexHandle` —— 不再开第二个
//! sqlite 句柄,也不跑 freshness sweep(watch 线程已经在保持新鲜)。

use tauri::{AppHandle, Manager};

use crate::mcp::{dispatch, tools::ToolEnv};

/// Backoff between failed `accept()`s. A single failure is routine (a
/// transient pipe-creation error on Windows, see `platform::ipc::Listener`'s
/// doc comment) and self-heals on the next call; without a delay here a
/// *sustained* failure would spin a tokio worker at 100% CPU forever, since
/// `accept()` can return `Err` without ever awaiting real I/O.
const ACCEPT_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

pub fn init(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut listener = match crate::platform::ipc::listen().await {
            Ok(l) => l,
            Err(e) => {
                // A packaged app has no attached terminal — `eprintln!` here
                // goes nowhere and MCP is then silently dead for the rest of
                // the process's life with no trace anywhere. Route through
                // the log bus (lands in logs/app-YYYY-MM-DD.log + the in-app
                // log window), same channel `search`/`vault_sync` use for
                // backend failures.
                crate::log_cat!("mcp", "error", "MCP 监听未启动: {e}");
                return;
            }
        };
        loop {
            match listener.accept().await {
                Ok(stream) => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move { serve_one(app, stream).await });
                }
                Err(e) => {
                    // Visible + throttled, not a silent instant-retry spin.
                    crate::log_cat!("mcp", "error", "MCP accept 失败: {e}");
                    tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                }
            }
        }
    });
}

async fn serve_one(app: AppHandle, stream: crate::platform::ipc::Stream) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (r, mut w) = tokio::io::split(stream);
    let mut lines = BufReader::new(r).lines();
    // 会话状态:外壳在转发 `tools/call` 之前会先发一条 `notemd/roots` 通知
    // (同一条连接),把它反向问到的 client roots 带过来。这里记住,后续每次
    // `tools/call` 都复用,不必每条消息都问。
    let mut roots: Option<Vec<String>> = None;

    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else { continue };

        if msg.get("method").and_then(|v| v.as_str()) == Some("notemd/roots") {
            roots = msg
                .get("params")
                .and_then(|p| p.get("roots"))
                .and_then(|r| r.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
            continue;
        }

        let mut env = build_env(&app);
        if let Some(e) = env.as_mut() { e.roots = roots.clone(); }
        let reply = dispatch::handle(env.as_ref(), &msg);
        if let Some(reply) = reply {
            if w.write_all(format!("{reply}\n").as_bytes()).await.is_err() { break; }
            let _ = w.flush().await;
        }
    }
}

/// 每次调用重建:vault 可能在会话中途被切换,缓存住就会答错 vault。
///
/// 索引句柄走 `search::handle` —— 这是该模块自己的访问器,不自己
/// `app.state::<IndexHandle>()`。注意它内部用 `app.state()`,未托管会 panic,
/// 所以 `mcp::server::init` **必须**排在 `search::init` 之后(见 lib.rs setup
/// 里的插入位置)。
///
/// `open_phase` 同理读的是 `search::init` 托管的 `OpenState`——同一条排序
/// 约束覆盖两个字段,不是各自独立的巧合。`roots` 由 `serve_one` 在返回后
/// 覆盖:它是会话状态,不是每次都能从 app 里现算的。
fn build_env(app: &AppHandle) -> Option<ToolEnv> {
    let root = crate::sotvault::resolve_vault_root(app)?;
    Some(ToolEnv {
        vault_root: root,
        index: crate::search::handle(app),
        roots: None,
        open_phase: Some(app.state::<crate::search::OpenState>().get()),
    })
}

/// client 是否声明了 roots 能力。没声明就绝不发 `roots/list` ——
/// 对不支持的 client 发请求会挂住,而 roots 只是加固,不值得拿可用性换。
pub(crate) fn client_supports_roots(init_params: &serde_json::Value) -> bool {
    init_params
        .get("capabilities")
        .and_then(|c| c.get("roots"))
        .is_some()
}

#[cfg(test)]
mod tests {
    /// 会话状态:initialize 时记下 client 是否声明了 roots 能力。
    /// 没声明就永远不发 `roots/list` —— 对不支持的 client 发请求会挂住。
    #[test]
    fn session_records_roots_capability() {
        let params = serde_json::json!({
            "capabilities": { "roots": { "listChanged": true } }
        });
        assert!(super::client_supports_roots(&params));
        let params = serde_json::json!({ "capabilities": {} });
        assert!(!super::client_supports_roots(&params));
        assert!(!super::client_supports_roots(&serde_json::json!({})));
    }
}
