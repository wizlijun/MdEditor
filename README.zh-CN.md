# note.md

[English](README.md) · [简体中文](README.zh-CN.md) · [notemd.net](https://notemd.net)

> **读 AI 写的，留下你想的，留住只有你才写得出的字。**

为 AI-native 时代打造的 markdown 阅读器、编辑器、双链笔记工具。原生 macOS 应用，下载 7 MB，  
装完 11 MB。你的笔记是磁盘上一个纯 `.md` 文件夹——永远属于你。

[下载](https://notemd.net/download) · [插件市场](https://plugins.notemd.net) · [完整功能清单](docs/FEATURES.zh-CN.md)

---

## 1. 读 agent 写的东西，这里体验最好

富文本与源码双模，一个快捷键之隔。任意导入 Notion、Typora 主题。Mermaid、Graphviz、  
KaTeX 都专门调过，按需加载。没有捆绑 Chromium——整个应用 11 MB。

在这里，阅读不是被动的。高亮一句断言，在旁边留下你的疑问，就地把写错的句子改对。

Claude、Codex、OpenClaw 各有各的对话窗口，但没有一个是**读**的地方。这里是。

## 2. 上一代笔记工具做对的事，全都内置

local-first、git sync、大纲、`[[双链]]`与反向链接、wiki 页面、每日笔记、  
插件机制。

这些是 Roam Research 和 Obsidian 想明白的事，note.md 把它们落在文件上：一个  
插件导入你整份 Roam 数据，Obsidian 的 vault 直接打开。

## 3. 它自己不带 AI。它仍然是 AI-native。

note.md 不调模型、不发一个请求。它做的是另一件事。

你的 vault 被设计成多 agent、多 harness 共用的、受版本控制的上下文环境——  
Claude Cowork、Claude Code、Codex、ChatGPT Work、OpenClaw、Hermes——它们通过  
公共约定（`AGENTS.md`、块引用、手记 `.note.md`）读写同一批文件。记忆系统在路上。

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

## AI 写的，人负责

note.md 完全由 AI Coding 开发和维护——这也是它懂「读 AI 写的东西」是什么感觉的
原因：这个工具就是按它期待你工作的方式造出来的。

所以更新很快。而把关的是一个专业的软件工程玩家：每一次改动都经过审阅、测试、
发布前实机验证。速度是 AI 的，质量由人来担。

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
notemd share draft.md                      # 发布分享链接，输出 URL
notemd share draft.md --json               # 结构化输出
notemd share draft.md --unshare            # 取消分享
notemd plugin list                         # 列出插件及启用状态
notemd reading-insights report --vault ~/Vault --date 7d
notemd help                                # 完整帮助
```

内置核心命令，外加**已启用**插件贡献的子命令。从  
**Help → Install 'notemd' Command in PATH…** 安装。

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
- 写插件：`[docs/plugin-v2-development.md](docs/plugin-v2-development.md)`
- 设计与计划：`docs/superpowers/specs/`、`docs/superpowers/plans/`

## 致谢

感谢 **[Effie](https://www.effie.co/)** 与  
**[葫芦笔记（Hulunote）](https://github.com/hulunote/hulunote)** 一路的支持与  
鼓励，也感谢它们让人看见一款无干扰写作工具、一款开源双链大纲笔记可以是什么  
样子。内置的 **effie** 主题，是向前者的致意。

## 许可证

Apache-2.0（与 `@moraya/core` 一致）。