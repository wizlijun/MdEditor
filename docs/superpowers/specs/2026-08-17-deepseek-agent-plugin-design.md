# DeepSeek Harness Agent 插件设计(notemd.deepseek-agent)

日期:2026-08-17  
状态:方案,待确认  
对标:`docs/superpowers/specs/2026-07-30-claude-headless-agent-plugin-design.md`(claude-agent,骨架来源)  
外部依据:

- `@deepseek-ai/dsh-acp` npm 实查(2026-08-17):`0.0.1-rc.1`,官方,"Automation-only Agent Client Protocol server for driving DeepSeek Harness agents over JSON-RPC stdio";依赖 `@agentclientprotocol/sdk 0.25.1`;peer 依赖 `@deepseek-ai/cordis` / `dsh-agent` / `dsh-session` / `dsh-invariants` / `dsh-user-approval`(均 rc)
- `@deepseek-ai/dsh` npm 实查:最新 `0.1.0-rc.6`,开发者预览,上游明示会有破坏性变更
- DSH Desktop 升级机制分析:`deepseek-harness-desktop/docs/analysis/upgrade-and-upstream-integration.zh.md`(profile composition、断言式契约、版本齐步走闸门的实证)

## 一句话

把用户电脑上已装的 {==DeepSeek Harness 经 **ACP**==}{>>Claude code是否支持ACP?<<}**(Agent Client Protocol v1)** 接进 vault:notemd 侧一个 ACP 客户端插件,dsh 侧**零自研代码**(官方 `dsh-acp` + 一个 notemd 专用 profile,纯配置);依赖钉在公共协议上而不钉在 dsh 版本上;同时把宿主的 agent 插槽从「claude 专用」泛化为「可注册、可设默认」,让 deepseek 成为主力 agent 实现。

## 0. 对原始设想的裁决

原设想:「在 deepseek harness 上增加一个 notemd 插件;notemd 中再实现一个 deepseek harness 插件;插件只需维护好和 deepseek harness 的版本依赖关系」。三条裁决:

1. **「dsh 上加 notemd 插件」→ 不需要自研。** 官方 `@deepseek-ai/dsh-acp` 已经就是那个插件:它以 Cordis 插件形态挂进 dsh,把会话、流式输出、工具调用、权限审批、会话续传、MCP 挂载全部以 ACP(NDJSON JSON-RPC 2.0 over stdio)暴露给外部程序。自研 dsh 插件意味着追着 `0.1.0-rc` 阶段的 Cordis 内部 API 跑——这正是要避免的版本包袱。notemd 专属能力(vault 搜索、大纲写回工具)后续经 ACP 的 **MCP 挂载**注入会话,MCP 同样是稳定公共协议,依旧不写 dsh 侧代码。
2. **「notemd 中实现 deepseek 插件」→ 对,但它是 ACP 客户端,不是 dsh 客户端。** 插件二进制对 dsh 的了解只有两件事:怎么找到它(可执行探测),怎么跟它说话(ACP)。
3. **「维护版本依赖关系」→ 反转为「钉协议,不钉版本」。** 不追踪 dsh 版本号;`initialize` 握手协商 ACP 协议版本 + 能力探测 + 缺失能力降级 + 断言式 fail loud。dsh 侧唯一落点是一个 profile(`$DSH_HOME/profiles/notemd`,配置而非代码),增删改一律走上游 CLI `dsh plugin --profile notemd …`——「上游 CLI 保持权威」是 DSH Desktop 已实证的姿势(见其分析报告 §5.1)。

## 1. 为什么走 ACP 而不是 headless CLI

dsh 提供 `dsh --profile headless "job"`(跑完打印结果退出),对标 `claude -p`。但用户要求「比 claude -p 能力更强,作为主力插槽实现」,逐项对比:

| 能力 | `claude -p` stream-json(现 claude-agent) | `dsh --profile headless` | **ACP(dsh-acp)** |
| --- | --- | --- | --- |
| 流式事件 | 有(单向,粗粒度) | 无结构化流 | 有:文本/思考 delta、工具调用含 diff 与文件定位 |
| 会话续传 | `--resume` 存在但未接入;record 只存 session_id | 不可控 | `session/load` 完整回放 + 静默恢复;`session/list` 枚举 |
| 权限审批 | 无人值守,只能**全量预批准**(settings.local.json) | 同左 | `session/request_permission` 运行时逐次请求,插件按策略应答或转人工 |
| 计划/模式汇报 | 无 | 无 | plan 条目、mode 切换、slash commands |
| 注入自有工具 | 每机器配 `.mcp.json` | 同左 | `session/new` 参数级 MCP 挂载,按会话注入 |
| 取消 | kill 进程组 | kill | `session/cancel`(协议级)+ kill 兜底 |
| 版本耦合面 | claude CLI 的 argv 与输出格式(未承诺稳定) | dsh CLI 输出(rc,必变) | **ACP v1,Zed 维护,独立于 dsh 演进,多 agent 已实现** |

结论:ACP 每一项都持平或更强,且耦合面最小。headless CLI 仅作为 `selfcheck` 里的环境验证手段,不作为运行通道。

## 2. 解耦分层(每层钉在公共协定上)

```
第 5 层  宿主 agent 插槽泛化      agent provider 注册 + 默认 agent 设置(本设计 §6)
第 4 层  notemd.deepseek-agent    ACP 客户端插件;复用 agent 运行骨架(本设计 §3)
─────────────────────────── 协议边界(本插件唯一的对外依赖)───────────────────────────
第 3 层  ACP v1 / MCP             @agentclientprotocol/sdk;版本经 initialize 协商
第 2 层  dsh 侧落点               官方 dsh-acp + $DSH_HOME/profiles/notemd(纯配置,上游 CLI 权威)
第 1 层  文件层公共约定           AGENTS.md、OKF v0.2、.note.md、actor 署名 —— 不装任何插件也成立
```

第 1 层是底线:即使第 2-4 层全部失效,用户仍可 `cd <task> && dsh` 手工跑——任务模板是纯文本、住在 vault 里(信念 2「文件高于应用」);第 5 层是产品面:插槽对**任意** harness 开放,deepseek 只是第一个受益者(信念 5「一个 vault,多个 agent」从文件层推进到插槽层)。

## 3. 形态与进程模型

插件 `notemd.deepseek-agent`,后端 + 前端形态,工程骨架完全对标 `plugins-src/claude-agent/`:

- 后端:Rust crate `plugins-src/deepseek-agent/backend` → `bin/notemd-deepseek-agent`
- 前端:Svelte 独立窗口 `ui/`(流式 UI 复用 claude-agent 的 `events.ts` wire 契约与 `RunStream` 归并逻辑)
- `capabilities: ["ui", "toast", "vault.read", "notify"]`
- `activation.events`:同 claude-agent(`onCommand:open/run/run-note/run-task/run-status` + `onCli:dsagent`)
- `request_timeout_seconds: 300`;CLI 默认 detach(runner 模式原样复用)

### 3.1 先抽共享 crate:`agent-run-core`

claude-agent 后端里 harness 无关的模块——`lock.rs`(任务锁)、`record.rs`(RunRecord/Progress)、`artifacts.rs`、`okf.rs`、`task.rs`(模板扫描)、`precheck.rs`、`mirror.rs`、runner 骨架——抽成 workspace 内共享 crate `plugins-src/agent-run-core`,claude-agent 同步迁移。两个插件此后共享同一套「运行记录/锁/进度/工件」语义,`agent-runs/` 目录格式天然一致,宿主插槽读到的状态形状相同。

harness 特有、各自实现的只剩三块:**可执行探测、传输层、事件映射**。

### 3.2 dsh 发现与 profile 引导(一次性)

探测链(照 `discover.rs` 的三层结构):插件设置显式路径 → `$DSH_PATH` → `/bin/zsh -lic 'command -v dsh'`(带回 login PATH,OnceLock 缓存)→ 常见候选(`~/.local/bin/dsh`、`/opt/homebrew/bin/dsh`、`/usr/local/bin/dsh`、npm 全局 bin)。找不到 → toast + 窗口内安装指引(`npx @deepseek-ai/dsh` 或 DSH Desktop)。

首次运行引导 notemd 专用 profile(幂等):

```
dsh plugin --profile notemd add -w @deepseek-ai/dsh-acp
```

- profile 不存在则上游 CLI 自建;已存在则不动用户自有插件(照 DSH Desktop `desktopBundleList` 的「官方前缀 + 保序第三方」精神,但我们更简单:只保证 dsh-acp 在场)
- **插件永不直接改写 `$DSH_HOME` 下任何文件**,一切经上游 CLI —— 版本解析、lockfile、bundle reconcile 都是上游语义
- 引导失败(网络/registry)→ fail loud,给出可复制的命令让用户在终端自跑

### 3.3 进程模型:每 run 一个 dsh-acp 子进程

沿用 `engine.rs` 的全部生命周期语义:`setsid` 自成进程组、stdout 逐行读、**静默超时**(每收到事件 reset,不设总时长上限)、取消即 `killpg(SIGTERM)`、stderr 尾部 2KB 入 record。

差异仅在传输层:stdin/stdout 不再是单向 stream-json,而是双向 NDJSON JSON-RPC 2.0:

```
spawn dsh-acp (cwd = <vault>/.notemd/agent-tasks/<task>/, env: DEEPSEEK_API_KEY?, DSH_PERMISSION_MODE)
  → initialize            协商 protocolVersion,取 agentCapabilities → 能力降级表(§5)
  → session/new           cwd = 任务目录;mcpServers = [](Phase 3 挂 vault 搜索)
  → session/prompt        三段拼接 prompt(§4),复用 claude-agent 的组装顺序
  ← session/update*       流式:文本/思考 delta、tool_call、plan → 映射进 Event → ui_post + progress
  ← session/request_permission → 按任务 policy 应答(§4.2);窗口模式可转人工
  ← stopReason            → 写 RunRecord(status、num_turns、acp session id)→ 终止进程
```

续传:新起进程 → `initialize` → `session/load`(能力在场时)→ 继续 `session/prompt`。会话日志活在 `$DSH_HOME`(dsh 的 Trajectory),RunRecord 只存指针(session id + harness 标识)——与「索引是派生数据,文件是唯一事实源」同构:我们不复制会话,只记录怎么找回它。

选择「每 run 一进程」而非常驻 server:与锁模型(同任务互斥)、detached runner(CLI 触发后插件宿主可退出)、崩溃隔离全部天然对齐;常驻化是纯性能优化,留到有实测需求再做。

### 3.4 事件映射(ACP → 既有 wire 契约)

`stream.rs` 的 `Event` 枚举是宿主与窗口都认的契约,deepseek-agent 实现 `acp.rs` 做投影:

| ACP update | Event(既有) | 备注 |
| --- | --- | --- |
| `agent_message_chunk`(text) | `Text { text }` | 前端已有连续文本归并 |
| `tool_call` / `tool_call_update` | `ToolUse { name, brief }` | brief 取 title/文件路径,截 120 字符;diff 详情 Phase 2 进窗口 |
| thought/reasoning delta | `Text`(可选前缀)或丢弃 | v1 丢弃,record 不存思考 |
| `plan` | 新增 `Plan { items }`(可选变体) | 旧前端忽略未知 kind,向后兼容 |
| permission 请求/应答 | 新增 `Permission { tool, decision }` | 进流供审计,同时落 record |
| stopReason | `Result(RunResult)` | `session_id` 存 ACP sessionId |

新增变体只增不改,`events.ts` 同步扩展;claude-agent 前端零改动。

## 4. vault 目录约定与权限

### 4.1 任务模板:同一目录,双 harness 共用

复用 `<vault>/.notemd/agent-tasks/<task>/`,`task.json` 形状不变(`model` 字段透传给 `session/new`)。指令文件:

- 模板目录新增 `AGENTS.md`(harness 中立的公共约定名,vault 根早已如此:`CLAUDE.md` 是 `AGENTS.md` 的 symlink);现有模板的 `CLAUDE.md` 反转为指向 `AGENTS.md` 的 symlink
- **Spike 必验项**:dsh 是否沿 cwd 向上发现 `AGENTS.md`(vault 根那份 + 任务那份都要被读到)。若不发现,则降级方案:把两份指令拼进 `session/prompt` 的首个内容块——语义等价,只是离开了「cwd 定在任务目录就够了」的免费午餐

### 4.2 权限:从「全量预批准」升级为「策略应答」

claude-agent 因 headless 无人值守,被迫把权限全部预写进 `settings.local.json`。ACP 给了更强的模型:运行时逐次 `session/request_permission`,插件是应答方。任务模板新增 harness 中立的 `policy.json`:

```jsonc
{
  "permission_mode": "workspace-write",        // dsh-acp 三级沙盒预设:read-only | workspace-write | danger-full-access
  "allow": ["read:${VAULT}/**", "write:${VAULT}/**/*.note.md", "write:${VAULT}/answers/**"],
  "deny":  ["shell:*", "write:${VAULT}/**/*.md"]   // 例:answer-note-question 不给源 .md 写权限
}
```

- 后端 materialize `${VAULT}`/`${NOTE}` 占位(照 `settings.rs` 的做法),据此应答每个权限请求;不匹配任何规则 → 默认拒绝并记进 record(fail-closed)
- 窗口在场时,未匹配请求可弹给人确认(Phase 2)——这是 claude-agent 做不到的交互面
- claude-agent 后续(Phase 3,非本期)可从同一份 `policy.json` 生成它的 `settings.local.json`,两 harness 收敛到单一策略源

### 4.3 署名与协议红线(与 claude-agent 完全一致)

- actor 一律 `deepseek-harness/<model>`(如 `deepseek-harness/deepseek-v4`),写进 `by::`、`generated.by`;**绝不 `human:`**(OKF §7;`✦`/`●` 的同源红线)
- `type:: answer` 即署名,绝不手写 `✦`(围栏损毁问题,见 `AGENTS.md`)
- 状态机:agent 只许 `open → answered`;`closed`/`adopted` 归人。三重落地:AGENTS.md、任务指令、`policy.json` 的 deny 规则
- 新写入点过 `src/lib/okf/concept.ts` 登记 + `pnpm okf:lint` 自检(CLAUDE.md 硬规矩)

## 5. 版本策略:「不特别维护版本」的具体机制

| 机制 | 内容 | 出处/类比 |
| --- | --- | --- |
| 协议协商 | `initialize` 声明客户端 ACP 版本,不满足 → 明确报错 + 指引 `dsh plugin --profile notemd update` | ACP v1 规范 |
| 能力降级表 | `loadSession` 缺 → 禁用续传 UI,退回无状态 run;image 能力缺 → 不发图;MCP 挂载失败 → 无 vault 工具继续跑。**每项降级都 toast 告知,不静默** | DSH Desktop「fail loud / 降级要通知」 |
| 断言式契约 | 握手形状、必需方法在场性,启动即验;上游改名/移除 → 立刻报错而非跑出错误组合 | DSH Desktop §4.4(c) 的 row 断言 |
| 只记不 gate | RunRecord 记录观察到的 dsh/dsh-acp 版本(诊断用);插件**不**因版本号拒绝运行 | 反向借鉴 `upstream.json`:桌面产品要齐步走闸门,插件要的是最大兼容 |
| 上游 CLI 权威 | profile 内包版本永远由 `dsh plugin` 解析,插件零 npm 依赖、零 lockfile | DSH Desktop §5.1 |
| CI 契约测试 | 定时 job:装 latest rc 的 dsh + dsh-acp,跑桩会话脚本(initialize → session/new → prompt → 取消),drift 在用户遇到前暴露 | DSH Desktop 报告 §7.2 建议的自动化探测 |

**风险登记**:dsh 全家族仍在 `0.1.0-rc`,dsh-acp 仅 `0.0.1-rc.1`,破坏性变更是承诺过的常态。上述机制把破坏面收敛为「CI 先红 → 适配一次事件映射/握手」,而不是用户侧静默坏掉。备援:第三方 `@openma/deepseek-harness-acp`(同为 ACP server,Apache-2.0)可作 dsh-acp 停摆时的替换件——**这正是钉协议的红利:server 可换,客户端不动**。

## 6. 宿主泛化:插槽从「claude 专用」到「可注册、可设默认」

现状两处硬编码,是 deepseek 成为「主力插槽实现」的仅有障碍:

1. `src/lib/agent-workspace/store.svelte.ts` — `PLUGIN_ID = 'notemd.claude-agent'`(插槽 A:大纲面板 Agent 区)
2. `src-tauri/src/plugin_runtime/ui_rpc.rs:1073` — `const AGENT_PLUGIN = "notemd.claude-agent"`(插槽 B:`host.agent.*` 中转)

改法(约定优于新协议,不加新 manifest 字段族):

- **provider 约定**:声明了 `agent` 语义的插件 = manifest `contributes` 标记 `"agent_provider": true` + 实现标准命令三件套 `run-task` / `run-note` / `run-status`(claude-agent 已满足,加一行标记即可)
- **默认 agent 设置**:宿主设置项 `agent.default_provider`(默认 `notemd.claude-agent`,可切 `notemd.deepseek-agent`);`agent_execute()` 读设置替代常量;`host.agent.run` 增加可选 `harness` 参数显式指定(缺省走默认)——`ebook-import`/`idea-spark` 零改动
- **插槽 A 列表化**:`agentPluginAvailable()` 改为枚举 provider 插件;每个 provider × 每个任务一行(`AgentWorkspace.svelte` 的注释早已预告这个形态);轮询协议不变
- 兼容性:单 claude-agent 安装时行为与今天逐字节一致

## 7. 失败面

| 情形 | 判据 | 处理 |
| --- | --- | --- |
| 找不到 `dsh` | 探测链全空 | toast + 窗口安装指引(`npx @deepseek-ai/dsh` / DSH Desktop) |
| profile 引导失败 | `dsh plugin` 非零退出 | 呈现 stderr,给出可复制命令让用户终端自跑 |
| 未认证 | 握手/首 prompt 报 auth | 指引设置 `DEEPSEEK_API_KEY`(插件设置,仅注入子进程环境)或 dsh 侧登录 |
| 协议版本不满足 | initialize 失败 | fail loud + `dsh plugin --profile notemd update` 指引 |
| 能力缺失 | agentCapabilities 探测 | 按 §5 降级表,toast 告知 |
| 权限请求超策略 | 无匹配规则 | 默认拒绝,记进 record;窗口在场转人工(Phase 2) |
| 同任务已在跑 | `lock` 在且 pid 活 | 拒绝并告知时长;陈旧锁回收(agent-run-core) |
| 限流 | 错误文本/错误码 | 记录并提示,不自动重试 |
| 静默超时 | 超 `timeout_seconds` 无事件 | `killpg`,状态 `timeout` |
| 插件进程被关 | `$deactivate` / idle | kill 在跑子进程;detached runner 不受影响 |

## 8. 测试

- **Rust 单测**(不碰真 dsh):握手/能力协商状态机;ACP→Event 映射(半行、乱序 update、未知 update 容错);policy 应答(allow/deny/默认拒/占位替换);探测链优先级;record/lock 复用 agent-run-core 既有测试
- **端到端**:桩 ACP server 脚本(按剧本吐 NDJSON JSON-RPC)驱动五条路径——成功 / 错误 / 静默超时 / 取消 / 权限拒绝
- **前端 vitest**:新增 Plan/Permission 变体的 reducer 归并;旧消息形状回归(claude-agent 前端零改动的证明)
- **协议死锁回归**:照抄 claude-agent 的 `activate_never_blocks_the_protocol_loop` 两个测试——同一个坑不踩第二次
- **人工验收**:selfcheck 模板(报告 dsh 版本、协议版本、能力位、读到的 AGENTS.md 清单);answer-note-question 在 deepseek 下跑通完整 sweep 且署名/状态机红线全部成立;claude 与 deepseek 各跑一任务并行互不干扰

## 9. 分期

| 期 | 内容 | 动到宿主? |
| --- | --- | --- |
| **P0 Spike(~1 天)** | 手工验证:dsh-acp 启动形态(profile 方式 vs 独立命令)、initialize 能力位实录、**AGENTS.md 是否被 cwd 发现**(§4.1 分叉点)、session/load 行为 | 否 |
| **P1 MVP** | 抽 `agent-run-core`(claude-agent 迁移,行为不变);deepseek-agent 插件本体:发现+引导、ACP 客户端、三件套命令、窗口、CLI detach、record/OKF/署名;selfcheck 模板 | 否(仅 workspace 重构) |
| **P2 主力化** | 宿主 provider 泛化 + `agent.default_provider` 设置(§6);交互式审批 UI;续传/会话列表 UI;tool diff 展示 | 是(两处硬编码 + 设置) |
| **P3 深化** | `session/new` 挂 vault 搜索 MCP(搜索设计 L3 的落点);`policy.json` 统一双 harness;CI 契约定时 job | 否 |

工程杂项(每期照惯例):`scripts/dev-install-plugin.sh` 加分支、`scripts/release-plugins.sh` 打包签名、`CHANGELOG.md` + `CHANGELOG.zh-CN.md` 「未发布」区双语条目(**硬门禁**)、插件市场 index。

## 10. 使用边界

- 计费面:dsh 走 `DEEPSEEK_API_KEY` 或任意 OpenAI 兼容端点,无 claude 订阅那类「仅限 `-p`」的政策约束;但插件仍仅供账号本人的自动化,不得包装为多用户服务(写进 README 与市场描述)
- dsh 处于开发者预览期:插件市场描述明示「实验性,依赖 DeepSeek Harness rc 版本」,并链接 §5 的降级行为说明