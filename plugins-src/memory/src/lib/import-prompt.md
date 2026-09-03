# 任务：把你记住的关于我的内容，导出为 note.md 记忆主张

你是我正在使用的另一个 AI 助手。我在本机用 note.md 管理一个 vault，
它是我所有 AI 助手共用的长期记忆存放处。请把**你自己记忆系统里关于我的
条目**导出成 note.md 能直接导入的命令。

你只是在**提议**。每条都会以「待确认」进入我的 vault，必须我本人在
note.md 的记忆窗口里点确认才会生效。所以宁可多列几条让我删，也不要
替我省略。

---

## 第一步 · 如实列出

1. 打开你的持久化记忆（memory / 长期记忆 / 自定义指令 / 用户画像），
   **逐条**列出所有关于我的条目。不要抽样、不要归纳合并、不要润色。
2. 如果你没有记忆功能，或记忆是空的 —— 直接回答「没有可导出的记忆条目」
   然后停止。**不要**拿本次对话现编。
3. 分不清某条是「真的存在你的记忆里」还是「刚刚这次对话才知道的」——
   标为不确定，放进最后的「待我确认」清单，不要写进导出命令。

## 第二步 · 筛掉不该长期保存的

**必须删掉**：

- 一次性、临时的东西：某次任务、当天的待办、会话状态、聊天寒暄。
- 主语不是我的事实：同事、家人、客户、别人的项目 —— 即使它出现在
  我的对话里，也不属于我的记忆。
- 任何你从对话里推断、但我从未明确说过或确认过的内容，包括心理画像、
  性格判断和背景猜测。拿不准来源的放入末尾「待我确认」，不要生成命令。
- **任何凭据**：密码、API key、token、私钥、证件号、银行卡号、
  完整住址、门禁码。这些**绝不**出现在输出里，哪怕我以前告诉过你。
  遇到就整条丢弃，并在末尾只说一句「已丢弃 N 条含凭据的记忆」。

**保留**：身份、稳定偏好、行为边界、我已经作出的决定、长期承诺、
稳定做法、以及我明确要求你记住的背景事实。

## 第三步 · 拆成原子主张

一条主张 = 一个我可以单独同意或反对的判断。

- 「我用 TypeScript，讨厌 any，PR 要小」→ 拆成 3 条。
- 每条独立可读：不含「上面提到的」「他」「这个方案」这类回指。
- 带条件的必须写出条件：「写生产代码时用 X」，不是「用 X」。
- 用我平时跟你说话的语言写，不要翻译成英文。
- 一句话，不换行。

## 第四步 · 给每条打标

**scope 与 category**（决定它落在哪份投影里）：

| scope | category | 用于 |
|---|---|---|
| user | owner | 我是谁（姓名、账号、身份锚点） |
| user | identity | 身份、职业、所属、长期角色 |
| user | preferences | 口味、风格、工具、语言偏好 |
| user | work-style | 我的工作节奏与协作方式 |
| user | boundaries | 我不接受的做法 |
| memory | decisions | 我已经拍板的决定 |
| memory | constraints | 长期约束、限制条件 |
| memory | practices | 我稳定沿用的做法 |
| memory | context | 需要长期记住的背景事实 |

拿不准就用 other。

**claim-kind**（十选一）：
identity / preference / boundary / decision / belief /
observation / commitment / practice / material-fact / quotation

**其余标签**，按下表推导，不要随意发挥：

| 标签 | 取值 | 规则 |
|---|---|---|
| polarity | positive / negative / neutral | 「要这样做」→ positive；「别这样做」→ negative；纯事实 → neutral |
| risk-class | informational / behavioral / action-sensitive | boundary → action-sensitive；decision、practice、commitment → behavioral；其余 → informational |
| trust-tier | identity / stable-preference / contextual | identity 类 → identity；preference、boundary、practice → stable-preference；其余 → contextual |
| salience | normal / pinned | 只有我明确说过「务必记住 / 每次都要」的才 pinned |
| sensitivity | normal / private | 健康、家庭、财务、身份细节 → private |
| basis | owner-stated | 只导出我亲口说过或明确确认过的条目；推断内容不得生成命令 |
| space | global 或 project/名字 | 只在某个项目里成立的，写成 project/名字 |
| purpose | planning / writing / information-answer / projection / sync | 逗号分隔，可以多填。默认写 planning,writing,information-answer；只影响文风的可以只写 writing。绝不要填 external-action |
| provider-policy | deny / prompt / allow | sensitivity 为 private 的填 deny，其余填 prompt |

guidance 写「助手应该怎么用这条」，avoid-error 写「这条是为了防止哪种
具体错误」。凡 polarity 为 negative、或 claim-kind 是 boundary /
practice 的，这两项必填。

只有原记忆带有可信生效时间时，才为有时效的条目补完整 RFC 3339 UTC 时间，
例如 --valid-from "2026-09-02T00:00:00Z"；不知道准确时间就省略，不要猜。

## 第五步 · 输出

先给我一张速览表，再给**一个** bash 代码块，块里是每条一句的
notemd memory propose create。命令模板：

```bash
notemd memory propose create \
  --request-id "你的名字-memory-export/v1:今天的日期:英文短slug" \
  --recorded-by "产品名/模型名" \
  --scope user --category preferences --claim-kind preference \
  --text "写 TypeScript 时坚持 strict 模式，不用 any 兜底。" \
  --basis owner-stated \
  --polarity negative --salience normal --sensitivity normal \
  --trust-tier stable-preference --risk-class informational \
  --space global --purpose "planning,writing,information-answer" --provider-policy prompt \
  --guidance "生成 TS 代码时默认开 strict，类型难题给出具体类型。" \
  --avoid-error "用 any 或 @ts-ignore 绕过类型错误。"
```

硬性要求：

- --recorded-by 填**你自己**的产品名和模型名，例如 chatgpt/gpt-5、
  gemini/2.5-pro。**绝不**写成 human: 开头 —— 那个前缀只留给我本人，
  是区分「我想的」和「AI 写的」的唯一标记。
- --request-id 每条唯一且可重跑：同一条记忆重新导出时必须得到同一个 id，
  这样重复运行不会产生第二份。slug 用英文小写连字符。
- 所有值都用双引号包住。文本里不要出现英文双引号、反引号、美元符号和
  反斜杠；需要引号就用「」。
- 不要加 --vault，不要加 sudo，不要写循环或脚本，不要用 && 串联，
  不要输出除这个代码块之外的任何可执行内容。
- 提醒我在运行前逐条审阅 bash 代码块；不要声称生成的命令已经执行或安全。
- 不要调用 notemd memory approve / reject / delete —— 这些命令只接受
  我本人在 note.md 界面里的操作，你写了也会被拒绝。

最后附两段：

1. **待我确认**：你不确定是否真在记忆里的条目。
2. **一句话统计**：导出 N 条，丢弃 M 条（其中含凭据 K 条）。
