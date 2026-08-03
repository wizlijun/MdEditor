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

## 格式约束:OKF v0.2(知识文档格式,规范性)

**`docs/okf-v0.2-format-constraints.md`** 是本项目的知识文档格式约束文档(Open Knowledge Format v0.2,Google Cloud 开放规范的中文整理版,SSOT 见文首链接)。凡产出/消费 OKF 知识包(Knowledge Bundle)——例如把 vault 内容导出为可分发的知识集合、或让 agent 按公共约定读写知识文档——**必须严格按该文档执行**,不要凭印象写字段;有疑问先读文档对应小节,再动手。

要点(细节以文档为准,此处只是索引):

- **生产者侧三条硬约束**:非保留 `.md` 必须有可解析的 YAML frontmatter;frontmatter 必须有非空 `type`;`index.md` / `log.md` 是保留文件名,不得当概念文档用且须遵循各自结构(§8/§9)。
- **消费者侧宽容义务**:缺可选字段、未知 `type`、未知附加键、断链、缺 `index.md` 都 **MUST NOT** 拒绝;裸 `verified` mapping 必须当单元素列表处理(§11)。
- **来源/信任/生命周期字段族**(`sources`、`generated`、`verified`、`status`、`stale_after`)全部可选,但缺失本身有含义;信任层级是派生值,不存储、也不是访问控制(§5)。
- **actor 统一格式**:`<producer>/<version>` / `human:<id>` / `process:<id>`;人工撰写或人工确认必须用 `human:` 前缀(§7)——这与信念 3「`✦` AI 写的、`●` 你想的」同源:人机署名不能混。
- **Attested Computation 红线**:agent 只能给声明的 `parameters` 提供*值*,**MUST NOT** 撰写或修改计算本身(§10.2)。

当前一致性状况与整改进度见 **`docs/okf-v0.2-conformance-audit.md`**(2026-08-03 全量审计 + 分期计划;第 1 步已落地)。

**写 `.md` 的硬规矩**:任何新的写入点都必须经 `src/lib/okf/concept.ts`(`CONCEPT_TYPE` 是 `type` 取值的唯一登记处),并用 `pnpm okf:lint <目录>` 或单测里的 `scripts/okf-lint-core.mjs` 自检;插件侧规范见 `docs/plugin-v2-development.md` §9.1。

与产品主张的关系:OKF 是「文件高于应用」和「一个 vault,多个 agent」的外部落点——纯 Markdown + YAML frontmatter、可 diff、可移植,正是 vault 对外交换知识时的公共约定。它约束的是知识包的交换格式,不改变 vault 内 `.md` / `.note.md` 的既有约定。
