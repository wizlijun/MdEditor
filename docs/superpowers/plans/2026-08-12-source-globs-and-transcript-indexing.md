# 原始资料模式 + 转写收录 + 权重设置 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把「原始资料」从「缺 frontmatter 就算」改成「你指定的通配模式」,同一份模式同时决定收录范围(让 `.srt`/`.vtt`/`.txt` 进索引)与分层归属;缺 frontmatter 改判新的第 4 层「未标注」;四层权重进设置。

**Architecture:** `searchidx` 新增一个纯粹的 glob 匹配器,它同时喂给 `is_indexable`(收录)与 `origin::derive`(分层)。`Origin` 加第 4 个变体。转写与纯文本各有一个分块器,接进 `chunk::parse_file` 那个唯一分派点。权重是**查询时**参数,与存储的 `origin` 正交 —— 改权重不重建。

**Tech Stack:** Rust(`searchidx`、`src-tauri`)· Svelte 5 runes · SQLite schema 迁移(bump + 自动重建)

**Spec:** `docs/superpowers/specs/2026-08-12-source-globs-and-transcript-indexing-design.md`

**前置:** 分级功能(`2026-08-11-md-origin-tiering.md`)已随 v6.812.1 发布并合入 main。

## Global Constraints

- **规则表顺序是规范性的**(spec §3),首次命中即止。改顺序就是改行为。
- **`origin` 永不回写文件。** 它是索引侧的推导物。
- **`path#Lnnn` 引用契约不可破**:剔除时间码只影响进 FTS 的文本,不影响 `line_start`/`line_end`。一个自信但指错位置的引用比没有引用更糟。
- **`SCHEMA_VERSION` 从 2 bump 到 3**,所有人索引自动全量重建,**不写迁移脚本**。
- **模式变更触发原地重建(`DELETE FROM` + 重新盖章),绝不 unlink** —— GUI 可能正握着活的 WAL 连接。
- **权重变更不触发重建。**
- **`Weights::default()` 必须等于 `1.25 / 1.0 / 0.9 / 0.3`**,并有测试钉住。
- **四档权重逐档单独 mutation 验证。** 前置项目出现过「两个乘数同向、任一失效测试仍通过」的假阴性。
- **回归集必须显式使用 `Weights::default()`**;期望值有变更须逐条人工确认,**不允许照着新输出批量刷新**。
- **`ScanOptions` 只有一个构造点** `src-tauri/src/search/options.rs::for_vault`,由 `search_scan_options_contract.rs` 挡着。权重照抄这套。
- **`notemd search` 默认输出逐字保持 `path:line:text`。**
- 测试命令:`cargo test --manifest-path searchidx/Cargo.toml`、`cargo test --manifest-path src-tauri/Cargo.toml`、`pnpm check`、`pnpm test`。`plugin_runtime_integration.rs` 在并行下是**既有** flake(`$initialize: timeout`),用 `--test-threads=1`,不要追。
- **共享 worktree**:只精确 `git add`,绝不 `git add -A`。

---

## File Structure

| 文件 | 改动 |
| --- | --- |
| `searchidx/src/globs.rs`(新) | `SourceGlobs`:解析、匹配、规范化 |
| `searchidx/src/origin.rs` | `Origin::Unlabeled`;规则 5′/6′ |
| `searchidx/src/scan.rs` | `ScanOptions.source_globs` 取代 `sync_dir`;`is_indexable` 扩展名白名单 |
| `searchidx/src/transcript.rs`(新) | `.srt`/`.vtt` 分块 |
| `searchidx/src/plain.rs`(新) | `.txt` 分块 |
| `searchidx/src/chunk.rs` | 按扩展名分派到新分块器 |
| `searchidx/src/store.rs` | `SCHEMA_VERSION` 3;globs 盖章 |
| `searchidx/src/query.rs` | `Weights`;`score_of` 四档;`origin:unlabeled` |
| `src-tauri/src/search/options.rs` | 构造 `source_globs`;新增权重构造点 |
| `src-tauri/src/search/mod.rs` | 命中数命令;stats 四层 |
| `src-tauri/src/sotvault/vault_settings.rs` | `searchSourceGlobs`、`searchWeights` |
| `src-tauri/src/agents_sync/logic.rs` + `templates/AGENTS.md` | 广告 `origin:unlabeled` |
| `src/lib/search/glob-suggest.ts`(新) | 纯函数:样例路径 → 候选模式 |
| `src/lib/search/grouping.ts` | 第 4 组 |
| `src/components/SettingsDialog.svelte` | 模式编辑器、权重、四层统计 |

---

## Task 1: glob 匹配器

**Files:** Create `searchidx/src/globs.rs`;Modify `searchidx/src/lib.rs`

**Interfaces:** Produces
- `pub struct SourceGlobs`(`Default` = 空,匹配零个)
- `pub fn parse(patterns: &[String]) -> SourceGlobs`
- `SourceGlobs::matches(&self, rel: &str) -> bool`
- `SourceGlobs::stamp(&self) -> String` —— 规范化形式,用于 `meta` 盖章
- `SourceGlobs::is_empty(&self) -> bool`

- [ ] **Step 1: 写失败的测试**

```rust
    fn g(p: &[&str]) -> SourceGlobs {
        parse(&p.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test] fn a_double_star_crosses_directory_levels() {
        let s = g(&["ebook/**"]);
        assert!(s.matches("ebook/a.md"));
        assert!(s.matches("ebook/三体/第一部/x.srt"));
        assert!(!s.matches("other/a.md"));
    }

    /// 前缀相似不得误命中 —— `ebook2/` 不是 `ebook/` 的子目录。
    /// 这是既有 sync_dir 匹配器踩过的同一个坑,那里靠显式补 `/` 边界解决。
    #[test] fn a_lookalike_prefix_does_not_match() {
        let s = g(&["ebook/**"]);
        assert!(!s.matches("ebook2/a.md"));
        assert!(!s.matches("my-ebook/a.md"));
        assert!(!s.matches("x/ebook/a.md"));
    }

    #[test] fn a_single_star_does_not_cross_a_level() {
        let s = g(&["clips/*.txt"]);
        assert!(s.matches("clips/a.txt"));
        assert!(!s.matches("clips/sub/a.txt"));
    }

    #[test] fn a_bare_double_star_matches_everything() {
        assert!(g(&["**/*.srt"]).matches("any/where/deep/a.srt"));
        assert!(!g(&["**/*.srt"]).matches("any/where/a.txt"));
    }

    /// 空列表匹配零个,不是匹配一切 —— 反过来会让首次升级时全库变原始资料。
    #[test] fn an_empty_set_matches_nothing() {
        assert!(!g(&[]).matches("a.md"));
        assert!(g(&[]).is_empty());
    }

    /// 无法解析的模式被丢弃而不是让整份列表失效(容忍义务)。
    #[test] fn an_unparseable_pattern_is_dropped_not_fatal() {
        let s = g(&["", "   ", "ebook/**"]);
        assert!(s.matches("ebook/a.md"), "合法的那条仍须生效");
    }

    /// 规范化:顺序、空白、首尾斜杠不同但语义相同的两份列表必须产出同一个
    /// 串。否则每次保存设置都会触发一次无谓的全库重建。
    #[test] fn the_stamp_is_order_and_whitespace_insensitive() {
        assert_eq!(g(&["b/**", "a/**"]).stamp(), g(&["  a/** ", "/b/**/"]).stamp());
        assert_ne!(g(&["a/**"]).stamp(), g(&["a/*"]).stamp());
    }
```

- [ ] **Step 2: 跑测试确认失败** — `cargo test --manifest-path searchidx/Cargo.toml globs`

- [ ] **Step 3: 实现**

不要引入 glob crate —— 语法故意做窄(spec §4.1),自己实现比裁剪一个通用库更可控。把模式与路径都按 `/` 切成段,逐段比对,`**` 消耗任意多段。

```rust
fn matches_segments(pat: &[String], path: &[&str]) -> bool {
    match pat.first().map(String::as_str) {
        None => path.is_empty(),
        Some("**") => (0..=path.len()).any(|i| matches_segments(&pat[1..], &path[i..])),
        Some(p) => match path.first() {
            Some(seg) if segment_matches(p, seg) => matches_segments(&pat[1..], &path[1..]),
            _ => false,
        },
    }
}
```

`segment_matches` 处理单段内的 `*`(不跨 `/`)。规范化 = 每条 trim + 去首尾 `/` + 丢弃空条 + 排序 + 用 `\n` 连接。

- [ ] **Step 4: 跑测试通过**

- [ ] **Step 5: mutation check**

把 `Some("**")` 那条改成只消耗一段,确认 `a_double_star_crosses_directory_levels` 变红而 `a_single_star_does_not_cross_a_level` 保持绿。把 `stamp` 里的排序去掉,确认 `the_stamp_is_order_and_whitespace_insensitive` 变红。两次输出都贴进报告。

- [ ] **Step 6: 提交**

```bash
git add searchidx/src/globs.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): 原始资料通配模式匹配器"
```

---

## Task 2: `Origin::Unlabeled` 与规则 5′/6′

**Files:** Modify `searchidx/src/origin.rs`

**Interfaces:**
- Consumes `SourceGlobs`(Task 1)
- Produces `Origin::Unlabeled`(`as_str()` → `"unlabeled"`);`derive` 签名改为
  `pub fn derive(rel_path: &str, fm: Option<&Frontmatter>, globs: &SourceGlobs) -> Origin`

- [ ] **Step 1: 写失败的测试**

```rust
    fn globs(p: &[&str]) -> crate::globs::SourceGlobs {
        crate::globs::parse(&p.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test] fn rule5_a_matched_path_is_source() {
        assert_eq!(derive("ebook/a.md", Some(&fm("title: t")), &globs(&["ebook/**"])), Origin::Source);
    }

    /// 这是本次的核心修正:缺 frontmatter 不再等同于原始资料。
    #[test] fn rule6_no_frontmatter_and_no_match_is_unlabeled() {
        assert_eq!(derive("notes/a.md", None, &globs(&["ebook/**"])), Origin::Unlabeled);
    }

    /// 规则 5′ 压过规则 6′ —— 指定目录里没有 frontmatter 的文件是原始资料,
    /// 不是未标注。
    #[test] fn a_matched_path_without_frontmatter_is_source_not_unlabeled() {
        assert_eq!(derive("ebook/a.md", None, &globs(&["ebook/**"])), Origin::Source);
    }

    /// 规则 2/4 仍压过规则 5′ —— 指定目录里的 AI 摘要仍是 AI 产出。
    /// (取代既有的 a_registered_type_beats_the_mirror_dir。)
    #[test] fn a_registered_type_beats_a_source_glob() {
        assert_eq!(
            derive("ebook/s.md", Some(&fm("type: Book Summary")), &globs(&["ebook/**"])),
            Origin::Derived
        );
    }
    #[test] fn a_generated_stamp_beats_a_source_glob() {
        assert_eq!(
            derive("ebook/s.md", Some(&fm("generated:\n  by: claude/1")), &globs(&["ebook/**"])),
            Origin::Derived
        );
    }

    #[test] fn round_trips_through_str() {
        for o in [Origin::Human, Origin::Derived, Origin::Source, Origin::Unlabeled] {
            assert_eq!(Origin::from_str(o.as_str()), Some(o));
        }
    }
```

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 实现**

`Origin` 加 `Unlabeled`;规则 5 的 `sync_dir` 前缀判断整段换成 `globs.matches(rel_path)`;规则 6 的返回值从 `Source` 改成 `Unlabeled`。

**必须同时改注释。** 规则 6 现有那段长注释论证的是「刻意误判成 source 而不是 human」,现在结论变了 —— 保留「不能默认判 human」的论证(那部分仍然成立),把「所以判 source」改成「所以判 unlabeled,并解释这一层是老实说明无人声明,不是一个关于内容的断言」。留着旧注释比没有注释更糟。

**保留** `some_default_frontmatter_is_not_the_same_as_none` 及 `derive` 文档注释里那段陷阱说明 —— `chunk::parse_file` 的 `unwrap_or_default()` 仍在,规则 6′ 仍靠 `None` 触发。

- [ ] **Step 4: 跑测试通过**

- [ ] **Step 5: 顺序 mutation check**

把规则 5′ 挪到规则 4 之前,确认 `a_registered_type_beats_a_source_glob` 与 `a_generated_stamp_beats_a_source_glob` 变红而单条规则测试保持绿。把规则 6′ 挪到 5′ 之前,确认 `a_matched_path_without_frontmatter_is_source_not_unlabeled` 变红。输出贴进报告。

- [ ] **Step 6: 提交**

```bash
git add searchidx/src/origin.rs
git commit -m "feat(searchidx): 未标注层 + 规则 5′/6′ 改由模式判定"
```

---

## Task 3: `ScanOptions.source_globs` 与收录白名单

**Files:** Modify `searchidx/src/scan.rs`、`searchidx/src/chunk.rs`

**Interfaces:**
- `ScanOptions.sync_dir: String` **删除**,替换为 `pub source_globs: SourceGlobs`
- `chunk::parse_file(rel_path, raw, mtime_secs, globs: &SourceGlobs)`

- [ ] **Step 1: 写失败的测试**

```rust
    #[test]
    fn md_is_indexed_regardless_of_the_globs() {
        let opts = ScanOptions::default();
        assert!(is_indexable("anywhere/a.md", &opts));
        assert!(is_indexable("b.note.md", &opts));
    }

    #[test]
    fn transcripts_are_indexed_only_inside_a_source_glob() {
        let mut opts = ScanOptions::default();
        opts.source_globs = crate::globs::parse(&["media/**".to_string()]);
        assert!(is_indexable("media/talk.srt", &opts));
        assert!(is_indexable("media/talk.vtt", &opts));
        assert!(is_indexable("media/notes.txt", &opts));
        assert!(!is_indexable("elsewhere/talk.srt", &opts), "模式外的转写不得进索引");
    }

    #[test]
    fn an_unlisted_extension_is_never_indexed_even_inside_a_glob() {
        let mut opts = ScanOptions::default();
        opts.source_globs = crate::globs::parse(&["media/**".to_string()]);
        assert!(!is_indexable("media/a.pdf", &opts));
        assert!(!is_indexable("media/a.mp4", &opts));
    }

    /// 排除优先于收录。
    #[test]
    fn exclude_dirs_win_over_a_source_glob() {
        let mut opts = ScanOptions::default();
        opts.source_globs = crate::globs::parse(&["media/**".to_string()]);
        opts.exclude_dirs = vec!["media/raw".to_string()];
        assert!(!is_indexable("media/raw/a.srt", &opts));
    }
```

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 实现**

`is_indexable` 的扩展名闸门改为:`.md` 直接通过;`.srt`/`.vtt`/`.txt` 需 `opts.source_globs.matches(rel)`;其余一律否。点段检查与 `exclude_dirs` 检查位置不变(排除仍在最后,保证优先)。

`ScanOptions::default()` 的 `source_globs` 为空 `SourceGlobs`(匹配零个)。

**跨计划依赖:** `ScanOptions` 的构造点只有 `src-tauri/src/search/options.rs::for_vault` 一个,由 `search_scan_options_contract.rs` 挡着。本任务只改结构体与 `searchidx` 内部;`for_vault` 在 Task 8 改。为了让 crate 先编译通过,`for_vault` 这里先传 `SourceGlobs::default()` 并留一行 `// Task 8 接真实设置`。

- [ ] **Step 4: 全量测试 + 提交**

```bash
git add searchidx/src/scan.rs searchidx/src/chunk.rs src-tauri/src/search/options.rs
git commit -m "feat(searchidx): 收录范围由原始资料模式决定"
```

---

## Task 4: `.srt` / `.vtt` 分块器

**Files:** Create `searchidx/src/transcript.rs`;Modify `searchidx/src/chunk.rs`、`searchidx/src/lib.rs`

**Interfaces:** Produces `pub fn chunk(body: &str, body_start_line: u32) -> Vec<Block>`

- [ ] **Step 1: 写失败的测试**

```rust
    const SRT: &str = "\
1
00:00:01,000 --> 00:00:03,000
今天讲一个关于检索的问题

2
00:00:03,000 --> 00:00:06,000
它的难点不在存储

3
00:00:06,000 --> 00:00:09,000
而在判断什么值得留下
";

    /// 引用契约:时间码被剔出索引文本,但行号必须仍指向原文件的真实行。
    /// 第一条字幕的文本在第 3 行。
    #[test]
    fn line_numbers_point_at_the_original_text_lines() {
        let b = chunk(SRT, 1);
        assert_eq!(b[0].line_start, 3, "必须是文本行,不是序号行也不是时间码行");
    }

    #[test]
    fn timecodes_and_cue_numbers_are_not_indexed() {
        let joined = chunk(SRT, 1).iter().map(|b| b.text.clone()).collect::<String>();
        assert!(!joined.contains("00:00:01"), "时间码会污染分词");
        assert!(!joined.contains("-->"));
    }

    /// 单条字幕只有五到十个字,逐条成块会让块数暴涨,而且跨条的句子永远
    /// 匹配不上 —— 这是收录转写的主要动机之一。
    #[test]
    fn consecutive_cues_are_merged_so_a_sentence_can_span_them() {
        let b = chunk(SRT, 1);
        assert_eq!(b.len(), 1, "三条短字幕应合成一块,实际 {:?}", b);
        assert!(b[0].text.contains("难点不在存储"));
        assert!(b[0].text.contains("什么值得留下"));
        assert_eq!(b[0].line_end, 11, "块尾须是最后一条字幕的文本行");
    }

    /// WEBVTT 头与可选的 cue 标识行都不进索引。
    #[test]
    fn a_vtt_header_and_cue_identifier_are_skipped() {
        let vtt = "WEBVTT\n\nintro\n00:00:01.000 --> 00:00:03.000\n开场白\n";
        let b = chunk(vtt, 1);
        assert_eq!(b.len(), 1);
        assert!(!b[0].text.contains("WEBVTT"));
        assert!(!b[0].text.contains("intro"));
        assert_eq!(b[0].line_start, 5);
    }

    /// 容忍义务:格式异常的行按普通文本收,不报错、不 panic。
    #[test]
    fn a_malformed_cue_is_kept_as_text_rather_than_dropped() {
        let bad = "1\n00:00:01,000 -> 00:00:03,000\n内容还在\n";
        let b = chunk(bad, 1);
        assert!(b.iter().any(|x| x.text.contains("内容还在")));
    }
```

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 实现**

逐行扫描,维护「当前行在原文件中的行号」。判定并跳过三类行:纯数字的序号行、含 `-->` 的时间码行、`WEBVTT` 头。`.vtt` 的 cue 标识行 = 紧跟空行之后、且**下一行是时间码行**的非时间码行 —— 必须靠下一行判定,否则会把正文首行误当标识。

文本行累积进当前块。**新增一个常量 `TARGET_CHARS: usize = 200`**(按 `chars()` 计,不是字节 —— 语料以中文为主)。

> **计划勘误:** spec §4.2 与本计划早先版本写的是「复用散文分块器已有的那个常量」。**那个常量不存在** —— `prose.rs` 按 markdown 结构切(段落、标题、章节),不做任何按字数合并,其唯一的数字常量是面包屑的 40 字截断。所以这里必须自己定一个,并把理由写进常量的文档注释:200 字与散文段落的量级相当(既不会让 bm25 因为块太短而虚高,也不会让 `#Lnnn` 锚点因为块太长而失去精度),而单条字幕通常只有五到十个字,不合并就永远匹配不上跨条的句子。

达到目标即封块,`line_start` 取块内首个文本行的原始行号,`line_end` 取最后一个。

`level` 填 `"line"`,`breadcrumb` 留空(文件标题由 `FileMeta.title` 承担,见 Task 5 说明)。

- [ ] **Step 4: 跑测试通过**

- [ ] **Step 5: mutation check**

把「跳过时间码行」改成不跳过,确认 `timecodes_and_cue_numbers_are_not_indexed` **和** `line_numbers_point_at_the_original_text_lines` 都变红。把合并逻辑改成逐条成块,确认 `consecutive_cues_are_merged_so_a_sentence_can_span_them` 变红。输出贴进报告。

- [ ] **Step 6: 提交**

```bash
git add searchidx/src/transcript.rs searchidx/src/chunk.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): srt/vtt 分块器,行号仍指向原始行"
```

---

## Task 5: `.txt` 分块器

**Files:** Create `searchidx/src/plain.rs`;Modify `searchidx/src/chunk.rs`、`searchidx/src/lib.rs`

**Interfaces:** Produces `pub fn chunk(body: &str, body_start_line: u32) -> Vec<Block>`

- [ ] **Step 1: 写失败的测试**

```rust
    #[test]
    fn blank_lines_separate_paragraphs() {
        let b = chunk("第一段第一行\n第一段第二行\n\n第二段\n", 1);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].line_start, 1);
        assert_eq!(b[1].line_start, 4, "行号取段落首行");
    }

    #[test]
    fn runs_of_blank_lines_do_not_produce_empty_blocks() {
        assert_eq!(chunk("甲\n\n\n\n乙\n", 1).len(), 2);
    }

    #[test]
    fn an_empty_body_yields_no_blocks() {
        assert!(chunk("", 1).is_empty());
        assert!(chunk("\n\n\n", 1).is_empty());
    }
```

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 实现**

按空行切段;`line_start` 取段落首行、`line_end` 取末行;`level` 填 `"line"`,`breadcrumb` 留空。

**在 `chunk::parse_file` 里接上分派**,按扩展名:`.note.md` → outline,`.srt`/`.vtt` → transcript,`.txt` → plain,其余 `.md` → prose。

**这三类文件没有 frontmatter**,所以 `parse_file` 里 `frontmatter::split` 的结果天然是 `None`,`fm_present` 为 false —— 但它们必然先被规则 5′ 命中(只有命中模式才会被收录),所以永不落进未标注。加一条测试钉住这个不变量:

```rust
    #[test]
    fn a_transcript_is_source_never_unlabeled() {
        let g = crate::globs::parse(&["media/**".to_string()]);
        let p = parse_file("media/a.srt", "1\n00:00:01,000 --> 00:00:02,000\n话\n", 0, &g);
        assert_eq!(p.meta.origin, crate::Origin::Source);
    }
```

- [ ] **Step 4: 跑测试通过 + 提交**

```bash
git add searchidx/src/plain.rs searchidx/src/chunk.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): txt 分块器 + 按扩展名分派"
```

---

## Task 6: schema bump 3 与模式盖章

**Files:** Modify `searchidx/src/store.rs`

- [ ] **Step 1: 写失败的测试**

```rust
    /// 模式变了,已存的 origin 全部作废 —— 增量扫描的 stat/hash 快路径永远
    /// 不会回头重算它。这是 sync_dir 踩过的同一个坑。
    #[test]
    fn changed_globs_rebuild_in_place_and_keep_the_inode() {
        // 建库并写入一个文件 → 记下 inode → 用不同的 globs 重开
        // → files 表为空,且 inode 不变(绝不 unlink,GUI 可能握着活的 WAL 连接)
    }

    #[test]
    fn a_version_2_database_is_wiped_on_open() {
        // 建库 → 手工把 meta.schema_version 改回 "2" → 重开 → files 表为空
    }
```

两条测试都必须写出真实实现,不能留注释桩。第一条的 inode 断言参照既有的 `a_changed_sync_dir_must_not_unlink_the_file` 写法。

- [ ] **Step 2-3: 实现**

`SCHEMA_VERSION` → 3。`open`/`try_open`/`create_fresh`/`stamp_fresh_schema` 的 `sync_dir: &str` 参数整体换成 `globs_stamp: &str`(调用方传 `SourceGlobs::stamp()`)。`meta` 的 `sync_dir` 键改名 `source_globs`。

盖章比对必须加进与 `schema_version`/`tokenizer_id` **同一个** `ok` 布尔,不是尽力而为那一组;不匹配走 `rebuild_in_place`,不是 `wipe`。

- [ ] **Step 4: mutation check**

把盖章比对短路成恒真,确认 `changed_globs_rebuild_in_place_and_keep_the_inode` 变红。注意:**只断言行数会漏**(全量 wipe 同样让行数归零),inode 断言才是那条测试存在的理由 —— 上一轮正是这样发现的。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/store.rs
git commit -m "feat(searchidx): schema 3,模式盖章不匹配则原地重建"
```

---

## Task 7: `Weights` 与四档权重

**Files:** Modify `searchidx/src/query.rs`

**Interfaces:** Produces
- `pub struct Weights { pub human: f64, pub derived: f64, pub source: f64, pub unlabeled: f64 }`
- `impl Default for Weights`(`1.25 / 1.0 / 0.9 / 0.3`)
- `Weights::sanitized(self) -> Weights` —— 非有限/非正/超 5.0 的分量回落到默认值
- `search_with(conn, q, limit, today, limits, weights: &Weights)`;`search` 仍是 `Weights::default()` 的薄包装

- [ ] **Step 1: 写失败的测试**

```rust
    /// 默认值就是已发布的四个常量。前车之鉴:Limits::default() 的
    /// deep:false 与该类型自己的向后兼容承诺相反,一个未来的
    /// `..Default::default()` 会静默拿到快路径。
    #[test]
    fn the_default_weights_are_the_shipped_constants() {
        let w = Weights::default();
        assert_eq!((w.human, w.derived, w.source, w.unlabeled), (1.25, 1.0, 0.9, 0.3));
    }

    /// 四档必须各自独立可验 —— 逐档断言 score_of 本身,而不是端到端排序。
    #[test]
    fn each_origin_tier_moves_the_score_on_its_own() {
        let w = Weights::default();
        let s = |o| score_of(-1.0, &hit_with(o), false, false, TODAY, &w);
        assert!(s(Origin::Human) > s(Origin::Derived), "human 必须高于 derived");
        assert!(s(Origin::Derived) > s(Origin::Source), "derived 必须高于 source");
        assert!(s(Origin::Source) > s(Origin::Unlabeled), "source 必须高于 unlabeled");
        assert_eq!(s(Origin::Derived), 0.5, "derived 必须恰好是单位元");
    }

    #[test]
    fn an_invalid_weight_falls_back_to_the_default() {
        for bad in [f64::NAN, -1.0, 0.0, 6.0] {
            let w = Weights { human: bad, ..Default::default() }.sanitized();
            assert_eq!(w.human, 1.25, "非法值 {bad} 必须回落");
        }
    }

    /// 用户可以把原始资料调得比你写的还高 —— 那是他自己的 vault。
    #[test]
    fn a_deliberate_inversion_is_allowed() {
        let w = Weights { human: 0.5, source: 2.0, ..Default::default() }.sanitized();
        assert_eq!((w.human, w.source), (0.5, 2.0));
    }
```

- [ ] **Step 2: 跑测试确认失败**

- [ ] **Step 3: 实现**

`score_of` 增加 `weights: &Weights` 参数,`match hit.origin` 四个分支取对应字段。`fts_search`/`like_search`/`finish` 逐层透传。

保留既有那段解释「与 `human_verified ×1.1` 叠加是有意的」的注释,并补一句:未标注的 ×0.3 是刻意的强降权,配合默认前 20 条会让这类文件很可能被挤出结果集,这是经确认接受的设计(spec §3.1)。

- [ ] **Step 4: 逐档 mutation check**

四档分别改成 ×1.0(未标注那档改成 ×0.9),确认**且只有**对应那条断言变红。**四次输出都贴进报告。** `derived` 那档因为本身就是 ×1.0,改成 ×1.0 是空操作 —— 改用 ×1.5 验证该分支是活的,并在报告里说明这次偏离及理由。

- [ ] **Step 5: 提交**

---

## Task 8: 设置项与两个构造点

**Files:** Modify `src-tauri/src/sotvault/vault_settings.rs`、`src-tauri/src/search/options.rs`、`src-tauri/src/search/mod.rs`、`src-tauri/tests/search_scan_options_contract.rs`

**Interfaces:** Produces
- `vault_settings`:`search_source_globs: Option<Vec<String>>`、`search_weights: Option<SearchWeights>`
- `search::options::for_vault` 填 `source_globs`
- `search::options::weights_for_vault(vault_root) -> Weights` —— **权重的唯一构造点**
- 命令 `notemd_search_glob_matches(patterns: Vec<String>) -> Result<usize, String>`

- [ ] **Step 1: 写失败的测试**

```rust
    /// 首次升级时模式为空,用**当前解析出的** syncDir 种一条,而不是字面量
    /// "sync/**" —— 改过镜像目录名的用户否则会突然发现镜像文件不算原始资料。
    #[test]
    fn an_absent_glob_list_is_seeded_from_the_resolved_sync_dir() {
        // 写一个 syncDir: "box" 的 settings.json → for_vault 的 source_globs
        // 必须匹配 "box/x.md" 且不匹配 "sync/x.md"
    }

    #[test]
    fn an_explicit_empty_list_is_respected_not_reseeded() {
        // searchSourceGlobs: [] → 不得再种 syncDir;否则用户清空不掉
    }

    #[test]
    fn weights_fall_back_to_defaults_when_unset_or_invalid() {
        // 缺字段 → Weights::default();字段为 0 → 该档回落
    }
```

契约测试同步扩一条:GUI 与 CLI 取到**同一份权重**。

- [ ] **Step 2-3: 实现**

`weights_for_vault` 与 `for_vault` 同样只读一次 `settings.json`(既有教训:读两次会产生撕裂的配置)。

`notemd_search_glob_matches` **按真实 vault 现场走一遍文件树计数**,不查索引 —— 模式命中的 `.srt`/`.txt` 此刻还不在索引里,用索引反推会永远少算,而少算的方向恰好诱导用户把模式写得更宽(spec §7.1)。复用 `is_indexable` 的同一套判定,避免第二份真相。

- [ ] **Step 4: 跑两个 cargo 套件 + 契约测试 + 提交**

---

## Task 9: `origin:unlabeled` 与对外文档

**Files:** Modify `searchidx/src/query.rs`、`src-tauri/src/cli/search.rs`、`src-tauri/src/agents_sync/logic.rs`、`src-tauri/templates/AGENTS.md`、`src-tauri/src/cli/builtin.rs`

- [ ] **Step 1: 测试**

`parse("x origin:unlabeled")` 解析正确;过滤生效;非法值仍 fail-closed(既有行为,不要改)。

`cli/search.rs` 的 `fallback_scan`(无索引降级路径)也要拿到 `SourceGlobs` 并调 `origin::derive`,否则 `--json` 里同一个文件在有索引与无索引两条路上会给出不同的 `origin`。既有测试 `the_no_index_fallback_reports_the_same_origin_tier_the_index_would` 覆盖这条,**必须扩到未标注这一层**。

- [ ] **Step 2: 实现**

`AGENTS.md` 的 `SEARCH_SECTION` 与 `templates/AGENTS.md` 同步加 `origin:unlabeled`;`--help` 同步。**这是唯一广告位**:`append_search_section` 是 append-only 的、只在整段缺失时才写,漏了就是永久静默。既有的漂移测试 `template_contains_the_same_search_section_the_append_path_writes` 会挡住两者不一致,但挡不住两者一起漏 —— 所以要**读一遍实际文本**确认新值在里面。

- [ ] **Step 3: 确认 CLI 默认输出未变** —— 契约测试断言 `path:line:text` 逐字不变。

- [ ] **Step 4: 提交**

---

## Task 10: 前端 —— 候选模式生成与分组

**Files:** Create `src/lib/search/glob-suggest.ts` + 测试;Modify `src/lib/search/grouping.ts` + 测试、`src/lib/search/api.ts`、i18n 四语

- [ ] **Step 1: 写纯函数测试**

```ts
describe('suggestGlobs', () => {
  it('从窄到宽给出候选', () => {
    const s = suggestGlobs('ebook/三体/book.md')
    expect(s.map(x => x.pattern)).toEqual(['ebook/三体/**', 'ebook/**/*.md', 'ebook/**'])
  })
  it('根目录下的文件不产出空目录段的模式', () => {
    expect(suggestGlobs('a.md').every(x => !x.pattern.startsWith('/'))).toBe(true)
  })
  it('候选去重且始终非空', () => {
    // 单层路径下,「目录/**」与「目录/**/*.ext」可能塌成同一条
    const s = suggestGlobs('clips/a.txt')
    expect(new Set(s.map(x => x.pattern)).size).toBe(s.length)
    expect(s.length).toBeGreaterThan(0)
  })
})

describe('groupHits', () => {
  it('四组顺序:你写的 → AI 产出 → 原始资料 → 未标注', () => {})
  it('未标注为空时不显示该组', () => {})
})
```

- [ ] **Step 2-3: 实现**

`suggestGlobs` 是纯函数,不碰 Svelte。命中数**不在这里算** —— 由调用方拿 `notemd_search_glob_matches` 填,因为只有后端能看到未进索引的文件。

`grouping.ts` 增加 `unlabeled` 组,排在 `source` 之后。中间层组头仍渲染**未翻译的原始 `concept_type` 串**(既有人工裁决,不要改)。

- [ ] **Step 4: 四语 i18n**。非英语按 `Record<keyof Messages, string>` 类型,缺键是编译错误,`pnpm check` 是真闸门。

- [ ] **Step 5: 提交**

---

## Task 11: 设置页界面

**Files:** Modify `src-tauri/src/search/mod.rs`(`OriginCounts` / `OriginCountsDto` 加第四层)、`src/lib/search/api.ts`、`src/components/SettingsDialog.svelte` + `SettingsDialog.search-tab.test.ts`、i18n 四语

> **计划勘误(C-T8 期间发现):** 本任务原先只列了前端文件,但「分层统计从三层改四层」在后端就做不到 —— `OriginCounts`/`OriginCountsDto` 自 B-T8 起就是三层,`unlabeled` 的行数被**静默排除在统计之外**(当时有一条测试专门钉住这个已知的少算)。前端拿不到第四个数字,所以后端必须一起改,`VaultSettingsDto` 也要补 `searchSourceGlobs`/`searchWeights` 两个字段。

- [ ] **Step 1: 组件测试**

`SettingsDialog.search-tab.test.ts` 已有真实 `mount`/`unmount` + happy-dom 的挂载测试,扩它,不要另起一套。

断言必须**按标签定位再取值**,不能对整页 `textContent` 做 `toContain` —— 上一轮正是这个写法让「两极数字对调」在全绿下溜过去。至少覆盖:

- 粘一条样例路径后出现三条候选,且**默认选中最窄的那条**
- 四层统计各自的数字挂在各自的标签下
- 未标注那一行可点,点击后触发 `origin:unlabeled` 搜索
- 权重输入的非法值分支

```ts
  // 按标签定位再取值 —— 不要对整页 textContent 做 toContain
  const rowValue = (label: string) => {
    const row = [...container.querySelectorAll('.tier-row')]
      .find(r => r.querySelector('.tier-label')?.textContent?.includes(label))
    return row?.querySelector('.tier-count')?.textContent
  }

  it('每层数字挂在自己的标签下', async () => {
    await mountSearchTab({ originCounts: { human: 40, derived: 33, source: 7, unlabeled: 18 } })
    expect(rowValue(t('search.group.human'))).toBe('40')
    expect(rowValue(t('search.group.source'))).toBe('7')
    expect(rowValue(t('search.group.unlabeled'))).toBe('18')
  })

  it('粘样例路径后默认选中最窄的候选', async () => {
    await pasteSample('ebook/三体/book.md')
    const checked = container.querySelector('input[name="glob-candidate"]:checked')
    expect(checked?.getAttribute('value')).toBe('ebook/三体/**')
  })
```

- [ ] **Step 2-3: 实现**

「原始资料模式」块:模式列表 + 每条命中数 + 「从样例路径生成」。

两类输入错误分开处理,对应 spec §8 前两行:
- **空白模式** —— 保存时拒绝并指出是哪一条。crate 侧 `parse` 对它是**容忍**的(直接丢弃,见 Task 1 的 `an_unparseable_pattern_is_dropped_not_fatal`),那是消费者义务;但 UI 静默吞掉用户刚输入的一行是另一回事,用户会以为存上了。
- **命中 0 个** —— 允许保存,但当场提示。这是大小写字面匹配(spec §4.1)的兜底:vault 目录大小写被改过时,模式会全部落空而语法完全合法。

「权重」块:四个数字输入 + 「恢复默认」。**旁注必须写明:改权重立即生效、不重建索引;改模式会触发一次重建。** 这是两类设置最容易被混淆的地方。

分层统计从三层改四层。

- [ ] **Step 4: 四个门禁 + 提交**

---

## Task 12: 回归集与体积实测

**Files:** `searchidx/tests/fixtures/retrievability.json`、`searchidx/tests/fixtures/corpus/`、`docs/2026-08-10-vault-search-index-measurements.md`

- [ ] **Step 1: 全部用例重跑**

`cargo test --manifest-path searchidx/Cargo.toml --test acceptance retrievability`

**回归集必须显式传 `Weights::default()`** —— 它是唯一的排序裁判,跟着用户配置走就等于没有裁判。

- [ ] **Step 2: 逐条人工确认**

**禁止照着新输出批量刷新。** 每条变更在报告里单列:用例、旧期望、新期望、判定理由。

预期会动的是原先靠规则 6 判 `source`、现在改判 `unlabeled` 的语料 —— 权重从 ×0.9 掉到 ×0.3,排序变化会比上一次大得多。注意:该测试只断言前 20 条召回、**不看顺序**,所以「测试全绿」不等于「没有变化」;必须像上一轮那样**自己 diff 完整有序结果**再判断。

- [ ] **Step 3: 补新用例**

至少 6 条:模式命中的 `.srt` 能被跨字幕条的句子搜到且行号正确;模式外的 `.srt` 搜不到;未标注排在原始资料之后;`origin:unlabeled` 只剩未标注层;指定目录里的 AI 摘要仍判 AI 产出;权重非默认时排序随之改变。

- [ ] **Step 4: 体积实测**

在真实 vault 上量一次收录转写前后的索引大小,更新 measurements 文档,并据此判断索引大文件阈值默认值(现 10 MB)是否要调。结论写进报告,**不要顺手改默认值** —— 那是独立的产品决定。

- [ ] **Step 5: 提交**

---

## 人工 GUI 验收清单

1. 粘一条 ebook 路径,确认三条候选与命中数合理,默认选中最窄
2. 保存一条命中 0 个的模式,确认有提示
3. 改权重后立即搜索,确认顺序变化且**没有**触发重建
4. 改模式后确认触发了一次重建(进度条出现)
5. 分层统计四层数字与实际相符
6. 点击未标注统计,确认跳到 `origin:unlabeled` 结果
7. 搜一句只在转写稿里出现的话,确认能命中且点开落在正确行
8. 深浅色主题下四组组头正常

## 已知取舍

- **×0.3 会让大量既有笔记从搜索里消失。** 经确认接受;出口是 `origin:unlabeled`。release note 必须正面说明。
- **模式是字面大小写匹配**,vault 目录大小写被改过就会全部落空,靠命中 0 个的提示兜底。
- **规则 2 先于 5′ 的已知盲区仍在**(见 `origin.rs` 规则 2 注释):给 `type: Book` 补 `generated:` 戳的流水线会把电子书移出原始资料层。本次不改顺序。
