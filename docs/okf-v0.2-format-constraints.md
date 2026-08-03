# Open Knowledge Format (OKF) v0.2 格式约束文档

> 依据官方规范整理，带解释与示例。规范关键词遵循 RFC 2119：**MUST**（必须）/ **SHOULD**（应当）/ **MAY**（可以）。
>
> **官方完整文档（SSOT）**：
> - 规范全文：https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md
> - 仓库（含参考实现与示例 bundle）：https://github.com/GoogleCloudPlatform/knowledge-catalog
> - 官方介绍博客：https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing

---

## 1. 概览

OKF 是"开放的、对人和 agent 都友好的知识表示格式"，用于承载围绕数据与系统的元数据、上下文和策展洞见。四条设计原则：

1. 人类无需工具即可阅读（纯 Markdown）；
2. agent 无需专用 SDK 即可解析（YAML frontmatter）；
3. 在版本控制中可 diff；
4. 跨工具、跨组织、跨时间可移植。

v0.2 围绕机器生成知识回答五个问题：**来源**（从什么生成、如何验证）、**信任**（该信多少）、**新鲜度**（还成立吗）、**生命周期**（是否为当前版本）、**可证明性**（数字是否按规定方式算出）。

## 2. 术语

| 术语 | 定义 |
|------|------|
| Knowledge Bundle | 自包含的、层级化的知识文档集合；**分发单位** |
| Concept | bundle 内的单个知识单元 = 一个 Markdown 文档 |
| Concept ID | 概念文件在 bundle 内的路径，去掉 `.md` 后缀 |
| Frontmatter | 文档顶部由 `---` 包裹的 YAML 元数据块 |
| Body | frontmatter 之后的全部内容 |
| Link | 标准 Markdown 链接，表达层级之外的关系 |
| Source | 概念的派生材料（外部或内部） |
| Credibility signal | 客观的、逐来源的事实，用于推断信任 |
| Actor | 标识"谁/什么做了此动作"的字符串 |
| Trust tier | 由 `verified` 字段派生的信任层级 |

## 3. Bundle 结构

```
path/to/bundle/
  index.md                      # 可选：目录索引
  log.md                        # 可选：变更日志
  <concept>.md                  # 根级概念
  <subdirectory>/               # 用子目录分组
    index.md
    <concept>.md
    <subdirectory>/
      ...
```

**分发方式**：git 仓库（推荐）、tarball/zip、或大仓库中的子目录。

**保留文件名（规范性约束）**：

| 文件名 | 用途 | 约束 |
|--------|------|------|
| `index.md` | 目录索引 | **MUST NOT** 用作概念文档 |
| `log.md` | 变更历史 | **MUST NOT** 用作概念文档 |

其余所有 `.md` 文件都是概念文档。

## 4. 概念文档

每个概念文档由两部分组成：`---` 包裹的 YAML frontmatter + Markdown body。

### 4.1 Frontmatter 字段

| 字段 | 级别 | 说明 |
|------|------|------|
| `type` | **REQUIRED** | 标识概念种类的短字符串。类型值**不做中心注册**；消费者 **MUST** 优雅容忍未知类型 |
| `title` | RECOMMENDED | 人类可读的显示名 |
| `description` | RECOMMENDED | 单句摘要 |
| `resource` | RECOMMENDED | 唯一标识底层资产的 URI |
| `tags` | RECOMMENDED | 短字符串的 YAML 列表，用于横切分类 |

**扩展规则**：生产者 **MAY** 加入任意额外键；消费者往返处理时 **SHOULD** 保留未知键，且 **MUST NOT** 因未识别字段拒绝文档。

### 4.2 Body 约定

生产者 **SHOULD** 优先使用结构化 Markdown（标题、列表、表格、代码围栏）而非自由散文。约定标题（适用时 **SHOULD** 使用）：

| 标题 | 用途 |
|------|------|
| `# Schema` | 资产列/字段的结构化描述 |
| `# Examples` | 具体用例 |
| `# Computation` | Attested Computation 类型的受认可计算 |

### 4.3 示例：绑定资源的概念

```markdown
---
type: BigQuery Table
title: Customer Orders
description: One row per completed customer order across all channels.
resource: https://console.cloud.google.com/bigquery?p=acme&d=sales&t=orders
tags: [sales, orders, revenue]
generated: { by: reference_agent/gemini-2.5-pro, at: 2026-05-28T14:30:00Z }
---

# Schema

| Column        | Type      | Description                              |
|---------------|-----------|------------------------------------------|
| `order_id`    | STRING    | Globally unique order identifier.        |
| `customer_id` | STRING    | Foreign key into [customers](/tables/customers.md). |
| `total_usd`   | NUMERIC   | Order total in US dollars.               |
| `placed_at`   | TIMESTAMP | When the customer submitted the order.   |

# Joins

Joined with [customers](/tables/customers.md) on `customer_id`.
```

### 4.4 示例：不绑定资源的概念（如 Playbook）

```markdown
---
type: Playbook
title: "Incident response: data freshness alert"
description: Steps to triage a freshness alert on the orders pipeline.
tags: [oncall, incident]
generated: { by: human:ahormati, at: 2026-04-12T09:00:00Z }
---

# Trigger

A freshness alert fires when `orders` lags more than 30 minutes behind its
expected SLA. See the [orders table](/tables/orders.md).

# Steps

1. Check the [ingestion job dashboard](https://example.com/dash).
2. ...
```

## 5. 来源、信任与生命周期字段族

四个字段族**全部可选**。缺失本身携带含义：未验证的概念与已验证的可区分，但**绝不因此被拒绝**。

### 5.1 来源：`sources`

```yaml
sources:
  - id: ga4-schema
    resource: https://developers.google.com/analytics/bigquery/export-schema
    title: GA4 BigQuery Export schema
    author: team:ga4-docs
    usage_count: 5000
    last_modified: 2026-05-30
usage_window: { from: 2026-06-01, to: 2026-06-30 }
```

每条 source 的字段：

| 字段 | 级别 | 说明 |
|------|------|------|
| `resource` | 条目内 **REQUIRED** | 消费者可跟随的具体产物（绝对 URL / bundle 相对路径 / `references/` 内路径），或不可跟随的范围描述符 |
| `id` | 可选 | body 引用该来源时 **SHOULD** 提供，作为脚注键 |
| `title` | 可选 | 人类可读标签 |
| `author` | 可选 | 产出该来源的 actor（可信度信号） |
| `usage_count` | 可选 | `usage_window` 内的使用次数（活跃度信号） |
| `last_modified` | 可选 | 来源最后修改日期 `YYYY-MM-DD`（新近度信号，区别于 `generated.at`） |

`usage_window: { from, to }` 为所有 `usage_count` 提供统一时间窗。

**逐条断言归因**：用 Markdown 脚注，脚注标签 = `sources[].id`：

```markdown
The `events_` table is sharded daily as `events_YYYYMMDD`.[^ga4-schema]

[^ga4-schema]: GA4 BigQuery Export schema
```

设计意图：OKF 只记录**客观的逐来源信号**，由消费者自行判断信任，而不是让生产者自评"可信度分数"。

### 5.2 信任：`generated` 与 `verified`

```yaml
generated: { by: reference_agent/gemini-2.5-pro, at: 2026-06-20T22:53:05Z }
verified:
  - { by: human:ahormati, at: 2026-06-25T09:00:00Z }
  - { by: process:finance-nightly, at: 2026-06-26T02:00:00Z }
```

- `generated.by`：`generated` 内 **REQUIRED**，actor 格式见 §7；
- `generated.at`：ISO 8601 时间，指内容**最后一次实质变更**；
- `verified`：验证事件列表，每条含 `by`（actor）+ `at`（ISO 8601）；单个验证者 **MAY** 写成不带列表短横的裸 mapping，消费者 **MUST** 将裸 mapping 当作单元素列表处理；
- 两者独立：内容可以变更而未重新确认，事实也可以重新确认而无需重新生成。

### 5.3 信任层级（派生，非存储）

| 条件 | 层级 |
|------|------|
| 无 `verified` 键 | unverified |
| 仅被非 `human:` actor 验证 | machine-confirmed |
| 存在 `human:<id>` 验证 | human-reviewed |

信任层级是**建议性信号，不是访问控制**；消费者 **MUST NOT** 因缺信任数据而拒绝概念。

### 5.4 生命周期：`status` 与 `stale_after`

```yaml
status: stable          # draft | stable | deprecated
stale_after: 2026-09-23 # YYYY-MM-DD
```

| 值 | 含义 |
|----|------|
| `draft` | 未经审查，可能不完整 |
| `stable` | 默认值；可供消费。**缺省 `status` ⇒ `stable`** |
| `deprecated` | 为链接与历史保留；不再是当前版本 |

`stale_after` 为绝对日期；当 `today >= stale_after` 时概念视为过期（stale）。

## 6. 链接与路径

### 6.1 概念间链接

两种形式：

```markdown
See the [customers table](/tables/customers.md) for the join key.   <!-- 绝对（bundle 相对），推荐 -->
See the [neighboring concept](./other.md).                          <!-- 相对 -->
```

以 `/` 开头的绝对形式相对 bundle 根解释，**推荐**——文档在子目录内移动时链接仍稳定。

**语义规则**：A→B 的链接只断言"存在关系"；关系种类（父子、引用、join、依赖）由周围散文表达，链接本身不承载。消费者 **MUST** 容忍断链——目标不存在的链接不算格式错误。

### 6.2 路径值字段

适用于 `resource`、`sources[].resource`、`computation`、`executor.resource`、`attester.resource`，接受三种形式：绝对 URL、以 `/` 开头的 bundle 相对路径、普通相对路径（如 `../computations/revenue.md`）。例外：`sources[].resource` 还可为范围描述符。

### 6.3 `references/` 惯例

`references/` 子目录按惯例把外部材料、运行说明、代码镜像为 bundle 内的一等概念（如 `references/attesters/revenue.py`）。这是**命名惯例，不是要求**。

## 7. Actor 约定

所有身份字段（`generated.by`、`verified[].by`）统一格式：

| 格式 | 用途 | 示例 |
|------|------|------|
| `<producer>/<version>` | agent / 工具 | `reference_agent/gemini-2.5-pro` |
| `human:<id>` | 人 | `human:ahormati` |
| `process:<id>` | 自动流程 | `process:finance-nightly` |

信任分级以 `human:` 前缀为键，因此人工撰写或人工确认的内容**MUST** 使用 `human:` 前缀。

## 8. index.md

- **MAY** 出现在任意目录（含 bundle 根）；
- **frontmatter 限制**：仅 bundle 根的 `index.md` **MAY** 携带 frontmatter，且只允许 `okf_version` 一个键（这是 index.md 中唯一允许 frontmatter 的位置）；
- body 为一个或多个分组小节，列表项 **SHOULD** 带上被链概念 frontmatter 里的 description；
- 生产者 **MAY** 自动生成 index.md，消费者在缺失时 **MAY** 动态合成。

```markdown
# Section / Group Heading

* [Title 1](relative-url-1) - short description of item 1
* [Title 2](relative-url-2) - short description of item 2

# Another Section

* [Subdirectory](subdir/) - short description of the subdirectory
```

## 9. log.md

- **MAY** 出现在任意层级；结构为按日期分组的扁平列表，**最新在前**；
- 日期标题 **MUST** 用 ISO 8601 `YYYY-MM-DD`；
- 条目是散文；开头加粗词（`**Update**`、`**Creation**`、`**Deprecation**`）是惯例而非要求。

```markdown
# Directory Update Log

## 2026-05-22
* **Update**: Added a BigQuery table reference for [Customer Metrics](/tables/customer-metrics.md).
* **Creation**: Established the [Dataplex Playbook](/playbooks/dataplex.md).

## 2026-05-15
* **Initialization**: Created foundational directory structure.
```

## 10. Attested Computation（可证明计算）

独立概念类型 `type: Attested Computation`，设计动机：runtime 决定参数语义（SQL 绑定变量 / dbt var / Python 参数）；一个计算可被多个消费方（指标、看板、报告）链接复用；每个计算有独立的 `verified` / `stale_after` / `attester` 信任状态。

### 10.1 契约字段

| 字段 | 级别 | 说明 |
|------|------|------|
| `runtime` | **REQUIRED** | 执行环境，如 `bigquery`、`postgres`、`dbt`、`python`、`Looker` |
| `parameters` | 可选 | 类型化命名参数列表：`{ name, type, required }` |
| `computation` | 可选 | 外部计算文件路径；缺省则使用 body 中 `# Computation` 代码围栏 |
| `executor` | 可选 | `resource`（运行说明/代码）+ `receipt`（运行返回的字段列表） |
| `attester` | 可选 | `resource` 指向确定性验证代码，产出裁决 |

```yaml
---
type: Attested Computation
title: Revenue for fiscal year
description: Recognized revenue for a fiscal year, per Finance's definition.
status: stable
runtime: bigquery
parameters:
  - { name: year, type: integer, required: true }
executor:
  resource: references/skills/run-on-bq.md
  receipt: [job_id, executed_sql, result]
attester:
  resource: references/attesters/revenue.py
generated: { by: reference_agent/gemini-2.5-pro, at: 2026-06-20T22:53:05Z }
verified: { by: human:ahormati, at: 2026-06-25T09:00:00Z }
stale_after: 2026-09-23
sources:
  - id: rev-policy
    resource: https://wiki.acme/finance/revenue-recognition
    title: Revenue recognition policy
---
```

### 10.2 计算本体

内联（短计算推荐）写在 `# Computation` 下：

```markdown
# Computation

    SELECT SUM(amount) AS revenue
    FROM finance.recognized_revenue
    WHERE fiscal_year = @year
```

或外部文件（长/共享计算）：`computation: references/computations/lib/revenue.sql`。

**核心红线**：agent **MAY** 只为声明的 `parameters` 提供*值*，**MUST NOT** 撰写或修改计算本身。消费方绑定参数值，attester 独立重推该绑定并与实际执行比对。

### 10.3 消费流程（资料性，非规范）

发现（按 `type`）→ 加载契约与计算 → 参数化 → 执行并取回 `receipt` → 运行 attester 出裁决 → 门禁（attest 失败拒绝展示；`today >= stale_after` 时警告或拒绝）。

### 10.4 verified vs. attestation

`verified` 确认**定义**符合政策——文档级、慢、存于 bundle；attestation 确认**单次运行**用了受认可的方式——逐调用、运行时、不存储。二者并存：过期的定义可能仍能 attest 通过，新验证的定义每次运行仍需 attest。

## 11. 一致性（Conformance）

**Bundle 合规三条件**：

1. 每个非保留 `.md` 文件含可解析的 YAML frontmatter；
2. 每个 frontmatter 含非空 `type` 字段；
3. 保留文件名存在时遵循 §8 / §9 结构。

**消费者规范性规则**：

- **MUST** 将裸 `verified` mapping 当作单元素列表；
- **MUST NOT** 因缺任何可选字段族而拒绝概念；
- **SHOULD** 仅从规范定义的字段派生信任层级与过期状态。

**宽容一致性——消费者 MUST NOT 因以下情况拒绝**：缺可选 frontmatter 字段、未知 `type` 值、未知附加键、断链、缺 `index.md`。其余约束 **SHOULD** 视为软性指导。

## 12. 版本化

- 版本号 `<major>.<minor>`：minor = 向后兼容的新增（新可选字段、新约定标题）；major = 破坏性变更（重命名必填字段、更改保留文件名）；
- bundle **MAY** 在根 `index.md` frontmatter 中声明 `okf_version: "0.2"`；
- 不识别所声明版本的消费者 **SHOULD** 尽力消费而非拒绝。

## 13. v0.1 → v0.2 变更

**破坏性**：`timestamp` → `generated.at`（消费者 **MAY** 在 `generated` 缺失时回退读 `timestamp`）；body 的 `# Citations` 列表 → frontmatter `sources`（消费者 **MAY** 兼容解析旧文档）。

**新增**：`sources`（含可信度信号）、`usage_window`、`generated`、`verified`、`status`、`stale_after`；新类型 `Attested Computation`；新约定标题 `# Computation`；actor 约定。其余（bundle 布局、保留文件名、必填 `type`、链接、index/log、宽容一致性）与 v0.1 一致。

## 附录：官方完整示例（损益表，v0.2 形态）

指标概念只做叙述，数字交给独立的可证明计算：

**`metrics/income-statement.md`**

```markdown
---
type: Metric
title: Income statement (fiscal year)
description: Headline income-statement figures for a fiscal year.
tags: [finance, income-statement]
status: stable
generated: { by: reference_agent/gemini-2.5-pro, at: 2026-06-20T22:53:05Z }
verified: { by: human:ahormati, at: 2026-06-25T09:00:00Z }
stale_after: 2026-12-31
sources:
  - id: fpa-handbook
    resource: https://wiki.acme/finance/fpa-handbook
    title: FP&A reporting handbook
---

# Definition
The income statement reports [revenue](../computations/revenue.md) and
[gross profit](../computations/profit.md) for a fiscal year, per the FP&A
reporting handbook.[^fpa-handbook] Each figure is produced by a sanctioned,
attestable computation; this concept only narrates them.

[^fpa-handbook]: FP&A reporting handbook
```

**`computations/profit.md`**（dbt runtime、process 验证、已过期——展示多 runtime 与 stale 状态）

```markdown
---
type: Attested Computation
title: Gross profit for fiscal year
description: Gross profit by segment for a fiscal year, per the cost-allocation standard.
tags: [finance, profit]
status: stable
runtime: dbt
parameters:
  - { name: year, type: integer, required: true }
  - { name: segment, type: string, required: true }
executor:
  resource: references/skills/run-dbt.md
  receipt: [run_id, compiled_sql, result]
attester:
  resource: references/attesters/dbt-binding.py
generated: { by: reference_agent/gemini-2.5-pro, at: 2026-06-14T14:00:00Z }
verified: { by: process:finance-nightly, at: 2026-06-12T08:00:00Z }
stale_after: 2026-06-15
sources:
  - id: cost-alloc
    resource: https://wiki.acme/finance/cost-allocation
    title: Cost allocation standard
---

# Computation

    SELECT gross_profit
    FROM {{ ref('fct_income_statement') }}
    WHERE fiscal_year = {{ var('year') }}
      AND segment = {{ var('segment') }}

Gross profit by segment per the cost-allocation standard.[^cost-alloc]

[^cost-alloc]: Cost allocation standard
```

（revenue 计算的完整示例见规范 Appendix A。）

## 速查：硬约束一览

生产者侧真正的硬约束只有三条：非保留 `.md` 必须有可解析 YAML frontmatter；frontmatter 必须有非空 `type`；`index.md`/`log.md` 不得用作概念文档（且遵循其结构）。其余全部是 RECOMMENDED/MAY 或针对消费者的宽容义务——这正是 OKF"极简约束"哲学的体现。
