# Simplified Chinese translations of build_pages.py PAGES.
# Same list, same order, same keys. Paths, URLs, HTML structure,
# <code> contents and <pre><code> blocks are unchanged.

PAGES = [
# ---------------------------------------------------------------- compare
{
 "path": "/compare/roam-research/",
 "title": "note.md vs Roam Research（2026）— 文件、agent，以及 Roam 后来怎么了",
 "desc": "一份诚实的对比：note.md 与 Roam Research——大纲笔记、每日笔记和 [[链接]]，一边是带 AI agent 支持的本地纯文本文件，一边是 Roam 的浏览器内图谱。附迁移路径。",
 "crumb": "对比",
 "h1": "note.md vs Roam Research",
 "lead": "两者都爱大纲、每日笔记和 [[双方括号]]。一个把你十年的思考放在某家公司的浏览器标签页里。另一个放在一个属于你的文件夹里。",
 "table": {
  "head": ["", "note.md", "Roam Research"],
  "rows": [
   ["笔记住在哪", "磁盘上的纯 markdown 文件", "云端的私有图数据库"],
   ["价格", "免费，开源", "每月 $15 起"],
   ["每日笔记 &amp; 大纲", "有 —— <code>.note.md</code> 大纲文件", "有 —— 这个模式的诞生地"],
   ["[[Wikilinks]] &amp; 反向链接", "有，全 vault 一个命名空间", "有，外加块引用和查询"],
   ["块级引用", "有 —— <code>((file#b-xxxxxx))</code>，编辑后依然有效", "有 —— 块引用，走得更深（嵌入、查询）"],
   ["AI agent", "一等公民：纯文件 + <code>AGENTS.md</code>，agent 会读你的批注", "没有内置"],
   ["阅读并批注 AI 文档", "核心工作流 —— sidecar 文件<code>.note.md</code>", "不是重点"],
   ["开发节奏", "活跃", "自 2021 年前后出了名地安静"],
   ["离线 / 长久性", "任何编辑器都能读，永远", "得先导出；读图谱离不开它的应用"],
  ]},
 "sections": [
  ("实话实说", """<p>Roam 在 2020 年发明了“每日笔记加反向链接”这种思考方式，该给的功劳要给：如果你重度使用块引用、嵌入和 datalog 查询，Roam 在这些方向上仍然比 note.md 走得深。这里不假装不是这样。</p>
<p>但 Roam 押了一个没经受住时间的赌注：你的图谱住在他们的数据库里，锁在他们的订阅后面，看他们路线图的脸色——而那份路线图已经安静了好几年。与此同时，世界翻了个面。agent 以兆字节为单位写 markdown，如今真正要紧的工具，是那些读写<em>纯文件</em>的工具。浏览器标签页里的图谱当不了你 agent 的记忆。一个 markdown 文件夹可以。</p>
<p>note.md 保留了 Roam 的好东西——大纲编辑器、每日笔记、一个大 <code>[[namespace]]</code>、即时搜索——然后在文件之上把它重建了一遍。你的 vault 今天能在任何编辑器里打开，五十年后也能。它还补上了 Roam 从未有过的东西：你的 agent 是一等公民，动笔之前先读你的批注。</p>"""),
  ("从 Roam 迁移", """<p>把图谱导出为 JSON（Roam 支持完整导出），note.md 的 Roam 导入器（在路线图上，转换器已可用）会把页面转成 <code>wikipage/</code> 大纲笔记，把每日笔记转成 <code>dailynote/yyyy/yyyy-MM-dd.note.md</code>——同时把 <code>[[July 10th, 2026]]</code> 这样的日期链接改写成规范的 <code>[[2026-07-10]]</code>，并报告所有失效链接。你三年的笔记，变成三年 agent 可搜索的上下文。</p>"""),
  ("怎么选", """<ul>
<li><b>留在 Roam</b>：如果块引用、嵌入和查询在你的工作流里是承重墙，而且你对订阅和它的节奏都还满意。</li>
<li><b>选 note.md</b>：如果你想要 Roam 的书写手感、但文件归你所有，想让笔记兼任 agent 的记忆，想让阅读 AI 的产出成为一等公民的动作。</li>
</ul>"""),
 ],
 "faq": [
  ("我能把 Roam Research 的图谱导入 note.md 吗？",
   "能——在 Roam 里把图谱导出为 JSON，然后把页面转成 wiki 笔记、每日笔记转成带日期的大纲文件。日期链接会被改写成规范的 [[yyyy-MM-dd]] 形式，失效链接会被报告。"),
  ("note.md 有 Roam 那样的块引用吗？",
   "note.md 有稳定的块 ID：每个顶层块都有一个 b-xxxxxx id，可以在任何地方以 ((file#b-xxxxxx)) 引用。它覆盖引用和跳转；Roam 式的转写嵌入（transclusion/embeds）不是目标。"),
  ("note.md 免费吗？",
   "免费。note.md 免费且开源（Apache-2.0）。Roam Research 每月 $15 起。"),
 ],
},
{
 "path": "/compare/obsidian/",
 "title": "note.md vs Obsidian（2026）— 两个信奉“文件重于应用”的编辑器，一个为 agent 而生",
 "desc": "note.md 和 Obsidian 都把笔记存成本地 markdown。区别在于：note.md 为阅读和批注 AI 产出而生，手记（sidecar）和 agent 约定开箱即用。",
 "crumb": "对比",
 "h1": "note.md vs Obsidian",
 "lead": "最近的表亲。都信“文件重于应用”。Obsidian 是万能工具箱；note.md 是为 AI 阅读循环磨快的一把刀。你的 vault 在两边都能打开——这是故意的。",
 "table": {
  "head": ["", "note.md", "Obsidian"],
  "rows": [
   ["存储", "纯 markdown 文件，本地", "纯 markdown 文件，本地"],
   ["价格", "免费，开源", "免费（闭源）；Sync/Publish 付费"],
   ["阅读 AI 文档", "核心工作流：干净的阅读视图，批注保留", "通用编辑器；折腾一番也能做到"],
   ["批注", "sidecar 文件<code>.note.md</code> —— 原文保持干净", "直接改原文，或装社区插件"],
   ["agent 支持", "内置：<code>AGENTS.md</code> 约定、块级引用、批注即 agent 输入", "靠插件和自己动手（一个流行玩法）"],
   ["大纲编辑", "原生 <code>.note.md</code> 大纲视图", "靠插件；Obsidian 以页面为中心"],
   ["插件生态", "小而克制，进程外，按能力授权", "庞大 —— 数千个社区插件"],
   ["移动端", "还没有（macOS 优先）", "优秀的 iOS/Android 应用"],
   ["互操作", "vault 可在 Obsidian 打开", "vault 可在 note.md 打开"],
  ]},
 "sections": [
  ("实话实说", """<p>如果你爱 Obsidian，留着它——认真的。它是史上最成功的“文件重于应用”编辑器，插件生态无人能敌，把 Claude Code 指向一个 Obsidian vault 是这十年最棒的 DIY 玩法之一。note.md 的 vault 格式刻意保持 Obsidian 兼容，因为我们信的是同一件事：你的文件应该在任何地方都打得开。</p>
<p>区别在于开箱之后发生什么。Obsidian 是一个要自己组装的通用工具箱：想跑通 AI 阅读循环，你得接插件、定约定、配 agent，然后祈祷这些零件一直兼容。note.md 把这个循环当产品交付：agent 写文档，你在一个为判断而造的视图里读，你的高亮落进sidecar <code>.note.md</code>，永远不污染原文，而每个造访你 vault 的 agent 都会先读你的页边批注。不用组装。</p>
<p>sidecar 文件才是真正的岔路口。Obsidian 的批注住在文档内部——你自己写的笔记没问题，agent 生成、还可能重新生成的文档就尴尬了。note.md 把可再生的（AI 的文本）和不可替代的（你的判断）分开，一个文件一个文件地分。</p>"""),
  ("两个都用", """<p>这不是离婚。note.md 的 vault 就是一个 markdown 文件夹：用 Obsidian 打开它看图谱、在手机上速记；用 note.md 打开它跑阅读-批注循环和 agent 工作流。两个客户端，一个事实来源。这就是文件的全部意义。</p>"""),
  ("怎么选（或者不选）", """<ul>
<li><b>选 Obsidian</b>：如果你要最多的插件、移动应用和图谱视图——而且享受亲手组装自己的 AI 工作流。</li>
<li><b>选 note.md</b>：如果你的日常越来越多是在读 agent 写的东西，想要批注即数据、agent 约定开箱即用，一步都不用装。</li>
<li><b>两个都用</b>，同一个 vault。文件不逼你二选一。</li>
</ul>"""),
 ],
 "faq": [
  ("我能在 Obsidian 里打开 note.md 的 vault 吗？",
   "能。note.md 的 vault 就是纯 markdown，[[wikilinks]] 按文件名解析，并刻意保持 Obsidian 兼容。sidecar .note.md 文件在那边显示为普通笔记。"),
  ("用 note.md 必须离开 Obsidian 吗？",
   "不用。两个应用指向同一个文件夹即可。很多人留着 Obsidian 做移动速记和图谱视图，用 note.md 阅读 AI 文档、写批注。"),
  ("什么是sidecar 批注？",
   "在 note.md 里对 xxx.md 高亮或评论时，你的批注会存进一个sidecar 文件 xxx.note.md。原文档保持干净、可再生；你的判断成为独立、可搜索的数据。"),
 ],
},
{
 "path": "/compare/notion/",
 "title": "note.md vs Notion（2026）— 你的文件 vs 他们的工作区",
 "desc": "Notion 是一体化云工作区。note.md 是你磁盘上的一个 markdown 文件夹，为 AI 时代而生。所有权、长久性、agent，以及两边各自真正的胜场。",
 "crumb": "对比",
 "h1": "note.md vs Notion",
 "lead": "Notion 想成为你们团队做一切事的工作区。note.md 什么都不想成为——只是文件、一个好的阅读器，和你的判断。对同一个未来下了相反的注。",
 "table": {
  "head": ["", "note.md", "Notion"],
  "rows": [
   ["模式", "归你所有的本地 markdown 文件", "云工作区，块存在他们的数据库里"],
   ["价格", "免费，开源", "有免费档；团队按席位付费，AI 另算"],
   ["离线", "永远可用 —— 这是你的磁盘", "有限；云优先"],
   ["AI", "任何 agent，经由纯文件 —— 你说了算", "Notion AI，在 Notion 里，按他们的规矩"],
   ["团队协作", "基于 git 的共享；单人优先", "优秀 —— 实时多人、评论"],
   ["数据库 &amp; 项目工具", "没有 —— 它是笔记工具（自带 CSV 表格）", "有 —— 表格、看板、日历、表单"],
   ["数据长久性", "五十年后任何编辑器都能读", "可导出 markdown/CSV；结构会掉"],
   ["锁定", "没有 —— 文件夹就是产品", "工作区才是产品"],
  ]},
 "sections": [
  ("实话实说", """<p>如果你要管团队 wiki、项目追踪和招聘流程，Notion 是真的好用，note.md 也没打算成为那个。实时多人、数据库、权限——那是 Notion 的主场，它的席位费挣得堂堂正正。</p>
<p>但个人知识是另一场游戏，时间尺度也不同。你的笔记应该活得比你的雇主长，比你的工具长，说不定也比 Notion Labs Inc. 长。每一页写进云工作区的东西，都是将来要导出、重排、然后对着叹气的东西——问问从 Evernote 撤出来的人就知道。note.md 的答案是结构性的：没有什么可导出，因为从头到尾就只有文件。</p>
<p>然后是 AI 的问题。Notion 给你 Notion AI——一个助手，在一个应用里，按席位收费。note.md 给你一个任何 agent 都能上手干活的 vault：今天是 Claude Code，下周不管发布什么新东西，读的都是同样的文件、同一份 <code>AGENTS.md</code>。在助手一个月换一茬的十年里，把知识押给某一家的 AI，才是新的锁定。</p>"""),
  ("怎么选", """<ul>
<li><b>选 Notion</b>：团队 wiki、项目管理，以及一切需要多人编辑和数据库的东西。</li>
<li><b>选 note.md</b>：你自己的思考——阅读 AI 产出、每日笔记、一个能复利几十年、喂饱你未来所有 agent 的个人知识库。</li>
<li><b>常见搭配：</b>团队用 Notion，自己用 note.md。</li>
</ul>"""),
 ],
 "faq": [
  ("note.md 能替代团队用的 Notion 吗？",
   "基本不能。note.md 单人优先——建立在纯文件上的个人阅读与笔记工具，共享靠 git。Notion 的数据库和实时协作不是它的目标。"),
  ("我能把 Notion 页面导出到 note.md 吗？",
   "能。Notion 支持导出 markdown；把文件丢进你的 vault，它们就成了可以阅读、批注、互链的普通笔记。"),
  ("为什么本地优先对 AI 重要？",
   "agent 在能直接读写的纯文件上干活最顺手。一个本地 markdown vault，任何命令行 agent 拿来就能用——不要 API token，不限速，没有哪家厂商的 AI 当门卫。"),
 ],
},
# ------------------------------------------------------------ integrations
{
 "path": "/integrations/openclaw/",
 "title": "note.md 搭配 OpenClaw —— 给你的个人 agent 一份真正的记忆",
 "desc": "OpenClaw 把记忆存成 markdown 文件。note.md 是一个带阅读-批注循环的 markdown vault。把两者指向同一个文件夹，agent 的记忆就成了你的笔记本。",
 "crumb": "集成",
 "h1": "note.md + OpenClaw",
 "lead": "OpenClaw 的哲学：模型只记得写到磁盘上的东西。note.md 的哲学：磁盘就是产品。这算不上什么集成——更像两个工具发现彼此天生一对。",
 "sections": [
  ("为什么这对组合成立", """<p>OpenClaw 把记忆存成纯 markdown——长期事实放 <code>MEMORY.md</code>，每日工作笔记放 <code>memory/YYYY-MM-DD.md</code>。这和 note.md vault 的 <code>wikipage/</code> 加 <code>dailynote/</code> 约定在结构上一模一样：带日期的大纲，加上精心维护的页面。同一个想法，各自独立进化出来。</p>
<p>配成一对，双方各补所缺：OpenClaw 得到一个真的会阅读、会整理它记忆的人，外加一个为此而造的视图；你得到一个昼夜干活、并且把一切写在你看得见的地方的 agent。</p>"""),
  ("配置", """<ol>
<li>在 vault 根目录放一份 <code>AGENTS.md</code>，写明约定（sidecar 文件配对、每日笔记路径、<code>[[yyyy-MM-dd]]</code> 日期链接）。摘要可从 <a href="/llms-full.txt">llms-full.txt</a> 取。</li>
<li>把 OpenClaw 的工作区指向你的 vault（或把它的 <code>memory/</code> 软链到 <code>dailynote/</code>——带日期的文件就是带日期的文件）。</li>
<li>让 OpenClaw 把报告和调研写成 <code>.md</code> 文档，放进 vault。</li>
<li>在 note.md 里打开、阅读、高亮、提问——你的批注落进sidecar <code>.note.md</code> 文件。</li>
<li>告诉 OpenClaw 做后续工作前先读sidecar 文件。你的判断成了它的方向盘。</li>
</ol>"""),
  ("循环实战", """<p>晚上：OpenClaw 调研一个主题，把 <code>research/topic.md</code> 丢进 vault。早上：你端着咖啡在 note.md 里读，高亮两个论断，写下一个疑问。下午：OpenClaw 捡起 <code>research/topic.note.md</code>，看清哪些论断赢得了你的注意，在你起疑的地方接着深挖。没有提示词工程——只有文件。</p>"""),
 ],
 "faq": [
  ("OpenClaw 需要插件才能和 note.md 配合吗？",
   "不需要。两边说的都是纯 markdown 文件。在 vault 根目录放一份写明约定的 AGENTS.md，全部的“集成”就这么多。"),
  ("让 OpenClaw 写进我的 vault 安全吗？",
   "把 vault 放进 git（见 GitHub 指南），agent 的每次写入都可 diff、可回滚。按约定，agent 不应写你的 .note.md sidecar 文件——把这条规矩写进 AGENTS.md。"),
 ],
},
{
 "path": "/integrations/cowork/",
 "title": "note.md 搭配 Claude Cowork —— 批注 Claude 造出来的东西",
 "desc": "Claude 的 Cowork 交付 markdown 报告和文档。把它们收进 note.md vault，在本地阅读和批注，让下一个会话读你的页边批注。",
 "crumb": "集成",
 "h1": "note.md + Claude Cowork",
 "lead": "Cowork 在云端跑 Claude，连接你 Mac 上的文件夹。连上你的 vault，Claude 产出的一切都变成你能阅读、能标记、能留存的东西。",
 "sections": [
  ("为什么这对组合成立", """<p>Cowork 的交付物绝大多数是 markdown：调研报告、计划、规格、草稿。默认情况下它们四处散落——这里一个下载，那里一个会话附件。把 Cowork 指向你的 note.md vault，它的产出就落在你阅读循环所在的地方：每份报告有个家，每次通读留下一份写满判断的sidecar 文件，而你的下一个 Cowork 会话可以被要求先读这些sidecar 文件。</p>"""),
  ("配置", """<ol>
<li>在 Claude 桌面应用里，把 vault 文件夹连进 Cowork 会话（“Add folder”）。</li>
<li>在 vault 根目录加一份 <code>AGENTS.md</code>（约定摘要见 <a href="/llms-full.txt">llms-full.txt</a>）——Claude 会自动读取并遵守家规。</li>
<li>让 Claude 把交付物存进 vault，例如 <code>research/2026-07-11-competitor-scan.md</code>。</li>
<li>在 note.md 里读；你的高亮和笔记存进sidecar <code>.note.md</code> 文件。</li>
<li>下个会话，一句话：“读你上周写的报告对应的 .note.md sidecar 文件，回应我的页边批注。”循环闭合。</li>
</ol>"""),
  ("小技巧", """<ul>
<li>让 Claude 使用 <code>[[wikilinks]]</code> 和 <code>[[yyyy-MM-dd]]</code> 日期格式，它的文档就会汇入你 vault 的链接图谱，而不是漂在外面。</li>
<li>把 vault 放进 git——Cowork 的写入随时可 diff，它的文件版本管理和你的也不会打架。</li>
</ul>"""),
 ],
 "faq": [
  ("Claude 会遵守 vault 的约定吗？",
   "会，只要你把约定写进文件夹根目录的 AGENTS.md——读取 agent 指令文件是 Claude Code 和 Cowork 的标准做法。"),
  ("Claude 能读我的批注吗？",
   "这正是重点。sidecar .note.md 文件就是纯 markdown；让任何会话去读，它就能看到你到底高亮了什么、质疑了什么。"),
 ],
},
{
 "path": "/integrations/codex/",
 "title": "note.md 搭配 Codex —— AGENTS.md 本来就是它的母语",
 "desc": "OpenAI 的 Codex CLI 按惯例读取 AGENTS.md。note.md 的 vault 恰好把规则写在这个文件里。在 vault 里运行 codex，它已经知道该怎么做。",
 "crumb": "集成",
 "h1": "note.md + Codex",
 "lead": "Codex 让 AGENTS.md 流行起来——一个纯文本文件，告诉 agent 这个文件夹怎么运转。note.md 的 vault 正是一个规则写在 AGENTS.md 里的文件夹。你能猜到接下来的剧情。",
 "sections": [
  ("为什么这对组合成立", """<p>Codex 从运行目录读取 <code>AGENTS.md</code>——这是它的原生惯例，零配置。note.md 的 vault 把文件规则（sidecar 文件配对、大纲格式、日期链接、块级引用）恰好发布在这个文件里。所以集成就是：<code>cd vault &amp;&amp; codex</code>。完事。</p>
<p>Codex 最强的角色是干活的 agent：让它起草、重构文档、批量处理笔记，或者写 vault 里日积月累的小脚本（导入器、链接检查器、报告生成器）。它写的一切都是 vault 里的 markdown，也就意味着它写的一切都会流进你的阅读-批注循环。</p>"""),
  ("配置", """<ol>
<li>把 <a href="/llms-full.txt">llms-full.txt</a> 里的约定摘要复制进 vault 根目录的 <code>AGENTS.md</code>。</li>
<li>加上 vault 专属规则——例如“绝不修改 <code>*.note.md</code>”、“新调研放 <code>research/</code> 下，文件名带日期前缀”。</li>
<li>在 vault 目录运行 <code>codex</code>。它会自动捡起这些规则。</li>
<li>在 note.md 里审读它的产出；批注；告诉下一次运行去读sidecar 文件。</li>
</ol>"""),
 ],
 "faq": [
  ("Codex 需要 MCP 服务器才能用 vault 吗？",
   "不需要。vault 就是工作目录里的纯文件——Codex 的主场。MCP 端点是给分享 worker 用的（发布页面），基本的 vault 工作用不上。"),
  ("AGENTS.md 里该禁止什么？",
   "唯一的硬规矩：agent 不写你的 .note.md sidecar 文件——那里存的是人的判断。其余（命名、目录、链接风格）都是各家的偏好。"),
 ],
},
{
 "path": "/integrations/hermes/",
 "title": "note.md 搭配 Hermes —— 持久记忆遇上永久笔记本",
 "desc": "Hermes（Nous Research）是带持久记忆、遵循 AGENTS.md 约定的开放 agent。给它一个 note.md vault，它的记忆就变成你能阅读、批注、拥有的东西。",
 "crumb": "集成",
 "h1": "note.md + Hermes",
 "lead": "Hermes 与你一起成长——一个记得住的开放 agent。note.md 是人存放判断的地方。同一个文件夹，两份工作。",
 "sections": [
  ("为什么这对组合成立", """<p>Hermes（Nous Research 出品）围绕持久的、基于文件的记忆构建，并遵循 <code>AGENTS.md</code> 约定——与 OpenClaw 同属开放 agent 一脉，更强调自托管的主权。这套世界观正是 note.md 的世界观：没有隐藏状态，文件即事实，一切可检视。</p>
<p>让 Hermes 跑在 note.md 的 vault 上，它积累的记忆就不再是一团不透明的 agent 产物，而成了你知识库的一部分：能在大纲视图里读，能用 <code>[[wikilinks]]</code> 互链，而且——关键在这——能批注。你可以名副其实地在你 agent 的记忆上写页边笔记。</p>"""),
  ("配置", """<ol>
<li>老规矩，vault 根目录放 <code>AGENTS.md</code>——约定取自 <a href="/llms-full.txt">llms-full.txt</a>，再加你的家规。</li>
<li>把 Hermes 的记忆/工作区目录配置到 vault 里面（例如 <code>agents/hermes/</code>），或让它把产出写进你的 vault 目录。</li>
<li>让它干活。在 note.md 里读它写的东西；批注。</li>
<li>叮嘱 Hermes 重访一个主题前先查 <code>*.note.md</code> sidecar 文件——你的纠正成了它的辅助轮。</li>
</ol>"""),
 ],
 "faq": [
  ("Hermes 和 OpenClaw 是一回事吗？",
   "不是——Hermes 是 Nous Research 的开放 agent，主打持久记忆和自托管运行；OpenClaw 是另一个爆火的开源个人 agent。两者都说 markdown 和 AGENTS.md，所以和 note.md 的搭配方式完全一样。"),
  ("多个 agent 能共用一个 vault 吗？",
   "能——这就是设计意图。纯文件加一份 AGENTS.md，OpenClaw、Codex、Hermes 和 Claude 可以在同一个 vault 里干活。放进 git，每次写入都可追溯、可回滚。"),
 ],
},
# ----------------------------------------------------------------- guides
{
 "path": "/guides/share-on-cloudflare/",
 "title": "用 note.md 在 Cloudflare 上免费分享文档 —— 你自己的 worker，你自己的链接",
 "desc": "十分钟把 note.md 的分享 worker 部署到 Cloudflare 免费档。把任何 markdown 发布成漂亮的自包含页面——数学公式、图表、深色模式——跑在你掌控的基础设施上。",
 "crumb": "指南",
 "h1": "在你自己的 Cloudflare 上免费分享",
 "lead": "Cmd+Shift+L 把文档发布成网页——KaTeX、Mermaid、深色模式、移动端就绪。转折在这里：它发布到你的 Cloudflare 账号，不是我们的。免费档轻松扛下个人负载。",
 "sections": [
  ("为什么要自托管分享", """<p>你按过的每一个“分享”按钮，都把你的文档传到了别人的服务器上，守着别人的条款，活到别人规定的日子。note.md 的分享插件往<em>你的</em> Cloudflare 账号里部署一个小 Worker：你的链接，你的数据，你的总开关。免费档（每天 10 万请求）远超一个人分享文档所需。</p>"""),
  ("十分钟部署", """<pre><code>cd worker
pnpm install
wrangler login
wrangler kv:namespace create SHARES     # copy the id into wrangler.toml
openssl rand -hex 32 | wrangler secret put SHARE_API_KEY
wrangler deploy                          # prints your Worker URL</code></pre>
<p>把 Worker URL 和 API key 粘进 <b>note.md → Preferences → Share</b>，重启，完事。完整细节见仓库里的 <code>worker/README.md</code>。</p>"""),
  ("你会得到什么", """<ul>
<li><b>一个快捷键：</b><code>Cmd+Shift+L</code> 发布当前文件；URL 直接进剪贴板。再分享一次即原地更新；取消分享后返回 410。</li>
<li><b>忠实渲染：</b>KaTeX 数学公式、Mermaid 图表转 SVG、语法高亮、跟随 <code>prefers-color-scheme</code> 的明暗模式、移动端优化。</li>
<li><b>图片也带上：</b>图多的文档会自动溢出到 Cloudflare R2（同样有免费档）。</li>
<li><b>为 agent 就绪：</b>Worker 暴露一个 MCP 端点，你的 agent 可以代你发布——<code>notemd -s draft.md</code> 在任何脚本里都能跑。</li>
</ul>"""),
 ],
 "faq": [
  ("这要花多少钱？",
   "个人使用一分不花。Cloudflare 免费档包含每天 10 万次 Worker 请求和 10GB 的 R2 存储——比一个人分享文档的用量高出几个数量级。"),
  ("分享出去的页面能撤下来吗？",
   "能，立刻。File → Unshare（或 notemd share --unshare）吊销链接；访客看到 410。Worker 是你的——你甚至可以直接把它删掉。"),
 ],
},
{
 "path": "/guides/vault-on-github/",
 "title": "在 GitHub 上免费托管 vault —— 给一个 markdown 文件夹版本历史和同步",
 "desc": "note.md 的 vault 是纯文件，所以 git 天生好使：GitHub 免费私有托管、完整版本历史、多设备同步，agent 的每次写入都可 diff、可回滚。",
 "crumb": "指南",
 "h1": "你的 vault 放上 GitHub，免费",
 "lead": "vault 是一个 markdown 文件夹。git 就是为文本文件夹造的。GitHub 免费托管私有仓库。三个事实相加，等于一套装得下一辈子笔记的零成本防弹基础设施。",
 "sections": [
  ("为什么 git 是 vault 的完美后端", """<p>数据库需要你总会忘记做的备份。同步服务需要订阅和信任。git 两样都不要：每次保存是一次提交，每次提交是一段历史，每次推送是一份异地备份。而在 agent 时代它双倍回本——<b>agent 写进你的 vault 时，git 让每次写入都可 diff、可追溯、可回滚。</b>agent 发挥失常的一天，就是一句 <code>git revert</code>，不是一场悲剧。</p>"""),
  ("配置", """<pre><code>cd ~/Vault
git init
printf '.DS_Store\\n.mdeditor/\\n' &gt; .gitignore
git add -A &amp;&amp; git commit -m "vault: day one"
gh repo create my-vault --private --source=. --push</code></pre>
<p>就这样。GitHub 私有仓库免费，历史无上限。之后想提交多勤都行——或者交给自动化。</p>"""),
  ("同步与自动化", """<ul>
<li><b>note.md 集成：</b>Sync-to-Vault 插件把文件以带日期前缀的名字拷进 git 同步的 vault，刷新时能感知冲突；最近文件历史通过 vault 在设备间镜像。</li>
<li><b>自动提交：</b>一行 cron 或 launchd 任务，每小时跑 <code>git add -A &amp;&amp; git commit -m "auto" &amp;&amp; git push</code>，就是不费力的持续备份。</li>
<li><b>多设备：</b>在第二台 Mac 上克隆仓库；写前 pull，写后 push。大纲文件很小，冲突罕见，真撞上了 git 会告诉你到底发生了什么。</li>
<li><b>agent：</b>给 agent 一个工作副本。像审同事的 PR 一样审它们的提交——因为它们现在就是同事。</li>
</ul>"""),
 ],
 "faq": [
  ("GitHub 私有仓库真的免费吗？",
   "真的——GitHub 免费计划包含无限私有仓库和完整历史。几十年的文本 vault 也就几兆字节。"),
  ("敏感笔记怎么办？",
   "vault 是你的：可以选私有仓库、自托管 Gitea，或者干脆不要远端——git 本地也能用。再谨慎一点，git-crypt 或 age 可以加密指定路径。"),
  ("我需要会 git 吗？",
   "会一点就够。三条命令覆盖日常（add、commit、push），note.md 的同步功能把大部分都藏了起来。回报——你写下的每个念头的完整历史——大得不成比例。"),
 ],
},
{
 "path": "/integrations/chatgpt-work/",
 "title": "用 note.md 搭配 ChatGPT（工作模式）—— 生成到一个属于你的 vault 里",
 "desc": "ChatGPT 的工作模式能连接你的文件夹，擅长起草和生图。把它指向一个 note.md vault，它做出来的一切就都成了你能阅读、批注、留存的 markdown。",
 "crumb": "集成",
 "h1": "note.md + ChatGPT（工作模式）",
 "lead": "ChatGPT 是大多数人手边最强的通才——擅长起草、总结、生成图片。note.md 给它的产物一个永久的家：你的 vault、你的文件，再加上你的判断。",
 "sections": [
  ("为什么这对搭档成立", """<p>ChatGPT 的工作模式能连接文件夹和文件，在流水线的生成一端最出彩：把粗略的大纲变成初稿、把一摞文档总结出来，以及——越来越多地——批量生成图片和图表。放任不管，这些产出就住在一条你迟早会弄丢的对话里。把它指向一个 note.md vault，每份成果就作为纯 markdown 落地（图片就在旁边），正好被你的阅读-批注循环接住。</p>
<p>这正是“一个 vault，多个 agent”开始回本的地方：ChatGPT 很少是你唯一的 agent。它是你伸手去<em>生产</em>时用的那个快通才——而审阅、长跑自动化、以及最终判断，都可以各自交给最擅长的那一位。同一批文件，不同的工人。</p>"""),
  ("配置", """<ol>
<li>把 vault 放在 ChatGPT 够得着的文件夹里——一个 OpenAI 连接的文件夹，或一个它能读写的云 / git 同步目录。</li>
<li>在 vault 根放一个 <code>AGENTS.md</code>（约定摘要见 <a href="/llms-full.txt">llms-full.txt</a>），并把同样的 house rules 贴进 ChatGPT 的项目指令——它不像 CLI agent 那样自动读文件，所以要主动告诉它。</li>
<li>让它把成果按日期存成 markdown，例如 <code>drafts/2026-07-23-launch-post.md</code>，生成的图片用相对链接放进 <code>{docname}_files/</code>。</li>
<li>在 note.md 里打开结果；阅读、高亮、发问——你的标记落进sidecar 的 <code>.note.md</code>，原文保持干净、可再生成。</li>
</ol>"""),
  ("循环跑起来是什么样", """<p>你让 ChatGPT 起草一篇发布稿并生成三张主图；它写出 <code>drafts/launch-post.md</code>，填满一个 <code>_files/</code> 文件夹。你在 note.md 里读它，删掉两张图，把一段夸大的话高亮，留一句批注。接着把 <code>launch-post.note.md</code> 交给一个更谨慎的审阅 agent——“照我的批注改”。ChatGPT 生成得快；vault 留住了它；你做了判断。这就是分工。</p>"""),
 ],
 "faq": [
  ("ChatGPT 会自动读 AGENTS.md 吗？",
   "不像 CLI agent（Codex、Claude Code）那样自动读。把 vault 约定贴进 ChatGPT 的项目或自定义指令，并把它指向 AGENTS.md 文件，它就会遵守同一套 house rules——sidecar 文件不许碰，新活儿写成带日期的 markdown。"),
  ("ChatGPT 生成的图片能住进我的 vault 吗？",
   "能。把它们和文档并排放进 {docname}_files/ 文件夹、用相对链接——和 note.md 处理粘贴截图是同一套约定。它们会在阅读视图里渲染，并随 vault 一起进 git。"),
  ("我必须只选一个 agent 吗？",
   "不必——这正是重点。快速生成用 ChatGPT，仔细审阅用另一个 agent，私密工作用本地 agent。它们通过文件协作，你来编排。见编排指南。"),
 ],
},
{
 "path": "/orchestrate-agents/",
 "title": "一个 vault，多个 agent —— 编排 Cowork、Codex、OpenClaw 与 ChatGPT（2026）",
 "desc": "你的 markdown vault 是一个给 agent 用的 git 仓库。Claude Cowork、Claude Code、Codex、ChatGPT、OpenClaw、Hermes 都能读写同一批文件——所以你按谁擅长什么、用哪个模型来派活儿，把判断留给自己。",
 "crumb": "指南",
 "h1": "一个 vault，多个 agent。你是编排者。",
 "lead": "没人提醒你的那种锁定，不是应用——是 agent。把知识放在纯文件里，就没有哪个 AI 拥有它。Cowork 起草，Codex 重构，ChatGPT 生成，本地 agent 看守你的秘密——而握笔的是你。",
 "sections": [
  ("vault 是中立地带", """<p>大多数 AI 工具想成为你思考的家：知识进它的数据库，批注进它的格式，agent 是它自带的那一个，模型是它锁定你的那一款。于是“用哪个 AI？”变成了“要不要把一切都搬走？”——你被圈进单一供应商的路线图里。</p>
<p>note.md 的 vault 把它翻了过来。vault 是一个带公共约定的纯 markdown 文件夹——一个讲 house rules 的 <code>AGENTS.md</code>、用来精确引用的 <code>((file#b-xxxxxx))</code> 块引用、装着<em>你的</em>判断的sidecar <code>.note.md</code>、以及构成单一命名空间的 <code>[[wikilinks]]</code>。这些约定是一套<b>公共协议</b>：任何 agent 都读得懂，不需要适配器。agent 和模型成了可替换的工人；唯一不变的是 vault。它是个 git 仓库，它们都往里 commit。</p>"""),
  ("谁擅长什么，就派谁", """<p>没有一个 agent 什么都最强。所以别让一个 agent 干所有事——搭一条流水线，把每个工具放到它最强的工位上：</p>
<table><thead><tr><th>环节</th><th>合适的人选</th><th>为什么</th></tr></thead><tbody>
<tr><td>夜间自动化</td><td>OpenClaw / Hermes</td><td>长跑、基于文件、自托管记忆</td></tr>
<tr><td>仔细审阅与修订</td><td>Claude Cowork / Code</td><td>推理强；动笔前先读你的批注</td></tr>
<tr><td>快速起草与生图</td><td>ChatGPT（工作模式）</td><td>通才式生成，批量出图</td></tr>
<tr><td>仓库内重构与脚本</td><td>Codex</td><td>原生 <code>AGENTS.md</code>，在工作目录里跑</td></tr>
<tr><td>最终判断</td><td>你</td><td>唯一没有模型能生成的东西</td></tr>
</tbody></table>
<p>你按活儿挑 agent <em>和</em>模型——便宜快模型做分诊，前沿模型做推理，本地模型处理一切私密内容。vault 不在乎用哪个；它只负责保管它们相互传递的文件。</p>"""),
  ("一个真实的循环", """<p>这是一条真实的流水线，四个工具、三种模型，跑在一个 vault 上：</p>
<ol>
<li><b>OpenClaw</b> 夜里长跑，把一批原始笔记处理成 <code>drafts/*.md</code>。</li>
<li>你把这些初稿交给用谨慎模型的 <b>Claude Cowork</b>——“审阅并修订这些，把不牢靠的地方标出来”。</li>
<li><b>ChatGPT</b> 批量生成主图，放进每篇文档的 <code>_files/</code> 文件夹。</li>
<li>成稿落进 note.md，在那里<b>你</b>阅读它们，砍掉夸大的部分，高亮要紧的地方，留下只有你写得出的批注。</li>
</ol>
<p>四个工具、三种模型、一个 vault、一个编排者。没人需要共享记忆或说某家的私有协议——它们在磁盘上传递 <code>.md</code> 文件，而你的sidecar <code>.note.md</code> 批注，就是给下一位的转向信号。</p>"""),
  ("为什么靠文件才行得通", """<ul>
<li><b>反锁定，再深一层。</b>file-over-app 把你从应用里解放出来；这一条把你从 agent 和模型里解放出来。今天最强的模型下个月就被换掉——你的知识不该跟着它走。</li>
<li><b>不靠平台的协作。</b>agent 在磁盘上传递 <code>.md</code>——没有共享内存，没有私有 API，没有插件商店。一个 agent 的产出就是下一个的输入。</li>
<li><b>你始终在环内,守在检查点。</b>它不是一头跑到尾的黑箱;而是一条有人守在质检口的多工位流水线。agent 写、审、配图——拍板发布的是你。</li>
<li><b>仍然只是文件。</b>没有编排数据库，没有隐藏状态。规则在 <code>AGENTS.md</code>，产出在 <code>.md</code>，判断在 <code>.note.md</code>——Obsidian、CLI、任何 agent 都能读。换掉 note.md，这个 vault 照样是所有人的公共工作区。</li>
</ul>
<p>把 vault 放进 <a href="/guides/vault-on-github/">git</a>，每一次 agent 写入都可 diff、可归属、可回滚——agent 的糟糕一天，是一次 <code>git revert</code>，而不是一场事故。</p>"""),
  ("动手搭起来", """<ol>
<li>在 vault 根放一个 <code>AGENTS.md</code>——从 <a href="/llms-full.txt">llms-full.txt</a> 取来约定，再加上 house rules（最硬的一条：agent 绝不写进 <code>*.note.md</code> sidecar 文件）。</li>
<li>把每个 agent 都接到同一个文件夹上:<a href="/integrations/openclaw/">OpenClaw</a>、<a href="/integrations/cowork/">Cowork</a>、<a href="/integrations/codex/">Codex</a>、<a href="/integrations/chatgpt-work/">ChatGPT</a>、<a href="/integrations/hermes/">Hermes</a>。</li>
<li>在 note.md 里阅读并批注结果；让下一个 agent 先读sidecar 文件。循环在你的磁盘上闭合。</li>
</ol>"""),
 ],
 "faq": [
  ("不同的 AI agent 真的能共享一个 vault 吗？",
   "能——这就是设计。vault 是纯 markdown 加一个描述约定的 AGENTS.md。Claude Cowork、Claude Code、Codex、ChatGPT、OpenClaw、Hermes 都读写这些文件，所以你能把每项任务路由给最合适的 agent（和模型）。把 vault 放进 git，每次写入都可 diff、可回滚。"),
  ("agent 之间怎么把活儿交接给彼此？",
   "通过文件。一个 agent 把 markdown 写进 vault；下一个把它当输入读。你的批注住在sidecar 的 .note.md 文件里，充当转向信号——agent 在下一轮之前先读你的批注。不需要共享内存或私有协议。"),
  ("这需要专门的编排工具或 MCP server 吗？",
   "不需要。编排者是你，介质是文件系统。没有中枢数据库、没有隐藏状态——规则在 AGENTS.md，产出在 .md，判断在 .note.md。给偏好工具接口的 agent 准备的 Vault MCP server 在路线图上，但纯文件今天就能用。"),
  ("为什么不干脆用一个 AI 包办一切？",
   "因为没有一个 agent 什么都最强。夜间自动化、仔细审阅、快速生图、私密的本地工作、最终判断，是各有最佳工具的不同活儿。把它们分给各路专家——在你自己拥有的文件之上——胜过一个通才一把梭，还让你随时能换掉任何一个工人。"),
 ],
},
]
