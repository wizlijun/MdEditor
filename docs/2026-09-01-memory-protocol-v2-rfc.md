# RFC：Memory Protocol v2——YAML Claim 资产、纯文本投影与 Git 多设备归并

> 状态：RFC 0.10 Review Candidate / Protocol Freeze Blocked Pending P0 Evidence
> 文档版本：0.10
> 日期：2026-09-01
> 目标版本：note.md Host 6.x 后续版本 / Memory Plugin 2.0.0
> 评审对象：产品、Claim 语义、数据模型、分布式一致性、Git 同步、安全、迁移、Agent 契约与可用性
> 本轮结论：架构方向与 P0 语义修订已纳入规范；跨语言 hash vectors、语义评测阈值和外部复审完成前，不得权威迁移或冻结长期兼容格式

## 目录

1. 执行摘要与评审请求
2. 背景、目标、非目标与核心不变式
3. 总体架构、目录布局、bootstrap 与 protocol/authority revision
4. Memory Claim Revision schema、状态机与 reducer
5. 纯文本投影与手工编辑导入
6. 本机事务、Git 多设备同步和崩溃恢复
7. 信任、安全、隐私、Host API、CLI 与 Agent 契约
8. Memory 插件产品与交互
9. v1 → v2 迁移、兼容、回滚与旧版本门禁
10. 完整性、测试矩阵、实施阶段与发布门禁
11. 风险、替代方案、实现影响面和开放裁决

---

## 0. 执行摘要

本 RFC 建议完全重构 note.md Memory 插件及宿主的 `memory_control` 模块。

目标模型是：

1. `.notemd/memory/` 是 Memory 唯一权威主张与控制资产源。
2. 每个逻辑主张有稳定 `claim_id`；每次建议、人工新增、编辑、批准、否认、忽略、撤销、删除或冲突解决，都新增一份内容寻址、不可变的 `MemoryClaimRevision` 完整快照。
3. Claim revision 通过 `parents` 表达主张因果关系，形成每个 claim 一张小型 DAG；`claim_kind`、subject、asserted/recorded/approved roles、有效时间和使用范围都是权威语义的一部分。
4. `USER.md` 与 `MEMORY.md` 只是由当前可投影主张确定性生成的纯文本视图，不再保存或暴露 ID、状态、来源、SHA、优先级、引用备注等程序元数据。
5. Git 负责不可变资产的同步、提交历史与恢复；Memory reducer 负责从记录集合计算当前主张、有效时间、授权状态、场景适用性和语义冲突，并重建投影。
6. 不引入 SQL，不使用跨设备锁，不使用全局递增 revision，不使用 last-write-wins，也不使用 Git 时间戳裁决事实真伪。
7. 人工新增和人工点击确认均为一次交互并立即生效；Agent 只能创建 `pending` 建议，不能伪造 `human:*` 审批。
8. 不同 claim 的离线并发写入自然取并集；同一 claim 的并发有效修改必须显式冲突，由人选择或合并，不能静默丢弃任何一侧。
9. Owner 批准“忠实表达我”不等于验证外部命题为真，也不等于授权 Agent 执行现实行动；这些含义必须由 `approval_kind` 与 `authority_scope` 明确区分。
10. 任何 Agent context 都必须按 Space、purpose、caller、provider/model 和 tools 选择 claim，并生成可审计 Context Manifest；未明确场景不得跨敏感空间加载。

本方案不是完整 CRDT。底层不可变记录集合具有 grow-only set 的可交换合并特性；Claim 语义由类型、主体、审批边界、有效时间和因果 DAG 决定。这比 LWW CRDT 更适合“正确性比新鲜度重要”的个人记忆系统。

### 0.1 请求评审人重点裁决

外部评审需要明确回答以下五个问题：

1. 是否接受“不可变 full-snapshot Claim Revision”作为唯一权威写模型，而不是每 claim 一个可变 YAML？
2. 同一 claim 出现多个已批准因果 head 时，是否接受普通正文暂不投影、action-sensitive claim 额外执行安全覆盖的 fail-closed 策略？
3. 是否接受 `USER.md` / `MEMORY.md` 冲突时从 YAML 直接重建，而 `.notemd/memory/**/*.yaml` 冲突必须阻断写入？
4. 是否接受普通删除只形成 tombstone，不能承诺从 Git 历史物理擦除？
5. 是否接受 v1 到 v2 只做一次权威切换，不长期双写两个协议？

上述五项外部裁决均已接受或有条件接受。当前冻结阻塞点不在存储骨架，而在 P0 Claim 语义、authority revision、transition 矩阵、风险冲突和 Context Manifest；详见 §3。

---

## 1. 背景与现状问题

### 1.1 当前 v1 数据职责

当前 Memory v1 将状态分散在四处：

| 位置 | 当前职责 | 问题 |
| --- | --- | --- |
| `USER.md` / `MEMORY.md` | 保存当前事实及大量内联控制字段 | 既是用户可读文件又是状态数据库，职责冲突 |
| `inbox/memory-candidates/*.memory-candidate.md` | 保存 Agent/人工候选 | 格式是 Markdown + YAML frontmatter，仍在专用目录外 |
| `memory/events/**/*.memory-event.md` | 保存批准/否认事件 | event 不会被完整重放成当前状态 |
| `.notemd/memory/state.json` | 保存全局 revision 与投影 hash | 高频共享热点，多设备 Git 合并脆弱 |
| `.notemd/memory/control.lock` | 本机 `fs2` 排他锁 | 只能处理单机并发，目前还可能被 Git 跟踪 |

相关实现集中于：

- `src-tauri/src/memory_control/model.rs`
- `src-tauri/src/memory_control/document.rs`
- `src-tauri/src/memory_control/store.rs`
- `src-tauri/src/vault_sync/conflict.rs`
- `src-tauri/src/vault_sync/git_ops.rs`

### 1.2 当前写入的一致性缺口

v1 的批准大致按以下顺序发生：

1. 修改 `USER.md` 或 `MEMORY.md`。
2. 写决定 event。
3. 重新读取两个根文档。
4. 写 `state.json`。

这是跨多个文件的非事务序列。进程在任意一步崩溃都可能留下 drift。

更严重的是：当前 current state 主要从根 Markdown 反解析，而不是从 candidate/event 重放。多设备合并后，某一侧的 candidate 和 approved event 可能已经存在，但对应事实没有进入最终根投影；现有完整性检查仍有可能通过。这属于静默语义丢失。

### 1.3 当前 Git 冲突策略不适用于 Memory

Vault sync 当前普通冲突路径会倾向保留一侧并生成 conflict copy。这适用于一般用户文档，但不适用于：

- 可由事实源重新生成的 `USER.md` / `MEMORY.md`；
- 声明为不可变的 Memory YAML 源资产；
- 需要领域语义判断的同一事实并发更新。

Memory v2 必须在 Git merge 后执行专用 validate/reduce/rebuild，而不是沿用普通文本冲突策略。

### 1.4 真实迁移规模

截至本 RFC 审计时，真实 `sotvault` 中至少已有：

- 113 个旧 candidate 文件；
- 25 个旧 event 文件；
- 34 个当前逻辑 entries：15 active、19 pending；
- 88 个 pending proposals；
- 2 个 `.notemd/memory` 控制文件，`state.json` 为 protocol 1 / revision 28；
- 根目录的 `USER.md` 与 `MEMORY.md` 当前投影。

只读基线验证还确认：113 个 candidate ID 与 dedupe key 均唯一；25 个 event 中 24 approve、1 reject，全部能唯一关联 candidate；Memory 受控路径内没有 conflict artifact。真实 apply 前必须重新生成 source manifest 和 plan hash，本文数字只能作为评审基线，不能替代迁移时校验。

因此迁移必须提供 dry-run、逐项映射、状态守恒、幂等 apply 和可验证回滚，不能直接覆盖。

---

## 2. 目标、非目标与规范语言

### 2.1 目标

- 让纯文本 Markdown 保持对人和普通 Agent 友好。
- 让 claim 类型、主体、断言者、证据关系、有效时间、使用场景、确定度、审批和历史拥有稳定的结构化表示。
- 所有持久过程资产集中在 `.notemd/memory/`。
- 支持多设备离线写入和 Git 归并，不静默覆盖事实。
- 任意时刻可以仅凭 Git 跟踪的 YAML 重建当前状态与两个投影。
- 人工交互足够简单：添加只需分类与正文，确认只需点击一次。
- 明确 Vault owner、人工审批和 Agent 建议的信任边界。
- 保留正向/负向、trust tier、risk class、salience、确定度和避免犯错等结构化语义，但不污染根 Markdown。

### 2.2 非目标

- 不建设云端 Memory 服务。
- 不引入 SQL、SQLite、向量数据库或常驻服务。
- 不解决 Git 远端本身的身份认证和访问控制。
- 不把 Git commit author 当成人工审批身份。
- 不自动判断并发事实中哪一条“更新”或“更正确”。
- 不承诺普通删除能从 Git 历史、远端和其他设备物理擦除。
- 不在 v2 首版实现事实图谱、跨 Vault 联邦或自动共享。
- 不把 task、每日记录或其他人的责任默认提升为 owner 记忆。
- v2 只管理具有跨会话价值的个人 semantic claims；原始 episodic 日志、当前 working memory、可执行 procedural/skill memory、task/reminder/idea、credential/secret 均不属于本协议。
- `commitment` claim 只保存“某主体作出过长期承诺”这一主张，不承担任务执行、提醒或完成状态；可执行动作仍进入 `inbox/tasks/*.md`。

### 2.3 规范语言

本文中的 MUST、MUST NOT、SHOULD、SHOULD NOT、MAY 分别表示必须、禁止、建议、不建议和可选行为。实现与本文冲突时，应视为协议偏差，而不是“等价实现”。

---

## 3. 协议冻结门禁与核心不变式

### 3.1 P0 语义冻结门禁

RFC 0.10 保留 0.9 的不可变资产、Git reconcile 和迁移架构，并已把评审要求的语义修改写入规范。勾选表示“RFC 已给出规范性裁决”，不表示代码已经实现；协议冻结必须等到以下项目全部完成：

- [x] 核心对象统一为 `MemoryClaimRevision`，并定义 `claim_kind / subject / asserted_by / recorded_by`。
- [x] `approval_kind`、`authority_scope` 与 evidence relation 能区分自我表达、行为授权、记录证明和外部事实核验。
- [x] valid time、uttered/observed time、recorded time 与 decision time 分离，并定义按 claim kind 的组合约束。
- [x] protocol 与 owner authority 使用不可变、内容寻址 revision DAG；历史决定不依赖当前可变文件即可验证。
- [x] transition 矩阵与跨 claim merge operation schema 完整、可机器验证。
- [x] action-sensitive conflict 采用保守安全覆盖层，禁止事项和隐私边界不会因冲突从 Agent 上下文消失。
- [x] `trust_tier / risk_class / salience` 与读取侧 retrieval weight 完全分离。
- [x] Space、purpose、caller、provider/model、tools、consent 与 Context Manifest 进入读取协议。
- [x] 没有字段级加密前，`restricted` 明文持久化被确定性禁止。
- [ ] canonical hash 规范和 Rust/TypeScript 跨语言 golden vectors 发布。
- [x] 语义可靠性评测项目进入发布门禁，而不仅是文件和 Git 一致性测试。
- [ ] 用真实 sotvault/合成 fixture 确定语义评测数值阈值，并由外部架构、安全、Git、语义与产品评审签字。

在这些门禁完成前：MUST NOT 将状态标为 `Protocol Frozen`，MUST NOT 对真实 Vault 执行权威 v2 cutover，MUST NOT 承诺长期兼容 schema。

### 3.2 语义不变式

1. **Claim 不是裸事实**：系统保存的是“关于某个 subject、由某主体断言、被某人批准以某种方式使用的主张”，不是未经限定的客观真理。
2. **角色不可互推**：`subject`、`asserted_by`、`recorded_by` 与 `decision.actor_id` 必须分别存在；producer、source、approver 和 Git author 不得互相代替。
3. **批准含义显式**：批准忠实表达 owner、批准 Agent 据此行动、验证外部命题和证明某人说过，必须是不同 approval kind；普通批准不得隐式提升 certainty、trust tier 或 risk class。
4. **时间分轴**：事实有效时间、表达/观察时间、系统记录时间、人工决定时间与 Git 到达时间不得复用同一字段。
5. **场景最小化**：没有显式 Space 和 purpose 时，context 不得跨场景加载；存储同意不自动等于同步、投影、外发或分享同意。
6. **安全不因冲突放宽**：并发 allow/deny、grant/revoke、隐私边界冲突时，运行时能力只可保持或收紧，不得恢复权限。
7. **读取可解释**：任何给可行动 Agent 的 context 必须说明选了哪些 claims、为什么、用于什么、将发给谁，并暴露 abstention/conflict。

### 3.3 存储与并发不变式

Memory v2 必须持续满足以下不变式：

1. **单一权威**：只有被 Git 跟踪的 v2 YAML 权威资产可以决定 claim、protocol 与 authority 状态。
2. **投影可弃**：删除 `USER.md` / `MEMORY.md` 后，必须能从 YAML 无损重建。
3. **记录不可变**：已发布 revision 文件不得原地修改或删除；修正必须新增记录。
4. **无全局时钟**：不能依赖全局递增整数、墙上时间或 Git commit 时间决定事实先后。
5. **因果显式**：claim 版本关系只由 `parents` 与精确 payload hash 表达；有效时间不得代替因果顺序。
6. **审批显式**：只有可信人工入口可以产生 `human:*` approval。
7. **Agent 不自批**：Agent 只能创建 pending revision，不能创建 active/rejected/ignored/revoked/deleted 人工决定。
8. **冲突不吞数据**：多个并发有效 head 必须保留，并进入领域冲突状态。
9. **来源先于投影**：必须先发布不可变 YAML，再重建根 Markdown。
10. **确定性**：同一有效 YAML 集合、相同 as-of/context request，无论扫描顺序、设备和进程如何，reducer 结果与投影字节必须相同。
11. **owner 限制**：Agent 只能提议与 Vault owner 本人直接相关的长期主张，不得记录其他人的任务或把他人的责任改写成 owner 义务。
12. **未知不升级**：迁移或校验时无法证明审批的条目，不得变成 active。

---

## 4. 总体架构

```text
                  ┌───────────────────────────────┐
                  │ .notemd/memory/**/*.yaml     │
                  │ 唯一权威、Git 跟踪、不可变     │
                  └───────────────┬───────────────┘
                                  │ scan + validate
                                  ▼
                  ┌───────────────────────────────┐
                  │ Memory Reducer                │
                  │ DAG / approval / conflict     │
                  └───────┬───────────┬───────────┘
                          │           │
                    current view   diagnostics
                          │           │
                ┌─────────▼───┐  ┌────▼───────────┐
                │ Projector   │  │ UI / CLI / RPC │
                └─────┬───────┘  └────────────────┘
                      │
          ┌───────────┴───────────┐
          ▼                       ▼
      USER.md                 MEMORY.md
      纯文本投影               纯文本投影
```

Git 并不理解事实状态。Git 只合并不可变文件集合。Memory reducer 在 merge 前后解释集合中的因果关系、审批和冲突。

---

## 5. 存储布局

```text
.notemd/memory/
├── bootstrap.yaml
├── protocol-revisions/
│   └── <revision-id>.<file-sha256-prefix>.yaml
├── authority-revisions/
│   └── <revision-id>.<file-sha256-prefix>.yaml
├── claims/
│   └── <claim-id-prefix>/
│       └── <claim-id>/
│           └── revisions/
│               └── <revision-id>.<file-sha256-prefix>.yaml
├── imports/
│   └── <import-id>.<file-sha256-prefix>.yaml
├── operations/
│   └── <operation-id>.<file-sha256-prefix>.yaml
├── context-manifests/
│   └── YYYY-MM/<manifest-id>.<file-sha256-prefix>.yaml
├── migrations/
│   └── <migration-id>/
│       ├── manifest.yaml
│       ├── mapping.yaml
│       └── legacy-content.yaml
├── staging/
│   └── <migration-or-import-id>/
├── legacy/
│   └── v1/
└── .local/
    ├── cache/
    ├── diagnostics/
    ├── imports/
    ├── purge/
    ├── projection-generation.yaml
    └── tmp/
```

### 5.1 Git 跟踪规则

MUST 跟踪：

- `bootstrap.yaml`（初始化后不可修改）
- `protocol-revisions/**/*.yaml`
- `authority-revisions/**/*.yaml`
- `claims/**/*.yaml`
- `imports/**/*.yaml`
- `operations/**/*.yaml`
- `context-manifests/**/*.yaml`
- `migrations/**/*.yaml`
- `staging/**`，但 reducer 必须忽略未激活 run
- `legacy/v1/**`

MUST NOT 跟踪：

```gitignore
.notemd/memory/.local/
```

临时文件不得直接创建在会被 `git add -A` 捕获的路径中。仓库写锁不放在工作树：实现必须以 canonical Git common directory 为键，在 common git dir 或应用本机数据目录中建立锁，从而协调同一仓库的多个 worktree。

### 5.2 为什么不采用一个可变 YAML

以下设计被明确否决：

```text
.notemd/memory/claims/<claim-id>.yaml
```

即使“一主张一文件”，同一 claim 在两台设备上并发修改仍会产生同路径文本冲突。若同步层选择 ours/theirs，就会丢失一侧；若让 Git 合并 YAML 行，则可能产生语法有效但语义无效的混合对象。

v2 使用稳定 `claim_id` + 每次修改一个新 `revision_id`，让不同设备写入不同路径。最终文件名还携带 raw file SHA-256 前缀；加载时必须校验路径摘要与文件字节一致。相同 `revision_id` 出现不同摘要对象时属于 integrity conflict。Git 负责文件集合取并集，领域层负责识别 sibling revisions。

### 5.3 分片目录

`<claim-id-prefix>` SHOULD 使用 `claim_id` 的前两个十六进制字符，避免大量目录项影响文件系统性能。该前缀不参与语义。

### 5.4 权威资产、过程资产与本机诊断

- Claim、protocol、authority revision、人工决定、原子 operation、已实际外发/使用的最小 Context Manifest、migration mapping 是 Git 跟踪的权威或审计资产。
- deterministic conflict、projection eligibility、stale/expired 状态由 reducer 运行时派生，不另写一个会制造冲突的“当前冲突文件”。人工 resolve 才产生不可变 operation/revision。
- 带本机路径、到达时间、环境信息或完整正文副本的 diagnostics、import recovery、purge plan 放在 `.local/`，不进入 Git，也不参与语义。
- tracked import 只保存规范化 proposals、hash、source span 和 lineage；可能含 secret 的 actual/expected 原文先留在 `.local/imports/`，经用户清理和确认后才能发布最小资产。

---

## 6. Bootstrap、Protocol Revision 与 Authority Revision

根 `protocol.yaml` 不再保存可变的唯一 protocol/owner 状态。v2 使用一个初始化后不可修改的 bootstrap，以及两张内容寻址的不可变 revision DAG。

### 6.1 Bootstrap

```yaml
schema: notemd.memory/bootstrap/v2
vault_id: 019...
protocol_family: notemd.memory
initial_protocol_revision:
  revision_id: 019...
  payload_sha256: ...
initial_authority_revision:
  revision_id: 019...
  payload_sha256: ...
```

- `bootstrap.yaml` 只建立 Vault 身份与两个 DAG root，初始化后 MUST NOT 修改。
- 首次 bootstrap 只能由可信本机 owner-setup 或独立 recovery proof 创建；输入一个 `human:*` 字符串不是身份验证。
- 同一 Vault 出现两个不同 bootstrap 是 repository integrity failure。

### 6.2 Protocol Revision

```yaml
schema: notemd.memory/protocol-revision/v2
revision_id: 019...
base_heads:
  - revision_id: 018...
    payload_sha256: ...
causal_context:
  parents:
    - record_id: 019...
      raw_sha256: ...
protocol_major: 2
protocol_minor: 0
renderer_version: notemd.memory.projector/2
claim_schema: notemd.memory/claim-revision/v2
category_registry:
  user: [owner, identity, preferences, work-style, boundaries, other]
  memory: [decisions, constraints, practices, context, other]
decision:
  verdict: approve
  actor_id: owner:019...
  authority_context:
    heads:
      - revision_id: 019...
        payload_sha256: ...
    capability: memory.protocol.modify
transition:
  operation: replace
payload_sha256: ...
```

- 分类 key、状态/operation 枚举、claim schema、canonicalization 和 renderer compatibility 均由 protocol revision 声明。
- 当前 protocol 是 reducer 算出的唯一 maximal approved head；多个 heads 时 protocol conflict，所有 Memory 写入与投影重建 fail closed。
- 旧 protocol revisions 永久保留，因此历史 decision 绑定的完整 protocol payload 在删除 Git commit 历史后仍可验证。
- 自定义分类若未来允许，必须通过 protocol revision；首版固定枚举，Agent 不得任意创造近义分类。

### 6.3 Authority Revision

```yaml
schema: notemd.memory/authority-revision/v2
revision_id: 019...
base_heads:
  - revision_id: 018...
    payload_sha256: ...
causal_context:
  parents:
    - record_id: 019...
      raw_sha256: ...
owner:
  owner_id: owner:019...
  actor_id: human:bruce
principals:
  - actor_id: human:bruce
    capabilities:
      - memory.claim.approve
      - memory.claim.resolve
      - memory.protocol.modify
      - memory.authority.modify
recovery:
  mode: local-owner-setup
decision:
  verdict: approve
  actor_id: human:bruce
  authority_context:
    heads:
      - revision_id: 018...
        payload_sha256: ...
    capability: memory.authority.modify
transition:
  operation: replace
payload_sha256: ...
```

- 每个生效 authority revision 是一个不可变 authority epoch；epoch 身份由 revision ID/hash 决定，不使用全局整数。
- 每个 revision 保存完整 principal/capability 快照。Owner 合法变更后，旧决定仍可验证它当时绑定的 authority payload。
- authority change 必须引用创建者当时看到的完整 authority head 集；authority conflict 时 capability 使用安全交集，任一 head deny、撤销或未授予的新权限都不得生效。
- grant/revoke 并发时 grant 不生效，但冲突仍然存在；只有同时后继于所有 heads 的明确 resolution 才能重新授权。
- 如果安全交集中没有 `memory.authority.resolve`，只能使用 bootstrap 时预先定义的 recovery 机制，不能让任一冲突 owner 自选为胜者。

### 6.3.1 Control-plane transition 矩阵

| 对象 / operation | 合法 actor | 必须引用 | 生效条件 |
| --- | --- | --- | --- |
| protocol `initialize` | bootstrap owner-setup | 空 base + bootstrap intent | 只能创建一次，hash 与 bootstrap root 一致 |
| protocol `replace` | `memory.protocol.modify` | 唯一 current protocol head + current authority context | schema/renderer 兼容检查通过 |
| protocol `resolve` | `memory.protocol.resolve` | 全部 maximal protocol heads + current authority context | 新 revision 后继于全部 heads |
| authority `initialize` | bootstrap owner-setup/recovery proof | 空 base + bootstrap intent | 只能创建一次，owner 稳定 ID 唯一 |
| authority `replace` | `memory.authority.modify` | 创建者看到的全部 authority heads + protocol context | 无 authority conflict，且旧 capability 有效 |
| authority `resolve` | 安全集合中的 `memory.authority.resolve` 或 bootstrap recovery | 全部 maximal authority heads + protocol context | capability 不能超出明确 resolution；重新授权逐项展示 |

Control-plane revision 没有 Agent pending 路径。所有修改都必须来自受控 human/recovery capability，`decision.verdict` 固定为 approve；非法 actor、缺 head、stale base、重复 key/different payload 与迟到 sibling 的处理规则和 Claim transition 相同。合法重复请求按 idempotency 折叠，不按时间取胜。

### 6.4 全局因果上下文

Claim、decision、operation、protocol 和 authority 对象除各自 `base_heads` 外，还必须携带创建时观察到的共享记录 frontier：

```yaml
causal_context:
  parents:
    - record_id: 019...
      raw_sha256: ...
```

- 这是多写设备中的观察关系，不是墙上时间。Git 晚到、mtime、UUIDv7 顺序和 `recorded_at` 都不得替代它。
- Authority revision 若观察过一个旧 epoch decision，该 decision 是 authority change 的因果祖先，保持历史有效。
- 旧 epoch decision 若观察过撤权 revision却仍声称旧 authority，则无效；若两者并发，则进入 `authorization-conflict`，业务效果 quarantine。
- 普通有效时间不得影响 authority；回填 `valid_from` 不能让旧权限复活。
- 可写无业务效果的 join record 压缩 frontier，但 join 只能表达“已观察”，不能改变 claim 或权限。

### 6.5 Owner 读取契约

`notemd memory owner --json` MUST 从 authority reducer 返回稳定 owner ID、actor、authority heads、conflict 和 capabilities。Agent 不得通过解析 `USER.md` 文案猜测机器身份或权限。

根 `protocol.yaml` 若为兼容旧客户端而暂时保留，只能是 bootstrap/派生摘要或 protocol 2 write fence，绝不能重新成为可变权威源。

---

## 7. Memory Claim Revision YAML

### 7.1 完整示例

```yaml
schema: notemd.memory/claim-revision/v2
claim_id: 01999f80-28cf-7ac4-9a09-f515b071fe1a
revision_id: 01999f81-01a7-70f7-af93-87fca7ec225c
request_id: memory-ui/01999f81-16e8-7ab5-91f6-c2bf1215ad5e

parents:
  - revision_id: 01999f7e-a69a-7862-94fa-b0d51b62d90b
    payload_sha256: 8ea7...
causal_context:
  parents:
    - record_id: 01999f7f-26f1-78a6-8bf8-50a05f6f4894
      raw_sha256: 6b3e...

claim_kind: preference
kind_data:
  preference:
    dimension: response-structure
subject:
  kind: vault-owner
  id: owner:01999f70-46da-7ed5-84a0-37e8f34363cb
  relation_to_owner: self
asserted_by:
  - kind: owner
    id: owner:01999f70-46da-7ed5-84a0-37e8f34363cb
    basis: direct-input
recorded_by:
  kind: host
  id: notemd.memory-ui
  device_id: 772a3d4a-8528-4b5d-8d8c-9f95052d1c65
recorded_at: 2026-09-01T08:30:00Z

text: |-
  用户希望回答先给出结论。
  不确定的内容需要明确标注并核验。

projection:
  target: user
  category: preferences
  visibility: projection

workflow:
  state: approved
lifecycle:
  state: active

temporal:
  uttered_at: 2026-09-01T08:30:00Z
  observed_at: null
  valid_from: 2026-09-01T08:30:00Z
  valid_until: null
  planned_for: null
  due_at: null
  review_after: null

epistemic:
  basis: owner-stated
  representation_certainty: high
  truth_status: not-assessed
  truth_confidence: unknown

trust_tier: stable-preference
risk_class: behavioral
salience: pinned
polarity: positive
sensitivity: normal

context:
  spaces: [work/hemory]
  applies_when: []
  excludes_when: []
consent:
  scope: personal-assistant-only
  allowed_purposes: [planning, writing]
  external_provider_policy: prompt

agent_use:
  guidance: |-
    组织回答时先给出可执行结论，再补充必要原因。
  avoid_error: |-
    不要把沟通偏好扩张为未表达的外部行动授权。

decision:
  verdict: approve
  approval_kind: self-representation
  authority_scope: personal-assistant
  actor_id: human:bruce
  decided_at: 2026-09-01T08:30:00Z
  protocol_context:
    heads:
      - revision_id: 019...
        payload_sha256: 55a3...
  authority_context:
    heads:
      - revision_id: 019...
        payload_sha256: 22b4...
    capability: memory.claim.approve

transition:
  operation: approve
  approves_revision_id: 01999f80-aaaa-7aaa-8aaa-aaaaaaaaaaaa
  approves_payload_sha256: f165...

evidence:
  - relation: evidence-of-speech
    resource: notes/2026-09-01-example.md#L20
    content_sha256: 2f4a...
    title: Owner statement

lineage:
  derived_from: []
  produced_by_operation: null
dedupe_key: owner/preferences/conclusion-first
payload_sha256: 87a2...
```

### 7.2 角色与字段职责

| 字段 | 规范语义 | MUST 约束 |
| --- | --- | --- |
| `claim_id` | 跨 revision 稳定的逻辑主张 ID | UUIDv7；不得从正文、姓名或路径推导 |
| `revision_id` | 不可变版本 ID | 同逻辑 ID 不同内容属于 integrity conflict |
| `parents` | 同 claim revision DAG 的直接父节点 | 创建为空；普通更新一个；resolve 可多个；不得跨 claim |
| `claim_kind` | 主张的语义类型 | 不得用 category、scope 或文件位置代替 |
| `kind_data` | 与 `claim_kind` 对应的 tagged payload | 只能包含一个与 claim kind 同名的 key；不得把类型专属字段塞进正文 |
| `subject` | 这条主张关于谁或什么 | 稳定 ID 必填；显示名不是身份；必须表达与 owner 的关系 |
| `asserted_by` | 谁表达、观察、承诺、判断或推断了该命题 | 至少一项；Agent 摘要/推断不能冒充 owner 原话 |
| `recorded_by` | 谁/哪个进程把本 revision 写入系统 | 恰好一个，由 Host 从 caller context 注入，不信任请求体 |
| `decision.actor_id` | 谁批准把精确主张按某种含义纳入记忆 | 不等于 subject、asserted_by、recorded_by 或 Git author |
| `projection` | 纯文本视图的 target/category/visibility | 只负责显示，不承担 claim kind、主体或权限语义 |
| `workflow.state` | 人工决定流程状态 | `pending`、`approved`、`rejected`、`ignored`；decision-conflict 由 reducer 派生 |
| `lifecycle.state` | revision 声明的业务状态 | `active`、`revoked`、`deleted`、`merged`；conflict/stale/expired 是 reducer 派生 |
| `temporal` | 命题表达、观察和现实有效时间 | 不得用 recorded/approved/Git time 自动填充 valid time |
| `epistemic` | 记录忠实度与外部真值状态 | 两者分轴；owner-stated/high 只保证忠实表达 owner |
| `trust_tier` | 长期治理/信任层 | 不等于 approval、truth、risk 或 salience |
| `risk_class` | 错误使用造成的行动风险 | 不等于 sensitivity、priority 或 polarity |
| `salience` | 跨会话召回显著性 | `pinned` 或 `normal`；不是 task 优先级或 deadline |
| `context/consent` | 可在哪个场景、目的和 provider 条件下使用 | 空 Space 不解释为 global；存储同意不自动等于外发同意 |
| `evidence.relation` | 来源支持哪种命题 | `evidence-of-speech`、`evidence-of-observation`、`evidence-of-truth` 或 `derived-from` |
| `lineage` | 跨 claim/import/migration/operation 的派生关系 | 不能用 source 或自由文本 reason 代替 |

### 7.3 Claim Kind 约束矩阵

| `claim_kind` | 主张含义 | 最低语义要求 | 默认有效时间 | 合法 approval kind | 默认 risk |
| --- | --- | --- | --- | --- | --- |
| `identity` | owner 或直接相关实体是谁/处于何种身份 | subject 稳定；owner 身份必须 owner asserted | timeless 或 interval | self-representation | behavioral；owner authority 另走 authority DAG |
| `preference` | 某主体偏好什么 | subject/assertor；不得去掉“谁偏好” | open interval；可有 valid_until | self-representation | informational 或 behavioral |
| `boundary` | owner 明确允许、禁止或要求先询问的边界 | `behavior_policy.effect/actions/resources` 与 avoid_error 必填 | open interval | behavioral-authorization | action-sensitive |
| `decision` | 某主体在某范围作出的决定 | decision-maker、decided_at、scope、effective interval | instant + optional interval | self-representation；若指导行动则 behavioral-authorization | behavioral/action-sensitive |
| `belief` | 某主体相信或判断 P | 文本必须保留“subject believes P” | interval/unknown | self-representation | informational |
| `observation` | 某主体/系统观察到 P | observer、observed_at、evidence-of-observation | instant/interval | self-representation 或 factual-verification | informational/behavioral |
| `commitment` | 某主体承诺做 P | committed_by、planned_for/due_at（若有）；不是 Task | instant + future interval | self-representation | behavioral |
| `practice` | owner/project 的稳定做法或工作准则 | applies_when/excludes_when 至少一项 | open interval | behavioral-authorization | behavioral |
| `material_fact` | 试图描述外部世界为真的命题 | subject、evidence-of-truth、truth status；来源只证明说过时不得使用此类 | timeless/instant/interval | factual-verification | informational/behavioral |
| `quotation` | 某主体在某时说过 P | asserted speaker、uttered_at、evidence-of-speech | instant | self-representation | informational |
| `legacy-unclassified` | v1 迁移但语义不足的保真记录 | 仅 migration 可创建；缺失字段逐项标 reason | unknown | 无；等待补全 | informational；永不 action-sensitive |

`legacy-unclassified` 只能保持 approved history + quarantined application，不能进入 projection 或默认 Agent context。新 UI/Agent 不得主动创建该类型。

#### 7.3.1 `kind_data` tagged union

`decision` 顶层字段只表示“人类如何批准本 revision”；主张本身的决定时间、承诺者或安全策略必须进入 `kind_data`，避免 approval 与 claim semantics 重名。合法 shape 如下；`...` 表示普通标量/列表，而不是允许任意未知字段：

```yaml
kind_data:
  identity:
    identity_type: person | role | account | relationship
    value: ...

  preference:
    dimension: ...

  boundary:
    behavior_policy:
      effect: deny | allow | prompt
      actions: [...]
      resources: [...]
      conditions: [...]

  decision:
    made_by: human:...
    decided_at: 2026-09-01T08:00:00Z
    decision_scope: ...

  belief:
    proposition: ...

  observation:
    observer: human:... | system:...

  commitment:
    committed_by: human:...
    beneficiary: owner | project:... | external:...

  practice:
    practice_scope: ...

  material_fact:
    proposition: ...

  quotation:
    speaker: human:...

  legacy-unclassified:
    missing_semantics: [claim-kind, subject, asserted-by, valid-time]
```

实际 revision 只能出现其中一个成员。`boundary.behavior_policy` 的 actions/resources 使用 protocol revision 中的稳定枚举；未知 action 默认不可授权。`decision.made_by`、`observation.observer`、`commitment.committed_by`、`quotation.speaker` 必须能与 `asserted_by` 的关系解释一致，validator 不允许相互矛盾。`legacy-unclassified.missing_semantics` MUST 非空。

#### 7.3.2 按 Claim Kind 的投影与默认读取

| claim kind | 允许的根投影 | 额外规则 |
| --- | --- | --- |
| identity | `USER.md` / owner、identity | 只投影 owner 自我身份；authority 身份绝不从此读取 |
| preference | `USER.md` / preferences、work-style | 文本必须保留主体；stale/expired/contested 不投影 |
| boundary | `USER.md` / boundaries | 正常时可投影自然语言；冲突时只用 §9.4 固定安全提示，行动读取必须走 context |
| decision | `MEMORY.md` / decisions | 仅当前有效决定；指导现实行动时必须 behavioral authorization |
| belief | `USER.md` / other 或 `MEMORY.md` / context | 文本必须明确“谁相信/判断”；不能投影为无主语外部事实 |
| observation | 默认 `trusted-agent` | 只有 factual verification 且低风险时可投影；必须保留观察者与时间 |
| commitment | `MEMORY.md` / context | 只作长期承诺背景，不替代 `inbox/tasks/` 的执行条目；过期后不投影 |
| practice | `MEMORY.md` / practices、constraints | 仅在 applies_when 对应的 Space/purpose 读取 |
| material_fact | `MEMORY.md` / context | 必须 factual verification + evidence-of-truth + current valid time；否则仅结构化 `do_not_rely` |
| quotation | 默认 `trusted-agent` | 只有 owner 明确选择 projection 才可进入根文档，且必须保留说话人语义 |
| legacy-unclassified | 禁止 | 只在迁移复核 UI 可见 |

Projector 必须同时满足本表、protocol category registry、visibility、consent、temporal、epistemic 与 reducer application state；任一条件不满足即不输出正文。将 claim 从默认 `trusted-agent` 改为 projection 是独立的人类 `change-context-consent` 决定，Agent 建议不能自动扩大可见面。

### 7.4 批准语义

| `approval_kind` | 人工决定实际批准了什么 | 不代表什么 |
| --- | --- | --- |
| `self-representation` | 这忠实表达了 owner 的身份、偏好、信念、观察、决定、承诺或引述 | 不证明外部命题 P 为真；不自动授权现实行动 |
| `behavioral-authorization` | 在 `authority_scope` 与 context/consent 边界内，个人助理可据此调整行为 | 不授权超出 scope 的发送、支付、删除或代表用户承诺 |
| `factual-verification` | 人工明确确认 exact material claim 在给定证据、时间和范围内可视为已核验 | 不改变来源质量；不保证未来仍有效 |

规则：

- Owner 普通点击“确认记住”默认是 `self-representation`，除非 UI 明确展示更强语义。
- `behavioral-authorization` 必须显示 action/resource/scope 和后果；action-sensitive 逐条批准，禁止批量。
- `factual-verification` 必须明确询问“这条外部事实是否正确”，并绑定 evidence-of-truth；不能由普通记忆确认、迁移批次或 source URL 推导。
- pending 被批准后，`claim_kind`、epistemic、truth confidence、trust tier、risk class、有效时间和来源质量不得隐式升级。
- 相同正文作为 preference、belief、material_fact 时，context API 必须返回不同语义，Agent 必须分别理解为“owner 偏好”“owner 相信”“经核验外部主张”。

### 7.5 Epistemic 与证据关系

- `representation_certainty` 回答“系统是否忠实记录了 asserted_by 的表达/观察”；`truth_status/truth_confidence` 回答“命题内容是否被验证”。
- `owner-stated + representation_certainty: high` 不得被转述为无主语事实。
- `source-supported` 必须通过 evidence relation 说明来源支持“有人说过”“有人观察到”还是“命题为真”。
- `inferred` 的 asserted_by 必须包含 Agent/推断者，truth confidence 最高为 medium，除非另有独立 factual verification revision。
- `contested/unknown/low` 默认不进入无元数据纯文本投影；结构化 context 返回 `do_not_rely: true`。

### 7.6 双时态与有效区间

| 字段 | 含义 | 约束 |
| --- | --- | --- |
| `uttered_at` | 某主体何时表达这句话 | quotation、直接表达的 preference/belief 可用 |
| `observed_at` | 观察何时发生 | observation 必填；不是 recorded_at |
| `valid_from` / `valid_until` | 主张声称在现实世界成立的半开区间 `[from, until)` | preference/boundary/practice/有效决定通常要求；空 until 表示开放区间 |
| `planned_for` | 意图/决定计划何时生效或发生 | 只描述计划，不表示已发生 |
| `due_at` | commitment 约定期限 | 不把 claim 变成 Task |
| `review_after` | 何时必须复核易变或 unknown 时间语义 | 到期后降为 stale，不物理改旧 YAML |
| `recorded_at` | 系统何时创建 revision | 事务时间；不得充当 valid time |
| `decision.decided_at` | 人工何时作决定 | 不自动等于 claim 生效时间 |

查询必须支持 `as_of_valid_time`。同一 claim 的 concurrent revisions 若有效区间完全不重叠，在任何具体 `as_of` 下都不构成当前值冲突；区间重叠才进入 risk reducer。因果后继可以修正过去区间，但必须保留“系统何时知道”的 revision history。Authority capability 永远不读取普通 valid time。

### 7.7 Trust、Risk、Salience 与读取权重

- `trust_tier`: `identity | stable-preference | contextual`，表达治理与保留层；Agent 只能建议，owner 明确决定升级。
- `risk_class`: `action-sensitive | behavioral | informational`，表达错误采用的损害；降级必须单独 `reclassify-risk`，不能夹带在 replace/merge。
- `salience`: `pinned | normal`，表达 owner 是否希望优先召回；“认为重要”只改变 salience，不自动改变 trust/risk/truth。
- `retrieval_weight`、recall count、时间衰减、排序得分和 embedding 只属于可重建读取侧状态，不能写进 Claim Revision 或由 Agent 写入晋升长期地位。
- 旧 `priority` 只可在迁移报告中作为 `suggested_salience`，不能自动变成安全关键或 identity tier。

### 7.8 Context、Consent 与外发边界

- `context.spaces` MUST 非空；`global` 必须显式写出，项目/家庭/健康/关系等 Space 不能因空值互相污染。
- `consent.allowed_purposes` 至少区分 planning、writing、information-answer、external-action、sync、projection、export/share；批准存储不自动授予 export/share。
- `external_provider_policy`: `deny | prompt | allow`。这是 owner/policy engine 的结果，Agent proposal 不得把自己改成 allow。
- v2 没有字段级加密时，`sensitivity: restricted` MUST 拒绝持久化到 Git；不得以 `ui-only` 明文绕过。`private` 默认 provider policy 为 deny 或 prompt。
- 第三方 subject 必须与 owner 具有 `self | direct | shared-context` 关系；`external` 或无关他人不得写入长期 Memory。
- context/consent/policy unknown、withdrawn 或 deny 时，claim 不进入默认 context；withdrawn 触发 forget/purge review，而不是只降低 salience。

### 7.9 完整快照、Lineage 与统一 Revision Envelope

每个 revision 保存一个可独立确认/否认的原子 claim 完整快照，不保存 patch。Agent pending、人工 approve/reject/ignore、replace/revoke/delete 都用不可变 child revision 表达，因此人工直接新增可用一个 owner-authored active revision 原子完成。

- 同 claim 直接因果只用 `parents`；跨 claim merge/split/import/migration 使用 `lineage.derived_from` + complete operation manifest。
- source 是证据，不是 lineage；recorded_by 是捕获者，不是 asserted_by。
- `conflicted/stale/expired/superseded/quarantined` 是 reducer 的 application disposition，不能回写旧 revision。
- 如果未来改用独立 decision objects，必须额外证明人工直接新增、重试和多资产 merge 的原子性；不能依赖内存 request ID。

### 7.10 Canonical Payload Hash

协议冻结前必须发布 schema-specific normalization + RFC 8785 JSON Canonicalization Scheme golden vectors：

1. YAML 只作输入；拒绝重复 key、alias/anchor、自定义 tag、非 UTF-8 和未知关键字段。
2. 解析为严格 typed object；完整 payload 除 `payload_sha256` 与 raw filename hash 外全部纳入语义 hash。
3. Optional 缺失即省略；禁止 absent/null 表达同一语义；schema defaults 在规范化前显式物化。
4. 文本 NFC；CRLF/CR 转 LF；移除每行尾部空白与首尾空行，保留内部换行和有意义前导空格；批准前展示规范化最终值。
5. UUID lowercase；枚举 ASCII lowercase；RFC3339 转 UTC `Z`；禁止浮点、NaN、Infinity 和 `-0`。
6. Set-like arrays 去重并排序：parents 按 ID/hash、asserted_by 按 canonical bytes、evidence 按 relation/resource/hash；语义有序数组保留顺序。
7. JCS UTF-8 bytes 无 BOM、无末尾换行，计算 lowercase SHA-256。
8. 固定 YAML serializer 另算 raw file hash并进入文件名，不写回内容。

Rust 与 TypeScript 必须共享至少一组完整输入 YAML、canonical JCS bytes、expected payload hash，以及 NFC/CRLF/array-order 等价变体和每个语义字段改变的负例。在 golden vectors 发布并双实现通过前，protocol 不能冻结。

---

## 8. Workflow、Lifecycle 与规范 Transition

### 8.1 三层状态

| 维度 | 枚举 | 含义 |
| --- | --- | --- |
| `workflow.state` | pending、approved、rejected、ignored | 建议是否经过何种人工决定 |
| `lifecycle.state` | active、revoked、deleted、merged | 该 revision 试图对 claim 生命周期产生什么效果 |
| `application_state` | current、superseded、quarantined、claim-conflict、expired、stale、invalid | reducer 在完整记录集、as-of 时间、authority 与 context 下计算的实际结果 |

`decision-conflict`、`claim-conflict`、`quarantined`、`expired`、`stale` 都是派生状态，不写回旧 revision。Approved 不等于 current；一个合法 approved revision 与远端 sibling 合并后可以保持 approved，但 application 变为 conflict。

### 8.2 Transition 矩阵

| operation | 合法 actor/capability | 合法 parent/base | 输出 workflow/lifecycle | 参与 current head |
| --- | --- | --- | --- | --- |
| `propose-create` | Agent、可信 human UI | claim 不存在；parents 空 | pending / intended active | 否 |
| `create-approved` | 可信 human UI / `memory.claim.approve` | claim 不存在；parents 空 | approved / active | 是 |
| `propose-replace` | Agent、human | 唯一 current active head | pending / intended active | 否 |
| `approve` | human / `memory.claim.approve` | 精确 pending revision + 其 base 未 stale | approved / pending 所声明状态 | 是 |
| `reject` | human / `memory.claim.approve` | 精确 pending | rejected / 无业务变化 | 否 |
| `ignore` | human / `memory.claim.approve` | 精确 pending | ignored / 无业务变化；保留 dedupe 抑制 | 否 |
| `replace` | human-approved | 唯一 active head | approved / active | 是 |
| `set-salience` | human-approved | active head | approved / 原 lifecycle | 是；只改 salience |
| `reclassify-trust` | human / 专用 capability | active head | approved / 原 lifecycle | 是；不得夹带正文/risk 变化 |
| `reclassify-risk` | human / 专用高敏 capability | active head | approved / 原 lifecycle | 是；降级逐条确认 |
| `change-context-consent` | human-approved | active head | approved / 原 lifecycle | 是；scope 扩大视为高风险 |
| `revoke` | human | active head | approved / revoked | 是，作为关闭 head；不投影 |
| `delete` | human | active/revoked/merged head | approved / deleted | 是，作为 tombstone；不投影 |
| `reinstate` | human | revoked head | approved / active | 是；不能用 replace 绕过 |
| `restore-deleted` | human / 高敏 capability | deleted head | approved / active | 是；UI 明确重新加入 |
| `resolve` | human | 完整 maximal conflict heads | approved / active、revoked 或 deleted | 是 |
| `merge-claims` | human | 所有 participant 的完整 current heads | approved operation：target active + sources merged | 原子应用 |

通用 MUST：

- Agent 只能产生 pending，不能写 human actor、decision、最终 provider allow 或 active lifecycle。
- 人工直接新增和点击确认都是一次 UI 动作；Host 在同一不可变 envelope 中写 exact decision，不叠第二层确认。
- 按钮重试复用持久 `request_id`；相同 key/payload 返回原对象，不同 payload 报 integrity conflict。
- 本机发现 stale base 时不写 approval，也不偷偷 rebase；离线设备各自合法批准后合并出的 sibling 由 reducer 冲突处理。
- 普通 replace 不能恢复 revoked/deleted/merged，不能降 risk/trust，不能扩大 consent/context。
- conflict 上禁止普通 mutation，只允许覆盖全部 current heads 的 resolve。
- rejected/ignored 不改变既有 claim lifecycle。
- No-op 不创建新 revision；幂等请求返回旧 receipt。

### 8.3 Agent 建议边界

Agent MUST 提供：owner relevance、suggested claim kind、subject、asserted_by basis、evidence relation、temporal unknowns、risk、context、dedupe key 和 avoid-error。无法证明的字段显式 unknown，不得猜成 global、owner-stated、timeless、allow 或 high trust。

Agent MUST NOT 记录他人的 task、一次性 daily context、credential/secret、与 owner 无关第三方资料，或把“owner believes P”改写成“P”。

### 8.4 跨 Claim Merge Operation

跨 claim 合并不能靠多次独立 replace/revoke。必须由一个内容寻址 operation envelope 原子列出 inputs、result 与 effects：

```yaml
schema: notemd.memory/operation/v2
operation_id: 019...
operation_kind: merge-claims
run_id: merge/019...
causal_context:
  parents:
    - record_id: 019...
      raw_sha256: ...
merge_inputs:
  target:
    claim_id: 019...
    base_heads:
      - revision_id: 019...
        payload_sha256: ...
  sources:
    - claim_id: 018...
      base_heads:
        - revision_id: 018...
          payload_sha256: ...
result:
  claim_id: 019...
  revision_id: 019...
  payload_sha256: ...
effects:
  - claim_id: 018...
    revision_id: 019...
    payload_sha256: ...
    lifecycle: merged
    merged_into: 019...
lineage:
  derived_from:
    - claim_id: 018...
      revision_id: 018...
      payload_sha256: ...
decision:
  verdict: approve
  actor_id: human:bruce
  authority_context: { ... }
state: complete
payload_sha256: ...
```

- 至少两个互异 `merge_inputs`；单个 merge 只允许一个 USER/MEMORY target scope；`run_id` 必须出现在 result 和每个 source tombstone revision 中。
- 每个 input 必须携带完整 current approved heads；任一 stale，整个 operation 不应用。
- Result 是完整 Claim snapshot；sources 只能原子转为 merged，不能夹带 delete、authority 变化、risk 降级或 consent 扩大。
- Result risk 不得低于任何 source；v2 首版只允许同 risk merge。
- Source 的 evidence、负向约束、structured deny 与 avoid_error 不得丢失。
- operation manifest 最后以 create-new/no-clobber 写入 `state: complete`；缺失 manifest 或任一 result/effect revision 时 reducer 忽略整个 run，并报 partial operation。
- 迟到 source head 未被 merge 因果覆盖时，整个 operation 重新进入 conflict：target result 与全部 merged effects 一起失效/quarantine，不能只恢复一半。
- Split/unmerge 若 v2 未正式定义，必须禁止，不能用 replace 模拟。

### 8.5 删除与 Purge

普通 delete 是不可变 tombstone，只从当前投影/context 移除，Git、legacy、import、migration 和衍生物可能仍保留。UI 不得称“永久删除”。

v2 首版只实现本机只读 `purge-plan`，遍历 claim revisions、descendants、operations、imports、migrations/legacy、Context Manifests、projection、索引/cache、Git commits/tags/reflogs、remote branches 和其他 clone 限制。Plan 留在 `.local/purge/`，避免把待清除正文/hash再写进 Git。真正 history rewrite、crypto-shred 或 purge fence 需要独立 RFC。

---

## 9. Reducer 规范

### 9.1 输入

Reducer 输入是：

- 通过 schema 校验的 bootstrap、全部 protocol/authority/claim revision；
- 完成态 operation、migration、import 与已使用的 Context Manifest；
- 查询参数 `as_of_valid_time`、Space、purpose、caller、provider/model、tools 和 external transfer 意图；
- 不包括根 Markdown；
- 不包括 `.local` 缓存。

Reducer 先计算 protocol 与 authority，再计算 claim。protocol 或 authority 不完整、出现多个未解决 heads、hash/parent 不可验证时，所有写入 fail closed；authority 冲突期间，所有可产生现实行动的 API 关闭。读取可返回诊断和最小安全提示，但不能返回新的授权。

### 9.2 基本算法

对每个 `claim_id`：

1. 收集全部 revision。
2. 校验 revision ID 唯一、payload hash、parent 存在性和 parent hash。
3. 构建有向无环图；出现环时标为 corruption。
4. 校验 transition 的 actor、parent、approval kind、authority revision、protocol revision 和 causal frontier。
5. 聚合 proposal workflow 决定；pending/rejected/ignored 不改变既有 current application。
6. 找出没有已批准有效后继的 lifecycle heads，并按 `as_of_valid_time` 过滤当前有效区间。
7. 区间不重叠的并发 revision 可分别成为不同 `as_of` 的答案；同一 `as_of` 下多个 maximal heads 才进入 semantic conflict。
8. 若唯一有效 head，计算 `workflow_state`、`lifecycle_state`、`application_state`、projection eligibility 和 context eligibility。
9. 若多个 heads 冲突，按 §9.4 的 risk reducer 计算安全覆盖层，绝不按时间戳、文件顺序、Git author 或 commit 顺序选边。
10. 每次都从完整 DAG 重新计算；既有 resolution 只有在因果上覆盖全部当前 maximal heads 时才有效。迟到 sibling 会重新打开冲突。

Reducer 输出至少包含：

```yaml
claim_id: 019...
as_of_valid_time: 2026-09-01T10:00:00Z
workflow_state: approved
lifecycle_state: active
application_state: current
current_heads: [019...]
projection_eligible: true
context_eligible: true
do_not_rely: false
conflict: null
safety_overlay: null
```

`expired`、`future`、`stale`、`superseded`、`conflicted`、`quarantined` 均为 reducer 派生的 application state，不得伪装成历史 revision 中的人类决定。

### 9.3 冲突规则

以下情况必须冲突：

- 同一 active base 被并发批准为两个不同正文；
- approve 与 reject 对同一精确 pending 同时出现；
- active edit 与 delete/revoke 并发；
- owner authority 或 protocol 出现多个未解决 maximal revisions；
- 相同 idempotency key 对应不同 payload；
- 同 revision 路径出现不同字节或不同 payload hash；
- revision 引用不存在或 hash 不匹配的 parent，并且无法证明只是未完成迁移。
- 人工决定绑定的 authority/protocol revision 不存在、无权限，或与撤权无法证明因果先后；
- 同一 `as_of` 下重叠有效区间给出不可兼容的正文、授权或边界；
- 跨 claim operation 没有覆盖全部参与者 heads，或只完成部分 effects。

以下情况可语义幂等：

- 同一 pending、相同 payload、同一 owner、相同 verdict 被重复确认；
- 不同设备写出了规范化 payload 完全相同且 idempotency key 相同的重复记录。

实现 MAY 产生一个确定性 coalesce revision，但首版更安全的做法是仅在 reducer 中折叠为同一语义决定，并保留原始记录。对同一 pending 出现 approve 和 reject 时，reducer 必须聚合全部决定并得到 `decision-conflict`；禁止用 HashMap 覆盖、文件遍历顺序、mtime 或提交顺序选择一个决定。

### 9.4 按风险分类的冲突策略

冲突不能统一处理为“正文从投影消失”。Reducer 必须按风险得到以下确定结果：

| 冲突类型 | 根投影 | 结构化 context | 行动能力 |
| --- | --- | --- | --- |
| informational 判断、普通偏好、身份描述 | 不投影任一冲突正文 | 返回共同祖先、全部 heads、`do_not_rely: true` 和澄清建议 | 不得据此作确定判断 |
| 正向授权、允许项 | 不投影为有效授权 | 返回冲突与 `effective_permission: deny` | 不新增、不恢复权限 |
| 禁止事项、隐私边界、安全约束 | 不暴露冲突正文；输出通用安全提示 | 返回受控的 `safety_overlay`；正文仍按调用方权限最小化 | 执行所有可证明适用的 deny；allow 取交集 |
| owner/authority/protocol | 只输出系统不可行动提示 | 只返回修复诊断 | 关闭全部现实行动和人工决定 API，只允许只读/repair |

Action-sensitive claim 必须使用结构化 `behavior_policy`，不能仅靠自由文本猜测许可：

```yaml
kind_data:
  boundary:
    behavior_policy:
      effect: deny          # deny | allow | prompt
      actions: [send-message]
      resources: [external-recipient]
      conditions: []
```

安全覆盖规则是：同一 action/resource 的 `deny` 取并集，`allow` 取交集；无法确定适用范围时按 deny 处理。边界 weakening、grant 与 revoke 并发时 weakening/grant 不生效。该覆盖层不等于自动解决冲突，仍须人类产生覆盖全部 heads 的 resolution。

根投影不得泄露冲突正文，但在 `USER.md` 的保留分类 `## 安全状态` 中输出确定性提示：

> 存在未解决的权限或边界冲突，相关外部行动已暂停。

仅读取根投影的 Agent 只能进行信息性回答，不能据此执行外部行动。可行动 Agent 必须调用 reducer-backed context，提交 caller、Space、purpose、provider/model 与 tools，并验证 `action_allowed=true` 且不存在相关 action-sensitive conflict。插件和 Agent 会话启动检查必须醒目暴露未解决冲突数量，不能只写日志。

### 9.5 冲突解决

普通 claim 的人工解决可选择：

- 保留 A；
- 保留 B；
- 合并编辑；
- 全部撤销。

解决结果必须新增 revision：

```yaml
parents:
  - revision_id: <head-a>
    payload_sha256: ...
  - revision_id: <head-b>
    payload_sha256: ...
transition:
  operation: resolve
```

只有完整引用所有待解决 heads、使用与 claim kind/risk 匹配的 approval kind、且由当前 authority 授权的人类 revision 才能关闭冲突。不得修改或删除 A/B。降低 risk/trust、放宽 deny 或新增 behavioral authorization 必须在精确 diff 上单独确认，不能搭便车继承普通正文确认。

冲突解决不是永久终局。如果之后同步进来一个未被 resolution 因果覆盖的迟到离线 head，reducer 必须重新打开冲突。`conflict_id` 应由排序后的 head revision ID 与 payload hash 集合确定性派生，使所有设备得到同一冲突身份。

### 9.6 重复事实

两台设备可能以不同 `claim_id` 创建语义相同主张。系统可根据 `dedupe_key` 和文本相似度提出合并建议，但不得自动把两个 claim 当成同一因果对象。

人工合并必须使用 §8.4 的原子 `merge-claims` operation，创建目标 revision，并显式产生来源 claim 的 merged tombstone，避免双重投影。正文相同但 subject、claim kind、valid time、approval kind 或 context scope 不同的记录不得合并。

---

## 10. 纯文本投影契约

### 10.1 文件内容

`USER.md` 示例：

```markdown
# USER

## 偏好

- 用户希望回答先给出结论。
  不确定的内容需要明确标注并核验。

- 用户主要使用中文交流。

## 边界

- 未经明确要求，不应代表用户对外发送消息。
```

`MEMORY.md` 示例：

```markdown
# MEMORY

## 已确认决定

- Memory 的权威事实源位于 `.notemd/memory/`。

## 约束

- Agent 只能提议与 Vault owner 直接相关的长期事实。
```

### 10.2 格式约束

- 文件 MUST 无 YAML frontmatter。
- 文件 MUST 无 inline property、ID、revision、status、priority、SHA、source、approval、引用备注或脚注。
- 文档只允许一个 H1 和一层 H2 分类。
- 不允许 H3 或嵌套分类。
- 每条事实使用 `- ` 开头。
- 多行正文的续行使用两个空格缩进。
- 条目之间保留一个空行。
- 统一 UTF-8、LF 和一个末尾换行。
- 仅输出 `workflow_state=approved`、`application_state=current`、`lifecycle_state=active`、`visibility=projection`，且当前 Space/purpose 允许 projection 的主张。
- informational 冲突正文不投影；action-sensitive 冲突按 §9.4 只输出通用安全提示。
- `trusted-agent` 与 `ui-only` 主张只通过受权限控制的 context/UI 读取，绝不进入根文件。
- 不支持嵌套列表、checkbox、表格、引用块或代码围栏。
- 事实续行若以 `#`、`-`、`+`、`*`、`>`、反引号、`---` 或其他 CommonMark 控制序列开头，projector 必须转义，保证它仍属于同一 bullet；importer 必须无损反向处理该转义。

### 10.3 确定性排序

排序规则固定为：

1. current protocol revision 中的目标和分类顺序；
2. `salience`：pinned、normal；
3. `claim_kind` 的 protocol 固定顺序；
4. `claim_id` 字节序。

时间戳不得成为冲突裁决条件。使用 `claim_id` 作为最终显示排序只是为了确保确定性。

写入 canonical claim 前，文本必须按 Unicode NFC 规范化并向用户展示规范化后的最终值；跨平台 projector 不使用 locale-sensitive 排序。trust、risk、certainty、recall count、访问时间和 retrieval weight 均不得偷偷改变显示顺序。

### 10.4 搜索与 Agent 阅读

- `USER.md` 与 `MEMORY.md` 继续纳入 Git，使普通编辑器和外部 Agent 在没有 Memory API 时仍能阅读；它们始终是可再生派生文件。
- 搜索索引继续索引根 Markdown，以便人和普通 Agent 搜到简洁事实文本。
- `.notemd` 默认不进入普通内容索引，避免内部元数据污染召回。
- 搜索分类必须对 `USER.md` 和 `MEMORY.md` 使用路径特例，不因缺少通用 Markdown frontmatter 而标为损坏或未分类。
- Agent 普通对话可读投影，但该路径只授权信息性回答；需要来源、确定度、有效时间、负向约束或任何现实行动时必须使用 `notemd memory context --json`。
- `agent_use.guidance`、`avoid_error`、safety overlay、`do_not_rely` 和适用范围必须在结构化 context API 中与主张一起返回，不能拆散。

### 10.5 Context Request 与 Context Manifest

每次受控 context 选择都必须显式提交：

```yaml
space: work/hemory
purpose: planning
caller: agent:codex
provider: openai
model: gpt-5
tools: [notemd-search]
external_transfer: true
as_of_valid_time: 2026-09-01T10:00:00Z
```

缺少 Space 或 purpose 时，默认只返回无敏感性的全局 identity 与明确允许该目的的条目；不得扫描并拼接所有 Space。`external_transfer: true` 时逐条执行 `external_provider_policy`：deny 排除，prompt 在人类同意前排除，allow 才可选择。

每个实际使用 context 的 run 产生一个内容寻址、不可变的最小 Context Manifest：

```yaml
schema: notemd.memory.context-manifest.v2
manifest_id: 019...
request:
  space: work/hemory
  purpose: planning
  caller: agent:codex
  provider: openai
  model: gpt-5
  tools: [notemd-search]
  external_transfer: true
as_of_valid_time: 2026-09-01T10:00:00Z
selected:
  - claim_id: 019...
    revision_id: 019...
    payload_sha256: ...
    reasons: [space-match, purpose-allowed, current-at-as-of]
excluded_summary:
  provider-deny: 2
  wrong-space: 5
conflicts:
  - conflict_id: sha256:...
    action_allowed: false
policy_result:
  external_action_allowed: false
```

Manifest 不复制 claim 正文、prompt、模型响应、绝对路径或 secret，只记录“选择了什么、为何选择、发给谁、用来做什么、使用哪些工具、哪些策略阻止了什么”。仅 preview、被取消或未实际使用的选择留在 `.local`；实际外发或供 action-capable Agent 使用的 manifest 进入 Git。Manifest 自身也必须携带 protocol/authority refs 与 causal context，并接受 restricted 数据禁止规则。

---

## 11. 根 Markdown 的手工编辑

根 Markdown 是计算结果，但产品只应表述为“不建议直接编辑”，不能悄悄覆盖用户输入。

### 11.1 drift 检测

每次打开插件、执行写操作、Git merge 后或显式 `check` 时：

1. 从 YAML 生成 expected projection。
2. 与磁盘 actual projection 比较。
3. 相同则健康。
4. 不同则创建可恢复的 import/drift 诊断，不把 actual 当成权威事实。

如果 actual 只是 Git 对两份已知 renderer 输出产生的冲突，并且合并后 YAML 集合通过完整性校验，可直接按最终 YAML 重建，不把 conflict markers 或整份派生差异误生成几十条人工候选。只有无法证明为纯派生差异的外部正文修改才进入 import 流程。

### 11.2 可解析修改

如果 actual 仍符合“一层分类 + 多行 bullet”格式，UI 显示预览：

- 新增 bullet → create import proposal；
- 修改 bullet → replace import proposal；
- 删除 bullet → revoke proposal，绝不解释为 purge；
- 分类移动 → category-change proposal；
- 仅调整顺序 → 默认恢复确定性顺序，不改变事实。

外部文件编辑本身不构成人工认证，因为 Agent 也能修改文件。导入资产先形成 pending；用户在可信 Memory UI 查看精确 diff 并点击一次确认后，才产生 active/revoked 权威 revision。两个相同文本或无法唯一映射的条目必须标为 ambiguous，不猜测 target。

### 11.3 不可解析修改

将完整原文、expected 和 diff 写入仅本机目录：

```text
.notemd/memory/.local/imports/<import-id>/
```

Git 跟踪的 `.notemd/memory/imports/<import-id>.<sha>.yaml` 只保存规范化 proposal、actual/expected raw hash、来源投影路径、lineage、映射置信度和批准结果，不复制整份根文档或敏感上下文。本机诊断时间、绝对路径与环境信息只进入 `.local`。

然后提供：

- 导入为候选；
- 下载/查看差异；
- 按事实源重新生成。

不得在保存备份前覆盖 actual。投影 drift 不应让已有 YAML 事实不可读，但在未处理时应阻止会覆盖用户修改的自动重建。

---

## 12. 本机写入与崩溃一致性

### 12.1 共享仓库锁

Memory 写操作与 Vault Git staging/sync 必须共用同一个本地仓库写锁。仅锁 Memory 自己而不锁 Git staging，仍可能让 `git add -A` 捕获半成品。

锁必须以 canonical Git common directory + `vault_id` 为键，存放于 Git common dir 或应用本机数据目录，不进入 Git。不能只把锁放在某个 worktree 内，否则同一仓库的多个 worktree 无法互斥。锁只保证本机受控进程互斥，不表达跨设备状态；网络文件系统无法提供可靠锁时应拒绝同一 Vault 多实例写入。

### 12.2 单 revision 写入

1. 获取锁。
2. 检查仓库是否处于 unresolved merge/rebase 状态；若是则拒绝新写入。
3. reload + validate 当前记录图。
4. 校验 expected heads 与 payload hash。
5. 在 `.local/tmp/` 写完整文件。
6. flush/fsync 文件。
7. 使用 no-clobber 原子 rename 发布到最终唯一路径。
8. fsync 父目录。
9. reducer 重算。
10. 原子替换该 revision 所属 target 的根投影；单个普通 claim 操作禁止同时改变 USER 与 MEMORY 两个 target。
11. 更新可删除的 `.local/projection-generation.yaml`，记录 record-set digest、renderer version 和两个 expected projection hash。
12. 释放锁。

若最终路径已存在：

- 内容语义相同且 idempotency key 相同：返回既有结果；
- 内容不同：完整性冲突，禁止覆盖。

### 12.3 多资产操作

迁移、一次手工批量导入或跨 claim 合并可能生成多条 revision。此类操作必须使用 operation/migration/import manifest：

- 所有子记录携带同一 `run_id`；
- manifest 在最后写入 `state: complete`；
- reducer 忽略没有 complete manifest 的 staged run；
- 重试按 source hash 和 run ID 幂等补齐；
- 不通过回写旧 YAML 标记完成。

跨 claim merge 不允许先激活目标 claim、再逐条 tombstone 来源。所有 effects 必须由同一 complete manifest 原子激活；在任意写点崩溃时，reducer 要么应用全部 effects，要么一个也不应用。

### 12.4 投影不是提交标记

YAML 已发布但投影未刷新时，Claim 仍然有效；下次启动或 sync 前自动重建。

投影已写但 YAML 未发布不应发生，因为实现顺序要求来源先落盘。若磁盘故障仍造成该状态，下次重建必须移除虚假投影内容。

根 Markdown 内严格不放 generation metadata。`.local/projection-generation.yaml` 只是新鲜度缓存，不是事实源，可删除重建，也不进入 Git。任意外部进程直接读取两个根文件时，协议不承诺跨文件原子 snapshot；需要原子一致视图的调用方必须使用 reducer-backed `memory context/snapshot` API。

---

## 13. Git 多设备同步协议

### 13.1 基本模型

- Git：传输、提交历史、远端协作、恢复。
- Memory DAG：Claim、protocol 与 authority revision 的语义因果。
- 本地锁：单设备进程互斥。

三者职责不可互换。Git commit hash 不写入 canonical payload，避免记录文件必须预知包含它的 commit 并形成循环依赖。

### 13.2 同步顺序

1. 获取共享仓库写锁。
2. 修复或重建任何派生投影 drift。
3. 提交本地完整 Memory 资产与投影。
4. fetch 远端。
5. merge 远端，不 reset、不 force、不丢弃本地记录。
6. 校验所有 v2 YAML、hash、actor、DAG 和 manifests。
7. 对合并后的全集执行 reducer。
8. 重建 `USER.md` 和 `MEMORY.md`。
9. 如果产生投影变化或领域冲突清单，创建 reconciliation commit。
10. push。
11. 若 push non-fast-forward，进入有界 fetch/merge/validate/reduce/rebuild/push 重试；耗尽后报告“本地已提交，尚未上传”，不得报告同步成功。

每次受控 sync 开始和结束都必须检查 worktree、index 与 HEAD 是否被不遵守锁的外部 Git 客户端改变；检测到意外变化时重试或停止，不能假装获得了全局事务隔离。每次 merge 后必须重新 reduce，绝不能复用 merge 前的 head 集。

### 13.3 文件级冲突分类

| 冲突对象 | 处理方式 |
| --- | --- |
| `USER.md` / `MEMORY.md` | 不选 ours/theirs；校验 YAML 后重建 |
| 新增的不同 revision YAML | Git 自然取并集 |
| 同路径 revision 内容不同 | fail closed；视为 ID 碰撞、篡改或 bug |
| revision delete/modify | fail closed；删除必须用 tombstone |
| 新增的 protocol/authority revision | Git 取并集；reducer 若得到多 head，按 control-plane conflict fail closed |
| bootstrap 或兼容 sentinel 不同修改 | fail closed；bootstrap 不可变，sentinel 只能由受控迁移生成 |
| `.local/diagnostics/**` | 不进入 Git；只作本机诊断，reducer 永不扫描为权威资产 |
| 普通 Vault 文档 | 继续使用既有通用冲突策略 |

Memory 源资产出现 integrity failure 时不得创建或 push 一个“选择 ours 后看似健康”的 merge commit。应保留双方诊断、停止同步并要求 repair。合法领域冲突则由 reducer 确定性产生相同降级视图，可以正常提交和同步。

### 13.4 不使用 Git merge driver 作为唯一正确性机制

可以提供 `.gitattributes` 固定 LF，但不应依赖每台设备都安装自定义 merge driver。移动端、第三方 Git 客户端或手工同步可能不执行 driver。

`notemd memory reconcile` 必须能够在任何普通 Git merge 之后恢复派生投影并发现 YAML 源冲突。

---

## 14. 信任、安全与隐私

### 14.1 信任边界

| 主体 | 权限 |
| --- | --- |
| Vault owner 的可信 UI | 新增、编辑、批准、否认、忽略、撤销、删除、解决冲突 |
| 人工 CLI | 同上，但必须明确人工模式且不能被 Agent 透传 |
| Agent | 读取投影和 context；创建 pending 建议 |
| Git author | 只作为版本历史信息，不等同人工审批 |
| 外部编辑器 | 可编辑文件，但不能自动获得 human approval 语义 |

`actor_id: human:*` 不是密码学签名。基础 v2 防的是误操作、重复请求、陈旧写入、进程崩溃和多设备并发；它不抵御拥有仓库任意写权限、且能同时重写内容与 hash 的恶意参与者。产品文案只能声称“通过受信任的本地 note.md 会话批准”，不能在未实现设备签名前声称密码学意义上的“只有 owner 能批准”。

如果外部安全评审要求强身份保证，应在后续协议加入 owner 授权设备公钥和 decision 签名：私钥存在 OS Keychain、不进 Git；签名覆盖 revision hash、action、authority epoch、owner ID 与 device ID。首版是否强制签名列为 P0 开放裁决。

### 14.2 RPC 边界

- 官方 Memory 插件使用受限、版本化的 `host.memory.v2.*` RPC。
- Host 必须继续校验官方 plugin ID/capability。
- `approve/reject/resolve/delete` 必须来自可信窗口、绑定精确 revision hash 的短时单用途 gesture token；Host 从调用上下文注入 caller principal、capabilities、owner session 和 device ID。
- Agent harness 不得获得可生成该 token 的接口。
- `--yes`、`human_confirmed: true` 等可由 Agent随意拼出的布尔值不能单独构成人工授权。

### 14.3 owner relevance

Agent 提议前必须回答：“这是否是 Vault owner 本人需要跨会话保存的事实、偏好、边界、决定或避免错误指引？”

以下内容禁止写入：

- 其他人的待办或义务；
- 归属不明的团队任务；
- 仅仅与 owner 有关、但行动主体不是 owner 的事项；
- 无法区分事实与推断的陈述；
- 短期 inbox task；
- 凭据、token、私钥、恢复码。

### 14.4 敏感信息

`.notemd/memory` 是普通 YAML，会进入 Git，并不加密。隐藏目录不是保密边界。

- `sensitivity: private` 只能在 UI 明确提示后由 owner 批准。
- `sensitivity: restricted` 在 v2 首版一律拒绝持久化到 Git，也不得以 `visibility: ui-only` 绕过；只可在不落盘的当前会话中使用，或等待未来经独立安全评审的字段级加密协议。
- secret 级内容必须拒绝持久化。
- 只有 `visibility: projection` 进入根 Markdown；`trusted-agent` 仅进入有相应 capability 的结构化 context；`ui-only` 只在可信 UI 展示。
- source path 也可能泄露敏感上下文；UI 应在批准前显示来源。
- 导出诊断包默认应脱敏 actor、绝对路径和正文，除非用户明确选择完整导出。

### 14.5 YAML 安全解析

解析器必须：

- 限制单文件与总扫描大小；
- 禁止或限制 YAML alias/anchor 展开；
- 拒绝自定义 tag；
- 防止路径穿越和 symlink 越界；
- 使用严格 schema，未知关键字段 fail closed；
- 对未来 schema 只读提示升级，不按旧规则写回。

---

## 15. Host API、CLI 与 Agent 契约

### 15.1 建议 Host RPC

读操作：

```text
host.memory.v2.snapshot
host.memory.v2.list
host.memory.v2.show
host.memory.v2.context
host.memory.v2.contextManifest
host.memory.v2.pending
host.memory.v2.conflicts
host.memory.v2.check
host.memory.v2.owner
```

写操作：

```text
host.memory.v2.propose
host.memory.v2.add
host.memory.v2.approve
host.memory.v2.reject
host.memory.v2.ignore
host.memory.v2.revoke
host.memory.v2.delete
host.memory.v2.resolve
host.memory.v2.mergeClaims
host.memory.v2.setSalience
host.memory.v2.reclassify
host.memory.v2.changeContextConsent
host.memory.v2.importProjection
host.memory.v2.rebuild
host.memory.v2.migrate
```

写响应必须包含：

- `claim_id`
- `revision_id`
- `payload_sha256`
- 当前 effective status
- 是否产生 conflict
- projection 是否成功重建
- 可机器处理的 error code

公共请求包含 `request_id`、expected protocol revision/hash；target mutation 还必须包含 expected head ID/hash。`context` 必须包含 §10.5 的 request 字段，并返回 manifest ID、selected reasons、conflicts、redactions 与 action decision。授权信息不得来自请求 body。建议 capabilities：`memory.read`、`memory.propose`、`memory.human-decide`、`memory.maintain`、`memory.purge`；Agent 永远没有 `human-decide` 或 `purge`。

### 15.2 CLI

```text
notemd memory owner --json
notemd memory list [--scope ...] [--status ...] [--json]
notemd memory show <claim-or-revision-id> [--json]
notemd memory context --space ... --purpose ... --caller ... [--provider ...] [--model ...] [--tool ...] [--external-transfer] [--as-of ...] [--json]
notemd memory context-manifest <manifest-id> [--json]
notemd memory pending [--json]
notemd memory conflicts [--json]
notemd memory propose create|replace|revoke ...
notemd memory add ...
notemd memory approve <revision-id> --expected-sha256 ...
notemd memory reject <revision-id> --expected-sha256 ...
notemd memory ignore <revision-id> --expected-sha256 ...
notemd memory revoke <claim-id> --base-head ...
notemd memory delete <claim-id> --base-head ...
notemd memory resolve <claim-id> --heads ...
notemd memory merge-claims --operation ...
notemd memory set-salience <claim-id> pinned|normal --base-head ...
notemd memory reclassify <claim-id> --trust ... --risk ... --base-head ...
notemd memory rebuild
notemd memory reconcile
notemd memory check [--json]
notemd memory doctor [--json]
notemd memory purge-plan <claim-id> [--json]
notemd memory migrate --dry-run
notemd memory migrate --apply <migration-id>
```

CLI 要求：

- JSON 输出 schema 稳定。
- 退出码稳定：0 成功、1 无匹配/无待办、2 输入错误、3 完整性错误、4 并发/stale base、5 协议不兼容。
- `propose` 可供 Agent 使用。
- 首版建议 CLI 只允许 read/propose/maintenance；human approval 仅由可信 UI 执行。若必须支持人工 CLI，需要真实 TTY 展示精确 diff + SHA，并另行设计不可被 Agent 透传的 challenge/gesture 机制；不能只靠 `--yes` 或可伪造布尔 flag。
- stale base 必须返回当前 heads，不得自动 rebase 并继续批准。

### 15.3 稳定错误码

| 错误码 | 含义 | 是否可自动重试 |
| --- | --- | --- |
| `MEMORY_STALE_BASE` | expected heads 已变化 | 否；必须重新展示 diff |
| `MEMORY_REVISION_HASH_CHANGED` | 待处理 revision 内容与用户看到的不一致 | 否 |
| `MEMORY_UNAUTHORIZED` | caller 缺少 capability/gesture/authority | 否 |
| `MEMORY_CONCURRENT_HEADS` | claim 存在多个有效 heads | 否；进入 resolve |
| `MEMORY_AUTHORITY_CONFLICT` | authority 或 protocol revision 冲突 | 否；阻断人工决定与现实行动 |
| `MEMORY_CONTEXT_INCOMPLETE` | context 缺 Space、purpose、caller 或 provider 信息 | 否；补齐明确场景 |
| `MEMORY_EXTERNAL_TRANSFER_DENIED` | claim policy 禁止或尚未同意外发 | 否；排除条目或获取可信 UI 同意 |
| `MEMORY_RESTRICTED_PERSISTENCE_DENIED` | 尝试把 restricted 明文写入 Git | 否 |
| `MEMORY_TAMPERED_ASSET` | 不可变路径、raw hash 或 payload hash 异常 | 否；doctor/repair |
| `MEMORY_PROJECTION_EDITED` | 根投影存在未导入外部修改 | 否；preview/import/rebuild |
| `MEMORY_PARTIAL_OPERATION` | migration/import/merge 缺 activation marker | 可通过 resume/rollback |
| `MEMORY_PROTOCOL_UNSUPPORTED` | 未知 major protocol/schema | 否；升级 Host |
| `MEMORY_LEGACY_AFTER_CUTOVER` | cutover 后同步到 v1 写入 | 否；显式导入 |
| `MEMORY_GIT_IN_PROGRESS` | 仓库存在未完成 merge/rebase | 否；先恢复 Git |
| `MEMORY_PUSH_PENDING` | 本地已提交但远端尚未接受 | 是；继续同步 |

本地化文案由 UI 处理，RPC/CLI 的机器码保持稳定。

### 15.4 AGENTS 契约

AGENTS 模板需要说明：

- 会话开始时读取 `USER.md` 和 `MEMORY.md` 作为简洁上下文。
- 两文件是计算投影，不建议手工编辑。
- 机器读取 owner、确定度、有效时间、来源和冲突时使用 `notemd memory ... --json`。
- Agent 只能调用 propose，不得自批。
- 只提议与 owner 直接相关的长期事实。
- task 仍写 `inbox/tasks/`，不能把 task 混入 Memory。
- 所有负向限制、低确定度和 contested 状态必须随 context 一起传递，不能只摘录正文。
- 只读 `USER.md` / `MEMORY.md` 不能授权现实行动；可行动 Agent 必须提交明确 Space/purpose/provider/tools 的 context request，并遵守 manifest 中的 safety overlay。

---

## 16. 插件产品与交互

### 16.1 信息架构

Memory 插件分为三个主区：

1. **已确认**：当前 active claims，按 USER/MEMORY 与一级分类分组。
2. **待确认**：Agent pending 建议。
3. **冲突与历史**：并发冲突、撤销、删除、导入和维护诊断。

### 16.2 当前主张列表

列表行默认只显示：

- 多行正文摘要；
- pinned 标识；
- 正向/负向/中性标识；
- 必要的“关于谁”“这是什么主张”和确定度提示。

来源、SHA、asserted/recorded/approved actor、valid time、risk/trust、consent、revision 与 Agent guidance 放在详情中，不进入根 Markdown，也不堆在主列表。

### 16.3 人工添加

默认表单保持最少字段：

- 分类；
- 多行主张正文。

系统先根据目标分类给出 `claim_kind`、subject 和 approval kind 的安全默认建议；若涉及外部事实、behavioral authorization、边界放宽、第三方 subject、valid time 或跨 Space/provider，保存前只追问对应的一个必要选择。高级选项折叠展示 asserted_by、有效时间、trust、risk、salience、context/consent、epistemic、guidance、avoid-error 和 sensitivity。

可信 UI 中 owner 手工新增，点击保存即创建 `create-approved` revision；精确确认页本身就是批准，不再出现第二步。保存失败时保留草稿；成功但投影刷新失败时应明确说明“主张已保存，投影等待重建”，不能误报为全部失败。

### 16.4 pending 快捷动作

每条 pending 默认只提供：

- 确认；
- 否认；
- 认为重要；
- 可以忽略；
- 删除候选。

其中确认/重要/忽略均为一次点击完成对应人类 revision，不再先变 active 再要求额外批准。“确认”必须显示其批准含义：默认是 self-representation；若提议要求 factual-verification 或 behavioral-authorization，按钮文案改为“确认事实正确”或“允许此行为”。“认为重要”只写 `salience: pinned`，不得继承 Agent 建议的高 trust/risk。否认和删除可保留一次语义清晰的 sheet，但不能依赖 `window.confirm`。

### 16.5 冲突解决

冲突卡展示：

- 最后共同祖先；
- 设备 A 的版本；
- 设备 B 的版本；
- 来源、时间和审批 actor；
- 保留 A、保留 B、合并编辑、全部撤销。

解决前禁止继续基于某一冲突 head 做普通编辑。

### 16.6 Apple 风格与可访问性

- 所有按钮使用真实 button 语义。
- 最小点击目标符合 macOS/iOS 可用性要求。
- 正文字号、辅助文字和元数据层级统一，不在同一卡片混用不一致字号。
- 颜色不能成为状态的唯一表达。
- 全键盘可操作，焦点环清晰。
- VoiceOver 能读出主张正文、状态和动作结果。
- 菜单复用全局 `.menu-panel` / `.menu-row` 样式。
- 宿主插件窗口继续支持 `accept_first_mouse(true)`。
- 一次人工点击必须由测试证明只产生一次写 RPC。

---

## 17. v1 → v2 迁移

### 17.1 原则

- 先盘点，后迁移。
- 先写新资产，后切权威协议。
- 不明确的审批不升级。
- 迁移可重复执行且结果相同。
- 不长期双写 v1/v2。
- 旧原文和 hash 必须可审计。

### 17.2 Dry-run

`notemd memory migrate --dry-run` 必须检查：

- Git 工作树和 merge 状态；
- 当前 `state.json` 与根投影 drift；
- 重复 entry/proposal/event ID；
- candidate raw SHA；
- event 引用存在性；
- 同 proposal 多个决定；
- owner actor 合法性；
- 根投影条目与 approved event 的对应关系；
- 分类、旧 priority/polarity/epistemic/certainty 的字段来源与缺口；
- claim kind、subject、asserted_by、recorded_by、approval kind、valid time、risk/trust/salience、context/consent 的逐字段映射依据；
- v2 claim/revision ID 的确定性映射；
- 预期新投影与旧投影的语义 diff。

Dry-run 必须真正零写入：不得在 Vault 创建 lock、目录、临时文件或日志，也不得修改 Git index。它通过 stdout/UI 内存结果输出排序稳定的 source manifest、每个 path/size/SHA、`source_manifest_sha256`、目标 path/hash、projection preview、warnings、blockers、确定性 `migration_id` 与 `plan_sha256`。`now()`、UUIDv4 和设备时间不得参与 plan identity。

### 17.3 确定性映射

- 旧 entry ID 可直接映射为稳定 claim ID；不符合 UUID 时使用基于旧路径+ID 的确定性 UUIDv5。
- 旧 candidate 映射为 pending revision。
- 有唯一合法 approved event 且根条目一致时，只能证明 `workflow_state=approved`；能否成为 `application_state=current` 还必须通过下面的语义字段门禁。
- reject event 映射为 rejected child revision。
- event 与根投影不一致时进入 quarantine，不猜测哪一侧正确。
- 仅存在根条目、无法证明人工审批时默认 pending；旧协议能通过 `approved-by`、event SHA 和 owner actor 三方证明时，也只继承“曾被批准”，不能推导 claim kind、外部真值、有效时间或行为授权。
- 旧全局 revision 只保留在 migration metadata，不进入 v2 因果语义。
- 旧 source、guidance、avoid-error 和分类逐项保留；category 不是 claim kind，`priority` 只能形成 `suggested_salience`，不得自动提升 trust/risk。
- 每个 v1 candidate 必须同时保留 `legacy.path`、v1 raw bytes SHA-256 与新的 semantic SHA；历史 event 先验证 raw hash，新 v2 决定使用 semantic hash。
- 每个 v1 event 的 event ID、action、proposal ID、raw SHA、entry ID、prior/current revision、actor、time、reason 和 legacy path 必须进入对应 child revision 的 `transition.legacy` 证据。
- 同 proposal 多个同向同 actor event 可以保留并告警；approve/reject 或 actor 冲突必须阻断，不能按时间选边。
- v1 owner 只有在 referenced proposal、matching approve event、owner actor 三方链有效时才能 active；缺链 owner 降为 pending/unknown。
- maintenance prose、document sources、普通 bullet 不得被误造为事实，只迁移 v1 managed blocks 与 owner。

每个迁移字段都必须带来源和推导依据：

```yaml
migration_semantics:
  claim_kind:
    value: legacy-unclassified
    provenance: v1.category
    basis: category-is-not-claim-kind
    confidence: insufficient
  subject:
    value: unknown
    provenance: null
    basis: v1.scope-does-not-prove-subject
    confidence: insufficient
  asserted_by:
    value: unknown
    provenance: null
    basis: producer-and-approver-are-not-assertor
    confidence: insufficient
  temporal.valid_from:
    value: null
    provenance: null
    basis: created-at-is-record-time-only
    confidence: insufficient
```

规范推断边界：

- `scope: user-owner` 或明确 user-profile managed block 可支持 `subject: human:<owner>`；普通 `memory` scope 不支持。
- proposal producer、文件 author、event actor 和 approver 都不能自动成为 `asserted_by`。
- `created_at`、event time、Git commit time 只能保留为 recorded/decision time，不能成为 `valid_from`、`observed_at` 或 `uttered_at`。
- source URL/文档只证明 evidence provenance；除非旧记录明确区分，否则不能标为 evidence-of-truth。
- 未能证明 claim kind、subject 与 approval meaning 的旧 approved 条目，保存为 `approved + legacy-unclassified + quarantined`：审批历史不丢，但不进入投影、默认 context 或行为授权。

截至 2026-09-01 的 sotvault 只读基线包含 34 个现存条目（15 active、19 pending）。迁移预分类必须至少得到：

- 19 个 pending 继续 pending，不因当前出现在候选库而升级；
- 11 个 active 的 epistemic/certainty 为 unknown/unknown，保存 approved history，但 application quarantined；
- 3 个 owner-stated/high 仍需 owner 补认 claim kind、approval meaning 与有效时间，不能仅凭 certainty 进入投影；
- 只有明确的 owner activation/identity 链在 authority、subject 和 approval meaning 都可证明时才可自动 eligible。

这些数量只是当前评审快照。Apply 必须重新扫描并在 plan 中输出每条 disposition；数量变化导致 plan hash 变化。迁移 UI 必须让 owner 用批量建议 + 单条必要修正完成语义复核，禁止用一个“全部确认”把 `legacy-unclassified` 批量升级为 factual verification 或 behavioral authorization。

### 17.4 Apply

1. 要求 `expected-plan-sha256`，获取共享锁后重新扫描；任何 source byte/path/count 变化都以 stale-plan 中止。
2. 在 `.notemd/memory/staging/<run-id>/` 创建确定性 staged bundle；新旧 reducer 均忽略未激活 staging。
3. 在 Git 跟踪 migration YAML 中保存旧路径、raw SHA、semantic mapping 和计数；旧 `USER.md` / `MEMORY.md` 精确全文及大体积 actual/expected 恢复副本默认只放 `.local` 或用户选择的加密备份。
4. 写 migration intent。
5. **原子替换旧 `state.json` 为 protocol 2 write-fence tombstone**。这是旧 Host 的写栅栏；绝不能简单删除 state，否则旧 Host 可能把 Vault 当成 unmanaged 并继续写 v1。
6. 使用 create-new/no-clobber 发布 bootstrap、protocol/authority revisions、claim revisions、operations 和 migration mapping；兼容 `protocol.yaml` 只能是 write fence/派生摘要。
7. 对 v2 集合执行 reducer 和投影，逐条比对。
8. 将旧 candidate/event 和原 state 证据通过 Git move 收拢到 `legacy/v1/<run-id>/`；write-fence `state.json` 保留在原控制路径。
9. 生成新的纯文本 `USER.md` / `MEMORY.md`。
10. **最后**写 immutable activation/complete manifest，列出全部资产与 projection hash；没有 activation marker 时，新 Host 只能 recovery/read-only。
11. 形成单一迁移 commit。

即使外部 Git sync 在任意中间阶段捕获并提交了部分文件，write-fence 与 activation-last 也必须让所有客户端识别为不可写的 incomplete migration，而不是半激活状态。

### 17.5 幂等

- 相同 source hashes 和 migration ID 重跑必须返回同一结果。
- 已存在且 hash 相同的 revision 视为成功。
- 已存在但内容不同的同 ID revision 必须停止。
- staged 但无 complete manifest 的 run 不参与 current state，可由 doctor 补齐或隔离。
- apply 重跑必须沿用同一 migration ID 并继续/验证，不生成第二套随机 ID、时间戳或 claim。

### 17.6 回滚

- complete 前可删除或隔离 staged run，并逐字节恢复旧文件。
- complete 后、没有任何新 v2 人工/Agent revision 时，可执行自动 rollback。
- 已有新 v2 写入后禁止静默 rollback，以免丢失新记忆；只允许 Git revert、显式 export-v1 或人工迁移。
- rollback 必须验证恢复后的旧文件 SHA 与 migration bundle 一致。
- rollback 也必须提供 dry-run、plan hash 与 expected-plan 绑定；Git revert 是附加恢复手段，不是唯一备份保证。

---

## 18. 协议兼容与旧版本门禁

### 18.1 Host / Plugin 版本

Memory 2.0.0 依赖新增 Host v2 reducer、Git reconcile 和 RPC，因此发布顺序必须是：

1. 发布具备 protocol v2 能力的 Host；
2. 验证升级与旧插件只读行为；
3. 发布 Memory Plugin 2.0.0，并将 `min_host_version` 指向确切 Host 版本；
4. 再开放 Vault v1 → v2 迁移。

### 18.2 旧 Host 保护

旧 Host 看到 `.notemd/memory/bootstrap.yaml` 或兼容 `.notemd/memory/protocol.yaml` sentinel 的 protocol 2 时必须只读并提示升级。若现有 v1 会把缺少 `state.json` 解释为 unmanaged 并重新写入 v1，则过渡期需要一个明确的 v2 sentinel，使旧实现 fail closed。

该 sentinel 可临时使用 `.notemd/memory/state.json`，内容只声明 protocol 2 和不可写原因；它不是 v2 事实源。所有支持 v2 的版本发布稳定后再移除该兼容例外。

迁移 watermark 之后若离线旧设备同步进新的 v1 candidate/event，新 Host 必须报告 `legacy-after-cutover` 冲突并要求显式导入；不得忽略，也不得自动升级为 active。

### 18.3 不允许长期双写

迁移期间可以 shadow read/compare，但不能长期让 v1 Markdown 状态和 v2 YAML 同时接受写入。cutover 后：

- v2 是唯一权威；
- v1 reader 只用于迁移和诊断；
- v1 writer 必须关闭。

---

## 19. 完整性检查、可观测性与恢复

### 19.1 `memory check`

至少检查：

- protocol/schema 兼容；
- bootstrap、current protocol revision 与 current authority revision 可唯一归约；
- YAML 大小、语法和字段；
- revision 路径与内部 ID 一致；
- 文件名 raw hash 与实际文件字节一致；
- payload hash；
- parent 存在性/hash；
- DAG 无环；
- claim kind 的必填语义字段与 temporal 组合合法；
- actor、asserted_by、recorded_by 与 decision 合法性；
- decision 绑定的 authority/protocol revision 当时存在且具备 capability；
- authority revoke 与离线旧决定的全局 causal frontier 可验证；
- approval kind 与 claim kind/risk 的组合合法；
- Context Manifest 的 selection、provider/purpose、引用 hash 与 policy result 可重放；
- owner relevance 标记；
- idempotency key 冲突；
- dedupe key 重复；
- migration/import manifest 完整；
- projection 是否等于 deterministic render；
- Git 是否处于未完成 merge；
- tracked `.local` 文件；
- Git 中出现 `sensitivity: restricted` 明文；
- 被修改或删除的不可变记录。
- cutover 后出现的 v1 candidate/event。

### 19.2 健康状态

对用户只展示四种高层状态：

- 正常；
- 有待确认；
- 有冲突；
- 数据需要修复。

详情页再提供具体 error code、路径和恢复建议。

### 19.3 故障矩阵

| 故障 | 行为 |
| --- | --- |
| revision 已写，投影未写 | 启动/sync 前自动重建 |
| 投影被手改 | 保存 import diff，等待导入或恢复 |
| YAML 语法损坏 | repository integrity error；停止受控写入、merge commit 与 push，等待 repair |
| parent 缺失 | 若非已知 incomplete run，repository integrity error；不得激活或继续同步 |
| hash 不符 | tamper/corruption，fail closed |
| Git push 失败 | 保留本地 commit，下轮继续 |
| Git merge 未完成 | 阻止新的 Memory 决定 |
| duplicate approve | 幂等折叠 |
| approve vs reject | decision conflict；按 risk reducer 降级，不静默选边 |
| allow vs deny / grant vs revoke | deny 生效、allow 不新增；等待人工 resolution |
| authority 多 head | 关闭现实行动与决定 API，只读 repair |
| 缓存损坏/删除 | 从 YAML 全量重建 |
| protocol 未知 | 只读并要求升级 |
| migration 未 activation | recovery/read-only；幂等 resume 或 rollback |
| legacy-after-cutover | 阻止自动归并，要求显式导入 |

### 19.4 诊断日志

日志可记录：operation ID、claim/revision ID、阶段、error code、耗时和 Git 状态；默认不得记录完整主张正文、source 内容、owner 名称或绝对用户路径。`doctor --bundle` 默认只包含 schema、ID/hash 前缀、计数和错误码；加入正文必须由用户单独勾选。

---

## 20. 测试与验收矩阵

### 20.1 Schema 与序列化

- YAML round-trip。
- multiline literal block。
- Unicode、Emoji、CJK、组合字符。
- LF/CRLF 输入规范化。
- canonical payload hash golden tests。
- 至少 Rust、TypeScript 和独立参考实现共享的 canonical hash golden vectors，覆盖 absent/null/default、集合排序、重复 source、NFC/NFD、未知字段和 schema 升级。
- 未知 enum、未知 schema、重复 key。
- YAML alias bomb、自定义 tag、超大文件。

### 20.2 Reducer 属性测试

- 从 §6.3.1 与 §8.2 的每一行自动生成 conformance cases：正向、非法 actor/capability、非法/缺失 parent、stale base、相同请求重试、相同 key/不同 payload、并发 sibling；不得只挑常用 transition 测试。
- 随机打乱记录读取顺序，current view 完全一致。
- 任意重复输入不改变结果。
- 有效 parent 图无环。
- pending 不覆盖 active。
- rejected/ignored 分支不改变当前 active。
- sibling active heads 必然冲突。
- resolution 必须覆盖所有 heads。
- resolution 后到达未覆盖的离线 head 必须重新打开冲突。
- action-sensitive conflict fail closed。
- 相同 idempotency key/相同 payload 幂等。
- 相同 key/不同 payload 报错。
- approved 与 current/application state 不得混为同一字段。
- owner authority epoch 变化与离线旧 owner 决定进入 authorization conflict。
- 旧 authority 决定早于 revoke 可验证；晚于或并发于 revoke 不得复活权限。
- 相同 claim 的非重叠有效区间可按 as-of 各自读取，重叠 sibling 才进入冲突。
- boundary allow/deny 冲突时 deny union、allow intersection，纯文本缺失正文也不会恢复权限。
- trust/risk/salience/retrieval weight 互不隐式升级。
- merge-claims 全部 effects 原子生效，任一 stale participant 整体失败。
- 删除 `.git` 历史副本后，仅保留当前受控 YAML 集合，所有历史人工决定仍能解析并验证其 protocol/authority binding。

### 20.3 投影 golden tests

- 只有 H1、H2 和事实 bullet。
- 多行缩进固定。
- 无 frontmatter、ID、状态、SHA、来源、引用备注、脚注。
- pending/rejected/ignored/revoked/deleted/conflict 不泄漏。
- action-sensitive conflict 只生成固定安全提示，不泄漏正文且不会完全隐藏行动暂停状态。
- 分类与条目顺序稳定。
- 删除缓存后 byte-identical 重建。
- 输入含假标题、列表符号、引用符、分隔线和代码围栏时仍只生成一条事实。
- macOS、Windows、Linux fixture 逐字节一致。

### 20.4 崩溃注入

在以下每一步后强制终止进程：

- tmp 写入前后；
- fsync 前后；
- atomic rename 前后；
- 第一个投影写入后；
- 第二个投影写入后；
- Git add/commit/fetch/merge/rebuild/push 各阶段。

重启后不得出现虚假 active、丢失 revision 或无法解释的 projection state。

### 20.5 两个真实 Git clone

必须覆盖：

1. 两端离线创建不同事实 → 取并集。
2. 同 claim、同 base 并发 replace → sibling conflict。
3. approve vs reject → conflict。
4. update vs delete → conflict。
5. 同一确认按钮重试 → 幂等。
6. 两端产生语义相同但不同 claim ID → 重复建议，不自动合并。
7. projection-only 文本冲突 → 从 YAML 重建。
8. immutable YAML 同路径不同内容 → fail closed。
9. push race → 重复 merge/reconcile，无记录丢失。
10. 一端旧 Host、一端 v2 → 旧端只读，不回写 v1。
11. resolution 后合并迟到 sibling → 冲突重新打开。
12. authority revision 更新与旧 owner 离线批准 → authorization conflict。
13. 两个本地进程和同仓库两个 worktree 并发 → common-dir 锁只允许一个 writer。
14. `.local/tmp` 存在任意半写文件 → `git add -A` 永不捕获。
15. 手工修改 pending YAML 一字后再批准 → expected semantic hash 不匹配，批准失败。
16. 相同 revision ID、不同内容摘要 → duplicate logical ID integrity failure。
17. allow 与 deny 并发 → 两 clone 都计算 deny，投影含同一安全提示。
18. authority grant 与 revoke 并发 → capabilities 取安全交集，旧 owner 离线批准不可复活。
19. 同一偏好去年/今年区间不重叠 → as-of 查询分别返回正确版本，不报并发冲突。
20. 两 Space 分别含私人 claim → 未指定 Space 的请求不跨场景加载。
21. provider deny/prompt/allow → Context Manifest 可证明选择和排除原因。
22. merge-claims 任一 participant stale → 不得部分写目标或 tombstone。

### 20.6 迁移

- 使用真实 v1 fixture，包括当前 113 candidate / 25 event 规模。
- 条目数、状态数、owner、分类、正文、来源、decision、历史逐项守恒。
- event 与投影不一致时 quarantine。
- 未知审批不升级 active。
- approved history 与 application eligibility 分开守恒。
- v1 category 不自动成为 claim kind；producer/approver 不自动成为 asserted_by；时间戳不自动成为 valid time。
- 当前 sotvault 基线的 19 pending、11 unknown/unknown approved、3 owner-stated/high 和 owner activation 分组结果有 golden fixture。
- dry-run 零写入。
- apply 幂等。
- complete 前回滚字节一致。
- complete 后新增 v2 数据时拒绝静默回滚。

### 20.7 UI 与可访问性

- 人工添加一次保存、一次写入。
- pending 确认一次点击、恰好一次 approve RPC。
- 重复点击不重复创建语义决定。
- 失败保留草稿并恢复按钮状态。
- delete 的语义说明准确。
- 冲突键盘操作、VoiceOver、焦点顺序和点击目标。
- 字体层级与全局菜单样式一致。

### 20.8 Agent eval

- 只提议 owner 相关事实。
- 不把其他人的 task 写入 Memory。
- 不把 task/daily transient context 写成长期事实。
- inferred 不能伪装 owner-stated。
- Agent 不能自批。
- 负向、低确定度、contested 和 avoid-error 在 context JSON 中不丢失。
- secret 输入被拒绝。
- “用户认为 P”不得被主语提升为“P”；统计 subject-promotion error rate。
- preference、belief、material fact 的 epistemic-type confusion rate 达到发布阈值。
- 过期 claim 不在当前查询召回，但在历史 as-of 查询可召回。
- 冲突不得静默选边；统计 silent-conflict-resolution rate，门禁要求为 0。
- 未选 Space 时不跨场景泄漏；统计 cross-space leakage rate，门禁要求为 0。
- 对矛盾或证据不足主张正确 abstain，测量 abstention precision/recall。
- owner 修正后，所有 projection/context/cache 的 correction propagation convergence time 达到发布阈值。
- 相同正文在 preference、belief、material_fact 下必须得到不同批准提示、读取策略和 action eligibility。

### 20.9 性能建议门槛

以 10,000 个逻辑事实、50,000 个 revision/过程资产为压力规模：

- 冷启动全量 validate/reduce 在目标硬件上可接受；建议门槛 2 秒内，最终以实测确定。
- 增量单 revision 归并与投影重建建议 200ms 内。
- `.local` cache 可优化性能，但删除 cache 后结果必须完全相同。

---

## 21. 实施阶段与门禁

### P0：协议冻结

交付：

- MemoryClaimRevision、protocol revision、authority revision、operation 与 Context Manifest schema；
- claim kind/subject/actor、approval kind、双时态、trust/risk/salience、context/consent 约束矩阵；
- 规范 transition 与按风险分类的 conflict reducer；
- canonical hash 规则和跨语言 golden vectors；
- error code；
- 投影格式 golden fixtures；
- restricted 明文禁止策略与语义可靠性 eval 阈值；
- 本 RFC 的评审结论。

门禁：§3.1 全部勾选，外部架构、安全、Git、语义和产品评审签字。在此之前 RFC 状态保持 `Protocol Freeze Blocked`，不得权威迁移。

### P1：v2 核心只读

交付：

- YAML repository；
- validator；
- DAG reducer；
- deterministic projector；
- as-of/context selector 与 Context Manifest generator；
- `check` / `doctor` / `rebuild`；
- property tests。

旧 v1 仍是写路径，v2 只做 fixture 和 shadow compare，不形成第二权威。

### P2：写事务与并发

交付：

- immutable revision writer；
- local shared lock；
- atomic/no-clobber；
- idempotency；
- stale base；
- crash recovery；
- conflict reducer/resolution。
- protocol/authority reducer 与撤权 non-resurrection。

门禁：双 clone 与 crash injection 测试。

### P3：Git reconcile

交付：

- Vault sync Memory 专用冲突分类；
- post-merge validate/reduce/rebuild；
- push race 重试；
- immutable YAML fail-closed；
- projection conflict regeneration。

### P4：RPC、CLI 与插件 UI

交付：

- v2 API；
- Context Manifest、Space/purpose/provider consent UI；
- 人工添加自动批准；
- pending 一击确认；
- 冲突与历史；
- import projection；
- 可访问性与四语文案。

### P5：迁移与兼容

交付：

- dry-run；
- apply；
- migration bundle；
- rollback；
- legacy v1 read-only；
- 旧 Host fail-closed sentinel；
- sotvault 副本演练报告。

### P6：模板与 Agent 契约

交付：

- USER/MEMORY 纯文本模板；
- AGENTS 模板及存量 sotvault AGENTS 更新；
- owner CLI；
- search 路径分类；
- Agent eval。

### P7：发布

1. Host 发版。
2. 验证升级、Git reconcile 和协议门禁。
3. Memory Plugin 2.0.0 发版。
4. 分批开放 migration。
5. 观察一个版本周期后再考虑移除 v1 importer/sentinel。

---

## 22. 主要风险与缓解

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| 不可变 revision 数量持续增长 | 扫描性能下降 | 目录分片、可弃 cache、性能门槛；不以破坏审计换速度 |
| protocol/authority revision 出现多个 heads | 写入或行动权限不明确 | 不可变 DAG + 全局 causal frontier；capability 取安全交集；冲突 fail closed |
| 外部 Git 客户端不运行 reconcile | 根投影暂时冲突或陈旧 | 启动、插件打开、CLI 与 sync 前自动 check/rebuild |
| 用户误以为 deleted 等于物理擦除 | 隐私预期不符 | UI 明确 tombstone/Git 历史语义，秘密内容禁止入库 |
| Agent 伪造 human actor | 错误事实被激活 | 分离可信 UI token 与 Agent RPC；不信任普通布尔参数 |
| 旧 Host 回写 v1 | 双重权威 | protocol sentinel、min host、旧版只读门禁 |
| 迁移把未知旧条目升级 active | 获得虚假确定性 | 三方证明；否则 pending/quarantine |
| drift 导入误判 bullet 对应关系 | 错误覆盖事实 | 必须预览；低置信映射只生成 pending，不自动 edit/delete |
| 同义主张不同 claim ID | 重复投影 | dedupe 建议 + 原子人工 merge，不静默合并 |
| owner 批准个人表达被误当外部真值 | Agent 获得虚假确定性 | approval kind、representation/truth certainty 分离，material fact 需要 factual verification |
| 边界冲突正文从投影消失 | Agent 错误恢复权限 | deny union / allow intersection、安全提示、可行动 Agent 强制 context |
| 全局画像跨场景污染 | 私人记忆错误外发 | Space/purpose/provider policy、最小 Context Manifest、未指定场景默认不跨域 |
| Git 仓库被恶意任意改写 | 审批可伪造 | 明确信任边界；未来可选签名，不在 v2 假装已解决 |

---

## 23. 被否决的替代方案

### 23.1 根 Markdown 继续做数据库

否决。它会继续混合用户可读文本和程序状态，并使 Git 文本冲突等于状态冲突。

### 23.2 每个 Claim 一个可变 YAML

否决。只是把冲突热点从 Markdown 搬到 YAML，没有解决同事实离线并发。

### 23.3 单个 append-only YAML/JSONL

否决。所有设备仍写同一个文件，Git 行合并和尾部追加都可能冲突或产生不完整记录。

### 23.4 SQLite

否决。用户明确不需要；数据库二进制不适合 Git merge，也会削弱可检查性。

### 23.5 时间戳 / LWW

否决。“较晚”不等于“正确”，设备时间也不可靠。尤其 approve/reject 或 update/delete 不能被钟表决定。

### 23.6 完整 CRDT

暂不采用。事实文本和审批不是适合逐字符自动合并的数据；强行 CRDT 化会隐藏语义冲突。v2 只让记录集合可交换合并，事实冲突由人裁决。

### 23.7 长期双写 v1/v2

否决。两个权威源发生差异时无法可靠判断哪一边正确。只允许 shadow read 和一次 cutover。

### 23.8 依赖自定义 Git merge driver

否决为唯一方案。第三方客户端、移动端和未配置环境无法保证运行 driver；正确性必须由 `memory reconcile` 自证。

---

## 24. 实现影响面

### Host Rust

- `memory_control/model.rs`：替换为 v2 disk schema、reducer DTO、稳定 error codes。
- `memory_control/document.rs`：从 root parser/updater 改为纯 projector + legacy importer。
- `memory_control/store.rs`：重写为 YAML repository、DAG reducer、writer、doctor、migration。
- `memory_control/mod.rs`：扩展 v2 dispatch，保留必要兼容 adapter。
- `cli/memory.rs`：更新命令、JSON schema、退出码和人工/Agent权限边界。
- `vault_sync/conflict.rs`、`git_ops.rs`：增加 Memory 专用 merge/reconcile hook。
- search watcher/index：确保根投影重建后重新索引，并继续忽略 `.notemd`。

### Memory Plugin

- `plugins-src/memory/src/lib/types.ts`：从 Proposal/Event/MemoryEntry v1 类型迁移到 Claim/Revision/Conflict v2。
- `plugins-src/memory/src/App.svelte`：已确认、待确认、冲突与历史三层信息架构。
- RPC client/domain：幂等 token、stale base、一次点击一次写入。
- UI tests：快捷动作、失败保留、可访问性、字体和点击目标。

### Templates / Vault

- `src-tauri/templates/USER.md`
- `src-tauri/templates/MEMORY.md`
- `src-tauri/templates/AGENTS.md`
- `sotvault/AGENTS.md`（独立 Vault 仓库）
- `.gitignore` / `.gitattributes`

---

## 25. 外部评审清单

### 架构

- [ ] 单一权威是否无歧义？
- [ ] full-snapshot revision 是否足以表达所有操作？
- [ ] reducer 是否在任意输入顺序下确定？
- [ ] 冲突与重复的边界是否清晰？
- [ ] claim kind、subject、asserted_by、recorded_by 与 approval actor 是否没有混用？
- [ ] valid time 与 record/decision time 是否支持现在和历史 as-of？
- [ ] 是否存在需要全局可变状态的隐藏需求？

### Git / 并发

- [ ] 两 clone 的并发矩阵是否覆盖主要 race？
- [ ] shared local lock 是否覆盖 Memory write 与 Git staging？
- [ ] post-merge reconcile 是否在所有 sync 路径执行？
- [ ] immutable YAML 冲突是否真正 fail closed？
- [ ] 删除 Git 历史后，仅凭当前受控 YAML 集合能否验证旧决定当时绑定的 authority/protocol？
- [ ] authority revoke 与离线旧决定是否不会发生权限复活？
- [ ] push retry 是否可能产生无限 reconciliation commit？

### 安全

- [ ] Agent 与 human approval 是否在能力层真正分离？
- [ ] owner 初始化和变更是否存在越权路径？
- [ ] 删除与敏感信息文案是否符合真实 Git 语义？
- [ ] YAML parser 是否具备资源与路径防护？
- [ ] 日志、诊断和 migration bundle 是否可能泄露正文？
- [ ] restricted 明文是否在全部写路径拒绝？
- [ ] allow/deny、边界 weakening 与 authority 冲突是否执行最严格约束？
- [ ] Context Manifest 是否能证明 Space、purpose、provider/model、tools 与选择原因？

### 迁移

- [ ] v1 active 的证明条件是否足够保守？
- [ ] event/projection 不一致时的 quarantine 是否可操作？
- [ ] approved history 与 current application 是否分开迁移？
- [ ] category/producer/timestamp 是否都不会被错误提升为 kind/assertor/valid time？
- [ ] dry-run 与 apply 是否真正幂等？
- [ ] rollback 门槛是否避免丢失新 v2 数据？
- [ ] 旧 Host 是否能可靠 fail closed？

### 产品

- [ ] 人工新增和确认是否保持一次交互？
- [ ] 根 Markdown 是否足够纯净且可读？
- [ ] 冲突 UI 是否能让非技术用户作出决定？
- [ ] “删除但 Git 历史保留”是否表达清楚？
- [ ] 高级元数据是否隐藏得当但仍可检查？
- [ ] preference、belief、material fact 是否显示不同批准语义？
- [ ] 可行动 Agent 是否无法绕过 context/safety overlay？

---

## 26. 尚待评审裁决的问题

以下问题不阻止 RFC 0.10 外部评审，但必须在 P0 结束前定案。已经由本轮评审确定的事项——Claim 命名、approval kind、双时态、不可变 authority/protocol、风险冲突、Context Manifest 和 restricted 禁止——不再列为开放问题：

1. current protocol revision 的分类表首版是否允许 owner 自定义，还是只允许内置稳定枚举？本 RFC 推荐首版固定枚举。
2. 语义完全相同的重复 approve 是否只在 reducer 中折叠，还是生成确定性 coalesce revision？本 RFC推荐首版只折叠。
3. 结构化 context 对冲突事实返回多少历史正文？本 RFC已经确定根投影排除冲突；推荐 context 返回共同祖先与 heads 的受控诊断，但所有授权性 guidance 标记 `do_not_rely`。
4. 人工 CLI 的“可信启动路径”采用本机交互式 challenge、GUI-issued token 还是限制为 UI？本 RFC推荐首版审批只由 UI 完成，CLI 只提供 propose/read/maintenance；若必须人工 CLI，再单独评审认证方案。
5. v2 首版是否要求 owner 授权设备的密码学签名？若不要求，公开保证必须限定为“受信任的本地 note.md 会话”，不得声称抵御恶意 Git 写入。
6. Purge 首版是否只生成清除计划，还是协助 Git history rewrite？本 RFC 推荐首版只提供 plan，不自动改写远端历史。
7. 性能门槛和各语义 eval 阈值需要用真实 sotvault 与 50k revision fixture 实测后确定；本文 2 秒/200ms 仅为建议目标。

---

## 27. 设计依据与可追溯证据

本 RFC 的语义补强不是从存储便利性反推，而是先用 sotvault 的既有研究与现状资产做事实检索，再把结论转为规范约束。评审人可在 Vault 根目录运行 `notemd search '<关键词>' --json` 回到原文。以下文档是本轮直接使用的设计依据：

| Vault 文档 | 本 RFC 采用的结论 |
| --- | --- |
| `2026-08-27-research-public-knowledge-vs-personal-memory.md` | 记录/来源不等于命题真值；必须保存主体、认识论类型、证据关系、时间与批准语义 |
| `2026-06-18-hemory-memory-mechanism-spec-v0.1.md` | preference、decision、fact、belief 等类型有不同真值规则与生命周期，不能压成一种文本事实 |
| `2026-07-30-claude-memory-design-consensus-spec.md` | 信任分层、时间一等化、冲突 abstention、Markdown 仅作 interface、负向约束不可静默丢失 |
| `2026-06-11-zep-memory-design-insights.md` | valid time 与 system/record time 分离，历史版本保留并支持 as-of 查询 |
| `2026-09-01-web-memorybox-context-runtime-design-report.md` | Space 边界、按运行选择上下文、provider/purpose 限制与 Context Manifest，避免全局画像跨场景污染 |
| `2026-09-01-codex-memory-protocol-v2-review.md` | 本轮 P0/P1 评审意见、冻结结论和验收条件 |

迁移数量来自对 `/Users/bruce/git/sotvault/.notemd/memory`、根 `USER.md` / `MEMORY.md` 和 v1 candidate/event 的只读盘点；它们是 2026-09-01 的评审基线，不是协议常量。迁移工具必须自行重算 source manifest 与 plan hash，不能信任本文快照。

外部论文、产品或网页研究在上述 Vault 文档中已有来源链；本 RFC 只抽取能转化为本地协议不变式的结论，不把研究摘要本身当作 authority。若规范文字与证据笔记冲突，以本 RFC 经批准的 protocol revision 为实现规范，并将差异重新提交评审。

---

## 28. 最终建议

这次重构应被视为个人记忆控制协议升级，而不是 Memory UI 改版，也不是完整的通用记忆系统。

最安全且长期可维护的路径是：

- 用不可变 YAML revision 代替可变 Markdown 状态；
- 用 reducer 代替根文档反解析；
- 用 Claim 类型、主体、断言者、批准含义和双时态保存语义，避免把“用户认为 P”压缩成“P”；
- 用 Space、purpose、provider policy 与 Context Manifest 控制每次读取和外发；
- 用明确因果和人工冲突解决代替时间戳覆盖；
- 用按风险分类的 fail-closed 覆盖保证冲突不会放宽权限；
- 用 deterministic projector 保留 USER/MEMORY 的纯文本体验；
- 用 dry-run、quarantine、协议门禁和 Git rollback 完成一次性迁移；
- 在核心、Git 与迁移验证完成后再切 UI 和发布。

任何实现若重新引入“一个可变当前状态文件”“根 Markdown 作为输入”“时间较新即胜出”“owner 确认自动等于外部真值/行为授权”“冲突时边界从 Agent 上下文消失”或“v1/v2 双写”，都应在架构评审阶段直接否决。
