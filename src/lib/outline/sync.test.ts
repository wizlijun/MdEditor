// src/lib/outline/sync.test.ts
import { describe, it, expect } from 'vitest'
import { syncAutoItems, regenerate } from './sync'
import { deriveAutoItems } from './derive'
import { createTree, addNode, childrenOf, wrapAnswerBody, answerBodyOf } from './model'

const md1 = '# A\n## B\n^^hl^^\n'

function build(md: string) {
  const tree = createTree()
  syncAutoItems(tree, deriveAutoItems(md))
  return tree
}

describe('syncAutoItems', () => {
  it('builds initial auto tree', () => {
    const t = build(md1)
    const roots = childrenOf(t, null)
    // H1 A is skipped; ## B is the top-level heading; the highlight nests under B.
    expect(roots.map(n => [n.source, n.content])).toEqual([['toc', 'B']])
    expect(childrenOf(t, roots[0].id).map(n => [n.source, n.content])).toEqual([['highlight', 'hl']])
  })
  it('keeps id + collapsed + manual children across re-derive (diff match)', () => {
    const t = build(md1)
    const b = [...t.nodes.values()].find(n => n.content === 'B')!
    b.collapsed = true
    addNode(t, { id: 'note1', parentId: b.id, order: 500, content: 'my note', collapsed: false, source: 'manual' })
    syncAutoItems(t, deriveAutoItems('# A\n## B\n^^hl^^\nnew text\n'))
    const b2 = [...t.nodes.values()].find(n => n.content === 'B')!
    expect(b2.id).toBe(b.id)
    expect(b2.collapsed).toBe(true)
    expect(childrenOf(t, b2.id).some(n => n.id === 'note1')).toBe(true)
  })
  it('removing a highlight PRESERVES its node as manual (content not lost); child stays attached', () => {
    const t = build(md1)
    const hl = [...t.nodes.values()].find(n => n.source === 'highlight')!
    addNode(t, { id: 'child', parentId: hl.id, order: 0, content: 'attached', collapsed: false, source: 'manual' })
    // Re-derive keeps heading B (still has a highlight) but drops the old 'hl'.
    syncAutoItems(t, deriveAutoItems('## B\n^^other^^\n'))
    const preserved = [...t.nodes.values()].find(n => n.content === 'hl')!
    expect(preserved).toBeDefined()
    expect(preserved.source).toBe('manual')          // converted, no longer synced
    expect(preserved.anchorLine).toBeUndefined()      // stale anchor cleared
    expect(t.nodes.get('child')!.parentId).toBe(preserved.id)  // child stays under it
  })
  it('an all-auto note is NOT emptied when the main doc loses all its marks', () => {
    const t = build('## Section\n^^one^^\n[[Page]]\n')
    expect(t.nodes.size).toBeGreaterThan(0)
    syncAutoItems(t, deriveAutoItems('## Section\n\nplain body, no marks\n'))
    // Section toc survives (still a heading); the highlight + wikilink convert to
    // manual rather than vanish — the note keeps its content.
    expect(t.nodes.size).toBeGreaterThan(0)
    const contents = [...t.nodes.values()].map(n => n.content)
    expect(contents).toContain('one')
    expect([...t.nodes.values()].filter(n => n.source !== 'manual' && n.source !== 'toc')).toHaveLength(0)
  })
  it('anchorLine refreshes on match', () => {
    const t = build(md1)
    syncAutoItems(t, deriveAutoItems('intro\n\n# A\n## B\n^^hl^^\n'))
    expect([...t.nodes.values()].find(n => n.content === 'B')!.anchorLine).toBe(4)
  })
  it('root-level manual node survives and stays at root', () => {
    const t = build(md1)
    addNode(t, { id: 'root-note', parentId: null, order: 950, content: 'root note', collapsed: false, source: 'manual' })
    syncAutoItems(t, deriveAutoItems('# A2\n'))
    expect(t.nodes.get('root-note')!.parentId).toBeNull()
  })

  it('typing around a [[wikilink]] updates the node in place — no orphan pile-up', () => {
    // The wikilink item's content is the WHOLE sentence, so every keystroke changes
    // it. Each debounced re-derive must keep the SAME synced node (same [[target]]
    // on the same line), not demote the old text to manual and spawn a fresh node.
    const t = build('I study [[X]] now.\n')
    const wiki0 = [...t.nodes.values()].filter(n => n.source === 'wikilink')
    expect(wiki0).toHaveLength(1)
    const id0 = wiki0[0].id

    syncAutoItems(t, deriveAutoItems('I study [[X]] now, a lot.\n'))
    syncAutoItems(t, deriveAutoItems('I study [[X]] now, a lot more.\n'))

    const wikis = [...t.nodes.values()].filter(n => n.source === 'wikilink')
    const manuals = [...t.nodes.values()].filter(n => n.source === 'manual')
    expect(wikis).toHaveLength(1)                                    // still one synced node
    expect(wikis[0].id).toBe(id0)                                    // same node, updated in place
    expect(wikis[0].content).toBe('I study [[X]] now, a lot more.')
    expect(manuals).toHaveLength(0)                                  // no orphaned handwritten notes
  })

  it('changing the [[target]] on a line is NOT an in-place edit (old preserved as manual)', () => {
    const t = build('note about [[X]].\n')
    syncAutoItems(t, deriveAutoItems('note about [[Y]].\n'))
    const bySource = (s: string) => [...t.nodes.values()].filter(n => n.source === s)
    expect(bySource('wikilink').map(n => n.content)).toEqual(['note about [[Y]].'])
    expect(bySource('manual').map(n => n.content)).toEqual(['note about [[X]].'])
  })
})

describe('regenerate', () => {
  it('rebuilds autos fresh but keeps manual nodes (spec 验收 5)', () => {
    const t = build(md1)
    const b = [...t.nodes.values()].find(n => n.content === 'B')!
    addNode(t, { id: 'keep', parentId: b.id, order: 999, content: 'keep me', collapsed: false, source: 'manual' })
    regenerate(t, deriveAutoItems(md1))
    expect(t.nodes.get('keep')).toBeDefined()
    // md1 now derives a single top-level heading (B); H1 A is skipped.
    expect([...t.nodes.values()].filter(n => n.source === 'toc')).toHaveLength(1)
  })
})

describe('timestamps', () => {
  it('stamps createdAt on new highlight nodes but not toc nodes', () => {
    const t = build(md1)
    const nodes = [...t.nodes.values()]
    const toc = nodes.find(n => n.source === 'toc')!
    const hl = nodes.find(n => n.source === 'highlight')!
    expect(hl.createdAt).toBeDefined()
    expect(toc.createdAt).toBeUndefined()
  })
  it('keeps original createdAt on matched highlight across re-derive', () => {
    const t = build(md1)
    const hl = [...t.nodes.values()].find(n => n.source === 'highlight')!
    hl.createdAt = '2026-01-01T00:00:00.000Z'
    syncAutoItems(t, deriveAutoItems('# A\n## B\nmore\n^^hl^^\n'))
    const hl2 = [...t.nodes.values()].find(n => n.source === 'highlight')!
    expect(hl2.createdAt).toBe('2026-01-01T00:00:00.000Z')
  })
})

describe('syncAutoItems — annotation note children', () => {
  it('creates an annotation node with a note child', () => {
    const tree = createTree()
    syncAutoItems(tree, [
      { source: 'annotation', content: '原文', note: '批注内容', depth: 0, anchorLine: 3 },
    ])
    const anno = [...tree.nodes.values()].find(n => n.source === 'annotation')!
    expect(anno.content).toBe('原文')
    const kids = childrenOf(tree, anno.id)
    expect(kids).toHaveLength(1)
    expect(kids[0].source).toBe('note')
    expect(kids[0].content).toBe('批注内容')
  })

  it('updates the note child in place when the md note changes', () => {
    const tree = createTree()
    syncAutoItems(tree, [{ source: 'annotation', content: '原文', note: '旧', depth: 0, anchorLine: 1 }])
    const noteId = [...tree.nodes.values()].find(n => n.source === 'note')!.id
    syncAutoItems(tree, [{ source: 'annotation', content: '原文', note: '新', depth: 0, anchorLine: 1 }])
    const note = tree.nodes.get(noteId)!
    expect(note.content).toBe('新')
  })

  it('preserves a vanished annotation (and its note comment) as manual instead of deleting', () => {
    const tree = createTree()
    syncAutoItems(tree, [{ source: 'annotation', content: '原文', note: 'n', depth: 0, anchorLine: 1 }])
    syncAutoItems(tree, [])
    expect(tree.nodes.size).toBe(2)
    const nodes = [...tree.nodes.values()]
    expect(nodes.every(n => n.source === 'manual')).toBe(true)
    expect(nodes.map(n => n.content).sort()).toEqual(['n', '原文'])
  })

  it('editing the annotated ORIGINAL text updates in place — no note-child pile-up', () => {
    // The wrapped annotation's content IS the 被批注原文, so revising that phrase
    // changes content on every keystroke. Each debounced re-derive must keep the
    // SAME synced annotation + its SAME single note child (same comment on the
    // same line), not demote to manual and spawn a fresh pair each character.
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems('{==foo==}{>>my note<<}\n'))
    const anno0 = [...tree.nodes.values()].filter(n => n.source === 'annotation')
    expect(anno0).toHaveLength(1)
    const annoId = anno0[0].id
    const noteId = childrenOf(tree, annoId).find(c => c.source === 'note')!.id

    syncAutoItems(tree, deriveAutoItems('{==foox==}{>>my note<<}\n'))
    syncAutoItems(tree, deriveAutoItems('{==fooxy==}{>>my note<<}\n'))

    const annos = [...tree.nodes.values()].filter(n => n.source === 'annotation')
    const notes = [...tree.nodes.values()].filter(n => n.source === 'note')
    const manuals = [...tree.nodes.values()].filter(n => n.source === 'manual')
    expect(annos).toHaveLength(1)                 // still one synced annotation
    expect(annos[0].id).toBe(annoId)              // same node, updated in place
    expect(annos[0].content).toBe('fooxy')
    expect(notes).toHaveLength(1)                 // single note child, not multiplied
    expect(notes[0].id).toBe(noteId)
    expect(notes[0].content).toBe('my note')
    expect(manuals).toHaveLength(0)               // no residue pile-up
  })

  it('changing the annotation COMMENT while editing original still re-pairs by line', () => {
    // Comment text is the re-pair identity; when only the original churns and the
    // comment is stable, re-pair holds. (Editing the comment alone already matches
    // via stable content and needs no re-pair.)
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems('{==foo==}{>>c<<}\n'))
    const id0 = [...tree.nodes.values()].find(n => n.source === 'annotation')!.id
    syncAutoItems(tree, deriveAutoItems('{==foobar==}{>>c<<}\n'))
    const annos = [...tree.nodes.values()].filter(n => n.source === 'annotation')
    expect(annos).toHaveLength(1)
    expect(annos[0].id).toBe(id0)
    expect(annos[0].content).toBe('foobar')
  })

  it('keeps manual children of an annotation node alongside the note child', () => {
    const tree = createTree()
    syncAutoItems(tree, [{ source: 'annotation', content: '原文', note: 'n', depth: 0, anchorLine: 1 }])
    const anno = [...tree.nodes.values()].find(n => n.source === 'annotation')!
    tree.nodes.set('m1', {
      id: 'm1', parentId: anno.id, order: 500, content: '手写', collapsed: false, source: 'manual',
    })
    syncAutoItems(tree, [{ source: 'annotation', content: '原文', note: 'n2', depth: 0, anchorLine: 1 }])
    const kids = childrenOf(tree, anno.id)
    expect(kids.map(k => k.source).sort()).toEqual(['manual', 'note'])
    expect(kids.find(k => k.source === 'note')!.content).toBe('n2')
  })
})

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

  it('clears question status when the annotation is deleted (no stale answered on the demoted node)', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(qMd))
    noteChild(tree).status = 'answered'
    syncAutoItems(tree, deriveAutoItems('正文没有批注了\n'))
    const kept = [...tree.nodes.values()].find(n => n.content === '这里为什么能到 90%?')!
    expect(kept.source).toBe('manual')
    expect(kept.status).toBeUndefined()
  })
})

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
    syncAutoItems(tree, deriveAutoItems(qMd))
    const a = tree.nodes.get('ans-1')!
    expect(a.source).toBe('answer')
    expect(a.answeredBy).toBe('claude-code')
    expect(answerBodyOf(a)).toContain('因为前缀高度重复')
  })

  it('keeps the answer node even when the annotation is deleted from the main doc', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(qMd))
    seedAnswer(tree)
    syncAutoItems(tree, deriveAutoItems('正文没有批注了\n'))
    const a = tree.nodes.get('ans-1')!
    expect(a.source).toBe('answer')
  })
})

describe('regenerate keeps agent answers and question state', () => {
  const qMd = '正文 {==原文==}{>>为什么?<<}\n'

  it('survives 重新从原文提取: answer kept, re-attached, status restored', () => {
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(qMd))
    const q0 = [...tree.nodes.values()].find(n => n.source === 'question')!
    q0.status = 'answered'
    tree.nodes.set('ans-r', {
      id: 'ans-r', parentId: q0.id, order: 100,
      content: wrapAnswerBody('答复正文'), collapsed: false,
      source: 'answer', answeredBy: 'claude-code',
    })

    regenerate(tree, deriveAutoItems(qMd))

    const a = tree.nodes.get('ans-r')
    expect(a).toBeDefined()                       // 绝不能被全量重建抹掉
    expect(a!.source).toBe('answer')
    expect(a!.answeredBy).toBe('claude-code')
    const q1 = [...tree.nodes.values()].find(n => n.source === 'question')!
    expect(q1.status).toBe('answered')            // 状态还原,不回落 open
    expect(a!.parentId).toBe(q1.id)               // 重新挂回同一问题
  })

  it('keeps separate answers attached to duplicate question texts', () => {
    const duplicateMd = [
      '第一处 {==甲==}{>>为什么?<<}',
      '第二处 {==乙==}{>>为什么?<<}',
      '',
    ].join('\n')
    const tree = createTree()
    syncAutoItems(tree, deriveAutoItems(duplicateMd))
    const questions = [...tree.nodes.values()].filter(n => n.source === 'question')
    questions.forEach((q, index) => {
      q.status = 'answered'
      tree.nodes.set(`ans-${index}`, {
        id: `ans-${index}`, parentId: q.id, order: 100,
        content: wrapAnswerBody(`答复 ${index + 1}`), collapsed: false, source: 'answer',
      })
    })

    regenerate(tree, deriveAutoItems(duplicateMd))

    const rebuilt = [...tree.nodes.values()].filter(n => n.source === 'question')
    expect(rebuilt.map(q => answerBodyOf(childrenOf(tree, q.id).find(n => n.source === 'answer')!)))
      .toEqual(['答复 1', '答复 2'])
  })
})
