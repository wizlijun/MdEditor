//! One live v2 plugin process: NDJSON JSON-RPC channel + supervision hooks.
//!
//! Deviations from the plan's Task 5 draft (each pinned by an integration
//! test in `tests/plugin_runtime_integration.rs`):
//! - When the stdout reader loop ends (process died / closed stdout), the
//!   `pending` map is drained so in-flight `request()` calls fail fast with
//!   "channel closed" instead of hanging out their full timeout.
//! - `shutdown()` only reports graceful when `$deactivate` actually
//!   *succeeded* within the grace window (the draft's `.is_ok()` on the
//!   timeout wrapper alone would report a dead process as graceful), and it
//!   force-kills if the process still hasn't exited after the post-grace wait.
//! - `request()` removes its `pending` entry when the stdin write fails
//!   (the draft leaked the entry).
//! - The draft's `try_wait_exit() -> Option<i32>` (`.and_then(|s| s.code())`)
//!   conflated "still running" with "killed by signal"; replaced by
//!   `has_exited() -> Option<i32>` where `None` = still running and
//!   signal-death maps to `-1` (Task 6's crash supervision needs the
//!   distinction).
//! - `kill_on_drop(true)` so an abandoned channel can't leak a live child.
//! - `append_plugin_log(dir, plugin_id, level, msg)` is the shared log
//!   helper (Task 7 reuses it for `host.log.*`); stderr capture logs with
//!   level "stderr". Files roll to `.log.1` past 5MB.

use plugin_protocol as proto;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::{oneshot, Mutex};

pub const DEACTIVATE_GRACE_SECS: u64 = 5; // spec §4.2
pub const DEFAULT_REQUEST_TIMEOUT: u64 = 30; // spec §2
pub const MAX_REQUEST_TIMEOUT: u64 = 300;

/// 宿主侧回调：插件发来的 host.* 请求/通知（由 host_api 处理）。
pub type HostSink = Arc<dyn Fn(proto::RpcRequest) -> Option<proto::RpcResponse> + Send + Sync>;

pub struct PluginProcess {
    child: Mutex<Child>,
    stdin: Mutex<tokio::process::ChildStdin>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<proto::RpcResponse>>>>,
    next_id: AtomicU64,
    /// **静默上限,不是耗时上限**:一个请求只有在该进程「彻底不吭声」这么久
    /// 之后才判超时(见 [`PluginProcess::request`])。
    pub request_timeout: Duration,
    /// 该进程最近一次吐出任意一行 stdout 的时刻 —— 「还活着且在干活」的证据。
    last_output: std::sync::Mutex<std::time::Instant>,
    /// 读循环任务与进程退出监视由 lifecycle 持有的 JoinHandle 管理。
    pub reader_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl PluginProcess {
    /// spawn + 启动读循环。stderr 追加写入 `<log_dir>/<plugin_id>.log`
    /// （上限滚动 5MB：超限时 rename 为 .1 重开）。
    pub async fn spawn(
        binary: &Path,
        plugin_id: &str,
        log_dir: &Path,
        timeout_secs: u64,
        host_sink: HostSink,
    ) -> Result<Arc<Self>, String> {
        let mut cmd = crate::platform::tokio_command(binary);
        // Sanitize the child environment: native plugins are trusted by
        // signature, but inheriting the full app environment would needlessly
        // leak host secrets (API keys etc.) into every plugin subprocess —
        // pointless exposure, especially ahead of third-party plugins. Clear
        // everything, then re-add a minimal safe allowlist. openclaw needs
        // HOME/PATH to resolve its UDS socket path; these are covered.
        // Do NOT add secret-bearing vars here.
        // The allowlist is per-platform: on Windows a bare `env_clear()` is
        // fatal rather than merely restrictive — without `SystemRoot` the
        // loader cannot initialise and the child dies before `main`. See
        // `platform::plugin_env_allowlist`.
        cmd.env_clear();
        for key in crate::platform::plugin_env_allowlist() {
            if let Ok(v) = std::env::var(key) {
                cmd.env(key, v);
            }
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let proc = Arc::new(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            pending: Default::default(),
            next_id: AtomicU64::new(1),
            request_timeout: Duration::from_secs(timeout_secs.clamp(1, MAX_REQUEST_TIMEOUT)),
            last_output: std::sync::Mutex::new(std::time::Instant::now()),
            reader_task: Mutex::new(None),
        });
        // stderr → 日志文件
        {
            let log_dir = log_dir.to_path_buf();
            let plugin_id = plugin_id.to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    append_plugin_log(&log_dir, &plugin_id, "stderr", &l);
                }
            });
        }
        // stdout 读循环：response → pending；request/notification → host_sink，
        // 有 id 的把 host_sink 的应答写回插件 stdin。
        //
        // host_sink 绝不在读循环里跑。它是同步的、而且可以**跑很久** —— host.agent.*
        // 是转给另一个插件的中转,要等对方答复;一旦它在读循环里阻塞,这个进程的
        // 应答就没人路由了:插件明明早就回了话,调用方还是会被判超时(v6.804.5 的
        // 「agent relay timeout after 300s」正是如此:AI 阅读实际 4 分 48 秒跑完并
        // 成功落盘,调用方却收到超时)。所以 host_sink 挪到自己的线程上,读循环只
        // 做「解析 + 路由 + 入队」,永远通畅。队列是单线程 FIFO,插件的 host.* 之间
        // 仍按到达顺序处理。
        let (sink_tx, sink_rx) = std::sync::mpsc::channel::<proto::RpcRequest>();
        let (reply_tx, mut reply_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        std::thread::spawn(move || {
            while let Ok(req) = sink_rx.recv() {
                if let Some(resp) = host_sink(req) {
                    if reply_tx.send(serde_json::to_string(&resp).unwrap()).is_err() {
                        break;
                    }
                }
            }
        });
        {
            let me = proc.clone();
            tokio::spawn(async move {
                while let Some(line) = reply_rx.recv().await {
                    if me.write_line(&line).await.is_err() {
                        break;
                    }
                }
            });
        }
        {
            let me = proc.clone();
            let task = tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    // 任何一行都算「在干活」,包括进度推送 —— request() 的静默计时
                    // 从这里重新起算。
                    *me.last_output.lock().unwrap() = std::time::Instant::now();
                    if line.trim().is_empty() {
                        continue;
                    }
                    if let Ok(resp) = serde_json::from_str::<proto::RpcResponse>(&line) {
                        if resp.result.is_some() || resp.error.is_some() {
                            if let Some(tx) = me.pending.lock().await.remove(&resp.id) {
                                let _ = tx.send(resp);
                            }
                            continue;
                        }
                    }
                    if let Ok(req) = serde_json::from_str::<proto::RpcRequest>(&line) {
                        // 队列断了(sink 线程没了)只影响 host.* 的处理,读循环照常。
                        let _ = sink_tx.send(req);
                    }
                }
                // Reader gone ⇒ no response can ever arrive: drop every pending
                // sender so in-flight `request()` calls fail fast ("channel
                // closed") instead of sitting out their full timeout.
                me.pending.lock().await.clear();
                // sink_tx 在这里落地 ⇒ sink 线程排空后退出 ⇒ reply_tx 落地 ⇒
                // 回信任务退出。三者不会有一个残留。
            });
            *proc.reader_task.lock().await = Some(task);
        }
        Ok(proc)
    }

    async fn write_line(&self, l: &str) -> Result<(), String> {
        let mut w = self.stdin.lock().await;
        w.write_all(l.as_bytes()).await.map_err(|e| e.to_string())?;
        w.write_all(b"\n").await.map_err(|e| e.to_string())?;
        w.flush().await.map_err(|e| e.to_string())
    }

    /// 带超时的宿主→插件 request。超时只 fail 该请求（spec §12），不杀进程。
    ///
    /// 超时是**静默超时**,不是总耗时上限:计时在该进程每吐一行 stdout 时重新
    /// 起算,所以一个还在推进度、还在流式输出的插件永远不会被判超时 —— 只有
    /// 「一声不吭满 `request_timeout`」才算失联。总耗时封顶交给业务侧(任务自己
    /// 的 timeout、用户点取消、进程死亡时的 pending drain),而不是在这里把一个
    /// 正在正常干活的长任务腰斩。
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, mut rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let req = proto::RpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(id),
            method: method.into(),
            params,
        };
        if let Err(e) = self.write_line(&serde_json::to_string(&req).unwrap()).await {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }
        let sent_at = std::time::Instant::now();
        let received = loop {
            // 静默从「本请求发出」和「进程最后一次出声」里更晚的那个算起。
            let quiet_since = (*self.last_output.lock().unwrap()).max(sent_at);
            let deadline = quiet_since + self.request_timeout;
            let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) else {
                self.pending.lock().await.remove(&id);
                return Err(format!(
                    "timeout:{} (no output from the plugin for that long)",
                    self.request_timeout.as_secs()
                ));
            };
            match tokio::time::timeout(left, &mut rx).await {
                Ok(r) => break r,
                // 窗口内进程又出过声 ⇒ 下一轮 deadline 自动后移,继续等。
                Err(_) => continue,
            }
        };
        match received {
            Err(_) => Err("channel closed (process died?)".into()),
            Ok(resp) => match (resp.result, resp.error) {
                (Some(v), _) => Ok(v),
                (_, Some(e)) => Err(format!("plugin error {}: {}", e.code, e.message)),
                _ => Err("empty response".into()),
            },
        }
    }

    /// $deactivate → 等 5s 优雅退出 → 超时/失败 kill。返回是否优雅。
    pub async fn shutdown(&self) -> bool {
        // Graceful only if $deactivate actually succeeded within the grace
        // window: a dead process errors immediately (Ok(Err(..))), which must
        // not count as graceful.
        let graceful = matches!(
            tokio::time::timeout(
                Duration::from_secs(DEACTIVATE_GRACE_SECS),
                self.request("$deactivate", Value::Null),
            )
            .await,
            Ok(Ok(_))
        );
        let mut child = self.child.lock().await;
        if !graceful {
            let _ = child.start_kill();
        }
        if tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .is_err()
        {
            // Acknowledged $deactivate but won't exit: force-kill.
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
        graceful
    }

    /// 非阻塞检查进程是否已退出（供 lifecycle 崩溃监督轮询/等待）。
    /// `None` = 仍在运行；`Some(code)` = 已退出，被信号杀死映射为 -1。
    pub async fn has_exited(&self) -> Option<i32> {
        self.child
            .lock()
            .await
            .try_wait()
            .ok()
            .flatten()
            .map(|s| s.code().unwrap_or(-1))
    }
}

/// 共享日志 helper（Task 7 的 host.log.* 复用）：向 `<dir>/<plugin_id>.log`
/// 追加一行 `[{level}] {msg}`。
pub(crate) fn append_plugin_log(dir: &Path, plugin_id: &str, level: &str, msg: &str) {
    // Mirror into the unified log bus (category=plugin:<id>). File append below
    // is untouched. Map raw stderr level to a bus level.
    let bus_level = match level {
        "debug" | "info" | "warn" | "error" => level,
        _ => "info", // e.g. "stderr"
    };
    crate::log_bus::push_cat(&format!("plugin:{plugin_id}"), "backend", bus_level, msg.to_string());
    let _ = std::fs::create_dir_all(dir);
    append_log_line(&dir.join(format!("{plugin_id}.log")), &format!("[{level}] {msg}"));
}

fn append_log_line(path: &Path, line: &str) {
    use std::io::Write;
    const MAX: u64 = 5 * 1024 * 1024;
    if std::fs::metadata(path).map(|m| m.len() > MAX).unwrap_or(false) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

/// 握手 + 激活（lifecycle 调用）。
pub async fn initialize_and_activate(
    proc: &PluginProcess,
    init: &proto::InitializeParams,
    event: &str,
) -> Result<(), String> {
    proc.request("$initialize", serde_json::to_value(init).unwrap())
        .await
        .map_err(|e| format!("$initialize: {e}"))?;
    proc.request("$activate", serde_json::json!({ "event": event }))
        .await
        .map_err(|e| format!("$activate: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_plugin_log_writes_level_tagged_lines_and_rolls_past_5mb() {
        let dir = tempfile::tempdir().unwrap();
        append_plugin_log(dir.path(), "pub.name", "stderr", "boom");
        append_plugin_log(dir.path(), "pub.name", "info", "hello");
        let path = dir.path().join("pub.name.log");
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "[stderr] boom\n[info] hello\n");

        // Inflate past 5MB → next append rolls to .log.1 and starts fresh.
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            f.write_all(&vec![b'x'; 5 * 1024 * 1024 + 1]).unwrap();
        }
        append_plugin_log(dir.path(), "pub.name", "info", "after-roll");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[info] after-roll\n");
        assert!(dir.path().join("pub.name.log.1").exists(), "rolled file missing");
    }
}
