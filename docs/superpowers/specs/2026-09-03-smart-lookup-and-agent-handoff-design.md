# 智能搜索：自然语言智能查找与 Agent 交接

> 类型：产品、交互、检索与 Harness 设计 · 日期：2026-09-03 · 状态：已实施，进入 v6.903.6 发布；旧任务兼容周期中
>
> 本文是内置智能搜索的后续权威规格。它完整取代《智能检索问答：最简可靠架构》，并取代《全局智能搜索与问答》中规划、Tune、冻结证据、Memory、自动回答、归档回答和详细文档部分；原规格中的全局入口、输入法、搜索索引、结果来源分组和原文导航设计继续有效。

## 0. 决策摘要

内置能力从“检索后自动回答的 Agent”重新定位为：

> **自然语言驱动的 Vault 快速查找器：几秒内理解条件、找到可核对的原文、组织结果；复杂问题一键交给外部 Codex、Claude 或其他完整 Agent。**

默认主链只有一次小模型调用：

```text
输入中：本地即时预览（0 token）

Enter：Fast Planner
  → 可信宿主校验 SearchPlanV1
  → 与 notemd search 同源的 typed search
  → 宿主确定性生成解释条、结果列表和原文预览
```

用户可再明确选择：

```text
生成简答 → 最多一次 fast Summary 调用，只总结少量完整索引块
交给 Agent → 新建独立的完整 Agent run，由 Agent 自己调用 notemd search
             并按 Memory policy 请求 notemd memory context
```

硬决定：

1. `Enter` 表示“智能查找”，不再表示“问 Agent”。
2. 删除自动 Tune、自动 Answer、自动 Memory、回答归档和详细文档生成。
3. SearchPlan 继续使用现有 V1；不为了 `coverage` 升级 V2。
4. 搜索结果是“匹配线索”，不宣称是语义完备证据。
5. 默认结果完全由宿主渲染，不需要第二次模型调用。
6. 简答只由用户显式触发，默认使用 fast profile，不读取 Memory。
7. 完整列表、精确数量、否定证明、跨文档综合和任务执行属于外部 Agent。
8. 没有安装 Agent 时，窗口仍是完整可用的本地搜索与原文导航工具。

## 1. 产品定位

### 1.1 核心用户问题

内置智能搜索优先解决：

- “我记得写过，但不知道在哪。”
- “把自然语言里的时间、路径、标签、来源条件正确应用到搜索。”
- “哪些结果最值得先打开，为什么匹配？”
- “先给我一句很短的当前结果概括。”
- “这个问题需要深入分析，直接交给我已安装的 Agent。”

### 1.2 与普通关键词搜索的差异

普通搜索直接执行用户输入；智能查找在用户提交后增加一次受控理解：

- 时间表达转换为 `after/before`，不作为正文关键词。
- 路径、标签、类型、扩展名、来源和链接页转换为 typed filters。
- 主题可拆成最多一个 precision 和一个 recall 查询臂。
- 结果显示理解出的条件、扩展词和每条命中原因。

输入过程中的本地预览仍是普通快速检索，必须明确标为“快速预览”；只有 Planner 成功且宿主执行校验后的计划，才标为“智能结果”。

### 1.3 与外部 Agent 的差异

| 能力 | 内置智能搜索 | 外部 Codex / Claude / DSH Agent |
| --- | --- | --- |
| 目标 | 快速找到原文 | 深入阅读、综合、推理和执行 |
| 自动模型调用 | 0 或 1 次 fast Plan | 由 Agent 工作流决定 |
| Vault 正文入模 | 默认不进入 | Agent 按权限自主读取 |
| Memory | 不读取 | Agent 按 Role/Scope/purpose 请求 |
| 工具 | 无 | 可调用 `notemd` 和获准工具 |
| 写入 | 无 | 由任务 sandbox 和用户授权决定 |
| 完整性结论 | 不提供 | Agent 阅读和交叉验证后自行限定 |
| 典型耗时 | 毫秒到数秒 | 数十秒到数分钟 |

## 2. 支持范围

### 2.1 P0 必须支持

1. 文件、章节、段落和决定的定位。
2. 自然语言时间范围、路径、标签、类型、来源和排序条件。
3. 最多两个逻辑查询臂的主题扩展。
4. 按来源/文件或日期分组的结果列表。
5. 最相关原文预览、匹配词高亮和匹配原因。
6. 打开原文、复制引用、复制 Vault-relative 位置。
7. 将原问题、已解析条件和可选结果引用交给完整 Agent。
8. Planner 不可用或失败时继续展示本地预览。

### 2.2 P1 可选简答

用户点击“生成简答”后，可基于少量未截断的 Line 级索引块生成：

- 一句话概括；或
- 最多三个简短要点。

每个实质要点都必须引用 `[Sx]`。简答固定显示“基于当前匹配结果”，不能显示为完整回答。

### 2.3 明确不做

内置主链不支持：

- “所有、完整列表、精确数量、从未、不存在、有没有遗漏”等完备性结论。
- 对数万行文档或多篇长文做完整总结。
- 依靠分散在多个段落的关键词自动建立因果或论证关系。
- 生成报告、归档回答、写文档、修改 Vault 或执行任务。
- 自动加载 USER/MEMORY facts。
- 自动调优、模型 rerank、embedding、向量数据库或自由 tool loop。

遇到这些意图仍可展示相关搜索结果，但右侧提示：

> 当前结果适合定位原文；这个问题需要完整 Agent 深入阅读。

并突出“交给 Agent”操作。

## 3. 用户可见流程

### 3.1 输入与提交

- 输入过程中沿用 IME 安全的 120/400 ms debounce，执行 FTS-only 本地预览。
- 输入过程不启动 Planner、不加载 Memory、不自动 deep scan。
- `Shift+Enter` 输入换行。
- `Enter` 冻结本次问题并启动智能查找。
- 新输入立即使旧 Planner/搜索结果失去交互资格；旧搜索由现有后端 ticket 取消，旧 Planner 在 provider 支持时调用 `run-cancel`。

### 3.2 主状态机

```text
idle
  → previewing
  → understanding
  → searching
  → ready

understanding → preview_only       Planner 不可用、超时或输出非法
searching     → search_partial     deadline 到达但已有结果
searching     → no_results
任一运行      → superseded

ready → summarizing → ready       显式简答；失败不清空搜索结果
ready → handing_off → agent_run    显式交接；后续状态在 Agent 窗口展示
```

`preview_only`、`search_partial`、`no_results` 都是正常结果，不显示“Agent 回答失败”。

### 3.3 工作过程

保留滚动工作记录，但只显示宿主生成的结构化状态：

```text
正在使用快速模型理解问题
已识别时间范围：2026-08-01 至 2026-08-31
已识别主题：发布、延期；扩展：上线、推迟
正在执行 2 个受控查询
当前返回 12 条结果，来自 5 篇文档
结果已按日期排列
```

规则：

- 新状态立即追加并自动滚动。
- 成功、部分结果和失败后都保留。
- 用户向上滚动后停止自动跟随；回到底部后恢复。
- 最多保留 50 条，本流程正常应少于 10 条。
- 禁止显示 provider 自由文本 `last`、系统 Prompt、正文、Memory、命令参数、token、密钥或内部绝对路径。

## 4. 窗口结构

### 4.1 顶部命令条

```text
[搜索图标] [自然语言输入……………………] [智能查找 ↵]
             输入中即时搜索 · ⇧↵ 换行
```

- Agent 不再是使用搜索窗口的前置条件。
- Planner provider/model 收进“智能理解”设置，不长期占据主按钮旁的选择框。
- Planner 不可用时，按钮仍可点击；执行当前本地结果并提示“未进行智能理解”。

### 4.2 理解结果条

Planner 成功后显示只读 chips：

- 主题和扩展词。
- 时间范围及其时区。
- path/tag/type/ext/origin/page filters。
- 排序方式。
- 未支持约束和歧义警告。

P0 不在 chips 内编辑计划。用户通过修改原问题纠正理解，避免增加第二套查询编辑器。

### 4.3 双栏主体

左栏是结果列表：

- 标题、breadcrumb、行号、最多两行命中预览。
- 命中词高亮。
- 最多两个稳定匹配原因。
- 来源、人类确认、日期和类型元数据。
- 多选、Shift 连选和移除继续保留；移除只影响当前显示、简答和交接，不删除文件。

右栏是当前结果卡：

- Vault-relative 路径和真实索引范围。
- breadcrumb 或文件名。
- 当前索引块文本；File/Section 显示有界预览并明确类型。
- “打开原文”“复制引用”“生成简答”“交给 Agent”。
- 简答不可用时说明原因，不用灰色按钮让用户猜。

### 4.4 结果计数措辞

当前 smart search 的 `total` 是应用 limit 后的数量，不是 corpus 总数。因此统一写：

- `当前返回 12 条`；
- `当前返回 50 条，结果可能未完全显示`；
- 不写 `共 50 条`，除非未来有独立的精确 count API。

## 5. Plan：唯一自动模型阶段

### 5.1 沿用 SearchPlanV1

P0 不新增 SearchPlanV2。继续使用现有严格 DTO：

```json
{
  "schemaVersion": 1,
  "intent": { "kind": "locate", "focus": "8 月发布延期的讨论" },
  "time": {
    "appliesTo": "document_date",
    "sourceText": "8 月",
    "expression": { "kind": "absolute_range", "after": "2026-08-01", "before": "2026-08-31" }
  },
  "constraints": {
    "paths": { "anyOf": [], "allOf": [] },
    "tags": { "anyOf": [], "allOf": [] },
    "types": { "anyOf": [] },
    "extensions": { "anyOf": [] },
    "origins": { "anyOf": [] },
    "linkedPages": { "allOf": [] }
  },
  "queries": [
    {
      "id": "precision",
      "purpose": "precision",
      "terms": ["发布", "延期"],
      "phrases": [],
      "weight": 1.5,
      "rationale": "直接主题"
    },
    {
      "id": "recall",
      "purpose": "recall",
      "terms": ["上线", "推迟"],
      "phrases": [],
      "weight": 1.0,
      "rationale": "常见同义表达"
    }
  ],
  "sort": "doc_date_desc",
  "unsupportedConstraints": [],
  "ambiguities": [],
  "confidence": "high"
}
```

### 5.2 Planner 输入

只包含：

- 原始问题，最多 2,000 Unicode 字符且最多 8 KiB UTF-8。
- `referenceTime/timezone/locale`。
- 宿主预解析并锁定的显式查询 filters。
- 完整 SearchPlanV1 schema 和紧凑规则。

不包含：

- Vault 正文或文件列表。
- 本地预览结果。
- Memory。
- 上一次回答或搜索遥测。
- Tune 数据。

### 5.3 Planner 输出与宿主规则

- 新智能查找最多 2 个逻辑查询臂、8 个物理查询；物理上限沿用当前 compiler，避免多值显式 filters 展开后产生新的兼容错误。
- 通常一个 precision、一个 recall；不需要扩展时允许只有一个 precision。
- 显式 filters 不能被删除、放宽或改写。
- 相对时间仍由 Rust 解算最终日期。
- Planner 的 `intent.kind/confidence` 只用于 UI 提示和交接推荐，不是可靠性门禁。
- `unsupportedConstraints/ambiguities` 不阻止查找；宿主显示警告，且不得假装相关条件已应用。
- 不把自然语言原问题作为“智能结果”的关键词 fallback。
- JSON 非法、schema 非法或超出预算时不自动 repair；结束为 `preview_only`。用户点击“重新理解”才产生下一次调用。

### 5.4 Provider 能力

Planner provider 必须满足：

- `harness.ok=true`。
- 支持 `search-plan`。
- `search_plan_schemas` 包含 `1`。
- `terminal_result=true`。
- `input_only_isolation=true`。
- 支持 fast 或 default model profile。

优先 fast profile；没有 fast 时可以使用 default，但 UI 显示实际 resolved model。搜索窗口不再要求 provider 同时支持 `search-answer`。

## 6. Search 执行

### 6.1 可信边界

- WebView 只提交原问题、Planner JSON、冻结时间和现有设置。
- Rust 使用现有 `notemd_search_plan_context` 锁定显式 filters。
- Rust 严格解析 SearchPlanV1，编译为 typed `searchidx::Query`。
- 不执行 Planner 生成的 shell、CLI 字符串、绝对路径或工具调用。
- 继续复用 `notemd_planned_search` 与 `notemd search` 的索引、排序、来源权重和取消机制。

### 6.2 搜索策略

- 输入预览：FTS-only，使用现有即时搜索预算。
- Enter 后智能搜索：执行已校验查询臂，默认不自动 full scan。
- 零结果时显示“扩大查找”；只有用户设置了自动扩大，才自动启用 bounded deep scan。
- deep scan 到期可以返回部分结果，但 UI 必须显示 `结果可能未完全显示`。
- 搜索结果永远用于发现和导航，不据此推导“不存在”或“全部”。

### 6.3 长范围与 stale

- File/Section 命中范围可以任意长，不能因超过 500/64,000 行判为非法。
- 结果卡只显示有界预览，打开时由编辑器处理真实范围。
- 若索引行号已过期，打开文件并夹取到当前合法行；同时提示“文件已变化，结果可能来自旧索引”，触发后台索引刷新。
- stale 导航、空文件和超长行不能显示成 Planner/Agent 失败。
- P0 不重读长文、不构建 Evidence atom、不做固定上下文行截取。

## 7. 确定性结果输出

### 7.1 结果摘要

宿主/前端可以不调用模型直接生成：

- 当前返回结果数。
- 去重后的文档数。
- 是否 partial/truncated。
- 实际执行的查询臂数量。
- 当前分组和排序方式。
- 未执行或超时的查询臂。

### 7.2 匹配原因

沿用现有稳定枚举：

- `exact_page`
- `strict_query`
- `exact_phrase`
- `filename_match`
- `breadcrumb_match`
- `multiple_queries`
- `relaxed_query`

这些只解释检索排名，不表示事实置信度。不得显示“可信度 87%”。

### 7.3 分组

- `auto`：时间排序时按日期分组，否则沿用来源层级 + 文件分组。
- `source`：human/source/derived/unlabeled，再按文件。
- `date`：按月或日分组，无日期单列“日期未知”。

分组只改变展示，不改变检索和融合分数。

### 7.4 引用复制

支持两种复制格式：

- 简洁：`path/to/file.md:42`
- Markdown：`[标题](path/to/file.md#L42)`，范围存在时带 `L42-L48`

复制的是定位引用，不宣称是可独立理解的完整证据。

## 8. 显式快速简答（P1）

### 8.1 触发条件

只有用户点击“生成简答”才调用模型。系统绝不在 Enter 后自动生成。

默认来源：

- 用户有选择时使用选中结果。
- 没有选择时，从当前排序中取不同文件的前若干个合格结果。

合格来源必须：

- `level=line`。
- 是一个完整、未截断的索引 block。
- 单块和总字符数在预算内。
- 仍属于当前 query/run，不能从前端新增 path、range 或正文。

File/Section、截断块、超大块和仅 metadata 命中不进入简答。来源不足时按钮说明：

> 当前结果适合打开阅读，不能安全生成短摘要。

### 8.2 轻量运行快照

P1 为防止 WebView 篡改，宿主只维护 request-scoped `LookupRun`：

- `lookupRunId` 是随机 opaque id，只用于宿主查表，不自包含、不由前端解析。
- 绑定当前 window label、Vault identity 和原问题。
- 只保存在内存，默认 TTL 10 分钟。
- 每窗口最多 20 个，LRU 清理；活动 summary run 不清理。
- Vault 切换、窗口销毁和应用退出立即失效。
- 不落盘、不跨重启、不形成多级 hash 权限链。

宿主从该 run 选择 canonical hits，不能接受前端回传正文。

### 8.3 文件变化

简答前对候选文件读取一次当前字节并核对索引时 content hash：

- 相同：使用 canonical 索引 block 文本。
- 不同或消失：丢弃该来源并提示 stale。
- 不做 freshness 重搜；剩余来源不足时停止简答，保留搜索结果。

一次读取所得的同一字节用于 hash/状态判断，避免检查与读取之间产生竞态承诺。

### 8.4 Prompt 和输出

Summary 只接收：

- 原问题。
- “这是当前匹配结果，不是完整 corpus”的固定 limitation。
- `[S1]…[Sn]` 及完整索引 block 文本。

不接收 Memory，不开放工具、Bash、文件读取、网络或二次搜索。

输出约束：

- `sentence`：一个短段落。
- `bullets`：最多三个要点。
- 每个实质要点至少一个 `[Sx]`。
- Prompt 要求任何全称、否定或数量表述都限定为“当前提供的匹配结果”；UI 外层始终固定显示“基于当前匹配结果，不代表完整 Vault”。
- 未知 citation、空正文、非 success terminal result 或超出输出硬上限均不显示为成功。
- 失败只影响简答卡，搜索结果继续可用。

### 8.5 不保留旧回答功能

简答只支持查看、复制和打开引用，不支持：

- 点赞归档 Answer 文件。
- 点踩写 Agent run feedback。
- 重试长回答。
- 生成详细文档。
- 将简答自动写入 Vault。

需要沉淀内容时交给完整 Agent，或由用户复制到文档。

## 9. 交给完整 Agent

### 9.1 交接目标

“交给 Agent”是与智能查找分离的显式操作。支持：

- Codex Agent。
- Claude Agent。
- DeepSeek/DSH Agent。
- 后续任何声明 `vault-research` task 的 provider。

Planner provider 与交接 provider 相互独立。例如可以用 DeepSeek fast 模型理解搜索，再交给 Codex 深入处理。

### 9.2 HandoffPacket

只传递小型线索包：

```json
{
  "version": 1,
  "question": "分析 8 月发布延期的完整原因，并比较几次讨论的变化",
  "resolvedFilters": {
    "after": "2026-08-01",
    "before": "2026-08-31"
  },
  "queryTerms": ["发布", "延期", "上线", "推迟"],
  "selectedRefs": [
    { "path": "projects/launch.md", "line": 42, "lineEnd": 58 }
  ],
  "limitations": ["lookup_results_are_not_complete_evidence"]
}
```

约束：

- 最多 20 个 refs。
- 不包含正文、Memory、系统 Prompt、绝对路径或密钥。
- refs 是检索线索，不是可信证据；Agent 必须自行重搜、重读。
- 若 Planner 失败，仍可只带原问题和用户当前选择的 refs 交接。

### 9.3 `vault-research` task

三家官方 provider 增加统一托管任务：

- 运行于 Vault 的只读研究 sandbox。
- 允许执行 `notemd search`、读取必要源文和使用受控 Memory CLI。
- 不直接继承内置搜索的 summary Prompt。
- 默认使用 provider 的 default/质量模型，而不是 Planner fast 模型。
- Agent 必须把 HandoffPacket 当线索，不能假定其完整。

任务提示的核心语义：

```text
回答用户问题。先根据根 AGENTS.md 确认 Vault 约定；使用 notemd search
验证并扩展候选来源。需要个人或项目长期上下文时，按当前 Agent 身份、
Role、Scope 和 purpose=information-answer 调用 notemd memory context；
不要读取或使用未经 context broker 允许的 Memory。候选 refs 只是起点。
```

### 9.4 Memory 边界

- 内置智能搜索和快速简答的 Memory 调用数必须为 0。
- HandoffPacket 不包含 `memoryManifestId` 或 Memory facts。
- 外部 Agent 使用自己的真实 caller/provider/model 身份请求 Memory。
- Memory 是否可用不影响智能搜索结果。
- Agent 获取 Memory 失败时由 Agent 工作流呈现，不回写成搜索错误。

### 9.5 交接交互

- 已设置默认目标时，点击对应按钮立即启动新的 `vault-research` run 并打开该 Agent 的运行界面。
- 默认目标为“每次询问”时弹出标准 `.menu-panel` 目标菜单。
- Provider 未安装或不支持 `vault-research` 时，允许“复制交接提示”，不阻塞搜索。
- Agent 的耗时、token、工具进度和错误只在 Agent 工作区展示；搜索窗口显示一个可返回该 run 的链接。

启动 task 必须使用调用方生成的 `invocationId + inputHash`：同一 invocation 和同一输入返回原 `runId`，同一 invocation 配不同输入必须拒绝。启动 RPC 结果不确定时只查询该 invocation，不能再创建任务。这一契约同样用于 Plan 和 Summary，避免网络抖动重复消耗 token。

## 10. 可配置参数

### 10.1 存储原则

智能查找体验设置是设备级偏好，存入应用配置 `settings.json` 的 `smartLookup`，不写入 Vault 的 `.notemd/settings.json`，原因是：

- 已安装 provider 和模型是设备能力。
- 超时、结果数量和 UI 分组是个人交互偏好。
- 不应把个人 Agent 选择同步给团队成员。

索引范围、来源 glob、大文件阈值和搜索权重继续使用既有 Vault settings，两者不得重复定义。

### 10.2 设置结构

```ts
interface SmartLookupSettings {
  planner: {
    enabled: boolean
    provider: 'auto' | string
    modelByProvider: Record<string, ModelPreference>
    timeoutMs: number
  }
  results: {
    limit: 20 | 50 | 100
    groupBy: 'auto' | 'source' | 'date'
    autoDeepOnZero: boolean
    deepTimeoutMs: number
  }
  summary: {
    enabled: boolean
    provider: 'same_as_planner' | 'auto' | string
    modelByProvider: Record<string, ModelPreference>
    sourceLimit: number
    charLimit: number
    style: 'sentence' | 'bullets'
    timeoutMs: number
  }
  handoff: {
    defaultProvider: 'ask' | string
    includeSelectedRefs: boolean
  }
}
```

### 10.3 用户可设置值

| 设置 | 默认 | 允许值 | 位置 |
| --- | --- | --- | --- |
| 智能理解 | 开 | 开/关 | 搜索与索引 |
| 理解问题的 Agent | 自动 | 自动或已安装且合格的 provider | 搜索窗口快捷设置 + 设置页 |
| 理解模型 | fast profile | fast/default/该 provider 声明的精确模型 | 搜索窗口快捷设置 |
| Planner 等待时间 | 8 秒 | 3–15 秒 | 高级设置 |
| 结果数量 | 50 | 20/50/100 | 搜索与索引 |
| 结果分组 | 自动 | 自动/来源/日期 | 搜索窗口 + 设置页 |
| 零结果自动扩大查找 | 关 | 开/关 | 搜索与索引 |
| 扩大查找期限 | 4 秒 | 1–5 秒 | 高级设置 |
| 快速简答 | 开，但只手动触发 | 开/关 | 搜索与索引 |
| 简答 Agent | 跟随 Planner | 跟随/自动/指定 provider | 高级设置 |
| 简答模型 | fast profile | fast/default/精确模型 | 高级设置 |
| 简答来源数 | 4 | 1–6 | 高级设置 |
| 简答字符预算 | 4,000 | 1,000–6,000 | 高级设置 |
| 简答格式 | 三个要点 | 一句话/三个要点 | 搜索窗口 + 设置页 |
| 简答等待时间 | 15 秒 | 5–30 秒 | 高级设置 |
| 默认深入 Agent | 每次询问 | 询问或支持 `vault-research` 的 provider | 搜索与索引 |
| 交接包含当前选择 | 开 | 开/关 | 搜索与索引 |

### 10.4 不可配置硬上限

以下边界不允许用户提高：

- Enter 后自动 Planner 调用最多 1 次。
- 自动 Tune、自动 repair、自动 Summary、自动 Memory 均为 0 次。
- Planner 问题最多 2,000 字符/8 KiB，Plan JSON 最多 16 KiB。
- 最多 2 个逻辑查询臂、8 个物理查询。
- 结果 limit 最大 100。
- deep scan 最大 5 秒。
- Summary 最多 6 个来源、6,000 字符；单来源最多 3,000 字符且不能截断。
- Summary 输出最多 1,200 Unicode 字符。
- Handoff refs 最多 20 个，完整 packet 最大 16 KiB。
- 智能搜索与 Summary 永远不读取 Memory、不开工具、不写 Vault。

设置值越界或损坏时逐字段回落默认值，不能因为一个坏字段丢弃整组设置。UI 保存时拒绝越界值并保留原值。

### 10.5 Provider 解析顺序

- Planner `auto`：上次仍合格的 Planner provider → 应用默认 Agent → 官方 provider 稳定顺序中的第一个合格项。
- Summary `same_as_planner`：当前 Planner provider 支持 `search-summary` 时复用；否则按 `auto` 解析。
- 精确模型不存在时只回落该 provider 的 `fast`，再回落 `default`，并显示一次非阻塞提示。
- Handoff 指定 provider 不可用时回到“每次询问”，不能悄悄把任务交给另一家 Agent。

### 10.6 旧设置迁移

- 现有 `global-search` provider 偏好迁移为 `smartLookup.planner.provider`，仅在 provider 仍合格时采用。
- 现有每 provider 的 `plan` 模型偏好迁移到 `planner.modelByProvider`。
- 旧 `answer` 模型偏好不迁移为 Summary；Summary 必须重新使用默认 fast profile。
- 旧 localStorage key 保留一个版本但不再写入，下一版本清理。
- 缺失设置全部使用上述默认值，升级后无需用户先配置。

## 11. 超时、取消与失败

### 11.1 Deadline

| 阶段 | 默认 UI 期限 | 硬上限 | 到期行为 |
| --- | ---: | ---: | --- |
| 本地预览 | 现有查询预算 | 现有后端预算 | 保留已有结果 |
| Planner | 8 秒 | 15 秒 | 取消并进入 `preview_only` |
| typed search | 2 秒 | 5 秒 | 保留 partial，显示限制 |
| deep scan | 4 秒 | 5 秒 | 保留 partial |
| Summary | 15 秒 | 30 秒 | 保留结果，关闭简答 working 状态 |
| Handoff 启动 RPC | 10 秒 | 10 秒 | 按 invocation 查询同一次启动；显示可复制提示 |

官方 `search-plan` 任务的 provider 内层 timeout 不得超过 20 秒，`search-summary` 不得超过 35 秒，保证后台任务不会在 UI 放弃后长时间占用资源。

### 11.2 Planner 失败

以下都降级为 `preview_only`：

- 没有安装合格 provider。
- Provider 未登录或启动失败。
- Planner timeout/lost/cancelled。
- JSON 或 schema 非法。
- 模型选择已失效。

统一文案：

> 智能理解未完成，当前展示普通匹配结果。

附带具体但不泄密的短原因和“重新理解”。绝不显示“回答失败”。

### 11.3 搜索失败

- Index opening：显示索引进度，本地窗口保持可操作。
- Index failed：显示“搜索索引不可用”和打开索引设置。
- Partial：展示已有结果并标注限制。
- No results：提供“扩大查找”“修改问题”“交给 Agent”。
- 新 query：旧结果标为 superseded，不作为错误留在日志。

### 11.4 Summary 失败

- 不删除、不替换当前搜索结果。
- stale/超预算属于“无法生成短摘要”，不是 Agent failure。
- Provider 错误显示在简答卡内，允许用户再次明确点击重试。
- 重试重新选择当前合格来源，不承诺复用旧 snapshot。

## 12. 安全与隐私

1. Planner 是 input-only 任务，不读取 Vault、Memory 或工具。
2. Planner 输出只进入严格 typed plan compiler。
3. 搜索结果正文按不可信内容展示，不执行其中的指令。
4. Summary 只使用宿主保存的 canonical hit，不接受 WebView 正文。
5. Summary provider 必须声明 input-only isolation；第三方声明属于 provider 信任边界，UI 首次选择第三方时沿用 Agent 插件权限提示。
6. Handoff 是用户明确启动的完整 Agent run，权限、工具和可能的 token 消耗必须在 Agent 界面可见。
7. HandoffPacket 不携带 Memory；Memory 由目标 Agent 的真实 caller 重新授权。
8. 工作日志和遥测不记录问题全文、搜索正文或 Memory。
9. 路径展示使用 Vault-relative path；绝对路径只在宿主内部用于打开文件。

## 13. 数据与接口

### 13.1 P0 尽量复用

继续使用：

- `notemd_search_plan_context(originalQuery)`。
- `notemd_planned_search(originalQuery, plan, referenceTime, timezone, options)`。
- `notemd_search` / `notemd_smart_search` 用于输入预览。
- 现有 `SearchPlanV1`、`ResolvedSearchPlan`、`SmartSearchResponse` 和 relevance reasons。

从主链移除调用：

- `smart_search_freeze_sources`。
- `smart_search_memory_context`。
- `smart_search_archive_answer`。
- `smart_search_record_feedback`。
- `smart_search_write_document`。
- `search-answer` task。

后端命令可保留一个兼容版本，但新窗口不得调用。

### 13.2 P1 轻量运行接口

启用 Summary 后，`notemd_planned_search` 增加可选 `retainRun=true`：

```ts
interface RetainedPlannedSearchResponse extends PlannedSearchResponse {
  lookupRunId: string
  search: SmartSearchResponse & {
    hits: Array<SmartSearchHit & { resultId: string }>
  }
}
```

宿主内部的每条 `resultId` 绑定 `path/range/level/indexedText/indexedContentHash`，但 WebView 只看到 opaque id。随后：

```text
smart_lookup_start_summary(
  lookupRunId,
  selectedResultIds,
  provider,
  modelSelector,
  invocationId
)
```

由宿主按当前设置重新应用 source/char 硬上限、构造 Prompt 并启动 `search-summary`；不能接受 WebView 提供的正文、hash、path 或范围。未启用 Summary 时不创建 `LookupRun`。

Handoff 使用独立的 `smart_lookup_start_handoff`。宿主只接受 Vault-relative refs，逐项拒绝 absolute path、`..`、NUL、非普通文件和超过预算的输入；这些 refs 进入 Agent 后仍只是 untrusted hints，不被宿主读取为正文。

### 13.3 前端结果状态

```ts
type LookupAuthority = 'preview' | 'planned'
type LookupOutcome =
  | 'idle'
  | 'preview_only'
  | 'ready'
  | 'partial'
  | 'no_results'

interface LookupViewState {
  query: string
  authority: LookupAuthority
  outcome: LookupOutcome
  resolvedPlan: ResolvedSearchPlan | null
  results: SmartSearchResponse | null
  warnings: LookupWarning[]
}

type LookupWarning =
  | { code: 'planner_unavailable'; detail?: string }
  | { code: 'planner_timeout' }
  | { code: 'invalid_plan' }
  | { code: 'unsupported_constraint'; label: string }
  | { code: 'ambiguous_constraint'; label: string }
  | { code: 'partial_results' }
  | { code: 'stale_navigation'; path: string }
```

warnings 必须是枚举加有界显示文本；不能直接展示 provider 的自由日志。

### 13.4 Agent task

官方 provider 最终提供：

- `search-plan`：新宿主只发 `mode=plan`；模板额外兼容旧宿主的 `mode=tune` 一个版本，fast、input-only。
- `search-summary`：fast、input-only、无工具、无 Memory。
- `vault-research`：default/quality、只读 Vault、允许 `notemd` 和 Memory context broker。

`search-answer` 和 `mode=tune` 只为旧宿主兼容一个插件版本；新运行不使用。

三种 task 共用幂等启动协议：

```text
run-task(invocationId, inputHash, task, prompt, modelSelector)
```

相同 id + hash 返回原 run；相同 id + 不同 hash hard fail。官方 provider 同时暴露幂等 `run-cancel(runId)`，已完成或已取消的 run 再次 cancel 仍返回最终状态。

## 14. 实施范围与顺序

### P0A：先更新官方 Agent 契约

1. 三家 `search-plan` 托管模板让新调用只使用 Plan 语义，收紧 turn/timeout并继续输出 SearchPlanV1；为了 Agent 先发布不破坏旧宿主，旧 `mode=tune` 暂兼容一个版本。
2. capability 明确 `search-plan` 的 fast/default 可用模型和 input-only isolation。
3. 增加 `vault-research` 托管任务，证明能调用 `notemd search`，并按根 AGENTS.md 使用 Memory context broker。
4. 增加 `invocationId/inputHash` 幂等启动查询与统一 `run-cancel`；保证超时不重复扣 token，Planner 也不会在 UI 降级后继续长期运行。
5. `search-answer` 保持兼容但标记 deprecated。

### P0B：宿主与窗口改造

1. Enter 改为 `Plan → planned search → results`，删除 Tune/Freeze/Memory/Answer 主链。
2. 无 Agent 时允许本地预览、打开、复制和手动 deep search。
3. 把右侧回答面板改成原文结果卡和操作区。
4. 显示理解 chips、匹配原因、部分结果和结构化工作日志。
5. 加入 Agent handoff；无新任务 capability 时降级为复制提示。
6. 接入 `smartLookup` 设置与旧 provider/plan-model 偏好迁移。

### P1：快速简答

1. 三家 provider 增加 `search-summary` 托管任务。
2. 宿主增加短期 `LookupRun` 与 canonical hit 查表。
3. 实现合格 Line block 选择、stale 核对、预算和 citation validation。
4. 接入简答设置；保持手动触发。

### P2：清理与发布

1. 删除窗口中 Answer archive/feedback/document UI 和不可达代码。
2. 一个兼容周期后移除旧 `search-answer` 托管任务和废弃命令。
3. 更新中英日德文案、README、插件说明和 changelog。
4. 先发布 Agent provider，再发布宿主；宿主核心搜索不得硬依赖新插件版本。
5. 不发布过渡性的“截前 500 行”提交。

## 15. 必过测试

### 15.1 调用与成本

- 输入 100 次但不按 Enter：Plan/Tune/Summary/Memory 调用数均为 0。
- 正常 Enter：Plan 恰好 1 次、planned search 恰好 1 次，Tune/Answer/Summary/Memory 均为 0。
- Planner 非法 JSON 不自动 repair。
- “重新理解”是第二次用户动作，才产生新的 Plan run。
- “生成简答”恰好产生 1 次 Summary run，使用选定 fast profile。

### 15.2 自然语言约束

- “8 月发布延期”把时间放进 after/before，`8 月` 不进入正文 terms。
- “最近两周”“去年 Q4”“上个月”按冻结 referenceTime/timezone 解算。
- 显式 path/tag/type/ext/origin/page filters 不被 Planner 删除或放宽。
- content date 与 document date 不确定时显示 ambiguity，不偷偷应用文档日期。
- Planner unsupported constraint 显示为警告，不能显示成已应用 chip。

### 15.3 结果与导航

- File/Section 命中跨 600、5,000、64,000 和 100,000 行都能展示和打开，不报 `invalid source line range`。
- title、breadcrumb、正文和多个 query 命中显示正确 relevance reasons。
- truncated 时只写“当前返回 N 条，结果可能未完全显示”。
- stale 行号打开时夹取合法位置并提示刷新，不显示 Agent 错误。
- 多选、Shift 连选、移除和撤销只影响当前窗口。
- groupBy 只改变展示，不改变命中集合和 fused order 的组内顺序。

### 15.4 降级

- 没安装任何 Agent：输入预览、结果导航、复制和 deep search 均可用。
- Planner timeout/lost/invalid plan：保留 preview，状态为 `preview_only`。
- Search partial：显示已返回结果和 limitation。
- Summary timeout/stale/超预算：结果列表完全保留。
- 新问题 supersede 旧 Planner/Search/Summary，旧结果不能覆盖新状态。

### 15.5 Summary

- 只有用户点击才调用。
- File/Section、partial block、超 3,000 字符单块不能进入 Prompt。
- 应用用户 source/char 设置后仍不得超过硬上限。
- Prompt 不含 Memory、绝对路径或未选中的正文。
- 每个要点有已知 `[Sx]`；未知引用、空结果或非 complete terminal result 拒绝显示。
- Prompt 与 UI 固定把输出限定为当前匹配；真实模型评测中不得出现无此限定的全称/否定完备性承诺。

### 15.6 Handoff 与 Memory

- packet 不含正文、Memory、manifest、绝对路径或超过 20 个 refs。
- 目标 Agent 能用 refs 重新执行 `notemd search`，不会把 refs 当完整证据。
- Memory context 请求的 caller 是真实目标 provider，不是智能搜索窗口。
- 切换 Codex/Claude/DSH 会产生各自独立的新授权和 run。
- 不支持 `vault-research` 时可以复制提示，搜索不失败。

### 15.7 设置

- 所有默认值、最小值、最大值和越界拒绝有单元测试。
- 单个损坏字段只回落该字段。
- provider 卸载后回落 `auto`，不删除用户其他 provider 偏好。
- 旧 plan 偏好精确迁移，旧 answer 偏好不误迁移到 Summary。
- 设置变更通过 `settings://changed` 在搜索窗口实时生效。

## 16. 发布门槛

- 普通智能查找自动模型调用数 P50/P95 都为 1；自动 Tune/Answer/Summary/Memory 为 0。
- Planner 新增业务 payload 100% 小于 16 KiB；不包含 Vault/Memory。
- 默认设置下，warm Planner + planned search P95 不超过 8 秒；超时必须在预算内降级并保留预览。
- Planner valid-plan rate ≥ 98%。
- 时间条件 exact match ≥ 95%，时间文字误入全文 terms = 0。
- 自然语言 + 时间评测 Recall@10 ≥ 90%。
- 长范围场景 `invalid source line range` 次数为 0。
- 搜索窗口出现“Agent 回答失败”的次数为 0，因为该状态已不存在。
- Summary 每次来源 ≤ 6、正文 ≤ 6,000 字符、Memory 调用 = 0、未知引用 = 0。
- HandoffPacket 100% 小于 16 KiB且不含正文/Memory；三家官方 provider 均证明能重新调用 `notemd search`。
- 无 Agent、旧版 Agent、Agent 未登录、索引 opening/failed 和大 Vault 场景全部有可用降级路径。

## 17. 最终产品承诺

智能搜索的对外描述固定为：

> **用自然语言快速找到 Vault 中最值得阅读的原文；需要深入分析时，无缝交给你的 Agent。**

不得再描述为：

- “自动理解整个 Vault 并给出完整答案”。
- “证明某件事从未发生”。
- “无需 Agent 即可完成跨文档深度问答”。
- “零 token”——输入预览是零 token，提交后的智能理解会使用用户选择的 Harness 模型。

衡量成功的第一指标不是回答长度，而是：用户能否快速打开正确原文，或以很小的交接包让完整 Agent 继续工作。
