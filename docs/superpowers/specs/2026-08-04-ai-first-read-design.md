# AI 先读:导入完成的电子书一键交给 agent 生成摘要(设计)

日期:2026-08-04
状态:已确认

## 目标

在 ebook-import 插件的导入队列中,状态为 done 的书籍行上增加"AI 先读"按钮。点击后由
claude-agent 插件异步阅读该书的 `book.md`,生成全书大纲摘要 md 写到该书的导入目录下;
行内显示进度(状态 + 耗时);完成后通过**托盘全局提醒**通知用户,点击提醒直接打开摘要 md。

托盘提醒做成通用子系统(任何插件可用),不只服务本功能。

## 非目标

- 不做持久化"书库":按钮只出现在本次会话队列中 done 的行上。
- 不做 OS 系统通知(不引入 tauri-plugin-notification),提醒只走托盘。
- 不转发 agent 的 turn 级事件流到 ebook-import 窗口,进度只到"状态 + 耗时"粒度。
- 不做通用 `host.plugin.execute`(权限面过大)。

## 架构总览

```
ebook-import UI ── plugin.ai_read_start ──▶ ebook-import 后端(FIFO 队列 + 轮询线程)
                                             │  host.agent.run / host.agent.status(新宿主 API)
                                             ▼
主程序 host_api ── command.execute 转发 ──▶ notemd.claude-agent(run / run-status)
                                             │  detached claude -p,任务模板 ai-read-ebook
                                             ▼
                              <vault>/<ebooks_root>/<YYYY-MM>/<书名>/YYYY-MM-DD-summary.md
完成 ──▶ ebook-import 后端 host.notify(新宿主 API)──▶ 托盘提醒注册表 + "提醒 (n)" 子菜单
点击提醒 ──▶ 聚焦主窗口 + editor://open-path 打开摘要 md,消掉该条提醒
```

## 组件设计

### 1. 宿主 API `host.agent.*`(新 capability `agent`)

- `src-tauri/src/plugin_runtime/host_api.rs` 的 `method_capability` 表新增
  `host.agent.run` / `host.agent.status` → capability `agent`;`ui_rpc.rs` 同步接线。
- 实现:主程序转发到 `notemd.claude-agent` 的 `command.execute`(复用
  `lifecycle.rs` 既有派发),`run` 对应参数 `{task, prompt, note_path}`,
  `status` 对应 `run-status` 的 `{task, run_id}`,结果原样透传。
- claude-agent 未安装/未启用时返回明确错误码(如 `agent_unavailable`),
  调用方据此提示用户安装。
- 调用方必须在 manifest `capabilities` 声明 `agent`,否则按既有 capability
  门控拒绝。

### 2. claude-agent 新内置任务 `ai-read-ebook`(展示名"AI 阅读电子书")

- 新目录 `plugins-src/claude-agent/backend/templates/ai-read-ebook/`:
  - `task.json`:name「AI 阅读电子书」,prompt 为用户原文:

    > 根据 book.md,生成全书的大纲摘要,让我一眼能看到讲的是什么,按照我可能
    > 关注的方式,推荐优先阅读,突出核心观点和洞察,突出反常识的信息,生成
    > 简要的 md,以方便我后续追问阅读。

    外加输出约定:摘要写到与 book.md 同目录的 `YYYY-MM-DD-summary.md`;文件
    开头带 OKF frontmatter,`type: Book Summary`,`generated` actor 用
    `claude-agent/<version>` 署名(✦ AI 写的);语言跟随书的语言;同日重读
    直接覆盖。`max_turns`、`timeout_seconds` 参照 `answer-note-question` 量级。
  - `CLAUDE.md`、`.claude/settings.json`(允许读该书目录、写摘要文件;细化在
    实施计划里对齐 engine 的 scoped policy 机制)。
- `task.rs` 的 `BUILTIN` 常量登记新模板,种子文件计数断言与 `discover`
  id 列表断言同步更新。
- 每次运行由调用方(ebook-import)像 `run-note` 那样附加定位段落:
  「本次只读 `<书目录相对路径>/book.md`,摘要写到 `<书目录相对路径>/<日期>-summary.md`,
  不要读 vault 其它文件」。日期由 ebook-import 后端按本地时间计算并传入,
  完成校验也用同一文件名。

### 3. ebook-import:按钮、FIFO、轮询

- `App.svelte` 队列行 `status === 'done'` 分支新增"AI 先读"按钮(与"打开
  编辑器"并列)。点击 → 后端新 RPC `plugin.ai_read_start { job_id }`。
- 后端(`backend/src/plugin.rs`)维护 AI 阅读 FIFO:claude-agent 的锁是
  per-task 的,同一时刻只跑一本;其余排队。每项状态:
  `queued → running → done | failed`。
- 后端轮询线程每 2s 调 `host.agent.status`,经 `host.ui.post` 推
  `{type:'ai_read', job_id, state, started_at, summary_rel?}` 给窗口;窗口关
  闭不影响后端收尾与提醒。
- 行内 UI:排队中 → 「AI 阅读中… 3m12s」(前端用 started_at 自算耗时)→
  完成后按钮变"查看摘要"(`host.editor.open` 摘要文件);失败显示错误、可重试。
- run 记录 state=done 后再校验摘要文件确实存在,存在才算成功;record 成功但
  文件缺失按失败处理(agent 没照约定写)。
- 已知局限:app 整体退出时 detached 的 claude 进程仍会写出摘要,但轮询与
  提醒丢失;重开后不恢复,属可接受损失。

### 4. 托盘全局提醒(通用子系统)

- `src-tauri/src/lib.rs`:新增提醒注册表 `Mutex<Vec<Reminder>>`,
  `Reminder { id, title, action }`,action 两种:
  - `OpenPath(路径)`:聚焦主窗口 + 走既有 `editor://open-path` 打开文件;
  - `OpenPluginWindow(plugin_id, window)`:复用 `tray-plugin:*` 打开插件窗口。
- 新宿主 API `host.notify`(新 capability `notify`):插件推一条提醒,
  参数 `{title, action}`。
- 托盘变化:有提醒时菜单出现「🔔 n 条提醒」子菜单(每条一项 + "清除全部提醒"),
  并在图标旁挂数字角标(`tray.set_title(n)`);原有图标四态(错误/大文件警告/同步中/
  空闲)逻辑不变——角标比新图标资源多传达"几条",且无需新增美术资产。
- 点击某条提醒:执行 action、从注册表移除、重建菜单并按剩余状态刷新图标。
- 本功能的用法:成功 →「《书名》AI 摘要已生成」action=OpenPath(摘要);
  失败 →「《书名》AI 阅读失败」action=OpenPluginWindow(claude-agent 主窗口,
  那里有完整运行日志)。

### 5. i18n 与 OKF 规范

- ebook-import 新增字符串(按钮、状态、提醒文案)进插件 i18n 目录
  (zh/en/ja/de),`strings.test.ts` 补齐断言;托盘菜单字符串走既有菜单
  locale 机制。
- `src/lib/okf/concept.ts` 的 `CONCEPT_TYPE` 登记 `bookSummary: 'Book Summary'`;
  任务模板 prompt 中明确 frontmatter 要求,产出可过 `pnpm okf:lint`。

### 6. 错误处理汇总

| 场景 | 行为 |
| --- | --- |
| claude-agent 未安装/未启用 | `host.agent.run` 报 `agent_unavailable`,窗口 toast 提示安装 |
| claude 可执行文件找不到 | claude-agent 既有错误透传,行内失败 + 失败提醒 |
| 任务被占(锁) | 进 FIFO 排队,不报错 |
| run 完成但摘要文件缺失 | 按失败处理 |
| 超时/agent 失败 | record 状态透传,行内失败 + 失败提醒 |
| 窗口关闭 | 导入窗口一关,宿主 `WindowEvent::Destroyed` 就 `deactivate()` 掉 ebook-import 进程,行内进度不再更新;但**提醒由 claude-agent 发**(`run-task` 的 `notify` 规格),与 ebook-import 窗口生死无关。点提醒直接开 md,不依赖窗口 |
| claude-agent 窗口被开过又关掉 | 该进程同样被 Destroyed 拆掉,在跑的 run 被中断、提醒不发(已知残留边界,接受) |
| app 退出 | detached run 继续写摘要,提醒丢失(接受) |

### 7. 测试与发布

- Rust 单测:BUILTIN 模板计数/ID 断言、提醒注册表增删与图标态、FIFO 串行、
  `host.agent.*`/`host.notify` capability 门控。
- 前端:queue 相关纯函数与 strings 测试。
- GUI(按钮、托盘子菜单、提醒点击链路)由用户 dev 实机验证:起 dev 构建 +
  手动测试步骤,不做 UI 自动化。
- 发布:主程序 bump(新宿主 API + 托盘提醒),ebook-import 插件 bump
  (manifest 声明 `agent`/`notify`,设 min host version),claude-agent 插件
  bump(新任务模板);验证通过后按惯例 commit/release + 市场发布。
