# 索引与搜索设置页 实施计划(项目 A)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让索引从一个看不见的后台派生物,变成用户能看到全部状态、能安全操作、出问题能自己查日志的东西。

**Architecture:** 进度以**回调**形式离开 `searchidx`(核心 crate 仍不依赖 `tauri`);宿主把回调翻译成 Tauri 事件,同时写进一份**独立于索引锁**的状态,让中途打开设置页也能立刻看到进度。重建改为后台任务,命令立即返回。

**Tech Stack:** Rust(`searchidx` / `src-tauri`)· Svelte 5 runes · Tauri 2 事件与命令

**Spec:** `docs/superpowers/specs/2026-08-11-search-index-settings-design.md`

---

## Global Constraints

- **`searchidx` 不得依赖 `tauri`**,不得新增依赖。进度只能通过回调出去。
- **索引永不回写 vault**。
- **一个算法,三个 adapter**:GUI 与 CLI 必须用同一套 `ScanOptions`。阈值回落逻辑只能有一份实现。
- **检索永不阻塞调用方**:任何失败降级 + 一行提示,不报错退出。
- **日志粒度**:分阶段 + 每 500 文件里程碑 + 异常逐条。**不得逐文件写日志** —— `log_bus` 是全局共享的 3000 行环形缓冲。
- **进度回调必须节流**:每 25 个文件或每 200 ms(先到者),外加阶段切换必触发。
- 所有用户可见字符串走 `t()`,`en`/`zh`/`ja`/`de` 四语言同批。
- 测试命令:`cargo test --manifest-path searchidx/Cargo.toml`、`cargo test --manifest-path src-tauri/Cargo.toml`、`pnpm check`(0 errors)、`pnpm test`。
- **共享 worktree**:提交只精确 `git add` 目标文件,**绝不 `git add -A`**。
- 本计划有九次「测试通过的理由不是它声称的那个」的前科(见前置项目 ledger)。**每个断言排序、进度或降级的测试都要 mutation 验证**:把被测行为改坏,确认它变红。

---

## File Structure

### `searchidx/`
| 文件 | 改动 |
| --- | --- |
| `src/scan.rs` | 新增 `Phase`/`Progress`/`ProgressFn`;`build_full`/`sweep`/`sweep_with_budget` 增加进度参数;节流器 |
| `src/lib.rs` | 再导出进度类型 |

### `src-tauri/`
| 文件 | 改动 |
| --- | --- |
| `src/sotvault/vault_settings.rs` | 新增 `search_large_file_threshold_mb` + `merge` 参数 |
| `src/sotvault/mod.rs` | `notemd_vault_settings_set` 透传 |
| `src/search/options.rs`(新) | **唯一**的 `ScanOptions` 构造 + 阈值回落 |
| `src/search/mod.rs` | 用 `options::for_vault`;后台重建;进度状态;`notemd_search_progress`;`search` 分类日志 |
| `src/cli/search.rs` | 改用 `options::for_vault` |
| `src/lib.rs` | 注册新命令 |

### 前端
| 文件 | 改动 |
| --- | --- |
| `src/lib/search/api.ts` | `progress()`、`SearchProgress` 类型 |
| `src/lib/search/index-status.svelte.ts`(新) | 设置页状态 store:stats + 进度 + 事件订阅 |
| `src/lib/search/index-status.test.ts`(新) | store 测试 |
| `src/components/SettingsDialog.svelte` | 新 tab `search`;迁走两个设置 |
| `src/components/side-panel/SearchPanel.svelte` | 重建按钮 → 齿轮按钮 |
| `src/lib/i18n/{en,zh,ja,de}.ts` | `settings.tab.search` + `search.index.*` |

---

## Task 1: 索引阈值与 git 门禁解耦

**Files:**
- Modify: `src-tauri/src/sotvault/vault_settings.rs`
- Modify: `src-tauri/src/sotvault/mod.rs`
- Create: `src-tauri/src/search/options.rs`
- Modify: `src-tauri/src/search/mod.rs`(改用新模块)
- Modify: `src-tauri/src/cli/search.rs`(改用新模块)
- Create: `src-tauri/tests/search_scan_options_contract.rs`

**Interfaces:**
- Produces:
  - `VaultSettings.search_large_file_threshold_mb: Option<u32>`(JSON `searchLargeFileThresholdMb`)
  - `merge(..., search_large_file_threshold_mb: Option<u32>)` —— 追加在参数表末尾
  - `crate::search::options::for_vault(vault_root: &Path) -> searchidx::ScanOptions`

- [ ] **Step 1: 写失败的测试**

在 `vault_settings.rs` 的 `mod tests` 追加:

```rust
    #[test]
    fn search_threshold_round_trips_and_is_absent_when_unset() {
        let tmp = tempfile::tempdir().unwrap();
        let s = VaultSettings { search_large_file_threshold_mb: Some(50), ..Default::default() };
        write(tmp.path(), &s).unwrap();
        assert_eq!(read(tmp.path()).search_large_file_threshold_mb, Some(50));

        write(tmp.path(), &VaultSettings::default()).unwrap();
        let txt = std::fs::read_to_string(tmp.path().join(".notemd/settings.json")).unwrap();
        assert!(!txt.contains("searchLargeFileThresholdMb"), "{txt}");
    }

    #[test]
    fn merge_rejects_a_zero_search_threshold() {
        let base = VaultSettings::default();
        assert!(merge(base, None, None, None, None, None, None, Some(0)).is_err());
    }
```

新建 `src-tauri/tests/search_scan_options_contract.rs`:

```rust
//! GUI 与 CLI 必须用同一份 ScanOptions。它们是两个进程写同一个索引库,
//! 阈值不一致就意味着同一个 vault 被两个口径索引 —— 直接违反「一个算法,
//! 三个 adapter」这条本功能赖以成立的前提。所以回落逻辑只能有一份实现,
//! 这个测试钉住「只有一份」。

use std::path::Path;

fn write_settings(root: &Path, json: &str) {
    std::fs::create_dir_all(root.join(".notemd")).unwrap();
    std::fs::write(root.join(".notemd/settings.json"), json).unwrap();
}

#[test]
fn the_index_threshold_falls_back_to_the_git_gate_when_unset() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{"largeFileThresholdMb": 25}"#);
    let opts = mdeditor_lib::search::options::for_vault(d.path());
    assert_eq!(opts.large_file_threshold_mb, 25, "未设索引阈值时应跟随 git 门禁");
}

#[test]
fn an_explicit_index_threshold_decouples_from_the_git_gate() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{"largeFileThresholdMb": 10, "searchLargeFileThresholdMb": 50}"#);
    let opts = mdeditor_lib::search::options::for_vault(d.path());
    assert_eq!(opts.large_file_threshold_mb, 50, "显式设过就不再跟随");
}

#[test]
fn both_unset_falls_back_to_the_default() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{}"#);
    assert_eq!(mdeditor_lib::search::options::for_vault(d.path()).large_file_threshold_mb, 10);
}

/// 两个 adapter 的构造必须是同一个函数,不是两份「碰巧一致」的实现。
#[test]
fn the_cli_and_the_gui_build_options_through_one_function() {
    let d = tempfile::tempdir().unwrap();
    write_settings(d.path(), r#"{"searchLargeFileThresholdMb": 33, "searchExcludeDirs": ["a"]}"#);
    let gui = mdeditor_lib::search::options::for_vault(d.path());
    let cli = mdeditor_lib::cli::search::scan_options_for(d.path());
    assert_eq!(gui.large_file_threshold_mb, cli.large_file_threshold_mb);
    assert_eq!(gui.exclude_dirs, cli.exclude_dirs);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test search_scan_options_contract`
Expected: 编译失败(`search::options` 不存在)。

- [ ] **Step 3: 加设置字段**

`vault_settings.rs` 的 `VaultSettings` 末尾:

```rust
    /// 索引跳过阈值,与 `large_file_threshold_mb`(git 大文件门禁)**语义不同**:
    /// 那个决定什么不进 commit,这个决定什么不进索引。原始资料类 md
    /// (ebook 导出、字幕转写)常态超过 1 MB,索引可以放宽而 git 门禁不该跟着放。
    ///
    /// 缺失时回落到 git 门禁的值(见 `search::options::for_vault`),所以
    /// 已经调过门禁的用户不会突然发现索引行为变了;一旦显式设过就彻底脱钩。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_large_file_threshold_mb: Option<u32>,
```

`merge` 签名末尾加 `search_large_file_threshold_mb: Option<u32>`,函数体加:

```rust
    if let Some(mb) = search_large_file_threshold_mb {
        if mb == 0 {
            return Err("search index threshold must be at least 1 MB".into());
        }
        out.search_large_file_threshold_mb = Some(mb);
    }
```

其余 `merge` 调用点补 `None`。`sotvault/mod.rs` 的 `notemd_vault_settings_set` 加同名参数并透传。

- [ ] **Step 4: 抽共享的 options 构造**

新建 `src-tauri/src/search/options.rs`:

```rust
//! `ScanOptions` 的**唯一**构造点。
//!
//! GUI 与 CLI 是两个进程,写同一个索引库。它们的扫描口径必须逐字段一致 ——
//! 否则同一个 vault 会被两套阈值/排除规则索引,而这个功能的全部前提是
//! 「一个算法,三个 adapter」。所以这里只有一个函数,两边都调它;
//! `tests/search_scan_options_contract.rs` 钉住这一点。

use std::path::Path;

use searchidx::ScanOptions;

/// 未配置任何阈值时的默认值。与 git 大文件门禁的默认值相同,但这是巧合
/// 不是耦合 —— 两者语义不同,可以各自演化。
const DEFAULT_THRESHOLD_MB: u32 = 10;

pub fn for_vault(vault_root: &Path) -> ScanOptions {
    let vs = crate::sotvault::vault_settings::read(vault_root);
    ScanOptions {
        // 回落链:索引阈值 → git 门禁 → 默认。中间那一跳是一次性的善意,
        // 让既有用户的索引行为不因为这次拆分而改变。
        large_file_threshold_mb: vs
            .search_large_file_threshold_mb
            .or(vs.large_file_threshold_mb)
            .unwrap_or(DEFAULT_THRESHOLD_MB),
        exclude_dirs: vs.search_exclude_dirs.unwrap_or_default(),
    }
}
```

在 `src-tauri/src/search/mod.rs` 加 `pub mod options;`,把既有 `pub fn scan_options` 改成 `pub use options::for_vault as scan_options;`(保留旧名以免动所有调用点),或直接替换调用点 —— 二选一,报告里说明选了哪个。

`src-tauri/src/cli/search.rs` 里的私有 `scan_options` 改为 `pub fn scan_options_for(vault_root: &Path) -> ScanOptions { crate::search::options::for_vault(vault_root) }`,内部调用点改用它。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: 全绿,含 4 条新契约测试。

- [ ] **Step 6: Mutation check**

把 `.or(vs.large_file_threshold_mb)` 删掉,确认 `the_index_threshold_falls_back_to_the_git_gate_when_unset` 变红;恢复。报告里贴结果。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/sotvault/vault_settings.rs src-tauri/src/sotvault/mod.rs src-tauri/src/search/options.rs src-tauri/src/search/mod.rs src-tauri/src/cli/search.rs src-tauri/tests/search_scan_options_contract.rs
git commit -m "feat(search): 索引阈值与 git 大文件门禁解耦,ScanOptions 收成单一构造点"
```

---

## Task 2: 进度回调进核心 crate

**Files:**
- Modify: `searchidx/src/scan.rs`
- Modify: `searchidx/src/lib.rs`

**Interfaces:**
- Consumes: 无
- Produces:
  - `pub enum Phase { Walking, Indexing, Removing, Done }`
  - `pub struct Progress { pub phase: Phase, pub done: usize, pub total: usize, pub current: Option<String>, pub elapsed_ms: u128 }`
  - `pub type ProgressFn<'a> = &'a (dyn Fn(&Progress) + Send + Sync);`
  - `build_full(conn, vault_root, opts, progress: Option<ProgressFn>)`
  - `sweep(conn, vault_root, opts, deadline, progress: Option<ProgressFn>)`

- [ ] **Step 1: 写失败的测试**

`searchidx/src/scan.rs` 的 `mod tests`:

```rust
    use std::sync::{Arc, Mutex};

    fn recording() -> (Arc<Mutex<Vec<(Phase, usize, usize)>>>, impl Fn(&Progress) + Send + Sync) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let l = log.clone();
        (log, move |p: &Progress| l.lock().unwrap().push((p.phase, p.done, p.total)))
    }

    /// 阶段切换必须逐个报告 —— UI 靠它决定显示什么,漏一个就会卡在上一阶段。
    #[test]
    fn every_phase_transition_is_reported() {
        let v = vault(&[("a.md", "x\n"), ("b.md", "y\n")]);
        let mut c = conn_for(v.path());
        let (log, cb) = recording();
        build_full(&mut c, v.path(), &ScanOptions::default(), Some(&cb)).unwrap();
        let phases: Vec<Phase> = log.lock().unwrap().iter().map(|e| e.0).collect();
        assert!(phases.first() == Some(&Phase::Walking), "{phases:?}");
        assert!(phases.contains(&Phase::Indexing), "{phases:?}");
        assert_eq!(phases.last(), Some(&Phase::Done), "{phases:?}");
    }

    /// `total` 在 Walking 阶段还不知道(0),扫描完成后必须被填上真实值 ——
    /// 否则进度条永远停在不确定态。
    #[test]
    fn total_is_unknown_while_walking_and_filled_in_afterwards() {
        let v = vault(&[("a.md", "x\n"), ("b.md", "y\n"), ("c.md", "z\n")]);
        let mut c = conn_for(v.path());
        let (log, cb) = recording();
        build_full(&mut c, v.path(), &ScanOptions::default(), Some(&cb)).unwrap();
        let entries = log.lock().unwrap().clone();
        assert_eq!(entries[0], (Phase::Walking, 0, 0));
        assert!(entries.iter().any(|e| e.0 == Phase::Indexing && e.2 == 3), "{entries:?}");
    }

    /// 节流:60 个文件不能产生 60 次回调。8,826 个文件逐个跨 IPC emit 会把
    /// 主线程淹掉,这条测试是那个约束的机器表达。
    #[test]
    fn indexing_callbacks_are_throttled_not_per_file() {
        let files: Vec<(String, String)> =
            (0..60).map(|i| (format!("f{i}.md"), format!("body {i}\n"))).collect();
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let v = vault(&refs);
        let mut c = conn_for(v.path());
        let (log, cb) = recording();
        build_full(&mut c, v.path(), &ScanOptions::default(), Some(&cb)).unwrap();
        let indexing = log.lock().unwrap().iter().filter(|e| e.0 == Phase::Indexing).count();
        assert!(indexing <= 6, "60 个文件产生了 {indexing} 次 Indexing 回调,节流没生效");
        assert!(indexing >= 2, "一次都没节流出来也不对: {indexing}");
    }

    /// 不传回调时行为必须与从前逐字一致(既有调用点全部传 None)。
    #[test]
    fn a_none_callback_changes_nothing() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        let s = build_full(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_indexed, 1);
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml scan`
Expected: 编译失败。

- [ ] **Step 3: 写实现**

`searchidx/src/scan.rs` 顶部加:

```rust
/// 索引的阶段。UI 用它决定显示什么;顺序即实际执行顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// 遍历 vault,总数尚未知
    Walking,
    /// 逐文件解析入库
    Indexing,
    /// 删除已消失文件的行(仅 sweep)
    Removing,
    Done,
}

/// 一次进度快照。`current` 是 vault 相对路径,已按跨平台规约用 `/` 分隔。
#[derive(Debug, Clone)]
pub struct Progress {
    pub phase: Phase,
    pub done: usize,
    /// Walking 阶段为 0 —— 那时还不知道有多少文件。
    pub total: usize,
    pub current: Option<String>,
    pub elapsed_ms: u128,
}

pub type ProgressFn<'a> = &'a (dyn Fn(&Progress) + Send + Sync);

/// 节流器。逐文件回调在真实 vault(8,826 文件)上会把调用方的事件循环淹掉,
/// 所以按「每 N 个文件或每 T 毫秒,先到者」放行,并强制放行阶段切换 ——
/// 阶段是 UI 的状态机输入,漏一个就会显示错的阶段。
struct Throttle {
    every_n: usize,
    every: Duration,
    last_at: Instant,
    last_n: usize,
}

impl Throttle {
    fn new() -> Self {
        Throttle { every_n: 25, every: Duration::from_millis(200), last_at: Instant::now(), last_n: 0 }
    }
    fn should_emit(&mut self, done: usize, force: bool) -> bool {
        if force || done >= self.last_n + self.every_n || self.last_at.elapsed() >= self.every {
            self.last_at = Instant::now();
            self.last_n = done;
            return true;
        }
        false
    }
}
```

`build_full` 改为:

```rust
pub fn build_full(
    conn: &mut Connection,
    vault_root: &Path,
    opts: &ScanOptions,
    progress: Option<ProgressFn>,
) -> rusqlite::Result<ScanStats> {
    let started = Instant::now();
    let mut throttle = Throttle::new();
    let report = |phase: Phase, done: usize, total: usize, current: Option<&str>| {
        if let Some(f) = progress {
            f(&Progress {
                phase,
                done,
                total,
                current: current.map(|s| s.to_string()),
                elapsed_ms: started.elapsed().as_millis(),
            });
        }
    };

    report(Phase::Walking, 0, 0, None);
    let (candidates, skipped) = walk(vault_root, opts);
    let total = candidates.len();
    let mut stats = ScanStats { files_skipped_large: skipped, ..Default::default() };

    let tx = conn.transaction()?;
    tx.execute_batch("DELETE FROM blocks_fts; DELETE FROM blocks; DELETE FROM links; DELETE FROM files;")?;
    for (i, c) in candidates.iter().enumerate() {
        if index_into(&tx, vault_root, c)? {
            stats.files_indexed += 1;
        }
        // 强制放行第一个,让 UI 立刻从 Walking 切到 Indexing 并拿到 total
        if throttle.should_emit(i + 1, i == 0) {
            report(Phase::Indexing, i + 1, total, Some(&c.rel));
        }
    }
    tx.commit()?;
    store::meta_set(conn, "built_at", &format!("{}", now_secs()))?;
    stats.took_ms = started.elapsed().as_millis();
    report(Phase::Done, total, total, None);
    Ok(stats)
}
```

`sweep` / `sweep_with_budget` 同构处理:`Walking` → `Indexing`(逐候选)→ `Removing`(删除阶段,`done` 为已删数)→ `Done`。`sweep` 的签名末尾加 `progress: Option<ProgressFn>`,`sweep_with_budget` 同。

`lib.rs` 再导出:`pub use scan::{Phase, Progress, ProgressFn};`

**既有调用点**(`SearchIndex::rebuild`/`sweep`、`cli/search.rs`、`watch::drain`)全部传 `None`。`SearchIndex` 门面新增 `rebuild_with_progress` / `sweep_with_progress`,旧方法保留并内部传 `None`。

- [ ] **Step 4: 跑测试 + mutation check**

Run: `cargo test --manifest-path searchidx/Cargo.toml`
Expected: 全绿。

Mutation:把 `should_emit` 改成永远返回 `true`,确认 `indexing_callbacks_are_throttled_not_per_file` 变红;恢复。贴结果。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/scan.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): 节流的进度回调 —— 阶段、计数、当前文件"
```

---

## Task 3: 索引日志统一走 `search` 分类

**Files:**
- Modify: `src-tauri/src/search/mod.rs`、`src-tauri/src/search/watch.rs`

**Interfaces:** 无新公开接口。

- [ ] **Step 1: 写失败的测试**

`src-tauri/src/search/mod.rs` 的测试模块:

```rust
    /// 一次全量重建产出的 search 分类日志必须在个位到几十行,而不是逐文件。
    /// log_bus 是全局共享的 3000 行环形缓冲 —— 逐文件写会把 git sync、插件、
    /// 前端 console 的日志全部冲掉,连自己早期的行也留不住。
    #[test]
    fn a_full_rebuild_logs_milestones_not_every_file() {
        let _g = crate::log_bus::test_guard();
        crate::log_bus::clear();

        let v = tempfile::tempdir().unwrap();
        for i in 0..300 {
            std::fs::write(v.path().join(format!("f{i}.md")), format!("body {i}\n")).unwrap();
        }
        let d = tempfile::tempdir().unwrap();
        let mut idx = searchidx::SearchIndex::open_at(v.path(), &d.path().join("i.db")).unwrap();
        log_rebuild_with(&mut idx, &searchidx::ScanOptions::default(), None).unwrap();

        let lines: Vec<_> = crate::log_bus::snapshot()
            .into_iter()
            .filter(|l| l.category == "search")
            .collect();
        assert!(!lines.is_empty(), "一条都没记");
        assert!(lines.len() < 30, "300 个文件产生了 {} 行 search 日志,粒度太细", lines.len());
        assert!(lines.iter().any(|l| l.message.contains("300")), "汇总行应含文件总数: {lines:?}");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml search::`
Expected: 编译失败(`log_rebuild` 不存在)。

- [ ] **Step 3: 写实现**

在 `search/mod.rs` 加一个把进度翻译成日志的封装:

```rust
/// 索引日志的唯一出口。粒度是刻意选的(spec §5):分阶段 + 每 500 文件
/// 里程碑 + 异常逐条,**不逐文件**。理由见上面那条测试的注释。
/// `extra` 是调用方追加的回调(Task 4 用它写进度状态并 emit 事件)。
/// 日志与进度共用同一次回调,避免核心 crate 被要求支持多个订阅者。
pub(crate) fn log_rebuild_with(
    idx: &mut SearchIndex,
    opts: &ScanOptions,
    extra: Option<&(dyn Fn(&searchidx::Progress) + Send + Sync)>,
) -> Result<searchidx::ScanStats, String> {
    use searchidx::Phase;
    crate::log_cat!("search", "info",
        "rebuild start: vault={} threshold={}MB excludes={:?}",
        idx.vault_root().display(), opts.large_file_threshold_mb, opts.exclude_dirs);

    let last_logged = std::sync::atomic::AtomicUsize::new(0);
    let cb = move |p: &searchidx::Progress| {
        if let Some(f) = extra { f(p) }
        match p.phase {
            Phase::Walking => {}
            Phase::Indexing => {
                if p.done == 0 { return }
                // 每 500 个一行
                let prev = last_logged.load(std::sync::atomic::Ordering::Relaxed);
                if p.done >= prev + 500 {
                    last_logged.store(p.done, std::sync::atomic::Ordering::Relaxed);
                    crate::log_cat!("search", "info", "indexing {}/{}", p.done, p.total);
                }
            }
            Phase::Removing | Phase::Done => {}
        }
    };
    let stats = idx.rebuild_with_progress(opts, Some(&cb))?;

    crate::log_cat!("search", "info",
        "rebuild done: {} indexed, {} removed, {} ms",
        stats.files_indexed, stats.files_removed, stats.took_ms);
    for path in &stats.files_skipped_large {
        crate::log_cat!("search", "warn", "skipped (over threshold): {path}");
    }
    Ok(stats)
}
```

同时把 `search/mod.rs`、`search/watch.rs` 里既有的 `dlog!` 索引相关调用改成 `log_cat!("search", …)`。

- [ ] **Step 4: 跑测试 + mutation check**

Run: `cargo test --manifest-path src-tauri/Cargo.toml search::`

Mutation:把每 500 的门槛改成每 1,确认行数断言变红;恢复。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/search/mod.rs src-tauri/src/search/watch.rs
git commit -m "feat(search): 索引日志统一走 search 分类,里程碑粒度"
```

---

## Task 4: 后台重建 + 独立进度状态 + 新命令

这是本计划的核心不变量所在:**进度必须在重建进行中可读**。现在 `notemd_search_rebuild` 全程持有索引锁,所以 `notemd_search_stats` 也读不到 —— 设置页无法靠轮询报告进度。

**Files:**
- Modify: `src-tauri/src/search/mod.rs`
- Modify: `src-tauri/src/lib.rs`(注册命令)

**Interfaces:**
- Produces:
  - `pub struct ProgressDto { pub phase: String, pub done: usize, pub total: usize, pub current: Option<String>, pub elapsedMs: u128 }`(serde camelCase)
  - `notemd_search_progress(app) -> Option<ProgressDto>` —— **不碰索引锁**
  - `notemd_search_rebuild` 改为立即返回 `Result<(), String>`
  - 事件 `search://progress`

- [ ] **Step 1: 写失败的测试**

```rust
    /// 本任务的核心不变量:重建进行中,进度依然读得到。
    /// 如果进度状态和索引句柄共用一把锁,这条会挂在 lock 上超时。
    #[test]
    fn progress_is_readable_while_the_index_lock_is_held() {
        let state = ProgressState::default();
        let handle: IndexHandle = Default::default();
        let _held = handle.lock().unwrap(); // 模拟重建期间持锁

        state.set(Some(searchidx::Progress {
            phase: searchidx::Phase::Indexing,
            done: 7, total: 100, current: Some("a.md".into()), elapsed_ms: 12,
        }));
        let got = state.get().expect("持索引锁时进度必须仍可读");
        assert_eq!(got.done, 7);
        assert_eq!(got.total, 100);
    }

    /// 重建进行中再点重建必须被拒,而不是排队 —— 排队意味着用户连点三下
    /// 就锁死三轮全量重建。
    #[test]
    fn a_second_rebuild_is_refused_while_one_is_running() {
        let flag = RebuildFlag::default();
        assert!(flag.try_begin(), "第一次应当拿到");
        assert!(!flag.try_begin(), "进行中第二次必须被拒");
        flag.end();
        assert!(flag.try_begin(), "结束后应当可以再来");
    }

    /// 完成后进度必须清空,否则设置页会一直显示一个停在 100% 的旧进度。
    #[test]
    fn progress_is_cleared_when_the_run_finishes() {
        let state = ProgressState::default();
        state.set(Some(searchidx::Progress {
            phase: searchidx::Phase::Done, done: 5, total: 5, current: None, elapsed_ms: 1,
        }));
        state.clear();
        assert!(state.get().is_none());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml search::`

- [ ] **Step 3: 写实现**

```rust
/// 进度状态。**刻意与 `IndexHandle` 用不同的锁** —— 重建全程持有索引锁,
/// 若进度也挂在那把锁上,设置页就只能在重建结束后才读到进度,而那正是
/// 唯一需要进度的时候。
#[derive(Default, Clone)]
pub struct ProgressState(Arc<Mutex<Option<searchidx::Progress>>>);

impl ProgressState {
    pub fn set(&self, p: Option<searchidx::Progress>) {
        if let Ok(mut g) = self.0.lock() { *g = p }
    }
    pub fn get(&self) -> Option<searchidx::Progress> {
        self.0.lock().ok().and_then(|g| g.clone())
    }
    pub fn clear(&self) { self.set(None) }
}

/// 单次重建的守门。`try_begin` 失败即「已有一轮在跑」,命令直接返回错误 ——
/// 排队会让连点变成连续多轮全量重建。
#[derive(Default, Clone)]
pub struct RebuildFlag(Arc<std::sync::atomic::AtomicBool>);

impl RebuildFlag {
    pub fn try_begin(&self) -> bool {
        self.0.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }
    pub fn end(&self) { self.0.store(false, Ordering::SeqCst) }
}
```

`init` 里 `app.manage(ProgressState::default())` 与 `app.manage(RebuildFlag::default())`。

`notemd_search_rebuild` 改为:

```rust
#[tauri::command]
pub fn notemd_search_rebuild(app: AppHandle) -> Result<(), String> {
    let flag = app.state::<RebuildFlag>().inner().clone();
    if !flag.try_begin() {
        return Err("rebuild already running".into());
    }
    let progress = app.state::<ProgressState>().inner().clone();
    let idx_handle = handle(&app);
    let app2 = app.clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let mut guard = lock(&idx_handle);
            let idx = require_index_mut(&mut guard)?;
            let root = idx.vault_root().to_path_buf();
            let opts = options::for_vault(&root);
            let p = progress.clone();
            let a = app2.clone();
            let cb = move |pr: &searchidx::Progress| {
                p.set(Some(pr.clone()));
                let _ = a.emit("search://progress", progress_dto(pr));
            };
            log_rebuild_with(idx, &opts, &cb)?;
            Ok(())
        })();
        progress.clear();
        flag.end();
        if let Err(e) = result {
            crate::log_cat!("search", "error", "rebuild failed: {e}");
        }
        let _ = app2.emit("search://index-updated", ());
    });
    Ok(())
}

#[tauri::command]
pub fn notemd_search_progress(app: AppHandle) -> Option<ProgressDto> {
    app.state::<ProgressState>().get().as_ref().map(progress_dto)
}
```

在 `src-tauri/src/lib.rs` 的 `invoke_handler` 注册 `search::notemd_search_progress`。

- [ ] **Step 4: 跑测试 + mutation check**

Mutation:把 `ProgressState` 改成复用 `IndexHandle` 的锁,确认 `progress_is_readable_while_the_index_lock_is_held` 挂死/变红;恢复。**这条是本任务存在的理由,必须验。**

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/search/mod.rs src-tauri/src/lib.rs
git commit -m "feat(search): 重建改后台任务,进度状态独立于索引锁"
```

---

## Task 5: 设置页 tab 骨架 + 索引状态 + i18n

**Files:**
- Create: `src/lib/search/index-status.svelte.ts`、`src/lib/search/index-status.test.ts`
- Modify: `src/lib/search/api.ts`
- Modify: `src/components/SettingsDialog.svelte`
- Modify: `src/lib/i18n/{en,zh,ja,de}.ts`

**Interfaces:**
- Produces:
  - `searchApi.progress(): Promise<SearchProgress | null>`
  - `indexStatus.{ stats, progress, loading, error }`、`indexStatus.refresh()`、`indexStatus.subscribe()`

- [ ] **Step 1: 写失败的 store 测试**

```ts
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { indexStatus, _setIndexApi } from './index-status.svelte'

beforeEach(() => indexStatus.reset())

describe('indexStatus', () => {
  it('中途打开时从 progress() 拉到当前进度,而不是空等下一个事件', async () => {
    _setIndexApi({
      stats: async () => ({ files: 10, blocks: 40, dbBytes: 1024, builtAt: null, tokenizerId: 'v1' }),
      progress: async () => ({ phase: 'indexing', done: 3, total: 10, current: 'a.md', elapsedMs: 5 }),
      rebuild: async () => {},
    })
    await indexStatus.refresh()
    expect(indexStatus.progress?.done).toBe(3)
    expect(indexStatus.stats?.files).toBe(10)
  })

  it('进度事件覆盖轮询到的快照', async () => {
    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild: async () => {} })
    indexStatus.applyProgress({ phase: 'indexing', done: 8, total: 10, current: 'b.md', elapsedMs: 9 })
    expect(indexStatus.progress?.done).toBe(8)
  })

  it('完成事件清空进度,避免停在 100% 不动', async () => {
    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild: async () => {} })
    indexStatus.applyProgress({ phase: 'indexing', done: 9, total: 10, current: null, elapsedMs: 9 })
    indexStatus.applyProgress({ phase: 'done', done: 10, total: 10, current: null, elapsedMs: 10 })
    expect(indexStatus.progress).toBeNull()
  })

  // 索引未就绪是启动期的正常状态,不是崩溃 —— 面板必须说人话。
  it('把后端的 not ready 呈现为状态而不是报错串', async () => {
    _setIndexApi({
      stats: async () => { throw new Error('search index not ready') },
      progress: async () => null,
      rebuild: async () => {},
    })
    await indexStatus.refresh()
    expect(indexStatus.notReady).toBe(true)
    expect(indexStatus.error).toBeNull()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- index-status`

- [ ] **Step 3: 实现 store 与 api**

`api.ts` 增加:

```ts
export interface SearchProgress {
  phase: 'walking' | 'indexing' | 'removing' | 'done'
  done: number
  total: number
  current: string | null
  elapsedMs: number
}
// searchApi 增加:
progress: () => invoke<SearchProgress | null>('notemd_search_progress'),
```

`index-status.svelte.ts`:注入式 api(便于测试)、`refresh()` 并发拉 stats + progress、`applyProgress()` 在 `phase === 'done'` 时清空、`notReady` 用 `isIndexNotReady()`(已存在于 `store.svelte.ts`)判定。

- [ ] **Step 4: 加 tab 与状态区**

`SettingsDialog.svelte` 的 `tab-strip`,在 `vault` 之后加:

```svelte
<button class:active={selectedTab === 'search'}
        onclick={() => { selectedTab = 'search'; void indexStatus.refresh() }}>
  {t('settings.tab.search')}
</button>
```

内容区加 `{:else if selectedTab === 'search'}` 分支:vault 未配置时只显示一句说明;否则显示索引状态表(文件数/块数/库大小/建立时间/tokenizer),外加一个**分层统计占位块**(项目 B 回填,现在显示 `t('search.index.tiersPending')`)。

i18n 四语言新增键:`settings.tab.search`、`search.index.files`、`search.index.blocks`、`search.index.dbSize`、`search.index.builtAt`、`search.index.tokenizer`、`search.index.noVault`、`search.index.tiersPending`。

- [ ] **Step 5: 跑检查**

Run: `pnpm check && pnpm test`
Expected: 0 errors,全绿。

- [ ] **Step 6: 提交**

```bash
git add src/lib/search/api.ts src/lib/search/index-status.svelte.ts src/lib/search/index-status.test.ts src/components/SettingsDialog.svelte src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(settings): 搜索与索引 tab —— 索引状态与分层统计占位"
```

---

## Task 6: 进度 UI + 重建二次确认

**Files:**
- Modify: `src/components/SettingsDialog.svelte`
- Modify: `src/lib/search/index-status.svelte.ts`
- Modify: `src/lib/i18n/{en,zh,ja,de}.ts`

- [ ] **Step 1: 写失败的测试**

```ts
  it('确认对话框取消时不触发重建', async () => {
    const rebuild = vi.fn()
    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild })
    await indexStatus.requestRebuild(async () => false) // 用户点了取消
    expect(rebuild).not.toHaveBeenCalled()
  })

  it('确认后才触发重建', async () => {
    const rebuild = vi.fn()
    _setIndexApi({ stats: async () => null, progress: async () => null, rebuild })
    await indexStatus.requestRebuild(async () => true)
    expect(rebuild).toHaveBeenCalledTimes(1)
  })

  // 后端已在跑时会返回 rebuild already running —— 这不是崩溃,要说人话。
  it('把已在运行呈现为提示而不是错误', async () => {
    _setIndexApi({
      stats: async () => null, progress: async () => null,
      rebuild: async () => { throw new Error('rebuild already running') },
    })
    await indexStatus.requestRebuild(async () => true)
    expect(indexStatus.busyNotice).toBe(true)
  })
```

- [ ] **Step 2-3: 跑失败 → 实现**

`requestRebuild(confirm: () => Promise<boolean>)`:先 `confirm()`,false 直接返回;true 才调 `rebuild()`;捕获 `rebuild already running` 置 `busyNotice`。

设置页进度区(仅 `indexStatus.progress` 非空时渲染):阶段文案、`done/total` 与百分比、进度条、当前文件(路径过长时中间省略)、已耗时。

二次确认用现有的对话框组件(`grep -rn "confirm" src/lib/dialogs.ts` 找既有实现)。文案必须说清:全量重建、期间搜索不可用、预计耗时(按 `stats.files` 估)、**不会丢失任何笔记**。

订阅 `search://progress` 与 `search://index-updated`,分别 `applyProgress` 与 `refresh`。

- [ ] **Step 4: 检查 + 提交**

Run: `pnpm check && pnpm test`

```bash
git add src/components/SettingsDialog.svelte src/lib/search/index-status.svelte.ts src/lib/search/index-status.test.ts src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(settings): 索引进度显示与重建二次确认"
```

---

## Task 7: 设置迁移 + 跳过清单 + 日志跳转

**Files:**
- Modify: `src/components/SettingsDialog.svelte`
- Modify: `src-tauri/src/search/mod.rs`(跳过清单命令)
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/i18n/{en,zh,ja,de}.ts`

- [ ] **Step 1: 后端暴露跳过清单**

`SearchStatsDto` 增加 `skippedLarge: Vec<SkippedDto>`(路径 + 字节数),来自最近一次扫描的 `ScanStats.files_skipped_large`。需要在 `search/mod.rs` 里把最近一次 stats 存进一个轻量状态(与进度状态同样独立于索引锁)。

- [ ] **Step 2: 前端迁移两个设置**

把 `searchExcludeDirs` 的 label/textarea/保存逻辑从 vault tab 移到 search tab(整块搬,不复制)。新增 `searchLargeFileThresholdMb` 输入框,附一句说明它与 git 门禁的区别,以及**一旦显式设置就不再跟随**(spec §9 点名这是最容易误解的地方)。

vault tab 保留 `largeFileThresholdMb`,label 不变。

- [ ] **Step 3: 跳过清单与日志按钮**

跳过清单逐条显示路径 + 实际大小 + 一句「不进索引,`rg` 仍可查」。

「查看日志」按钮调用后端已存在的 `open_logs_window(app, Some("search"))` —— 需要一个薄命令包装(`grep -n "open_logs_window" src-tauri/src/lib.rs` 看既有调用点怎么触发的,照抄)。

- [ ] **Step 4: 检查 + 提交**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && pnpm check && pnpm test`

```bash
git add src/components/SettingsDialog.svelte src-tauri/src/search/mod.rs src-tauri/src/lib.rs src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(settings): 迁移搜索设置、跳过清单、日志跳转"
```

---

## Task 8: 面板改为跳转按钮

**Files:**
- Modify: `src/components/side-panel/SearchPanel.svelte`
- Modify: `src/lib/i18n/{en,zh,ja,de}.ts`

- [ ] **Step 1: 实现**

删除面板里的「重建索引」按钮及其 `rebuilding` 状态、确认逻辑、禁用输入的分支。换成齿轮图标按钮,`title` 为 `t('search.openIndexSettings')`,点击:打开偏好设置并把 `selectedTab` 设为 `search`。

打开设置页的既有入口:`grep -rn "openSettings" src/lib/commands.ts` —— 复用它,并加一个可选的 tab 参数。

面板保留:结果计数、耗时、`t1-scan` 降级提示。

> **注意:** 移除 `rebuilding` 会连带移除面板对「本地发起的重建」的输入禁用。这是刻意的 —— 重建入口已经不在面板里了。但**外部发起的重建**仍会让查询阻塞(见前置项目 ledger 的 KNOWN GAP),保留那条注释。

- [ ] **Step 2: 检查 + 提交**

Run: `pnpm check && pnpm test`

```bash
git add src/components/side-panel/SearchPanel.svelte src/lib/commands.ts src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(ui): 搜索面板的重建按钮改为跳转索引设置"
```

---

## 人工 GUI 验收清单(交付给用户)

自动化测试覆盖不到这些,必须实机点一遍:

1. 打开偏好设置,「搜索与索引」tab 显示正确的文件数/块数/库大小
2. 点「重建索引」,确认对话框出现;点取消 → 什么都没发生
3. 再点一次并确认 → 进度条动、当前文件在变、百分比推进、完成后统计更新
4. 重建期间点搜索面板 → 查询被正确处理(不是静默卡死)
5. 重建期间再点重建 → 提示已在运行,不排队
6. 「查看日志」→ 日志窗口打开且只显示 `search` 分类
7. 改 `searchExcludeDirs` 保存 → 下次 sweep 生效
8. 改索引阈值 → 与 Vault tab 的 git 门禁互不影响
9. 搜索面板的齿轮按钮 → 打开设置并停在「搜索与索引」tab
10. 深色/浅色主题下两处 UI 都正常

---

## 已知取舍

- **`total` 在 Walking 阶段未知**,进度条前几百毫秒是不确定态。可接受。
- **节流参数(25 文件 / 200 ms)是拍的**,真实 vault 上可能偏密或偏疏,发布前按实测调一次。
- **阈值回落是单向门**:显式设过就不再跟随 git 门禁。UI 必须说明,否则会有人以为改门禁还能影响索引。
- **CLI 侧不显示进度**,`notemd search --rebuild` 保持现状(同步 + 完成后打印汇总)。
