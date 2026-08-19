# notemd MCP server 设计

日期:2026-08-19
状态:已与用户对齐,待评审后进入实施
上游需求:`sotvault/project/note.md/2026-08-18-claude-notemd-mcp-vault-info-spec.md`

## 背景与目标

沙盒里的 agent 想检索这个 vault,却拿不到 notemd 的三样核心价值——中文分词、
相关性排序、以及「人工批注优先于机器摘要」的 origin 加权,只能退回 rg/grep 全文扫描。
把 `notemd search` 以 MCP server 的形式供出去,让 agent 以工具调用访问跑在索引所在
本机的检索能力。

对应产品信念:**一个 vault,多个 agent,你是编排者**(同一份索引喂 Cowork / Claude Code /
Codex,谁擅长什么按活儿派)、**agent 建议你确认**(MCP 只读,永不写 vault)、
**文件高于应用**(返回 vault 相对路径,读全文仍由文件系统承担)。

### 上游 spec 的核心前提已被推翻

上游 spec 假设「沙盒 agent 够不到本机 notemd,因为命令跑在 Linux 虚拟机里」。
2026-08-19 的实测探针证明**这只对 bash 成立,对 MCP 不成立**:

- Cowork 的 agent loop(clientInfo 为 `local-agent-mode-*`)跑在宿主原生进程里,
  **不在 VM 里**。探针在 Cowork 的 VM 完全离线(镜像下载失败)的情况下,
  仍然完成了 `initialize` → `tools/list`。
- 因此 MCP server 不需要解决「VM → 宿主」的网络穿透,也不需要公网可达。
- 协议版本 `2025-11-25`;Cowork 声明 `roots.listChanged` 能力并主动推送变更通知。
- Claude Desktop 的 `claude_desktop_config.json` **只接受 stdio**,配置 HTTP 传输会被
  判为 "not valid MCP server configurations" 而跳过。

这三条直接决定了下面的传输选型与握手设计。探针本身是一次性的,已清理。

## 已定决策(用户拍板)

1. **常驻形态**:GUI 主程序内嵌。复用主程序里已经热的 `searchidx` 索引与 watch 线程,
   零额外索引开销;开关与状态放设置页。
2. **工具面**:只给 `search` + `vault_info`,**不给 `read`**。读全文一律靠 agent 自己的
   挂载目录,MCP 不做文件分发。
3. **共用 CLI 接口的切法**:抽纯函数共用,**不是 spawn 子进程**(推翻上游 spec 的
   「内部 spawn `notemd search --json`」)。
4. **IPC**:外壳与主程序之间走 Unix domain socket(Windows 用 Named Pipe),
   不开 TCP 端口。
5. **随主程序启动,可在设置里关**:设置项 `mcpServer.enabled`,**默认开**。

## 一、架构与进程边界

```
Cowork / Claude Code / Codex
      │  stdio, JSON-RPC 2025-11-25        ← 探针已实证可用
      ▼
  notemd mcp                               ← CLI 子命令,极薄外壳,不碰索引
      │  UDS(unix) / Named Pipe(windows)
      ▼
  note.md GUI 主程序
      └─ mcp::server                       ← 工具分发、roots 握手
           └─ search::execute()            ← 与 CLI 共用的纯函数
                └─ searchidx               ← 已经热的索引 + watch 线程
```

### 1.1 外壳与主程序是同一个二进制

`cli::is_cli_mode()` 已经在按 argv 判别 GUI 与 CLI,`notemd mcp` 只是多一个子命令
(`cli::router::Builtin` 加一个分支)。由此白捡一个性质:**工具的 JSON Schema 是同一个
编译期常量**,外壳报的和 server 认的不可能对不上——不靠约定,靠编译。

### 1.2 note.md 未运行时,注册仍然成立

外壳自己持有静态工具定义,`initialize` 与 `tools/list` **不需要主程序在线**;
只有 `tools/call` 才连 IPC。这条不是优化,是必需:MCP 客户端在**会话启动那一刻**
枚举工具,而那一刻用户的 note.md 未必开着。若此时返回空列表,agent 会认为
notemd 这个能力根本不存在,整场会话都不会再问一次。

连不上时 `tools/call` 返回带 `isError: true` 的结果(不是协议层 error),内容说明
「note.md 未运行」,让模型能据此降级到 grep 而不是把整轮工具调用判死。

## 二、共用 CLI 接口:抽纯函数,不 spawn

这是对上游 spec 唯一的实质性推翻。理由不是性能(虽然也省掉 fork + 重开 sqlite +
重跑 freshness sweep),而是**正确性**:`cli/search.rs` 里 `scan_options_for` /
`weights_for` / `conventions_for` 三个「单一构造点」的注释反复讲同一件事——
两份判断必然漂移。spawn 是把进程边界当接口,等于又造了一份需要维护的一致性。

### 2.1 切法

把 `cli::search::run()` 里「执行检索」与「打印 + 算 exit code」拆开:

```rust
/// 只执行,不打印,不管 exit code。CLI 与 MCP 的唯一共同入口。
pub fn execute(args: &SearchArgs) -> Result<SearchOutcome, SearchError>;
```

`SearchOutcome` 装的就是 `print_json` 现在拼的那些字段,一个不多一个不少:
`path` `line` `line_end` `text` `score` `breadcrumb` `level` `doc_date`
`source_ref` `provenance{agent_by,human_verified}` `origin` `attention_minutes`,
外加顶层的 `query` `route` `took_ms` `total`。

- `cli::search::run()` 改为:`execute()` → `print_plain`/`print_json` → exit code。
  外部行为**逐字不变**(含 exit 0/1/2 语义、`--context` 命中行失效时丢弃的规则)。
- `mcp::tools::search` 改为:`execute()` → 序列化成 tool result。

`--json` 的输出格式因此不是被 MCP 复制的,而是同一份数据的两个渲染。

### 2.2 参数面:一个语法,一个解析器

MCP 的 `search` 工具只收 `query` 字符串,里面照样写 `tag:x type:y origin:human
after:2026-01-01`——直接复用 `cli::search::parse_args` 已有的语法。agent 学的东西
与用户在 CLI 里敲的完全一样,vault 根 `AGENTS.md` 的 "Searching this vault" 一节
一字不用改。

**刻意不把 tag/type/path/ext/after/before 拆成独立参数**(与上游 spec 的
「参数直接映射 CLI」有出入):拆开等于把同一套过滤语义实现两遍,而这正是
§2 开头那条纪律要避免的。参数越少,模型越不容易写错。

## 三、跨平台

### 3.1 现状基线

| 平台 | 状态 | 依据 |
|---|---|---|
| macOS | 主力 | `tauri.conf.json` targets = `app`/`dmg` |
| Windows | 在发 | `scripts/release-windows.ps1`、`src-tauri/src/platform.rs`、39 处 `cfg(windows)/cfg(unix)` |
| Linux | 未发 | 全仓无 appimage/deb target,无 linux 专属分支 |

### 3.2 agent 面零差异

三个平台的注册行是同一句:

```json
{ "command": "notemd", "args": ["mcp"] }
```

stdio 是 MCP 规范里各平台一致的传输。**平台差异被完整关在「外壳 ↔ 主程序」这一跳里,
用户与 agent 永远看不见。**

### 3.3 那一跳的实现

| | macOS / Linux | Windows |
|---|---|---|
| 原语 | AF_UNIX socket | Named Pipe |
| 路径 | mac:`~/Library/Application Support/net.notemd.app/mcp.sock`<br>linux:`$XDG_RUNTIME_DIR/notemd/mcp.sock` | `\\.\pipe\net.notemd.app.mcp` |
| tokio | `tokio::net::UnixStream` | `tokio::net::windows::named_pipe` |
| 访问控制 | 文件权限 `0600` | 建管道时挂 SECURITY_DESCRIPTOR 限本用户 |

**不用「AF_UNIX 一把梭」**:Windows 10 1803+ 虽支持 AF_UNIX,但 **tokio 在 Windows 上
不支持它**(`UnixStream` 由 `cfg(unix)` 门死),需另引 `uds_windows` 并自建异步桥——
所谓统一只是换个地方分叉,还多背一个依赖。Named Pipe 是 tokio 一等公民。

**上层零分叉**:两边都是「一条字节流上跑 NDJSON JSON-RPC」,只有 `connect()` / `listen()`
两个函数需要 cfg 分叉。这个 framing 在本仓库已被验证可移植——插件体系用的就是它。

新增的 IPC 抽象**必须放进 `src-tauri/src/platform.rs`**,遵该模块已声明的纪律
(「the single funnel」;新代码不得在模块外直接调用平台原语)。

### 3.4 两个平台特有的坑

1. **unix 僵尸 socket**:主程序崩溃后 `.sock` 残留,下次 `bind()` 得 `EADDRINUSE`。
   正确姿势是**先 `connect()` 探活,被拒才 `unlink` 重建**——无脑删会踢掉一个正在
   健康运行的实例。Named Pipe 随进程消失,无此问题。
2. **`sun_path` 长度上限**:macOS 104 字节 / Linux 108。默认路径 63 字节安全,
   但用户名可以很长,代码里应**断言而非假设**,超限时降级并在设置页报错。

Linux 当前不发,但 unix 分支在 Linux 上逐字可用;路径按 `$XDG_RUNTIME_DIR` 分叉
现在写进去(一个 `if`)比以后返工便宜——runtime socket 不属于 config 目录。

## 四、vault 身份与握手

### 4.1 `.notemd/vault-id`

- 内容:UUID v4,一次生成永不改变;**重建索引不换 ID**。
- 位置:`<vault>/.notemd/vault-id`,与 `settings.json` 同待遇——**随 git 同步**,
  不按 deviceId 分区。同一个 vault 在多台机器上就是同一个身份。
- 生成时机:vault 首次被解析时幂等创建(存在且能解析成 UUID 则原样保留,否则创建)。
  唯一写入点,与 `search::options::for_vault` 同层。
- 不会引起索引抖动:`search::watch::should_forward` 已排除 `.notemd/`
  (仅放行 `.notemd/analytics/`),写 vault-id 不触发重索引。

### 4.2 用 roots 把握手从君子协定升级成服务端判定

上游 spec 的握手是纯君子协定:agent 自己 Read `.notemd/vault-id`、自己比对、
自己降级。agent 忘了比对,错配就静默发生。

探针实测 Cowork 声明 `roots.listChanged` 并主动推送——意味着 **server 能反过来问
client「你挂载了哪些目录」**,自己完成比对:

1. 连接建立后调 `roots/list`;订阅 `notifications/roots/list_changed` 使结果失效重取。
2. 对每个 root 读 `<root>/.notemd/vault-id`。
3. 与本机 vault 的 ID 比对,得出三态:

| `mount.status` | 含义 | 行为 |
|---|---|---|
| `matched` | 某个 root 的 vault-id 与本机一致 | 正常返回;agent 可安全按相对路径读原文 |
| `mismatched` | 有 roots,但没有一个匹配 | **正常返回检索结果**,但响应显式标注路径不可在其挂载点解析 |
| `unknown` | client 未声明 roots 能力 | 正常返回;回落到上游 spec 的 agent 自查协议 |

**`mismatched` 为什么答而不拒**:检索结果对 server 的 vault 永远是对的,危险的不是
结果本身,而是 agent 把 `/dailynote/2026/x.note.md` 拿去在**自己的**挂载点解析,
读到一个同路径的别的文件。所以正确的缓解是**让错配无法被误解**,而不是拒绝服务——
agent 只是想知道你的笔记里有什么(而不打算读原文)是完全正当的用法,拒绝会误伤。

### 4.3 每次响应都带身份

`search` 与 `vault_info` 的响应**都**携带 `vault_id` 与 `mount.status`,防止会话中途
MCP 端更换 vault 配置而 agent 不知情。

## 五、工具契约

### 5.1 `vault_info`

无参数。零副作用、即时返回——agent 每会话至少调一次不应有成本顾虑。

| 字段 | 类型 | 说明 |
|---|---|---|
| `vault_id` | string | 身份判定的唯一依据 |
| `vault_root` | string | 本机视角绝对路径,**仅供人核对**;agent 不得用于路径拼接 |
| `entry_count` | number | 当前索引条目数 |
| `indexed_at` | ISO 8601 | 最后一次索引完成时间 |
| `notemd_version` | string | 便于排查行为差异 |
| `mount` | object | `{ status, matched_root }`,见 §4.2 |

`indexed_at` 的用途:agent 可自行判断新鲜度——明显早于挂载目录内最新文件 mtime 时,
检索近期内容应补一轮 grep 兜底。

### 5.2 `search`

| 参数 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `query` | string | 必填 | 支持全部过滤语法:`tag:` `type:` `path:` `ext:` `after:` `before:` `page:[[X]]` `origin:` |
| `limit` | integer | 20 | `0` = 不设上限 |
| `context` | integer | 0 | 每条命中前后附带的行数 |

返回沿用 `--json` 的完整字段(见 §2.1),外加 `vault_id` 与 `mount`。

**路径一律是 vault 相对路径**(如 `/dailynote/2026/2026-07-21.note.md`),
**绝不返回 `/Users/...` 绝对路径**;`vault_root` 只在 `vault_info` 里出现一次。

工具描述里必须写清 `provenance.agent_by` 与 `origin` 的含义,使 agent 能按 vault
`AGENTS.md` 的既有教义行事(见到 `agent_by` 就追 sources、需要时过滤 `origin:human`)。

## 五之二、启动与设置开关

MCP 监听**随主程序启动**,设置项 `mcpServer.enabled` **默认开**。

**为什么默认开**:这个能力的价值全在「agent 想用的时候它就在」。默认关意味着
用户要先知道有这个功能、再去找到开关——而绝大多数人是在 agent 报「找不到 notemd」
之后才会去翻设置。默认开的代价近乎为零:不开端口、不联网、只读、进程内复用
已有索引,没开 note.md 时连监听都不存在。

**为什么放应用级 `settings.json` 而不是 `.notemd/settings.json`**:后者随 git 同步,
而「这台机器要不要对外提供 MCP」是**每台机器各自的事**——台式机上开、笔记本上关
是完全合理的配置,同步过去反而是错的。

| 项 | 值 |
|---|---|
| 存储 | 应用级 `settings.json`(`app.store("settings.json")`,Rust 与前端同源) |
| 键 | `mcpServer.enabled` |
| 默认 | `true`。**键缺失即视为开**——老用户升级上来不需要做任何事 |
| 生效 | 立即。关 → 停止监听并清掉 socket 文件;开 → 重新监听。不需要重启 |
| UI | 设置页,与其他功能开关同列;副文案给出注册用的那行 JSON,方便直接复制 |

关闭时 socket 文件必须**一并删除**,否则外壳会连上一个不再有人 accept 的端点然后挂住;
外壳侧同时要有超时,不能无限等(见 §6)。

## 六、错误与降级

每一级失败都降级,不把整轮工具调用判死:

| 情形 | 行为 |
|---|---|
| note.md 未运行 / IPC 连不上 | `isError: true`,文案说明启动即可用 |
| MCP 被用户关掉 | 同上——外壳无从区分「没开」与「关了」,文案两种都提 |
| 端点在但无人 accept(残留 socket) | 外壳侧 5s 超时 → `isError: true`,不无限等 |
| 未配置 vault | `isError: true`,指向 Preferences |
| 索引不可用 | `execute()` 已有的 `fallback_scan` 兜底;响应里 `route` 如实标 `scan` |
| freshness sweep 超时 | 按现有索引作答,响应里标注(与 CLI 的 2s 上限同一条路径) |
| `roots/list` 失败或超时 | 降级为 `mount.status = "unknown"`,不影响检索 |

## 七、安全

- **只读**:不暴露任何写操作。MCP 面不提供创建/修改/删除,与信念 3 一致。
- **无网络面**:UDS / Named Pipe 不在网络栈上,不存在端口占用、DNS rebinding、
  CSRF、Origin 校验这一整类问题,也无需 token。
- **访问控制交给 OS**:unix `0600`;Windows 建管道时限本用户。
- **`vault_root` 会泄漏本机用户名给云端模型**——这是上游 spec 明确要的字段
  (供人核对),保留,但仅在 `vault_info` 出现,不进每条命中。

## 八、测试策略

1. **CLI/MCP 契约测试**:同一组参数下,`execute()` 的结果与 `notemd search --json`
   的 stdout 逐字段相等。仓库已有同类先例(`tests/search_cli_contract.rs`、
   `search_scan_options_contract.rs`),照写。
2. **CLI 行为不变回归**:`execute()` 抽取属纯重构,现有 search CLI 测试必须全绿且
   不修改。
3. **MCP 协议层单测**:`initialize` / `tools/list` / `tools/call` / 未知方法;
   note.md 未运行时 `tools/list` 仍返回完整工具定义。
4. **握手三态**:`matched` / `mismatched` / `unknown` 各一例,含 roots 中途变更。
5. **vault-id 幂等**:重复解析不换 ID;非法内容被替换;重建索引不换 ID。
6. **跨平台 IPC 冒烟**:unix 与 windows 两条分支各自可 listen/connect/往返一帧;
   unix 僵尸 socket 的探活-重建路径单独覆盖。

## 九、分期实施

| 阶段 | 内容 | 可独立验收 |
|---|---|---|
| P1 | 抽 `execute()` + 契约测试(纯重构,零行为变化) | ✅ CLI 行为逐字不变 |
| P2 | `.notemd/vault-id` 生成 + 幂等测试 | ✅ 不依赖 MCP |
| P3 | `platform.rs` 里的 IPC 层(UDS / Named Pipe) | ✅ 冒烟往返 |
| P4 | `notemd mcp` 外壳 + `mcp::server` + 两个工具 + roots 握手 | ✅ 端到端 |
| P5 | 设置页开关(`mcpServer.enabled`,默认开)与即时生效;文档(README、AGENTS.md 模板) | ✅ |

P1/P2 互不依赖,可并行。

## 十、未决与待验证

1. **Cowork 发起的 `tools/call` 未实证**。探针只走到 `tools/list`(Cowork 的 VM 镜像
   下载失败,起不了会话)。风险低——会枚举工具的 client 不会不调用——但没有实测,
   应在 P4 验收时补上。
2. **Cowork 认不认项目级 `.mcp.json` 的 `streamable-http` 注册,未实证**。本设计不
   依赖它;若将来证明可用且有需求,再在 §3.3 那一跳旁边加一个可选 TCP 监听
   (届时必须自带 token 与 Origin 校验,不能沿用 UDS 的「OS 即认证」假设)。
3. **外壳是否应在 note.md 未运行时自动拉起它**——当前设计是不拉起、只报错。
   自动拉起会让 agent 的一次检索变成弹出一个 GUI 窗口,侵入性偏高,留待真实使用
   反馈后再定。
4. **多 vault**:MCP 永远指向主程序当前配置的那一个 vault。若将来支持多 vault,
   `vault_info` 的返回形状需要扩展,届时按加字段而非改字段处理。
