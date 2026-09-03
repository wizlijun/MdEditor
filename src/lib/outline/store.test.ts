// src/lib/outline/store.test.ts
import { describe, it, expect } from 'vitest'
import { outline, companionPathFor, persistIdsFor, attachDoc, serializeDoc, setChangeSink, markDirty, markSynced, markSaved, detach, isEffectivelyEmptyTree, noteTextHasContent, signOutlineFrontmatterOnCreate } from './store.svelte'
import { createTree, addNode } from './model'

describe('companionPathFor', () => {
  it('maps main file to sibling .note.md', () => {
    expect(companionPathFor('/d/foo.md')).toBe('/d/foo.note.md')
    expect(companionPathFor('/d/bar.markdown')).toBe('/d/bar.note.md')
  })
  it('null for mdx — it needs its own build pipeline and gets no sidecar note', () => {
    expect(companionPathFor('/d/foo.mdx')).toBeNull()
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

// signOutlineFrontmatterOnCreate 是 flushDisk() 创建分支(!existed)的唯一
// 内存签名入口 —— flushDisk 本身没有测试挂钩(纯 Svelte 组件闭包 + Tauri fs
// I/O),所以直接在这个 seam 上验证:签名必须进 tree.frontmatter,否则下一次
// serializeDoc()(保存路径,任何时候都不传 generated)会在没有变更的情况下
// 序列化出一份不带署名的文本,把刚写盘的签名抹掉——这正是 fix round 2 要堵的洞。
describe('signOutlineFrontmatterOnCreate', () => {
  it('签名写进内存树,且不是"仅这一次"——后续 serializeDoc() 依然带着(第二次落盘不会把它冲掉)', async () => {
    await attachDoc('/v/sign1.note.md', '- hello\n', null)
    // 生产路径里 flushDisk 收到的 text 就是 markDirty → persistToDisk(serializeDoc())
    // 的产物——serializeDoc() 的 touch 步骤已经把 tree.frontmatter 从 null 补成一个
    // 真正的 frontmatter 块,签名函数是在那之后才跑的。这里复现同一顺序。
    serializeDoc()
    signOutlineFrontmatterOnCreate('/v/sign1.note.md', { by: 'human:bruce', at: '2026-08-20T10:00:00.000Z' })
    const first = serializeDoc()
    expect(first).toContain('generated:\n  by: human:bruce\n  at: 2026-08-20T10:00:00.000Z')
    // 模拟"文件已存在后的第二次保存":flushDisk 不会再调用这个函数(!existed
    // 分支只在创建那一次命中),但树本身已经带着这个键——serializeDoc() 不需要
    // 重新签名就该继续带着它,这才是"内存是唯一事实源"的意思。
    const second = serializeDoc()
    expect(second).toContain('generated:\n  by: human:bruce\n  at: 2026-08-20T10:00:00.000Z')
    detach()
  })
  it('docPath 已经切到另一篇文档——跳过,不把签名写错树', async () => {
    await attachDoc('/v/sign2.note.md', '- x\n', null)
    signOutlineFrontmatterOnCreate('/v/some-other-doc.note.md', { by: 'human:bruce', at: '2026-08-20T10:00:00.000Z' })
    expect(serializeDoc()).not.toContain('generated')
    detach()
  })
  it('树里已经有 generated——再传一个署名也不覆盖(补缺失键,不改已有值)', async () => {
    await attachDoc('/v/sign3.note.md', '---\ngenerated:\n  by: claude-code/opus-5\n  at: 2026-01-01T00:00:00.000Z\n---\n- x\n', null)
    signOutlineFrontmatterOnCreate('/v/sign3.note.md', { by: 'human:bruce', at: '2026-08-20T10:00:00.000Z' })
    const out = serializeDoc()
    expect(out).toContain('by: claude-code/opus-5')
    expect(out).not.toContain('human:bruce')
    detach()
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
