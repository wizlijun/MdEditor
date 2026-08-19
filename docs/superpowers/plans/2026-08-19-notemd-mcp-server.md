# notemd MCP server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `notemd search` 以 MCP server 的形式供给 agent,server 内嵌在 note.md GUI 主程序里,agent 经 `notemd mcp` 这个 stdio 外壳访问。

**Architecture:** GUI 主程序内嵌 MCP server,复用已经热的 `searchidx` 索引;`notemd mcp` 是同一个二进制的一个极薄 CLI 子命令,把 stdio 上的 JSON-RPC 转给主程序,中间走 UDS(unix)/ Named Pipe(windows);CLI 与 MCP 共用一个 `execute()` 纯函数,不 spawn 子进程。

**Tech Stack:** Rust / tokio(需新增 `net` feature)/ serde_json / `rand 0.8`(生成 UUID v4,不新增 uuid 依赖)/ Tauri 2

**Spec:** `docs/superpowers/specs/2026-08-19-notemd-mcp-server-design.md`

## Global Constraints

- **MCP 协议版本**:回显客户端请求的 `protocolVersion`;本机默认值 `2025-11-25`(探针实测 Cowork 发的值)。
- **只读**:MCP 面不得暴露任何写操作。
- **路径**:返回 vault 相对路径,**绝不**返回绝对路径。唯一例外是 `vault_info.vault_root`。
- **不开 TCP 端口**:IPC 一律 UDS / Named Pipe。
- **平台原语纪律**:新增的 IPC 平台分叉**必须**放进 `src-tauri/src/platform.rs`(该模块自述为 "the single funnel",并明文禁止在模块外直接调用平台原语)。
- **单一构造点**:`ScanOptions` / `Weights` / `Conventions` 只能经 `crate::search::options::{for_vault, weights_for_vault, conventions_for_vault}` 取得。
- **CLI 行为零变化**:P1 是纯重构,`notemd search` 的 stdout / stderr / exit code 必须逐字不变。
- **测试命令**:`cd src-tauri && cargo test <filter>`。
- **CHANGELOG**:本功能对用户可见(设置页开关 + 新 CLI 子命令),合并前必须往 `CHANGELOG.md` 与 `CHANGELOG.zh-CN.md` 的「未发布」区各加一条(见 Task 8)。

---

## File Structure

| 文件 | 责任 |
|---|---|
| `src-tauri/src/cli/search.rs`(改) | 拆出 `execute()`;`run()` 退化为「开索引 + 调 execute + 打印」 |
| `src-tauri/src/sotvault/vault_id.rs`(新) | `.notemd/vault-id` 的唯一读写点 |
| `src-tauri/src/platform.rs`(改) | 新增 `ipc` 子模块:UDS / Named Pipe 的 listen/connect |
| `src-tauri/src/mcp/mod.rs`(新) | 模块入口 |
| `src-tauri/src/mcp/protocol.rs`(新) | JSON-RPC 帧 + 工具 schema 常量(外壳与 server 共用) |
| `src-tauri/src/mcp/dispatch.rs`(新) | 方法分发:initialize / tools/list / tools/call |
| `src-tauri/src/mcp/tools.rs`(新) | `search` / `vault_info` 两个工具的实现 |
| `src-tauri/src/mcp/roots.rs`(新) | roots 拉取与三态判定 |
| `src-tauri/src/mcp/shim.rs`(新) | `notemd mcp` 外壳:stdio ↔ IPC |
| `src-tauri/src/mcp/server.rs`(新) | GUI 侧监听器 |
| `src-tauri/tests/mcp_contract.rs`(新) | CLI/MCP 契约测试 |

---

## Task 1: 抽出 `execute()` 纯函数

**Files:**
- Modify: `src-tauri/src/cli/search.rs`
- Test: `src-tauri/tests/search_cli_contract.rs`(新增用例,不改既有)

**Interfaces:**
- Produces:
  ```rust
  pub struct SearchContext<'a> {
      pub root: &'a std::path::Path,
      pub index: Option<&'a searchidx::SearchIndex>,
      pub opts: &'a searchidx::ScanOptions,
  }
  pub struct SearchOutcome {
      pub query: String,
      pub route: searchidx::Route,
      pub took_ms: u128,
      pub hits: Vec<searchidx::Hit>,
  }
  pub fn execute(ctx: &SearchContext, query: &str, limit: usize) -> SearchOutcome;
  pub fn hit_to_json(h: &searchidx::Hit) -> serde_json::Value;
  ```

**为什么 `index` 是借用而不是自己开**:GUI 主程序已经持有 `search::IndexHandle`
(`Arc<Mutex<Option<SearchIndex>>>`),MCP 必须复用它而不是再开一个 sqlite 句柄。
索引的生命周期归各自的宿主管,`execute()` 只负责「给定一个已打开的索引,怎么查、
怎么降级、产出什么形状」。freshness sweep 同理留在 CLI 侧——GUI 有 watch 线程,
不需要 sweep。

- [ ] **Step 1: 写失败的契约测试**

在 `src-tauri/tests/search_cli_contract.rs` 末尾追加:

```rust
/// `execute()` 产出的每条命中,序列化后必须与 `--json` 里那条逐字段相等。
/// 这是 CLI 与 MCP 之间唯一的一致性保证:两边渲染同一份 `SearchOutcome`。
#[test]
fn execute_hits_serialize_identically_to_cli_json() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("notes")).unwrap();
    std::fs::write(
        root.join("notes/a.md"),
        "---\ntype: Note\n---\n\n# 标题\n\nquickbrownfox 出现在这里\n",
    )
    .unwrap();

    let opts = notemd_lib::cli::search::scan_options_for(root);
    let stamp = opts.source_globs.stamp();
    let mut index = searchidx::SearchIndex::open(root, &stamp).unwrap();
    index.ensure_built(&opts).unwrap();

    let ctx = notemd_lib::cli::search::SearchContext {
        root,
        index: Some(&index),
        opts: &opts,
    };
    let outcome = notemd_lib::cli::search::execute(&ctx, "quickbrownfox", 20);
    assert_eq!(outcome.total_for_test(), outcome.hits.len());
    assert!(!outcome.hits.is_empty(), "fixture must produce a hit");

    let v = notemd_lib::cli::search::hit_to_json(&outcome.hits[0]);
    // 字段集必须与 print_json 拼的完全一致,一个不多一个不少。
    let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "attention_minutes", "breadcrumb", "doc_date", "level", "line",
            "line_end", "origin", "path", "provenance", "score", "source_ref", "text",
        ]
    );
    assert_eq!(v["path"], "notes/a.md");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test --test search_cli_contract execute_hits_serialize -- --nocapture`
Expected: 编译失败,`SearchContext` / `execute` / `hit_to_json` 未定义。

- [ ] **Step 3: 在 `cli/search.rs` 加类型与 `execute()`**

在 `SearchArgs` 定义之后插入:

```rust
/// 一次检索所需的全部环境:已打开的索引 + 该 vault 的扫描口径。
///
/// 索引是**借用**的:GUI 主程序持有 `search::IndexHandle`,MCP 复用它;
/// CLI 自己开一个。索引的生命周期归宿主,不归这里。
pub struct SearchContext<'a> {
    pub root: &'a Path,
    pub index: Option<&'a SearchIndex>,
    pub opts: &'a ScanOptions,
}

/// 一次检索的结果。CLI 拿它去 println!,MCP 拿它去序列化 —— 同一份数据的两个渲染。
pub struct SearchOutcome {
    pub query: String,
    pub route: searchidx::Route,
    pub took_ms: u128,
    pub hits: Vec<searchidx::Hit>,
}

impl SearchOutcome {
    /// 测试用:命中条数。生产代码直接读 `hits.len()`。
    pub fn total_for_test(&self) -> usize { self.hits.len() }
}

/// 执行一次检索。不打印、不管 exit code、不碰索引生命周期。
///
/// `weights` / `conventions` 在这里面解析,不由调用方传入 —— 这两个是
/// `search::options` 声明的单一构造点,让每个宿主各自解析一遍就是在制造漂移。
pub fn execute(ctx: &SearchContext, query: &str, limit: usize) -> SearchOutcome {
    let started = std::time::Instant::now();
    let weights = weights_for(ctx.root);
    let conventions = conventions_for(ctx.root);
    let (hits, route) = match ctx
        .index
        .map(|i| i.search_ranked(query, limit, &Limits::full(), &weights, &conventions))
    {
        Some(Ok(a)) => (a.hits, a.route),
        Some(Err(e)) => {
            eprintln!("notemd: query failed ({e}); scanning files directly");
            (fallback_scan(ctx.root, query, limit, ctx.opts), searchidx::Route::Scan)
        }
        None => (fallback_scan(ctx.root, query, limit, ctx.opts), searchidx::Route::Scan),
    };
    SearchOutcome {
        query: query.to_string(),
        route,
        took_ms: started.elapsed().as_millis(),
        hits,
    }
}

/// 一条命中的 JSON 形状。`print_json` 与 MCP 共用,保证两边字段集永远相同。
pub fn hit_to_json(h: &searchidx::Hit) -> serde_json::Value {
    serde_json::json!({
        "path": h.path,
        "line": h.line,
        "line_end": h.line_end,
        "text": h.text,
        "score": h.score,
        "breadcrumb": h.breadcrumb,
        "level": h.level,
        "doc_date": h.doc_date,
        "source_ref": h.source_ref(),
        "provenance": { "agent_by": h.agent_by, "human_verified": h.human_verified },
        "origin": h.origin.as_str(),
        "attention_minutes": h.attention_minutes,
    })
}
```

- [ ] **Step 4: 让 `run()` 与 `print_json()` 改用新函数**

`run()` 里从 `let query = args.query.join(" ");` 之后那段(原来的 `weights_for` /
`conventions_for` / `search_ranked` / `took`)整段替换为:

```rust
    let ctx = SearchContext { root: &root, index: index.as_ref(), opts: &opts };
    let outcome = execute(&ctx, &query, args.limit);
    let (hits, route, took) = (outcome.hits, outcome.route, outcome.took_ms);
```

`print_json` 的 body 改为复用 `hit_to_json`:

```rust
fn print_json(query: &str, route: searchidx::Route, took_ms: u128, hits: &[searchidx::Hit]) {
    let arr: Vec<serde_json::Value> = hits.iter().map(hit_to_json).collect();
    println!(
        "{}",
        serde_json::json!({
            "query": query, "route": route.as_str(), "took_ms": took_ms,
            "total": hits.len(), "hits": arr
        })
    );
}
```

- [ ] **Step 5: 跑全部 search 测试**

Run: `cd src-tauri && cargo test search`
Expected: 全绿,**且没有修改任何既有测试** —— 这是「CLI 行为零变化」的验收。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/cli/search.rs src-tauri/tests/search_cli_contract.rs
git commit -m "refactor(search): 抽出 execute() 纯函数供 CLI 与 MCP 共用"
```

---

## Task 2: `.notemd/vault-id`

**Files:**
- Create: `src-tauri/src/sotvault/vault_id.rs`
- Modify: `src-tauri/src/sotvault/mod.rs`(挂 `pub mod vault_id;`)

**Interfaces:**
- Produces: `pub fn ensure(vault_root: &std::path::Path) -> std::io::Result<String>`

- [ ] **Step 1: 写失败的测试**

在新文件 `src-tauri/src/sotvault/vault_id.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_once_and_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let a = ensure(dir.path()).unwrap();
        let b = ensure(dir.path()).unwrap();
        assert_eq!(a, b, "vault-id 一次生成永不改变");
        assert_eq!(a.len(), 36, "UUID v4 规范形式");
        assert_eq!(&a[14..15], "4", "版本号必须是 4");
    }

    #[test]
    fn replaces_unparseable_content() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(dir.path().join(".notemd/vault-id"), "garbage").unwrap();
        let id = ensure(dir.path()).unwrap();
        assert_ne!(id, "garbage");
        assert_eq!(id.len(), 36);
    }

    #[test]
    fn tolerates_surrounding_whitespace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        let written = "3f2504e0-4f89-41d3-9a0c-0305e82c3301";
        std::fs::write(
            dir.path().join(".notemd/vault-id"),
            format!("  {written}\n"),
        )
        .unwrap();
        assert_eq!(ensure(dir.path()).unwrap(), written);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test vault_id`
Expected: 编译失败,`ensure` 未定义。

- [ ] **Step 3: 实现**

`src-tauri/src/sotvault/vault_id.rs` 顶部:

```rust
//! `.notemd/vault-id` —— vault 身份的唯一读写点。
//!
//! 一次生成永不改变;**重建索引不换 ID**。与 `.notemd/settings.json` 同待遇:
//! 随 git 同步、不按 deviceId 分区 —— 同一个 vault 在多台机器上就是同一个身份,
//! 这正是 MCP 握手能判定「agent 挂载的是不是我这个 vault」的前提。
//!
//! 写这个文件不会引起索引抖动:`search::watch::should_forward` 已排除
//! `.notemd/`(仅放行 `.notemd/analytics/`)。
//!
//! 不引入 `uuid` crate:仓库已有 `rand 0.8`,v4 就是 16 字节随机数打两个标记位。

use std::io;
use std::path::{Path, PathBuf};

fn path_of(vault_root: &Path) -> PathBuf {
    vault_root.join(".notemd").join("vault-id")
}

/// 形如 `3f2504e0-4f89-41d3-9a0c-0305e82c3301` 才算数:长度、连字符位置、
/// 版本位(第 15 个字符)、变体位(第 20 个字符)全部校验。宽松一点就等于
/// 让一次误写永久污染 vault 身份。
fn is_uuid_v4(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 { return false; }
    for (i, c) in b.iter().enumerate() {
        let ok = match i {
            8 | 13 | 18 | 23 => *c == b'-',
            14 => *c == b'4',
            19 => matches!(*c, b'8' | b'9' | b'a' | b'b' | b'A' | b'B'),
            _ => c.is_ascii_hexdigit(),
        };
        if !ok { return false; }
    }
    true
}

fn generate() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant 10
    let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// 读取,不存在或不合法则创建。幂等。
pub fn ensure(vault_root: &Path) -> io::Result<String> {
    let p = path_of(vault_root);
    if let Ok(raw) = std::fs::read_to_string(&p) {
        let trimmed = raw.trim();
        if is_uuid_v4(trimmed) {
            return Ok(trimmed.to_string());
        }
    }
    let id = generate();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, format!("{id}\n"))?;
    Ok(id)
}
```

在 `src-tauri/src/sotvault/mod.rs` 的模块声明区加一行:

```rust
pub mod vault_id;
```

- [ ] **Step 4: 跑测试**

Run: `cd src-tauri && cargo test vault_id`
Expected: 3 个用例全绿。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/sotvault/vault_id.rs src-tauri/src/sotvault/mod.rs
git commit -m "feat(vault): .notemd/vault-id 身份文件,幂等生成随 git 同步"
```

---

## Task 3: 平台 IPC 层

**Files:**
- Modify: `src-tauri/src/platform.rs`
- Modify: `src-tauri/Cargo.toml`(tokio 加 `net` feature)

**Interfaces:**
- Consumes: 无
- Produces:
  ```rust
  pub mod ipc {
      pub fn endpoint() -> std::io::Result<std::path::PathBuf>;   // unix: sock 路径; windows: 管道名
      pub async fn listen() -> std::io::Result<Listener>;
      pub async fn connect() -> std::io::Result<Stream>;
      pub struct Listener; impl Listener { pub async fn accept(&mut self) -> std::io::Result<Stream>; }
      pub type Stream = /* UnixStream | NamedPipeClient/Server */;
  }
  ```

- [ ] **Step 1: Cargo.toml 加 net feature**

把第 31 行改为:

```toml
tokio = { version = "1", features = ["time", "process", "io-util", "macros", "rt-multi-thread", "sync", "net"] }
```

- [ ] **Step 2: 写失败的测试**

在 `src-tauri/src/platform.rs` 的 `mod tests` 里追加:

```rust
    /// 端点路径必须在 `sun_path` 上限之内 —— macOS 104 / Linux 108 字节。
    /// 用户名可以很长,这里断言而不是假设(spec §3.4)。
    #[cfg(unix)]
    #[test]
    fn ipc_endpoint_fits_sun_path() {
        let p = super::ipc::endpoint().expect("endpoint resolvable");
        let len = p.as_os_str().len();
        assert!(len < 104, "socket path too long ({len}): {}", p.display());
    }

    /// 一个往返:listen → connect → 写一帧 → 读回来。两个平台各自的分支都要过。
    #[tokio::test]
    async fn ipc_round_trip() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let mut listener = super::ipc::listen().await.expect("listen");
        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (r, mut w) = tokio::io::split(stream);
            let mut lines = BufReader::new(r).lines();
            let line = lines.next_line().await.unwrap().unwrap();
            w.write_all(format!("echo:{line}\n").as_bytes()).await.unwrap();
        });
        let stream = super::ipc::connect().await.expect("connect");
        let (r, mut w) = tokio::io::split(stream);
        w.write_all(b"hello\n").await.unwrap();
        let mut lines = BufReader::new(r).lines();
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "echo:hello");
        server.await.unwrap();
    }

    /// 僵尸 socket:主程序崩溃后文件残留,再 listen 必须能重建(spec §3.4、§8.6)。
    #[cfg(unix)]
    #[tokio::test]
    async fn stale_socket_file_is_reclaimed() {
        let path = super::ipc::endpoint().unwrap();
        let _ = std::fs::remove_file(&path);
        // 造一个「有文件但没人监听」的现场 —— 正是崩溃后留下的样子。
        std::fs::write(&path, b"").unwrap();
        assert!(path.exists());
        let _l = super::ipc::listen().await.expect("必须能回收僵尸 socket");
        let _ = std::fs::remove_file(&path);
    }

    /// 反过来:已有实例在健康监听时,第二次 listen 必须**失败**而不是把
    /// 对方的 socket 删掉。无脑 unlink 会踢掉一个正在服务的实例。
    #[cfg(unix)]
    #[tokio::test]
    async fn live_listener_is_not_evicted() {
        let path = super::ipc::endpoint().unwrap();
        let _ = std::fs::remove_file(&path);
        let _first = super::ipc::listen().await.expect("first listen");
        let second = super::ipc::listen().await;
        assert!(second.is_err(), "健康实例不得被顶掉");
        assert!(path.exists(), "对方的 socket 文件必须还在");
        let _ = std::fs::remove_file(&path);
    }
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cd src-tauri && cargo test platform::tests::ipc`
Expected: 编译失败,`ipc` 模块不存在。

- [ ] **Step 4: 实现 `ipc` 子模块**

在 `platform.rs` 末尾(`mod tests` 之前)插入:

```rust
/// 外壳(`notemd mcp`)与 GUI 主程序之间的那一跳。
///
/// **不开 TCP 端口**:UDS / Named Pipe 都不在网络栈上,于是端口占用、
/// DNS rebinding、CSRF、Origin 校验这一整类问题连同 token 一起消失,
/// 访问控制交给 OS(unix 文件权限 / Windows 管道 ACL)。
///
/// 不用「AF_UNIX 一把梭」:Windows 10 1803+ 虽支持 AF_UNIX,但 tokio 在
/// Windows 上不支持它(`UnixStream` 由 `cfg(unix)` 门死),得另引
/// `uds_windows` 再自建异步桥 —— 所谓统一只是换个地方分叉,还多背一个依赖。
pub mod ipc {
    use std::io;
    use std::path::PathBuf;

    #[cfg(unix)]
    pub type Stream = tokio::net::UnixStream;
    #[cfg(windows)]
    pub type Stream = tokio::net::windows::named_pipe::NamedPipeServer;

    /// unix:socket 文件路径。Linux 用 `$XDG_RUNTIME_DIR`(runtime socket
    /// 不属于 config 目录),macOS 无此变量,回落 App Support。
    #[cfg(unix)]
    pub fn endpoint() -> io::Result<PathBuf> {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .map(|d| d.join("notemd"))
            .or_else(|| dirs::config_dir().map(|d| d.join(crate::app_dirs::BUNDLE_ID)))
            .ok_or_else(|| io::Error::other("no runtime or config dir"))?;
        std::fs::create_dir_all(&base)?;
        Ok(base.join("mcp.sock"))
    }

    #[cfg(windows)]
    pub fn endpoint() -> io::Result<PathBuf> {
        Ok(PathBuf::from(r"\\.\pipe\net.notemd.app.mcp"))
    }

    #[cfg(unix)]
    pub struct Listener(tokio::net::UnixListener);

    #[cfg(unix)]
    impl Listener {
        pub async fn accept(&mut self) -> io::Result<Stream> {
            let (s, _) = self.0.accept().await?;
            Ok(s)
        }
    }

    /// unix 的僵尸 socket:主程序崩溃后 `.sock` 残留,再 `bind()` 得
    /// `EADDRINUSE`。**先 connect 探活,被拒才 unlink** —— 无脑删会踢掉一个
    /// 正在健康运行的实例(spec §3.4)。
    #[cfg(unix)]
    pub async fn listen() -> io::Result<Listener> {
        use std::os::unix::fs::PermissionsExt;
        let path = endpoint()?;
        if path.exists() {
            match tokio::net::UnixStream::connect(&path).await {
                Ok(_) => return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another note.md instance is already serving MCP",
                )),
                Err(_) => { let _ = std::fs::remove_file(&path); }
            }
        }
        let l = tokio::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener(l))
    }

    #[cfg(unix)]
    pub async fn connect() -> io::Result<tokio::net::UnixStream> {
        tokio::net::UnixStream::connect(endpoint()?).await
    }

    #[cfg(windows)]
    pub struct Listener {
        name: String,
        next: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
    }

    #[cfg(windows)]
    impl Listener {
        pub async fn accept(&mut self) -> io::Result<Stream> {
            use tokio::net::windows::named_pipe::ServerOptions;
            let server = self.next.take().ok_or_else(|| io::Error::other("listener closed"))?;
            server.connect().await?;
            // 下一个实例必须在把当前这个交出去之前建好,否则客户端会在
            // 两次 accept 之间撞上 ERROR_FILE_NOT_FOUND。
            self.next = Some(ServerOptions::new().create(&self.name)?);
            Ok(server)
        }
    }

    #[cfg(windows)]
    pub async fn listen() -> io::Result<Listener> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let name = endpoint()?.to_string_lossy().to_string();
        let first = ServerOptions::new().first_pipe_instance(true).create(&name)?;
        Ok(Listener { name, next: Some(first) })
    }

    #[cfg(windows)]
    pub async fn connect() -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
        use tokio::net::windows::named_pipe::ClientOptions;
        let name = endpoint()?.to_string_lossy().to_string();
        ClientOptions::new().open(&name)
    }
}
```

**注意**:`round_trip` 测试里 `connect()` 在 unix 返回 `UnixStream`、windows 返回
`NamedPipeClient`,两者都实现 `AsyncRead + AsyncWrite`,`tokio::io::split` 都能吃。

- [ ] **Step 5: 跑测试**

Run: `cd src-tauri && cargo test platform::tests::ipc -- --test-threads=1`
Expected: 两个用例绿。`--test-threads=1` 是必需的——两个用例抢同一个端点。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/platform.rs src-tauri/Cargo.toml
git commit -m "feat(platform): MCP 用的 IPC 层,unix UDS / windows 命名管道"
```

---

## Task 4: MCP 协议核心(纯函数,无 IO)

**Files:**
- Create: `src-tauri/src/mcp/mod.rs`, `src-tauri/src/mcp/protocol.rs`
- Modify: `src-tauri/src/lib.rs`(加 `pub mod mcp;`)

**Interfaces:**
- Produces:
  ```rust
  pub const DEFAULT_PROTOCOL_VERSION: &str = "2025-11-25";
  pub fn tool_definitions() -> serde_json::Value;      // tools/list 的 result
  pub fn initialize_result(client_version: Option<&str>) -> serde_json::Value;
  pub fn error(id: &serde_json::Value, code: i64, msg: &str) -> serde_json::Value;
  pub fn tool_error(id: &serde_json::Value, msg: &str) -> serde_json::Value;  // isError:true
  pub fn tool_ok(id: &serde_json::Value, payload: &serde_json::Value) -> serde_json::Value;
  ```

- [ ] **Step 1: 写失败的测试**

`src-tauri/src/mcp/protocol.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_list_has_exactly_two_readonly_tools() {
        let v = tool_definitions();
        let tools = v["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["search", "vault_info"]);
        // 只读面:工具名里不得出现任何写动作词。
        for n in &names {
            for bad in ["write", "create", "delete", "update", "edit", "move"] {
                assert!(!n.contains(bad), "只读面被破坏: {n}");
            }
        }
    }

    #[test]
    fn search_schema_exposes_only_query_limit_context() {
        let v = tool_definitions();
        let search = &v["tools"][0];
        let mut props: Vec<&str> =
            search["inputSchema"]["properties"].as_object().unwrap()
                .keys().map(|s| s.as_str()).collect();
        props.sort();
        assert_eq!(props, vec!["context", "limit", "query"]);
        assert_eq!(search["inputSchema"]["required"], serde_json::json!(["query"]));
    }

    #[test]
    fn initialize_echoes_client_protocol_version() {
        let v = initialize_result(Some("2025-06-18"));
        assert_eq!(v["protocolVersion"], "2025-06-18");
        let v = initialize_result(None);
        assert_eq!(v["protocolVersion"], DEFAULT_PROTOCOL_VERSION);
    }

    #[test]
    fn tool_error_is_result_not_protocol_error() {
        // 降级信号必须走 result.isError,不能走协议层 error —— 否则模型
        // 会把整轮工具调用判死,而不是退回 grep(spec §1.2)。
        let v = tool_error(&serde_json::json!(1), "note.md 未运行");
        assert!(v.get("error").is_none());
        assert_eq!(v["result"]["isError"], true);
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("note.md"));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test mcp::protocol`
Expected: 模块不存在,编译失败。

- [ ] **Step 3: 实现**

`src-tauri/src/mcp/mod.rs`:

```rust
//! MCP server —— 把 notemd 的检索面供给 agent。
//!
//! 进程拓扑见 `docs/superpowers/specs/2026-08-19-notemd-mcp-server-design.md`:
//! agent --stdio--> `notemd mcp` 外壳 --UDS/管道--> GUI 主程序。
//! 外壳与 server 是同一个二进制,于是工具 schema 是同一个编译期常量,
//! 两边不可能对不上 —— 不靠约定,靠编译。

pub mod protocol;
pub mod dispatch;
pub mod tools;
pub mod roots;
pub mod shim;
pub mod server;
```

`src-tauri/src/mcp/protocol.rs`:

```rust
//! JSON-RPC 帧与工具 schema。**无 IO、无状态**,外壳与 server 共用。

use serde_json::{json, Value};

/// 探针实测 Cowork 发的版本。客户端报什么就回什么,这只是没得回时的默认值。
pub const DEFAULT_PROTOCOL_VERSION: &str = "2025-11-25";

pub fn initialize_result(client_version: Option<&str>) -> Value {
    json!({
        "protocolVersion": client_version.unwrap_or(DEFAULT_PROTOCOL_VERSION),
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "notemd", "version": env!("CARGO_PKG_VERSION") },
    })
}

/// 两个工具,只读。
///
/// `search` 只收 `query`/`limit`/`context`:过滤语法写在 query 字符串里,
/// 与 CLI 逐字相同 —— 一个语法,一个解析器。把 tag/type/path/… 拆成独立参数
/// 等于把同一套过滤语义实现两遍。
pub fn tool_definitions() -> Value {
    json!({ "tools": [
        {
            "name": "search",
            "description": concat!(
                "Full-text search over the user's note.md vault, with Chinese ",
                "segmentation, relevance ranking, and origin weighting (human-written ",
                "notes rank above machine summaries).\n\n",
                "Filters go inside `query`, same syntax as the `notemd search` CLI:\n",
                "  tag:x  type:x  path:x  ext:x  after:YYYY-MM-DD  before:YYYY-MM-DD\n",
                "  page:[[X]]  origin:human|derived|source\n\n",
                "Each hit carries `origin` (which tier the file falls in) and ",
                "`provenance.agent_by` (set when a file was written by an agent — follow ",
                "its `sources` rather than citing it as primary). Paths are vault-relative; ",
                "resolve them against your own mount only when `mount.status` is \"matched\"."
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search terms, optionally with the filters above." },
                    "limit": { "type": "integer", "description": "Max hits. 0 = no cap.", "default": 20 },
                    "context": { "type": "integer", "description": "Lines of surrounding context per hit.", "default": 0 }
                },
                "required": ["query"]
            }
        },
        {
            "name": "vault_info",
            "description": concat!(
                "Identity and freshness of the vault this server is serving. Call once per ",
                "session before relying on `search`: compare `vault_id` against the ",
                ".notemd/vault-id in your own mounted folder. Zero side effects."
            ),
            "inputSchema": { "type": "object", "properties": {} }
        }
    ]})
}

pub fn error(id: &Value, code: i64, msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } })
}

/// 降级信号:`result.isError`,**不是**协议层 error。模型据此退回 grep,
/// 而不是把整轮工具调用判死。
pub fn tool_error(id: &Value, msg: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": {
        "isError": true,
        "content": [{ "type": "text", "text": msg }]
    }})
}

pub fn tool_ok(id: &Value, payload: &Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": {
        "content": [{ "type": "text", "text": serde_json::to_string(payload).unwrap_or_default() }]
    }})
}
```

在 `src-tauri/src/lib.rs` 的模块声明区加:

```rust
#[cfg(not(target_os = "ios"))]
pub mod mcp;
```

- [ ] **Step 4: 跑测试**

Run: `cd src-tauri && cargo test mcp::protocol`
Expected: 4 个用例绿。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/mcp/ src-tauri/src/lib.rs
git commit -m "feat(mcp): 协议层与两个只读工具的 schema"
```

---

## Task 5: roots 三态判定

**Files:**
- Create: `src-tauri/src/mcp/roots.rs`

**Interfaces:**
- Consumes: `crate::sotvault::vault_id::ensure`(Task 2)
- Produces:
  ```rust
  pub enum MountStatus { Matched, Mismatched, Unknown }
  pub fn classify(roots: Option<&[String]>, our_id: &str) -> (MountStatus, Option<String>);
  pub fn to_json(status: MountStatus, matched_root: Option<String>) -> serde_json::Value;
  ```

- [ ] **Step 1: 写失败的测试**

`src-tauri/src/mcp/roots.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn vault_with(id: &str) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".notemd")).unwrap();
        std::fs::write(d.path().join(".notemd/vault-id"), format!("{id}\n")).unwrap();
        d
    }
    const ID: &str = "3f2504e0-4f89-41d3-9a0c-0305e82c3301";
    const OTHER: &str = "11111111-4111-8111-8111-111111111111";

    #[test]
    fn matching_root_is_matched() {
        let d = vault_with(ID);
        let uri = format!("file://{}", d.path().display());
        let (st, matched) = classify(Some(&[uri.clone()]), ID);
        assert_eq!(st, MountStatus::Matched);
        assert_eq!(matched.as_deref(), Some(uri.as_str()));
    }

    #[test]
    fn non_matching_roots_are_mismatched() {
        let d = vault_with(OTHER);
        let uri = format!("file://{}", d.path().display());
        let (st, matched) = classify(Some(&[uri]), ID);
        assert_eq!(st, MountStatus::Mismatched);
        assert_eq!(matched, None);
    }

    /// client 没声明 roots ⇒ unknown,回落到 agent 自查协议。
    /// 绝不能因此拒绝服务。
    #[test]
    fn absent_roots_are_unknown() {
        let (st, _) = classify(None, ID);
        assert_eq!(st, MountStatus::Unknown);
    }

    /// 有 roots 但都读不到 vault-id ⇒ 仍是 mismatched,不是 unknown。
    /// unknown 的含义是「无从判断」,这里是「判断了,不匹配」。
    #[test]
    fn roots_without_vault_id_are_mismatched() {
        let d = tempfile::tempdir().unwrap();
        let uri = format!("file://{}", d.path().display());
        let (st, _) = classify(Some(&[uri]), ID);
        assert_eq!(st, MountStatus::Mismatched);
    }

    #[test]
    fn mismatched_json_carries_actionable_advice() {
        let v = to_json(MountStatus::Mismatched, None);
        assert_eq!(v["status"], "mismatched");
        let advice = v["advice"].as_str().unwrap();
        assert!(advice.contains("do not"), "必须明确告诉 agent 别去解析路径");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test mcp::roots`
Expected: 编译失败。

- [ ] **Step 3: 实现**

```rust
//! roots 握手 —— 把错配检测从君子协定升级成服务端判定。
//!
//! 上游 spec 的握手靠 agent 自己 Read `.notemd/vault-id`、自己比对、自己降级;
//! agent 忘了比对,错配就静默发生。探针实测 Cowork 声明 `roots.listChanged`
//! 并主动推送变更 —— 于是 server 能反过来问「你挂载了哪些目录」,自己比对。

use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountStatus { Matched, Mismatched, Unknown }

impl MountStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            MountStatus::Matched => "matched",
            MountStatus::Mismatched => "mismatched",
            MountStatus::Unknown => "unknown",
        }
    }
}

/// `file:///a/b` → `/a/b`。非 file: 的 root(极少见)直接跳过。
fn uri_to_path(uri: &str) -> Option<PathBuf> {
    uri.strip_prefix("file://").map(PathBuf::from)
}

/// `None` 表示 client 未声明 roots 能力 ⇒ `Unknown`,回落 agent 自查协议。
/// 空切片表示「声明了,但没挂任何目录」⇒ 同样无从判断,也是 `Unknown`。
pub fn classify(roots: Option<&[String]>, our_id: &str) -> (MountStatus, Option<String>) {
    let Some(roots) = roots else { return (MountStatus::Unknown, None) };
    if roots.is_empty() { return (MountStatus::Unknown, None) }
    for uri in roots {
        let Some(p) = uri_to_path(uri) else { continue };
        let Ok(raw) = std::fs::read_to_string(p.join(".notemd").join("vault-id")) else { continue };
        if raw.trim() == our_id {
            return (MountStatus::Matched, Some(uri.clone()));
        }
    }
    (MountStatus::Mismatched, None)
}

/// `mismatched` 时**照常返回检索结果**,只是让错配无法被误解。
///
/// 危险的从来不是结果本身(对 server 的 vault 永远是对的),而是 agent 拿
/// `/dailynote/2026/x.note.md` 去自己的挂载点解析、读到同路径的别的文件。
/// 拒绝服务会误伤一类正当用法:agent 只想知道你笔记里有什么,并不打算读原文。
pub fn to_json(status: MountStatus, matched_root: Option<String>) -> Value {
    let advice = match status {
        MountStatus::Matched =>
            "Paths in this response resolve against your mounted vault.",
        MountStatus::Mismatched =>
            "Your mounted folders are NOT this vault — do not resolve these paths against them; \
             a same-named file there is a different file. Use the returned text and breadcrumb, \
             or ask the user to mount the vault.",
        MountStatus::Unknown =>
            "Mount could not be determined. Before resolving paths, read .notemd/vault-id in \
             your mounted folder and compare it with vault_id above.",
    };
    json!({ "status": status.as_str(), "matched_root": matched_root, "advice": advice })
}
```

- [ ] **Step 4: 跑测试**

Run: `cd src-tauri && cargo test mcp::roots`
Expected: 5 个用例绿。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/mcp/roots.rs
git commit -m "feat(mcp): roots 三态握手,错配由服务端判定而非 agent 自觉"
```

---

## Task 6: 工具实现与分发

**Files:**
- Create: `src-tauri/src/mcp/tools.rs`, `src-tauri/src/mcp/dispatch.rs`

**Interfaces:**
- Consumes: `cli::search::{SearchContext, SearchOutcome, execute, hit_to_json}`(Task 1)、
  `sotvault::vault_id::ensure`(Task 2)、`mcp::protocol::*`(Task 4)、`mcp::roots::*`(Task 5)
- Produces:
  ```rust
  pub struct ToolEnv { pub vault_root: PathBuf, pub index: crate::search::IndexHandle, pub roots: Option<Vec<String>> }
  pub fn search(env: &ToolEnv, args: &serde_json::Value) -> Result<serde_json::Value, String>;
  pub fn vault_info(env: &ToolEnv) -> Result<serde_json::Value, String>;
  pub fn handle(env: Option<&ToolEnv>, msg: &serde_json::Value) -> Option<serde_json::Value>;
  ```

- [ ] **Step 1: 写失败的测试**

`src-tauri/src/mcp/dispatch.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req(id: i64, method: &str) -> serde_json::Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": method })
    }

    /// 通知(无 id)不产生响应。
    #[test]
    fn notification_yields_no_response() {
        let m = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(None, &m).is_none());
    }

    /// **note.md 未运行时 `tools/list` 仍必须给出完整工具定义**(spec §1.2)。
    /// MCP 客户端在会话启动那一刻枚举工具;此时返回空列表,agent 整场会话
    /// 都不会再问第二次。
    #[test]
    fn tools_list_works_without_env() {
        let r = handle(None, &req(1, "tools/list")).unwrap();
        assert_eq!(r["result"]["tools"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn initialize_works_without_env() {
        let m = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                        "params": { "protocolVersion": "2025-11-25" } });
        let r = handle(None, &m).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-11-25");
    }

    /// 没有 env(主程序不在)时调工具 ⇒ isError,不是协议 error。
    #[test]
    fn tools_call_without_env_is_tool_error() {
        let m = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                        "params": { "name": "search", "arguments": { "query": "x" } } });
        let r = handle(None, &m).unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r.get("error").is_none());
    }

    #[test]
    fn unknown_method_is_protocol_error() {
        let r = handle(None, &req(3, "nope/nope")).unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }

    #[test]
    fn unknown_tool_name_is_tool_error() {
        let m = json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                        "params": { "name": "delete_everything", "arguments": {} } });
        let r = handle(None, &m).unwrap();
        assert_eq!(r["result"]["isError"], true);
    }
}
```

`src-tauri/src/mcp/tools.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env_with_fixture() -> (tempfile::TempDir, ToolEnv) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("notes")).unwrap();
        std::fs::write(d.path().join("notes/a.md"), "# T\n\nzebraquux 在这里\n").unwrap();
        let opts = crate::cli::search::scan_options_for(d.path());
        let mut idx = searchidx::SearchIndex::open(d.path(), &opts.source_globs.stamp()).unwrap();
        idx.ensure_built(&opts).unwrap();
        let env = ToolEnv {
            vault_root: d.path().to_path_buf(),
            index: std::sync::Arc::new(std::sync::Mutex::new(Some(idx))),
            roots: None,
        };
        (d, env)
    }

    #[test]
    fn search_returns_relative_paths_only() {
        let (_d, env) = env_with_fixture();
        let out = search(&env, &json!({ "query": "zebraquux" })).unwrap();
        let hits = out["hits"].as_array().unwrap();
        assert!(!hits.is_empty());
        for h in hits {
            let p = h["path"].as_str().unwrap();
            assert!(!p.starts_with('/'), "绝不返回绝对路径: {p}");
            assert!(!p.contains("/Users/"), "绝不泄漏本机路径: {p}");
        }
    }

    #[test]
    fn every_search_response_carries_identity() {
        let (_d, env) = env_with_fixture();
        let out = search(&env, &json!({ "query": "zebraquux" })).unwrap();
        assert_eq!(out["vault_id"].as_str().unwrap().len(), 36);
        assert_eq!(out["mount"]["status"], "unknown");
    }

    #[test]
    fn vault_info_reports_identity_and_root() {
        let (d, env) = env_with_fixture();
        let out = vault_info(&env).unwrap();
        assert_eq!(out["vault_id"].as_str().unwrap().len(), 36);
        assert_eq!(out["vault_root"], d.path().to_string_lossy().to_string());
        assert!(out["entry_count"].as_u64().is_some());
    }

    #[test]
    fn missing_query_is_an_error() {
        let (_d, env) = env_with_fixture();
        assert!(search(&env, &json!({})).is_err());
    }

    #[test]
    fn limit_zero_means_no_cap() {
        let (_d, env) = env_with_fixture();
        let out = search(&env, &json!({ "query": "zebraquux", "limit": 0 })).unwrap();
        assert!(out["hits"].as_array().is_some());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test mcp::tools mcp::dispatch`
Expected: 编译失败。

- [ ] **Step 3: 实现 `tools.rs`**

```rust
//! 两个只读工具。检索本身一行都不在这里 —— 全部委托 `cli::search::execute`,
//! 与 `notemd search --json` 是同一份数据的两个渲染。

use serde_json::{json, Value};
use std::path::PathBuf;

use crate::cli::search::{execute, hit_to_json, SearchContext};
use crate::mcp::roots;
use crate::sotvault::vault_id;

pub struct ToolEnv {
    pub vault_root: PathBuf,
    pub index: crate::search::IndexHandle,
    /// client 声明的 roots;`None` = 未声明能力。
    pub roots: Option<Vec<String>>,
}

impl ToolEnv {
    fn identity(&self) -> (String, Value) {
        let id = vault_id::ensure(&self.vault_root).unwrap_or_default();
        let (status, matched) = roots::classify(self.roots.as_deref(), &id);
        (id, roots::to_json(status, matched))
    }
}

pub fn search(env: &ToolEnv, args: &Value) -> Result<Value, String> {
    let query = args.get("query").and_then(|v| v.as_str())
        .ok_or_else(|| "search 需要 query 参数".to_string())?;
    if query.trim().is_empty() {
        return Err("query 不能为空".to_string());
    }
    // `0` 是「不设上限」的哨兵,与 CLI 的 `--limit 0` / `--all` 同义。
    let limit = match args.get("limit").and_then(|v| v.as_u64()) {
        Some(0) => searchidx::NO_LIMIT,
        Some(n) => n as usize,
        None => 20,
    };
    let context = args.get("context").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    let opts = crate::cli::search::scan_options_for(&env.vault_root);
    let guard = env.index.lock().map_err(|_| "索引锁中毒".to_string())?;
    let ctx = SearchContext { root: &env.vault_root, index: guard.as_ref(), opts: &opts };
    let outcome = execute(&ctx, query, limit);
    drop(guard);

    let (id, mount) = env.identity();
    let hits: Vec<Value> = outcome.hits.iter().map(|h| {
        let mut v = hit_to_json(h);
        if context > 0 {
            if let Some(lines) = crate::cli::search::context_lines_public(&env.vault_root, h, context) {
                v["context"] = json!(lines.iter().map(|(n, t)| json!({ "line": n, "text": t }))
                    .collect::<Vec<_>>());
            }
        }
        v
    }).collect();

    Ok(json!({
        "vault_id": id,
        "mount": mount,
        "query": outcome.query,
        "route": outcome.route.as_str(),
        "took_ms": outcome.took_ms,
        "total": hits.len(),
        "hits": hits,
    }))
}

pub fn vault_info(env: &ToolEnv) -> Result<Value, String> {
    let (id, mount) = env.identity();
    let guard = env.index.lock().map_err(|_| "索引锁中毒".to_string())?;
    let (entry_count, indexed_at) = match guard.as_ref().and_then(|i| i.stats().ok()) {
        Some(s) => (s.files, Some(s.built_at)),
        None => (0, None),
    };
    Ok(json!({
        "vault_id": id,
        // 本机视角绝对路径,**仅供人核对**;agent 不得用于路径拼接。
        "vault_root": env.vault_root.to_string_lossy(),
        "entry_count": entry_count,
        "indexed_at": indexed_at,
        "notemd_version": env!("CARGO_PKG_VERSION"),
        "mount": mount,
    }))
}
```

**同时**在 `cli/search.rs` 里把 `context_lines` 暴露出来(保持私有实现,加一个公开薄壳):

```rust
/// `context_lines` 的公开壳,给 MCP 用。逻辑一行不改 —— 包括「行号已失效
/// 就整条丢弃」那条规则:陈旧的引用比缺失的引用更糟。
pub fn context_lines_public(
    root: &Path, hit: &searchidx::Hit, context: usize,
) -> Option<Vec<(u32, String)>> {
    context_lines(root, hit, context)
}
```

- [ ] **Step 4: 实现 `dispatch.rs`**

```rust
//! 方法分发。`initialize` 与 `tools/list` **不需要 env**(主程序可以没开);
//! 只有 `tools/call` 需要。

use serde_json::{json, Value};

use crate::mcp::protocol;
use crate::mcp::tools::{self, ToolEnv};

pub fn handle(env: Option<&ToolEnv>, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone(); // 无 id = 通知,不回
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    Some(match method {
        "initialize" => {
            let v = params.get("protocolVersion").and_then(|v| v.as_str());
            json!({ "jsonrpc": "2.0", "id": id, "result": protocol::initialize_result(v) })
        }
        "tools/list" => json!({ "jsonrpc": "2.0", "id": id, "result": protocol::tool_definitions() }),
        "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
        "resources/list" => json!({ "jsonrpc": "2.0", "id": id, "result": { "resources": [] } }),
        "prompts/list" => json!({ "jsonrpc": "2.0", "id": id, "result": { "prompts": [] } }),
        "tools/call" => {
            let Some(env) = env else {
                return Some(protocol::tool_error(
                    &id,
                    "note.md 未运行。启动 note.md 后即可检索;在此之前请用 grep/rg 兜底。",
                ));
            };
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let out = match name {
                "search" => tools::search(env, &args),
                "vault_info" => tools::vault_info(env),
                other => Err(format!("未知工具 '{other}';本 server 只提供 search 与 vault_info")),
            };
            match out {
                Ok(v) => protocol::tool_ok(&id, &v),
                Err(e) => protocol::tool_error(&id, &e),
            }
        }
        other => protocol::error(&id, -32601, &format!("no such method: {other}")),
    })
}
```

- [ ] **Step 5: 跑测试**

Run: `cd src-tauri && cargo test mcp::`
Expected: protocol / roots / tools / dispatch 全绿。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/mcp/tools.rs src-tauri/src/mcp/dispatch.rs src-tauri/src/cli/search.rs
git commit -m "feat(mcp): search/vault_info 两个只读工具与方法分发"
```

---

## Task 7: 外壳 `notemd mcp` 与 GUI 侧监听器

**Files:**
- Create: `src-tauri/src/mcp/shim.rs`, `src-tauri/src/mcp/server.rs`
- Modify: `src-tauri/src/cli/router.rs`(加 `Builtin::Mcp`)、`src-tauri/src/cli/builtin.rs`(分发)、`src-tauri/src/lib.rs`(setup 里起监听)

**Interfaces:**
- Consumes: `platform::ipc`(Task 3)、`mcp::dispatch::handle`(Task 6)
- Produces: `pub fn run_shim() -> std::process::ExitCode;`、`pub fn init(app: &tauri::AppHandle);`

- [ ] **Step 1: 写失败的路由测试**

在 `src-tauri/src/cli/router.rs` 的 `mod tests` 里追加:

```rust
    /// `mcp` 是 core:agent 的检索入口不能被插件遮蔽,也不能被禁用。
    #[test]
    fn mcp_routes_as_builtin() {
        let r = route_with(&["mcp"], vec![], Default::default());
        assert!(matches!(r, Route::Builtin(Builtin::Mcp)), "got {r:?}");
    }

    #[test]
    fn mcp_is_not_shadowed_by_a_plugin() {
        let m = manifest_with_cli("evil", "mcp", &[]);
        let mut enabled = std::collections::HashMap::new();
        enabled.insert("evil".to_string(), true);
        let r = route_with(&["mcp"], vec![(m, PathBuf::from("/tmp"))], enabled);
        assert!(matches!(r, Route::Builtin(Builtin::Mcp)), "got {r:?}");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test cli::router::tests::mcp`
Expected: `Builtin::Mcp` 不存在,编译失败。

- [ ] **Step 3: 加路由**

`router.rs` 的 `enum Builtin` 里加:

```rust
    /// `mcp` —— MCP server 的 stdio 外壳。Core,never disabled:
    /// agent 的检索入口不能取决于插件状态。
    Mcp,
```

在 `doctor` 那个分支之后加:

```rust
    if first == "mcp" {
        return Route::Builtin(Builtin::Mcp);
    }
```

`builtin.rs` 的 `match b` 里加:

```rust
        Builtin::Mcp => crate::mcp::shim::run_shim(),
```

- [ ] **Step 4: 实现外壳**

`src-tauri/src/mcp/shim.rs`:

```rust
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
```

- [ ] **Step 5: 实现 GUI 侧监听器**

`src-tauri/src/mcp/server.rs`:

```rust
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
fn build_env(app: &AppHandle) -> Option<ToolEnv> {
    let root = crate::sotvault::resolve_vault_root(app)?;
    Some(ToolEnv {
        vault_root: root,
        index: crate::search::handle(app),
        roots: None,
    })
}
```

在 `src-tauri/src/lib.rs` 的 `.setup(...)` 里,紧跟 `search::init(&app.handle());` 之后加:

```rust
                mcp::server::init(&app.handle());
```

- [ ] **Step 6: 跑测试 + 手动冒烟**

Run: `cd src-tauri && cargo test`
Expected: 全绿。

手动冒烟(需要先 `cargo build`):

```bash
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"tools/list"}' \
  | ./src-tauri/target/debug/notemd --cli mcp
```
Expected: 一行 JSON,`result.tools` 有 2 个,**且 note.md 没开也能出**。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/mcp/shim.rs src-tauri/src/mcp/server.rs \
        src-tauri/src/cli/router.rs src-tauri/src/cli/builtin.rs src-tauri/src/lib.rs
git commit -m "feat(mcp): notemd mcp 外壳与 GUI 侧监听器"
```

---

## Task 8: roots 拉取、帮助文案、CHANGELOG

**Files:**
- Modify: `src-tauri/src/mcp/server.rs`(会话内拉 roots)
- Modify: `src-tauri/src/cli/builtin.rs`(`render_help` 加 `mcp` 一行)
- Modify: `CHANGELOG.md`、`CHANGELOG.zh-CN.md`

- [ ] **Step 1: 写失败的测试**

在 `src-tauri/src/mcp/server.rs` 末尾:

```rust
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test mcp::server`
Expected: `client_supports_roots` 未定义。

- [ ] **Step 3: 实现**

在 `server.rs` 加:

```rust
/// client 是否声明了 roots 能力。没声明就绝不发 `roots/list` ——
/// 对不支持的 client 发请求会挂住,而 roots 只是加固,不值得拿可用性换。
pub(crate) fn client_supports_roots(init_params: &serde_json::Value) -> bool {
    init_params
        .get("capabilities")
        .and_then(|c| c.get("roots"))
        .is_some()
}
```

把 `serve_one` 改成带会话状态的形式:

```rust
async fn serve_one(app: AppHandle, stream: crate::platform::ipc::Stream) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (r, mut w) = tokio::io::split(stream);
    let mut lines = BufReader::new(r).lines();
    let mut roots: Option<Vec<String>> = None;

    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else { continue };

        // 外壳把 initialize 就地答了,但会把 client 的 roots 原样带过来
        // (见下面的 shim 改动),server 只需记下来。
        if msg.get("method").and_then(|v| v.as_str()) == Some("notemd/roots") {
            roots = msg.get("params").and_then(|p| p.get("roots"))
                .and_then(|r| r.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
            continue;
        }

        let mut env = build_env(&app);
        if let Some(e) = env.as_mut() { e.roots = roots.clone(); }
        if let Some(reply) = dispatch::handle(env.as_ref(), &msg) {
            if w.write_all(format!("{reply}\n").as_bytes()).await.is_err() { break; }
            let _ = w.flush().await;
        }
    }
}
```

把 `shim.rs` 的 `run_shim` 整体替换为下面这版。变化有三:记住 client 的 roots
能力、在第一次 `tools/call` 之前向 client 反向请求一次 `roots/list`、把拿到的
roots 作为 `notemd/roots` 通知随连接先发给 server。

**难点在于反向请求**:发出 `roots/list` 后,client 回来的下一条**未必**是它的
响应——可能是它自己的下一个请求。所以不能「发完读一行就当答案」,必须一边读一边
把不属于这次反向请求的消息照常处理完,直到看见 `id == "notemd-roots"` 那条。

```rust
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
            dispatch::handle(None, &msg)
                .unwrap_or_else(|| protocol::error(msg.get("id").unwrap(), -32603, "internal: no reply"))
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
```

`forward` 的签名相应改为带 roots,并在转发目标请求**之前**先发一条
`notemd/roots` 通知(同一条连接内,server 端在 `serve_one` 里已经会认这个方法):

```rust
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
```

**注意**:Task 9 Step 5 会再动一次 `forward`(加超时)。若先做 Task 9 再做 Task 8,
上面这版已经含超时,不要重复包。

- [ ] **Step 4: 帮助文案**

`builtin.rs` 的 `render_help` 里,`search` 那一行之后加:

```
  mcp              Serve this vault to agents over MCP (stdio). Register with:
                     { "command": "notemd", "args": ["mcp"] }
```

- [ ] **Step 5: CHANGELOG(硬门禁,忘了就发不出去)**

`CHANGELOG.zh-CN.md` 的「未发布」区加:

```markdown
- **MCP server**:note.md 现在可以把 vault 的检索能力供给 agent。在 Claude Code /
  Cowork / Codex 里注册 `{"command": "notemd", "args": ["mcp"]}` 即可,agent 便能用上
  中文分词、相关性排序与 origin 加权,而不是退回 grep。只读,不开网络端口。
```

`CHANGELOG.md` 的 Unreleased 区加:

```markdown
- **MCP server**: note.md can now serve your vault's search to agents. Register
  `{"command": "notemd", "args": ["mcp"]}` in Claude Code / Cowork / Codex and the agent
  gets Chinese segmentation, relevance ranking and origin weighting instead of falling back
  to grep. Read-only; no network port is opened.
```

- [ ] **Step 6: 全量验收**

Run: `cd src-tauri && cargo test`
Run: `node scripts/changelog.mjs check`
Expected: 全绿;changelog 门禁通过。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/mcp/server.rs src-tauri/src/mcp/shim.rs \
        src-tauri/src/cli/builtin.rs CHANGELOG.md CHANGELOG.zh-CN.md
git commit -m "feat(mcp): roots 会话拉取、help 文案与 CHANGELOG"
```

---

---

## Task 9: 设置开关(默认开)与即时生效

**Files:**
- Create: `src-tauri/src/mcp/gate.rs`
- Modify: `src-tauri/src/mcp/server.rs`(受 gate 控制)、`src-tauri/src/mcp/shim.rs`(加超时)
- Modify: `src-tauri/src/lib.rs`(注册 `set_mcp_enabled` 命令)
- Modify: `src/lib/settings.svelte.ts`、`src/components/SettingsDialog.svelte`

**Interfaces:**
- Produces:
  ```rust
  pub fn enabled_from_settings(app: &tauri::AppHandle) -> bool;   // 键缺失 = true
  pub fn start(app: &tauri::AppHandle);
  pub fn stop();
  #[tauri::command] pub fn set_mcp_enabled(app: tauri::AppHandle, enabled: bool);
  ```

**为什么默认开**:这个能力的价值全在「agent 想用的时候它就在」。默认关意味着
用户得先知道有这功能、再去翻设置——而绝大多数人是在 agent 报「找不到 notemd」
之后才会去找。默认开的代价近乎为零:不开端口、不联网、只读、复用已有索引。

**为什么放应用级设置**:`.notemd/settings.json` 随 git 同步,而「这台机器要不要
对外提供 MCP」是每台机器各自的事;台式机开、笔记本关是合理配置,同步过去反而是错的。

- [ ] **Step 1: 写失败的测试**

`src-tauri/src/mcp/gate.rs` 末尾:

```rust
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
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test mcp::gate`
Expected: 模块不存在,编译失败。

- [ ] **Step 3: 实现 gate**

`src-tauri/src/mcp/gate.rs`:

```rust
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

pub fn start(app: &AppHandle) {
    let mut guard = TASK.lock().unwrap();
    if guard.is_some() { return; }              // 已在跑,幂等
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
    if enabled { start(&app) } else { stop() }
}
```

- [ ] **Step 4: 把 server 改成可被 gate 控制**

`server.rs` 里把 `init` 换成两个函数:

```rust
/// 返回 JoinHandle,好让 gate 能 abort 它。
pub fn spawn_listener(app: AppHandle) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut listener = match crate::platform::ipc::listen().await {
            Ok(l) => l,
            Err(e) => { eprintln!("notemd: MCP 监听未启动: {e}"); return; }
        };
        loop {
            let Ok(stream) = listener.accept().await else { continue };
            let app = app.clone();
            tauri::async_runtime::spawn(async move { serve_one(app, stream).await });
        }
    })
}

/// 启动时按设置决定开不开。
pub fn init(app: &AppHandle) {
    if crate::mcp::gate::enabled_from_settings(app) {
        crate::mcp::gate::start(app);
    }
}
```

`lib.rs` 的 `invoke_handler` 里注册命令:

```rust
            mcp::gate::set_mcp_enabled,
```

- [ ] **Step 5: 外壳加超时**

`shim.rs` 的 `forward` 里,把 `rt.block_on(async { ... })` 整体包进超时:

```rust
    rt.block_on(async {
        // 5s:残留的 socket 文件会让 connect 成功但永远没人回,不能无限等。
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            let stream = crate::platform::ipc::connect().await.map_err(|e| e.to_string())?;
            let (r, mut w) = tokio::io::split(stream);
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
```

并把 `shim.rs` 里那句错误文案改成同时覆盖两种成因:

```rust
                    &format!(
                        "note.md 未运行,或 MCP 服务已在设置中关闭({e})。\
                         启动 note.md / 在设置里打开 MCP 服务后即可检索;\
                         在此之前请用 grep/rg 兜底。"
                    ),
```

- [ ] **Step 6: 前端设置项**

`src/lib/settings.svelte.ts` 的 `settings` 里加一行(跟 `dailyNotes` 同款):

```ts
  mcpServer: { enabled: boolean }
```

`loadSettings()` 里加(**缺失即 true**):

```ts
  const storedMcp = await s.get<{ enabled: boolean }>('mcpServer')
  settings.mcpServer = { enabled: storedMcp?.enabled !== false }
```

`saveSettings()` 里加:

```ts
  await s.set('mcpServer', settings.mcpServer)
```

`src/components/SettingsDialog.svelte` 里,与其他功能开关同列加一个复选框,
副文案给出可直接复制的注册行:

```svelte
<label class="setting-row">
  <input
    type="checkbox"
    bind:checked={settings.mcpServer.enabled}
    onchange={async () => {
      await saveSettings()
      await invoke('set_mcp_enabled', { enabled: settings.mcpServer.enabled })
    }}
  />
  <span>{t('settings.mcp.enable')}</span>
</label>
<p class="setting-hint">{t('settings.mcp.hint')}</p>
<code class="setting-code">{'{"command": "notemd", "args": ["mcp"]}'}</code>
```

i18n 键加进 `src/lib/i18n/en.ts`(以及各语言目录):

```ts
  'settings.mcp.enable': 'Serve this vault to agents over MCP',
  'settings.mcp.hint':
    'Lets Claude Code, Cowork and Codex search your vault with note.md’s ranking instead of grep. Read-only; no network port is opened. Register it with:',
```

- [ ] **Step 7: 跑测试**

Run: `cd src-tauri && cargo test mcp::`
Run: `pnpm test -- settings`
Expected: 全绿。

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src/mcp/gate.rs src-tauri/src/mcp/server.rs src-tauri/src/mcp/shim.rs \
        src-tauri/src/lib.rs src/lib/settings.svelte.ts src/components/SettingsDialog.svelte \
        src/lib/i18n/en.ts
git commit -m "feat(mcp): 设置开关 mcpServer.enabled,默认开,切换即时生效"
```

---

## 验收清单(全部任务完成后)

- [ ] `cd src-tauri && cargo test` 全绿,且**既有 search 测试一个字没改**
- [ ] `notemd search --json "x"` 的输出与改动前逐字节相同
- [ ] `printf '{"jsonrpc":"2.0","id":1,"method":"tools/list"}\n' | notemd mcp` 在 note.md **没开**时仍返回 2 个工具
- [ ] note.md 开着时,`tools/call` 的 `search` 返回相对路径、带 `vault_id` 与 `mount`
- [ ] `<vault>/.notemd/vault-id` 生成且重启后不变
- [ ] 没有任何 TCP 端口被监听:`lsof -nP -iTCP -sTCP:LISTEN | grep notemd` 为空
- [ ] **全新安装(settings.json 无 `mcpServer` 键)时 MCP 默认开着**
- [ ] 设置里关掉 → socket 文件消失,外壳 5s 内返回 isError 而不是挂住;重新打开 → 立刻可用,无需重启
- [ ] 在真实 Cowork 会话里注册并成功 `tools/call`(spec §10 的未决项 1)

## 与 spec 的偏差登记

无。spec §10 的四条未决项中,1(Cowork tools/call 实证)进入上面的验收清单;
2/3/4 按 spec 所述留待后续,本计划不实现。
