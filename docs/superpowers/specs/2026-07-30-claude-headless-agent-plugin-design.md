# Claude Headless Agent 插件设计

日期:2026-07-30
状态:已确认,待实现
来源技术方案:`docs/2026-07-30-claude-headless-automation-implementation-plan.md`(用户提供,以其为技术准绳)

## 一句话

一个通用的 headless 运行器插件:在 vault 内的任务模板目录里跑 `claude -p`,窗口实时看流、随时中断,CLI 可无头触发,让"一个 vault 多个 agent"里的 Claude Code 成为可编排的一等公民。

## 目标与非目标

**目标**

1. 通用 headless 运行器:插件只管"起 claude、喂 prompt、收流、写运行记录",具体用途以**任务模板**的形式挂上去。
2. 两个触发面:窗口手动触发(实时流 + 可中断)、CLI 子命令触发(可 detach,供 cron/脚本)。
3. 首发自带两个模板:`selfcheck`(环境自检)与 `annotation-sweep`(批注问答闭环的 sweep 执行体)。

**非目标(本期不做)**

- 定时轮询与 vault 文件监听触发(调度交给外部 cron + CLI)
- 限流后的自动退避重试
- 把运行能力暴露给其它插件(宿主没有插件间调用通道,`host_api.rs:32-51`)
- 全量事件流落盘与历史回放
- 内置 API key 计费路径(按来源方案:只走订阅额度的 `claude -p`,不用 Agent SDK)

## 产品定位

对应信念 5「一个 vault,多个 agent,你是编排者」:note.md 不跑模型,只**编排**。本插件是编排面的第一块基础设施 —— 把 Claude Code 这个 harness 接到 vault 上,任务定义是纯文本、住在 vault 里、可被人和 agent 一起改。

对应信念 2「文件高于应用」:任务模板是 `.md` + `.json`,脱离 note.md 用 `cd <task> && claude -p` 手工跑完全等价。插件只是那条命令的 GUI 与调度壳。

## 1. 形态与进程模型

插件 `notemd.claude-agent`,**后端 + 前端**形态:

- 后端:Rust crate `plugins-src/claude-agent/backend` → `bin/notemd-claude-agent`,依赖 `notemd-plugin-sdk`(工程骨架照 `plugins-src/pos-log/backend/`)
- 前端:Svelte 独立窗口 `ui/`(流式 UI 照 `plugins-src/openclaw/`)
- `capabilities: ["ui", "toast", "vault.read"]` —— spawn `claude`、读写任务目录都是插件进程自己的文件系统能力,不需要宿主授权;`vault.read` 只为 `host.vault.info` 拿 vault 根路径
- `activation.events: ["onCommand:open", "onCommand:run", "onCli:agent"]`
- `request_timeout_seconds: 300`(协议上限)

窗口 ↔ 后端双向通道沿用既有机制(`plugins-src/openclaw/src/lib/bridge.ts:1-11`):

- 窗口 → 后端:`window.notemd.request('plugin.<method>')` → 宿主转为 `ui.request` → SDK 的 `on_ui_request`
- 后端 → 窗口:`Host::ui_post(window_id, payload)` → 窗口 `onMessage`

### 1.1 链路 A:窗口(实时流)

```
菜单「Claude Agent…」→ 宿主激活插件进程 + 开窗(命令携带 TabSnapshot)
  → 窗口 plugin.tasks.list          → 后端扫 .notemd/agent-tasks/*/task.json
  → 窗口 plugin.context.get         → 后端返回开窗时记住的 tab 上下文
  → 窗口 plugin.run.start {task, prompt, use_context}
      → 后端取任务锁 → spawn claude 子进程 → 逐行解析 stream-json
      → Host::ui_post {kind:'event'|'result'|'error', ...} → 窗口渲染
      → 结束:写 runs/<runId>.json、释放锁、ui_post 终态
  → 窗口 plugin.run.cancel {runId}  → kill 进程组 → 终态 cancelled
```

`plugin.run.start` **立即返回** `{run_id}`(SDK 的 `on_ui_request` 是同步的,长任务必须在 tokio task 里跑,不能阻塞协议循环)。

### 1.2 链路 B:CLI(默认 detach)

`notemd agent <task> [-p "…"] [--wait]`

CLI 子命令走宿主的一次性 invoke,并且宿主会**另起一个无头 app 实例**执行(`src-tauri/src/cli/runner.rs:82`)。由此两条硬约束:

1. 单次 invoke 受 `request_timeout_seconds` 上限 **300 秒**约束;
2. 无头实例执行完即退出,会连带收走插件进程及其子进程。

sweep 这类任务超 5 分钟很正常,所以 **CLI 默认 detach**:

- 后端以 `setsid` 拉起**自身二进制的 runner 模式**:`notemd-claude-agent --runner <runDir>`;
- runner 读 `runDir/request.json`,spawn claude、解析流(不落全量流)、写 `runs/<runId>.json`、管理锁;
- 插件立即返回 `{run_id, status: "started"}`,无头实例干净退出,任务继续跑。
- `--wait` 则不 detach:同步等待并把结果 message 返回给 CLI;超 300s 由宿主判超时,提示改用窗口或去掉 `--wait`。

两条链路共用同一份 **argv 组装 / stream-json 解析 / 运行记录写入**代码;差异只在**谁持有子进程**(插件进程 vs detached runner)。

## 2. vault 目录约定

```
<vault>/.notemd/
├── agent-tasks/                     # 任务模板 —— git 跟踪,人可直接编辑
│   ├── selfcheck/
│   │   ├── task.json
│   │   ├── CLAUDE.md
│   │   └── .claude/settings.json
│   └── annotation-sweep/
│       ├── task.json
│       ├── CLAUDE.md
│       └── .claude/settings.json
└── agent-runs/                      # 派生数据 —— 写进 vault .gitignore
    └── <task>/
        ├── lock                     # {pid, run_id, started_at}
        └── runs/<runId>.json
```

`task.json`:

```jsonc
{
  "name": "Annotation sweep",          // 显示名
  "description": "扫描 open 问题并作答",
  "prompt": "……模板固定 prompt……",     // 拼接第 1 段
  "max_turns": 50,
  "timeout_seconds": 1800,
  "model": null                         // 可选,透传 --model
}
```

`runs/<runId>.json`:

```jsonc
{
  "run_id": "20260730T104233Z-a1b2c3",
  "task": "annotation-sweep",
  "trigger": "window" | "cli",
  "started_at": "…", "ended_at": "…",
  "status": "success" | "error" | "timeout" | "cancelled",
  "exit_code": 0,
  "num_turns": 12,
  "session_id": "…",
  "result": "……最终答复文本(截断至 8KB)……",
  "stderr_tail": "……失败时最后 2KB……"
}
```

首次启动时,模板目录不存在则写入内置模板;**已存在则不覆盖**(用户改过的模板归用户)。同时确保 vault `.gitignore` 含 `.notemd/agent-runs/`(照 `src-tauri/src/agents_sync/mod.rs:90-107` 的幂等 append 写法)。

### 2.1 cwd 定在任务模板目录(关键)

用户要"原地模式"(agent 直接改 vault 里的笔记),但 cwd **不能**定在 vault 根 —— 那样任务模板里的 `CLAUDE.md` 和 `.claude/skills/` 不会被发现。

cwd 定在 `<vault>/.notemd/agent-tasks/<task>/`:

- 它本身就在 vault 内,原地语义成立;
- Claude Code 向上递归查找 `CLAUDE.md`,于是 **vault 根的 CLAUDE.md**(`agents_sync` 维护的 vault 约定)与**任务 CLAUDE.md** 两份都加载;
- `.claude/skills/`、`.mcp.json` 相对 cwd 自动发现,符合来源方案 §4;
- **不传 `--bare`**(来源方案 §4 明确:会跳过这些自动发现)。

vault 里的笔记用绝对路径读写,由权限白名单授权。

### 2.2 `${VAULT}` 占位与 settings.local.json

模板的 `.claude/settings.json` 是**可移植**的,里面写占位符:

```json
{
  "permissions": {
    "allow": ["Read(${VAULT}/**)", "Write(${VAULT}/**)", "Edit(${VAULT}/**)"]
  }
}
```

每次运行前,后端把 `${VAULT}` 替换成真实 vault 绝对路径,生成同目录的 `.claude/settings.local.json`(Claude Code 原生的本地覆盖层)。模板保持不含机器路径,换机器无需改动;生成物写进 `.gitignore`。

## 3. Prompt 组装

最终 `-p` 参数 = 三段固定顺序拼接,段间空行分隔:

1. `task.json` 的 `prompt`
2. 窗口/CLI 传入的 prompt(可为空)
3. 上下文块(勾选时):
   ```
   ## 当前文档
   路径:<abs path>
   选中内容:
   <selection>
   ```

顺序固定并写进模板作者文档,便于模板预期。

上下文来自**开窗那一刻的 `TabSnapshot`** —— 菜单触发 `open` 时宿主把 v1 形状的 context 传进 `ExecuteCommandParams.context`(`plugin-protocol/src/lib.rs:143-146`),后端记住,窗口通过 `plugin.context.get` 取。不需要新的 host API。

## 4. 窗口 UI

- **左**:任务列表(名称 + 描述 + 运行中指示)
- **右上**:prompt 输入框 + 上下文条「上下文:`xxx.md`(选中 128 字)☑」,可取消勾选
- **中**:流式事件区。`--output-format stream-json --verbose` 的事件按类型渲染:工具调用折叠成一行(`Read src/foo.ts`)、助手文本流式追加、错误红显
- **底**:状态栏(运行中 / 已用时 / turns)+ 「停止」按钮
- **历史**:当前任务最近 20 条 `runs/*.json`,点开看结果与失败原因

i18n 走插件自带字符串表(`docs/plugin-v2-development.md` §8),zh/en/ja/de;窗口即时跟随语言按既有基座约定(`settings://changed` + reload)。

## 5. 失败面

| 情形 | 判据 | 处理 |
|---|---|---|
| 找不到 `claude` | 探测失败 | GUI 应用 PATH 精简,是头号坑。探测顺序:插件设置里的显式路径 → `/bin/zsh -lic 'command -v claude'` → `~/.claude/local/claude`、`~/.local/bin/claude`、`/opt/homebrew/bin/claude`、`/usr/local/bin/claude`。都没有 → toast + 窗口内给安装指引 |
| 未认证 | claude 报 auth 错 | 提示去终端跑一次 `claude` 登录;或在插件设置里填 `CLAUDE_CODE_OAUTH_TOKEN`,仅注入子进程环境 |
| 同任务已在跑 | `lock` 存在且 pid 活着 | 拒绝并告知已运行时长;pid 已死 → 判为陈旧锁,清理后继续 |
| 撞限流 | result 文本含 rate limit | 记进运行记录并提示,不自动重试 |
| 超时 | 超 `timeout_seconds`(默认 1800) | kill 整个进程组,状态 `timeout` |
| 权限不足卡住 | claude 结果里工具被拒 | 原样呈现,指向该任务的 `settings.json` |
| 非零退出 / `is_error: true` | — | 一律失败;stderr 尾部 2KB 存进运行记录 |
| 插件进程被关 | `$deactivate` / idle shutdown | 先 kill 在跑的子进程再退,不留孤儿;detached runner 不受影响(它本就该活下去) |

并发策略:**同任务互斥(锁文件),跨任务并行**。锁文件含 pid,陈旧锁自动回收。

## 6. 内置模板

**`selfcheck`** —— 环境自检。prompt 让 claude 报告:能读到哪几份 CLAUDE.md、可用 skills、vault 根路径、当前权限白名单,并往 `output/` 写一个 `selfcheck.md`。它同时就是来源方案 §7 验收清单第 1-3 条的执行体。

**`annotation-sweep`** —— 批注问答闭环的 sweep 执行体,严格遵循 `docs/superpowers/specs/2026-07-27-annotation-qa-loop-design.md` §3 的协议:

> 扫 vault 中 `type:: question` 且 `status:: open` 的 `.note.md` 节点 → 结合 `line::` 定位源文上下文作答 → 短答案以 `✦` 前缀子节点回填(附 `answered::`/`by::`),长答案写 `answers/` 并在节点下留摘要 + 链接 → 置 `status:: answered`;**绝不置 closed,绝不改源 `.md`,绝不动 ● 内容**。

其 `settings.json` 白名单相应收紧:允许写 `${VAULT}/**/*.note.md` 与 `${VAULT}/answers/**`,不给源 `.md` 写权限 —— 让协议约束落到权限层,而非只靠 prompt 自觉。

## 7. 测试

**Rust 单测**(不碰真 claude):

- argv 组装:三段 prompt 拼接、`--output-format stream-json --verbose`、`--max-turns`、`--model`、cwd、无 `--bare`
- stream-json 解析:半行 / 超长行 / 非 JSON 噪声行容错,`type: result` 字段抽取
- 锁:获取、冲突拒绝、陈旧 pid 回收
- `${VAULT}` 替换生成 `settings.local.json`
- claude 可执行探测的优先级顺序
- 运行记录写入与截断(result 8KB / stderr 2KB)
- 模板首写不覆盖已存在文件;`.gitignore` 幂等 append

**端到端**:用假 `claude` 桩脚本(按参数吐预设 stream-json)驱动四条路径 —— 成功 / `is_error` / 超时 / 取消。

**前端 vitest**:事件流 → 视图模型归并(工具调用折叠、文本追加、终态切换)。

**人工验收**(来源方案 §7):`claude --version` 与最小 headless 调用跑通;模板内 MCP 任务无交互批准;skill 被触发;断网 / 超时 / 权限拒绝三条失败路径均被捕获;两个任务并行互不干扰。

## 8. 使用边界

按来源方案 §3:`claude -p` 走订阅额度是官方允许的用法,**仅供账号本人的自动化**;插件不得被包装成多用户服务。这条写进插件的 README 与市场描述。
