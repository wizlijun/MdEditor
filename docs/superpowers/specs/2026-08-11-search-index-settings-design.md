# 索引与搜索设置页 —— 设计(项目 A)

> 类型:设计规格 · 日期:2026-08-11
> 前置:`docs/2026-08-10-vault-search-index-design.md`(检索功能 spec v3.2,已实现并合入 main)
> 姊妹项目:`2026-08-11-md-origin-tiering-design.md`(项目 B,md 分级与检索优先级)

## 0 · 一句话

把索引从「一个看不见的后台派生物」变成「一个你能看见全部状态、能安全操作、出问题能自己查日志的东西」。

## 1 · 为什么

现在索引对用户是完全不透明的:

- **GUI 里没有任何地方能看到索引状态。** 后端命令 `notemd_search_stats` 写了,前端 `searchApi.stats()` 包装也写了,**没有任何组件调用它**。唯一的出口是 `notemd search --stats` CLI。
- **构建进度完全没有。** 后端只在 watcher 重索引完成后发一个 `search://index-updated`(一个「完事了」信号,没有「进行到哪了」)。全量重建期间面板只是禁用输入框配一句静态文案 —— 在 8,826 文件的 vault 上,和卡死无法区分。
- **重建按钮放在搜索面板里,点了没有二次确认。** 一次误触就是一次全量重建,期间所有查询阻塞。
- **索引日志没有分类。** `log_bus` 的 `log_cat!` 宏存在但全项目没有一处使用;索引侧目前走 `dlog`,和其他所有东西混在一起。

## 2 · 范围

**做:**
- 偏好设置新增「搜索与索引」tab,承载全部索引状态与设置
- 重建改为后台任务 + 进度事件 + 独立于索引锁的可读状态
- 重建二次确认
- 索引日志统一走 `search` 分类,设置页可一键跳转到已过滤的日志查看器
- 搜索面板的重建按钮 → 齿轮按钮,跳转到设置页对应 tab
- 索引大文件阈值从 git 门禁解耦,成为独立设置
- `searchExcludeDirs` 从 Vault tab 迁到新 tab

**不做(属于项目 B):**
- `origin` 分级推导、排序权重、分组展示、`origin:` 过滤器

**不做(其他):**
- CLI 侧的进度显示(`notemd search --rebuild` 保持现状:同步、完成后打印汇总)
- 索引库的手动删除/导出入口

## 3 · 阈值解耦

现状:`vault_settings.large_file_threshold_mb` 一个值两个消费者 —— git 同步的大文件门禁,和 `searchidx::ScanOptions.large_file_threshold_mb` 的索引跳过阈值。标签写的是 git 门禁。

改为两个独立设置:

| 设置 | 键 | 默认 | 归属 tab |
| --- | --- | --- | --- |
| git 大文件门禁 | `largeFileThresholdMb` | 10 | Vault(不动) |
| 索引跳过阈值 | `searchLargeFileThresholdMb` | 10 | 搜索与索引(新) |

两者默认值相同、语义不同。**迁移:`searchLargeFileThresholdMb` 缺失时回落到 `largeFileThresholdMb`**,所以已经调过 git 门禁的用户不会突然发现索引行为变了;一旦用户在新 tab 里显式设过,就与 git 那个彻底脱钩。

回落逻辑放在 `searchidx` 的调用方(`src-tauri` 的 `search::scan_options` 与 `cli::search::scan_options`)—— **两处必须一致**,否则 GUI 与 CLI 会用不同阈值索引同一个 vault,破坏「一个算法三个 adapter」的前提。抽一个共享函数,并加契约测试钉住两者相等。

> 项目 B 会让原始资料类文件(ebook、字幕转写)常态超过 1 MB —— 阈值解耦是它的前提:索引可以放宽到 50 MB 收原始资料,而 git 门禁仍然守在 10 MB。

## 4 · 进度:后台任务 + 事件 + 可读状态

### 4.1 为什么不能只发事件

`notemd_search_rebuild` 现在全程持有索引锁,所以重建期间 `notemd_search_stats` 也读不到 —— 设置页无法靠轮询报告进度。而如果**只**发事件,中途才打开设置页的用户会看到一片空白,直到下一个事件到来。

因此两者都要:事件驱动实时更新,独立状态兜住「中途打开」。

### 4.2 核心 crate 侧

`searchidx` 不依赖 `tauri`,进度必须以回调形式出去:

```rust
pub enum Phase { Walking, Indexing, Removing, Done }

pub struct Progress {
    pub phase: Phase,
    pub done: usize,
    pub total: usize,          // Walking 阶段为 0(还不知道总数)
    pub current: Option<String>, // vault 相对路径
    pub elapsed_ms: u128,
}

pub type ProgressFn<'a> = &'a (dyn Fn(&Progress) + Send + Sync);
```

`build_full` / `sweep` 增加一个 `progress: Option<ProgressFn>` 参数。回调在扫描循环里调用,**节流**:每处理 25 个文件或每 200 ms 调一次(取先到者),外加每个阶段切换时必调一次。节流的理由是 8,826 次跨 IPC 的 emit 会把主线程淹掉。

现有调用点全部传 `None`,行为不变。

### 4.3 宿主侧

- `notemd_search_rebuild` 立即返回;重建在后台线程跑
- 一个 `Arc<Mutex<Option<Progress>>>`,**独立于索引句柄的锁**,由回调写入
- 回调同时 `app.emit("search://progress", &progress)`
- 新命令 `notemd_search_progress() -> Option<ProgressDto>`,读上面那份状态,**不碰索引锁**,所以重建期间可用
- 完成时 emit `search://index-updated`(沿用现有事件),并把进度状态清空

**并发保护:** 重建期间再次点重建必须被拒绝而不是排队。用一个 `AtomicBool` 守门,`notemd_search_rebuild` 在已有重建进行中时返回 `Err("rebuild already running")`。

## 5 · 日志

索引侧全部改走 `log_cat!("search", level, ...)`。粒度(经用户裁决,理由是 `log_bus` 是全局共享的 3000 行环形缓冲,逐文件写会把别人的日志冲掉、也会把自己早期的行冲掉):

**逐条记:**
- 开始:vault 路径、模式(全量/增量)、有效阈值、排除目录
- 扫描完成:发现 N 个可索引文件,跳过 M 个(超阈值)
- 每 500 个文件一行进度
- 完成汇总:耗时、索引数、删除数、库大小
- 异常:每个超阈值跳过的文件(路径 + 实际大小)、每个解析失败、每次降级(LIKE 兜底、sweep 超时、索引不可用)

**不记:**逐文件的成功行。那个粒度放在设置页的实时进度里(`current` 字段),不受缓冲区限制。

设置页的「查看日志」按钮调用已有的 `open_logs_window(app, Some("search"))` —— 这个带 filter 参数的函数已经存在,不用新写。

## 6 · 设置页

新 tab id `search`,标签 `settings.tab.search`(四语言)。插在 `vault` 之后、`outline-notes` 之前。

### 6.1 内容(自上而下)

**索引状态**
- 文件数 / 块数 / 库大小 / 建立时间 / tokenizer 版本
- vault 未配置时:整个 tab 显示一句「未配置 vault」,不显示任何按钮

**分层统计** —— 项目 B 落地前显示占位(「分层信息将在启用后显示」);B 落地后填入每个 `origin` 的文件数与 `derived` 的类型分布。**A 阶段就把这一块的容器和 i18n 键做出来**,避免 B 再动一次布局。

**实时进度** —— 仅在重建/sweep 进行中显示:阶段、`done/total` 进度条与百分比、当前文件路径、已耗时。空闲时整块隐藏。

**操作**
- 「重建索引」按钮 → 二次确认对话框。文案要说清后果:全量重建、期间搜索不可用、耗时约 N 秒(按当前文件数估)、不会丢失任何笔记(索引是可弃派生物)
- 「查看日志」按钮 → 打开日志窗口并过滤 `search`

**设置**
- `searchExcludeDirs`(从 Vault tab 迁来,每行一个目录)
- `searchLargeFileThresholdMb`(新)

**被跳过的文件** —— 超阈值清单,逐条显示路径与实际大小,附一句「这些文件不进索引,但 `rg` 仍可查」

### 6.2 面板改动

搜索面板底部的「重建索引」按钮删除,换成齿轮图标按钮,点击:打开偏好设置并切到 `search` tab。面板不再有任何破坏性操作。

面板保留:结果计数、耗时、`t1-scan` 降级提示。

## 7 · 错误与降级

| 情况 | 行为 |
| --- | --- |
| vault 未配置 | tab 显示说明文字,无按钮 |
| 索引未就绪(启动中) | 状态区显示「索引构建中」,进度区正常显示 |
| 重建进行中再次点重建 | 按钮禁用;若经由其他窗口/CLI 触发,命令返回 `rebuild already running`,前端提示 |
| 进度命令失败 | 进度区隐藏,不影响其余部分 |
| 日志窗口打不开 | toast 提示,不阻塞 |

## 8 · 测试

**Rust**
- 进度回调:节流生效(N 个文件只触发预期次数)、阶段切换必触发、`total` 在 Walking 后被填上
- 后台重建:命令立即返回;进行中第二次调用被拒绝;完成后进度状态清空
- `notemd_search_progress` 在重建进行中可读(**这条是这次改动的核心不变量** —— 它证明进度状态确实独立于索引锁)
- 阈值回落:`searchLargeFileThresholdMb` 缺失时用 `largeFileThresholdMb`;显式设置后不再回落
- 日志分类:一次全量重建产出的 `search` 分类行数在预期范围内(证明节流有效,不会冲掉缓冲区)

**前端**
- 设置页 store:进度事件累积、完成后清空、中途打开时从 `notemd_search_progress` 拉到当前状态
- 二次确认:取消不触发重建
- 面板齿轮按钮打开设置页并选中 `search` tab

**人工(GUI)**
- 在真实 vault 上点重建,确认进度条动、当前文件在变、完成后统计更新
- 重建期间查询被正确禁用且有解释
- 日志按钮跳转后确实只显示 `search` 分类
- 深浅色主题

## 9 · 残余风险

- **进度的 `total` 在 Walking 阶段未知**,所以进度条前几百毫秒只能显示不确定态。可接受。
- **节流参数(25 文件 / 200 ms)是拍的**,真实 vault 上可能偏密或偏疏,发布前按实测调一次。
- **`searchLargeFileThresholdMb` 的回落逻辑是一次性的善意**:用户一旦在新 tab 显式保存过,就再也不跟随 git 门禁。这个单向门必须在 UI 上说明,否则会有人以为改 git 门禁还能影响索引。
- 后台重建线程与 vault 切换的交互:切 vault 时若有重建在跑,应让它自然结束并丢弃结果(generation 机制已存在于 `search::open_vault`,复用即可)。
