import { describe, expect, it } from 'vitest'
import { CanvasHistory, createCanvasPatch } from './history'
import { decodeJsonCanvas } from './json-canvas'
import { commitNodePositions } from './model'

function initialDocument() {
  const result = decodeJsonCanvas(JSON.stringify({
    nodes: [{ id: 'a', type: 'text', x: 0, y: 0, width: 100, height: 100, text: 'a' }],
    edges: [],
  }))
  if (!result.ok) throw new Error(result.diagnostics[0]?.message)
  return result.document
}

describe('Canvas history patches', () => {
  it('stores an isolated before/after patch and performs undo/redo', () => {
    const before = initialDocument()
    const after = commitNodePositions(before, [{ id: 'a', x: 40, y: 20 }])
    const patch = createCanvasPatch('移动节点', before, after)
    expect(patch).toBeDefined()
    expect(patch!.estimatedBytes).toBeGreaterThan(0)

    const history = new CanvasHistory()
    history.record('移动节点', before, after)
    expect(history.canUndo).toBe(true)
    expect(history.undoLabel).toBe('移动节点')
    expect(history.undo()!.nodes[0]).toMatchObject({ x: 0, y: 0 })
    expect(history.canRedo).toBe(true)
    expect(history.redo()!.nodes[0]).toMatchObject({ x: 40, y: 20 })
  })

  it('does not record no-ops and clears redo after a new branch', () => {
    const before = initialDocument()
    const history = new CanvasHistory()
    expect(history.record('no-op', before, before)).toBeUndefined()
    expect(history.canUndo).toBe(false)
    const first = commitNodePositions(before, [{ id: 'a', x: 10, y: 0 }])
    history.record('first', before, first)
    history.undo()
    expect(history.canRedo).toBe(true)
    const branch = commitNodePositions(before, [{ id: 'a', x: 20, y: 0 }])
    history.record('branch', before, branch)
    expect(history.canRedo).toBe(false)
  })

  it('enforces the configured transaction limit', () => {
    const start = initialDocument()
    const history = new CanvasHistory({ maxEntries: 1, maxBytes: 1_000_000 })
    const first = commitNodePositions(start, [{ id: 'a', x: 10, y: 0 }])
    const second = commitNodePositions(first, [{ id: 'a', x: 20, y: 0 }])
    history.record('first', start, first)
    history.record('second', first, second)
    expect(history.size).toBe(1)
    expect(history.undo()!.nodes[0]).toMatchObject({ x: 10 })
    expect(history.undo()).toBeUndefined()
  })
})
