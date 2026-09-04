import { describe, expect, it } from 'vitest'
import {
  MAX_CANVAS_EDGES,
  MAX_CANVAS_NODES,
  decodeJsonCanvas,
  encodeJsonCanvas,
  jsonValueToNative,
} from './json-canvas'
import { commitNodePositions } from './model'
import { isCanvasEdge, isKnownCanvasNode } from './types'
import roundTripFixture from './fixtures/json-canvas-1.0-roundtrip.canvas?raw'

const COMPLETE = JSON.stringify({
  vendorRoot: { exact: 7 },
  nodes: [
    { id: 'text', type: 'text', x: -10, y: 20, width: 300, height: 180, text: '# Hello', color: '1', vendorNode: { flag: true } },
    { id: 'file', type: 'file', x: 320, y: 20, width: 300, height: 180, file: 'assets/a.png', subpath: '#page=2' },
    { id: 'link', type: 'link', x: 640, y: 20, width: 300, height: 180, url: 'https://example.com' },
    { id: 'group', type: 'group', x: -20, y: 0, width: 1000, height: 240, label: '', background: 'assets/bg.jpg', backgroundStyle: 'cover' },
  ],
  edges: [{
    id: 'edge', fromNode: 'text', fromSide: 'right', fromEnd: 'arrow',
    toNode: 'file', toSide: 'left', toEnd: 'none', color: '#ff00aa', label: '',
    vendorEdge: ['kept'],
  }],
})

describe('JSON Canvas codec', () => {
  it('fails closed before entry decoding when hard safety caps are exceeded', () => {
    const source = JSON.stringify({
      nodes: new Array(MAX_CANVAS_NODES + 1).fill(null),
      edges: new Array(MAX_CANVAS_EDGES + 1).fill(null),
    })

    const decoded = decodeJsonCanvas(source)

    expect(decoded.ok).toBe(false)
    expect(decoded.source).toBe(source)
    expect(decoded.diagnostics).toEqual([
      expect.objectContaining({ code: 'canvas-node-limit', path: '$.nodes', severity: 'error' }),
      expect.objectContaining({ code: 'canvas-edge-limit', path: '$.edges', severity: 'error' }),
    ])
    expect(decoded.diagnostics.some((item) => item.code === 'invalid-node')).toBe(false)
    expect(decoded.diagnostics.some((item) => item.code === 'invalid-edge')).toBe(false)
  })

  it('round-trips the checked-in JSON Canvas 1.0 interoperability fixture', () => {
    const decoded = decodeJsonCanvas(roundTripFixture)
    expect(decoded.ok).toBe(true)
    if (!decoded.ok) return

    const moved = commitNodePositions(decoded.document, [{ id: 'text-intro', x: 13, y: -7 }])
    const encoded = encodeJsonCanvas(moved)
    const native = JSON.parse(encoded)
    expect(native.nodes.map((node: { id: string }) => node.id)).toEqual([
      'group-main', 'text-intro', 'file-image', 'link-web',
    ])
    expect(native.nodes[1]).toMatchObject({ x: 13, y: -7, 'x-fixture-number': 9007199254740992 })
    // JSON.parse rounds this unsafe integer; the serialized token itself must
    // remain byte-for-byte exact for an Obsidian -> note.md -> Obsidian pass.
    expect(encoded).toContain('9007199254740993')
    expect(native.nodes[0]['x-fixture-group']).toEqual({ preserve: true })
    expect(native.edges[0]['x-fixture-edge']).toEqual(['preserve'])
    expect(native['x-fixture-root']).toEqual({ version: 'unknown-extension' })
    const reopened = decodeJsonCanvas(encoded)
    expect(reopened.ok).toBe(true)
    if (reopened.ok) expect(encodeJsonCanvas(reopened.document)).toBe(encoded)
  })

  it('round-trips all four node types, edge fields, order and unknown fields', () => {
    const decoded = decodeJsonCanvas(COMPLETE)
    expect(decoded.ok).toBe(true)
    if (!decoded.ok) return
    expect(decoded.document.nodes.map((node) => isKnownCanvasNode(node) ? node.type : 'opaque')).toEqual([
      'text', 'file', 'link', 'group',
    ])
    expect(decoded.document.nodes[0]).toMatchObject({ id: 'text', x: -10, color: '1' })
    expect(decoded.document.nodes[3]).toMatchObject({ id: 'group', label: '', backgroundStyle: 'cover' })
    expect(decoded.document.edges[0]).toMatchObject({
      id: 'edge', fromSide: 'right', fromEnd: 'arrow', toSide: 'left', toEnd: 'none', label: '',
    })

    const encoded = encodeJsonCanvas(decoded.document)
    const decodedAgain = decodeJsonCanvas(encoded)
    expect(decodedAgain.ok).toBe(true)
    if (!decodedAgain.ok) return
    expect(encodeJsonCanvas(decodedAgain.document)).toBe(encoded)
    expect(jsonValueToNative(decodedAgain.document.extras.get('vendorRoot')!)).toEqual({ exact: 7 })
    const first = decodedAgain.document.nodes[0]
    expect(isKnownCanvasNode(first) && jsonValueToNative(first.extras.get('vendorNode')!)).toEqual({ flag: true })
    const edge = decodedAgain.document.edges[0]
    expect(isCanvasEdge(edge) && jsonValueToNative(edge.extras.get('vendorEdge')!)).toEqual(['kept'])
  })

  it('preserves unknown number tokens exactly when another node moves', () => {
    const source = '{"future":{"large":9007199254740993,"decimal":1.2300000000000001,"exp":4.2e+99},"nodes":[{"id":"a","type":"text","x":0,"y":0,"width":100,"height":100,"text":"a","future":-0.000e-10}],"edges":[]}'
    const decoded = decodeJsonCanvas(source)
    expect(decoded.ok).toBe(true)
    if (!decoded.ok) return
    const moved = commitNodePositions(decoded.document, [{ id: 'a', x: 10.4, y: -2.6 }])
    const encoded = encodeJsonCanvas(moved)
    expect(encoded).toContain('9007199254740993')
    expect(encoded).toContain('1.2300000000000001')
    expect(encoded).toContain('4.2e+99')
    expect(encoded).toContain('-0.000e-10')
    expect(moved.nodes[0]).toMatchObject({ x: 10, y: -3 })
  })

  it('preserves optional absence and invalid optional values', () => {
    const decoded = decodeJsonCanvas(JSON.stringify({
      nodes: [{ id: 'g', type: 'group', x: 0, y: 0, width: 100, height: 100, backgroundStyle: 'future-style' }],
      edges: [{ id: 'e', fromNode: 'g', toNode: 'g', fromSide: 'diagonal', toEnd: 7 }],
    }))
    expect(decoded.ok).toBe(true)
    if (!decoded.ok) return
    const encoded = encodeJsonCanvas(decoded.document)
    const native = JSON.parse(encoded)
    expect(native.nodes[0]).not.toHaveProperty('color')
    expect(native.nodes[0].backgroundStyle).toBe('future-style')
    expect(native.edges[0]).not.toHaveProperty('fromEnd')
    expect(native.edges[0].fromSide).toBe('diagonal')
    expect(native.edges[0].toEnd).toBe(7)
    expect(decoded.diagnostics.filter((item) => item.code === 'invalid-optional-field')).toHaveLength(3)
  })

  it('fails closed on duplicate JSON properties and keeps the source', () => {
    const source = '{"nodes":[],"nodes":[],"edges":[]}'
    const decoded = decodeJsonCanvas(source)
    expect(decoded.ok).toBe(false)
    expect(decoded.source).toBe(source)
    expect(decoded.diagnostics[0]).toMatchObject({ code: 'duplicate-key', path: '$.nodes' })
    expect(decoded.diagnostics[0].line).toBe(1)
  })

  it('retains invalid entries, duplicate ids and dangling edges with diagnostics', () => {
    const source = JSON.stringify({
      nodes: [
        { id: 'same', type: 'text', x: 0, y: 0, width: 100, height: 100, text: 'a' },
        { id: 'same', type: 'mystery', x: 120, y: 0, width: 100, height: 100, payload: 9 },
        { id: 'bad', type: 'text', x: 0.5, y: 0, width: 100, height: 100, text: 'bad' },
      ],
      edges: [{ id: 'd', fromNode: 'same', toNode: 'missing' }],
    })
    const decoded = decodeJsonCanvas(source)
    expect(decoded.ok).toBe(true)
    if (!decoded.ok) return
    expect(decoded.document.nodes).toHaveLength(3)
    expect(decoded.diagnostics.map((item) => item.code)).toEqual(expect.arrayContaining([
      'invalid-node', 'duplicate-node-id', 'ambiguous-edge', 'dangling-edge',
    ]))
    const encoded = encodeJsonCanvas(decoded.document)
    expect(JSON.parse(encoded).nodes[1]).toMatchObject({ type: 'mystery', payload: 9 })
    expect(JSON.parse(encoded).nodes[2].x).toBe(0.5)
  })

  it('keeps absent root arrays absent until content is added', () => {
    const decoded = decodeJsonCanvas('{"future":true}')
    expect(decoded.ok).toBe(true)
    if (!decoded.ok) return
    expect(JSON.parse(encodeJsonCanvas(decoded.document))).toEqual({ future: true })
  })
})
