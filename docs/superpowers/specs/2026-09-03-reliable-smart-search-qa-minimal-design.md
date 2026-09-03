# 智能检索问答：最简可靠架构

> 类型：产品与技术设计 · 日期：2026-09-03 · 状态：已废止
>
> **后续实现以 [`2026-09-03-smart-lookup-and-agent-handoff-design.md`](2026-09-03-smart-lookup-and-agent-handoff-design.md) 为准。** 内置智能搜索已重新定位为“自然语言智能查找 + Agent 交接”；本文的 Evidence Builder、自动 Answer、Memory、Bundle 和 complete coverage 均不再实施。

## 0. 决策

正常问答只保留两次模型推理：

```text
Plan LLM（快速模型）
  → 可信宿主：校验计划、检索、语义证据构建、Memory 授权
  → Answer LLM（默认/质量模型）
```

- 删除自动 Tune。一次 Plan 必须同时给出 precision/recall 查询臂；低命中不再触发第二次模型猜词。
- Search、Evidence Builder、Memory 都是确定性宿主步骤，不新增 Evidence Agent。
- SearchHit 只用于发现和导航，绝不直接进入回答 Prompt。
- Section/File 长命中展开为完整的细粒度语义块；不把整篇文档一次送进模型，也不固定截取前后若干行冒充完整上下文。
- 快速问答无法在预算内取得完整证据时，明确返回“需要深度阅读”，不生成看似完整的回答。

这里的“可靠”不是保证每个问题都回答，而是保证：回答一旦生成，来源来自当前 Vault、范围真实、上下文边界可见、检索完整性没有被夸大；证据不足时诚实停止。

## 1. 功能边界

### 1.1 快速问答支持

- 定位文件、章节或决定。
- 根据一个或少量完整段落回答事实问题。
- 对少量对象做有证据的比较。
- 在完整命中集合和证据预算内生成列表或小范围总结。
- 识别自然语言里的时间、路径、标签、类型、来源和排序条件。

### 1.2 快速问答不伪装支持

- 数万行文档的完整总结。
- “所有、从未、有没有遗漏、完整数量”等问题在检索未完整结束时的结论。
- 只能从超大、不可完整装入预算的单一代码块或无结构段落推导出的事实。
- Planner 不能表达的关键约束，或会实质改变结果但尚未澄清的歧义。

这些情况返回结构化的 `needs_deep_read`、`incomplete_retrieval` 或 `needs_clarification`，不调用 Answer Agent。深度全文归纳属于后续独立工作流，不塞进快捷问答的自动主链。

## 2. 用户可见流水线

```text
输入问题
  ├─ 输入中：本地快速预览（零模型调用，不是回答证据）
  └─ Enter：
       1. 理解问题
       2. 执行结构化检索
       3. 从候选文档准备完整语义证据
       4. 检查证据覆盖范围与 Memory 授权
       5. 基于冻结证据回答
```

状态机固定为：

```text
idle → planning → searching → preparing_evidence → memory → answering → done
                    ├─ no_results
                    ├─ incomplete_retrieval
                    ├─ needs_deep_read
                    └─ hard_error

任一运行 → superseded（新问题、停止或窗口销毁后真实取消旧任务）
```

没有 `tuning` 状态。格式修正若保留，只是 Plan 阶段对非法 JSON 的一次有界 repair，不读取搜索结果，也不改变约束。

## 3. Plan：唯一的检索意图模型阶段

### 3.1 输入

Planner 只接收：

- `queryId` 和原始问题。
- 宿主冻结的 `referenceTime/timezone/locale`。
- 宿主预解析并锁定的显式 filters。
- `SearchPlan` schema、枚举和预算。

不接收 Vault 正文、快速预览、搜索命中、Memory 或上一次回答。

### 3.2 输出

继续使用严格、拒绝未知字段的 SearchPlan；只增加一个回答完备性字段：

```json
{
  "schemaVersion": 2,
  "intent": {
    "kind": "answer",
    "focus": "8 月发布延期的原因",
    "coverage": "focused"
  },
  "time": {},
  "constraints": {},
  "queries": [
    { "id": "precision", "purpose": "precision", "terms": ["发布", "延期"], "phrases": [], "weight": 1.5 },
    { "id": "recall", "purpose": "recall", "terms": ["上线", "推迟"], "phrases": [], "weight": 1.0 }
  ],
  "sort": "relevance",
  "unsupportedConstraints": [],
  "ambiguities": [],
  "confidence": "high"
}
```

`coverage` 只允许：

- `focused`：找到足以回答当前焦点的直接证据即可。
- `complete`：问题包含全部、完整列表、精确数量、从未、不存在等全称或否定语义；只有检索范围完整扫描后才可回答。

其余限制沿用 SearchPlanV1：最多 4 个逻辑臂、8 个物理查询；相对时间只输出受限 expression，由 Rust 解算；显式时间、路径、标签、类型、来源约束不能被 Planner 删除或放宽。

### 3.3 执行规则

- 正常情况只调用一次快速模型。
- 计划不是独立 JSON 或不符合 schema 时，允许同一 Plan 阶段修正一次；第二次仍非法则停止。
- `unsupportedConstraints` 中存在影响答案的关键条件，或 material ambiguity 未解决时，不执行搜索。
- 不把原问题作为关键词 fallback；Planner 失败时快速预览仍只标为预览。

## 4. Search：一次计划，一次可信执行

宿主直接把已校验计划编译为 typed `searchidx::Query`，执行计划已经给出的 precision/recall 臂，并稳定融合结果。

- 不 spawn shell，不拼接 `notemd search ...` 字符串。
- 所有查询臂共享一张 ticket、一个 index generation 和一个总 deadline。
- `INDEX_BUSY` 等瞬时只读故障可以用同一计划重试一次；不能重新调用 Planner。
- `coverage=focused` 时允许带 `truncated=true` 的部分检索继续，但 Answer 必须知道结果是部分的。
- `coverage=complete` 时使用无结果数截断的扫描上限；任何 arm 超时、deep 未完成、索引不可用或结果超过可审计上限，都返回 `incomplete_retrieval`，不调用 Answer。

每个内部命中由宿主生成 `hitId`，并绑定：

```text
queryId + planHash + indexGeneration + path + indexedContentHash + block range
```

WebView 只能显示命中或提交要排除的 `hitId`，不能重新定义 path、line、lineEnd、origin、humanVerified 或正文。

## 5. Discovery Hit 与 Evidence 必须分离

### 5.1 Discovery Hit

用于发现与 UI 导航，允许：

- `File`：整篇文档。
- `Section`：完整章节。
- `Line`：段落、列表节点、代码块或普通文本段。

其范围可以是 1–64,000 行。范围大不是错误，也不代表应把全部内容交给模型。

### 5.2 Evidence Atom

用于回答的最小证据单位，必须来自当前文件重新解析后的完整语义块：

- Markdown：完整段落、完整列表节点、完整代码块，并附标题祖先链。
- Outline：完整节点及其 continuation，并附父节点路径。
- TXT：完整空行分隔段落。
- Transcript：完整发言/字幕单元。

`File` 和 `Section` 永远不能直接成为 Prompt source。它们只限定 Evidence Builder 应在什么范围内寻找细粒度 atom。

## 6. 确定性 Evidence Builder

Evidence Builder 在 Rust 宿主运行，不调用模型。

### 6.1 重读与新鲜度

1. 根据宿主保存的 snapshot 解析 `hitId`，不接受 WebView 的 hit JSON。
2. 检查当前文件仍在 Vault 内、不是符号链接、大小未超限。
3. 比较当前内容 hash 与搜索 snapshot 的 `indexedContentHash`。
4. 若文件变化，使用同一 Resolved Plan 做一次确定性的 freshness sweep + 重搜；不调用 Planner。
5. 再次变化或消失的单条来源标为 stale 并丢弃；全部来源失效时返回 `no_results`。

路径穿越、snapshot/token/hash 不匹配属于完整性错误，整次 hard fail；普通文件删除或编辑属于可解释的 stale，不冒充安全攻击。

### 6.2 展开长命中

- `Line` 命中未超预算：直接成为 atom。
- `Section/File` 命中：重新解析当前文件，只取范围内的细粒度 `Line` atoms。
- 多个查询词分散在同一章节时，不要求某一个 atom 包含所有词；使用最小覆盖集合选择多个完整 atoms。
- 仅标题/文件名命中且正文无相关 atom：`locate` 可以返回文档 identity evidence；事实问答返回证据不足，不能拿首段正文代替答案。
- 宿主 snapshot 必须保存命中的 physical query、route 和 typed Query。Evidence Builder 使用与原检索相同的 tokenizer、phrase/LIKE 语义重放覆盖判断；title、breadcrumb、正文命中分别记录，不能把 metadata 命中伪装成正文证据。

### 6.3 选择算法

不使用 embedding，也不引入模型 reranker。先针对每个实际命中的 physical query arm 寻找能覆盖其正文 query atoms 的最小连续语义单元区间；能在预算内完整容纳时，输出一个连续证据。不能完整容纳时，再对完整 atoms 做 deterministic set-cover，每个相离 atom 都是独立来源，绝不插入省略号拼成一个伪连续引用。

对每个 atom 计算稳定的匹配元组：

1. 命中的完整 phrase 数量。
2. 对各 query arm 新覆盖的 term/phrase 权重。
3. breadcrumb/title 中的匹配数量。
4. discovery hit 的融合排名。
5. atom 越短且完整，优先级越高。
6. 最终以 `path + line + lineEnd` 稳定打破平局。

多个 arm 都可用时按以下顺序稳定选择：完整覆盖优先于部分覆盖，precision 优先于 recall，随后比较 phrase 数、arm weight、计划顺序和原 hit 排名。全局使用有上限的 greedy set-cover：每次选择“新增查询概念覆盖权重”最高的 atom；无新增覆盖后再按相关度补充。默认先限制同文件最多 2 个 atom，只有仍有未覆盖概念且总预算允许时放宽到 3 个。最终按原 hit 排名、path、line、query id 稳定排序后再分配 S1…Sn，不依赖 HashMap 顺序。

### 6.4 上下文完整性

证据不使用“命中行前后 N 行”。每条 atom 总是携带：

- 完整 atom 正文。
- heading/breadcrumb 祖先链。
- 当前文件、真实起止行和内容 hash。
- `contextStatus`。

`contextStatus`：

- `complete_atom`：完整语义块已进入证据。
- `complete_multi_atom`：同一论点由同章节多个完整语义块共同覆盖。
- `partial_oversized_atom`：最小语义块本身仍超过单文档预算。
- `document_identity_only`：只证明该文档/标题存在，不证明正文结论。

最小语义块仍过大时，有限预算与语义完整无法同时保证。Evidence Builder 只生成围绕真实 query atom 的连续定位窗口：优先句子边界，再用行边界；单行仍过长时用字符窗口并返回列偏移。必须同时设置 `completeUnit=false`、`truncatedBefore/After`，代码片段不得补造 fence。快速事实回答不能只依赖这种片段；若没有其他完整证据，返回 `needs_deep_read`。表格、HTML 等 chunker 没产生细粒度 atom 的内容，最后才允许 raw-line fallback，并标记 `structuralFallback=true`。

每条 evidence 的 `text` 必须是当前文件中的真实连续文本。breadcrumb、截断提示和省略标记只能放在结构化字段或 Prompt 包装层，不能混入引用正文。

### 6.5 固定预算

使用跨 provider 都能稳定承受的固定上限，不根据前端声称的模型窗口扩大：

- 最多 12 个 evidence atoms。
- 最多 8 个文件。
- 单 atom 最多 500 行、4,000 Unicode 字符、16 KiB UTF-8。
- 单文件最多 8,000 字符、32 KiB UTF-8。
- 全部原始证据最多 24,000 字符、96 KiB UTF-8。
- Evidence Bundle 序列化后最多 128 KiB。
- Evidence Builder 绝对期限 10 秒。

先按完整语义块和覆盖率选取，再执行总预算；不能先把 50 MiB 单行跨 IPC 传给前端后才截断。

## 7. 不同问题类型的可靠性规则

| 问题 | 可以回答的条件 | 条件不满足时 |
| --- | --- | --- |
| 定位 | 有真实 document/atom identity | `no_results` |
| 聚焦事实 | 至少一条完整 atom 直接覆盖核心概念 | `insufficient_evidence` |
| 比较 | 每个比较对象至少一条完整 atom | 明确缺少哪一侧，不做单边比较 |
| 列表/数量 | `coverage=complete`，检索完整，全部项目能进入结构化结果 | `incomplete_retrieval` 或 `needs_deep_read` |
| 否定/从未 | `coverage=complete`，目标 corpus 完整扫描 | 不得从 Top-N 片段推断不存在 |
| 小范围总结 | 全部目标原始证据在 24,000 字符内 | 正常回答 |
| 长文/多文档完整总结 | 快速问答预算放不下 | `needs_deep_read`，不自动 Map-Reduce |

“基于当前最相关证据”与“完整扫描后得到”是两个不同产品承诺，UI 和回答必须明确区分。

## 8. Evidence Bundle 与可信运行链

链路身份固定为：

```text
queryId
  → planHash
  → searchSnapshotId + indexGeneration
  → evidenceBundleId + evidenceBundleHash
  → answerRunId
```

最小 Bundle：

```json
{
  "bundleVersion": 1,
  "bundleId": "random-128-bit-id",
  "queryId": "...",
  "questionHash": "sha256:...",
  "planHash": "sha256:...",
  "searchSnapshotId": "...",
  "retrievalComplete": true,
  "limitations": [],
  "sources": [
    {
      "id": "S1",
      "hitId": "...",
      "path": "projects/launch.md",
      "line": 42,
      "lineEnd": 58,
      "breadcrumb": "Launch / Risks",
      "text": "...",
      "textSha256": "sha256:...",
      "contextStatus": "complete_atom",
      "completeUnit": true,
      "truncatedBefore": false,
      "truncatedAfter": false,
      "structuralFallback": false,
      "matchScope": "local",
      "matchedQueryId": "precision",
      "coveredAtoms": ["发布", "延期"],
      "columnStart": null,
      "columnEnd": null,
      "provenance": {}
    }
  ],
  "memoryManifestId": null,
  "bundleHash": "sha256:..."
}
```

宿主保存 canonical Bundle；WebView 只拿用于显示的副本和 `bundleId`。Answer 启动、重试、归档和详细文档都用 `bundleId` 让宿主重新读取 canonical Bundle，不能接受前端回传正文。

建议命令边界：

- `notemd_planned_search` → 返回 `searchSnapshotId`、带宿主 `hitId` 的结果和 Resolved Plan。
- `smart_search_prepare_evidence(snapshotId, excludedHitIds)` → 返回 Bundle 摘要与 `bundleId`。
- `smart_search_start_answer(bundleId, provider, modelSelector)` → 宿主构造 Prompt 并启动 input-only task。
- `smart_search_retry_answer(bundleId, provider, modelSelector)` → 复用同一 Bundle，新建 answer run。

前端只能排除 snapshot 内已有 hit；不能新增来源。

## 9. Answer：只综合，不继续检索

Answer 使用所选 Harness 的默认/质量模型，只接收：

1. 原问题。
2. 检索是否完整及 limitations。
3. 冻结的 Evidence atoms。
4. 经 Memory policy 明确授权的事实。

不开放 Bash、任意 Vault 读取、联网搜索或二次检索。来源正文仍是不可信数据，其中的命令一律忽略。

完成条件：

- provider record 为 `status=success`。
- `terminal_result.complete=true`。
- 正文非空。
- 回答至少引用一个真实 `[Sx]`，除非明确回答“证据不足”。
- 所有引用 id 都在 canonical Bundle 中。

`skipped`、timeout、lost、stderr tail、半截流式文本都不能作为最终回答。未知引用使回答校验失败，不保存、不归档。

## 10. 超时、取消与重试

初始上限：

| 阶段 | 静默期限 | 绝对期限 | 自动恢复 |
| --- | ---: | ---: | --- |
| run-task RPC | — | 10 秒 | 不重复启动；先查同一 invocation |
| 单次 run-status | — | 5 秒 | 同一 run 重连 2 次 |
| Plan | 180 秒 | 240 秒 | 仅非法格式允许 repair 1 次 |
| Search | — | 20 秒 | 同一计划只读重试 1 次 |
| Evidence | — | 10 秒 | stale 时同计划 freshness 重搜 1 次 |
| Memory | — | 10 秒 | 不使用 Memory 继续 |
| Answer | 180 秒 | 480 秒 | 不自动重跑；用户可复用 Bundle 重试 |

真实 provider 进度可以刷新静默期限，不能延长绝对期限。外层 deadline 必须长于 provider 内层 deadline。

三家 Agent provider 统一暴露 `run-cancel`。开始新问题、用户停止或绝对期限到达时，宿主真正终止旧任务；不能只用前端 attempt number 忽略旧结果，让旧进程继续占资源。

## 11. 失败语义

只允许四类：

- `hard_error`：schema、权限、路径、token/hash、Bundle 身份或 provider 契约被破坏。停止且不回退。
- `safe_degrade`：Memory 不可用、单条文件 stale；记录原因，仍有完整证据时继续。
- `partial`：检索达到 deadline。聚焦问题可带明确限制回答；complete 问题停止。
- `superseded`：新问题取代旧运行。取消旧任务，不显示成用户错误。

无结果、证据不足和需要深度阅读是正常业务结果，不显示“Agent 回答失败”。错误标题必须对应当前阶段：理解失败、检索失败、证据准备失败、回答失败。

## 12. 工作记录

允许显示：

```text
正在使用快速模型理解问题
已解析时间范围：2026-08-01 至 2026-08-31
正在执行 2 个受控检索臂
找到 6 篇候选文档
正在把 2 个长章节展开为完整语义块
已选择 7 条完整证据，覆盖 5 个查询概念
1 条来源已变化并被安全跳过
正在使用质量模型基于冻结证据回答
```

禁止显示 provider 自由文本 `last`、系统 Prompt、完整来源正文、Memory 正文、命令参数、密钥或 token。日志成功或失败后保留并自动滚动。

## 13. 最小实施顺序

### P0：先修正确性，不发固定窗口补丁

1. 在共享 Agent 契约与三家托管模板增加 SearchPlanV2 `coverage`，保留 V1 解析仅用于旧运行记录，不让 V1 驱动新的可靠问答。
2. 保留现有时间解算、多臂 typed search 和模型路由；从主链删除自动 Tune，但 provider 可兼容旧 `mode=tune` 一版。
3. 为 Search 结果增加宿主 `hitId/indexGeneration/contentHash` snapshot。
4. 实现 Evidence Builder：当前文件重读、细粒度 atom、按实际 physical query 语义计算覆盖、deterministic set-cover、预算和 contextStatus。
5. 用 snapshot + excludedHitIds 替代 WebView 回传任意 `SearchHit` 的 freeze API。
6. Answer/重试改为消费 canonical `bundleId`。

### P1：运行生命周期收口

1. 三家 provider 增加统一 `run-cancel`。
2. 串起 query/plan/search/bundle/answer hashes。
3. 严格 terminal result 和 citation validation。
4. 更新分阶段 UI 错误与滚动工作记录。

### P2：评测后发布

1. 跑确定性、故障、安全和大 Vault 性能矩阵。
2. 对 Claude/Codex/DeepSeek 做 opt-in 真模型 Plan/Answer 测试。
3. 先发布需要的 Agent provider 版本，再发布宿主；无插件源码变化则不制造插件版本。

## 14. 必过测试

### 14.1 调用次数与编排

- 正常问题恰好 1 次 Plan、1 次 planned search、1 次 Evidence、1 次 Answer。
- 1–2 个结果不会触发 Tune；零结果不会调用 Evidence/Answer，也不会使用快速预览回答。
- 非法 Plan 最多 repair 1 次；搜索重试和 freshness 重搜都不重新调用 Plan。
- Answer 重试复用完全相同的 Bundle hash。

### 14.2 长来源与语义证据

- 600、5,000、64,000 行的 File/Section 命中不会报 `invalid source line range`。
- 查询词位于第 500 行之后时，选择包含该词的完整 atom，不取文档前 500 行。
- 多个词分散在同章节不同段落时，最小覆盖集合包含相应完整 atoms。
- title、breadcrumb、正文分别命中时，覆盖类型与原 physical query/route 一致，metadata 不冒充正文。
- 返回的 path/line/lineEnd/text/hash 与当前文件逐字一致。
- 标题命中只允许 locate 使用 identity evidence；正文问答不能拿无关首段作答。
- 单一语义块超过单文档预算且无其他完整证据时返回 `needs_deep_read`。
- 超长单行、Unicode、CRLF、空文件、长代码块均不切坏编码或虚报范围。
- 相离片段永远是两个 source，不拼成一个伪连续引用；单行字符窗返回准确 columnStart/columnEnd。

### 14.3 完备性

- `focused + search.partial` 可以带 limitation 回答直接事实。
- “全部/数量/从未”被规划为 `coverage=complete`；任一 arm partial 时 Answer 调用数为 0。
- compare 缺一侧证据时不生成单边比较。
- 长文总结超过 24,000 字符时返回 `needs_deep_read`，不自动启动隐式 Map-Reduce。

### 14.4 安全与并发

- 任意 path/range/origin/text 篡改不能通过 hitId/snapshot/bundle 校验。
- traversal、absolute path、symlink、Vault 切换和 index generation 错配全部 hard fail。
- 单条普通 stale 来源可降级；全部 stale 只允许同计划重搜一次。
- 新 query 真正 cancel 旧 Plan/Answer；旧结果永不写入新状态。
- status transport 抖动只重连同一 run，不产生重复任务。
- 来源 prompt injection 无法触发工具、文件读取或网络。

### 14.5 终态与 UI

- timeout/lost/skipped/incomplete terminal result 都不显示为成功回答。
- 未知 `[Sx]` 引用拒绝归档。
- Plan、Search、Evidence、Answer 各自显示准确错误标题。
- 工作记录不含 prompt、正文、密钥或 provider 自由日志。

## 15. 发布门槛

- 确定性、安全、故障矩阵 100% 通过。
- 真实 Planner valid-plan rate ≥ 98%，时间约束 exact match ≥ 95%。
- 自然语言 + 时间评测 Recall@10 ≥ 90%，时间文字误入全文 terms = 0。
- 长块测试中 `invalid source line range` 出现次数为 0。
- 最终引用 id 可解析率 100%，引用文本与当前文件 hash 一致率 100%。
- complete 问题在任何 partial 检索下错误生成完整结论的次数为 0。
- 三家 provider 都证明 Plan 使用 fast profile、Answer 使用 default/质量模型，并记录实际 resolved model。

## 16. 保留决定

1. 最简方案不是“永远给答案”，而是“只在证据边界可证明时给答案”。
2. 不引入向量数据库、embedding、模型 reranker、自动 Tune 或自由 tool loop。
3. 不用固定前后行解决上下文；完整语义 atom 是默认单位，无法完整容纳时诚实转深度阅读。
4. 长文 Map-Reduce 是独立深度功能，不进入快捷问答 P0。
5. 过渡性的“截前 500 行”实现不发布；它可以保留为 Evidence Builder 的资源上限测试参考，但不能成为证据选择策略。
