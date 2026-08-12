# 搜索面板可读性三项改动 — 设计

日期:2026-08-12
状态:已确认,待实施

## 背景

搜索面板(`src/components/side-panel/SearchPanel.svelte`)当前把后端返回的命中直接铺平渲染:
`hit.text` 是原始 markdown 块,带 `##`、`**`、`[[…]]`、CriticMarkup 等标记;同一个文件的多条命中
各占一行,重复的文件名把列表撑长;关键词没有任何视觉标记。另一侧,`log_bus` 的分类下拉里没有
`search` 这一项,而后端其实已经在用这个分类打索引日志,用户看不到也筛不出。

三项改动互相独立,共用一次实施:

1. 日志窗口支持按「索引与搜索」分类过滤,并补上查询侧的日志行。
2. 搜索结果文本剥掉非文字标记,并高亮关键词。
3. 搜索结果按文件分组折叠,在窄侧栏里不增加缩进。

## 一、日志:索引与搜索分类

### 现状

后端已经在用 `crate::log_cat!("search", …)` 打日志,覆盖:初始建库、重建、watch 触发的重索引、
超阈值文件跳过(`src-tauri/src/search/mod.rs`、`src-tauri/src/search/watch.rs`)。
**查询本身一行不打。** 前端 `src/logs-app.svelte` 的分类下拉只列了
`core / git-sync / notification / frontend / plugin:*`,`search` 分类的行只能在「全部」里混着看。

### 改动

**后端** — `notemd_search` 命令在返回 DTO 前补一行 debug:

```
[search] debug  query="外骨骼" route=t1-fts hits=12 34ms deep=false truncated=false
```

`hits` 只有一个数 —— `SearchResponse.total` 与返回条数在后端本来就是同一个值
(`total: answer.hits.len()`),写成「返回/总数」会是假精度。被更新 ticket 抢占而中止的查询打:

```
[search] debug  query="外骨骼" superseded
```

**级别固定 debug**,不是 info。理由写在 `mod.rs:461` 那条既有注释里:`log_bus` 是全局共享的
3000 行环形缓冲,git sync、插件、核心都往里写;查询行随打字频率产生,若默认可见会把其它来源的
日志顶出缓冲区。debug 级可以被 Logs 窗口的 level 过滤器挡掉,需要时再放出来。

**前端** — `src/logs-app.svelte`:

- 分类下拉在 `git-sync` 之后插入 `<option value="search">`。
- `catClass()` 增加分支:`cat === 'search'` → `cat-search`,样式用 `light-dark()` 一对颜色,
  与既有 `cat-git` / `cat-notif` 同风格。
- 现有的 `matchCategory` 是精确匹配,`search` 不需要额外逻辑。

**i18n** — 四个 locale(`en` / `zh` / `ja` / `de`)补 `logs.categories.search`:

| locale | 值 |
| --- | --- |
| en | `Index & Search` |
| zh | `索引与搜索` |
| ja | `インデックスと検索` |
| de | `Index & Suche` |

索引侧的日志调用点不改。

## 二、结果预览行 + 文本清洗 + 关键词高亮

### 为什么是「预览行」而不是「清洗整块」

`hit.text` 是一整个块。实测本仓库对应的 vault 里,`.md` 正文含 1607 个 ```` ```json ````
围栏、1831 个 ```` ```bash ````、1048 个 ```` ```mermaid ````(mermaid 节点文本里全是
`<br/>`)。命中落进这类围栏时,面板会把整个几十行的 JSON 砸进侧栏 —— 这才是「结果里全是 json、
html 标记」的根因,单靠剥标记解决不了。

所以预览取**命中所在的那一行**,不是整块。围栏内容本身按原样显示(代码里的引号括号是语义,
剥掉反而认不出来),只在行前标一个语言标签。

### 新模块 `src/lib/search/preview.ts`

纯函数,不 import Svelte,与 `src/lib/search/grouping.ts` 同规格 —— 面板调用它,逻辑本身在
单测里验证,不需要组件 harness。

#### `previewLine(raw: string, terms: string[]): { text: string; lang: string | null }`

面板的入口。`terms` 由下面的 `parseHighlightTerms(query)` 提供。

**块级预处理**(必须在按行切分之前,因为这些结构跨行):

1. **frontmatter** — 开头的 `---` … `---` 整体删除。
2. **HTML 注释** — `<!--` … `-->` 整体删除。
3. **`<script>` / `<style>` 块** — 连标签带内容整体删除(内容不是正文)。

**按行选取**:逐行扫描,同时跟踪围栏状态(` ``` ` 或 `~~~` 起止,记录 info string 的第一个
词、小写、截断到 12 字符作为 `lang`;不在围栏内时 `lang = null`)。围栏定界行本身永远不会被
选为预览行。对每个非空、非定界的行:先过 `cleanLineText`,若结果非空且**包含任一 term**(大小
写不敏感),即返回该行与其 `lang`。

**回退顺序**:没有任何行命中 term → 取第一个 `cleanLineText` 结果非空的非定界行 → 仍然没有
(整块都是标记)→ 返回 `{ text: '', lang: null }`,面板此时只渲染 `L12` 与文件名,不渲染空预览。

term 匹配跑在 `cleanLineText` 的**输出**上而不是原始行上:`**外**骨骼` 这类被标记切开的写法,
只有清洗后才含有连续的 `外骨骼`。清洗是懒执行的,扫到第一个命中行即停。

#### `cleanLineText(line: string): string`

行级清洗,按以下顺序,后一步作用在前一步的输出上:

1. **HTML 标签** — `<[^>]*>` 删除标签本身,保留标签间文字。(mermaid 的 `<br/>` 走这条。)
2. **HTML 实体** — 在标签剥离**之后**解码:`&nbsp;` → 空格,`&amp;` `&lt;` `&gt;` `&quot;`
   `&#39;` `&#x27;`,以及数字实体 `&#\d+;` / `&#x[0-9a-fA-F]+;`。解码后**不再重跑标签剥离** ——
   原文里写 `&lt;div&gt;` 是想让人看见 `<div>` 这几个字,不是一个标签。
3. **图片** — `![alt](url)` 与 `![[file]]` 整体删除(连 alt 一起,alt 不是正文)。
4. **wikilink** — `[[a|b]]` → `b`,`[[a]]` → `a`。
5. **markdown 链接** — `[t](u)` → `t`。
6. **CriticMarkup** — `{==x==}` → `x`,`{++x++}` → `x`,`{--x--}` → 删除,
   `{>>x<<}` → 删除(批注内容本身也去掉),`{~~a~>b~~}` → `b`。
7. **行首块标记** — `#{1,6} `、`> `、`- ` / `* ` / `+ `、`1. `、`- [ ] ` / `- [x] `。
8. **行内标记** — `**`、`__`、`*`、`_`、`` ` ``、`~~`、`^^`、`==`。
9. **分隔线** — 整行为 `---` / `***` / `___` 的行清空。
10. **空白折叠** — 连续空白折叠为单个空格,首尾 trim。

顺序是有约束的,不能随意调换:HTML 实体必须在标签之后(见上),wikilink 必须在行内标记之前
(否则 `[[a_b]]` 里的 `_` 会被当强调吃掉),图片必须在 md 链接之前(`![](…)` 是 `[](…)` 的
超集),CriticMarkup 必须在行内标记之前(`{==x==}` 的 `==` 会被高亮标记规则误吃)。

**围栏内的行同样过这套清洗。** 代价是 JSON 里的 `**` 之类会被误剥,但收益是 mermaid 的
`<br/>`、HTML 片段、markdown 围栏里的示例标记都被处理掉;JSON 的结构符号(`{}` `[]` `"` `:`
`,`)不在规则表里,原样保留。

`✦`(agent 写的)和 `●`(人工确认)两个 marker 由面板作为独立 `<span>` 渲染,**不经过此函数** ——
它们是 UI 元数据,不是文本里的标记。

#### `parseHighlightTerms(query: string): string[]`

query 解析镜像后端 `searchidx/src/query.rs` 的 `split_respecting_quotes`:

- 空白分词,双引号内的空白不分词;闭合引号内的内容是短语,原样进高亮词表。
- 丢弃过滤器 token:前缀为 `tag:`、`type:`、`path:`、`ext:`、`origin:`、`after:`、`before:`、
  `page:` 的 token 不参与高亮 —— 它们约束的是文件属性,不是正文内容,高亮它们会误标。
- 剩余 token 与短语构成高亮词表。

#### `highlightParts(text: string, terms: string[]): Array<{ text: string; hit: boolean }>`

面板把 `hit === true` 的段渲染成 `<mark>`。大小写不敏感;**按词长降序**依次匹配,已匹配区间不再
参与后续匹配(避免 `外骨骼` 和 `骨` 同时在词表时产生嵌套/重叠段);未匹配的文字原样成段。词表
为空或 `text` 为空时返回单个 `{ text, hit: false }`。

## 三、按文件分组折叠(窄栏版)

### 数据结构

`src/lib/search/grouping.ts` 的 `HitGroup.hits: SearchHit[]` 改为 `HitGroup.files: FileGroup[]`:

```ts
export interface FileGroup {
  path: string      // vault 相对路径,分组键 + 悬停 title
  absPath: string   // 打开文件用
  name: string      // basename,显示用
  hits: SearchHit[]
}
```

文件在组内的顺序 = 该文件**首次出现**的次序。`hits` 上游已按分排序(`searchidx::query::finish`),
所以首次出现即该文件的最高分命中,不需要另行排序。组内 `hits` 保持原有相对顺序 —— 这条约束与
现有 `groupHits` 的「本函数从不重排」注释一致。

`HitGroup` 的其余部分(`kind`、`conceptType`、组的排列顺序、空组省略)全部不变。命中总数
(组头右侧那个数)仍是命中条数,不是文件数。

### 视觉与交互

```
人写的                          3
▸ 会议纪要.md                   2
  今天和 X 谈到了外骨骼的髋关节…
▾ 读书笔记.md                   5
│ L12  外骨骼的能量回收路径
│ L88  json  "entity_boost": ...,  # 实体加成贡献
```

侧栏很窄,所以三层结构**只允许一级、8px 的缩进**:

- **类型组头**与**文件行**齐左,共用现在的 `padding: 4px 8px` 基线,文件行不再因为多一层而缩进。
  类型组头保持现有 11px 小写标签样式。
- **命中行**缩进 8px,并以 1px `border-left` 画导引线 —— 用线而不是空白表达从属关系。
- **删掉命中行现有的 `hit.path:line` 尾行**,只留 `L12`。文件名已经在上一行,重复路径既占宽又占高。
- **`breadcrumb` 只在展开的命中行显示**,折叠预览行不显示。
- 文件名只显示 basename;完整 `path` 挂 `title` 悬停查看。
- 命中数用 `margin-left: auto` 右对齐贴边,不占左侧宽度。
- 折叠预览行(首条命中的清洗+高亮片段)**不缩进**,直接接在文件行下,靠 12px 字号与低透明度区分。
- 展开箭头用 CSS `rotate` 的三角,固定 10px 宽,与文件名间隔 4px。不用字符字形 —— 见
  `button` 不继承 `font-size` 的既有坑。
- `previewLine` 返回的 `lang` 非空时,在预览文本前渲染一个淡色小标签(`json` / `mermaid` /
  `bash`),10px 字号、不换行、不参与省略截断。`lang` 为 `null` 时不占位。

交互:

- **默认全部折叠**。折叠行 = 文件名 + 命中数 + 首条命中预览。
- 点折叠行展开/收起该文件;展开后点具体命中行走现有 `onOpenHit(hit)` 打开并定位。
- **单命中文件不做折叠**:仍渲染为一行,点击直接打开该命中。展开只会显示同一条命中,零信息增量。
- 展开状态存组件本地 `$state<Set<string>>`(键为 `path`)。**`searchStore.query` 变化时清空** ——
  否则上一次查询的展开集会挂到新结果的同名文件上。
- 文件行是 `<button aria-expanded={…}>`;单命中文件的行不设 `aria-expanded`(它不是可展开控件)。

## 测试

- `src/lib/search/grouping.test.ts` — 补文件分组用例:同文件多命中聚合、文件顺序取首次出现、
  组内 hits 不重排、跨类型组的同名文件互不合并。
- `src/lib/search/preview.test.ts`(新)— `cleanLineText` 每条规则一例 + 顺序敏感的组合例
  (`&lt;div&gt;` 不被二次剥、`[[a_b]]`、`![](…)`、`{==x==}`);`previewLine` 的围栏语言识别、
  定界行不入选、命中行优先于首行、无命中的回退、整块皆标记时返回空;`parseHighlightTerms` 的
  过滤器丢弃与短语;`highlightParts` 的重叠词与空词表。
- `src/components/side-panel/SearchPanel.test.ts` — 默认折叠、点击展开、单命中文件直接打开、
  命中文本里存在 `<mark>`、查询变化后展开态被清空。
- `src-tauri/src/search/mod.rs` — 沿用现有 `log_bus::test_guard()` 模式,断言一次查询产出一行
  `search` 分类的 debug 日志,且被抢占的查询产出 `superseded` 那行。
- `src/lib/i18n/store.test.ts` 既有的键完备性检查覆盖新增的 `logs.categories.search`。

## 明确不做

- 不改 `notemd search` CLI 的输出。它的契约是扁平 `path:line:text`
  (`src-tauri/tests/search_cli_contract.rs`),清洗与分组都是 UI 侧的事。
- 不改后端排序、评分或 `SearchHit` 的 wire 形状。分组与清洗全部在前端完成。
- 不新增「按文件/按类型」的分组模式切换开关。嵌套结构同时保留了两种信息。
- 不做索引侧日志调用点的增删。
