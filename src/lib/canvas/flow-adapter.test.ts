import { describe, expect, it } from 'vitest'
import { MAX_CANVAS_NODES, decodeJsonCanvas, encodeJsonCanvas } from './json-canvas'
import {
  applyFlowEdgeConnection,
  applyFlowNodeChanges,
  flowConnectionToCanvasEdge,
  projectCanvasToFlow,
} from './flow-adapter'

function decode(value: unknown) {
  const result = decodeJsonCanvas(JSON.stringify(value))
  if (!result.ok) throw new Error(result.diagnostics[0]?.message)
  return result.document
}

describe('Canvas Svelte Flow adapter', () => {
  it('refuses to project an oversized programmatic document', () => {
    const document = decode({
      nodes: [{ id: 'a', type: 'text', x: 0, y: 0, width: 10, height: 10, text: 'a' }],
      edges: [],
    })
    document.nodes = new Array(MAX_CANVAS_NODES + 1).fill(document.nodes[0])

    const projection = projectCanvasToFlow(document)

    expect(projection.nodes).toEqual([])
    expect(projection.edges).toEqual([])
    expect(projection.diagnostics).toEqual([
      expect.objectContaining({ code: 'canvas-node-limit', severity: 'error' }),
    ])
  })

  it('projects absolute geometry and stable z without Flow-only persistence fields', () => {
    const document = decode({
      nodes: [
        { id: 'g', type: 'group', x: -10, y: 5, width: 400, height: 300, label: 'Group' },
        { id: 't', type: 'text', x: 20, y: 30, width: 200, height: 120, text: 'hello', color: '2' },
      ],
      edges: [],
    })
    const projection = projectCanvasToFlow(document)
    expect(projection.options).toEqual({
      nodeOrigin: [0, 0], zIndexMode: 'manual', elevateNodesOnSelect: false, connectionMode: 'loose',
    })
    expect(projection.nodes[0]).toMatchObject({ id: 'g', position: { x: -10, y: 5 }, zIndex: 0, data: { kind: 'group', label: 'Group' } })
    expect(projection.nodes[1]).toMatchObject({ id: 't', position: { x: 20, y: 30 }, zIndex: 1, data: { kind: 'text', text: 'hello', color: '2' } })
    expect(projection.nodes[1]).not.toHaveProperty('parentId')

    const changed = applyFlowNodeChanges(document, [{
      id: 't', position: { x: 20.6, y: 29.2 }, width: 210.8,
      selected: true, dragging: true, measured: { width: 999 }, zIndex: 99, parentId: 'g',
    }])
    expect(changed.nodes[1]).toMatchObject({ x: 21, y: 29, width: 211, height: 120 })
    const saved = JSON.parse(encodeJsonCanvas(changed))
    expect(saved.nodes[1]).not.toHaveProperty('selected')
    expect(saved.nodes[1]).not.toHaveProperty('measured')
    expect(saved.nodes[1]).not.toHaveProperty('parentId')
    expect(saved.nodes.map((node: { id: string }) => node.id)).toEqual(['g', 't'])
  })

  it('maps explicit sides and applies JSON Canvas end defaults only in the view', () => {
    const document = decode({
      nodes: [
        { id: 'a', type: 'text', x: 0, y: 0, width: 100, height: 100, text: 'a' },
        { id: 'b', type: 'text', x: 200, y: 0, width: 100, height: 100, text: 'b' },
      ],
      edges: [{ id: 'e', fromNode: 'a', fromSide: 'right', toNode: 'b', toSide: 'left' }],
    })
    const edge = projectCanvasToFlow(document).edges[0]
    expect(edge).toMatchObject({ source: 'a', target: 'b', sourceHandle: 'side:right', targetHandle: 'side:left' })
    expect(edge.markerStart).toBeUndefined()
    expect(edge.markerEnd).toEqual({ type: 'arrowclosed' })
    expect(edge.data).toMatchObject({ effectiveFromEnd: 'none', effectiveToEnd: 'arrow' })
    const saved = JSON.parse(encodeJsonCanvas(document))
    expect(saved.edges[0]).not.toHaveProperty('fromEnd')
    expect(saved.edges[0]).not.toHaveProperty('toEnd')
  })

  it('omits ambiguous/dangling edges and duplicate nodes from the interactive graph', () => {
    const document = decode({
      nodes: [
        { id: 'same', type: 'text', x: 0, y: 0, width: 100, height: 100, text: 'a' },
        { id: 'same', type: 'text', x: 120, y: 0, width: 100, height: 100, text: 'b' },
      ],
      edges: [
        { id: 'ambiguous', fromNode: 'same', toNode: 'same' },
        { id: 'dangling', fromNode: 'same', toNode: 'gone' },
      ],
    })
    const projection = projectCanvasToFlow(document)
    expect(projection.nodes).toHaveLength(2)
    expect(projection.nodes.every((node) => node.type === 'canvas-diagnostic' && !node.draggable)).toBe(true)
    expect(new Set(projection.nodes.map((node) => node.id)).size).toBe(2)
    expect(projection.edges).toHaveLength(0)
    expect(projection.diagnostics.map((item) => item.code)).toEqual(expect.arrayContaining([
      'duplicate-node-id', 'ambiguous-edge', 'dangling-edge',
    ]))
  })

  it('converts connection handles and reconnects without losing edge extensions', () => {
    const created = flowConnectionToCanvasEdge('e', {
      source: 'a', target: 'b', sourceHandle: 'side:bottom', targetHandle: 'invalid',
    })
    expect(created).toMatchObject({ id: 'e', fromNode: 'a', toNode: 'b', fromSide: 'bottom' })
    expect(created.toSide).toBeUndefined()

    const document = decode({
      nodes: [
        { id: 'a', type: 'text', x: 0, y: 0, width: 10, height: 10, text: 'a' },
        { id: 'b', type: 'text', x: 20, y: 0, width: 10, height: 10, text: 'b' },
      ],
      edges: [{ id: 'e', fromNode: 'a', toNode: 'b', label: 'kept', vendor: 42 }],
    })
    const reconnected = applyFlowEdgeConnection(document, 'e', {
      source: 'b', target: 'a', sourceHandle: 'side:left', targetHandle: 'side:right',
    })
    const edge = JSON.parse(encodeJsonCanvas(reconnected)).edges[0]
    expect(edge).toMatchObject({ id: 'e', fromNode: 'b', toNode: 'a', fromSide: 'left', toSide: 'right', label: 'kept', vendor: 42 })
  })
})
