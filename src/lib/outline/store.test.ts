// src/lib/outline/store.test.ts
import { describe, it, expect } from 'vitest'
import { outline, companionPathFor, persistIdsFor, attachDoc, serializeDoc, setChangeSink, markDirty, markSynced, markSaved, detach, isEffectivelyEmptyTree, noteTextHasContent } from './store.svelte'
import { createTree, addNode } from './model'

describe('companionPathFor', () => {
  it('maps main file to sibling .note.md', () => {
    expect(companionPathFor('/d/foo.md')).toBe('/d/foo.note.md')
    expect(companionPathFor('/d/bar.markdown')).toBe('/d/bar.note.md')
  })
  it('keeps the full .mdx suffix so foo.md and foo.mdx do not share one note', () => {
    // index.md 与 index.mdx 共存在 Astro / Next 仓库里不算罕见;去后缀会让
    // 两份文档共用同一份手记且无法分开。
    expect(companionPathFor('/d/foo.mdx')).toBe('/d/foo.mdx.note.md')
    expect(companionPathFor('/d/foo.mdx')).not.toBe(companionPathFor('/d/foo.md'))
  })
  it('null for the mdx companion itself (no recursive derivation)', () => {
    expect(companionPathFor('/d/foo.mdx.note.md')).toBeNull()
  })
  it('null for companion files themselves (new and legacy suffix) and non-md', () => {
    expect(companionPathFor('/d/foo.note.md')).toBeNull()
    expect(companionPathFor('/d/foo.notes.md')).toBeNull()
    expect(companionPathFor('/d/FOO.NOTE.MD')).toBeNull()
    expect(companionPathFor('/d/x.png')).toBeNull()
  })
})

describe('persistIdsFor', () => {
  it('collects block-ref targets and auto nodes with manual children', () => {
    const t = createTree()
    addNode(t, { id: 'toc1', parentId: null, order: 0, content: 'T', collapsed: false, source: 'toc', anchorLine: 1 })
    addNode(t, { id: 'm1', parentId: 'toc1', order: 0, content: 'child', collapsed: false, source: 'manual' })
    addNode(t, { id: 'm2', parentId: null, order: 100, content: 'see ((m1))', collapsed: false, source: 'manual' })
    const ids = persistIdsFor(t)
    expect(ids.has('toc1')).toBe(true)   // auto 带手写子节点 → 保 id
    expect(ids.has('m1')).toBe(true)     // 被 ((m1)) 引用
    expect(ids.has('m2')).toBe(false)
  })
})

describe('attachDoc / serializeDoc', () => {
  it('parses text (front-matter carried) and serializeDoc stamps title/updated', async () => {
    await attachDoc('/v/foo.note.md', '- hello\n', null)
    expect(outline.docPath).toBe('/v/foo.note.md')
    const out = serializeDoc()
    expect(out).toContain('title: foo')
    expect(out).toContain('updated:')
    expect(out).toContain('- hello')
    detach()
  })
  it('derives auto items from main content when provided', async () => {
    // deriveAutoItems only emits headings lazily when highlights appear beneath them;
    // '# Heading One\n\ntext\n' produces zero auto items. Use content with an H2 + highlight.
    await attachDoc('/v/doc.note.md', '- manual\n', '## Section\n^^important^^\n')
    const contents = [...outline.tree.nodes.values()].map(n => n.content)
    expect(contents).toContain('manual')
    expect(contents.length).toBeGreaterThan(1) // 至少派生出一个 auto 节点
    detach()
  })
  it('markDirty invokes the registered change sink', async () => {
    await attachDoc('/v/foo.note.md', '- x\n', null)
    let called = 0
    setChangeSink(() => { called++ })
    markDirty()
    expect(called).toBe(1)
    setChangeSink(null)
    detach()
  })
  it('serializeDoc(false) does not stamp updated (attach-compare must be side-effect-free)', async () => {
    await attachDoc('/v/bare.note.md', '- x\n', null)
    expect(serializeDoc(false)).toBe('- x\n')
    detach()
  })
})

// Data-loss guards: a companion note must never be overwritten by an empty or
// single-blank-node tree that only exists because of an attach/detach race on
// the shared global outline singleton.
describe('isEffectivelyEmptyTree', () => {
  it('true for an empty tree', () => {
    expect(isEffectivelyEmptyTree(createTree())).toBe(true)
  })
  it('true for a single blank manual node (the auto-created ready-to-type root)', () => {
    const t = createTree()
    addNode(t, { id: 'n1', parentId: null, order: 0, content: '   ', collapsed: false, source: 'manual' })
    expect(isEffectivelyEmptyTree(t)).toBe(true)
  })
  it('false for a single node that has text', () => {
    const t = createTree()
    addNode(t, { id: 'n1', parentId: null, order: 0, content: 'hello', collapsed: false, source: 'manual' })
    expect(isEffectivelyEmptyTree(t)).toBe(false)
  })
  it('false when there are multiple nodes', () => {
    const t = createTree()
    addNode(t, { id: 'n1', parentId: null, order: 0, content: '', collapsed: false, source: 'manual' })
    addNode(t, { id: 'n2', parentId: null, order: 100, content: '', collapsed: false, source: 'manual' })
    expect(isEffectivelyEmptyTree(t)).toBe(false)
  })
})

describe('noteTextHasContent', () => {
  it('true when the note markdown has a bullet with text', () => {
    expect(noteTextHasContent('- real content\n')).toBe(true)
    expect(noteTextHasContent('---\ntitle: x\n---\n  - nested\n')).toBe(true)
  })
  it('false for empty / whitespace / front-matter-only / blank bullets', () => {
    expect(noteTextHasContent('')).toBe(false)
    expect(noteTextHasContent('   \n\n')).toBe(false)
    expect(noteTextHasContent('---\ntitle: x\n---\n')).toBe(false)
    expect(noteTextHasContent('- \n')).toBe(false)
  })
})

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

describe('dirty / armed 保存门控', () => {
  it('attachDoc: 有内容则 armed，空则不 armed，均非 dirty', async () => {
    await attachDoc('/v/a.note.md', '- hello\n', null)
    expect(outline.armed).toBe(true)
    expect(outline.dirty).toBe(false)
    await attachDoc('/v/b.note.md', '', null)
    expect(outline.armed).toBe(false)
    expect(outline.dirty).toBe(false)
    detach()
  })
  it('markSynced 只置 dirty；未 armed 不触发 sink，armed 后触发', async () => {
    await attachDoc('/v/c.note.md', '', null)   // armed=false
    let calls = 0
    setChangeSink(() => { calls++ })
    markSynced()
    expect(outline.dirty).toBe(true)
    expect(calls).toBe(0)
    markDirty()                                  // 用户编辑激活
    expect(outline.armed).toBe(true)
    expect(calls).toBe(1)
    markSynced()                                 // 已 armed → sink 触发
    expect(calls).toBe(2)
    setChangeSink(null)
    detach()
  })
  it('markSaved 清 dirty', async () => {
    await attachDoc('/v/d.note.md', '- x\n', null)
    markDirty()
    expect(outline.dirty).toBe(true)
    markSaved()
    expect(outline.dirty).toBe(false)
    detach()
  })
})
