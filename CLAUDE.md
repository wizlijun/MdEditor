# CLAUDE.md

note.md —— 为 AI-native 时代打造的 markdown 阅读器与编辑器。
一句话总纲:**读 AI 写的,留下你想的,留住只有你才写得出的字。**

## 产品主张(以此为准绳做设计决策)

### 五大核心信念

1. **AI 文字无限,注意力有限——判断才是残余。** 你真正读过、标注过的文档,才赢得了你的注意力;留在字里行间的判断/意图/私有事实/决定,是任何模型都生成不出的、最有价值的数据。note.md 把它留存,而非任其消失在滚动条里。
2. **文件高于应用(files over app)。** 每篇笔记是磁盘上的纯 `.md`:git 友好、可 grep、任何编辑器可开、五十年后仍可读。索引是派生数据,文件是唯一事实源。
3. **agent 是一等公民——它建议,你确认。** vault 的全部约定都是 agent 可读的纯文本;`✦` 代表 AI 写的、`●` 代表你想的。agent 写文档、可*建议*链接,但关系图只在**你**确认处生长——绝不自动串联,也绝不用 agent 垃圾灌满 vault。
4. **你的批注属于 vault,不属于路径。** 你读的文件常住在 vault 之外,路径在设备/工具间是脆弱的。一落笔批注,note.md 就把源文件镜像进 vault,批注获得稳定、git 版本化的宿主——换设备/移动/删除都不丢宿主。
5. **一个 vault,多个 agent,你是编排者。** vault 是所有 harness 的中立公共地带(像 git 仓库);Cowork、Code、Codex、ChatGPT Work、OpenClaw、Hermes 通过公共约定(`AGENTS.md`、块引用、`.note.md`)读写同一批文件。谁擅长什么、用哪个模型,你按活儿来派;人在关键处阅读、判断、定稿。反锁定从「文件层」推进到「agent/模型层」。

### 支撑性产品原则(`docs/` 外宣素材,权威表述)

- **关系只在人确认处生长** — 捕获与结网刻意分离;只有 `.note.md` 进关系系统,纯 `.md` 不结网。见 `docs/product-principle-relationships-only-grow-where-confirmed.md`。
- **你的批注属于 vault,不属于路径** — sync 镜像作批注宿主。见 `docs/product-principle-mirror-hosted-marks.md`。
- **一个 vault,多个 agent,你是编排者** — 见 `docs/product-principle-one-vault-many-agents-you-orchestrate.md`。

外宣落点:README(中/英)的「产品理念」段;官网 notemd.net(`website/`);`website/public/llms.txt` / `llms-full.txt`(给 agent 的公共约定)。
