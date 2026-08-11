# md 分级与检索优先级 —— 设计(项目 B)

> 类型:设计规格 · 日期:2026-08-11
> 前置:`docs/2026-08-10-vault-search-index-design.md`(检索 spec v3.2,已实现)、`2026-08-11-search-index-settings-design.md`(项目 A,先落地)

## 0 · 一句话

vault 里的 md 不是一种东西。把「你写的」和「原始资料」两极认出来并突出,中间的 AI 产物按类型自然分组 —— 让检索结果的**顺序和分组**反映这个事实。

## 1 · 为什么

同一个 vault 里至少三类 md,尺寸、可信度、可再生性都不同:

1. **原始事实数据** —— ebook 导出、字幕转写、博客原文。可能超过 1 MB,不可再生(丢了要重新抓),但也不是你的判断。
2. **AI 生成的中间产物** —— 摘要、答复、论证文档。可重新生成,代表你派给 agent 的预读任务。一般 1 MB 以内。
3. **人工写的笔记与标注** —— 通常很小,但**是任何模型都生成不出来的那部分**。

现在的排序对这三类基本一视同仁。产品主张(CLAUDE.md 信念 1)说的是「你留过判断的内容最有价值」,但索引侧只有两个弱信号在体现它:`is_annotation` ×1.2 和 `human_verified` ×1.1 —— 而后者在真实 vault 里只有 7 个文件带。

结果是:你搜一个词,一份 AI 摘要和你自己写的笔记按 bm25 混排,而摘要往往更长、词频更高。

## 2 · 关键发现:信号已经在了

盘过一遍,分级所需的信号**几乎全部已经写进文件、也已经进了索引**,只差没用:

| 信号 | 现状 |
| --- | --- |
| `files.ext`(`note.md` vs `md`) | 已存,只用于选分块器 |
| `files.concept_type`(frontmatter `type`) | **已存,排序完全没用** |
| `files.human_verified` | 已存,×1.1 |
| `blocks.agent_by`(大纲 `by::`) | 已存,×0.85,**但只覆盖 `.note.md` 的节点** |
| **`generated.by`** | **写文件时写了,`frontmatter.rs` 解析时只取 `at`,把 `by` 丢了** |

最后一条是这个项目的核心缺口:对散文 `.md` 而言,`generated.by` 是「这份文档是 AI 生成的」唯一可靠的一手声明,而我们把它扔了。

而且 `type` 不是自由文本 —— `src/lib/okf/concept.ts` 的 `CONCEPT_TYPE` 是项目内唯一登记表(15 个值),它几乎就是上面那三层。

**所以分级是推导出来的,不是配置出来的。** 用户不需要指定目录。

## 3 · `origin` 的推导

`files` 新增列 `origin TEXT`,三值 `human` / `derived` / `source`。索引时推导,**永不回写文件**(信念 2)。

按优先级,第一条命中即止:

| # | 条件 | 判定 |
| --- | --- | --- |
| 1 | 后缀 `.note.md` | `human` |
| 2 | frontmatter `generated.by` 存在 | `human:` 前缀 → `human`;否则 → `derived` |
| 3 | `verified` 含 `human:` 前缀的 `by` | `human` |
| 4 | `concept_type` 在登记表内 | 见 §3.1 |
| 5 | 路径在镜像目录(`sync_dir`,默认 `sync/`)下 | `source` |
| 6 | 完全没有 frontmatter | `source` |
| 7 | 其余(有 frontmatter,类型不认识) | `derived` |

### 3.1 类型 → 层的映射

- **human** — `Note`、`Outline Note`、`Daily Note`、`Wiki Page`、`Idea`、`Vault Conventions`
- **derived** — `Book Summary`、`Answer`、`Idea Proof`、`Reading Report`、`Decision Board`、`Decision Archive`
- **source** — `Book`

映射表放在 `searchidx` 里,并加一条测试**钉住它与 `CONCEPT_TYPE` 的同步**:登记表新增值而映射表没跟上时,测试失败。跨语言(TS 登记表 / Rust 映射表)靠共享 fixture,与本项目 outline 解析器的做法一致。

### 3.2 三条要写进代码注释的裁决

**文件级 `human` 与块级 `agent_by` 是两层,不冲突。** 一个 `.note.md` 整体是你的批注容器(`origin=human`),里面 agent 写的答复节点仍带 `by:: claude/1`、仍吃 ×0.85。容器是你的,某些句子是它写的。

**未登记的 `type` 判 `derived`(规则 7)。** 它有 frontmatter、有类型,说明被某个生产者刻意产出;OKF §11 也要求消费者不得因不认识 `type` 而拒绝文档。

**规则 6 的误判方向是刻意选的。** 裸 md 判 `source`,所以你手写但没加 frontmatter 的笔记会被排后、分进「原始资料」组。反方向的代价更大:AI 产物混进最该被信任的一层,降权同时失效。修正手段是给文件加 frontmatter —— 本来就是项目既有约定。

## 4 · 排序

`score_of` 增加一档乘性权重:

| origin | 系数 |
| --- | --- |
| `human` | ×1.25 |
| `derived` | ×1.0 |
| `source` | ×0.9 |

**与 `human_verified` ×1.1 叠加是有意的。** 规则 3 意味着签过字的文件同时拿 ×1.25 和 ×1.1,但两者语义不同:一个是「这类文档通常是人写的」,一个是「有人明确签了字」。后者更强,值得再加一档。

**这会改变每一个查询的排序。** 50 条 retrievability 回归集必须全部重跑,部分期望值可能需要重新固化 —— 按前置 spec §4,调这类常量本来就必须过回归集。**这是本项目最花时间的部分,不是加字段。**

## 5 · 分组展示

面板结果分组,顺序固定:

```
你写的            (origin = human)
  <类型 A>        (derived,按 concept_type 分)
  <类型 B>
  …
原始资料          (origin = source)
```

- 组内照常按分数排
- 组头显示条数
- 中间各组的标题就是类型名(`Book Summary`、`Answer`…),**所以组数 = 2 个极 + 结果里实际出现的类型数**,天然不止 3 组;插件引入新 `type` 就自动多一组,不改代码
- 空组不显示
- `derived` 里没有 `concept_type` 的(规则 7 命中)归入一个「其他」组,排在各具名类型之后

**查询过滤器**新增 `origin:human` / `origin:derived` / `origin:source`。`type:` 过滤器已存在 —— 「中间的通过类型区分」在查询侧是白拿的。

### 5.1 CLI 不分组

分组是 UI 侧的呈现,**CLI 的默认输出保持扁平的 `path:line:text`**。`notemd search` 的整个价值在于长得像 grep,agent 按行解析;插入组头会破坏那个契约。

CLI 侧只做两件事:
- `--json` 的每条命中增加 `origin` 字段(agent 可据此自行分层,与既有 `provenance` 字段并列)
- `origin:` 过滤器与 UI 共用同一个解析器,所以 `notemd search x origin:human` 直接可用

排序权重对 CLI **生效** —— 顺序变了,格式没变。

## 6 · 对项目 A 的回填

项目 A 的设置页预留了「分层统计」容器。B 落地后填入:

- 每个 `origin` 的文件数
- `derived` 内按 `concept_type` 的分布
- 一句说明:分级是推导的,改文件的 frontmatter 可以纠正

## 7 · schema 与迁移

新增 `files.origin` 列 → `SCHEMA_VERSION` 从 1 bump 到 2 → 所有人的索引在下次打开时**自动全量重建**。

这是既有设计的正常路径,不是意外:索引是可弃派生物,重建约 10 秒,不丢任何数据。无需迁移脚本。

## 8 · 测试

**推导规则** —— 每条规则至少一个 fixture,外加优先级测试:`.note.md` 且带 `generated.by: claude/1` 必须判 `human`(规则 1 压过规则 2);有 `type: Book Summary` 且在 `sync/` 下必须判 `derived`(规则 4 压过规则 5)。

**映射表同步** —— 与 `CONCEPT_TYPE` 的一致性测试(见 §3.1)。

**排序** —— 每个系数单独 mutation 验证:把某一档改成 ×1.0,必须且只能让对应的那条测试变红。**前置项目里同类测试出现过「两个乘数一起推同一方向、任一个单独失效测试仍通过」的假阴性,这里必须逐档隔离。**

**回归集** —— 50 条全部重跑;任何期望值变更都要逐条人工确认后再固化,不允许照着新输出批量刷新。

**分组** —— 空组不显示;组数随结果中出现的类型数变化;`derived` 无类型的归入「其他」并排在末尾。

## 9 · 残余风险

- **规则 6 会误判无 frontmatter 的人工笔记。** 已在 §3.2 声明方向与修正手段。设置页的分层统计是发现误判的主要途径 —— 如果「原始资料」的文件数明显偏高,说明 vault 里缺 frontmatter。
- **`origin` 与现有 `agent_by` / `human_verified` 加成叠加后的实际排序需要实测。** 理论上四个乘数最多叠到 ×1.25×1.1×1.2×1.2 ≈ ×1.98,不至于失控,但回归集是唯一的裁判。
- **`CONCEPT_TYPE` 是活的**(插件会加类型)。映射表同步测试能挡住「加了没映射」,但挡不住「映射到了错误的层」—— 那需要加类型的人自己判断,应在 `concept.ts` 的登记注释里写明这一点。
- **分组会让结果列表变长**(组头占位)。在结果很少时可能显得啰嗦;若实测如此,可在总数低于某阈值时退化为不分组。
