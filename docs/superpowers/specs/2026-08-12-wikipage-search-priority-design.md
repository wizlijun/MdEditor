# wikipage 检索优先级设计

日期：2026-08-12
状态：设计已确认，待实施计划

## 1. 目标

检索结果里，wikilink 相关的内容要拿到更高的优先级，具体两条：

1. **精确页置顶**：搜关键词 K 时，wikilink 目录下名为 K 的那篇笔记硬置顶为第一条。
2. **链接提及加权**：任何命中块里 K 以 `[[…K…]]` 形式出现的，分数 ×1.5。

「wikilink 目录」= vault 设置里的 `wikipageDir`（默认 `wikipage`，存 `{vault}/.notemd/settings.json`，
见 `src/lib/outline/dirs.svelte.ts:6` 与 `src-tauri/src/sotvault/vault_settings.rs:27`）。
**这个目录名用户随时可改**，本设计的所有判定都必须在目录改名后立即生效，不得要求重建索引。

## 2. 现状与必须先解决的召回问题

排序全部在 `searchidx::query::score_of`（`searchidx/src/query.rs:597`）：bm25 取负 → 一串乘法加权
（phrase / level / 批注 / human_verified / origin 档位 / agent 惩罚 / 时间衰减）→ `r/(1+r)` 压缩。
searchidx 完全不知道 `wikipageDir` 的存在。

更关键的是：**wikipage 的页名现在根本搜不到**。

- FTS 只索引 `blocks.tok_text` 和 `tok_breadcrumb`（`searchidx/src/store.rs:42`）。
- `files.title` 存了，但没进 FTS。
- breadcrumb 只是标题链，不含文件名（`searchidx/src/prose.rs`、`outline.rs`）。
- 通过 wikilink 建的页面正文就是 `- `，标题只在 front-matter 里（`src/lib/outline/create.ts:11`）。

所以搜「张三」命中不了 `wikipage/张三.md` —— 不是排序低，是这条结果不存在。目标 1 因此必然是两件事：
**先让页名可检索，再把它顶到最高**。

## 3. 索引侧：标题进 FTS

给 `blocks_fts` 加第三列 `tok_title`，**只在 File 级块写入**，其余块写空串；bm25 调用相应改为
`bm25(blocks_fts, 1.0, 2.0, 4.0)`。

写入内容为 `tokenize(title)` 与 `tokenize(文件名 stem)` 两份的拼接。

### 决策理由

- **为什么单开一列，而不是把标题拼进 File 块的 `tok_text`**（后者改动更小）：File 级块的 text 是整篇正文，
  bm25 的长度归一化会把一两个标题词稀释到几乎没有权重，标题命中会排得很后，「搜文件名找文件」这个
  普遍收益基本落空。单开一列才能给标题一个明确、独立、可调的权重。既然这次注定要全量重建索引，一次做对。
- **为什么只给 File 级块**：给每个块都写，会让「搜张三 → 张三.md 的每一段都命中」，噪声爆炸。
  File 级块每个文件恰好一条，正是「这篇文档叫什么」的粒度。
- **为什么 title 和文件名 stem 都要写**：wikipage 建页是「文件名 slug 化、fm title 存原文」
  （`src/lib/outline/create.ts:23`），两者可能不同；而 wikilink 在本产品里是**按文件名解析**的，
  所以两个都必须可搜。`chunk::parse_file` 现有的 `meta.title` 回落链是 `fm.title → 首个 H1 → stem`
  （`searchidx/src/chunk.rs:43`），当 `fm.title` 存在时 stem 不在其中，必须单独取。
- **展示不受影响**：`blocks_fts.tok_text` 与 `blocks.text` 本来就是分离的两份数据
  （`store.rs:395` 写入 `tokenize(&b.text)`），新列同理。命中预览不会突然多出标题。

### 迁移

`SCHEMA_VERSION` 从 2 提到 3（`searchidx/src/store.rs:27`）。老索引在 `open` 时自动 wipe + 全量重建，
用户无需操作；大 vault 有一次性重建开销，日志与设置页进度条可见。

> **合并顺序约束**：兄弟 worktree `source-globs-transcripts` 已把 `SCHEMA_VERSION` 提到 3，
> 并在同一批文件（`searchidx/src/store.rs`、`query.rs`、设置页）上做 source globs / 权重 / 4 档统计的大改。
> 两边必须串行合并，后合的一方负责再提一次版本号并解决 `blocks_fts` 建表语句与 `bm25()` 列权重的冲突。
> 绝不整体 merge，按既有姿势逐个解冲突。

### 已知的行为面扩大

这不只影响 wikipage —— **全 vault 的文件名/标题都变成可搜的**。「搜文件名找文件」本来就是刚需，
判断为净收益，但结果列表里会多出一批 file 级命中。这是「标题进 FTS」这条路的必然结果，已确认接受。

## 4. 查询侧：硬置顶

### 配置通路

`wikipageDir` **走查询侧传参，不进索引**，这样目录改名立刻生效、不用重建索引（§1 的硬要求）。

- `searchidx::Limits` 增加 `wikipage_dir: Option<String>`；`SearchIndex::search_with` 透传给 `query`。
- 旧的 `SearchIndex::search()` 接口传 `None`，行为不变。
- host 侧在 `src-tauri/src/search/options.rs` 旁边加 `query_options`，从 `vault_settings` 读 `wikipage_dir`，
  默认值与 `src-tauri/src/plugin_runtime/ui_rpc.rs:102` 的 `DEFAULT_WIKI_DIR` **共用同一个常量**，不再抄第二遍。
- GUI 与 CLI 走同一个函数，比照 `options.rs` 文首「一个算法，三个 adapter」的既有约束，
  并补一个与 `src-tauri/tests/search_scan_options_contract.rs` 同款的契约测试。

### 触发条件

查询恰好是单一关键词（`q.terms.len() + q.phrases.len() == 1`）**且**不带任何过滤器
（`tags` / `types` / `paths` / `exts` / `origins` / `pages` / `after` / `before` 全空）。
多词查询谈不上「这个关键词的页」。

### 判定

hit 所属文件满足：

- `path` 以 `{wikipageDir}/` 开头，且
- 文件名 stem **或** `files.title` 与关键词精确相等（trim + 大小写不敏感）。

### 实现

`Hit` 增加查询期字段 `pinned: bool`（不落库），`query::finish` 的排序改为 `(pinned 降序, score 降序)`。

判定口径是「**该 hit 所属文件**是精确匹配的 wikipage」，而不是「该 hit 是 File 级块」。原因：
若该文件正文里也含关键词，`drop_redundant_rollups`（`query.rs:552`）会干掉 File 级 rollup、
只留下 line 级 hit —— pin 挂在文件上才不会被这个既有机制吃掉。同一文件若留下多条 hit，
它们都 pinned，内部按 score 排，第一条仍是该文件，「置顶」成立。

### 前端

搜索面板按 origin 分组渲染（`src/lib/search/grouping.ts`）。置顶 hit 若 origin 是 `derived`，
会被塞进中间那组，「第一条」在视觉上不成立。所以：

- `groupHits` 先把 pinned hits 抽出来单独成组排最前，新增 `HitGroupKind: 'pinned'`。
- 组标题走 i18n，四语齐全（en / zh / ja / de）。
- CLI 输出是扁平的 `path:line:text`，天然按顺序，不改。

## 5. 查询侧：`[[…]]` 提及 ×1.5

在 `score_of` 现有乘数群里加一档：命中块文本中存在 `[[…]]`，且其 target 匹配查询词时 `r *= 1.5`。

### 口径（已确认）

- **target 包含关键词即算命中**，不要求精确相等。搜「张三」时，含 `[[张三的项目]]` 的块同样加权。
  比对方式：取 `[[…]]` 内 `|` 之前那段并 trim，做大小写不敏感的子串包含判断。
- **不限定单一关键词**。搜「张三 电话」时，含 `[[张三]]` 的块照样加权。任一查询词命中即加权，
  加权只加一次（不按命中词数叠乘）。

### 实现理由

直接扫 `hit.text`，不查 `links` 表：links 表带行号、能精确到行，但 File / Section 级块跨多行，
join 起来要判区间，SQL 复杂度不值当；而 `hit.text` 在 `finish` 里就在手上，零成本。
`links` 表继续只服务已有的 `page:` 过滤器。

1.5 这个常数写进 `score_of` 现有常数群，注释注明它是产品诉求、不是调参结果 ——
该函数的文档注释明确要求改常数要重跑召回回归集。

## 6. 测试

### searchidx 单测

- 标题可检索：建 `wikipage/zhang-san.md`（fm `title: 张三`，正文只有 `- `），搜「张三」与「zhang-san」都能命中。
- 展示不污染：上述命中的 `hit.text` 不含标题文本。
- 非 File 级块不因 title 命中。
- 置顶：wikipage 精确匹配排第一，哪怕另一篇 bm25 高得多、或 origin 是 `human`。
- 置顶不误伤：多词查询不 pin；带任一过滤器不 pin；非 wikipage 目录下的同名文件不 pin。
- 目录改名：改 `wikipage_dir` 后，旧目录立即不再 pin、新目录立即 pin，且**不重建索引**（这条是 §1 硬要求的钉子）。
- `[[…]]` ×1.5：照 `score_of_boosts_human_verified_content` 的既有写法直接调 `score_of` 断言纯函数，
  不受 bm25 / SQLite / fixture 长度影响。
- `[[张三的项目]]` 在搜「张三」时**加权**（口径 §5 的钉子，防止将来有人改回精确匹配）。
- 多词查询「张三 电话」时 `[[张三]]` 仍加权。

### host

- `query_options` 契约测试：GUI 与 CLI 取到同一个 `wikipage_dir`。

### 前端

- `grouping.test.ts`：pinned 组排最前；无 pinned 时该组不渲染。
- i18n `strings.test.ts` 风格：四语齐全。

## 7. 不做的事

- 不给整个 wikipage 目录无差别加权（「关键词」这个限定必须保留，否则等于给整个目录开后门）。
- 不把 `wikipageDir` 写进索引（会让目录改名需要重建索引）。
- 不改 `notemd search` CLI 的扁平输出格式。
- 不做历史索引的增量迁移 —— schema 版本号跳变即全量重建，是既有约定。
