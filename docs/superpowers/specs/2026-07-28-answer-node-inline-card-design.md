# 答复节点 + 正文内联卡片 + 人工采纳 设计

日期:2026-07-28
状态:已确认
前置:`docs/superpowers/specs/2026-07-27-annotation-qa-loop-design.md`(批注问答闭环,已随 v6.728.x 上线)

## 一句话

agent 的大段答复以「围栏包裹的 answer 节点」存进 `.note.md`;读源 md 时按需懒加载,在被批注段落下方显示可展开的 ✦ 卡片;**只有你点「采纳入正文」**,答复才作为干净 markdown 写进源文件。

## 背景与动机

问答闭环上线后,答复只活在 `.note.md` 大纲里:读原文时看不见,想用还得手动搬。本设计补齐最后一段动线——**读原文即见答复,一键采纳成正文**——同时守住已确立的红线:**agent 永不写源 `.md`**,源文件的每一次改动都由人落笔。

对应信念:
- **信念 3(agent 建议,你确认)**:卡片是建议,采纳是确认;写源文件的动作只有人能触发。
- **信念 2(文件高于应用)**:答复是 `.note.md` 里的纯文本围栏块,Obsidian/CLI/grep 都能读;采纳产物是干净 markdown,不留专有标记。
- **信念 1(判断是残余)**:采纳与否本身就是判断,记录在 `status::` 上。

## 1. 数据模型与文件协议(`.note.md`)

### 1.1 answer 节点

新增 `NodeSource` 值 `answer`,作为 **question 节点的子节点**。大段 markdown 以**代码围栏**包裹存在节点 content 里:

```
  - 判断力吗?
    type:: question
    status:: answered
    - ```markdown
      答复正文,可含任意 markdown:

      - 列表项
      甚至 key:: 这种看着像属性的行

      ```python
      code = "嵌套代码块也安全"
      ```
      ```
      type:: answer
      by:: claude-code
      answered:: 2026-07-28T14:22:00Z
```

- **围栏是 content 的一部分**(首行开围栏、末行闭围栏)。这样序列化只是原样写出,**字节级 roundtrip 稳定**(此前 roundtrip 不稳定已造成过数据损坏,不再冒险)。
- 取答复正文用 `answerBodyOf(node)`(剥掉首尾围栏行);程序化创建答复用 `wrapAnswerBody(body)`(按 CommonMark 规则选比正文内最长反引号串更长的围栏)。
- `type:: answer` / `by::` / `answered::` 属性行**跟在闭合围栏之后**。
- 一个 question 下**最多一个** answer 节点(重复作答就地覆盖)。

### 1.2 状态机扩展

`QuestionStatus` 增加 `adopted`:

```
open ──(agent 作答)──> answered ──(人采纳入正文)──> adopted
                          │                            
                          ├──(人裁决)──> closed
                          └──(人追问)──> open
```

`adopted` 与 `closed` 都表示「此问题不再需要你处理」,区别是 adopted 说明答复已进正文。二者都不再出卡片。

### 1.3 取代旧的答复形态

已上线协议里的「短答 `✦ ` bullet + 长答 `answers/` 文件」**统一收敛为 answer 节点这一种形态**,`answers/` 目录约定从协议中移除。理由:一种形态更易被 agent 正确实现,也避免长答散落成孤儿文件。

存量数据:特性上线仅一天、无实际已答数据,**不做迁移**。历史 `✦ ` bullet 会被解析成普通 manual 节点(内容不丢、不报错),不特殊处理。

## 2. 解析与序列化(围栏感知)

这是本设计唯一有真实解析风险的部分,规则必须精确。

### 2.1 进入 raw 模式

`parseOutline` 遇到 bullet,若其 content 匹配 `^(\`{3,})` (开围栏)→ 进入 raw 模式,记住围栏长度。

### 2.2 raw 模式内

逐行处理,**不做 bullet / 属性行识别**:

- **缩进剥离**:该行以 `contIndent`(节点缩进+2)开头则剥掉;否则最多剥掉 `contIndent.length` 个前导空格(容忍手改文件,同时保留答复内部的更深缩进,如嵌套列表)。
- **空行必须保留**为一条空的 content 行(markdown 段落间的空行是语义)。注意现有 parser 对空行是 `continue` 跳过的,raw 模式必须**先于**该跳过逻辑处理。
- 剥离后的行若是**闭合围栏**(只含反引号、长度 ≥ 开围栏、允许尾随空白)→ 追加该行后**退出 raw 模式**;其后的行恢复正常属性行解析。
- 文件在闭合前结束(围栏未闭合)→ 已消费的行留在 content 里,fail open,不报错。

### 2.3 序列化

`serializeOutline` 写多行 content 时:首行跟在 `- ` 后,其余行加 `${indent}  ` 前缀,**但空行写成空字符串**(不写只有空白的行)。这样 `''` → `''` → `''` 双向稳定;非空行 `foo` → `  foo` → `foo`。

对非 answer 节点无行为变化(现有 parser 从不产出含空行的 content)。

### 2.4 与同步管线的关系(关键)

`syncAutoItems` 的 `autoSequence` 会把所有非 manual/note/question 节点推进 LCS 序列。answer 节点不由源文档派生,一旦进入 LCS 必然匹配失败 → 被降级为 manual、`type:: answer` 丢失、agent 的成果被毁。**因此 `autoSequence` 必须排除 `answer`**,与 note/question 同列。这是本设计最容易踩空的一处,须有专门测试。

批注从源文档消失时,annotation 的 note/question 子节点降级为 manual(既有行为);answer 是 question 的**孙**节点,不在降级循环内,内容原样保留——符合「不丢 agent 成果」。

## 3. 答复索引与懒加载

### 3.1 锚定方式:按批注文本匹配,不用行号

卡片要贴在源文档中被批注的那一段下面。**不用 `line::`**——行号随编辑漂移,且 rich 模式下 PM 文档位置与源行号无稳定映射。

改用**批注文本**作为锚:PM 文档里 annotation mark 与 note_anchor 节点都带 `note` 属性,而 question 节点的 content 就是那段批注文本。这正是 `sync.ts` 修复配对时用的稳定身份。

同一文档内两条批注文本完全相同时,两处都会挂同一张卡片——罕见且无害,不额外处理。

### 3.2 索引派生(纯函数)

`deriveAnswers(tree)` → `AnswerEntry[]`:

```ts
interface AnswerEntry {
  noteText: string        // question 节点 content = 源文档里的批注文本
  status: QuestionStatus  // 只有 'answered' 会出卡片
  body: string            // 剥掉围栏的答复 markdown
  by?: string
  answeredAt?: string
  questionId: string      // 采纳后回写 status 用
}
```

### 3.3 懒加载(两层)

1. **索引懒加载**:只为**当前活动的主文档**加载其配套 `.note.md`(经 `noteHomeForRead` 解析落点 → 读盘 → `parseOutline` → `deriveAnswers`),不预加载整个 vault。按路径缓存;大纲已挂载同一 note 时直接用内存树(单一事实源,避免读到过期盘上内容);文件变更事件 / 采纳后失效重建。
2. **正文渲染懒加载**:卡片默认折叠,只渲染「✦ 答复」+ 摘要(答复首个非空行,截断)。**展开时才**把 `body` 渲染成 HTML(复用导出/分享用的同一 marked 渲染器 `renderMarkdownInline` 所在模块)。

## 4. 正文内联卡片(rich 模式)

- ProseMirror 插件(mdeditor 侧 `note-anno`,**不动 moraya-core**)扫描文档:对每个 annotation mark 运行末端 / note_anchor 节点,取 `note` 属性;若索引中有 `status === 'answered'` 的同文本条目 → 在**其所在顶层块之后**插入 `Decoration.widget`,DOM 为 `<div>` 块级卡片。
- 卡片:折叠态一行(`✦ 答复` + 摘要 + 展开箭头);展开态显示渲染后的 markdown + 两个按钮:**「采纳入正文」**、**「收起」**。
- 只有 `answered` 出卡片;`open` 只有 ⁇ 徽标(既有),`adopted` / `closed` 不出卡片。
- **source 模式不放卡片**(那里是原始 `{>>…<<}` 文本,保持所见即所得)。
- 卡片是 decoration,不进文档、不进历史、不影响 `serializeMarkdown` → **源文件字节不变**。

## 5. 采纳入正文(唯一改源 md 的动作)

点「采纳入正文」:

1. 用 moraya-core 的 `parseMarkdown(body)` 把答复解析成 PM 文档,内容包进 **blockquote 节点**,**插入到该块之后**——一次 transaction、一次撤销(⌘Z)即回退。用 schema 节点包裹而非给每行加 `> `,嵌套列表/代码块才能正确成块。
2. 插入的是**引用块形式的干净 markdown**:仍无 `✦`、无出处、无 HTML 注释标记,但用 `>` 让「后续补充进来的内容」与你原本的正文一眼可辨。(初版为无格式插入,实测看不出是补充,故改为引用块。)
3. 同步把 `.note.md` 里该 question 置 `status:: adopted`:
   - 大纲已挂载该 note → 改内存树 + `markDirty()`(走既有 intent-save 落盘);
   - 未挂载 → 读盘 → 解析 → 改 status → 序列化 → 写回(复用 `question-capture` 的写盘防线:hash 比对幂等、绝不用空内容盖非空)。
4. 卡片随之消失(状态不再是 answered)。

可逆性:源文件靠 ⌘Z / git;`.note.md` 的 `adopted` 可由你在大纲 chip 上拨回。

## 6. 大纲面板里的 answer 节点

大纲里 answer 节点若直接显示 content,首行会是 ```` ```markdown ```` ——难看且无用。改为渲染 `✦ ` + 答复首个非空行(截断),只读、不可编辑(auto 节点语义)。

## 7. 协议文本更新

`src-tauri/templates/AGENTS.md` + `website/public/llms.txt` + `llms-full.txt`:
- 答复写法改为 answer 节点(围栏 + `type:: answer` + `by::` + `answered::`),给出可逐字照抄的样例;
- 移除 `answers/` 目录约定(vault layout 与协议步骤两处);
- 硬规则新增一条:**`status:: adopted` 只能由人设置**(与 `closed` 同级),agent 只做 `open → answered`。

## 8. 组件边界

| 文件 | 职责 |
|---|---|
| `src/lib/outline/model.ts` | `answer` source、`adopted` status、`answerBodyOf` / `wrapAnswerBody` |
| `src/lib/outline/markdown.ts` | 围栏感知解析 + 空行安全序列化 |
| `src/lib/outline/sync.ts` | answer 节点排除出 LCS(防降级毁数据) |
| `src/lib/outline/answers.ts`(新) | `deriveAnswers(tree)` 纯函数 + `AnswerEntry` 类型 |
| `src/lib/note-anno/answers-store.svelte.ts`(新) | 按活动文档懒加载索引、缓存与失效 |
| `src/lib/note-anno/answer-card.ts`(新) | PM 卡片插件(widget 构建 + 展开渲染 + 按钮事件) |
| `src/lib/note-anno/adopt-answer.ts`(新) | 采纳:插入 PM slice + 回写 `adopted` |
| `src/components/outline/OutlineNode.svelte` | answer 节点的 ✦ 摘要渲染 |
| 协议三文件 | 见 §7 |

每个新文件单一职责、可独立测试;纯逻辑(解析/序列化/索引/围栏包裹)全部单测覆盖。

## 9. 验证

- **单测**:围栏解析 roundtrip(含空行、嵌套代码块、未闭合 fail open)、`autoSequence` 不吞 answer、`deriveAnswers` 过滤与剥壳、`wrapAnswerBody` 围栏长度、采纳后 status 迁移。
- **端到端**:手写一份含 answer 节点的 `.note.md` → 打开源文档 → 卡片出现 → 展开渲染 → 采纳 → 源 md 出现干净段落且 `.note.md` 变 `adopted` → ⌘Z 回退。
- **GUI 由用户实机验证**(卡片外观、展开、采纳、深色模式)。

## 10. 明确不做(YAGNI)

- 不迁移历史 `✦` bullet / `answers/` 文件;
- source 模式不出卡片;
- 不做「采纳到指定位置」(固定插在该块之后);
- 不做卡片内编辑答复(要改去大纲或让 agent 重答);
- 不自动采纳、不自动 close/adopt;
- 不预加载全 vault 索引。
