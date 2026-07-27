# 批注问答闭环(Annotation Q&A Loop)设计

日期:2026-07-27
状态:已确认

## 一句话

阅读时批注里带 `?`/`？` 即向 agent 异步提问;任意外部 agent 按 AGENTS.md 协议 sweep 作答,✦ 答案回填 `.note.md`;人裁决 close。源 md 永远不动。

## 背景与动机

当前高亮/批注是"写入即死亡"的信号:留存了用户的注意力痕迹,但没有复利——事后不再被读、不被复用。本设计把批注从记录的**终点**变为异步协作的**起点**:批注 = 用户对 agent 说的半句话,agent 负责异步补完另外半句。

与产品五大信念的对应:

- **信念 1(判断是残余)**:从"留存判断"推进到"判断产生复利"。
- **信念 3(agent 建议,你确认)**:agent 只能作答,不能关闭问题;✦/● 标记作者身份。
- **信念 5(一个 vault 多个 agent)**:note.md 只定协议不跑模型,任意 harness 可执行 sweep——这是该信念的第一个完整落地场景。
- **既有原则「关系只在 .note.md 结网」**:问题元数据全部落 `.note.md`,源 md 与 CriticMarkup 语法零改动。

## MVP 范围

三块:①数据格式与状态机;②note.md 端捕获/展示/裁决 UI;③AGENTS.md / llms.txt 协议文本。**不含**任何内置模型调用——sweep 由外部 agent(Claude Code cron、OpenClaw heartbeat、手动指令等)执行。

## 1. 数据格式(唯一事实源,全部纯文本)

### 问题节点

复用 `.note.md` 现有 `::` 属性行机制:

```markdown
- ?这里说的 KV cache 命中率为什么能到 90%?
  type:: question
  status:: open
  line:: 142
  created:: 2026-07-27T10:03:00Z
```

### 作答后(agent 写入,短答案)

```markdown
- ?这里说的 KV cache 命中率为什么能到 90%?
  type:: question
  status:: answered
  line:: 142
  created:: 2026-07-27T10:03:00Z
  - ✦ 因为推理场景 prompt 前缀高度重复……(要点)
    answered:: 2026-07-27T14:22:00Z
    by:: claude-code
```

### 长答案

需要展开研究的答案落独立文件:vault 根下约定目录 `answers/`,文件名 `yyyy-MM-dd-<slug>.md`。问题节点下只放一行 ✦ 摘要 + 链接(wikilink 或相对路径)。`answers/` 写进 AGENTS.md 的 vault layout 说明。

### 状态机

```
open ──(agent 作答)──> answered ──(人确认)──> closed
  ↑                         │
  └───────(人追问)───────────┘
```

- **agent 只允许 open → answered 这一个迁移**;closed 只能由人拨。这是信念 3 在状态机上的硬约束。
- 人追问:在答案下以 ● 子节点追加内容,状态拨回 open,形成异步对话线程。
- 误判的"假问题"(反问语气等)由人直接 close 或删节点,捕获端不做复杂度。

## 2. 捕获(note.md 端)

**约定式识别,自然书写即协议**:批注文本含 `?`/`？` → 该批注是问题。同步进 `.note.md` 大纲时该节点自动带 `type:: question, status:: open`。

**提问视为明确保存意图,触发 `.note.md` 落盘**(与既有"按意图保存"规则一致——外部 agent 只能读磁盘文件,不落盘协议就是空话)。

源 md 里的 CriticMarkup(`{==…==}{>>…<<}`)原样不动,零新语法。

### UI 改造三条

1. **问题徽标换符号**:批注含 `?`/`？` 时,正文里的批注徽标从 `※` 换成 `⁇`(U+2047)。实现走 CSS 属性选择器:`.moraya-note-anchor[data-note*="?"]::before { content: '⁇' }` + 全角 `？` 变体;包裹批注的 `note-badge` widget 同样带上 `data-note` 属性即可复用同一规则(现符号定义在 `src/styles/editor-base.css` 的 `::before`)。视觉效果:扫过正文即可分辨"我的备注"(※)与"抛给 agent 的问题"(⁇)。
2. **批注输入气泡加醒目「⁇ 提问」按钮**:置于保存按钮旁,accent 色突出。点击行为 = 文本尾部无问号则自动补 `？` 并保存。按钮只是糖——**问题身份始终由文本中的问号承载**,不引入隐藏状态;在 Obsidian/CLI 手写问号完全等价(file-over-app 不破)。
3. **引导性 placeholder**:批注输入框空态提示语,如"写批注……以 ? 结尾即向 agent 提问",把协议教学摊进每次输入。

## 3. 协议(AGENTS.md 新增段落,模板自带;同步进 llms.txt / llms-full.txt)

> - 扫描 vault 中 `type:: question` 且 `status:: open` 的 `.note.md` 节点;
> - 结合 `line::` 定位伴生源文件的原文上下文作答;
> - 短答案以 `✦` 前缀子节点回填,附 `answered::` 与 `by::`;长答案写入 `answers/`,节点下只放 ✦ 摘要 + 链接;
> - 将 `status::` 置为 `answered`;**绝不置 closed,绝不修改源 `.md`,绝不改动 ● 内容**。

llms.txt 中现有"agent 对 `.note.md` 只读"的约定需相应放宽为:仅允许上述问答回填写入,其余仍只读。

## 4. 展示与裁决(note.md 端 UI)

- **大纲 tab**:question 节点显示状态 chip(open / answered / closed);✦ 答案节点按既有 ✦ 视觉呈现。
- **FolderView**:文件行角标显示"N 个新回答"(`status:: answered` 计数),复用既有"有笔记"角标机制。
- **裁决**:chip 一键 close;answered 状态下人在答案下追加 ● 子节点时,提示是否拨回 open。
- **正文锚点联动**(MVP 或二期,视实现顺手程度):状态为 answered 时,正文 `⁇` 徽标原地变色/加小圆点,点开批注气泡直接读答案——阅读动线比跳大纲更短。

## 5. 明确不做(YAGNI,格式留门)

- 不内置模型 / API key / 自动 sweep 调度;
- 不自动 close、不自动建 `[[链接]]`(consent-based 原则不变);
- 其他信号类型(存疑/待办/@召唤/高亮周报)不进 MVP——`type::` 枚举天然可扩展,协议文档预留一句"未来会有更多 type";
- 不做跨文档聚合、不做通知推送(角标即通知)。

## 6. 验证方式

- 格式解析/状态机/问号识别走单测;
- 端到端:在真实文档上提两个问题 → 用 Claude Code 按 AGENTS.md 协议跑一次真实 sweep → GUI 中验证角标、✦ 答案展示、close 一个、追问 reopen 一个;
- GUI 部分按惯例由用户实机验证。

## 远期方向(不进本期,仅记录)

- **闭环数据复利**:closed 问答对是"你关心什么、什么答案能说服你"的高纯度语料,可喂 Reading Insights 或个人化检索。
- **纯高亮周期回收**:`type:: highlight` 节点每周聚合进 Weekly Review,配 consent-based `[[链接]]` 建议。
- **跨文档共振**:agent 发现同一论断在多篇文档被反复划线/提问时主动提示,连线仍等人确认。
