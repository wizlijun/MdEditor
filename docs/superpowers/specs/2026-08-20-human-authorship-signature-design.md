# 人写署名(Human Authorship Signature)设计

> 日期:2026-08-20 · 基准:`docs/okf-v0.2-format-constraints.md`(OKF v0.2 §5.2 / §7)
> · 前置审计:`docs/okf-v0.2-conformance-audit.md`(F4 / F5 / F6)
>
> 一句话:**把「这是人写的、是谁写的」从「靠 `generated` 缺席来推断」改成
> 元数据里的一句明话 —— 人工创建的每一份文档都签 `generated: { by: human:<id>, at }`。**

---

## 1. 为什么

信念 3 说「`✦` 代表 AI 写的、`●` 代表你想的」。这条线今天在**符号层**画得很清楚,
在**元数据层**却是单向的:

- `src-tauri/templates/AGENTS.md:86-92` 要求 agent **必须**签 `generated`、
  **绝不能**自签 `human:` —— AI 侧有硬义务;
- 人这一侧**没有任何对应义务**。全项目只有一处写出过 `human:` 署名:
  `src/lib/note-anno/adopt-answer.ts:48-49`(采纳答复入正文时写 `verified`)。

后果写在 `searchidx/src/origin.rs:200-215` 的注释里,很直白:一个没 frontmatter 的
手写笔记会被误判,而且默认**刻意不给 `Human`** —— 因为「忘了盖章的 AI 产物」和
「忘了盖章的人类笔记」从签名上根本分不开,而把默认设成 `human` 会让每一份
无署名的 AI 倾倒物落进最该被信任的那一档。

**缺席不是署名。** 只要人写的一侧也留下正向署名,这个死结才解得开:
`generated.by` 有 `human:` 前缀 = 你写的;有 `<producer>/<version>` = 机器写的;
两者都没有 = 真的没人认领。三态可分,而不是今天的两态混淆。

离开 note.md 同样成立 —— 这正是信念 2(文件高于应用)与信念 5(一个 vault,
多个 agent)要的:vault 换到 Obsidian、grep、或另一个 harness 里,
「哪些是我亲手写的」依然读得出来。

---

## 2. 范围

### 2.1 签(人工创建入口)

**主程序** —— 由人的一个动作直接产生的文档:

| 入口 | 代码 |
|------|------|
| ⌘N 新建 / EmptyState / File▸New | `src/lib/tabs.svelte.ts:90` → `src/lib/new-file.ts:7` |
| 快速笔记(托盘 / Cmd+Ctrl+M) | `src/lib/quick-note.svelte.ts:70` |
| 新建 `.note.md` 手记 | `src/lib/outline/create.ts:8` |
| 日期笔记 | `src/lib/outline/daily.ts:60` |
| wikilink 建页(vault 内) | `src/lib/outline/backlinks-io.svelte.ts:123` |
| wikilink 建页(vault 外) | `src/lib/outline/backlinks-io.svelte.ts:138` |
| 富文本里点 `[[链接]]` 建文件 | `src/components/RichEditor.svelte:404-405` |

**插件** —— 用户亲手敲进去的原始稿:

| 入口 | 代码 |
|------|------|
| idea-spark 的 idea 原文 | `plugins-src/idea-spark/src/lib/idea-doc.ts:9` |
| trace-source 的委托稿 | `plugins-src/trace-source/src/lib/inbox.ts:145` |

### 2.2 不签

| 不签的 | 为什么 |
|--------|--------|
| roam-import 导入页 | 导入是**搬运**不是撰写。签 `human:` 会谎报「你在 note.md 里写的」。归属由 `type`(`Daily Note`/`Wiki Page` → Human 档)表达即可 |
| ebook `book.md` | 源材料。而且 `origin.rs` 规则 2 先于规则 4:一签 `generated`,全部导入电子书会从 `Source` 静默滑到 `Derived`(该处注释已预警) |
| sync 镜像 | 逐字快照,不是新撰写。来源由伴生笔记的 `sources[]` 表达 |
| **既有文件的再保存** | 只在**创建**时签,绝不回填。一个装着 agent 答复节点的 `.note.md` 不该因为你保存了一次就获得文档级 `human:` 署名 |
| insights 报告 / decision-log 看板 | 机器产物。它们缺的是 `generated`(机器署名),是另一件事,见 §6 |

### 2.3 明确不在本次范围

- decision-log 的裁决签 `verified`(人工确认语义,不是创建语义)—— 审计 F9 的遗留项
- insights / decision-log 补机器侧 `generated` —— 审计 F6 的遗留项
- 存量文件批量迁移 —— 用户 2026-08-04 已决定:**旧数据一律不迁移**

---

## 3. 署名长什么样

```yaml
---
type: Note
title: 火星上的第一家咖啡馆
generated:
  by: human:bruce
  at: 2026-08-20T10:31:00.000Z
---
```

- `by` 用 §7 的 `human:<id>` 形式,`id` 由**现有** `humanActorId` 规则推出,不新造机制:
  git `user.email` 的 local-part → git `user.name` 的 slug → 系统用户名 → `local`。
  双端实现已存在(`src/lib/okf/actor.ts:32` / `src-tauri/src/okf/mod.rs:13`),
  由共享 fixture `scripts/fixtures/okf-human-id.json` 钉住,采纳答复已在用。
- `at` 是创建时刻的 ISO 8601。
- 走 `touchConceptFrontmatter` 的**只补缺失键**语义:已有 `generated` 一律不覆盖。

---

## 4. 关键设计决策

### 4.1 ⌘N 不能为了署名而变慢

`humanActorId` 要跑两次 `git config` 子进程。⌘N 是最热的路径,不能为它阻塞。

**方案**:`src/lib/okf/identity.ts` 已经有会话级缓存。在此之上:

- 应用启动时与 vault 根变更时**预热**缓存(`warmHumanActor()`);
- 新增**同步**取值器 `humanActorNow(): string | null`,返回已缓存值,未预热则 `null`;
- `newFile()` 保持同步,拿 `null` 就不签。

代价明说:启动竞态窗口内建的第一个文件可能不带署名。用「⌘N 卡两次子进程」换
「极窄窗口内偶尔漏签」是划算的 —— 而且预热在 App 初始化里,窗口几乎不存在。

### 4.2 插件怎么拿到身份 —— 扩 `host.vault.info`,不加新方法

插件跑在隔离 webview 里,零 Tauri IPC,只有 `window.notemd.request()`。
`src-tauri/src/plugin_runtime/host_api.rs:32-69` 的方法表里**没有**任何身份方法。

三个选项里选**扩展既有的 `host.vault.info`**,加一个 `author` 字段:

- 身份本来就是**从 vault 的 git 配置推出来的** —— 它字面意义上就是 vault info;
- 复用既有的 `vault.read` capability 门禁,不新增权限、不改 capability 表;
- **纯增字段**:老插件读不到就是 `undefined`,不炸。因此**不强制 bump
  `engines.notemd`**,插件按 `undefined` 优雅降级(不签),宿主发版后自然生效。

(否决的选项:新增 `host.okf.human_actor` 方法 —— 要动 capability 表、要新 token、
要 bump engines、要两段式发布,换来的语义和 `vault.info.author` 一模一样。)

### 4.3 纯函数保持纯

`newFileText` / `newOutlineFileText` / `newPageFileText` / `buildIdeaDoc` /
`buildRequestDoc` 都是可测的纯函数。署名**作为参数传进去**,不在函数内部
去 `await` 身份 —— 时间与身份都由调用方注入,测试才钉得住逐字输出。

### 4.4 顺手补掉那个硬伤

`src/components/RichEditor.svelte:405` 富文本模式点未存在的 `[[链接]]` 时
`writeTextFile(abs, '')` —— 建 0 字节 `.md`,没有 frontmatter、没有 `type`,
违反 OKF §4.1 唯一的硬约束,`pnpm okf:lint` 会报,索引里落进最低档 `Unlabeled`。
`backlinks-io.svelte.ts:138` 对同一件事早就走 `newPageFileText` 了。
两条路合并到 `newPageFileText`,硬伤和署名一起解决。

### 4.5 trace-source 的 frontmatter 要走枢纽

CLAUDE.md:「任何新的写入点都必须经 `src/lib/okf/concept.ts`」。
`inbox.ts:145` 今天是手拼字符串(还自己手写 YAML 引号转义)。要往里加**嵌套的**
`generated:` mapping,手拼只会把引号 bug 再翻一倍。按 idea-spark 的既有做法
vendor 一份 `concept.ts` 进去。

---

## 5. 读侧不用改

`searchidx/src/frontmatter.rs:18` 已经解析 `generated_by`,
`searchidx/src/origin.rs:160-163` 的规则 2 已经按 `human:` 前缀分档。
**读侧本来就在等这个署名,只是从来没人写过。** 本次只补写侧。

---

## 6. 验收

- 上述 9 个人工入口写出的文档,frontmatter 含 `generated.by` 且以 `human:` 开头;
- 同一批文档 `pnpm okf:lint` 零违规;
- `searchidx` 对它们 `derive()` 出 `Origin::Human`(经规则 2 而非规则 4);
- 既有 `.note.md` 再保存**不会**长出 `generated`;
- roam 导入页、`book.md` **不带** `generated`,且 `book.md` 仍是 `Origin::Source`;
- 富文本点 `[[新链接]]` 建出的文件带合规 frontmatter(不再是 0 字节);
- 老版本插件(读不到 `vault.info.author`)不报错,只是不签。
