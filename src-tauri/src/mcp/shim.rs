//! `notemd mcp` —— agent 面的 stdio 外壳。
//!
//! **自己不碰索引**:`initialize` / `tools/list` 用编译进来的静态定义直接答
//! (主程序可以没开),`tools/call` 才连 IPC。这不是优化,是必需 ——
//! MCP 客户端在会话启动那一刻枚举工具,而那一刻用户的 note.md 未必开着;
//! 若此时返回空列表,agent 整场会话都不会再问第二次(spec §1.2)。

use std::io::{BufRead, Write};
use std::process::ExitCode;

use crate::mcp::{dispatch, protocol};

pub fn run_shim() -> ExitCode {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() { continue; }
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else { continue };

        // 通知不回。
        if msg.get("id").is_none() { continue; }

        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let reply = if matches!(method, "tools/call") {
            forward(&msg).unwrap_or_else(|e| {
                protocol::tool_error(
                    msg.get("id").unwrap(),
                    &format!("note.md 未运行({e})。启动后即可检索;在此之前请用 grep/rg 兜底。"),
                )
            })
        } else {
            // 静态面:不需要主程序。
            dispatch::handle(None, &msg).unwrap_or_else(|| protocol::error(
                msg.get("id").unwrap(), -32603, "internal: no reply",
            ))
        };
        let _ = writeln!(stdout, "{reply}");
        let _ = stdout.flush();
    }
    ExitCode::SUCCESS
}

/// 一次请求一次连接。MCP 的调用频率远低于建连成本,换来的是外壳完全无状态 ——
/// 主程序中途重启也不需要外壳做任何重连逻辑。
fn forward(msg: &serde_json::Value) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(async {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let stream = crate::platform::ipc::connect().await.map_err(|e| e.to_string())?;
        let (r, mut w) = tokio::io::split(stream);
        w.write_all(format!("{msg}\n").as_bytes()).await.map_err(|e| e.to_string())?;
        w.flush().await.map_err(|e| e.to_string())?;
        let mut lines = BufReader::new(r).lines();
        let line = lines.next_line().await.map_err(|e| e.to_string())?
            .ok_or_else(|| "主程序未回应".to_string())?;
        serde_json::from_str(&line).map_err(|e| e.to_string())
    })
}
