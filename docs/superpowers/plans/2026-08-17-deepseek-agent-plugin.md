# DeepSeek Agent 插件(notemd.deepseek-agent)实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 DeepSeek Harness 经 ACP(Agent Client Protocol v1)接进 vault,成为与 claude-agent 并列、可设默认的第二个 agent provider。

**Architecture:** 三层。①抽出 harness 无关的共享 crate `agent-run-core`(锁/运行记录/工件/OKF/模板/precheck/镜像/detach 骨架),claude-agent 原样迁移;②新插件 `notemd.deepseek-agent` 作为 **ACP 客户端**,以 NDJSON JSON-RPC 2.0 驱动 `dsh-acp-demo` 子进程,harness 特有的只有三块:可执行探测、传输层、事件映射;③宿主把 agent 插槽从 `notemd.claude-agent` 常量泛化为 provider 注册表 + `agent.default_provider` 设置。

**Tech Stack:** Rust(tokio、serde_json、notemd-plugin-sdk)、Svelte 5 + Vite(插件窗口)、DeepSeek Harness `@deepseek-ai/dsh-acp-demo`、ACP v1。

## Global Constraints

以下每条都是**实测校核过的事实**,与原设计文档 `docs/superpowers/specs/2026-08-17-deepseek-agent-plugin-design.md` 有出入的以本节为准(出入原因见 §「与设计文档的偏离」)。

- **ACP 协议版本**:`PROTOCOL_VERSION = 1`(`@agentclientprotocol/sdk@0.25.1` 的 `dist/schema/index.js:48`)。
- **客户端只发四个方法**:`initialize`、`session/new`、`session/prompt`、`session/cancel`(最后一个是**通知**,无 id)。
- **代理端只回两个方法**:`session/update`(通知)、`session/request_permission`(请求,必须应答)。
- **`session/update` 只有一种载荷有用**:`update.sessionUpdate === "agent_message_chunk"`,`update.content` 为 `{type:"text",text}`。`agent_thought_chunk` 等其余一律消费但不上报。**没有 tool_call、没有 plan、没有 diff** —— `packages/acp/acp/src/index.ts:154` 的注释原文:"Raw chunks, reasoning, tools, plans, titles, and retry markers … stay off the automation wire"。
- **`session/request_permission` 只携带 `toolCall.toolCallId`**,没有工具名、没有路径、没有入参(`index.ts:215-229`)。选项固定 `allow-once` / `reject-once`。**任何按路径匹配的策略都不可实现**。
- **`session/new` 拒绝非空 `mcpServers` 与非空 `additionalDirectories`**(`index.ts:430-436`),`cwd` 必须是绝对路径。
- **`loadSession` / `session/list` / `session/resume` / `session/fork` 全部不存在**(README「Known Limitations」)。续传与会话列表**不在本期范围**,也不留占位 UI。
- **`stopReason` 取值**:`end_turn` | `max_tokens` | `refusal` | `cancelled` | `max_turn_requests`。未知取值一律按失败处理,绝不当成功。
- **`initialize` 回包的能力位只有** `promptCapabilities: {image,audio,embeddedContext}`,全 `false`;`authMethods: []`。没有别的能力位可探测。
- **prompt 内容块**只允许 `text` 与 `resource_link`;其余类型代理端直接报 `invalid params`。
- **actor 署名**一律 `deepseek-harness/<model>`,写进 `by::` 与 `generated.by`;**绝不 `human:`**(OKF §7)。
- **状态机红线**:agent 只许 `open → answered`;`closed`/`adopted` 归人。
- **答复围栏**:`type:: answer` 即署名,绝不手写 `✦`;围栏必须紧跟 `- ` 同一行开头(见 `reference_answer_fence_must_open_bullet`)。
- **OKF**:任何新写入点必须过 `src/lib/okf/concept.ts` 登记,并用 `pnpm okf:lint` 自检。
- **CHANGELOG 硬门禁**:`CHANGELOG.md` 与 `CHANGELOG.zh-CN.md` 的「未发布」区都必须有本次条目,否则 `release.sh` 停在 pre-flight。
- **共享工作区纪律**:提交只精确 `git add` 目标文件,绝不 `git add -A`。
- **不做 UI 自动化**:GUI 验收由用户手动执行;实现方只负责起 dev 构建并给出手动测试步骤。

## 与设计文档的偏离(每条都有源码依据)

| 设计文档 | 实际 | 本计划的做法 |
| --- | --- | --- |
| §3.4 映射表六行 | 只有 `Text` 与 `Result` 有数据源 | 只实现这两行;`ToolUse`/`Plan` 不实现,前端对 deepseek 运行不显示工具行 |
| §4.2 `policy.json` 路径规则 | 权限请求无路径信息 | `policy.json` 瘦身为 `permission_mode` + `on_permission_request`(见 Task 8) |
| §3.2 `dsh plugin --profile notemd add -w @deepseek-ai/dsh-acp` | `dsh-acp` 无 `dsh.bundle` 声明,不会进 layer stack(`apps/cli/src/plugin.ts` 的 `reconcilePlugins`) | 改为 `dsh-acp-demo` bin + vault 内 cordis.yml(Task 9) |
| §5 能力降级表 | 无能力位可探测 | 只做「握手断言」:协议版本必须是数字且 ≥1,否则 fail loud |
| §6 `contributes.agent_provider: true` | `Contributes` 是 `deny_unknown_fields`,加字段会让**老宿主完全加载不出插件** | 改为按 `activation.events` 约定识别(Task 14),零 manifest schema 变更 |
| P2 续传/会话列表 UI | 协议不支持 | 移出范围 |
| P2 tool diff 展示 | 无 tool 事件 | 移出范围 |

## File Structure

### 新增:共享 crate `plugins-src/agent-run-core/`

| 文件 | 职责 |
| --- | --- |
| `Cargo.toml` | lib crate `agent-run-core`,依赖 serde/serde_json/chrono/libc/tokio |
| `src/lib.rs` | `pub mod` 汇总 + 重导出 |
| `src/lock.rs` | 任务互斥锁(从 claude-agent 原样搬,零改动) |
| `src/record.rs` | `RunRecord`/`Status`/`Progress`/日志(原样搬) |
| `src/artifacts.rs` | 交付物收集(原样搬) |
| `src/okf.rs` | OKF 头补写(原样搬) |
| `src/mirror.rs` | 镜像→源文解析(原样搬) |
| `src/precheck.rs` | precheck 脚本(原样搬,去掉内嵌的 claude 模板测试) |
| `src/scope.rs` | `Scope`(从 claude-agent `settings.rs` 抽出) |
| `src/event.rs` | `Event`/`RunResult`/`Step` —— 两个插件与前端共用的 wire 契约 |
| `src/task.rs` | `TaskDef`/`discover`/`tasks_root`/`runs_root`/`ensure_gitignore`;seed 与 rename 迁移改为**接受模板表参数** |
| `src/prompt.rs` | `compose`/`with_source_context`(harness 无关的两个) |
| `src/discover.rs` | `discover_with`/`parse_probe`/`runtime_path_with` + 按二进制名缓存的 `probe` |
| `src/detach.rs` | detach 骨架:`spawn_detached`/`read_request`/`cleanup` |
| `src/scaffold.rs` | 运行脚手架:`preflight`、`ProgressTracker`、`finalize` |

### 新增:插件 `plugins-src/deepseek-agent/`

| 文件 | 职责 |
| --- | --- |
| `manifest.v2.json` | 插件清单;`onCommand:run-task/run-note/run-status` 三件套 + `onCli:dsagent` |
| `package.json` / `vite.config.ts` / `tsconfig.json` / `vitest.config.ts` / `index.html` | 前端工程(对标 claude-agent) |
| `backend/Cargo.toml` | bin `notemd-deepseek-agent` + 测试用 bin `stub-acp` |
| `backend/src/main.rs` | 两种模式:`--runner <dir>` 与 SDK serve |
| `backend/src/plugin.rs` | `NotemdPlugin` 实现:5 个 UI 方法 + 5 个命令 |
| `backend/src/discover.rs` | `dsh-acp-demo` 启动器探测(5 层) |
| `backend/src/acp.rs` | JSON-RPC 帧解析、请求关联、update→Event 映射、权限应答构造 |
| `backend/src/engine.rs` | 进程生命周期 + 握手序列 + 静默超时 + 取消 |
| `backend/src/policy.rs` | `policy.json` 读取与 materialize |
| `backend/src/composition.rs` | vault 内 `cordis.yml` 的播种与刷新 |
| `backend/src/task.rs` | 本插件的内置模板表(薄) |
| `backend/src/runner.rs` | CLI detach 的 runner 主体 |
| `backend/src/bin/stub_acp.rs` | 测试用桩 ACP 服务端(按环境变量演剧本) |
| `backend/templates/_dsh/cordis.yml` | 随插件下发的 harness 组合配置 |
| `backend/templates/selfcheck/{task.json,AGENTS.md,policy.json}` | 自检模板 |
| `backend/templates/answer-note-question/{task.json,AGENTS.md,policy.json,precheck.sh}` | 答疑模板 |
| `src/**` | Svelte 窗口(结构对标 claude-agent) |

### 修改

| 文件 | 改动 |
| --- | --- |
| `plugins-src/claude-agent/backend/Cargo.toml` | 加 `agent-run-core` 依赖 |
| `plugins-src/claude-agent/backend/src/*.rs` | 删除被抽走的模块,改为 `use agent_run_core::…` |
| `src-tauri/src/plugin_runtime/agent_provider.rs`(新增) | provider 枚举与默认 provider 解析 |
| `src-tauri/src/plugin_runtime/ui_rpc.rs:1073` | `AGENT_PLUGIN` 常量 → 设置驱动 + `harness` 参数 |
| `src-tauri/src/plugin_runtime/mod.rs` | 挂 `agent_provider` 模块 |
| `src/lib/agent-workspace/store.svelte.ts` | `PLUGIN_ID` 常量 → provider 列表 |
| `scripts/dev-install-plugin.sh` / `scripts/release-plugins.sh` | 加 `deepseek-agent` 分支 |
| `CHANGELOG.md` / `CHANGELOG.zh-CN.md` | 「未发布」区双语条目 |
| `pnpm-workspace.yaml` | 纳入新插件前端包 |

---

## 任务清单

每个任务结束时都必须能独立通过测试。

- **Task 1** 建 `agent-run-core` crate,搬入 lock/record/artifacts/okf/mirror/precheck,原样带上各自单测。
- **Task 2** 继续搬 scope/event/prompt/discover/detach,并把 `task.rs` 的 seed/rename 改成接受模板表参数。
- **Task 3** 加 `scaffold.rs`:`preflight`(锁+precheck)、`ProgressTracker`、`finalize`(工件+OKF+落盘)。
- **Task 4** claude-agent 迁移到 core,删除重复模块,`cargo test` 行为不变。
- **Task 5** deepseek-agent 骨架:crate、manifest、main.rs、plugin.rs 的 `tasks.list` 与协议死锁回归测试。
- **Task 6** `discover.rs`:五层启动器探测(显式设置 → 环境变量 → login shell → 常见路径 → 本地 monorepo)。
- **Task 7** `acp.rs`:帧解析(`Incoming` 三态)、update→Event、权限应答构造;全是纯函数,全有单测。
- **Task 8** `policy.rs`:`policy.json` → `DSH_PERMISSION_MODE` 环境变量 + 笼统应答决策;fail-closed 默认。
- **Task 9** `composition.rs` + `templates/_dsh/cordis.yml`:vault 内组合配置的播种与刷新。
- **Task 10** `bin/stub_acp.rs`:桩 ACP 服务端,按环境变量演成功/错误/静默/取消/权限拒绝五条剧本。
- **Task 11** `engine.rs`:握手 → session/new → session/prompt → 事件泵 → 终止;用桩跑通五条端到端路径。
- **Task 12** 模板与 runner:selfcheck / answer-note-question(AGENTS.md + policy.json + precheck),CLI detach。
- **Task 13** Svelte 窗口:复用 claude-agent 的 events/reduce 契约,去掉工具行与续传位。
- **Task 14** 宿主 provider 注册表:按 `activation.events` 三件套识别 provider + `agent.default_provider` 设置。
- **Task 15** `ui_rpc.rs` 的 `agent_execute` 改为设置驱动 + 可选 `harness` 参数;`agent-workspace` store 列表化。
- **Task 16** 构建脚本、CHANGELOG 双语条目、市场索引,发版。
