# 答复节点 + 正文内联卡片 + 人工采纳 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** agent 的大段答复以围栏包裹的 `type:: answer` 节点存进 `.note.md`;读源 md 时懒加载,在被批注段落下方显示可展开 ✦ 卡片;点「采纳入正文」才把干净 markdown 写进源文件。

**Architecture:** 全部改动在 mdeditor 前端 + 协议文本,**不动 moraya-core、不动 Rust**。答复 content 含围栏保证字节级 roundtrip;卡片是 ProseMirror decoration(不进文档、不改源文件);采纳用 moraya-core 的 `parseMarkdown` 插入 PM slice(一次撤销可回退)。

**Tech Stack:** Svelte 5 (runes)、TypeScript、Vitest、ProseMirror。测试 `pnpm test`,类型 `pnpm check`(基线 0 errors / 36 warnings)。

**Spec:** `docs/superpowers/specs/2026-07-28-answer-node-inline-card-design.md`

**背景知识(执行前必读):**
- `.note.md` 结构:annotation 父节点(带 `line::`)→ question 子节点(含问号的批注文本,带 `status::`)→ **本次新增** answer 孙节点(围栏包裹的大段 markdown)。
- `parseOutline` 逐行处理:bullet 行 `^((?:  )*)- (.*)$`;续行/属性行需 `contIndent = 节点缩进+2`;空行当前被 `continue` 跳过。
- `syncAutoItems` 的 `autoSequence` 把非 manual/note/question 节点推进 LCS;**answer 必须排除**,否则被降级为 manual、`type:: answer` 丢失(最易踩空的一处)。
- 徽标插件注册在 `src/components/RichEditor.svelte:976-984`(动态 import + `view.state.plugins.concat(...)`)。
- `renderMarkdownInline(md)`(`src/lib/plugins/host-render-html.ts:161`)是完整的 `marked.parse`,可渲染多段落 markdown。
- `parseMarkdown(md, schema)` 由 `@moraya/core` 导出,markdown → PM 文档。
- 每次 commit **只精确 add 本任务列出的文件,绝不 `git add -A`**。

---

### Task 1: 数据模型 — `answer` source、`adopted` status、围栏 helper

**Files:**
- Modify: `src/lib/outline/model.ts`
- Test: `src/lib/outline/model.test.ts`

- [ ] **Step 1: 写失败测试**(追加到 `model.test.ts` 末尾;所需符号合并进文件头部现有 `./model` import)

```typescript
describe('answer node helpers', () => {
  it('wrapAnswerBody fences a plain body', () => {
    expect(wrapAnswerBody('hello\nworld')).toBe('```markdown\nhello\nworld\n```')
  })

  it('wrapAnswerBody grows the fence past any nested backtick run', () => {
    const out = wrapAnswerBody('see:\n```python\nx = 1\n```\ndone')
    expect(out.startsWith('````markdown\n')).toBe(true)
    expect(out.endsWith('\n````')).toBe(true)
    // 嵌套的三反引号原样保留
    expect(out).toContain('```python')
  })

  it('answerBodyOf strips the fence lines', () => {
    const node = { content: '```markdown\nhello\n\nworld\n```' } as OutlineNode
    expect(answerBodyOf(node)).toBe('hello\n\nworld')
  })

  it('answerBodyOf round-trips wrapAnswerBody', () => {
    const body = 'a\n\n- x\n- y\n\n```js\nz\n```'
    const node = { content: wrapAnswerBody(body) } as OutlineNode
    expect(answerBodyOf(node)).toBe(body)
  })

  it('answerBodyOf returns content unchanged when there is no fence (fail open)', () => {
    const node = { content: 'no fence here' } as OutlineNode
    expect(answerBodyOf(node)).toBe('no fence here')
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/outline/model.test.ts`
Expected: FAIL — `wrapAnswerBody` / `answerBodyOf` 未导出。

- [ ] **Step 3: 实现** — `src/lib/outline/model.ts`

1. 类型扩展(文件头部):

```typescript
export type NodeSource = 'toc' | 'highlight' | 'wikilink' | 'annotation' | 'note' | 'question' | 'answer' | 'manual'
export type QuestionStatus = 'open' | 'answered' | 'closed' | 'adopted'
```

2. 文件末尾追加(放在 `treeHasQuestion` 之后):

```typescript
/** 开/闭代码围栏行(≥3 反引号)。答复正文整体被围栏包住,使任意 markdown
 *  (列表、`key::` 样式的行、嵌套代码块)对大纲解析器完全不透明。 */
const FENCE_OPEN_RE = /^(`{3,})/
const FENCE_CLOSE_RE = /^(`{3,})\s*$/

/**
 * 把答复正文包进自定界围栏。围栏长度 = 正文内最长反引号串 + 1(不少于 3),
 * 依 CommonMark 规则保证嵌套代码块不会提前闭合。
 */
export function wrapAnswerBody(body: string): string {
  let longest = 0
  for (const m of body.matchAll(/`+/g)) longest = Math.max(longest, m[0].length)
  const fence = '`'.repeat(Math.max(3, longest + 1))
  return `${fence}markdown\n${body}\n${fence}`
}

/** 取答复节点的正文(剥掉首尾围栏行)。无围栏时原样返回(fail open)。 */
export function answerBodyOf(node: Pick<OutlineNode, 'content'>): string {
  const lines = node.content.split('\n')
  const open = lines[0]?.match(FENCE_OPEN_RE)
  if (!open) return node.content
  const last = lines.length - 1
  const close = lines[last]?.match(FENCE_CLOSE_RE)
  const end = close && close[1].length >= open[1].length ? last : lines.length
  return lines.slice(1, end).join('\n')
}

/** 树中该 question 节点下的答复节点(至多一个) */
export function answerNodeOf(tree: OutlineTree, questionId: string): OutlineNode | undefined {
  return childrenOf(tree, questionId).find(c => c.source === 'answer')
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/outline/model.test.ts` → PASS,再跑全量 `pnpm test` 防回归。

- [ ] **Step 5: Commit**

```bash
git add src/lib/outline/model.ts src/lib/outline/model.test.ts
git commit -m "feat(outline): answer node source, adopted status, fence helpers"
```

---

### Task 2: 围栏感知解析 + 空行安全序列化

**Files:**
- Modify: `src/lib/outline/markdown.ts`
- Test: `src/lib/outline/markdown.test.ts`

- [ ] **Step 1: 写失败测试**(追加到 `markdown.test.ts` 末尾)

```typescript
describe('fenced answer nodes', () => {
  const sample = [
    '- 原文',
    '  type:: annotation',
    '  line:: 12',
    '  - 为什么?',
    '    type:: question',
    '    status:: answered',
    // 正文含 ```python,故外围栏必须更长(4 反引号)——这正是 wrapAnswerBody 的算法
    '    - ````markdown',
    '      第一段。',
    '',
    '      - 列表项',
    '      key:: 看着像属性但不是',
    '',
    '      ```python',
    '      x = 1',
    '      ```',
    '      ````',
    '      type:: answer',
    '      by:: claude-code',
    '      answered:: 2026-07-28T14:22:00Z',
    '',
  ].join('\n')

  it('parses the fenced block as one answer node', () => {
    const t = parseOutline(sample)
    const a = [...t.nodes.values()].find(n => n.source === 'answer')!
    expect(a).toBeDefined()
    expect(a.answeredBy).toBe('claude-code')
    expect(a.answeredAt).toBe('2026-07-28T14:22:00Z')
  })

  it('keeps list items, key:: lines, blank lines and nested fences inside the body', () => {
    const t = parseOutline(sample)
    const a = [...t.nodes.values()].find(n => n.source === 'answer')!
    const body = answerBodyOf(a)
    expect(body).toBe('第一段。\n\n- 列表项\nkey:: 看着像属性但不是\n\n```python\nx = 1\n```')
    // 围栏内的内容绝不产出额外节点
    expect([...t.nodes.values()].some(n => n.content === '列表项')).toBe(false)
  })

  it('round-trips the fenced answer byte-for-byte', () => {
    expect(serializeOutline(parseOutline(sample))).toBe(sample)
  })

  it('resumes normal parsing after the closing fence', () => {
    const t = parseOutline(sample)
    const q = [...t.nodes.values()].find(n => n.source === 'question')!
    expect(q.status).toBe('answered')      // 闭合围栏后的属性行仍被识别
  })

  it('fails open on an unclosed fence (content kept, no crash)', () => {
    const t = parseOutline('- ```markdown\n  未闭合\n')
    const n = [...t.nodes.values()][0]
    expect(n.content).toBe('```markdown\n未闭合')
  })

  it('a shorter nested fence does not close the outer fence', () => {
    // 外围栏 4 反引号、正文里的 ``` 只有 3 → 不构成闭合
    const md = '- ````markdown\n  a\n  ```\n  b\n  ```\n  c\n  ````\n  type:: answer\n'
    const t = parseOutline(md)
    const a = [...t.nodes.values()].find(n => n.source === 'answer')!
    expect(answerBodyOf(a)).toBe('a\n```\nb\n```\nc')
    expect(serializeOutline(t)).toBe(md)
  })

  it('parses status:: adopted', () => {
    const t = parseOutline('- q?\n  type:: question\n  status:: adopted\n')
    expect([...t.nodes.values()][0].status).toBe('adopted')
  })
})
```

(测试文件头部 import 需补 `answerBodyOf`,从 `./model`。)

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/outline/markdown.test.ts`
Expected: FAIL — 围栏内的行被当作 bullet/属性行拆散;`adopted` 不在 status 白名单。

- [ ] **Step 3: 实现** — `src/lib/outline/markdown.ts`

1. `serializeOutline` 的多行 content 写出(现为 `for (const cont of contentLines.slice(1)) lines.push(\`${indent}  ${cont}\`)`)改为:

```typescript
      // 空行写成空字符串(不写只含空白的行):答复正文里的段落空行必须双向稳定
      for (const cont of contentLines.slice(1)) lines.push(cont === '' ? '' : `${indent}  ${cont}`)
```

2. `parseOutline`:在 `const push = …` 之后、主循环之前加围栏状态:

```typescript
  // >0 表示正处在答复围栏内(raw 模式):逐行原样收,不识别 bullet / 属性行
  let fenceLen = 0
```

3. 主循环开头(在 `if (raw.trim() === '') continue` **之前**)插入 raw 模式分支:

```typescript
  for (const raw of body.split('\n')) {
    if (fenceLen > 0 && current) {
      // raw 模式:剥掉本节点的续行缩进(容忍手改文件:最多剥这么多前导空格),
      // 空行原样保留——markdown 段落间的空行是语义。
      const contIndent = '  '.repeat(currentDepth) + '  '
      const line = raw.startsWith(contIndent)
        ? raw.slice(contIndent.length)
        : raw.replace(new RegExp(`^ {0,${contIndent.length}}`), '')
      current.content += '\n' + line
      const close = line.match(/^(`{3,})\s*$/)
      if (close && close[1].length >= fenceLen) fenceLen = 0   // 闭合,回到常规解析
      continue
    }
    if (raw.trim() === '') continue
    // …以下保持原样
```

4. bullet 分支里,建节点后判断是否开围栏:

```typescript
    if (bullet) {
      current = push(bullet[1].length / 2, bullet[2])
      currentDepth = bullet[1].length / 2
      const open = bullet[2].match(/^(`{3,})/)
      if (open) fenceLen = open[1].length      // 进入 raw 模式
      continue
    }
```

5. `type` 白名单加 `'answer'`;`status` 白名单加 `'adopted'`:

```typescript
          if (key === 'type' && ['toc', 'highlight', 'wikilink', 'annotation', 'note', 'question', 'answer'].includes(value)) current.source = value as NodeSource
```

```typescript
          else if (key === 'status') {
            if (value === 'open' || value === 'answered' || value === 'closed' || value === 'adopted') {
              current.status = value
              if (current.source === 'manual' || current.source === 'note') current.source = 'question'
            }
          }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/outline/markdown.test.ts` → PASS;再 `pnpm test` 全量(既有 roundtrip 用例必须不回归)。

- [ ] **Step 5: Commit**

```bash
git add src/lib/outline/markdown.ts src/lib/outline/markdown.test.ts
git commit -m "feat(outline): fence-aware parsing and blank-line-safe serialization for answer nodes"
```

---

### Task 3: 同步管线保住 answer 节点(防降级毁数据)

**Files:**
- Modify: `src/lib/outline/sync.ts`
- Test: `src/lib/outline/sync.test.ts`

- [ ] **Step 1: 写失败测试**(追加到 `sync.test.ts` 末尾;`answerBodyOf`/`wrapAnswerBody` 从 `./model` 引入)

```typescript
describe('answer nodes survive re-derive', () => {
  const qMd = '正文 {==原文==}{>>为什么?<<}\n'
  const seedAnswer = (tree: ReturnType<typeof createTree>) => {
    const q = [...tree.nodes.values()].find(n => n.source === 'question')!
    const a = {
      id: 'ans-1', parentId: q.id, order: 100,
      content: wrapAnswerBody('因为前缀高度重复。\n\n- 要点一'),
      collapsed: false, source: 'answer' as const,
      answeredBy: 'claude-code', answeredAt: '2026-07-28T14:22:00Z',
    }
    tree.nodes.set(a.id, a)
    q.status = 'answered'
    return a
  }

  it('keeps the answer node and its props across a re-derive', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(qMd))
    seedAnswer(tree)
    syncAutoItems(tree, deriveAutoItems(qMd))    // 主文档重派生
    const a = tree.nodes.get('ans-1')!
    expect(a.source).toBe('answer')              // 绝不被降级为 manual
    expect(a.answeredBy).toBe('claude-code')
    expect(answerBodyOf(a)).toContain('因为前缀高度重复')
  })

  it('keeps the answer node even when the annotation is deleted from the main doc', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(qMd))
    seedAnswer(tree)
    syncAutoItems(tree, deriveAutoItems('正文没有批注了\n'))
    const a = tree.nodes.get('ans-1')!
    expect(a.source).toBe('answer')              // agent 的成果不因源文档改动而丢失
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/outline/sync.test.ts`
Expected: FAIL — answer 节点进了 LCS 序列、匹配不上 → `source` 变成 `'manual'`。

- [ ] **Step 3: 实现** — `src/lib/outline/sync.ts` 的 `autoSequence`(现第 26 行)排除 `answer`:

```typescript
      // note/question 子节点由 annotation 父节点管理;answer 由 agent 写入、不从源文档派生。
      // 三者都不参与 LCS——answer 一旦进入必然匹配失败并被降级为 manual,毁掉 agent 的答复。
      if (n.source !== 'manual' && n.source !== 'note' && n.source !== 'question' && n.source !== 'answer') out.push(n)
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/outline/sync.test.ts` → PASS;`pnpm test` 全量。

- [ ] **Step 5: Commit**

```bash
git add src/lib/outline/sync.ts src/lib/outline/sync.test.ts
git commit -m "fix(outline): keep answer nodes out of LCS so re-derive cannot demote them"
```

---

### Task 4: 答复索引纯函数 `deriveAnswers`

**Files:**
- Create: `src/lib/outline/answers.ts`
- Test: `src/lib/outline/answers.test.ts`

- [ ] **Step 1: 写失败测试** — Create `src/lib/outline/answers.test.ts`:

```typescript
import { describe, it, expect } from 'vitest'
import { deriveAnswers, answeredByNoteText } from './answers'
import { parseOutline } from './markdown'

const note = [
  '- 原文一',
  '  type:: annotation',
  '  line:: 3',
  '  - 为什么能到 90%?',
  '    type:: question',
  '    status:: answered',
  '    - ```markdown',
  '      因为前缀重复。',
  '      ```',
  '      type:: answer',
  '      by:: claude-code',
  '  - 还没答的问题?',
  '    type:: question',
  '    status:: open',
  '- 原文二',
  '  type:: annotation',
  '  line:: 9',
  '  - 已采纳的问题?',
  '    type:: question',
  '    status:: adopted',
  '    - ```markdown',
  '      旧答复。',
  '      ```',
  '      type:: answer',
  '',
].join('\n')

describe('deriveAnswers', () => {
  it('returns one entry per question that has an answer node', () => {
    const rows = deriveAnswers(parseOutline(note))
    expect(rows.map(r => r.noteText).sort())
      .toEqual(['为什么能到 90%?', '已采纳的问题?'].sort())
  })

  it('carries body (fence stripped), status and author', () => {
    const rows = deriveAnswers(parseOutline(note))
    const r = rows.find(x => x.noteText === '为什么能到 90%?')!
    expect(r.body).toBe('因为前缀重复。')
    expect(r.status).toBe('answered')
    expect(r.by).toBe('claude-code')
    expect(r.questionId).toBeTruthy()
  })

  it('skips questions with no answer node', () => {
    const rows = deriveAnswers(parseOutline(note))
    expect(rows.some(r => r.noteText === '还没答的问题?')).toBe(false)
  })

  it('answeredByNoteText only exposes status answered', () => {
    const map = answeredByNoteText(deriveAnswers(parseOutline(note)))
    expect(map.has('为什么能到 90%?')).toBe(true)
    expect(map.has('已采纳的问题?')).toBe(false)   // adopted 不再出卡片
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/outline/answers.test.ts`
Expected: FAIL — 模块不存在。

- [ ] **Step 3: 实现** — Create `src/lib/outline/answers.ts`:

```typescript
// src/lib/outline/answers.ts
// 从 .note.md 树派生「答复索引」:供正文内联卡片按批注文本锚定。
// 纯函数,无 IO —— 懒加载与缓存在 note-anno/answers-store 里。
import { childrenOf, answerBodyOf, type OutlineTree, type QuestionStatus } from './model'

export interface AnswerEntry {
  /** question 节点内容 = 源文档里那条批注的文本(锚定用的稳定身份) */
  noteText: string
  status: QuestionStatus
  /** 剥掉围栏的答复 markdown */
  body: string
  by?: string
  answeredAt?: string
  /** 采纳后回写 status:: adopted 用 */
  questionId: string
}

/** 树里每个「带答复节点的 question」派生一条索引 */
export function deriveAnswers(tree: OutlineTree): AnswerEntry[] {
  const out: AnswerEntry[] = []
  for (const q of tree.nodes.values()) {
    if (q.source !== 'question') continue
    const a = childrenOf(tree, q.id).find(c => c.source === 'answer')
    if (!a) continue
    out.push({
      noteText: q.content,
      status: q.status ?? 'open',
      body: answerBodyOf(a),
      by: a.answeredBy,
      answeredAt: a.answeredAt,
      questionId: q.id,
    })
  }
  return out
}

/** 批注文本 → 待你处理的答复。只含 answered:open 无答复、adopted/closed 已了结。 */
export function answeredByNoteText(entries: AnswerEntry[]): Map<string, AnswerEntry> {
  const m = new Map<string, AnswerEntry>()
  for (const e of entries) if (e.status === 'answered') m.set(e.noteText, e)
  return m
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/outline/answers.test.ts` → PASS。

- [ ] **Step 5: Commit**

```bash
git add src/lib/outline/answers.ts src/lib/outline/answers.test.ts
git commit -m "feat(outline): derive answer index keyed by annotation text"
```

---

### Task 5: 懒加载 store

**Files:**
- Create: `src/lib/note-anno/answers-store.svelte.ts`
- Test: `src/lib/note-anno/answers-store.test.ts`

- [ ] **Step 1: 写失败测试** — Create `src/lib/note-anno/answers-store.test.ts`:

```typescript
import { describe, it, expect, beforeEach } from 'vitest'
import { answersStore, setAnswersFromText, clearAnswers, answeredMap } from './answers-store.svelte'

const note = [
  '- 原文',
  '  type:: annotation',
  '  - 为什么?',
  '    type:: question',
  '    status:: answered',
  '    - ```markdown',
  '      因为如此。',
  '      ```',
  '      type:: answer',
  '',
].join('\n')

describe('answers store', () => {
  beforeEach(() => clearAnswers())

  it('starts empty', () => {
    expect(answersStore.entries).toHaveLength(0)
    expect(answeredMap().size).toBe(0)
  })

  it('parses note text into entries and bumps version', () => {
    const before = answersStore.version
    setAnswersFromText('/v/x.note.md', note)
    expect(answersStore.notePath).toBe('/v/x.note.md')
    expect(answersStore.entries).toHaveLength(1)
    expect(answeredMap().get('为什么?')?.body).toBe('因为如此。')
    expect(answersStore.version).toBeGreaterThan(before)
  })

  it('clearAnswers resets path and entries', () => {
    setAnswersFromText('/v/x.note.md', note)
    clearAnswers()
    expect(answersStore.notePath).toBeNull()
    expect(answersStore.entries).toHaveLength(0)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/note-anno/answers-store.test.ts`
Expected: FAIL — 模块不存在。

- [ ] **Step 3: 实现** — Create `src/lib/note-anno/answers-store.svelte.ts`:

```typescript
// src/lib/note-anno/answers-store.svelte.ts
// 正文内联答复卡片的数据源:只为「当前活动主文档」按需加载其配套 .note.md。
// 不预加载整个 vault(懒加载第一层);答复正文的 markdown 渲染在卡片展开时才做(第二层)。
import { parseOutline } from '../outline/markdown'
import { deriveAnswers, answeredByNoteText, type AnswerEntry } from '../outline/answers'
import { outline } from '../outline/store.svelte'
import { serializeDoc } from '../outline/store.svelte'
import { noteHomeForRead } from '../outline/note-home'

interface AnswersState {
  /** 索引来源的 .note.md 路径(采纳回写要用) */
  notePath: string | null
  entries: AnswerEntry[]
  /** 每次重建自增:宿主据此让 PM 插件重建卡片 decoration */
  version: number
}

export const answersStore = $state<AnswersState>({ notePath: null, entries: [], version: 0 })

/** 批注文本 → 待处理答复(只含 answered) */
export function answeredMap(): Map<string, AnswerEntry> {
  return answeredByNoteText(answersStore.entries)
}

/** 用一段 .note.md 文本重建索引(纯内存,供测试与内存树路径复用) */
export function setAnswersFromText(notePath: string, text: string): void {
  answersStore.notePath = notePath
  answersStore.entries = deriveAnswers(parseOutline(text))
  answersStore.version++
}

export function clearAnswers(): void {
  answersStore.notePath = null
  answersStore.entries = []
  answersStore.version++
}

/**
 * 为某个主文档加载答复索引。大纲已挂载同一 note 时直接用内存树(单一事实源,
 * 避免读到过期的盘上内容);否则按需读盘。任何失败都静默清空——卡片是增强,不是必需。
 */
export async function loadAnswersFor(mainPath: string | null | undefined): Promise<void> {
  if (!mainPath || !/\.md$/i.test(mainPath) || /\.notes?\.md$/i.test(mainPath)) { clearAnswers(); return }
  try {
    const { sotvaultStore } = await import('../sotvault.svelte')
    const notePath = noteHomeForRead(mainPath, {
      vaultRoot: sotvaultStore.vaultRoot,
      records: sotvaultStore.records,
    })
    if (!notePath) { clearAnswers(); return }
    if (outline.docPath === notePath) { setAnswersFromText(notePath, serializeDoc(false)); return }
    const fs = await import('@tauri-apps/plugin-fs')
    if (!(await fs.exists(notePath).catch(() => false))) { clearAnswers(); return }
    const text = await fs.readTextFile(notePath).catch(() => null)
    if (text == null) { clearAnswers(); return }
    setAnswersFromText(notePath, text)
  } catch (e) {
    console.warn('[answers] load failed:', e)
    clearAnswers()
  }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/note-anno/answers-store.test.ts` → PASS;`pnpm check` 无新增错误。

- [ ] **Step 5: Commit**

```bash
git add src/lib/note-anno/answers-store.svelte.ts src/lib/note-anno/answers-store.test.ts
git commit -m "feat(note-anno): lazy answer index store for the active document"
```

---

### Task 6: 卡片锚点计算(纯函数)

先把「卡片挂在哪」这段纯逻辑独立出来并测透,DOM 留到 Task 7。

**Files:**
- Create: `src/lib/note-anno/answer-sites.ts`
- Test: `src/lib/note-anno/answer-sites.test.ts`

- [ ] **Step 1: 写失败测试** — Create `src/lib/note-anno/answer-sites.test.ts`:

```typescript
import { describe, it, expect } from 'vitest'
import { Schema } from 'prosemirror-model'
import { collectCardSites } from './answer-sites'
import type { AnswerEntry } from '../outline/answers'

// 最小测试 schema:collectCardSites 只关心「文本上的 annotation mark」与「note_anchor 节点」。
// @moraya/core 的 createSchema 需要 MediaResolver,对纯函数单测过重,故本地造一个。
const schema = new Schema({
  nodes: {
    doc: { content: 'block+' },
    paragraph: { group: 'block', content: 'inline*' },
    text: { group: 'inline' },
    note_anchor: { group: 'inline', inline: true, atom: true, attrs: { note: { default: '' } } },
  },
  marks: {
    annotation: { attrs: { note: { default: '' } } },
    strong: {},
  },
})

const entry = (noteText: string): AnswerEntry => ({
  noteText, status: 'answered', body: 'body', questionId: 'q1',
})

function docWithAnnotation(note: string) {
  const anno = schema.marks.annotation.create({ note })
  return schema.node('doc', null, [
    schema.node('paragraph', null, [schema.text('前 '), schema.text('被批注', [anno])]),
    schema.node('paragraph', null, [schema.text('后一段')]),
  ])
}

describe('collectCardSites', () => {
  it('anchors a card just after the block holding the annotation', () => {
    const doc = docWithAnnotation('为什么?')
    const sites = collectCardSites(doc, new Map([['为什么?', entry('为什么?')]]))
    expect(sites).toHaveLength(1)
    // 插入点 = 第一段之后 = 第一个段落节点的 nodeSize
    expect(sites[0].pos).toBe(doc.child(0).nodeSize)
  })

  it('ignores annotations with no matching answer', () => {
    const doc = docWithAnnotation('没人答过?')
    expect(collectCardSites(doc, new Map([['别的问题?', entry('别的问题?')]]))).toHaveLength(0)
  })

  it('emits one site for a note_anchor node', () => {
    const doc = schema.node('doc', null, [
      schema.node('paragraph', null, [
        schema.text('句子'), schema.nodes.note_anchor.create({ note: '这样对吗?' }),
      ]),
    ])
    const sites = collectCardSites(doc, new Map([['这样对吗?', entry('这样对吗?')]]))
    expect(sites).toHaveLength(1)
  })

  it('does not duplicate a card when the annotation spans several text nodes', () => {
    const anno = schema.marks.annotation.create({ note: '为什么?' })
    const strong = schema.marks.strong.create()
    const doc = schema.node('doc', null, [
      schema.node('paragraph', null, [
        schema.text('a', [anno]), schema.text('b', [anno, strong]),
      ]),
    ])
    expect(collectCardSites(doc, new Map([['为什么?', entry('为什么?')]]))).toHaveLength(1)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/note-anno/answer-sites.test.ts`
Expected: FAIL — 模块不存在。

- [ ] **Step 3: 实现** — Create `src/lib/note-anno/answer-sites.ts`:

```typescript
// src/lib/note-anno/answer-sites.ts
// 算出「答复卡片挂在哪」:按批注文本(annotation mark / note_anchor 的 note 属性)
// 与答复索引匹配,把卡片锚在该批注所在顶层块之后。
// 不用 line:: —— 行号随编辑漂移,批注文本才是 sync 也在用的稳定身份。
import type { Node as PMNode } from 'prosemirror-model'
import type { AnswerEntry } from '../outline/answers'

export interface CardSite {
  /** 文档位置:该批注所在顶层块之后 */
  pos: number
  entry: AnswerEntry
}

export function collectCardSites(doc: PMNode, entries: Map<string, AnswerEntry>): CardSite[] {
  if (entries.size === 0) return []
  const sites: CardSite[] = []
  const seen = new Set<string>()
  doc.descendants((node, pos) => {
    let note: string | null = null
    let end = pos
    if (node.isText) {
      const mark = node.marks.find(m => m.type.name === 'annotation')
      if (!mark) return
      end = pos + node.nodeSize
      // 同一条批注跨多个文本片段(如中间有加粗)时只在末段出一次
      const after = doc.resolve(end).nodeAfter
      if (after && mark.isInSet(after.marks)) return
      note = mark.attrs.note as string
    } else if (node.type.name === 'note_anchor') {
      note = node.attrs.note as string
      end = pos + node.nodeSize
    }
    if (note == null) return
    const entry = entries.get(note)
    if (!entry) return
    const $end = doc.resolve(end)
    const insertPos = $end.depth >= 1 ? $end.after(1) : end
    const key = `${insertPos}\n${note}`
    if (seen.has(key)) return
    seen.add(key)
    sites.push({ pos: insertPos, entry })
  })
  return sites
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/note-anno/answer-sites.test.ts` → PASS。

- [ ] **Step 5: Commit**

```bash
git add src/lib/note-anno/answer-sites.ts src/lib/note-anno/answer-sites.test.ts
git commit -m "feat(note-anno): compute answer card anchor sites by annotation text"
```

---

### Task 7: 采纳动作(插入正文 + 回写 adopted)

**Files:**
- Create: `src/lib/note-anno/adopt-answer.ts`
- Test: `src/lib/note-anno/adopt-answer.test.ts`

- [ ] **Step 1: 写失败测试** — Create `src/lib/note-anno/adopt-answer.test.ts`:

```typescript
import { describe, it, expect } from 'vitest'
import { markAdoptedInText } from './adopt-answer'
import { parseOutline } from '../outline/markdown'

const note = [
  '- 原文',
  '  type:: annotation',
  '  - 为什么?',
  '    type:: question',
  '    status:: answered',
  '    - ```markdown',
  '      因为如此。',
  '      ```',
  '      type:: answer',
  '',
].join('\n')

describe('markAdoptedInText', () => {
  it('flips the question status to adopted', () => {
    const q = [...parseOutline(note).nodes.values()].find(n => n.source === 'question')!
    const out = markAdoptedInText(note, q.content)!
    expect(out).toContain('status:: adopted')
    expect(out).not.toContain('status:: answered')
  })

  it('keeps the answer node intact', () => {
    const out = markAdoptedInText(note, '为什么?')!
    const t = parseOutline(out)
    expect([...t.nodes.values()].some(n => n.source === 'answer')).toBe(true)
  })

  it('returns null when the question is not found (no write)', () => {
    expect(markAdoptedInText(note, '不存在的问题?')).toBeNull()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/note-anno/adopt-answer.test.ts`
Expected: FAIL — 模块不存在。

- [ ] **Step 3: 实现** — Create `src/lib/note-anno/adopt-answer.ts`:

```typescript
// src/lib/note-anno/adopt-answer.ts
// 「采纳入正文」——本特性里唯一会改源 .md 的动作,且只由人点击触发。
// 插入的是干净 markdown(无 ✦、无出处、无隐形标记):你确认过,它就是你的正文。
import type { EditorView } from 'prosemirror-view'
import { parseMarkdown } from '@moraya/core'
import { parseOutline, serializeOutline } from '../outline/markdown'
import type { AnswerEntry } from '../outline/answers'
import { answersStore, loadAnswersFor } from './answers-store.svelte'

/** 在一段 .note.md 文本里把指定问题置为 adopted。找不到该问题返回 null(不写盘)。 */
export function markAdoptedInText(noteText: string, questionContent: string): string | null {
  const tree = parseOutline(noteText)
  const q = [...tree.nodes.values()].find(n => n.source === 'question' && n.content === questionContent)
  if (!q) return null
  q.status = 'adopted'
  return serializeOutline(tree)
}

/** 把答复 markdown 作为干净正文插入到锚点块之后:一次 transaction,⌘Z 即回退。 */
export function insertAnswerIntoDoc(view: EditorView, entry: AnswerEntry, pos: number): void {
  const parsed = parseMarkdown(entry.body, view.state.schema)
  view.dispatch(view.state.tr.insert(pos, parsed.content).scrollIntoView())
}

/** 回写 .note.md 的 status:: adopted。大纲挂载着同一 note 时改内存树,否则读改写盘。 */
export async function markAdoptedOnDisk(entry: AnswerEntry): Promise<void> {
  const notePath = answersStore.notePath
  if (!notePath) return
  try {
    const { outline, bump, markDirty } = await import('../outline/store.svelte')
    if (outline.docPath === notePath) {
      const q = outline.tree.nodes.get(entry.questionId)
        ?? [...outline.tree.nodes.values()].find(n => n.source === 'question' && n.content === entry.noteText)
      if (q) { q.status = 'adopted'; bump(); markDirty() }
      return
    }
    const fs = await import('@tauri-apps/plugin-fs')
    const disk = await fs.readTextFile(notePath).catch(() => null)
    if (disk == null) return
    const next = markAdoptedInText(disk, entry.noteText)
    if (next == null || next === disk) return
    await fs.writeTextFile(notePath, next)
  } catch (e) {
    console.warn('[adopt] writeback failed:', e)
  }
}

/** 卡片按钮入口:插正文 → 回写状态 → 刷新索引(卡片随即消失)。 */
export async function adoptAnswer(
  view: EditorView, entry: AnswerEntry, pos: number, mainPath: string | null,
): Promise<void> {
  insertAnswerIntoDoc(view, entry, pos)
  await markAdoptedOnDisk(entry)
  await loadAnswersFor(mainPath)
}
```

注意:测试只覆盖 `markAdoptedInText` 这个纯函数(其余两个要 EditorView / Tauri fs,留给 GUI 验证)。`parseMarkdown` 静态引入自 `@moraya/core`(外部包,无环路风险);`pnpm check` 必须干净。

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/note-anno/adopt-answer.test.ts` → PASS;`pnpm check` 0 errors。

- [ ] **Step 5: Commit**

```bash
git add src/lib/note-anno/adopt-answer.ts src/lib/note-anno/adopt-answer.test.ts
git commit -m "feat(note-anno): adopt answer into the document and flip status to adopted"
```

---

### Task 8: 卡片 PM 插件(DOM + 展开渲染)

**Files:**
- Create: `src/lib/note-anno/answer-card.ts`
- Create: `src/styles/answer-card.css`(或并入既有样式文件——先查 `src/styles/` 现有组织再定,不要新建重复用途的文件)
- Modify: `src/lib/i18n/en.ts`、`zh.ts`、`ja.ts`、`de.ts`

无单测(纯 DOM/插件装配,逻辑已在 Task 6/7 测过),靠 `pnpm check` + GUI 验证。

- [ ] **Step 1: i18n 键**(四语言各 4 键;先看现有 `noteedit.*` 键区风格再插入)

en:
```typescript
  'answerCard.label': 'Answer',
  'answerCard.adopt': 'Insert into document',
  'answerCard.expand': 'Show answer',
  'answerCard.collapse': 'Hide answer',
```
zh:
```typescript
  'answerCard.label': '答复',
  'answerCard.adopt': '采纳入正文',
  'answerCard.expand': '展开答复',
  'answerCard.collapse': '收起答复',
```
ja:
```typescript
  'answerCard.label': '回答',
  'answerCard.adopt': '本文に取り込む',
  'answerCard.expand': '回答を表示',
  'answerCard.collapse': '回答を隠す',
```
de:
```typescript
  'answerCard.label': 'Antwort',
  'answerCard.adopt': 'In den Text übernehmen',
  'answerCard.expand': 'Antwort anzeigen',
  'answerCard.collapse': 'Antwort ausblenden',
```

- [ ] **Step 2: 插件实现** — Create `src/lib/note-anno/answer-card.ts`:

```typescript
// src/lib/note-anno/answer-card.ts
// 正文内联答复卡片:贴在被批注段落之后的块级 decoration。
// 卡片是 decoration —— 不进文档、不进撤销历史、不影响序列化,源文件字节不变。
import { Plugin, PluginKey } from 'prosemirror-state'
import { Decoration, DecorationSet } from 'prosemirror-view'
import type { EditorView } from 'prosemirror-view'
import type { Node as PMNode } from 'prosemirror-model'
import { collectCardSites } from './answer-sites'
import type { AnswerEntry } from '../outline/answers'
import { t } from '../i18n/store.svelte'

const answerCardKey = new PluginKey<DecorationSet>('answer-cards')
/** 宿主在答复索引变化后用它强制重建 */
export const ANSWER_CARDS_REFRESH = 'answer-cards-refresh'

interface CardDeps {
  getEntries: () => Map<string, AnswerEntry>
  onAdopt: (entry: AnswerEntry, pos: number, view: EditorView) => void
}

/** 答复首个非空行,作折叠态摘要 */
function summaryOf(body: string): string {
  const line = body.split('\n').find(l => l.trim() !== '') ?? ''
  return line.length > 60 ? line.slice(0, 60) + '…' : line
}

function buildCard(entry: AnswerEntry, pos: number, view: EditorView, deps: CardDeps): HTMLElement {
  const root = document.createElement('div')
  root.className = 'answer-card'
  root.contentEditable = 'false'

  const head = document.createElement('button')
  head.className = 'answer-card-head'
  head.type = 'button'
  head.innerHTML = `<span class="answer-card-sigil">✦</span><span class="answer-card-title">${t('answerCard.label')}</span>`
  const summary = document.createElement('span')
  summary.className = 'answer-card-summary'
  summary.textContent = summaryOf(entry.body)
  head.appendChild(summary)
  head.title = t('answerCard.expand')
  root.appendChild(head)

  const bodyEl = document.createElement('div')
  bodyEl.className = 'answer-card-body'
  bodyEl.hidden = true
  root.appendChild(bodyEl)

  let rendered = false
  head.addEventListener('mousedown', (e) => { e.preventDefault(); e.stopPropagation() })
  head.addEventListener('click', (e) => {
    e.preventDefault(); e.stopPropagation()
    const show = bodyEl.hidden
    bodyEl.hidden = !show
    head.title = show ? t('answerCard.collapse') : t('answerCard.expand')
    root.classList.toggle('open', show)
    if (show && !rendered) {
      rendered = true
      // 懒渲染:展开时才把答复 markdown 变成 HTML
      void import('../plugins/host-render-html').then(({ renderMarkdownInline }) => {
        const html = document.createElement('div')
        html.className = 'answer-card-md'
        html.innerHTML = renderMarkdownInline(entry.body)
        const actions = document.createElement('div')
        actions.className = 'answer-card-actions'
        const adopt = document.createElement('button')
        adopt.type = 'button'
        adopt.className = 'answer-card-adopt'
        adopt.textContent = t('answerCard.adopt')
        adopt.addEventListener('mousedown', (ev) => { ev.preventDefault(); ev.stopPropagation() })
        adopt.addEventListener('click', (ev) => {
          ev.preventDefault(); ev.stopPropagation()
          deps.onAdopt(entry, pos, view)
        })
        actions.appendChild(adopt)
        bodyEl.appendChild(html)
        bodyEl.appendChild(actions)
      })
    }
  })
  return root
}

function build(doc: PMNode, view: EditorView | null, deps: CardDeps): DecorationSet {
  if (!view) return DecorationSet.empty
  const decos = collectCardSites(doc, deps.getEntries()).map(({ pos, entry }) =>
    Decoration.widget(pos, () => buildCard(entry, pos, view, deps), {
      side: 1, key: `answer-card-${pos}-${entry.questionId}`,
    }),
  )
  return DecorationSet.create(doc, decos)
}

export function answerCardPlugin(deps: CardDeps): Plugin<DecorationSet> {
  let view: EditorView | null = null
  return new Plugin<DecorationSet>({
    key: answerCardKey,
    view(v) { view = v; return {} },
    state: {
      init: () => DecorationSet.empty,
      apply(tr, old, _oldState, newState) {
        if (tr.docChanged || tr.getMeta(ANSWER_CARDS_REFRESH)) return build(newState.doc, view, deps)
        return old
      },
    },
    props: {
      decorations(state) { return answerCardKey.getState(state) },
    },
  })
}
```

- [ ] **Step 3: 样式**(卡片视觉;放进 `src/styles/` 里合适的既有文件或新建后在入口引入——按现有组织办)

```css
/* 正文内联答复卡片:✦ = AI 写的,与手写内容视觉上要分得开 */
.answer-card {
  margin: 0.6em 0;
  border: 1px solid color-mix(in srgb, #b8860b 35%, transparent);
  border-left: 3px solid #b8860b;
  border-radius: 6px;
  background: color-mix(in srgb, #fff3bf 40%, transparent);
  font-size: 0.92em;
}
.answer-card-head {
  display: flex; align-items: center; gap: 6px;
  width: 100%; padding: 6px 10px;
  background: none; border: none; cursor: pointer; text-align: left;
  font-family: inherit; font-size: 0.95em; color: inherit;
}
.answer-card-sigil { color: #b8860b; }
.answer-card-title { font-weight: 600; }
.answer-card-summary { opacity: 0.7; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.answer-card-body { padding: 0 10px 8px 10px; }
.answer-card-md > :first-child { margin-top: 0; }
.answer-card-actions { display: flex; justify-content: flex-end; margin-top: 6px; }
.answer-card-adopt {
  padding: 3px 10px; border: none; border-radius: 5px; cursor: pointer;
  font-family: inherit; font-size: 12px; font-weight: 600;
  color: #fff; background: var(--accent-color, #4a80d4);
}
.answer-card-adopt:hover { filter: brightness(1.1); }
@media (prefers-color-scheme: dark) {
  .answer-card { background: color-mix(in srgb, #4d3f00 45%, transparent); border-color: color-mix(in srgb, #e3b341 35%, transparent); border-left-color: #e3b341; }
  .answer-card-sigil { color: #e3b341; }
}
```

- [ ] **Step 4: 检查**

Run: `pnpm check`(0 errors)+ `pnpm test`(全量绿)。

- [ ] **Step 5: Commit**

```bash
git add src/lib/note-anno/answer-card.ts src/styles/<改动的样式文件> src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(note-anno): inline answer card widget with lazy markdown rendering"
```

---

### Task 9: 接进 RichEditor(注册插件 + 懒加载 + 刷新)

**Files:**
- Modify: `src/components/RichEditor.svelte`

- [ ] **Step 1: 注册插件**(第 976-984 行那段动态 import + `plugins.concat`)

```typescript
          const { wikilinkPlugin } = await import('../lib/wikilink-plugin')
          const { noteBadgePlugin } = await import('../lib/note-anno/note-plugin')
          const { placeholderPlugin } = await import('../lib/placeholder-plugin')
          const { answerCardPlugin } = await import('../lib/note-anno/answer-card')
          const { answeredMap } = await import('../lib/note-anno/answers-store.svelte')
          const { adoptAnswer } = await import('../lib/note-anno/adopt-answer')
```

`concat(...)` 里加:

```typescript
                answerCardPlugin({
                  getEntries: () => answeredMap(),
                  onAdopt: (entry, pos, v) => { void adoptAnswer(v, entry, pos, tab?.filePath ?? null) },
                }),
```

(`tab` 用该组件内已有的 tab 引用名;若无则用现成的主文档路径变量。)

- [ ] **Step 2: 文档切换时懒加载索引 + 索引变化时刷新卡片**

在组件内加两个 effect(放在其它 `$effect` 附近;`untrack` 用法参照文件内既有写法):

```typescript
  // 切换文档:按需加载该文档的答复索引(不预加载整个 vault)
  $effect(() => {
    const p = tab?.filePath ?? null
    void import('../lib/note-anno/answers-store.svelte').then(m => m.loadAnswersFor(p))
  })

  // 索引变化(加载完成/采纳后失效重建)→ 让卡片插件重建 decoration
  $effect(() => {
    const v = answersVersion()          // 见下:读 store 的 version 建立依赖
    void v
    const view = editorView             // 组件内既有的 EditorView 引用名
    if (!view) return
    import('../lib/note-anno/answer-card').then(({ ANSWER_CARDS_REFRESH }) => {
      view.dispatch(view.state.tr.setMeta(ANSWER_CARDS_REFRESH, true))
    })
  })
```

其中 `answersVersion()` 需要静态引用 store 才能建立响应式依赖 —— 在 `<script>` 顶部静态 `import { answersStore } from '../lib/note-anno/answers-store.svelte'`,effect 里直接 `void answersStore.version`(不必包成函数)。**实现时以能真正建立 Svelte 依赖为准**,别为省事写成动态 import 导致不响应。

- [ ] **Step 3: 检查**

Run: `pnpm check` + `pnpm test`。

- [ ] **Step 4: Commit**

```bash
git add src/components/RichEditor.svelte
git commit -m "feat(editor): wire answer cards into the rich editor with lazy loading"
```

---

### Task 10: 大纲面板里的 answer 节点显示

**Files:**
- Modify: `src/components/outline/OutlineNode.svelte`

- [ ] **Step 1: 实现**

answer 节点若直接显示 content,首行是 ```` ```markdown ```` ——无用。改为显示 `✦ ` + 答复首个非空行(截断),只读。

在 `<script>` 里(`noteLike` 附近)加:

```typescript
  /** answer 节点:大纲里只显示 ✦ + 答复首个非空行(围栏原文无展示价值) */
  const answerSummary = $derived.by(() => {
    if (node.source !== 'answer') return null
    void outline.version
    const body = answerBodyOf(node)
    const line = body.split('\n').find(l => l.trim() !== '') ?? ''
    return '✦ ' + (line.length > 80 ? line.slice(0, 80) + '…' : line)
  })
```

(`answerBodyOf` 从 `../../lib/outline/model` 引入。)

模板里非编辑态 content span 的渲染改为:answer 节点渲染 `answerSummary` 纯文本、其余走既有 `InlineRender`。`editable` 保持不变(answer 不是 manual/noteLike,本就只读)。bullet 加 `class:src-answer={node.source === 'answer'}`,样式:

```css
  .bullet.src-answer { color: #b8860b; }
  .content.answer-summary { opacity: 0.85; font-style: italic; }
```

- [ ] **Step 2: 检查**

Run: `pnpm check` + `pnpm test`。

- [ ] **Step 3: Commit**

```bash
git add src/components/outline/OutlineNode.svelte
git commit -m "feat(outline): show answer nodes as a sparkle summary instead of raw fences"
```

---

### Task 11: 协议文本更新

**Files:**
- Modify: `src-tauri/templates/AGENTS.md`
- Modify: `website/public/llms.txt`
- Modify: `website/public/llms-full.txt`

- [ ] **Step 1: AGENTS.md** — 三处改动

1. Vault layout 里**删掉** `answers/` 条目(答复不再落独立文件)。

2. `## Questions & answers` 一节的第 3、4 步替换为(第 5 步保留):

```markdown
3. Write the answer as a single child bullet of the question node, with
   the whole answer body wrapped in a markdown code fence. The fence
   makes arbitrary markdown (lists, `key::`-looking lines, nested code
   blocks) opaque to the outline parser:

       - why is this claim true?
         type:: question
         status:: answered
         - ```markdown
           Because …

           - a list item is fine
           ```python
           nested_code = "is fine too"
           ```
           ```
           type:: answer
           by:: your-agent-name
           answered:: 2026-07-28T14:22:00Z

   Use a longer fence (````, `````, …) if the answer itself contains a
   run of three or more backticks. One answer node per question; answering
   again replaces it.
```

3. Hard rules 追加 `adopted`:

```markdown
Hard rules: never set `status:: closed` or `status:: adopted` (only the
human closes or adopts), never edit the main `.md`, never modify any
existing bullet that is not your own `✦` answer, never touch any other
part of the outline.
```

- [ ] **Step 2: llms.txt** — Q&A loop 那条摘要改为:

```markdown
- Q&A loop: an annotation containing `?` becomes a `type:: question` node
  (`status:: open`) in the companion `.note.md`; agents answer with a
  `type:: answer` child whose body is wrapped in a markdown code fence, and
  set `status:: answered`; only the human sets `closed` / `adopted`.
```

- [ ] **Step 3: llms-full.txt** — 同 AGENTS.md:删 `answers/` 段、答复写法改围栏 answer 节点、硬规则加 `adopted`。

- [ ] **Step 4: 验证协议样例真能被解析器读**(重要:agent 会逐字照抄)

写一个临时测试,把 AGENTS.md 里那段 answer 样例(剥掉文档缩进)喂给 `parseOutline`,断言:解析出 `source:'answer'`、`answeredBy` 正确、`answerBodyOf` 拿到含嵌套代码块的正文、`serializeOutline` 字节级 roundtrip。**跑通后删掉临时测试文件,不提交。**

- [ ] **Step 5: Commit**

```bash
git add src-tauri/templates/AGENTS.md website/public/llms.txt website/public/llms-full.txt
git commit -m "docs(protocol): fenced answer nodes replace bullet answers and answers/ files"
```

---

### Task 12: 终审 + 合并

- [ ] **Step 1: 全量验证**

Run: `pnpm check`(0 errors)+ `pnpm test`(全绿)。

- [ ] **Step 2: 端到端(纯数据链路,不依赖 GUI)**

临时脚本/测试验证:一份含 answer 节点的 `.note.md` → `parseOutline` → `deriveAnswers` → `answeredByNoteText` 得到条目 → `markAdoptedInText` 置 adopted → 重新派生后该条目不再出现在 answered map。跑通后删除临时文件。

- [ ] **Step 3: 合并到 main**

```bash
cd /Users/bruce/git/mdeditor
git merge --no-ff feat/answer-node-inline-card -m "Merge answer nodes + inline answer cards + human adoption"
git push origin main
```

- [ ] **Step 4: 交付 GUI 验证清单**(用户实机验证,不做 UI 自动化)

1. 手写一份含 answer 节点的 `.note.md`(用协议样例),打开对应源 md → 被批注段落下方出现折叠的 ✦ 答复卡片。
2. 点开卡片 → 答复 markdown 正确渲染(列表/代码块/多段落)。
3. 点「采纳入正文」→ 干净 markdown 插入到该段之后;⌘Z 一次即回退。
4. 采纳后卡片消失;`.note.md` 里该问题变 `status:: adopted`。
5. 大纲面板里 answer 节点显示 `✦ 摘要`,不是 ```` ```markdown ````。
6. 深色模式下卡片配色正常;source 模式下无卡片。
7. 回归:⁇ 徽标、chip、FolderView 角标、普通批注均照旧。
