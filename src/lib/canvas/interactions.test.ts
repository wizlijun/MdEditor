import { describe, expect, it } from 'vitest'
import { decodeJsonCanvas } from './json-canvas'
import {
  alignCanvasSelection,
  buildCanvasNodeSpatialIndex,
  buildCanvasObstacleIndex,
  buildCanvasSnapIndex,
  canvasNodesIntersectPolygon,
  computeCanvasAutoPanVelocity,
  computeCanvasResizeSnap,
  computeCanvasSnap,
  createCanvasResizeSnapshot,
  distributeCanvasSelection,
  fitCanvasGroupToContents,
  getCanvasNodesBounds,
  getCanvasSelectionBounds,
  getCanvasSelectionRoots,
  polygonIntersectsRect,
  resizeCanvasSelection,
  resolveCanvasEdgeSides,
  resolveCanvasResizeScale,
  spreadCanvasSelection,
} from './interactions'

function documentFrom(value: unknown) {
  const result = decodeJsonCanvas(JSON.stringify(value))
  if (!result.ok) throw new Error(result.diagnostics[0]?.message)
  return result.document
}

function textNode(id: string, x: number, y: number, width = 20, height = 20) {
  return { id, type: 'text', x, y, width, height, text: id }
}

function changesById(changes: ReturnType<typeof alignCanvasSelection>) {
  return Object.fromEntries(changes.map((change) => [change.id, change]))
}

describe('Canvas interaction geometry', () => {
  it('computes selection bounds from editable absolute rectangles', () => {
    const document = documentFrom({
      nodes: [
        { id: 'group', type: 'group', x: -20, y: 10, width: 100, height: 90 },
        textNode('inside', 0, 30),
        textNode('outside', 180, -10, 40, 30),
      ],
      edges: [],
    })

    expect(getCanvasSelectionBounds(document, ['group', 'outside'])).toEqual({
      x: -20,
      y: -10,
      width: 240,
      height: 110,
    })
    expect(getCanvasSelectionBounds(document, ['missing'])).toBeNull()
    expect(getCanvasNodesBounds(document.nodes.slice(0, 2))).toEqual({
      x: -20,
      y: 10,
      width: 100,
      height: 90,
    })
  })

  it('fits a group around its geometric members without moving content', () => {
    const document = documentFrom({
      nodes: [
        { id: 'group', type: 'group', x: 0, y: 0, width: 500, height: 400 },
        textNode('left', 80, 90, 60, 40),
        textNode('right', 240, 180, 80, 50),
        textNode('outside', 700, 700, 40, 40),
      ],
      edges: [],
    })

    const fitted = fitCanvasGroupToContents(document, 'group')
    expect(fitted.nodes.find((node) => 'id' in node && node.id === 'group')).toMatchObject({
      x: 44,
      y: 38,
      width: 312,
      height: 228,
    })
    expect(fitted.nodes.find((node) => 'id' in node && node.id === 'left'))
      .toMatchObject({ x: 80, y: 90, width: 60, height: 40 })
    expect(fitted.nodes.find((node) => 'id' in node && node.id === 'outside'))
      .toMatchObject({ x: 700, y: 700 })
  })

  it('leaves an empty group unchanged when fitting contents', () => {
    const document = documentFrom({
      nodes: [{ id: 'group', type: 'group', x: 0, y: 0, width: 300, height: 200 }],
      edges: [],
    })
    expect(fitCanvasGroupToContents(document, 'group')).toBe(document)
  })

  it.each([
    ['left', { a: { x: 0 }, b: { x: 0 } }],
    ['center-h', { a: { x: 60 }, b: { x: 50 } }],
    ['right', { a: { x: 120 }, b: { x: 100 } }],
    ['top', { a: { y: 0 }, b: { y: 0 } }],
    ['center-v', { a: { y: 60 }, b: { y: 50 } }],
    ['bottom', { a: { y: 120 }, b: { y: 100 } }],
  ] as const)('aligns a selection to %s', (direction, expected) => {
    const document = documentFrom({
      nodes: [textNode('a', 0, 0, 20, 10), textNode('b', 100, 100, 40, 30)],
      edges: [],
    })

    const changes = changesById(alignCanvasSelection(document, ['a', 'b'], direction))
    expect(changes.a).toMatchObject(expected.a)
    expect(changes.b).toMatchObject(expected.b)
  })

  it('moves a selected group and its geometric closure as one alignment root', () => {
    const document = documentFrom({
      nodes: [
        { id: 'group', type: 'group', x: 0, y: 0, width: 100, height: 100 },
        textNode('inside', 10, 10),
        textNode('outside', 200, 0),
      ],
      edges: [],
    })

    const changes = changesById(alignCanvasSelection(document, ['group', 'outside'], 'right'))
    expect(changes.group).toMatchObject({ x: 120, y: 0 })
    expect(changes.inside).toMatchObject({ x: 130, y: 10 })
    expect(changes.outside).toMatchObject({ x: 200, y: 0 })
    expect(getCanvasSelectionRoots(document, ['group', 'inside', 'outside']).map((node) => node.id))
      .toEqual(['group', 'outside'])
  })

  it('distributes different-size nodes with uniform horizontal and vertical gaps', () => {
    const horizontal = documentFrom({
      nodes: [
        textNode('a', 0, 0, 20, 20),
        textNode('b', 70, 0, 20, 20),
        textNode('c', 180, 0, 20, 20),
      ],
      edges: [],
    })
    expect(changesById(distributeCanvasSelection(horizontal, ['a', 'b', 'c'], 'horizontal')).b)
      .toMatchObject({ x: 90, y: 0 })

    const vertical = documentFrom({
      nodes: [
        textNode('a', 0, 0, 20, 10),
        textNode('b', 0, 40, 20, 20),
        textNode('c', 0, 110, 20, 30),
      ],
      edges: [],
    })
    expect(changesById(distributeCanvasSelection(vertical, ['a', 'b', 'c'], 'vertical')).b)
      .toMatchObject({ x: 0, y: 50 })
  })

  it('spreads overlapping roots without mutating the document', () => {
    const document = documentFrom({
      nodes: [textNode('a', 0, 0), textNode('b', 10, 0)],
      edges: [],
    })

    const changes = changesById(spreadCanvasSelection(document, ['a', 'b'], 10))
    expect(changes.b).toMatchObject({ x: 30, y: 0 })
    expect(document.nodes[1]).toMatchObject({ x: 10, y: 0 })
  })
})

describe('Canvas smart snap', () => {
  it('excludes the moving group closure from the candidate index', () => {
    const document = documentFrom({
      nodes: [
        { id: 'group', type: 'group', x: 0, y: 0, width: 100, height: 100 },
        textNode('inside', 10, 10),
        textNode('outside', 200, 0),
      ],
      edges: [],
    })

    const index = buildCanvasSnapIndex(document, ['group'])
    expect(Array.from(index.rectsById.keys())).toEqual(['outside'])
  })

  it('snaps rectangle edges and centers independently per axis', () => {
    const document = documentFrom({ nodes: [textNode('anchor', 0, 0, 50, 50)], edges: [] })
    const index = buildCanvasSnapIndex(document, [])

    expect(computeCanvasSnap(
      { x: 51, y: 80, width: 20, height: 20 },
      index,
      { thresholdFlow: 6 },
    )).toMatchObject({ deltaX: -1, deltaY: 0 })
    expect(computeCanvasSnap(
      { x: 21, y: 80, width: 10, height: 20 },
      index,
      { thresholdFlow: 6 },
    )).toMatchObject({ deltaX: -1, deltaY: 0 })
  })

  it('prefers middle equal-spacing, then extends a trailing rhythm', () => {
    const middleDocument = documentFrom({
      nodes: [textNode('left', 0, 0, 50, 50), textNode('right', 400, 0, 50, 50)],
      edges: [],
    })
    const middle = computeCanvasSnap(
      { x: 226, y: 0, width: 50, height: 50 },
      buildCanvasSnapIndex(middleDocument, []),
      { thresholdFlow: 30 },
    )
    expect(middle.deltaX).toBe(-26)
    expect(middle.guides).toEqual(expect.arrayContaining([
      expect.objectContaining({ kind: 'equal-spacing', mode: 'middle', axis: 'x' }),
    ]))

    const trailingDocument = documentFrom({
      nodes: [textNode('first', 0, 0, 50, 50), textNode('second', 100, 0, 50, 50)],
      edges: [],
    })
    const trailing = computeCanvasSnap(
      { x: 202, y: 0, width: 50, height: 50 },
      buildCanvasSnapIndex(trailingDocument, []),
      { thresholdFlow: 6 },
    )
    expect(trailing.deltaX).toBe(-2)
    expect(trailing.guides).toEqual(expect.arrayContaining([
      expect.objectContaining({ kind: 'equal-spacing', mode: 'trailing', axis: 'x' }),
    ]))
  })

  it('bypasses every snap when Alt is held', () => {
    const document = documentFrom({ nodes: [textNode('anchor', 0, 0, 50, 50)], edges: [] })
    expect(computeCanvasSnap(
      { x: 51, y: 0, width: 20, height: 20 },
      buildCanvasSnapIndex(document, []),
      { thresholdFlow: 6, bypass: true },
    )).toEqual({ deltaX: 0, deltaY: 0, guides: [] })
  })

  it('snaps only the active resize edges and keeps the opposite corner anchored', () => {
    const document = documentFrom({ nodes: [textNode('anchor', 0, 0, 50, 50)], edges: [] })
    const index = buildCanvasSnapIndex(document, [])

    expect(computeCanvasResizeSnap(
      { x: 52, y: 80, width: 98, height: 100 },
      index,
      {
        thresholdFlow: 6,
        activeEdges: { x: 'min', y: 'none' },
        minimumWidth: 20,
        minimumHeight: 20,
      },
    )).toMatchObject({
      rectangle: { x: 50, y: 80, width: 100, height: 100 },
      guides: [expect.objectContaining({ kind: 'alignment', axis: 'x', value: 50 })],
    })

    expect(computeCanvasResizeSnap(
      { x: 48, y: 80, width: 52, height: 100 },
      index,
      {
        thresholdFlow: 6,
        activeEdges: { x: 'min', y: 'none' },
        minimumWidth: 51,
        minimumHeight: 20,
      },
    )).toEqual({ rectangle: { x: 48, y: 80, width: 52, height: 100 }, guides: [] })
  })
})

describe('Canvas advanced pointer geometry', () => {
  it('ramps auto-pan velocity inside the edge zone and caps outside the surface', () => {
    expect(computeCanvasAutoPanVelocity({ x: 20, y: 50 }, { width: 200, height: 100 }))
      .toEqual({ x: 400, y: 0 })
    expect(computeCanvasAutoPanVelocity({ x: 190, y: -20 }, { width: 200, height: 100 }))
      .toEqual({ x: -600, y: 800 })
    expect(computeCanvasAutoPanVelocity({ x: 100, y: 50 }, { width: 200, height: 100 }))
      .toEqual({ x: 0, y: 0 })
  })

  it('chooses facing edge handles while preserving explicit standard sides', () => {
    const source = { x: 0, y: 0, width: 100, height: 100 }
    expect(resolveCanvasEdgeSides(source, { x: 300, y: 25, width: 100, height: 50 }))
      .toEqual({ fromSide: 'right', toSide: 'left' })
    expect(resolveCanvasEdgeSides(source, { x: 25, y: -300, width: 50, height: 100 }))
      .toEqual({ fromSide: 'top', toSide: 'bottom' })
    expect(resolveCanvasEdgeSides(source, { x: 300, y: 0, width: 100, height: 100 }, 'bottom'))
      .toEqual({ fromSide: 'bottom', toSide: 'left' })
    expect(resolveCanvasEdgeSides(source, { x: 300, y: 0, width: 100, height: 100 }, 'top', 'right'))
      .toEqual({ fromSide: 'top', toSide: 'right' })
  })

  it('avoids a blocked direct side pair and ignores its own endpoints', () => {
    const source = { id: 'source', x: 0, y: 0, width: 100, height: 100 }
    const target = { id: 'target', x: 300, y: 0, width: 100, height: 100 }
    const direct = { fromSide: 'right', toSide: 'left' }
    expect(resolveCanvasEdgeSides(source, target, undefined, undefined, [
      { id: 'far', x: 0, y: 400, width: 100, height: 100 },
    ])).toEqual(direct)
    expect(resolveCanvasEdgeSides(source, target, undefined, undefined, [
      source,
      target,
      { id: 'blocker', x: 110, y: 44, width: 40, height: 12 },
    ])).not.toEqual(direct)
  })

  it('preserves obstacle-aware side scoring when candidates come from a spatial index', () => {
    const source = { id: 'source', x: 0, y: 0, width: 100, height: 100 }
    const target = { id: 'target', x: 300, y: 0, width: 100, height: 100 }
    const obstacles = [
      source,
      target,
      { id: 'blocker', x: 110, y: 44, width: 40, height: 12 },
      ...Array.from({ length: 2_000 }, (_, index) => ({
        id: `far-${index}`, x: index * 80, y: 10_000, width: 40, height: 40,
      })),
    ]

    expect(resolveCanvasEdgeSides(source, target, undefined, undefined, buildCanvasObstacleIndex(obstacles)))
      .toEqual(resolveCanvasEdgeSides(source, target, undefined, undefined, obstacles))
  })

  it('chooses an obstacle-aware side pair for a blocked diagonal layout', () => {
    expect(resolveCanvasEdgeSides(
      { id: 'source', x: 50, y: 50, width: 350, height: 250 },
      { id: 'target', x: 650, y: 300, width: 400, height: 450 },
      undefined,
      undefined,
      [{ id: 'blocker', x: 270, y: 350, width: 300, height: 56 }],
    )).toEqual({ fromSide: 'right', toSide: 'top' })
  })

  it('uses same-side handles when a group contains the other endpoint', () => {
    expect(resolveCanvasEdgeSides(
      { x: 0, y: 0, width: 500, height: 300 },
      { x: 30, y: 120, width: 80, height: 60 },
    )).toEqual({ fromSide: 'left', toSide: 'left' })
  })

  it('builds a complete snap index for a 10,000-node canvas', () => {
    const document = documentFrom({
      nodes: Array.from({ length: 10_000 }, (_, index) => textNode(
        `node-${index}`,
        (index % 100) * 240,
        Math.floor(index / 100) * 160,
        180,
        100,
      )),
      edges: [],
    })
    const index = buildCanvasSnapIndex(document, [])
    expect(index.rectsById.size).toBe(10_000)
    expect(index.byX).toHaveLength(30_000)
    expect(index.byY).toHaveLength(30_000)
  })

  it('keeps worst-case equal-spacing lookup inside an interactive budget', () => {
    const document = documentFrom({
      nodes: Array.from({ length: 20_000 }, (_, index) => textNode(`node-${index}`, index * 3, 0, 1, 100)),
      edges: [],
    })
    const index = buildCanvasSnapIndex(document, [])
    const started = performance.now()
    computeCanvasSnap(
      { x: 30_000.123, y: 0.123, width: 100.123, height: 99.123 },
      index,
      { thresholdFlow: 0.01 },
    )
    expect(performance.now() - started).toBeLessThan(250)
  })
})

describe('Canvas lasso geometry', () => {
  it('detects polygon/rectangle containment and edge intersection', () => {
    expect(polygonIntersectsRect(
      [{ x: 0, y: 0 }, { x: 20, y: 0 }, { x: 10, y: 20 }],
      { x: 9, y: 8, width: 2, height: 2 },
    )).toBe(true)
    expect(polygonIntersectsRect(
      [{ x: 0, y: 0 }, { x: 20, y: 0 }, { x: 10, y: 20 }],
      { x: 30, y: 30, width: 2, height: 2 },
    )).toBe(false)
  })

  it('selects intersecting nodes but not a group that contains the entire lasso', () => {
    const document = documentFrom({
      nodes: [
        { id: 'group', type: 'group', x: 0, y: 0, width: 200, height: 200 },
        textNode('inside', 30, 30, 30, 30),
        textNode('outside', 300, 300),
      ],
      edges: [],
    })
    const polygon = [
      { x: 20, y: 20 }, { x: 80, y: 20 }, { x: 80, y: 80 }, { x: 20, y: 80 },
    ]

    expect(canvasNodesIntersectPolygon(document.nodes, polygon).map((node) => node.id)).toEqual(['inside'])
    expect(canvasNodesIntersectPolygon(buildCanvasNodeSpatialIndex(document), polygon).map((node) => node.id))
      .toEqual(['inside'])
  })
})

describe('Canvas multi-select resize', () => {
  it('snapshots selected roots plus group closures and scales absolute geometry', () => {
    const document = documentFrom({
      nodes: [
        { id: 'group', type: 'group', x: 0, y: 0, width: 100, height: 100 },
        textNode('inside', 10, 10),
        textNode('outside', 200, 0),
      ],
      edges: [],
    })
    const snapshot = createCanvasResizeSnapshot(document, ['group', 'outside'], 'br')
    expect(snapshot?.nodes.map((node) => node.id)).toEqual(['group', 'inside', 'outside'])
    expect(snapshot).toMatchObject({
      anchor: { x: 0, y: 0 },
      diagonal: { x: 220, y: 100 },
    })

    const resized = resizeCanvasSelection(document, snapshot!, 2, 2)
    const geometry = Object.fromEntries(resized.nodes.flatMap((node) => 'kind' in node ? [] : [[node.id, node]]))
    expect(geometry.group).toMatchObject({ x: 0, y: 0, width: 200, height: 200 })
    expect(geometry.inside).toMatchObject({ x: 20, y: 20, width: 40, height: 40 })
    expect(geometry.outside).toMatchObject({ x: 400, y: 0, width: 40, height: 40 })
  })

  it('keeps a group-plus-descendant selection resizable as one scaling root', () => {
    const document = documentFrom({
      nodes: [
        { id: 'group', type: 'group', x: 0, y: 0, width: 200, height: 200 },
        textNode('inside', 20, 20, 40, 40),
      ],
      edges: [],
    })

    const snapshot = createCanvasResizeSnapshot(document, ['group', 'inside'], 'br')

    expect(snapshot).not.toBeNull()
    expect(snapshot?.nodes.map((node) => node.id)).toEqual(['group', 'inside'])
    expect(snapshot?.bounds).toEqual({ x: 0, y: 0, width: 200, height: 200 })
  })

  it('resolves free-axis and projected uniform scales without crossing node minimums', () => {
    const document = documentFrom({
      nodes: [textNode('a', 0, 0, 200, 200), textNode('b', 200, 0, 200, 200)],
      edges: [],
    })
    const snapshot = createCanvasResizeSnapshot(document, ['a', 'b'], 'br')!

    expect(resolveCanvasResizeScale(snapshot, { x: 800, y: 100 }, false))
      .toEqual({ scaleX: 2, scaleY: 0.5 })
    expect(resolveCanvasResizeScale(snapshot, { x: 800, y: 0 }, true))
      .toEqual({ scaleX: 1.6, scaleY: 1.6 })
    expect(resolveCanvasResizeScale(snapshot, { x: -100, y: -100 }, false))
      .toEqual({ scaleX: 0.8, scaleY: 0.5 })
  })
})
