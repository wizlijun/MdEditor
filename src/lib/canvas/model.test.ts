import { describe, expect, it } from 'vitest'
import { decodeJsonCanvas, encodeJsonCanvas } from './json-canvas'
import {
  commitNodePositions,
  commitNodeRectangles,
  copyCanvasSelection,
  deleteCanvasSelection,
  freezeCanvasMove,
  freezeGroupMove,
  moveFrozenNodes,
  pasteCanvasSelection,
  reorderCanvasNodes,
} from './model'
import { isCanvasEdge, isKnownCanvasNode } from './types'

function documentFrom(value: unknown) {
  const result = decodeJsonCanvas(JSON.stringify(value))
  if (!result.ok) throw new Error(result.diagnostics[0]?.message)
  return result.document
}

function textNode(id: string, x: number, y: number, width = 20, height = 20) {
  return { id, type: 'text', x, y, width, height, text: id }
}

describe('Canvas domain model', () => {
  it('rounds committed move/resize geometry without mutating its input', () => {
    const original = documentFrom({ nodes: [textNode('a', 0, 0)], edges: [] })
    const moved = commitNodePositions(original, [{ id: 'a', x: 10.49, y: -4.5 }])
    expect(original.nodes[0]).toMatchObject({ x: 0, y: 0 })
    expect(moved.nodes[0]).toMatchObject({ x: 10, y: -4 })
    const resized = commitNodeRectangles(moved, [{ id: 'a', x: 1.6, y: 2.4, width: 99.8, height: 80.2 }])
    expect(resized.nodes[0]).toMatchObject({ x: 2, y: 2, width: 100, height: 80 })
    expect(() => commitNodeRectangles(original, [{ id: 'a', x: 0, y: 0, width: 0.1, height: 10 }])).toThrow()
  })

  it('freezes fully-contained group members and applies one delta to nested entries', () => {
    const document = documentFrom({
      nodes: [
        { id: 'outer', type: 'group', x: 0, y: 0, width: 200, height: 200 },
        textNode('inside', 10, 10),
        textNode('boundary', 180, 180),
        textNode('partial', 190, 190),
        { id: 'inner', type: 'group', x: 50, y: 50, width: 100, height: 100 },
        textNode('nested', 60, 60),
      ],
      edges: [],
    })
    const frozen = freezeGroupMove(document, 'outer')
    expect(new Set(frozen.nodeIds)).toEqual(new Set(['outer', 'inside', 'boundary', 'inner', 'nested']))
    const moved = moveFrozenNodes(document, frozen, { x: 10, y: -5 })
    const positions = Object.fromEntries(moved.nodes.filter(isKnownCanvasNode).map((node) => [node.id, [node.x, node.y]]))
    expect(positions).toMatchObject({
      outer: [10, -5], inside: [20, 5], boundary: [190, 175], inner: [60, 45], nested: [70, 55],
      partial: [190, 190],
    })
  })

  it('unions explicit nodes and every selected group closure for multi-node drags', () => {
    const document = documentFrom({
      nodes: [
        { id: 'left-group', type: 'group', x: 0, y: 0, width: 100, height: 100 },
        textNode('left-child', 10, 10),
        { id: 'right-group', type: 'group', x: 200, y: 0, width: 100, height: 100 },
        textNode('right-child', 210, 10),
        textNode('explicit', 400, 0),
        textNode('untouched', 500, 0),
      ],
      edges: [],
    })

    const frozen = freezeCanvasMove(document, ['left-group', 'right-group', 'explicit'])
    expect(new Set(frozen.nodeIds)).toEqual(new Set([
      'left-group', 'left-child', 'right-group', 'right-child', 'explicit',
    ]))

    const moved = moveFrozenNodes(document, frozen, { x: 5, y: 7 })
    const positions = Object.fromEntries(moved.nodes.filter(isKnownCanvasNode).map((node) => [node.id, [node.x, node.y]]))
    expect(positions).toMatchObject({
      'left-group': [5, 7], 'left-child': [15, 17],
      'right-group': [205, 7], 'right-child': [215, 17],
      explicit: [405, 7], untouched: [500, 0],
    })
  })

  it('uses node-array order as z-order and only changes it on explicit layer commands', () => {
    const original = documentFrom({ nodes: [textNode('a', 0, 0), textNode('b', 0, 0), textNode('c', 0, 0)], edges: [] })
    const front = reorderCanvasNodes(original, new Set(['a']), 'front')
    expect(front.nodes.map((node) => isKnownCanvasNode(node) ? node.id : '')).toEqual(['b', 'c', 'a'])
    const backward = reorderCanvasNodes(front, new Set(['a']), 'backward')
    expect(backward.nodes.map((node) => isKnownCanvasNode(node) ? node.id : '')).toEqual(['b', 'a', 'c'])
    expect(original.nodes.map((node) => isKnownCanvasNode(node) ? node.id : '')).toEqual(['a', 'b', 'c'])
  })

  it('copies internal edges and remaps every pasted id without touching unknown values', () => {
    const document = documentFrom({
      nodes: [
        { ...textNode('a', 0, 0), vendor: { nodeRef: 'a' } },
        textNode('b', 100, 0),
        textNode('outside', 300, 0),
      ],
      edges: [
        { id: 'ab', fromNode: 'a', toNode: 'b', vendorRef: 'a' },
        { id: 'bo', fromNode: 'b', toNode: 'outside' },
      ],
    })
    const payload = copyCanvasSelection(document, new Set(['a', 'b']))
    expect(payload.nodes).toHaveLength(2)
    expect(payload.edges).toHaveLength(1)
    const generated = ['a2', 'b2', 'ab2']
    const pasted = pasteCanvasSelection(document, payload, {
      offset: { x: 25, y: 40 },
      idFactory: () => generated.shift()!,
    })
    expect(pasted.idMap).toEqual(new Map([['a', 'a2'], ['b', 'b2']]))
    expect(pasted.insertedNodeIds).toEqual(['a2', 'b2'])
    expect(pasted.insertedEdgeIds).toEqual(['ab2'])
    expect(pasted.document.nodes.at(-2)).toMatchObject({ id: 'a2', x: 25, y: 40 })
    expect(pasted.document.nodes.at(-1)).toMatchObject({ id: 'b2', x: 125, y: 40 })
    const edge = pasted.document.edges.at(-1)!
    expect(edge).toMatchObject({ id: 'ab2', fromNode: 'a2', toNode: 'b2' })
    expect(isCanvasEdge(edge) && edge.extras.get('vendorRef')).toBe('a')
    const pastedNative = JSON.parse(encodeJsonCanvas(pasted.document))
    expect(pastedNative.nodes.at(-2).vendor).toEqual({ nodeRef: 'a' })
    expect(encodeJsonCanvas(document)).not.toContain('a2')
  })

  it('deletes incident edges in the same structural result', () => {
    const document = documentFrom({
      nodes: [textNode('a', 0, 0), textNode('b', 50, 0)],
      edges: [{ id: 'e', fromNode: 'a', toNode: 'b' }],
    })
    const deleted = deleteCanvasSelection(document, new Set(['a']))
    expect(deleted.nodes).toHaveLength(1)
    expect(deleted.edges).toHaveLength(0)
  })
})
