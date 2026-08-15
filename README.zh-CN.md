# note.md

[English](README.md) · [简体中文](README.zh-CN.md) · [notemd.net](https://notemd.net)

> **读 AI 写的，留下你想的，留住只有你才写得出的字。**

为 AI-native 时代打造的 markdown 阅读器、编辑器、双链笔记工具。原生 macOS 应用，下载约 11 MB，  
装完约 15 MB。你的笔记是磁盘上一个纯 `.md` 文件夹——永远属于你。

[下载](https://notemd.net/download) · [插件市场](https://plugins.notemd.net) · [完整功能清单](docs/FEATURES.zh-CN.md)

---

## 1. 读 agent 写的东西，这里体验最好

富文本与源码双模，一个快捷键之隔。任意导入 Notion、Typora 主题。Mermaid、Graphviz、  
KaTeX 都专门调过，按需加载。没有捆绑 Chromium——整个应用装完约 15 MB。

高亮一句断言，在旁边留下你的疑问，就地把写错的句子改对。

Claude、Codex、OpenClaw 各有各的对话窗口，但没有一个是**读**的地方。这里是。

## 2. 上一代笔记工具做对的事，全都内置

local-first、git sync、大纲、`[[双链]]`与反向链接、wiki 页面、每日笔记、  
全库检索、插件机制。

这些是 Roam Research 和 Obsidian 想明白的事，note.md 把它们落在文件上：一个  
插件导入你整份 Roam 数据，Obsidian 的 vault 直接打开。

## 3. 它自己不带 AI。它仍然是 AI-native。

note.md 不调模型、不发一个请求。

你的 vault 被设计成多 agent、多 harness 共用的、受版本控制的上下文环境——  
Claude Cowork、Claude Code、Codex、ChatGPT Work、OpenClaw、Hermes——它们通过  
公共约定（`AGENTS.md`、块引用、手记 `.note.md`）读写同一批文件。记忆系统在路上。  
留得越多，agent 越懂你。

你随时可以换 AI 工具。资产始终在你手里。

用 Claude Code 做产品这条路径——写文档、读文档、审批 AI 生成的文档——是专门  
调优过的。

## 4. 剩下的，你自己长出来

写个插件。配一条 OpenClaw 定时任务。挂上 skills。

在批注里打一个 `?`，agent 就会接走：异步改这篇文档、补上你要的上下文，再交  
回来等你决定采不采纳。

更多等你发掘。

---

## 五个信念

1. **AI 的文字无限，你的注意力有限——你的判断才是残余。**  
   你留在字里行间的东西，是任何模型都生成不出来的。
2. **文件高于应用（files over app）。** 每篇笔记都是纯 `.md`：对 git 友好、  
   可 grep、五十年后依然可读。索引是派生数据，文件是唯一事实源。
3. **agent 是一等公民——它建议，你确认。**  
   [关系只在人确认处生长](docs/product-principle-relationships-only-grow-where-confirmed.md)；  
   note.md 绝不自动串联你的笔记，也绝不用 agent 的垃圾填满 vault。
4. **[你的批注属于 vault，不属于路径。](docs/product-principle-mirror-hosted-marks.md)**  
   在哪儿批注都行，源文件会被镜像进 vault，批注由此获得稳定的、git 版本化的宿主。
5. **[一个 vault，多个 agent——你是编排者。](docs/product-principle-one-vault-many-agents-you-orchestrate.md)**  
   工人可替换，握笔的是你。

## 严格遵循 OKF v0.2

信念 2 需要的是一套格式，不只是一个后缀。note.md 严格遵循  
[Open Knowledge Format](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
（OKF）v0.2——Google Cloud 开放的知识文档规范，人与 agent 交换知识用的公共约定：  
纯 Markdown、YAML frontmatter、可 diff、可移植。

- **写出去的每一份都合规。** note.md 新建的每份文档都以 YAML frontmatter 开头、  
  带上必填的 `type`——⌘N 新建、`.note.md` 手记、每日笔记、wiki 页、Roam 与电子书  
  导入、生成的报告，一个不落。存量大纲笔记在你下次保存时补上 `type`；从别处带进来  
  的纯 `.md` 一个字都不动——绝不往你的文件里塞应用私货。
- **来源与信任用规范自己的词汇。** `sources`、`generated`、`verified`、`status`、  
  `stale_after` 全部沿用 OKF 的字段名；actor 用 OKF 的三种形式——工具  
  `<producer>/<version>`、人 `human:<id>`、自动流程 `process:<id>`。于是"这句是人  
  确认过的"成了机器可读的事实，而不是靠猜。
- **读进来的一份都不拒绝。** 缺可选字段、不认识的 `type`、多出来的未知键、断掉的  
  链接——任何一条都不会让文档被拒绝，note.md 读不懂的键在往返后原样保留。这是  
  OKF 的宽容一致性，规范把它定为 MUST。
- **是验过的，不是嘴上的。** `pnpm okf:lint <目录>` 按规范的三条硬约束审计任意  
  文件夹；每一个写文档的路径都有测试拿它校验产物。

你的 agent 拿到同一份契约：vault 的 `AGENTS.md` 里写明了 OKF 要求，在这个文件夹里  
干活的 agent 也照此写文件。还在路上的部分：由应用自动填 `generated` / `verified`，  
以及 bundle 级导出（`index.md`、`log.md`、把 wikilink 改写成 OKF 链接）——进度见  
[一致性审计](docs/okf-v0.2-conformance-audit.md)。格式细节见  
[`docs/okf-v0.2-format-constraints.md`](docs/okf-v0.2-format-constraints.md)。

## AI 写的，人负责

note.md 完全由 AI Coding 开发和维护，所以更新很快。维护者是专业的软件工程玩家，
每一次改动都经过审阅、测试和发布前实机验证。

## 引擎盖下

基于 [Tauri](https://tauri.app) 与  
`[@moraya/core](https://www.npmjs.com/package/@moraya/core)` 构建：签名并公证的  
原生 macOS `.app`——原生 Rust 二进制，菜单、窗口、托盘均为系统原生控件——编辑器  
UI 渲染在系统 WebView（WKWebView）里，而不是一个捆绑的浏览器。

产品名为 **note.md**（全小写——一篇笔记就是一个 markdown 文件）；CLI 二进制与  
bundle identifier 为 `notemd` / `net.notemd.app`，旧的 `mdedit` 软链仍可用。  
源码中仍会看到 `mdeditor`（Rust crate 名为 `mdeditor_lib`）。v4.8.0 之前以  
**M↓** 为名发布。

## 开发与构建

```bash
pnpm install
pnpm tauri dev            # 开发
pnpm tauri build          # 构建，当前架构
```

两个架构分别构建（各自独立 `.app`，Universal 模式已废弃）：

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm tauri build --target aarch64-apple-darwin
pnpm tauri build --target x86_64-apple-darwin
```

输出：`src-tauri/target/<arch>-apple-darwin/release/bundle/macos/note.md.app`  
（当前架构则在 `src-tauri/target/release/…`）。

## CLI

```bash
notemd search "关键词" --vault ~/Vault      # 全文检索，输出 path:line:text
notemd search "关键词" --json               # 带 source_ref、origin、来源信息、attention_minutes（由桌面端记录）
notemd search "关键词" --all                # 返回全部命中（默认上限 20 条，--limit N 可调）
notemd share draft.md                      # 发布分享链接，输出 URL
notemd share draft.md --json               # 结构化输出
notemd share draft.md --unshare            # 取消分享
notemd plugin list                         # 列出插件及启用状态
notemd reading-insights report --vault ~/Vault --date 7d
notemd doctor                              # 自检环境、Vault、索引、插件与网络（--offline、--json）
notemd help                                # 完整帮助
```

内置核心命令，外加**已启用**插件贡献的子命令。从  
**Help → Install 'notemd' Command in PATH…** 安装。

agent 最常用的是 `notemd search`。它有意长成 grep 的样子，`rg` 的习惯照用；
`--json` 每条命中给出 `source_ref`（`path#Lline`）与来源信息 —— 模型写的命中
会自报家门，可以顺着它回到原始文档，而不是直接采信。过滤语法与应用内搜索
面板一致（`tag:` `type:` `path:` `ext:` `after:` `before:` `page:[[X]]`
`origin:`）。详见 `notemd help search`。

## 发布（仓库维护者）

```bash
scripts/release.sh <x.y.z>
```

依次执行：测试 → 版本号 → 按架构签名构建 → 公证 → 打 tag → push → GitHub  
Release（两个 `.dmg`、两个 updater 包及签名、驱动按架构自动更新的  
`latest.json`）。需要 `.env.release` 中的 `APPLE_ID`、`APPLE_PASSWORD`、  
`APPLE_TEAM_ID`，以及 `~/.tauri/mdeditor.key` 的 updater 签名私钥。

## 文档

- 完整功能清单：`[docs/FEATURES.zh-CN.md](docs/FEATURES.zh-CN.md)`
- 知识文档格式（OKF v0.2）：[`docs/okf-v0.2-format-constraints.md`](docs/okf-v0.2-format-constraints.md)
  · [一致性审计](docs/okf-v0.2-conformance-audit.md)
- 写插件：`[docs/plugin-v2-development.md](docs/plugin-v2-development.md)`
- 设计与计划：`docs/superpowers/specs/`、`docs/superpowers/plans/`

## 致谢

感谢 **[Effie](https://www.effie.co/)** 与  
**[葫芦笔记（Hulunote）](https://github.com/hulunote/hulunote)** 一路的支持与  
鼓励，也感谢它们让人看见一款无干扰写作工具、一款开源双链大纲笔记可以是什么  
样子。内置的 **effie** 主题，是向前者的致意。

## 许可证

Apache-2.0（与 `@moraya/core` 一致）。