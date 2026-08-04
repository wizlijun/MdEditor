# agent 上下文回到源目录 + vault 外 sync 文档的 `↔` 标记 —— 设计

日期:2026-08-04

## 问题

用户在 vault **之外**打开一篇 `.md` 并写批注时,note.md 会把源文件镜像进 vault,
伴生 `.note.md` 落在 vault 镜像旁(信念 4:批注属于 vault,不属于路径)。但由此产生
三个问题:

1. **agent 的上下文错位。** `answer-note-question` 跑起来时,`${SOURCE}` 解析成
   vault 里的**镜像副本**,agent 只能在 vault 这个孤岛里读一份快照。用户真正的语境
   ——源文档所在的那个项目目录(相邻文档、图片、项目自己的约定文件)——完全看不到。
   基于 vault 处理上下文不合理:文档的家在原目录。
2. **`OutlinePanel` 把错的笔记路径交给 agent。** 面板用裸 `companionPathFor(源路径)`
   算伴生路径,没走 vault-homed 解析,于是对一篇 vault 外的已同步文档给出
   `<源目录>/X.note.md` —— 一个**不存在**的路径。agent 后端拒绝:
   `note is outside the vault: …`。
3. **用户看不出自己在编辑一份「有镜像的源文件」。** 打开 vault 镜像时程序会自动
   重定向到源件(`tabs.svelte.ts:251`),但标题栏/标签页没有任何提示。

## 目标

- 所有 sync 过来的文档,其笔记的 agent 上下文位置保持在**用户文档的原目录**下。
- vault 外、已有 vault 镜像的文档,在标签页与窗口标题上用 `↔` 醒目标出,含义是
  「始终编辑源 md」。
- 顺手修掉 (2) 的路径 bug。

## 非目标

- 不改 cwd。claude 的 cwd 仍是任务模板目录 —— vault 约定、`.claude/skills`、
  `.mcp.json` 的发现都依赖它;切到用户目录会连带加载用户项目自己的 CLAUDE.md。
- 不给 agent 任何源侧写权限。写权限仍然只有 `${NOTE}`,「绝不修改源 md」的硬约束不变。
- 不迁移存量数据,不改 `.note.md` / 镜像 meta 的磁盘格式。

---

## §0 前置修复:`OutlinePanel` 的笔记落点

`src/components/outline/OutlinePanel.svelte:17`

```svelte
let companionPath = $derived(applicable && tab ? companionPathFor(tab.filePath) : null)
```

改为走与 `OutlineEditor.svelte:83` 同一条解析:

```svelte
let companionPath = $derived(
  applicable && tab
    ? (noteHomeForRead(tab.filePath, { vaultRoot: sotvaultStore.vaultRoot, records: sotvaultStore.records })
        ?? companionPathFor(tab.filePath))
    : null,
)
```

`noteHomeForRead` 对 `.note.md` tab 本身返回 null,与旧行为一致。该 derived 同时喂给
`AgentWorkspace`(bug 现场)、铅笔菜单的「打开笔记 markdown」与「删除笔记」——后两者
此前会在不存在的源侧路径上开空 buffer / 删不到文件,一并修好。

`question-capture.ts:53` 的 `companionPathFor` 只用于「让位给大纲编辑器」的快速判断,
后面还有 `outline.docPath === notePath` 第二道兜底,不受影响,不动。

---

## §1 源路径解析(claude-agent 后端新模块 `mirror.rs`)

后端进程不知道 deviceId(它由前端生成、存在宿主 settings 里),所以**按存在性解析**,
不按设备:

- `read_metas(vault) -> Vec<MirrorMeta>`:扫 `{vault}/.notemd/mirrors/*.json`,
  逐个 `serde_json` 解析,坏文件跳过(消费者宽容义务)。字段取 `mirror`(vault 相对
  路径)、`source`(绝对路径)、`checksum`。
- `source_for_mirror(vault, mirror_abs, metas) -> Option<PathBuf>`:筛出 `mirror`
  指向同一个绝对路径的 metas → 在候选 `source` 里取**本机存在、且文件名与镜像 stem
  相符**的第一个。跨设备来的 meta 天然落空。
- `local_source_dirs(metas) -> Vec<PathBuf>`:本机存在的 `source` 的父目录,去重、
  排序(输出稳定,便于测试)。

解析不到 → `None`,一律回退现有行为(读 vault 镜像),不报错、不中断运行。

## §2 agent 上下文改为原目录

- `settings::Scope` 增加 `source_dir`;`Scope::for_note(vault, note)` 的 `source`
  优先取 `source_for_mirror` 解析出的**原始 md 绝对路径**,拿不到才回退同名 vault
  镜像。`source_dir` = `source` 的父目录。
- `templates/answer-note-question/settings.scoped.json` 新增
  `"Read(${SOURCE_DIR}/**)"`。`${SOURCE}` 语义自然变为原文件。写/改权限不变。
- **全库扫描**(`scope = None`,CLI 与守望器走这条):`settings::materialize` 在字符串
  替换后再解析一次 JSON,把 `local_source_dirs` 的每个目录作为 `Read(<dir>/**)`
  追加进 `permissions.allow`。这是动态列表,不进模板。
- `templates/answer-note-question/CLAUDE.md` 协议第 2 条改写:源文按笔记 front-matter
  的 `sources:` 指向的**原始文件**读(可能在 vault 外),该路径读不到时回退到笔记同目录
  的同名 `.md`(vault 镜像)。「绝不修改源 `.md`」原样保留。
- `plugin.rs::run_note` 里那段动态 prompt 补一句同样的口径,避免与 CLAUDE.md 打架。

## §3 `↔` 标记

**打标条件**:tab 的 `filePath` 是 vault 外、且已有 vault 镜像的源文件 —— 即
`sotvault-logic.ts` 现成的 `isSyncedSource(path, records)`。纯 vault 文件、无镜像的
普通文件、本机无源路径的跨设备镜像本体都不打标。

- `src/lib/sotvault.svelte.ts` 导出 store 包装 `isMirroredSource(path)`。
- `TabBar.svelte`:标题前加独立 `<span class="sync-mark" aria-hidden="true">↔</span>`
  (不拼进文件名,免得被截断后误读成文件名的一部分);tab 的 `title=` 改为
  `` `${t('syncMark.tooltip')}\n${tab.filePath}` ``。
- `App.svelte:699` 窗口标题 effect:打标时 `↔ foo.md — note.md`;effect 里读
  `sotvaultStore.tick`,首次 sync 完成后标题立即刷新。
- i18n 四语新增 `syncMark.tooltip`(en/zh/ja/de),文案义为「始终编辑源 md ·
  已同步到 vault」。`↔` 字符本身不翻译。

标题拼装抽成纯函数 `windowTitleFor(tab, marked)` 放 `src/lib/window-title.ts`,便于单测。

---

## 测试

- Rust(`mirror.rs`):metas 扫描容错(坏 JSON / 缺字段)、跨设备源不存在时返回 None、
  文件名不符时不误配、`local_source_dirs` 去重排序。
- Rust(`settings.rs`):scoped 运行的 `${SOURCE}`/`${SOURCE_DIR}` 落到原目录;
  解析不到时回退镜像;全库运行追加 `Read(<dir>/**)` 且不追加任何写权限;模板文件
  自身永不被改写(现有断言沿用)。
- 前端(vitest):`isSyncedSource` 打标判定(源件/vault 文件/未同步文件/note 文件)、
  `windowTitleFor` 拼装、`OutlinePanel` 的 note home 解析(纯函数层
  `noteHomeForRead` 已有测试,补一条 vault-homed 源件 → vault 伴生路径的用例)。
- GUI 手动验证(按惯例由用户实机跑):
  1. 打开 vault 外一篇已同步文档 → 标签页与窗口标题出现 `↔`,hover 有说明;
  2. 该文档的手记里对一条批注提问 → 点「问 agent」不再报
     `note is outside the vault`,运行成功;
  3. 运行记录里可见 agent 读的是原目录下的源文件;
  4. 打开纯 vault 文件 / 未同步的普通文件 → 无 `↔`。
