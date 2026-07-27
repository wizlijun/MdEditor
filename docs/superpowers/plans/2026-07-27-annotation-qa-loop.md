# 批注问答闭环(Annotation Q&A Loop)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 批注含 `?`/`？` 即成为向 agent 的异步提问:落盘为 `.note.md` 的 `type:: question` 节点(open/answered/closed 状态机),外部 agent 按 AGENTS.md 协议作答回填;note.md 端提供 ⁇ 徽标、提问按钮、状态 chip、新回答角标。

**Architecture:** 全部改动在 mdeditor 前端 + 模板文本,**不动 moraya-core、不动 Rust**。问题身份由批注文本中的问号承载(file-over-app);元数据落 `.note.md` 的 `::` 属性行。核心链路:derive(已有)→ sync 里把批注 note 子节点按问号升格为 `question` 并管理状态迁移 → parser/serializer 支持 `status::/answered::/by::` 三个新属性 → 两条落盘路径(大纲编辑器已挂载时走既有 intent-save + 强制 arm;未挂载时走新的 headless 捕获,挂在 tabs.setContent 上)。

**Tech Stack:** Svelte 5 (runes)、TypeScript、Vitest、Tauri plugin-fs。测试命令 `pnpm test`,类型检查 `pnpm check`。

**Spec:** `docs/superpowers/specs/2026-07-27-annotation-qa-loop-design.md`

**与 spec 的两处已定实现映射**(spec 示例是示意,以下才是真实结构):
1. 现有派生结构是「annotation 父节点(content=被批注原文或 ※,带 `line::`)+ note 子节点(content=批注文本)」。**question = 含问号的 note 子节点升格**(`type:: question`),`line::` 在其 annotation 父节点上。协议文本(Task 10)按此真实结构撰写。
2. 追问 reopen 不弹对话框:在 answered 问题下追加/修改 ● 手写内容即自动拨回 open(确定性行为,零打扰)。

**背景知识(执行前必读):**
- `.note.md` 数据流:`deriveAutoItems(md)` 扫主文档产 AutoItem → `syncAutoItems(tree, items)` LCS 增量合并进树(note 子节点不参与 LCS,由 annotation 父节点管理)→ `serializeOutline` 写文本。全局单例 store 在 `src/lib/outline/store.svelte.ts`。
- intent-save:`outline.armed` 为 true 才落盘;`markDirty()`(用户编辑)会 arm,`markSynced()`(派生同步)只在已 arm 时落盘。
- 伴生笔记只住 vault:写盘目标由 `planNoteHome()` 决定(`use`/`sync`/`configure-vault` 三种 action),首写会把源文件 sync 进 vault。
- 共享 worktree:每次 commit **只精确 add 本任务列出的文件,绝不 `git add -A`**。

---

### Task 1: 数据模型 — NodeSource 增加 `question`,新属性字段

**Files:**
- Modify: `src/lib/outline/model.ts`(类型定义在文件头部,`NodeSource` 与 `OutlineNode` interface)
- Test: `src/lib/outline/model.test.ts`

- [ ] **Step 1: 写失败测试**

在 `src/lib/outline/model.test.ts` 末尾追加(import 行合并进文件头部现有 import):

```typescript
import { isQuestionText, treeHasQuestion, createTree, addNode, newId } from './model'
import type { OutlineNode } from './model'

describe('question helpers', () => {
  it('isQuestionText matches half/full-width question marks', () => {
    expect(isQuestionText('这是为什么?')).toBe(true)
    expect(isQuestionText('为什么？')).toBe(true)
    expect(isQuestionText('mid?dle')).toBe(true)
    expect(isQuestionText('只是备注')).toBe(false)
    expect(isQuestionText('')).toBe(false)
  })

  it('treeHasQuestion finds question nodes', () => {
    const tree = createTree()
    expect(treeHasQuestion(tree)).toBe(false)
    const n: OutlineNode = {
      id: newId(), parentId: null, order: 0, content: '为什么?',
      collapsed: false, source: 'question', status: 'open',
    }
    addNode(tree, n)
    expect(treeHasQuestion(tree)).toBe(true)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/outline/model.test.ts`
Expected: FAIL — `isQuestionText` / `treeHasQuestion` 未导出,且 `source: 'question'` 类型不符。

- [ ] **Step 3: 实现**

`src/lib/outline/model.ts`:

1. `NodeSource` 类型加 `'question'`:

```typescript
export type NodeSource = 'toc' | 'highlight' | 'wikilink' | 'annotation' | 'note' | 'question' | 'manual'
export type QuestionStatus = 'open' | 'answered' | 'closed'
```

2. `OutlineNode` interface 追加三个可选字段(放在 `updatedAt` 之后):

```typescript
  /** 仅 source==='question':问答状态机(spec 2026-07-27-annotation-qa-loop)。缺省视为 open */
  status?: QuestionStatus
  /** ✦ 作答节点:作答时间(ISO8601),由外部 agent 写入,须原样 roundtrip */
  answeredAt?: string
  /** ✦ 作答节点:作答者标识(如 claude-code),由外部 agent 写入,须原样 roundtrip */
  answeredBy?: string
```

3. 文件末尾追加两个纯函数:

```typescript
/** 批注文本含半角/全角问号即视为「向 agent 提问」(spec:自然书写即协议) */
export function isQuestionText(s: string): boolean {
  return /[?？]/.test(s)
}

/** 树中存在任何 question 节点(用于:含提问即激活伴生笔记落盘) */
export function treeHasQuestion(tree: OutlineTree): boolean {
  for (const n of tree.nodes.values()) if (n.source === 'question') return true
  return false
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/outline/model.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/outline/model.ts src/lib/outline/model.test.ts
git commit -m "feat(outline): add question node source and status fields to model"
```

---

### Task 2: 序列化 — `status:: / answered:: / by::` 属性行

**Files:**
- Modify: `src/lib/outline/markdown.ts`(PROP_RE 第 4 行;parseOutline 属性分支约 93-108 行;serializeOutline 约 29-38 行)
- Test: `src/lib/outline/markdown.test.ts`

- [ ] **Step 1: 写失败测试**

`src/lib/outline/markdown.test.ts` 末尾追加:

```typescript
describe('question / answer properties', () => {
  const sample = [
    '- 被批注的原文',
    '  type:: annotation',
    '  line:: 142',
    '  - 这里为什么能到 90%?',
    '    type:: question',
    '    status:: answered',
    '    - ✦ 因为前缀高度重复',
    '      answered:: 2026-07-27T14:22:00.000Z',
    '      by:: claude-code',
    '',
  ].join('\n')

  it('parses type:: question with status', () => {
    const t = parseOutline(sample)
    const q = [...t.nodes.values()].find(n => n.source === 'question')!
    expect(q).toBeDefined()
    expect(q.status).toBe('answered')
    expect(q.content).toBe('这里为什么能到 90%?')
  })

  it('parses answered::/by:: on the ✦ answer node instead of swallowing them into content', () => {
    const t = parseOutline(sample)
    const a = [...t.nodes.values()].find(n => n.content.startsWith('✦'))!
    expect(a.answeredAt).toBe('2026-07-27T14:22:00.000Z')
    expect(a.answeredBy).toBe('claude-code')
    expect(a.content).toBe('✦ 因为前缀高度重复')
  })

  it('roundtrips question and answer properties', () => {
    const t = parseOutline(sample)
    expect(serializeOutline(t)).toBe(sample)
  })

  it('question without status serializes status:: open', () => {
    const t = parseOutline('- 为什么?\n  type:: question\n')
    expect(serializeOutline(t)).toContain('status:: open')
  })

  it('ignores invalid status values', () => {
    const t = parseOutline('- q?\n  type:: question\n  status:: banana\n')
    const q = [...t.nodes.values()][0]
    expect(q.status).toBeUndefined()
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/outline/markdown.test.ts`
Expected: FAIL — `type:: question` 不在白名单(source 保持 manual),`status::/answered::/by::` 被当续行并进 content。

- [ ] **Step 3: 实现**

`src/lib/outline/markdown.ts`:

1. 第 4 行 PROP_RE 扩键:

```typescript
const PROP_RE = /^(type|line|id|collapsed|created|updated|status|answered|by):: (.*)$/
```

2. parseOutline 的 type 白名单(现第 95 行)加 `'question'`:

```typescript
if (key === 'type' && ['toc', 'highlight', 'wikilink', 'annotation', 'note', 'question'].includes(value)) current.source = value as NodeSource
```

3. 同一 if/else 链追加三个分支(插在 `key === 'updated'` 分支之后):

```typescript
          else if (key === 'status') {
            if (value === 'open' || value === 'answered' || value === 'closed') current.status = value
          }
          else if (key === 'answered') current.answeredAt = value
          else if (key === 'by') current.answeredBy = value
```

4. serializeOutline:`line::` 行(现第 31 行)之后写 status(只对 question 写,缺省补 open);`updated::` 行之后写 answered/by:

```typescript
      if (n.source !== 'manual') {
        lines.push(`${indent}  type:: ${n.source}`)
        if (n.anchorLine != null) lines.push(`${indent}  line:: ${n.anchorLine}`)
        if (n.source === 'question') lines.push(`${indent}  status:: ${n.status ?? 'open'}`)
      }
      if (n.createdAt) lines.push(`${indent}  created:: ${n.createdAt}`)
      if (n.updatedAt) lines.push(`${indent}  updated:: ${n.updatedAt}`)
      if (n.answeredAt) lines.push(`${indent}  answered:: ${n.answeredAt}`)
      if (n.answeredBy) lines.push(`${indent}  by:: ${n.answeredBy}`)
```

注意 roundtrip 测试对属性行顺序敏感:上面顺序(type→line→status→created→updated→answered→by→id→collapsed)即为规范顺序,Step 1 的 sample 按此顺序书写。

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/outline/markdown.test.ts`
Expected: PASS(含既有用例——PROP_RE 扩键不改变已有键行为)。

- [ ] **Step 5: Commit**

```bash
git add src/lib/outline/markdown.ts src/lib/outline/markdown.test.ts
git commit -m "feat(outline): parse and serialize question status and answer props"
```

---

### Task 3: 同步管线 — 批注问号升格 question 与状态生命周期

**Files:**
- Modify: `src/lib/outline/sync.ts`(autoSequence 第 26 行;oldNoteText 第 78-79 行;降级块第 111-118 行;note 子节点块第 148-161 行)
- Test: `src/lib/outline/sync.test.ts`

**规则**:批注文本含 ?/？ → note 子节点 source='question'、初始 status='open';重派生时已是 question 的**保留现有 status**(agent 置的 answered 不被冲掉);问号被删 → 回落 note、status 清除;批注从主文档消失 → 与 note 同规则降级 manual 保文字。

- [ ] **Step 1: 写失败测试**

`src/lib/outline/sync.test.ts` 末尾追加(文件头部已有 `syncAutoItems`/`deriveAutoItems`/`createTree`/`childrenOf` 等 import,缺哪个补哪个):

```typescript
describe('question lifecycle', () => {
  const qMd = '正文 {==原文==}{>>这里为什么能到 90%?<<}\n'
  const plainMd = '正文 {==原文==}{>>只是个备注<<}\n'
  const noteChild = (tree: ReturnType<typeof createTree>) => {
    const anno = [...tree.nodes.values()].find(n => n.source === 'annotation')!
    return childrenOf(tree, anno.id).find(c => c.source === 'note' || c.source === 'question')!
  }

  it('annotation with ? derives a question child with status open', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(qMd))
    const c = noteChild(tree)
    expect(c.source).toBe('question')
    expect(c.status).toBe('open')
  })

  it('annotation without ? stays a plain note child', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(plainMd))
    const c = noteChild(tree)
    expect(c.source).toBe('note')
    expect(c.status).toBeUndefined()
  })

  it('re-sync preserves agent-set answered status', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(qMd))
    noteChild(tree).status = 'answered'          // 模拟 agent 作答后从盘上读回
    syncAutoItems(tree, deriveAutoItems(qMd))    // 主文档重派生
    expect(noteChild(tree).status).toBe('answered')
  })

  it('editing the note to add ? upgrades it to an open question', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(plainMd))
    syncAutoItems(tree, deriveAutoItems('正文 {==原文==}{>>只是个备注,对吗?<<}\n'))
    const c = noteChild(tree)
    expect(c.source).toBe('question')
    expect(c.status).toBe('open')
  })

  it('removing the ? demotes question back to note and drops status', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(qMd))
    noteChild(tree).status = 'answered'
    syncAutoItems(tree, deriveAutoItems('正文 {==原文==}{>>结论已明<<}\n'))
    const c = noteChild(tree)
    expect(c.source).toBe('note')
    expect(c.status).toBeUndefined()
  })

  it('annotation deleted from md demotes question child to manual, keeping text', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(qMd))
    syncAutoItems(tree, deriveAutoItems('正文没有批注了\n'))
    const kept = [...tree.nodes.values()].find(n => n.content === '这里为什么能到 90%?')!
    expect(kept.source).toBe('manual')
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/outline/sync.test.ts`
Expected: FAIL — 子节点 source 恒为 'note'。

- [ ] **Step 3: 实现**

`src/lib/outline/sync.ts`:

1. import 行(第 2 行)加 `isQuestionText`,并引入 `QuestionStatus` 无需(用字面量):

```typescript
import { childrenOf, newId, nowIso, setNodeContent, calculateOrderBetween, isQuestionText, type OutlineTree, type OutlineNode } from './model'
```

2. autoSequence(第 26 行)排除 question(与 note 同为 annotation 管理的子节点):

```typescript
      if (n.source !== 'manual' && n.source !== 'note' && n.source !== 'question') out.push(n)
```

3. oldNoteText(第 78-79 行):

```typescript
  const oldNoteText = (node: OutlineNode): string =>
    childrenOf(tree, node.id).find(c => c.source === 'note' || c.source === 'question')?.content ?? ''
```

4. 降级块(第 111-118 行内层 for):

```typescript
      for (const c of childrenOf(tree, node.id)) {
        if (c.source === 'note' || c.source === 'question') { c.source = 'manual'; c.anchorLine = undefined }
      }
```

(降级为 manual 后 serializeOutline 不再写 `type::`/`status::`,agent 不会再扫到——语义正确:源批注已消失。status 字段残留在内存对象上无害。)

5. note 子节点创建/更新块(第 148-161 行)整体替换为:

```typescript
    if (it.source === 'annotation') {
      // 唯一的 note/question 子节点承载批注内容:创建或原地更新(保 id)。
      // 批注文本含 ?/？ = 向 agent 提问 → 升格 question(status 初始 open);
      // 重派生不重置已有 status(agent 置 answered 后不被主文档编辑冲掉);
      // 问号删除 → 回落 note,状态清除。
      const noteText = it.note ?? ''
      const isQ = isQuestionText(noteText)
      const child = childrenOf(tree, node.id).find(c => c.source === 'note' || c.source === 'question')
      if (child) {
        setNodeContent(child, noteText)
        if (isQ) {
          if (child.source !== 'question') { child.source = 'question'; child.status = 'open' }
        } else if (child.source === 'question') {
          child.source = 'note'
          delete child.status
        }
      } else {
        const fresh: OutlineNode = {
          id: newId(), parentId: node.id, order: -100,
          content: noteText, collapsed: false,
          source: isQ ? 'question' : 'note',
          ...(isQ ? { status: 'open' as const } : {}),
          createdAt: nowIso(),
        }
        tree.nodes.set(fresh.id, fresh)
      }
    }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/outline/sync.test.ts`
Expected: PASS(含既有用例)。

- [ ] **Step 5: 跑全量 outline 测试防回归**

Run: `pnpm test -- src/lib/outline`
Expected: PASS。若 note-writeback / store 相关用例因 source 判断失败,检查是否遗漏 `'question'` 分支。

- [ ] **Step 6: Commit**

```bash
git add src/lib/outline/sync.ts src/lib/outline/sync.test.ts
git commit -m "feat(outline): promote ?-annotations to question nodes with status lifecycle"
```

---

### Task 4: 含提问即激活落盘(arm)

**Files:**
- Modify: `src/lib/outline/store.svelte.ts`(attachDoc 第 135-139 行)
- Modify: `src/components/outline/OutlineEditor.svelte`(派生 effect 第 271 行附近;import 第 20 行)
- Test: `src/lib/outline/store.test.ts`

- [ ] **Step 1: 写失败测试**

`src/lib/outline/store.test.ts` 追加(参考文件内既有 attachDoc 用例的写法与清理约定,通常有 beforeEach 调 detach):

```typescript
describe('question arming', () => {
  it('arms auto-save when the main doc carries a question annotation', async () => {
    await attachDoc('/tmp/q.note.md', '', '正文 {==原文==}{>>这是为什么?<<}\n')
    expect(outline.armed).toBe(true)
  })

  it('does not arm for a plain annotation', async () => {
    await attachDoc('/tmp/p.note.md', '', '正文 {==原文==}{>>只是备注<<}\n')
    expect(outline.armed).toBe(false)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/outline/store.test.ts`
Expected: 第一个用例 FAIL(armed 为 false——noteText 为空,`noteTextHasContent('')` 为 false)。

- [ ] **Step 3: 实现**

1. `src/lib/outline/store.svelte.ts`:头部 import(第 2 行附近,从 `./model` 的 import)加 `treeHasQuestion`;attachDoc 派生行(第 138 行)后追加一行:

```typescript
  if (mainContent != null) syncAutoItems(outline.tree, deriveAutoItems(mainContent))
  // 提问视为明确保存意图:树中出现 question 节点即激活落盘(spec §2)
  if (treeHasQuestion(outline.tree)) outline.armed = true
  bump()
```

2. `src/components/outline/OutlineEditor.svelte`:import 第 20 行加 `treeHasQuestion`;派生 effect 内(第 271-273 行)改为:

```typescript
          const before = serializeDoc(false)
          syncAutoItems(outline.tree, deriveAutoItems(mc))
          if (treeHasQuestion(outline.tree)) outline.armed = true
          // 同步只置脏、进内存;未激活自动保存时不落盘(浏览/主文档编辑不自动生成笔记)
          if (serializeDoc(false) !== before) { bump(); markSynced() }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `pnpm test -- src/lib/outline/store.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/lib/outline/store.svelte.ts src/components/outline/OutlineEditor.svelte src/lib/outline/store.test.ts
git commit -m "feat(outline): arm companion persistence when a question is present"
```

---

### Task 5: Headless 捕获 — 大纲未挂载也落盘

大纲面板关着时没有 OutlineEditor,提问也必须落盘(spec §2)。挂点:`tabs.setContent`(rich/source 双模式的内容变更都汇于此),廉价门卫 + 1.5s 防抖 + 动态 import 防环。

**Files:**
- Create: `src/lib/outline/question-capture.ts`
- Modify: `src/lib/tabs.svelte.ts`(setContent 第 357-360 行)
- Test: `src/lib/outline/question-capture.test.ts`

- [ ] **Step 1: 写失败测试**

Create `src/lib/outline/question-capture.test.ts`:

```typescript
import { describe, it, expect } from 'vitest'
import { mdHasQuestionAnnotation } from './question-capture'

describe('mdHasQuestionAnnotation', () => {
  it('detects a question inside wrapped annotation', () => {
    expect(mdHasQuestionAnnotation('x {==原文==}{>>为什么?<<} y')).toBe(true)
  })
  it('detects a question inside point annotation (full-width)', () => {
    expect(mdHasQuestionAnnotation('末尾{>>这个对吗？<<}')).toBe(true)
  })
  it('ignores plain annotations and bare question marks', () => {
    expect(mdHasQuestionAnnotation('x {>>备注<<} 正文里的问号?')).toBe(false)
    expect(mdHasQuestionAnnotation('没有批注')).toBe(false)
  })
})
```

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/outline/question-capture.test.ts`
Expected: FAIL — 模块不存在。

- [ ] **Step 3: 实现捕获模块**

Create `src/lib/outline/question-capture.ts`:

```typescript
// src/lib/outline/question-capture.ts
// Headless 提问捕获:大纲编辑器未挂载时,主文档里出现「提问批注」({>>…?<<})
// 也要把伴生 .note.md 写上盘——外部 agent 只能读到磁盘文件(spec §2)。
// 挂在 tabs.setContent 上,廉价门卫 + 按路径防抖;大纲已挂载同文档时让位
// (它的派生管线 + Task 4 的 arm 会落盘,双写会互踩)。
import { deriveAutoItems } from './derive'
import { syncAutoItems } from './sync'
import { parseOutline, serializeOutline } from './markdown'
import { treeHasQuestion, isQuestionText } from './model'
import { outline, companionPathFor, noteTextHasContent } from './store.svelte'
import { planNoteHome } from './note-home'

/** 主文档文本里是否存在「提问批注」。廉价门卫,避免每次输入都走重逻辑 */
export function mdHasQuestionAnnotation(md: string): boolean {
  for (const m of md.matchAll(/\{>>(.*?)<<\}/g)) if (isQuestionText(m[1])) return true
  return false
}

const timers = new Map<string, ReturnType<typeof setTimeout>>()

export function scheduleQuestionCapture(mainPath: string | null | undefined, md: string): void {
  if (!mainPath || !/\.md$/i.test(mainPath) || /\.notes?\.md$/i.test(mainPath)) return
  if (!mdHasQuestionAnnotation(md)) return
  const prev = timers.get(mainPath)
  if (prev) clearTimeout(prev)
  timers.set(mainPath, setTimeout(() => {
    timers.delete(mainPath)
    void captureQuestions(mainPath, md)
  }, 1500))
}

async function captureQuestions(mainPath: string, md: string): Promise<void> {
  try {
    // 大纲编辑器已挂载同一文档 → 让位给它的 intent-save 管线
    const companion = companionPathFor(mainPath)
    if (companion && outline.docPath === companion) return
    const { sotvaultStore, syncSourceToVaultAsHome, refreshSotvault } = await import('../sotvault.svelte')
    const fs = await import('@tauri-apps/plugin-fs')
    const legacyNoteExists = companion ? await fs.exists(companion).catch(() => false) : false
    const plan = planNoteHome(mainPath, {
      vaultRoot: sotvaultStore.vaultRoot,
      records: sotvaultStore.records,
      legacyNoteExists,
    })
    let notePath: string | null = null
    if (plan.action === 'use') notePath = plan.notePath
    else if (plan.action === 'sync') {
      // 首写:源复制进 vault,笔记落 vault 副本旁(伴生笔记只住 vault)
      const rec = await syncSourceToVaultAsHome(mainPath)
      notePath = companionPathFor(rec.vault_path)
      await refreshSotvault()
    } else {
      return   // configure-vault:无 vault 静默跳过;批注仍在 md,配好 vault 后重开即捕获
    }
    if (!notePath) return
    if (outline.docPath === notePath) return   // 建家期间大纲挂上来了(竞态)→ 让位
    const existed = await fs.exists(notePath).catch(() => false)
    const diskText = existed ? await fs.readTextFile(notePath).catch(() => null) : ''
    if (diskText == null) return
    const tree = parseOutline(diskText)
    syncAutoItems(tree, deriveAutoItems(md))
    if (!treeHasQuestion(tree)) return         // 门卫误报(如 ? 在代码块里):不写
    const out = serializeOutline(tree)
    if (out === diskText) return
    // 数据丢失防线:绝不用"无内容"的序列化盖有内容的落点
    if (!noteTextHasContent(out) && noteTextHasContent(diskText)) return
    await fs.writeTextFile(notePath, out)
  } catch (e) {
    console.warn('[question-capture] failed:', e)
  }
}
```

已知取舍(可接受,写入注释即可,不需要额外处理):读盘→写盘之间无 hash 锁,与并发 agent 写有毫秒级竞态窗口;deriveAutoItems 内部会跳过代码块,门卫的简单正则可能误报,由 `treeHasQuestion(tree)` 兜底。

- [ ] **Step 4: 挂载到 tabs.setContent**

`src/lib/tabs.svelte.ts` 第 357-360 行改为:

```typescript
export function setContent(id: string, md: string): void {
  const t = tabs.find((x) => x.id === id)
  if (!t) return
  t.currentContent = md
  // 提问捕获:含 {>> 才动态加载(键入热路径,先做廉价字面判断)
  if (t.filePath && md.includes('{>>')) {
    void import('./outline/question-capture').then((m) => m.scheduleQuestionCapture(t.filePath, md))
  }
}
```

- [ ] **Step 5: 跑测试与类型检查**

Run: `pnpm test -- src/lib/outline/question-capture.test.ts && pnpm test -- src/lib/tabs.test.ts && pnpm check`
Expected: 全 PASS。`pnpm check` 无新增错误(基线可能已有既存告警,只看增量)。

- [ ] **Step 6: Commit**

```bash
git add src/lib/outline/question-capture.ts src/lib/outline/question-capture.test.ts src/lib/tabs.svelte.ts
git commit -m "feat(outline): headless question capture writes companion note on annotation edits"
```

---

### Task 6: 正文徽标 — 提问批注 ※ → ⁇

**Files:**
- Modify: `src/styles/editor-base.css`(第 56-59 行的 `::before` 规则之后)

徽标元素已带 `data-note` 属性(note-badge widget 在 `src/lib/note-anno/note-plugin.ts:28` 设 `el.dataset.note`;note_anchor 的 toDOM 在 moraya-core schema 设 `data-note`),纯 CSS 属性选择器即可,**无 JS 改动**。

- [ ] **Step 1: 加规则**

在 `.note-badge::before / .moraya-note-anchor::before { content: '※' }` 规则(第 56-59 行)之后插入:

```css
/* 提问批注({>>…<<} 内含 ?/？):徽标换 ⁇(U+2047),一眼区分「我的备注」与「抛给 agent 的问题」 */
.moraya-editor .note-badge[data-note*='?']::before,
.moraya-editor .note-badge[data-note*='？']::before,
.moraya-editor .moraya-note-anchor[data-note*='?']::before,
.moraya-editor .moraya-note-anchor[data-note*='？']::before {
  content: '⁇';
}
```

- [ ] **Step 2: 类型检查(无测试可写,CSS 由 Task 11 GUI 验证)**

Run: `pnpm check`
Expected: 无新增错误。

- [ ] **Step 3: Commit**

```bash
git add src/styles/editor-base.css
git commit -m "feat(editor): render question annotations with a doubled question mark badge"
```

---

### Task 7: 批注气泡 — ⁇ 提问按钮 + 引导 placeholder

**Files:**
- Modify: `src/lib/note-anno/NoteEditPopup.svelte`
- Modify: `src/lib/i18n/en.ts`(noteedit 键区,第 150-151 行)
- Modify: `src/lib/i18n/zh.ts`(第 145-146 行)、`src/lib/i18n/ja.ts`(第 145-146 行)、`src/lib/i18n/de.ts`(第 143-144 行)

- [ ] **Step 1: i18n 键**

en.ts(改 placeholder 值,新增两键;保持字母区块内相邻插入):

```typescript
  'noteedit.placeholder': 'Write a note… end with ? to ask your agent',
  'noteedit.delete': 'Delete note',
  'noteedit.ask': 'Ask',
  'noteedit.askHint': 'Mark as a question for your agent (appends ? and saves)',
```

zh.ts:

```typescript
  'noteedit.placeholder': '输入批注……以 ? 结尾即向 agent 提问',
  'noteedit.delete': '删除批注',
  'noteedit.ask': '提问',
  'noteedit.askHint': '标记为向 agent 提问(补全问号并保存)',
```

ja.ts:

```typescript
  'noteedit.placeholder': '注釈を入力…「？」で終えるとエージェントへの質問に',
  'noteedit.delete': '注釈を削除',
  'noteedit.ask': '質問',
  'noteedit.askHint': 'エージェントへの質問にする(？を補って保存)',
```

de.ts:

```typescript
  'noteedit.placeholder': 'Notiz schreiben… mit ? beenden, um den Agenten zu fragen',
  'noteedit.delete': 'Notiz löschen',
  'noteedit.ask': 'Fragen',
  'noteedit.askHint': 'Als Frage an den Agenten markieren (ergänzt ? und speichert)',
```

- [ ] **Step 2: 气泡组件**

`src/lib/note-anno/NoteEditPopup.svelte`:

1. `<script>` 内 `onDelete` 之后加:

```typescript
  /** ⁇ 提问:尾部无问号则补全角 ？,保存并关闭。问题身份由文本中的问号承载,按钮只是糖 */
  function onAsk() {
    if (!/[?？]\s*$/.test(text)) text = text.trimEnd() + '？'
    close(true)
  }
```

2. `.row` 区块(第 65-72 行)改为(提问按钮居左醒目,删除钮居右):

```svelte
  <div class="row">
    <button class="ask" onclick={onAsk} title={t('noteedit.askHint')}>⁇ {t('noteedit.ask')}</button>
    <button
      class="del"
      onclick={onDelete}
      title={t('noteedit.delete')}
      aria-label={t('noteedit.delete')}
    >{@html iconSvg('trash')}</button>
  </div>
```

3. `<style>` 内 `.row` 改为 `justify-content: space-between;`,并加(注意坑:button 不继承 font,须显式设):

```css
  .row { display: flex; justify-content: space-between; align-items: center; margin-top: 4px; }
  .ask {
    display: flex; align-items: center; gap: 4px;
    padding: 3px 10px;
    border: none; border-radius: 5px; cursor: pointer;
    font-family: inherit; font-size: 12px; font-weight: 600;
    color: #fff;
    background: var(--accent-color, #4a80d4);
  }
  .ask:hover { filter: brightness(1.1); }
```

- [ ] **Step 3: 检查**

Run: `pnpm check && pnpm test`
Expected: PASS(i18n Messages 类型由 en.ts 推导,zh/ja/de 是 Partial,新键不会型错;若 check 报 en 键缺失说明有拼写不一致)。

- [ ] **Step 4: Commit**

```bash
git add src/lib/note-anno/NoteEditPopup.svelte src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(note-anno): add ask-agent button and guiding placeholder to note popup"
```

---

### Task 8: 大纲节点 — question 可编辑 + 状态 chip + 追问自动 reopen

**Files:**
- Modify: `src/components/outline/OutlineNode.svelte`
- Modify: `src/lib/i18n/en.ts`、`src/lib/i18n/zh.ts`、`src/lib/i18n/ja.ts`、`src/lib/i18n/de.ts`(outline 键区各加 1 键)

question 节点是升格的 note 子节点,所有 `node.source === 'note'` 的行为分支(可编辑、回车提交、回写主文档、结构命令禁用)必须同样适用。UI 无单测,靠 `pnpm check` + Task 11 GUI 验证。

- [ ] **Step 1: i18n 键**

en.ts outline 键区加:

```typescript
  'outline.questionChip': 'Question status — click to close / reopen',
```

zh.ts: `'outline.questionChip': '问题状态——点击关闭/重开',`
ja.ts: `'outline.questionChip': '質問ステータス——クリックでクローズ/再オープン',`
de.ts: `'outline.questionChip': 'Fragestatus — Klick zum Schließen/Wiedereröffnen',`

- [ ] **Step 2: note-like 判定统一**

`src/components/outline/OutlineNode.svelte` `<script>`:

1. import 区加 t(现文件不引 i18n):

```typescript
  import { t } from '../../lib/i18n/store.svelte'
```

2. 顶部(props 解构之后)加谓词,并替换全部 6 处 `=== 'note'` 判断:

```typescript
  /** note/question 同族:annotation 的批注文本子节点(question=含问号升格) */
  const noteLike = (s: NodeT['source']) => s === 'note' || s === 'question'
```

逐处替换:
- 第 80 行:`let editable = $derived(node.source === 'manual' || noteLike(node.source))`
- 第 86 行:`if (editing && noteLike(node.source)) noteBaseline = node.content`
- 第 122 行 commitEdit:`if (noteLike(node.source)) {`
- 第 163 行 Enter 分支:`if (noteLike(node.source)) {`
- 第 183 行 Backspace 分支:`if (noteLike(node.source)) return`
- 第 194 行 Arrow 分支:`if (noteLike(node.source)) commitEdit(el.value)`
- 第 196 行 focus 判定:`focusNode(nb.source === 'manual' || noteLike(nb.source) ? nb.id : null)`
- 第 202 行结构命令:`if (noteLike(node.source)) { e.preventDefault(); return }`

3. 第 293 行 bullet 类:`class:src-note={noteLike(node.source)}`

- [ ] **Step 3: 状态 chip + 交互**

1. `<script>` 加:

```typescript
  // status 是普通属性,变更经 bump() 重读(同 content/collapsed 的处理)
  let qStatus = $derived.by(() => { if (!readonly) void outline.version; return node.status })

  /** chip 单击:open/answered → closed(裁决/误判清理);closed → open(重开) */
  function onChipClick() {
    node.status = node.status === 'closed' ? 'open' : 'closed'
    bump(); markDirty()
  }

  /** 追问自动重开:在 answered 问题下追加/修改 ● 手写内容 = 拨回 open(spec §4) */
  function reopenAnsweredAncestor() {
    let pid = node.parentId
    while (pid) {
      const p = outline.tree.nodes.get(pid)
      if (!p) break
      if (p.source === 'question') {
        if (p.status === 'answered') p.status = 'open'
        break
      }
      pid = p.parentId
    }
  }
```

2. commitEdit 的 manual 分支末尾(现第 136-140 行)改为:

```typescript
    const changed = value !== node.content
    setNodeContent(node, value)
    outline.editingId = null
    bump()
    if (changed) { reopenAnsweredAncestor(); markDirty() }
```

3. 模板:非编辑态 content span(第 318-326 行的 `{:else}` 块)之后、`</div>`(row 结束)之前插入:

```svelte
    {#if !readonly && node.source === 'question'}
      <button class="qchip"
        class:st-answered={qStatus === 'answered'}
        class:st-closed={qStatus === 'closed'}
        title={t('outline.questionChip')}
        onclick={(e) => { e.stopPropagation(); onChipClick() }}
      >{qStatus ?? 'open'}</button>
    {/if}
```

(chip 文本用协议状态词原文 open/answered/closed——它们是文件格式词汇,与 `.note.md` 中的 `status::` 一字对应,不翻译。)

4. `<style>` 加(button 不继承 font 的坑:显式设 font-family/size):

```css
  .qchip {
    align-self: center;
    margin-left: 6px;
    padding: 0 7px;
    border: none; border-radius: 8px; cursor: pointer;
    font-family: inherit;
    font-size: calc(var(--outline-font-size, 13px) * 0.72);
    line-height: 1.7;
    color: #fff;
    background: var(--accent-color, #4a80d4);   /* open */
  }
  .qchip.st-answered { background: #2da44e; }
  .qchip.st-closed {
    color: color-mix(in srgb, currentColor 55%, transparent);
    background: color-mix(in srgb, currentColor 12%, transparent);
  }
```

- [ ] **Step 4: 检查 + 全量测试**

Run: `pnpm check && pnpm test`
Expected: PASS。重点确认 note-writeback 相关既有测试仍过(question 节点编辑走同一 writeBackNoteEdit 路径)。

- [ ] **Step 5: Commit**

```bash
git add src/components/outline/OutlineNode.svelte src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(outline): question status chip, editable question nodes, auto-reopen on follow-up"
```

---

### Task 9: FolderView — 「N 个新回答」角标

**Files:**
- Modify: `src/lib/folder-view.svelte.ts`(FolderEntry 接口第 9-18 行;readFolder 管线第 313 行附近)
- Modify: `src/components/FolderTreeNode.svelte`(note-badge 块第 129-140 行)
- Modify: `src/lib/i18n/en.ts`、`zh.ts`、`ja.ts`、`de.ts`(folderView 键区各 1 键)
- Test: `src/lib/folder-view.test.ts`

- [ ] **Step 1: 写失败测试**

`src/lib/folder-view.test.ts` 追加:

```typescript
import { countAnsweredQuestions } from './folder-view.svelte'

describe('countAnsweredQuestions', () => {
  it('counts status:: answered lines', () => {
    const note = [
      '- 原文', '  type:: annotation',
      '  - q1?', '    type:: question', '    status:: answered',
      '  - q2?', '    type:: question', '    status:: open',
      '- 原文2', '  type:: annotation',
      '  - q3?', '    type:: question', '    status:: answered',
    ].join('\n')
    expect(countAnsweredQuestions(note)).toBe(2)
  })
  it('returns 0 for notes without answers', () => {
    expect(countAnsweredQuestions('- 手写\n- 内容 status:: answered 不是属性行')).toBe(0)
  })
})
```

(第二个用例:属性行判定须锚定行首缩进,正文里出现的字样不计。)

- [ ] **Step 2: 跑测试确认失败**

Run: `pnpm test -- src/lib/folder-view.test.ts`
Expected: FAIL — 未导出。

- [ ] **Step 3: 实现数据层**

`src/lib/folder-view.svelte.ts`:

1. FolderEntry 接口(第 9-18 行)加字段:

```typescript
  /** 伴生笔记中 agent 已作答、待人裁决的问题数(status:: answered 计数) */
  answeredCount?: number
```

2. 纯函数(与 pairNoteEntries 相邻处):

```typescript
/** 伴生笔记文本里 agent 已作答、待人裁决的问题数。属性行 = 缩进 + status:: answered 独占一行 */
export function countAnsweredQuestions(noteText: string): number {
  return (noteText.match(/^\s+status:: answered$/gm) ?? []).length
}
```

3. readFolder 管线:在 `const withVaultNotes = augmentVaultNotes(pairNoteEntries(base), sotvaultStore.records)`(第 313 行)之后、pinned map 之前插入:

```typescript
  const { readTextFile } = await import('@tauri-apps/plugin-fs')
  await Promise.all(withVaultNotes.map(async (e) => {
    if (!e.hasNote || !e.notePath) return
    const txt = await readTextFile(e.notePath!).catch(() => null)
    if (txt) {
      const c = countAnsweredQuestions(txt)
      if (c > 0) e.answeredCount = c
    }
  }))
```

(性能:每次列目录多读若干小文本文件,伴生笔记通常 KB 级,可接受;失败静默视为 0。)

- [ ] **Step 4: 渲染角标**

1. i18n:en.ts folderView 键区加 `'folderView.newAnswers': 'New answers from your agent',`;zh `'来自 agent 的新回答'`;ja `'エージェントからの新しい回答'`;de `'Neue Antworten deines Agenten'`。

2. `src/components/FolderTreeNode.svelte` note-badge 块(第 129-140 行):在 `</svg>` 之后、`</span>` 之前插入计数泡:

```svelte
      {#if entry.answeredCount}
        <span class="answer-count" title={t('folderView.newAnswers')}>{entry.answeredCount}</span>
      {/if}
```

3. `<style>` 加:

```css
  .answer-count {
    font-size: 9px; font-weight: 700; line-height: 13px;
    color: #fff; background: #2da44e;
    border-radius: 7px; padding: 0 4px; margin-left: 2px;
  }
```

- [ ] **Step 5: 跑测试 + 检查**

Run: `pnpm test -- src/lib/folder-view.test.ts && pnpm check`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/lib/folder-view.svelte.ts src/lib/folder-view.test.ts src/components/FolderTreeNode.svelte src/lib/i18n/en.ts src/lib/i18n/zh.ts src/lib/i18n/ja.ts src/lib/i18n/de.ts
git commit -m "feat(folder-view): answered-question count badge on files with companion notes"
```

---

### Task 10: 协议文本 — AGENTS.md 模板 + llms.txt / llms-full.txt

**Files:**
- Modify: `src-tauri/templates/AGENTS.md`
- Modify: `website/public/llms.txt`
- Modify: `website/public/llms-full.txt`

- [ ] **Step 1: AGENTS.md 模板**

`src-tauri/templates/AGENTS.md` 三处修改:

1. Vault layout 列表(`sync/` 条目之后)加:

```markdown
- `answers/` — long-form answers written by agents to human questions
  (see "Questions & answers" below), named `yyyy-MM-dd-<slug>.md`.
```

2. `## The .note.md suffix` 一节中 "Do not 'fix' the outline structure…" 条目末尾追加一句:

```markdown
    The only sanctioned write is the Q&A protocol below.
```

3. `## House rules` 之前插入新节:

```markdown
## Questions & answers (`type:: question`)

A human annotation whose text contains `?` (or `？`) is a **question
addressed to you**. In the companion `.note.md` it appears as a child of
an annotation node:

    - the annotated source text
      type:: annotation
      line:: 142
      - why is this claim true?
        type:: question
        status:: open

Sweep protocol — how to answer:

1. Find nodes with `type:: question` and `status:: open` across the
   vault (`grep -rn "status:: open" --include="*.note.md"` works).
2. Read the source context: the parent annotation node's `line::`
   points at the annotated line in the companion main document
   (`xxx.md` beside `xxx.note.md`).
3. Write a short answer as an indented child bullet of the question
   node, prefixed with `✦ ` and followed by two property lines:

       - why is this claim true?
         type:: question
         status:: answered
         - ✦ because …
           answered:: 2026-07-27T14:22:00Z
           by:: your-agent-name

4. A long answer goes to its own file under `answers/`
   (`answers/yyyy-MM-dd-<slug>.md`); keep only a one-line `✦` summary
   plus a link under the question node.
5. Set the question's `status::` to `answered`.

Hard rules: never set `status:: closed` (only the human closes a
question), never edit the main `.md`, never modify human-written (`●`)
bullets, never touch any other part of the outline.
```

- [ ] **Step 2: llms.txt**

`website/public/llms.txt` 的 `## Core conventions (summary)` 列表加一条:

```markdown
- Q&A loop: an annotation containing `?` becomes a `type:: question` node
  (`status:: open`) in the companion `.note.md`; agents answer with a `✦`
  child bullet and set `status:: answered`; only the human sets `closed`.
```

- [ ] **Step 3: llms-full.txt**

`website/public/llms-full.txt`:在 `.note.md` 格式说明一节之后插入完整协议段(内容 = Step 1 第 3 点的 `## Questions & answers` 全文,标题层级按该文件现有层级适配)。若文件里有「agent 对 `.note.md` 只读/不主动写入」的表述,修改为:

```
Agents treat `.note.md` as read-only, with one sanctioned exception: the
Q&A answer-writeback protocol described below.
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/templates/AGENTS.md website/public/llms.txt website/public/llms-full.txt
git commit -m "docs(protocol): agent Q&A sweep protocol in AGENTS.md template and llms.txt"
```

(注:已有 vault 的 AGENTS.md 不会自动更新——模板只在文件不存在时写入,存量用户手动补协议段,MVP 接受。)

---

### Task 11: 全量验证 + GUI 手动验证清单 + 发布

- [ ] **Step 1: 全量检查**

Run: `pnpm check && pnpm test`
Expected: 全 PASS。

- [ ] **Step 2: dev 构建交用户 GUI 验证(不做 UI 自动化,用户自测)**

起 dev 构建,把下面清单交给用户:

1. **⁇ 徽标**:rich 模式选中文字加批注,输入含 `?` 的文本 → 正文徽标显示 ⁇;删掉问号 → 回到 ※。
2. **提问按钮**:批注气泡里可见醒目「⁇ 提问」按钮;输入无问号文本点它 → 尾部补 `？`、气泡关闭、徽标变 ⁇;placeholder 显示引导文案。
3. **落盘(面板开)**:大纲面板开着写提问批注 → `.note.md` 生成/更新,问题节点带 `type:: question` + `status:: open`。
4. **落盘(面板关)**:关掉大纲面板,在另一篇 md 写提问批注,等 2 秒 → 伴生 `.note.md` 依然落盘(vault 未配置时应静默无事)。
5. **状态 chip**:大纲里问题节点显示 open chip;手改 `.note.md` 把 status 改 answered 并加 ✦ 子节点(模拟 agent),重开文档 → chip 变绿 answered、✦ 答案可见;点 chip → closed;再点 → open。
6. **追问 reopen**:answered 状态下在答案下追加手写子节点 → chip 自动回 open。
7. **角标**:FolderView 中该文件行出现绿色计数泡;全部 close 后刷新目录 → 泡消失。
8. **回归**:普通批注(无问号)、高亮、wikilink 派生行为不变;`.note.md` 手写节点编辑保存正常。

- [ ] **Step 3: 端到端协议实测**

用户 GUI 通过后,由 Claude Code(本会话或子代理)扮演外部 agent:读一个真实 vault 的 AGENTS.md 新协议段,对 2 个 open 问题按协议作答(改 `.note.md`),回到 GUI 确认 chip/角标/✦ 展示正确。

- [ ] **Step 4: 发布**

按发布惯例(独立 worktree + `.env.release` + pnpm install;架构自检;gh 账号确认 wizlijun):

```bash
git worktree add ../mdeditor-release-qa main
# 在 release worktree 内: 配 .env.release → pnpm install → ./scripts/release.sh(版本号按日期规则自动推导)
```

发布完成后更新 memory(annotation-qa-loop 状态→已发布)。
