# 人脑 × AI 协作插件生态报告

> 版本：1.0
>
> 调研日期：2026-09-01
> 范围：note.md 插件分类、单功能插件机会、Obsidian / Roam / Notion 市场对照

## 1. 执行摘要

本报告以认知科学中的外部记忆、阅读理解、创造性认知、执行控制、元认知和外部表征为理论约束，将 note.md 插件市场组织为六个面向用户的分类：

1. **记录**：留住重要的信息。
2. **阅读**：读懂更多。
3. **灵感**：发现新的可能。
4. **推进**：把想法变成下一步。
5. **回顾**：从经历中持续改进。
6. **创作**：让想法成为作品。

AI 不作为与六类并列的一级分类，而是贯穿其中的协作方式。市场顶部可以设置“与 AI 一起完成”精选集合，插件卡片使用“AI 阅读、AI 启发、AI 推理、AI 整理、AI 执行、AI 创作”等能力标签。

本轮共形成 **192 个单一功能插件候选**，每类 32 个。其中，市场已经充分验证快速捕捉、语音/OCR、阅读高亮、相关笔记、间隔复习、任务/日历、会议行动项、格式转换和发布等需求；相对稀缺的是主张与证据辨识、隐含前提、来源多样性、预测校准、目标漂移、价值一致、二阶影响和 AI 结果审计。

note.md 的差异化不应是复制更多通用聊天框，而应是建立一组小而精确的“认知动作”：用户在原工作位置触发一次操作，AI 完成受约束的识别、解释、生成或比较，人负责选择、修正、承诺与发布。

## 2. 分类原则

### 2.1 一级分类按人的认知结果划分

分类判定问题是：**完成插件主要流程后，用户首先获得了什么认知结果？**

不能把工作记忆、情景记忆、语义记忆和程序记忆直接平铺为市场分类。它们不处于同一理论层级，也无法自然容纳导出、编辑和自动化。工作记忆主要处理当前任务中的短暂保存与操作；例如“下一步（Next）”保存的是未来意图，专业上更接近前瞻记忆和认知卸载，因此归入“推进”。

### 2.2 AI 是第二维能力

| AI 能力 | AI 的作用 | 人的责任 |
|---|---|---|
| AI 阅读 | 提取、解释、总结 | 判断是否正确、可信 |
| AI 启发 | 生成候选、扩大搜索空间 | 选择值得发展的方向 |
| AI 推理 | 比较、论证、找矛盾 | 承担最终判断 |
| AI 整理 | 分类、结构化、关联 | 确认归属和语义 |
| AI 执行 | 调用工具、修改状态 | 授权、验收和撤销 |
| AI 创作 | 改写、生成、呈现 | 保留作者身份并发布 |

### 2.3 单功能插件约束

每个插件应具备：

- 一个清晰触发入口；
- 一次主要认知转换；
- 一个可验收结果；
- 一个明确的人类确认点；
- 高影响操作先预览，再执行；
- 不以模型、Prompt 或底层技术命名；
- 不退化为无边界的通用聊天窗口。

## 3. 市场扫描

### 3.1 数据源与范围

- Obsidian：从官方 `community-plugins.json` 读取到 7,145 条社区插件记录，并参考官方 `community-plugin-stats.json`；代表能力包括 QuickAdd、Tasks、Spaced Repetition、PDF++、Zotero Integration、Smart Connections、Whisper、Excalidraw 和 Pandoc。
- Roam：从官方 Roam Depot 仓库读取到 206 个扩展元数据；代表能力包括 Quick Capture、Hypothesis、Otter、Related Notes、Better Tasks、Nautilus、SmartBlocks、Presentation 和 Live AI。
- Notion：它不是本地插件市场，重点分析 Connections、AI Connectors、Custom Agents 和 AI Skills；代表能力包括 Web Clipper、AI Meeting Notes、Meeting Action Extractor、Weekly Review Assistant 和 Agent Performance Reviewer。

数量只用于理解生态规模，不用于直接比较平台优劣。关键词匹配用于发现候选，最终标记依据官方简介和代表性详情人工归并。

### 3.2 市场共同信号

1. **捕捉入口必须足够近**：快捷键、选中文字、分享菜单、语音和浏览器剪藏已经被反复验证。
2. **上下文比聊天框更重要**：选中文本、当前块、当前文档、会议或任务数据库，都是比空白 Prompt 更自然的入口。
3. **相关内容发现需求强烈**：语义搜索、相关笔记、随机旧笔记和引用图谱在 Obsidian、Roam 都有清晰先例。
4. **任务市场拥挤**：任务、日历、番茄钟、提醒和通用 Agent 已有大量产品，新增产品必须收窄到一个决策点。
5. **可追溯输出更可信**：来源链接、时间戳、引用跳转和执行预览是 AI 协作的重要信任机制。
6. **审批是 Agent 的产品界面**：Notion 的会议行动项 Agent 会先提出建议，等待批准后才创建任务，这比默认自动执行更适合知识工作。
7. **高阶校准仍稀缺**：多数产品记录“发生了什么”，较少帮助用户检查“为什么判断错了”。

### 3.3 标记说明

| 标记 | 含义 |
|---|---|
| `●` | 官方市场中已有直接、成熟或明确的先例 |
| `◐` | 已有相邻能力，但通常被包含在大型插件或通用 Agent 中 |
| `◇` | 本次官方目录扫描中未发现清晰的单功能先例 |
| `＋` | 根据本轮市场扫描新增的候选 |
| `Ob / Ro / No` | Obsidian / Roam / Notion |

`◇` 表示相对空白，不代表全球绝对不存在相似产品。

## 4. 插件机会地图

### 4.1 记录：留住重要的信息

| # | 插件 | 单一功能 | 市场标记 |
|---:|---|---|---|
| 1 | 快速记录（Quick Note） | 将一句输入整理成带日期的原子笔记 | `● Ob/Ro` |
| 2 | 语音速记（Voice Note） | 将一段语音转成清晰文字 | `● Ob` |
| 3 | 截图笔记（Screenshot Note） | 识别截图文字并连同原图保存 | `● Ob/Ro` |
| 4 | 引文收藏（Quote Keeper） | 保存选中文字并补齐来源 | `● Ob/Ro` |
| 5 | 链接留存（Link Keeper） | 保存网址标题、来源和简短说明 | `● Ob/No` |
| 6 | 稍后继续（Resume Marker） | 保存当前阅读位置和暂停原因 | `◐ Ob` |
| 7 | 情境快照（Context Snapshot） | 切换任务前保存当前工作情境 | `◐ Ob` |
| 8 | 承诺捕捉（Promise Capture） | 找出“我会做”的承诺，确认后记录 | `● No` |
| 9 | 问题收件箱（Question Inbox） | 提取尚未回答的问题 | `◇` |
| 10 | 术语收集（Term Collector） | 将陌生术语加入个人词汇表 | `● Ob/Ro` |
| 11 | 人物卡片（People Note） | 从对话生成一张人物背景卡片 | `◐ Ob/Ro` |
| 12 | 事实摘录（Fact Capture） | 只提取明确陈述的事实 | `◐ No` |
| 13 | 证据摘录（Evidence Clip） | 将选中证据关联到已有主张 | `◐ Ob` |
| 14 | 图片笔记（Photo Note） | 为一张图片生成可搜索描述 | `● Ob` |
| 15 | 白板转写（Board Capture） | 将白板照片转换为结构化文字 | `◐ Ob/Ro` |
| 16 | 邮件留存（Mail Note） | 将一封邮件保存为带来源的笔记 | `● No` |
| 17 | 对话摘录（Chat Clip） | 保存选中的对话及其上下文 | `● Ro/No` |
| 18 | 时间标记（Event Marker） | 将一句事件保存到个人时间线 | `● Ob/Ro/No` |
| 19 | 地点记忆（Place Memory） | 记录当前地点和现场说明 | `● Ro` |
| 20 | 高亮收藏（Highlight Keeper） | 将阅读高亮连同上下文保存 | `● Ob/Ro` |
| 21 | 数据点（Data Point） | 提取一个数值及单位、时间和来源 | `◐ No` |
| 22 | 错误现场（Error Snapshot） | 保存报错、操作步骤和环境 | `◇` |
| 23 | 灵感碎片（Thought Fragment） | 将零散念头整理成独立卡片 | `● Ob/Ro` |
| 24 | 来源印记（Source Stamp） | 为当前笔记补充来源和访问时间 | `● Ob/Ro` |
| 25 | 记录同意（Consent Check） | 录音前确认并保存参与者同意状态 | `＋◐ No` |
| 26 | 说话人（Speaker Labels） | 为一份转录稿标注说话人 | `＋● No` |
| 27 | 播客片段（Podcast Clip） | 保存当前音频片段、转录和节目来源 | `＋● Ob` |
| 28 | 视频转录（Video Transcript） | 将视频链接转换为带时间戳的文本 | `＋● Ob/Ro` |
| 29 | 阅读同步（Reading Sync） | 从阅读服务同步高亮 | `＋● Ob/Ro` |
| 30 | 移动收件箱（Mobile Inbox） | 将手机分享内容送入统一收件箱 | `＋● Ob/Ro` |
| 31 | 身体记录（Body Log） | 将穿戴设备当日摘要写入日记 | `＋● Ro` |
| 32 | 日程背景（Calendar Context） | 为会议笔记补充日历参与者和议题 | `＋● Ob/Ro/No` |

### 4.2 阅读：读懂更多

| # | 插件 | 单一功能 | 市场标记 |
|---:|---|---|---|
| 1 | 选中解释（Explain Selection） | 解释当前选中的内容 | `● Ob/No` |
| 2 | 语境释义（Define in Context） | 解释一个词在当前文章中的含义 | `● Ob/Ro` |
| 3 | 简明表达（Plain Language） | 将复杂段落改成容易理解的版本 | `● No` |
| 4 | 前置知识（Prerequisite Guide） | 列出理解内容所缺少的基础概念 | `◐ Ob` |
| 5 | 段落要点（Paragraph Point） | 用一句话指出当前段落的作用 | `● No` |
| 6 | 章节摘要（Chapter Brief） | 只总结当前章节 | `● Ob/No` |
| 7 | 论证骨架（Argument Outline） | 提取结论、理由和证据关系 | `● Ob` |
| 8 | 主张识别（Claim Finder） | 标出可被验证或反驳的主张 | `◐ Ob` |
| 9 | 证据识别（Evidence Finder） | 区分证据、解释、观点和修辞 | `◐ Ob` |
| 10 | 来源检查（Source Check） | 显示来源作者、时间和可验证依据 | `◐ Ob/Ro` |
| 11 | 隐含前提（Hidden Premise） | 指出论证依赖但未写出的前提 | `◇` |
| 12 | 反例寻找（Counterexample） | 为概括性主张寻找可能反例 | `◐ Ob` |
| 13 | 矛盾发现（Contradiction Finder） | 指出文档中互相冲突的陈述 | `● Ob` |
| 14 | 来源比较（Compare Sources） | 比较两份材料对同一问题的说法 | `◐ Ro/No` |
| 15 | 偏差透镜（Bias Lens） | 提示表述可能采用的观察立场 | `◐ Ob` |
| 16 | 图表解释（Figure Explainer） | 解释一张图表达的主要关系 | `◐ Ob/No` |
| 17 | 表格阅读（Table Reader） | 指出表格中最值得注意的变化 | `◐ Ob/No` |
| 18 | 公式分解（Formula Steps） | 逐项解释公式变量和运算 | `◐ Ob` |
| 19 | 代码解释（Explain Code） | 说明代码的输入、处理和输出 | `◐ Ob/No` |
| 20 | 语境翻译（Translate in Context） | 结合上下文翻译选中文字 | `● Ob/Ro` |
| 21 | 时间线提取（Timeline Builder） | 从材料中提取事件顺序 | `◐ Ro/No` |
| 22 | 实体关系（Entity Map） | 显示人物、组织和概念关系 | `● Ob/Ro` |
| 23 | 阅读问题（Reading Questions） | 根据章节生成理解检查题 | `● Ob` |
| 24 | 回忆检查（Recall Check） | 隐藏原文后检查是否真正记住 | `● Ob/Ro` |
| 25 | 相关笔记（Related Notes） | 推荐最相关的三条旧笔记 | `＋● Ob/Ro` |
| 26 | 引用路径（Citation Trail） | 显示主张沿参考文献的追溯路径 | `＋◐ Ob` |
| 27 | 阅读队列（Reading Queue） | 按长度、期限和进度安排下一篇 | `＋● Ob` |
| 28 | 朗读（Read Aloud） | 朗读当前选择或章节 | `＋● Ob` |
| 29 | 深度研究（Deep Research） | 围绕一个问题生成带来源的报告 | `＋● Ro/No` |
| 30 | 来源多样性（Source Diversity） | 检查材料是否过度依赖单一来源 | `＋◇` |
| 31 | 学习卡片（Study Card） | 将一个概念转换成一张回忆卡片 | `＋● Ob/Ro` |
| 32 | 论文邻居（Paper Neighbors） | 推荐少量直接相关研究 | `＋● Ro` |

### 4.3 灵感：发现新的可能

| # | 插件 | 单一功能 | 市场标记 |
|---:|---|---|---|
| 1 | 问题火花（Question Spark） | 围绕主题生成值得追问的问题 | `◐ Ob/Ro` |
| 2 | 类比（Analogy） | 为抽象概念寻找恰当类比 | `◇` |
| 3 | 隐喻（Metaphor） | 为当前想法生成表达性隐喻 | `◐ Ob/Ro` |
| 4 | 强制连接（Forced Connection） | 连接两个看似无关的概念 | `◇` |
| 5 | 约束卡片（Constraint Card） | 加入一个有助于创造的限制条件 | `◇` |
| 6 | 换个角度（New Perspective） | 从另一角色视角重新观察问题 | `◐ Ob/No` |
| 7 | 反转假设（Assumption Flip） | 反转默认假设并观察后果 | `◇` |
| 8 | 如果这样（What If） | 改变一个条件并生成可能结果 | `◐ Ob` |
| 9 | 邻近可能（Adjacent Possible） | 找出当前想法最自然的下一变化 | `◇` |
| 10 | 空白发现（Gap Finder） | 指出方案尚未覆盖的需求 | `● No` |
| 11 | 假设生成（Hypothesis Maker） | 将观察转换成可验证假设 | `◐ Ro` |
| 12 | 示例生成（Example Maker） | 为抽象概念生成具体示例 | `◐ No` |
| 13 | 使用场景（Use Case） | 为一个想法寻找新使用情境 | `◐ No` |
| 14 | 受众视角（Audience Lens） | 从指定受众角度提出方向 | `◐ No` |
| 15 | 命名灵感（Name Spark） | 为产品或概念生成短名称 | `◐ Ob/No` |
| 16 | 标题灵感（Title Spark） | 为内容生成不同角度的标题 | `◐ Ob/No` |
| 17 | 开场灵感（Opening Spark） | 为文章生成一种开场方式 | `◐ Ob/No` |
| 18 | 画面灵感（Visual Spark） | 将概念转换成视觉场景描述 | `◐ Ob/Ro` |
| 19 | 概念连接（Concept Link） | 发现当前笔记与旧概念的联系 | `● Ob/Ro` |
| 20 | 变化生成（Variation Maker） | 围绕方案生成有限数量的变体 | `◐ No` |
| 21 | 反向头脑风暴（Reverse Brainstorm） | 从如何失败反推机会 | `◇` |
| 22 | 最小实验（Tiny Experiment） | 为想法生成最小验证实验 | `◐ No` |
| 23 | 意外后果（Second Effect） | 推演一个选择的二阶影响 | `◇` |
| 24 | 随机刺激（Random Stimulus） | 引入远距离概念打破固定思路 | `● Ob/Ro` |
| 25 | 被遗忘的笔记（Forgotten Note） | 找出长期未打开但相关的旧笔记 | `＋● Ob/Ro` |
| 26 | 思考路径（Thought Path） | 建议一条三步探索路线 | `＋◐` |
| 27 | 对立观点（Opposing View） | 生成一个最有力的反方观点 | `＋◐ Ob/Ro` |
| 28 | 想法谱系（Idea Lineage） | 显示想法从旧笔记演化的路径 | `＋◇` |
| 29 | 微弱信号（Weak Signal） | 找出刚开始重复出现的新主题 | `＋◇` |
| 30 | 未用素材（Unused Material） | 找出尚未被作品采用的相关材料 | `＋◇` |
| 31 | 想法聚类（Idea Clusters） | 将碎片按潜在方向分簇 | `＋◐ Ob/Ro` |
| 32 | 思维算法（Thinking Move） | 一次只应用一种思考方法 | `＋● Ro` |

### 4.4 推进：把想法变成下一步

| # | 插件 | 单一功能 | 市场标记 |
|---:|---|---|---|
| 1 | 最小下一步（Next Move） | 将目标转换成一个立即动作 | `◐ Ob/No` |
| 2 | 任务拆分（Task Split） | 将任务拆成可独立完成的小步骤 | `● No` |
| 3 | 优先一个（Pick One） | 根据约束只推荐一件先做的事 | `● No` |
| 4 | 依赖关系（Dependency Map） | 指出必须先完成的步骤 | `◐ No` |
| 5 | 阻塞发现（Blocker Finder） | 识别任务不能继续的真正原因 | `● No` |
| 6 | 完成标准（Done Criteria） | 为任务写出清晰完成定义 | `◐ No` |
| 7 | 时间估计（Time Estimate） | 基于任务组成给出时间区间 | `◇` |
| 8 | 时间盒（Timebox） | 将任务压缩成指定时长范围 | `● Ob/Ro` |
| 9 | 开始提示（Start Cue） | 生成开始任务的第一个动作 | `◐ Ob` |
| 10 | 专注冲刺（Focus Sprint） | 启动固定时长的专注周期 | `● Ob/Ro` |
| 11 | 截止倒推（Backplan） | 从截止时间倒推出关键节点 | `◐ No` |
| 12 | 日程落位（Find a Slot） | 在空闲时间中寻找任务时段 | `● No` |
| 13 | 等待追踪（Waiting Tracker） | 记录正在等待谁和什么 | `● Ob/Ro` |
| 14 | 跟进草稿（Follow-up Draft） | 为等待事项生成跟进消息 | `● No` |
| 15 | 委托说明（Delegation Brief） | 将任务整理成可交付说明 | `◐ No` |
| 16 | 责任确认（Owner Check） | 检查行动项是否有唯一负责人 | `● No` |
| 17 | 事前验尸（Pre-mortem） | 列出任务最可能失败的原因 | `◇` |
| 18 | 决策期限（Decision Deadline） | 为未决问题建议决策时间 | `◇` |
| 19 | 停滞提醒（Stuck Nudge） | 为长期无进展事项提出解阻动作 | `◐ No` |
| 20 | 智能体选择（Agent Picker） | 根据任务推荐已安装智能体 | `● Ob/Ro` |
| 21 | 执行预览（Action Preview） | 操作前显示将修改的内容 | `● No` |
| 22 | 完成证据（Completion Check） | 检查任务是否具备完成证据 | `◐ No` |
| 23 | 交接检查（Handoff Check） | 检查交付物是否足够接手 | `◐ Ro/No` |
| 24 | 归位建议（File This） | 建议唯一存放位置，确认后移动 | `● Ob/No` |
| 25 | 会议行动（Meeting Actions） | 从会议提取行动项，逐条确认创建 | `＋● No` |
| 26 | 任务分流（Task Triage） | 为新任务建议唯一项目和负责人 | `＋● No` |
| 27 | 审批收件箱（Approval Inbox） | 集中展示待确认的 AI 操作 | `＋◐ No` |
| 28 | 自动化草稿（Workflow Draft） | 从自然语言生成流程预览 | `＋● No` |
| 29 | 日程冲突（Calendar Conflict） | 指出任务计划与日程冲突 | `＋◐ No` |
| 30 | 负荷平衡（Workload Balance） | 提示任务是否集中于某人或某天 | `＋● No` |
| 31 | 智能体记录（Agent Run Log） | 显示 Agent 读取、修改和执行内容 | `＋● No` |
| 32 | 过期任务（Stale Tasks） | 找出长期未更新且无下一步的任务 | `＋● Ob/Ro` |

### 4.5 回顾：从经历中持续改进

| # | 插件 | 单一功能 | 市场标记 |
|---:|---|---|---|
| 1 | 今日回顾（Daily Review） | 从当天记录生成简短回顾 | `● Ro/No` |
| 2 | 周度回顾（Weekly Review） | 汇总本周完成、停滞和待处理项 | `● Ob/No` |
| 3 | 决策结果（Decision Outcome） | 将过去决策与实际结果对照 | `◇` |
| 4 | 预测校准（Prediction Score） | 比较预测概率与真实结果 | `◇` |
| 5 | 假设审计（Assumption Audit） | 检查项目关键假设是否仍成立 | `◇` |
| 6 | 目标漂移（Goal Drift） | 指出当前行动与原始目标的偏离 | `◐ Ob` |
| 7 | 模式镜子（Pattern Mirror） | 显示近期记录中反复出现的主题 | `● No` |
| 8 | 重复阻塞（Recurring Blocker） | 发现多次导致停滞的同类原因 | `◐ No` |
| 9 | 估时校准（Estimate Review） | 比较预计耗时和实际耗时 | `◇` |
| 10 | 中断回顾（Interruption Review） | 总结常见的工作中断来源 | `◐ Ob/Ro` |
| 11 | 精力模式（Energy Pattern） | 从记录中寻找精力变化规律 | `● Ob/Ro` |
| 12 | 习惯触发（Habit Trigger） | 寻找行为最常见的触发情境 | `◐ Ob/Ro` |
| 13 | 学习摘要（Learning Digest） | 汇总一段时间内的新概念 | `◐ Ob/No` |
| 14 | 错误循环（Mistake Loop） | 识别重复出现的错误类型 | `◇` |
| 15 | 成果证据（Wins） | 提取已经完成的具体成果 | `● No` |
| 16 | 反馈主题（Feedback Themes） | 归纳多份反馈中的重复主题 | `● No` |
| 17 | 会议成效（Meeting Outcome） | 检查会议是否产生明确结果 | `● No` |
| 18 | 项目复盘（Project Retro） | 按目标、结果和偏差生成复盘 | `● No` |
| 19 | 阅读保持（Reading Retention） | 检查读过内容还记住多少 | `● Ob` |
| 20 | 未解问题（Open Questions） | 汇总长期没有回答的问题 | `◐ Ob` |
| 21 | 承诺兑现（Promise Review） | 检查过去承诺是否完成 | `◐ No` |
| 22 | 关系跟进（Relationship Review） | 提示长期未联系的重要人物 | `● Ro` |
| 23 | 价值一致（Values Check） | 比较时间投入与个人价值排序 | `◇` |
| 24 | 盲点提示（Blind Spot） | 指出长期缺失但重要的视角 | `◇` |
| 25 | 去年今日（On This Day） | 展示往年同一天的记录 | `＋● Ro` |
| 26 | 注意力热图（Attention Map） | 显示时间实际流向哪些主题 | `＋● Ob/Ro` |
| 27 | 智能体回顾（Agent Review） | 评估 Agent 结果、失败和人工修正 | `＋● No` |
| 28 | 自动化审计（Automation Audit） | 检查自动流程近期异常结果 | `＋◐ No` |
| 29 | 知识衰减（Knowledge Decay） | 找出长期未复习的知识 | `＋● Ob` |
| 30 | 收件箱债务（Capture Debt） | 找出长期未处理的记录碎片 | `＋◇` |
| 31 | 来源平衡（Source Balance） | 回顾阅读是否集中于单一来源 | `＋◇` |
| 32 | 会议兑现（Meeting Follow-through） | 检查会议行动项完成比例 | `＋● No` |

### 4.6 创作：让想法成为作品

| # | 插件 | 单一功能 | 市场标记 |
|---:|---|---|---|
| 1 | 大纲（Outline） | 从素材生成层级清晰的大纲 | `● Ob/No` |
| 2 | 标题（Headline） | 为当前内容生成准确标题 | `◐ No` |
| 3 | 摘要（Summary） | 将内容压缩成指定长度 | `● Ob/No` |
| 4 | 清晰改写（Make Clear） | 改写含糊句子，不改变原意 | `◐ No` |
| 5 | 精简（Make Shorter） | 删除重复和不必要表达 | `◐ No` |
| 6 | 扩写（Expand） | 将一个要点扩展成完整段落 | `◐ No` |
| 7 | 语气（Tone） | 只调整语气，不改变事实 | `◐ No` |
| 8 | 受众适配（For This Audience） | 为指定读者调整表达方式 | `◐ No` |
| 9 | 补充示例（Add an Example） | 为抽象段落增加具体例子 | `◐ No` |
| 10 | 过渡句（Transition） | 连接两个相邻但跳跃的段落 | `◐ No` |
| 11 | 结尾（Conclusion） | 根据正文生成收束段落 | `◐ No` |
| 12 | 摘要页（Abstract） | 为长文生成正式摘要 | `◐ No` |
| 13 | 邮件草稿（Email Draft） | 将选中内容转换成邮件 | `● No` |
| 14 | 会议简报（Meeting Brief） | 将资料整理为会前一页简报 | `◐ No` |
| 15 | 幻灯大纲（Slide Outline） | 将文章转换为逐页演示结构 | `● Ob/Ro` |
| 16 | 表格（Make a Table） | 将同类信息转换成比较表 | `● Ob/Ro` |
| 17 | 图示（Make a Diagram） | 将关系描述转换成结构图 | `● Ob` |
| 18 | 视觉简报（Visual Brief） | 将想法转换成视觉创作说明 | `◐ Ob/No` |
| 19 | 替代文本（Alt Text） | 为图片生成无障碍描述 | `◐ No` |
| 20 | 翻译（Translate） | 转换语言并保留格式 | `● Ob/Ro` |
| 21 | 引用格式（Format Citation） | 将来源转换成指定引用格式 | `● Ob/Ro` |
| 22 | 术语表（Glossary） | 从文档生成关键术语附录 | `◐ Ob/No` |
| 23 | 导出 PDF（Export to PDF） | 将当前文档生成可分享 PDF | `● Ob/Ro` |
| 24 | 发布说明（Release Notes） | 从变更记录生成用户版说明 | `◐ No` |
| 25 | 分享卡片（Share Card） | 将一个块生成适合分享的图片 | `＋● Ro` |
| 26 | 幻灯片（Make Slides） | 将当前大纲生成可预览演示文稿 | `＋● Ob/Ro` |
| 27 | 新闻通讯（Newsletter Draft） | 将文章转换成邮件通讯稿 | `＋● No` |
| 28 | 社交长帖（Social Thread） | 将文章转换成单一平台连续帖子 | `＋● No` |
| 29 | 溯源综述（Sourced Synthesis） | 生成每个结论可跳回原文的综述 | `＋◐ Ob` |
| 30 | 我的语气（Voice Guide） | 从确认样本提取写作风格规则 | `＋● No` |
| 31 | 格式清理（Format Cleanup） | 只修复 Markdown 结构与格式 | `＋● Ob` |
| 32 | 带引用导出（Export with Sources） | 导出时保留引用和来源注释 | `＋● Ro` |

## 5. 推荐路线图

### 5.1 P0：差异化认知工具

优先选择市场相对空白、认知价值明确、能保持单一职责的方向：

1. **隐含前提（Hidden Premise）**：补足论证中未写出的前提。
2. **证据识别（Evidence Finder）**：区分事实证据、解释、意见和修辞。
3. **来源多样性（Source Diversity）**：发现材料来源的结构性偏斜。
4. **反转假设（Assumption Flip）**：打破默认前提形成的新方案空间。
5. **意外后果（Second Effect）**：推演二阶影响。
6. **事前验尸（Pre-mortem）**：在执行前暴露失败路径。
7. **完成证据（Completion Check）**：用可观察证据判断任务是否真正完成。
8. **决策结果（Decision Outcome）**：连接历史决策与实际结果。
9. **预测校准（Prediction Score）**：长期校准概率判断。
10. **目标漂移（Goal Drift）**：发现执行与原始目标的偏离。
11. **价值一致（Values Check）**：比较实际投入和价值排序。
12. **智能体回顾（Agent Review）**：评估 AI 输出和人工修正模式。

### 5.2 P1：需求已验证，以体验取胜

- 稍后继续（Resume Marker）
- 承诺捕捉（Promise Capture）
- 选中解释（Explain Selection）
- 相关笔记（Related Notes）
- 最小下一步（Next Move）
- 会议行动（Meeting Actions）
- 审批收件箱（Approval Inbox）
- 清晰改写（Make Clear）
- 溯源综述（Sourced Synthesis）

这些方向已有市场验证，note.md 的竞争点应是更少设置、更近的触发入口、更好的来源呈现和更明确的确认边界。

### 5.3 暂不优先

- 通用 AI 聊天；
- 全能知识库 Copilot；
- 普通任务管理器；
- 普通番茄钟；
- 普通网页剪藏；
- 普通 PDF 导出；
- 普通模板系统；
- 不限定输出的“帮我写”；
- 同时承诺摘要、翻译、搜索、写作和执行的超级插件。

这些方向并非没有价值，而是市场已拥挤，且容易破坏单一功能原则。

## 6. 市场呈现建议

### 6.1 一级分类

市场固定使用：

> 记录 · 阅读 · 灵感 · 推进 · 回顾 · 创作

每个插件只有一个一级分类。分类依据主要认知结果，不依据 AI、文件格式或实现技术。

### 6.2 AI 精选集合

市场顶部设置：

> **与 AI 一起完成**
> 从理解到执行，选择适合你的智能搭档。

精选集合不是第七个分类。Agent 和 AI 微工具仍保留各自的一级分类。

### 6.3 卡片信息

建议每张卡片只展示：

- 本地化产品名（English Name）；
- 一句单功能价值说明；
- 一个主分类；
- 最多两个 AI 能力标签；
- 已安装、可更新或需要授权状态；
- 高影响插件的“先预览”提示。

## 7. 产品判断

三个市场已经证明用户愿意安装大量插件来降低捕捉、检索、任务和表达成本，但也暴露了两个问题：一是大型插件功能持续膨胀，二是通用 AI 助手很难让用户预期一次操作会发生什么。

note.md 更适合采取相反策略：

1. 用认知任务组织市场，而不是按技术组织；
2. 用微插件把 AI 能力变成可预测动作；
3. 用来源、差异预览和审批建立信任；
4. 让用户组合自己的认知工作台；
5. 优先投资判断、校准和反思，而不是重复已有的聊天与任务市场。

最终产品定位可以概括为：

> **不是替人思考，而是让人与 AI 在每一个认知动作上更好地协作。**

## 8. 主要来源

### 认知科学

- [Working Memory: Theories, Models, and Controversies — Baddeley](https://doi.org/10.1146/annurev-psych-120710-100422)
- [Strategic Offloading of Delayed Intentions into the External Environment — Gilbert](https://pmc.ncbi.nlm.nih.gov/articles/PMC4448673/)
- [Cognitive Offloading — Risko & Gilbert](https://doi.org/10.1016/j.tics.2016.07.002)
- [The Role of Knowledge in Discourse Comprehension — Kintsch](https://doi.org/10.1037/0033-295X.95.2.163)
- [Metacognition and Cognitive Monitoring — Flavell](https://doi.org/10.1037/0003-066X.34.10.906)
- [Source Monitoring — Johnson, Hashtroudi & Lindsay](https://doi.org/10.1037/0033-2909.114.1.3)
- [Creative Cognition / Geneplore Model — Ward, Smith & Finke](https://ecologylab.net/research/publications/WardSmithFinke.pdf)
- [External Cognition — Scaife & Rogers](https://doi.org/10.1006/ijhc.1996.0048)

### 插件市场

- [Obsidian Community Plugins 官方仓库](https://github.com/obsidianmd/obsidian-releases)
- [Obsidian 社区插件目录](https://github.com/obsidianmd/obsidian-releases/blob/master/community-plugins.json)
- [Obsidian 社区插件下载统计](https://github.com/obsidianmd/obsidian-releases/blob/master/community-plugin-stats.json)
- [Roam Depot 官方仓库](https://github.com/Roam-Research/roam-depot)
- [Notion Connections](https://www.notion.com/connections)
- [Notion AI Connectors](https://www.notion.com/help/notion-ai-connectors)
- [Notion Web Clipper](https://www.notion.com/help/web-clipper)
- [Notion AI Meeting Notes](https://www.notion.com/help/ai-meeting-notes)
- [Notion AI Skills](https://www.notion.com/templates/collections/work-smarter-with-ai-skills)
- [Notion Meeting Action Extractor](https://www.notion.com/custom-agent-templates/meeting-action-extractor-every)
- [Notion Weekly Review Assistant](https://www.notion.com/custom-agent-templates/weekly-review)
- [Notion Agent Performance Reviewer](https://www.notion.com/custom-agent-templates/agent-performance-reviewer)
