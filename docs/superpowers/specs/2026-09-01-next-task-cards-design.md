# Next Task 卡片设计

- 日期：2026-09-01
- 状态：已实现（Next 1.5.0）
- 影响范围：`notemd.next`、OKF 类型登记、每日总结 Agent 写入约定

## 0. 结论

Next 应把 `Task` 作为与 `Idea` 并列的源内容类型，但继续只保留一套承诺状态机：

```text
Idea ─┐
      ├─ 收件箱 → 进行中 / 等待 / 稍后 / 已完成
Task ─┘
```

- 每个 Task 是 `inbox/tasks/` 下的一份普通 Markdown；文件保存任务内容、稳定身份与来源。
- Task 文件不保存 `status`、`done` 或 `lane`。五泳道状态仍由 `thinking/next.note.md` 的追加事件唯一决定。
- 新建 Task 默认只进入收件箱，不自动占 WIP。人工创建时可显式选择「保存并标记为当前」，或以后通过「安放 → 现在推进」承担；每日总结 Agent 只能投递收件箱。
- Task 与 Idea 共用现有 `commit / wait / park / settle / reopen` 状态机、WIP 上限、项目 Tag、搜索和拖动交互，不增加第六条泳道。
- 首次为 Task 写生命周期事件时，Next ledger 安全升级到 v2；不能把 Task 伪装成 v1 的 `idea_id`。

核心区分是：

```text
Task = 这是一件可执行的事
WIP  = 人已经确认现在承担它
```

因此 Task 可以在收件箱、等待、稍后和已完成；“是任务”不等于“正在做”。

## 1. 为什么不把 Task 当成 Idea

Idea 是尚未承诺的念头，可能需要论证、搁置、转移或停止。Task 已经用动作语言表达，但仍可能需要人确认是否现在承担。把 Task 写成 `type: Idea` 会丢失来源语义，也会迫使每日总结 Agent 产出虚假的 idea。

反方向也不能让 Agent 直接写 WIP：每日总结可以发现明确义务，但不能替人占用三个当前承诺槽位。Agent 只创建 Task 源文件；人的安放事件才改变承诺状态。

## 2. 存储位置与文件命名

固定目录：

```text
inbox/tasks/
```

首版只扫描该目录的直接子文件，不递归。文件名必须严格以 `-task.md` 结尾，与现有 `-idea.md` 约定一致：

```text
YYYY-MM-DD-HHmm-<slug>-task.md
YYYY-MM-DD-HHmm-<slug>-2-task.md
```

例如：

```text
inbox/tasks/2026-09-01-1120-submit-testflight-task.md
inbox/tasks/2026-09-01-1120-submit-testflight-2-task.md
```

命名规则：

- 时间使用创建者本地时间，只用于浏览和排序提示；权威时刻是 frontmatter 的 `created`。
- `slug` 从标题生成，Unicode NFKC 归一、空白折叠为 `-`、移除路径分隔符和控制字符，最多保留 48 个 Unicode code point；为空时使用 `task`。
- 同名时只追加 `-2 / -3`，绝不覆盖。Task 身份不依赖文件名。
- 文件进入 WIP、Waiting 或完成后都不移动；路径不编码状态，避免链接、去重键和 Agent 引用失效。

## 3. Task Markdown 契约

### 3.1 最小人工创建文件

```markdown
---
type: Task
title: 提交 TestFlight 构建
created: 2026-09-01T03:20:00Z
task:
  version: 1
  id: 8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8
---

确认签名环境变量，重新运行发布脚本。
```

必填字段：

| 字段 | 约束 | 用途 |
|---|---|---|
| `type` | 固定为 `Task` | OKF 内容类型 |
| `title` | trim 后非空字符串 | 卡片标题与搜索主文本 |
| `created` | 带时区的 RFC 3339，writer 统一写 UTC `Z` | 创建时间 |
| `task.version` | 首版固定为 `1` | Task 文件协议版本 |
| `task.id` | UUID v4，创建后永不改变 | 跨重命名的稳定身份 |

可选字段：

```yaml
description: 一句话上下文
task:
  version: 1
  id: 8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8
  due: "2026-09-02"
  done_when: 构建出现在 TestFlight，且安装验证通过
  dedupe_key: daily-summary/v1:2026-09-01:testflight
```

- `task.due` 是可选计划日期，只显示中性日期，不自动改变泳道，也不引入日历或逾期奖惩。
- `task.done_when` 是安放到 WIP 时的关闭条件建议；最终承诺值仍复制进人的 `commit` 事件。
- 日期值必须加引号，避免 YAML 1.1 消费器把它变成 date 对象。
- 正文保存补充上下文、链接或清单；Next 预览正文但不解析正文标题作为状态。
- 消费者必须保留并忽略未知字段。`task.version > 1`、坏 YAML、重复 `task.id` 进入只读修复区，绝不被重写为空状态。

`status` 不能表示任务状态。OKF 已把顶层 `status` 定义为 `draft / stable / deprecated` 的文档生命周期；Task 的承诺状态属于 Next ledger。

### 3.2 每日总结 Agent 文件

Agent 创建时必须额外写生成者、可追溯来源和稳定去重键：

```markdown
---
type: Task
title: 提交 TestFlight 构建
description: 今天的发布已完成构建，但还没有上传验证。
created: 2026-09-01T03:20:00Z
task:
  version: 1
  id: 8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8
  due: "2026-09-02"
  done_when: 构建出现在 TestFlight，且安装验证通过
  dedupe_key: daily-summary/v1:2026-09-01:testflight-upload
generated:
  by: daily-summary-agent/1
  at: 2026-09-01T03:20:00Z
sources:
  - id: daily-note
    resource: /dailynote/2026/2026-09-01.note.md
    title: 2026-09-01 日记
---

上传前确认根目录 `.env` 已提供 `MATCH_PASSWORD`。
```

约束：

- `generated.by` 使用 OKF actor 格式 `<producer>/<version>`；`generated.at` 为本次内容生成时刻。
- `sources[].resource` 优先指向触发任务的 Vault 文档；存在明确来源时不得只写一段不可追溯的摘要。
- `task.dedupe_key` 对自动生产者必填，对人工创建可省略。推荐格式为 `<producer>/<schema-version>:<period>:<stable-source-key>`。
- Agent 重跑前扫描全部 Task 文件：同一 `dedupe_key` 已存在即 no-op；相同 key 但实质内容不同则报告冲突，不覆盖、不复活、不模糊合并。
- 周期任务必须把本次 occurrence 日期放入 key，使每天的实例彼此独立。

## 4. 内容与生命周期的真相源

| 数据 | 真相源 | 写入者 |
|---|---|---|
| 标题、说明、截止日期、完成条件建议、来源 | 单个 `*-task.md` | 人或创建该文件的 Agent |
| Inbox / WIP / Waiting / Dormant / Closed | `thinking/next.note.md` events | Next 中人的确认动作 |
| 当前项目 Tag | Next event | Next 中人的确认动作 |
| Agent 生成归属 | Task 的 `generated` | 生成该 Task 的 Agent |

生命周期动作不修改、移动或删除 Task 文件。用户从卡片打开源文件后可以编辑内容；Next 刷新时重读最新内容。源文件消失不会推断为完成，只会形成 orphan。

`task.id` 使 Task 重命名可以安全解析：同一个唯一 ID 出现在新路径时可恢复引用；重复 ID 必须进入修复区。Idea 仍保留现有 `path + created` 与人工 relink 兼容行为，不借本次改动批量迁移历史 Idea。

## 5. Next ledger v2

### 5.1 为什么需要 v2

现有 v1 事件把实体固定命名为 `idea_id`，旧版扫描器也只认识 `*-idea.md`。把 Task ID 塞进 `idea_id` 会让旧版把已知的 `commit` 当成“丢失的 Idea”，产生危险的假兼容。

另一种方案是把 Task 状态写回各自文件，让 v1 ledger 继续只管 Idea。它虽然减少迁移代码，却会让同一个看板拥有两套状态协议、两种并发写入和两种恢复逻辑；Task 文件的整文件覆盖也会与用户或 Agent 编辑正文冲突。因此不采用。

### 5.2 兼容格式

v2 保留历史事件原样，只为新事件使用通用实体字段：

```yaml
---
type: Next
version: 2
source_dirs:
  - inbox/ideas
task_dirs:
  - inbox/tasks
events:
  # v1 历史事件：保留原字段，v2 reader 归一为 item_kind=idea
  - at: 2026-08-29T14:10:00Z
    event_id: e-old
    idea_id: i-old
    action: commit
    source:
      path: inbox/ideas/2026-08-29-1341-idea.md
      created: 2026-08-29T05:41:00Z
    commitment: 验证 Next
    next_action: 回看最近的 Idea
    close_condition: 得出继续或停止的证据

  # v2 新事件
  - at: 2026-09-01T03:30:00Z
    event_id: e-new
    item_id: 8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8
    item_kind: task
    action: commit
    source:
      path: inbox/tasks/2026-09-01-1120-submit-testflight-task.md
      created: 2026-09-01T03:20:00Z
    commitment: 提交 TestFlight 构建
    next_action: 确认环境变量并运行发布脚本
    close_condition: 构建出现在 TestFlight，且安装验证通过
---
```

迁移规则：

- 新版 reader 同时读取 v1、v2。
- v1 的 `idea_id` 只在内存中归一为 `item_id`，`item_kind=idea`；原事件映射写回时保留，不批量改写历史。
- 只有第一次为 Task 追加生命周期事件时，才把顶层 `version` 升为 2 并加入 `task_dirs`。仅扫描或创建 Inbox Task 不触发迁移。
- v2 中的新 Idea 与 Task 事件都使用 `item_id + item_kind`。
- 旧版 reader 遇到 `version: 2` 会按现有规则明确只读，优于把 Task 误显示为 orphan 或覆盖未知状态。
- v2 继续保留未知顶层字段、未知事件字段和未知 action；写前仍重读 ledger、重新归约并验证，写后逐字节回读确认。

事件 action、状态转换、WIP 计数和关闭出口不变。WIP 上限统计 Idea 与 Task 的总和，任何筛选都不能改变真实容量。

## 6. 写入协议

### 6.1 Next 中人工创建

1. 校验标题，生成 UUID、`created`、slug 和最终文件名。
2. 在 `inbox/tasks/` 写一个不匹配 `-task.md` 的同目录临时文件。
3. 回读并完整解析 Task YAML，确认内容一致。
4. 调用现有 `host.vault.rename` 把临时文件 no-clobber 发布为最终名；目标已存在则重新计算 `-2 / -3` 后重试。
5. 失败时尽力删除临时文件并保留 sheet 草稿；成功后刷新，Task 出现在收件箱。

现有 Host rename 对目标使用 `O_EXCL`，因此比 `exists → vault.write` 更适合“只新增、绝不覆盖”的 Task。Task 生命周期只写 ledger，不需要覆盖 Task 文件，也不需要为本期新增 CAS Host API。

### 6.2 每日总结 Agent

每日总结 Agent 是 create-only writer：

- 只可新增合法的 `inbox/tasks/*-task.md`。
- 写入前按 exact `dedupe_key` 去重。
- 先写非匹配临时文件，校验后再无覆盖地发布最终文件。
- 不修改、移动或删除任何已有 Task。
- 不修改 `thinking/next.note.md`，不产生 `commit / settle`，不替用户选项目或关闭结果。
- 不能把“建议考虑”升级成 Task；只把总结中存在明确责任、明确下一动作或明确截止要求的事项写入。

## 7. UI

顶栏使用两个直接按钮，不做下拉菜单：

- 主按钮：`＋ 新建任务`
- 次按钮：`新建 Idea`
- 保留已有 `⌘N / Ctrl+N` 新建 Idea，避免改变既有肌肉记忆；Task 增加 `⌘⇧N / Ctrl+Shift+N`。

Task sheet 首版提供：

- 任务标题：必填。
- 补充说明：可选。
- 完成条件：可选。
- 明确显示保存位置 `inbox/tasks/`。
- 默认动作「加入收件箱」；`⌘Enter / Ctrl+Enter` 触发该动作，Escape 取消。
- 显式动作「保存并标记为当前」；选择它时完成条件变为必填，并在写入前检查真实 WIP 容量。
- 任一保存失败都保留草稿。

用户点「安放」、拖到进行中，或在创建页选择「保存并标记为当前」时：

- `commitment` 默认取 Task 标题；
- `next_action` 默认取 Task 标题，用户可改；
- `close_condition` 默认取 `task.done_when`，缺失时仍要求用户填写；
- 用户确认后才追加事件、占用 WIP。

「保存并标记为当前」按 source-first 顺序执行：先 no-clobber 创建并验证 Task 文件，再基于最新 ledger 追加 `commit`。两份文件无法组成单个事务；若 ledger 校验或写入失败，不回滚已经安全落盘的 Task，而是明确提示“任务已在收件箱，尚未标记为当前”。这样失败只会降级为待安放，不会丢任务或绕过 WIP 约束。

卡片增加 `任务` 类型徽标；Agent 生成的 Task 再显示低强调的生成来源。Inbox Task 可以显示 `due`，但不使用红色逾期奖励/惩罚。进入其他泳道后，详情仍优先显示现有 `next_action / waiting_for / wake_trigger / result`。

首版不增加 Task 专属泳道、优先级、估时、子任务、周期调度、类型筛选或批量完成。最新 Task 与 Idea 统一按创建/最近事件时间排序，仍遵守 Inbox 最近 10 条的默认上限；搜索可以找到其余项目。

## 8. OKF 与搜索接线

- `src/lib/okf/concept.ts` 登记 `task: 'Task'`。
- `searchidx/src/origin.rs` 将未署名 `Task` 映射为 Human；带非 `human:` `generated.by` 的 Agent Task 会被更高优先级规则归为 Derived。
- 运行 `pnpm gen:origin-types` 更新跨语言 fixture，并补对应类型与 provenance 测试。
- Vault 的 Agent 约定和未来每日总结任务模板引用本文件的 schema；不要在不同 Agent prompt 中复制出漂移版本。

## 9. 验收标准

- 人与 Agent 创建的每个 Task 都是一份独立、合法、可搜索的 `inbox/tasks/*-task.md`，绝不覆盖已有文件。
- 合法 Task 刷新后进入收件箱；错误后缀、`type` 不符、坏 YAML、重复 ID 不进入正常泳道。
- Agent Task 有 `generated`、`sources` 和稳定 `dedupe_key`；同一次每日总结重跑不产生重复文件。
- 默认新建 Task 不写 lifecycle event、不占 WIP；只有人的安放，或人工显式「保存并标记为当前」，可以进入 WIP。
- Task 可打开、预览、搜索、拖动，并复用五泳道全部生命周期动作。
- Idea 与 Task 共用一个真实 WIP 计数和上限。
- Task 生命周期动作只追加 ledger，不改变 Task 源文件字节；删除源文件不会自动完成。
- v1 ledger 在没有 Task 生命周期事件时保持 v1；首次安放 Task 后安全升级 v2，历史 v1 events 原样保留。
- 新版可重建 v1/v2 混合事件投影；旧版面对 v2 明确只读，不产生假 orphan 或覆盖。
- OKF 类型、搜索来源分层、四语言文案、Svelte check、Next 全量测试和生产构建全部通过。

## 10. 实现顺序

1. Task schema、命名、解析、去重与原子 create，先用纯函数和内存 Vault 测试固定文件契约。
2. ledger v2 双读、v1 事件归一和“首次 Task 事件才升级”迁移测试。
3. 将 `IdeaProjection / WorkspaceItem` 泛化为 `ItemProjection / WorkspaceItem(kind)`，复用领域状态机。
4. 新建 Task sheet、卡片类型展示、预填与快捷键。
5. OKF/search 接线与每日总结 Agent 写入约定。
6. 全量自动验证后实机检查深浅色、窄窗口、键盘、pointer 拖动与源文件字节不变。
