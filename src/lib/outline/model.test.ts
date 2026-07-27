// src/lib/outline/model.test.ts
import { describe, it, expect } from 'vitest'
import {
  createTree, addNode, childrenOf, calculateOrderBetween,
  normalizeSiblingOrders, collectDescendantIds, isValidDropTarget,
  visibleNodes, removeSubtree, setNodeContent, ancestorsOf, newId,
  isQuestionText, treeHasQuestion, type OutlineTree, type OutlineNode,
} from './model'

function sampleTree(): OutlineTree {
  // a(0) ── b(100) ── c(200)；b 有子 b1(0)、b2(100)
  const t = createTree()
  addNode(t, { id: 'a', parentId: null, order: 0, content: 'A', collapsed: false, source: 'manual' })
  addNode(t, { id: 'b', parentId: null, order: 100, content: 'B', collapsed: false, source: 'manual' })
  addNode(t, { id: 'c', parentId: null, order: 200, content: 'C', collapsed: false, source: 'manual' })
  addNode(t, { id: 'b1', parentId: 'b', order: 0, content: 'B1', collapsed: false, source: 'manual' })
  addNode(t, { id: 'b2', parentId: 'b', order: 100, content: 'B2', collapsed: false, source: 'manual' })
  return t
}

describe('calculateOrderBetween (hulunote render.cljs:612)', () => {
  it('midpoint when both defined', () => expect(calculateOrderBetween(0, 100)).toBe(50))
  it('prev+100 when next null', () => expect(calculateOrderBetween(200, null)).toBe(300))
  it('next/2 when prev null', () => expect(calculateOrderBetween(null, 100)).toBe(50))
  it('0 when both null', () => expect(calculateOrderBetween(null, null)).toBe(0))
})

describe('ancestorsOf (zoom breadcrumb)', () => {
  it('root-first ancestor chain, excludes the node itself', () => {
    const t = sampleTree()
    expect(ancestorsOf(t, 'b1').map(n => n.id)).toEqual(['b'])
  })
  it('deeper chain is root-first', () => {
    const t = sampleTree()
    addNode(t, { id: 'b1a', parentId: 'b1', order: 0, content: 'B1A', collapsed: false, source: 'manual' })
    expect(ancestorsOf(t, 'b1a').map(n => n.id)).toEqual(['b', 'b1'])
  })
  it('top-level node has no ancestors', () => {
    expect(ancestorsOf(sampleTree(), 'a')).toEqual([])
  })
  it('unknown id → empty', () => {
    expect(ancestorsOf(sampleTree(), 'nope')).toEqual([])
  })
  it('cycle-safe: broken parent loop terminates', () => {
    const t = createTree()
    addNode(t, { id: 'x', parentId: 'y', order: 0, content: 'X', collapsed: false, source: 'manual' })
    addNode(t, { id: 'y', parentId: 'x', order: 0, content: 'Y', collapsed: false, source: 'manual' })
    // must not hang; returns the one reachable ancestor before the cycle closes
    expect(ancestorsOf(t, 'x').map(n => n.id)).toEqual(['y'])
  })
})

describe('tree basics', () => {
  it('childrenOf sorts by order', () => {
    const t = sampleTree()
    expect(childrenOf(t, null).map(n => n.id)).toEqual(['a', 'b', 'c'])
    expect(childrenOf(t, 'b').map(n => n.id)).toEqual(['b1', 'b2'])
  })
  it('normalizeSiblingOrders re-assigns idx*100', () => {
    const t = sampleTree()
    t.nodes.get('a')!.order = 5
    t.nodes.get('b')!.order = 5   // duplicate
    normalizeSiblingOrders(t, null)
    expect(childrenOf(t, null).map(n => n.order)).toEqual([0, 100, 200])
  })
  it('collectDescendantIds', () => {
    expect([...collectDescendantIds(sampleTree(), 'b')].sort()).toEqual(['b1', 'b2'])
  })
  it('isValidDropTarget rejects self and own descendant', () => {
    const t = sampleTree()
    expect(isValidDropTarget(t, 'b', 'b')).toBe(false)
    expect(isValidDropTarget(t, 'b', 'b1')).toBe(false)
    expect(isValidDropTarget(t, 'b', 'c')).toBe(true)
  })
  it('visibleNodes hides children of collapsed parents', () => {
    const t = sampleTree()
    expect(visibleNodes(t).map(n => n.id)).toEqual(['a', 'b', 'b1', 'b2', 'c'])
    t.nodes.get('b')!.collapsed = true
    expect(visibleNodes(t).map(n => n.id)).toEqual(['a', 'b', 'c'])
  })
  it('removeSubtree removes node and descendants', () => {
    const t = sampleTree()
    removeSubtree(t, 'b')
    expect([...t.nodes.keys()].sort()).toEqual(['a', 'c'])
  })
})

describe('setNodeContent timestamps', () => {
  it('sets updatedAt when content changes on non-toc nodes', () => {
    const t = sampleTree()
    const n = t.nodes.get('a')!
    setNodeContent(n, 'A2')
    expect(n.content).toBe('A2')
    expect(n.updatedAt).toBeDefined()
    expect(() => new Date(n.updatedAt!)).not.toThrow()
  })
  it('is a no-op when content is unchanged', () => {
    const t = sampleTree()
    const n = t.nodes.get('a')!
    setNodeContent(n, n.content)
    expect(n.updatedAt).toBeUndefined()
  })
  it('never stamps toc nodes', () => {
    const t = sampleTree()
    const n = t.nodes.get('a')!
    n.source = 'toc'
    setNodeContent(n, 'changed')
    expect(n.content).toBe('changed')
    expect(n.updatedAt).toBeUndefined()
  })
})

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
