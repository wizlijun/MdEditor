# md 分级与检索优先级 实施计划(项目 B)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把「你写的」和「原始资料」两极认出来并突出,中间的 AI 产物按 `concept_type` 自然分组,让检索结果的顺序与分组反映这个事实。

**Architecture:** `origin` 从**已经写进文件、已经进了索引但没被使用**的信号推导,不需要用户配置目录。存为 `files.origin`,驱动排序权重(全部 adapter 生效)与分组展示(仅 UI)。

**Tech Stack:** Rust(`searchidx`)· Svelte 5 runes · SQLite schema 迁移(bump + 自动重建)

**Spec:** `docs/superpowers/specs/2026-08-11-md-origin-tiering-design.md`

**前置:** 项目 A(`2026-08-11-search-index-settings.md`)应先完成 —— 它的设置页里有为本项目预留的「分层统计」容器,Task 8 回填。

---

## Global Constraints

- **`origin` 永不回写文件。** 它是索引侧的推导物。
- **推导规则的优先级是规范性的**(spec §3 的表),第一条命中即止。改顺序就是改行为。
- **分组仅限 UI。** `notemd search` 的默认输出保持扁平 `path:line:text` —— 那个命令的价值就是长得像 grep,插组头会毁掉 agent 的按行解析。排序权重对 CLI 生效,格式不变。
- **`SCHEMA_VERSION` 从 1 bump 到 2**,所有人索引自动全量重建。这是既有设计的正常路径(可弃派生物,约 10 秒,不丢数据),**不写迁移脚本**。
- 排序常量的每一档都要**单独** mutation 验证。前置项目里出现过「两个乘数一起推同一方向,任一个单独失效测试仍通过」的假阴性 —— 这次每档必须隔离。
- **回归集 50 条全部重跑**;期望值有变更的必须逐条人工确认后再固化,**不允许照着新输出批量刷新**。
- 测试命令:`cargo test --manifest-path searchidx/Cargo.toml`、`cargo test --manifest-path src-tauri/Cargo.toml`、`pnpm check`、`pnpm test`。
- **共享 worktree**:只精确 `git add`,绝不 `git add -A`。

---

## File Structure

| 文件 | 改动 |
| --- | --- |
| `searchidx/src/frontmatter.rs` | 读 `generated.by`(现在被丢弃) |
| `searchidx/src/origin.rs`(新) | `Origin` 枚举、`CONCEPT_TYPE` 映射表、`derive()` |
| `searchidx/src/block.rs` | `FileMeta.origin` |
| `searchidx/src/chunk.rs` | `parse_file` 调 `origin::derive` |
| `searchidx/src/store.rs` | schema bump、`files.origin` 列、读写 |
| `searchidx/src/query.rs` | `origin` 权重、`origin:` 过滤器、`Hit.origin` |
| `searchidx/tests/fixtures/origin/`(新) | 每条规则一个 fixture |
| `src-tauri/src/cli/search.rs` | `--json` 增加 `origin` |
| `src/lib/search/api.ts` | `SearchHit.origin` |
| `src/lib/search/grouping.ts`(新) | 纯函数:命中 → 分组 |
| `src/components/side-panel/SearchPanel.svelte` | 分组渲染 |
| `src/components/SettingsDialog.svelte` | 分层统计回填 |

---

## Task 1: 把 `generated.by` 读回来

现在 `frontmatter.rs` 解析 `generated:` 块时**只取 `at`,把 `by` 丢了**。对散文 `.md` 而言,`generated.by` 是「这份文档是 AI 生成的」唯一可靠的一手声明(`agent_by` 只覆盖 `.note.md` 的大纲节点)。整个分级的地基就是这个字段。

**Files:** Modify `searchidx/src/frontmatter.rs`

**Interfaces:** Produces `Frontmatter.generated_by: Option<String>`

- [ ] **Step 1: 写失败的测试**

```rust
    #[test]
    fn reads_generated_by_alongside_generated_at() {
        let f = parse("generated:\n  by: claude/1\n  at: 2026-08-01T10:00:00Z");
        assert_eq!(f.generated_by.as_deref(), Some("claude/1"));
        assert_eq!(f.generated_at.as_deref(), Some("2026-08-01T10:00:00Z"));
    }

    /// OKF §7:人工撰写或人工确认必须用 `human:` 前缀。原样保留,
    /// 由 origin 推导去判断前缀 —— 解析层不做语义。
    #[test]
    fn a_human_generated_by_is_preserved_verbatim() {
        assert_eq!(parse("generated:\n  by: human:bruce").generated_by.as_deref(), Some("human:bruce"));
    }

    #[test]
    fn generated_by_is_none_when_absent() {
        assert_eq!(parse("title: x").generated_by, None);
        assert_eq!(parse("generated:\n  at: 2026-01-01").generated_by, None);
    }
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test --manifest-path searchidx/Cargo.toml frontmatter`

- [ ] **Step 3: 实现** — `Frontmatter` 加字段;`"generated"` 分支里除 `at` 外也匹配 `by`。

- [ ] **Step 4: 跑测试通过 + 提交**

```bash
git add searchidx/src/frontmatter.rs
git commit -m "feat(searchidx): 读回被丢弃的 generated.by —— 分级的地基"
```

---

## Task 2: `origin` 推导 + 类型映射表

**Files:** Create `searchidx/src/origin.rs`;Modify `searchidx/src/lib.rs`

**Interfaces:**
- `pub enum Origin { Human, Derived, Source }`(`as_str()` → `"human"|"derived"|"source"`,`from_str`)
- `pub fn derive(rel_path: &str, fm: Option<&Frontmatter>, sync_dir: &str) -> Origin`

- [ ] **Step 1: 写失败的测试** —— 每条规则一个,外加优先级测试

```rust
    fn fm(s: &str) -> Frontmatter { crate::frontmatter::parse(s) }

    #[test] fn rule1_note_md_is_always_human() {
        assert_eq!(derive("a.note.md", None, "sync"), Origin::Human);
    }
    #[test] fn rule2_generated_by_agent_is_derived() {
        assert_eq!(derive("a.md", Some(&fm("generated:\n  by: claude/1")), "sync"), Origin::Derived);
    }
    #[test] fn rule2_generated_by_human_is_human() {
        assert_eq!(derive("a.md", Some(&fm("generated:\n  by: human:bruce")), "sync"), Origin::Human);
    }
    #[test] fn rule3_verified_by_human_is_human() {
        assert_eq!(derive("a.md", Some(&fm("verified:\n  by: human:me")), "sync"), Origin::Human);
    }
    #[test] fn rule4_maps_registered_types() {
        assert_eq!(derive("a.md", Some(&fm("type: Note")), "sync"), Origin::Human);
        assert_eq!(derive("a.md", Some(&fm("type: Book Summary")), "sync"), Origin::Derived);
        assert_eq!(derive("a.md", Some(&fm("type: Book")), "sync"), Origin::Source);
    }
    #[test] fn rule5_mirror_dir_is_source() {
        assert_eq!(derive("sync/x/a.md", Some(&fm("title: t")), "sync"), Origin::Source);
    }
    #[test] fn rule6_no_frontmatter_is_source() {
        assert_eq!(derive("a.md", None, "sync"), Origin::Source);
    }
    #[test] fn rule7_unknown_type_is_derived() {
        assert_eq!(derive("a.md", Some(&fm("type: Some Plugin Thing")), "sync"), Origin::Derived);
    }

    /// 优先级:规则 1 压过规则 2 —— agent 往你的批注容器里写了答复,
    /// 容器仍然是你的。文件级 human 与块级 agent_by 是两层。
    #[test] fn note_md_beats_generated_by() {
        assert_eq!(derive("a.note.md", Some(&fm("generated:\n  by: claude/1")), "sync"), Origin::Human);
    }
    /// 规则 4 压过规则 5 —— 镜像目录里的摘要仍是 AI 产物,不是原始资料。
    #[test] fn a_registered_type_beats_the_mirror_dir() {
        assert_eq!(derive("sync/s.md", Some(&fm("type: Book Summary")), "sync"), Origin::Derived);
    }
```

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 实现** —— 按 spec §3 的表逐条,注释写明每条的理由,尤其规则 6 的误判方向。

- [ ] **Step 4: 加映射表同步测试**

`CONCEPT_TYPE` 是活的(插件会加类型)。加一条测试钉住 Rust 映射表与 `src/lib/okf/concept.ts` 的同步:登记表新增值而映射表没跟上时变红。用共享 fixture(与本项目 outline 解析器的跨语言做法一致):从 `concept.ts` 生成一份类型清单 JSON,Rust 侧读它,断言每个值都在映射表里有归属。

> 这条测试挡得住「加了没映射」,挡不住「映射到了错的层」—— 后者需要加类型的人自己判断。在 `concept.ts` 的登记注释里写明这一点。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/origin.rs searchidx/src/lib.rs searchidx/tests/fixtures/origin src/lib/okf/concept.ts
git commit -m "feat(searchidx): origin 三层推导 + 与 CONCEPT_TYPE 同步的映射表"
```

---

## Task 3: schema bump 与存储

**Files:** Modify `searchidx/src/store.rs`、`block.rs`、`chunk.rs`

- [ ] **Step 1: 写失败的测试**

```rust
    #[test]
    fn origin_round_trips_through_the_files_table() {
        // 存入 human,读出 human
    }

    /// bump 后旧库必须被整体重建,而不是带着缺列硬用。
    #[test]
    fn a_version_1_database_is_wiped_on_open() {
        // 建库 → 手工把 meta.schema_version 改回 "1" → 重开 → files 表为空
    }
```

- [ ] **Step 2-3: 实现**

`SCHEMA_VERSION` → 2;`SCHEMA_SQL` 的 `files` 加 `origin TEXT`;`FileMeta` 加 `origin: Origin`;`chunk::parse_file` 调 `origin::derive`;`replace_file` 写入;`query` 读出。

`derive` 需要知道镜像目录名,所以 **`ScanOptions` 增加 `sync_dir: String`**(默认 `"sync"`)。

> **跨计划依赖:** 项目 A 的 Task 1 把 `ScanOptions` 的构造收敛成了唯一的 `search::options::for_vault`。这个新字段必须加在**那一个地方**,从 `vault_settings::resolve_sync_dir(vault_root)` 取值 —— 不要在 CLI 侧另写一份,那正是 A 的契约测试 `search_scan_options_contract.rs` 要挡的事。加完后跑那个测试确认仍绿。

- [ ] **Step 4: 全量测试 + 提交**

```bash
git add searchidx/src/store.rs searchidx/src/block.rs searchidx/src/chunk.rs searchidx/src/scan.rs
git commit -m "feat(searchidx): files.origin 列,schema bump 到 2"
```

---

## Task 4: 排序权重

**Files:** Modify `searchidx/src/query.rs`

- [ ] **Step 1: 写失败的测试** —— **每档单独隔离**

```rust
    /// 三档权重必须各自独立可验。前置项目里出现过「两个乘数一起推同一
    /// 方向、任一个单独失效测试仍通过」的假阴性 —— 所以这里逐档断言
    /// score_of 本身,而不是端到端排序。
    #[test]
    fn each_origin_tier_moves_the_score_on_its_own() {
        let base = hit_with(Origin::Derived);
        let human = score_of(RANK, &hit_with(Origin::Human), false, false, TODAY);
        let derived = score_of(RANK, &base, false, false, TODAY);
        let source = score_of(RANK, &hit_with(Origin::Source), false, false, TODAY);
        assert!(human > derived, "human 必须高于 derived");
        assert!(derived > source, "derived 必须高于 source");
    }
```

- [ ] **Step 2-3: 实现** —— `score_of` 加一档:`Human` ×1.25 / `Derived` ×1.0 / `Source` ×0.9。注释说明与 `human_verified` ×1.1 的叠加是有意的(一个是「这类文档通常人写」,一个是「有人签了字」)。

- [ ] **Step 4: 逐档 mutation check**

三档分别改成 ×1.0,确认**且只有**对应那条断言变红。三次结果都贴进报告。

- [ ] **Step 5: 提交**

---

## Task 5: 回归集重跑

**这是本计划最花时间的一步,不是加字段。**

**Files:** `searchidx/tests/fixtures/retrievability.json`、`searchidx/tests/fixtures/corpus/`

- [ ] **Step 1: 跑回归集,记录所有变化**

Run: `cargo test --manifest-path searchidx/Cargo.toml --test acceptance retrievability`

- [ ] **Step 2: 逐条人工确认**

每一条失败的用例:判断是**排序改善**(新权重让更该出现的文档上来了 → 更新期望)还是**回归**(该找到的找不到了 → 修代码或修 fixture)。

**禁止照着新输出批量刷新。** 每条变更在报告里单列:用例、旧期望、新期望、判定理由。

- [ ] **Step 3: 补分层用例**

回归集增加至少 6 条专门验分级的:同词条下人工笔记排在 AI 摘要前;原始资料排在最后;`origin:` 过滤器各值;`.note.md` 里 agent 写的答复节点仍被降权(文件级 human 不该抵消块级 `agent_by`)。

- [ ] **Step 4: 提交**

---

## Task 6: `origin:` 过滤器 + CLI 输出

**Files:** `searchidx/src/query.rs`、`src-tauri/src/cli/search.rs`

- [ ] **Step 1: 测试** —— `parse("x origin:human")` 解析正确;过滤生效;非法值(`origin:bogus`)不报错而是忽略(检索永不失败于调用方)。

- [ ] **Step 2: 实现** —— `Query.origins: Vec<String>`;`push_filters` 加 `f.origin = ?N`;`Hit.origin` 字段;CLI `--json` 每条命中加 `origin`(与既有 `provenance` 并列)。

- [ ] **Step 3: 确认 CLI 默认输出未变** —— 契约测试断言 `path:line:text` 格式逐字不变。

- [ ] **Step 4: 提交**

---

## Task 7: 分组展示

**Files:** Create `src/lib/search/grouping.ts` + 测试;Modify `SearchPanel.svelte`、`api.ts`、i18n

- [ ] **Step 1: 写纯函数测试**

```ts
describe('groupHits', () => {
  it('两极固定在首尾,中间按类型', () => { /* human 组第一,source 组最后 */ })
  it('空组不显示', () => { /* 没有 source 命中时不出现原始资料组 */ })
  it('组数随结果中出现的类型数变化', () => { /* 2 种类型 → 4 组 */ })
  it('derived 里没有类型的归入「其他」并排在具名类型之后', () => {})
  it('组内保持原有分数顺序', () => {})
})
```

- [ ] **Step 2-3: 实现** —— `groupHits(hits) -> Group[]`,纯函数,不碰 Svelte;面板渲染组头(标题 + 条数)。

- [ ] **Step 4: 检查 + 提交**

---

## Task 8: 设置页分层统计回填

**Files:** `src-tauri/src/search/mod.rs`(stats 增加分层计数)、`SettingsDialog.svelte`、i18n

- [ ] **Step 1: 后端** —— `SearchStatsDto` 增加 `originCounts: { human, derived, source }` 与 `typeCounts: Record<string, number>`(仅 derived)。SQL 分组统计。

- [ ] **Step 2: 前端** —— 替换项目 A 留的 `search.index.tiersPending` 占位块。附一句说明:分级是推导的,改文件 frontmatter 可以纠正。

- [ ] **Step 3: 检查 + 提交**

---

## 人工 GUI 验收清单

1. 搜一个既有笔记又有 AI 摘要的词 → 「你写的」组排在最前
2. 组数与 vault 里实际存在的类型数一致
3. 没有原始资料命中时,「原始资料」组不出现
4. `origin:human` 过滤只剩人工层
5. 设置页分层统计的数字与实际相符;若「原始资料」偏高,说明 vault 里缺 frontmatter(spec §9 说的发现途径)
6. `.note.md` 里 agent 写的答复仍带 ✦ 且排得比你写的节点靠后
7. 深浅色主题下组头正常
8. 结果很少时分组是否显得啰嗦(spec §9 的退化开关判据)

---

## 已知取舍

- **规则 6 会误判无 frontmatter 的人工笔记为原始资料。** 方向是刻意选的,修正手段是加 frontmatter。
- **四个乘数最多叠到 ×1.98**,理论上不失控,但回归集是唯一裁判。
- **`CONCEPT_TYPE` 是活的**;同步测试挡得住漏映射,挡不住映射错层。
- **分组让结果列表变长**;若实测啰嗦,在总数低于阈值时退化为不分组。
