# Markdown 脚注 `[^label]` 支持 —— 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 rich 模式认识 `[^label]` 脚注 —— 角标可读可点、hover 看内容,而磁盘上的 `.md` 一个字节都不被改写。

**Architecture:** 引入 `markdown-it-footnote@4.0.0` 但禁掉 `footnote_tail`(core)与 `footnote_inline`(inline)两条规则,使定义节点原位保真、孤儿定义不被删;补一条兜底 inline 规则让无定义的裸引用也成节点。schema 新增 `footnote_ref`(inline atom)与 `footnote_definition`(block)。角标编号是派生值,由 ProseMirror 插件用 Decoration 计算填充,**不落盘**。

**Tech Stack:** TypeScript · markdown-it 14 · markdown-it-footnote 4 · prosemirror-model / -state / -view / -markdown · vitest + happy-dom

**Spec:** `docs/superpowers/specs/2026-08-11-footnote-support-design.md`

## Global Constraints

- **两个仓库**:解析/schema/插件改在 `/Users/bruce/git/moraya-core`(本地 fork);样式与 GUI 验证在 `/Users/bruce/git/mdeditor`。
- **往返保真是最高优先级**:`serializeMarkdown(parseMarkdown(x)) === x`,字节相等。任何任务不得为了效果牺牲这一条。
- **编号绝不写进节点 attrs**。编号是渲染层派生值,落盘即违反 file-over-app 原则。
- **上标样式禁用 `display:inline-block` 与 `vertical-align:super`**。必须用 `display:inline` + `line-height:0` + `position:relative; top:-0.4em`。原因见 `mdeditor/src/styles/editor-base.css:45` 的长注释:非替换 inline 盒对行框高度贡献等于其 line-height(0),而 inline-block 会按 margin 盒高度参与布局并凭空给行框加出下伸部 —— 大标题和表格里行高会被撑大。`<sup>` 标签浏览器默认就是 `vertical-align:super`,**必须显式覆盖**。
- **改完 moraya-core 必须 `pnpm build`(tsup) + 回 mdeditor `pnpm sync:core`**,否则 Vite deps 缓存吃不到改动。
- **提交只精确 `git add` 目标文件,绝不 `git add -A`** —— 主 worktree 常被并行会话共享。
- schema 新增的是 node 不是 mark,不涉及 mark 声明顺序决定嵌套层级那个坑。

---

### Task 1: 依赖与 schema 节点

**Files:**
- Modify: `moraya-core/package.json`(peerDependencies + devDependencies)
- Modify: `moraya-core/src/schema.ts`(新增 2 个 NodeSpec + 注册进 nodes)
- Test: `moraya-core/src/__tests__/footnote.spec.ts`(新建)

**Interfaces:**
- Consumes: 无
- Produces: schema 节点 `footnote_ref`(inline atom,attrs `{ label: string }`)、`footnote_definition`(block,`content: 'block+'`,attrs `{ label: string }`)。后续任务靠 `schema.nodes.footnote_ref` / `schema.nodes.footnote_definition` 引用。

- [ ] **Step 1: 装依赖**

在 `/Users/bruce/git/moraya-core` 执行:

```bash
pnpm add -D markdown-it-footnote@^4.0.0
```

然后手工编辑 `package.json`,在 `peerDependencies` 里 `markdown-it-deflist` 之后加一行(保持字母序):

```json
    "markdown-it-footnote": "^4.0.0",
```

> 为什么同时进 peerDependencies:tsup 自动把 peerDeps external 化,所以 dist 里保留 `import fnPlugin from "markdown-it-footnote"` 而不内联。运行时由 `moraya-core/node_modules` 解析(`markdown-it-deflist` 同理),**mdeditor 侧 package.json 无需改动**。

- [ ] **Step 2: 写失败测试**

新建 `src/__tests__/footnote.spec.ts`:

```ts
import { describe, test, expect } from 'vitest'
import { createSchema } from '../schema'
import { BrowserMediaResolver } from '../adapters/browser-media-resolver'

const schema = createSchema({ mediaResolver: new BrowserMediaResolver() })

describe('footnote schema', () => {
  test('footnote_ref is an inline atom carrying a label', () => {
    const type = schema.nodes.footnote_ref
    expect(type).toBeDefined()
    expect(type.isInline).toBe(true)
    expect(type.isAtom).toBe(true)
    const node = type.create({ label: 'loop' })
    expect(node.attrs.label).toBe('loop')
  })

  test('footnote_ref renders as <sup> with data-label', () => {
    const node = schema.nodes.footnote_ref.create({ label: 'loop' })
    const [tag, attrs] = schema.nodes.footnote_ref.spec.toDOM!(node) as [string, Record<string, string>]
    expect(tag).toBe('sup')
    expect(attrs['data-label']).toBe('loop')
    expect(attrs.class).toBe('moraya-footnote-ref')
  })

  test('footnote_definition is a block node accepting block content', () => {
    const type = schema.nodes.footnote_definition
    expect(type).toBeDefined()
    expect(type.isBlock).toBe(true)
    const para = schema.nodes.paragraph.create(null, schema.text('循环的注释。'))
    const def = type.create({ label: 'loop' }, para)
    expect(def.attrs.label).toBe('loop')
    expect(def.childCount).toBe(1)
  })
})
```

- [ ] **Step 3: 运行测试,确认失败**

```bash
cd /Users/bruce/git/moraya-core && pnpm vitest run src/__tests__/footnote.spec.ts
```

预期:FAIL —— `schema.nodes.footnote_ref` 为 `undefined`,`expect(type).toBeDefined()` 不通过。

- [ ] **Step 4: 加两个 NodeSpec**

在 `src/schema.ts` 的 `note_anchor`(约 583 行)定义之后插入:

```ts
// ── Footnote NodeSpecs ──────────────────────────────────────────

// `[^label]` 引用。atom + 无 content:角标是不可编辑的整体。
// 注意 attrs 里**只有 label,没有编号** —— 编号是按文档顺序派生的渲染层产物,
// 由 footnote-plugin 用 Decoration 挂 data-num,写进 attrs 就会污染磁盘语义。
const footnote_ref: NodeSpec = {
  group: 'inline',
  inline: true,
  atom: true,
  selectable: true,
  attrs: { label: { default: '' } },
  parseDOM: [{
    tag: 'sup[data-footnote-ref]',
    getAttrs(dom: HTMLElement) { return { label: dom.dataset.label ?? '' } },
  }],
  toDOM(node) {
    return ['sup', {
      'data-footnote-ref': '',
      'data-label': node.attrs.label as string,
      class: 'moraya-footnote-ref',
      contenteditable: 'false',
    }]
  },
}

// `[^label]: 内容` 定义。content 为 block+ 而非 inline*,因为脚注定义
// 允许缩进续行构成多个段落(markdown-it-footnote 会解析成多个 paragraph)。
const footnote_definition: NodeSpec = {
  group: 'block',
  content: 'block+',
  defining: true,
  attrs: { label: { default: '' } },
  parseDOM: [{
    tag: 'div[data-footnote-def]',
    getAttrs(dom: HTMLElement) { return { label: dom.dataset.label ?? '' } },
  }],
  toDOM(node) {
    return ['div', {
      'data-footnote-def': '',
      'data-label': node.attrs.label as string,
      class: 'moraya-footnote-def',
    }, 0]
  },
}
```

然后在 `nodes` 对象(约 1012 行)里 `note_anchor,` 之后加两行:

```ts
    note_anchor,
    footnote_ref,
    footnote_definition,
```

- [ ] **Step 5: 运行测试,确认通过**

```bash
cd /Users/bruce/git/moraya-core && pnpm vitest run src/__tests__/footnote.spec.ts
```

预期:3 个测试全部 PASS。

- [ ] **Step 6: 提交**

```bash
cd /Users/bruce/git/moraya-core
git add package.json pnpm-lock.yaml src/schema.ts src/__tests__/footnote.spec.ts
git commit -m "feat(schema): footnote_ref / footnote_definition 节点

编号不进 attrs —— 派生值落盘会污染磁盘语义。"
```

---

### Task 2: 解析与序列化 —— 往返保真

这是全计划的核心交付。做完这一步,§1 记录的三种数据破坏应当全部消失。

**Files:**
- Modify: `moraya-core/src/markdown.ts`(引入插件 + 禁 2 规则 + 兜底规则 + parserTokens + serializer)
- Modify: `moraya-core/src/__tests__/footnote.spec.ts`
- Create: `moraya-core/src/__tests__/fixtures/footnote.md`

**Interfaces:**
- Consumes: Task 1 的 `footnote_ref` / `footnote_definition` 节点
- Produces: `parseMarkdown` 认识脚注;`serializeMarkdown` 写回 `[^label]` 与 `[^label]: 内容`

- [ ] **Step 1: 写失败的往返测试**

追加到 `src/__tests__/footnote.spec.ts`:

```ts
import { parseMarkdown, serializeMarkdown } from '../markdown'

/** 往返必须字节相等 —— 这是本功能存在的理由。 */
function expectByteStable(src: string) {
  expect(serializeMarkdown(parseMarkdown(src))).toBe(src.trimEnd())
}

describe('footnote roundtrip byte-fidelity', () => {
  test('中文定义(无空格) —— 曾被当作链接引用定义吃掉', () => {
    expectByteStable('脚注[^loop]。\n\n[^loop]: 循环的注释。\n')
  })

  test('英文定义(有空格) —— 曾被加反斜杠', () => {
    expectByteStable('text[^loop] here.\n\n[^loop]: This is a loop note.\n')
  })

  test('裸引用无定义 —— 曾被加反斜杠', () => {
    expectByteStable('只有引用[^loop]，没有定义。\n')
  })

  test('孤儿定义 —— markdown-it-footnote 的 footnote_tail 会删掉它', () => {
    expectByteStable('正文没有引用。\n\n[^orphan]: 孤儿定义内容。\n')
  })

  test('定义写在引用之前 —— 位置必须原地不动', () => {
    expectByteStable('[^a]: 先定义。\n\n后引用[^a]。\n')
  })

  test('同一 label 引用两次', () => {
    expectByteStable('一[^x] 二[^x]。\n\n[^x]: 内容。\n')
  })

  test('多段缩进续行 —— 不得退化成缩进代码块', () => {
    expectByteStable('引用[^m]。\n\n[^m]: 第一段。\n\n    第二段续行。\n')
  })

  test('转义逃逸的字面量不被识别为脚注', () => {
    expectByteStable('字面量 \\[^notafootnote] 保持原样。\n')
  })

  test('内联脚注 ^[...] 按纯文本处理且不被破坏', () => {
    expectByteStable('内联 ^[行内内容] 当纯文本。\n')
  })
})

describe('footnote parse structure', () => {
  test('引用解析为 footnote_ref 并保留原始 label', () => {
    const doc = parseMarkdown('脚注[^loop]。\n\n[^loop]: 循环的注释。\n')
    const refs: string[] = []
    doc.descendants((n) => {
      if (n.type.name === 'footnote_ref') refs.push(n.attrs.label as string)
    })
    expect(refs).toEqual(['loop'])
  })

  test('定义解析为 footnote_definition 且留在原位(不搬到文末)', () => {
    const doc = parseMarkdown('引用[^a]。\n\n[^a]: A 的内容。\n\n后面还有一段正文。\n')
    const types = doc.content.content.map((n) => n.type.name)
    expect(types).toEqual(['paragraph', 'footnote_definition', 'paragraph'])
  })

  test('多段定义包含两个 paragraph 子节点', () => {
    const doc = parseMarkdown('引用[^m]。\n\n[^m]: 第一段。\n\n    第二段续行。\n')
    let def = null as null | typeof doc
    doc.descendants((n) => { if (n.type.name === 'footnote_definition') def = n as never })
    expect(def).not.toBeNull()
    expect(def!.childCount).toBe(2)
  })
})
```

- [ ] **Step 2: 运行测试,确认失败**

```bash
cd /Users/bruce/git/moraya-core && pnpm vitest run src/__tests__/footnote.spec.ts
```

预期:9 个往返测试与 3 个结构测试 FAIL。其中"中文定义"的实际输出是 `脚注[^loop](循环的注释。)。` —— 正是要修的破坏。

- [ ] **Step 3: 接入插件并禁掉两条规则**

在 `src/markdown.ts` 顶部 import 区,`markPlugin` 之后加:

```ts
import footnotePlugin from 'markdown-it-footnote'
```

修改 markdown-it 实例构造(约 33 行),在 `.use(markPlugin)` 后追加 `.use(footnotePlugin)`,并紧接着加两行 disable:

```ts
const md = new MarkdownIt({
  html: true,
  linkify: false,
  typographer: false,
})
  .enable(['table', 'strikethrough'])
  .use(deflistPlugin)
  .use(texmathPlugin)
  .use(markPlugin)
  .use(footnotePlugin)

// markdown-it-footnote 默认把所有定义搬到文末、按引用顺序重排、并**删掉未被引用的
// 定义** —— 这三件事全部由 core 规则 `footnote_tail` 一手包办。它的块规则本身是
// 原位产出 token 的,所以禁掉 tail 就得到原位保真 + 孤儿定义保留。
md.core.ruler.disable('footnote_tail')

// 我们不做内联脚注 `^[内容]`(见 spec §8)。这不是"不支持"而是**必须显式禁用**:
// 留着它会把 `^[内容]` 解析成没有 label 的 footnote_ref,序列化时无从知道写回什么,
// 等于引入一种新的数据破坏。禁掉后它安分地保持纯文本。
md.inline.ruler.disable('footnote_inline')
```

- [ ] **Step 4: 加兜底规则**

紧接上一步,在两行 disable 之后加:

```ts
// markdown-it-footnote 的 `footnote_ref` 规则要求 label 已在 env.footnotes.refs 里
// 登记过(即定义必须存在),因此无定义的裸引用 `[^loop]` 不成节点,会退回纯文本并被
// 序列化器的 esc() 加上反斜杠。兜底规则在它之后接手,产出同名 token,使有无定义都
// 成节点(与 Obsidian 一致)。
md.inline.ruler.after('footnote_ref', 'footnote_ref_orphan', (state, silent) => {
  const src = state.src
  const start = state.pos
  if (src.charCodeAt(start) !== 0x5B /* [ */) return false
  if (src.charCodeAt(start + 1) !== 0x5E /* ^ */) return false

  let pos = start + 2
  for (; pos < state.posMax; pos++) {
    const ch = src.charCodeAt(pos)
    // label 内不允许空格/换行 —— 与 markdown-it-footnote 的 label 规则一致
    if (ch === 0x20 || ch === 0x0A) return false
    if (ch === 0x5D /* ] */) break
  }
  if (pos === start + 2) return false      // 空 label
  if (pos >= state.posMax) return false    // 未闭合

  if (!silent) {
    const tok = state.push('footnote_ref', '', 0)
    tok.meta = { label: src.slice(start + 2, pos) }
  }
  state.pos = pos + 1
  return true
})
```

- [ ] **Step 5: 加 parserTokens 映射**

在 `src/markdown.ts` 的 `parserTokens` 对象里,`critic_note` 条目之后加:

```ts
  // markdown-it-footnote 的引用 token(以及我们的兜底规则)都叫 footnote_ref。
  footnote_ref: {
    node: 'footnote_ref',
    getAttrs: (tok) => ({ label: ((tok.meta as { label?: string } | null)?.label) ?? '' }),
  },
  // 定义的 token 名是 footnote_reference_open/close —— markdown-it-footnote 的
  // 既定命名,不是 footnote_definition_*。
  footnote_reference: {
    block: 'footnote_definition',
    getAttrs: (tok) => ({ label: ((tok.meta as { label?: string } | null)?.label) ?? '' }),
  },
```

> `block:` 规格会自动配对 `footnote_reference_open` / `footnote_reference_close`,无需分别登记。

- [ ] **Step 6: 加序列化**

在 `src/markdown.ts` serializer 的 nodes 表里,`note_anchor`(约 850 行)之后加:

```ts
    footnote_ref(state, node) {
      state.write(`[^${node.attrs.label as string}]`)
    },
    footnote_definition(state, node) {
      state.write(`[^${node.attrs.label as string}]: `)
      // 第一段紧跟冒号,后续段落缩进 4 空格 —— 标准脚注续行写法。
      // delim 传 '    ' 让 wrapBlock 给第二段起加缩进;firstDelim 传 '' 因为
      // 首段的前缀已经由上面的 write 写过了。
      state.wrapBlock('    ', '', node, () => state.renderContent(node))
    },
```

- [ ] **Step 7: 运行测试,确认通过**

```bash
cd /Users/bruce/git/moraya-core && pnpm vitest run src/__tests__/footnote.spec.ts
```

预期:12 个测试全部 PASS。

> 若"多段缩进续行"用例仍失败,问题几乎一定出在 Step 6 的 `wrapBlock` 参数上。用
> `node -e` 打印 `serializeMarkdown(parseMarkdown('引用[^m]。\n\n[^m]: 第一段。\n\n    第二段续行。\n'))`
> 看实际缩进,对照调整 delim。不要改测试去迁就实现。

- [ ] **Step 8: 加 roundtrip fixture**

新建 `src/__tests__/fixtures/footnote.md`(纳入既有的二次往返 CI gate,该目录下 `*.md` 被自动扫描):

```markdown
# 脚注

正文里引用一下脚注[^loop]，同一条再引一次[^loop]。

中间隔一段正文，定义就留在它原本的位置。

[^loop]: 循环的注释内容。

英文脚注[^en] 与多段脚注[^multi]。

[^en]: This is a loop note with spaces.

[^multi]: 第一段。

    第二段续行。

[^orphan]: 没有任何引用的孤儿定义，必须原样保留。

裸引用[^undefined] 没有定义，也不该被加反斜杠。

字面量 \[^notafootnote] 与内联 ^[行内内容] 都按纯文本处理。
```

- [ ] **Step 9: 跑全量测试,确认没打破既有行为**

```bash
cd /Users/bruce/git/moraya-core && pnpm test
```

预期:全绿。特别关注 `roundtrip.spec.ts`(新 fixture 要过二次往返稳定性)与 `api-contract.spec.ts`。

> 若既有 fixture 出现回归,最可能的原因是兜底规则误吞了本该是链接的 `[...]`。
> 回到 Step 4 收紧条件,不要放宽测试。

- [ ] **Step 10: 提交**

```bash
cd /Users/bruce/git/moraya-core
git add src/markdown.ts src/__tests__/footnote.spec.ts src/__tests__/fixtures/footnote.md
git commit -m "feat(markdown): 脚注 [^label] 解析与序列化,往返字节保真

禁 footnote_tail 换取定义原位 + 孤儿定义保留;禁 footnote_inline 避免
无 label 的 ref 造成新破坏;补兜底规则让无定义的裸引用也成节点。

修复:[^x]: 内容 曾被当作 CommonMark 链接引用定义吃掉,脚注定义永久丢失。"
```

---

### Task 3: 角标编号(Decoration)

**Files:**
- Create: `moraya-core/src/plugins/footnote-plugin.ts`
- Modify: `moraya-core/src/setup.ts`(注册插件)
- Test: `moraya-core/src/__tests__/footnote-plugin.spec.ts`(新建)

**Interfaces:**
- Consumes: Task 1 的 `footnote_ref` 节点
- Produces: `createFootnotePlugin(): Plugin`(默认导出为具名导出),给每个 `footnote_ref` 挂 `data-num` 属性

- [ ] **Step 1: 写失败测试**

新建 `src/__tests__/footnote-plugin.spec.ts`:

```ts
import { describe, test, expect } from 'vitest'
import { EditorState } from 'prosemirror-state'
import { DecorationSet } from 'prosemirror-view'
import { parseMarkdown } from '../markdown'
import { createSchema } from '../schema'
import { BrowserMediaResolver } from '../adapters/browser-media-resolver'
import { createFootnotePlugin, footnotePluginKey } from '../plugins/footnote-plugin'

const schema = createSchema({ mediaResolver: new BrowserMediaResolver() })

function decosFor(src: string): string[] {
  const doc = parseMarkdown(src, schema)
  const state = EditorState.create({ doc, plugins: [createFootnotePlugin()] })
  const set = footnotePluginKey.getState(state) as DecorationSet
  return set.find().map((d) => (d as unknown as { type: { attrs: Record<string, string> } }).type.attrs['data-num'])
}

describe('footnote numbering', () => {
  test('按首次出现顺序编号', () => {
    expect(decosFor('甲[^a] 乙[^b]。\n\n[^a]: A。\n\n[^b]: B。\n')).toEqual(['1', '2'])
  })

  test('同一 label 多次引用共用同一编号', () => {
    expect(decosFor('一[^x] 二[^x] 三[^y]。\n\n[^x]: X。\n\n[^y]: Y。\n')).toEqual(['1', '1', '2'])
  })

  test('编号按正文出现顺序,与定义书写顺序无关', () => {
    expect(decosFor('先[^second] 后[^first]。\n\n[^first]: 1。\n\n[^second]: 2。\n')).toEqual(['1', '2'])
  })

  test('无定义的裸引用照样参与编号', () => {
    expect(decosFor('裸[^none]。\n')).toEqual(['1'])
  })
})
```

- [ ] **Step 2: 运行测试,确认失败**

```bash
cd /Users/bruce/git/moraya-core && pnpm vitest run src/__tests__/footnote-plugin.spec.ts
```

预期:FAIL —— 模块 `../plugins/footnote-plugin` 不存在。

- [ ] **Step 3: 实现插件**

新建 `src/plugins/footnote-plugin.ts`:

```ts
/**
 * Footnote plugin — 角标编号与交互。
 *
 * 编号是**派生值**:按 `footnote_ref` 在正文中首次出现的顺序给每个 label 分配序号,
 * 同一 label 的多次引用共用一个编号。它不进节点 attrs(那会污染磁盘语义),而是每次
 * 文档变化时重算并以 Decoration 的形式挂上 `data-num`,由 CSS `content: attr(data-num)`
 * 渲染出来。
 *
 * Schema-agnostic:通过 `node.type.name` 判定,不引用 schema 单例。
 */

import { Plugin, PluginKey } from 'prosemirror-state'
import type { EditorState } from 'prosemirror-state'
import { Decoration, DecorationSet } from 'prosemirror-view'
import type { Node as PmNode } from 'prosemirror-model'

export const footnotePluginKey = new PluginKey('moraya-footnote')

/** 按首次出现顺序给每个 label 编号,并为每个引用生成一个 data-num decoration。 */
function buildDecorations(doc: PmNode): DecorationSet {
  const numByLabel = new Map<string, number>()
  const decos: Decoration[] = []

  doc.descendants((node, pos) => {
    if (node.type.name !== 'footnote_ref') return
    const label = (node.attrs.label as string) || ''
    let num = numByLabel.get(label)
    if (num === undefined) {
      num = numByLabel.size + 1
      numByLabel.set(label, num)
    }
    decos.push(Decoration.node(pos, pos + node.nodeSize, { 'data-num': String(num) }))
  })

  return DecorationSet.create(doc, decos)
}

export function createFootnotePlugin(): Plugin {
  return new Plugin({
    key: footnotePluginKey,
    state: {
      init(_config, state: EditorState) {
        return buildDecorations(state.doc)
      },
      apply(tr, old: DecorationSet) {
        // 文档没变就复用,避免每次光标移动都全文扫描。
        return tr.docChanged ? buildDecorations(tr.doc) : old
      },
    },
    props: {
      decorations(state) {
        return footnotePluginKey.getState(state) as DecorationSet
      },
    },
  })
}
```

- [ ] **Step 4: 运行测试,确认通过**

```bash
cd /Users/bruce/git/moraya-core && pnpm vitest run src/__tests__/footnote-plugin.spec.ts
```

预期:4 个测试全部 PASS。

- [ ] **Step 5: 注册进 setup.ts**

在 `src/setup.ts` 的 import 区(约 61 行,`createEditorPropsPlugin` 之后)加:

```ts
import { createFootnotePlugin } from './plugins/footnote-plugin'
```

在 `createEditorPlugins` 里,`plugins.push(createInlineCodeConvertPlugin(...))`(约 841 行)之后加:

```ts
  plugins.push(createFootnotePlugin())
```

- [ ] **Step 6: 跑全量测试**

```bash
cd /Users/bruce/git/moraya-core && pnpm test
```

预期:全绿。注意 `plugin-order.spec.ts` 有插件顺序指纹快照 —— 若它失败,是因为新增插件改变了顺序,**确认新顺序符合预期后**用 `pnpm vitest run -u` 更新快照,并在提交信息里说明。

- [ ] **Step 7: 提交**

```bash
cd /Users/bruce/git/moraya-core
git add src/plugins/footnote-plugin.ts src/setup.ts src/__tests__/footnote-plugin.spec.ts
git commit -m "feat(footnote): 角标编号 decoration

编号按首次出现顺序派生,同 label 共用;不落 attrs。"
```

---

### Task 4: hover 浮层

**Files:**
- Modify: `moraya-core/src/plugins/footnote-plugin.ts`
- Test: `moraya-core/src/__tests__/footnote-plugin.spec.ts`

**Interfaces:**
- Consumes: Task 3 的 `createFootnotePlugin`
- Produces: 导出 `findDefinition(doc, label): { node, pos } | null`,Task 5 的点击跳转复用它

- [ ] **Step 1: 写失败测试**

追加到 `src/__tests__/footnote-plugin.spec.ts`:

```ts
import { findDefinition, definitionText } from '../plugins/footnote-plugin'

describe('footnote definition lookup', () => {
  test('按 label 找到定义节点及其位置', () => {
    const doc = parseMarkdown('引用[^a]。\n\n[^a]: A 的内容。\n', schema)
    const hit = findDefinition(doc, 'a')
    expect(hit).not.toBeNull()
    expect(hit!.node.type.name).toBe('footnote_definition')
    expect(doc.nodeAt(hit!.pos)!.attrs.label).toBe('a')
  })

  test('label 不存在时返回 null', () => {
    const doc = parseMarkdown('裸引用[^none]。\n', schema)
    expect(findDefinition(doc, 'none')).toBeNull()
  })

  test('definitionText 取出定义的纯文本', () => {
    const doc = parseMarkdown('引用[^m]。\n\n[^m]: 第一段。\n\n    第二段。\n', schema)
    expect(definitionText(doc, 'm')).toBe('第一段。 第二段。')
  })

  test('无定义时 definitionText 返回空串', () => {
    const doc = parseMarkdown('裸[^none]。\n', schema)
    expect(definitionText(doc, 'none')).toBe('')
  })
})
```

- [ ] **Step 2: 运行测试,确认失败**

```bash
cd /Users/bruce/git/moraya-core && pnpm vitest run src/__tests__/footnote-plugin.spec.ts
```

预期:FAIL —— `findDefinition` / `definitionText` 未导出。

- [ ] **Step 3: 实现查找函数**

在 `src/plugins/footnote-plugin.ts` 的 `buildDecorations` 之前加:

```ts
/** 按 label 查找定义节点。找不到返回 null。 */
export function findDefinition(doc: PmNode, label: string): { node: PmNode; pos: number } | null {
  let hit: { node: PmNode; pos: number } | null = null
  doc.descendants((node, pos) => {
    if (hit) return false
    if (node.type.name === 'footnote_definition' && node.attrs.label === label) {
      hit = { node, pos }
      return false
    }
    return true
  })
  return hit
}

/** 定义的纯文本,用于 hover 浮层。多段之间用空格连接。 */
export function definitionText(doc: PmNode, label: string): string {
  const hit = findDefinition(doc, label)
  if (!hit) return ''
  const parts: string[] = []
  hit.node.forEach((child) => { parts.push(child.textContent) })
  return parts.join(' ').trim()
}
```

- [ ] **Step 4: 运行测试,确认通过**

```bash
cd /Users/bruce/git/moraya-core && pnpm vitest run src/__tests__/footnote-plugin.spec.ts
```

预期:8 个测试全部 PASS。

- [ ] **Step 5: 接上 hover 浮层**

在 `createFootnotePlugin` 的 `props` 里,`decorations` 之后加 `handleDOMEvents`:

```ts
      handleDOMEvents: {
        mouseover(view, event) {
          const target = (event.target as HTMLElement | null)?.closest?.('[data-footnote-ref]')
          if (!(target instanceof HTMLElement)) return false
          const label = target.dataset.label ?? ''
          const text = definitionText(view.state.doc, label)
          // title 属性即原生 tooltip:零依赖、跟随系统样式、不会和编辑器
          // 自己的浮层管理打架。无定义时明确提示,而不是静默空白。
          target.title = text ? `[^${label}] ${text}` : `[^${label}] (未定义)`
          return false
        },
      },
```

> 用原生 `title` 而不是自绘浮层:脚注 hover 是低频只读交互,自绘浮层要处理定位、
> 边界、滚动跟随、销毁时机,不值当。若后续要富文本预览再升级。

- [ ] **Step 6: 跑全量测试并提交**

```bash
cd /Users/bruce/git/moraya-core && pnpm test
git add src/plugins/footnote-plugin.ts src/__tests__/footnote-plugin.spec.ts
git commit -m "feat(footnote): hover 显示 label 与定义内容"
```

---

### Task 5: 点击跳转与高亮

**Files:**
- Modify: `moraya-core/src/plugins/footnote-plugin.ts`
- Test: `moraya-core/src/__tests__/footnote-plugin.spec.ts`

**Interfaces:**
- Consumes: Task 4 的 `findDefinition`
- Produces: 点击角标滚动到定义并加 `moraya-footnote-flash` 类;定义块可点击回跳到首个引用

- [ ] **Step 1: 写失败测试**

追加到 `src/__tests__/footnote-plugin.spec.ts`:

```ts
import { findFirstRef } from '../plugins/footnote-plugin'

describe('footnote back-reference lookup', () => {
  test('按 label 找到首个引用的位置', () => {
    const doc = parseMarkdown('一[^x] 二[^x]。\n\n[^x]: X。\n', schema)
    const first = findFirstRef(doc, 'x')
    expect(first).not.toBeNull()
    expect(doc.nodeAt(first!.pos)!.type.name).toBe('footnote_ref')
  })

  test('孤儿定义没有引用时返回 null', () => {
    const doc = parseMarkdown('正文。\n\n[^orphan]: 孤儿。\n', schema)
    expect(findFirstRef(doc, 'orphan')).toBeNull()
  })
})
```

- [ ] **Step 2: 运行测试,确认失败**

```bash
cd /Users/bruce/git/moraya-core && pnpm vitest run src/__tests__/footnote-plugin.spec.ts
```

预期:FAIL —— `findFirstRef` 未导出。

- [ ] **Step 3: 实现 findFirstRef**

在 `findDefinition` 之后加:

```ts
/** 按 label 查找首个引用节点,用于从定义块回跳。找不到返回 null。 */
export function findFirstRef(doc: PmNode, label: string): { node: PmNode; pos: number } | null {
  let hit: { node: PmNode; pos: number } | null = null
  doc.descendants((node, pos) => {
    if (hit) return false
    if (node.type.name === 'footnote_ref' && node.attrs.label === label) {
      hit = { node, pos }
      return false
    }
    return true
  })
  return hit
}
```

- [ ] **Step 4: 运行测试,确认通过**

```bash
cd /Users/bruce/git/moraya-core && pnpm vitest run src/__tests__/footnote-plugin.spec.ts
```

预期:10 个测试全部 PASS。

- [ ] **Step 5: 接上点击跳转**

在 `createFootnotePlugin` 的 `handleDOMEvents` 里,`mouseover` 之后加 `mousedown`:

```ts
        mousedown(view, event) {
          const el = event.target as HTMLElement | null
          const refEl = el?.closest?.('[data-footnote-ref]')
          const defEl = el?.closest?.('[data-footnote-def]')

          // 角标 → 跳到定义
          if (refEl instanceof HTMLElement) {
            const hit = findDefinition(view.state.doc, refEl.dataset.label ?? '')
            if (!hit) return false
            event.preventDefault()
            scrollToAndFlash(view, hit.pos)
            return true
          }

          // 定义块 → 回跳到首个引用。只认定义块自己的空白区域,
          // 不拦截其中的文字,否则定义内容没法正常编辑和选中。
          if (defEl instanceof HTMLElement && el === defEl) {
            const hit = findFirstRef(view.state.doc, defEl.dataset.label ?? '')
            if (!hit) return false
            event.preventDefault()
            scrollToAndFlash(view, hit.pos)
            return true
          }

          return false
        },
```

在文件末尾(`createFootnotePlugin` 之外)加辅助函数:

```ts
/** 滚动到指定位置并短暂高亮。 */
function scrollToAndFlash(view: EditorView, pos: number): void {
  const dom = view.nodeDOM(pos)
  const el = dom instanceof HTMLElement ? dom : (dom as ChildNode | null)?.parentElement
  if (!el) return
  el.scrollIntoView({ behavior: 'smooth', block: 'center' })
  el.classList.add('moraya-footnote-flash')
  view.dom.ownerDocument.defaultView?.setTimeout(() => {
    el.classList.remove('moraya-footnote-flash')
  }, 1200)
}
```

并在 import 区补上类型:

```ts
import type { EditorView } from 'prosemirror-view'
```

- [ ] **Step 6: 跑全量测试并提交**

```bash
cd /Users/bruce/git/moraya-core && pnpm test
git add src/plugins/footnote-plugin.ts src/__tests__/footnote-plugin.spec.ts
git commit -m "feat(footnote): 点击角标跳到定义,定义块回跳引用"
```

---

### Task 6: 宿主样式与实机验证

**Files:**
- Modify: `mdeditor/src/styles/editor-base.css`
- 构建:`moraya-core` → tsup;`mdeditor` → `pnpm sync:core`

**Interfaces:**
- Consumes: Task 1 的 `moraya-footnote-ref` / `moraya-footnote-def` 类名、Task 3 的 `data-num`、Task 5 的 `moraya-footnote-flash`
- Produces: 可视角标与定义块样式

- [ ] **Step 1: 构建 core 并同步**

```bash
cd /Users/bruce/git/moraya-core && pnpm build
cd /Users/bruce/git/mdeditor && pnpm sync:core
```

预期:tsup 构建成功,`sync:core` 打印 `synced+vite-cache-cleared`。

- [ ] **Step 2: 写样式**

在 `mdeditor/src/styles/editor-base.css` 的 note-anchor 规则块之后(约 90 行,深色模式块之前)插入:

```css
/* 脚注角标。上标定位刻意抄 .moraya-note-anchor 的做法:display:inline + line-height:0
   + 相对偏移。绝不能用 inline-block 或 vertical-align:super —— 那会让非替换 inline
   盒按 margin 盒高度参与布局、凭空给行框加出下伸部,大标题和表格里行高被撑大。
   <sup> 浏览器默认就是 vertical-align:super,所以这里必须显式覆盖。 */
.moraya-editor .moraya-footnote-ref {
  display: inline;
  line-height: 0;
  vertical-align: baseline;
  position: relative;
  top: -0.4em;
  font-size: 0.72em;
  color: #3b7dd8;
  cursor: pointer;
  user-select: none;
  margin: 0 1px;
}
/* 编号由 footnote-plugin 以 decoration 形式挂在 data-num 上,不落盘。
   缺 data-num(插件未启用)时回退显示 label,不至于变成一个看不见的空角标。 */
.moraya-editor .moraya-footnote-ref::before {
  content: '[' attr(data-num) ']';
}
.moraya-editor .moraya-footnote-ref:not([data-num])::before {
  content: '[^' attr(data-label) ']';
}
.moraya-editor .moraya-footnote-ref:hover {
  color: #2a5da8;
  text-decoration: underline;
}

/* 脚注定义块 */
.moraya-editor .moraya-footnote-def {
  position: relative;
  padding-left: 1.2em;
  border-left: 2px solid rgba(59, 125, 216, 0.35);
  color: rgba(0, 0, 0, 0.72);
  font-size: 0.92em;
}
.moraya-editor .moraya-footnote-def::before {
  content: '[^' attr(data-label) ']';
  position: absolute;
  left: 0;
  transform: translateX(-100%);
  padding-right: 0.4em;
  color: #3b7dd8;
  font-size: 0.85em;
  cursor: pointer;
  user-select: none;
}

/* 跳转后的短暂高亮 */
.moraya-editor .moraya-footnote-flash {
  animation: moraya-footnote-flash 1.2s ease-out;
}
@keyframes moraya-footnote-flash {
  0% { background: rgba(59, 125, 216, 0.28); }
  100% { background: transparent; }
}

@media (prefers-color-scheme: dark) {
  .moraya-editor .moraya-footnote-ref { color: #6fa8ec; }
  .moraya-editor .moraya-footnote-ref:hover { color: #9ac6f5; }
  .moraya-editor .moraya-footnote-def {
    border-left-color: rgba(111, 168, 236, 0.4);
    color: rgba(255, 255, 255, 0.74);
  }
  .moraya-editor .moraya-footnote-def::before { color: #6fa8ec; }
  .moraya-editor .moraya-footnote-flash {
    animation-name: moraya-footnote-flash-dark;
  }
  @keyframes moraya-footnote-flash-dark {
    0% { background: rgba(111, 168, 236, 0.3); }
    100% { background: transparent; }
  }
}
```

- [ ] **Step 3: 类型检查与构建**

```bash
cd /Users/bruce/git/mdeditor && pnpm check && pnpm build
```

预期:`svelte-check` 零错误,`vite build` 成功。

- [ ] **Step 4: 起 dev 供实机验证**

```bash
cd /Users/bruce/git/mdeditor && pnpm tauri dev
```

**GUI 由用户实机验证** —— 不跑 osascript 自动化。给出下面的手动清单:

准备一个测试文件,内容用 Task 2 Step 8 的 fixture。在 rich 模式下逐条确认:

1. `[^loop]` 显示为蓝色上标 `[1]`,同一 label 第二次引用也是 `[1]`
2. `[^en]` 是 `[2]`、`[^multi]` 是 `[3]`、`[^undefined]` 是 `[4]`
3. 悬停角标出现 tooltip:`[^loop] 循环的注释内容。`;悬停 `[^undefined]` 显示 `(未定义)`
4. 点击角标滚动到对应定义并闪一下蓝底
5. 点击定义块左侧的 `[^loop]` 标记回跳到正文首个引用
6. **大标题里插一个脚注角标,确认行高没有被撑大**(这是 inline-block 坑的回归点)
7. 表格单元格里插一个角标,同样确认行高正常
8. 切到 source 模式再切回 rich,角标正常
9. **改一个字后保存,用 `git diff` 确认脚注相关的行一个字节都没变**(最关键)
10. 深色模式下角标与定义块配色正常

- [ ] **Step 5: 提交**

```bash
cd /Users/bruce/git/mdeditor
git add src/styles/editor-base.css
git commit -m "feat(footnote): rich 模式脚注角标与定义块样式

上标定位沿用 note-anchor 的 inline + line-height:0 方案,
避免 inline-block/vertical-align:super 撑大标题与表格行高。"
```

---

## 收尾

两个仓库各自有提交,`moraya-core` 需要单独 push:

```bash
cd /Users/bruce/git/moraya-core && git push origin main
```

mdeditor 侧是否发版由用户决定 —— 本功能依赖 `moraya-core` 的 dist,发版前确认 `pnpm sync:core` 已执行且 `dist` 变更已提交(该仓库把 `dist/` 纳入版本控制,见既有提交 `a9dd81c build: rebuild dist for annotation mark-order fix`)。
