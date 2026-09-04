import { commitNodeRectangles, freezeCanvasMove, isNodeContained, type NodePositionChange } from './model'
import {
  type CanvasDocument,
  type CanvasNodeEntry,
  type CanvasSide,
  type KnownCanvasNode,
  isKnownCanvasNode,
} from './types'

export interface CanvasPoint {
  x: number
  y: number
}

export interface CanvasRect extends CanvasPoint {
  width: number
  height: number
}

export type CanvasAlignDirection =
  | 'left'
  | 'center-h'
  | 'right'
  | 'top'
  | 'center-v'
  | 'bottom'

export type CanvasDistributeAxis = 'horizontal' | 'vertical'
export type ResizeCorner = 'tl' | 'tr' | 'bl' | 'br'

interface CandidateLine {
  axis: 'x' | 'y'
  value: number
  edge: 'min' | 'mid' | 'max'
  nodeId: string
  from: number
  to: number
}

export interface SnapIndex {
  byX: CandidateLine[]
  byY: CandidateLine[]
  rectsById: Map<string, CanvasRect>
}

export type SnapGuide =
  | {
    kind: 'alignment'
    axis: 'x' | 'y'
    value: number
    from: number
    to: number
  }
  | {
    kind: 'equal-spacing'
    mode: 'middle' | 'trailing'
    /** The axis along which the equal gaps are measured. */
    axis: 'x' | 'y'
    /** Perpendicular coordinate used to draw the annotation line. */
    value: number
    from: number
    to: number
    rects: [CanvasRect, CanvasRect, CanvasRect]
  }

export interface CanvasSnapOptions {
  thresholdFlow: number
  bypass?: boolean
  activeEdges?: {
    x: 'min' | 'max' | 'both' | 'none'
    y: 'min' | 'max' | 'both' | 'none'
  }
  enableEqualSpacing?: boolean
}

export interface CanvasSnapResult {
  deltaX: number
  deltaY: number
  guides: SnapGuide[]
}

export interface CanvasResizeSnapResult {
  rectangle: CanvasRect
  guides: SnapGuide[]
}

export interface CanvasResizeSnapshotNode extends CanvasRect {
  id: string
  type: KnownCanvasNode['type']
}

export interface CanvasResizeSnapshot {
  corner: ResizeCorner
  bounds: CanvasRect
  anchor: CanvasPoint
  diagonal: CanvasPoint
  nodes: CanvasResizeSnapshotNode[]
}

export interface CanvasAutoPanVelocity extends CanvasPoint {}

export interface CanvasEdgeSides {
  fromSide: CanvasSide
  toSide: CanvasSide
}

export interface CanvasObstacleRect extends CanvasRect {
  id?: string
}

export interface CanvasGroupFitInsets {
  top: number
  right: number
  bottom: number
  left: number
}

const OPPOSITE_SIDE: Record<CanvasSide, CanvasSide> = {
  top: 'bottom',
  right: 'left',
  bottom: 'top',
  left: 'right',
}

/**
 * Returns viewport velocity in screen pixels per second. Positive values move
 * the canvas right/down. The ramp avoids a sudden jump when entering the edge
 * activation zone and is capped when the pointer leaves the surface.
 */
export function computeCanvasAutoPanVelocity(
  point: CanvasPoint,
  bounds: Pick<CanvasRect, 'width' | 'height'>,
  edgeThreshold = 40,
  maxSpeed = 800,
): CanvasAutoPanVelocity {
  if (edgeThreshold <= 0 || maxSpeed <= 0 || bounds.width <= 0 || bounds.height <= 0) {
    return { x: 0, y: 0 }
  }
  const velocity = (coordinate: number, size: number): number => {
    if (coordinate < edgeThreshold) {
      return maxSpeed * Math.min(1, Math.max(0, (edgeThreshold - coordinate) / edgeThreshold))
    }
    if (coordinate > size - edgeThreshold) {
      return -maxSpeed * Math.min(1, Math.max(0, (coordinate - size + edgeThreshold) / edgeThreshold))
    }
    return 0
  }
  return { x: velocity(point.x, bounds.width), y: velocity(point.y, bounds.height) }
}

const CARDINAL_SIDES: CanvasSide[] = ['right', 'left', 'bottom', 'top']
const DIRECT_EDGE_SIDES: CanvasEdgeSides[] = [
  { fromSide: 'right', toSide: 'left' },
  { fromSide: 'left', toSide: 'right' },
  { fromSide: 'bottom', toSide: 'top' },
  { fromSide: 'top', toSide: 'bottom' },
]
const CORNER_EDGE_SIDES: CanvasEdgeSides[] = [
  { fromSide: 'right', toSide: 'top' },
  { fromSide: 'right', toSide: 'bottom' },
  { fromSide: 'left', toSide: 'top' },
  { fromSide: 'left', toSide: 'bottom' },
  { fromSide: 'bottom', toSide: 'left' },
  { fromSide: 'bottom', toSide: 'right' },
  { fromSide: 'top', toSide: 'left' },
  { fromSide: 'top', toSide: 'right' },
]

function sideAnchor(rect: CanvasRect, side: CanvasSide): CanvasPoint {
  if (side === 'right') return { x: rect.x + rect.width, y: rect.y + rect.height / 2 }
  if (side === 'left') return { x: rect.x, y: rect.y + rect.height / 2 }
  if (side === 'bottom') return { x: rect.x + rect.width / 2, y: rect.y + rect.height }
  return { x: rect.x + rect.width / 2, y: rect.y }
}

function sideNormal(side: CanvasSide): CanvasPoint {
  if (side === 'right') return { x: 1, y: 0 }
  if (side === 'left') return { x: -1, y: 0 }
  if (side === 'bottom') return { x: 0, y: 1 }
  return { x: 0, y: -1 }
}

function containsRectWithSlack(outer: CanvasRect, inner: CanvasRect, slack = 4): boolean {
  return inner.x >= outer.x - slack
    && inner.y >= outer.y - slack
    && inner.x + inner.width <= outer.x + outer.width + slack
    && inner.y + inner.height <= outer.y + outer.height + slack
}

function closestContainerSide(inner: CanvasRect, outer: CanvasRect): CanvasSide {
  const offsetX = (inner.x + inner.width / 2 - (outer.x + outer.width / 2)) / (outer.width / 2 || 1)
  const offsetY = (inner.y + inner.height / 2 - (outer.y + outer.height / 2)) / (outer.height / 2 || 1)
  if (Math.abs(offsetX) > Math.abs(offsetY)) return offsetX > 0 ? 'right' : 'left'
  return offsetY > 0 ? 'bottom' : 'top'
}

function segmentHitsInflatedRect(start: CanvasPoint, end: CanvasPoint, rect: CanvasRect, margin: number): boolean {
  const minX = rect.x - margin
  const minY = rect.y - margin
  const maxX = rect.x + rect.width + margin
  const maxY = rect.y + rect.height + margin
  const dx = end.x - start.x
  const dy = end.y - start.y
  let from = 0
  let to = 1
  for (const [direction, distance] of [
    [-dx, start.x - minX],
    [dx, maxX - start.x],
    [-dy, start.y - minY],
    [dy, maxY - start.y],
  ] as const) {
    if (direction === 0) {
      if (distance < 0) return false
      continue
    }
    const ratio = distance / direction
    if (direction < 0) {
      if (ratio > to) return false
      if (ratio > from) from = ratio
    } else {
      if (ratio < from) return false
      if (ratio < to) to = ratio
    }
  }
  return from < to
}

function routePolyline(source: CanvasPoint, target: CanvasPoint, sides: CanvasEdgeSides): CanvasPoint[] {
  const sourceNormal = sideNormal(sides.fromSide)
  const targetNormal = sideNormal(sides.toSide)
  const distance = Math.hypot(target.x - source.x, target.y - source.y)
  const stub = Math.min(40, distance / 2)
  return [
    source,
    { x: source.x + sourceNormal.x * stub, y: source.y + sourceNormal.y * stub },
    { x: target.x + targetNormal.x * stub, y: target.y + targetNormal.y * stub },
    target,
  ]
}

/**
 * Chooses facing, obstacle-aware handles without adding private edge data.
 * Explicit JSON Canvas sides remain authoritative; missing sides are derived
 * from current geometry and may therefore reroute after a node move.
 */
export function resolveCanvasEdgeSides(
  source: CanvasObstacleRect,
  target: CanvasObstacleRect,
  fromSide?: CanvasSide,
  toSide?: CanvasSide,
  obstacles: readonly CanvasObstacleRect[] = [],
): CanvasEdgeSides {
  if (fromSide && toSide) return { fromSide, toSide }

  const targetInsideSource = containsRectWithSlack(source, target)
  const sourceInsideTarget = containsRectWithSlack(target, source)
  if (targetInsideSource || sourceInsideTarget) {
    const inferred = targetInsideSource
      ? closestContainerSide(target, source)
      : closestContainerSide(source, target)
    const side = fromSide ?? toSide ?? inferred
    return { fromSide: fromSide ?? side, toSide: toSide ?? side }
  }

  const sourceCenter = { x: source.x + source.width / 2, y: source.y + source.height / 2 }
  const targetCenter = { x: target.x + target.width / 2, y: target.y + target.height / 2 }
  const dx = targetCenter.x - sourceCenter.x
  const dy = targetCenter.y - sourceCenter.y
  const directionLength = Math.hypot(dx, dy) || 1
  const direction = { x: dx / directionLength, y: dy / directionLength }
  const horizontalGap = Math.max(target.x - (source.x + source.width), source.x - (target.x + target.width))
  const verticalGap = Math.max(target.y - (source.y + source.height), source.y - (target.y + target.height))
  const clearlyHorizontal = horizontalGap > 0 && verticalGap <= 0
  const clearlyVertical = horizontalGap <= 0 && verticalGap > 0
  const constrainedCandidates = fromSide || toSide
    ? CARDINAL_SIDES.flatMap((candidateFrom) => CARDINAL_SIDES.map((candidateTo) => ({
      fromSide: candidateFrom,
      toSide: candidateTo,
    }))).filter((candidate) => (!fromSide || candidate.fromSide === fromSide) && (!toSide || candidate.toSide === toSide))
    : [...DIRECT_EDGE_SIDES, ...CORNER_EDGE_SIDES]

  const slack = 48
  const searchBounds = {
    x: Math.min(source.x, target.x) - slack,
    y: Math.min(source.y, target.y) - slack,
    width: Math.max(source.x + source.width, target.x + target.width) - Math.min(source.x, target.x) + slack * 2,
    height: Math.max(source.y + source.height, target.y + target.height) - Math.min(source.y, target.y) + slack * 2,
  }
  const localObstacles = obstacles.filter((obstacle) =>
    obstacle.id !== source.id
      && obstacle.id !== target.id
      && obstacle.x <= searchBounds.x + searchBounds.width
      && obstacle.x + obstacle.width >= searchBounds.x
      && obstacle.y <= searchBounds.y + searchBounds.height
      && obstacle.y + obstacle.height >= searchBounds.y,
  )

  let best = constrainedCandidates[0] ?? {
    fromSide: fromSide ?? 'right',
    toSide: toSide ?? OPPOSITE_SIDE[fromSide ?? 'right'],
  }
  let bestScore = Infinity
  for (const candidate of constrainedCandidates) {
    const sourceAnchor = sideAnchor(source, candidate.fromSide)
    const targetAnchor = sideAnchor(target, candidate.toSide)
    const path = routePolyline(sourceAnchor, targetAnchor, candidate)
    const hits = localObstacles.reduce((count, obstacle) => count + (path.some((point, index) =>
      index < path.length - 1 && segmentHitsInflatedRect(point, path[index + 1], obstacle, 8),
    ) ? 1 : 0), 0)
    const wrongAxis = (clearlyVertical
      ? Number(candidate.fromSide === 'left' || candidate.fromSide === 'right')
        + Number(candidate.toSide === 'left' || candidate.toSide === 'right')
      : clearlyHorizontal
        ? Number(candidate.fromSide === 'top' || candidate.fromSide === 'bottom')
          + Number(candidate.toSide === 'top' || candidate.toSide === 'bottom')
        : 0)
    const sourceNormal = sideNormal(candidate.fromSide)
    const targetNormal = sideNormal(candidate.toSide)
    const facingPenalty = 1 - (sourceNormal.x * direction.x + sourceNormal.y * direction.y)
      + 1 - (targetNormal.x * -direction.x + targetNormal.y * -direction.y)
    const score = Math.hypot(targetAnchor.x - sourceAnchor.x, targetAnchor.y - sourceAnchor.y)
      + hits * 600
      + wrongAxis * 200
      + facingPenalty * 120
    if (score < bestScore) {
      best = candidate
      bestScore = score
    }
  }
  return best
}

function editableKnownNodes(document: CanvasDocument): KnownCanvasNode[] {
  const counts = new Map<string, number>()
  for (const entry of document.nodes) {
    if (isKnownCanvasNode(entry)) counts.set(entry.id, (counts.get(entry.id) ?? 0) + 1)
  }
  return document.nodes.filter(isKnownCanvasNode).filter((node) => counts.get(node.id) === 1)
}

export function getCanvasNodesBounds(nodes: Iterable<CanvasNodeEntry | KnownCanvasNode>): CanvasRect | null {
  let minX = Infinity
  let minY = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  let found = false
  for (const entry of nodes) {
    if (!isKnownCanvasNode(entry as CanvasNodeEntry)) continue
    const node = entry as KnownCanvasNode
    found = true
    minX = Math.min(minX, node.x)
    minY = Math.min(minY, node.y)
    maxX = Math.max(maxX, node.x + node.width)
    maxY = Math.max(maxY, node.y + node.height)
  }
  return found ? { x: minX, y: minY, width: maxX - minX, height: maxY - minY } : null
}

/**
 * Fits a standard JSON Canvas group around its geometrically contained
 * members. Membership is deliberately frozen before resizing because the
 * format has no parentId/nesting field.
 */
export function fitCanvasGroupToContents(
  document: CanvasDocument,
  groupId: string,
  insets: Partial<CanvasGroupFitInsets> = {},
): CanvasDocument {
  const group = editableKnownNodes(document).find((node) => node.id === groupId && node.type === 'group')
  if (!group || group.type !== 'group') return document
  const members = editableKnownNodes(document).filter((node) => isNodeContained(group, node))
  const bounds = getCanvasNodesBounds(members)
  if (!bounds) return document
  const padding: CanvasGroupFitInsets = {
    top: insets.top ?? 52,
    right: insets.right ?? 36,
    bottom: insets.bottom ?? 36,
    left: insets.left ?? 36,
  }
  return commitNodeRectangles(document, [{
    id: group.id,
    x: bounds.x - padding.left,
    y: bounds.y - padding.top,
    width: Math.max(180, bounds.width + padding.left + padding.right),
    height: Math.max(120, bounds.height + padding.top + padding.bottom),
  }])
}

export function getCanvasSelectionRoots(
  document: CanvasDocument,
  nodeIds: Iterable<string>,
): KnownCanvasNode[] {
  const selectedIds = new Set(nodeIds)
  const selected = editableKnownNodes(document).filter((node) => selectedIds.has(node.id))
  const selectedGroups = selected.filter((node) => node.type === 'group')
  return selected.filter((node) =>
    !selectedGroups.some((group) => group.id !== node.id && isNodeContained(group, node)),
  )
}

export function getCanvasSelectionBounds(
  document: CanvasDocument,
  nodeIds: Iterable<string>,
): CanvasRect | null {
  return getCanvasNodesBounds(getCanvasSelectionRoots(document, nodeIds))
}

function changesForRootPositions(
  document: CanvasDocument,
  roots: KnownCanvasNode[],
  positions: ReadonlyMap<string, CanvasPoint>,
): NodePositionChange[] {
  const byId = new Map(editableKnownNodes(document).map((node) => [node.id, node]))
  const changes = new Map<string, NodePositionChange>()
  for (const root of roots) {
    const next = positions.get(root.id)
    if (!next) continue
    const delta = { x: next.x - root.x, y: next.y - root.y }
    const closure = root.type === 'group'
      ? freezeCanvasMove(document, [root.id]).nodeIds
      : [root.id]
    for (const id of closure) {
      if (changes.has(id)) continue
      const node = byId.get(id)
      if (node) changes.set(id, { id, x: node.x + delta.x, y: node.y + delta.y })
    }
  }
  return document.nodes.flatMap((entry) =>
    isKnownCanvasNode(entry) && changes.has(entry.id) ? [changes.get(entry.id)!] : [],
  )
}

export function alignCanvasSelection(
  document: CanvasDocument,
  nodeIds: Iterable<string>,
  direction: CanvasAlignDirection,
): NodePositionChange[] {
  const roots = getCanvasSelectionRoots(document, nodeIds)
  const bounds = getCanvasNodesBounds(roots)
  if (roots.length < 2 || !bounds) return []
  const positions = new Map<string, CanvasPoint>()
  for (const node of roots) {
    let x = node.x
    let y = node.y
    if (direction === 'left') x = bounds.x
    else if (direction === 'center-h') x = bounds.x + (bounds.width - node.width) / 2
    else if (direction === 'right') x = bounds.x + bounds.width - node.width
    else if (direction === 'top') y = bounds.y
    else if (direction === 'center-v') y = bounds.y + (bounds.height - node.height) / 2
    else y = bounds.y + bounds.height - node.height
    positions.set(node.id, { x, y })
  }
  return changesForRootPositions(document, roots, positions)
}

export function distributeCanvasSelection(
  document: CanvasDocument,
  nodeIds: Iterable<string>,
  axis: CanvasDistributeAxis,
): NodePositionChange[] {
  const roots = getCanvasSelectionRoots(document, nodeIds)
  if (roots.length < 3) return []
  const horizontal = axis === 'horizontal'
  const ordered = [...roots].sort((left, right) =>
    (horizontal ? left.x - right.x : left.y - right.y) || left.id.localeCompare(right.id),
  )
  const first = ordered[0]
  const last = ordered.at(-1)!
  const spanStart = horizontal ? first.x : first.y
  const spanEnd = horizontal ? last.x + last.width : last.y + last.height
  const totalSize = ordered.reduce((sum, node) => sum + (horizontal ? node.width : node.height), 0)
  const gap = (spanEnd - spanStart - totalSize) / (ordered.length - 1)
  const positions = new Map<string, CanvasPoint>()
  let cursor = spanStart
  for (const node of ordered) {
    positions.set(node.id, horizontal ? { x: cursor, y: node.y } : { x: node.x, y: cursor })
    cursor += (horizontal ? node.width : node.height) + gap
  }
  return changesForRootPositions(document, roots, positions)
}

export function spreadCanvasSelection(
  document: CanvasDocument,
  nodeIds: Iterable<string>,
  gap = 24,
): NodePositionChange[] {
  const roots = getCanvasSelectionRoots(document, nodeIds)
  if (roots.length < 2) return []
  const rects = roots.map((node) => ({
    id: node.id, x: node.x, y: node.y, width: node.width, height: node.height,
  })).sort((left, right) => left.x - right.x || left.y - right.y || left.id.localeCompare(right.id))
  for (let iteration = 0; iteration < 50; iteration++) {
    let moved = false
    for (let leftIndex = 0; leftIndex < rects.length; leftIndex++) {
      for (let rightIndex = leftIndex + 1; rightIndex < rects.length; rightIndex++) {
        const left = rects[leftIndex]
        const right = rects[rightIndex]
        const overlapX = left.x < right.x + right.width + gap
          && left.x + left.width + gap > right.x
        const overlapY = left.y < right.y + right.height + gap
          && left.y + left.height + gap > right.y
        if (!overlapX || !overlapY) continue
        moved = true
        const pushes = [
          { value: left.x + left.width + gap - right.x, apply: () => { right.x += left.x + left.width + gap - right.x } },
          { value: left.y + left.height + gap - right.y, apply: () => { right.y += left.y + left.height + gap - right.y } },
          { value: right.x + right.width + gap - left.x, apply: () => { left.x -= right.x + right.width + gap - left.x } },
          { value: right.y + right.height + gap - left.y, apply: () => { left.y -= right.y + right.height + gap - left.y } },
        ]
        pushes.reduce((best, candidate) => candidate.value < best.value ? candidate : best).apply()
      }
    }
    if (!moved) break
  }
  const positions = new Map(rects.map((rect) => [rect.id, { x: rect.x, y: rect.y }]))
  return changesForRootPositions(document, roots, positions)
}

function rectForNode(node: KnownCanvasNode): CanvasRect {
  return { x: node.x, y: node.y, width: node.width, height: node.height }
}

export function buildCanvasSnapIndex(
  document: CanvasDocument,
  movingNodeIds: Iterable<string>,
): SnapIndex {
  const excluded = new Set(freezeCanvasMove(document, movingNodeIds).nodeIds)
  const byX: CandidateLine[] = []
  const byY: CandidateLine[] = []
  const rectsById = new Map<string, CanvasRect>()
  for (const node of editableKnownNodes(document)) {
    if (excluded.has(node.id)) continue
    const rect = rectForNode(node)
    rectsById.set(node.id, rect)
    const xs = [
      ['min', rect.x],
      ['mid', rect.x + rect.width / 2],
      ['max', rect.x + rect.width],
    ] as const
    const ys = [
      ['min', rect.y],
      ['mid', rect.y + rect.height / 2],
      ['max', rect.y + rect.height],
    ] as const
    for (const [edge, value] of xs) {
      byX.push({ axis: 'x', edge, value, nodeId: node.id, from: rect.y, to: rect.y + rect.height })
    }
    for (const [edge, value] of ys) {
      byY.push({ axis: 'y', edge, value, nodeId: node.id, from: rect.x, to: rect.x + rect.width })
    }
  }
  const compare = (left: CandidateLine, right: CandidateLine) =>
    left.value - right.value || left.nodeId.localeCompare(right.nodeId) || left.edge.localeCompare(right.edge)
  byX.sort(compare)
  byY.sort(compare)
  return { byX, byY, rectsById }
}

function findBestAt(lines: CandidateLine[], target: number, tolerance: number): CandidateLine | null {
  let low = 0
  let high = lines.length
  const lower = target - tolerance
  while (low < high) {
    const middle = (low + high) >>> 1
    if (lines[middle].value < lower) low = middle + 1
    else high = middle
  }
  let best: CandidateLine | null = null
  let bestDistance = Infinity
  for (let index = low; index < lines.length && lines[index].value <= target + tolerance; index++) {
    const distance = Math.abs(lines[index].value - target)
    if (distance < bestDistance) {
      best = lines[index]
      bestDistance = distance
    }
  }
  return best
}

interface AxisHit {
  delta: number
  value: number
  candidate: CandidateLine
}

function bestAxisHit(
  lines: CandidateLine[],
  min: number,
  mid: number,
  max: number,
  tolerance: number,
  active: 'min' | 'max' | 'both' | 'none',
): AxisHit | null {
  if (active === 'none') return null
  const probes: number[] = []
  if (active === 'min' || active === 'both') probes.push(min)
  if (active === 'both') probes.push(mid)
  if (active === 'max' || active === 'both') probes.push(max)
  let best: AxisHit | null = null
  for (const probe of probes) {
    const candidate = findBestAt(lines, probe, tolerance)
    if (!candidate) continue
    const delta = candidate.value - probe
    if (!best || Math.abs(delta) < Math.abs(best.delta)) {
      best = { delta, value: candidate.value, candidate }
    }
  }
  return best
}

function perpendicularOverlap(axis: 'x' | 'y', left: CanvasRect, right: CanvasRect): boolean {
  return axis === 'x'
    ? left.y + left.height > right.y && right.y + right.height > left.y
    : left.x + left.width > right.x && right.x + right.width > left.x
}

function minAlong(axis: 'x' | 'y', rect: CanvasRect): number {
  return axis === 'x' ? rect.x : rect.y
}

function maxAlong(axis: 'x' | 'y', rect: CanvasRect): number {
  return minAlong(axis, rect) + (axis === 'x' ? rect.width : rect.height)
}

function equalSpacingHit(
  axis: 'x' | 'y',
  candidates: Iterable<CanvasRect>,
  source: CanvasRect,
  tolerance: number,
): { delta: number; mode: 'middle' | 'trailing'; neighbours: [CanvasRect, CanvasRect] } | null {
  const sourceMin = minAlong(axis, source)
  const sourceMax = maxAlong(axis, source)
  const sourceSize = sourceMax - sourceMin
  const before: CanvasRect[] = []
  const after: CanvasRect[] = []
  for (const rect of candidates) {
    if (!perpendicularOverlap(axis, source, rect)) continue
    if (maxAlong(axis, rect) <= sourceMin) before.push(rect)
    else if (minAlong(axis, rect) >= sourceMax) after.push(rect)
  }
  let best: { delta: number; mode: 'middle' | 'trailing'; neighbours: [CanvasRect, CanvasRect] } | null = null
  for (const leading of before) {
    for (const trailing of after) {
      const free = minAlong(axis, trailing) - maxAlong(axis, leading) - sourceSize
      if (free < 0) continue
      const target = maxAlong(axis, leading) + free / 2
      const delta = target - sourceMin
      if (Math.abs(delta) <= tolerance && (!best || Math.abs(delta) < Math.abs(best.delta))) {
        best = { delta, mode: 'middle', neighbours: [leading, trailing] }
      }
    }
  }
  if (best) return best

  const tryTrailing = (
    side: CanvasRect[],
    direction: 'before' | 'after',
  ): void => {
    if (side.length < 2) return
    side.sort((left, right) => direction === 'before'
      ? maxAlong(axis, right) - maxAlong(axis, left)
      : minAlong(axis, left) - minAlong(axis, right))
    const near = side[0]
    const far = side[1]
    if (!perpendicularOverlap(axis, near, far)) return
    const referenceGap = direction === 'before'
      ? minAlong(axis, near) - maxAlong(axis, far)
      : minAlong(axis, far) - maxAlong(axis, near)
    if (referenceGap < 0) return
    const target = direction === 'before'
      ? maxAlong(axis, near) + referenceGap
      : minAlong(axis, near) - referenceGap - sourceSize
    const delta = target - sourceMin
    if (Math.abs(delta) <= tolerance && (!best || Math.abs(delta) < Math.abs(best.delta))) {
      best = {
        delta,
        mode: 'trailing',
        neighbours: direction === 'before' ? [far, near] : [near, far],
      }
    }
  }
  tryTrailing(before, 'before')
  tryTrailing(after, 'after')
  return best
}

export function computeCanvasSnap(
  source: CanvasRect,
  index: SnapIndex,
  options: CanvasSnapOptions,
): CanvasSnapResult {
  if (options.bypass || options.thresholdFlow <= 0) return { deltaX: 0, deltaY: 0, guides: [] }
  const activeX = options.activeEdges?.x ?? 'both'
  const activeY = options.activeEdges?.y ?? 'both'
  const enableEqualSpacing = options.enableEqualSpacing ?? true
  const xHit = bestAxisHit(
    index.byX,
    source.x,
    source.x + source.width / 2,
    source.x + source.width,
    options.thresholdFlow,
    activeX,
  )
  const yHit = bestAxisHit(
    index.byY,
    source.y,
    source.y + source.height / 2,
    source.y + source.height,
    options.thresholdFlow,
    activeY,
  )
  let deltaX = xHit?.delta ?? 0
  let deltaY = yHit?.delta ?? 0
  const guides: SnapGuide[] = []
  if (xHit) {
    guides.push({
      kind: 'alignment',
      axis: 'x',
      value: xHit.value,
      from: Math.min(xHit.candidate.from, source.y + deltaY),
      to: Math.max(xHit.candidate.to, source.y + deltaY + source.height),
    })
  }
  if (yHit) {
    guides.push({
      kind: 'alignment',
      axis: 'y',
      value: yHit.value,
      from: Math.min(yHit.candidate.from, source.x + deltaX),
      to: Math.max(yHit.candidate.to, source.x + deltaX + source.width),
    })
  }
  if (!xHit && enableEqualSpacing && activeX === 'both') {
    const hit = equalSpacingHit('x', index.rectsById.values(), source, options.thresholdFlow)
    if (hit) {
      deltaX = hit.delta
      const snapped = { ...source, x: source.x + deltaX }
      const rects = [hit.neighbours[0], snapped, hit.neighbours[1]]
        .sort((left, right) => left.x - right.x) as [CanvasRect, CanvasRect, CanvasRect]
      guides.push({
        kind: 'equal-spacing',
        mode: hit.mode,
        axis: 'x',
        value: snapped.y + snapped.height / 2,
        from: rects[0].x,
        to: rects[2].x + rects[2].width,
        rects,
      })
    }
  }
  if (!yHit && enableEqualSpacing && activeY === 'both') {
    const hit = equalSpacingHit('y', index.rectsById.values(), source, options.thresholdFlow)
    if (hit) {
      deltaY = hit.delta
      const snapped = { ...source, y: source.y + deltaY }
      const rects = [hit.neighbours[0], snapped, hit.neighbours[1]]
        .sort((left, right) => left.y - right.y) as [CanvasRect, CanvasRect, CanvasRect]
      guides.push({
        kind: 'equal-spacing',
        mode: hit.mode,
        axis: 'y',
        value: snapped.x + snapped.width / 2,
        from: rects[0].y,
        to: rects[2].y + rects[2].height,
        rects,
      })
    }
  }
  return { deltaX, deltaY, guides }
}

export function computeCanvasResizeSnap(
  source: CanvasRect,
  index: SnapIndex,
  options: CanvasSnapOptions & { minimumWidth: number; minimumHeight: number },
): CanvasResizeSnapResult {
  const active = options.activeEdges ?? { x: 'none', y: 'none' }
  const snap = computeCanvasSnap(source, index, { ...options, enableEqualSpacing: false })
  const rectangle = { ...source }
  let keepXGuide = true
  let keepYGuide = true

  if (active.x === 'min') {
    rectangle.x += snap.deltaX
    rectangle.width -= snap.deltaX
  } else if (active.x === 'max') {
    rectangle.width += snap.deltaX
  }
  if (rectangle.width < options.minimumWidth) {
    rectangle.x = source.x
    rectangle.width = source.width
    keepXGuide = false
  }

  if (active.y === 'min') {
    rectangle.y += snap.deltaY
    rectangle.height -= snap.deltaY
  } else if (active.y === 'max') {
    rectangle.height += snap.deltaY
  }
  if (rectangle.height < options.minimumHeight) {
    rectangle.y = source.y
    rectangle.height = source.height
    keepYGuide = false
  }

  return {
    rectangle,
    guides: snap.guides.filter((guide) =>
      (guide.axis !== 'x' || keepXGuide) && (guide.axis !== 'y' || keepYGuide),
    ),
  }
}

function pointInPolygon(point: CanvasPoint, polygon: CanvasPoint[]): boolean {
  let inside = false
  for (let index = 0, previous = polygon.length - 1; index < polygon.length; previous = index++) {
    const currentPoint = polygon[index]
    const previousPoint = polygon[previous]
    const crosses = currentPoint.y > point.y !== previousPoint.y > point.y
      && point.x < (previousPoint.x - currentPoint.x) * (point.y - currentPoint.y)
        / (previousPoint.y - currentPoint.y || Number.EPSILON) + currentPoint.x
    if (crosses) inside = !inside
  }
  return inside
}

function orientation(a: CanvasPoint, b: CanvasPoint, c: CanvasPoint): number {
  return (b.y - a.y) * (c.x - b.x) - (b.x - a.x) * (c.y - b.y)
}

function onSegment(a: CanvasPoint, b: CanvasPoint, c: CanvasPoint): boolean {
  return Math.min(a.x, c.x) <= b.x && b.x <= Math.max(a.x, c.x)
    && Math.min(a.y, c.y) <= b.y && b.y <= Math.max(a.y, c.y)
}

function segmentsIntersect(
  firstStart: CanvasPoint,
  firstEnd: CanvasPoint,
  secondStart: CanvasPoint,
  secondEnd: CanvasPoint,
): boolean {
  const first = orientation(firstStart, firstEnd, secondStart)
  const second = orientation(firstStart, firstEnd, secondEnd)
  const third = orientation(secondStart, secondEnd, firstStart)
  const fourth = orientation(secondStart, secondEnd, firstEnd)
  if (first > 0 !== second > 0 && third > 0 !== fourth > 0) return true
  return (first === 0 && onSegment(firstStart, secondStart, firstEnd))
    || (second === 0 && onSegment(firstStart, secondEnd, firstEnd))
    || (third === 0 && onSegment(secondStart, firstStart, secondEnd))
    || (fourth === 0 && onSegment(secondStart, firstEnd, secondEnd))
}

function rectCorners(rect: CanvasRect): [CanvasPoint, CanvasPoint, CanvasPoint, CanvasPoint] {
  return [
    { x: rect.x, y: rect.y },
    { x: rect.x + rect.width, y: rect.y },
    { x: rect.x + rect.width, y: rect.y + rect.height },
    { x: rect.x, y: rect.y + rect.height },
  ]
}

function polygonEdges(points: CanvasPoint[]): Array<[CanvasPoint, CanvasPoint]> {
  return points.map((point, index) => [point, points[(index + 1) % points.length]])
}

function polygonCrossesRect(polygon: CanvasPoint[], rect: CanvasRect): boolean {
  const corners = rectCorners(rect)
  const rectEdges = polygonEdges(corners)
  return polygonEdges(polygon).some(([start, end]) =>
    rectEdges.some(([rectStart, rectEnd]) => segmentsIntersect(start, end, rectStart, rectEnd)),
  )
}

export function polygonIntersectsRect(polygon: CanvasPoint[], rect: CanvasRect): boolean {
  if (polygon.length < 3) return false
  const corners = rectCorners(rect)
  if (corners.some((corner) => pointInPolygon(corner, polygon))) return true
  if (polygon.some((point) =>
    point.x >= rect.x && point.x <= rect.x + rect.width
    && point.y >= rect.y && point.y <= rect.y + rect.height,
  )) return true
  return polygonCrossesRect(polygon, rect)
}

export function canvasNodesIntersectPolygon(
  nodes: Iterable<CanvasNodeEntry>,
  polygon: CanvasPoint[],
): KnownCanvasNode[] {
  return Array.from(nodes).filter(isKnownCanvasNode).filter((node) => {
    const rect = rectForNode(node)
    if (node.type !== 'group') return polygonIntersectsRect(polygon, rect)
    return rectCorners(rect).some((corner) => pointInPolygon(corner, polygon))
      || polygonCrossesRect(polygon, rect)
  })
}

export function createCanvasResizeSnapshot(
  document: CanvasDocument,
  nodeIds: Iterable<string>,
  corner: ResizeCorner,
): CanvasResizeSnapshot | null {
  const selectedIds = new Set(nodeIds)
  const roots = getCanvasSelectionRoots(document, selectedIds)
  const bounds = getCanvasNodesBounds(roots)
  const selectedCount = editableKnownNodes(document).filter((node) => selectedIds.has(node.id)).length
  if (selectedCount < 2 || roots.length === 0 || !bounds || bounds.width <= 0 || bounds.height <= 0) return null
  const included = new Set<string>()
  for (const root of roots) {
    const ids = root.type === 'group' ? freezeCanvasMove(document, [root.id]).nodeIds : [root.id]
    for (const id of ids) included.add(id)
  }
  const nodes = editableKnownNodes(document).filter((node) => included.has(node.id)).map((node) => ({
    id: node.id,
    type: node.type,
    x: node.x,
    y: node.y,
    width: node.width,
    height: node.height,
  }))
  const anchor = {
    x: corner === 'tl' || corner === 'bl' ? bounds.x + bounds.width : bounds.x,
    y: corner === 'tl' || corner === 'tr' ? bounds.y + bounds.height : bounds.y,
  }
  const dragged = {
    x: corner === 'tl' || corner === 'bl' ? bounds.x : bounds.x + bounds.width,
    y: corner === 'tl' || corner === 'tr' ? bounds.y : bounds.y + bounds.height,
  }
  return {
    corner,
    bounds,
    anchor,
    diagonal: { x: dragged.x - anchor.x, y: dragged.y - anchor.y },
    nodes,
  }
}

function minimumScale(snapshot: CanvasResizeSnapshot): CanvasPoint {
  let x = 0.05
  let y = 0.05
  for (const node of snapshot.nodes) {
    const minimumWidth = node.type === 'group' ? 180 : 160
    const minimumHeight = node.type === 'group' ? 120 : 100
    x = Math.max(x, Math.min(1, minimumWidth / node.width))
    y = Math.max(y, Math.min(1, minimumHeight / node.height))
  }
  return { x, y }
}

export function resolveCanvasResizeScale(
  snapshot: CanvasResizeSnapshot,
  cursor: CanvasPoint,
  uniform: boolean,
): { scaleX: number; scaleY: number } {
  const offX = cursor.x - snapshot.anchor.x
  const offY = cursor.y - snapshot.anchor.y
  const floor = minimumScale(snapshot)
  if (uniform) {
    const lengthSquared = snapshot.diagonal.x ** 2 + snapshot.diagonal.y ** 2
    let scale = lengthSquared > 0
      ? (offX * snapshot.diagonal.x + offY * snapshot.diagonal.y) / lengthSquared
      : 1
    if (!Number.isFinite(scale)) scale = 1
    scale = Math.max(scale, floor.x, floor.y)
    return { scaleX: scale, scaleY: scale }
  }
  let scaleX = snapshot.diagonal.x === 0 ? 1 : offX / snapshot.diagonal.x
  let scaleY = snapshot.diagonal.y === 0 ? 1 : offY / snapshot.diagonal.y
  if (!Number.isFinite(scaleX)) scaleX = 1
  if (!Number.isFinite(scaleY)) scaleY = 1
  return { scaleX: Math.max(scaleX, floor.x), scaleY: Math.max(scaleY, floor.y) }
}

export function resizeCanvasSelection(
  document: CanvasDocument,
  snapshot: CanvasResizeSnapshot,
  scaleX: number,
  scaleY: number,
): CanvasDocument {
  return commitNodeRectangles(document, snapshot.nodes.map((node) => ({
    id: node.id,
    x: snapshot.anchor.x + (node.x - snapshot.anchor.x) * scaleX,
    y: snapshot.anchor.y + (node.y - snapshot.anchor.y) * scaleY,
    width: node.width * scaleX,
    height: node.height * scaleY,
  })))
}
