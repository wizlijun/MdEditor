# Memory 插件：受控 USER/MEMORY 设计

日期：2026-09-01
状态：实施基线

## 1. 目标

`/USER.md` 与 `/MEMORY.md` 继续是人和 Agent 都能直接阅读、搜索和引用的 Markdown，
但不再是可任意编辑的普通文件。所有变化必须经过 Memory 插件的受控流程：

1. 人或 Agent 提出一条原子候选；
2. 系统展示相对当前条目的精确增量；
3. 人批准或拒绝；
4. 插件依据不可变候选和批准事件重建只读投影；
5. 外部直接修改只会触发 drift，不会被静默当作批准。

Agent 可以列出、审计、提出新增、替换、合并、撤销和优先级建议，但 Agent 生成的变化
在被当前 owner 明确批准前不得成为 active。Task、提醒、每日流水和秘密不进入此系统。

## 2. 三层模型

### 2.1 候选

路径：`/inbox/memory-candidates/YYYY-MM-DD-HHmm-<proposal-id>.memory-candidate.md`

候选一项一文件、创建后不可修改，由核心服务用同目录临时文件和 no-clobber rename 发布。
核心字段：

```yaml
---
type: Memory Proposal
title: 将 Hemory 定位更新为第二记忆
created: 2026-09-01T06:00:00Z
proposal:
  version: 1
  id: 8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8
  scope: memory
  operation: replace
  target_id: 26399214-4521-4f6f-b8a3-c73bdb78a418
  base_revision: 1
  section: Hemory 与 note.md 的长期产品边界
  suggested_priority: high
  dedupe_key: memory/v1/second-memory-positioning
generated:
  by: codex/gpt-5
  at: 2026-09-01T06:00:00Z
sources:
  - id: weekly-candidate
    resource: /hemory/2026-08-20-hemory-weekly-candidate-extraction.md#L198
---

Hemory 应强调“第二记忆”，避免“第二大脑”的过度承诺。

## Reason

三场会话重复出现；当前载体仍是派生抽取，需要人工确认。
```

约束：

- `proposal.id` 是 UUID v4；`dedupe_key` 在全部候选中稳定唯一。
- `scope`：`user-owner | user-profile | memory`。
- `operation`：`create | replace | merge | revoke | set-priority`。
- replace/merge/revoke/set-priority 必须带 `target_id` 与当前 `base_revision`；merge 的每个
  来源使用 `merge_from: [<id>@<revision>]` 绑定审阅时的 active revision。
- `suggested_priority`：`normal | high`。它只影响展示与检索权重，不授予权限。
- Agent 候选必须有非空 `generated.by`、`generated.at` 和至少一个精确 source。
- `user.owner`，以及权限、授权、承诺和行动敏感偏好强制标为 action-sensitive；候选不能
  自己降级敏感性，也不能批量批准。

### 2.2 批准事件

路径：`/memory/events/YYYY-MM/YYYY-MM-DD-HHmmss-<event-id>.memory-event.md`

每个事件一份不可变 Markdown，批准绑定候选规范化内容的 SHA-256：

```yaml
---
type: Memory Decision
title: 批准记忆候选 8afad9c5
created: 2026-09-01T06:05:00Z
event:
  version: 1
  id: 4ff8bb4e-0904-4af6-82ae-ab367a993c60
  action: approve
  proposal_id: 8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8
  proposal_sha256: <64 lowercase hex>
  entry_id: 26399214-4521-4f6f-b8a3-c73bdb78a418
  prior_revision: 1
  revision: 2
  decided_by: human:bruce
  decided_at: 2026-09-01T06:05:00Z
---
```

UI 与 CLI 必须回传审阅时展示的 candidate SHA-256；CLI 使用
`--proposal-sha256`。若 candidate 在展示后改变，决定必须失败并要求重新审阅。

- action 是 `approve | reject`；拒绝也保留事件，不能删候选掩盖历史。
- approve 必须来自插件中的直接人机交互，或当前对话里 owner 对精确 proposal id/diff 的
  明确指令。CLI 要求同时给出 `--approved-by human:<id>` 和
  `--confirm-human-approved`，AGENTS 禁止 Agent 自行推断或批量代批。
- 同一 `base_revision` 的并发批准以 conflict 错误失败，不允许 last-write-wins。
- 删除用 revoke 事件得到 `revoked`，不物理删除普通历史。若秘密误入，不在本工作流中
  伪装成普通删除；应停止同步、轮换凭据，并单独执行 Git 历史清理。

### 2.3 只读投影

根 `/USER.md` 与 `/MEMORY.md` 只由 projector 写。frontmatter 增加：

```yaml
managed:
  by: notemd.memory
  protocol: 1
  revision: 12
  projection_hash: <sha256>
```

正文顶部必须写 `GENERATED / READ-ONLY`。每条 active/revoked 条目至少包含：

```text
id:: <稳定 UUID v4>
revision:: <递增整数>
status:: active | revoked
priority:: normal | high
proposal:: <proposal UUID>
approved-by:: human:<id>
approved-at:: <RFC 3339>
source:: <原始来源>
```

正常投影显示 active 当前版本和 revoked tombstone，不在当前区域制造双真相。首次迁移的
旧条目例外保留为显式 `status:: pending`，直到逐项决定；Agent 不得把它们当作确定记忆。
`USER.md` 的兼容 owner block 继续供 Task owner gate 使用，首次专门批准后增加
entry/revision/confirmed_by/confirmed_at；owner 首次认领与后续变更必须逐项确认，不能借
整份文档批准。

projector 在每次写前比较已记录的 projection hash。发现外部编辑时进入 drift/repair，停止
批准和投影写入，只提供“把外部差异导入为候选”或“恢复已批准投影”；绝不自动采纳或覆盖。

## 3. Memory 插件

插件依赖本轮新增的受控宿主 API，manifest 的最低宿主版本为 `>=6.901.5`。在包含这些
API 的宿主版本发布前不得单独上架插件，避免旧版应用成功安装后运行时才失败。

插件 ID：`notemd.memory`，名称 `Memory / 记忆`，归入 Thinking / 思考。

窗口包含：

- 当前条目：按 USER/MEMORY、active/revoked、normal/high 过滤和搜索；
- 待确认：展示 before/after、来源、提议者、敏感性、冲突和逐项批准/拒绝；
- 添加/编辑：人的直接编辑也先生成候选和批准事件，再更新投影；
- 改善建议：先提供 legacy、精确重复、缺来源和过长条目的确定性检查；Agent 可据此通过
  CLI 提出更高语义层的合并或修正候选；
- 维护：显式迁移和 drift 提示。repair 与完整事件重建不在首版 UI 中静默执行。

所有 popup menu 使用宿主 `.menu-panel` / `.menu-row` 全局样式。

## 4. CLI

统一入口 `notemd memory <action>`：

- `list`：列出条目和 pending proposal；支持 scope/status/priority/json。
- `show --id <entry-or-proposal-id>`：输出当前条目、来源、版本和 pending diff。
- `suggest`：输出结构问题与给 Agent 的改善上下文，不直接写盘。
- `propose`：创建 create/replace/merge/revoke/set-priority 候选；永不直接改投影。
- `approve` / `reject`：只接受精确 proposal id；approve 强制显式 human actor 与确认 flag。
- `check`（兼容别名 `doctor`）：检查 projection hash、重复 ID、未知协议和 drift。
- `migrate`：显式、幂等地把 v1 条目转为待确认候选；不能启动时静默迁移。

CLI 默认输出稳定的人类文本，`--json` 使用宿主统一 envelope。未找到为非零退出；drift、
hash 不匹配、过期 revision 和非法 actor 必须失败关闭。

## 5. 首次迁移

- 插件加载不自动创建或改写 USER/MEMORY；人点击“开始迁移”或显式执行 migrate 才运行。
- MEMORY 现有 UUID 复用；USER 普通事实和 owner block分配稳定 UUID。
- 当前正文声明“未逐条人审”，迁移不得生成 `approved_by: human:bruce`。每条成为 pending
  proposal；投影可继续显示为 `review-state:: pending`，Agent 不得视为确定记忆。
- 普通条目由人在迁移窗口逐条确认；首版不提供批量批准。owner、权限、授权和行动偏好
  始终只能单项确认。
- 迁移完成前兼容读取 v1，避免 owner gate 或既有工作骤然失效。

## 6. 搜索与隐私

- `Memory Proposal` 与 `Memory Decision` 保留独立 `type`；搜索会按 `generated.by` 正确标为
  Derived。候选可能与根投影同时命中，调用方应查看 `status`、path 和来源，不能把 pending
  candidate 当作确定事实。
- 人审权重来自条目级批准元数据，不得把整份 Derived 投影提升为 Human。
- 插件、CLI、候选、事件和投影都禁止秘密；共享、公开、外部上下文仍需 owner 授权。

## 7. 验收

1. migration 幂等、USER ID 稳定，原文字节可追溯且不伪造人审。
2. proposal/event 均 no-clobber；批准精确绑定 proposal hash。
3. stale base revision、并发批准和重复 ID 失败关闭，不覆盖当前条目。
4. replace/merge/revoke/set-priority 都留下不可变事件；一个 entry 只有一个 active revision。
5. 外部直接编辑触发 drift，不能被 list/propose/approve 静默吸收。
6. UI 与 CLI 复用同一 backend/domain，不各写一套状态机。
7. USER/MEMORY 仍可被 notemd search 检索，pending 不被当作确定记忆。
8. 插件测试、Rust 测试、Svelte check、生产构建、manifest 校验、真实 sotvault 迁移演练通过。
