import { analyzeCanvasDocument, canvasEntryLimitDiagnostics } from './json-canvas'
import { commitNodeRectangles } from './model'
import {
  type CanvasDocument,
  type CanvasEdge,
  type CanvasEdgeEntry,
  type CanvasSide,
  type Diagnostic,
  type JsonValue,
  type KnownCanvasNode,
  cloneJsonValue,
  isCanvasEdge,
  isKnownCanvasNode,
} from './types'

export interface FlowPoint {
  x: number
  y: number
}

export interface FlowCanvasNodeData {
  kind: KnownCanvasNode['type'] | 'diagnostic'
  canonicalId?: string
  text?: string
  file?: string
  subpath?: string
  url?: string
  label?: string
  background?: string
  backgroundStyle?: string
  color?: string
  diagnostic?: Diagnostic
}

/** Dependency-free DTO matching the subset consumed by @xyflow/svelte. */
export interface FlowCanvasNode {
  id: string
  type: `canvas-${KnownCanvasNode['type']}` | 'canvas-diagnostic'
  position: FlowPoint
  width: number
  height: number
  zIndex: number
  selectable: boolean
  draggable: boolean
  data: Readonly<FlowCanvasNodeData>
}

export type FlowNodeProjection = FlowCanvasNode

export interface FlowMarkerProjection {
  type: 'arrowclosed'
}

export interface FlowEdgeProjection {
  id: string
  source: string
  target: string
  sourceHandle?: string
  targetHandle?: string
  markerStart?: FlowMarkerProjection
  markerEnd?: FlowMarkerProjection
  label?: string
  data: Readonly<{
    canonicalId: string
    colorToken?: string
    effectiveFromEnd: 'none' | 'arrow'
    effectiveToEnd: 'none' | 'arrow'
  }>
}

export type FlowCanvasEdge = FlowEdgeProjection

export interface CanvasFlowProjection {
  nodes: FlowNodeProjection[]
  edges: FlowEdgeProjection[]
  diagnostics: Diagnostic[]
  options: Readonly<{
    nodeOrigin: readonly [0, 0]
    zIndexMode: 'manual'
    elevateNodesOnSelect: false
    connectionMode: 'loose'
  }>
}

export interface FlowNodeUpdate {
  id: string
  position?: FlowPoint
  width?: number
  height?: number
  /** Deliberately ignored: transient Flow state is never canonical. */
  selected?: boolean
  dragging?: boolean
  measured?: unknown
  zIndex?: number
  parentId?: string
}

export interface FlowConnection {
  source: string
  target: string
  sourceHandle?: string | null
  targetHandle?: string | null
}

function rawString(raw: JsonValue, key: string): string | undefined {
  return raw instanceof Map && typeof raw.get(key) === 'string' ? raw.get(key) as string : undefined
}

function entryId(entry: import('./types').CanvasNodeEntry | CanvasEdgeEntry): string | undefined {
  return 'kind' in entry ? rawString(entry.raw, 'id') : entry.id
}

function countIds(entries: Array<import('./types').CanvasNodeEntry | CanvasEdgeEntry>): Map<string, number> {
  const counts = new Map<string, number>()
  for (const entry of entries) {
    const id = entryId(entry)
    if (id !== undefined) counts.set(id, (counts.get(id) ?? 0) + 1)
  }
  return counts
}

function knownNodeData(node: KnownCanvasNode): FlowCanvasNodeData {
  const common = { canonicalId: node.id, kind: node.type, color: node.color } as const
  if (node.type === 'text') return { ...common, text: node.text }
  if (node.type === 'file') return { ...common, file: node.file, subpath: node.subpath }
  if (node.type === 'link') return { ...common, url: node.url }
  return {
    ...common,
    label: node.label,
    background: node.background,
    backgroundStyle: node.backgroundStyle,
  }
}

function duplicateNodeDiagnostic(id: string): Diagnostic {
  return {
    code: 'duplicate-node-id', severity: 'error', id, path: '$.nodes',
    message: `节点 id“${id}”重复；此投影不可交互`,
  }
}

export function flowHandleForSide(side: CanvasSide | undefined): string | undefined {
  return side === undefined ? undefined : `side:${side}`
}

export function sideFromFlowHandle(handle: string | null | undefined): CanvasSide | undefined {
  if (!handle?.startsWith('side:')) return undefined
  const side = handle.slice(5)
  return side === 'top' || side === 'right' || side === 'bottom' || side === 'left' ? side : undefined
}

/** Builds a disposable Svelte Flow view; it never exposes canonical mutable objects. */
export function projectCanvasToFlow(document: CanvasDocument): CanvasFlowProjection {
  const options = {
    nodeOrigin: [0, 0] as const,
    zIndexMode: 'manual' as const,
    elevateNodesOnSelect: false as const,
    connectionMode: 'loose' as const,
  }
  const limitDiagnostics = canvasEntryLimitDiagnostics(document.nodes.length, document.edges.length)
  if (limitDiagnostics.length > 0) {
    return { nodes: [], edges: [], diagnostics: limitDiagnostics, options }
  }
  const nodeCounts = countIds(document.nodes)
  const edgeCounts = countIds(document.edges)
  const nodes: FlowNodeProjection[] = []
  const endpointViewIds = new Map<string, string>()

  document.nodes.forEach((entry, index) => {
    const id = entryId(entry)
    const duplicate = id !== undefined && (nodeCounts.get(id) ?? 0) > 1
    const viewId = duplicate || id === undefined ? `__canvas_diagnostic_node_${index}` : id
    if (isKnownCanvasNode(entry)) {
      if (!duplicate) endpointViewIds.set(entry.id, viewId)
      nodes.push({
        id: viewId,
        type: duplicate ? 'canvas-diagnostic' : `canvas-${entry.type}`,
        position: { x: entry.x, y: entry.y },
        width: entry.width,
        height: entry.height,
        zIndex: index,
        selectable: !duplicate,
        draggable: !duplicate,
        data: duplicate
          ? { canonicalId: entry.id, kind: 'diagnostic', diagnostic: duplicateNodeDiagnostic(entry.id) }
          : knownNodeData(entry),
      })
      return
    }
    if (!entry.view) return
    if (id !== undefined && !duplicate) endpointViewIds.set(id, viewId)
    nodes.push({
      id: viewId,
      type: 'canvas-diagnostic',
      position: { x: entry.view.x, y: entry.view.y },
      width: entry.view.width,
      height: entry.view.height,
      zIndex: index,
      selectable: false,
      draggable: false,
      data: { canonicalId: id, kind: 'diagnostic', diagnostic: duplicate && id ? duplicateNodeDiagnostic(id) : { ...entry.diagnostic } },
    })
  })

  const edges: FlowEdgeProjection[] = []
  document.edges.forEach((entry) => {
    if (!isCanvasEdge(entry) || (edgeCounts.get(entry.id) ?? 0) !== 1) return
    const source = endpointViewIds.get(entry.fromNode)
    const target = endpointViewIds.get(entry.toNode)
    if (!source || !target || (nodeCounts.get(entry.fromNode) ?? 0) !== 1 || (nodeCounts.get(entry.toNode) ?? 0) !== 1) return
    const effectiveFromEnd = entry.fromEnd ?? 'none'
    const effectiveToEnd = entry.toEnd ?? 'arrow'
    edges.push({
      id: entry.id,
      source,
      target,
      sourceHandle: flowHandleForSide(entry.fromSide),
      targetHandle: flowHandleForSide(entry.toSide),
      markerStart: effectiveFromEnd === 'arrow' ? { type: 'arrowclosed' } : undefined,
      markerEnd: effectiveToEnd === 'arrow' ? { type: 'arrowclosed' } : undefined,
      label: entry.label,
      data: {
        canonicalId: entry.id,
        colorToken: entry.color,
        effectiveFromEnd,
        effectiveToEnd,
      },
    })
  })

  return {
    nodes,
    edges,
    diagnostics: analyzeCanvasDocument(document),
    options,
  }
}

/** Applies only committed geometry; selection, measurement, parent and z values are ignored. */
export function applyFlowNodeChanges(document: CanvasDocument, changes: Iterable<FlowNodeUpdate>): CanvasDocument {
  const byId = new Map(document.nodes.filter(isKnownCanvasNode).map((node) => [node.id, node]))
  return commitNodeRectangles(document, Array.from(changes).flatMap((change) => {
    const node = byId.get(change.id)
    if (!node || (!change.position && change.width === undefined && change.height === undefined)) return []
    return [{
      id: change.id,
      x: change.position?.x ?? node.x,
      y: change.position?.y ?? node.y,
      width: change.width ?? node.width,
      height: change.height ?? node.height,
    }]
  }))
}

export function flowConnectionToCanvasEdge(id: string, connection: FlowConnection): CanvasEdge {
  if (typeof id !== 'string' || typeof connection.source !== 'string' || typeof connection.target !== 'string') {
    throw new Error('连线必须有 id/source/target')
  }
  const fromSide = sideFromFlowHandle(connection.sourceHandle)
  const toSide = sideFromFlowHandle(connection.targetHandle)
  return {
    id,
    fromNode: connection.source,
    toNode: connection.target,
    fromSide,
    toSide,
    extras: new Map(),
    preservedInvalid: new Map(),
    optionalPresence: new Set([
      ...(fromSide ? ['fromSide'] : []),
      ...(toSide ? ['toSide'] : []),
    ]),
  }
}

/** Reconnects one unambiguous edge while preserving label/color/extras and end markers. */
export function applyFlowEdgeConnection(
  document: CanvasDocument,
  edgeId: string,
  connection: FlowConnection,
): CanvasDocument {
  const counts = countIds(document.edges)
  if ((counts.get(edgeId) ?? 0) !== 1) return document
  const index = document.edges.findIndex((entry) => isCanvasEdge(entry) && entry.id === edgeId)
  if (index < 0) return document
  const current = document.edges[index]
  if (!isCanvasEdge(current)) return document
  const fromSide = sideFromFlowHandle(connection.sourceHandle)
  const toSide = sideFromFlowHandle(connection.targetHandle)
  const next: CanvasEdge = {
    ...current,
    fromNode: connection.source,
    toNode: connection.target,
    fromSide,
    toSide,
    extras: cloneJsonValue(current.extras),
    preservedInvalid: cloneJsonValue(current.preservedInvalid),
    optionalPresence: new Set(current.optionalPresence),
  }
  next.preservedInvalid.delete('fromSide')
  next.preservedInvalid.delete('toSide')
  if (fromSide) next.optionalPresence.add('fromSide'); else next.optionalPresence.delete('fromSide')
  if (toSide) next.optionalPresence.add('toSide'); else next.optionalPresence.delete('toSide')
  const edges = [...document.edges]
  edges[index] = next
  return { ...document, edges }
}
