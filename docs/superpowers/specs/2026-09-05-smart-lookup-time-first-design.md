# Smart Lookup 时间优先检索设计

> 类型：增量产品与技术规格 · 日期：2026-09-05 · 状态：已实现

## 1. 目标

用户提交自然语言问题后，Smart Lookup 的 Planner 必须先识别时间，再生成内容查询。时间表达是候选集约束，不是正文关键词。

本改动保持低 token、单次 fast `search-plan` 调用，并保持 `SearchPlanV1` wire 完全不变。宿主在调用 Planner 前，基于同一份冻结的 `referenceTime + IANA timezone` 生成可信时间锚点；Planner 只做语义匹配，不自行计算常见相对日期。

输入过程中的 FTS-only 预览保持不变。它只提供即时反馈，不参与 Enter 后的权威智能结果。

## 2. 处理顺序

```text
冻结 referenceTime + timezone
  → 宿主生成 referenceDate、可信时间锚点并锁定显式 filters
  → Planner 先逐字提取时间线索、分类时间对象、选择有界 expression
  → Planner 再生成主题 terms/phrases 与最多两个查询臂
  → 宿主校验完整 SearchPlanV1 和 document_date 来源片段
  → 宿主解算 expression，并与显式 after/before 求交
  → 宿主丢弃仍混入时间片段的整个 term/phrase
  → 将同一最终范围复制到全部 typed queries
  → 访问索引
```

Planner 失败、范围非法、来源文字不是原问题片段或范围无交集时，智能结果 fail closed；界面保留此前的本地预览，但不得将其冒充为时间受控结果。

## 3. 时间契约

- `SearchPlanV1` 的枚举、字段必需性和 `schemaVersion: 1` 不变，不引入隐式 V2。
- 宿主上下文新增只读、可重算的 `referenceDate` 与可信时间锚点：今天/昨天/明天，本/上/下周，本/上/下月，本/上/下季度，本/去/明年。
- 相对周/月/年使用现有 `calendar_week/calendar_month/year` 交给宿主解算；相对日与相对季度使用现有 `absolute_range { after, before }`，两端逐字复制宿主锚点。
- 明确季度继续使用 `quarter`；滚动时长继续使用 `rolling_window`；完整显式日期区间继续使用双端 `absolute_range`。
- `rolling_window` 两端均包含：N days 为含今天在内恰好 N 个日期，N weeks 为 N×7 个日期。月/年采用半开日历区间 `(reference - N units, reference]`，再转换为包含两端的日期范围。这修复旧实现多包含一天的语义错误；V1 wire 不变，但旧计划重放将得到修正后的边界。
- 缺年、开放边界等 V1 不能由宿主可靠表达的时间写入 ambiguity，不让模型猜年份或月末。

## 4. 时间来源与分类

- Planner 必须先遍历问题中的每个时间表达，再规划检索。多个表达无法合并为一个文档日期窗口时标记 `ambiguous`；属于正文事件日期的表达保留为主题文本，不得任取或静默丢弃。
- Planner 输出非空 `document_date` 时，`sourceText` 必须是原问题中的最小连续时间片段；宿主只对会缩小候选集的 `document_date` 强制此校验。宿主不在词法层替 Planner 猜测时间对象，避免把标题、专名或正文事件日期误判成文档日期。
- 对宿主识别的常见相对日/周/月/季度/年，最终解算范围必须与 `sourceText` 对应锚点完全一致，否则在访问索引前拒绝。
- `document_date` 只接受可由宿主确定性核验的表达：可信固定锚点、显式年份/季度、紧凑或空格分隔的“最近/过去 N 天、周、月、年”，以及 ISO 单日或由“到/至/破折号/波浪号”明确连接的双端闭区间。“和/与/及”连接的离散日期不得扩成连续区间。数字、单位与日期端点必须与 `sourceText` 一致；混杂说明文字、多个滚动窗口或三个以上日期会在访问索引前拒绝。
- 原问题没有时间线索时不估算日期范围；`time: null` 保持全范围召回。
- “今天”“最近”“当前”“最新”“本周”等属于时间线索，但仍须区分：它们是在限定要找的文档，还是问题所谈事件的日期。
- `content_date` 不生成文档日期过滤器；日期文字可作为正文主题保留。
- `activity_time` 与 `ambiguous` 不偷换为文档日期，分别进入 unsupported/ambiguity 提示。
- Planner 不得把 `document_date sourceText` 放入 terms/phrases。宿主兜底时丢弃整个受污染值，绝不做可能产生“的发布”一类残词的子串拼接；若某个原有主题文本的查询臂因此变成纯日期查询，则拒绝整份计划。

## 5. 兼容性与失败策略

- 旧 V1 计划省略可选 `time` 仍可解析；新 Prompt 始终要求输出顶层 `time`，无范围时为 `null`。
- 不新增第二次 LLM 调用、repair、Tune 或索引扫描。
- 同一冻结时间、时区与已解析计划用于浅检索和后续 deep 重跑。
- 显式 DSL `after:/before:` 始终锁定；与自然语言时间范围无交集时停止，不做无界降级。

## 6. 验收

- 在 `2026-09-04T16:30:00Z + Asia/Taipei` 下，宿主锚点为今天 09-05、昨天 09-04；上周和跨年季度边界由宿主测试锁定。
- 最近 1 天为当天；最近 7 天为 08-30…09-05；月末和闰年的月/年滚动语义有回归测试。
- 所有展开后的物理查询共享同一 `after/before`；搜索索引的日期过滤测试证明范围外文档不命中。
- 前端 Prompt 和三家官方 Agent 模板使用同一“时间闸门 → 主题查询”顺序与同一 V1 表达。
- 无时间问题不附加日期范围；非法 document-date 计划快速失败并保留 preview-only。
