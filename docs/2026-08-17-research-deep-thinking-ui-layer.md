# 深度研究原始结果：harness 时代的深度思考 UI 层——欧美现状（2026）

> 这是"思考 UI 层"论题的深度研究产出存档，供撰写产品论纲/外宣素材时溯源使用。
>
> **研究问题**：客户反馈"harness 一句话召回很强，但需要深度思考的场景，chat 交互不够"。欧美互联网（话语圈、产品圈、研究圈、协议圈）如何看待和解决"chat 之上的深度思考界面"问题？harness 明确不做的 UI 层，谁在做、做成了什么形态、还空着什么？对 note.md 意味着什么？
>
> **方法与统计**：前导 5 轮搜索 + 三路并行子研究（话语层 / 产品层 / 研究与协议层），合计约 34 轮搜索、40+ 次页面抓取，关键文本均全文核验。  
> **核验标注**：✅ = 已抓取原文核实；◐ = 仅搜索摘要，引用前建议二次核实。  
> **快照时间**：2026 年 8 月 17 日。

---

## 0. 综述：六条结论

1. **"chat 不够思考"在欧美已从少数派檄文变成三层共识**——认知层（chat 伤害思考）、协作层（chat 无法承载多 agent 并行）、工程层（文本流无法承载 agent 输出）。三层共用同一句式："chat 是 2022 年最快能 ship 的历史偶然，不是终局范式。"
2. **agent 监督面正在收敛为两种形态**：board/看板（Notion 3.6、GitHub Agent HQ、Vibe Kanban）与 canvas+评论（tldraw）。但两者都是**吞吐量导向**（派单→执行→diff→验收），没有一家做"思考/沉淀"层。
3. **没有 agent 的纯画布已死**（Kosmik 关停、Muse 停滞、Kuse 转型）；**有 agent 没沉淀的监工 UI 正被平台吸收**（Vibe Kanban sunset、Terragon 关停、GitHub/Notion 收编）。幸存与增长的，都是"agent × 持久空间"配对者（Heptabase、Fabric、tldraw）。
4. **harness 正典明确把 UI 排除在自身边界之外**（OpenAI、HumanLayer、Avi Chawla），只谈环境与验证、不谈人读什么、在哪判断。"harness 之上的判断/沉淀层"是西方话语**命名了边界但没人占据的空位**。
5. **没有任何主流产品同时做到三件事**：agent 监工面 + 持久思考面 + 文件为唯一事实源。Cowork 缺后两者，Notion 缺文件层，Obsidian 缺产品化 agent 层。也**没有一家把"批注/判断留存"当作画布的第一性用途**。
6. **"办公入口代际论"（office 文件 → 浏览器/搜索 → harness）在西方只有拼图、没有完整论述**——"agent-first world""browser as agent runtime""the end of apps"各说一角。这条论述本身是 note.md 的差异化空间。

---

## 1. 话语层："chat 不够思考"如何成为共识

### 1.1 思想源头（2023）

**Amelia Wattenberger《Why Chatbots Are Not the Future》**（2023-05）✅ 是本轮话语公认的源头，2025–26 年以"被追认的预言"形式反复被引。四大批判：文本框 affordance 不明；context 全靠用户手动打进 prompt；回应彼此孤立、迭代靠滚动对比；人机各占一半的"No man's land"把人变成 machine operator 而非 craftsperson。

> "Good tools make it clear how they should be used. And more importantly, how they should *not* be used."  
> "I want to see more tools and fewer operated machines."

### 1.2 2025–26 批判文批次（按论点分组）

**协作层旗舰：Angelina Yang《Canvas vs. Chat — Why Spatial Interfaces Win for AI Collaboration》**（Medium，2026-06）✅。chat 的线性、turn-based 架构把任务**串行化**——多 agent 并行、人类中途插手时 chat 即崩溃；canvas 让多 agent 在同一空间不同区域并行工作、互见产出。核心证据是 tldraw 的 fairies 实验（见 §2.2）。结论：这是**架构优势**而非时机偶然。

> "The problem with chat is that it's linear. Messages arrive in a queue. Everyone waits for the previous response before the next action begins."  
> "All of the things that make a Canvas good for collaborating with other people make it good for collaborating with AI."

**认知层最强文本：Chelsey Qiu《What we lost in the AI chat stream》**（2026-05-18）✅。chat 造成三重损失：交付物丢失（聊天史是迭代残渣不是产出物）、批判性思维退化、外包了"形成问题"这一步。引 Lee et al. 2025（Microsoft Research + CMU，CHI，319 名知识工作者）："Higher confidence in the AI was associated with less critical thinking."。并用 Clark & Chalmers 的 Extended Mind 标准判定 chat 流不合格作为心智延伸。给 builder 的处方：给 chat 配持久表面（documents、canvases、artifacts），把对话收敛为交付物。

> "Most chat history is the iteration, not the answer."  
> "The risk isn't that AI gives you bad answers. It's that you stop forming the question."  
> "A real extension of your mind has to be findable, persistent, and reliably navigable."

这篇几乎是"AI 文字无限、注意力有限、判断才是残余"的西方镜像表述。

**界面失配论：Maxim Kich《Chat is the wrong interface for AI》**（UX Collective，2026-08-03）✅。LLM 推理能力已远超 chat 所能承载（capability–interface mismatch）；chat 适合维持上下文，却是糟糕的**决策记录系统**（system of record）。

> "And all of this intelligence is trapped inside a messenger, like a chatbot from 2015 but on steroids."  
> "My best ideas disappear somewhere between 10 tool calls, 3 deliverables, and 100 affirmations."

**历史偶然论：Adi Leviim《The chat box isn't a UI paradigm. It's what shipped.》**（UX Collective；Roger Wong 评介 2026-04-22）✅（经评介版核验）。chat 的统治源于"2022 年最快能 ship 的东西"固化为行业默认；此后各家在 chat 周围 retrofit 结构化表面（Canvas、Artifacts、panel），等于承认 chat 独木难支。

> "Expressing intent does not require prose. A date picker expresses temporal intent more precisely than any sentence."  
> "The good AI UX work of the next three years will be distributed across a thousand of those scoped surfaces rather than concentrated in one generalized text field."

**实践者的结构方案：Eric J. Ma《Canvas Chat: A Visual Interface for Thinking with LLMs》**（2025-12-31，入选 SciPy 2026）✅。把对话建为 **DAG**：消息为节点、任意点分叉、多线程合并、矩阵比较；LLM 始终获得沿祖先节点收集的路径上下文。

> "Linear chat actively works against this kind of thinking."

产品自证侧：LMCanvas 口号 **"Linear chat is a dead end."** ✅（主站核验）；thesecondbrain.io 的 **"Line vs Landscape"** 对比框架（2025-10-09）✅。

### 1.3 学术背书（2025–26）

- **CanvasConvo**（arXiv 2605.15848，LMU 慕尼黑，2026-05）✅（摘要页）：把线性 chat 变成嵌入空间画布的 branching conversation tree；**24 人、5–7 天 field study** 证实非线性对话结构支持 exploratory workflows。
- CHI 2026 设立 **Sensemaking workshop**，多篇多人多 agent 知识工作 sensemaking 论文（dl.acm.org/10.1145/3772318.3791157、arXiv 2606.09840）◐；**Cocoa**（CHI 2025，arXiv 2412.10999）把 agent 计划做成文档内可编辑对象（document-integrated planning）◐；**Semantic Prompting**（arXiv 2604.19971）用画布上的空间摆放作为语义 prompt ◐。
- 批判性思维实证：Lee et al. 2025（CHI）见上；Stack Overflow 官方博客《AI is becoming a second brain at the expense of your first one》（2026-03-19）◐。

### 1.4 高频概念词表（外宣可直接取用）

| 概念 | 出处/语境 |
| --- | --- |
| linear chat vs spatial canvas；**"Line vs Landscape"** | 全体；thesecondbrain.io 表述最工整 |
| turn-based vs parallel；serialization vs parallelization | Yang（多 agent 协调视角） |
| spatial presence / observable agent states | Yang、tldraw fairies |
| chat 史 = iteration residue，不是 deliverable；system of record | Qiu、Kich |
| Extended Mind 三标准：findable, persistent, navigable | Qiu 引 Clark & Chalmers |
| scoped surfaces / intent beyond prose | Wattenberger → Leviim |
| cognitive offloading / critical thinking atrophy | Lee et al. 2025 (CHI) |
| sensemaking / meta-cognition / AI-resilient interfaces | Appleton、CHI Tools for Thought 圈 |
| facilitation whiteboard vs **thinking canvas** | 2026 市场评测归纳的品类分化（非单篇原创术语） |
| malleable software / adaptation at the point of use | Ink & Switch / Litt |

---

## 2. 产品层：两条对向而行的路线

### 2.1 传统 TfT / 知识工具加 agent

**Notion——把自己变成 harness 之上的 UI 层（最激进）**

- 3.0（2025-09-18）首发 Notion Agent：单次自主工作约 20 分钟、跨数百页面 ◐；3.3（约 2026-02）Custom Agents 按 schedule/trigger 自主运行 ◐。
- **3.6（2026-07-01）External Agents** ✅：官方原话 **"Now you can bring multiple agents into a single UI"**——首批接入 **Claude 和 Cursor**；从共享 board 给 agent 派任务、像 @ 同事一样 @-mention agent、全团队围观执行过程；配套 External Agents API、Agent SDK、Markdown API；企业侧 agent 活动进 audit log。
- 监督方式 = **board 即监督面**：任务卡片承载 agent 状态，人通过看板分派与验收。代价：数据在 Notion 的库里，无文件层。

**tldraw——空间化 agent 的最深探索者，教训最有价值**

- 路线：makereal（2023）→ tldraw.computer → agent starter kit → **fairies.tldraw.com**（2025-12 为期一月的付费实验：多个精灵形态 agent 在画布上并行干活，一个 fairy 起草计划、建 todo、分派给其他 agent；共享 todo + 画布截图 + 结构化 shape 数据作公共上下文）◐。
- **《Text is not enough》**（2026-03-30）✅：tldraw MCP App 让 LLM 在对话里直接画图，且**双向**——"You can edit the canvas too—and the agent can see your changes."
- **《Agents can't point》**（2026-08-11）✅：chat/侧栏/终端里的 agent 缺视觉上下文，无法解析"this/that"指示性引用。fairies 的教训：**让 agent 直接往画布上贴 shape/便签，结果是"匿名、杂乱"**；正解是**评论线程**——"its own record, sitting parallel to the canvas and pointing into it"（平行于画布、指向画布内部的独立记录），署名、线程化、agent 可回复人。

**Heptabase——"AI 帮你想而不是替你想"，agent 产出物化为空间结构**

- 官网定位 "Master anything you learn. Do your best research with AI." ✅。2026 changelog（官方 wiki）✅：v1.88.0（2026-03-31）AI Tutor 生成课程大纲走 **diff view 人工确认后才接受**；v1.90.6（2026-04-20）Goal Discovery 通读你近期白板、浮现正在成形的方向；v1.103.0（2026-08-14）"Break down book skill" 把整本书拆成"段落完整、引用可溯"的白板，**agent 获得建立/更新白板连线的能力**；CLI 0.2→0.4 给外部 agent 开了纯文本入口。

**Tana——警示案例：头部 TfT 弃阵地**

✅ 官网已整体转型为 "an agentic meeting platform"，tagline **"Work gets done in the meeting, not after"**；原 outliner 产品（supertags/nodes）迁至 outliner.tana.inc，官方要求把旧内容视为历史。一个头部 TfT 产品放弃"笔记+结网"主战场、押注会议场景的实例。

**Obsidian 生态——最接近 note.md 路线，但只有约定没有产品化**

- **Bases**（1.9.10+ 核心插件）◐：对本地 md/frontmatter 做 table/cards/list/map 数据库视图，视图逻辑存 `.base` 文件，表格内编辑**写回笔记 frontmatter**——"文件为唯一事实源的视图层"已有成功先例。
- Claude Code × vault 工作流成为 2026 社区内容品类 ◐：obsidian-claude-code-mcp、MCPVault（read-only 模式、大笔记导航）等。社区最佳实践："vault 装你的真实思考，Claude 读它取上下文但不污染它——Claude 的 plan/memory 放 `~/.claude/`"——与 `✦`/`●` 署名分离哲学同构，但**只有约定、没有产品化的落笔区分**。
- Steph Ango 2025 年底发布官方 Obsidian Claude Skills（vault-aware 工作流）◐。

### 2.2 harness / agent 侧补 UI

**Claude Cowork——任务监工面，不是思考面**

- 官方 ✅："shows each step: the files it opens, tools it uses, and choices it makes"；大项目分块并行；产出直接落进你的文件夹。
- 设计师实测批评（nervegna substack）✅，对"思考型工作"的缺口即 note.md 的机会清单：**"It overwrites files. Cowork has no version control."**；**"Memory is thin... memory works only within projects."**；好评集中在"does the task"（研究综述、竞品审计这类 "the work around the work"）。UI 是进度/许可卡片/文件产出的**监工面**，没有持久的空间/标注/结网层。
- Claude Code Artifacts beta（2026-06-18）：会话产出发布成自更新私有 URL 页 ◐——补的是"产出呈现层"，仍非可积累的思考空间。

**Cursor 向非代码知识工作渗透** ◐：GTM 团队用它做数据分析、纪要、邮件；"Cursor for knowledge workers" 成教程品类；且 Cursor 作为 external agent 反向接入 Notion 3.6 ✅——harness 借 Notion 的 board 当监督 UI。

**"mission control for agents"层——整合期，正被平台吸收**

| 产品 | 形态 | 状态 |
| --- | --- | --- |
| GitHub Agent HQ（2025-10-28）◐ | 官方 "mission control"，跨 GitHub/VS Code/CLI 指挥多家 agent | 平台收编第三方编排 UI 空间 |
| Conductor ✅ | Mac 本地并行 agent（git worktree 隔离），"review and merge" | 存活且活跃 |
| Vibe Kanban ✅ | 给 coding agent 的看板（30k+ 用户、100k+ PR） | **官网自宣 sunsetting**，转社区开源；母公司 Bloop 2026-04 关停 ◐ |
| Terragon ◐ | 云 VM 编排 | 2026-01 关停 |

**格局判断**：独立"agent 监工 UI"没有独立生意——被平台（GitHub、Notion）吸收，幸存者靠本地+评审体验。且全部是吞吐量导向（派单→diff→merge），**无一做"思考/沉淀"**。

### 2.3 AI-native 画布 / second-brain 盘点（一句话定位）

**真正把 agent 和持久空间视图配对的**：Fabric ✅（"The AI workspace that thinks with you"——有名字有性格的 agent 团队各配专属收件箱 + 无限画布）；Storyflow ✅/◐（AI 通读整块画布转结构化产出）；Heptabase、tldraw（见上）。

**agent 有、空间无**：Mem ✅（"Your AI chief of staff"，主动式提醒）；Kuse ✅（已从 AI 画布转向自动化跑活——**画布叙事淡出**）；Recall、Reflect、Capacities、MyMind ◐（chat-with-notes 或收藏型）。

**已死/停滞（画布赛道出清）**：Kosmik 2026-04-30 宣布关停 ◐；Muse 实质停滞（updates 页 404，Adam Wiggins 的 retrospective 是该路线经典复盘）◐；Scrintal 无 agent 动作、沦为各家替代榜单靶子 ◐；Napkin 只是文字→图示生成器。

---

## 3. 研究与协议层

### 3.1 Ink & Switch 一脉：版本控制即人机协作界面

- **《Malleable Software》**（Litt/Horowitz/van Hardenberg/Matthews，2025）✅：**AI 写代码本身不带来可塑性**——"AI code generation alone does not address all the barriers to malleability."；"Bringing AI coding tools into today's software ecosystem is like bringing a talented sous chef to a food court."。但 AI 是可塑环境的极佳互补。
- **Patchwork** ✅：universal version control——把 branching/diffing/history 变成对所有创作介质可用的通用原语，建在 Automerge（local-first CRDT）上；Dispatch #17（2026-05）✅ 确认它已是团队两年来日常工作的容器。
- **Litt 的关键论述** ✅（newsletter）+ ◐（X）：**"Just like human collaborators, AI tools need a way to stage tentative changes for others to review."**；X 上的表述更直接——**"your ability to check the agent's work is the bottleneck"**（需要 better diff views、zoomed-out diffs）。这是"harness 之上缺判断层"最直接的西方佐证。
- **Andy Matuschak**（2025-03-31 patron letter）✅：LLM 端到端生成 Obsidian 插件成功，但前提是系统行为要有**共享表征**（可见的 spec）——"Understanding bounds the complexity of systems we can create—even if an LLM is doing the programming."。近况 ◐：参与做 Pico——"a conservatory for human attention"（人类注意力的温室）。
- **Maggie Appleton** ✅（/now，2026-01）：2025-10 起加入 GitHub Next 做 agents 研究——"Agents. AI agents are all I can see, read, build, and think about these days."。此前原型 Lodestone：让 LM 引导人"想得更多而不是更少"（明确 claims → 找 evidence → 搭 argument 结构）。
- **Gordon Brander**（2024-05）◐：关停 Subconscious/Noosphere 时的转身宣言——要放大智能，不该建去中心化笔记图谱，而应 "go to work on personal AI"。tools-for-thought 一代向 AI 转向的标志性文本。

### 3.2 "笔记已死"论战：现成的正反引文库

- 正方经典：Dan Shipper《The End of Organizing》（Every，2023-01）◐——LLM 会替你组织、打标、串联、写作时自动 resurface，整理笔记的时间白花了。
- 反方经典：Casey Newton（Platformer，2023）◐——"Thinking takes place in your brain. And thinking is an active pursuit… It is a process that stubbornly resists automation."
- **2026 新引爆点：Karpathy 的 "LLM wiki"** ◐——raw 材料丢进文件夹，LLM 读取、写出干净页面并互相链接；分工表述 **"the LLM is the librarian, you are the curator"**。
- 2026 反方 ◐："If the LLM fabricates 10% of the information in your wiki… you won't know which 10%"；"to curate correctly, you must first know what is correct"；**"You can outsource your thinking. But you can't outsource your understanding."**（Tony Demol 等）。

这组正反引文与"读 AI 写的，留下你想的"几乎互为镜像，外宣可直接对话。

### 3.3 harness 正典：UI 被明确排除在边界外

- **OpenAI《Harness engineering》**（原文 403，经 InfoQ/HumanLayer 转述交叉核实）◐：5 个月、小团队**零手写代码**发布约 100 万行代码的 beta 产品；人的工作变为 design environments、specify intent、build feedback loops。**通篇讲环境与验证，不讲人类阅读/判断界面**。
- **HumanLayer《Skill Issue: Harness Engineering for Coding Agents》**（2026-03-12）✅：harness = agent 的 "runtime, or its peripherals"（skills、MCP、subagents、memory）；**明确说 harness 不是模型、不是 UI**。
- **Avi Chawla《The Anatomy of an Agent Harness》**（2026-04-06）✅：11 组件解剖；**UI 明确不在 harness 内**——CLI/VS Code/web 等 client surfaces "共享同一个 harness"但与之分离。"If you're not the model, you're the harness."
- **Nimbalyst《Eight Pillars》**（2026-05-20）✅：西方话语里**罕见把 Visual Interface 列为一等支柱**的文本（diffs、审批门、"决策附着于 artifact 而非埋在 chat 里"），但语境仍是编码工作台。另一句与 file-over-app 完全同频、极可引：**"If you cannot read it, edit it, version it, point a different agent at it, and take it with you, it is not really yours."**
- **meta-harness** ◐（CodePick 指南、danielrosehill 快照 ✅）：Claude Code/Codex 之上的 cross-repo coordination / session memory / policy 层，2026-06 一波落地（Databricks Omnigent、Zed ACP、Conductor 等）。仍是执行编排，不是思考层。

### 3.4 files over apps × agents

- Steph Ango《File over app》（2023）◐："If you want to create digital artifacts that last, they must be files you can control."
- **Kurtis Redux《Markdown in 2026: Not Just for Writing Anymore》**（2026-05-18）✅：**"the 'small and beautiful' Markdown editor is quietly turning into a local workbench for AI agents"**；判别问题是 "can an AI agent operate your local text corpus safely, directly, and under your control?"；存活要素：安全隐私、信息架构、**MCP、CLI、Git、快速本地搜索**。——note.md 的定位被外部话语直接验证。

### 3.5 Generative UI 协议层：agent 如何渲染超越文本流的界面

- **CopilotKit《The Developer's Guide to Generative UI in 2026》**（2026-01-29）✅ 给出三档谱系：① Static（**AG-UI**：前端保留控制，agent 选预定义组件填数据）② Declarative（**A2UI**、Open-JSON-UI：agent 返回结构化 UI 规格）③ Open-ended（**MCP Apps**：agent 返回完整 UI 表面）。"When agents move beyond chatting and start doing things, text alone becomes a bottleneck."
- **AG-UI** 生态（截至 2026-05）◐：LangGraph、CrewAI、Microsoft Agent Framework、Google ADK、AWS Strands、Pydantic AI、LlamaIndex 已支持；AWS Bedrock AgentCore 接入。
- **Google A2UI v0.9**（2026-07，InfoQ 报道）◐：核心转向——**agent 应说应用现有 design system 的语言，而不是自己发明组件**；新增 client-server 数据同步（支持与 agent 协同编辑）。
- 对 note.md 的含义：agent 往画布/看板里"投喂结构化内容"的协议基建正在成熟，UI 层产品可以站在协议上而不必自造。

### 3.6 办公入口代际论：西方只有拼图

西方**没有人完整说出"office 文件 → 浏览器/搜索 → harness"这条代际链**。现有拼图 ◐：OpenAI 官方措辞 "an agent-first world"；Savluc《The End of Apps Is Coming》（2026-04，下一代不再围绕人打开的 app 组织，而围绕 agent）；Ashe《ChatGPT's computer control turns the browser into the new agent runtime》（2026-07-18，浏览器变成 agent 的运行时而非人的入口）；《The Agentic Browser Wars 2026》（Atlas/Comet/Dia/Arc/Leo 争夺默认 AI 浏览器）。**完整的代际论述 + "每代入口旁都有一个沉淀层"的推论，是 note.md 论述的差异化空间。**

---

## 4. 结构性判断：空位在哪里

1. **监督面两形态都不是思考面。** board（Notion/GitHub）和 canvas+comments（tldraw）解决的是"agent 干得怎么样"，不是"我怎么想清楚"。验收 ≠ 判断，diff ≠ 沉淀。
2. **三件套无人集齐**：agent 监工面 + 持久思考面 + 文件为唯一事实源。Cowork 有监工面，缺版本化与持久思考层（实测批评直指 "no version control""memory is thin"——恰是 vault+git 天然能补的）；Notion 三者有二但数据在库里；Obsidian 有文件层与视图先例（Bases），缺产品化的 agent 协作与署名。
3. **tldraw 用实验替所有人交了学费**：agent 不该直接往画布上贴产出（匿名、杂乱），正解是**平行于画布、指向画布内部、有署名、可线程化的记录**——这正是 `.note.md` 手记 + 块引用 + `✦`/`●` 署名的形态。西方最深的空间化 agent 探索，结论收敛到了 note.md 已有的约定上。
4. **"判断留存"无人当第一性用途。** 全部画布产品把空间当"摆放思考的地方"，没有一家把"人读完 AI 产出后留下的确认/批注/否决"当作画布存在的理由。认知层批判文（Qiu：交付物丢失、停止形成问题）已经把需求论证完了，产品端没人接。
5. **纯执行编排无独立生意，纯画布无 agent 也无生意**——两边的死亡名单（Terragon、Vibe Kanban；Kosmik、Muse）共同指向：**可防御的位置在中间层**——判断与沉淀，且以用户拥有的文件为底座（Nimbalyst："If you cannot read it… it is not really yours."）。

## 5. 对 note.md 的启示（映射五大信念)

| 调研发现 | note.md 落点 |
| --- | --- |
| Qiu"chat 史是迭代残渣"；Lee et al. 批判性思维退化 | 信念 1"判断才是残余"的西方实证与话语弹药 |
| Obsidian Bases：`.base` 视图文件、编辑写回 frontmatter | 信念 2 之上做视图层的成功先例：**召回结果落成派生视图（画布/看板/列表），布局本身是纯文本、agent 可读、git 可版本化** |
| tldraw《Agents can't point》：评论=平行记录+指向；Heptabase diff view 确认；Litt "stage tentative changes for review" | 信念 3 的界面形态被外部验证：**agent 产出须经署名的、可审阅的落笔区（`✦`/`●`、`.note.md`、diff/确认流）进入你的结构** |
| Cowork 实测缺口："no version control""memory thin" | 信念 4：vault 镜像 + git 版本化恰好补上 harness 自己不做的持久层 |
| Notion 3.6 External Agents 反向证明 harness 需要外部 UI 层；但 Notion 用私有库承接 | 信念 5 的竞争叙事：**同样承接多 agent，note.md 用中立的纯文件 vault 承接**——"一个 vault，多个 agent"对"一个库，多个 agent" |
| Karpathy "librarian vs curator" 论战 | 外宣现成句式："the LLM is the librarian, you are the curator" ↔ "读 AI 写的，留下你想的" |
| GenUI 三档协议（AG-UI/A2UI/MCP Apps）成熟中 | 画布/看板接 agent 产出时可站在公共协议上,与"钉协议不钉版本"方针一致 |

能力清单（综合 §4 空位,新产品形态的最小集）：

1. **召回即视图**：一句话召回的结果落成可保存、可版本化的空间视图,不消失在对话滚动条里;视图是文件的派生投影,文件仍是唯一事实源。
2. **卡片上有判断落点**：每张卡片可批注、确认、建链;`✦`/`●` 一眼可分;关系只在人确认处生长——agent 只建议摆放与聚类。
3. **agent 走评论道,不走画布道**：agent 对空间结构的意见以平行记录（署名、线程、指向具体块）呈现,经确认才物化为连线/结构（tldraw 教训 + Heptabase diff view 先例）。
4. **引用一键点开、宿主稳定**：块引用 + 镜像宿主,换设备不断链。
5. **思考↔执行双向**：卡片上落定的决定可派回任一 harness 成任务（Notion 3.6 的 board 派单先例,但落在中立文件层）。

---

## 6. 来源总表

### 已抓取核实（fetched）

**话语层**

- Wattenberger, *Why Chatbots Are Not the Future*（2023-05）— https://wattenberger.com/thoughts/boo-chatbots/
- Angelina Yang, *Canvas vs. Chat*（2026-06）— https://angelina-yang.medium.com/canvas-vs-chat-why-spatial-interfaces-win-for-ai-collaboration-61c26c004a48
- Chelsey Qiu, *What we lost in the AI chat stream*（2026-05-18）— https://medium.com/design-bootcamp/what-we-lost-in-the-ai-chat-stream-2f96a22a6b80
- Maxim Kich, *Chat is the wrong interface for AI*（2026-08-03）— https://uxdesign.cc/chat-is-the-wrong-interface-for-ai-de14d352de1b
- Roger Wong 评 Leviim（2026-04-22）— https://rogerwong.me/2026/04/chat-box-wrong-ui-paradigm （原文 https://uxdesign.cc/the-chat-box-isnt-a-ui-paradigm-it-s-what-shipped-96e931d92769 ）
- Eric J. Ma, *Canvas Chat*（2025-12-31）— https://ericmjl.github.io/blog/2025/12/31/canvas-chat-a-visual-interface-for-thinking-with-llms/
- thesecondbrain.io, *Visual AI Chat vs Linear Chat*（2025-10-09）— https://www.thesecondbrain.io/blog/visual-ai-chat-vs-linear-chat
- LMCanvas 主站 — https://www.lmcanvas.ai/
- CanvasConvo（arXiv，2026-05）— https://arxiv.org/abs/2605.15848
- CopilotKit, *Developer's Guide to Generative UI in 2026*（2026-01-29）— https://www.copilotkit.ai/blog/the-developer-s-guide-to-generative-ui-in-2026

**产品层**

- Notion 3.6 releases（2026-07-01）— https://www.notion.com/releases/2026-07-01
- tldraw, *Agents can't point*（2026-08-11）— https://tldraw.dev/blog/agents-cant-point
- tldraw, *Text is not enough*（2026-03-30）— https://tldraw.dev/blog/text-is-not-enough
- fairies.tldraw.com — https://fairies.tldraw.com/
- Heptabase changelog — https://wiki.heptabase.com/changelog ；官网 https://heptabase.com/
- Tana（转型后官网）— https://tana.inc/
- Claude Cowork 官方 — https://claude.com/product/cowork
- nervegna, *Claude Cowork for designers*（实测）— https://nervegna.substack.com/p/claude-cowork-for-designers-a-working
- Conductor — https://conductor.build/ ；Vibe Kanban — https://vibekanban.com/
- Fabric — https://fabric.so/ ；Kuse — https://www.kuse.ai/ ；Mem — https://get.mem.ai/ ；Storyflow — https://storyflow.so/

**研究与协议层**

- Ink & Switch, *Malleable Software*（2025）— https://www.inkandswitch.com/essay/malleable-software/
- Ink & Switch, Patchwork — https://www.inkandswitch.com/project/patchwork/ ；Dispatch #17（2026-05）— https://www.inkandswitch.com/newsletter/dispatch-017/
- Geoffrey Litt, *Towards universal version control with Patchwork*（2024-05-05）— https://buttondown.com/geoffreylitt/archive/towards-universal-version-control-with-patchwork/
- Andy Matuschak, patron letter（2025-03-31）— https://andymatuschak.org/files/2025-03-31.html
- Maggie Appleton /now（2026-01）— https://maggieappleton.com/now/
- HumanLayer, *Skill Issue: Harness Engineering*（2026-03-12）— https://www.humanlayer.dev/blog/skill-issue-harness-engineering-for-coding-agents
- Avi Chawla, *The Anatomy of an Agent Harness*（2026-04-06）— https://blog.dailydoseofds.com/p/the-anatomy-of-an-agent-harness
- Nimbalyst, *Eight Pillars*（2026-05-20）— https://nimbalyst.com/blog/agent-harness-above-claude-code-codex/
- danielrosehill/AI-Harnesses（2026-04 快照）— https://github.com/danielrosehill/AI-Harnesses
- Kurtis Redux, *Markdown in 2026*（2026-05-18）— https://kurtis-redux.medium.com/markdown-in-2026-not-just-for-writing-anymore-d4433fa1ec9a

### 仅搜索摘要（snippet-only，引用前二次核实）

- OpenAI, *Harness engineering* — https://openai.com/index/harness-engineering/ （403;InfoQ 报道 https://www.infoq.com/news/2026/02/openai-harness-engineering-codex/ ）
- CodePick, *Meta-Harness 2026 Buyer's Guide* — https://codepick.dev/en/guides/meta-harness-2026/ （403）
- LMCanvas 博客文 — https://lmcanvas.ai/blog/best-ai-chat-interface-2026 （SPA 404）
- Notion 3.0（2025-09-18）— https://www.notion.com/releases/2025-09-18 ；TechCrunch — https://techcrunch.com/2025/09/18/notion-launches-agents-for-data-analysis-and-task-automation/
- tldraw agent starter kit — https://tldraw.dev/starter-kits/agent ；fairies 细节 — https://x.com/tldraw/status/2002113715043467509
- Obsidian Bases — https://obsidian.md/roadmap/ ；https://got.md/obsidian-bases/
- obsidian-claude-code-mcp — https://github.com/iansinnott/obsidian-claude-code-mcp
- Claude Code Artifacts beta — https://venturebeat.com/data/anthropics-claude-code-artifacts-update-brings-live-shared-dashboards-and-interactive-workspaces-to-enterprises
- GitHub Agent HQ（2025-10-28）— https://github.blog/news-insights/company-news/welcome-home-agents/
- Steph Ango, *File over app* — https://stephango.com/file-over-app
- Dan Shipper, *The End of Organizing* — https://every.to/chain-of-thought/the-end-of-organizing
- Karpathy LLM wiki 论战 — https://angelo-lima.fr/en/karpathy-second-brain-obsidian-claude-en/ ；https://medium.com/@tony.demol/karpathys-llm-wiki-with-a-single-brain-975df9c84be6
- Stack Overflow blog（2026-03-19）— https://stackoverflow.blog/2026/03/19/ai-is-becoming-a-second-brain-at-the-expense-of-your-first-one/
- Gordon Brander, *Subconscious is winding down* — https://newsletter.squishy.computer/p/subconscious-is-winding-down
- Andy Matuschak, *Exorcising us of the Primer* — https://andymatuschak.org/primer/
- CHI 2026 sensemaking — https://dl.acm.org/doi/10.1145/3772318.3791157 ；https://arxiv.org/pdf/2606.09840 ；Cocoa — https://arxiv.org/pdf/2412.10999 ；Semantic Prompting — https://arxiv.org/pdf/2604.19971
- A2UI v0.9（2026-07）— https://www.infoq.com/news/2026/07/google-a2ui-genui/ ；AG-UI — https://docs.ag-ui.com/introduction
- 入口代际拼图 — https://medium.com/@paulgeorgesavluc/the-end-of-apps-is-coming-2026-is-the-year-ai-agents-become-the-new-workforce-e05cb0109988 ；https://kenashe.ai/blog/2026-07-18-chatgpts-computer-control-turns-the-browser-into-the-new-agent-runtime/ ；https://presenc.ai/research/agentic-browser-wars-2026
- Kosmik 关停 — https://www.youtube.com/watch?v=anUCeRf58nA ；Muse retrospective — https://adamwiggins.com/muse-retrospective/
- Vibe Kanban/Bloop 关停 — https://aq.dev/alternatives/vibe-kanban/