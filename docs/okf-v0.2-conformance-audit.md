# OKF v0.2 一致性审计与整改建议

> 审计日期:2026-08-03 · 基准:`docs/okf-v0.2-format-constraints.md`(OKF v0.2)· 代码基线:`main` @ 735cacf
>
> 结论一句话:**note.md 在"宽容消费"一侧基本达标,在"生产"一侧几乎全线不合规——绝大多数由本程序写出的 `.md` 既没有 frontmatter,也没有 OKF 的必填 `type`;唯一合规的生产者是 decision-log 插件。**

---

## 0. 整改进度

**第 1 步(P0.1 + P0.2 + P2.3 的 lint)已完成**(2026-08-03):

| 项 | 落点 |
|----|------|
| 唯一 frontmatter 生产入口 | `src/lib/okf/concept.ts`(`CONCEPT_TYPE` 登记表 + `touchConceptFrontmatter` / `conceptFileText`) |
| 硬约束校验 | `scripts/okf-lint-core.mjs`(纯函数,单测与 CLI 共用)+ `scripts/okf-lint.mjs` + `pnpm okf:lint <目录>` |
| 已接线的写入点 | ⌘N 新建(`src/lib/new-file.ts`)、`.note.md`/日记/wikipage(`src/lib/outline/frontmatter.ts` 的 `type` + `outlineConceptType`)、vault 外建页(`newPageFileText`)、insights 报告(前端 `report.ts` + CLI `insights-report-core.mjs`)、roam-import、ebook-import `book.md`(`bookconf::book_frontmatter`) |
| 存量文件 | 不批量迁移;打开后**首次保存**时机会性补 `type`(只补缺失键,顺序不变) |
| 开发规范 | `docs/plugin-v2-development.md` §9.1(类型登记表 + 硬约束 + 自检命令) |

未做(按原计划排在后面):P0.3 迁移命令、P0.4 `index.md`/`log.md` 的读侧支持、P1 全部(actor / `verified` / `sources` / 分享元数据)、P2.1 导出插件、P2.2 Attested Computation。
下文的发现明细保留审计当时的状态,便于对照。

---

## 1. 审计范围与判定标准

### 1.1 什么算"受 OKF 约束"

OKF 约束的是**知识包(Knowledge Bundle)的交换格式**,不是应用的全部磁盘产物。本审计按三条判定纳入范围:

| 纳入 | 说明 |
| --- | --- |
| A. vault 内的概念文档 | 用户/agent 读写的 `.md`、`.note.md`——vault 本身就是"可分发的知识集合",这正是 OKF 的分发单位 |
| B. 程序/插件机器生成的文档 | roam-import、ebook-import、insights 报告、decision-log 看板、agent 答复——OKF v0.2 的全部新增字段族就是为"机器生成知识"设计的 |
| C. 对外导出/分享产物 | share/publish 出去的 HTML、未来的导出包 |

**明确排除**(派生数据,不是知识文档,file-over-app 信念 2 里"索引是派生数据"):

- `.notemd/` 下的一切(mirrors 元数据、outliner-folds.json、settings.json)
- 每文件夹的 `.notemd.json`(排序/置顶,`src/lib/folder-view.svelte.ts:293`)
- mdblock 的 `block.yaml`(`src/lib/blockio/yaml-rw.ts`)
- `.base` 文件(Obsidian Base 兼容格式,有自己的规范)
- 仓库自身的工程文档(`docs/`、`README`、`AGENTS.md`)——它们是本项目的源码资产,不是用户 vault 的知识包

### 1.2 方法

对 A/B/C 三类的**每一个写入点**做定位,逐点对照 OKF 的三条硬约束(§11)、字段族(§4/§5)、actor(§7)、链接(§6)、保留文件名(§8/§9)。证据一律给到 `文件:行`。

### 1.3 严重度分级

| 级别 | 含义 |
| --- | --- |
| **B(Blocking)** | 违反 OKF 硬约束(§11 三条)或消费者 MUST 规则——产物拿给任何 OKF 消费者都不合规 |
| **S(Should)** | 违反 SHOULD/约定,或语义错位(字段名撞车、actor 格式不合) |
| **O(Opportunity)** | 规范给了能力而我们完全没用(能力缺口,不是错误) |

---

## 2. 结论总表

| \# | 写入点 | 证据 | frontmatter | `type` | 判定 |
| --- | --- | --- | --- | --- | --- |
| 1 | 新建空白文档(⌘N) | `src/lib/tabs.svelte.ts:76-88` | ✗ 无 | ✗ | **B** |
| 2 | `.note.md` 伴生/大纲笔记 | `src/lib/outline/create.ts:6-9`、`frontmatter.ts:24-34` | ✓ title/created/updated | ✗ | **B** |
| 3 | wikipage 建页(vault 内) | `src/lib/outline/backlinks-io.svelte.ts:108` | ✓(同上) | ✗ | **B** |
| 4 | wikipage 建页(vault 外) | `src/lib/outline/backlinks-io.svelte.ts:123` | ✗ 只有 `# 标题` | ✗ | **B** |
| 5 | roam-import 导入页 | `plugins-src/roam-import/src/lib/roam-import/convert.ts:45` | ✓(同 2) | ✗ | **B** |
| 6 | ebook-import `book.md` | `plugins-src/ebook-import/backend/src/pipeline.rs:239` | ✗ 元数据在 `config.txt` | ✗ | **B** |
| 7 | insights 阅读数据报告 | `src/lib/insights/report.ts:75,106` | ✗ | ✗ | **B** |
| 8 | decision-log 看板/归档 | `plugins-src/decision-log/src/lib/board-io.ts:11-13,28,43` | ✓ | ✓ | 合规(见 F9) |
| 9 | sync 镜像文件 | `src-tauri/src/vault_sync/` | ✗(逐份快照) | ✗ | **B**(见 F10 权衡) |
| 10 | 分享导出 | `src/lib/plugins/share-baker.ts:118-119` | 主动剥除 | — | **S** |

消费侧(读)总体是**宽容**的,符合 §11:未知键往返保留(`src/lib/outline/frontmatter.ts` 用 `yaml` 的 `parseDocument`,只 set 三个键)、原始 frontmatter 字符串原样回写(`src/lib/outline/markdown.ts:32`)、缺字段不报错、未知 `type` 不影响渲染。**这一侧不需要大改**,只有三处细节要补(F13)。

---

## 3. 发现明细

### F1 — 概念文档普遍缺 `type`(级别 B,OKF §4.1/§11 条件 2)

`type` 是 OKF 里**唯一的必填 frontmatter 字段**。本项目所有生产路径都没写:

- `src/lib/outline/frontmatter.ts:24-34` `touchFrontmatter()` 只补 `title` / `created`,并刷新 `updated`——这是 `.note.md`、wikipage、roam 导入页三条路径共用的唯一 frontmatter 生成器,它决定了 vault 里绝大多数带 frontmatter 的文件长什么样。
- 因此 vault 中 `.note.md` 的典型 frontmatter 是 `title/created/updated`,合规性上等价于"没有 frontmatter"——OKF 消费者会判定为不合规文档(而按 §11 它不会拒绝,但也无法归类)。

**影响**:vault 无法作为 OKF bundle 分发;外部 agent 无法按 `type` 发现概念(这正是 §10.3"发现(按 `type`)"的入口),只能靠路径约定猜。

### F2 — 大量文档完全没有 frontmatter(级别 B,§11 条件 1)

- `src/lib/tabs.svelte.ts:76-88`:十个随机新建模板,统一是 `# 标题\n\n正文`,零 frontmatter。
- `src/lib/outline/backlinks-io.svelte.ts:123`:vault 外解析 wikilink 建页写的是 `# ${safe}\n`。
- `src/lib/insights/report.ts:75,106`:阅读数据报告以 `# 阅读数据 · <区间>` 开头,正文是 `## 时间段` / `## 链接` 等分节——**body 结构其实很 OKF(结构化标题、表格)**,唯独缺 frontmatter。
- `plugins-src/ebook-import/backend/src/pipeline.rs:239`:`input.md` 直接 copy 成 `book.md`,是 Calibre/OCR 的原始转换产物。

### F3 — 保留文件名 `index.md` / `log.md` 零处理(级别 B,§8/§9)

全仓 grep(`src`、`src-tauri`、`plugins-src`、`scripts`、`skills`)对 `index.md` / `log.md` **无任何命中**。后果两面:

- **消费侧**:导入一个真实 OKF bundle 时,`index.md` 与 `log.md` 会被当成普通概念文档展示(文件夹视图、大纲、关系索引一视同仁),而规范规定它们 **MUST NOT** 用作概念文档。
- **生产侧**:note.md 的"目录索引"能力今天由 `.notemd.json`(排序/置顶)和文件夹视图承担——那是**应用私有的派生数据**,离开 note.md 就没人看得懂;OKF 的 `index.md` 是等价能力的纯文本版本。变更历史同理:今天全靠 git 历史,而 OKF 的 `log.md` 是"离开 git 也读得懂"的版本。

### F4 — actor 字符串不符 §7(级别 S)

- `src/lib/outline/markdown.ts:48` 写 `by:: <answeredBy>`,实际值是 `claude-code`(见 `src/lib/outline/sync.test.ts:295`、`docs/superpowers/plans/2026-07-30-claude-agent-plugin.md`),**没有版本段**。§7 要求 agent 形如 `<producer>/<version>`,例:`reference_agent/gemini-2.5-pro`。
- vault 模板里给 agent 的示范也是裸名:`src-tauri/templates/AGENTS.md` 写 `by:: your-agent-name`——**这是外部 agent 抄写的样板,错的示范会被复制到每一个 vault**。
- 人类署名从未使用 `human:` 前缀。§7 明确"信任分级以 `human:` 前缀为键",没有前缀 ⇒ 所有人工内容在 OKF 眼里最多是 machine-confirmed,永远拿不到 human-reviewed 层级。

这一条与产品信念 3(`✦` AI 写的、`●` 你想的)**同构**:note.md 已经在符号层区分人机,但没有在**元数据层**用规范格式表达,外部工具读不到。

### F5 — 人工确认没有落成 `verified`(级别 S,§5.2/§5.3)

`src/lib/note-anno/adopt-answer.ts:44` 人工"采纳入正文"时只写 `status:: adopted`,**不记录是谁、什么时候**。这正是 OKF `verified: [{by: human:<id>, at: <ISO8601>}]` 的语义,也是 note.md 最有价值的数据(信念 1:"你确认过的判断")。今天这条信息只以一个枚举值存在,离开 note.md 就丢失了主体与时间。

### F6 — 生成署名缺 `generated`(级别 S,§5.2)

`created` / `updated`(`src/lib/outline/frontmatter.ts:31-33`)只有时间没有主体,而且 `updated` 是**每次 touch 都刷新**,与 §5.2"`generated.at` 指内容最后一次实质变更"的语义不同(我们的 `updated` 包含非实质改动)。`generated.by` 在文档级完全缺席——哪怕整份文件是 roam-import 转换来的、或由 agent 生成,也看不出来。

### F7 — `status` 语义撞车(级别 S,§5.4)

`src/lib/outline/markdown.ts:4,43` 的 `status::` 取值是 `open|answered|adopted`(问答闭环状态,**节点级**);OKF 的 `status` 是 `draft|stable|deprecated`(生命周期,**文档级**)。目前分处两个命名空间(`::` 属性 vs frontmatter),暂不冲突;但一旦文档级引入 OKF `status`,同名不同义会长期误导人和 agent。需要在开发规范里显式钉死边界。

### F8 — `sources` 全线缺失(级别 S,§5.1)

三处本该有来源却没有的地方:

1. **sync 镜像**:镜像文件的源路径存在 `.notemd/mirrors/<deviceId>` 的 JSON 里(信念 4 的实现),文档本身不带来源。OKF 的 `sources[].resource` 正是为此设计的。
2. **ebook-import**:原书文件名、`creator`、`publisher`、`language` 被写进 `plugins-src/ebook-import/backend/src/bookconf.rs:56-90` 的 `config.txt`(`key=value` 自定义格式),而不是 `book.md` 的 frontmatter——一份 markdown 文档旁挂一个只有本插件看得懂的元数据文件,恰好是 OKF 想消灭的形态。
3. **roam-import**:只保留 `roam-uid` 之类的键,不记"这份内容来自哪个 Roam 导出包、导出于何时"。

### F9 — decision-log 是唯一合规生产者,但字段族空白(级别 S)

`plugins-src/decision-log/src/lib/board-io.ts:28,43` 写出 `type: decision-board` / `type: decision-archive`,**满足 §11 全部三条硬约束**,读侧也宽容(`parseBoard` 遇缺字段返回 `[]`)。差距只在:无 `title`/`description`(RECOMMENDED)、无 `generated`/`verified`(裁决明明是人做的)、无 `stale_after`(`check-date` 已经是同义信息,却用了自定义键)。**建议把它作为其余插件的样板去补齐,而不是另起炉灶。**

### F10 — 分享导出主动剥除 frontmatter(级别 S)

`src/lib/plugins/share-baker.ts:118-119` 在生成分享 HTML 前 `body.replace(/^---\n[\s\S]*?\n---\n?/, '')`。对 HTML 渲染这是对的(不该把 YAML 当正文渲染),但意味着**对外产物零元数据**:没有来源、没有生成/验证署名、没有生命周期。OKF 的"可移植"目标在最外层断掉了。

### F11 — 链接形态不是 OKF 链接(级别 S,§6,有取舍)

vault 用 `[[wikilink]]` 与 `((block-ref))`,OKF §6 规定概念间关系用**标准 Markdown 链接**,推荐以 `/` 开头的 bundle 绝对路径。这不是 bug——wikilink 是 Obsidian 生态兼容的刻意选择(见 `docs/` 与项目惯例"约束放写入端,wikilink 只按文件名解析")。但它是**导出时必须转换**的一环:通用 OKF 消费者不解析 `[[...]]`。

### F12 — 无 bundle 概念、无 `okf_version`(级别 O,§8/§12)

vault 根没有 `index.md`,自然也没有 `okf_version: "0.2"` 的声明位置;没有任何"把 vault 的一个子树打包成 bundle"的能力。今天分享的最小单位是**单文件 HTML**,没有"一组相关概念"的分发单位。

### F13 — 消费侧三处待补(级别 S/O,§11)

审计确认消费侧总体宽容,但有三处需要处理:

1. **裸 `verified` mapping**:§11 规定消费者 **MUST** 把不带列表短横的 `verified:` mapping 当作单元素列表。我们今天不读这个字段,等实现读取时必须一并做,否则一上来就违反 MUST 规则。
2. **空 frontmatter 不被识别**:`src/lib/outline/markdown.ts:7` 的 `FM_RE = /^---\r?\n([\s\S]*?)\r?\n---(\r?\n|$)/`,对 `---\n---\n`(空 mapping)不匹配,会把它当正文里的分隔线。OKF 文档不会出现空 frontmatter(必须有 `type`),但作为容错这属于"应当兼容"。
3. **嵌套/流式映射只读**:`src/lib/frontmatter-view.ts:77-86,96`,非标量值(如 `generated: { by: …, at: … }`、`sources:` 列表)渲染为只读——**这是正确且安全的**(不会破坏结构),但一旦我们开始写 OKF 字段族,用户在富文本模式下就编辑不了它们。属于后续 UI 工作,不是合规问题。

### F14 — Attested Computation 完全空白(级别 O,§10)

规范里最重的一块能力我们一点没用,而 note.md 里已经存在两处"数字"需要可证明性:

- decision-log 的信心值/命中率记分牌(`starOf`、`confLabel`)——数字由插件内代码算出,文档只留结果;
- insights 的阅读时长/价值统计(`src/lib/insights/value.ts`、`report.ts`)——同上。

这两处天然适合 `type: Attested Computation`:把计算本体写进 `# Computation`,消费方只绑定参数值(§10.2 红线:**agent MUST NOT 撰写或修改计算本身**)。这与信念 3"它建议,你确认"完全同向。

---

## 4. 整改方案

### P0 — 让本程序写出的每一份文档合规(消除全部 B 级)

**P0.1 建立唯一的 frontmatter 生产入口**

新增 `src/lib/okf/concept.ts`,所有写 `.md` 的路径必须经它:

```ts
export interface ConceptMeta {
  type: string                    // REQUIRED，§4.1
  title?: string
  description?: string
  resource?: string               // 绑定的外部资产（sync 镜像的源路径等）
  tags?: string[]
  generated?: { by: string; at: string }
  sources?: Array<{ id?: string; resource: string; title?: string; last_modified?: string }>
  status?: 'draft' | 'stable' | 'deprecated'
  stale_after?: string            // YYYY-MM-DD
  [extra: string]: unknown        // §4.1：生产者 MAY 加任意键
}

/** 生成/合并 frontmatter：已有键与顺序保留（往返安全），只补缺失项。 */
export function touchConceptFrontmatter(raw: string | null, meta: ConceptMeta, now?: string): string
```

实现要点:沿用 `src/lib/outline/frontmatter.ts` 已经做对的事——`yaml` 的 `parseDocument` + 只 `set` 缺失键,**未知键与顺序原样保留**(§4.1 往返要求),非 mapping 的 frontmatter 原样返回不破坏。现有 `touchFrontmatter` 收敛为它的薄封装。

**P0.2 给每个写入点定 `type`**

| 写入点 | 建议 `type` | 备注 |
| --- | --- | --- |
| `.note.md` 伴生/大纲笔记 | `Outline Note` | `title` 沿用现有值 |
| wikipage 建页 | `Wiki Page` |  |
| 日记 | `Daily Note` |  |
| 新建空白文档 | `Note` | 模板正文保持不变(那是产品调性) |
| roam-import 导入页 | `Outline Note` | 另加 `sources`(见 P1.3) |
| ebook-import `book.md` | `Book` | 元数据从 `config.txt` 迁入(见 P1.3) |
| insights 报告 | `Reading Report` | body 已经很结构化,只补头 |
| decision-log | 保持 `decision-board`/`decision-archive` | 建议改为首字母大写形式以与其余一致 |

§4.1 明确"类型值不做中心注册",所以取值我们自己定;但**必须在 `docs/plugin-v2-development.md` 里登记一张表**,避免每个插件各造一套。

**P0.3 迁移策略:只对新写入生效,旧文件不批量改写**

- 理由:项目既有惯例是"不做历史数据迁移";批量重写会把 vault 的 git 历史冲成一次巨型 diff,违背"精准改动"。
- 旧文件的补齐做成**机会性 + 可关闭**:仅当用户本来就要保存该文件、且 frontmatter 已存在时补 `type`;纯 `.md` 不主动加 frontmatter(否则等于给每个用户文件塞了应用私货,违背 file-over-app)。
- 提供一次性命令 `notemd okf migrate <dir> --dry-run`,由用户主动执行。

**P0.4 保留文件名的最小支持**

- 读侧:`index.md` / `log.md` **不进关系索引、不当概念**(与"关系只在人确认处生长"天然一致);文件夹视图把同目录 `index.md` 的正文作为该目录说明展示。
- 写侧:不自动生成——git 就是变更日志,自动写 `log.md` 属于"没被要求的功能"。只在 P2 的导出流程里生成。

### P1 — 语义对齐(消除 S 级)

**P1.1 actor 规范化(§7)**

```ts
export const actor = {
  human: (id: string) => `human:${id}`,
  agent: (producer: string, version: string) => `${producer}/${version}`,
  process: (id: string) => `process:${id}`,
}
```

- `by::` 的写入侧改为要求带版本:`claude-code/opus-5`;读侧继续接受裸名(宽容一致性,§11)。
- **同步改 `src-tauri/templates/AGENTS.md` 的样例**——它是外部 agent 的抄写模板,是传播力最强的一处。改完记得同步 `website/public/llms-full.txt`。

**P1.2 人工确认落成 `verified`(§5.2)**

"采纳入正文"(`src/lib/note-anno/adopt-answer.ts`)在写 `status:: adopted` 的同时,追加文档级 `verified: [{ by: human:<本机身份>, at: <ISO8601> }]`。身份取现有 deviceId/用户名配置即可(不引入账号体系)。这一步把信念 1 的"人的判断"变成规范化、可被任何 agent 读取的数据——**收益最高的一条**。

**P1.3 `sources` 落地(§5.1)**

- **sync 镜像**:优先写在**伴生 `.note.md`** 的 frontmatter 里(`sources[].resource` = 源绝对路径,`generated.at` = 镜像时间),**不改镜像文件本身**——镜像是源文件的快照,往里塞元数据会让"镜像 = 源"的心智模型破裂。若产品上希望镜像自带来源,则必须是显式设置项。
- **ebook-import**:`book.md` frontmatter 写 `type: Book` + `title`/`creator`→`sources[].author`/`resource`(原文件)/`language`;`config.txt` 保留(翻译流程还在用),但**元数据以 frontmatter 为准**。
- **roam-import**:`sources[].resource` 指向导出包文件名,`sources[].last_modified` 取导出时间。

**P1.4 `generated` / `status` / `stale_after`**

- 文档级补 `generated: { by, at }`:人工创建 → `human:<id>`;导入/生成 → 对应 producer。`created`/`updated` 保留(扩展键,§4.1 允许),不动现有行为。
- 文档级 `status` **只用 OKF 三值**;节点级 `status::` 保持问答语义不变,并在 `docs/plugin-v2-development.md` 写明"两个 status 不同义、不互转"。
- `stale_after` 先只给两处天然有到期语义的:decision-log 的 `check-date`、insights 报告的区间末尾。

**P1.5 分享导出保留元数据**

`share-baker` 剥除 frontmatter 后,把 `title`/`description`/`generated`/`sources` 注入 HTML 的 `<meta>`(或 JSON-LD),而不是丢弃。

### P2 — 能力新增(把规范用起来)

**P2.1 OKF 导出/导入**

- `notemd okf export <vault 子树> <目标目录>`:生成 `index.md`(带 `okf_version: "0.2"`,§8 规定只有 bundle 根的 index.md 可带 frontmatter,且只允许这一个键)、按 git 历史生成 `log.md`、**把 `[[wikilink]]` 与 `((block-ref))` 转成 bundle 绝对路径的 Markdown 链接**(§6)、缺 `type` 的文档按路径推断补齐。
- 导入方向:识别 `index.md`/`log.md`、按 `type` 分组展示。
- 形态建议:**独立插件**(`notemd.okf`),不进核心——核心只提供 `src/lib/okf/`。

**P2.2 Attested Computation 试点**

先在 decision-log 做一个:记分牌的命中率写成 `type: Attested Computation`(`runtime: js`、`parameters: [{name: period, type: string, required: true}]`、计算本体放 `# Computation`),插件只绑定参数、不生成计算。验证跑通后再考虑 insights。

**P2.3 `okf-lint`**

`scripts/okf-lint.mjs` + 对应 vitest:扫一个目录,报告三条硬约束的违反(缺 frontmatter / 缺非空 `type` / 保留文件名被当概念用),**只报告不修改**。同时给单测用的纯函数版本 `src/lib/okf/lint.ts`,让 P0 的每个写入点都能在测试里断言"我写出的文档过 lint"。

---

## 5. 明确不改(取舍与理由)

| 不改 | 理由 |
| --- | --- |
| vault 内继续用 `[[wikilink]]` / `((block-ref))` | Obsidian 生态兼容 + 既有 file-over-app 硬原则;OKF 链接形态只在**导出时**转换(P2.1) |
| 节点级 `::` 属性不迁进 frontmatter | Roam/Logseq 约定,是大纲笔记的核心数据结构;OKF 只管文档级 |
| 不批量迁移历史文件 | 项目既有惯例 + git 历史保护 |
| 派生数据(`.notemd/`、`.notemd.json`、`block.yaml`)不 OKF 化 | 它们不是知识文档,是索引/状态;信念 2 明确"索引是派生数据" |
| 不因缺字段拒绝任何文档 | §11 宽容一致性是 MUST;现状已符合,改动中必须守住 |
| 随机新建模板的正文不动 | 那是产品调性,只在其上加 frontmatter |

---

## 6. 验收标准

P0 完成的判据(全部可自动化):

1. `pnpm vitest run` 中新增用例:每个写入点的产物喂给 `okf/lint.ts` 全部通过;
2. `node scripts/okf-lint.mjs <测试 vault>` 零 B 级违反;
3. 往返测试:含 `sources`/`generated`/未知键的 frontmatter,经 `.note.md` 打开→保存后**逐字节不变**(§4.1 往返要求);
4. 宽容测试:`type: 未知类型`、未知附加键、断链、缺 `index.md` 的文档全部正常打开、不报错、不被过滤掉。

P1 的判据:`by::` 新写入全部匹配 `^(human:|process:|[^/]+/.+)`;人工采纳后 `.note.md` 出现 `verified` 且 `by` 带 `human:` 前缀。

---

## 7. 建议排期

| 阶段 | 内容 | 规模 | 风险 |
| --- | --- | --- | --- |
| 第 1 步 | P0.1 + P0.2 + P2.3 的 lint(先有尺子再改) | 中 | 低——只影响新写入 |
| 第 2 步 | P0.3 迁移命令 + P0.4 保留文件名 | 小 | 低 |
| 第 3 步 | P1.1 + P1.2(actor + verified) | 小 | 需同步改 AGENTS.md 模板与 llms-full.txt |
| 第 4 步 | P1.3 + P1.4 + P1.5 | 中 | sync 镜像那条要先定产品取舍 |
| 第 5 步 | P2.1 导出插件 | 大 | 独立插件,不阻塞主程序 |
| 第 6 步 | P2.2 Attested Computation 试点 | 中 | 探索性 |

**建议起点**:第 1 步的 lint + `src/lib/okf/concept.ts`。没有尺子之前做任何字段改造都只是换一种不合规。