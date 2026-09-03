# 会议记录插件与 Hemory 迁移 —— 已批准规格

> 状态：已批准，进入实现
>
> 日期：2026-09-03
>
> 用户于 2026-09-03 确认 §11 的产品决策；本文是 V1 实现契约。

## 0 · 结论

新建 note.md 原生插件 `notemd.meetings`，把会议统一归档到：

```
ssot/meetings/<conversation-id>/
├── transcript.srt        # 或 transcript.md；同一会议只保留一个权威逐字稿
├── summary.md            # 来源存在时迁移；不是权威逐字稿
└── meta.yml              # 通用契约可选；Hemory 导入时必写
```

Hemory V1 优先把用户确认已准备好的 `content.md` 原始字节复制为 `transcript.md`；只有 `content.md` 不存在时，才把合格的 `pro_asr.srt` 原始字节复制为 `transcript.srt`。来源存在 `summary.md` 时也原样迁入，但它不取代逐字稿。不复制任何源目录，也绝不写入 MP3、WAV、M4A、AAC 等音频。

全量和增量使用同一个迁移 planner。迁移默认幂等、按会议原子提交、逐条 checkpoint；源内容变化而目标未被用户修改时才更新，目标有本地修改或来源身份不明时只报告冲突，不覆盖，不同步删除。

## 1 · V1 范围

### 做

- 会议归档的固定目录、文件名和 metadata schema。
- 插件主窗口中的会议列表与“从 Hemory 迁移”入口。
- Hemory 现行 Vault、历史 Vault 和老 iOS session 布局的发现与解析。
- 全量、增量、预检、dry-run、冲突保护、中断恢复和机器可读报告。
- 与 UI 共用同一 Rust 迁移核心的 CLI。
- 为以后新增其他 ASR provider 保留窄的 source adapter 接口。

### 不做

- 不迁移、引用或缓存音频；`audio.path`、`audio_files` 也不写进目标 metadata。
- 不迁移 Hemory 的向量、voiceprint clips、graph、缓存、任务状态或整目录副本。
- 不双向同步，不把目标改动写回 Hemory。
- 不传播 Hemory 删除，不自动删除 `ssot/meetings` 中的内容。
- 不提供 `--force`、静默覆盖、自动合并冲突或跨用户自动合并。
- V1 不实现其他会议软件同步；后续 provider 只接入统一的规范化模型和 planner。

## 2 · Hemory 来源事实

### 2.1 需要识别的目录布局

以生产源码为准，同时兼容仓库中有明确历史证据的旧布局：

```
# 现行 Vault / 当前 iOS
<root>/<user>/conversation/YYYYMM/YYYYMMDD_HHmmss[_N]/

# 更旧 Vault
<root>/<user>/conversations/YYYYMM/conv_YYYYMMDD_HHmmss[_NN]/

# 老 iOS local-first session
<root>/conversation/YYYY/MM/DD/session_yyyyMMdd_HHmmss_<session-id>/
<root>/conversation/YYYY/MM/DD/yyyy-MM-dd'T'HH-mm-ss/
```

现行源码使用单数 `conversation` 且没有 `conv_` 前缀；旧 README、测试和 iOS 迁移器证明另外两种布局真实存在，所以发现器必须按内容识别 schema，不能只看父目录名字。

扫描规则：

- UI 只接受用户显式选择的 Hemory 根、用户根或 `conversation(s)` 根；CLI 只接受显式 path 参数。
- 若所选根下检测到多个用户，必须由用户选择一个；CLI 中 `--user` 必填。V1 一次迁移一个 Hemory 用户。
- 不跟随 symlink；canonicalize 后越出选择根的路径一律拒绝。
- 排除 `_deleted`、`_deleted_*`、`.deleted_*` 等 tombstone。
- 月目录只是枚举线索：会议可能改过开始时间并移动过目录，不能靠 ID 反推唯一路径。
- 不用月 `index.json` 作为事实来源，它是可重建缓存。
- 原始 `audio/` 记录不是 conversation，不能因“有 SRT”就自动当成会议。

### 2.2 可读取的来源文件白名单

```
meta.json
content.md
pro_asr.srt
speakers.json
summary.md
```

老 metadata 的 `transcript_file` 只允许指向所选 conversation 目录内的安全相对路径，且目标必须是 `.srt`。其他文件不读取正文、不复制。

明确忽略并计数报告：

```
*.mp3  *.wav  *.m4a  *.aac
audio/**  voiceprint/**  pro_asr/*.m4a
manifest 中的音频路径及未知二进制文件
```

## 3 · 目标归档契约

### 3.1 conversation ID

- 保留合法的 Hemory ID：`YYYYMMDD_HHmmss[_N]`。
- 历史 `conv_` 前缀只在解析来源时接受，写入目标时去掉前缀。
- 老 session 若无合法 ID，从 `meta.created_at` 派生；只有 metadata 缺失时才使用目录名中的时间。
- ID 表示创建时采用的本地 wall-clock，不应解释为 UTC。准确时间另存 `started_at`，必须含 offset。
- 若历史时间没有 offset，预检标为 `needs_timezone`；UI 要求选择 IANA 时区，CLI 要求 `--timezone`，不使用运行机器当前时区静默猜测。
- `meta.created_at` 后续可能变更而 ID 保持稳定，因此更新时不重命名既有目标目录。
- 目标目录已被另一来源或未知内容占用时标为冲突；V1 不自动加一个看似 Hemory 原生的 `_N` 来掩盖冲突。

来源稳定键是 `hemory:<source-instance-id>:<user-id>:<normalized-conversation-id>`。`source-instance-id` 是插件首次接入该 Hemory 根时生成的 opaque ID；canonical source root 与 opaque ID 的绑定只保存在设备本地 plugin data，不写入 Vault。完整来源键、原始目录名和原始 ID 只进入迁移 ledger，不污染会议 metadata。

### 3.2 权威逐字稿

同一会议只能有以下一个文件：

- `transcript.srt`：标准 SRT，每个 cue 有 start/end，并能解析出 speaker；或
- `transcript.md`：后续原生记录/其他 provider 可用，每段必须含机器可解析的 start、可选 end 和 speaker。

Hemory V1 的权威正文选择规则为：

1. conversation 根存在 `content.md`：它必须通过 Markdown 会议正文校验，然后按原始字节复制为 `transcript.md`。
2. conversation 根不存在 `content.md`：检查 `pro_asr.srt`，通过 SRT 校验后按原始字节复制为 `transcript.srt`。
3. 两者都不存在或候选不合格：`blocked:no_valid_transcript`。V1 不自动尝试 `conv.srt`、`pre_asr.srt`、老 `transcript_file` 或 `transcript/transcript.srt`。

`content.md` 的合格条件：

- UTF-8（允许 BOM），至少有一个非空发言行；
- 允许 Hemory 已有的标题、分隔线、时间、Summary、人物等 header；
- header 后每个非空发言行符合 `HH:MM:SS  <speaker>: <text>`，时间和 speaker 均非空；
- 原始字节直接复制，不改写标题、正文、换行或文件末尾。

`pro_asr.srt` 的合格条件：

- SRT 序号和时间码可解析，`start <= end`，cue 顺序合法；
- 每个非空 cue 都有可识别的 speaker label；
- 支持 `[spk_01]`、`[00_spk_01]` 与可映射的历史 label；
- 若 label 需要 `speakers.json` 才能统一，映射必须完整且无歧义；
- 至少有一个非空 cue，且文件为 UTF-8（允许 BOM）；
- 通过校验后按原始字节复制，不润色、不重新断句、不改换行或文件末尾。

“不存在”和“不合格”是不同状态：只有 `content.md` 不存在才允许选择 pro SRT；`content.md` 存在但校验失败时必须阻断并报告，不能静默降级为另一份正文。被选择的来源哈希与目标 transcript 哈希必须相同。

### 3.3 `meta.yml`

通用会议目录允许没有 `meta.yml`；由 Hemory 导入的目录必须有。会议 metadata 只保存用户选择的业务字段；增量来源、文件哈希、原始路径和 checkpoint 全部进入 `.notemd` ledger。

建议 V1 schema：

```yaml
conversation_id: "20260403_173300"
title: "周会"
created_at: "2026-04-03T17:33:00+08:00"
end_at: "2026-04-03T18:05:12+08:00"
duration_ms: 1932000
language: zh-CN
category: meeting
source: hemory_v1.0:ios
key_topics:
  - 发布计划
speaker_count: 3
transcript_file: transcript.md
summary_file: summary.md
imported_from: hemory
updated_at: "2026-09-03T10:00:00+08:00"
```

映射规则：

| Hemory / 迁移结果 | `meta.yml` | 说明 |
| --- | --- | --- |
| `conv_id` / `session_id` | `conversation_id` | 规范后的目录 ID |
| `created_at` | `created_at` | 解析失败阻断；不退化成文件 mtime |
| `end_at` | `end_at` | 可选 |
| `audio.duration_ms` / 老 `duration` | `duration_ms` | 老 `duration` 按秒转毫秒；不保留音频路径 |
| `title` | `title` | 可选 |
| `language` | `language` | 可选 |
| `category` | `category` | 兼容历史 string/list 漂移，异常值放 warning |
| `key_topics` / `tags` | `key_topics` | 去重但保持原顺序 |
| `source` / 老 `device_source` | `source` | `hemory_v1.0:<原始文本>`；两者都缺失时为 `hemory_v1.0:unknown` |
| 顶层 `speaker_count` 或正文计算值 | `speaker_count` | 输出为整数 |
| `content.md` | 目标 `transcript.md` | 首选；通过正文校验后按原始字节复制 |
| `pro_asr.srt` | 目标 `transcript.srt` | 仅在 `content.md` 不存在时使用；通过 SRT 校验后按原始字节复制 |
| `summary.md` | 目标 `summary.md` | 存在且为 UTF-8 Markdown 时按原始字节复制，并记录哈希；缺失不阻断会议迁移 |
| 目标 transcript 文件名 | `transcript_file` | `transcript.md` 或 `transcript.srt` |
| 目标 summary 文件名 | `summary_file` | 仅在文件存在时写 `summary.md` |
| 固定值 | `imported_from` | `hemory` |
| 来源 `updated_at` / 成功迁移时刻 | `updated_at` | 来源有合法值时保留；否则写本次成功提交时刻，RFC 3339 含 offset |

最终只允许以下 14 个一级字段，不写任何嵌套对象：

```
conversation_id, created_at, end_at, title, category, key_topics,
language, source, duration_ms, speaker_count, transcript_file,
summary_file, imported_from, updated_at
```

可选值缺失时省略对应 key，不写嵌套 `audio`、`proAsr`、`speakers`、`source` provenance、`transcript` hash 或 `migration` 对象。`summary.md`、`audio.path`、`audio_files`、私有 `_idempotency_key`、删除/同步/流水线状态也不抄入 metadata。

speaker mapping 只用于校验正文和计算 `speaker_count`，不复制进扁平 metadata。正文中的现有说话人名字或 label 保持原样。

### 3.4 后续 ASR 接入边界

V1 不设计一个对外稳定的通用框架，只在后端保留最小内部边界：Hemory adapter 负责 `detect → discover → normalize`，输出统一的 `NormalizedMeeting`（source key、conversation ID、时间、metadata、speaker map、唯一 transcript 文件名/原始字节和 fingerprint）；planner/writer 不认识 Hemory 路径或字段。以后新增其他会议软件时只增加 adapter，复用目标 schema、冲突状态机、原子写入和报告，不为每个来源另写一套迁移器。

## 4 · 全量与增量

### 4.1 统一 planner

两种模式先得到稳定排序的规范化来源清单，再走同一状态判定：

| 条件 | 动作 |
| --- | --- |
| 目标不存在 | `create` |
| 目标存在但没有本插件 lineage | `conflict`，不写 |
| 来源 fingerprint 未变化且目标哈希匹配上次提交 | `skip` |
| 来源变化且目标仍等于上次提交哈希 | `update` |
| 来源变化且目标也被本地修改 | `conflict`，不覆盖 |
| 来源消失/删除 | `source_missing`，保留目标 |
| 无合法逐字稿、时间或身份冲突 | `blocked` |
| 音频或其他非白名单文件 | `excluded`，只计数 |

来源 fingerprint 由规范化 metadata、被选中的 `content.md` 或 `pro_asr.srt`、speaker mapping 和可选 `summary.md` 的内容哈希组成。`meta.version`、`updated_at`、mtime 和 size 只能帮助扫描，不能单独决定 unchanged，因为 Hemory 可直接改正文或 `speakers.json` 而不 bump metadata。

### 4.2 模式语义

- **增量**：扫描已知布局，计算相关内容 fingerprint；只对新增或 fingerprint 变化的会议重新解析和计划写入。
- **全量**：忽略解析缓存，对全部发现项重新做 schema、逐字稿和目标完整性验证；仍然幂等，仍不覆盖冲突，也不删除目标。
- 第一次增量因没有 checkpoint，会自然得到一次完整导入计划。
- 两种模式都不把“源目录更新日期晚于某个 watermark”当作完整性依据。

### 4.3 ledger

checkpoint 固定在 Vault 内：

```
.notemd/meetings/hemory-import-v1.json
```

每条记录包含 source key、来源相对路径、来源 fingerprint、目标相对路径、上次生成的 transcript/summary/meta 哈希和成功提交时间。ledger 不包含逐字稿正文、绝对源路径或音频信息。

- 每提交一条会议就原子更新一次 ledger，超时或关闭窗口后可从下一条继续。
- dry-run 零写入，不创建目标目录、ledger、锁或日志。
- ledger 损坏时 apply fail closed；dry-run 可以继续发现并报告，但不能据此覆盖任何既有目标。
- V1 不做隐式“空 ledger 继续”。显式 state rebuild 只在后续确有需要时设计；当前可依据目标 `meta.yml` 手工审计后恢复 ledger。

## 5 · 写入、安全与中断恢复

### 5.1 原子提交

每条 meeting 是独立事务：

1. 在 Vault 内、与目标同一文件系统创建唯一 staging 目录。
2. 按计划写唯一的 `transcript.md` 或 `transcript.srt`，flush、`sync_all`，重新解析并验证输出；来源有 `summary.md` 时再按原始字节写入并校验哈希。
3. 生成扁平 `meta.yml`；metadata 最后写，作为激活标记。transcript/summary/meta 哈希只写 ledger。
4. 创建使用 no-clobber rename；更新在提交前重读目标并做 expected-hash CAS。
5. 成功激活目标后，再原子更新 ledger。
6. staging、transcript、meta、ledger 任一点崩溃，重跑都只能恢复或报告冲突，不能留下一个被会议列表当成成功记录的半成品。

Create 和 Update 都先写单事务 journal，再激活目录、更新 ledger、清理旧目录并清除 journal。恢复过程必须先核对事务 nonce 和 transcript/summary/meta 的完整哈希；任何额外文件或哈希偏差都 fail closed，不能递归删除未验证目录。新事务不得覆盖尚未恢复的 journal。

插件扫描目标库时，只展示同时满足以下条件的目录：目录名合法、`meta.yml.transcript_file` 指向唯一存在且校验通过的逐字稿；若声明 `summary_file`，对应文件也必须存在。完整性哈希从 ledger 校验，不要求写入 metadata。

### 5.2 权限边界

- UI 的目录选择只负责得到用户明确选择；递归读取放在受信任的原生后端，因为当前 Host 外部文件授权不会自动覆盖所有后代。
- 后端只暴露 Hemory plan/apply 这组窄 RPC，接收本地 UI 选择出的目录或 CLI 已 canonicalize 的 path 后再次做根目录、schema、symlink 和越界检查；不提供通用的任意文件读取 RPC。
- 目标固定为当前 Vault 的 `ssot/meetings`，不做可配置任意目标路径。
- 目标父目录和每个 leaf 都检查 symlink；任何 path traversal 或越界都阻断。
- UI 与 CLI 并发迁移使用同一把进程间锁；单个 UI 按钮禁用不视为并发保护。

## 6 · CLI

现有插件 CLI 是扁平命令，V1 定义：

```bash
# 预检，不写入
notemd meetings-import-hemory <source> --dry-run [--full] [--user <id>] [--timezone Asia/Taipei] [--json]

# 执行；默认增量，首次运行自然导入全部
notemd meetings-import-hemory <source> [--full] [--user <id>] [--timezone Asia/Taipei] [--json]
```

- `<source>` 在 manifest 中声明为 `path`，由 Host canonicalize 并验证存在。
- `--full` 是 boolean；缺省为 incremental。`--json` 使用 Host 已有全局参数。
- 检出多用户时，必须 `--user`；时区只在历史时间缺 offset 时要求。
- 参数错误由 Host 返回 2；插件禁用返回 3；迁移执行错误返回 4。
- 正常完成但有 `conflict/blocked` 时命令仍输出完整报告，并返回 4，方便自动化发现未完全迁移。
- `--json` 下该情形输出 `{ "ok": false, "data": <MigrationReport> }` 到 stdout，stderr 保持为空；报告不再嵌入错误字符串。
- CLI Host 对插件任务有约 300 秒 watchdog。由于每条会议都会提交 checkpoint，大库超时后可安全重跑；V1 不承诺单进程无限时长。

CLI 与 UI 必须调用同一个 Rust `MigrationService::{plan, apply}`，不能复制两套字段解析或冲突规则。

## 7 · 插件主界面

主窗口先提供最小可用会议库：按 `started_at` 倒序展示会议，显示标题、时长、speaker 数和来源，点击用编辑器打开逐字稿。

“从 Hemory 迁移…”流程：

```
选择只读来源目录
  → 检测布局和用户
  → 选择用户 / 必要时补历史时区
  → 选择增量或全量
  → 自动预检
  → 展示 create/update/skip/conflict/blocked/excluded 明细
  → 用户确认迁移
  → 后台逐条提交与 checkpoint
  → 结果页可打开目标或导出 JSON 报告
```

- 首次建议“增量（将导入全部）”，不需要用户理解一个特殊首跑模式。
- 确认使用窗口内 sheet，不使用浏览器 `window.confirm`。
- 进度按已提交会议数展示；关闭窗口或停止任务只在条目边界终止。
- 报告中展示来源相对路径和目标路径，不显示绝对源路径、逐字稿正文或不必要的说话人隐私信息。
- SRT 只有命中 note.md 的原始资料模式才进入全局搜索。插件应给出“一键添加 `ssot/meetings/**` 搜索来源”的显式操作，不静默修改用户搜索设置。

## 8 · 稳定报告

人类界面和 JSON 共用以下语义：

```
schema_version, mode, dry_run, source_user
scanned, eligible, create, update, skip
conflict, blocked, excluded_audio, committed, source_missing
warnings[], errors[], items[]
```

每个 item 至少有：

```
conversation_id, source_relative_path, source_schema
selected_transcript, source_fingerprint, target_relative_path
action, reason, output_hashes
```

items 按 conversation ID、来源相对路径稳定排序。dry-run 和 apply 对未变化来源必须生成同一 plan；apply 仍在提交前重读 source/target 做 CAS，防止预检后内容变化。

## 9 · 验收门禁

### 来源与解析

- 当前单数布局、历史复数 `conv_` 布局、老 iOS session 布局都有 fixture。
- 路径已迁但 metadata 仍是旧 schema 时按字段识别成功。
- `category` 的 string/list 历史漂移不会造成全批失败。
- `content.md` 存在时原字节输出 `transcript.md`；它不存在时 pro SRT 原字节输出 `transcript.srt`。
- `content.md` 存在但无时间/说话人时被阻断，不静默 fallback；没有 content 且 pro SRT 缺 speaker 时也被阻断。
- `[spk_NN]`、`[NN_spk_MM]` 与 speaker mapping 正确。
- malformed/空 SRT、非法时间、ID 与 metadata 冲突、只有音频均不会产生目标目录。

### 幂等与冲突

- 同一源重复增量和重复全量，输出字节与 mtime 不变。
- 来源变化且目标未动可更新；目标被本地编辑绝不覆盖。
- 来源删除或进入 tombstone 后目标仍保留并报告。
- 同 ID 跨用户、同用户重复来源、未管理的目标目录均明确冲突。
- dry-run 前后 Vault 与来源的递归文件快照、mtime 完全一致。

### 安全与恢复

- 输出递归检查只含 `transcript.srt|transcript.md|summary.md|meta.yml`，没有任何音频扩展名或音频路径。
- source symlink、target symlink、越界 transcript_file 全部拒绝。
- 在 staging、transcript 提交、meta 激活、ledger 更新四点注入崩溃，重跑均无重复或半成品展示。
- UI/CLI 并发、预检后来源变化、提交前目标变化均由锁或 CAS 拒绝。
- ledger 损坏阻断 apply，不退化成覆盖模式。
- UI 与 CLI 对同一 fixture 生成完全相同的 plan 和输出哈希。

### 真实数据门禁

`hemorydesign` 仓库没有真实 `.srt` / metadata fixture，只有生产代码、spec 与测试内嵌样例。实现开始后、正式 apply 前必须：

1. 由用户选择真实 Hemory 的只读目录或提供脱敏副本。
2. 先跑 dry-run，人工抽查现行、最老、speaker 最复杂和坏数据各一条。
3. 确认扫描数、阻断数、时间区、speaker 映射和“音频零输出”。
4. 用临时 Vault 做一次 apply，再重复 incremental/full 验证幂等。

## 10 · 实施顺序（审核通过后）

1. 建 Hemory current/legacy fixtures 和规范化模型，先写失败测试。
2. 实现只读发现、metadata/SRT/speaker parser 与白名单校验。
3. 实现 planner、ledger、锁、原子 writer、崩溃恢复与测试。
4. 接 CLI，验证 dry-run、JSON、退出码和 watchdog 后重跑。
5. 建插件窗口、会议列表、目录选择、预检/确认/进度/结果页。
6. 接入构建/开发安装/发布脚本中的插件显式清单。
7. 用真实只读 Hemory 数据完成 §9 的门禁；确认后才允许正式迁移。

## 11 · 已批准决定

### A. 导入哪些 Hemory Conversation

**已确认：导入所有具有合格逐字稿的 Conversation，并保留原 category。**

理由：历史数据的 `category` 可能缺失或发生 string/list 漂移，只筛 `meeting` 会静默漏掉真实会议。UI 预检可以按 category 筛选，但默认不因分类质量丢资料。

### B. 是否带入已有总结

**已确认：来源存在 `summary.md` 时，原样复制为目标 `summary.md`。**

summary 是派生内容，不取代权威逐字稿；`meta.yml` 只记录 `summary_file`，来源和哈希进入 ledger。缺失 summary 不阻断会议迁移。

### C. Hemory 导入输出格式

**最终确认：优先复制 Hemory `content.md` 原文件为 `transcript.md`；只有它不存在时，复制 `pro_asr.srt` 原文件为 `transcript.srt`。**

同一会议始终只有一个权威 transcript。`meta.yml` 只记录目标文件名；报告和 ledger 记录 `source_kind` 及源/目标哈希。不自动使用其他 SRT fallback。

### D. metadata 字段

**最终确认：`meta.yml` 只保留 14 个扁平字段。**

字段为 `conversation_id`、`created_at`、`end_at`、`title`、`category`、`key_topics`、`language`、`source`、`duration_ms`、`speaker_count`、`transcript_file`、`summary_file`、`imported_from`、`updated_at`。其中 `source` 固定为 `hemory_v1.0:<原 source/device_source>`，缺失文本时为 `hemory_v1.0:unknown`。覆盖保护以 ledger 中的上次输出哈希和提交前 CAS 为准，不能只依赖 `updated_at`，否则手工编辑但未更新时间的目标仍可能被覆盖。