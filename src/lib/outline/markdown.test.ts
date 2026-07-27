// src/lib/outline/markdown.test.ts
import { describe, it, expect } from 'vitest'
import { serializeOutline, parseOutline } from './markdown'
import { createTree, addNode, type OutlineTree } from './model'

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
