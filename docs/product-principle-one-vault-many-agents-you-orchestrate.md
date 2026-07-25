# 产品原则:一个 vault,多个 agent,你是编排者

> 状态:产品原则 / 外宣素材
> 适用:note.md 的 agent 协作模型(AGENTS.md 约定 / 块引用 / 手记 / vault-as-workspace)

## 一句话

> 你的 vault 是所有 agent 的公共工作区——像一个软件项目的 git 仓库。Claude Cowork、Claude Code、Codex、ChatGPT Work、OpenClaw、Hermes……它们在同一个 vault 里读写同一批 markdown 文件;谁擅长什么、该用哪个模型,由你按活儿来派。没有哪一家 harness 拥有这个库,note.md 也不拥有——**你是编排者,vault 是中立地带。**

---

## 核心主张:vault 是中立地带,不属于任何一家 harness

大多数 AI 工具想成为你思考的家:知识进它的库、批注进它的格式、agent 是它自带的那一个、模型是它锁定的那一款。于是"用哪个 AI"变成了"搬不搬家"——你被单一供应商的能力边界和路线图圈住。

我们把这件事彻底翻过来:**agent 和模型是可替换的工人,vault 是不可替换的工地。**

- **vault(工地)**——一个 git 仓库那样的纯 markdown 文件夹。它有统一约定:`AGENTS.md` 讲规矩、`((file#b-xxxxxx))` 定位、`.note.md` 存人的判断、`[[wikilink]]` 结网。这些约定是**公共协议**,任何 agent 都读得懂。
- **harness(工人)**——Cowork、Code、Codex、ChatGPT Work、OpenClaw、Hermes,各有所长:有的擅长长跑自动化,有的擅长审阅推敲,有的擅长批量生图,有的擅长跑在你自己的机器上。它们来了又走,你随时换人、换模型。

**你按"谁擅长什么"编排一条流水线,agent 之间通过文件交接,人在关键处阅读、判断、下笔。**

一个真实的循环长这样:

> OpenClaw 夜里自动处理一批 md,产出初稿;你派 Cowork 的 Fable 模型去审阅、修订这些稿子;再交给 ChatGPT Work 批量配图;整条流水线产出的文档,由你亲手阅读、圈注、编辑定稿。**四个工具、三种模型、一个 vault,一个编排者。**

---

## 为什么这样更好

1. **反锁定,从文件层推进到 agent/模型层**
   file-over-app 让你不被某个应用锁住;这条原则让你不被某个 agent、某个模型锁住。今天最强的模型下个月就换了——你的知识不该跟着谁走。vault 不变,工人随便换。

2. **谁擅长什么,你就派谁**
   没有一个 agent 什么都最强。自动化长跑、严谨审阅、批量生图、本地私有运行——分别是不同 harness 的主场。把每段活儿交给它最擅长的工具,整体质量高过任何单一 agent 一把梭。

3. **协作通过文件发生,不靠某家的 API**
   agent 之间不需要共享内存、不需要私有协议、不需要谁家的插件商店——它们交接的是磁盘上的 md。一个 agent 的产出就是下一个 agent 的输入,人的批注(`.note.md`)是所有 agent 的转向信号。协议是公共的,所以谁都能接进来。

4. **人始终在环内,且在关键处**
   编排的不是"全自动黑箱",而是"多工位流水线,人守在质检口"。agent 写、agent 审、agent 配图,但**留下判断、拍板定稿的是你**——那几行只有你写得出的字,是整条流水线唯一不可再生的产物。

5. **仍然 file-over-app**
   这套协作没有中枢数据库、没有编排引擎的私有状态。规则在 `AGENTS.md`,产物是 `.md`,判断是 `.note.md`,一切人类可读、Obsidian/CLI/任何 agent 可直接解析。换掉 note.md,这个 vault 照样是所有 agent 的公共工作区。

---

## 可外宣的凝练版

> **一个 vault,多个 agent,你是编排者。**
> 你的 vault 像一个 git 仓库:Cowork、Codex、OpenClaw、ChatGPT Work、Hermes 在同一批文件上协作,谁擅长什么你就派谁、用哪个模型。没有哪个 agent 拥有你的知识——**它们是可替换的工人,你是那个下判断的人。**

> **One vault, many agents. You orchestrate.**
> Your vault is a git repo for agents: Cowork, Codex, OpenClaw, ChatGPT Work, and Hermes all work the same files, and you assign each job to whoever's best at it, on whatever model. No agent owns your knowledge — they're interchangeable workers, and you're the one who judges.

---

## 与既有原则的关系

这条原则是 **file-over-app** 与 **"agent 是一等公民——它建议,你确认"** 的自然延伸,把"反锁定"和"人在环内"从两个方向补全:

- **file-over-app** 说"用什么承载"——纯文件,不靠应用魔法。
- **agent 是一等公民** 说"agent 与人的关系"——agent 建议,人确认。
- **一个 vault,多个 agent** 说"多个 agent 之间的关系"——它们是同一工地上按长处分工的工人,人是编排者。

三条合起来是同一个立场:**知识归你,应用可换,agent 可换,模型可换,只有你的判断不可替代。**

## 工程落点(实现锚点)

- **公共协议**:`AGENTS.md`(vault 根,house rules)+ `((file#b-xxxxxx))` 块引用 + `.note.md` 手记 + `[[wikilink]]` 命名空间——任何 agent 免适配读写。
- **交接介质**:git 仓库(见 `docs/` 官网 guides/vault-on-github),每个 agent 的写入都可 diff、可归属、可 `git revert`。
- **外宣锚点**:官网 `/integrations/{openclaw,cowork,codex,hermes,chatgpt-work}/` 各讲一个 harness 如何接入同一 vault;llms.txt / llms-full.txt 暴露公共约定给所有 agent。
