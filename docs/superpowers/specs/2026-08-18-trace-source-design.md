# 溯源(Trace to Source)设计

日期:2026-08-18
状态:已与用户对齐方案 A,进入实施

## 背景与目标

用户读到一段话(AI 写的或转述的),想知道它的原始出处——YouTube 视频、论文、欧美博客。
「溯源」把选中文本交给 agent:检索定位原文 → 下载字幕/正文 → 按用户关注点生成摘要 md,
摘要带反向链接指回提出问题的文档与选区,用户点相对链接继续阅读溯源到的全文材料。

对应产品信念:判断才是残余(你追问出处的那段话值得留存)、文件高于应用(产物全是纯 md)、
agent 建议你确认(agent 只写 traces/,永不写源 md)。

## 已定决策(用户拍板)

1. **触发**:右键「溯源」与输入面指令两路都要,统一归到一个委托界面;奇思妙想(idea-spark)
   作为输入面,用户输入 `/溯源 …` 委托,执行一律异步。
2. **抓取能力全交给 harness**:不写宿主/插件侧抓取代码,靠提示词模板指导 agent 用
   WebSearch / WebFetch / 本机 yt-dlp。
3. **结果回流取轻方案**:只生成摘要 md + 统一通知;不写源 md,也不写源文档的 .note.md。
4. 为 YouTube 字幕给 trace 模板放开 `Bash(yt-dlp:*)` 一个口子(没装则降级);摘要落
   `<vault>/traces/`。

## 核心抽象:指令(directive)= task 模板

`.notemd/agent-tasks/<id>/task.json` 新增**可选**字段 `directive: string[]`
(名字列表,如 `["溯源", "trace"]`,首项为展示名)。声明了 `directive` 的 task 模板
自动成为输入面可用的 `/xxx` 指令。

- 字段加在 agent-run-core 的 `TaskDef`(自家 crate,serde 可选字段,老版本忽略即可;
  **不碰** manifest——`ManifestV2`/`Contributes` 是 `deny_unknown_fields` 红线)。
- 用户自己写一个带 `directive` 的 task 模板,就能造出自己的 `/指令`——纯文件、agent 可读、
  可 git,零应用内注册。溯源只是第一个实例。

## 组件设计

### 1. agent-run-core:`TaskDef.directive`

`plugins-src/agent-run-core/src/task.rs` 的 `TaskDef` 增加
`#[serde(default)] pub directive: Vec<String>`。不参与运行语义,仅供发现/展示。
两个 agent 插件(claude-agent / deepseek-agent)随 crate 自然获得,无行为变化。

### 2. 指令发现(idea-spark)

idea-spark 前端枚举 `<vault>/.notemd/agent-tasks/*/task.json`,取含非空 `directive`
的模板构建指令表 `{ name → taskId }`(全部名字都注册,首项用于展示)。

- 通道:优先复用现有 host RPC;若缺目录枚举能力,在宿主 `ui_rpc.rs` 增加最小的
  `host.vault.list`(列目录,vault 内路径校验与 read 同规)。实现期以现状为准。
- 打开委托界面时刷新一次即可,不做 watcher(YAGNI)。

### 3. idea-spark 输入面:指令解析与委托路由

- 文本以 `/` 开头 → 取首行首个空白前的 token 作指令名,查指令表;命中则 UI 显示指令
  chip(名称 + 模板 description),未命中不报错、按普通文字处理。
- 点「委托」:命中指令 → `agentRun({ task: <taskId>, prompt: 去掉指令 token 后的全文,
  notify: {...} })`;未命中 → 现有 idea-proof 流程完全不变。
- **输出路径由委托方指定**(沿 ai-read-ebook 既有模式):委托时在文本末尾追加一行
  `输出: traces/<YYYY-MM-DD>-<HHmmss>.md`(时间戳定名,标题放 frontmatter/H1,
  agent 不得改名)。这样 `notify.expect_file` / `open_path` 可确定性指向摘要 md,
  点通知即打开。

### 4. 编辑器右键「溯源」与预填通道

- `menu-model.ts` 增加 `item('trace', 'ctxmenu.trace', { needsSelection: true })`;
  rich / source 两个适配器分别取选区文本;i18n 四语言补 `ctxmenu.trace`。
- 动作:收集 `{ 选区文本, 源文档 vault 相对路径(或 vault 外绝对路径), 选区所在标题
  链(可得则带) }` → 打开 idea-spark 窗口并预填:

  ```
  /溯源 
  > <选区文本(引用块)>

  源文档: <路径>
  ```

  光标停在 `/溯源 ` 之后,用户可补范围说明(如「只查 YouTube 和 arxiv」)再点委托。
- 预填传递机制:实现期读现有插件窗口打开代码后取最小方案(窗口 URL query 或
  `.notemd/` 下一次性 pending 文件,用后即删)。宿主只负责「打开 + 传参」,不理解指令。

### 5. `trace-source` task 模板(idea-spark 播种)

沿用 idea-spark 现有 `seedTaskTemplate()` 机制(幂等、不覆盖用户改过的文件;注意
host.vault.write 无 chmod、precheck 不依赖可执行位;模板文本用数组拼行防 `${}` 与
反引号坑)。目录 `.notemd/agent-tasks/trace-source/`:

- **task.json**:`directive: ["溯源", "trace"]`,`okf_type: "traceReport"`,
  `timeout_seconds` 放宽(网络任务,建议 2700),`max_turns` 适当放宽。
- **CLAUDE.md(协议核心)**,要点:
  1. 输入即用户委托文本:引用块=待溯源文本,`源文档:` 行=反向链接目标,其余文字=
     用户的范围/关注点说明(如限定 YouTube/论文库/博客;未指定则三类都试)。
  2. 流程:提取可检索的断言与关键词 → WebSearch(按范围收窄)→ 候选出处逐个核验:
     博客/新闻用 WebFetch 取正文;论文优先 arxiv abs 页(WebFetch);YouTube 先探测
     `yt-dlp --version`,可用则 `yt-dlp --skip-download --write-auto-subs`(及
     `--write-subs`)取字幕并转纯文本,不可用则**降级**:给出视频链接 + 基于搜索
     摘要的描述,并在报告中如实声明「未取到字幕」。
  3. 产物写入(路径均相对 vault):
     - 摘要:写到委托文本 `输出:` 行指定的路径(`traces/<YYYY-MM-DD>-<HHmmss>.md`),
       **不得改名**;标题写在 frontmatter `title` 与正文 H1。
     - 材料全文:`traces/<YYYY-MM-DD>-<HHmmss>/<nn>-<来源slug>.md`,每份一个来源
       (字幕全文转写 / 博客正文 / 论文摘要+关键节选),frontmatter `type: Trace
       Material` + `sources[]` 单条指向原 URL。
  4. 摘要结构(报告即产品):frontmatter(`type: Trace Report`、`generated`、
     `sources[]` 列出全部核验过的出处)+ 正文:
     - 「缘起」:引用原选区文本(选区文本即稳定锚,项目已验证的做法)+ 指回源文档的
       链接(vault 内→相对 md 链接;vault 外→纯文本路径)。
     - 「结论」:最可能的原始出处(可信度分级:确认/高度疑似/未找到),每条断言旁
       标 URL(与 with_toolbelt 既有要求一致)。
     - 「摘要」:按用户关注点组织的有价值内容提炼,行文用界面语言。
     - 「继续阅读」:指向本次材料全文的**相对链接**列表。
  5. 红线:绝不修改 `traces/` 之外的任何文件;绝不写 `human:` 署名;找不到出处也要
     产出报告(声明未找到 + 已排查的候选)。
- **settings.json / settings.scoped.json**:allow `WebSearch`、`WebFetch`、
  `Bash(yt-dlp:*)`、写 `traces/**`;deny 其余 Bash 与 vault 外写。scoped 版本才是
  真约束(单靠 prompt 拦不住 grep 全 vault,项目原话)。
- **precheck.sh**:轻量——vault 可写即过;yt-dlp 缺失**不**拦(协议内降级)。

### 6. 产物与链接约定(落盘形状)

```
traces/
  2026-08-18-143012.md                     ← Trace Report(摘要,入口,委托方定名)
  2026-08-18-143012/
    01-karpathy-blog.md                    ← Trace Material(博客正文)
    02-youtube-lecture-subs.md             ← Trace Material(字幕转写)
```

- 摘要在 `traces/` 根、材料进同名子目录:时间戳定名避免重名,也避免 `report.md`
  之类固定名破坏「wikilink 按文件名解析」;摘要→材料用相对链接(用户点击继续阅读)。
- OKF:`src/lib/okf/concept.ts` 的 `CONCEPT_TYPE` 新增 `traceReport: 'Trace Report'`
  与 `traceMaterial: 'Trace Material'`;同步 `searchidx/src/origin.rs`
  `mapped_type_origin`(report→derived,material→source);跑 `pnpm gen:origin-types`
  更新共享 fixture。三件套缺一测试即红。
- agent 忘写 frontmatter 时,agent-run-core 的 okf 兜底按 `task.okf_type` 补
  `traceReport` stamp(现成机制)。

### 7. 通知

复用统一通知基建:成功 → 「溯源完成:<主题>」,点开摘要 md;失败 → 带 stderr 摘要。
经 `agentRun` 的 `notify` 参数即可,无新代码。

## 数据流(右键路径)

```
选中文本 → 右键「溯源」→ 宿主收集{选区,路径}→ 打开 idea-spark(预填)
→ 用户补范围 → 委托 → host.agent.run(task=trace-source, prompt=委托文本)
→ provider resolve(harness 选择器现成)→ headless 运行(WebSearch/WebFetch/yt-dlp)
→ 写 traces/ 摘要+材料 → RunRecord 完结 → 统一通知 → 点开摘要 → 相对链接读材料
                                                    ↘ 摘要内「缘起」链接回源文档
```

## 错误处理与降级

| 情形 | 行为 |
|---|---|
| yt-dlp 未安装 | 协议内降级:给链接+搜索描述,报告声明未取字幕;不失败 |
| 找不到出处 | 仍产出报告:声明未找到 + 已排查候选;通知按成功走 |
| 指令不存在(手打 `/xxx`) | 不报错,按普通奇思妙想文本处理 |
| agent 插件未装 | 沿用 `agent_unavailable:` 前缀识别,提示装 claude-agent |
| 源文档在 vault 外 | 摘要「缘起」记纯文本路径(不 mirror、不写 .note.md) |
| 运行超时 | 沿用「有输出就重新起算」的既有超时语义 |

## 安全

- `Read(vault/**)` + `WebFetch` 的 prompt-injection 外泄通道:与 idea-spark 同一权衡,
  **显式接受**(沿 2026-08-04 round1 followups 的既有决策)。
- 新增面:`Bash(yt-dlp:*)`。仅此一个命令前缀,scoped settings 限定;yt-dlp 本身会访问
  任意 URL——视为与 WebFetch 同级风险,接受。
- task id / 路径:沿用 `valid_task_id` 与 vault 路径校验;`host.vault.list`(若新增)
  与 read 同一套校验。

## 测试

- agent-run-core:`directive` 字段序列化/缺省的单测(Rust)。
- idea-spark:指令解析(命中/未命中/仅 `/`/多行)、指令表构建、委托路由的前端单测;
  strings.test.ts 覆盖新增文案(插件 i18n 审计规矩)。
- 宿主:menu-model 新项的 gate(无选区禁用)单测;`host.vault.list`(若新增)的路径
  校验单测。
- OKF:三件套一致性由现有 `concept-origin-sync.test.ts` + 重新生成的 fixture 把守;
  `pnpm okf:lint` 过 traces 样例。
- 模板协议:照「环境探测发版前真机跑」的教训,发版前用真实 harness 跑一次端到端
  (喂一段已知出处的文本,验摘要/材料/链接形状)。
- GUI(右键项、预填、指令 chip):dev 实机验证,用户手测(不做 UI 自动化)。

## 非目标(YAGNI)

- 不实现 `[[file#^id]]` 块引用;锚定沿用「选区文本即身份」。
- 不写源 md、不写源文档 .note.md、无 ✦ 卡片回流(用户已选轻方案;留作后续扩展位)。
- 不做宿主/插件侧抓取代码、不打包 yt-dlp。
- 不做批注 sweep 式的批量溯源协议(指令抽象天然留了扩展位)。
- 不做指令的自动补全/模糊搜索,只做前缀精确匹配 + chip 提示。
