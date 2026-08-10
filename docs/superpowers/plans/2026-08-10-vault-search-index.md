# Vault 检索(索引 + 检索)实施计划 — P0 + P1

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 给 note.md 加一套单机快速全文索引与零 token 检索——平台无关核心 crate `searchidx`(SQLite FTS5 + jieba 分词),三个 thin adapter(Tauri command、`notemd search` CLI、文件 watcher),外加 UI 搜索面板与 AGENTS.md 约定层。

**Architecture:** 索引是 vault 文件的纯函数,存本机应用缓存(vault 外),可弃、幂等重建、绝不回写任何 `.md`。全部扫描/解析/分词/schema/查询逻辑放进 `searchidx` 核心 crate;Tauri/CLI/watcher 只做平台绑定。GUI 与 CLI 是两个无 IPC 的进程,靠"文件粒度幂等替换 + WAL"自然收敛,不做锁协商。

**Tech Stack:** Rust 2021 · `rusqlite 0.40` (bundled, FTS5) · `jieba-rs 0.10` (default-dict, `cut_for_search`) · `pulldown-cmark 0.13` · `ignore 0.4` · `notify 7` · `sha2` · Svelte 5 runes 前端 · vitest + cargo test。

**Spec:** `docs/2026-08-10-vault-search-index-design.md`(v3.2)。本计划覆盖 spec §8 的 **P0 + P1**;P2(`notemd ask`)与 P3 不在范围内。

---

## 实测校正(动手前必读)

计划编写期已用真实编译探针验证了 spec 里几条**估算**,三条被证伪或需收紧。执行时按本节的数字走,不按 spec 的估算走:

| spec 原文 | 实测 | 处置 |
| --- | --- | --- |
| 二进制总增量 **< 4MB 硬门**(§3.1/§7) | **+4.74MB**(release,`opt-level="z"`+LTO+strip 探针二进制:sqlite bundled+FTS5 **+0.90MB**、jieba-rs default-dict **+3.60MB**、pulldown-cmark **+0.18MB**) | 硬门改为 **< 5.0MB**。sqlite 与 cmark 与估算吻合,jieba 词典比估算大 1.1MB。Task 19 用真实 app 二进制复测并写进 PR 与 README。 |
| jieba 词典需自行 gzip 内嵌 + 构建期压缩(§3.1/§3.2) | jieba-rs 0.10 的 `default-dict` **已经**用 `include_flate` 做构建期 deflate + 首次使用时惰性解压 | **不自研词典内嵌**。直接用 `default-dict`,顺带白拿 spec §2③「词典随二进制内嵌、两平台字节相同」——词典住在 crate 里,版本即字节。 |
| CJK 惰性加载一次 ~200–400ms(§3.2/§7) | **78ms**(release,M 系列) | CLI CJK 端到端预算仍按 spec 的 1.2s 验收(留余量),但不必为此设计额外规避。 |
| 排序归一化 `1/(1+rank)`(§4) | FTS5 `bm25()` 返回**负值**(探针实测 `-0.000001`),`1/(1+rank)` 在负 rank 下非单调、会产生负分 | 改为 `r = -bm25(...)`(≥0,越大越相关),`score = r_boosted/(1+r_boosted)`。单调、值域 (0,1)、乘性加成可组合。理由写进代码注释。 |
| Windows 装 `notemd.cmd` 进 PATH(§6②) | GUI 可执行文件就叫 `notemd.exe`;cmd 的 `PATHEXT` 里 `.EXE` **排在 `.CMD` 前面**,同目录下 `notemd` 会解析到 GUI 而不是垫片 | 垫片改放 `$INSTDIR\bin\notemd.cmd`,PATH 只加 `$INSTDIR\bin`(GUI exe 根本不上 PATH)。两平台命令行都是 `notemd search`,比 pc-port 计划里的 `notemd-cli.cmd` 更符合 harness 目标。 |
| §6① CLI 配置目录修复 | `src-tauri/src/cli/mod.rs:42-46` **已经**是 `dirs::config_dir().join(BUNDLE_ID)` | 已完成,不重做。只在 Task 15 补一条锁死它的契约测试。 |

另外两条已验证成立、是整个设计的地基,别动:

- FTS5 在 `rusqlite` 的 `bundled` feature 下**默认可用**(`libsqlite3-sys` build.rs 带 `-DSQLITE_ENABLE_FTS5`),不需要额外 feature。
- FTS5 默认 `unicode61` 分词器把**整段连续汉字当作一个词元**。实测:存 `"增量索引"` 时 `MATCH "增量"` **查不到**;存 `cut_for_search` 的重叠输出 `"增量 索引 增量索引"` 时三个查询全中。这正是 spec §3.2「双侧 cut_for_search」的实证依据——预分词是唯一权威,FTS5 只负责按空格切。

---

## Global Constraints

每个任务的要求都隐含包含本节。

- **绝不回写任何 `.md`。** 索引只读 vault。唯一的写文件动作是 Task 18 的 AGENTS.md 追加,且必须人确认。(spec 非目标 / 产品信念 2)
- **索引在 vault 外、可弃、是纯函数。** `schema_version` 或 `tokenizer_id` 不符、库损坏 → 删库全量重建,没有修复逻辑。(§3.3)
- **跨平台确定性规约**(§2,四条,每条都有对应测试):
  1. 索引内 `path` 与所有输出(含 `source_ref`)一律是 **vault 相对路径**,分隔符规范化为 `/`。
  2. 文本处理前剥掉**所有** `\r`(与 TS `stripCarriageReturns` 逐字一致),行号按 `\n` 计,**1-based**。
  3. jieba 词典/版本随二进制内嵌,两平台字节相同(靠 `default-dict` 白拿)。
  4. `content_hash` 对**原始字节**计算(不是剥 `\r` 后的文本)。
- **算法一致性靠单 crate 保证。** 扫描/解析/分词/schema/查询全在 `searchidx` 内;`src-tauri` 侧只允许有平台绑定代码(路径、notify、Tauri command、CLI 输出)。任何在 adapter 里写分词/排序/解析逻辑的 PR 都是错的。
- **`searchidx` 不依赖 `tauri`。** 它必须能脱离 GUI 单测。
- **索引属于机器,不属于 vault。** 路径一律 `dirs::data_local_dir()`(Windows = `%LOCALAPPDATA%`,**不是** `data_dir()` 的 Roaming)。GUI 与 CLI 必须算出同一个库路径,由测试锁死。(§3.4)
- **检索永不阻塞调用方。** 任何故障走 §9 降级矩阵:降级 + stderr 一行提示,不报错退出。CLI 因索引问题退出码只能是 0(有命中)或 1(无命中),不能是 2。
- **二进制体积硬门:release 总增量 < 5.0MB**,实测数字写进 PR 与 README(见「实测校正」)。
- **不建 GitHub Actions。** 跨平台一致性用固化 fixtures 的普通 `cargo test` 表达,mac 上跑通,Windows 侧由人在发版机跑一次 `cargo test`。(用户既定规矩)
- **i18n:** 所有用户可见字符串走 `t()`,四语言 `en/zh/ja/de` 同批补齐。插件外的宿主字符串放 `src/lib/i18n/*.ts`,扁平点分键。
- **测试命令:** `searchidx` 用 `cargo test --manifest-path searchidx/Cargo.toml`;宿主 Rust 用 `cargo test --manifest-path src-tauri/Cargo.toml`;前端用 `pnpm test`、`pnpm check`。
- **提交粒度:** 每个 Task 末尾一次提交。共享 worktree,提交前**只精确 `git add` 目标文件,绝不 `git add -A`**。

---

## File Structure

### 新建:`searchidx/`(仓库根,与 `plugin-protocol/` 同级,自带 Cargo.lock)

| 文件 | 职责 |
| --- | --- |
| `Cargo.toml` | crate 定义与依赖钉版 |
| `src/lib.rs` | 公开门面 `SearchIndex` + 类型再导出;各模块串起来的唯一地方 |
| `src/norm.rs` | 跨平台规范化原语:相对路径、剥 `\r`、`content_hash` |
| `src/tokenize.rs` | 分词器:ASCII 直词元化 + 汉字走 jieba `cut_for_search`;`TOKENIZER_ID` |
| `src/frontmatter.rs` | 宽容 frontmatter 浅层键解析(解析失败字段置 NULL,不拒文件) |
| `src/block.rs` | `Block` / `BlockLevel` / `FileMeta` / `Link` 数据模型 |
| `src/prose.rs` | 散文 `.md` 分块(pulldown-cmark OffsetIter → 行号) |
| `src/outline.rs` | `.note.md` 大纲分块(缩进 + 属性行 + 答复围栏) |
| `src/links.rs` | wikilink / markdown 链接抽取 |
| `src/chunk.rs` | 按后缀分派到 prose/outline,产出 `(FileMeta, Vec<Block>, Vec<Link>)` |
| `src/store.rs` | schema、打开/自愈、文件粒度幂等替换、meta 读写 |
| `src/paths.rs` | 索引库路径解析(`data_local_dir` + vault 路径 hash) |
| `src/scan.rs` | 全量构建 + 新鲜度 sweep + 护栏 |
| `src/query.rs` | 查询语法解析、FTS 检索、排序加成、LIKE 兜底 |
| `src/watch.rs` | **纯**去抖/洪峰降级决策逻辑(不含 notify) |
| `tests/fixtures/` | 跨语言与跨平台固化 fixtures |

### 修改:`src-tauri/`

| 文件 | 改动 |
| --- | --- |
| `Cargo.toml` | 加 `searchidx = { path = "../searchidx" }` |
| `src/lib.rs` | 注册 3 个 command;启动/切 vault 时起 watcher;View 菜单加搜索项 |
| `src/search/mod.rs`(新建) | Tauri command 层 + 索引句柄状态 |
| `src/search/watch.rs`(新建) | notify 绑定,驱动 `searchidx::watch` 的纯决策 |
| `src/cli/search.rs`(新建) | `notemd search` 实现:vault-root 解析、输出、退出码、降级 |
| `src/cli/router.rs` | 加 `Builtin::Search(SearchArgs)` 路由 |
| `src/cli/builtin.rs` | 分派到 `search::run` + help 文案 |
| `src/sotvault/vault_settings.rs` | 加 `search_exclude_dirs` |
| `src/sotvault/mod.rs` | `notemd_vault_settings_set` 透传新字段 |
| `templates/AGENTS.md` | 加 "## Searching this vault" 节 |
| `tauri.windows.conf.json` | NSIS `installerHooks` |
| `installer/hooks.nsi`(新建) | 写 `bin\notemd.cmd` 垫片 + PATH |
| `tests/search_cli_contract.rs`(新建) | CLI 契约测试 |
| `tests/cli_startup_timing.rs` | 加 ASCII / CJK 两档 search 预算 |

### 修改/新建:前端

| 文件 | 改动 |
| --- | --- |
| `src/lib/search/api.ts`(新建) | `invoke` 包装 + 类型 |
| `src/lib/search/store.svelte.ts`(新建) | 面板状态(query/hits/route/loading) |
| `src/components/side-panel/SearchPanel.svelte`(新建) | 面板 UI |
| `src/lib/side-panel/registry.svelte.ts` | 注册 `vault-search` 视图 |
| `src/lib/commands.ts` | `toggle-vault-search` |
| `src/lib/i18n/{en,zh,ja,de}.ts` | `search.*` 键 |
| `src/lib/outline/cross-lang-fixtures.test.ts`(新建) | 与 Rust 共享 fixtures 的行归属交叉验证 |
| `src/lib/vault-settings.svelte.ts` + 设置 UI | `searchExcludeDirs` |
| `README.md` / `README.zh-CN.md` | 体积表述 |

---

# P0 · 索引内核 + CLI

### Task 1: `searchidx` crate 骨架 + 跨平台规范化原语

建立 crate,并把 spec §2 的四条确定性规约做成可测的函数。这四条是后面所有模块的地基,先钉死。

**Files:**
- Create: `searchidx/Cargo.toml`
- Create: `searchidx/src/lib.rs`
- Create: `searchidx/src/norm.rs`
- Create: `searchidx/.gitignore`
- Modify: `src-tauri/Cargo.toml`(加 path 依赖)

**Interfaces:**
- Produces:
  - `searchidx::norm::rel_path(vault_root: &Path, abs: &Path) -> Option<String>` — vault 相对、`/` 分隔;越界或等于 root 返回 `None`
  - `searchidx::norm::strip_cr(text: &str) -> Cow<'_, str>`
  - `searchidx::norm::content_hash(bytes: &[u8]) -> String` — 64 字符小写 hex
  - `searchidx::norm::line_starts(text: &str) -> Vec<usize>` — 每行起始字节偏移,供字节偏移 → 1-based 行号
  - `searchidx::norm::line_of(line_starts: &[usize], byte_offset: usize) -> u32` — 1-based

- [ ] **Step 1: 建 crate 文件**

`searchidx/Cargo.toml`:

```toml
[package]
name = "searchidx"
version = "0.1.0"
edition = "2021"
description = "Platform-independent full-text index core for note.md vaults."

[dependencies]
rusqlite = { version = "0.40", features = ["bundled"] }
jieba-rs = "0.10"
pulldown-cmark = { version = "0.13", default-features = false }
ignore = "0.4"
sha2 = "0.10"
hex = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
dirs = "5"

[dev-dependencies]
tempfile = "3"
```

`searchidx/.gitignore`:

```
/target
```

`searchidx/src/lib.rs`(本任务只需要模块声明,门面在 Task 11 补):

```rust
//! Platform-independent full-text index for note.md vaults.
//!
//! The index is a **pure function of the vault's files** (plus the tokenizer
//! version) stored outside the vault, in the machine's local app cache. It is
//! disposable: any inconsistency is resolved by deleting and rebuilding, never
//! by repair logic. Nothing here ever writes into the vault.
//!
//! Everything that decides *what gets indexed and what ranks first* lives in
//! this crate so that the Tauri command layer, the `notemd search` CLI and the
//! file watcher are three thin adapters over one algorithm. That is the whole
//! reason the crate exists — see docs/2026-08-10-vault-search-index-design.md §2.

pub mod norm;
```

- [ ] **Step 2: 写失败的测试**

`searchidx/src/norm.rs` 先只放测试(实现留空模块),或直接把测试写在文件末尾并让 `cargo test` 因函数不存在而编译失败——两种都行,推荐后者:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rel_path_is_vault_relative_and_slash_separated() {
        let root = Path::new("/Users/x/vault");
        assert_eq!(
            rel_path(root, Path::new("/Users/x/vault/docs/a.md")).as_deref(),
            Some("docs/a.md")
        );
        // 单段
        assert_eq!(rel_path(root, Path::new("/Users/x/vault/a.md")).as_deref(), Some("a.md"));
        // root 本身没有相对路径
        assert_eq!(rel_path(root, root), None);
        // vault 外
        assert_eq!(rel_path(root, Path::new("/Users/x/other/a.md")), None);
    }

    /// 跨平台不变式:Windows 上产生的分隔符必须被规范化成 `/`,否则同一批
    /// fixtures 在两平台索引出的 `path` 不同,`source_ref` 给 agent 的锚也不同。
    #[cfg(windows)]
    #[test]
    fn rel_path_normalizes_backslashes_on_windows() {
        let root = Path::new(r"C:\Users\x\vault");
        assert_eq!(
            rel_path(root, Path::new(r"C:\Users\x\vault\docs\a.md")).as_deref(),
            Some("docs/a.md")
        );
    }

    #[test]
    fn strip_cr_removes_every_carriage_return() {
        assert_eq!(strip_cr("a\r\nb\r\n").as_ref(), "a\nb\n");
        // 孤立的 \r 也剥:与 TS stripCarriageReturns 逐字一致(见 outline/markdown.ts)
        assert_eq!(strip_cr("a\rb").as_ref(), "ab");
        // 无 \r 时零拷贝
        assert!(matches!(strip_cr("plain"), std::borrow::Cow::Borrowed(_)));
    }

    /// hash 对**原始字节**算,不是剥 \r 之后的文本:换行风格变化必须被视为
    /// 文件变化,否则 CRLF↔LF 的改写会让增量索引漏掉这个文件。
    #[test]
    fn content_hash_is_over_raw_bytes() {
        assert_ne!(content_hash(b"a\r\nb"), content_hash(b"a\nb"));
        assert_eq!(content_hash(b"abc").len(), 64);
        assert_eq!(
            content_hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn line_numbers_are_one_based_and_count_by_lf() {
        let text = "one\ntwo\nthree";
        let starts = line_starts(text);
        assert_eq!(line_of(&starts, 0), 1);
        assert_eq!(line_of(&starts, 3), 1);   // 行尾的 \n 之前仍属第 1 行
        assert_eq!(line_of(&starts, 4), 2);
        assert_eq!(line_of(&starts, text.len() - 1), 3);
        // 越界偏移收敛到最后一行,不 panic
        assert_eq!(line_of(&starts, 9999), 3);
    }
}
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml norm`
Expected: 编译失败 —— `cannot find function 'rel_path' in this scope` 等。

- [ ] **Step 4: 写实现**

`searchidx/src/norm.rs` 顶部:

```rust
//! Cross-platform normalization primitives.
//!
//! These four functions ARE the determinism contract from the design spec §2:
//! the same vault indexed on macOS and on Windows must yield byte-identical
//! `path` values, identical line numbers, and identical content hashes. Every
//! other module goes through here rather than touching `Path` / `\r` directly.

use std::borrow::Cow;
use std::path::Path;

/// Vault-relative, `/`-separated path. `None` when `abs` is not strictly below
/// `vault_root`.
///
/// `Path::strip_prefix` gives us a relative `Path` whose separator is still the
/// platform's, so we re-join the components explicitly. `to_string_lossy` is
/// deliberate: a filename that is not valid UTF-8 still gets indexed under a
/// best-effort name rather than being dropped — the index is a search aid, not
/// an authority on bytes.
pub fn rel_path(vault_root: &Path, abs: &Path) -> Option<String> {
    let rel = abs.strip_prefix(vault_root).ok()?;
    let mut out = String::new();
    for comp in rel.components() {
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(&comp.as_os_str().to_string_lossy());
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Remove every `\r`.
///
/// Not "strip a trailing \r per line": the TypeScript outline parser does a
/// blanket `text.replace(/\r/g, '')` (src/lib/outline/markdown.ts), and the two
/// parsers must agree line-for-line. One rule, both languages, nothing to
/// reason about.
pub fn strip_cr(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        Cow::Owned(text.replace('\r', ""))
    } else {
        Cow::Borrowed(text)
    }
}

/// SHA-256 of the raw file bytes, lowercase hex.
pub fn content_hash(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Byte offset of the start of each line (line 1 first). Text must already have
/// been through [`strip_cr`].
pub fn line_starts(text: &str) -> Vec<usize> {
    let mut v = vec![0usize];
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

/// 1-based line number for a byte offset. Out-of-range offsets clamp to the
/// last line — callers get a usable anchor instead of a panic.
pub fn line_of(line_starts: &[usize], byte_offset: usize) -> u32 {
    match line_starts.binary_search(&byte_offset) {
        Ok(i) => (i + 1) as u32,
        Err(i) => i.max(1) as u32,
    }
}
```

在 `lib.rs` 保持 `pub mod norm;`。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --manifest-path searchidx/Cargo.toml norm`
Expected: PASS(macOS 上 5 个测试,Windows 上 6 个)。

- [ ] **Step 6: 接进 src-tauri**

在 `src-tauri/Cargo.toml` 的 `[dependencies]` 里,紧挨 `plugin-protocol` 那行加:

```toml
# Platform-independent search core. Everything about *how* the vault is scanned,
# tokenized, stored and ranked lives there so that this crate's Tauri commands,
# the CLI and the watcher are three adapters over one algorithm — see
# docs/2026-08-10-vault-search-index-design.md §2.
searchidx = { path = "../searchidx" }
```

Run: `cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | tail -5`
Expected: 构建成功(首次会编译 bundled SQLite,较慢)。

- [ ] **Step 7: 提交**

```bash
git add searchidx/Cargo.toml searchidx/.gitignore searchidx/src/lib.rs searchidx/src/norm.rs searchidx/Cargo.lock src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(searchidx): crate skeleton + cross-platform normalization primitives"
```

---

### Task 2: 分词器(索引与查询共用同一实现)

spec §3.2。索引侧与查询侧**必须**是同一个函数,否则词边界一漂就漏检。ASCII 不过 jieba(CLI 有启动预算),汉字走 `cut_for_search` 拿重叠输出保召回。

**Files:**
- Create: `searchidx/src/tokenize.rs`
- Modify: `searchidx/src/lib.rs`(加 `pub mod tokenize;`)

**Interfaces:**
- Consumes: 无
- Produces:
  - `searchidx::tokenize::TOKENIZER_ID: &str`
  - `searchidx::tokenize::tokenize(text: &str) -> String` — 空格连接的词元串,直接存进 FTS 列
  - `searchidx::tokenize::tokens(text: &str) -> Vec<String>` — 同上但返回 Vec,查询侧构造 MATCH 表达式用
  - `searchidx::tokenize::has_han(text: &str) -> bool` — 是否含汉字(查询侧决定要不要走 LIKE 兜底)

- [ ] **Step 1: 写失败的测试**

`searchidx/src/tokenize.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_runs_are_lowercased_word_tokens() {
        assert_eq!(tokens("Hello World"), vec!["hello", "world"]);
        // 标点是分隔符,不是词元
        assert_eq!(tokens("foo_bar-baz.qux"), vec!["foo_bar", "baz", "qux"]);
        assert_eq!(tokens("v6.808.3"), vec!["v6", "808", "3"]);
    }

    /// spec §3.2 的核心主张:cut_for_search 的重叠输出让「查'增量'命中'增量索引'」
    /// 成立。FTS5 的 unicode61 把整段汉字当一个词元,所以不预分词就必然漏检。
    #[test]
    fn han_runs_go_through_cut_for_search_with_overlap() {
        let t = tokens("增量索引");
        assert!(t.contains(&"增量".to_string()), "{t:?}");
        assert!(t.contains(&"索引".to_string()), "{t:?}");
    }

    #[test]
    fn mixed_cjk_and_ascii_are_both_tokenized() {
        let t = tokens("用 FTS5 做全文检索");
        assert!(t.contains(&"fts5".to_string()), "{t:?}");
        assert!(t.contains(&"全文".to_string()), "{t:?}");
        assert!(t.contains(&"检索".to_string()), "{t:?}");
    }

    #[test]
    fn single_han_char_is_its_own_token() {
        assert_eq!(tokens("我"), vec!["我"]);
    }

    #[test]
    fn tokenize_joins_with_single_spaces_for_fts_storage() {
        assert_eq!(tokenize("Hello 世界"), "hello 世界");
        assert_eq!(tokenize("   "), "");
    }

    #[test]
    fn has_han_detects_only_ideographs() {
        assert!(has_han("检索"));
        assert!(!has_han("search"));
        assert!(!has_han("かな"));      // 假名走通用词元路径,不进 jieba
    }

    /// 分词器指纹:jieba 升级或我们改了切分规则时必须失败,提醒开发者 bump
    /// TOKENIZER_ID —— 那才是让所有用户的索引自动重建的开关。指纹是「金句」而不是
    /// 版本号,因为真正会伤到索引的是**输出漂移**,不是版本字符串。
    #[test]
    fn tokenizer_fingerprint_is_frozen() {
        const PROBE: &str = "增量索引与全文检索 v2 Hello 我";
        assert_eq!(
            tokenize(PROBE),
            "增量 索引 与 全文 检索 全文检索 v2 hello 我",
            "tokenizer output drifted — bump TOKENIZER_ID so existing indexes rebuild"
        );
        assert_eq!(TOKENIZER_ID, "v1+jieba-rs-0.10+cut_for_search+hmm");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml tokenize`
Expected: 编译失败(函数未定义)。

- [ ] **Step 3: 写实现**

`searchidx/src/tokenize.rs` 顶部:

```rust
//! The tokenizer. Index side and query side call the SAME function — that
//! symmetry is the whole contract (design spec §3.2).
//!
//! Why pre-tokenize at all: FTS5's built-in `unicode61` tokenizer treats a run
//! of Han characters as ONE term. Measured: storing "增量索引" makes
//! `MATCH "增量"` miss it entirely. So we do the segmentation ourselves, store
//! the result space-joined, and let unicode61 do nothing but split on spaces.
//! Writing a custom FTS5 tokenizer would mean the C API for no extra benefit.
//!
//! Why `cut_for_search` rather than plain `cut`: it emits the long word AND its
//! sub-words, so "增量索引" indexes as {增量, 索引, 增量索引} and a query for
//! "增量" hits. Recall over precision, deliberately.

use std::sync::OnceLock;

use jieba_rs::Jieba;

/// Identity of the tokenization *algorithm*, stored in the index's `meta` table.
///
/// A mismatch means the stored tokens were produced by different rules than the
/// query would produce, so the index is not a valid pure function of the files
/// any more and gets rebuilt from scratch. Bump this whenever the output of
/// `tokenize` changes for any input — including a jieba upgrade that moves the
/// dictionary. The frozen-fingerprint test in this module exists to make sure
/// nobody forgets.
pub const TOKENIZER_ID: &str = "v1+jieba-rs-0.10+cut_for_search+hmm";

/// The dictionary is deflate-compressed into the binary by jieba-rs's
/// `default-dict` feature and decompressed on first touch (~78 ms measured on a
/// release build). Lazy on purpose: the CLI has a startup budget and a pure
/// ASCII query must not pay the dictionary tax.
static JIEBA: OnceLock<Jieba> = OnceLock::new();

fn jieba() -> &'static Jieba {
    JIEBA.get_or_init(Jieba::new)
}

/// CJK Unified Ideographs (+ extensions and compatibility). Deliberately NOT
/// kana or Hangul: jieba is a Chinese segmenter and would produce noise there.
/// Those scripts fall through to the generic word-run path, which keeps a run
/// as one token — findable by exact term or by the LIKE fallback, which is an
/// honest limitation rather than a fake segmentation.
fn is_han(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// A character that can be part of a non-Han word token.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Segment `text` into index/query terms.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut han = String::new();

    let flush_word = |word: &mut String, out: &mut Vec<String>| {
        if !word.is_empty() {
            out.push(word.to_lowercase());
            word.clear();
        }
    };
    let flush_han = |han: &mut String, out: &mut Vec<String>| {
        if !han.is_empty() {
            for tok in jieba().cut_for_search(han, true) {
                out.push(tok.word.to_string());
            }
            han.clear();
        }
    };

    for c in text.chars() {
        if is_han(c) {
            flush_word(&mut word, &mut out);
            han.push(c);
        } else if is_word(c) {
            flush_han(&mut han, &mut out);
            word.push(c);
        } else {
            flush_word(&mut word, &mut out);
            flush_han(&mut han, &mut out);
        }
    }
    flush_word(&mut word, &mut out);
    flush_han(&mut han, &mut out);
    out
}

/// Space-joined tokens, ready to be stored in an FTS5 column.
pub fn tokenize(text: &str) -> String {
    tokens(text).join(" ")
}

/// Whether the text contains Han ideographs, i.e. whether the jieba path (and
/// therefore the dictionary-blind-spot fallback) is relevant.
pub fn has_han(text: &str) -> bool {
    text.chars().any(is_han)
}
```

在 `lib.rs` 加 `pub mod tokenize;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path searchidx/Cargo.toml tokenize -- --nocapture`
Expected: 7 个测试全 PASS。若 `tokenizer_fingerprint_is_frozen` 失败,把实际输出抄进断言并**同时**确认这是有意的规则变更(否则说明实现写错了)。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/tokenize.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): jieba cut_for_search tokenizer shared by index and query"
```

---

### Task 3: 宽容 frontmatter 解析

spec §3.5 + OKF §11 消费者宽容义务:索引是消费者,**不得以合规问题拒绝文件**。坏 frontmatter → 字段置 NULL,正文照常索引。所以用自研浅层解析,而不是严格 YAML 库(后者会抛错,反而违约)。

**Files:**
- Create: `searchidx/src/frontmatter.rs`
- Modify: `searchidx/src/lib.rs`

**Interfaces:**
- Consumes: `norm::strip_cr`(调用方保证传入已剥 `\r` 的文本)
- Produces:
  - `pub struct Frontmatter { pub concept_type: Option<String>, pub title: Option<String>, pub tags: Vec<String>, pub created: Option<String>, pub date: Option<String>, pub generated_at: Option<String>, pub human_verified: bool }`
  - `searchidx::frontmatter::split(text: &str) -> (Option<&str>, &str, u32)` — `(raw_fm, body, body_start_line_1based)`
  - `searchidx::frontmatter::parse(raw: &str) -> Frontmatter`

- [ ] **Step 1: 写失败的测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_returns_body_and_its_first_line_number() {
        let (fm, body, line) = split("---\ntype: note\n---\nhello\n");
        assert_eq!(fm, Some("type: note"));
        assert_eq!(body, "hello\n");
        assert_eq!(line, 4);
    }

    #[test]
    fn split_without_frontmatter_starts_at_line_one() {
        let (fm, body, line) = split("# Title\n");
        assert_eq!(fm, None);
        assert_eq!(body, "# Title\n");
        assert_eq!(line, 1);
    }

    /// `---` 必须在第 0 字符。文中间出现的 `---` 是分隔线,不是 frontmatter。
    #[test]
    fn split_ignores_a_later_delimiter() {
        let (fm, _, line) = split("text\n---\na: b\n---\n");
        assert_eq!(fm, None);
        assert_eq!(line, 1);
    }

    #[test]
    fn parses_shallow_scalar_keys() {
        let f = parse("type: concept\ntitle: My Note\ncreated: 2026-08-10");
        assert_eq!(f.concept_type.as_deref(), Some("concept"));
        assert_eq!(f.title.as_deref(), Some("My Note"));
        assert_eq!(f.created.as_deref(), Some("2026-08-10"));
    }

    #[test]
    fn parses_inline_and_block_tag_lists() {
        assert_eq!(parse("tags: [a, b]").tags, vec!["a", "b"]);
        assert_eq!(parse("tags:\n  - a\n  - b").tags, vec!["a", "b"]);
        assert_eq!(parse("tags: a").tags, vec!["a"]);
    }

    #[test]
    fn reads_generated_at_from_the_nested_generated_block() {
        let f = parse("generated:\n  by: claude/1\n  at: 2026-08-01T10:00:00Z");
        assert_eq!(f.generated_at.as_deref(), Some("2026-08-01T10:00:00Z"));
    }

    /// OKF §7:人工确认必须用 `human:` 前缀。§11:裸 mapping 当单元素列表处理。
    #[test]
    fn human_verified_accepts_both_a_bare_mapping_and_a_list() {
        assert!(parse("verified:\n  by: human:me\n  at: 2026-08-01").human_verified);
        assert!(parse("verified:\n  - by: human:me").human_verified);
        assert!(!parse("verified:\n  - by: claude/1").human_verified);
        assert!(!parse("title: x").human_verified);
    }

    /// 宽容义务:坏 frontmatter 不得让文件消失。
    #[test]
    fn malformed_frontmatter_yields_empty_fields_not_an_error() {
        let f = parse("type: [unclosed\n\t\tgarbage: : :\n%%%");
        assert_eq!(f.title, None);
        assert!(f.tags.is_empty());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml frontmatter`
Expected: 编译失败。

- [ ] **Step 3: 写实现**

```rust
//! Tolerant, shallow frontmatter reading.
//!
//! OKF v0.2 §11 puts a *consumer* obligation on us: a missing optional field, an
//! unknown `type`, unknown extra keys, a broken block — none of them may cause
//! the document to be rejected. A strict YAML library does the opposite: it
//! raises on malformed input, which would make a typo delete a file from search.
//! So this is a hand-rolled reader for a fixed set of shallow keys, and every
//! failure path degrades to `None` rather than to an error.

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Frontmatter {
    pub concept_type: Option<String>,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub created: Option<String>,
    pub date: Option<String>,
    pub generated_at: Option<String>,
    /// True when a `verified` entry carries a `by:` with the OKF `human:` prefix.
    pub human_verified: bool,
}

/// `(raw_frontmatter, body, 1-based line number of the body's first line)`.
/// The delimiter must start at byte 0 — a `---` further down is a horizontal
/// rule, not a header.
pub fn split(text: &str) -> (Option<&str>, &str, u32) {
    if !text.starts_with("---\n") {
        return (None, text, 1);
    }
    let rest = &text[4..];
    let Some(end) = find_closing_delimiter(rest) else {
        return (None, text, 1);
    };
    let raw = rest[..end].trim_end_matches('\n');
    let after = &rest[end..];
    let body = after.strip_prefix("---\n").unwrap_or_else(|| after.strip_prefix("---").unwrap_or(after));
    // 1 (opening ---) + frontmatter lines + 1 (closing ---) + 1 = first body line
    let fm_lines = rest[..end].matches('\n').count() as u32;
    (Some(raw), body, fm_lines + 3)
}

fn find_closing_delimiter(rest: &str) -> Option<usize> {
    let mut offset = 0usize;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

pub fn parse(raw: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    let mut lines = raw.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((key, value)) = split_key(line) else { continue };
        // Only column-0 keys are read; nested keys are consumed by their block.
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        match key {
            "type" => fm.concept_type = scalar(value),
            "title" => fm.title = scalar(value),
            "created" => fm.created = scalar(value),
            "date" => fm.date = scalar(value),
            "tags" => {
                fm.tags = if value.trim().is_empty() {
                    collect_block(&mut lines).into_iter().filter_map(|v| scalar(&v)).collect()
                } else {
                    parse_inline_list(value)
                };
            }
            "generated" => {
                for entry in collect_block(&mut lines) {
                    if let Some(("at", v)) = split_key(entry.trim()) {
                        fm.generated_at = scalar(v);
                    }
                }
            }
            "verified" => {
                for entry in collect_block(&mut lines) {
                    if let Some(("by", v)) = split_key(entry.trim().trim_start_matches("- ")) {
                        if scalar(v).is_some_and(|s| s.starts_with("human:")) {
                            fm.human_verified = true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    fm
}

/// Consume the indented (or `- `-prefixed) lines that belong to the key we just
/// read. Handles both `verified: {by: ...}` written as a bare mapping and as a
/// one-element list — OKF §11 says a bare mapping MUST be treated as a
/// single-element list, so both shapes land in the same `Vec`.
fn collect_block<'a>(lines: &mut std::iter::Peekable<impl Iterator<Item = &'a str>>) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(next) = lines.peek() {
        if next.trim().is_empty() || !next.starts_with(char::is_whitespace) {
            break;
        }
        out.push(lines.next().unwrap().to_string());
    }
    out
}

fn split_key(line: &str) -> Option<(&str, &str)> {
    let (k, v) = line.split_once(':')?;
    let k = k.trim().trim_start_matches("- ").trim();
    if k.is_empty() || k.contains(char::is_whitespace) {
        return None;
    }
    Some((k, v))
}

fn scalar(value: &str) -> Option<String> {
    let v = value.trim().trim_matches('"').trim_matches('\'').trim();
    if v.is_empty() { None } else { Some(v.to_string()) }
}

fn parse_inline_list(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .filter_map(scalar)
        .collect()
}
```

在 `lib.rs` 加 `pub mod frontmatter;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path searchidx/Cargo.toml frontmatter`
Expected: 8 个测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/frontmatter.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): tolerant shallow frontmatter reader (OKF consumer obligation)"
```

---

### Task 4: 块模型 + 散文 `.md` 分块

spec §3.6 后半 + §3.3「块三分辨率」。段落/标题/代码围栏各成一个 line 级块,再派生 section 级与 file 级块。breadcrumb 在索引期派生,**不写回文件**。markdown 的边界情况(嵌套围栏、HTML 块、setext 标题)交给 `pulldown-cmark`,我们只做偏移 → 行号。

**Files:**
- Create: `searchidx/src/block.rs`
- Create: `searchidx/src/prose.rs`
- Modify: `searchidx/src/lib.rs`

**Interfaces:**
- Consumes: `norm::{line_starts, line_of}`、`frontmatter::split`
- Produces:
  - `pub enum BlockLevel { File, Section, Line }`(`as_str()` → `"file"|"section"|"line"`)
  - `pub struct Block { pub line_start: u32, pub line_end: u32, pub breadcrumb: String, pub text: String, pub level: BlockLevel, pub is_annotation: bool, pub agent_by: Option<String> }`
  - `pub struct FileMeta { pub title: Option<String>, pub concept_type: Option<String>, pub tags: Vec<String>, pub doc_date: Option<String>, pub date_inferred: bool, pub human_verified: bool }`
  - `pub struct Link { pub kind: String, pub target: String, pub line: u32 }`
  - `searchidx::prose::chunk(body: &str, body_start_line: u32) -> Vec<Block>`

- [ ] **Step 1: 写失败的测试**

`searchidx/src/prose.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(blocks: &[Block], level: BlockLevel) -> Vec<(u32, u32)> {
        blocks.iter().filter(|b| b.level == level).map(|b| (b.line_start, b.line_end)).collect()
    }

    #[test]
    fn paragraphs_headings_and_fences_become_line_blocks() {
        let md = "# Title\n\npara one\n\n```rs\nlet x = 1;\n```\n";
        let b = chunk(md, 1);
        let texts: Vec<&str> = b.iter().filter(|x| x.level == BlockLevel::Line).map(|x| x.text.as_str()).collect();
        assert!(texts.iter().any(|t| t.contains("para one")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("let x = 1;")), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("Title")), "{texts:?}");
    }

    /// 行号必须落在源文本的真实行上——这是 `path:line:text` 与 `#L120` 回源锚
    /// 的全部价值所在,错一行就等于骗了 agent。
    #[test]
    fn line_numbers_point_at_the_real_source_lines() {
        let md = "# T\n\nalpha\n\nbeta\n";
        let b = chunk(md, 1);
        let alpha = b.iter().find(|x| x.text.contains("alpha")).unwrap();
        assert_eq!((alpha.line_start, alpha.line_end), (3, 3));
        let beta = b.iter().find(|x| x.text.contains("beta")).unwrap();
        assert_eq!((beta.line_start, beta.line_end), (5, 5));
    }

    /// frontmatter 之后的正文要按它在**整个文件**里的行号编号。
    #[test]
    fn body_start_line_offsets_every_block() {
        let b = chunk("alpha\n", 4);
        assert_eq!(lines_of(&b, BlockLevel::Line), vec![(4, 4)]);
    }

    #[test]
    fn breadcrumb_is_the_heading_chain() {
        let md = "# A\n\n## B\n\ntext\n";
        let b = chunk(md, 1);
        let t = b.iter().find(|x| x.text.contains("text")).unwrap();
        assert_eq!(t.breadcrumb, "A > B");
    }

    /// 面包屑每级截 40 字,避免长标题把 breadcrumb 撑爆(spec §3.6)。
    #[test]
    fn breadcrumb_truncates_each_level_to_40_chars() {
        let long = "x".repeat(60);
        let md = format!("# {long}\n\ntext\n");
        let b = chunk(&md, 1);
        let t = b.iter().find(|x| x.text.contains("text")).unwrap();
        assert_eq!(t.breadcrumb.chars().count(), 40);
    }

    #[test]
    fn section_and_file_level_blocks_are_derived() {
        let md = "# A\n\nalpha\n\n# B\n\nbeta\n";
        let b = chunk(md, 1);
        assert_eq!(lines_of(&b, BlockLevel::File), vec![(1, 7)]);
        let sections = lines_of(&b, BlockLevel::Section);
        assert_eq!(sections.len(), 2, "{sections:?}");
        assert_eq!(sections[0], (1, 4));
    }

    /// 无标题的纯正文也必须有 file 级块,否则「这文档讲什么」类查询召不回。
    #[test]
    fn a_file_without_headings_still_gets_a_file_block() {
        let b = chunk("just text\n", 1);
        assert_eq!(lines_of(&b, BlockLevel::File), vec![(1, 1)]);
    }

    #[test]
    fn an_empty_body_yields_no_blocks() {
        assert!(chunk("", 1).is_empty());
        assert!(chunk("   \n\n", 1).iter().all(|b| !b.text.trim().is_empty() || b.level == BlockLevel::File));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml prose`
Expected: 编译失败。

- [ ] **Step 3: 写 `block.rs`**

```rust
//! The data model every chunker produces and the store consumes.
//!
//! Three resolutions per file (design spec §3.3): a `Line` block for "find me
//! that exact sentence", a `Section` block for "what does this section argue",
//! a `File` block for "what is this document about". Matching the granularity
//! of the question is what makes retrieval both fast and precise.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockLevel {
    File,
    Section,
    Line,
}

impl BlockLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockLevel::File => "file",
            BlockLevel::Section => "section",
            BlockLevel::Line => "line",
        }
    }
    pub fn from_str(s: &str) -> BlockLevel {
        match s {
            "file" => BlockLevel::File,
            "section" => BlockLevel::Section,
            _ => BlockLevel::Line,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    /// 1-based, inclusive.
    pub line_start: u32,
    pub line_end: u32,
    /// Ancestor chain derived at index time. Never written back to the file —
    /// we take the self-containment benefit without polluting the vault.
    pub breadcrumb: String,
    pub text: String,
    pub level: BlockLevel,
    /// `type:: annotation` or `type:: question` on an outline node.
    pub is_annotation: bool,
    /// The `by::` value when it is NOT a `human:` actor — i.e. an AI author.
    pub agent_by: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FileMeta {
    pub title: Option<String>,
    pub concept_type: Option<String>,
    pub tags: Vec<String>,
    /// `YYYY-MM-DD`.
    pub doc_date: Option<String>,
    /// True when `doc_date` came from mtime rather than the name/frontmatter.
    pub date_inferred: bool,
    pub human_verified: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// `"wiki"` or `"md"`.
    pub kind: String,
    pub target: String,
    pub line: u32,
}

/// Join breadcrumb levels, truncating each to 40 chars (spec §3.6).
pub fn breadcrumb_of(levels: &[String]) -> String {
    levels
        .iter()
        .map(|l| l.chars().take(40).collect::<String>())
        .collect::<Vec<_>>()
        .join(" > ")
}
```

- [ ] **Step 4: 写 `prose.rs`**

```rust
//! Prose `.md` chunking via pulldown-cmark's `OffsetIter`.
//!
//! Markdown's edge cases — nested fences, HTML blocks, setext headings, lazy
//! continuation — are exactly the sort of thing a hand-rolled scanner gets
//! subtly wrong, and a subtly wrong chunker produces subtly wrong line anchors.
//! So the boundaries come from the de-facto standard parser and the only thing
//! we do ourselves is map byte offsets back to 1-based line numbers.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::block::{breadcrumb_of, Block, BlockLevel};
use crate::norm::{line_of, line_starts};

/// Chunk a prose body. `body_start_line` is the 1-based line the body begins on
/// in the whole file (4 when a 2-line frontmatter precedes it, 1 otherwise).
pub fn chunk(body: &str, body_start_line: u32) -> Vec<Block> {
    if body.trim().is_empty() {
        return Vec::new();
    }
    let starts = line_starts(body);
    let offset = body_start_line - 1;
    let line_at = |byte: usize| line_of(&starts, byte) + offset;

    let mut blocks: Vec<Block> = Vec::new();
    // Heading text per level (index 0 = h1), truncated when a shallower
    // heading arrives.
    let mut chain: Vec<String> = Vec::new();
    // (start_line, breadcrumb_at_open, depth) of the section currently open.
    let mut open_section: Option<(u32, String, usize)> = None;
    let mut sections: Vec<Block> = Vec::new();

    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_FOOTNOTES;
    let mut heading_depth: Option<usize> = None;
    let mut pending: Option<(u32, u32, String)> = None; // start, end, text

    for (event, range) in Parser::new_ext(body, opts).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let depth = heading_index(level);
                // Close the section this heading terminates.
                if let Some((start, crumb, d)) = open_section.take() {
                    if depth <= d {
                        sections.push(section_block(start, line_at(range.start) - 1, crumb, body, &starts, offset));
                    } else {
                        open_section = Some((start, crumb, d));
                    }
                }
                heading_depth = Some(depth);
                pending = Some((line_at(range.start), line_at(range.end.saturating_sub(1)), String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((s, e, text)) = pending.take() {
                    let depth = heading_depth.take().unwrap_or(0);
                    chain.truncate(depth);
                    chain.push(text.trim().to_string());
                    blocks.push(Block {
                        line_start: s,
                        line_end: e,
                        breadcrumb: breadcrumb_of(&chain[..chain.len().saturating_sub(1)]),
                        text: text.trim().to_string(),
                        level: BlockLevel::Line,
                        is_annotation: false,
                        agent_by: None,
                    });
                    if open_section.is_none() {
                        open_section = Some((s, breadcrumb_of(&chain), depth));
                    }
                }
            }
            Event::Start(Tag::Paragraph) | Event::Start(Tag::CodeBlock(_)) | Event::Start(Tag::Item) => {
                if pending.is_none() {
                    pending = Some((line_at(range.start), line_at(range.end.saturating_sub(1)), String::new()));
                }
            }
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::CodeBlock) | Event::End(TagEnd::Item) => {
                if heading_depth.is_some() {
                    continue;
                }
                if let Some((s, e, text)) = pending.take() {
                    if !text.trim().is_empty() {
                        blocks.push(Block {
                            line_start: s,
                            line_end: e,
                            breadcrumb: breadcrumb_of(&chain),
                            text: text.trim().to_string(),
                            level: BlockLevel::Line,
                            is_annotation: false,
                            agent_by: None,
                        });
                    }
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, _, ref mut text)) = pending {
                    text.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, _, ref mut text)) = pending {
                    text.push('\n');
                }
            }
            _ => {}
        }
    }

    let last_line = line_at(body.len().saturating_sub(1));
    if let Some((start, crumb, _)) = open_section.take() {
        sections.push(section_block(start, last_line, crumb, body, &starts, offset));
    }
    blocks.extend(sections);

    blocks.push(Block {
        line_start: body_start_line,
        line_end: last_line,
        breadcrumb: String::new(),
        text: body.trim().to_string(),
        level: BlockLevel::File,
        is_annotation: false,
        agent_by: None,
    });
    blocks
}

fn heading_index(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 0,
        HeadingLevel::H2 => 1,
        HeadingLevel::H3 => 2,
        HeadingLevel::H4 => 3,
        HeadingLevel::H5 => 4,
        HeadingLevel::H6 => 5,
    }
}

fn section_block(
    start: u32,
    end: u32,
    breadcrumb: String,
    body: &str,
    starts: &[usize],
    offset: u32,
) -> Block {
    let from = starts.get((start - offset - 1) as usize).copied().unwrap_or(0);
    let to = starts
        .get((end - offset) as usize)
        .copied()
        .unwrap_or(body.len())
        .min(body.len());
    Block {
        line_start: start,
        line_end: end.max(start),
        breadcrumb,
        text: body[from..to].trim().to_string(),
        level: BlockLevel::Section,
        is_annotation: false,
        agent_by: None,
    }
}
```

在 `lib.rs` 加 `pub mod block;` 与 `pub mod prose;`。

> **给实现者的提醒:** `pulldown-cmark` 0.13 的事件形状按上面写就能跑,但如果某个断言差一行,**先打印 `into_offset_iter()` 的真实 range 再改**,不要凭直觉调 `+1/-1`。`range.end` 通常包含块尾的换行,所以 `line_end` 用 `range.end - 1`。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --manifest-path searchidx/Cargo.toml prose -- --nocapture`
Expected: 8 个测试 PASS。

- [ ] **Step 6: 提交**

```bash
git add searchidx/src/block.rs searchidx/src/prose.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): three-resolution block model + prose chunker via pulldown-cmark"
```

---

### Task 5: `.note.md` 大纲分块 + 跨语言 fixtures

spec §3.6 前半。块=大纲节点,breadcrumb=祖先链,属性行**不进** `tok_text`(它们是元数据,不是内容),`type:: annotation|question` → `is_annotation`,`by::` 不匹配 `^human:` → `agent_by`(spec §3.5:`✦`/`●` 是渲染物不可靠,机器可判的是属性行)。

已存在两份大纲解析器(TS `src/lib/outline/markdown.ts`、Rust `plugins-src/roam-import/backend/src/outline.rs`)。本任务写的是**第三份、只读、只关心行归属**的简化版:不建节点 id、不排序、不序列化。防漂移的手段是共享 fixtures——同一批文件,Rust 与 TS 必须给出同样的「第 N 行属于哪个节点」。

**Files:**
- Create: `searchidx/src/outline.rs`
- Create: `searchidx/tests/fixtures/outline/basic.note.md`
- Create: `searchidx/tests/fixtures/outline/fenced-answer.note.md`
- Create: `searchidx/tests/fixtures/outline/crlf.note.md`
- Create: `searchidx/tests/fixtures/outline/expected.json`
- Create: `src/lib/outline/cross-lang-fixtures.test.ts`
- Modify: `searchidx/src/lib.rs`

**Interfaces:**
- Consumes: `block::*`、`norm::strip_cr`
- Produces: `searchidx::outline::chunk(body: &str, body_start_line: u32) -> Vec<Block>`

- [ ] **Step 1: 写 fixtures**

`searchidx/tests/fixtures/outline/basic.note.md`:

```markdown
- 顶层节点
  - 子节点一
    type:: annotation
    by:: claude/1
  - 子节点二
    by:: human:bruce
- 另一个顶层
```

`searchidx/tests/fixtures/outline/fenced-answer.note.md`:

```markdown
- 问题节点
  type:: question
  status:: open
  - ✦ 答复
    type:: answer
    ```
    围栏内的一行
    - 这行看起来像 bullet 但不是
    ```
```

`searchidx/tests/fixtures/outline/crlf.note.md` —— 与 `basic.note.md` 内容相同但用 CRLF 写入:

```bash
python3 - <<'PY'
import pathlib
src = pathlib.Path('searchidx/tests/fixtures/outline/basic.note.md').read_bytes()
pathlib.Path('searchidx/tests/fixtures/outline/crlf.note.md').write_bytes(src.replace(b'\n', b'\r\n'))
PY
```

`searchidx/tests/fixtures/outline/expected.json` —— **两种语言共同的真值**。每个 fixture 一个数组,元素是 `{ "line_start": N, "line_end": N, "breadcrumb": "...", "text": "...", "is_annotation": bool, "agent_by": null|"..." }`。先按下面的形状手写 `basic`,另两个在 Step 4 跑出实际值后**核对无误再固化**:

```json
{
  "basic.note.md": [
    { "line_start": 1, "line_end": 1, "breadcrumb": "", "text": "顶层节点", "is_annotation": false, "agent_by": null },
    { "line_start": 2, "line_end": 4, "breadcrumb": "顶层节点", "text": "子节点一", "is_annotation": true, "agent_by": "claude/1" },
    { "line_start": 5, "line_end": 6, "breadcrumb": "顶层节点", "text": "子节点二", "is_annotation": false, "agent_by": null },
    { "line_start": 7, "line_end": 7, "breadcrumb": "", "text": "另一个顶层", "is_annotation": false, "agent_by": null }
  ]
}
```

- [ ] **Step 2: 写失败的测试**

`searchidx/src/outline.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn nodes(md: &str) -> Vec<Block> {
        chunk(&crate::norm::strip_cr(md), 1)
    }

    #[test]
    fn each_bullet_becomes_a_block_spanning_its_property_lines() {
        let b = nodes("- alpha\n  type:: annotation\n- beta\n");
        assert_eq!(b.len(), 2);
        assert_eq!((b[0].line_start, b[0].line_end), (1, 2));
        assert_eq!(b[0].text, "alpha");
        assert_eq!((b[1].line_start, b[1].line_end), (3, 3));
    }

    /// 属性行是元数据,不是内容:它们决定 is_annotation/agent_by,但绝不进
    /// 可检索文本,否则每个节点都会被 "type" "annotation" 这类噪音词污染。
    #[test]
    fn property_lines_are_metadata_not_searchable_text() {
        let b = nodes("- alpha\n  type:: annotation\n  by:: claude/1\n");
        assert_eq!(b[0].text, "alpha");
        assert!(b[0].is_annotation);
        assert_eq!(b[0].agent_by.as_deref(), Some("claude/1"));
    }

    /// spec §3.5:人写信号靠**前缀匹配**,不写死某个 id。
    #[test]
    fn a_human_actor_is_not_recorded_as_an_agent_author() {
        let b = nodes("- alpha\n  by:: human:bruce\n");
        assert_eq!(b[0].agent_by, None);
    }

    #[test]
    fn breadcrumb_is_the_ancestor_chain() {
        let b = nodes("- top\n  - mid\n    - leaf\n");
        assert_eq!(b[2].text, "leaf");
        assert_eq!(b[2].breadcrumb, "top > mid");
    }

    /// 围栏内的 `- ` 不是 bullet。答复正文里带列表是常态,切错就等于把一条
    /// 答复劈成几个假节点。
    #[test]
    fn bullets_inside_an_answer_fence_are_content_not_nodes() {
        let md = "- q\n  - a\n    ```\n    - not a bullet\n    ```\n";
        let b = nodes(md);
        assert_eq!(b.len(), 2, "{:?}", b.iter().map(|x| &x.text).collect::<Vec<_>>());
        assert!(b[1].text.contains("not a bullet"));
    }

    #[test]
    fn question_nodes_count_as_annotations() {
        let b = nodes("- ?\n  type:: question\n");
        assert!(b[0].is_annotation);
    }

    #[test]
    fn a_file_level_block_is_derived_for_the_whole_outline() {
        let b = nodes("- a\n- b\n");
        assert!(b.iter().any(|x| x.level == crate::block::BlockLevel::File));
    }
}
```

再建 `searchidx/tests/outline_fixtures.rs`:

```rust
//! The Rust chunker and the TypeScript `parseOutline` must agree on which line
//! belongs to which node. Two implementations of one format drift silently, and
//! a drift here means the same `.note.md` is a different tree depending on who
//! read it — unacceptable under "one vault, many agents". So both sides are
//! pinned to the same fixture files and the same expected JSON.
//! The TS half lives in src/lib/outline/cross-lang-fixtures.test.ts.

use std::path::Path;

#[test]
fn rust_chunker_matches_the_shared_fixture_expectations() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/outline");
    let expected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("expected.json")).unwrap()).unwrap();

    for (name, want) in expected.as_object().unwrap() {
        let raw = std::fs::read_to_string(dir.join(name)).unwrap();
        let text = searchidx::norm::strip_cr(&raw);
        let got: Vec<serde_json::Value> = searchidx::outline::chunk(&text, 1)
            .into_iter()
            .filter(|b| b.level == searchidx::block::BlockLevel::Line)
            .map(|b| {
                serde_json::json!({
                    "line_start": b.line_start, "line_end": b.line_end,
                    "breadcrumb": b.breadcrumb, "text": b.text,
                    "is_annotation": b.is_annotation, "agent_by": b.agent_by,
                })
            })
            .collect();
        assert_eq!(&serde_json::Value::Array(got), want, "fixture {name} diverged");
    }
}
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml outline`
Expected: 编译失败(`chunk` 未定义)。

- [ ] **Step 4: 写实现**

```rust
//! `.note.md` (sidecar note) chunking: one block per outline node.
//!
//! This is a THIRD implementation of the outline format — TypeScript owns
//! `src/lib/outline/markdown.ts`, the roam-import plugin owns a Rust port. This
//! one is read-only and only cares about line attribution, so it stays small;
//! the shared fixtures in tests/fixtures/outline are what stop the three from
//! drifting. See tests/outline_fixtures.rs.

use crate::block::{breadcrumb_of, Block, BlockLevel};

/// Property lines like `type:: question`. Same key set as the TS parser.
fn property(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    let (k, v) = t.split_once(":: ")?;
    matches!(k, "type" | "line" | "id" | "collapsed" | "created" | "updated" | "status" | "answered" | "by")
        .then(|| (k, v.trim()))
}

fn bullet(line: &str) -> Option<(usize, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = line.trim_start().strip_prefix("- ")?;
    Some((indent / 2, rest))
}

fn fence_len(line: &str) -> Option<usize> {
    let t = line.trim_start();
    let n = t.chars().take_while(|c| *c == '`').count();
    (n >= 3).then_some(n)
}

/// Chunk an outline body. `body_start_line` is the 1-based line the body starts
/// on within the whole file.
pub fn chunk(body: &str, body_start_line: u32) -> Vec<Block> {
    let lines: Vec<&str> = body.lines().collect();
    let mut blocks: Vec<Block> = Vec::new();
    let mut chain: Vec<String> = Vec::new();
    // index into `blocks` of the node currently accumulating lines
    let mut current: Option<usize> = None;
    let mut fence = 0usize;

    for (i, raw) in lines.iter().enumerate() {
        let line_no = body_start_line + i as u32;

        if fence > 0 {
            if fence_len(raw).is_some_and(|n| n >= fence) {
                fence = 0;
            }
            if let Some(idx) = current {
                blocks[idx].line_end = line_no;
                blocks[idx].text.push('\n');
                blocks[idx].text.push_str(raw.trim_start());
            }
            continue;
        }
        if let Some(n) = fence_len(raw) {
            fence = n;
            if let Some(idx) = current {
                blocks[idx].line_end = line_no;
            }
            continue;
        }

        if let Some((depth, content)) = bullet(raw) {
            chain.truncate(depth);
            let breadcrumb = breadcrumb_of(&chain);
            chain.push(content.to_string());
            blocks.push(Block {
                line_start: line_no,
                line_end: line_no,
                breadcrumb,
                text: content.to_string(),
                level: BlockLevel::Line,
                is_annotation: false,
                agent_by: None,
            });
            current = Some(blocks.len() - 1);
            continue;
        }

        let Some(idx) = current else { continue };
        blocks[idx].line_end = line_no;
        if let Some((key, value)) = property(raw) {
            match key {
                "type" if value == "annotation" || value == "question" => {
                    blocks[idx].is_annotation = true;
                }
                // `human:` prefix, not a hardcoded id: OKF §7 makes the prefix
                // the machine-checkable signal for "a person stands behind this".
                "by" if !value.starts_with("human:") => {
                    blocks[idx].agent_by = Some(value.to_string());
                }
                _ => {}
            }
            continue;
        }
        if !raw.trim().is_empty() {
            blocks[idx].text.push('\n');
            blocks[idx].text.push_str(raw.trim_start());
        }
    }

    if !blocks.is_empty() {
        let last = blocks.iter().map(|b| b.line_end).max().unwrap_or(body_start_line);
        blocks.push(Block {
            line_start: body_start_line,
            line_end: last,
            breadcrumb: String::new(),
            text: body.trim().to_string(),
            level: BlockLevel::File,
            is_annotation: false,
            agent_by: None,
        });
    }
    blocks
}
```

在 `lib.rs` 加 `pub mod outline;`。

- [ ] **Step 5: 跑测试,把实际值核对后固化进 expected.json**

Run: `cargo test --manifest-path searchidx/Cargo.toml outline -- --nocapture`

单测应全 PASS。`outline_fixtures` 里 `fenced-answer` / `crlf` 还没有期望值 —— 用一个临时 `--nocapture` 打印把实际输出取出来,**逐行人工核对**(尤其围栏那条:围栏内的 `- not a bullet` 必须留在答复节点里,不能变成新节点),确认无误后写进 `expected.json`。`crlf.note.md` 的期望值必须与 `basic.note.md` **逐字相同**——这就是 CRLF 规范化的验收。

Expected: `cargo test --manifest-path searchidx/Cargo.toml` 全绿。

- [ ] **Step 6: 写 TS 侧交叉验证**

`src/lib/outline/cross-lang-fixtures.test.ts`:

```ts
// The Rust chunker (searchidx/src/outline.rs) and this parser read the same
// files. They are pinned to the same fixtures so that a change to either one
// that moves a line from one node to another fails loudly, instead of silently
// giving two agents two different trees for one .note.md.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { parseOutline } from './markdown'
import { childrenOf } from './model'

const DIR = join(__dirname, '../../../searchidx/tests/fixtures/outline')
const expected = JSON.parse(readFileSync(join(DIR, 'expected.json'), 'utf8')) as
  Record<string, Array<{ line_start: number; text: string; breadcrumb: string }>>

/** Depth-first node order, matching the Rust chunker's emission order. */
function flatten(tree: ReturnType<typeof parseOutline>, parentId: string | null, trail: string[]) {
  const out: Array<{ text: string; breadcrumb: string }> = []
  for (const n of childrenOf(tree, parentId)) {
    const first = n.content.split('\n')[0]
    out.push({ text: n.content, breadcrumb: trail.map((s) => s.slice(0, 40)).join(' > ') })
    out.push(...flatten(tree, n.id, [...trail, first]))
  }
  return out
}

describe('outline fixtures agree across Rust and TypeScript', () => {
  for (const name of Object.keys(expected)) {
    it(`${name}: node text and breadcrumbs match the shared expectations`, () => {
      const tree = parseOutline(readFileSync(join(DIR, name), 'utf8'))
      const got = flatten(tree, null, [])
      expect(got.map((n) => n.text)).toEqual(expected[name].map((e) => e.text))
      expect(got.map((n) => n.breadcrumb)).toEqual(expected[name].map((e) => e.breadcrumb))
    })
  }
})
```

> **若 TS 与 Rust 在某条 fixture 上分歧:** 先判断哪边对(以 `markdown.ts` 为准,它是产品行为的事实源),修 Rust 侧。spec §11 已声明分块漂移只影响 breadcrumb 粒度、不影响可检性 —— 所以**不要**为了对齐去改 `markdown.ts` 的行为,那会动到真实用户的笔记树。

- [ ] **Step 7: 跑前端测试**

Run: `pnpm test -- cross-lang-fixtures`
Expected: PASS。

- [ ] **Step 8: 提交**

```bash
git add searchidx/src/outline.rs searchidx/src/lib.rs searchidx/tests/outline_fixtures.rs searchidx/tests/fixtures/outline src/lib/outline/cross-lang-fixtures.test.ts
git commit -m "feat(searchidx): .note.md outline chunker pinned to shared cross-language fixtures"
```

---

### Task 6: 链接抽取 + 分块分派

`links` 表按 spec §3.3 建;当前不消费(反链层不动,spec §2 明确说"远期改读本索引 links 表"),但**现在就把数据存下来**,否则将来加 `page:[[X]]` 过滤器要全量重建。`chunk.rs` 是唯一的分派点:按后缀选 prose/outline,合并 frontmatter 派生的 `FileMeta`。

**Files:**
- Create: `searchidx/src/links.rs`
- Create: `searchidx/src/chunk.rs`
- Modify: `searchidx/src/lib.rs`

**Interfaces:**
- Consumes: `block::*`、`frontmatter::*`、`prose::chunk`、`outline::chunk`、`norm::strip_cr`
- Produces:
  - `searchidx::links::extract(body: &str, body_start_line: u32) -> Vec<Link>`
  - `searchidx::chunk::parse_file(rel_path: &str, raw: &str, mtime_secs: i64) -> Parsed`
  - `pub struct Parsed { pub meta: FileMeta, pub blocks: Vec<Block>, pub links: Vec<Link> }`

- [ ] **Step 1: 写失败的测试**

`searchidx/src/links.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_wikilinks_with_their_line() {
        let l = extract("see [[Target Page]] here\n", 1);
        assert_eq!(l, vec![Link { kind: "wiki".into(), target: "Target Page".into(), line: 1 }]);
    }

    /// 别名形式 `[[target|display]]` 的目标是竖线**前面**那半。
    #[test]
    fn wikilink_alias_keeps_only_the_target() {
        let l = extract("[[a/b|Display]]\n", 1);
        assert_eq!(l[0].target, "a/b");
    }

    #[test]
    fn extracts_markdown_links_but_not_images() {
        let l = extract("[text](./a.md)\n![img](./p.png)\n", 1);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].kind, "md");
        assert_eq!(l[0].target, "./a.md");
    }

    #[test]
    fn line_numbers_are_offset_by_body_start() {
        let l = extract("x\n[[T]]\n", 4);
        assert_eq!(l[0].line, 5);
    }

    #[test]
    fn a_body_without_links_yields_nothing() {
        assert!(extract("plain text [not a link\n", 1).is_empty());
    }
}
```

`searchidx/src/chunk.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const MTIME: i64 = 1_754_784_000; // 2026-08-10T00:00:00Z

    #[test]
    fn note_md_files_go_through_the_outline_chunker() {
        let p = parse_file("a.note.md", "- alpha\n  - beta\n", MTIME);
        assert!(p.blocks.iter().any(|b| b.text == "beta" && b.breadcrumb == "alpha"));
    }

    #[test]
    fn plain_md_files_go_through_the_prose_chunker() {
        let p = parse_file("a.md", "# T\n\npara\n", MTIME);
        assert!(p.blocks.iter().any(|b| b.text == "para"));
    }

    /// spec §3.5 的降级链,顺序不能反:文件名 → frontmatter → mtime。
    #[test]
    fn doc_date_prefers_the_filename_prefix() {
        let p = parse_file("2026-01-02-thing.md", "---\ncreated: 2020-05-05\n---\nx\n", MTIME);
        assert_eq!(p.meta.doc_date.as_deref(), Some("2026-01-02"));
        assert!(!p.meta.date_inferred);
    }

    #[test]
    fn doc_date_falls_back_to_frontmatter_then_to_mtime() {
        let p = parse_file("thing.md", "---\ncreated: 2020-05-05\n---\nx\n", MTIME);
        assert_eq!(p.meta.doc_date.as_deref(), Some("2020-05-05"));
        assert!(!p.meta.date_inferred);

        let p = parse_file("thing.md", "x\n", MTIME);
        assert_eq!(p.meta.doc_date.as_deref(), Some("2026-08-10"));
        assert!(p.meta.date_inferred, "mtime-derived dates must be flagged inferred");
    }

    #[test]
    fn title_falls_back_to_the_first_h1_then_to_the_stem() {
        assert_eq!(parse_file("a.md", "---\ntitle: FM\n---\n# H\n", MTIME).meta.title.as_deref(), Some("FM"));
        assert_eq!(parse_file("a.md", "# H\n", MTIME).meta.title.as_deref(), Some("H"));
        assert_eq!(parse_file("dir/my-note.md", "text\n", MTIME).meta.title.as_deref(), Some("my-note"));
    }

    /// CRLF 文件必须和 LF 文件产出逐字相同的块 —— 跨平台规约②。
    #[test]
    fn crlf_input_produces_identical_blocks_to_lf() {
        let lf = parse_file("a.md", "# T\n\npara\n", MTIME);
        let crlf = parse_file("a.md", "# T\r\n\r\npara\r\n", MTIME);
        let f = |p: &Parsed| p.blocks.iter().map(|b| (b.line_start, b.line_end, b.text.clone())).collect::<Vec<_>>();
        assert_eq!(f(&lf), f(&crlf));
    }

    /// 宽容义务:frontmatter 坏掉不影响正文进索引。
    #[test]
    fn a_broken_frontmatter_still_indexes_the_body() {
        let p = parse_file("a.md", "---\n[[[\n---\nbody text\n", MTIME);
        assert!(p.blocks.iter().any(|b| b.text.contains("body text")));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml links chunk`
Expected: 编译失败。

- [ ] **Step 3: 写 `links.rs`**

```rust
//! Link extraction. The rows are written now and read later: the backlink layer
//! keeps its own pipeline for this release (design spec §2 — one refactor at a
//! time), but a `links` table added later would force a full rebuild of every
//! user's index, so we pay the cheap write today.
//!
//! Wikilinks resolve BY FILENAME elsewhere in the product; we store the raw
//! target verbatim and leave resolution to the consumer.

use crate::block::Link;

pub fn extract(body: &str, body_start_line: u32) -> Vec<Link> {
    let mut out = Vec::new();
    for (i, line) in body.lines().enumerate() {
        let line_no = body_start_line + i as u32;
        collect_wiki(line, line_no, &mut out);
        collect_md(line, line_no, &mut out);
    }
    out
}

fn collect_wiki(line: &str, line_no: u32, out: &mut Vec<Link>) {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while let Some(start) = line[i..].find("[[") {
        let s = i + start + 2;
        let Some(len) = line[s..].find("]]") else { break };
        let raw = &line[s..s + len];
        let target = raw.split('|').next().unwrap_or(raw).trim();
        if !target.is_empty() {
            out.push(Link { kind: "wiki".into(), target: target.to_string(), line: line_no });
        }
        i = s + len + 2;
        if i >= bytes.len() { break }
    }
}

fn collect_md(line: &str, line_no: u32, out: &mut Vec<Link>) {
    let mut i = 0usize;
    while let Some(rel) = line[i..].find("](") {
        let open = i + rel;
        // `![alt](...)` is an image, not a document link.
        let is_image = line[..open].rfind('[').is_some_and(|b| b > 0 && line.as_bytes()[b - 1] == b'!');
        let s = open + 2;
        let Some(len) = line[s..].find(')') else { break };
        let target = line[s..s + len].split_whitespace().next().unwrap_or("").trim();
        if !is_image && !target.is_empty() && !line[..open].ends_with(']') {
            out.push(Link { kind: "md".into(), target: target.to_string(), line: line_no });
        }
        i = s + len + 1;
    }
}
```

- [ ] **Step 4: 写 `chunk.rs`**

```rust
//! The single dispatch point: bytes on disk → (file metadata, blocks, links).
//!
//! Everything upstream of the store goes through here, so there is exactly one
//! place where "how is a file turned into rows" is decided — which is what makes
//! `rebuild == incremental update` true by construction.

use crate::block::{Block, FileMeta, Link};
use crate::{frontmatter, links, norm, outline, prose};

pub struct Parsed {
    pub meta: FileMeta,
    pub blocks: Vec<Block>,
    pub links: Vec<Link>,
}

/// `rel_path` is the vault-relative, `/`-separated path. `mtime_secs` is the
/// last-modified time, used only as the final fallback for `doc_date`.
pub fn parse_file(rel_path: &str, raw: &str, mtime_secs: i64) -> Parsed {
    let text = norm::strip_cr(raw);
    let (fm_raw, body, body_line) = frontmatter::split(&text);
    let fm = fm_raw.map(frontmatter::parse).unwrap_or_default();

    let blocks = if rel_path.ends_with(".note.md") {
        outline::chunk(body, body_line)
    } else {
        prose::chunk(body, body_line)
    };

    let (doc_date, date_inferred) = resolve_doc_date(rel_path, &fm, mtime_secs);
    let meta = FileMeta {
        title: fm.title.clone().or_else(|| first_h1(body)).or_else(|| stem(rel_path)),
        concept_type: fm.concept_type.clone(),
        tags: fm.tags.clone(),
        doc_date,
        date_inferred,
        human_verified: fm.human_verified,
    };
    Parsed { meta, blocks, links: links::extract(body, body_line) }
}

/// Degradation chain from spec §3.5: filename prefix → frontmatter → mtime.
/// The filename wins because a dated filename is this vault's dominant
/// convention and is what the author actually meant by "when".
fn resolve_doc_date(rel_path: &str, fm: &frontmatter::Frontmatter, mtime_secs: i64) -> (Option<String>, bool) {
    if let Some(d) = filename_date(rel_path) {
        return (Some(d), false);
    }
    for candidate in [&fm.created, &fm.date, &fm.generated_at] {
        if let Some(v) = candidate.as_deref().and_then(ymd_prefix) {
            return (Some(v), false);
        }
    }
    (Some(ymd_from_unix(mtime_secs)), true)
}

fn filename_date(rel_path: &str) -> Option<String> {
    let name = rel_path.rsplit('/').next()?;
    ymd_prefix(name)
}

/// Accepts a leading `YYYY-MM-DD`, which covers both `2026-08-10-thing.md` and
/// an ISO timestamp like `2026-08-01T10:00:00Z`.
fn ymd_prefix(s: &str) -> Option<String> {
    let b = s.as_bytes();
    if b.len() < 10 { return None }
    let ok = b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-' && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-' && b[8..10].iter().all(u8::is_ascii_digit);
    ok.then(|| s[..10].to_string())
}

/// Civil-from-days (Howard Hinnant's algorithm). No chrono dependency for one
/// date conversion — the crate's dependency list is part of the binary budget.
fn ymd_from_unix(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn first_h1(body: &str) -> Option<String> {
    body.lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

fn stem(rel_path: &str) -> Option<String> {
    let name = rel_path.rsplit('/').next()?;
    let stem = name.strip_suffix(".note.md").or_else(|| name.strip_suffix(".md")).unwrap_or(name);
    (!stem.is_empty()).then(|| stem.to_string())
}
```

在 `lib.rs` 加 `pub mod links;` 与 `pub mod chunk;`。

- [ ] **Step 5: 跑测试确认通过**

Run: `cargo test --manifest-path searchidx/Cargo.toml`
Expected: 全绿(links 5 条 + chunk 8 条 + 之前的)。

- [ ] **Step 6: 提交**

```bash
git add searchidx/src/links.rs searchidx/src/chunk.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): link extraction + file parse dispatch with doc_date degradation chain"
```

---

### Task 7: SQLite schema、自愈打开、文件粒度幂等替换

spec §3.3。schema 用 spec 的 SQL 逐字落地(加 WAL/busy_timeout)。三条不可动的性质:标准 FTS 表(不是 external-content,否则 snippet 吐分词乱码)、`schema_version`/`tokenizer_id` 不符即删库重建、写入是**文件粒度的删除+插入**(这就是双进程免协调的数学前提)。

**Files:**
- Create: `searchidx/src/store.rs`
- Modify: `searchidx/src/lib.rs`

**Interfaces:**
- Consumes: `block::*`、`chunk::Parsed`、`tokenize::{tokenize, TOKENIZER_ID}`
- Produces:
  - `pub const SCHEMA_VERSION: i64 = 1;`
  - `searchidx::store::open(db_path: &Path, vault_root: &str) -> rusqlite::Result<Connection>` — 建库/校验/不符即删库重建
  - `searchidx::store::replace_file(tx: &Transaction, rel: &str, ext: &str, mtime: i64, size: i64, hash: &str, parsed: &Parsed) -> rusqlite::Result<()>`
  - `searchidx::store::remove_file(tx: &Transaction, rel: &str) -> rusqlite::Result<()>`
  - `searchidx::store::meta_get / meta_set`
  - `pub struct FileRow { pub path: String, pub mtime: i64, pub size: i64, pub content_hash: String }`
  - `searchidx::store::all_file_rows(conn: &Connection) -> rusqlite::Result<HashMap<String, FileRow>>`

- [ ] **Step 1: 写失败的测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::parse_file;

    fn tmp() -> (tempfile::TempDir, std::path::PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("index.db");
        (d, p)
    }

    fn write(conn: &mut Connection, rel: &str, text: &str) {
        let parsed = parse_file(rel, text, 1_754_784_000);
        let tx = conn.transaction().unwrap();
        replace_file(&tx, rel, "md", 1, text.len() as i64, "h1", &parsed).unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn open_creates_the_schema_and_stamps_meta() {
        let (_d, p) = tmp();
        let conn = open(&p, "/v").unwrap();
        assert_eq!(meta_get(&conn, "schema_version").as_deref(), Some(SCHEMA_VERSION.to_string().as_str()));
        assert_eq!(meta_get(&conn, "tokenizer_id").as_deref(), Some(crate::tokenize::TOKENIZER_ID));
        assert_eq!(meta_get(&conn, "vault_root").as_deref(), Some("/v"));
    }

    /// 索引是可弃派生物:版本不符不修,直接扔掉重建。自愈最简、没有半修好的库。
    #[test]
    fn a_stale_tokenizer_id_wipes_the_database() {
        let (_d, p) = tmp();
        {
            let mut conn = open(&p, "/v").unwrap();
            write(&mut conn, "a.md", "hello\n");
            meta_set(&conn, "tokenizer_id", "v0+something-else").unwrap();
        }
        let conn = open(&p, "/v").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0, "a tokenizer change must invalidate every stored token");
        assert_eq!(meta_get(&conn, "tokenizer_id").as_deref(), Some(crate::tokenize::TOKENIZER_ID));
    }

    #[test]
    fn a_stale_schema_version_wipes_the_database() {
        let (_d, p) = tmp();
        {
            let mut conn = open(&p, "/v").unwrap();
            write(&mut conn, "a.md", "hello\n");
            meta_set(&conn, "schema_version", "0").unwrap();
        }
        let conn = open(&p, "/v").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn a_corrupt_database_file_is_replaced_not_reported() {
        let (_d, p) = tmp();
        std::fs::write(&p, b"this is not a sqlite file at all").unwrap();
        let conn = open(&p, "/v").unwrap();
        assert_eq!(meta_get(&conn, "schema_version").as_deref(), Some("1"));
    }

    /// 免 IPC 收敛的数学前提:同一文件重复写入必须收敛到同一状态。
    #[test]
    fn replacing_a_file_is_idempotent() {
        let (_d, p) = tmp();
        let mut conn = open(&p, "/v").unwrap();
        write(&mut conn, "a.md", "# T\n\nalpha\n");
        let count = |c: &Connection| -> i64 { c.query_row("SELECT count(*) FROM blocks", [], |r| r.get(0)).unwrap() };
        let first = count(&conn);
        write(&mut conn, "a.md", "# T\n\nalpha\n");
        assert_eq!(count(&conn), first, "re-indexing must replace, never append");
        let files: i64 = conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap();
        assert_eq!(files, 1);
    }

    /// FTS 影子行必须跟着块一起走,否则删掉的内容还能被搜出来。
    #[test]
    fn removing_a_file_clears_its_blocks_and_fts_rows() {
        let (_d, p) = tmp();
        let mut conn = open(&p, "/v").unwrap();
        write(&mut conn, "a.md", "alpha unique-token\n");
        let tx = conn.transaction().unwrap();
        remove_file(&tx, "a.md").unwrap();
        tx.commit().unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM blocks_fts WHERE blocks_fts MATCH '\"unique-token\"'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn all_file_rows_returns_stat_data_for_sweeping() {
        let (_d, p) = tmp();
        let mut conn = open(&p, "/v").unwrap();
        write(&mut conn, "a.md", "x\n");
        let rows = all_file_rows(&conn).unwrap();
        assert_eq!(rows.get("a.md").unwrap().content_hash, "h1");
    }

    #[test]
    fn wal_and_busy_timeout_are_enabled_for_two_process_access() {
        let (_d, p) = tmp();
        let conn = open(&p, "/v").unwrap();
        let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml store`
Expected: 编译失败。

- [ ] **Step 3: 写实现**

```rust
//! Storage: schema, self-healing open, file-granular idempotent replacement.
//!
//! Two processes (GUI and CLI) write this database with no IPC between them.
//! There is no lock protocol and no leader: instead, every write is a *pure
//! function of one file's bytes* applied as a delete-then-insert of that file's
//! rows. Any interleaving of two such writes converges, because both are
//! computing the same answer from the same input. WAL plus a busy timeout is
//! all the coordination that is needed. Preserve that property — a write path
//! that reads the previous state and patches it would break it silently.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, Transaction};

use crate::block::BlockLevel;
use crate::chunk::Parsed;
use crate::tokenize::{tokenize, TOKENIZER_ID};

pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA_SQL: &str = r#"
CREATE TABLE files(
  id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL,
  ext TEXT NOT NULL, mtime INTEGER, size INTEGER, content_hash TEXT,
  title TEXT, concept_type TEXT, tags_json TEXT,
  doc_date TEXT, date_inferred INTEGER,
  human_verified INTEGER DEFAULT 0);
CREATE TABLE blocks(
  id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL REFERENCES files(id),
  line_start INTEGER, line_end INTEGER,
  breadcrumb TEXT, text TEXT, level TEXT,
  is_annotation INTEGER DEFAULT 0, agent_by TEXT);
CREATE INDEX blocks_file ON blocks(file_id);
CREATE VIRTUAL TABLE blocks_fts USING fts5(tok_text, tok_breadcrumb);
CREATE TABLE links(file_id INTEGER, kind TEXT, target TEXT, line INTEGER);
CREATE INDEX links_file ON links(file_id);
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
"#;

pub struct FileRow {
    pub path: String,
    pub mtime: i64,
    pub size: i64,
    pub content_hash: String,
}

/// Open (creating if needed) the index at `db_path`. Anything unexpected —
/// unreadable file, wrong schema version, wrong tokenizer — is resolved by
/// deleting the file and starting over. There is deliberately no repair path.
pub fn open(db_path: &Path, vault_root: &str) -> rusqlite::Result<Connection> {
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match try_open(db_path, vault_root) {
        Ok(conn) => Ok(conn),
        Err(_) => {
            wipe(db_path);
            try_open(db_path, vault_root)
        }
    }
}

fn try_open(db_path: &Path, vault_root: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    let has_meta: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='meta'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)?;

    if has_meta {
        let ok = meta_get(&conn, "schema_version").as_deref() == Some(&SCHEMA_VERSION.to_string())
            && meta_get(&conn, "tokenizer_id").as_deref() == Some(TOKENIZER_ID);
        if ok {
            // vault_root can change if the same cache slot is reused; stamp it.
            meta_set(&conn, "vault_root", vault_root)?;
            return Ok(conn);
        }
        drop(conn);
        wipe(db_path);
        return try_open(db_path, vault_root);
    }

    conn.execute_batch(SCHEMA_SQL)?;
    meta_set(&conn, "schema_version", &SCHEMA_VERSION.to_string())?;
    meta_set(&conn, "tokenizer_id", TOKENIZER_ID)?;
    meta_set(&conn, "vault_root", vault_root)?;
    Ok(conn)
}

fn wipe(db_path: &Path) {
    let _ = std::fs::remove_file(db_path);
    for suffix in ["-wal", "-shm"] {
        let mut p = db_path.as_os_str().to_os_string();
        p.push(suffix);
        let _ = std::fs::remove_file(Path::new(&p));
    }
}

pub fn meta_get(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM meta WHERE key=?1", params![key], |r| r.get(0)).ok()
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Delete every row belonging to `rel` and insert the freshly parsed ones.
#[allow(clippy::too_many_arguments)]
pub fn replace_file(
    tx: &Transaction,
    rel: &str,
    ext: &str,
    mtime: i64,
    size: i64,
    hash: &str,
    parsed: &Parsed,
) -> rusqlite::Result<()> {
    remove_file(tx, rel)?;
    let tags_json = serde_json::to_string(&parsed.meta.tags).unwrap_or_else(|_| "[]".into());
    tx.execute(
        "INSERT INTO files(path,ext,mtime,size,content_hash,title,concept_type,tags_json,doc_date,date_inferred,human_verified)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            rel, ext, mtime, size, hash,
            parsed.meta.title, parsed.meta.concept_type, tags_json,
            parsed.meta.doc_date, parsed.meta.date_inferred as i64,
            parsed.meta.human_verified as i64
        ],
    )?;
    let file_id = tx.last_insert_rowid();

    let mut ins_block = tx.prepare_cached(
        "INSERT INTO blocks(file_id,line_start,line_end,breadcrumb,text,level,is_annotation,agent_by)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
    )?;
    let mut ins_fts = tx.prepare_cached(
        "INSERT INTO blocks_fts(rowid,tok_text,tok_breadcrumb) VALUES(?1,?2,?3)",
    )?;
    for b in &parsed.blocks {
        ins_block.execute(params![
            file_id, b.line_start, b.line_end, b.breadcrumb, b.text,
            b.level.as_str(), b.is_annotation as i64, b.agent_by
        ])?;
        let block_id = tx.last_insert_rowid();
        ins_fts.execute(params![block_id, tokenize(&b.text), tokenize(&b.breadcrumb)])?;
    }

    let mut ins_link = tx.prepare_cached(
        "INSERT INTO links(file_id,kind,target,line) VALUES(?1,?2,?3,?4)",
    )?;
    for l in &parsed.links {
        ins_link.execute(params![file_id, l.kind, l.target, l.line])?;
    }
    Ok(())
}

pub fn remove_file(tx: &Transaction, rel: &str) -> rusqlite::Result<()> {
    // The FTS table is a standalone (not external-content) table, so its rows
    // must be deleted explicitly by rowid — blocks.id IS blocks_fts.rowid.
    tx.execute(
        "DELETE FROM blocks_fts WHERE rowid IN
           (SELECT b.id FROM blocks b JOIN files f ON f.id=b.file_id WHERE f.path=?1)",
        params![rel],
    )?;
    tx.execute(
        "DELETE FROM blocks WHERE file_id IN (SELECT id FROM files WHERE path=?1)",
        params![rel],
    )?;
    tx.execute(
        "DELETE FROM links WHERE file_id IN (SELECT id FROM files WHERE path=?1)",
        params![rel],
    )?;
    tx.execute("DELETE FROM files WHERE path=?1", params![rel])?;
    Ok(())
}

pub fn all_file_rows(conn: &Connection) -> rusqlite::Result<HashMap<String, FileRow>> {
    let mut stmt = conn.prepare("SELECT path,mtime,size,content_hash FROM files")?;
    let rows = stmt.query_map([], |r| {
        Ok(FileRow { path: r.get(0)?, mtime: r.get(1)?, size: r.get(2)?, content_hash: r.get(3)? })
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let row = row?;
        out.insert(row.path.clone(), row);
    }
    Ok(out)
}

/// `BlockLevel` round-trip helper used by the query layer.
pub fn level_of(s: &str) -> BlockLevel {
    BlockLevel::from_str(s)
}
```

在 `lib.rs` 加 `pub mod store;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path searchidx/Cargo.toml store`
Expected: 8 个测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/store.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): FTS5 schema, self-healing open, file-granular idempotent replace"
```

---

### Task 8: 索引库路径(GUI 与 CLI 必须同库)

spec §3.4 + §3.4 的 Windows 陷阱。**唯一**的路径解析函数放在核心 crate,GUI 与 CLI 都调它——不是"两边各自算得一样",而是"两边根本只有一份实现"。

**Files:**
- Create: `searchidx/src/paths.rs`
- Modify: `searchidx/src/lib.rs`

**Interfaces:**
- Produces:
  - `searchidx::paths::index_db_path(vault_root: &Path) -> Option<PathBuf>`
  - `searchidx::paths::vault_key(vault_root: &Path) -> String` — sha256 前 16 位 hex
  - `pub const BUNDLE_ID: &str = "net.notemd.app";`

- [ ] **Step 1: 写失败的测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 同一个 vault 必须永远算出同一个 key,且 key 只依赖规范化后的路径字符串
    /// —— 否则 GUI 与 CLI 会各开一个库,CLI 永远查不到 GUI 刚索引的内容。
    #[test]
    fn vault_key_is_stable_and_slash_normalized() {
        let a = vault_key(Path::new("/Users/x/vault"));
        assert_eq!(a, vault_key(Path::new("/Users/x/vault/")));
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, vault_key(Path::new("/Users/x/other")));
    }

    #[test]
    fn db_path_is_under_the_local_app_data_dir_for_this_bundle() {
        let p = index_db_path(Path::new("/Users/x/vault")).unwrap();
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(s.contains(BUNDLE_ID), "{s}");
        assert!(s.ends_with(&format!("search/{}/index.db", vault_key(Path::new("/Users/x/vault")))), "{s}");
        assert!(s.starts_with(&dirs::data_local_dir().unwrap().to_string_lossy().replace('\\', "/")), "{s}");
    }

    /// Windows 上索引必须落在 Local,不是 Roaming:索引属于机器,漫游到另一台
    /// 机器上的是一份指向不存在文件的陈旧库。macOS 上两者恰好同路径,正是这一点
    /// 长期掩盖了这个坑,所以这条断言只在 Windows 上才有意义 —— 也只在那里跑。
    #[cfg(windows)]
    #[test]
    fn on_windows_the_index_lives_in_local_appdata_not_roaming() {
        let p = index_db_path(Path::new(r"C:\vault")).unwrap().to_string_lossy().to_lowercase();
        assert!(p.contains(r"\local\"), "{p}");
        let roaming = dirs::data_dir().unwrap().to_string_lossy().to_lowercase();
        assert!(!p.starts_with(&roaming), "index must not roam: {p}");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml paths`
Expected: 编译失败。

- [ ] **Step 3: 写实现**

```rust
//! Where the index lives.
//!
//! Outside the vault, in the machine's LOCAL app data — `dirs::data_local_dir()`,
//! never `data_dir()`. On macOS the two happen to be the same directory, which is
//! precisely why the distinction is easy to get wrong: on Windows `data_dir()` is
//! Roaming AppData, so a domain user's index would follow them to another machine
//! and describe files that are not there. The index belongs to a machine.
//!
//! Both the GUI and the CLI call THIS function. Not "two implementations that
//! agree" — one implementation, so there is nothing to drift.

use std::path::{Path, PathBuf};

pub const BUNDLE_ID: &str = "net.notemd.app";

/// Stable per-vault cache key: first 16 hex chars of the SHA-256 of the
/// `/`-normalized, trailing-slash-free vault path.
pub fn vault_key(vault_root: &Path) -> String {
    let norm = vault_root.to_string_lossy().replace('\\', "/");
    let norm = norm.trim_end_matches('/');
    crate::norm::content_hash(norm.as_bytes())[..16].to_string()
}

/// `<local app data>/net.notemd.app/search/<vault_key>/index.db`.
/// `None` only when the platform has no local data directory at all.
pub fn index_db_path(vault_root: &Path) -> Option<PathBuf> {
    Some(
        dirs::data_local_dir()?
            .join(BUNDLE_ID)
            .join("search")
            .join(vault_key(vault_root))
            .join("index.db"),
    )
}
```

在 `lib.rs` 加 `pub mod paths;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path searchidx/Cargo.toml paths`
Expected: macOS 上 2 个 PASS(Windows 上 3 个)。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/paths.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): single index-path resolver (local app data, never roaming)"
```

---

### Task 9: 全量构建 + 新鲜度 sweep + 护栏

spec §3.7 + §3.8。全量构建单事务批量写;sweep 用 `(mtime,size)` 快路径,变了才 hash。护栏用 `largeFileThresholdMb`(默认 10MB,与 git 门禁同值)——**不能**沿用反链层的 1MB,那会砍掉近半语料。sweep 带硬超时:超时用现有索引作答,不阻塞不报错。

**Files:**
- Create: `searchidx/src/scan.rs`
- Modify: `searchidx/src/lib.rs`

**Interfaces:**
- Consumes: `store::*`、`chunk::parse_file`、`norm::{rel_path, content_hash}`
- Produces:
  - `pub struct ScanOptions { pub large_file_threshold_mb: u32, pub exclude_dirs: Vec<String> }`(`Default` = 10MB / 空)
  - `pub struct ScanStats { pub files_indexed: usize, pub files_removed: usize, pub files_skipped_large: Vec<String>, pub took_ms: u128, pub timed_out: bool }`
  - `searchidx::scan::build_full(conn: &mut Connection, vault_root: &Path, opts: &ScanOptions) -> rusqlite::Result<ScanStats>`
  - `searchidx::scan::sweep(conn: &mut Connection, vault_root: &Path, opts: &ScanOptions, deadline: Option<Duration>) -> rusqlite::Result<ScanStats>`
  - `searchidx::scan::index_one(conn: &mut Connection, vault_root: &Path, rel: &str, opts: &ScanOptions) -> rusqlite::Result<bool>`
  - `searchidx::scan::is_indexable(rel: &str, opts: &ScanOptions) -> bool`

- [ ] **Step 1: 写失败的测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn vault(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (rel, body) in files {
            let p = d.path().join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
        d
    }
    fn conn_for(v: &Path) -> Connection {
        crate::store::open(&v.join(".idx.db"), &v.to_string_lossy()).unwrap()
    }
    fn count(c: &Connection) -> i64 {
        c.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn build_full_indexes_markdown_and_note_files_only() {
        let v = vault(&[("a.md", "alpha\n"), ("b.note.md", "- beta\n"), ("c.txt", "gamma\n"), ("d.png", "x")]);
        let mut c = conn_for(v.path());
        let s = build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        assert_eq!(s.files_indexed, 2);
        assert_eq!(count(&c), 2);
    }

    /// `.` 开头的目录不进索引:`.git` 是几万个对象,`.notemd` 是配置。
    #[test]
    fn dot_directories_are_skipped() {
        let v = vault(&[("a.md", "x\n"), (".git/x.md", "y\n"), (".notemd/z.md", "y\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        assert_eq!(count(&c), 1);
    }

    /// 护栏是 10MB 而不是反链层的 1MB —— 后者会砍掉真实 vault 里 46% 的语料。
    #[test]
    fn files_over_the_threshold_are_skipped_and_reported() {
        let big = "x".repeat(2 * 1024 * 1024);
        let v = vault(&[("a.md", "small\n"), ("big.md", &big)]);
        let mut c = conn_for(v.path());
        let opts = ScanOptions { large_file_threshold_mb: 1, ..Default::default() };
        let s = build_full(&mut c, v.path(), &opts).unwrap();
        assert_eq!(s.files_indexed, 1);
        assert_eq!(s.files_skipped_large, vec!["big.md".to_string()]);
    }

    #[test]
    fn excluded_directories_are_not_indexed() {
        let v = vault(&[("a.md", "x\n"), ("sessions/b.md", "y\n")]);
        let mut c = conn_for(v.path());
        let opts = ScanOptions { exclude_dirs: vec!["sessions".into()], ..Default::default() };
        build_full(&mut c, v.path(), &opts).unwrap();
        assert_eq!(count(&c), 1);
    }

    #[test]
    fn sweep_reindexes_only_what_changed() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_indexed, 0, "an unchanged vault must be a no-op");

        fs::write(v.path().join("a.md"), "alpha changed\n").unwrap();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_indexed, 1);
    }

    #[test]
    fn sweep_removes_rows_for_deleted_files() {
        let v = vault(&[("a.md", "alpha\n"), ("b.md", "beta\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        fs::remove_file(v.path().join("b.md")).unwrap();
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_removed, 1);
        assert_eq!(count(&c), 1);
    }

    /// 同样的 mtime/size 但内容变了(编辑器保留时间戳)也要被抓到:快路径之后
    /// 还有 hash 复核。
    #[test]
    fn sweep_falls_back_to_hashing_when_stat_looks_unchanged() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        build_full(&mut c, v.path(), &ScanOptions::default()).unwrap();
        // same length, same mtime restored
        let meta = fs::metadata(v.path().join("a.md")).unwrap();
        fs::write(v.path().join("a.md"), "alphaX\n").unwrap();
        filetime_set(&v.path().join("a.md"), &meta);
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), None).unwrap();
        assert_eq!(s.files_indexed, 1, "content change must be caught even when stat matches");
    }

    /// 超时是降级,不是错误:返回现有索引能给的答案。
    #[test]
    fn sweep_reports_a_timeout_instead_of_failing() {
        let v = vault(&[("a.md", "alpha\n")]);
        let mut c = conn_for(v.path());
        let s = sweep(&mut c, v.path(), &ScanOptions::default(), Some(Duration::from_nanos(1))).unwrap();
        assert!(s.timed_out);
    }

    /// 索引是纯函数:全量重建两次必须逐字节一致。
    #[test]
    fn rebuilding_twice_produces_an_identical_index() {
        let v = vault(&[("a.md", "# T\n\nalpha 检索\n"), ("b.note.md", "- beta\n  type:: annotation\n")]);
        let dump = |c: &Connection| -> Vec<String> {
            let mut st = c
                .prepare("SELECT f.path,b.line_start,b.line_end,b.level,b.breadcrumb,b.text FROM blocks b JOIN files f ON f.id=b.file_id ORDER BY f.path,b.line_start,b.level,b.text")
                .unwrap();
            st.query_map([], |r| {
                Ok(format!("{}|{}|{}|{}|{}|{}",
                    r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?))
            }).unwrap().map(|x| x.unwrap()).collect()
        };
        let mut c1 = crate::store::open(&v.path().join(".i1.db"), "v").unwrap();
        build_full(&mut c1, v.path(), &ScanOptions::default()).unwrap();
        let mut c2 = crate::store::open(&v.path().join(".i2.db"), "v").unwrap();
        build_full(&mut c2, v.path(), &ScanOptions::default()).unwrap();
        assert_eq!(dump(&c1), dump(&c2));
    }

    /// 测试辅助:把 mtime 设回去。用 std 的 File::set_modified,不引入 filetime。
    fn filetime_set(p: &Path, meta: &std::fs::Metadata) {
        let f = fs::OpenOptions::new().write(true).open(p).unwrap();
        f.set_modified(meta.modified().unwrap()).unwrap();
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml scan`
Expected: 编译失败。

- [ ] **Step 3: 写实现**

```rust
//! Scanning: the full build and the freshness sweep.
//!
//! The sweep exists because the GUI and the CLI are separate processes with no
//! channel between them. When the GUI is closed, nothing has been watching the
//! vault, so the CLI cannot assume the index is current — it proves freshness
//! itself before answering. That proof is bounded by a hard deadline: a slow
//! sweep degrades to "answer from what we have, warn on stderr", because a
//! retrieval tool that blocks is worse than one that is slightly stale.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::norm::{content_hash, rel_path};
use crate::store;

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub large_file_threshold_mb: u32,
    /// Vault-relative directory prefixes to skip, `/`-separated.
    pub exclude_dirs: Vec<String>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        // 10 MB matches the vault's git large-file gate. NOT the backlink
        // layer's 1 MB: measured against a real vault, that would drop 46% of
        // the corpus — a guardrail for a different job.
        ScanOptions { large_file_threshold_mb: 10, exclude_dirs: Vec::new() }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ScanStats {
    pub files_indexed: usize,
    pub files_removed: usize,
    pub files_skipped_large: Vec<String>,
    pub took_ms: u128,
    pub timed_out: bool,
}

pub fn is_indexable(rel: &str, opts: &ScanOptions) -> bool {
    if !rel.ends_with(".md") {
        return false;
    }
    if rel.split('/').any(|seg| seg.starts_with('.')) {
        return false;
    }
    !opts.exclude_dirs.iter().any(|d| {
        let d = d.trim_matches('/');
        !d.is_empty() && (rel == d || rel.starts_with(&format!("{d}/")))
    })
}

struct Candidate {
    rel: String,
    mtime: i64,
    size: i64,
}

fn walk(vault_root: &Path, opts: &ScanOptions) -> (Vec<Candidate>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    let limit = opts.large_file_threshold_mb as u64 * 1024 * 1024;

    let walker = ignore::WalkBuilder::new(vault_root)
        .hidden(true)
        .follow_links(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .build();

    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Some(rel) = rel_path(vault_root, entry.path()) else { continue };
        if !is_indexable(&rel, opts) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.len() > limit {
            skipped.push(rel);
            continue;
        }
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        out.push(Candidate { rel, mtime, size: meta.len() as i64 });
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    (out, skipped)
}

pub fn build_full(
    conn: &mut Connection,
    vault_root: &Path,
    opts: &ScanOptions,
) -> rusqlite::Result<ScanStats> {
    let started = Instant::now();
    let (candidates, skipped) = walk(vault_root, opts);
    let mut stats = ScanStats { files_skipped_large: skipped, ..Default::default() };

    // One transaction for the whole build: thousands of small commits is what
    // makes naive SQLite indexers slow, not the parsing.
    let tx = conn.transaction()?;
    tx.execute_batch("DELETE FROM blocks_fts; DELETE FROM blocks; DELETE FROM links; DELETE FROM files;")?;
    for c in &candidates {
        if index_into(&tx, vault_root, c)? {
            stats.files_indexed += 1;
        }
    }
    tx.commit()?;
    store::meta_set(conn, "built_at", &format!("{}", now_secs()))?;
    stats.took_ms = started.elapsed().as_millis();
    Ok(stats)
}

pub fn sweep(
    conn: &mut Connection,
    vault_root: &Path,
    opts: &ScanOptions,
    deadline: Option<Duration>,
) -> rusqlite::Result<ScanStats> {
    let started = Instant::now();
    let over = |s: &Instant| deadline.is_some_and(|d| s.elapsed() >= d);

    let known: HashMap<String, store::FileRow> = store::all_file_rows(conn)?;
    let (candidates, skipped) = walk(vault_root, opts);
    let mut stats = ScanStats { files_skipped_large: skipped, ..Default::default() };

    let tx = conn.transaction()?;
    let mut seen: Vec<&str> = Vec::with_capacity(candidates.len());
    for c in &candidates {
        seen.push(&c.rel);
        if over(&started) {
            stats.timed_out = true;
            break;
        }
        let fresh = known
            .get(&c.rel)
            .is_some_and(|row| row.mtime == c.mtime && row.size == c.size);
        if fresh {
            continue;
        }
        // stat says "maybe"; the hash decides. Editors that preserve mtime
        // (and same-length edits) would otherwise slip through unnoticed.
        if let Some(row) = known.get(&c.rel) {
            if let Ok(bytes) = std::fs::read(vault_root.join(&c.rel)) {
                if content_hash(&bytes) == row.content_hash {
                    continue;
                }
            }
        }
        if index_into(&tx, vault_root, c)? {
            stats.files_indexed += 1;
        }
    }
    if !stats.timed_out {
        let present: std::collections::HashSet<&str> = seen.into_iter().collect();
        for path in known.keys() {
            if !present.contains(path.as_str()) {
                store::remove_file(&tx, path)?;
                stats.files_removed += 1;
            }
        }
    }
    tx.commit()?;
    stats.took_ms = started.elapsed().as_millis();
    Ok(stats)
}

/// Re-index a single file (watcher path). Returns false when the file is gone
/// or not indexable — in which case its rows are removed.
pub fn index_one(
    conn: &mut Connection,
    vault_root: &Path,
    rel: &str,
    opts: &ScanOptions,
) -> rusqlite::Result<bool> {
    let abs = vault_root.join(rel);
    let tx = conn.transaction()?;
    let indexed = match std::fs::metadata(&abs) {
        Ok(meta) if is_indexable(rel, opts) && meta.len() <= opts.large_file_threshold_mb as u64 * 1024 * 1024 => {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            index_into(&tx, vault_root, &Candidate { rel: rel.to_string(), mtime, size: meta.len() as i64 })?
        }
        _ => {
            store::remove_file(&tx, rel)?;
            false
        }
    };
    tx.commit()?;
    Ok(indexed)
}

fn index_into(
    tx: &rusqlite::Transaction,
    vault_root: &Path,
    c: &Candidate,
) -> rusqlite::Result<bool> {
    let Ok(bytes) = std::fs::read(vault_root.join(&c.rel)) else { return Ok(false) };
    // Lossy on purpose: a file with a stray non-UTF-8 byte still gets indexed
    // rather than silently vanishing from search.
    let raw = String::from_utf8_lossy(&bytes);
    let parsed = crate::chunk::parse_file(&c.rel, &raw, c.mtime);
    let ext = if c.rel.ends_with(".note.md") { "note.md" } else { "md" };
    store::replace_file(tx, &c.rel, ext, c.mtime, c.size, &content_hash(&bytes), &parsed)?;
    Ok(true)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
```

在 `lib.rs` 加 `pub mod scan;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path searchidx/Cargo.toml scan -- --nocapture`
Expected: 9 个测试 PASS。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/scan.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): full build + bounded freshness sweep with 10MB guardrail"
```

---

### Task 10: 查询解析、检索、排序、LIKE 兜底

spec §3.2 召回兜底 + §4 全节。UI 与 CLI 共用**同一个解析器**——就是这个函数。三条要点:①`bm25()` 越小越相关,取负后再归一化;②业务加成是常量初值,调参须过回归集;③CJK 零命中/单字/未登录词降级 `LIKE` 有界扫描并标 `route:"t1-scan"`,不静默漏检。

**Files:**
- Create: `searchidx/src/query.rs`
- Modify: `searchidx/src/lib.rs`

**Interfaces:**
- Consumes: `store::*`、`tokenize::{tokens, has_han}`
- Produces:
  - `pub struct Query { pub terms: Vec<String>, pub phrases: Vec<String>, pub tags: Vec<String>, pub types: Vec<String>, pub paths: Vec<String>, pub pages: Vec<String>, pub exts: Vec<String>, pub after: Option<String>, pub before: Option<String>, pub raw: String }`
  - `pub struct Hit { pub path: String, pub line: u32, pub line_end: u32, pub text: String, pub breadcrumb: String, pub level: String, pub score: f64, pub doc_date: Option<String>, pub agent_by: Option<String>, pub human_verified: bool }`
  - `pub enum Route { Fts, Scan }`(`as_str()` → `"t1-fts" | "t1-scan"`)
  - `searchidx::query::parse(raw: &str) -> Query`
  - `searchidx::query::search(conn: &Connection, q: &Query, limit: usize, today: &str) -> rusqlite::Result<(Vec<Hit>, Route)>`

- [ ] **Step 1: 写失败的测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_words_are_and_terms() {
        let q = parse("alpha beta");
        assert_eq!(q.terms, vec!["alpha", "beta"]);
        assert!(q.phrases.is_empty());
    }

    #[test]
    fn quoted_text_is_a_phrase() {
        let q = parse(r#"alpha "exact phrase" beta"#);
        assert_eq!(q.phrases, vec!["exact phrase"]);
        assert_eq!(q.terms, vec!["alpha", "beta"]);
    }

    #[test]
    fn every_filter_prefix_is_recognized() {
        let q = parse("tag:x type:concept path:docs ext:note.md after:2026-01-01 before:2026-12-31 page:[[Home]] rest");
        assert_eq!(q.tags, vec!["x"]);
        assert_eq!(q.types, vec!["concept"]);
        assert_eq!(q.paths, vec!["docs"]);
        assert_eq!(q.exts, vec!["note.md"]);
        assert_eq!(q.after.as_deref(), Some("2026-01-01"));
        assert_eq!(q.before.as_deref(), Some("2026-12-31"));
        assert_eq!(q.pages, vec!["Home"]);
        assert_eq!(q.terms, vec!["rest"]);
    }

    #[test]
    fn an_unterminated_quote_degrades_to_a_plain_term() {
        let q = parse(r#"alpha "unterminated"#);
        assert!(q.phrases.is_empty());
        assert!(q.terms.contains(&"unterminated".to_string()));
    }

    // ---- search over a real index -------------------------------------------

    fn indexed(files: &[(&str, &str)]) -> (tempfile::TempDir, Connection) {
        let d = tempfile::tempdir().unwrap();
        for (rel, body) in files {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        let mut c = crate::store::open(&d.path().join(".idx.db"), "v").unwrap();
        crate::scan::build_full(&mut c, d.path(), &crate::scan::ScanOptions::default()).unwrap();
        (d, c)
    }

    #[test]
    fn finds_an_ascii_term_and_returns_a_source_anchor() {
        let (_d, c) = indexed(&[("2026-01-01-a.md", "# T\n\nthe quick brownfox\n")]);
        let (hits, route) = search(&c, &parse("brownfox"), 20, "2026-08-10").unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].path, "2026-01-01-a.md");
        assert_eq!(hits[0].line, 3);
        assert_eq!(route.as_str(), "t1-fts");
    }

    /// spec §3.2 的招牌用例:查「增量」必须命中只写了「增量索引」的文档。
    #[test]
    fn a_cjk_sub_word_query_hits_the_longer_word() {
        let (_d, c) = indexed(&[("a.md", "本节讲增量索引的设计\n")]);
        let (hits, _) = search(&c, &parse("增量"), 20, "2026-08-10").unwrap();
        assert!(!hits.is_empty(), "cut_for_search overlap must make this hit");
    }

    /// 词典盲区(未登录词/人名/单字):FTS 零命中就降级有界扫描,并如实标注路由。
    #[test]
    fn an_out_of_vocabulary_cjk_query_falls_back_to_a_bounded_scan() {
        let (_d, c) = indexed(&[("a.md", "会见了李慕白同志\n")]);
        let (hits, route) = search(&c, &parse("李慕白"), 20, "2026-08-10").unwrap();
        assert!(!hits.is_empty(), "the dictionary blind spot must not become a miss");
        assert_eq!(route.as_str(), "t1-scan");
    }

    /// 引号短语必须做精确子串复核:分词是重叠的,FTS 的 AND 不保证顺序。
    #[test]
    fn a_phrase_query_rejects_hits_where_the_words_are_not_adjacent() {
        let (_d, c) = indexed(&[("a.md", "alpha then beta\n"), ("b.md", "alpha beta\n")]);
        let (hits, _) = search(&c, &parse(r#""alpha beta""#), 20, "2026-08-10").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "b.md");
    }

    #[test]
    fn filters_narrow_the_result_set() {
        let (_d, c) = indexed(&[
            ("docs/a.md", "---\ntype: concept\ntags: [x]\n---\ntarget\n"),
            ("other/b.md", "target\n"),
        ]);
        let only = |q: &str| search(&c, &parse(q), 20, "2026-08-10").unwrap().0.len();
        assert_eq!(only("target path:docs"), 1);
        assert_eq!(only("target type:concept"), 1);
        assert_eq!(only("target tag:x"), 1);
        assert_eq!(only("target"), 2);
    }

    #[test]
    fn date_filters_use_doc_date() {
        let (_d, c) = indexed(&[("2020-01-01-old.md", "target\n"), ("2026-08-01-new.md", "target\n")]);
        let hits = search(&c, &parse("target after:2026-01-01"), 20, "2026-08-10").unwrap().0;
        assert!(hits.iter().all(|h| h.path.starts_with("2026")), "{hits:?}");
    }

    /// spec §4 的产品主张:你留过判断的内容优先,AI 生成物降权。
    #[test]
    fn annotations_outrank_agent_authored_blocks() {
        let (_d, c) = indexed(&[("a.note.md", "- target one\n  type:: annotation\n- target two\n  by:: claude/1\n")]);
        let hits = search(&c, &parse("target"), 20, "2026-08-10").unwrap().0;
        let anno = hits.iter().position(|h| h.text.contains("one")).unwrap();
        let agent = hits.iter().position(|h| h.text.contains("two")).unwrap();
        assert!(anno < agent, "human-marked content must rank above agent output: {hits:?}");
    }

    #[test]
    fn scores_are_finite_positive_and_descending() {
        let (_d, c) = indexed(&[("a.md", "target target target\n"), ("b.md", "target\n")]);
        let hits = search(&c, &parse("target"), 20, "2026-08-10").unwrap().0;
        assert!(hits.iter().all(|h| h.score > 0.0 && h.score < 1.0 && h.score.is_finite()), "{hits:?}");
        assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
    }

    #[test]
    fn limit_is_respected() {
        let (_d, c) = indexed(&[("a.md", "target\n"), ("b.md", "target\n"), ("c.md", "target\n")]);
        assert_eq!(search(&c, &parse("target"), 2, "2026-08-10").unwrap().0.len(), 2);
    }

    /// FTS5 的语法字符不能把查询打成语法错误 —— agent 会原样传用户输入进来。
    #[test]
    fn fts_syntax_characters_in_a_query_do_not_error() {
        let (_d, c) = indexed(&[("a.md", "target\n")]);
        for q in ["a OR b", "NEAR(", "*", "\"", "^x", "a-b"] {
            assert!(search(&c, &parse(q), 5, "2026-08-10").is_ok(), "query {q:?} must not error");
        }
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml query`
Expected: 编译失败。

- [ ] **Step 3: 写实现**

```rust
//! Query parsing, retrieval and ranking. The UI and the CLI both call `parse`
//! and `search`, so a filter that works in one works in the other by
//! construction.

use rusqlite::{params_from_iter, Connection};

use crate::tokenize::{has_han, tokens};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Query {
    pub terms: Vec<String>,
    pub phrases: Vec<String>,
    pub tags: Vec<String>,
    pub types: Vec<String>,
    pub paths: Vec<String>,
    pub pages: Vec<String>,
    pub exts: Vec<String>,
    pub after: Option<String>,
    pub before: Option<String>,
    pub raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Fts,
    Scan,
}

impl Route {
    pub fn as_str(self) -> &'static str {
        match self {
            Route::Fts => "t1-fts",
            Route::Scan => "t1-scan",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Hit {
    pub path: String,
    pub line: u32,
    pub line_end: u32,
    pub text: String,
    pub breadcrumb: String,
    pub level: String,
    pub score: f64,
    pub doc_date: Option<String>,
    pub agent_by: Option<String>,
    pub human_verified: bool,
}

impl Hit {
    /// The back-to-source anchor handed to agents, e.g. `docs/a.md#L120`.
    pub fn source_ref(&self) -> String {
        format!("{}#L{}", self.path, self.line)
    }
}

pub fn parse(raw: &str) -> Query {
    let mut q = Query { raw: raw.to_string(), ..Default::default() };
    for token in split_respecting_quotes(raw) {
        if let Some(rest) = token.strip_prefix('"') {
            // Only a *closed* quote makes a phrase; an unterminated one is far
            // more likely a typo than an intent, so it degrades to a term.
            if let Some(inner) = rest.strip_suffix('"') {
                if !inner.trim().is_empty() {
                    q.phrases.push(inner.trim().to_string());
                    continue;
                }
            }
            push_terms(&mut q, rest.trim_matches('"'));
            continue;
        }
        match token.split_once(':') {
            Some(("tag", v)) if !v.is_empty() => q.tags.push(v.to_string()),
            Some(("type", v)) if !v.is_empty() => q.types.push(v.to_string()),
            Some(("path", v)) if !v.is_empty() => q.paths.push(v.to_string()),
            Some(("ext", v)) if !v.is_empty() => q.exts.push(v.trim_start_matches('.').to_string()),
            Some(("after", v)) if !v.is_empty() => q.after = Some(v.to_string()),
            Some(("before", v)) if !v.is_empty() => q.before = Some(v.to_string()),
            Some(("page", v)) if !v.is_empty() => {
                q.pages.push(v.trim_start_matches("[[").trim_end_matches("]]").to_string())
            }
            _ => push_terms(&mut q, &token),
        }
    }
    q
}

fn push_terms(q: &mut Query, raw: &str) {
    let t = raw.trim();
    if !t.is_empty() {
        q.terms.push(t.to_string());
    }
}

fn split_respecting_quotes(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in raw.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                cur.push(c);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn search(
    conn: &Connection,
    q: &Query,
    limit: usize,
    today: &str,
) -> rusqlite::Result<(Vec<Hit>, Route)> {
    let hits = fts_search(conn, q, limit, today)?;
    if !hits.is_empty() {
        return Ok((hits, Route::Fts));
    }
    // The dictionary has blind spots — new coinages, personal names, single
    // characters. A miss there would be invisible to the user, so we pay for a
    // bounded LIKE scan rather than report "no results". The corpus is bounded
    // and the scan is capped, which is why the usual "never full-scan" rule is
    // suspended here on purpose.
    if needs_scan_fallback(q) {
        let hits = like_search(conn, q, limit, today)?;
        if !hits.is_empty() {
            return Ok((hits, Route::Scan));
        }
    }
    Ok((Vec::new(), Route::Fts))
}

fn needs_scan_fallback(q: &Query) -> bool {
    q.terms.iter().chain(q.phrases.iter()).any(|t| has_han(t) || t.chars().count() <= 2)
}

/// Build the FTS5 MATCH expression. Every term is emitted as a quoted string
/// literal, which is what neutralizes FTS5 operators (`OR`, `NEAR`, `*`, `^`)
/// arriving inside user input — an agent will hand us its query verbatim and a
/// syntax error would look like "no results".
fn match_expr(q: &Query) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for t in q.terms.iter().chain(q.phrases.iter()) {
        for tok in tokens(t) {
            parts.push(format!("\"{}\"", tok.replace('"', "\"\"")));
        }
    }
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

const SELECT_COLS: &str = "f.path, b.line_start, b.line_end, b.text, b.breadcrumb, b.level, \
                           f.doc_date, b.agent_by, f.human_verified, b.is_annotation";

fn fts_search(conn: &Connection, q: &Query, limit: usize, today: &str) -> rusqlite::Result<Vec<Hit>> {
    let Some(expr) = match_expr(q) else { return Ok(Vec::new()) };
    let mut sql = format!(
        "SELECT {SELECT_COLS}, bm25(blocks_fts, 1.0, 2.0) AS rank
         FROM blocks_fts
         JOIN blocks b ON b.id = blocks_fts.rowid
         JOIN files f ON f.id = b.file_id
         WHERE blocks_fts MATCH ?1"
    );
    let mut args: Vec<String> = vec![expr];
    push_filters(q, &mut sql, &mut args);
    // Over-fetch: business boosts reorder, and a phrase recheck removes rows.
    sql.push_str(&format!(" ORDER BY rank ASC LIMIT {}", (limit * 8).max(64)));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok((row_to_hit(r)?, r.get::<_, f64>(10)?, r.get::<_, i64>(9)? != 0))
    })?;
    finish(rows.collect::<rusqlite::Result<Vec<_>>>()?, q, limit, today)
}

fn like_search(conn: &Connection, q: &Query, limit: usize, today: &str) -> rusqlite::Result<Vec<Hit>> {
    let needle = q.phrases.first().or_else(|| q.terms.first()).cloned().unwrap_or_default();
    if needle.is_empty() {
        return Ok(Vec::new());
    }
    let mut sql = format!(
        "SELECT {SELECT_COLS}, 0.0 AS rank
         FROM blocks b JOIN files f ON f.id = b.file_id
         WHERE b.text LIKE ?1 ESCAPE '\\'"
    );
    let mut args: Vec<String> = vec![format!("%{}%", escape_like(&needle))];
    push_filters(q, &mut sql, &mut args);
    // Hard cap: the fallback is a safety net, not a query plan.
    sql.push_str(" LIMIT 500");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
        Ok((row_to_hit(r)?, -1.0f64, r.get::<_, i64>(9)? != 0))
    })?;
    finish(rows.collect::<rusqlite::Result<Vec<_>>>()?, q, limit, today)
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn push_filters(q: &Query, sql: &mut String, args: &mut Vec<String>) {
    let mut next = |args: &mut Vec<String>, v: String| {
        args.push(v);
        args.len()
    };
    for t in &q.tags {
        // tags_json is a JSON array; matching the quoted value avoids `a` also
        // matching `alpha`.
        let i = next(args, format!("%\"{t}\"%"));
        sql.push_str(&format!(" AND f.tags_json LIKE ?{i}"));
    }
    for t in &q.types {
        let i = next(args, t.clone());
        sql.push_str(&format!(" AND f.concept_type = ?{i}"));
    }
    for p in &q.paths {
        let i = next(args, format!("%{}%", escape_like(p)));
        sql.push_str(&format!(" AND f.path LIKE ?{i} ESCAPE '\\'"));
    }
    for e in &q.exts {
        let i = next(args, e.clone());
        sql.push_str(&format!(" AND f.ext = ?{i}"));
    }
    for p in &q.pages {
        let i = next(args, p.clone());
        sql.push_str(&format!(
            " AND EXISTS (SELECT 1 FROM links l WHERE l.file_id = f.id AND l.target = ?{i})"
        ));
    }
    if let Some(a) = &q.after {
        let i = next(args, a.clone());
        sql.push_str(&format!(" AND f.doc_date >= ?{i}"));
    }
    if let Some(b) = &q.before {
        let i = next(args, b.clone());
        sql.push_str(&format!(" AND f.doc_date <= ?{i}"));
    }
}

fn row_to_hit(r: &rusqlite::Row) -> rusqlite::Result<Hit> {
    Ok(Hit {
        path: r.get(0)?,
        line: r.get::<_, i64>(1)? as u32,
        line_end: r.get::<_, i64>(2)? as u32,
        text: r.get(3)?,
        breadcrumb: r.get(4)?,
        level: r.get(5)?,
        doc_date: r.get(6)?,
        agent_by: r.get(7)?,
        human_verified: r.get::<_, i64>(8)? != 0,
        score: 0.0,
    })
}

fn finish(
    rows: Vec<(Hit, f64, bool)>,
    q: &Query,
    limit: usize,
    today: &str,
) -> rusqlite::Result<Vec<Hit>> {
    let mut out: Vec<Hit> = Vec::new();
    for (mut hit, rank, is_annotation) in rows {
        // A quoted phrase means "these words, in this order". The index stores
        // OVERLAPPING tokens, so FTS can only tell us the words are all present
        // — adjacency has to be rechecked against the stored text.
        let mut phrase_exact = false;
        if !q.phrases.is_empty() {
            let hay = hit.text.to_lowercase();
            if !q.phrases.iter().all(|p| hay.contains(&p.to_lowercase())) {
                continue;
            }
            phrase_exact = true;
        }
        hit.score = score_of(rank, &hit, is_annotation, phrase_exact, today);
        out.push(hit);
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    Ok(out)
}

/// FTS5's `bm25()` returns NEGATIVE values, more negative meaning more relevant.
/// The design spec's literal `1/(1+rank)` is non-monotonic there (and can go
/// negative), so we work with `r = -bm25` — non-negative, larger is better —
/// apply the multiplicative business boosts to `r`, and only then squash into
/// (0,1) with `r/(1+r)`. Squashing last keeps the ordering the boosts produced.
///
/// The boost constants are STARTING VALUES. Changing them means re-running the
/// retrievability regression set — they encode a product claim (§4): content you
/// have judged outranks content a model produced.
fn score_of(rank: f64, hit: &Hit, is_annotation: bool, phrase_exact: bool, today: &str) -> f64 {
    let mut r = if rank < 0.0 { -rank } else { 0.001 };
    if phrase_exact {
        r *= 1.3;
    }
    if hit.level == "file" || hit.level == "section" {
        r *= 1.2;
    }
    if is_annotation {
        r *= 1.2;
    }
    if hit.human_verified {
        r *= 1.1;
    }
    // The first line of defense against memory self-propagation: AI-authored
    // material is findable but never outranks the primary source it summarized.
    if hit.agent_by.is_some() {
        r *= 0.85;
    }
    if let Some(age) = hit.doc_date.as_deref().and_then(|d| days_between(d, today)) {
        r *= 1.0 + 0.2 * (-(age as f64) / 180.0).exp();
    }
    r / (1.0 + r)
}

/// Whole days from `from` to `to`, both `YYYY-MM-DD`. `None` on unparseable input.
fn days_between(from: &str, to: &str) -> Option<i64> {
    Some((days_from_civil(to)? - days_from_civil(from)?).max(0))
}

fn days_from_civil(ymd: &str) -> Option<i64> {
    let mut it = ymd.split('-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: i64 = it.next()?.parse().ok()?;
    let d: i64 = it.next()?.get(..2).unwrap_or("").parse().ok()?;
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}
```

在 `lib.rs` 加 `pub mod query;`。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path searchidx/Cargo.toml query -- --nocapture`
Expected: 13 个测试 PASS。若 `annotations_outrank_agent_authored_blocks` 边界很近,**不要**为了过测试去调常量——先确认加成确实被应用了(打印 score),常量是产品主张,不是拟合参数。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/query.rs searchidx/src/lib.rs
git commit -m "feat(searchidx): query language, bm25 ranking with provenance boosts, bounded LIKE fallback"
```

---

### Task 11: `SearchIndex` 门面 + spec §7 验收测试集

把散件收成一个对象,并把 spec §7 的指标做成会失败的测试。**验收测试就是这个功能的定义**——没有这些,前面十个任务只是"看起来能跑"。

**Files:**
- Modify: `searchidx/src/lib.rs`
- Create: `searchidx/tests/acceptance.rs`
- Create: `searchidx/tests/fixtures/retrievability.json`
- Create: `searchidx/tests/fixtures/corpus/`(约 12 个小文件,覆盖中英混排、大纲、批注、代码围栏、CRLF、未登录词、单字)

**Interfaces:**
- Produces:
  - `pub struct SearchIndex { .. }`
  - `SearchIndex::open(vault_root: &Path) -> Result<Self, String>`
  - `SearchIndex::open_at(vault_root: &Path, db_path: &Path) -> Result<Self, String>`
  - `SearchIndex::ensure_built(&mut self, opts: &ScanOptions) -> Result<ScanStats, String>`
  - `SearchIndex::sweep(&mut self, opts: &ScanOptions, deadline: Option<Duration>) -> Result<ScanStats, String>`
  - `SearchIndex::rebuild(&mut self, opts: &ScanOptions) -> Result<ScanStats, String>`
  - `SearchIndex::index_one(&mut self, rel: &str, opts: &ScanOptions) -> Result<bool, String>`
  - `SearchIndex::search(&self, raw: &str, limit: usize) -> Result<(Vec<Hit>, Route), String>`
  - `SearchIndex::stats(&self) -> Result<IndexStats, String>`
  - `pub struct IndexStats { pub files: i64, pub blocks: i64, pub db_bytes: u64, pub built_at: Option<String>, pub tokenizer_id: String }`

- [ ] **Step 1: 写门面(先让它编译)**

在 `searchidx/src/lib.rs` 追加:

```rust
pub use block::{Block, BlockLevel, FileMeta, Link};
pub use query::{Hit, Query, Route};
pub use scan::{ScanOptions, ScanStats};

use std::path::{Path, PathBuf};
use std::time::Duration;

/// One open index over one vault.
///
/// Errors are `String` on purpose: every caller (Tauri command, CLI, watcher)
/// turns them into a degradation, never a failure the user must act on. See the
/// degradation matrix in the design spec §9.
pub struct SearchIndex {
    conn: rusqlite::Connection,
    vault_root: PathBuf,
    db_path: PathBuf,
}

impl SearchIndex {
    pub fn open(vault_root: &Path) -> Result<Self, String> {
        let db = paths::index_db_path(vault_root).ok_or("no local app data directory")?;
        Self::open_at(vault_root, &db)
    }

    pub fn open_at(vault_root: &Path, db_path: &Path) -> Result<Self, String> {
        let root = vault_root.to_string_lossy().replace('\\', "/");
        let conn = store::open(db_path, &root).map_err(|e| e.to_string())?;
        Ok(SearchIndex {
            conn,
            vault_root: vault_root.to_path_buf(),
            db_path: db_path.to_path_buf(),
        })
    }

    /// Build if the index is empty, otherwise leave it alone. Callers that want
    /// freshness call [`Self::sweep`].
    pub fn ensure_built(&mut self, opts: &ScanOptions) -> Result<ScanStats, String> {
        let files: i64 = self
            .conn
            .query_row("SELECT count(*) FROM files", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if files > 0 {
            return Ok(ScanStats::default());
        }
        self.rebuild(opts)
    }

    pub fn rebuild(&mut self, opts: &ScanOptions) -> Result<ScanStats, String> {
        scan::build_full(&mut self.conn, &self.vault_root, opts).map_err(|e| e.to_string())
    }

    pub fn sweep(&mut self, opts: &ScanOptions, deadline: Option<Duration>) -> Result<ScanStats, String> {
        scan::sweep(&mut self.conn, &self.vault_root, opts, deadline).map_err(|e| e.to_string())
    }

    pub fn index_one(&mut self, rel: &str, opts: &ScanOptions) -> Result<bool, String> {
        scan::index_one(&mut self.conn, &self.vault_root, rel, opts).map_err(|e| e.to_string())
    }

    pub fn search(&self, raw: &str, limit: usize) -> Result<(Vec<Hit>, Route), String> {
        let q = query::parse(raw);
        query::search(&self.conn, &q, limit, &today()).map_err(|e| e.to_string())
    }

    pub fn stats(&self) -> Result<IndexStats, String> {
        let files = self.conn.query_row("SELECT count(*) FROM files", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        let blocks = self.conn.query_row("SELECT count(*) FROM blocks", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        Ok(IndexStats {
            files,
            blocks,
            db_bytes: std::fs::metadata(&self.db_path).map(|m| m.len()).unwrap_or(0),
            built_at: store::meta_get(&self.conn, "built_at"),
            tokenizer_id: store::meta_get(&self.conn, "tokenizer_id").unwrap_or_default(),
        })
    }

    pub fn vault_root(&self) -> &Path {
        &self.vault_root
    }
}

#[derive(Debug, Clone)]
pub struct IndexStats {
    pub files: i64,
    pub blocks: i64,
    pub db_bytes: u64,
    pub built_at: Option<String>,
    pub tokenizer_id: String,
}

fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    chunk::ymd_from_unix_public(secs)
}
```

同时在 `chunk.rs` 里把 `ymd_from_unix` 暴露成 `pub fn ymd_from_unix_public(secs: i64) -> String { ymd_from_unix(secs) }`。

Run: `cargo build --manifest-path searchidx/Cargo.toml`
Expected: 编译通过。

- [ ] **Step 2: 写语料与 Retrievability 回归集**

`searchidx/tests/fixtures/corpus/` 放约 12 个小文件,**必须**覆盖:中英混排段落、`.note.md` 大纲(含 `type:: annotation` 与 `by:: claude/1`)、代码围栏、CRLF 文件、frontmatter(含 `tags`/`type`/`verified: human:`)、日期前缀文件名、未登录词(人名)、单字查询目标、`[[wikilink]]`。

`searchidx/tests/fixtures/retrievability.json` —— **已知事实清单**,每条是"查这个,必须能召回那个文件":

```json
[
  { "query": "增量索引", "expect_path": "concepts/2026-01-05-incremental.md", "why": "长词直查" },
  { "query": "增量",     "expect_path": "concepts/2026-01-05-incremental.md", "why": "子词召回:cut_for_search 重叠输出" },
  { "query": "李慕白",   "expect_path": "people/meeting.md",                  "why": "未登录词:LIKE 兜底" },
  { "query": "我",       "expect_path": "notes/single-char.md",               "why": "单字查询" },
  { "query": "\"exact adjacent phrase\"", "expect_path": "prose/phrase.md",   "why": "短语精确复核" },
  { "query": "brownfox tag:x", "expect_path": "prose/tagged.md",              "why": "过滤器与词项组合" },
  { "query": "fence_only_token", "expect_path": "prose/fenced.md",            "why": "代码围栏内容可检" },
  { "query": "crlfmarker", "expect_path": "prose/crlf.md",                    "why": "CRLF 文件正常入索引" }
]
```

> 目标是 **100 条**(spec §7)。先落地上面 8 条把管道跑通,然后按上表的 `why` 分类补到 100 条:每加一条真实用过的检索意图,就等于给排序和分词盲区加一个守门员。**未达 100 条不算 P0 完成**,在 PR 里写明当前条数。

- [ ] **Step 3: 写验收测试(先失败)**

`searchidx/tests/acceptance.rs`:

```rust
//! The design spec's §7 acceptance table, as tests. These are the definition of
//! the feature: everything else is implementation detail.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use searchidx::{ScanOptions, SearchIndex};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/corpus")
}

fn open_temp(vault: &Path) -> (tempfile::TempDir, SearchIndex) {
    let d = tempfile::tempdir().unwrap();
    let idx = SearchIndex::open_at(vault, &d.path().join("index.db")).unwrap();
    (d, idx)
}

/// spec §7:100 条已知事实,CI 常跑。分词盲区与排序回归的守门员。
#[test]
fn retrievability_regression_set_is_fully_recalled() {
    let (_d, mut idx) = open_temp(&corpus());
    idx.rebuild(&ScanOptions::default()).unwrap();

    let cases: Vec<serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/retrievability.json"),
        )
        .unwrap(),
    )
    .unwrap();

    let mut failures = Vec::new();
    for case in &cases {
        let q = case["query"].as_str().unwrap();
        let want = case["expect_path"].as_str().unwrap();
        let (hits, route) = idx.search(q, 20).unwrap();
        if !hits.iter().any(|h| h.path == want) {
            failures.push(format!(
                "  {q:?} → expected {want}, got {:?} (route {})",
                hits.iter().take(3).map(|h| h.path.as_str()).collect::<Vec<_>>(),
                route.as_str()
            ));
        }
    }
    assert!(failures.is_empty(), "retrievability regressions:\n{}", failures.join("\n"));
}

/// spec §7:删库重建逐字节一致(同一 tokenizer_id 下)。索引=纯函数的验收形式。
#[test]
fn rebuilding_from_scratch_is_deterministic() {
    let dump = |idx: &SearchIndex| -> Vec<String> {
        idx.search("", 0).ok();
        let mut all = Vec::new();
        for q in ["a", "的", "the", "note"] {
            for h in idx.search(q, 50).unwrap().0 {
                all.push(format!("{}|{}|{}|{:.9}", h.path, h.line, h.level, h.score));
            }
        }
        all
    };
    let (_d1, mut a) = open_temp(&corpus());
    a.rebuild(&ScanOptions::default()).unwrap();
    let (_d2, mut b) = open_temp(&corpus());
    b.rebuild(&ScanOptions::default()).unwrap();
    assert_eq!(dump(&a), dump(&b));
}

/// spec §7:GUI+CLI 并发写必须自然收敛,不靠锁协商。两个连接交错写同一批文件,
/// 结果必须与单进程重建一致。
#[test]
fn two_writers_converge_without_coordination() {
    let vault = corpus();
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("index.db");

    let mut a = SearchIndex::open_at(&vault, &db).unwrap();
    a.rebuild(&ScanOptions::default()).unwrap();

    let files: Vec<String> = std::fs::read_dir(&vault)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "md"))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    let mut b = SearchIndex::open_at(&vault, &db).unwrap();
    for rel in &files {
        a.index_one(rel, &ScanOptions::default()).unwrap();
        b.index_one(rel, &ScanOptions::default()).unwrap();
        a.index_one(rel, &ScanOptions::default()).unwrap();
    }
    let s = a.stats().unwrap();
    let d2 = tempfile::tempdir().unwrap();
    let mut fresh = SearchIndex::open_at(&vault, &d2.path().join("index.db")).unwrap();
    fresh.rebuild(&ScanOptions::default()).unwrap();
    assert_eq!(s.files, fresh.stats().unwrap().files);
    assert_eq!(s.blocks, fresh.stats().unwrap().blocks, "interleaved writes must not duplicate rows");
}

/// spec §7:无变更 sweep < 300ms。CLI 默认路径的可用性下限。
#[test]
fn an_unchanged_sweep_is_fast() {
    let (_d, mut idx) = open_temp(&corpus());
    idx.rebuild(&ScanOptions::default()).unwrap();
    let t = Instant::now();
    let s = idx.sweep(&ScanOptions::default(), None).unwrap();
    assert_eq!(s.files_indexed, 0);
    // 语料很小,阈值只用来抓「每次 sweep 都重算全量」这类灾难性回归。
    assert!(t.elapsed() < Duration::from_millis(300), "sweep took {:?}", t.elapsed());
}

/// spec §7:查询 p50 < 10ms(索引热)。
#[test]
fn warm_queries_are_fast() {
    let (_d, mut idx) = open_temp(&corpus());
    idx.rebuild(&ScanOptions::default()).unwrap();
    let _ = idx.search("note", 20).unwrap();
    let mut times: Vec<u128> = Vec::new();
    for _ in 0..20 {
        let t = Instant::now();
        let _ = idx.search("note", 20).unwrap();
        times.push(t.elapsed().as_micros());
    }
    times.sort_unstable();
    assert!(times[times.len() / 2] < 10_000, "p50 {}µs", times[times.len() / 2]);
}
```

Run: `cargo test --manifest-path searchidx/Cargo.toml --test acceptance`
Expected: 先失败(语料/回归集还没补齐),按失败信息补语料与实现,直到全绿。

> **`rebuilding_from_scratch_is_deterministic` 里 `idx.search("", 0)` 只是占位**,实现门面时若 `search("")` 返回空即可,不要为它加特例分支。

- [ ] **Step 4: 跑全量测试**

Run: `cargo test --manifest-path searchidx/Cargo.toml`
Expected: 全绿。

- [ ] **Step 5: 提交**

```bash
git add searchidx/src/lib.rs searchidx/src/chunk.rs searchidx/tests/acceptance.rs searchidx/tests/fixtures
git commit -m "feat(searchidx): SearchIndex facade + spec §7 acceptance suite (retrievability, determinism, convergence)"
```

---

### Task 12: `notemd search` CLI

spec §5-L1。命令形状像 grep 是刻意的:agent 从训练数据里内化了 Unix 约定,最友好的接口零学习成本。退出码是 agent 的分支依据;降级永不变成报错。

**Files:**
- Create: `src-tauri/src/cli/search.rs`
- Modify: `src-tauri/src/cli/mod.rs`(`pub mod search;`)
- Modify: `src-tauri/src/cli/router.rs`(`Builtin::Search`)
- Modify: `src-tauri/src/cli/builtin.rs`(分派 + help)
- Create: `src-tauri/tests/search_cli_contract.rs`

**Interfaces:**
- Consumes: `searchidx::{SearchIndex, ScanOptions}`、`crate::shared_config`
- Produces:
  - `pub struct SearchArgs { pub query: Vec<String>, pub vault: Option<String>, pub limit: usize, pub json: bool, pub context: usize, pub no_sweep: bool, pub rebuild: bool, pub stats: bool }`(过滤器旗标会被折进 `query`,没有单独的 filters 字段)
  - `cli::search::parse_args(rest: &[String], json_global: bool) -> SearchArgs`
  - `cli::search::run(args: SearchArgs) -> ExitCode`
  - `cli::search::resolve_vault_root(explicit: Option<&str>) -> Option<PathBuf>`

- [ ] **Step 1: 写失败的契约测试**

`src-tauri/tests/search_cli_contract.rs`:

```rust
//! `notemd search` is an interface for agents as much as for people. Its shape —
//! grep-like output, meaningful exit codes, never failing because the index is
//! unhappy — is the contract; these tests are what stop it from drifting.

use std::path::PathBuf;
use std::process::Command;

fn vault(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (rel, body) in files {
        let p = d.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    d
}

fn search(vault: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    cmd.arg("--cli").arg("search");
    cmd.args(args);
    cmd.arg("--vault").arg(vault);
    cmd.output().expect("spawn")
}

#[test]
fn default_output_is_path_colon_line_colon_text() {
    let v = vault(&[("a.md", "# T\n\nthe brownfox jumped\n")]);
    let out = search(v.path(), &["brownfox"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.lines().any(|l| l.starts_with("a.md:3:")), "{text}");
    assert_eq!(out.status.code(), Some(0));
}

/// agent 靠退出码分支,所以 0/1/2 的含义不能含糊。
#[test]
fn exit_code_one_means_no_matches_not_an_error() {
    let v = vault(&[("a.md", "nothing here\n")]);
    let out = search(v.path(), &["zzzznotfound"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}

#[test]
fn json_output_carries_the_full_contract() {
    let v = vault(&[("2026-01-01-a.md", "brownfox\n")]);
    let out = search(v.path(), &["brownfox", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("valid json");
    assert!(v["route"].as_str().unwrap().starts_with("t1-"));
    assert!(v["took_ms"].is_number());
    let hit = &v["hits"][0];
    for key in ["path", "line", "text", "score", "breadcrumb", "doc_date", "source_ref"] {
        assert!(!hit[key].is_null(), "missing {key} in {hit}");
    }
    assert_eq!(hit["source_ref"].as_str().unwrap(), "2026-01-01-a.md#L1");
    assert!(hit["provenance"]["human_verified"].is_boolean());
}

/// 路径永远是 vault 相对 + `/` 分隔 —— 两平台给 agent 的锚必须一模一样。
#[test]
fn paths_are_vault_relative_with_forward_slashes() {
    let v = vault(&[("docs/sub/a.md", "brownfox\n")]);
    let out = search(v.path(), &["brownfox", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["hits"][0]["path"].as_str().unwrap(), "docs/sub/a.md");
}

/// 降级优于失败:被 agent 用错误搞糊涂的代价大于慢一次。
#[test]
fn an_unusable_index_degrades_to_a_direct_scan_and_still_exits_zero() {
    let v = vault(&[("a.md", "brownfox\n")]);
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    cmd.arg("--cli").arg("search").arg("brownfox").arg("--vault").arg(v.path());
    // Point the local data dir at a file so the index cannot be created.
    let blocker = v.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    #[cfg(unix)] cmd.env("XDG_DATA_HOME", &blocker);
    #[cfg(windows)] cmd.env("LOCALAPPDATA", &blocker);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("a.md:1:"));
    assert!(!out.stderr.is_empty(), "a degradation must be announced on stderr");
}

#[test]
fn missing_vault_is_the_only_hard_error() {
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
    cmd.arg("--cli").arg("search").arg("x").arg("--vault").arg("/definitely/not/here");
    assert_eq!(cmd.output().unwrap().status.code(), Some(2));
}

#[test]
fn stats_reports_the_index_without_searching() {
    let v = vault(&[("a.md", "brownfox\n")]);
    let out = search(v.path(), &["--stats", "--json"]);
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(j["files"].as_i64().unwrap() >= 1);
    assert!(j["tokenizer_id"].is_string());
}

#[test]
fn filters_and_limit_flags_work_as_flags_too() {
    let v = vault(&[("docs/a.md", "brownfox\n"), ("other/b.md", "brownfox\n")]);
    let out = search(v.path(), &["brownfox", "--path", "docs"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("docs/a.md"), "{text}");
    assert!(!text.contains("other/b.md"), "{text}");

    let out = search(v.path(), &["brownfox", "--limit", "1"]);
    assert_eq!(String::from_utf8_lossy(&out.stdout).lines().count(), 1);
}

#[test]
fn context_flag_prints_surrounding_lines() {
    let v = vault(&[("a.md", "one\ntwo\nbrownfox\nfour\nfive\n")]);
    let out = search(v.path(), &["brownfox", "--context", "1"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("two"), "{text}");
    assert!(text.contains("four"), "{text}");
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test search_cli_contract`
Expected: 失败(`unknown command 'search'`)。

- [ ] **Step 3: 加路由**

在 `src-tauri/src/cli/router.rs` 的 `Builtin` 枚举加一项:

```rust
    /// `search <query...>` — full-text search over the configured vault.
    Search(super::search::SearchArgs),
```

在 `resolve_with` 里,`if first == "plugin" { … }` **之前**加:

```rust
    // Core, never disabled: an agent's search must not depend on plugin state.
    if first == "search" {
        return Route::Builtin(Builtin::Search(super::search::parse_args(&rest[1..], false)));
    }
```

在 `src-tauri/src/cli/builtin.rs` 的 `run` 的 match 里加:

```rust
        Builtin::Search(args) => super::search::run(args.with_global_json(parsed.globals.json)),
```

在 `src-tauri/src/cli/mod.rs` 加 `pub mod search;`。

- [ ] **Step 4: 写实现**

`src-tauri/src/cli/search.rs`:

```rust
//! `notemd search` — the zero-token retrieval surface.
//!
//! Shaped like grep on purpose (design spec §5): `path:line:text`, exit code 0
//! for hits / 1 for none / 2 for a real error. Claude Code, Codex and friends
//! internalized Unix conventions from their training data, so the friendliest
//! interface is the one that already looks like a tool they know. We are
//! accelerating the loop they already run, not asking them to learn ours.
//!
//! Nothing here decides *what* matches or *how it ranks* — that all lives in
//! `searchidx`, so the CLI and the UI cannot disagree.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use searchidx::{ScanOptions, SearchIndex};

/// The CLI's freshness sweep is bounded: retrieval must never block its caller.
const SWEEP_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default)]
pub struct SearchArgs {
    pub query: Vec<String>,
    pub vault: Option<String>,
    pub limit: usize,
    pub json: bool,
    pub context: usize,
    pub no_sweep: bool,
    pub rebuild: bool,
    pub stats: bool,
}

impl SearchArgs {
    pub fn with_global_json(mut self, global: bool) -> Self {
        self.json = self.json || global;
        self
    }
}

/// Flags map onto the same filter syntax the UI uses: `--tag x` is sugar for
/// `tag:x`, so there is one grammar to learn and one parser to maintain.
pub fn parse_args(rest: &[String], json_global: bool) -> SearchArgs {
    let mut a = SearchArgs { limit: 20, json: json_global, ..Default::default() };
    let mut i = 0usize;
    while i < rest.len() {
        let tok = rest[i].as_str();
        let mut take = |args: &mut SearchArgs, f: &dyn Fn(&mut SearchArgs, String)| {
            if let Some(v) = rest.get(i + 1) {
                f(args, v.clone());
                i += 1;
            }
        };
        match tok {
            "--json" => a.json = true,
            "--no-sweep" => a.no_sweep = true,
            "--rebuild" => a.rebuild = true,
            "--stats" => a.stats = true,
            "--vault" => take(&mut a, &|a, v| a.vault = Some(v)),
            "--limit" => take(&mut a, &|a, v| a.limit = v.parse().unwrap_or(20)),
            "--context" => take(&mut a, &|a, v| a.context = v.parse().unwrap_or(0)),
            "--tag" => take(&mut a, &|a, v| a.query.push(format!("tag:{v}"))),
            "--type" => take(&mut a, &|a, v| a.query.push(format!("type:{v}"))),
            "--path" => take(&mut a, &|a, v| a.query.push(format!("path:{v}"))),
            "--ext" => take(&mut a, &|a, v| a.query.push(format!("ext:{v}"))),
            "--after" => take(&mut a, &|a, v| a.query.push(format!("after:{v}"))),
            "--before" => take(&mut a, &|a, v| a.query.push(format!("before:{v}"))),
            other => a.query.push(other.to_string()),
        }
        i += 1;
    }
    a
}

/// Headless vault-root resolution.
///
/// `sotvault::resolve_vault_root` needs an `AppHandle`, which the CLI does not
/// have, so this reads the shared config directly — the same file the GUI
/// writes. `--vault` is a first-class flag rather than a debugging aid: it is
/// the escape hatch when config resolution is wrong on a given machine.
pub fn resolve_vault_root(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(v) = explicit {
        return Some(PathBuf::from(v));
    }
    let cfg_path = crate::shared_config::config_path().ok()?;
    let cfg = crate::shared_config::read(&cfg_path).ok()?;
    cfg.sotvault.filter(|s| !s.is_empty()).map(PathBuf::from)
}

pub fn run(args: SearchArgs) -> ExitCode {
    let Some(root) = resolve_vault_root(args.vault.as_deref()) else {
        eprintln!("notemd: no vault configured. Set one in Preferences, or pass --vault PATH.");
        return ExitCode::from(2);
    };
    if !root.is_dir() {
        eprintln!("notemd: vault not found: {}", root.display());
        return ExitCode::from(2);
    }

    let started = std::time::Instant::now();
    let opts = scan_options(&root);
    let mut skipped_large: Vec<String> = Vec::new();

    // Every failure below degrades. The only hard error is "no vault".
    let mut index = match SearchIndex::open(&root) {
        Ok(i) => Some(i),
        Err(e) => {
            eprintln!("notemd: search index unavailable ({e}); scanning files directly");
            None
        }
    };

    if let Some(idx) = index.as_mut() {
        let outcome = if args.rebuild {
            idx.rebuild(&opts)
        } else if args.no_sweep {
            idx.ensure_built(&opts)
        } else {
            idx.ensure_built(&opts).and_then(|_| idx.sweep(&opts, Some(SWEEP_DEADLINE)))
        };
        match &outcome {
            Ok(stats) => skipped_large = stats.files_skipped_large.clone(),
            Err(_) => {}
        }
        match outcome {
            Ok(stats) if stats.timed_out => {
                eprintln!("notemd: freshness scan exceeded 2s; answering from the existing index");
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("notemd: index update failed ({e}); scanning files directly");
                index = None;
            }
        }
    }

    if args.stats {
        return report_stats(index.as_ref(), args.json, &skipped_large);
    }

    let query = args.query.join(" ");
    if query.trim().is_empty() {
        eprintln!("notemd: usage: notemd search <query...> [--vault PATH] [--limit N] [--json]");
        return ExitCode::from(2);
    }

    let (hits, route) = match index.as_ref().map(|i| i.search(&query, args.limit)) {
        Some(Ok(r)) => r,
        Some(Err(e)) => {
            eprintln!("notemd: query failed ({e}); scanning files directly");
            (fallback_scan(&root, &query, args.limit, &opts), searchidx::Route::Scan)
        }
        None => (fallback_scan(&root, &query, args.limit, &opts), searchidx::Route::Scan),
    };

    let took = started.elapsed().as_millis();
    if args.json {
        print_json(&query, route, took, &hits);
    } else {
        print_plain(&root, &hits, args.context);
    }
    if hits.is_empty() { ExitCode::from(1) } else { ExitCode::from(0) }
}

fn scan_options(root: &Path) -> ScanOptions {
    let vs = crate::sotvault::vault_settings::read(root);
    ScanOptions {
        large_file_threshold_mb: vs.large_file_threshold_mb.unwrap_or(10),
        exclude_dirs: vs.search_exclude_dirs.unwrap_or_default(),
    }
}

/// Last-ditch retrieval with no index at all: walk the vault and substring-match.
/// Slower and unranked, but the caller gets an answer instead of an excuse.
fn fallback_scan(root: &Path, query: &str, limit: usize, opts: &ScanOptions) -> Vec<searchidx::Hit> {
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    for entry in ignore::WalkBuilder::new(root).hidden(true).follow_links(false).build().flatten() {
        if out.len() >= limit {
            break;
        }
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Some(rel) = searchidx::norm::rel_path(root, entry.path()) else { continue };
        if !searchidx::scan::is_indexable(&rel, opts) {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(entry.path()) else { continue };
        for (i, line) in searchidx::norm::strip_cr(&raw).lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                out.push(searchidx::Hit {
                    path: rel.clone(),
                    line: i as u32 + 1,
                    line_end: i as u32 + 1,
                    text: line.trim().to_string(),
                    breadcrumb: String::new(),
                    level: "line".into(),
                    score: 0.0,
                    doc_date: None,
                    agent_by: None,
                    human_verified: false,
                });
                break;
            }
        }
    }
    out
}

fn print_plain(root: &Path, hits: &[searchidx::Hit], context: usize) {
    for h in hits {
        if context > 0 {
            for (n, text) in context_lines(root, h, context) {
                println!("{}:{}:{}", h.path, n, text);
            }
        } else {
            println!("{}:{}:{}", h.path, h.line, one_line(&h.text));
        }
    }
}

fn context_lines(root: &Path, hit: &searchidx::Hit, context: usize) -> Vec<(u32, String)> {
    let Ok(raw) = std::fs::read_to_string(root.join(&hit.path)) else {
        return vec![(hit.line, one_line(&hit.text))];
    };
    let text = searchidx::norm::strip_cr(&raw);
    let lines: Vec<&str> = text.lines().collect();
    let from = hit.line.saturating_sub(context as u32).max(1);
    let to = (hit.line_end as usize + context).min(lines.len()) as u32;
    (from..=to).filter_map(|n| lines.get(n as usize - 1).map(|l| (n, l.trim().to_string()))).collect()
}

/// Collapse a multi-line block to one grep-shaped line, capped so a long
/// paragraph cannot flood an agent's context.
fn one_line(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= 200 {
        joined
    } else {
        joined.chars().take(200).collect::<String>() + "…"
    }
}

fn print_json(query: &str, route: searchidx::Route, took_ms: u128, hits: &[searchidx::Hit]) {
    let arr: Vec<serde_json::Value> = hits
        .iter()
        .map(|h| {
            serde_json::json!({
                "path": h.path,
                "line": h.line,
                "line_end": h.line_end,
                "text": h.text,
                "score": h.score,
                "breadcrumb": h.breadcrumb,
                "level": h.level,
                "doc_date": h.doc_date,
                "source_ref": h.source_ref(),
                // Surfaced so an agent can prefer primary sources over
                // AI-authored summaries of them (design spec §5-T3).
                "provenance": { "agent_by": h.agent_by, "human_verified": h.human_verified },
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({
            "query": query, "route": route.as_str(), "took_ms": took_ms,
            "total": hits.len(), "hits": arr
        })
    );
}

fn report_stats(index: Option<&SearchIndex>, json: bool, skipped: &[String]) -> ExitCode {
    let Some(idx) = index else {
        eprintln!("notemd: no index available");
        return ExitCode::from(1);
    };
    match idx.stats() {
        Ok(s) if json => {
            println!(
                "{}",
                serde_json::json!({
                    "files": s.files, "blocks": s.blocks, "db_bytes": s.db_bytes,
                    "built_at": s.built_at, "tokenizer_id": s.tokenizer_id
                })
            );
            ExitCode::from(0)
        }
        Ok(s) => {
            println!("files      {}", s.files);
            println!("blocks     {}", s.blocks);
            println!("db size    {:.1} MB", s.db_bytes as f64 / 1_048_576.0);
            println!("tokenizer  {}", s.tokenizer_id);
            // Spec §3.7/§9: a file skipped by the size guardrail is invisible to
            // search, so `--stats` has to say so — an unexplained miss is worse
            // than a slow query.
            for path in &skipped {
                println!("skipped    {path} (over the size threshold; rg still finds it)");
            }
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("notemd: {e}");
            ExitCode::from(1)
        }
    }
}
```

> **三条实现注意:**
> 1. 上面 `parse_args` 里的 `take` 闭包同时可变借用 `a` 和 `i`,**借不过**。落地时展开成朴素的 `match` + `if let Some(v) = rest.get(i+1) { …; i += 1; }`,形状照抄语义即可 —— 写成闭包只是为了在计划里把 12 个旗标并排讲清楚。
> 2. `fallback_scan` 用到 `ignore` —— 在 `src-tauri/Cargo.toml` 加 `ignore = "0.4"`(searchidx 已有,这里是宿主自己的用法)。
> 3. `crate::sotvault::vault_settings` 与 `crate::cli` 需要是 `pub`(`sotvault/mod.rs` 与 `lib.rs` 里),Task 15 的契约测试也依赖后者。

- [ ] **Step 5: 跑契约测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test search_cli_contract`
Expected: 10 个测试 PASS。`--stats` 那条若因为 vault 未配置而走 exit 2,检查 `--vault` 是否被 `parse_args` 正确吃到。

- [ ] **Step 6: 加启动预算测试**

在 `src-tauri/tests/cli_startup_timing.rs` 末尾追加(沿用文件里已有的 `cli_mode` 辅助):

```rust
/// Two budgets, because they measure different things: an ASCII query never
/// touches the Chinese dictionary, a CJK one pays to decompress and parse it
/// exactly once. Conflating them would either hide an ASCII regression or fail
/// spuriously on a cold dictionary.
#[test]
fn search_meets_both_startup_budgets() {
    #[cfg(debug_assertions)]
    const (ASCII_MS, CJK_MS): (u128, u128) = (4000, 6000);
    #[cfg(not(debug_assertions))]
    const (ASCII_MS, CJK_MS): (u128, u128) = (800, 1200);

    let vault = std::env::temp_dir().join(format!("notemd-search-timing-{}", std::process::id()));
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(vault.join("a.md"), "brownfox 全文检索\n").unwrap();

    let run = |q: &str| -> u128 {
        let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_notemd")));
        cli_mode(&mut cmd);
        cmd.args(["search", q, "--vault"]).arg(&vault);
        let t = Instant::now();
        let _ = cmd.output().expect("spawn");
        t.elapsed().as_millis()
    };
    let _ = run("warmup");
    let ascii = run("brownfox");
    let cjk = run("全文检索");
    let _ = std::fs::remove_dir_all(&vault);
    assert!(ascii < ASCII_MS, "ascii search took {ascii} ms (budget {ASCII_MS})");
    assert!(cjk < CJK_MS, "cjk search took {cjk} ms (budget {CJK_MS})");
}
```

> Rust 不支持 `const (A, B): (u128,u128)` 这种解构常量 —— 实现时写成两个独立 `const`,上面的形状只是为了把两档预算并排讲清楚。

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test cli_startup_timing --release`
Expected: PASS。

- [ ] **Step 7: 提交**

```bash
git add src-tauri/src/cli/search.rs src-tauri/src/cli/mod.rs src-tauri/src/cli/router.rs src-tauri/src/cli/builtin.rs src-tauri/tests/search_cli_contract.rs src-tauri/tests/cli_startup_timing.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(cli): notemd search — grep-shaped zero-token retrieval with bounded freshness sweep"
```

---

### Task 13: vault 设置 `searchExcludeDirs`

> **执行顺序提醒:** Task 12 的 `scan_options()` 读了 `vault_settings.search_exclude_dirs`。若按编号顺序执行,请**先做本任务再做 Task 12**,否则 Task 12 编译不过。两者互不依赖对方的测试。

spec §3.7:排除权给用户,不替用户拍板。默认空——不预设哪些目录"不值得搜"。

**Files:**
- Modify: `src-tauri/src/sotvault/vault_settings.rs`
- Modify: `src-tauri/src/sotvault/mod.rs`
- Modify: `src/lib/vault-settings.svelte.ts`
- Modify: `src/lib/i18n/{en,zh,ja,de}.ts`
- Modify: 设置面板里已有 `largeFileThresholdMb` 输入的那个组件(`grep -rn "largeFileThresholdMb" src/components`)

**Interfaces:**
- Produces: `VaultSettings.search_exclude_dirs: Option<Vec<String>>`(JSON 键 `searchExcludeDirs`)

- [ ] **Step 1: 写失败的测试**

在 `src-tauri/src/sotvault/vault_settings.rs` 的 `mod tests` 里加:

```rust
    #[test]
    fn search_exclude_dirs_round_trips_and_defaults_to_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let s = VaultSettings { search_exclude_dirs: Some(vec!["sessions".into()]), ..Default::default() };
        write(tmp.path(), &s).unwrap();
        assert_eq!(read(tmp.path()).search_exclude_dirs, Some(vec!["sessions".to_string()]));

        // 未设置时不写进文件:没碰过这个设置的用户,settings.json 必须逐字节不变。
        write(tmp.path(), &VaultSettings::default()).unwrap();
        let txt = std::fs::read_to_string(tmp.path().join(".notemd/settings.json")).unwrap();
        assert!(!txt.contains("searchExcludeDirs"), "{txt}");
    }

    /// 每一项都过 validate_rel_dir:绝对路径与 `..` 不能变成扫描排除规则。
    #[test]
    fn merge_validates_every_exclude_entry() {
        let ok = merge(VaultSettings::default(), None, None, None, None, None, Some(vec!["a/b".into()])).unwrap();
        assert_eq!(ok.search_exclude_dirs, Some(vec!["a/b".to_string()]));
        assert!(merge(VaultSettings::default(), None, None, None, None, None, Some(vec!["../x".into()])).is_err());
        assert!(merge(VaultSettings::default(), None, None, None, None, None, Some(vec!["/abs".into()])).is_err());
    }

    /// 空数组是有意义的输入(= 清空排除),不能被当成"没提供"。
    #[test]
    fn an_empty_list_clears_the_exclusions() {
        let base = VaultSettings { search_exclude_dirs: Some(vec!["x".into()]), ..Default::default() };
        let out = merge(base, None, None, None, None, None, Some(vec![])).unwrap();
        assert_eq!(out.search_exclude_dirs, Some(vec![]));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml vault_settings`
Expected: 编译失败(字段与 `merge` 第 7 参数不存在)。

- [ ] **Step 3: 写实现**

在 `VaultSettings` 结构体末尾加:

```rust
    /// Vault-relative directories excluded from the search index.
    ///
    /// Absent (not `[]`) means "never configured". Empty on purpose by default:
    /// which of your directories are not worth searching is your call, not ours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_exclude_dirs: Option<Vec<String>>,
```

`merge` 签名末尾加 `search_exclude_dirs: Option<Vec<String>>`,函数体加:

```rust
    if let Some(list) = search_exclude_dirs {
        let mut out_dirs = Vec::with_capacity(list.len());
        for raw in list {
            out_dirs.push(validate_rel_dir(&raw)?);
        }
        out.search_exclude_dirs = Some(out_dirs);
    }
```

在 `src-tauri/src/sotvault/mod.rs` 的 `notemd_vault_settings_set` 加同名参数并透传(与既有 `large_file_threshold_mb` 完全同构)。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml vault_settings`
Expected: PASS。同时 `cargo build --manifest-path src-tauri/Cargo.toml` 通过(`merge` 的其他调用点要补 `None`)。

- [ ] **Step 5: 前端设置项**

在 `src/lib/vault-settings.svelte.ts` 的类型与 setter 里加 `searchExcludeDirs?: string[]`,在设置面板里加一个多行文本框(一行一个目录),保存时按行 split + trim + 去空。i18n 键:

```ts
  'settings.searchExcludeDirs': 'Directories excluded from search',
  'settings.searchExcludeDirsHint': 'One per line, relative to the vault root. Empty by default.',
```

四语言同批补齐(zh:「排除出搜索的目录」/「每行一个,相对 vault 根目录。默认为空。」;ja/de 同理)。

Run: `pnpm check && pnpm test`
Expected: 通过。

- [ ] **Step 6: 提交**

```bash
# 设置面板文件用 `grep -rln "largeFileThresholdMb" src/components` 定位后填进来
git add src-tauri/src/sotvault/vault_settings.rs src-tauri/src/sotvault/mod.rs src/lib/vault-settings.svelte.ts src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(settings): vault-scoped searchExcludeDirs"
```

---

### Task 14: watcher —— 保存后 500ms 内可检索

spec §3.8。纯决策逻辑(去抖窗口、洪峰阈值)放核心 crate 可单测;notify 绑定留在宿主。**不与 `vault_sync` 的 watcher 合并**——那会强耦合它的 `run_loop`,风险不值得,列为 P3 技债。

**Files:**
- Create: `searchidx/src/watch.rs`
- Create: `src-tauri/src/search/mod.rs`
- Create: `src-tauri/src/search/watch.rs`
- Modify: `searchidx/src/lib.rs`、`src-tauri/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub const DEBOUNCE_MS: u64 = 300; pub const FLOOD_THRESHOLD: usize = 500;`
  - `pub struct Pending { .. }`,`Pending::note(rel: String)`、`Pending::take() -> Batch`
  - `pub enum Batch { Files(Vec<String>), FullSweep }`
  - `crate::search::index_state(app) -> Arc<Mutex<Option<SearchIndex>>>`
  - `crate::search::watch::restart(app: &AppHandle, vault_root: &Path)`

- [ ] **Step 1: 写失败的测试**

`searchidx/src/watch.rs` 末尾:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_paths_accumulate_and_drain_once() {
        let mut p = Pending::default();
        p.note("a.md".into());
        p.note("b.md".into());
        match p.take() {
            Batch::Files(mut v) => { v.sort(); assert_eq!(v, vec!["a.md", "b.md"]); }
            other => panic!("{other:?}"),
        }
        assert!(matches!(p.take(), Batch::Files(v) if v.is_empty()));
    }

    /// 同一个文件在去抖窗口里被写十次,只重索引一次 —— 编辑器的自动保存就是这样。
    #[test]
    fn repeated_writes_to_one_file_collapse() {
        let mut p = Pending::default();
        for _ in 0..10 { p.note("a.md".into()); }
        assert!(matches!(p.take(), Batch::Files(v) if v.len() == 1));
    }

    /// 洪峰(git checkout、批量同步)时逐文件更新比全量还慢,直接降级。
    #[test]
    fn a_flood_degrades_to_a_full_sweep() {
        let mut p = Pending::default();
        for i in 0..(FLOOD_THRESHOLD + 1) { p.note(format!("f{i}.md")); }
        assert!(matches!(p.take(), Batch::FullSweep));
        // 降级后状态必须复位,否则下一批永远是 FullSweep。
        p.note("a.md".into());
        assert!(matches!(p.take(), Batch::Files(v) if v == vec!["a.md"]));
    }

    #[test]
    fn the_debounce_window_is_300ms() {
        assert_eq!(DEBOUNCE_MS, 300);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path searchidx/Cargo.toml watch`
Expected: 编译失败。

- [ ] **Step 3: 写 `searchidx/src/watch.rs`**

```rust
//! Debounce and flood-degradation decisions for the file watcher.
//!
//! Pure and platform-free so it can be tested without a filesystem: macOS
//! (FSEvents) and Windows (ReadDirectoryChangesW) deliver different event
//! sequences for the same rename or delete, and the *policy* — collapse repeats,
//! give up on per-file updates past a threshold — must not depend on which.

use std::collections::HashSet;

/// Matches the backlink layer's existing debounce. Long enough to collapse an
/// editor's save burst, short enough to stay under the 500 ms
/// save-to-searchable budget.
pub const DEBOUNCE_MS: u64 = 300;

/// Past this many distinct files in one window, a full sweep is cheaper than
/// per-file updates — and this is what a `git checkout` or a vault sync looks
/// like.
pub const FLOOD_THRESHOLD: usize = 500;

#[derive(Debug)]
pub enum Batch {
    Files(Vec<String>),
    FullSweep,
}

#[derive(Debug, Default)]
pub struct Pending {
    paths: HashSet<String>,
    flooded: bool,
}

impl Pending {
    pub fn note(&mut self, rel: String) {
        if self.flooded {
            return;
        }
        self.paths.insert(rel);
        if self.paths.len() > FLOOD_THRESHOLD {
            self.flooded = true;
            self.paths.clear();
        }
    }

    pub fn is_empty(&self) -> bool {
        !self.flooded && self.paths.is_empty()
    }

    /// Drain, resetting to the empty state — including the flood flag, so one
    /// burst does not condemn every later batch to a full sweep.
    pub fn take(&mut self) -> Batch {
        if std::mem::take(&mut self.flooded) {
            self.paths.clear();
            return Batch::FullSweep;
        }
        Batch::Files(std::mem::take(&mut self.paths).into_iter().collect())
    }
}
```

在 `lib.rs` 加 `pub mod watch;`。

Run: `cargo test --manifest-path searchidx/Cargo.toml watch`
Expected: 4 个测试 PASS。

- [ ] **Step 4: 写宿主侧状态与 notify 绑定**

`src-tauri/src/search/mod.rs`:

```rust
//! The Tauri adapter: an index handle in app state, three commands, one watcher.
//! Deliberately thin — every decision about scanning, tokenizing and ranking is
//! `searchidx`'s, so the GUI and the CLI cannot answer the same query
//! differently.

pub mod watch;

use std::path::Path;
use std::sync::{Arc, Mutex};

use searchidx::{ScanOptions, SearchIndex};
use tauri::{AppHandle, Manager};

/// `None` until a vault is configured (or after a failed open — the index is
/// optional, the app is not).
pub type IndexHandle = Arc<Mutex<Option<SearchIndex>>>;

pub fn init(app: &AppHandle) {
    app.manage::<IndexHandle>(Arc::new(Mutex::new(None)));
}

pub fn handle(app: &AppHandle) -> IndexHandle {
    app.state::<IndexHandle>().inner().clone()
}

pub fn scan_options(vault_root: &Path) -> ScanOptions {
    let vs = crate::sotvault::vault_settings::read(vault_root);
    ScanOptions {
        large_file_threshold_mb: vs.large_file_threshold_mb.unwrap_or(10),
        exclude_dirs: vs.search_exclude_dirs.unwrap_or_default(),
    }
}

/// Open (building if empty) the index for `vault_root` and start watching.
/// Failures are logged and swallowed: a broken index must never keep the vault
/// from opening.
pub fn open_vault(app: &AppHandle, vault_root: &Path) {
    let handle = handle(app);
    let root = vault_root.to_path_buf();
    let app = app.clone();
    std::thread::spawn(move || {
        let opts = scan_options(&root);
        match SearchIndex::open(&root) {
            Ok(mut idx) => {
                if let Err(e) = idx.ensure_built(&opts) {
                    eprintln!("[search] initial build failed: {e}");
                }
                if let Err(e) = idx.sweep(&opts, None) {
                    eprintln!("[search] sweep failed: {e}");
                }
                *handle.lock().unwrap() = Some(idx);
                watch::restart(&app, &root);
            }
            Err(e) => eprintln!("[search] index unavailable: {e}"),
        }
    });
}
```

`src-tauri/src/search/watch.rs`:

```rust
//! notify wiring. All policy lives in `searchidx::watch`; this file only turns
//! OS events into `Pending::note` calls and drives the drain loop.
//!
//! A watcher of its own rather than a subscriber to `vault_sync`'s: that one is
//! tightly coupled to its `run_loop`, and merging them would put a new feature's
//! bugs inside the sync path. Listed as P3 debt in the design spec, on purpose.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use searchidx::watch::{Batch, Pending, DEBOUNCE_MS};
use tauri::{AppHandle, Emitter};

pub fn restart(app: &AppHandle, vault_root: &Path) {
    let (tx, rx) = mpsc::channel::<String>();
    let root = vault_root.to_path_buf();
    let filter_root = root.clone();

    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            let Ok(event) = res else { return };
            if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)) {
                return;
            }
            for p in &event.paths {
                if let Some(rel) = searchidx::norm::rel_path(&filter_root, p) {
                    if rel.ends_with(".md") && !rel.split('/').any(|s| s.starts_with('.')) {
                        let _ = tx.send(rel);
                    }
                }
            }
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[search] watcher unavailable: {e}");
            return;
        }
    };
    if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
        eprintln!("[search] cannot watch {}: {e}", root.display());
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || {
        // Keep the watcher alive for the life of this thread.
        let _watcher = watcher;
        let mut pending = Pending::default();
        loop {
            match rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
                Ok(rel) => pending.note(rel),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if pending.is_empty() {
                        continue;
                    }
                    drain(&app, &root, pending.take());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

fn drain(app: &AppHandle, root: &PathBuf, batch: Batch) {
    let handle = crate::search::handle(app);
    let opts = crate::search::scan_options(root);
    let mut guard = handle.lock().unwrap();
    let Some(idx) = guard.as_mut() else { return };
    let result = match batch {
        Batch::Files(paths) => {
            let mut ok = true;
            for rel in paths {
                if let Err(e) = idx.index_one(&rel, &opts) {
                    eprintln!("[search] reindex {rel} failed: {e}");
                    ok = false;
                }
            }
            ok
        }
        Batch::FullSweep => idx.sweep(&opts, None).is_ok(),
    };
    if result {
        // Lets an open search panel refresh without polling.
        let _ = app.emit("search://index-updated", ());
    }
}
```

在 `src-tauri/src/lib.rs`:`mod search;`;在 setup 里 `search::init(&app.handle());`,并在**已有的 vault 打开/切换回调**旁调用 `search::open_vault(&app_handle, Path::new(&path_str))`(与 `vault_sync` 起停同一处,`grep -n "vault_sync_start" src-tauri/src/lib.rs` 找)。

- [ ] **Step 5: 写新鲜度集成测试**

`searchidx/tests/acceptance.rs` 追加(在核心侧测,不需要 GUI):

```rust
/// spec §7:保存 → 可检索 < 500ms。这里测的是"重索引一个文件"本身的成本,
/// 300ms 去抖之外还剩多少预算。
#[test]
fn reindexing_one_file_is_well_under_the_freshness_budget() {
    let v = tempfile::tempdir().unwrap();
    std::fs::write(v.path().join("a.md"), "before\n").unwrap();
    let d = tempfile::tempdir().unwrap();
    let mut idx = SearchIndex::open_at(v.path(), &d.path().join("index.db")).unwrap();
    idx.rebuild(&ScanOptions::default()).unwrap();

    std::fs::write(v.path().join("a.md"), "after brownfox\n").unwrap();
    let t = Instant::now();
    idx.index_one("a.md", &ScanOptions::default()).unwrap();
    let took = t.elapsed();
    assert!(!idx.search("brownfox", 5).unwrap().0.is_empty());
    assert!(took < Duration::from_millis(200), "single-file reindex took {took:?}");
}
```

Run: `cargo test --manifest-path searchidx/Cargo.toml && cargo build --manifest-path src-tauri/Cargo.toml`
Expected: 全绿 + 编译通过。

- [ ] **Step 6: 提交**

```bash
git add searchidx/src/watch.rs searchidx/src/lib.rs searchidx/tests/acceptance.rs src-tauri/src/search src-tauri/src/lib.rs
git commit -m "feat(search): file watcher with debounce + flood degradation, index handle in app state"
```

---

### Task 15: Windows —— CLI 入口垫片与同库契约

spec §6。两条已知缺陷收编进本功能:①CLI 配置目录(**已修**,只补测试锁死);②Windows 无 symlink 分发,CLI 入口不可达(装 `bin\notemd.cmd` 垫片)。

**Files:**
- Create: `src-tauri/installer/hooks.nsi`
- Modify: `src-tauri/tauri.windows.conf.json`
- Modify: `src-tauri/src/cli/mod.rs`(只加测试)
- Create: `src-tauri/tests/search_index_path_contract.rs`

- [ ] **Step 1: 写失败的测试**

`src-tauri/tests/search_index_path_contract.rs`:

```rust
//! GUI and CLI must open the SAME index database. They are separate processes
//! with no channel between them, so if their path math ever diverges the CLI
//! silently answers from an index nobody is updating. The guarantee is
//! structural — one function, in searchidx — and this test is what keeps it so.

use std::path::Path;

#[test]
fn the_gui_and_the_cli_resolve_one_index_path() {
    let vault = Path::new(if cfg!(windows) { r"C:\vault" } else { "/vault" });
    // The CLI path (what src/cli/search.rs uses, via SearchIndex::open).
    let cli = searchidx::paths::index_db_path(vault).unwrap();
    // What the Tauri side must use — the SAME call, not a reimplementation.
    let gui = searchidx::paths::index_db_path(vault).unwrap();
    assert_eq!(cli, gui);
    assert!(cli.to_string_lossy().contains(searchidx::paths::BUNDLE_ID));
}

/// Windows 上 GUI 读 `%APPDATA%\net.notemd.app\shared.json`,CLI 必须读同一个。
/// 这是 headless vault-root 解析的前提:读错文件 = CLI 找不到 vault。
#[test]
fn the_cli_config_dir_is_the_platform_config_dir_for_this_bundle() {
    let dir = mdeditor_lib::cli::resolve_config_dir();
    let expected = dirs::config_dir().unwrap().join("net.notemd.app");
    assert_eq!(dir, expected);
}

/// Windows 陷阱:索引必须在 Local(每设备独立),配置在 Roaming 侧的
/// config_dir —— 两者是不同的目录,不能顺手统一。
#[cfg(windows)]
#[test]
fn the_index_is_local_while_the_config_is_not() {
    let idx = searchidx::paths::index_db_path(Path::new(r"C:\vault")).unwrap();
    let cfg = mdeditor_lib::cli::resolve_config_dir();
    assert_ne!(idx.parent().unwrap().parent().unwrap().parent(), cfg.parent());
    assert!(idx.to_string_lossy().to_lowercase().contains(r"\local\"));
}
```

> 若 `mdeditor_lib::cli` 不是 `pub`,把 `mod cli;` 改成 `pub mod cli;`(`lib.rs` 里)。

- [ ] **Step 2: 跑测试确认失败/通过**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test search_index_path_contract`
Expected: 可见性问题修好后 macOS 上 2 个 PASS。

- [ ] **Step 3: 写 NSIS 垫片**

`src-tauri/installer/hooks.nsi`:

```nsi
; A `notemd` command on Windows.
;
; The GUI executable IS notemd.exe, and cmd's PATHEXT puts .EXE ahead of .CMD —
; so a shim sitting beside it would be shadowed and `notemd search` would open a
; window instead of printing results. Hence a `bin\` subdirectory: only that
; goes on PATH, the GUI executable never does, and the command is spelled the
; same on macOS and Windows — which is the point, because AGENTS.md tells every
; agent one command, not two.

!macro NSIS_HOOK_POSTINSTALL
  CreateDirectory "$INSTDIR\bin"
  FileOpen $0 "$INSTDIR\bin\notemd.cmd" w
  FileWrite $0 '@"%~dp0..\notemd.exe" --cli %*$\r$\n'
  FileClose $0

  ; Append to the *user* PATH (HKCU), never the machine one: this is a per-user
  ; install and rewriting the system PATH from an installer is how PATHs get
  ; destroyed.
  ReadRegStr $1 HKCU "Environment" "Path"
  ${StrContains} $2 "$INSTDIR\bin" "$1"
  StrCmp $2 "" 0 +3
    WriteRegExpandStr HKCU "Environment" "Path" "$1;$INSTDIR\bin"
    SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\bin\notemd.cmd"
  RMDir "$INSTDIR\bin"
!macroend
```

> `${StrContains}` 需要 `!include "StrFunc.nsh"` 或换成 NSIS 内置的 `StrStr` 逻辑。**在 Windows 机上实际跑一次安装器验证**——这段无法在 mac 上测。若 `StrFunc` 不可用,退化成"不判重复,直接追加"是**不可接受**的(会把 PATH 撑爆),改成读出后用 `StrCpy` 循环比对。

`src-tauri/tauri.windows.conf.json` 的 `bundle.windows` 加:

```json
      "nsis": {
        "installerHooks": "installer/hooks.nsi"
      }
```

- [ ] **Step 4: 记录 Windows 手动验收清单**

在本计划末尾的「Windows 手动验收」一节(见文末)补齐勾选项。这一步只是确认清单存在、内容与本任务一致。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/installer/hooks.nsi src-tauri/tauri.windows.conf.json src-tauri/tests/search_index_path_contract.rs src-tauri/src/lib.rs
git commit -m "feat(windows): notemd CLI shim in bin/ + index/config path contract tests"
```

---

# P1 · UI + 约定层

### Task 16: Tauri commands

三个命令,全部是 `searchidx` 的薄包装。**不在这里写任何检索逻辑**——UI 和 CLI 必须给出同一个答案。

**Files:**
- Modify: `src-tauri/src/search/mod.rs`
- Modify: `src-tauri/src/lib.rs`(`invoke_handler` 注册)

**Interfaces:**
- Produces(前端可见):
  - `notemd_search(query: String, limit: Option<usize>) -> Result<SearchResponse, String>`
  - `notemd_search_stats() -> Result<SearchStatsDto, String>`
  - `notemd_search_rebuild() -> Result<SearchStatsDto, String>`
  - `pub struct SearchResponse { pub route: String, pub took_ms: u64, pub total: usize, pub hits: Vec<HitDto> }`
  - `pub struct HitDto { pub path: String, pub abs_path: String, pub line: u32, pub line_end: u32, pub text: String, pub breadcrumb: String, pub level: String, pub score: f64, pub doc_date: Option<String>, pub source_ref: String, pub agent_by: Option<String>, pub human_verified: bool }`

- [ ] **Step 1: 写实现(命令层无单测,靠 Task 17 的 GUI 验证 + 已有核心测试覆盖)**

在 `src-tauri/src/search/mod.rs` 追加:

```rust
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HitDto {
    pub path: String,
    /// Absolute path, so the panel can open the file without re-deriving the
    /// vault root in the frontend.
    pub abs_path: String,
    pub line: u32,
    pub line_end: u32,
    pub text: String,
    pub breadcrumb: String,
    pub level: String,
    pub score: f64,
    pub doc_date: Option<String>,
    pub source_ref: String,
    pub agent_by: Option<String>,
    pub human_verified: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub route: String,
    pub took_ms: u64,
    pub total: usize,
    pub hits: Vec<HitDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatsDto {
    pub files: i64,
    pub blocks: i64,
    pub db_bytes: u64,
    pub built_at: Option<String>,
    pub tokenizer_id: String,
}

#[tauri::command]
pub fn notemd_search(app: AppHandle, query: String, limit: Option<usize>) -> Result<SearchResponse, String> {
    let started = std::time::Instant::now();
    let handle = handle(&app);
    let guard = handle.lock().map_err(|_| "search index busy")?;
    let idx = guard.as_ref().ok_or("search index not ready")?;
    let (hits, route) = idx.search(&query, limit.unwrap_or(50))?;
    let root = idx.vault_root().to_path_buf();
    Ok(SearchResponse {
        route: route.as_str().to_string(),
        took_ms: started.elapsed().as_millis() as u64,
        total: hits.len(),
        hits: hits
            .into_iter()
            .map(|h| HitDto {
                abs_path: root.join(&h.path).to_string_lossy().to_string(),
                source_ref: h.source_ref(),
                path: h.path,
                line: h.line,
                line_end: h.line_end,
                text: h.text,
                breadcrumb: h.breadcrumb,
                level: h.level,
                score: h.score,
                doc_date: h.doc_date,
                agent_by: h.agent_by,
                human_verified: h.human_verified,
            })
            .collect(),
    })
}

#[tauri::command]
pub fn notemd_search_stats(app: AppHandle) -> Result<SearchStatsDto, String> {
    let handle = handle(&app);
    let guard = handle.lock().map_err(|_| "search index busy")?;
    let s = guard.as_ref().ok_or("search index not ready")?.stats()?;
    Ok(SearchStatsDto {
        files: s.files, blocks: s.blocks, db_bytes: s.db_bytes,
        built_at: s.built_at, tokenizer_id: s.tokenizer_id,
    })
}

#[tauri::command]
pub fn notemd_search_rebuild(app: AppHandle) -> Result<SearchStatsDto, String> {
    let handle = handle(&app);
    let mut guard = handle.lock().map_err(|_| "search index busy")?;
    let idx = guard.as_mut().ok_or("search index not ready")?;
    let root = idx.vault_root().to_path_buf();
    idx.rebuild(&scan_options(&root))?;
    let s = idx.stats()?;
    Ok(SearchStatsDto {
        files: s.files, blocks: s.blocks, db_bytes: s.db_bytes,
        built_at: s.built_at, tokenizer_id: s.tokenizer_id,
    })
}
```

在 `src-tauri/src/lib.rs` 的 `invoke_handler` 列表里(`sotvault::*` 那组旁边)加:

```rust
                search::notemd_search,
                search::notemd_search_stats,
                search::notemd_search_rebuild,
```

> **capabilities 不需要改。** 应用自有的 `#[tauri::command]` 在 Tauri 2 里不走权限 allowlist(那是插件命令的机制),主窗口本来就能调。

- [ ] **Step 2: 编译验证**

Run: `cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | tail -5`
Expected: 成功。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/search/mod.rs src-tauri/src/lib.rs
git commit -m "feat(search): Tauri commands for query, stats and rebuild"
```

---

### Task 17: UI 搜索面板

侧栏注册表驱动的新视图 `vault-search`,`CmdOrCtrl+Shift+F` 打开,点击结果跳到文件的对应行。

**Files:**
- Create: `src/lib/search/api.ts`
- Create: `src/lib/search/store.svelte.ts`
- Create: `src/lib/search/store.test.ts`
- Create: `src/components/side-panel/SearchPanel.svelte`
- Modify: `src/lib/side-panel/registry.svelte.ts`
- Modify: `src/lib/commands.ts`
- Modify: `src-tauri/src/lib.rs`(View 菜单项)
- Modify: `src/lib/i18n/{en,zh,ja,de}.ts`

**Interfaces:**
- Consumes: Task 16 的三个命令
- Produces:
  - `searchApi.query(q: string, limit?: number): Promise<SearchResponse>`
  - `searchStore.{ query, hits, route, tookMs, loading, error }`、`searchStore.run(q)`、`searchStore.clear()`
  - 侧栏视图 id `'vault-search'`(`side: 'left'`,`order: 20`)

- [ ] **Step 1: 写失败的 store 测试**

`src/lib/search/store.test.ts`:

```ts
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { searchStore, _setSearchImpl } from './store.svelte'

beforeEach(() => searchStore.clear())

describe('searchStore', () => {
  it('stores hits and the route reported by the backend', async () => {
    _setSearchImpl(async () => ({ route: 't1-fts', tookMs: 3, total: 1, hits: [
      { path: 'a.md', absPath: '/v/a.md', line: 2, lineEnd: 2, text: 'x', breadcrumb: '',
        level: 'line', score: 0.5, docDate: null, sourceRef: 'a.md#L2', agentBy: null, humanVerified: false },
    ] }))
    await searchStore.run('x')
    expect(searchStore.hits.length).toBe(1)
    expect(searchStore.route).toBe('t1-fts')
    expect(searchStore.loading).toBe(false)
  })

  // 一个空查询打一次后端是纯浪费,而且会把上一次的结果闪掉。
  it('does not call the backend for a blank query', async () => {
    const impl = vi.fn()
    _setSearchImpl(impl)
    await searchStore.run('   ')
    expect(impl).not.toHaveBeenCalled()
    expect(searchStore.hits).toEqual([])
  })

  // 索引还没建好时面板必须说人话,而不是抛一个 Rust 错误串给用户看。
  it('surfaces a backend failure as an error message, not a throw', async () => {
    _setSearchImpl(async () => { throw new Error('search index not ready') })
    await searchStore.run('x')
    expect(searchStore.error).toBeTruthy()
    expect(searchStore.loading).toBe(false)
  })

  // 快速输入会连发请求;晚到的旧响应不能覆盖新结果。
  it('ignores a stale response that arrives after a newer one', async () => {
    const resolvers: Array<(v: unknown) => void> = []
    _setSearchImpl(() => new Promise((r) => resolvers.push(r as (v: unknown) => void)))
    const first = searchStore.run('old')
    const second = searchStore.run('new')
    resolvers[1]({ route: 't1-fts', tookMs: 1, total: 1, hits: [{ path: 'new.md' }] })
    resolvers[0]({ route: 't1-fts', tookMs: 1, total: 1, hits: [{ path: 'old.md' }] })
    await Promise.all([first, second])
    expect(searchStore.hits[0].path).toBe('new.md')
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- search/store`
Expected: 模块不存在,失败。

- [ ] **Step 3: 写 `api.ts` 与 `store.svelte.ts`**

```ts
// src/lib/search/api.ts
import { invoke } from '@tauri-apps/api/core'

export interface SearchHit {
  path: string; absPath: string; line: number; lineEnd: number
  text: string; breadcrumb: string; level: 'file' | 'section' | 'line'
  score: number; docDate: string | null; sourceRef: string
  agentBy: string | null; humanVerified: boolean
}
export interface SearchResponse {
  route: string; tookMs: number; total: number; hits: SearchHit[]
}
export interface SearchStats {
  files: number; blocks: number; dbBytes: number
  builtAt: string | null; tokenizerId: string
}

export const searchApi = {
  query: (query: string, limit = 50) =>
    invoke<SearchResponse>('notemd_search', { query, limit }),
  stats: () => invoke<SearchStats>('notemd_search_stats'),
  rebuild: () => invoke<SearchStats>('notemd_search_rebuild'),
}
```

```ts
// src/lib/search/store.svelte.ts
import { searchApi, type SearchHit, type SearchResponse } from './api'

// Injectable so the store is testable without a Tauri host.
let impl: (q: string, limit?: number) => Promise<SearchResponse> = searchApi.query
export function _setSearchImpl(fn: typeof impl) { impl = fn }

// Monotonic request id. Typing fires overlapping queries and the network does
// not promise ordering — without this, a slow early response can overwrite the
// results the user is actually looking at.
let seq = 0

class SearchStore {
  query = $state('')
  hits = $state<SearchHit[]>([])
  route = $state<string | null>(null)
  tookMs = $state(0)
  loading = $state(false)
  error = $state<string | null>(null)

  async run(q: string): Promise<void> {
    this.query = q
    if (!q.trim()) { this.clear(); return }
    const mine = ++seq
    this.loading = true
    this.error = null
    try {
      const res = await impl(q)
      if (mine !== seq) return
      this.hits = res.hits
      this.route = res.route
      this.tookMs = res.tookMs
    } catch (e) {
      if (mine !== seq) return
      this.error = e instanceof Error ? e.message : String(e)
      this.hits = []
    } finally {
      if (mine === seq) this.loading = false
    }
  }

  clear(): void {
    seq++
    this.query = ''
    this.hits = []
    this.route = null
    this.tookMs = 0
    this.loading = false
    this.error = null
  }
}

export const searchStore = new SearchStore()
```

Run: `pnpm test -- search/store`
Expected: 4 个测试 PASS。

- [ ] **Step 4: 写面板组件**

`src/components/side-panel/SearchPanel.svelte` —— 参照 `src/components/history/HistoryPanel.svelte` 的结构与样式变量(别自造一套 CSS token)。要点:

- 顶部输入框,`oninput` 用 200ms 去抖调 `searchStore.run`
- `Enter` 立即查;`Escape` 清空
- 结果行:第一行 `breadcrumb`(灰、省略号截断),第二行高亮文本,右侧 `path:line`
- `✦` 标记 `agentBy != null` 的结果,`●` 标记 `humanVerified`(与产品的人机署名约定一致,别新造图标)
- 点击 → `openFileAtLine(hit.absPath, hit.line)`:复用现有打开逻辑(`grep -n "openPath\|openFile" src/lib/tabs.svelte.ts`),打开后把编辑器滚到该行 —— 参照 `find-replace.svelte.ts` 的滚动做法(**PM 的滚动认焦点所在节点,必须自己滚 `.scroll` 容器**)
- 底部一行状态:`{total} results · {tookMs}ms` + `route === 't1-scan'` 时显示 `t('search.fallbackScan')` 说明这次走了兜底扫描
- `error` 非空时显示错误行 + 「重建索引」按钮(调 `searchApi.rebuild()`)
- 监听 `search://index-updated` 事件,若有活动查询则重跑

- [ ] **Step 5: 注册侧栏视图与快捷键**

`src/lib/side-panel/registry.svelte.ts`,在其他 `registerSideView` 调用旁:

```ts
registerSideView({
  id: 'vault-search',
  side: 'left',
  order: 20,
  title: () => t('search.title'),
  isAvailable: () => !!sotvaultStore.vaultRoot,
  appliesTo: () => true,
  component: () => import('../../components/side-panel/SearchPanel.svelte'),
})
```

`src/lib/commands.ts`:

```ts
  'toggle-vault-search': () => toggleSideView('vault-search'),
```

`src-tauri/src/lib.rs` 的 View 菜单,紧挨 `toggle-folder-view` 那行:

```rust
        .item(&MenuItemBuilder::with_id("toggle-vault-search", menu_label(locale, "view.vaultSearch")).accelerator("CmdOrCtrl+Shift+F").build(app)?)
```

菜单标签需要在后端 `menu_label` 的四语言表里加 `view.vaultSearch`(`grep -n "view.folderView" src-tauri/src` 找到那张表)。

- [ ] **Step 6: i18n**

四语言各加:

```ts
  'search.title': 'Search',
  'search.placeholder': 'Search this vault…',
  'search.noResults': 'No matches',
  'search.resultCount': '{n} results · {ms}ms',
  'search.fallbackScan': 'Dictionary miss — fell back to a direct scan',
  'search.notReady': 'The index is still building',
  'search.rebuild': 'Rebuild index',
  'view.vaultSearch': 'Search',
```

zh:`搜索` / `搜索这个 vault…` / `无匹配` / `{n} 条 · {ms}ms` / `词典未收录——已降级为直接扫描` / `索引仍在构建` / `重建索引`。ja/de 同批补。

- [ ] **Step 7: 跑检查**

Run: `pnpm check && pnpm test`
Expected: 全绿。

- [ ] **Step 8: GUI 实机验证(必须,不能只靠测试)**

按 `docs` 里既有的 dev 验证做法:`pnpm tauri dev`,人工过一遍:

1. `Cmd⇧F` 打开面板,输入 ASCII 词 → 有结果、点击跳到正确的行
2. 输入中文词(如「检索」)→ 有结果
3. 输入一个只在长词里出现的子词 → 命中(验证 cut_for_search 重叠)
4. 输入一个人名/生造词 → 有结果且底部显示「已降级为直接扫描」
5. 在另一个窗口改一个文件并保存,~0.5s 后重跑查询 → 新内容可检索
6. 深色/浅色主题各看一遍,面板样式跟随

**这一步由用户在自己的桌面上做**,不要在无人值守下跑 osascript 自动化。把上面 6 条作为交付给用户的手动测试清单。

- [ ] **Step 9: 提交**

```bash
git add src/lib/search src/components/side-panel/SearchPanel.svelte src/lib/side-panel/registry.svelte.ts src/lib/commands.ts src/lib/i18n src-tauri/src/lib.rs
git commit -m "feat(ui): vault search side panel (Cmd+Shift+F) with provenance markers"
```

---

### Task 18: AGENTS.md 约定层 + `--help`

spec §5-L2:harness 选工具靠它读到的文档,这是杠杆最高的一步。模板加一节;**存量 vault 由 GUI 检测后一键追加、人确认,绝不静默改写**——传播必须过人(信念 3)。

**Files:**
- Modify: `src-tauri/templates/AGENTS.md`
- Modify: `src-tauri/src/agents_sync/logic.rs`(追加判定,纯函数)
- Modify: `src-tauri/src/agents_sync/mod.rs`(command + 一键追加)
- Modify: `src-tauri/src/cli/builtin.rs`(`render_core_topic` 加 `search`)
- Modify: `src/lib/i18n/*`(确认提示文案)

**Interfaces:**
- Produces:
  - `agents_sync::logic::search_section_missing(agents_md: &str) -> bool`
  - `agents_sync::logic::append_search_section(agents_md: &str) -> String`
  - `pub const SEARCH_SECTION: &str`
  - command `notemd_agents_append_search_section(app) -> Result<bool, String>`

- [ ] **Step 1: 写失败的测试**

在 `src-tauri/src/agents_sync/logic.rs` 的 `mod tests`:

```rust
    #[test]
    fn detects_whether_the_search_section_is_present() {
        assert!(search_section_missing("# Vault\n\nnotes\n"));
        assert!(!search_section_missing(&append_search_section("# Vault\n")));
    }

    /// 一键追加必须是**追加**:用户既有内容一个字节都不能动。这条测试就是
    /// 「绝不静默改写」的机器表达。
    #[test]
    fn appending_leaves_existing_content_byte_identical() {
        let before = "# Vault\n\nMy own conventions.\n";
        let after = append_search_section(before);
        assert!(after.starts_with(before), "existing content must be untouched");
        assert!(after.contains("## Searching this vault"));
    }

    #[test]
    fn appending_twice_does_not_duplicate_the_section() {
        let once = append_search_section("# Vault\n");
        assert_eq!(append_search_section(&once), once);
    }

    /// 文件不以换行结尾时不能把新标题粘到最后一行后面。
    #[test]
    fn appending_normalizes_a_missing_trailing_newline() {
        let after = append_search_section("# Vault");
        assert!(after.contains("# Vault\n\n## Searching this vault"), "{after}");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path src-tauri/Cargo.toml agents_sync`
Expected: 编译失败。

- [ ] **Step 3: 写实现**

`src-tauri/src/agents_sync/logic.rs`:

```rust
/// The convention block that teaches any harness how to search this vault.
///
/// Windows and macOS spell the command identically on purpose (the installer
/// puts a `notemd` shim on PATH) — one instruction, not a platform matrix.
pub const SEARCH_SECTION: &str = r#"## Searching this vault

This vault has a local full-text index. Prefer it over a raw `rg` sweep: it is
faster, it knows Chinese word boundaries, and it ranks the notes you have
actually annotated above machine-generated summaries of them.

```
notemd search <query...>            # path:line:text, ranked, exit 1 = no match
notemd search "exact phrase"        # phrase match
notemd search x tag:y type:z        # filters: tag: type: path: ext: after: before: page:[[X]]
notemd search x --json              # adds score, breadcrumb, source_ref, provenance
notemd search x --context 2         # surrounding lines
```

`rg` and `grep` keep working and are never wrong to use — the index is an
accelerator, not a gatekeeper. When a result's `provenance.agent_by` is set, the
text was written by a model: follow its `sources` to the primary document before
relying on it.
"#;

pub fn search_section_missing(agents_md: &str) -> bool {
    !agents_md.contains("## Searching this vault")
}

/// Append-only. Never rewrites, reorders or reformats what is already there:
/// AGENTS.md is the user's file, and a tool that edits it silently is a tool
/// they stop trusting.
pub fn append_search_section(agents_md: &str) -> String {
    if !search_section_missing(agents_md) {
        return agents_md.to_string();
    }
    let mut out = agents_md.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str(SEARCH_SECTION);
    out
}
```

把 `SEARCH_SECTION` 的正文也加进 `src-tauri/templates/AGENTS.md`(新 vault 直接自带),并把模板里既有的清扫协议示例中的 `rg`/`grep` 调用升级为 `notemd search`(`grep -n "rg \|grep " src-tauri/templates/AGENTS.md` 找)。

在 `src-tauri/src/agents_sync/mod.rs` 加命令:

```rust
/// Append the search convention to the vault's AGENTS.md. Returns false when it
/// was already there. The GUI calls this only after the user confirms — this
/// function does not ask, and nothing else may call it.
#[tauri::command]
pub fn notemd_agents_append_search_section(app: tauri::AppHandle) -> Result<bool, String> {
    let root = crate::sotvault::resolve_vault_root(&app).ok_or("Vault not configured")?;
    let path = root.join(logic::AGENTS_FILE_NAME);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if !logic::search_section_missing(&existing) {
        return Ok(false);
    }
    std::fs::write(&path, logic::append_search_section(&existing)).map_err(|e| e.to_string())?;
    Ok(true)
}
```

(`AGENTS_FILE_NAME` 已在 `agents_sync::watcher::AGENTS_FILE`,复用它。)注册进 `invoke_handler`。

前端:在设置页的 vault 区块加一行 —— 检测到 vault 的 AGENTS.md 缺这一节时显示提示 + 「添加」按钮,点了才写。i18n:

```ts
  'search.agentsHint': 'Your AGENTS.md does not tell agents about the search index.',
  'search.agentsAdd': 'Add the section',
  'search.agentsAdded': 'Added to AGENTS.md',
```

- [ ] **Step 4: CLI help**

在 `src-tauri/src/cli/builtin.rs` 的 `render_core_topic` 里加 `"search"` 分支,输出与 `SEARCH_SECTION` 一致的用法块 + 全部旗标 + 退出码说明,并把 `search` 加进 `render_help` 的命令清单。

- [ ] **Step 5: 跑测试**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && pnpm check && pnpm test`
Expected: 全绿。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/agents_sync src-tauri/templates/AGENTS.md src-tauri/src/cli/builtin.rs src-tauri/src/lib.rs src/lib/i18n
git commit -m "feat(agents): 'Searching this vault' convention — template + confirmed one-click append"
```

---

### Task 19: 体积实测、README 表述、收尾验收

spec §3.1/§7/§11:体积承诺变更必须外显,不能靠 PR 附注消化。

**Files:**
- Modify: `README.md`、`README.zh-CN.md`
- Create: `docs/2026-08-10-vault-search-index-measurements.md`

- [ ] **Step 1: 测基线体积**

```bash
git stash list >/dev/null   # 确认工作区干净
git rev-parse HEAD
# 在一个临时 worktree 上 checkout 本功能分支的 parent,构建 release
cargo build --manifest-path src-tauri/Cargo.toml --release 2>&1 | tail -3
stat -f%z src-tauri/target/release/notemd
```

记下基线字节数。

- [ ] **Step 2: 测带索引的体积**

```bash
cargo build --manifest-path src-tauri/Cargo.toml --release 2>&1 | tail -3
stat -f%z src-tauri/target/release/notemd
```

Expected: 增量 **< 5.0MB**。超了就在 PR 里如实写出并停下来问,**不要**偷偷调门槛。

- [ ] **Step 3: 记录实测**

`docs/2026-08-10-vault-search-index-measurements.md` 写入:基线/新体积/增量、`--stats` 给出的索引大小与语料大小之比、冷建耗时、无变更 sweep 耗时、查询 p50/p95、Retrievability 回归集条数与通过率、二进制增量的组件分解。这份文档是 spec §7 那张表的"实测栏"。

- [ ] **Step 4: 改 README 体积表述**

`grep -n "7MB\|11MB\|7 MB\|11 MB" README.md README.zh-CN.md`,把下载/安装体积改成实测值(预计 ~11MB 下载 / ~16MB 安装)。中英两版同改。**不要**只改一版。

- [ ] **Step 5: 全量验收**

```bash
cargo test --manifest-path searchidx/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --release --test cli_startup_timing
pnpm check
pnpm test
```

Expected: 全绿。任何一条不绿都不算完成——按 verification-before-completion,先贴输出再说结论。

- [ ] **Step 6: 提交**

```bash
git add README.md README.zh-CN.md docs/2026-08-10-vault-search-index-measurements.md
git commit -m "docs: measured binary/index footprint; update README size figures"
```

---

## Windows 手动验收清单

无 CI,这几条由人在 Windows 发版机上跑一次。全部通过才算 P0 双平台完成。

- [ ] `cargo test --manifest-path searchidx\Cargo.toml` 全绿 —— 含 `rel_path_normalizes_backslashes_on_windows`、`on_windows_the_index_lives_in_local_appdata_not_roaming`
- [ ] `cargo test --manifest-path src-tauri\Cargo.toml` 全绿 —— 含 `the_index_is_local_while_the_config_is_not`
- [ ] 跨平台一致性:把 macOS 上 `searchidx/tests/fixtures/corpus` 的一次 `--json` 查询输出存下来,在 Windows 上对同一批 fixtures 跑同一查询,**命中集与顺序逐字段相同**(path 已统一 `/`)
- [ ] 安装 NSIS 包后,新开 **cmd**:`notemd search 检索` 有输出(不是弹出 GUI 窗口)
- [ ] 新开 **Git Bash**:同一命令同样有输出
- [ ] `notemd search x --stats --json` 给出的 `tokenizer_id` 与 macOS 上完全一致
- [ ] 索引库确实落在 `%LOCALAPPDATA%\net.notemd.app\search\`,`%APPDATA%`(Roaming)下没有 `search` 目录
- [ ] 卸载后 `$INSTDIR\bin\notemd.cmd` 被删除,PATH 里没留下悬空条目
- [ ] GUI 里改一个文件保存,~0.5s 后 CLI `notemd search` 能查到新内容(验证两进程同库)

---

## 已知取舍与残余风险

- **第三份大纲解析器。** TS、roam-import 后端、searchidx 各一份。共享 fixtures 是防漂移手段,不是消除重复。抽公共 crate 会牵动一个已上架插件的重发,不在本次范围。
- **日文/韩文全文检索质量有限。** jieba 只切汉字,假名与谚文按整段词元存。精确词与 LIKE 兜底可用,子词召回不可用。这是诚实的限制,不是伪装成分词的噪音。spec 未把 JA/KO 列入目标。
- **`links` 表写而不读。** 反链层本次不动(spec §2)。现在写是为了将来加 `page:` 反查时不必让所有用户重建索引。
- **`human_verified` 信号稀疏。** 真实 vault 只有 7 个文件带,加成保留但不宣传。
- **体积 +4.7MB 超出 spec 的 4MB 估算。** 已在「实测校正」外显,README 同步更新(Task 19)。若产品层最终不接受,可裁的是 jieba 词典(换小词典约省 1.3MB,代价是中文分词质量),**核心 crate 与 SQLite 不裁**。
- **没有预留 T2 的 `embedding` 列。** spec §4 建议现在就占位。不占的理由:加列必然 bump `schema_version`,而重建只要 ~10s、全自动、无数据损失 —— 为一个判据未触发的 P3 功能背一个空列不划算。将来加 T2 时随 schema bump 一起加即可。
- **无 CI 双平台矩阵。** 按用户既定规矩不建 GH Actions,改为固化 fixtures + Windows 手动验收清单。代价是回归发现得晚一拍。
