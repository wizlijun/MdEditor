# Roam 增量同步 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `notemd.roam-import` 已有的「同步某一天」之上加一层增量:从持久化的水位出发,把 Roam 里之后真正变过的页(日记页 + wikipage)逐个同步进 vault,水位随成功推进、可中断续跑。

**Architecture:** 现有写入路径 (`convert_page` → `merge` → 原子写) 目前把「日记」写死在里面(标题=日期、OKF type=`Daily Note`、路径=`daily_rel_path`)。第一步把它泛化成「写一页到指定相对路径」,`sync_day` 退化成一个薄调用;随后新增台账 (`ledger.rs`)、落点判定 (`route.rs`)、变更发现与编排 (`incremental.rs`),最后接 UI 与 CLI。合并语义、outline 格式、原子写与「无变化不写」全部沿用,不做任何改动。

**Tech Stack:** Rust(`notemd-plugin-sdk`、serde_json、chrono、regex、tempfile)、Svelte 5 runes + Vite(插件 UI)、cargo test / vitest。

设计依据:`docs/superpowers/specs/2026-08-03-roam-incremental-sync-design.md`。当日同步的既有约定见 `docs/superpowers/specs/2026-08-03-roam-cli-daily-sync-design.md`。

## Global Constraints

- 工作目录是 isolated worktree;共享风险高,**只精确 `git add` 目标文件,绝不 `git add -A`**。
- 后端与宿主的通道是 stdin/stdout NDJSON;**任何 `println!` 到 stdout 都会污染协议**,调试走 `host.log_*`。
- `$activate` 在协议读循环上同步派发:**不得在其中 inline await `host.request`**。
- vault 文件读写用 `std::fs`;`host.vault.*` 只用来问 `host.vault.info`(它同时返回 `wiki_dir` 与 `daily_dir`)。
- **合并语义一行不改**:按块 uid 对位、Roam 为准、本地手写块永不丢、`collapsed`/`type`/`status`/`line`/`answered`/`by` 取本地、内容无变化则连写都不写。
- **`.note.md` 格式一行不改**。`plugins-src/roam-import/backend/tests/fixtures/daily.note.md` 与 `frontmatter-touch.json` 两份 golden 必须保持通过;前者的字节若被本计划改动,须逐行复核并说明。
- OKF §4.1:每篇写出的文档必须有非空 `type`。日记页 `Daily Note`,wikipage `Wiki Page`,与宿主 `outlineConceptType()` 的目录判定一致。
- 时间戳格式与 TS `new Date(ms).toISOString()` 一致:UTC、毫秒、`Z` 结尾。
- 用户可见文案四语言齐全:`en` / `zh` / `ja` / `de`,由 `strings.test.ts` 断言。
- 不做 UI 自动化;GUI 由用户实机验证。

---

### Task 1: 把写入路径从「日记专用」泛化为「写一页」

**Files:**
- Modify: `plugins-src/roam-import/backend/src/convert.rs`(`convert_page`)
- Modify: `plugins-src/roam-import/backend/src/outline.rs`(新增一个常量)
- Modify: `plugins-src/roam-import/backend/src/sync.rs`(抽出 `sync_page`,`sync_day` 改为调用它)

**Interfaces:**
- Consumes: `outline::{touch_frontmatter, parse_outline, serialize_outline, CONCEPT_TYPE_DAILY_NOTE}`、`merge::merge`、`roam_page::RoamPage`。
- Produces:
  - `outline::CONCEPT_TYPE_WIKI_PAGE: &str = "Wiki Page"`
  - `convert::convert_page(page: &RoamPage, title: &str, concept_type: &str) -> outline::Tree`
  - `sync::PageOutcome { pub path: String, pub created: usize, pub updated: usize, pub kept_local: usize, pub roam_gone_kept: usize, pub found: bool, pub wrote: bool }`
  - `sync::sync_page(vault: &Path, rel: &str, page: Option<&RoamPage>, title: &str, concept_type: &str, now: &str) -> Result<PageOutcome, String>`

`wrote` 是新增字段:`false` 表示这次是「无变化,一个字节都没写」。增量同步的报告要区分「同步了但没变」和「真的写了」。

- [ ] **Step 1: 写失败测试**

追加到 `sync.rs` 的 `mod tests`:

```rust
    #[test]
    fn sync_page_writes_a_wiki_page_with_its_own_title_and_type() {
        let dir = tempfile::tempdir().unwrap();
        let page = RoamPage {
            title: "回顾系统".into(), uid: Some("8IFJWtnad".into()),
            create_time: Some(1785600005019), edit_time: None,
            children: vec![RoamBlock {
                uid: Some("b1".into()), string: "第一条".into(), order: 0, heading: None,
                create_time: None, edit_time: None, children: vec![],
            }],
        };
        let out = sync_page(
            dir.path(), "wikipage/回顾系统.note.md", Some(&page),
            "回顾系统", crate::outline::CONCEPT_TYPE_WIKI_PAGE, NOW,
        ).unwrap();
        assert!(out.found && out.wrote);
        assert_eq!(out.created, 1);
        let text = std::fs::read_to_string(dir.path().join("wikipage/回顾系统.note.md")).unwrap();
        assert!(text.contains("type: Wiki Page"), "got:\n{text}");
        assert!(text.contains("title: 回顾系统"), "got:\n{text}");
        assert!(text.contains("- 第一条"));
        assert!(text.contains("id:: b1"));
    }

    #[test]
    fn sync_page_reports_wrote_false_when_nothing_changed() {
        let dir = tempfile::tempdir().unwrap();
        let page = RoamPage {
            title: "回顾系统".into(), uid: Some("u".into()),
            create_time: Some(1785600005019), edit_time: None,
            children: vec![RoamBlock {
                uid: Some("b1".into()), string: "x".into(), order: 0, heading: None,
                create_time: None, edit_time: None, children: vec![],
            }],
        };
        let rel = "wikipage/回顾系统.note.md";
        let first = sync_page(dir.path(), rel, Some(&page), "回顾系统",
                              crate::outline::CONCEPT_TYPE_WIKI_PAGE, NOW).unwrap();
        assert!(first.wrote);
        let second = sync_page(dir.path(), rel, Some(&page), "回顾系统",
                               crate::outline::CONCEPT_TYPE_WIKI_PAGE,
                               "2026-09-09T09:09:09.000Z").unwrap();
        assert!(!second.wrote, "a no-op sync must not write");
    }

    #[test]
    fn sync_page_rejects_a_path_that_escapes_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["/etc/passwd", "../outside.note.md", "wikipage/../../x.note.md", ""] {
            assert!(sync_page(dir.path(), bad, None, "t",
                              crate::outline::CONCEPT_TYPE_WIKI_PAGE, NOW).is_err(), "{bad}");
        }
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml sync_page`
Expected: 编译失败,`cannot find function sync_page`

- [ ] **Step 3: 实现**

1. `outline.rs` 在 `CONCEPT_TYPE_DAILY_NOTE` 旁加 `pub const CONCEPT_TYPE_WIKI_PAGE: &str = "Wiki Page";`,doc 注释指向 `src/lib/okf/concept.ts` 的 `CONCEPT_TYPE.wikiPage`。
2. `convert_page` 由 `(page, date)` 改为 `(page, title, concept_type)`:`touch_frontmatter(None, concept_type, title, …)`。更新它自己的测试(调用点补第三个参数,日记用例传 `CONCEPT_TYPE_DAILY_NOTE`、`title` 仍传日期串)。
3. `sync.rs`:把 `sync_day` 现有的 body 从「算 rel」之后整段搬进新的 `sync_page`,签名如上。路径校验改为**整条相对路径**的校验(非空、非绝对、无 `..` 段、不以 `/` 开头),而不再是 `is_safe_rel_dir(daily_dir)` + `is_iso_date(date)`。原来的 `is_safe_rel_dir` 保留给 `sync_day` 校验 `daily_dir`。
4. `sync_day` 变成:校验 `date`/`daily_dir` → `daily_rel_path` → `sync_page(vault, &rel, page, date, CONCEPT_TYPE_DAILY_NOTE, now)` → 把 `PageOutcome` 映射回 `SyncOutcome`(`date` 字段由 `sync_day` 自己填)。

- [ ] **Step 4: 跑全量确认零回归**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml`
Expected: 全绿,包含 `tests/golden.rs` 的两个 golden。**golden fixture 的字节必须没变** —— 这是纯重构,若 `daily.note.md` 变了说明改错了,回头查。

Run: `pnpm test -- src/lib/outline/roam-golden.test.ts`
Expected: 通过

- [ ] **Step 5: 提交**

```bash
git add plugins-src/roam-import/backend/src/convert.rs plugins-src/roam-import/backend/src/outline.rs plugins-src/roam-import/backend/src/sync.rs
git commit -m "refactor(roam-import): generalise the write path from a daily note to any page"
```

---

### Task 2: 台账 `.notemd/roam-sync.json`

**Files:**
- Create: `plugins-src/roam-import/backend/src/ledger.rs`
- Modify: `plugins-src/roam-import/backend/src/lib.rs`(加 `pub mod ledger;`)

**Interfaces:**
- Consumes: 无。
- Produces:
  - `ledger::LEDGER_REL: &str = ".notemd/roam-sync.json"`
  - `ledger::PageRecord { pub path: String, pub title: String }`
  - `ledger::Ledger { pub graph: Option<String>, pub last_synced_at: Option<String>, pub pages: BTreeMap<String, PageRecord> }`
  - `ledger::Ledger::load(vault: &Path) -> Ledger`(永不失败:文件缺失/JSON 损坏/字段缺失 → 默认值)
  - `ledger::Ledger::save(&self, vault: &Path) -> Result<(), String>`
  - `ledger::Ledger::path_of(&self, uid: &str) -> Option<&str>`
  - `ledger::Ledger::uid_at(&self, path: &str) -> Option<&str>`
  - `ledger::Ledger::claim(&mut self, uid: &str, path: &str, title: &str)`

序列化用 `camelCase`(`lastSyncedAt`),与 spec 的 JSON 样例一致;`BTreeMap` 让 `pages` 的键有序,避免每次保存产生无谓的 git diff。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_loads_as_an_empty_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let l = Ledger::load(dir.path());
        assert!(l.last_synced_at.is_none());
        assert!(l.pages.is_empty());
    }

    #[test]
    fn a_corrupt_file_loads_as_an_empty_ledger_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(dir.path().join(LEDGER_REL), "{ this is not json").unwrap();
        let l = Ledger::load(dir.path());
        assert!(l.pages.is_empty(), "a hand-mangled ledger must degrade to a full rescan, not a crash");
    }

    #[test]
    fn a_partial_file_keeps_what_it_has() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".notemd")).unwrap();
        std::fs::write(dir.path().join(LEDGER_REL), r#"{"lastSyncedAt":"2026-08-01T00:00:00.000Z"}"#).unwrap();
        let l = Ledger::load(dir.path());
        assert_eq!(l.last_synced_at.as_deref(), Some("2026-08-01T00:00:00.000Z"));
        assert!(l.pages.is_empty());
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = Ledger::default();
        l.graph = Some("bruce".into());
        l.last_synced_at = Some("2026-08-03T11:58:41.185Z".into());
        l.claim("8IFJWtnad", "wikipage/回顾系统.note.md", "回顾系统");
        l.save(dir.path()).unwrap();
        let back = Ledger::load(dir.path());
        assert_eq!(back.last_synced_at, l.last_synced_at);
        assert_eq!(back.path_of("8IFJWtnad"), Some("wikipage/回顾系统.note.md"));
        assert_eq!(back.uid_at("wikipage/回顾系统.note.md"), Some("8IFJWtnad"));
    }

    #[test]
    fn claiming_an_existing_uid_replaces_its_record() {
        let mut l = Ledger::default();
        l.claim("u", "wikipage/旧名.note.md", "旧名");
        l.claim("u", "wikipage/新名.note.md", "新名");
        assert_eq!(l.path_of("u"), Some("wikipage/新名.note.md"));
        assert_eq!(l.uid_at("wikipage/旧名.note.md"), None);
        assert_eq!(l.pages.len(), 1);
    }

    #[test]
    fn save_creates_the_notemd_folder() {
        let dir = tempfile::tempdir().unwrap();
        Ledger::default().save(dir.path()).unwrap();
        assert!(dir.path().join(LEDGER_REL).exists());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml ledger`
Expected: 编译失败,`cannot find type Ledger`

- [ ] **Step 3: 实现 `ledger.rs`**

`load` 的降级链:读不到文件 → 默认;`serde_json::from_str` 失败 → 默认;字段缺失 → `#[serde(default)]`。`save` 先 `create_dir_all(vault/.notemd)`,再 `serde_json::to_string_pretty` + 末尾换行写出。`claim` 先移除任何指向该 uid 的旧记录再插入(`pages` 以 uid 为键,直接 `insert` 即可)。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml ledger`
Expected: 6 passed

- [ ] **Step 5: 提交**

```bash
git add plugins-src/roam-import/backend/src/ledger.rs plugins-src/roam-import/backend/src/lib.rs
git commit -m "feat(roam-import): add the incremental sync ledger"
```

---

### Task 3: 落点判定 —— 分类、改名、重名

**Files:**
- Create: `plugins-src/roam-import/backend/src/route.rs`
- Modify: `plugins-src/roam-import/backend/src/lib.rs`(加 `pub mod route;`)

**Interfaces:**
- Consumes: `ledger::Ledger`、`dates`(uid 形状判定可复用 `dates` 的正则,也可自带)。
- Produces:
  - `route::sanitize_file_name(raw: &str) -> String`
  - `route::Target { pub rel: String, pub title: String, pub concept_type: &'static str, pub rename_from: Option<String> }`
  - `route::route_page(uid: &str, roam_title: &str, dirs: (&str, &str), ledger: &Ledger) -> Target` — `dirs` 是 `(wiki_dir, daily_dir)`

`sanitize_file_name` 是 `plugins-src/roam-import/src/lib/outline/slug.ts` 的 Rust 移植,行为必须一致:非法字符 `/ \ : * ? " < > |` → `-`,去首尾空白、前导点、首尾 `-`,空结果 → `untitled`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::Ledger;

    const DIRS: (&str, &str) = ("wikipage", "dailynote");

    #[test]
    fn sanitize_matches_the_typescript_rules() {
        assert_eq!(sanitize_file_name("回顾系统"), "回顾系统");
        assert_eq!(sanitize_file_name("a/b:c*d"), "a-b-c-d");
        assert_eq!(sanitize_file_name("  ..hidden  "), "hidden");
        assert_eq!(sanitize_file_name("///"), "untitled");
        assert_eq!(sanitize_file_name(""), "untitled");
    }

    #[test]
    fn a_daily_uid_routes_to_the_daily_folder_with_the_iso_date_as_title() {
        let t = route_page("08-02-2026", "August 2nd, 2026", DIRS, &Ledger::default());
        assert_eq!(t.rel, "dailynote/2026/2026-08-02.note.md");
        assert_eq!(t.title, "2026-08-02", "a daily note's title is the ISO date, never Roam's English one");
        assert_eq!(t.concept_type, crate::outline::CONCEPT_TYPE_DAILY_NOTE);
        assert!(t.rename_from.is_none());
    }

    #[test]
    fn any_other_uid_routes_to_the_wiki_folder_under_its_sanitised_title() {
        let t = route_page("8IFJWtnad", "回顾/系统", DIRS, &Ledger::default());
        assert_eq!(t.rel, "wikipage/回顾-系统.note.md");
        assert_eq!(t.title, "回顾/系统", "the front-matter title keeps the real Roam title");
        assert_eq!(t.concept_type, crate::outline::CONCEPT_TYPE_WIKI_PAGE);
    }

    #[test]
    fn a_title_change_since_the_last_sync_is_reported_as_a_rename() {
        let mut l = Ledger::default();
        l.claim("u", "wikipage/旧名.note.md", "旧名");
        let t = route_page("u", "新名", DIRS, &l);
        assert_eq!(t.rel, "wikipage/新名.note.md");
        assert_eq!(t.rename_from.as_deref(), Some("wikipage/旧名.note.md"));
    }

    #[test]
    fn an_unchanged_path_is_not_a_rename() {
        let mut l = Ledger::default();
        l.claim("u", "wikipage/名.note.md", "名");
        assert!(route_page("u", "名", DIRS, &l).rename_from.is_none());
    }

    #[test]
    fn a_path_held_by_another_uid_gets_a_numeric_suffix() {
        let mut l = Ledger::default();
        l.claim("other", "wikipage/PKM.note.md", "PKM");
        let t = route_page("mine", "PKM", DIRS, &l);
        assert_eq!(t.rel, "wikipage/PKM (2).note.md");
    }

    #[test]
    fn suffixes_keep_climbing_while_the_path_is_taken() {
        let mut l = Ledger::default();
        l.claim("a", "wikipage/PKM.note.md", "PKM");
        l.claim("b", "wikipage/PKM (2).note.md", "PKM");
        assert_eq!(route_page("c", "PKM", DIRS, &l).rel, "wikipage/PKM (3).note.md");
    }

    #[test]
    fn a_path_held_by_the_same_uid_gets_no_suffix() {
        let mut l = Ledger::default();
        l.claim("u", "wikipage/PKM (2).note.md", "PKM");
        // Re-routing the same page must not climb to (3) every sync.
        let t = route_page("u", "PKM", DIRS, &l);
        assert_eq!(t.rel, "wikipage/PKM (2).note.md");
        assert!(t.rename_from.is_none());
    }
}
```

最后一条是这个任务最容易写错的地方:同一个 uid 上次落在 `PKM (2).note.md`,这次重算若无脑取 `PKM.note.md` 就会每次同步都搬一次文件;若无脑加后缀就会一路爬到 `(3)`、`(4)`。规则是**先看台账里这个 uid 现有的路径,若它与「基础名或其任一后缀变体」相符则原地不动**。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml route`
Expected: 编译失败,`cannot find function route_page`

- [ ] **Step 3: 实现 `route.rs`**

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml route`
Expected: 8 passed

- [ ] **Step 5: 提交**

```bash
git add plugins-src/roam-import/backend/src/route.rs plugins-src/roam-import/backend/src/lib.rs
git commit -m "feat(roam-import): route a Roam page to its vault path, rename and all"
```

---

### Task 4: 变更发现 —— 两维查询与合并

**Files:**
- Modify: `plugins-src/roam-import/backend/src/roam_cli.rs`
- Create: `plugins-src/roam-import/backend/src/changed.rs`
- Modify: `plugins-src/roam-import/backend/src/lib.rs`(加 `pub mod changed;`)

**Interfaces:**
- Consumes: `roam_cli::run`。
- Produces:
  - `roam_cli::changed_blocks_query() -> String`
  - `roam_cli::changed_pages_query() -> String`
  - `roam_cli::fetch_changed(exe: &Path, graph: Option<&str>, since_ms: i64) -> Result<(serde_json::Value, serde_json::Value), String>`
  - `changed::Changed { pub uid: String, pub edited: i64 }`
  - `changed::merge_changed(blocks: &serde_json::Value, pages: &serde_json::Value) -> Result<Vec<Changed>, String>` — 按 `edited` 升序

两条查询串(`?since` 经 `--inputs` 传入,服务端过滤):

```
A) [:find ?uid (max ?t) :keys uid edited :in $ ?since
    :where [?p :block/uid ?uid] [?p :node/title _]
           [?b :block/page ?p] [?b :edit/time ?t] [(> ?t ?since)]]

B) [:find ?uid ?t :keys uid edited :in $ ?since
    :where [?p :node/title _] [?p :block/uid ?uid]
           [?p :edit/time ?t] [(> ?t ?since)]]
```

**两条都要**:实测块维度漏掉改名与建页(块没动),页面实体维度对日记页给的是建页时刻、漏掉内容修改。

- [ ] **Step 1: 写失败测试**

`changed.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merges_both_dimensions_taking_the_later_timestamp() {
        let blocks = json!([{ "uid": "d", "edited": 300 }, { "uid": "both", "edited": 100 }]);
        let pages  = json!([{ "uid": "w", "edited": 200 }, { "uid": "both", "edited": 900 }]);
        let got = merge_changed(&blocks, &pages).unwrap();
        assert_eq!(got, vec![
            Changed { uid: "w".into(), edited: 200 },
            Changed { uid: "d".into(), edited: 300 },
            Changed { uid: "both".into(), edited: 900 },
        ], "ascending by edited, and `both` takes 900 not 100");
    }

    #[test]
    fn empty_on_both_sides_is_an_empty_list_not_an_error() {
        assert!(merge_changed(&json!([]), &json!([])).unwrap().is_empty());
    }

    #[test]
    fn a_uid_in_only_one_dimension_still_appears() {
        let got = merge_changed(&json!([{ "uid": "a", "edited": 1 }]), &json!([])).unwrap();
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn a_row_missing_uid_or_edited_is_skipped_rather_than_failing_the_run() {
        let blocks = json!([{ "edited": 1 }, { "uid": "ok", "edited": 2 }, { "uid": "no-time" }]);
        let got = merge_changed(&blocks, &json!([])).unwrap();
        assert_eq!(got, vec![Changed { uid: "ok".into(), edited: 2 }]);
    }

    #[test]
    fn a_non_array_payload_is_an_error() {
        assert!(merge_changed(&json!({"error": "x"}), &json!([])).is_err());
    }
}
```

追加到 `roam_cli.rs` 的 `mod tests`:

```rust
    #[test]
    fn both_changed_queries_filter_server_side_and_target_the_right_attribute() {
        let b = changed_blocks_query();
        assert!(b.contains(":in $ ?since"));
        assert!(b.contains("[(> ?t ?since)]"));
        assert!(b.contains("[?b :block/page ?p]"), "the block dimension must join through :block/page");
        assert!(b.contains("(max ?t)"));

        let p = changed_pages_query();
        assert!(p.contains(":in $ ?since"));
        assert!(p.contains("[?p :edit/time ?t]"));
        assert!(!p.contains(":block/page"), "the page dimension must NOT join through blocks");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml changed`
Expected: 编译失败,`cannot find function merge_changed`

- [ ] **Step 3: 实现**

`fetch_changed` 用 `roam_cli::run(exe, &["datalog-query", "--query", &q, "--inputs", &format!("[{since_ms}]")], Duration::from_secs(60))`,`--graph` 与 `fetch_day` 同样处理,两条查询各跑一次,返回两个 `Value`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml changed`
Expected: 5 passed;`roam_cli` 的查询断言也通过

- [ ] **Step 5: 提交**

```bash
git add plugins-src/roam-import/backend/src/changed.rs plugins-src/roam-import/backend/src/roam_cli.rs plugins-src/roam-import/backend/src/lib.rs
git commit -m "feat(roam-import): discover changed pages across both edit-time dimensions"
```

---

### Task 5: 编排 —— 水位、逐页同步、可续、dry-run

**Files:**
- Create: `plugins-src/roam-import/backend/src/incremental.rs`
- Modify: `plugins-src/roam-import/backend/src/lib.rs`(加 `pub mod incremental;`)

**Interfaces:**
- Consumes: Task 1–4 的全部产物。
- Produces:
  - `incremental::Renamed { pub uid: String, pub from: String, pub to: String }`
  - `incremental::SyncReport { pub from: Option<String>, pub to: Option<String>, pub scanned: usize, pub synced: usize, pub skipped: usize, pub failed: usize, pub renamed: Vec<Renamed>, pub errors: Vec<String>, pub dry_run: bool }`
  - `incremental::default_since(today: chrono::NaiveDate) -> i64` — 昨天本地 00:00 的毫秒
  - `incremental::sync_since<D, F>(vault: &Path, dirs: (&str, &str), since_override: Option<&str>, today: NaiveDate, now: &str, dry_run: bool, discover: D, fetch: F) -> Result<SyncReport, String>`
    - `D: FnOnce(i64) -> Result<Vec<Changed>, String>`
    - `F: FnMut(&str) -> Result<Option<RoamPage>, String>`(参数是 page uid)

两个不纯的边都注入,与 `sync_requested_day` / `discover_with` 同一手法,整段编排因此不需要子进程即可测。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::roam_page::{RoamBlock, RoamPage};

    const NOW: &str = "2026-08-03T09:00:00.000Z";
    fn today() -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, 8, 3).unwrap() }
    const DIRS: (&str, &str) = ("wikipage", "dailynote");

    fn page(uid: &str, title: &str, body: &str) -> RoamPage {
        RoamPage {
            title: title.into(), uid: Some(uid.into()),
            create_time: Some(1785600005019), edit_time: None,
            children: vec![RoamBlock {
                uid: Some(format!("{uid}-b1")), string: body.into(), order: 0, heading: None,
                create_time: None, edit_time: None, children: vec![],
            }],
        }
    }

    #[test]
    fn syncs_a_daily_and_a_wiki_page_in_one_run() {
        let dir = tempfile::tempdir().unwrap();
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![
                Changed { uid: "08-02-2026".into(), edited: 1000 },
                Changed { uid: "8IFJWtnad".into(), edited: 2000 },
            ]),
            |uid| Ok(Some(match uid {
                "08-02-2026" => page("08-02-2026", "August 2nd, 2026", "日记内容"),
                _ => page("8IFJWtnad", "回顾系统", "概念内容"),
            })),
        ).unwrap();
        assert_eq!((r.scanned, r.synced, r.failed), (2, 2, 0));
        let daily = std::fs::read_to_string(dir.path().join("dailynote/2026/2026-08-02.note.md")).unwrap();
        assert!(daily.contains("type: Daily Note"));
        let wiki = std::fs::read_to_string(dir.path().join("wikipage/回顾系统.note.md")).unwrap();
        assert!(wiki.contains("type: Wiki Page"));
        let l = crate::ledger::Ledger::load(dir.path());
        // `edited` 是纪元毫秒:2000 ms → 1970-01-01T00:00:02.000Z(落地时已核算并改正)
        assert_eq!(l.last_synced_at.as_deref(), Some("1970-01-01T00:00:02.000Z"));
    }

    #[test]
    fn the_watermark_stops_at_the_first_failure_so_nothing_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![
                Changed { uid: "a".into(), edited: 1000 },
                Changed { uid: "b".into(), edited: 2000 },
                Changed { uid: "c".into(), edited: 3000 },
            ]),
            |uid| if uid == "b" { Err("network went away".into()) } else { Ok(Some(page(uid, uid, "x"))) },
        ).unwrap();
        assert_eq!((r.synced, r.failed), (1, 1));
        let l = crate::ledger::Ledger::load(dir.path());
        assert_eq!(l.last_synced_at.as_deref(), Some("1970-01-01T00:00:01.000Z"),
                   "the watermark must stay at `a`, so `b` and `c` are retried next run");
    }

    #[test]
    fn a_page_with_no_blocks_is_skipped_and_creates_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![Changed { uid: "tag".into(), edited: 1000 }]),
            |_| Ok(Some(RoamPage { title: "PKM".into(), uid: Some("tag".into()),
                                   create_time: None, edit_time: None, children: vec![] })),
        ).unwrap();
        assert_eq!((r.synced, r.skipped), (0, 1));
        assert!(!dir.path().join("wikipage/PKM.note.md").exists());
    }

    #[test]
    fn a_renamed_page_moves_its_file_and_keeps_the_local_blocks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("wikipage")).unwrap();
        std::fs::write(dir.path().join("wikipage/旧名.note.md"),
            "---\ntype: Wiki Page\ntitle: 旧名\n---\n- from roam\n  id:: u-b1\n- 我自己写的\n").unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.claim("u", "wikipage/旧名.note.md", "旧名");
        l.save(dir.path()).unwrap();

        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, false,
            |_| Ok(vec![Changed { uid: "u".into(), edited: 1000 }]),
            |_| Ok(Some(page("u", "新名", "from roam"))),
        ).unwrap();

        assert_eq!(r.renamed.len(), 1);
        assert_eq!(r.renamed[0].from, "wikipage/旧名.note.md");
        assert_eq!(r.renamed[0].to, "wikipage/新名.note.md");
        assert!(!dir.path().join("wikipage/旧名.note.md").exists(), "the old file must be moved, not left behind");
        let moved = std::fs::read_to_string(dir.path().join("wikipage/新名.note.md")).unwrap();
        assert!(moved.contains("我自己写的"), "a rename must not lose what the user wrote");
    }

    #[test]
    fn a_dry_run_writes_nothing_and_leaves_the_watermark_alone() {
        let dir = tempfile::tempdir().unwrap();
        let r = sync_since(
            dir.path(), DIRS, None, today(), NOW, true,
            |_| Ok(vec![Changed { uid: "8IFJWtnad".into(), edited: 1000 }]),
            |_| Ok(Some(page("8IFJWtnad", "回顾系统", "x"))),
        ).unwrap();
        assert!(r.dry_run && r.scanned == 1);
        assert!(!dir.path().join("wikipage/回顾系统.note.md").exists());
        assert!(crate::ledger::Ledger::load(dir.path()).last_synced_at.is_none());
    }

    #[test]
    fn nothing_changed_means_nothing_written_and_the_watermark_holds() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.last_synced_at = Some("2026-08-01T00:00:00.000Z".into());
        l.save(dir.path()).unwrap();
        let r = sync_since(dir.path(), DIRS, None, today(), NOW, false,
                           |_| Ok(vec![]), |_| panic!("must not fetch anything")).unwrap();
        assert_eq!((r.scanned, r.synced), (0, 0));
        assert_eq!(crate::ledger::Ledger::load(dir.path()).last_synced_at.as_deref(),
                   Some("2026-08-01T00:00:00.000Z"));
    }

    #[test]
    fn since_override_beats_the_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = crate::ledger::Ledger::default();
        l.last_synced_at = Some("2026-08-01T00:00:00.000Z".into());
        l.save(dir.path()).unwrap();
        let seen = std::cell::Cell::new(0i64);
        sync_since(dir.path(), DIRS, Some("2026-07-01"), today(), NOW, true,
                   |since| { seen.set(since); Ok(vec![]) }, |_| unreachable!()).unwrap();
        assert_eq!(seen.get(), 1782864000000, "2026-07-01T00:00:00Z in ms");
    }

    #[test]
    fn no_vault_and_no_ledger_starts_at_local_yesterday_midnight() {
        let dir = tempfile::tempdir().unwrap();
        let seen = std::cell::Cell::new(0i64);
        sync_since(dir.path(), DIRS, None, today(), NOW, true,
                   |since| { seen.set(since); Ok(vec![]) }, |_| unreachable!()).unwrap();
        assert_eq!(seen.get(), default_since(today()));
    }
}
```

`since_override` 断言里的毫秒数请在实现后用一次实际输出核对;若不符,**核算真值后改断言,不改实现**(实现只负责把 `yyyy-MM-dd` 当 UTC 零点转毫秒),并在报告里写出你的核算过程。

> 落地结果:原稿的 `1782921600000` 是 `2026-07-01T16:00:00Z`(东八区 7 月 2 日零点),真值为 **1782864000000**(20635 天 × 86_400_000,`date -u -j -f '%Y-%m-%d %H:%M:%S' '2026-07-01 00:00:00' +%s` = 1782864000);两处水位断言原稿把 1000/2000 毫秒写成了 2026 年的时刻,真值是 `1970-01-01T00:00:01.000Z` / `...02.000Z`。上面三处已改正。此外按「水位按毫秒组推进」新增了同毫秒失败、首页失败、全失败、改名旧文件缺失、改名目标已存在、dry-run 改名、目录逃逸、水位不可读、补历史不回退等用例,总数由 8 增至 23。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml incremental`
Expected: 编译失败,`cannot find function sync_since`

- [ ] **Step 3: 实现 `incremental.rs`**

顺序:校验 `wiki_dir`/`daily_dir`(空/绝对/含 `..` 一律拒绝,同 `sync_day` 对 `daily_dir` 的做法;
本函数是第一个用宿主给的目录名拼路径的调用方)→ 载入台账 → 解析 since(override > 台账 > `default_since`)→
`discover(since_ms)` → 按 `(edited, uid)` 升序遍历:
`fetch(uid)`(必须先 fetch:`route_page` 要 Roam 标题,而 `Changed` 里没有)→ 页不存在或无块则
`skipped++` 并**仍推进水位**(它确实已经处理完了)→
有块则 `route_page` → 若 `rename_from` 存在、旧文件在**且目标路径上没有文件**(绝不 `rename` 覆盖
用户自己建的那一篇),先 `std::fs::rename`(目标父目录先 `create_dir_all`)并记入 `renamed` →
`sync_page(vault, &t.rel, Some(&p), &t.title, t.concept_type, now)` → `ledger.claim(uid, &t.rel, roam_title)` →
**在组边界**推进水位(见下)→ 每页处理完 `ledger.save`(中途被杀也不丢进度)。
任一页 `Err` → `failed++`、错误进 `errors`、**立即 break**,保留当前水位(改名已发生的话台账照样落盘)。
`dry_run` 为真时:照常 route 与 fetch(为了报出真实的目标路径与改名),但**不 rename、不 sync_page、不 save**
(内存台账照常 `claim`,这样同一批里的重名避让在预演里也是真的)。

**水位规则(按毫秒组原子推进,不要按页推进)**:持久化的水位 = 「小于本批失败页最小 `edited`」
的最大 `edited`,无失败时 = 本批最大 `edited`;实现为「仅当下一页的 `edited` 严格更大时才推进」。
原因:`edited` 不唯一,两个 uid 常带同一毫秒 `T`;若成功一页就推到 `T`、同为 `T` 的另一页随后失败,
下次 `> since` 严格查询就再也看不到它,**永久静默跳页**。水位另需只进不退,免得 `--since` 补历史把
台账拨回过去。详见设计文档 §5。

`default_since(today)`:`today - 1 天` 的本地 00:00 转毫秒。用 `chrono::Local` 的偏移;`today` 已由调用方按本地日历给出。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml incremental`
Expected: 全绿(落地为 23 passed —— 见 Step 1 的补充用例)

- [ ] **Step 5: 跑全量**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml`
Expected: 全绿,两份 golden 未变

- [ ] **Step 6: 提交**

```bash
git add plugins-src/roam-import/backend/src/incremental.rs plugins-src/roam-import/backend/src/lib.rs
git commit -m "feat(roam-import): orchestrate an incremental sync from the watermark"
```

---

### Task 6: 接线 —— ui 方法、CLI 子命令、manifest

**Files:**
- Modify: `plugins-src/roam-import/backend/src/plugin.rs`
- Modify: `plugins-src/roam-import/manifest.v2.json`

**Interfaces:**
- Consumes: `incremental::{sync_since, SyncReport}`、`roam_cli::{fetch_changed, fetch_day}`、`changed::merge_changed`、既有的 `cli_str` / `cli_flag`。
- Produces:
  - ui 方法 `sync_since`,参数 `{ since?: string, graph?: string, roam_path?: string, dry_run?: boolean }`,返回 `SyncReport`
  - ui 方法 `sync_status`,无参,返回 `{ last_synced_at: string | null }`
  - `execute_command` 新增 `"sync-changed"` 分支

manifest:`contributes.cli` 增一条

```json
      {
        "subcommand": "roam-sync",
        "command": "sync-changed",
        "summary": "Sync every Roam page changed since the last sync",
        "args": [],
        "flags": [
          { "long": "--since", "type": "string", "help": "Override the stored watermark (yyyy-MM-dd)" },
          { "long": "--graph", "type": "string", "help": "Roam graph name (default: the CLI's own default)" },
          { "long": "--dry-run", "type": "boolean", "help": "List what would be synced without writing" }
        ],
        "requires_tab_context": false
      }
```

`activation.events` 增 `"onCli:roam-sync"`。版本升到 `1.2.0`。**先读 `plugin-protocol/src/lib.rs` 确认 `CliFlag` 是否接受 `"type": "boolean"`**;若不接受,用它实际支持的写法,不要凭印象填 —— `deny_unknown_fields` 会让整个插件加载失败。

- [ ] **Step 1: 接线**

`plugin.rs` 里加一个内部函数,窗口与 CLI 共用:发现 `roam` 可执行 → 组装两个闭包 → `incremental::sync_since`。`discover` 闭包 = `fetch_changed` + `merge_changed`;`fetch` 闭包 = `fetch_day(exe, graph, uid)` + `roam_page::parse_day_result`。`dirs` 取 `(wiki_dir, daily_dir)`,`wiki_dir` 需要在 `Inner` 里新增字段并在 `activate` 的 `host.vault.info` 回调里填(缺省 `"wikipage"`)。`sync_status` 只读台账。

- [ ] **Step 2: 编译 + 全量测试**

Run: `cargo test --manifest-path plugins-src/roam-import/backend/Cargo.toml`
Expected: 全绿,零警告

- [ ] **Step 3: 装 dev 插件并跑 CLI dry-run**

Run: `bash scripts/dev-install-plugin.sh roam-import`
Run: `notemd roam-sync --dry-run --json`
Expected: 输出 `{"ok":true,"data":{...,"dry_run":true,...}}`,且 `git -C <vault> status --porcelain` 前后一致(**一个字节都没写**)

- [ ] **Step 4: 提交**

```bash
git add plugins-src/roam-import/backend/src/plugin.rs plugins-src/roam-import/manifest.v2.json
git commit -m "feat(roam-import): expose the incremental sync to the window and the CLI"
```

---

### Task 7: 窗口按钮与文案

**Files:**
- Modify: `plugins-src/roam-import/src/lib/bridge.ts`
- Modify: `plugins-src/roam-import/src/App.svelte`
- Modify: `plugins-src/roam-import/src/lib/strings.ts`
- Modify: `plugins-src/roam-import/src/lib/strings.test.ts`

**Interfaces:**
- Consumes: ui 方法 `sync_since` / `sync_status`。
- Produces: `bridge.syncSince(opts)`、`bridge.syncStatus()`,以及对应的 `SyncReport` / `SyncStatus` TS 类型。

- [ ] **Step 1: 写文案键的失败测试**

`strings.test.ts` 的键列表追加:

```ts
  'inc.button', 'inc.lastSynced', 'inc.never', 'inc.running',
  'inc.result', 'inc.nothing', 'inc.renamed', 'inc.failed',
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm --filter roam-import-plugin test`
Expected: FAIL,缺 `inc.button` 等键

- [ ] **Step 3: 补四语言文案 + UI**

zh 示例(en/ja/de 同键):

```
'inc.button': '增量同步',
'inc.lastSynced': '上次同步:{when}',
'inc.never': '尚未同步过',
'inc.running': '正在扫描并同步…',
'inc.result': '扫描 {scanned} 页 · 同步 {synced} · 跳过 {skipped}',
'inc.nothing': '没有需要同步的变更',
'inc.renamed': '已重命名:{from} → {to}',
'inc.failed': '{failed} 页失败,水位已停在失败处,下次会重试',
```

`App.svelte`:在现有「同步当日」按钮旁加「增量同步」按钮(同样只在 `probeResult?.state === 'ready'` 时可用),按钮下方显示 `inc.lastSynced` / `inc.never`;结果区渲染统计、改名清单与失败提示。`onMount` 在 `useCli` 为真时顺带取一次 `sync_status`。

- [ ] **Step 4: 测试 + 类型检查 + 构建**

Run: `pnpm --filter roam-import-plugin test && pnpm --filter roam-import-plugin check && pnpm --filter roam-import-plugin build`
Expected: 全部干净

- [ ] **Step 5: 提交**

```bash
git add plugins-src/roam-import/src
git commit -m "feat(roam-import): add the incremental sync button to the import window"
```

---

### Task 8: README 与端到端

**Files:**
- Modify: `plugins-src/roam-import/README.md`

- [ ] **Step 1: 写文档**

README 增一节「增量同步」:台账位置与字段、两维发现(并说明为何两条查询都必需)、水位语义与可续、日记/wikipage 落点与 OKF type、改名会搬文件但**指向旧名的 wikilink 会断**、`--dry-run`、以及「删除不同步」这条与既有合并语义一致的取舍。

- [ ] **Step 2: 端到端(真实 vault,谨慎)**

先 `git -C <vault> status --porcelain` 存档。然后:

Run: `notemd roam-sync --json`
Expected: 报告里 `scanned/synced` 与你在 Roam 里的实际改动相符;`git -C <vault> diff` 只包含日记/wikipage 两类 `.note.md` 与 `.notemd/roam-sync.json`

Run: `notemd roam-sync --json`(立刻再跑一次)
Expected: `scanned` 为 0,`git -C <vault> status --porcelain` 与上一步完全一致 —— 幂等

**不得** 在 vault 里 `git add` / commit / revert 任何东西;把前后状态与 diff 写进报告,产物原样留给用户查看。

- [ ] **Step 3: 提交**

```bash
git add plugins-src/roam-import/README.md
git commit -m "docs(roam-import): document the incremental sync"
```

- [ ] **Step 4: 交给用户实机验证**

给出 GUI 手动验证清单:开窗 → 勾选 → 看「上次同步」→ 点「增量同步」→ 核对统计 → 在 Roam 改一个 wikipage 标题 → 再同步 → 确认文件被搬走且手写块还在 → 四语言各看一遍。

---

## Self-Review

**Spec coverage**

| spec 章节 | 落在哪个任务 |
|---|---|
| §1 两维发现必需 | Task 4(查询与合并,含断言两条查询各自的关键谓词) |
| §2 台账 schema / vault 内 / 降级 | Task 2 |
| §3 服务端过滤、升序、首次 = 昨天 00:00 | Task 4 + Task 5 |
| §4 落点与 OKF type / 跳过空页 / 改名 / 重名 | Task 1(type 泛化)、Task 3(路由)、Task 5(搬文件与跳过) |
| §5 水位推进与可续 | Task 5 |
| §6 窗口入口 / CLI / 报告 | Task 6、Task 7 |
| §7 测试 | Task 1–5 单测、Task 5 集成、Task 7 文案 |
| §8 验收 1–6 | Task 6 Step 3(dry-run)、Task 8 Step 2(幂等)、Step 4(GUI 清单) |

**Placeholder scan**:无 TBD。唯一一处「先跑再定」是 Task 5 的 `--since` 毫秒断言,给了明确判据(核算真值后改断言不改实现,并写出核算过程);Task 6 的 `"type": "boolean"` 要求先读 protocol 源码再填,理由是 `deny_unknown_fields`。

**Type consistency**:`PageOutcome`(Task 1)被 Task 5 消费;`Ledger` 的 `claim/path_of/uid_at`(Task 2)被 Task 3 与 Task 5 使用;`Target.rename_from`(Task 3)驱动 Task 5 的 `Renamed`;`Changed { uid, edited }`(Task 4)是 Task 5 `discover` 闭包的返回元素;`SyncReport`(Task 5)是 Task 6 两个入口与 Task 7 UI 的共同返回形状。名称在各任务间一致。
