//! GUI 侧监听器。复用主程序已经热的 `search::IndexHandle` —— 不再开第二个
//! sqlite 句柄,也不跑 freshness sweep(watch 线程已经在保持新鲜)。

use tauri::{AppHandle, Manager};

use crate::mcp::{dispatch, tools::ToolEnv};

pub fn init(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut listener = match crate::platform::ipc::listen().await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("notemd: MCP 监听未启动: {e}");
                return;
            }
        };
        loop {
            let Ok(stream) = listener.accept().await else { continue };
            let app = app.clone();
            tauri::async_runtime::spawn(async move { serve_one(app, stream).await });
        }
    });
}

async fn serve_one(app: AppHandle, stream: crate::platform::ipc::Stream) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (r, mut w) = tokio::io::split(stream);
    let mut lines = BufReader::new(r).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let env = build_env(&app);
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
/// 约束覆盖两个字段,不是各自独立的巧合。
fn build_env(app: &AppHandle) -> Option<ToolEnv> {
    let root = crate::sotvault::resolve_vault_root(app)?;
    Some(ToolEnv {
        vault_root: root,
        index: crate::search::handle(app),
        roots: None,
        open_phase: Some(app.state::<crate::search::OpenState>().get()),
    })
}
