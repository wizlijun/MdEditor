import {
  type CanvasDocument,
  type CanvasEdge,
  type CanvasEdgeEntry,
  type CanvasNodeEntry,
  type JsonObject,
  type JsonValue,
  type KnownCanvasNode,
  cloneCanvasDocument,
  cloneCanvasEdge,
  cloneCanvasNode,
  cloneJsonValue,
  isCanvasEdge,
  isKnownCanvasNode,
  isLosslessNumber,
} from './types'

export interface Point {
  x: number
  y: number
}

export interface NodePositionChange extends Point {
  id: string
}

export interface NodeRectangleChange extends NodePositionChange {
  width: number
  height: number
}

export interface FrozenGroupMove {
  readonly groupId: string
  readonly nodeIds: readonly string[]
}

export interface FrozenCanvasMove {
  readonly nodeIds: readonly string[]
}

export interface CanvasClipboardPayload {
  version: 1
  nodes: CanvasNodeEntry[]
  edges: CanvasEdgeEntry[]
  /** In-process only; never serialized into a JSON Canvas document. */
  sourceRoot?: string
}

export interface PasteCanvasOptions {
  offset?: Point
  idFactory?: (kind: 'node' | 'edge', previousId: string) => string
}

export interface PasteCanvasResult {
  document: CanvasDocument
  idMap: ReadonlyMap<string, string>
  insertedNodeIds: readonly string[]
  insertedEdgeIds: readonly string[]
}

function rawString(raw: JsonValue, key: string): string | undefined {
  return raw instanceof Map && typeof raw.get(key) === 'string' ? raw.get(key) as string : undefined
}

function entryId(entry: CanvasNodeEntry | CanvasEdgeEntry): string | undefined {
  return 'kind' in entry ? rawString(entry.raw, 'id') : entry.id
}

function idCounts(entries: Array<CanvasNodeEntry | CanvasEdgeEntry>): Map<string, number> {
  const counts = new Map<string, number>()
  for (const entry of entries) {
    const id = entryId(entry)
    if (id !== undefined) counts.set(id, (counts.get(id) ?? 0) + 1)
  }
  return counts
}

function roundedSafeInteger(value: number, label: string): number {
  if (!Number.isFinite(value)) throw new RangeError(`${label} 必须是有限数字`)
  const rounded = Math.round(value)
  if (!Number.isSafeInteger(rounded)) throw new RangeError(`${label} 超出安全整数范围`)
  return rounded
}

function editableNodeIndexes(document: CanvasDocument): Map<string, number> {
  const counts = idCounts(document.nodes)
  const indexes = new Map<string, number>()
  document.nodes.forEach((node, index) => {
    if (isKnownCanvasNode(node) && counts.get(node.id) === 1) indexes.set(node.id, index)
  })
  return indexes
}

/** Commits final Flow coordinates. Transient pointer coordinates never enter the model. */
export function commitNodePositions(
  document: CanvasDocument,
  changes: Iterable<NodePositionChange>,
): CanvasDocument {
  const indexes = editableNodeIndexes(document)
  const replacements = new Map<number, KnownCanvasNode>()
  for (const change of changes) {
    const index = indexes.get(change.id)
    if (index === undefined) continue
    const node = document.nodes[index]
    if (!isKnownCanvasNode(node)) continue
    const x = roundedSafeInteger(change.x, 'x')
    const y = roundedSafeInteger(change.y, 'y')
    if (node.x !== x || node.y !== y) replacements.set(index, { ...node, x, y })
  }
  if (replacements.size === 0) return document
  return {
    ...document,
    nodes: document.nodes.map((node, index) => replacements.get(index) ?? node),
  }
}

/** Commits final resize rectangles; the UI is responsible for its chosen minimum size. */
export function commitNodeRectangles(
  document: CanvasDocument,
  changes: Iterable<NodeRectangleChange>,
): CanvasDocument {
  const indexes = editableNodeIndexes(document)
  const replacements = new Map<number, KnownCanvasNode>()
  for (const change of changes) {
    const index = indexes.get(change.id)
    if (index === undefined) continue
    const node = document.nodes[index]
    if (!isKnownCanvasNode(node)) continue
    const next = {
      ...node,
      x: roundedSafeInteger(change.x, 'x'),
      y: roundedSafeInteger(change.y, 'y'),
      width: roundedSafeInteger(change.width, 'width'),
      height: roundedSafeInteger(change.height, 'height'),
    }
    if (next.width <= 0 || next.height <= 0) throw new RangeError('width/height 必须大于 0')
    if (node.x !== next.x || node.y !== next.y || node.width !== next.width || node.height !== next.height) {
      replacements.set(index, next)
    }
  }
  if (replacements.size === 0) return document
  return { ...document, nodes: document.nodes.map((node, index) => replacements.get(index) ?? node) }
}

export function isNodeContained(group: KnownCanvasNode, node: KnownCanvasNode): boolean {
  return group.type === 'group'
    && group.id !== node.id
    && node.x >= group.x
    && node.y >= group.y
    && node.x + node.width <= group.x + group.width
    && node.y + node.height <= group.y + group.height
}

/**
 * Freezes an entire drag selection at pointer-down. Explicitly dragged nodes
 * are always included; every dragged group additionally contributes its
 * fully-contained descendants. The set union prevents nested/overlapping
 * groups from applying the same delta more than once.
 */
export function freezeCanvasMove(
  document: CanvasDocument,
  draggedNodeIds: Iterable<string>,
): FrozenCanvasMove {
  const indexes = editableNodeIndexes(document)
  const movable = document.nodes.filter(isKnownCanvasNode).filter((node) => indexes.has(node.id))
  const byId = new Map(movable.map((node) => [node.id, node]))
  const included = new Set<string>()
  for (const id of draggedNodeIds) {
    if (byId.has(id)) included.add(id)
  }
  const visitGroup = (group: KnownCanvasNode): void => {
    for (const candidate of movable) {
      if (included.has(candidate.id) || !isNodeContained(group, candidate)) continue
      included.add(candidate.id)
      if (candidate.type === 'group') visitGroup(candidate)
    }
  }
  for (const id of Array.from(included)) {
    const node = byId.get(id)
    if (node?.type === 'group') visitGroup(node)
  }
  return { nodeIds: Array.from(included) }
}

/** Freezes one group's geometric membership for commands such as ungroup. */
export function freezeGroupMove(document: CanvasDocument, groupId: string): FrozenGroupMove {
  const group = document.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === groupId)
  if (!group || !isKnownCanvasNode(group) || group.type !== 'group') {
    throw new Error(`找不到可移动的 group“${groupId}”`)
  }
  return { groupId, ...freezeCanvasMove(document, [groupId]) }
}

export function moveFrozenNodes(
  document: CanvasDocument,
  frozen: FrozenCanvasMove,
  delta: Point,
): CanvasDocument {
  const indexes = editableNodeIndexes(document)
  const changes: NodePositionChange[] = []
  const dx = roundedSafeInteger(delta.x, 'delta.x')
  const dy = roundedSafeInteger(delta.y, 'delta.y')
  for (const id of new Set(frozen.nodeIds)) {
    const index = indexes.get(id)
    const node = index === undefined ? undefined : document.nodes[index]
    if (!node || !isKnownCanvasNode(node)) continue
    changes.push({ id, x: node.x + dx, y: node.y + dy })
  }
  return commitNodePositions(document, changes)
}

export function insertCanvasNode(document: CanvasDocument, node: KnownCanvasNode, index = document.nodes.length): CanvasDocument {
  if (idCounts([...document.nodes, ...document.edges]).has(node.id)) throw new Error(`id“${node.id}”已存在`)
  const at = Math.max(0, Math.min(document.nodes.length, Math.trunc(index)))
  return { ...document, nodes: [...document.nodes.slice(0, at), cloneCanvasNode(node), ...document.nodes.slice(at)] }
}

export function insertCanvasEdge(document: CanvasDocument, edge: CanvasEdge): CanvasDocument {
  if (idCounts([...document.nodes, ...document.edges]).has(edge.id)) throw new Error(`id“${edge.id}”已存在`)
  return { ...document, edges: [...document.edges, cloneCanvasEdge(edge)] }
}

export function deleteCanvasSelection(
  document: CanvasDocument,
  nodeIds: ReadonlySet<string>,
  edgeIds: ReadonlySet<string> = new Set(),
): CanvasDocument {
  const editable = editableNodeIndexes(document)
  const deletedNodes = new Set(Array.from(nodeIds).filter((id) => editable.has(id)))
  const edges = document.edges.filter((entry) => {
    if (!isCanvasEdge(entry)) return true
    return !edgeIds.has(entry.id) && !deletedNodes.has(entry.fromNode) && !deletedNodes.has(entry.toNode)
  })
  if (deletedNodes.size === 0 && edges.length === document.edges.length) return document
  return {
    ...document,
    nodes: document.nodes.filter((entry) => !isKnownCanvasNode(entry) || !deletedNodes.has(entry.id)),
    edges,
  }
}

export type LayerDirection = 'front' | 'back' | 'forward' | 'backward'

export function reorderCanvasNodes(
  document: CanvasDocument,
  nodeIds: ReadonlySet<string>,
  direction: LayerDirection,
): CanvasDocument {
  const editable = editableNodeIndexes(document)
  const selected = new Set(Array.from(nodeIds).filter((id) => editable.has(id)))
  if (selected.size === 0) return document
  const isSelected = (entry: CanvasNodeEntry): boolean => isKnownCanvasNode(entry) && selected.has(entry.id)
  let nodes: CanvasNodeEntry[]
  if (direction === 'front' || direction === 'back') {
    const picked = document.nodes.filter(isSelected)
    const rest = document.nodes.filter((entry) => !isSelected(entry))
    nodes = direction === 'front' ? [...rest, ...picked] : [...picked, ...rest]
  } else {
    nodes = [...document.nodes]
    if (direction === 'forward') {
      for (let index = nodes.length - 2; index >= 0; index--) {
        if (isSelected(nodes[index]) && !isSelected(nodes[index + 1])) {
          ;[nodes[index], nodes[index + 1]] = [nodes[index + 1], nodes[index]]
        }
      }
    } else {
      for (let index = 1; index < nodes.length; index++) {
        if (isSelected(nodes[index]) && !isSelected(nodes[index - 1])) {
          ;[nodes[index], nodes[index - 1]] = [nodes[index - 1], nodes[index]]
        }
      }
    }
  }
  return nodes.every((node, index) => node === document.nodes[index]) ? document : { ...document, nodes }
}

function edgeEndpoints(entry: CanvasEdgeEntry): { fromNode: string; toNode: string } | undefined {
  if (isCanvasEdge(entry)) return entry
  const fromNode = rawString(entry.raw, 'fromNode')
  const toNode = rawString(entry.raw, 'toNode')
  return fromNode !== undefined && toNode !== undefined ? { fromNode, toNode } : undefined
}

export function copyCanvasSelection(document: CanvasDocument, nodeIds: ReadonlySet<string>): CanvasClipboardPayload {
  const selected = new Set(freezeCanvasMove(document, nodeIds).nodeIds)
  return {
    version: 1,
    nodes: document.nodes.filter((node) => isKnownCanvasNode(node) && selected.has(node.id)).map(cloneCanvasNode),
    edges: document.edges.filter((edge) => {
      const endpoints = edgeEndpoints(edge)
      return !!endpoints && selected.has(endpoints.fromNode) && selected.has(endpoints.toNode)
    }).map(cloneCanvasEdge),
  }
}

let fallbackId = 0

function defaultIdFactory(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  fallbackId += 1
  return `canvas-${Date.now().toString(36)}-${fallbackId.toString(36)}`
}

function uniqueId(
  used: Set<string>,
  kind: 'node' | 'edge',
  previousId: string,
  factory: NonNullable<PasteCanvasOptions['idFactory']>,
): string {
  for (let attempt = 0; attempt < 1_000; attempt++) {
    const candidate = factory(kind, previousId)
    if (candidate && !used.has(candidate)) { used.add(candidate); return candidate }
  }
  throw new Error('无法生成唯一 Canvas id')
}

function updateOpaqueTopLevel(raw: JsonValue, updates: Record<string, JsonValue>): JsonValue {
  if (!(raw instanceof Map)) return cloneJsonValue(raw)
  const object: JsonObject = cloneJsonValue(raw)
  for (const [key, value] of Object.entries(updates)) object.set(key, cloneJsonValue(value))
  return object
}

export function pasteCanvasSelection(
  document: CanvasDocument,
  payload: CanvasClipboardPayload,
  options: PasteCanvasOptions = {},
): PasteCanvasResult {
  if (payload.version !== 1) throw new Error(`不支持 Canvas clipboard version ${String(payload.version)}`)
  const offset = options.offset ?? { x: 24, y: 24 }
  const dx = roundedSafeInteger(offset.x, 'offset.x')
  const dy = roundedSafeInteger(offset.y, 'offset.y')
  const factory = options.idFactory ?? (() => defaultIdFactory())
  const used = new Set(idCounts([...document.nodes, ...document.edges]).keys())
  const idMap = new Map<string, string>()
  const pastedNodes: CanvasNodeEntry[] = []

  for (const entry of payload.nodes) {
    const oldId = entryId(entry)
    if (oldId === undefined || idMap.has(oldId)) continue
    const id = uniqueId(used, 'node', oldId, factory)
    idMap.set(oldId, id)
    if (isKnownCanvasNode(entry)) {
      pastedNodes.push({
        ...cloneCanvasNode(entry) as KnownCanvasNode,
        id,
        x: roundedSafeInteger(entry.x + dx, 'x'),
        y: roundedSafeInteger(entry.y + dy, 'y'),
      })
    } else {
      const raw = updateOpaqueTopLevel(entry.raw, { id })
      pastedNodes.push({ ...cloneCanvasNode(entry), raw })
    }
  }

  const pastedEdges: CanvasEdgeEntry[] = []
  for (const entry of payload.edges) {
    const oldId = entryId(entry)
    const endpoints = edgeEndpoints(entry)
    if (oldId === undefined || !endpoints) continue
    const fromNode = idMap.get(endpoints.fromNode)
    const toNode = idMap.get(endpoints.toNode)
    if (!fromNode || !toNode) continue
    const id = uniqueId(used, 'edge', oldId, factory)
    if (isCanvasEdge(entry)) {
      pastedEdges.push({ ...cloneCanvasEdge(entry) as CanvasEdge, id, fromNode, toNode })
    } else {
      pastedEdges.push({
        ...cloneCanvasEdge(entry),
        raw: updateOpaqueTopLevel(entry.raw, { id, fromNode, toNode }),
      })
    }
  }

  return {
    document: {
      ...document,
      nodes: [...document.nodes, ...pastedNodes],
      edges: [...document.edges, ...pastedEdges],
    },
    idMap,
    insertedNodeIds: pastedNodes.map((entry) => entryId(entry)!).filter(Boolean),
    insertedEdgeIds: pastedEdges.map((entry) => entryId(entry)!).filter(Boolean),
  }
}

/** Changes a known node payload without exposing mutable canonical references to the UI. */
export function updateCanvasNode(
  document: CanvasDocument,
  id: string,
  update: (node: KnownCanvasNode) => KnownCanvasNode,
): CanvasDocument {
  const index = editableNodeIndexes(document).get(id)
  if (index === undefined) return document
  const node = document.nodes[index]
  if (!isKnownCanvasNode(node)) return document
  const next = update(cloneCanvasNode(node) as KnownCanvasNode)
  if (next.id !== id) throw new Error('updateCanvasNode 不能修改 id')
  const nodes = [...document.nodes]
  nodes[index] = next
  return { ...document, nodes }
}

/** Changes one unambiguous edge while preserving its extension containers. */
export function updateCanvasEdge(
  document: CanvasDocument,
  id: string,
  update: (edge: CanvasEdge) => CanvasEdge,
): CanvasDocument {
  const counts = idCounts(document.edges)
  if ((counts.get(id) ?? 0) !== 1) return document
  const index = document.edges.findIndex((edge) => isCanvasEdge(edge) && edge.id === id)
  if (index < 0) return document
  const edge = document.edges[index]
  if (!isCanvasEdge(edge)) return document
  const next = update(cloneCanvasEdge(edge) as CanvasEdge)
  if (next.id !== id) throw new Error('updateCanvasEdge 不能修改 id')
  const edges = [...document.edges]
  edges[index] = next
  return { ...document, edges }
}

export function cloneClipboardPayload(payload: CanvasClipboardPayload): CanvasClipboardPayload {
  return {
    version: 1,
    nodes: payload.nodes.map(cloneCanvasNode),
    edges: payload.edges.map(cloneCanvasEdge),
  }
}

export { cloneCanvasDocument, cloneJsonValue }
