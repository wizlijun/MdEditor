# 重命名/移动检测 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用索引里已有的 `content_hash` 认出重命名与移动,只重算路径派生的文件级元数据,不重算块 —— 让整目录改名不再是一次全量重建。

**Architecture:** 一个纯函数按 `(size, mtime)` 把「新路径」与「孤儿行」配对,调用方读文件用 hash 确认;确认后复用 `chunk::parse_file` 取元数据、丢弃其块,走一条 `UPDATE files`。`blocks`/`links`/`blocks_fts` 一行不动 —— 分词与 FTS 写入是省下来的大头。sweep 与 watcher 批次共用同一个纯函数。

**Tech Stack:** Rust(`searchidx`、`src-tauri`)· SQLite

**Spec:** `docs/superpowers/specs/2026-08-13-rename-detection-design.md`

## Global Constraints

- **`path#Lnnn` 引用契约不可破。** 快路径的定义就是不重算块,所以「改名后行号仍指对」必须端到端验证。
- **hash 确认不可省。** `(size, mtime)` 只是无 I/O 预筛;配错的后果是索引里出现「这个路径下是那个文件的内容」。
- **两条必须退回全量重建的边界**:新旧路径的分块器类别不同;目标路径已被占用(UNIQUE 约束)。
- **已配对的旧路径必须显式从删除集合摘除**,不得依赖「`DELETE ... WHERE path=旧路径` 恰好匹配不到」。
- **元数据推导只能有一条路径** —— 复用 `chunk::parse_file`,不新写只算元数据的函数。
- **不引入 Git**,不改删除语义、不改超时语义、不改 `index_one` 的单文件契约。
- `searchidx` 不得依赖 `tauri`。
- 测试命令:`cargo test --manifest-path searchidx/Cargo.toml`、`cargo test --manifest-path src-tauri/Cargo.toml -- --test-threads=1`(`plugin_runtime_integration.rs` 是**既有** flake,不要追)。
- **共享 worktree**:只精确 `git add`,绝不 `git add -A`。
- **磁盘曾满**:mutation 检查就地改再还原,用 `git status --porcelain` 确认,不要复制整仓。

---

## File Structure

| 文件 | 改动 |
| --- | --- |
| `searchidx/src/scan.rs` | 抽出 `ext_of` / `chunker_class`;sweep 接入配对 |
| `searchidx/src/rename.rs`(新) | 纯函数:新路径 × 孤儿 → 配对 |
| `searchidx/src/store.rs` | `rename_file`(UPDATE,不碰 blocks) |
| `searchidx/src/lib.rs` | 注册 `rename` 模块 |
| `src-tauri/src/search/watch.rs` | 批次层接入配对 |

---

## Task 1: 抽出两个路径派生助手

快路径要判断「分块器类别是否相同」并重算 `ext`。两者今天都以内联形式散落在 `scan.rs`/`chunk.rs` 里。先抽出来,后面的任务才有唯一的一处可用。

**Files:** Modify `searchidx/src/scan.rs`

**Interfaces:** Produces
- `pub(crate) fn ext_of(rel: &str) -> &'static str` —— `"note.md" | "srt" | "vtt" | "txt" | "md"`
- `pub(crate) fn chunker_class(rel: &str) -> ChunkerClass`,`#[derive(PartialEq, Eq, Debug, Clone, Copy)] pub(crate) enum ChunkerClass { Outline, Transcript, Plain, Prose }`

- [ ] **Step 1: 写失败的测试**

```rust
    #[test]
    fn ext_of_covers_every_indexable_shape() {
        assert_eq!(ext_of("a/b.note.md"), "note.md");
        assert_eq!(ext_of("a/b.md"), "md");
        assert_eq!(ext_of("a/b.SRT"), "srt", "大小写不敏感,与 is_indexable 同一决定");
        assert_eq!(ext_of("a/b.vtt"), "vtt");
        assert_eq!(ext_of("a/b.TXT"), "txt");
    }

    /// 快路径的前提。`a.md` 与 `a.note.md` 内容可以一字不差,但走的是不同
    /// 的分块器 —— 块必须重算,不能只换路径。
    #[test]
    fn chunker_class_separates_the_four_dispatch_targets() {
        assert_eq!(chunker_class("a.note.md"), ChunkerClass::Outline);
        assert_eq!(chunker_class("a.md"), ChunkerClass::Prose);
        assert_eq!(chunker_class("a.srt"), ChunkerClass::Transcript);
        assert_eq!(chunker_class("a.VTT"), ChunkerClass::Transcript);
        assert_eq!(chunker_class("a.txt"), ChunkerClass::Plain);
        assert_ne!(chunker_class("a.md"), chunker_class("a.note.md"));
    }
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test --manifest-path searchidx/Cargo.toml ext_of`

- [ ] **Step 3: 实现**

把 `index_into` 里那段计算 `ext` 的 if-chain**整体移进** `ext_of`,`index_into` 改为调用它 —— 不要复制一份,那正是这个任务要消除的东西。`chunker_class` 的判定顺序必须与 `chunk::parse_file` 的分派**逐条一致**(`.note.md` 优先于 `.md`),并在两处互相加一句指路注释。

- [ ] **Step 4: 跑全套测试通过 + 提交**

```bash
git add searchidx/src/scan.rs
git commit -m "refactor(searchidx): 抽出 ext_of 与 chunker_class,为重命名快路径备用"
```

---

## Task 2: 配对纯函数

**Files:** Create `searchidx/src/rename.rs`;Modify `searchidx/src/lib.rs`

**Interfaces:** Produces
- `pub(crate) struct Orphan { pub path: String, pub size: i64, pub mtime: i64, pub content_hash: String }`
- `pub(crate) struct NewPath { pub rel: String, pub size: i64, pub mtime: i64 }`
- `pub(crate) fn pair_candidates(news: &[NewPath], orphans: &[Orphan]) -> Vec<(usize, usize)>`
  —— 返回 `(news 下标, orphans 下标)`;**只做无 I/O 预筛**,hash 确认由调用方做。

- [ ] **Step 1: 写失败的测试**

```rust
    fn n(rel: &str, size: i64, mtime: i64) -> NewPath {
        NewPath { rel: rel.into(), size, mtime }
    }
    fn o(path: &str, size: i64, mtime: i64, h: &str) -> Orphan {
        Orphan { path: path.into(), size, mtime, content_hash: h.into() }
    }

    #[test]
    fn size_and_mtime_both_must_match() {
        let news = [n("new/a.md", 10, 100)];
        assert_eq!(pair_candidates(&news, &[o("old/a.md", 10, 100, "h")]), vec![(0, 0)]);
        assert!(pair_candidates(&news, &[o("old/a.md", 11, 100, "h")]).is_empty());
        assert!(pair_candidates(&news, &[o("old/a.md", 10, 101, "h")]).is_empty());
    }

    /// 分块器类别不同就不能走快路径 —— 块必须重算(spec §4.2)。
    #[test]
    fn a_chunker_class_change_is_not_a_rename() {
        let news = [n("a.note.md", 10, 100)];
        assert!(pair_candidates(&news, &[o("a.md", 10, 100, "h")]).is_empty());
    }

    /// 一个孤儿只能被认领一次。两个内容相同的新文件里,第二个走全量路径。
    #[test]
    fn one_orphan_is_claimed_at_most_once() {
        let news = [n("x.md", 10, 100), n("y.md", 10, 100)];
        let pairs = pair_candidates(&news, &[o("old.md", 10, 100, "h")]);
        assert_eq!(pairs.len(), 1);
    }

    /// 目标路径被占用的情形不在这里挡 —— 这个函数只看两个集合,不看库。
    /// 互换名字时两条配对都会产出,由调用方按 UNIQUE 约束退回(spec §4.2)。
    #[test]
    fn a_swap_produces_both_pairs_and_is_the_callers_problem() {
        let news = [n("a.md", 10, 100), n("b.md", 20, 200)];
        let orphans = [o("b.md", 10, 100, "h1"), o("a.md", 20, 200, "h2")];
        assert_eq!(pair_candidates(&news, &orphans).len(), 2);
    }

    #[test]
    fn empty_inputs_yield_no_pairs() {
        assert!(pair_candidates(&[], &[o("a.md", 1, 1, "h")]).is_empty());
        assert!(pair_candidates(&[n("a.md", 1, 1)], &[]).is_empty());
    }
```

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 实现**

按 `(size, mtime)` 建索引,对每个 `NewPath` 找第一个未被认领、且 `chunker_class` 相同的孤儿。用一个 `HashSet<usize>` 记已认领的孤儿下标。**不读文件、不碰数据库** —— 这个函数的全部价值就是可以脱离两者单测。

- [ ] **Step 4: 跑测试通过**

- [ ] **Step 5: mutation check**

把「分块器类别相同」这个条件去掉,确认 `a_chunker_class_change_is_not_a_rename` 变红而其余保持绿。把「已认领」的记录去掉,确认 `one_orphan_is_claimed_at_most_once` 变红。两次输出贴进报告。

- [ ] **Step 6: 提交**

```bash
git add searchidx/src/rename.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): 重命名配对纯函数(size/mtime 预筛 + 分块器类别守卫)"
```

---

## Task 3: `store::rename_file`

**Files:** Modify `searchidx/src/store.rs`

**Interfaces:** Produces
- `pub fn rename_file(tx: &Transaction, old_path: &str, new_path: &str, ext: &str, mtime: i64, size: i64, meta: &crate::block::FileMeta) -> rusqlite::Result<bool>`
  —— 返回 `false` 表示目标路径已被占用(调用方退回全量重建),`true` 表示已更新。

- [ ] **Step 1: 写失败的测试**

```rust
    /// `FileMeta` 只 derive 了 `Debug, Clone` —— **没有 `Default`**。
    /// 要自己写全七个字段,不要用 `..Default::default()`(不会编译)。
    fn meta(title: &str) -> crate::block::FileMeta {
        crate::block::FileMeta {
            title: Some(title.into()),
            concept_type: None,
            tags: Vec::new(),
            doc_date: None,
            date_inferred: false,
            human_verified: false,
            origin: crate::Origin::Unlabeled,
        }
    }

    /// 快路径的定义:块一行不动。改名后 blocks 的 id 必须原样保留 ——
    /// 这是「没有重算」的可验证证据,比行数相等强得多。
    #[test]
    fn rename_keeps_every_block_row_untouched() {
        let (_d, p) = tmp();
        let mut conn = open(&p, "/v", "").unwrap();
        write(&mut conn, "old.md", "# T\n\nalpha\n");
        let before: Vec<i64> = conn
            .prepare("SELECT b.id FROM blocks b JOIN files f ON f.id=b.file_id WHERE f.path='old.md' ORDER BY b.id")
            .unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert!(!before.is_empty());

        let tx = conn.transaction().unwrap();
        assert!(rename_file(&tx, "old.md", "new.md", "md", 7, 9, &meta("T")).unwrap());
        tx.commit().unwrap();

        let after: Vec<i64> = conn
            .prepare("SELECT b.id FROM blocks b JOIN files f ON f.id=b.file_id WHERE f.path='new.md' ORDER BY b.id")
            .unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(before, after, "块必须原样保留,连 id 都不变");
        let old_left: i64 = conn
            .query_row("SELECT count(*) FROM files WHERE path='old.md'", [], |r| r.get(0)).unwrap();
        assert_eq!(old_left, 0, "旧路径不得残留");
    }

    /// 目标被占用时必须报告失败而不是让事务炸掉 —— 调用方要靠这个返回值
    /// 决定退回全量重建(spec §4.2 的名字互换)。
    #[test]
    fn rename_reports_false_when_the_target_path_is_taken() {
        let (_d, p) = tmp();
        let mut conn = open(&p, "/v", "").unwrap();
        write(&mut conn, "a.md", "alpha\n");
        write(&mut conn, "b.md", "beta\n");
        let tx = conn.transaction().unwrap();
        assert!(!rename_file(&tx, "a.md", "b.md", "md", 1, 1, &meta("x")).unwrap());
        tx.commit().unwrap();
        // 两行都还在,谁也没被破坏
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }
```

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 实现**

先查 `SELECT count(*) FROM files WHERE path=?new`,非零直接 `Ok(false)`(**先查后写**,不要靠捕获 UNIQUE 错误 —— 那会污染事务状态)。然后:

```sql
UPDATE files SET path=?, ext=?, mtime=?, size=?,
                 title=?, doc_date=?, date_inferred=?, origin=?
WHERE path=?
```

**只更新路径派生的列。** `content_hash`、`concept_type`、`tags_json`、`human_verified` 是内容派生的,内容没变就不动 —— 在 SQL 旁边写一句注释说明这个划分,并指向 spec §4.1 的表。

- [ ] **Step 4: 跑测试通过 + 提交**

```bash
git add searchidx/src/store.rs
git commit -m "feat(searchidx): rename_file —— 只改路径派生列,块原样保留"
```

---

## Task 4: 接入 sweep

**Files:** Modify `searchidx/src/scan.rs`

**Interfaces:**
- Consumes `rename::pair_candidates`、`store::rename_file`、`ext_of`、`chunker_class`
- Produces `ScanStats.files_renamed: usize`

- [ ] **Step 1: 写失败的测试**

```rust
    /// 核心契约:改名后 `path#Lnnn` 仍然指对。快路径不重算块,所以这条
    /// 必须端到端验 —— 改名前搜到某行,改名后同一查询命中同一段文字、
    /// 行号一致、路径是新的。
    #[test]
    fn a_renamed_file_keeps_its_line_anchors() {
        let v = vault(&[("old/talk.md", "# H\n\nalpha line\n\nbeta line\n")]);
        let mut conn = conn_for(v.path());
        let opts = ScanOptions::default();
        build_full(&mut conn, v.path(), &opts, None).unwrap();
        let before = crate::query::search(&conn, &crate::query::parse("beta"), 10, "2026-08-13").unwrap().0;
        assert_eq!(before.len(), 1);
        let (old_path, old_line) = (before[0].path.clone(), before[0].line);

        fs::create_dir_all(v.path().join("new")).unwrap();
        fs::rename(v.path().join("old/talk.md"), v.path().join("new/talk.md")).unwrap();
        let stats = sweep(&mut conn, v.path(), &opts, None, None).unwrap();
        assert_eq!(stats.files_renamed, 1, "必须走快路径,而不是删除+重建");
        assert_eq!(stats.files_indexed, 0, "块不该被重算");
        assert_eq!(stats.files_removed, 0, "重命名不是删除");

        let after = crate::query::search(&conn, &crate::query::parse("beta"), 10, "2026-08-13").unwrap().0;
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].line, old_line, "行号必须一致");
        assert_ne!(after[0].path, old_path);
        assert_eq!(after[0].path, "new/talk.md");
    }

    /// 元数据确实重算了,不只是换了路径:移进原始资料模式命中的目录
    /// 必须改变分层(规则 5′)。
    #[test]
    fn moving_into_a_source_glob_changes_the_tier() {
        let v = vault(&[("notes/a.md", "plain body\n")]);
        let mut conn = conn_for(v.path());
        let mut opts = ScanOptions::default();
        opts.source_globs = crate::globs::parse(&["ebook/**".to_string()]);
        build_full(&mut conn, v.path(), &opts, None).unwrap();
        let tier = |c: &rusqlite::Connection, path: &str| -> String {
            c.query_row("SELECT origin FROM files WHERE path=?1", [path], |r| r.get(0)).unwrap()
        };
        assert_eq!(tier(&conn, "notes/a.md"), "unlabeled");

        fs::create_dir_all(v.path().join("ebook")).unwrap();
        fs::rename(v.path().join("notes/a.md"), v.path().join("ebook/a.md")).unwrap();
        let stats = sweep(&mut conn, v.path(), &opts, None, None).unwrap();
        assert_eq!(stats.files_renamed, 1);
        assert_eq!(tier(&conn, "ebook/a.md"), "source", "元数据必须重算,不能只换路径");
    }

    /// size 与 mtime 都相同但内容不同 —— hash 确认必须拦住。
    #[test]
    fn identical_stat_but_different_content_is_not_a_rename() {
        let v = vault(&[("old.md", "aaaa\n")]);
        let mut conn = conn_for(v.path());
        let opts = ScanOptions::default();
        build_full(&mut conn, v.path(), &opts, None).unwrap();
        let mt = fs::metadata(v.path().join("old.md")).unwrap().modified().unwrap();
        fs::remove_file(v.path().join("old.md")).unwrap();
        fs::write(v.path().join("new.md"), "bbbb\n").unwrap();
        filetime::set_file_mtime(v.path().join("new.md"), filetime::FileTime::from_system_time(mt)).unwrap();

        let stats = sweep(&mut conn, v.path(), &opts, None, None).unwrap();
        assert_eq!(stats.files_renamed, 0, "内容不同,不得配对");
        assert_eq!(stats.files_indexed, 1);
        assert_eq!(stats.files_removed, 1);
    }

    /// 分块器类别变化退回全量重建,且块按新分块器重算。
    #[test]
    fn changing_the_chunker_class_falls_back_to_a_full_reindex() {
        let v = vault(&[("a.md", "# H\n\nbody\n")]);
        let mut conn = conn_for(v.path());
        let opts = ScanOptions::default();
        build_full(&mut conn, v.path(), &opts, None).unwrap();
        fs::rename(v.path().join("a.md"), v.path().join("a.note.md")).unwrap();
        let stats = sweep(&mut conn, v.path(), &opts, None, None).unwrap();
        assert_eq!(stats.files_renamed, 0);
        assert_eq!(stats.files_indexed, 1, "必须重建");
        let ext: String = conn.query_row("SELECT ext FROM files WHERE path='a.note.md'", [], |r| r.get(0)).unwrap();
        assert_eq!(ext, "note.md");
    }
```

`filetime` 已是 dev-dependency 则直接用;若不是,在 `searchidx/Cargo.toml` 的 `[dev-dependencies]` 加 `filetime = "0.2"` —— **只加到 dev**,运行时依赖预算不变。

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 实现**

在 `sweep_with_budget` 的走查之后、候选循环之前:

1. 由 `known` 与候选路径集算出孤儿集(`known` 里、不在候选路径集里的)。**这两者在循环开始前都已就绪**(`known` 来自 `all_file_rows`,`candidates` 来自 `walk`)。
2. 新路径集 = 候选里 `known` 查不到的。
3. 调 `rename::pair_candidates` 得到配对。
4. 对每个配对:读新路径的文件、算 `content_hash`,与孤儿的 hash 比对;不等则丢弃该配对。
5. 相等则 `chunk::parse_file` 取 `parsed.meta`(**丢弃 blocks/links**),调 `store::rename_file`;返回 `false`(目标被占)则丢弃该配对,让它走全量路径。
6. 成功的配对:`stats.files_renamed += 1`,把新路径记入一个 `renamed: HashSet<String>`,并把**旧路径**记入 `renamed_from: HashSet<String>`。

然后:

- 候选循环里,`renamed` 中的路径**跳过**(已经处理完了),不要再走 stat/hash 判断。
- 删除轮的 `to_remove` 过滤条件增加 `&& !renamed_from.contains(p)` —— **这是 spec §5 明确要求的显式摘除**,不得依赖 `DELETE ... WHERE path=旧路径` 恰好匹配不到。旁边写一句注释说明为什么不能依赖那个巧合。

`ScanStats` 增加 `files_renamed: usize`。

- [ ] **Step 4: 跑全套测试通过**

- [ ] **Step 5: mutation check(三次,输出全贴进报告)**

1. 把 hash 确认短路成恒真 → `identical_stat_but_different_content_is_not_a_rename` 必须变红。
2. 把删除轮的 `!renamed_from.contains(p)` 去掉 → `a_renamed_file_keeps_its_line_anchors` 里 `files_removed == 0` 那条断言必须变红(**这正是「统计说谎」的证据**)。
3. 把 `parsed.meta` 换成沿用旧行的元数据(不重算)→ `moving_into_a_source_glob_changes_the_tier` 必须变红。

- [ ] **Step 6: 提交**

```bash
git add searchidx/src/scan.rs searchidx/Cargo.toml
git commit -m "feat(searchidx): sweep 认出重命名,跳过块重算"
```

---

## Task 5: 接入 watcher 批次

**Files:** Modify `src-tauri/src/search/watch.rs`;Modify `searchidx/src/lib.rs`(暴露批次入口)

**Interfaces:** Produces
- `SearchIndex::apply_batch(&mut self, rels: &[String], opts: &ScanOptions) -> rusqlite::Result<BatchOutcome>`
  ,`pub struct BatchOutcome { pub renamed: usize, pub reindexed: usize, pub removed: usize }`

- [ ] **Step 1: 写失败的测试**

```rust
    /// 一次重命名的删除与新增落在同一批次里 → 走配对,不重算块。
    #[test]
    fn a_batch_containing_both_ends_of_a_rename_takes_the_fast_path() {
        let v = vault(&[("old.md", "# H\n\nalpha\n")]);
        let (_d, p) = tmp();
        // 注意参数顺序:open_at(vault_root, db_path, globs_stamp)
        let mut idx = SearchIndex::open_at(v.path(), &p, "").unwrap();
        let opts = ScanOptions::default();
        idx.ensure_built(&opts).unwrap();
        fs::rename(v.path().join("old.md"), v.path().join("new.md")).unwrap();
        let out = idx.apply_batch(&["old.md".into(), "new.md".into()], &opts).unwrap();
        assert_eq!(out.renamed, 1);
        assert_eq!(out.reindexed, 0);
        assert_eq!(out.removed, 0);
    }

    /// 只看得到一半(跨 debounce 窗口)→ 退回逐个处理,即今天的行为。
    /// 这是降级,不是错误(spec §6)。
    #[test]
    fn a_batch_with_only_the_new_half_falls_back_to_a_reindex() {
        let v = vault(&[("old.md", "# H\n\nalpha\n")]);
        let (_d, p) = tmp();
        // 注意参数顺序:open_at(vault_root, db_path, globs_stamp)
        let mut idx = SearchIndex::open_at(v.path(), &p, "").unwrap();
        let opts = ScanOptions::default();
        idx.ensure_built(&opts).unwrap();
        fs::rename(v.path().join("old.md"), v.path().join("new.md")).unwrap();
        let out = idx.apply_batch(&["new.md".into()], &opts).unwrap();
        assert_eq!(out.renamed, 0);
        assert_eq!(out.reindexed, 1);
    }
```

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 实现**

`apply_batch` 把批次里的路径分成两类:磁盘上**已不存在且库里有行**的 → 孤儿;磁盘上**存在且库里没有行**的 → 新路径。其余的照旧逐个走 `index_one` 的等价逻辑。然后复用 Task 4 的同一套配对 + hash 确认 + `rename_file`。

**配对与确认的逻辑必须与 sweep 共用**(`rename::pair_candidates` 加上同一段确认代码抽成 `pub(crate) fn confirm_and_apply`),而不是在这里重写一遍 —— 两套实现正是这份 spec 反复要避免的形状。

`watch.rs` 的 `Batch::Files(paths)` 分支改为调 `apply_batch`,日志行按 `BatchOutcome` 三个计数分别播报。

- [ ] **Step 4: 跑两个套件通过 + 提交**

```bash
git add searchidx/src/lib.rs searchidx/src/scan.rs src-tauri/src/search/watch.rs
git commit -m "feat(search): watcher 批次也认出重命名"
```

---

## 人工 GUI 验收清单

1. 应用开着,在 Finder 里把一个装几十个笔记的目录改名 → 搜索结果里路径立刻更新,内容照旧搜得到
2. 同上,但目录里装的是转写稿(大文件)→ 明显比改名前快,进度条不再长时间停在 Indexing
3. 关掉应用,改名一个目录,再打开 → 打开时的 sweep 认出重命名
4. 把一个笔记从普通目录移进原始资料模式命中的目录 → 设置页分层统计里它从「未标注」挪到「原始资料」
5. 把 `a.md` 改名成 `a.note.md` → 大纲 tab 能正常打开(块确实按大纲分块器重算了)
6. 搜索命中后点进去 → 跳到正确的行

## 已知取舍

- **预筛依赖 mtime 被保留。** 若某工具用「读旧 + 写新 + 删旧」实现移动,mtime 会变 → 退回全量重建。正确,只是没省到。
- **跨 debounce 窗口的重命名配不上** → 同上。
- **省下的是分词与 FTS 写入,不是读文件。** 收益随文件大小上升,对一堆小 `.md` 不明显 —— 而痛点(整目录转写稿)恰好收益最大。
