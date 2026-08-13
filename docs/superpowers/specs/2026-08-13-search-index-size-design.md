# 搜索索引体积优化设计

日期：2026-08-13
状态：已实施（WAL 截断 + contentless FTS），第三项待议

## 1. 起因与实测基线

用户报告索引文件很大，并问「去掉 jieba 分词能不能小很多」。答案是不能——但测量过程找到了两处更大的浪费。

真实 vault：`/Users/bruce/git/sotvault`，8,977 个文件，1.6 GB 原始内容。

优化前（`dbstat` 实测）：

| 表 | 大小 | 占比 |
|---|---|---|
| `blocks` | 689 MB | 43% |
| `blocks_fts_content` | 632 MB | 39% |
| `blocks_fts_data` | 259 MB | 16% |
| 其余 | 37 MB | 2% |
| **`index.db` 合计** | **1,617 MB** | |
| `index.db-wal` | 1,706 MB | |
| **磁盘合计** | **3,323 MB** | |

参照系：file 级块正文合计 152 MB ≈ vault 可索引文本总量。索引是原文的 10.6 倍，算上 WAL 是 22 倍。

## 2. 为什么去掉 jieba 不解决问题（已否决）

带 jieba 的分词结果本来就存在库里（`blocks_fts_content.c0`），不带 jieba 的那份是纯规则、可直接算出，因此**无需重建索引即可精确对比**。抽样 1/17 的块（76,946 块 / 28 MB 原文），逐块计算两种分词的字节数：

| 粒度 | 原文 | 带 jieba | 不带 jieba | 比值 |
|---|---|---|---|---|
| file | 6.8 MB | 6.1 MB | 5.7 MB | 0.941 |
| line | 7.3 MB | 7.1 MB | 6.7 MB | 0.938 |
| section | 13.8 MB | 12.7 MB | 12.0 MB | 0.943 |
| 合计 | 28.0 MB | 26.0 MB | 24.4 MB | **0.941** |

只小 5.9%。原因：**这个 vault 汉字只占 10.2%**（239 万汉字 vs 2,114 万非汉字），jieba 只作用在汉字连续段上，其余 90% 的英文/代码/Markdown 走同一条 ASCII 路径、一个字节都不变。

更糟的是倒排索引会变大：去掉 jieba 后标点之间的整段汉字成为单个词元，抽样内不同词元数从 136,932 涨到 331,433（**2.42 倍**），`blocks_fts_data` 的词典部分要存这 2.4 倍的词元字符串。

净效果约 −2.3%，代价是中文子串检索直接失效（搜「增量」再也命中不了「增量索引」，即 `tokenize.rs` 文首写明的 jieba 存在的唯一理由）。**否决。**

## 3. 已实施一：重建后截断 WAL

WAL 会涨到「历史上最大那笔事务」的高水位，之后只被**复用**、不会自己缩小。一次全量重建因此留下一个与索引本体同量级的 WAL（实测 1.7 GB）。`wal_autocheckpoint` 跑的是 `PASSIVE`，只让 WAL 可复用，不还空间；只有 `TRUNCATE` 模式会还。

`store::checkpoint_truncate` 在 `scan::build_full` 提交后调用一次。

**只在 `build_full`，不在 `sweep`**：全量重建是唯一会产生巨型 WAL 的事务，而且它本身已经耗时几十秒，一次检查点是噪声；增量 sweep 停留在 SQLite 默认的 1000 页（≈4 MB）以内，每批 watcher 事件做一次 TRUNCATE 是纯开销、换不回空间。

**best-effort，且这不是偷懒**：`TRUNCATE` 需要其他连接全部让开，而「两个互不协调的写进程（GUI 与 `notemd search`）」是本 crate 明写的设计前提。这里的 `SQLITE_BUSY` 只说明「别人正在查询」，与刚刚成功的那次扫描无关，因此绝不能把那次扫描变成错误；下次重建再截断即可。

## 4. 已实施二：`blocks_fts` 改 contentless

FTS5 默认会把被索引的列**再存一份**在 `%_content` 影子表里。实测这份副本 632 MB，占索引的 39%，而且从来没人读：

- 查询一律 JOIN 回 `blocks` 取真正的文本（`SELECT_COLS` 取的是 `b.text`）；
- 全代码库无 `snippet()` / `highlight()` 调用；
- `bm25()` 在 contentless 表上照常工作。

改为 `content='' , contentless_delete=1`。`SCHEMA_VERSION` 4 → 5。

### 实测过的边界行为

用 bundled SQLite（3.53.2）逐条验过普通 fts5 与 contentless 的差异：

| 操作 | 普通 fts5 | contentless_delete=1 |
|---|---|---|
| `MATCH` + `bm25()` | ok | ok |
| `DELETE WHERE rowid IN (…)`（`remove_file` 用） | ok | ok |
| 全表 `DELETE`（`build_full` / `clear_in_place` 用） | ok | ok |
| 删单行后 MATCH 不再命中它 | ok | ok |
| 清空后重插再查 | ok | ok |
| **读列值** | 返回真实值 | **静默返回 NULL** |

最后一行是唯一的行为差异，也是唯一的陷阱：读 FTS 列值不会报错，只会得到空。今天没有任何代码这么做，`reading_an_fts_column_value_yields_null_rather_than_an_error` 把这个陷阱记录在案。

`blocks_fts_docsize` 从 15.1 MB 涨到 18.9 MB，是 `contentless_delete=1` 的墓碑位开销，相对 632 MB 的收益可以忽略。

## 5. 实测结果

同一 vault，用新代码全量重建（release，8,977 文件，30.9 秒）：

| | 优化前 | 优化后 | 变化 |
|---|---|---|---|
| `index.db` | 1,617 MB | **990.5 MB** | −38.7% |
| `index.db-wal` | 1,706 MB | **0.0 MB** | −100% |
| **磁盘合计** | **3,323 MB** | **990.5 MB** | **−70.2%** |

分表：`blocks` 689 MB（未动）、`blocks_fts_data` 259 MB（未动，倒排索引完好）、`blocks_fts_docsize` 18.9 MB、其余 ~23 MB。`blocks_fts_content` 已完全消失。

检索质量零变化：召回/排序回归集（`retrievability.json`）与全部验收测试通过。

## 6. 未做：rollup 正文重复（待议）

`blocks` 剩下的 689 MB 里，section 块（254 MB）与 file 块（152 MB）的 `text` 是 line 级正文的重复副本——533 MB 逻辑文本中有 405 MB 是重复。

可以只存行号区间、读取时按需从源文件切片，但这会让索引不再自洽（命中预览要回读文件，而文件可能已被修改或删除），改动面和风险都远大于上面两项。单独出 spec 再议。
