//! `notemd mcp` —— agent 面的 stdio 外壳。
//!
//! **自己不碰索引**:`initialize` / `tools/list` 用编译进来的静态定义直接答
//! (主程序可以没开),`tools/call` 才连 IPC。这不是优化,是必需 ——
//! MCP 客户端在会话启动那一刻枚举工具,而那一刻用户的 note.md 未必开着;
//! 若此时返回空列表,agent 整场会话都不会再问第二次(spec §1.2)。

use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use crate::mcp::{dispatch, protocol};

/// 反向 `roots/list` 的等待上限,与 `forward` 已有的 IPC 往返超时同一个数量级
/// (见下面 `forward` 的注释)——roots 只是加固,同样不值得拿可用性换。
const ROOTS_DEADLINE: Duration = Duration::from_secs(5);

pub fn run_shim() -> ExitCode {
    let rx = spawn_reader();
    let mut stdout = std::io::stdout();
    run_loop(&rx, &mut stdout)
}

/// `run_shim`'s actual message loop, with the input channel and output
/// sink parameterized out — purely so a test can drive it with a real
/// `mpsc::Sender` instead of real stdin (mirrors how `request_roots` below
/// is tested). Dropping the paired `Sender` ends the loop the same way EOF
/// does in production.
fn run_loop(rx: &mpsc::Receiver<Line>, stdout: &mut impl Write) -> ExitCode {
    let mut supports_roots = false;
    // `None` = 尚未问过;问过一次(不管拿没拿到)就永远是 `Some`,不再重问 ——
    // 这就是"每个连接只问一次"的全部实现,不需要额外的 bool。
    let mut roots: Option<Vec<String>> = None;

    while let Ok(line) = rx.recv() {
        let msg = match line {
            Line::Msg(v) => v,
            Line::ParseError => {
                reply_parse_error(stdout);
                continue;
            }
        };
        if msg.get("id").is_none() {
            // 通知不回。但 initialize 之后 client 会发 initialized,忽略即可。
            //
            // `notifications/roots/list_changed` (spec §4.2) 是这里唯一需要
            // 真的做点什么的通知:用户中途换了挂载目录,缓存的 `roots` 若
            // 继续沿用,后续每次 `tools/call` 都会照旧宣称 matched/mismatched
            // 针对**旧**挂载点——agent 可能因此把返回路径解析到新目录里一个
            // 同名但不同的文件,恰是握手本身要防的事。清掉缓存,下一次
            // `tools/call` 会照常触发 `request_roots` 重新问一遍。
            if msg.get("method").and_then(|v| v.as_str()) == Some("notifications/roots/list_changed") {
                roots = None;
            }
            continue;
        }
        // 有 `id` 但没有 `method` = 这是一条 JSON-RPC *响应*,不是发给我们的
        // 请求——例如客户端对我们早先发出的某次请求(不是这次 `request_roots`
        // 已经等到、消费掉的那条)迟到的答复。JSON-RPC 的响应与请求共享
        // "有 id" 这一个特征,`method` 才是能把两者分开的字段。误把它当请求
        // 处理会算出 `method == ""`,落进下面的 catch-all,给客户端自己拥有
        // 的 `id` 回一个 `-32601` 错误——对一条响应报错是协议违规,严格的
        // 客户端会记录甚至直接断开会话(finding 2)。
        let Some(method) = msg.get("method").and_then(|v| v.as_str()) else {
            continue;
        };

        if method == "initialize" {
            supports_roots = crate::mcp::server::client_supports_roots(
                msg.get("params").unwrap_or(&serde_json::Value::Null),
            );
        }

        // 这条 `tools/call` 触发了向 client 反向问 roots。等待期间 client 可能
        // 会先发它自己的下一个 `tools/call`——那条不能就地用
        // `dispatch::handle(None, ..)` 答掉(会答成"note.md 未运行",这是假的:
        // GUI 可能好好开着,只是我们还没问完 roots)。`request_roots` 把它们
        // 攒在 `queued_tool_calls` 里,原样按到达顺序补发。
        let mut queued_tool_calls = Vec::new();
        if method == "tools/call" && supports_roots && roots.is_none() {
            let outcome = request_roots(rx, stdout, ROOTS_DEADLINE);
            roots = Some(outcome.roots);
            queued_tool_calls = outcome.queued_tool_calls;
        }

        answer(&msg, roots.as_deref(), stdout);
        for q in queued_tool_calls {
            answer(&q, roots.as_deref(), stdout);
        }
    }
    ExitCode::SUCCESS
}

/// 派发一条消息并把回复写到 stdout。`tools/call` 走 IPC(`forward`);其余
/// 都是编译进来的静态面,`dispatch::handle(None, ..)` 足够。
fn answer(msg: &serde_json::Value, roots: Option<&[String]>, stdout: &mut impl Write) {
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let reply = if method == "tools/call" {
        forward(msg, roots).unwrap_or_else(|e| {
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
        dispatch::handle(None, msg).unwrap_or_else(|| {
            protocol::error(msg.get("id").unwrap(), -32603, "internal: no reply")
        })
    };
    let _ = writeln!(stdout, "{reply}");
    let _ = stdout.flush();
}

/// 一行输入解析后的结果:有效 JSON,还是解析失败。空行不算数,直接跳过。
enum Line {
    Msg(serde_json::Value),
    /// 非空但解析不了的一行。调用方决定怎么答(通常是 -32700 + null id),
    /// 这里只负责识别,不负责回复。
    ParseError,
}

/// 读下一条输入。空行跳过;EOF 才真正结束会话。
fn next_msg(reader: &mut impl BufRead) -> Option<Line> {
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return None, // EOF
            Ok(_) => {}
        }
        let t = line.trim();
        if t.is_empty() { continue; }
        return Some(match serde_json::from_str::<serde_json::Value>(t) {
            Ok(v) => Line::Msg(v),
            Err(_) => Line::ParseError,
        });
    }
}

/// stdin 读取搬到后台线程,主循环从 channel 收——这是能给
/// `request_roots` 一个真实截止时间的唯一办法:`std::io::Stdin::read_line`
/// 本身不可中断、也没有超时参数,想在"等 client 回应"上设上限,只能不让主
/// 循环直接阻塞在它上面。线程在 EOF 时自然退出,发送端随之被丢弃,
/// `rx.recv()`/`rx.recv_timeout()` 之后统一收到「断开」,与原来"EOF 结束会话"
/// 的语义完全对齐。
fn spawn_reader() -> mpsc::Receiver<Line> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        while let Some(line) = next_msg(&mut reader) {
            if tx.send(line).is_err() { break; } // 主循环已经退出,别再读了
        }
    });
    rx
}

/// `reply_parse_error` 的语义就是它自己的名字——JSON-RPC 的约定答法:
/// 解析失败连 `id` 都取不出来,答 `null`。
fn reply_parse_error(stdout: &mut impl Write) {
    let reply = protocol::error(&serde_json::Value::Null, -32700, "parse error: invalid JSON");
    let _ = writeln!(stdout, "{reply}");
    let _ = stdout.flush();
}

/// `request_roots` 的结果:问到的 roots(拿不到就是空表),以及等待期间攒下的
/// `tools/call`(需要在外面用真正的 `forward` 补答,不能已经被就地答掉)。
struct RootsOutcome {
    roots: Vec<String>,
    queued_tool_calls: Vec<serde_json::Value>,
}

/// 反向请求 client 的 roots,带截止时间。
///
/// 发出去之后,client 回来的下一条**未必**是这次请求的响应——它完全可以先发
/// 自己的下一个请求,所以这里一边等一边把不相干的消息照常处理,直到看见
/// `id == "notemd-roots"`。
///
/// **这里必须有自己的截止时间,不能只靠 EOF 兜底。** 触发这次反向请求的
/// 那条 `tools/call` 本身就在等我们的回复——如果 client 在等我们的时候恰好
/// 也不打算在我们的 `roots/list` 问完之前说别的话(它没有义务说),两边就会
/// 互相等对方先开口,而 client 还活着、管道还开着,EOF 永远不会来。之前的
/// 版本只在 EOF 上退出,这就是一个真实的死锁,不是理论上的边界情况——
/// 这个函数存在的全部理由就是"绝不阻塞",所以截止时间是它的核心部分,
/// 不是可选的加固。超时就放弃,回落到空 roots,让上层判定落到 `Unknown`。
///
/// 等待期间看到的 `tools/call` 不能用 `dispatch::handle(None, ..)` 就地答掉——
/// 那条路径只要 `env` 是 `None` 就无条件答"note.md 未运行",而这里的
/// `None` 只是"这条内部检查不需要 env",不代表 GUI 真的不在。就地这么答会
/// 把一个可能存在的 GUI 报成不存在,agent 可能因此整场会话退回 grep。
/// 所以这类消息只入队,交回调用方按普通 `tools/call` 走真正的 `forward`。
fn request_roots(
    rx: &mpsc::Receiver<Line>,
    stdout: &mut impl Write,
    deadline: Duration,
) -> RootsOutcome {
    const ID: &str = "notemd-roots";
    let _ = writeln!(stdout, r#"{{"jsonrpc":"2.0","id":"{ID}","method":"roots/list"}}"#);
    let _ = stdout.flush();

    let mut queued_tool_calls = Vec::new();
    let deadline_at = std::time::Instant::now() + deadline;
    loop {
        let remaining = deadline_at.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return RootsOutcome { roots: Vec::new(), queued_tool_calls };
        }
        let line = match rx.recv_timeout(remaining) {
            Ok(l) => l,
            // 超时或对端断开(client 退出/EOF):两种情况都一样,放弃 roots。
            Err(_) => return RootsOutcome { roots: Vec::new(), queued_tool_calls },
        };
        let msg = match line {
            Line::Msg(v) => v,
            Line::ParseError => {
                reply_parse_error(stdout);
                continue;
            }
        };
        if msg.get("id").and_then(|v| v.as_str()) == Some(ID) {
            let roots = msg
                .get("result")
                .and_then(|r| r.get("roots"))
                .and_then(|r| r.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.get("uri").and_then(|u| u.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            return RootsOutcome { roots, queued_tool_calls };
        }
        if msg.get("id").is_none() { continue; } // 通知,不回也不需要处理
        // 有 id 没 method = 响应,不是发给我们的请求(见 `run_shim` 里同一条
        // 判断的注释——两处必须一致,都不能把响应当成请求答复一个 -32601)。
        let Some(method) = msg.get("method").and_then(|v| v.as_str()) else { continue };
        if method == "tools/call" {
            queued_tool_calls.push(msg);
        } else if let Some(reply) = dispatch::handle(None, &msg) {
            // 静态面(initialize/tools/list/ping/…):不需要 env,就地答是对的。
            let _ = writeln!(stdout, "{reply}");
            let _ = stdout.flush();
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_call(id: i64) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": "vault_info", "arguments": {} }
        })
    }

    /// The critical fix: a client that never answers `roots/list` must not
    /// hang `request_roots` forever. With nothing ever sent on the channel,
    /// this must return (empty roots, no queued calls) once `deadline`
    /// elapses — not block until the sender is dropped or the heat death of
    /// the universe, whichever comes first.
    #[test]
    fn gives_up_on_roots_after_deadline_instead_of_blocking_forever() {
        let (_tx, rx) = mpsc::channel::<Line>();
        let mut out: Vec<u8> = Vec::new();
        let start = std::time::Instant::now();
        let outcome = request_roots(&rx, &mut out, Duration::from_millis(30));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "request_roots must return promptly once its deadline elapses, took {:?}",
            start.elapsed()
        );
        assert!(outcome.roots.is_empty());
        assert!(outcome.queued_tool_calls.is_empty());
    }

    /// A `tools/call` seen while waiting for the roots response must be
    /// queued, not answered inline — answering it via `dispatch::handle(None,
    /// ..)` here would unconditionally claim "note.md is not running", which
    /// may well be false.
    #[test]
    fn tools_call_seen_while_waiting_is_queued_not_answered_as_not_running() {
        let (tx, rx) = mpsc::channel::<Line>();
        tx.send(Line::Msg(tool_call(42))).unwrap();
        let mut out: Vec<u8> = Vec::new();
        let outcome = request_roots(&rx, &mut out, Duration::from_millis(50));

        assert_eq!(outcome.queued_tool_calls.len(), 1);
        assert_eq!(outcome.queued_tool_calls[0]["id"], 42);
        // Nothing must have been written for id 42 yet — in particular, no
        // premature "note.md not running" tool_error.
        let written = String::from_utf8(out).unwrap();
        assert!(!written.contains("42"), "must not answer the queued call inline: {written}");
    }

    /// A non-`tools/call` request seen while waiting (e.g. the client's next
    /// `tools/list`) is still answered inline, unchanged from before — these
    /// are the compiled-in static answers that need no env.
    #[test]
    fn non_tool_call_request_seen_while_waiting_is_still_answered_inline() {
        let (tx, rx) = mpsc::channel::<Line>();
        tx.send(Line::Msg(serde_json::json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list" })))
            .unwrap();
        let mut out: Vec<u8> = Vec::new();
        let _ = request_roots(&rx, &mut out, Duration::from_millis(50));

        let written = String::from_utf8(out).unwrap();
        assert!(written.contains("\"id\":7"), "expected an inline reply to id 7: {written}");
    }

    /// The happy path still works: a matching `id: "notemd-roots"` response
    /// ends the wait immediately with the roots it carried.
    #[test]
    fn matching_response_returns_roots_and_stops_waiting() {
        let (tx, rx) = mpsc::channel::<Line>();
        tx.send(Line::Msg(serde_json::json!({
            "jsonrpc": "2.0", "id": "notemd-roots",
            "result": { "roots": [{ "uri": "file:///a" }] }
        })))
        .unwrap();
        let mut out: Vec<u8> = Vec::new();
        let outcome = request_roots(&rx, &mut out, Duration::from_secs(5));
        assert_eq!(outcome.roots, vec!["file:///a".to_string()]);
        assert!(outcome.queued_tool_calls.is_empty());
    }

    /// finding 2: a stray JSON-RPC *response* (has `id`, no `method` — e.g. a
    /// late reply to some earlier request of the client's own) arriving while
    /// waiting for `notemd-roots` must be silently skipped, not answered.
    /// Before the fix this fell through the old `unwrap_or("")` into the
    /// catch-all, sending the client an error keyed on an id the client
    /// itself owns — a protocol violation strict clients may end the session
    /// over.
    #[test]
    fn stray_response_while_waiting_is_ignored_not_answered() {
        let (tx, rx) = mpsc::channel::<Line>();
        tx.send(Line::Msg(serde_json::json!({
            "jsonrpc": "2.0", "id": 99, "result": { "ok": true }
        })))
        .unwrap();
        tx.send(Line::Msg(serde_json::json!({
            "jsonrpc": "2.0", "id": "notemd-roots", "result": { "roots": [] }
        })))
        .unwrap();
        let mut out: Vec<u8> = Vec::new();
        let outcome = request_roots(&rx, &mut out, Duration::from_secs(5));
        assert!(outcome.roots.is_empty());
        assert!(outcome.queued_tool_calls.is_empty());
        // `out` does carry our own outgoing `roots/list` request (written
        // before the wait loop even starts) — that is not a reply to
        // anything. What must never appear is a reply keyed on id `99`,
        // which the client itself owns.
        let written = String::from_utf8(out).unwrap();
        assert!(!written.contains("99"), "a stray response must draw no reply at all: {written}");
    }

    /// A garbage line arriving while waiting still gets the conventional
    /// -32700 reply, and does not derail the wait for the real response.
    #[test]
    fn garbage_while_waiting_gets_parse_error_and_wait_continues() {
        let (tx, rx) = mpsc::channel::<Line>();
        tx.send(Line::ParseError).unwrap();
        tx.send(Line::Msg(serde_json::json!({
            "jsonrpc": "2.0", "id": "notemd-roots", "result": { "roots": [] }
        })))
        .unwrap();
        let mut out: Vec<u8> = Vec::new();
        let outcome = request_roots(&rx, &mut out, Duration::from_secs(5));
        assert!(outcome.roots.is_empty());
        let written = String::from_utf8(out).unwrap();
        assert!(written.contains("-32700"), "{written}");
    }

    /// finding 8 / spec §4.2: `notifications/roots/list_changed` must
    /// invalidate the cached roots, not be silently dropped. Drives the full
    /// `run_loop` (not just `request_roots`) through: `initialize` declaring
    /// roots support, a `tools/call` that fetches roots once, the
    /// `list_changed` notification, then a second `tools/call` — which must
    /// re-fetch (a second outgoing `roots/list` request) rather than reuse
    /// the stale cache. Before the fix, the notification fell into the
    /// no-`method`-handling "notification, nothing to do" branch and the
    /// second `tools/call` never asked again — every later response kept
    /// judging the *old* mount, exactly the harm the handshake exists to
    /// prevent.
    #[test]
    fn list_changed_notification_invalidates_cached_roots() {
        let (tx, rx) = mpsc::channel::<Line>();
        tx.send(Line::Msg(serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "capabilities": { "roots": { "listChanged": true } } }
        })))
        .unwrap();
        tx.send(Line::Msg(tool_call(2))).unwrap();
        tx.send(Line::Msg(serde_json::json!({
            "jsonrpc": "2.0", "id": "notemd-roots", "result": { "roots": [{ "uri": "file:///old" }] }
        })))
        .unwrap();
        tx.send(Line::Msg(serde_json::json!({
            "jsonrpc": "2.0", "method": "notifications/roots/list_changed"
        })))
        .unwrap();
        tx.send(Line::Msg(tool_call(3))).unwrap();
        tx.send(Line::Msg(serde_json::json!({
            "jsonrpc": "2.0", "id": "notemd-roots", "result": { "roots": [{ "uri": "file:///new" }] }
        })))
        .unwrap();
        drop(tx); // ends the loop like EOF would

        let mut out: Vec<u8> = Vec::new();
        run_loop(&rx, &mut out);

        let written = String::from_utf8(out).unwrap();
        let roots_list_requests = written.matches(r#""method":"roots/list""#).count();
        assert_eq!(
            roots_list_requests, 2,
            "must re-fetch roots after list_changed instead of reusing the stale cache: {written}"
        );
    }
}
