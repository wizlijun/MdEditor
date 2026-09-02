# Ebook Import 主题分类与索引设计

状态：Proposed，等待用户确认后再实施。

## 0. 一句话

保留电子书现有的 `YYYY-MM/<书名>/` 物理归档；在书库根增加一份可由用户维护的
`topics.yml`，每本书在自己的 `meta.yml` 中保存唯一 `topic_id`，插件据此确定性生成
`<关键词>.index.md`。导入界面把主题作为首要选择，新书没有有效主题时禁止开始导入；
Agent 通过独立任务基于现有书名和 metadata 生成 2–8 个主题及完整归类 proposal，用户
预览确认后再应用。

## 1. 目标与边界

### 目标

1. 在导入窗口醒目展示不超过 5 个书籍领域主题。
2. 用户可在插件窗口内新增、编辑、排序和迁移主题。
3. Agent 可依据现有书籍名称、作者、出版社、语言等 metadata 设计 2–8 个主题。
4. 每个主题包含稳定 ID、显示关键词、领域说明、相关词汇及逐词描述。
5. 每本新书必须且只能归入一个有效主题。
6. 每个主题生成一个 `<关键词>.index.md`，并包含该主题下全部已归类书籍。
7. YAML 是权威数据，Markdown 索引随时可重建，索引损坏不能反向污染分类。
8. 旧书不因升级而消失，并可通过 Agent 或人工批量补齐分类。

### 非目标

- 不把书籍目录改成 `<主题>/<月份>/<书名>`；现有路径、摘要链接和 AI 阅读入口保持不变。
- 首版不允许一本书同时属于多个主题；横切标签以后可独立增加，不能混入主分类。
- Agent 不读全书正文来设计分类，只使用最小 metadata inventory。
- Agent 不直接写 `topics.yml`、书籍 `meta.yml` 或最终 index。
- 不把主题设置暴露到 Host 全局设置；入口只在 Ebook Import 窗口内。

## 2. 已选架构

| 问题 | 设计 |
| --- | --- |
| 物理目录 | 继续使用 `<ebooks_root>/<YYYY-MM>/<Title>/` |
| 主题上限 | 最多 8 个，满足 `<=8`；Agent 生成 2–8 个，人工配置允许 1–8 个 |
| 书籍归属 | 每书恰好一个 `topic_id` |
| 主题定义真相源 | `<ebooks_root>/topics.yml` |
| 书籍归属真相源 | `<书目录>/meta.yml.topic_id` |
| Markdown 索引 | `<ebooks_root>/<index_file>`，完全由投影器生成 |
| Agent 输出 | `.notemd/ebook-import/topic-design/topics.proposal.yml`，仅为候选 |
| 应用权限 | 后端验证 proposal，用户在 UI 明确确认后由插件写入 |
| 旧书兼容 | 缺 `topic_id` 仍显示并标为“未分类”；但任何新导入都必须带有效主题 |

不把“书籍 → 主题”同时保存在 `topics.yml`，否则每次导入都要修改一个全局书单，Git
并发更容易冲突，也会与每书 metadata 形成双真相。`topics.yml` 只定义 taxonomy；书籍
归属随书保存。

## 3. 文件布局

```text
ssot/ebooks/
├── topics.yml                         # 用户/插件维护的主题词表
├── 商业战略.index.md                  # 生成物
├── 软件工程.index.md                  # 生成物
└── 2026-09/
    └── Seven Powers/
        ├── book.md
        ├── config.txt
        ├── meta.yml                   # added_at + topic_id
        └── images/

.notemd/
├── agent-tasks/organize-ebook-topics/ # ebook-import 首次创建的独立 Agent 任务
└── ebook-import/topic-design/
    ├── inventory.yml                  # 插件生成的最小只读快照，gitignored
    ├── topics.proposal.yml            # Agent 候选，gitignored
    └── apply-journal.json              # 批量应用恢复记录，成功后删除，gitignored
```

`ebooks_root` 仍取 `.notemd/ebook-import.json`，所以上述 `ssot/ebooks` 只是默认路径。

## 4. YAML 契约

### 4.1 `topics.yml`

```yaml
schema_version: 1
topics:
  - id: business-strategy
    label: 商业战略
    description: 研究企业如何建立、维持和更新竞争优势。
    index_file: 商业战略.index.md
    vocabulary:
      - term: 竞争优势
        description: 企业相对竞争者持续创造超额价值的能力。
      - term: 护城河
        description: 阻止竞争者复制价值获取方式的结构性屏障。
  - id: software-engineering
    label: 软件工程
    description: 关注软件系统的设计、交付、演化与工程组织。
    index_file: 软件工程.index.md
    vocabulary:
      - term: 架构
        description: 系统组成部分、边界及其关键关系的整体设计。
```

校验规则：

- `schema_version` 必须为 `1`。
- `topics` 数量为 1–8；Agent proposal 必须为 2–8。
- `id` 是稳定主键：`[a-z0-9]+(?:-[a-z0-9]+)*`，全局唯一，创建后改显示名也不改 ID。
- `label` 非空、唯一，建议 2–8 个字符，但不以语言长度硬拒绝。
- `description` 非空，说明纳入范围，避免只换一个近义词。
- `index_file` 是书库根下的单个安全文件名：不得含 `/`、`\\`、`..`，不得为
  `index.md` / `log.md`，必须以 `.index.md` 结尾，且全局唯一。
- `vocabulary` 至少 2 项；每项 `term`、`description` 非空；同一主题内 `term` 唯一。
- 未知字段读取时保留，便于未来 schema 扩展。

### 4.2 每书 `meta.yml`

```yaml
added_at: 2026-09-01T08:12:03Z
topic_id: business-strategy
```

- `added_at` 契约保持不变：RFC 3339 UTC、秒精度、`Z` 后缀。
- 新导入的 `topic_id` 必须引用当前 `topics.yml` 中的有效 ID。
- 旧书可暂时缺失 `topic_id`，扫描时标为未分类，不隐藏、不伪造默认分类。
- 主题改名只改 `label`，不批量改书；删除主题前必须把引用它的书迁移到另一个主题。
- 写入 `topic_id` 时保留 `meta.yml` 的其他未知字段。

### 4.3 Agent proposal

```yaml
schema_version: 1
inventory_sha256: <当前 inventory.yml 的 sha256>
topics:
  # 与 topics.yml 同 schema，必须 2–8 个
assignments:
  - book: 2026-08/Seven Powers
    topic_id: business-strategy
```

proposal 额外要求：

- `inventory_sha256` 必须与应用时重新生成的 inventory 一致；书库变化后旧 proposal 拒绝应用。
- `assignments` 必须对 inventory 中每本书恰好出现一次。
- 不得包含 inventory 之外的路径，不得漏书、重复书或引用未知主题。
- 后端只解析数据，不执行 proposal 内的任何指令或路径。

## 5. `<关键词>.index.md` 投影

索引由 `topics.yml + 所有 meta.yml + book.md frontmatter` 确定性生成。例如：

```markdown
---
type: Book Topic Index
title: "商业战略"
description: "研究企业如何建立、维持和更新竞争优势。"
tags: [ebooks, topic, business-strategy]
---
<!-- notemd:generated ebook-topic-index/v1; edit topics.yml or book meta.yml -->

# 商业战略

研究企业如何建立、维持和更新竞争优势。

## 相关词汇

- **竞争优势** — 企业相对竞争者持续创造超额价值的能力。
- **护城河** — 阻止竞争者复制价值获取方式的结构性屏障。

## 书籍

- [Seven Powers](<2026-08/Seven Powers/book.md>) — Hamilton Helmer · 2026-08-27
```

投影规则：

- 新增 `Book Topic Index` 到项目 OKF 类型表，并在 search origin 映射为 `derived`。
- 主题顺序与 `topics.yml` 一致；词汇顺序与 YAML 一致。
- 书籍按 `added_at` 新到旧排序；同时间按标题、相对路径稳定排序。
- 链接相对于 ebooks 根，含空格的路径用 `<...>` 包裹。
- 不写生成时钟，避免无内容变化时产生 Git diff。
- 文件开头必须有生成标记。插件只覆盖或清理带同版本生成标记的文件；遇到同名手写文件即停止并报冲突。
- 索引是投影。用户手改索引会在下次重建时丢失，因此 UI 的“编辑”入口打开
  `topics.yml` 或主题管理页，不把 index 当编辑入口。

## 6. 导入界面

### 6.1 主界面

在拖放区与导入队列之间增加醒目的“导入主题”区：

- 以 1–8 张主题卡展示 `label + 一行 description + 书籍数量`。
- 当前主题使用 accent 边框/浅色背景；这是持久界面选择，不使用 popup menu 样式。
- 点击卡片成为“当前导入主题”；随后拖入/选择的文件继承该 `topic_id`。
- 每个排队行显示主题 chip，并可从行内下拉改成其他有效主题。
- “开始导入”仅在所有 pending 行都有有效 `topic_id` 时启用。
- 运行开始时像 OCR 选项一样冻结每行主题；运行中修改当前主题只影响后续加入的文件。

操作区提供：

- “管理主题”：插件窗口内 sheet，新增/编辑/排序/迁移；不进入 Host 全局设置。
- “AI 根据书库设计”：复用 Agent Picker，启动独立任务。
- “重建索引”：只重建投影，不改变分类。
- “编辑 YAML”：在主编辑器打开 `<ebooks_root>/topics.yml`。

### 6.2 初次使用与错误态

- 没有 `topics.yml` 时显示 onboarding：人工创建或让 Agent 根据现有书库设计。
- 没有主题时禁止新导入，并明确说明“每本新书必须选择主题”。
- `topics.yml` 损坏时书库仍显示；主题选择和新导入被阻断，旧 index 保留，并提供打开 YAML 修复。
- 某本旧书缺失/引用不存在的 `topic_id` 时显示“未分类”，可在书库行直接选择主题。
- 书库顶部增加主题过滤；搜索同时匹配书名、主题名和 vocabulary term。

## 7. 导入与 CLI 协议

### UI RPC

```text
plugin.topic_state       -> topics + counts + unclassified + diagnostics
plugin.topic_save        -> 校验并保存主题定义/迁移，随后重建 index
plugin.topic_assign      -> 为一本文档写 topic_id，随后重建 index
plugin.topic_rebuild     -> 从 YAML/meta 重建全部 index
plugin.topic_agent_start -> 生成 inventory 并启动 Agent proposal
plugin.topic_agent_apply -> 校验/预览确认后的 proposal 并幂等应用
plugin.import_start      <- 新增必填 topic_id
plugin.library_list      -> 每本书新增 topic_id/topic_label
```

后端必须重新校验 `topic_id`，不能只信 UI。`run_import` / `PipelineCtx` 接收主题 ID，
`finalize` 写入 `added_at + topic_id`。

### CLI

现有命令增加：

```text
notemd ebook <file> --topic <topic-id> [--ocr ...]
```

- `--topic` 对所有新导入必填。
- 主题文件不存在、损坏或 ID 不存在时在转换开始前失败，不产生工作目录外的最终书目录。
- CLI 成功返回目标目录与对应 index 路径。

## 8. Agent 任务

独立任务 ID：`organize-ebook-topics`。不复用 `ai-read-ebook`，因为二者输入、权限和
产物完全不同。

### 输入 inventory

插件扫描现有书库并生成最小数据：

```yaml
schema_version: 1
books:
  - rel: 2026-08/Seven Powers
    title: Seven Powers
    creator: Hamilton Helmer
    publisher: Stripe Press
    language: en
    added_at: 2026-08-27T06:40:15Z
    current_topic_id: null
```

不包含 `book.md` 正文、摘要或用户其他 Vault 内容。metadata 优先读 `book.md`
frontmatter，缺失时退回 `config.txt` 和目录名。

### Prompt 约束

Agent 必须：

1. 生成 2–8 个稳定、互斥且有长期意义的书籍领域主题。
2. 不按作者、语言、文件格式、月份或“一般/其他”随意切分。
3. 每个主题给出简短关键词、清晰的领域边界、至少 2 个相关词汇及逐词描述。
4. 将 inventory 的每本书恰好归入一个主题；信息不足时按书名和 publisher 做最保守判断。
5. 只写固定 proposal 路径，不修改书籍、canonical YAML 或 index。
6. 严格输出可解析 YAML，不使用 Markdown code fence。

任务目录同时携带共享 `task.json`，以及 Claude / Codex / DeepSeek 各自需要的指令和权限
文件。Ebook Import 仅在文件缺失时创建默认模板，不覆盖用户已经编辑的版本。

### 应用流程

1. UI 选 Agent 并点击“AI 根据书库设计”。
2. 后端生成 inventory 和 SHA，启动 `host.agent.run`，轮询 `host.agent.status`。
3. Agent 成功后，后端解析并执行完整 schema/覆盖率/路径校验。
4. UI 展示 2–8 张候选主题卡、每类书籍数量、旧 → 新归属差异和警告。
5. 用户点击“应用”后，插件写 canonical YAML、更新旧书 meta、重建 indexes。
6. proposal 过期、非法或与当前书库不一致时不允许应用，可重新运行。

## 9. 一致性、并发与恢复

YAML/meta 是权威，index 是缓存投影。多个文件无法形成真正的跨文件原子 rename，因此
采用“锁 + 原子单文件写 + 可重放 reconcile”：

- 所有主题保存、书籍归属、导入 meta 提交和 index 重建共用 ebooks 根的文件锁。
- 单个 YAML/Markdown 先写同目录临时文件，`fsync` 后 rename。
- 新书先写 `book.md/config/images`，再以临时文件提交含 `topic_id` 的 `meta.yml`；只有
  `meta.yml` 存在才算已完成书籍。
- meta 提交后，在同一锁内扫描所有书并重建受影响 index；成功事件在重建结束后发送。
- 若进程在 meta 提交后崩溃，启动、`library_list` 和下一次导入都会 reconcile，补回 index。
- Agent 批量应用写 `apply-journal.json`，记录 proposal SHA 和目标步骤；重启后幂等完成或
  回报可恢复错误，不能留下不可解释的半应用状态。
- 并发投影总是重新扫描所有已提交 meta，不使用“读取 index 后追加一行”，避免丢更新。
- Git 合并后若 index 冲突，可直接重建；若 `topics.yml` 或同一本书 `meta.yml` 冲突则
  fail closed，要求用户解决权威 YAML，不能用 index 猜测。

## 10. 旧书迁移

升级后：

1. 原书库扫描逻辑继续容忍只有 `added_at` 的 `meta.yml`。
2. 旧书在 UI 中显示“未分类”，不会被隐藏，也不会移动目录。
3. 用户可逐本/批量人工归类，或运行 Agent proposal 一次覆盖全部旧书。
4. “新书必须分类”立即生效；旧书补齐不作为打开书库的门槛。
5. 删除主题时，管理 sheet 必须先选择替代主题并预览受影响书数；没有替代目标就拒绝。
6. 主题 label 改名不改 ID；`index_file` 改名时只自动移动带生成标记的旧 index。

## 11. 验收与测试

### 后端

- `topics.yml` schema：1–8、唯一 ID/label/file、安全路径、vocabulary 完整性。
- `meta.yml` 新字段写入、旧格式兼容、未知字段保留、非法/缺失主题诊断。
- index golden：frontmatter、词汇、排序、相对链接、无时钟漂移、同输入字节一致。
- 导入必须携带有效主题；无主题在 Calibre/OCR 前失败。
- meta 提交后 index 重建；注入 index 写失败/崩溃后 reconcile 能恢复。
- GUI/CLI 并发导入最终 index 包含并集，不丢书、不重复。
- proposal：inventory SHA、全书覆盖、路径防逃逸、未知/重复 topic/book 拒绝。
- 主题改名、迁移、删除、同名手写 index 冲突。

### 前端

- 主题卡显著展示、当前选中、计数与说明。
- 新文件继承当前主题，逐行可覆盖；任何 pending 未分类时 Start disabled。
- Agent 运行、失败、proposal 预览、过期和应用状态。
- 旧书未分类仍显示；主题过滤与搜索不影响 AI 阅读状态合并。
- 四语 i18n、键盘操作、浅色/深色可读性。

### 跨项目

- 注册 `Book Topic Index`，同步 search origin fixture，OKF lint 通过。
- Ebook Import Rust/TS 全量测试、类型检查、生产构建通过。
- 宿主插件协议与主应用回归通过。
- 实机验证：拖入多本书、切主题、hover/focus、导入、打开 index、重启后重建。

## 12. 预计改动范围

主要新增：

- `plugins-src/ebook-import/backend/src/topics.rs`
- `plugins-src/ebook-import/backend/src/topic_agent.rs`
- `plugins-src/ebook-import/backend/templates/organize-ebook-topics/*`
- `plugins-src/ebook-import/src/components/TopicBar.svelte`
- `plugins-src/ebook-import/src/components/TopicManager.svelte`
- `plugins-src/ebook-import/src/lib/topics.ts`

主要修改：

- `pipeline.rs` / `library.rs` / `plugin.rs` / `bookconf.rs`
- `App.svelte` / `queue.ts` / `library.ts` / `strings.ts`
- `manifest.v2.json` 的 CLI 参数、版本和用户可见描述
- `src/lib/okf/concept.ts`、search origin 映射与生成 fixture

不改：

- 书籍物理目录层级。
- 已有 `book.md` 路径、摘要命名和 AI 阅读任务语义。
- Host 全局设置体系。

## 13. 实施前需确认的产品决策

本设计采用以下推荐默认值，用户确认后按此实施：

1. “`<=8`”解释为书库全局最多 8 个一级主题，而不是“每本书最多 8 个标签”。
2. 一本书首版只归一个主题；不做多主题标签。
3. Agent 先生成 proposal，用户确认后应用；不允许 Agent 直接改 canonical 数据。
4. `<关键词>.index.md` 放在 ebooks 根目录，书籍继续按月份归档。
