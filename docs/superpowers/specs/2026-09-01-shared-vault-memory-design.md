# Shared Vault Memory：USER.md 与 MEMORY.md 设计

日期：2026-09-01

## 1. 目标

参考 OpenClaw 的 workspace memory 分层，为每个 Vault 提供人和 AI 可以共同维护、可读、
可检索、可同步的用户模型与长期记忆，同时不让记忆系统替人制造任务、权限或承诺。
“共同维护”指双方通过受控提案与人工决定维护，不表示任何一方直接编辑投影文件。

## 2. 文件分层

| 层 | 真相源 | 内容 | 明确排除 |
|---|---|---|---|
| 协作规则 | `/AGENTS.md` | Agent 行为、格式、读写和隐私规则 | 个人事实、任务状态 |
| 用户模型 | `/USER.md` | owner 身份、名字和稳定画像 | 每日事件、提醒、秘密 |
| 长期记忆 | `/MEMORY.md` | 跨会话有效的事实、约束和决定 | 原始聊天、流水、短期任务 |
| 情节记忆 | Vault 既有日记体系 | 每日和阶段性记录 | 另建第二套 daily memory |
| 前瞻行动 | `/inbox/tasks/*-task.md` | owner 已成立的明确义务 | 建议、他人任务、模糊责任 |

`USER.md` 是 owner 的唯一机器可读来源；`MEMORY.md` 用 `owner_ref: /USER.md` 指向它，
不复制身份真相。Task owner gate 必须先读 `USER.md`。

## 3. owner 激活条件

只有同时满足以下条件，Agent 才能把某人视为 Vault owner：

1. `owner.actor` 是非空 `human:<id>`；
2. `owner.names` 至少包含一个能在来源中识别该人的名字；
3. `owner.confirmed: true`；
4. 字段之间不存在冲突。

新模板使用 `actor: null`、`names: []`、`confirmed: false`。缺失、默认、冲突或未确认时，
Agent 可以继续读写普通知识，但不得创建 owner Task。

## 4. 受控共同维护协议

- `/USER.md` 与 `/MEMORY.md` 是生成的只读投影；人和 Agent 都不得直接编辑。文件外部改动
  只能被识别为 drift，不能被当作批准。
- 每次新增、修正、合并、优先级变化或废止都先生成一个不可变 Markdown candidate，放在
  `/inbox/memory-candidates/`。一份 candidate 只处理一个条目，并保存 before/after、来源、
  提案者和 SHA-256。
- owner 在 Memory 插件中查看精确差异后，逐条批准或拒绝。决定写成不可变 Markdown event，
  放在 `/memory/events/YYYY-MM/`；只有批准事件才能更新根投影。决定请求必须回传界面所见
  candidate 的 SHA-256，内容在展示后发生变化时拒绝批准。
- Agent 可以列出、搜索、提出和优化 candidate，但不得把自己的署名伪装成 `human:`，不得
  推断或批量代签批准。对话中代录决定时，owner 必须明确指明 candidate 或完整差异。
- 每个投影条目带稳定 `id`、递增 `revision`、`status`、`priority`、`proposal`、
  `approved-by`、`approved-at` 与 `source`。`priority: high` 仅影响展示和检索权重，不授予
  权限或批准。
- 删除采用可恢复的 `revoke` 事件，不物理删除历史。冲突、替换和合并必须留下明确事件，
  且不得保留两个当前真相。
- owner 身份、名字、权限、授权、承诺和其他行动敏感信息始终逐条明确确认。
- 两份文件保持紧凑；重复项通过候选合并，失去复用价值的内容通过候选废止。
- 两份文件会被 Vault 同步，禁止写入凭据、令牌、私钥或其他秘密；外部或共享上下文默认
  不披露内容。

## 5. 初始化与兼容

保持现有显式 bootstrap 语义：只有用户点击“编辑 AGENTS.md”时，应用才尝试补齐
`AGENTS.md`、`USER.md`、`MEMORY.md`。应用启动和切换 Vault 不静默创建文件。

每份文件独立使用 `OpenOptions.create_new(true)` 原子创建。已有普通文件、空文件、有效
软链或悬空软链一律不覆盖、不跟随；一个文件冲突或创建失败，不阻断另一个缺失文件。
编辑入口仍只打开 `AGENTS.md`。后两份文件可以由普通 Vault 文件视图查看和同步，但只能
通过 Memory 插件或 `notemd memory` 受控工作流修改。

## 6. OKF 与搜索

新增 `User Profile` 与 `Memory` 两个 OKF type，并在搜索 origin 类型表登记为 Human。
如果文件带非 `human:` 的 `generated.by`，现有更高优先级的 provenance 规则仍会把该文档
判为 Derived；类型登记不覆盖真实生产者署名。

## 7. 验收

- 新 Vault 在显式 bootstrap 后得到三份完整模板，第二次执行字节不变。
- 已有内容、空文件和悬空软链都不被覆盖或跟随。
- 默认未确认 owner 时，AGENTS 明确禁止 Agent 创建 Task。
- sotvault 的 `USER.md` 明确 `human:bruce`，Task 协议不再硬编码另一套 owner 真相源。
- `USER.md`、`MEMORY.md` 直接改动会触发 drift；candidate 未获批准时投影不变化。
- 每次批准绑定 candidate SHA-256，并留下不可变事件；废止为 `revoke` 而非删除历史。
- `MEMORY.md` 不包含 Task、提醒、每日流水或秘密，并规定来源、人审和废止格式。
- Rust 文件系统/模板契约测试、OKF 跨语言类型同步、格式和差异检查通过。

## 8. 参考

- OpenClaw Memory：<https://github.com/openclaw/docs/blob/main/docs/concepts/memory.md>
- OpenClaw Agent Workspace：<https://docs.openclaw.ai/concepts/agent-workspace>
- OpenClaw Memory Architecture：<https://github.com/openclaw/openclaw/blob/main/docs/concepts/memory-architecture.md>
