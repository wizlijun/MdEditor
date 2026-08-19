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
    let mut reader = stdin.lock();
    let mut stdout = std::io::stdout();
    let mut supports_roots = false;
    let mut roots: Option<Vec<String>> = None;

    while let Some(msg) = next_msg(&mut reader) {
        if msg.get("id").is_none() {
            // 通知不回。但 initialize 之后 client 会发 initialized,忽略即可。
            continue;
        }
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");

        if method == "initialize" {
            supports_roots = crate::mcp::server::client_supports_roots(
                msg.get("params").unwrap_or(&serde_json::Value::Null),
            );
        }

        if method == "tools/call" && supports_roots && roots.is_none() {
            roots = Some(request_roots(&mut reader, &mut stdout));
        }

        let reply = if method == "tools/call" {
            forward(&msg, roots.as_deref()).unwrap_or_else(|e| {
                protocol::tool_error(
                    msg.get("id").unwrap(),
                    &format!(
                        "note.md 未运行,或 MCP 服务已在设置中关闭({e})。\
                         启动 note.md / 在设置里打开 MCP 服务后即可检索;\
                         在此之前请用 grep/rg 兜底。"
                    ),
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

/// 读下一条 JSON 消息。空行与解析不了的行直接跳过 —— 一行垃圾不该终止会话。
fn next_msg(reader: &mut impl BufRead) -> Option<serde_json::Value> {
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return None, // EOF
            Ok(_) => {}
        }
        let t = line.trim();
        if t.is_empty() { continue; }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
            return Some(v);
        }
    }
}

/// 反向请求 client 的 roots。
///
/// 发出去之后,client 回来的下一条**未必**是这次请求的响应 —— 它完全可以先发
/// 自己的下一个请求。所以这里一边读一边把不相干的消息照常答掉,直到看见
/// `id == "notemd-roots"`。读到 EOF 或对方回 error 就返回空表,让判定落到
/// `Unknown`;**绝不阻塞**,roots 只是加固,不值得拿可用性换。
fn request_roots(reader: &mut impl BufRead, stdout: &mut impl Write) -> Vec<String> {
    const ID: &str = "notemd-roots";
    let _ = writeln!(stdout, r#"{{"jsonrpc":"2.0","id":"{ID}","method":"roots/list"}}"#);
    let _ = stdout.flush();

    while let Some(msg) = next_msg(reader) {
        if msg.get("id").and_then(|v| v.as_str()) == Some(ID) {
            return msg
                .get("result")
                .and_then(|r| r.get("roots"))
                .and_then(|r| r.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.get("uri").and_then(|u| u.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();
        }
        // 不是我们等的那条:照常处理,别把 client 晾着。
        if msg.get("id").is_some() {
            if let Some(reply) = dispatch::handle(None, &msg) {
                let _ = writeln!(stdout, "{reply}");
                let _ = stdout.flush();
            }
        }
    }
    Vec::new()
}

/// 一次请求一次连接。MCP 的调用频率远低于建连成本,换来的是外壳完全无状态 ——
/// 主程序中途重启也不需要外壳做任何重连逻辑。
///
/// 5 秒超时覆盖整个「连接 + 写请求 + 等回应」往返:GUI 侧没起来时
/// `platform::ipc::connect()` 本身可能快速失败,但也可能挂在系统调用上
/// (例如陈旧 socket 文件);不设上限,agent 的这一次工具调用就会无限期挂起。
fn forward(msg: &serde_json::Value, roots: Option<&[String]>) -> Result<serde_json::Value, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all().build().map_err(|e| e.to_string())?;
    let preface = roots.map(|r| serde_json::json!({
        "jsonrpc": "2.0", "method": "notemd/roots", "params": { "roots": r }
    }));
    rt.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            let stream = crate::platform::ipc::connect().await.map_err(|e| e.to_string())?;
            let (r, mut w) = tokio::io::split(stream);
            if let Some(p) = preface {
                w.write_all(format!("{p}\n").as_bytes()).await.map_err(|e| e.to_string())?;
            }
            w.write_all(format!("{msg}\n").as_bytes()).await.map_err(|e| e.to_string())?;
            w.flush().await.map_err(|e| e.to_string())?;
            let mut lines = BufReader::new(r).lines();
            let line = lines.next_line().await.map_err(|e| e.to_string())?
                .ok_or_else(|| "主程序未回应".to_string())?;
            serde_json::from_str::<serde_json::Value>(&line).map_err(|e| e.to_string())
        })
        .await
        .unwrap_or_else(|_| Err("等待主程序超时".to_string()))
    })
}
