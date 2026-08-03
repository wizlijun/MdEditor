// src/lib/outline/markdown.test.ts
import { describe, it, expect, vi, afterEach } from 'vitest'
import { serializeOutline, parseOutline } from './markdown'
import { createTree, addNode, answerBodyOf, type OutlineTree } from './model'

function roundTrip(md: string): string {
  return serializeOutline(parseOutline(md))
}

describe('parseOutline', () => {
  it('parses nesting by 2-space indent', () => {
    const t = parseOutline('- A\n  - A1\n    - A1a\n- B\n')
    const ids = [...t.nodes.values()]
    expect(ids).toHaveLength(4)
    const a = ids.find(n => n.content === 'A')!
    const a1 = ids.find(n => n.content === 'A1')!
    const a1a = ids.find(n => n.content === 'A1a')!
    expect(a.parentId).toBeNull()
    expect(a1.parentId).toBe(a.id)
    expect(a1a.parentId).toBe(a1.id)
  })
  it('reads property lines', () => {
    const md = '- Chapter\n  type:: toc\n  line:: 12\n  collapsed:: true\n  id:: abc-123\n'
    const n = [...parseOutline(md).nodes.values()][0]
    expect(n.source).toBe('toc')
    expect(n.anchorLine).toBe(12)
    expect(n.collapsed).toBe(true)
    expect(n.id).toBe('abc-123')
  })
  it('joins continuation lines into multi-line content', () => {
    const md = '- ```js\n  const x = 1\n  ```\n- next\n'
    const nodes = [...parseOutline(md).nodes.values()]
    expect(nodes[0].content).toBe('```js\nconst x = 1\n```')
    expect(nodes[1].content).toBe('next')
  })
  it('degrades unparseable lines to plain manual nodes (spec: 不丢内容)', () => {
    const t = parseOutline('stray text no bullet\n- ok\n')
    const contents = [...t.nodes.values()].map(n => n.content)
    expect(contents).toContain('stray text no bullet')
    expect(contents).toContain('ok')
  })
})

describe('serializeOutline', () => {
  it('writes only non-default props', () => {
    const t = createTree()
    addNode(t, { id: 'm', parentId: null, order: 0, content: 'hand', collapsed: false, source: 'manual' })
    addNode(t, { id: 'h', parentId: null, order: 100, content: 'marked', collapsed: false, source: 'highlight', anchorLine: 3 })
    const md = serializeOutline(t)
    expect(md).toBe('- hand\n- marked\n  type:: highlight\n  line:: 3\n')
  })
  it('persists manual node id only when flagged', () => {
    const t = createTree()
    addNode(t, { id: 'x-1', parentId: null, order: 0, content: 'ref target', collapsed: false, source: 'manual' })
    expect(serializeOutline(t)).not.toContain('id::')
    expect(serializeOutline(t, new Set(['x-1']))).toContain('id:: x-1')
  })
})

describe('round-trip（验收标准 2）', () => {
  it('lossless: nesting + props + multi-line + special chars', () => {
    const md = [
      '- Title',
      '  type:: toc',
      '  line:: 1',
      '  - ^^note^^ with [[link]] and #tag',
      '    type:: highlight',
      '    line:: 4',
      '    id:: h-1',
      '    collapsed:: true',
      '    - my thought **bold** `code`',
      '- ```py',
      '  print("hi :: not a prop")',
      '  ```',
      '',
    ].join('\n')
    expect(roundTrip(md)).toBe(md)
  })
})

describe('created/updated timestamps', () => {
  it('round-trips created:: and updated:: property lines', () => {
    const md = '- note\n  created:: 2026-07-10T01:02:03.000Z\n  updated:: 2026-07-10T04:05:06.000Z\n'
    const t = parseOutline(md)
    const n = [...t.nodes.values()][0]
    expect(n.createdAt).toBe('2026-07-10T01:02:03.000Z')
    expect(n.updatedAt).toBe('2026-07-10T04:05:06.000Z')
    expect(serializeOutline(t)).toBe(md)
  })
  it('omits timestamp lines when fields are absent', () => {
    expect(roundTrip('- plain\n')).toBe('- plain\n')
  })
})

describe('wikilink node type', () => {
  it('round-trips type:: wikilink', () => {
    const md = '- [[Page]]\n  type:: wikilink\n  line:: 3\n  created:: 2026-07-10T00:00:00.000Z\n'
    const n = [...parseOutline(md).nodes.values()][0]
    expect(n.source).toBe('wikilink')
    expect(serializeOutline(parseOutline(md))).toBe(md)
  })
})

describe('front-matter', () => {
  const fm = 'title: 我的笔记\ncreated: 2026-07-10T08:00:00.000Z\nroam-uid: abc'
  it('parseOutline extracts leading YAML block into tree.frontmatter', () => {
    const t = parseOutline(`---\n${fm}\n---\n- A\n`)
    expect(t.frontmatter).toBe(fm)
    expect([...t.nodes.values()].map(n => n.content)).toEqual(['A'])
  })
  it('round-trips front-matter byte-exact (unknown keys preserved)', () => {
    const md = `---\n${fm}\n---\n- A\n  - B\n`
    expect(roundTrip(md)).toBe(md)
  })
  it('no front-matter → tree.frontmatter is null, output unchanged', () => {
    const t = parseOutline('- A\n')
    expect(t.frontmatter).toBeNull()
    expect(roundTrip('- A\n')).toBe('- A\n')
  })
  it('serializes front-matter even when body is empty', () => {
    const t = parseOutline(`---\n${fm}\n---\n`)
    expect(serializeOutline(t)).toBe(`---\n${fm}\n---\n`)
  })
  it('a lone --- line in body is not front-matter', () => {
    const t = parseOutline('- A\n---\n')
    expect(t.frontmatter).toBeNull()
  })
})

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

describe('property value robustness (file-over-app: tolerate trailing whitespace)', () => {
  it('parses type:: question even with trailing whitespace on the line', () => {
    const t = parseOutline('- q?\n  type:: question  \n')
    expect([...t.nodes.values()][0].source).toBe('question')
  })
  it('parses a status value that carries trailing whitespace', () => {
    const t = parseOutline('- q?\n  type:: question\n  status:: answered  \n')
    expect([...t.nodes.values()][0].status).toBe('answered')
  })
  it('trims trailing whitespace off created/updated timestamps', () => {
    const t = parseOutline('- q?\n  type:: question\n  created:: 2026-07-28T03:01:01.041Z  \n')
    expect([...t.nodes.values()][0].createdAt).toBe('2026-07-28T03:01:01.041Z')
  })
  it('self-heals a node that has status:: but lost its type:: line — it is a question', () => {
    // Corrupted-in-the-wild shape: status lingers, type was stripped by external tooling.
    const t = parseOutline('- 判断力吗？\n  status:: open\n')
    const n = [...t.nodes.values()][0]
    expect(n.source).toBe('question')
    expect(n.status).toBe('open')
    // and it re-serializes WITH type:: question (status never travels without type)
    expect(serializeOutline(t)).toContain('type:: question')
  })
  it('does not promote a plain note/manual node without a valid status', () => {
    const t = parseOutline('- 判断力吗？\n  type:: note\n')
    expect([...t.nodes.values()][0].source).toBe('note')
  })
})

describe('duplicate id:: (content-loss guard)', () => {
  // 内容清单:每个用例都断言输入里的每个 block 在 round trip 后原样存在、嵌套不变。
  // 不满足时 Map 重键会把撞车的节点(及其整棵子树)逐出 tree.nodes,
  // childrenOf 只按 map 遍历 —— 该节点静默从下一次 serialize 里消失。

  it('parent + child sharing one id:: keeps both, child falls back to its generated id', () => {
    const md = '- parent\n  id:: dup\n  - child\n    id:: dup\n'
    const t = parseOutline(md)
    const contents = [...t.nodes.values()].map(n => n.content)
    expect(contents).toEqual(expect.arrayContaining(['parent', 'child']))
    const parent = [...t.nodes.values()].find(n => n.content === 'parent')!
    const child = [...t.nodes.values()].find(n => n.content === 'child')!
    expect(child.parentId).toBe(parent.id)
    expect(parent.id).toBe('dup')       // 先到先得
    expect(child.id).not.toBe('dup')    // 撞车:保留自己的生成 id
    expect(child.persistId).not.toBe(true)
    const out = serializeOutline(t)
    expect(out).toBe('- parent\n  id:: dup\n  - child\n')
  })

  it('parent + child + grandchild: duplicate id two levels up does not orphan the grandchild', () => {
    const md = '- parent\n  id:: dup\n  - child\n    id:: dup\n    - grandchild\n'
    const t = parseOutline(md)
    const contents = [...t.nodes.values()].map(n => n.content)
    expect(contents).toEqual(expect.arrayContaining(['parent', 'child', 'grandchild']))
    const parent = [...t.nodes.values()].find(n => n.content === 'parent')!
    const child = [...t.nodes.values()].find(n => n.content === 'child')!
    const grandchild = [...t.nodes.values()].find(n => n.content === 'grandchild')!
    expect(child.parentId).toBe(parent.id)
    expect(grandchild.parentId).toBe(child.id)
    const out = serializeOutline(t)
    expect(out).toBe('- parent\n  id:: dup\n  - child\n    - grandchild\n')
  })

  it('two siblings sharing one id:: — first wins, second keeps its generated id', () => {
    const md = '- first\n  id:: dup\n- second\n  id:: dup\n'
    const t = parseOutline(md)
    const contents = [...t.nodes.values()].map(n => n.content)
    expect(contents).toEqual(expect.arrayContaining(['first', 'second']))
    const out = serializeOutline(t)
    expect(out).toBe('- first\n  id:: dup\n- second\n')
  })

  it('three levels deep: an unrelated ancestor id does not interfere; the deeper duplicate is still guarded', () => {
    const md = '- top\n  id:: t\n  - parent\n    id:: dup\n    - child\n      id:: dup\n'
    const t = parseOutline(md)
    const contents = [...t.nodes.values()].map(n => n.content)
    expect(contents).toEqual(expect.arrayContaining(['top', 'parent', 'child']))
    const top = [...t.nodes.values()].find(n => n.content === 'top')!
    const parent = [...t.nodes.values()].find(n => n.content === 'parent')!
    const child = [...t.nodes.values()].find(n => n.content === 'child')!
    expect(top.id).toBe('t')
    expect(parent.id).toBe('dup')
    expect(parent.parentId).toBe(top.id)
    expect(child.parentId).toBe(parent.id)
    const out = serializeOutline(t)
    expect(out).toBe('- top\n  id:: t\n  - parent\n    id:: dup\n    - child\n')
  })

  // 注意:这条不是回归测试(对修复前的旧实现同样通过) —— 它是给新增
  // holder === current 分支的护栏测试,防止未来把「同一节点重复声明同一个
  // id::」误判成冲突。
  it('a node re-declaring the same id:: twice is not a self-collision (still persists)', () => {
    const md = '- note\n  id:: dup\n  id:: dup\n'
    const t = parseOutline(md)
    const n = [...t.nodes.values()][0]
    expect(n.id).toBe('dup')
    expect(n.persistId).toBe(true)
    expect(serializeOutline(t)).toBe('- note\n  id:: dup\n')
  })

  it('three nodes sharing one id:: — only the first keeps it, the other two fall back and survive', () => {
    const md = '- a\n  id:: dup\n- b\n  id:: dup\n- c\n  id:: dup\n'
    const t = parseOutline(md)
    const contents = [...t.nodes.values()].map(n => n.content)
    expect(contents).toEqual(expect.arrayContaining(['a', 'b', 'c']))
    const out = serializeOutline(t)
    expect(out).toBe('- a\n  id:: dup\n- b\n- c\n')
  })

  describe('collision is logged, not silent', () => {
    afterEach(() => { vi.restoreAllMocks() })

    it('warns exactly once per real collision (not once per line in the file)', () => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      // 3 个节点共享一个 id:: → 2 次真实碰撞(b 撞 a、c 撞 a),不是 3 次(每行一次)
      parseOutline('- a\n  id:: dup\n- b\n  id:: dup\n- c\n  id:: dup\n')
      expect(warn).toHaveBeenCalledTimes(2)
      expect(warn.mock.calls[0][0]).toContain('dup')
    })

    it('does not warn when a node re-declares its own id:: (not a collision)', () => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      parseOutline('- note\n  id:: dup\n  id:: dup\n')
      expect(warn).not.toHaveBeenCalled()
    })
  })
})

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
    '      answered:: 2026-07-28T14:22:00Z',
    '      by:: claude-code',
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

// 一个空块被序列化成 `- ` —— 破折号、空格、然后什么都没有。
// 于是「行尾空格」承载了语义:编辑器、格式化器、git 钩子都会例行删掉它,
// 而 file-over-app 明确把「vault 文件被外部改过」当作常态而非异常。
// 空格被删掉后,`-` 走到「无法归类的行」兜底分支:平铺时降级成内容为 "-" 的根节点
// (再存一次变 `- -`,每次保存多退化一层);嵌套时更狠——子节点整个消失,
// 它的 created::/updated::/id:: 变成父节点内容里的可见文本。
// 修法:解析端把「只有缩进和一个 `-`」的行也认作空 bullet;序列化端一个字节不改。
describe('a bare `-` is an empty bullet (trailing space must not be load-bearing)', () => {
  it('parses `-` alone as an empty bullet, not as content "-"', () => {
    const t = parseOutline('-\n')
    const nodes = [...t.nodes.values()]
    expect(nodes).toHaveLength(1)
    expect(nodes[0].content).toBe('')
  })

  it('attaches following property lines to the empty bullet itself', () => {
    const t = parseOutline('- kept\n-\n  created:: 2026-08-03T14:11:47.891Z\n  id:: X\n')
    const nodes = [...t.nodes.values()]
    expect(nodes.map(n => n.content)).toEqual(['kept', ''])
    // 属性必须落在空 bullet 上,而不是回填给上一个节点
    expect(nodes[0].createdAt).toBeUndefined()
    expect(nodes[1].createdAt).toBe('2026-08-03T14:11:47.891Z')
    expect(nodes[1].id).toBe('X')
  })

  it('keeps a nested empty bullet as a real child (the data-destroying case)', () => {
    const t = parseOutline('- parent\n  -\n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n')
    const nodes = [...t.nodes.values()]
    expect(nodes).toHaveLength(2)
    const parent = nodes.find(n => n.content === 'parent')!
    const child = t.nodes.get('x')!
    expect(child).toBeDefined()
    expect(child.content).toBe('')
    expect(child.parentId).toBe(parent.id)
    expect(child.createdAt).toBe('2026-08-03T14:11:47.891Z')
    // 父节点内容里绝不能出现属性行的字面文本
    expect(parent.content).toBe('parent')
  })

  it('parses `- ` (with the trailing space) exactly as before', () => {
    const withSpace = parseOutline('- parent\n  - \n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n')
    const withoutSpace = parseOutline('- parent\n  -\n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n')
    const shape = (t: OutlineTree) =>
      [...t.nodes.values()].map(n => [n.content, n.parentId === null, n.createdAt])
    expect(shape(withoutSpace)).toEqual(shape(withSpace))
    expect(roundTrip('- parent\n  - \n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n'))
      .toBe('- parent\n  - \n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n')
  })

  it('re-serialises a stripped file back to `- ` and then holds still', () => {
    const stripped = '- parent\n  -\n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n'
    const healed = '- parent\n  - \n    created:: 2026-08-03T14:11:47.891Z\n    id:: x\n'
    // 序列化端未改,故写回仍带行尾空格——但这一步不再丢数据
    expect(roundTrip(stripped)).toBe(healed)
    // 再解析一次稳定:不退化成 `- -`,也不左右摇摆
    expect(roundTrip(healed)).toBe(healed)
    expect(roundTrip(roundTrip(stripped))).toBe(healed)
  })

  it('leaves ---, -- and `- -` alone', () => {
    // 正文里的 --- / -- 仍是无法归类的行,降级成根节点内容,而不是空 bullet
    const rule = [...parseOutline('- A\n---\n').nodes.values()]
    expect(rule.map(n => n.content)).toEqual(['A', '---'])
    const dashes = [...parseOutline('- A\n--\n').nodes.values()]
    expect(dashes.map(n => n.content)).toEqual(['A', '--'])
    // front-matter 围栏仍在 bullet 扫描之前被切走
    const t = parseOutline('---\ntitle: x\n---\n- A\n')
    expect(t.frontmatter).toBe('title: x')
    expect([...t.nodes.values()].map(n => n.content)).toEqual(['A'])
    // `- -` 依然是内容为 "-" 的 bullet
    expect([...parseOutline('- -\n').nodes.values()].map(n => n.content)).toEqual(['-'])
    expect(roundTrip('- -\n')).toBe('- -\n')
  })

  it('an odd-indent `-` behaves exactly like an odd-indent `- x` today', () => {
    // 缩进单位仍是两空格:3 空格的 bullet 今天不是 bullet,而是上一节点的续行。
    // 空 bullet 不得改变这条规则,只是把 `- x` 换成 `-` 而已。
    const oddWithText = [...parseOutline('- A\n   - x\n').nodes.values()]
    expect(oddWithText.map(n => n.content)).toEqual(['A\n - x'])
    const odd = [...parseOutline('- A\n   -\n').nodes.values()]
    expect(odd.map(n => n.content)).toEqual(['A\n -'])
    // 没有上一节点可续行时,两者同样降级为根层节点
    expect([...parseOutline('   - x\n').nodes.values()].map(n => n.content)).toEqual(['- x'])
    expect([...parseOutline('   -\n').nodes.values()].map(n => n.content)).toEqual(['-'])
  })
})
