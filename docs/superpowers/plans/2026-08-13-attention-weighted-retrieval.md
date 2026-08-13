# 注意力加权检索 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Reading Insights 已经采集的注意力时长(带 30 天半衰期)接进搜索索引,让你投入过时间的文档在检索中既优先召回、也优先排序。

**Architecture:** 新模块 `searchidx/src/attention.rs` 承担全部算术(衰减、折算、加成曲线)与摄取 IO;结果落进索引 DB 的新表 `doc_attention`,由 `SearchIndex::refresh_attention` 全量重算写入;`query.rs` 在候选阶段加一条「按注意力排序」的臂,在 `score_of` 加一档乘性加成。注意力数据的唯一事实源始终是 vault 里 git 同步的 `.notemd/analytics/*.json`,索引侧只是可弃派生缓存。

**Tech Stack:** Rust(`searchidx` crate,rusqlite + serde_json,无新依赖)、Tauri 命令层(`src-tauri/src/search/`)、Svelte 5 设置页、四语 i18n。

## Global Constraints

- 设计规格:`docs/superpowers/specs/2026-08-13-attention-weighted-retrieval-design.md`。任何与它冲突的实现选择都要先回去改规格。
- 半衰期 `HALF_LIFE_DAYS = 30.0`;截断 `MAX_AGE_DAYS = 365`(第 365 天计入,第 366 天不计);参考尺度 `REF_MINUTES = 120.0`;编辑权重 `EDIT_WEIGHT = 1.5`;默认 `k = 0.4`。
- **只加分**:注意力为 0 时加成必须**严格等于 1.0**,不是近似。
- **永不写 vault**:`searchidx` 全 crate 只读 vault(见 `searchidx/src/lib.rs` 头部注释)。本项目不引入任何写入点。
- 新增/改动的用户可见文案必须四语齐全:`src/lib/i18n/{en,zh,ja,de}.ts`。
- Rust 侧验证命令固定为 `cd searchidx && cargo test`(crate 内)与 `cd src-tauri && cargo test`(命令层)。前端为 `pnpm test`。
- 提交信息用中文,句式随仓库既有风格(`feat(search): …` / `fix(search): …`)。

## 已存在的、必须先读懂的东西

| 东西 | 位置 | 为什么要看 |
| --- | --- | --- |
| `score_of` | `searchidx/src/query.rs:930` | 所有乘性加成在这里;**末尾有 `r / (1.0 + r)` 压缩**,所以 ×1.4 不等于最终分 ×1.4,但单调性保留 |
| 既有新鲜度加成 | `query.rs` 内 `hit.doc_date` 那段 `1.0 + 0.2 * exp(-age/180)` | 与本项目的时间衰减**是两件事**:那个衰减的是文档日期,这个衰减的是你的投入。别合并 |
| `days_between` / `days_from_civil` | `query.rs:1006`、`:1010`(现为私有) | 已有的 civil 日期差实现,**复用,不要重写** |
| `SELECT_COLS` | `query.rs:430` | 三条查询路径共用;**加列会移动 `rank` 的索引位**,这是本计划最容易出错的一处 |
| `Weights` / `sanitized` | `query.rs:218`–`258` | 新字段的 sanitize 规则与既有四档**相反**(必须允许 0) |
| `SCHEMA_SQL` / `SCHEMA_VERSION` | `searchidx/src/store.rs:55` | 当前是 5,本项目 bump 到 6 |
| `MirrorMeta` | `src-tauri/src/sotvault/mirror_meta.rs:14` | `{mirror, device_id, source}`,`abs:` 归因的查表依据 |
| `weights_from` | `src-tauri/src/search/options.rs:115` | `Weights` 的唯一构造点 |
| `should_forward` | `src-tauri/src/search/watch.rs:~60` | 当前把所有 `.` 开头目录挡在外面,Task 10 要开一个针眼 |
| 回归集 | `searchidx/tests/fixtures/retrievability.json` + `tests/acceptance.rs:98` | corpus 里**没有** `.notemd/analytics`,所以本项目预期它零变化 |
| 注意力数据格式 | `src/lib/insights/model.ts` | `docKeyFor` 的 `rel:` / `abs:` 前缀、`DayCounters` 字段名 |

## 文件结构

**新建**
- `searchidx/src/attention.rs` —— 本项目的全部算术与摄取。四块职责:①衰减与加成曲线(纯);②把日文件折成 `path → minutes`(纯);③扫 `.notemd/analytics/` 目录读盘(IO);④`abs:` → 镜像的查表(纯)。单文件约 350 行,含测试;超过就先拆测试到 `attention/tests.rs`。

**修改**
- `searchidx/src/lib.rs` —— 挂模块、`SearchIndex::refresh_attention`、`IndexStats` 加两个计数。
- `searchidx/src/store.rs` —— `doc_attention` 表、`SCHEMA_VERSION` 6、读写函数。
- `searchidx/src/query.rs` —— `Weights.attention`、`SELECT_COLS`、`Hit.attention_minutes`、`score_of` 加成、第二候选臂、`days_between` 改 `pub(crate)`。
- `searchidx/tests/acceptance.rs` —— 端到端验收。
- `src-tauri/src/sotvault/vault_settings.rs` —— `SearchWeights.attention`。
- `src-tauri/src/search/options.rs` —— `weights_from` 读新字段。
- `src-tauri/src/search/mod.rs` —— `refresh_attention` 调用点、`IndexStatsDto` 两个新字段。
- `src-tauri/src/search/watch.rs` —— 放行 `.notemd/analytics/`,独立防抖触发。
- `src-tauri/src/cli/search.rs` + `builtin.rs` —— `--json` 新字段与帮助文本。
- `src/lib/search/api.ts`、`src/components/SettingsDialog.svelte`、`src/lib/i18n/{en,zh,ja,de}.ts` —— 设置页覆盖率行。

---

### Task 1: 衰减、折算与加成曲线(纯函数)

**Files:**
- Create: `searchidx/src/attention.rs`
- Modify: `searchidx/src/lib.rs`(加 `pub mod attention;`)
- Modify: `searchidx/src/query.rs`(`days_between` 与 `days_from_civil` 改 `pub(crate)`)
- Test: `searchidx/src/attention.rs` 内 `#[cfg(test)] mod tests`(本 crate 的房规:单测与代码同文件)

**Interfaces:**
- Produces:
  - `pub const HALF_LIFE_DAYS: f64 = 30.0;`
  - `pub const MAX_AGE_DAYS: i64 = 365;`
  - `pub const REF_MINUTES: f64 = 120.0;`
  - `pub const EDIT_WEIGHT: f64 = 1.5;`
  - `pub fn decay(age_days: i64) -> f64`
  - `pub fn minutes_of(read_ms: i64, edit_ms: i64) -> f64`
  - `pub fn boost(stored_minutes: f64, age_days: i64, k: f64) -> f64`

- [ ] **Step 1: 写失败的测试**

在新文件 `searchidx/src/attention.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// 半衰期 30 天:今天 ×1、30 天前 ×0.5、90 天前 ×0.125。
    #[test]
    fn decay_halves_every_thirty_days() {
        assert!((decay(0) - 1.0).abs() < 1e-9);
        assert!((decay(30) - 0.5).abs() < 1e-9);
        assert!((decay(90) - 0.125).abs() < 1e-9);
    }

    /// 负 age(时钟回拨、跨时区日期桶错位)绝不能放大分数。
    #[test]
    fn decay_never_amplifies_for_a_negative_age() {
        assert!((decay(-5) - 1.0).abs() < 1e-9, "未来的日期桶按今天算,不给额外奖励");
    }

    /// 编辑按 1.5 倍计:写过比读过重(与看板 DEFAULT_WEIGHTS.edit 一致)。
    #[test]
    fn edit_time_counts_one_and_a_half_times() {
        // 60_000 ms = 1 min
        assert!((minutes_of(60_000, 0) - 1.0).abs() < 1e-9);
        assert!((minutes_of(0, 60_000) - 1.5).abs() < 1e-9);
        assert!((minutes_of(60_000, 60_000) - 2.5).abs() < 1e-9);
    }

    /// 负计数器(损坏文件)不得产生负分钟数,否则会污染整份合计。
    #[test]
    fn negative_counters_clamp_to_zero() {
        assert_eq!(minutes_of(-1_000_000, -1_000_000), 0.0);
    }

    /// 设计规格 §4.2 的硬约束:零注意力**严格**等于 1.0,不是近似。
    /// 未读的文档(AI 刚生成、你还没看)必须原地不动,否则新产出会被永久埋掉。
    #[test]
    fn zero_attention_is_exactly_one() {
        assert_eq!(boost(0.0, 0, 0.4), 1.0);
        assert_eq!(boost(0.0, 999, 0.4), 1.0);
    }

    /// 规格 §4.2 的系数表。容差 5e-3 —— 钉的是曲线形状,不是浮点位。
    #[test]
    fn the_boost_table_matches_the_spec() {
        let k = 0.4;
        for (m, want) in [(5.0, 1.15), (30.0, 1.29), (60.0, 1.34), (120.0, 1.40)] {
            let got = boost(m, 0, k);
            assert!((got - want).abs() < 5e-3, "m={m} 期望 ≈{want},实得 {got}");
        }
    }

    /// 封顶:超过参考尺度后不再增长,挡住「一份文档读了 100 小时就霸榜」。
    #[test]
    fn the_boost_is_capped(){
        let k = 0.4;
        assert_eq!(boost(REF_MINUTES, 0, k), boost(10_000.0, 0, k));
        assert!((boost(REF_MINUTES, 0, k) - (1.0 + k)).abs() < 1e-9);
    }

    /// k=0 是「关掉这个功能」的正确表达,必须恒等于 1.0。
    #[test]
    fn k_zero_disables_the_boost_entirely() {
        for m in [0.0, 5.0, 120.0, 10_000.0] {
            assert_eq!(boost(m, 0, 0.0), 1.0, "k=0 时 m={m} 也不能动分数");
        }
    }

    /// 查询时的二次衰减:表陈旧 30 天 → 存的 60 分钟只值 30 分钟。
    #[test]
    fn a_stale_table_decays_again_at_query_time() {
        let k = 0.4;
        assert!((boost(60.0, 30, k) - boost(30.0, 0, k)).abs() < 1e-9);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd searchidx && cargo test attention
```

Expected: 编译失败 —— `cannot find function decay` 等(此时 `attention.rs` 还没被挂进 `lib.rs`,先做 Step 3 的挂载)。

- [ ] **Step 3: 写实现**

`searchidx/src/attention.rs` 顶部(测试模块之前):

```rust
//! 注意力加权的全部算术与摄取。
//!
//! 唯一事实源是 vault 里 git 同步的 `.notemd/analytics/*.json`(Reading
//! Insights 采集,见 `src/lib/insights/model.ts`);索引里的 `doc_attention`
//! 表只是它的可弃派生缓存。本模块**只读** vault。
//!
//! 与 `query.rs` 里既有的 `doc_date` 新鲜度加成不是一回事:那个衰减的是
//! 「文档写于何时」,这个衰减的是「你何时在它身上花过时间」。两者独立叠加。

/// 30 天减半。设计规格 §4.2:匹配「手头活儿」的节奏 —— 不至于一周就忘,
/// 也不至于把去年读的书永久顶在前面。
pub const HALF_LIFE_DAYS: f64 = 30.0;

/// 只摄取最近这么多天的 analytics 文件。**这不是优化,是正确性边界**:
/// 30 天半衰期下 365 天前的贡献是 2^-12.2 ≈ 0.0002,低于任何有意义的精度,
/// 而没有这条截断,摄取成本会随 vault 的年龄线性膨胀。
pub const MAX_AGE_DAYS: i64 = 365;

/// 加成曲线的参考尺度:衰减后累计到 2 小时即封顶。
pub const REF_MINUTES: f64 = 120.0;

/// 编辑时间的权重。与看板 `value.ts` 的 `DEFAULT_WEIGHTS.edit` 一致:
/// 写过比读过重。
pub const EDIT_WEIGHT: f64 = 1.5;

/// `age_days` 天前的一分钟,今天还值多少。
///
/// 负 `age_days` 钳到 0:设备本地日的日期桶跨时区可能「来自未来」,时钟
/// 回拨也会造出同样的输入。让未来的投入拿到 >1 的乘数是纯粹的错误。
pub fn decay(age_days: i64) -> f64 {
    let age = age_days.max(0) as f64;
    0.5f64.powf(age / HALF_LIFE_DAYS)
}

/// 一天一文档的计数器折成注意力分钟数。负值(损坏文件)钳到 0。
pub fn minutes_of(read_ms: i64, edit_ms: i64) -> f64 {
    let read = read_ms.max(0) as f64;
    let edit = edit_ms.max(0) as f64;
    (read + EDIT_WEIGHT * edit) / 60_000.0
}

/// 排序加成。`stored_minutes` 是 `doc_attention.minutes`(已衰减到该表的
/// `as_of` 当天),`age_days` 是 `as_of` 到今天的天数 —— 表每天重算,但
/// app 开着不动时存量会冻住,这一项让陈旧的表优雅退化而不是发出过期高分。
///
/// **零注意力严格返回 1.0**(规格 §4.2):AI 刚生成、你还没读的文档注意力
/// 恒为 0,而搜索的场合恰恰常常是去找它们。惩罚未读 = 把新产出永久埋掉。
pub fn boost(stored_minutes: f64, age_days: i64, k: f64) -> f64 {
    if k <= 0.0 || !stored_minutes.is_finite() || stored_minutes <= 0.0 {
        return 1.0;
    }
    let m = stored_minutes * decay(age_days);
    let frac = (m.ln_1p() / REF_MINUTES.ln_1p()).min(1.0);
    1.0 + k * frac
}
```

`searchidx/src/lib.rs` 的模块列表里,按字母序插在 `pub mod block;` 之前:

```rust
pub mod attention;
```

`searchidx/src/query.rs` 把两个日期助手开放给同 crate 复用(**只改可见性,不动实现**):

```rust
/// Whole days from `from` to `to`, both `YYYY-MM-DD`. `None` on unparseable input.
///
/// `pub(crate)` since the attention ingest needs the same civil-day arithmetic
/// and this crate's house rule is that a utility stays where it was born and
/// gets exported (same as `chunk::ymd_from_unix_public`) rather than being
/// moved into a new "utils" module nobody owns.
pub(crate) fn days_between(from: &str, to: &str) -> Option<i64> {
```

```rust
pub(crate) fn days_from_civil(ymd: &str) -> Option<i64> {
```

- [ ] **Step 4: 跑测试确认通过**

```bash
cd searchidx && cargo test attention
```

Expected: 9 个测试全绿。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/attention.rs searchidx/src/lib.rs searchidx/src/query.rs
git commit -m "feat(searchidx): 注意力衰减与加成曲线"
```

---

### Task 2: 把 analytics 日文件折成 path → minutes(纯函数 + `abs:` 归因)

**Files:**
- Modify: `searchidx/src/attention.rs`
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: Task 1 的 `decay`、`minutes_of`、`MAX_AGE_DAYS`;`crate::query::days_between`
- Produces:
  - `pub struct DayFile { pub day: String, pub device_id: String, pub docs: Vec<DocDay> }`
  - `pub struct DocDay { pub key: String, pub read_ms: i64, pub edit_ms: i64 }`
  - `pub struct MirrorLink { pub device_id: String, pub source: String, pub mirror: String }`
  - `pub fn fold(files: &[DayFile], links: &[MirrorLink], as_of: &str) -> std::collections::BTreeMap<String, f64>`

- [ ] **Step 1: 写失败的测试**

追加到 `attention.rs` 的 `mod tests`:

```rust
    fn df(day: &str, device: &str, docs: &[(&str, i64, i64)]) -> DayFile {
        DayFile {
            day: day.into(),
            device_id: device.into(),
            docs: docs
                .iter()
                .map(|(k, r, e)| DocDay { key: (*k).into(), read_ms: *r, edit_ms: *e })
                .collect(),
        }
    }

    /// `rel:` key 直接去掉前缀就是索引里的 `files.path`。
    #[test]
    fn rel_keys_become_vault_relative_paths() {
        let m = fold(&[df("2026-08-13", "d1", &[("rel:notes/a.md", 60_000, 0)])], &[], "2026-08-13");
        assert!((m["notes/a.md"] - 1.0).abs() < 1e-9);
        assert_eq!(m.len(), 1);
    }

    /// 同一文档跨天、跨设备求和,各自按自己的日期衰减。
    #[test]
    fn the_same_doc_sums_across_days_and_devices() {
        let m = fold(
            &[
                df("2026-08-13", "d1", &[("rel:a.md", 60_000, 0)]),  // 今天 → 1.0
                df("2026-07-14", "d2", &[("rel:a.md", 60_000, 0)]),  // 30 天前 → 0.5
            ],
            &[],
            "2026-08-13",
        );
        assert!((m["a.md"] - 1.5).abs() < 1e-6, "实得 {}", m["a.md"]);
    }

    /// 截断边界:第 365 天计入,第 366 天不计。**这条是成本上界的守卫**,
    /// 删掉它摄取会随 vault 年龄线性变慢而没人发现。
    #[test]
    fn the_age_cutoff_includes_day_365_and_excludes_day_366() {
        // 2026-08-13 往前 365 天 = 2025-08-13;366 天 = 2025-08-12
        let m = fold(
            &[
                df("2025-08-13", "d1", &[("rel:in.md", 60_000, 0)]),
                df("2025-08-12", "d1", &[("rel:out.md", 60_000, 0)]),
            ],
            &[],
            "2026-08-13",
        );
        assert!(m.contains_key("in.md"), "第 365 天必须计入");
        assert!(!m.contains_key("out.md"), "第 366 天必须被截断");
    }

    /// 信念 4:vault 外源文件的阅读时长归给它在 vault 里的镜像副本。
    #[test]
    fn abs_keys_are_credited_to_their_mirror() {
        let links = vec![MirrorLink {
            device_id: "d1".into(),
            source: "/Users/bruce/Downloads/x.md".into(),
            mirror: "sync/x.md".into(),
        }];
        let m = fold(
            &[df("2026-08-13", "d1", &[("abs:/Users/bruce/Downloads/x.md", 60_000, 0)])],
            &links,
            "2026-08-13",
        );
        assert!((m["sync/x.md"] - 1.0).abs() < 1e-9);
    }

    /// **必须按 deviceId 配对。** 两台机器上的同一个绝对路径是两个不同的
    /// 文件;跨设备匹配 `source` 会把别人的阅读时间算到你的镜像上。
    #[test]
    fn abs_keys_do_not_match_another_devices_mirror_link() {
        let links = vec![MirrorLink {
            device_id: "OTHER".into(),
            source: "/Users/bruce/Downloads/x.md".into(),
            mirror: "sync/x.md".into(),
        }];
        let m = fold(
            &[df("2026-08-13", "d1", &[("abs:/Users/bruce/Downloads/x.md", 60_000, 0)])],
            &links,
            "2026-08-13",
        );
        assert!(m.is_empty(), "设备不匹配时必须丢弃,不能猜");
    }

    /// 查不到镜像的 `abs:` key 直接丢弃 —— 它指向一个不在索引里的文件。
    #[test]
    fn unmapped_abs_keys_are_dropped() {
        let m = fold(&[df("2026-08-13", "d1", &[("abs:/tmp/y.md", 60_000, 0)])], &[], "2026-08-13");
        assert!(m.is_empty());
    }

    /// 无法解析的日期不能让整批摄取失败,只跳过该文件。
    #[test]
    fn an_unparseable_day_is_skipped_not_fatal() {
        let m = fold(
            &[
                df("not-a-date", "d1", &[("rel:bad.md", 60_000, 0)]),
                df("2026-08-13", "d1", &[("rel:good.md", 60_000, 0)]),
            ],
            &[],
            "2026-08-13",
        );
        assert!(!m.contains_key("bad.md"));
        assert!(m.contains_key("good.md"));
    }

    /// 零分钟的条目不进表:它对排序毫无作用,只会让表和 LEFT JOIN 变大。
    #[test]
    fn zero_minute_entries_are_not_stored() {
        let m = fold(&[df("2026-08-13", "d1", &[("rel:a.md", 0, 0)])], &[], "2026-08-13");
        assert!(m.is_empty());
    }
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd searchidx && cargo test attention
```

Expected: FAIL —— `cannot find type DayFile` / `cannot find function fold`。

- [ ] **Step 3: 写实现**

追加到 `attention.rs`(测试模块之前):

```rust
use std::collections::BTreeMap;

/// 一份 `YYYY-MM-DD.<deviceId>.json` 里我们关心的部分。
#[derive(Debug, Clone)]
pub struct DayFile {
    /// 文件名里的日期桶(设备本地日,见 `insights/model.ts` 的 `dayKey`)。
    pub day: String,
    /// 文件名里的 deviceId。`abs:` 归因**必须**用它配对。
    pub device_id: String,
    pub docs: Vec<DocDay>,
}

/// 一天里一个文档的两个计时器。其余计数器(`open_count`、`mark_ops`…)
/// 刻意不读:批注已经由 `is_annotation ×1.2` 与 `origin=human ×1.25`
/// 加过两道了,再摆进来是同一件事数三遍(规格 §4.1)。
#[derive(Debug, Clone)]
pub struct DocDay {
    /// 原样的 docKey,含 `rel:` / `abs:` 前缀。
    pub key: String,
    pub read_ms: i64,
    pub edit_ms: i64,
}

/// 一条「某设备上的某绝对路径 ↔ vault 里的某镜像」记录,由调用方从
/// `.notemd/mirrors/` 读出后传入。
///
/// 由调用方供给而不是本 crate 自己读盘:`MirrorMeta` 的格式归
/// `src-tauri/src/sotvault/mirror_meta.rs` 所有,两个 crate 各解析一遍
/// 意味着格式一改就有一边静默错掉。
#[derive(Debug, Clone)]
pub struct MirrorLink {
    pub device_id: String,
    pub source: String,
    /// vault 相对路径,与 `files.path` 同域。
    pub mirror: String,
}

/// 把所有日文件折成 `vault 相对路径 → 衰减到 as_of 当天的注意力分钟数`。
///
/// 纯函数:同样的输入永远给同样的输出,不碰文件系统、不看时钟。摄取之所以
/// 是**全量重算**而不是增量累加,原因在规格 §3.1:当天的 analytics 文件整天
/// 都在被重写(存的是当日累计计数器,不是增量事件),任何水位方案算错时都是
/// **静默**的 —— 分数偏高,没有任何症状。
pub fn fold(files: &[DayFile], links: &[MirrorLink], as_of: &str) -> BTreeMap<String, f64> {
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    for f in files {
        let Some(age) = crate::query::days_between(&f.day, as_of) else { continue };
        if age > MAX_AGE_DAYS {
            continue;
        }
        let factor = decay(age);
        for d in &f.docs {
            let Some(path) = resolve(&d.key, &f.device_id, links) else { continue };
            let m = minutes_of(d.read_ms, d.edit_ms) * factor;
            if m > 0.0 {
                *out.entry(path).or_insert(0.0) += m;
            }
        }
    }
    out.retain(|_, v| *v > 0.0);
    out
}

/// docKey → vault 相对路径。`rel:` 去前缀;`abs:` 查同设备的镜像记录;
/// 都不匹配就是 `None`(丢弃,不猜)。
fn resolve(key: &str, device_id: &str, links: &[MirrorLink]) -> Option<String> {
    if let Some(rel) = key.strip_prefix("rel:") {
        return (!rel.is_empty()).then(|| rel.to_string());
    }
    let abs = key.strip_prefix("abs:")?;
    links
        .iter()
        .find(|l| l.device_id == device_id && l.source == abs)
        .map(|l| l.mirror.clone())
}
```

- [ ] **Step 4: 跑测试确认通过**

```bash
cd searchidx && cargo test attention
```

Expected: Task 1 的 9 条 + 本任务的 8 条,共 17 条全绿。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/attention.rs
git commit -m "feat(searchidx): analytics 日文件折算与镜像归因"
```

---

### Task 3: 摄取 IO —— 扫 `.notemd/analytics/` 目录

**Files:**
- Modify: `searchidx/src/attention.rs`
- Test: 同文件 `mod tests`(用 `tempfile`,已在 dev-dependencies)

**Interfaces:**
- Consumes: Task 2 的 `DayFile` / `DocDay`
- Produces: `pub fn collect(vault_root: &std::path::Path, as_of: &str) -> Vec<DayFile>`

- [ ] **Step 1: 写失败的测试**

追加到 `mod tests`:

```rust
    use std::path::Path;

    fn write_day(root: &Path, name: &str, body: &str) {
        let dir = root.join(".notemd/analytics");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    /// 正常路径:文件名解析出 day + deviceId,JSON 里取两个计时器。
    #[test]
    fn collect_reads_day_and_device_from_the_filename() {
        let d = tempfile::tempdir().unwrap();
        write_day(
            d.path(),
            "2026-08-13.DEV-1.json",
            r#"{"deviceId":"DEV-1","deviceName":"mac","docs":{
                 "rel:a.md":{"2026-08-13":{"read_ms":60000,"edit_ms":1000,"open_count":2,
                   "edit_sessions":1,"net_chars":10,"mark_ops":0,
                   "first_seen_at":0,"last_active_at":0}}}}"#,
        );
        let files = collect(d.path(), "2026-08-13");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].day, "2026-08-13");
        assert_eq!(files[0].device_id, "DEV-1");
        assert_eq!(files[0].docs.len(), 1);
        assert_eq!(files[0].docs[0].key, "rel:a.md");
        assert_eq!(files[0].docs[0].read_ms, 60_000);
        assert_eq!(files[0].docs[0].edit_ms, 1_000);
    }

    /// 超龄文件**在读盘前**就被文件名筛掉 —— 这是 MAX_AGE_DAYS 省 IO 的地方,
    /// 不只是省算术。
    #[test]
    fn collect_skips_files_older_than_the_cutoff_without_reading_them() {
        let d = tempfile::tempdir().unwrap();
        write_day(d.path(), "2020-01-01.DEV-1.json", "这不是合法 JSON,但也不该被读");
        assert!(collect(d.path(), "2026-08-13").is_empty());
    }

    /// 单个损坏文件跳过,其余照常 —— 规格 §7 的容错要求。
    #[test]
    fn a_corrupt_file_is_skipped_and_the_rest_still_load() {
        let d = tempfile::tempdir().unwrap();
        write_day(d.path(), "2026-08-12.DEV-1.json", "{ 半个 JSON");
        write_day(
            d.path(),
            "2026-08-13.DEV-1.json",
            r#"{"deviceId":"DEV-1","deviceName":"m","docs":{"rel:ok.md":{"2026-08-13":{"read_ms":1,"edit_ms":0,"open_count":0,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}"#,
        );
        let files = collect(d.path(), "2026-08-13");
        assert_eq!(files.len(), 1, "损坏的那个跳过,好的那个还在");
        assert_eq!(files[0].docs[0].key, "rel:ok.md");
    }

    /// 没有目录不是错误 —— 从没开过洞察的用户就是这个状态。
    #[test]
    fn a_missing_analytics_dir_is_empty_not_an_error() {
        let d = tempfile::tempdir().unwrap();
        assert!(collect(d.path(), "2026-08-13").is_empty());
    }

    /// 文件名不符合 `<day>.<deviceId>.json` 的一律忽略(README、.DS_Store…)。
    #[test]
    fn unrecognized_filenames_are_ignored() {
        let d = tempfile::tempdir().unwrap();
        write_day(d.path(), "README.md", "x");
        write_day(d.path(), ".DS_Store", "x");
        write_day(d.path(), "2026-08-13.json", "x");
        assert!(collect(d.path(), "2026-08-13").is_empty());
    }

    /// 文件名里的日期是权威,JSON 里内嵌的日期桶键**不覆盖**它。
    /// 两者本该一致;不一致时(手工编辑、同步冲突残留)以文件名为准,
    /// 因为超龄截断就是按文件名做的,让内嵌键翻案会绕过截断。
    #[test]
    fn the_filename_day_wins_over_the_inner_bucket_key() {
        let d = tempfile::tempdir().unwrap();
        write_day(
            d.path(),
            "2026-08-13.DEV-1.json",
            r#"{"deviceId":"DEV-1","deviceName":"m","docs":{"rel:a.md":{"1999-01-01":{"read_ms":60000,"edit_ms":0,"open_count":0,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}"#,
        );
        let files = collect(d.path(), "2026-08-13");
        assert_eq!(files[0].day, "2026-08-13");
    }
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd searchidx && cargo test attention
```

Expected: FAIL —— `cannot find function collect`。

- [ ] **Step 3: 写实现**

追加到 `attention.rs`:

```rust
use std::path::Path;

/// analytics 子目录,与 `src/lib/insights/store.svelte.ts` 的 `SUBDIR` 一致。
const ANALYTICS_SUBDIR: &str = ".notemd/analytics";

/// 扫 `<vault>/.notemd/analytics/`,读出未超龄的日文件。
///
/// 全程尽力而为:目录不存在、单个文件损坏、文件名不认识 —— 都是跳过,
/// 不是错误。从没开过 Reading Insights 的用户的正常状态就是「没有目录」,
/// 那必须退化成「这一档恒等于 ×1.0」,而不是让索引报错。
///
/// **超龄判断在读盘之前**,只看文件名:这是 `MAX_AGE_DAYS` 真正省下 IO 的
/// 地方,十年老 vault 的摄取成本因此是常数而不是线性。
pub fn collect(vault_root: &Path, as_of: &str) -> Vec<DayFile> {
    let dir = vault_root.join(ANALYTICS_SUBDIR);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some((day, device_id)) = split_name(&name) else { continue };
        match crate::query::days_between(day, as_of) {
            Some(age) if age <= MAX_AGE_DAYS => {}
            _ => continue,
        }
        let Ok(txt) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(parsed) = serde_json::from_str::<DeviceAnalyticsJson>(&txt) else { continue };
        let docs: Vec<DocDay> = parsed
            .docs
            .into_iter()
            .flat_map(|(key, days)| {
                days.into_values().map(move |c| DocDay {
                    key: key.clone(),
                    read_ms: c.read_ms,
                    edit_ms: c.edit_ms,
                })
            })
            .collect();
        if docs.is_empty() {
            continue;
        }
        out.push(DayFile { day: day.to_string(), device_id: device_id.to_string(), docs });
    }
    out
}

/// `2026-08-13.<deviceId>.json` → `("2026-08-13", "<deviceId>")`。
/// deviceId 是 UUID(不含点),日期不含点,所以「第一个点」和「最后一个点」
/// 就是全部的分隔信息 —— 与 `store.svelte.ts` 的 `FILE_RE` 同一条约定。
fn split_name(name: &str) -> Option<(&str, &str)> {
    let stem = name.strip_suffix(".json")?;
    let (day, device) = stem.split_once('.')?;
    if day.len() != 10 || device.is_empty() {
        return None;
    }
    Some((day, device))
}

/// 只声明我们要读的字段。`deviceName`、`sessions`、以及 `DayCounters` 里
/// 其余的计数器都被 serde 忽略 —— 采集侧加字段不该让摄取失败。
#[derive(serde::Deserialize)]
struct DeviceAnalyticsJson {
    #[serde(default)]
    docs: std::collections::BTreeMap<String, std::collections::BTreeMap<String, CountersJson>>,
}

#[derive(serde::Deserialize)]
struct CountersJson {
    #[serde(default)]
    read_ms: i64,
    #[serde(default)]
    edit_ms: i64,
}
```

- [ ] **Step 4: 跑测试确认通过**

```bash
cd searchidx && cargo test attention
```

Expected: 23 条全绿。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/attention.rs
git commit -m "feat(searchidx): 扫描 .notemd/analytics 目录"
```

---

### Task 4: `doc_attention` 表与 schema bump

**Files:**
- Modify: `searchidx/src/store.rs`
- Test: `searchidx/src/store.rs` 内 `mod tests`

**Interfaces:**
- Produces:
  - `SCHEMA_VERSION` 由 `5` 改为 `6`
  - `pub fn replace_attention(conn: &Connection, as_of: &str, rows: &BTreeMap<String, f64>) -> rusqlite::Result<usize>`
  - `pub fn attention_rows(conn: &Connection) -> rusqlite::Result<i64>`(统计用)

- [ ] **Step 1: 写失败的测试**

追加到 `store.rs` 的 `mod tests`:

```rust
    /// 新表新列 → schema 必须 bump,老库在下次打开时全量重建。
    #[test]
    fn the_schema_version_covers_doc_attention() {
        assert_eq!(SCHEMA_VERSION, 6, "加了 doc_attention 表就必须 bump");
    }

    /// 写入即整表替换:摄取是全量重算的,残留旧行等于双计。
    #[test]
    fn replace_attention_swaps_the_whole_table() {
        let (_d, p) = tmp();
        let c = open(&p, "/v", "sync").unwrap();
        let mut a = std::collections::BTreeMap::new();
        a.insert("x.md".to_string(), 3.0);
        a.insert("y.md".to_string(), 1.0);
        assert_eq!(replace_attention(&c, "2026-08-13", &a).unwrap(), 2);

        let mut b = std::collections::BTreeMap::new();
        b.insert("z.md".to_string(), 5.0);
        assert_eq!(replace_attention(&c, "2026-08-14", &b).unwrap(), 1);

        assert_eq!(attention_rows(&c).unwrap(), 1, "旧行必须被清掉,不能累加");
        let (p, m, d): (String, f64, String) = c
            .query_row("SELECT path, minutes, as_of FROM doc_attention", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(p, "z.md");
        assert!((m - 5.0).abs() < 1e-9);
        assert_eq!(d, "2026-08-14");
    }

    /// 空结果也要落地:它表达的是「摄取跑过了,一条都没有」,与
    /// 「从没跑过」在统计行里是两件事。
    #[test]
    fn replacing_with_an_empty_map_clears_the_table() {
        let (_d, p) = tmp();
        let c = open(&p, "/v", "sync").unwrap();
        let mut a = std::collections::BTreeMap::new();
        a.insert("x.md".to_string(), 3.0);
        replace_attention(&c, "2026-08-13", &a).unwrap();
        assert_eq!(replace_attention(&c, "2026-08-14", &Default::default()).unwrap(), 0);
        assert_eq!(attention_rows(&c).unwrap(), 0);
    }
```

> `tmp()` 与 `open(&p, "/v", "sync")` 是 `store.rs` 的 `mod tests` 里既有的建库 idiom(见 `open_creates_the_schema_and_stamps_meta`)。该模块没有内存库助手,别新造一个。

- [ ] **Step 2: 跑测试确认失败**

```bash
cd searchidx && cargo test --lib store::
```

Expected: FAIL —— `assert_eq!(SCHEMA_VERSION, 6)` 失败 + `cannot find function replace_attention`。

- [ ] **Step 3: 写实现**

`store.rs`:

```rust
pub const SCHEMA_VERSION: i64 = 6;
```

`SCHEMA_SQL` 里,在 `CREATE TABLE meta(...)` 之前插入:

```sql
CREATE TABLE doc_attention(
  path TEXT PRIMARY KEY, minutes REAL NOT NULL, as_of TEXT NOT NULL);
CREATE INDEX doc_attention_minutes ON doc_attention(minutes DESC);
```

> `doc_attention_minutes` 索引是 Task 7 的第二候选臂用的:那条查询以
> `ORDER BY a.minutes DESC` 开头,没有索引就是全表扫。

新增函数(放在同文件其它 `pub fn` 旁):

```rust
/// 整表替换 `doc_attention`,返回写入行数。
///
/// 替换而非 upsert:摄取是全量重算的(见 `attention::fold` 的文档),
/// 上一轮的残留行没有任何机会被更新到 —— 文件被删掉、镜像被解绑、
/// 或者干脆衰减到 0 的路径都不会出现在新一轮的输入里,留着就是双计。
/// 一个事务内完成,查询侧永远看不到「清空了但还没填」的中间态。
pub fn replace_attention(
    conn: &Connection,
    as_of: &str,
    rows: &std::collections::BTreeMap<String, f64>,
) -> rusqlite::Result<usize> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM doc_attention", [])?;
    {
        let mut st = tx.prepare("INSERT INTO doc_attention(path, minutes, as_of) VALUES(?1,?2,?3)")?;
        for (path, minutes) in rows {
            st.execute(rusqlite::params![path, minutes, as_of])?;
        }
    }
    tx.commit()?;
    Ok(rows.len())
}

/// 有注意力数据的文件数 —— 设置页的覆盖率行读它。
pub fn attention_rows(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT count(*) FROM doc_attention", [], |r| r.get(0))
}
```

- [ ] **Step 4: 跑测试确认通过**

```bash
cd searchidx && cargo test --lib store::
```

Expected: PASS。**注意**:全 crate 跑 `cargo test` 时,若有测试硬编码了 `SCHEMA_VERSION == 5`,一并改成 6(那是它们的本意 —— 钉住「bump 了就要重建」)。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/store.rs
git commit -m "feat(searchidx): doc_attention 表与 schema v6"
```

---

### Task 5: `SearchIndex::refresh_attention` —— 摄取端到端

**Files:**
- Modify: `searchidx/src/lib.rs`
- Test: `searchidx/tests/acceptance.rs`

**Interfaces:**
- Consumes: `attention::collect`、`attention::fold`、`attention::MirrorLink`、`store::replace_attention`
- Produces: `pub fn refresh_attention(&mut self, links: &[attention::MirrorLink]) -> Result<usize, String>`

- [ ] **Step 1: 写失败的测试**

追加到 `searchidx/tests/acceptance.rs`:

```rust
/// 摄取端到端:vault 里放一份 analytics 文件,refresh 后表里就有对应的行。
/// 用真实临时目录而不是内存库 —— 这条要验的正是「读盘 → 折算 → 落表」这条链
/// 有没有接错,而不是三段各自的算术(那些在 attention.rs 的单测里)。
#[test]
fn refresh_attention_ingests_analytics_into_the_index() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("a.md"), "# 标题\n正文\n").unwrap();
    let dir = vault.path().join(".notemd/analytics");
    std::fs::create_dir_all(&dir).unwrap();
    let today = searchidx::chunk::ymd_from_unix_public(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    );
    std::fs::write(
        dir.join(format!("{today}.DEV-1.json")),
        format!(
            r#"{{"deviceId":"DEV-1","deviceName":"m","docs":{{"rel:a.md":{{"{today}":{{"read_ms":600000,"edit_ms":0,"open_count":1,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}}}}}"#
        ),
    )
    .unwrap();

    // `open_temp` 返回 `(TempDir, SearchIndex)`,那个 TempDir 装的是 index.db ——
    // 必须绑住,提前 drop 会把库删掉。
    let (_db, mut idx) = open_temp(vault.path());
    idx.rebuild(&ScanOptions::default()).unwrap();
    assert_eq!(idx.refresh_attention(&[]).unwrap(), 1);
    let stats = idx.stats().unwrap();
    assert_eq!(stats.attention_files, 1);

    // 重复调用是幂等的 —— 全量重算的核心保证,也是「不做增量」的理由。
    assert_eq!(idx.refresh_attention(&[]).unwrap(), 1);
    assert_eq!(idx.stats().unwrap().attention_files, 1);
}

/// 没有 analytics 目录 = 从没开过洞察 = 空表,不是错误。
#[test]
fn refresh_attention_on_a_vault_without_insights_is_a_clean_no_op() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("a.md"), "正文\n").unwrap();
    let (_db, mut idx) = open_temp(vault.path());
    idx.rebuild(&ScanOptions::default()).unwrap();
    assert_eq!(idx.refresh_attention(&[]).unwrap(), 0);
    assert_eq!(idx.stats().unwrap().attention_files, 0);
}
```

> `open_temp` 是 `acceptance.rs:16` 已有的助手,只开库不建库,所以要自己跟一句 `rebuild`(与 `non_default_weights_reorder_the_same_query` 同样的用法)。

- [ ] **Step 2: 跑测试确认失败**

```bash
cd searchidx && cargo test --test acceptance refresh_attention
```

Expected: FAIL —— `no method named refresh_attention` / `no field attention_files`。

- [ ] **Step 3: 写实现**

`searchidx/src/lib.rs`,在 `impl SearchIndex` 内(挨着 `sweep`):

```rust
    /// 重新摄取 vault 的注意力数据,返回写入的文件数。
    ///
    /// **全量重算**,不是增量:当天的 analytics 文件整天都在被重写,任何
    /// 「读到哪儿了」的水位方案算错时都是静默的(分数偏高,无症状)。重算是
    /// 无状态的,因此也是幂等的 —— 连调两次结果完全相同。
    ///
    /// `links` 由调用方从 `.notemd/mirrors/` 读出(`MirrorMeta` 的格式归
    /// `src-tauri` 所有,见 `attention::MirrorLink` 的文档)。传空切片是
    /// 合法的:那只意味着 vault 外源文件的阅读时长不参与,vault 内的照常。
    pub fn refresh_attention(
        &mut self,
        links: &[attention::MirrorLink],
    ) -> Result<usize, String> {
        let as_of = today();
        let files = attention::collect(&self.vault_root, &as_of);
        let folded = attention::fold(&files, links, &as_of);
        store::replace_attention(&self.conn, &as_of, &folded).map_err(|e| e.to_string())
    }
```

`IndexStats` 增加两个字段:

```rust
pub struct IndexStats {
    // …既有字段…
    /// 有注意力数据的文件数。与 `files` 一起构成设置页的覆盖率行 ——
    /// 「摄取根本没跑起来」在别处没有任何可见症状,这是唯一的发现途径。
    pub attention_files: i64,
    /// `doc_attention.as_of`,`None` = 摄取从未跑过。
    pub attention_as_of: Option<String>,
}
```

`stats()` 里填充:

```rust
            attention_files: store::attention_rows(&self.conn).unwrap_or(0),
            attention_as_of: self
                .conn
                .query_row("SELECT as_of FROM doc_attention LIMIT 1", [], |r| r.get(0))
                .ok(),
```

- [ ] **Step 4: 跑测试确认通过**

```bash
cd searchidx && cargo test
```

Expected: 全绿(此时 `src-tauri` 还没跟上 `IndexStats` 的新字段,那是 Task 9;本步只跑 searchidx)。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/lib.rs searchidx/tests/acceptance.rs
git commit -m "feat(searchidx): SearchIndex::refresh_attention 摄取入口"
```

---

### Task 6: `Weights.attention` 与设置接线

**Files:**
- Modify: `searchidx/src/query.rs`(`Weights` + `sanitized`)
- Modify: `src-tauri/src/sotvault/vault_settings.rs`(`SearchWeights`)
- Modify: `src-tauri/src/search/options.rs`(`weights_from`)
- Test: 上述三个文件各自的 `mod tests`

**Interfaces:**
- Produces: `Weights { human, derived, source, unlabeled, attention }`,`attention` 默认 `0.4`

- [ ] **Step 1: 写失败的测试**

`searchidx/src/query.rs` 的 `mod tests`:

```rust
    /// attention 的 sanitize 规则与 origin 四档**相反**:那四档是乘数,0 会让
    /// 整层塌成 0 分、层内顺序变未定义,所以拒绝 0;attention 是加数,k=0
    /// 恰好是「关掉这个功能」的正确表达,必须放行。写成同一条规则就等于
    /// 剥夺了用户关掉它的能力。
    #[test]
    fn attention_weight_allows_zero_but_rejects_garbage() {
        let d = Weights::default();
        assert_eq!(d.attention, 0.4);

        let zero = Weights { attention: 0.0, ..Weights::default() }.sanitized();
        assert_eq!(zero.attention, 0.0, "k=0 必须原样保留 —— 它是关闭开关");

        for bad in [-1.0, f64::NAN, f64::INFINITY, 2.5] {
            let w = Weights { attention: bad, ..Weights::default() }.sanitized();
            assert_eq!(w.attention, d.attention, "非法值 {bad} 必须回落默认");
        }

        let ok = Weights { attention: 1.5, ..Weights::default() }.sanitized();
        assert_eq!(ok.attention, 1.5);
    }

    /// 一个坏的 attention 不得连累其它四档(既有约定,逐档独立回落)。
    #[test]
    fn a_bad_attention_does_not_clobber_the_origin_tiers() {
        let w = Weights { attention: f64::NAN, human: 2.0, ..Weights::default() }.sanitized();
        assert_eq!(w.human, 2.0);
        assert_eq!(w.attention, Weights::default().attention);
    }
```

`src-tauri/src/search/options.rs` 的 `mod tests`:

```rust
    /// 设置文件里的 attention 一路走到 Weights;0 必须活着到底。
    #[test]
    fn attention_weight_round_trips_from_settings() {
        let d = tempfile::tempdir().unwrap();
        write_settings(d.path(), r#"{"searchWeights": {"attention": 0}}"#);
        assert_eq!(weights_for_vault(d.path()).attention, 0.0, "用户关掉它就得关掉");

        write_settings(d.path(), r#"{"searchWeights": {"attention": 0.8}}"#);
        assert_eq!(weights_for_vault(d.path()).attention, 0.8);

        write_settings(d.path(), r#"{}"#);
        assert_eq!(
            weights_for_vault(d.path()).attention,
            searchidx::query::Weights::default().attention
        );
    }
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd searchidx && cargo test --lib query::tests::attention
cd ../src-tauri && cargo test attention_weight_round_trips
```

Expected: FAIL —— `no field attention on Weights` / `SearchWeights`。

- [ ] **Step 3: 写实现**

`searchidx/src/query.rs`:

```rust
pub struct Weights {
    pub human: f64,
    pub derived: f64,
    pub source: f64,
    pub unlabeled: f64,
    /// 注意力加成的上限增量(规格里的 `k`)。**语义与上面四个不同**:
    /// 那四个是乘数,这个是「最多再乘 (1+k)」里的 k,所以 0 是合法的
    /// 「关掉」而不是坏值 —— 见 `sanitized`。
    pub attention: f64,
}
```

```rust
impl Default for Weights {
    fn default() -> Self {
        Weights { human: 1.25, derived: 1.0, source: 0.9, unlabeled: 0.3, attention: 0.4 }
    }
}
```

`sanitized` 里,四档照旧,`attention` 单独一条:

```rust
        // `attention` 走自己的闸门,**不能**复用上面的 `clean`:那条规则
        // 拒绝 0(对乘数而言 0 会让整层塌成 0 分,层内顺序未定义),而对
        // 这个加数而言 0 正是用户表达「关掉这个功能」的唯一方式。上限 2.0
        // 而非 5.0:k=2 已经是 ×3 封顶,再高就不是加权而是覆盖排序了。
        let attention = if self.attention.is_finite() && (0.0..=2.0).contains(&self.attention) {
            self.attention
        } else {
            fallback.attention
        };
        Weights {
            human: clean(self.human, fallback.human),
            derived: clean(self.derived, fallback.derived),
            source: clean(self.source, fallback.source),
            unlabeled: clean(self.unlabeled, fallback.unlabeled),
            attention,
        }
```

`src-tauri/src/sotvault/vault_settings.rs`:

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention: Option<f64>,
```

`src-tauri/src/search/options.rs` 的 `weights_from`:

```rust
        attention: sw.attention.unwrap_or(default.attention),
```

- [ ] **Step 4: 跑测试确认通过**

```bash
cd searchidx && cargo test
cd ../src-tauri && cargo test search::options
```

Expected: 两边全绿。若 `src-tauri` 有构造 `Weights{..}` 字面量的测试,补上新字段或改用 `..Weights::default()`。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/query.rs src-tauri/src/sotvault/vault_settings.rs src-tauri/src/search/options.rs
git commit -m "feat(search): 注意力权重进 Weights 与 vault 设置"
```

---

### Task 7: `score_of` 接入加成 + `Hit.attention_minutes`

**Files:**
- Modify: `searchidx/src/query.rs`
- Test: 同文件 `mod tests`

**Interfaces:**
- Produces: `Hit.attention_minutes: f64`(已衰减到今天的分钟数,由 `finish` 填)

**⚠️ 本任务最容易出错的一处:列索引位移。** `SELECT_COLS` 现有 13 列(0–12),`rank` 在 13。加两列后 `rank` 变成 15,而 `fts_search` 里 `r.get::<_, f64>(13)` 是硬编码的。三条查询路径(`fts_search`、`like_search`、`filter_only_search`)都要跟着改,漏一条的症状是运行时 `InvalidColumnType`,不是编译错误。

- [ ] **Step 1: 写失败的测试**

追加到 `query.rs` 的 `mod tests`:

```rust
    /// 规格 §4.2:注意力只加分。零注意力的命中必须与接入前**逐位相同**。
    #[test]
    fn zero_attention_leaves_the_score_bit_identical() {
        let w = Weights::default();
        let h = hit_with(Origin::Derived);
        assert_eq!(h.attention_minutes, 0.0, "fixture 默认无注意力");
        let with_k = score_of(-1.0, &h, false, false, false, TODAY, &w);
        let no_k = score_of(-1.0, &h, false, false, false, TODAY, &Weights { attention: 0.0, ..w });
        assert_eq!(with_k, no_k, "注意力为 0 时,k 取任何值都不能改变分数");
    }

    /// 注意力必须**单独**能推动分数 —— 逐档隔离断言,不靠端到端排序。
    /// 前置项目(origin tiering)出现过「两个乘数一起推同一方向、任一个
    /// 单独失效测试仍通过」的假阴性,所以这里断言的是 `score_of` 本身。
    #[test]
    fn attention_alone_moves_the_score() {
        let w = Weights::default();
        let cold = hit_with(Origin::Derived);
        let mut warm = hit_with(Origin::Derived);
        warm.attention_minutes = 60.0;
        let a = score_of(-1.0, &cold, false, false, false, TODAY, &w);
        let b = score_of(-1.0, &warm, false, false, false, TODAY, &w);
        assert!(b > a, "读过 60 分钟的必须高于没读过的: {b} vs {a}");
    }

    /// 单调:更多注意力不得让分数下降。`score_of` 末尾的 `r/(1+r)` 压缩
    /// 保序,所以这条在压缩之后依然成立 —— 值得钉住,因为压缩很容易被
    /// 误读成「加成被吃掉了」。
    #[test]
    fn more_attention_never_lowers_the_score() {
        let w = Weights::default();
        let mut last = f64::MIN;
        for m in [0.0, 1.0, 5.0, 30.0, 120.0, 10_000.0] {
            let mut h = hit_with(Origin::Derived);
            h.attention_minutes = m;
            let s = score_of(-1.0, &h, false, false, false, TODAY, &w);
            assert!(s >= last, "m={m} 让分数掉了: {s} < {last}");
            last = s;
        }
    }

    /// k=0 关掉功能后,连高注意力命中也不动分。
    #[test]
    fn k_zero_disables_the_boost_in_score_of() {
        let off = Weights { attention: 0.0, ..Weights::default() };
        let cold = hit_with(Origin::Derived);
        let mut warm = hit_with(Origin::Derived);
        warm.attention_minutes = 10_000.0;
        assert_eq!(
            score_of(-1.0, &cold, false, false, false, TODAY, &off),
            score_of(-1.0, &warm, false, false, false, TODAY, &off)
        );
    }
```

> `hit_with` 是本模块已有的 fixture 助手;给它新增的 `attention_minutes` 字段填 `0.0`。

- [ ] **Step 2: 跑测试确认失败**

```bash
cd searchidx && cargo test --lib query::tests::attention
```

Expected: FAIL —— `no field attention_minutes on Hit`。

- [ ] **Step 3: 写实现**

`Hit` 新增字段:

```rust
    /// 已衰减到今天的注意力分钟数(`doc_attention.minutes` 再按表的 `as_of`
    /// 到今天二次衰减)。0 = 没有数据,不是「读了 0 分钟」—— 两者对排序的
    /// 影响相同,所以不用 `Option` 徒增调用方的分支。
    ///
    /// 与 `pinned` 一样由 `finish` 填,不在 `row_to_hit` 里:二次衰减需要
    /// `today`,而一行数据自己不知道今天是几号。
    pub attention_minutes: f64,
```

`SELECT_COLS` 追加两列,并在三条查询的 `FROM … JOIN files f` 之后加 `LEFT JOIN`:

```rust
const SELECT_COLS: &str = "f.path, b.line_start, b.line_end, b.text, b.breadcrumb, b.level, \
                           f.doc_date, b.agent_by, f.human_verified, b.is_annotation, f.origin, \
                           f.concept_type, f.title, \
                           COALESCE(att.minutes, 0.0), att.as_of";
```

```sql
LEFT JOIN doc_attention att ON att.path = f.path
```

> `LEFT JOIN` 不是可选风格:没有注意力数据的文件必须照常命中(它们的加成是 ×1.0),`INNER JOIN` 会把整个 vault 里没读过的文件从搜索结果里删掉。

三处 `query_map` 的闭包各自多读两列并把 `rank` 索引改为 **15**:

```rust
    // fts_search
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok((
            row_to_hit(r)?,
            r.get::<_, f64>(15)?,               // ← rank,因 SELECT_COLS 加了两列而位移
            r.get::<_, i64>(9)? != 0,
            r.get(12)?,
            r.get::<_, f64>(13)?,               // att.minutes(已 COALESCE)
            r.get::<_, Option<String>>(14)?,    // att.as_of
        ))
    })?;
```

`like_search` 与 `filter_only_search` 同样加最后两项,`rank` 仍用它们各自的常量(`-1.0` / `0.0`),不读第 15 列。

`row_to_hit` 里给新字段一个占位:

```rust
        attention_minutes: 0.0,   // `finish` 用 `today` 二次衰减后覆盖
```

`finish` 的入参类型改为 6 元组,并在填 `pinned` 的同一处填注意力:

```rust
    for (mut hit, rank, is_annotation, title, minutes, as_of) in rows {
        // 表每天重算,但 app 开着不动时存量会冻住;按 as_of 到今天的天数
        // 再衰减一次,让陈旧的表优雅退化而不是发出过期的高分。
        let age = as_of.as_deref().and_then(|d| days_between(d, today)).unwrap_or(0);
        hit.attention_minutes = minutes * crate::attention::decay(age);
        // …既有的 pinned / 短语复核逻辑…
    }
```

> 注意 `hit.attention_minutes` 存的是**已衰减到今天**的值,所以 `score_of`
> 里调 `boost` 时 `age_days` 传 `0`,不能再衰减第二遍。

`score_of` 里,紧跟 origin 那档之后:

```rust
    // 注意力加权(规格 §4.2)。与上面所有档一样是乘性的,但只向上:
    // `attention::boost` 在 0 分钟时严格返回 1.0,所以从没打开过的文档
    // ——包括 agent 昨天刚生成、你还没来得及读的那些——原地不动。
    // 这与 `doc_date` 那档的时间衰减是两件事:那个衰减「文档写于何时」,
    // 这个衰减「你何时在它身上花过时间」。
    //
    // `hit.attention_minutes` 已由 `finish` 衰减到今天,所以这里传 0。
    r *= crate::attention::boost(hit.attention_minutes, 0, weights.attention);
```

- [ ] **Step 4: 跑测试确认通过**

```bash
cd searchidx && cargo test
```

Expected: 全绿,**包括 `retrievability_regression_set_is_fully_recalled_and_correctly_ordered`**。回归集的 corpus 里没有 `.notemd/analytics`,所以每条命中的注意力都是 0、加成恒 ×1.0。**如果这条回归测试变红,说明代码错了,不要去改期望值。**

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/query.rs
git commit -m "feat(searchidx): score_of 接入注意力加成"
```

---

### Task 8: 第二条候选臂

**Files:**
- Modify: `searchidx/src/query.rs`(`fts_search`)
- Test: `searchidx/tests/acceptance.rs`

**Interfaces:**
- Consumes: Task 4 的 `doc_attention_minutes` 索引、Task 7 的 `SELECT_COLS`

- [ ] **Step 1: 写失败的测试**

追加到 `searchidx/tests/acceptance.rs`:

```rust
/// 规格 §5:纯排序加权救不了 bm25 极低的长文 —— 它连候选窗口都进不去。
/// 构造一份「查询词只出现一次的长文」+ 一堆词频更高的短文把窗口塞满,
/// 断言注意力臂把它捞了回来。
#[test]
fn a_high_attention_document_is_recalled_past_the_bm25_window() {
    let vault = tempfile::tempdir().unwrap();
    // 噪声:64 篇短文,每篇都密集包含查询词,足以填满 (limit*8).max(64) 的窗口。
    for i in 0..80 {
        std::fs::write(
            vault.path().join(format!("noise{i}.md")),
            "银河 银河 银河 银河 银河\n",
        )
        .unwrap();
    }
    // 目标:很长,查询词只出现一次 —— bm25 上必然垫底。
    let mut long = String::from("# 长文\n");
    for _ in 0..500 {
        long.push_str("无关的填充句子,用来把文档长度撑起来。\n");
    }
    long.push_str("这里出现一次银河。\n");
    std::fs::write(vault.path().join("target.md"), &long).unwrap();

    let dir = vault.path().join(".notemd/analytics");
    std::fs::create_dir_all(&dir).unwrap();
    let today = searchidx::chunk::ymd_from_unix_public(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64,
    );
    std::fs::write(
        dir.join(format!("{today}.DEV-1.json")),
        format!(
            r#"{{"deviceId":"DEV-1","deviceName":"m","docs":{{"rel:target.md":{{"{today}":{{"read_ms":36000000,"edit_ms":0,"open_count":9,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}}}}}"#
        ),
    )
    .unwrap();

    let (_db, mut idx) = open_temp(vault.path());
    idx.rebuild(&ScanOptions::default()).unwrap();

    // 摄取之前:没有注意力数据,长文进不了候选。
    let before = idx.search("银河", 10).unwrap().0;
    assert!(
        !before.iter().any(|h| h.path == "target.md"),
        "基线错了:长文本就该在纯 bm25 下落榜,否则这条测试证明不了任何事"
    );

    idx.refresh_attention(&[]).unwrap();
    let after = idx.search("银河", 10).unwrap().0;
    assert!(
        after.iter().any(|h| h.path == "target.md"),
        "注意力臂必须把它捞回来:{:?}",
        after.iter().map(|h| &h.path).collect::<Vec<_>>()
    );
}

/// 第二条臂共用同一个 MATCH 条件,所以**不能**引入不匹配的结果 ——
/// 「我读得最多的文档」不是「我搜的东西」。
#[test]
fn the_attention_arm_never_introduces_a_non_matching_hit() {
    let vault = tempfile::tempdir().unwrap();
    std::fs::write(vault.path().join("read-a-lot.md"), "完全无关的内容\n").unwrap();
    std::fs::write(vault.path().join("match.md"), "银河\n").unwrap();
    let dir = vault.path().join(".notemd/analytics");
    std::fs::create_dir_all(&dir).unwrap();
    let today = searchidx::chunk::ymd_from_unix_public(
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64,
    );
    std::fs::write(
        dir.join(format!("{today}.DEV-1.json")),
        format!(
            r#"{{"deviceId":"DEV-1","deviceName":"m","docs":{{"rel:read-a-lot.md":{{"{today}":{{"read_ms":36000000,"edit_ms":0,"open_count":9,"edit_sessions":0,"net_chars":0,"mark_ops":0,"first_seen_at":0,"last_active_at":0}}}}}}}}"#
        ),
    )
    .unwrap();

    let (_db, mut idx) = open_temp(vault.path());
    idx.rebuild(&ScanOptions::default()).unwrap();
    idx.refresh_attention(&[]).unwrap();
    let hits = idx.search("银河", 10).unwrap().0;
    assert!(hits.iter().all(|h| h.path != "read-a-lot.md"), "注意力不是匹配条件");
    assert!(hits.iter().any(|h| h.path == "match.md"));
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd searchidx && cargo test --test acceptance attention_arm
cd searchidx && cargo test --test acceptance high_attention_document
```

Expected: 第一条 FAIL(长文没被捞回来),第二条应当已经 PASS(它验的是不该发生的事)。

- [ ] **Step 3: 写实现**

`fts_search` 里,把单条查询改成两条臂并去重:

```rust
    // 主臂:相关度。
    let mut sql = format!(/* …既有 SELECT/FROM/WHERE… */);
    let mut args: Vec<String> = vec![expr.clone()];
    push_filters(q, &mut sql, &mut args);
    sql.push_str(&format!(" ORDER BY rank ASC LIMIT {}", (limit * 8).max(64)));

    // 保底臂:注意力。规格 §5 —— 纯排序加权救不了 bm25 极低的长文,它在
    // 打分之前就被 `LIMIT` 砍掉了。两条臂**共用同一个 MATCH**,所以这里
    // 不会引入任何不匹配的结果,只是让「你花过时间的那些」有机会走到评分。
    // `limit*2` 刻意取小:这是保底,不是主力 —— 主排序仍然是相关度。
    let mut arm = format!(/* …同样的 SELECT/FROM/WHERE… */);
    let mut arm_args: Vec<String> = vec![expr];
    push_filters(q, &mut arm, &mut arm_args);
    arm.push_str(&format!(
        // `att.minutes` 全表共用一个 `as_of`,衰减对所有行是同一个单调
        // 乘数,所以按存量排序 == 按衰减后排序,SQL 里不需要算指数。
        " AND att.minutes IS NOT NULL ORDER BY att.minutes DESC, rank ASC LIMIT {}",
        (limit * 2).max(8)
    ));
```

两次 `query_map` + `drain` 后按 `(path, line, line_end)` 去重合并,再交给 `finish`:

```rust
    let mut rows = main_rows;
    let seen: std::collections::HashSet<_> =
        rows.iter().map(|(h, ..)| (h.path.clone(), h.line, h.line_end)).collect();
    rows.extend(
        arm_rows
            .into_iter()
            .filter(|(h, ..)| !seen.contains(&(h.path.clone(), h.line, h.line_end))),
    );
    let truncated = main_truncated || arm_truncated;
    Ok((finish(rows, q, limit, today, weights, conventions)?, truncated))
```

> 用 `(path, line, line_end)` 而不是 block id 去重:`SELECT_COLS` 里没有
> block id,而这个三元组在 `blocks` 里已经唯一。要么两者都加,要么用现成
> 的 —— 别为去重去动 `SELECT_COLS` 的列序(那正是 Task 7 的地雷)。

`like_search` 与 `filter_only_search` **不加臂**(规格 §5):它们已是 `LIMIT 500` 的宽扫,截断压力不在那里,而 LIKE 路径本来就慢,不该再挂一条查询。在 `like_search` 的文档注释里写下这句,免得后人「顺手补齐」。

- [ ] **Step 4: 跑测试确认通过**

```bash
cd searchidx && cargo test
```

Expected: 全绿,回归集依然零变化。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/query.rs searchidx/tests/acceptance.rs
git commit -m "feat(searchidx): 高注意力文档的第二条候选臂"
```

---

### Task 9: 命令层接线 —— 摄取调用点与 DTO

**Files:**
- Modify: `src-tauri/src/search/mod.rs`
- Create: `src-tauri/src/search/attention_links.rs`
- Test: `src-tauri/src/search/attention_links.rs` 内 `mod tests`

**Interfaces:**
- Produces: `pub fn links_for_vault(vault_root: &Path) -> Vec<searchidx::attention::MirrorLink>`
- Produces: `SearchStatsDto` 新增 `attention_files: i64`、`attention_as_of: Option<String>`

- [ ] **Step 1: 写失败的测试**

`src-tauri/src/search/attention_links.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// `MirrorMeta` 的三个字段原样落到 `MirrorLink`。这个适配器是格式知识
    /// 的唯一跨 crate 出口 —— `searchidx` 刻意不认识 `MirrorMeta`,免得
    /// 格式一改就有一边静默错掉。
    #[test]
    fn mirror_metas_map_field_for_field() {
        let metas = vec![crate::sotvault::mirror_meta::MirrorMeta {
            mirror: "sync/x.md".into(),
            device_id: "DEV-1".into(),
            device_name: "mac".into(),
            source: "/Users/bruce/x.md".into(),
            synced_at: 0,
            checksum: "sha256:0".into(),
        }];
        let links = to_links(metas);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].mirror, "sync/x.md");
        assert_eq!(links[0].device_id, "DEV-1");
        assert_eq!(links[0].source, "/Users/bruce/x.md");
    }

    /// 没有 .notemd/mirrors 的 vault 给空列表,不报错。
    #[test]
    fn a_vault_without_mirrors_yields_no_links() {
        let d = tempfile::tempdir().unwrap();
        assert!(links_for_vault(d.path()).is_empty());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd src-tauri && cargo test attention_links
```

Expected: FAIL —— 模块不存在。

- [ ] **Step 3: 写实现**

新建 `src-tauri/src/search/attention_links.rs`:

```rust
//! `MirrorMeta` → `searchidx::attention::MirrorLink` 的唯一适配器。
//!
//! `searchidx` 刻意不认识 `MirrorMeta`:那个格式归 `sotvault` 所有,两个
//! crate 各解析一遍意味着格式一改就有一边静默错掉(而错的方向是「注意力
//! 归错文件」,没有任何症状)。所以镜像记录由命令层读出后传进去。

use std::path::Path;

use searchidx::attention::MirrorLink;

use crate::sotvault::mirror_meta::{self, MirrorMeta};

pub fn links_for_vault(vault_root: &Path) -> Vec<MirrorLink> {
    to_links(mirror_meta::read_all(vault_root))
}

fn to_links(metas: Vec<MirrorMeta>) -> Vec<MirrorLink> {
    metas
        .into_iter()
        .map(|m| MirrorLink { device_id: m.device_id, source: m.source, mirror: m.mirror })
        .collect()
}
```

> `mirror_meta::read_all`(`src-tauri/src/sotvault/mirror_meta.rs:78`)返回
> `Vec<MirrorMeta>` 而**不是** `Result` —— 读不到就是空列表,别再包一层
> `unwrap_or_default`。

`src-tauri/src/search/mod.rs`:
- 加 `mod attention_links;`
- `open_vault` 的建库/sweep 之后、写 `IndexHandle` 之前,插一次摄取:

```rust
    // 索引建好后立刻摄取一次注意力数据:它是排序的输入,晚一步就意味着
    // 用户开 vault 后的第一次搜索拿的是没有注意力的排序。
    let links = attention_links::links_for_vault(root);
    if let Err(e) = idx.refresh_attention(&links) {
        crate::log_cat!("search", "warn", "attention ingest failed: {e}");
    }
```

- `SearchStatsDto` 加两个字段并在构造处填 `s.attention_files` / `s.attention_as_of`(注意 serde 的 camelCase 重命名与既有字段保持一致)。

- [ ] **Step 4: 跑测试确认通过**

```bash
cd src-tauri && cargo test search::
```

Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/search/attention_links.rs src-tauri/src/search/mod.rs
git commit -m "feat(search): 命令层摄取注意力数据与统计 DTO"
```

---

### Task 10: watcher 放行 `.notemd/analytics/` 并独立防抖

**Files:**
- Modify: `src-tauri/src/search/watch.rs`
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: Task 9 的 `attention_links::links_for_vault`
- Produces: `const ATTENTION_DEBOUNCE_SECS: u64 = 60;`

**⚠️ 这是全计划唯一碰既有索引状态机的地方。** `should_forward` 现在把所有 `.` 开头的路径段挡在外面,挡的是 `.git` 的几万个对象;这个针眼开歪了会把 git 操作放进重索引风暴,而 v6.813.1 刚修过「索引一直在构建、重建按钮消失」。注意力事件走**独立通道**,绝不进 `Pending` —— 它触发的是摄取,不是重索引。

- [ ] **Step 1: 写失败的测试**

追加到 `watch.rs` 的 `mod tests`:

```rust
    /// analytics 文件必须被认出来 —— 它们不是 `.md`,且在 `.notemd/` 下,
    /// 两条现行规则各挡它一次。
    #[test]
    fn analytics_files_are_recognized() {
        assert!(is_analytics(".notemd/analytics/2026-08-13.DEV-1.json"));
    }

    /// 针眼只对 analytics 开。`.git` 与 `.notemd` 的其余内容必须照旧挡住,
    /// 否则一次 git 操作就是一场重索引风暴。
    #[test]
    fn the_pinhole_does_not_open_up_the_rest_of_the_dot_dirs() {
        for p in [
            ".git/objects/ab/cdef",
            ".notemd/settings.json",
            ".notemd/mirrors/DEV-1.json",
            ".notemd/analytics-backup/x.json",
        ] {
            assert!(!is_analytics(p), "{p} 不该被当成 analytics");
        }
    }

    /// analytics 事件绝不能进 `Pending`:它触发的是摄取,不是重索引。
    /// 混进去等于每次洞察 flush 都重扫一遍 vault。
    #[test]
    fn analytics_events_never_reach_the_reindex_queue() {
        assert!(!should_forward(".notemd/analytics/2026-08-13.DEV-1.json"));
    }

    /// 60 秒防抖:洞察 store 是持续 flush 的,防抖不到位索引会一直在忙。
    #[test]
    fn the_attention_debounce_is_sixty_seconds() {
        assert_eq!(ATTENTION_DEBOUNCE_SECS, 60);
    }
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd src-tauri && cargo test search::watch
```

Expected: FAIL —— `cannot find function is_analytics`。

- [ ] **Step 3: 写实现**

`watch.rs`:

```rust
/// 注意力摄取的防抖窗口。比索引的 300ms 长两个量级,因为触发它的东西
/// 不一样:洞察 store 在你读文档的整个过程里持续 flush 当天的文件,而
/// 摄取是**全量重算**。60 秒意味着最坏情况下每分钟一次全量重算,而不是
/// 每次 flush 一次。
pub const ATTENTION_DEBOUNCE_SECS: u64 = 60;

/// 这条路径是一份 analytics 日文件吗。
///
/// 针眼开得很窄,而且是**白名单**:`should_forward` 那条「任何 `.` 开头
/// 的路径段一律挡掉」的规则挡的是 `.git` 的几万个对象,放宽它的代价是
/// 一次 git 操作变成一场重索引风暴。所以这里精确匹配目录前缀 + `.json`
/// 后缀,`.notemd` 下的其它任何东西(设置、镜像记录)都不放行。
fn is_analytics(rel: &str) -> bool {
    rel.starts_with(".notemd/analytics/")
        && rel.ends_with(".json")
        && rel.matches('/').count() == 2
}
```

`relevant_paths` 拆成两路返回:普通索引路径 + analytics 命中标志。watcher 回调里,analytics 事件只 `set` 一个 `AtomicBool`(不进 `Pending`);drain 循环每次超时唤醒时检查:距上次摄取满 `ATTENTION_DEBOUNCE_SECS` 且标志为真,就执行摄取并复位标志:

```rust
        let mut last_attention = std::time::Instant::now();
        // …循环内,Timeout 分支的末尾…
        if attention_dirty.swap(false, Ordering::SeqCst)
            || last_attention.elapsed() >= Duration::from_secs(ATTENTION_DEBOUNCE_SECS)
        {
            // 只有真的脏了才干活:定时器到点但没有新事件时什么都不做。
        }
```

> 具体写法按该文件既有的 `stale()` / 代际检查风格来:摄取前必须先
> `if stale() { return }`,与 `drain` 同样的规矩 —— 用户切了 vault 之后,
> 这个线程绝不能再往新 vault 的索引里写旧 vault 的注意力。

摄取执行体:

```rust
fn drain_attention(app: &AppHandle, root: &Path) {
    let idx_handle = crate::search::handle(app);
    let links = crate::search::attention_links::links_for_vault(root);
    let mut guard = crate::search::lock(&idx_handle);
    let Some(idx) = guard.as_mut() else { return };
    match idx.refresh_attention(&links) {
        Ok(n) => crate::log_cat!("search", "info", "attention: {n} files"),
        Err(e) => crate::log_cat!("search", "error", "attention ingest failed: {e}"),
    }
    drop(guard);
    let _ = app.emit(INDEX_UPDATED_EVENT, ());
}
```

- [ ] **Step 4: 跑测试确认通过**

```bash
cd src-tauri && cargo test search::watch
```

Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/search/watch.rs
git commit -m "feat(search): watcher 触发注意力摄取"
```

---

### Task 11: CLI `--json` 暴露 `attention_minutes`

**Files:**
- Modify: `src-tauri/src/cli/search.rs`
- Modify: `src-tauri/src/cli/builtin.rs`(帮助文本)
- Test: `src-tauri/src/cli/builtin.rs` 内既有的帮助文本测试

**Interfaces:**
- Consumes: Task 7 的 `Hit.attention_minutes`

- [ ] **Step 1: 写失败的测试**

`src-tauri/src/cli/builtin.rs` 的 `mod tests` 里,照抄既有 `provenance` 那条的写法追加:

```rust
    /// `--json` 的字段是 agent 的公共约定,加了字段就得写进帮助,
    /// 否则只有读源码的人知道它存在。
    #[test]
    fn search_help_documents_attention_minutes() {
        let out = render_help(Some("search"), false, &[], &HashMap::new());
        assert!(out.contains("attention_minutes"), "必须记录 --json 的注意力字段:\n{out}");
    }
```

- [ ] **Step 2: 跑测试确认失败**

```bash
cd src-tauri && cargo test search_help_documents_attention
```

Expected: FAIL。

- [ ] **Step 3: 写实现**

`src-tauri/src/cli/search.rs` 的 JSON 构造处,与 `provenance` 并列:

```rust
                // 已衰减到今天的注意力分钟数(read + 1.5×edit,30 天半衰期)。
                // 与 `provenance` 并列而不是嵌进去:`provenance` 是文档自己
                // 声明的来源,这个是**你**在它身上花掉的时间 —— 一个来自
                // 文件内容,一个来自你的行为,不该混成一个对象。
                "attention_minutes": h.attention_minutes,
```

`builtin.rs` 的 `search` 帮助文本,在描述 `--json` 字段那段追加一句:

```
attention_minutes(你在这份文档上花过的注意力分钟数,已按 30 天半衰期
衰减到今天;0 = 没有数据。排序已经计入它,这个字段是让你能解释顺序)。
```

**扁平文本输出 `path:line:text` 一个字都不改** —— 前置 spec §5.1 的契约:`notemd search` 的价值在于长得像 grep,agent 按行解析。

- [ ] **Step 4: 跑测试确认通过**

```bash
cd src-tauri && cargo test cli::
```

Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/cli/search.rs src-tauri/src/cli/builtin.rs
git commit -m "feat(cli): search --json 暴露 attention_minutes"
```

---

### Task 12: 设置页覆盖率行与四语文案

**Files:**
- Modify: `src/lib/search/api.ts`
- Modify: `src/components/SettingsDialog.svelte`
- Modify: `src/lib/i18n/en.ts`、`zh.ts`、`ja.ts`、`de.ts`
- Test: `src/components/SettingsDialog.search-tab.test.ts`

**Interfaces:**
- Consumes: Task 9 的 `attentionFiles` / `attentionAsOf`

- [ ] **Step 1: 写失败的测试**

`src/components/SettingsDialog.search-tab.test.ts`,照该文件既有的渲染断言风格追加:

```ts
  it('显示注意力数据覆盖率', async () => {
    // 既有 fixture 的 stats 上补两个字段
    const stats = {
      /* …既有字段…, */
      files: 100,
      attentionFiles: 37,
      attentionAsOf: '2026-08-13',
    }
    // …渲染…
    expect(screen.getByText('37 / 100')).toBeTruthy()
  })

  it('摄取从未跑过时不显示覆盖率行', async () => {
    const stats = { /* …既有字段…, */ attentionFiles: 0, attentionAsOf: null }
    // …渲染…
    expect(screen.queryByText(/注意力|Attention/)).toBeNull()
  })
```

> 该测试文件已有的两处 `originCounts` fixture 也要补上新字段,否则类型报错。

- [ ] **Step 2: 跑测试确认失败**

```bash
pnpm test SettingsDialog.search-tab
```

Expected: FAIL。

- [ ] **Step 3: 写实现**

`src/lib/search/api.ts` 的 stats 类型:

```ts
  /** 有注意力数据的文件数(`doc_attention` 行数)。 */
  attentionFiles: number
  /** 注意力表算到哪一天;`null` = 摄取从未跑过。 */
  attentionAsOf: string | null
```

`SettingsDialog.svelte`,在「来源分层统计」那个 `section` 的末尾追加一行:

```svelte
              <!-- 覆盖率行:摄取「根本没跑起来」在别处没有任何可见症状 ——
                   搜索结果只是安静地退化成没有注意力加权的样子。这是唯一的
                   发现途径,所以哪怕它朴素也必须在。 -->
              {#if indexStatus.stats.attentionAsOf}
                <div class="row">
                  <span class="lbl">{t('search.index.attentionLabel')}</span>
                  <span>{indexStatus.stats.attentionFiles} / {indexStatus.stats.files}</span>
                </div>
              {/if}
```

四语文案(键 `search.index.attentionLabel`,插在各文件 `search.index.tiersHeading` 附近):

```ts
// en.ts
  'search.index.attentionLabel': 'Files with attention data',
// zh.ts
  'search.index.attentionLabel': '有注意力数据的文件',
// ja.ts
  'search.index.attentionLabel': '注意時間データのあるファイル',
// de.ts
  'search.index.attentionLabel': 'Dateien mit Aufmerksamkeitsdaten',
```

- [ ] **Step 4: 跑测试确认通过**

```bash
pnpm test SettingsDialog.search-tab && pnpm check
```

Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add src/lib/search/api.ts src/components/SettingsDialog.svelte src/components/SettingsDialog.search-tab.test.ts src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(settings): 索引统计显示注意力数据覆盖率"
```

---

### Task 13: 全量验证与 mutation 自检

**Files:** 无改动(除非发现缺陷)

- [ ] **Step 1: 全量跑测试**

```bash
cd searchidx && cargo test
cd ../src-tauri && cargo test
cd .. && pnpm check && pnpm test
```

Expected: 全绿。

- [ ] **Step 2: 确认回归集零变化**

```bash
cd searchidx && cargo test --test acceptance retrievability -- --nocapture
git diff --stat tests/fixtures/retrievability.json
```

Expected: 测试通过,且 `retrievability.json` **没有任何改动**。corpus 里没有 `.notemd/analytics`,所以每条命中的注意力都是 0、加成恒 ×1.0。**这份 fixture 出现 diff 就是缺陷信号** —— 回去查 `boost` 在 0 分钟时是否严格返回 1.0、`LEFT JOIN` 是否被写成了 `INNER JOIN`。

- [ ] **Step 3: mutation 自检 —— 逐档隔离**

依次做下面三个改动,每次只改一处,跑 `cd searchidx && cargo test`,记录**哪些**测试变红,然后**恢复**:

| 改动 | 必须变红 | 必须仍然绿 |
| --- | --- | --- |
| `attention::boost` 的 `1.0 + k * frac` 改成 `1.0` | `attention_alone_moves_the_score`、`the_boost_table_matches_the_spec` | 回归集、origin 三档的断言 |
| `Weights::default().attention` 由 `0.4` 改成 `0.0` | `attention_weight_allows_zero_but_rejects_garbage` | 回归集 |
| `sanitized` 里 `attention` 复用 `clean`(即拒绝 0) | `attention_weight_allows_zero_but_rejects_garbage`、`attention_weight_round_trips_from_settings` | 其余全部 |

前置项目(origin tiering)出现过「两个乘数一起推同一方向、任一个单独失效测试仍通过」的假阴性。上表任何一行「必须变红」的格子实际没红,就是测试写松了 —— **补测试,不是接受现状**。

- [ ] **Step 4: 手工冒烟(dev 构建)**

用户自己在 GUI 上验(不做 UI 自动化)。给出的手动步骤:

1. `pnpm tauri dev` 起来,打开一个有 Reading Insights 历史的 vault。
2. 设置 ▸ 搜索:确认「有注意力数据的文件」一行出现,分子不为 0。
3. 在某文档上停留几分钟,等 60 秒后再看搜索面板 —— 用它标题里的词搜,确认它靠前。
4. `/tmp/mdeditor.log`(或当前 identifier 对应的 app.log)里应有 `[search] attention: N files`,且**不该**每几秒刷一条。
5. 设置里把 `searchWeights.attention` 改成 0,重开 vault,确认排序退回原样。

- [ ] **Step 5: 提交**

无代码改动则跳过。若 Step 3 补了测试:

```bash
git add -u searchidx/src
git commit -m "test(searchidx): 补齐注意力档的 mutation 隔离"
```

---

## 自检结果

对着规格逐节核对:

| 规格小节 | 落点 |
| --- | --- |
| §3 摄取全量重算 | Task 2(`fold` 幂等)+ Task 5(`refresh_attention`) |
| §3.1 365 天截断 | Task 2 边界测试 + Task 3 读盘前筛选 |
| §3.2 触发时机 | Task 9(开 vault)+ Task 10(watcher 60s 防抖) |
| §3.3 `abs:` → 镜像,按 deviceId 配对 | Task 2 两条测试 + Task 9 适配器 |
| §4.1 read + 1.5×edit,不计 marks/受众 | Task 1 `minutes_of` + `DocDay` 只声明两个字段 |
| §4.2 衰减、加成表、只加分、查询时二次衰减 | Task 1 + Task 7(`finish` 里的二次衰减) |
| §4.3 `Weights.attention`,k=0 可关 | Task 6 |
| §5 第二候选臂,LIKE 路径不加 | Task 8 |
| §6 UI 不变 / CLI `--json` / 设置页覆盖率 | Task 11 + Task 12 |
| §7 全部测试项 | Task 1–3、7、8、13 |
| §8 残余风险 | 已写进对应代码注释(`decay` 的负值钳制、`collect` 的容错、`is_analytics` 的针眼) |

四处外部助手已核实为真名,计划里用的就是它们:`acceptance.rs:16` 的 `open_temp`(返回 `(TempDir, SearchIndex)`,只开库不建库)、`store.rs` 测试模块的 `tmp()` + `open(&p, "/v", "sync")`、`mirror_meta.rs:78` 的 `read_all`(返回 `Vec`,非 `Result`)、`cli/builtin.rs` 的 `render_help(Some("search"), false, &[], &HashMap::new())`。
