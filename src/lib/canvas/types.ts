/** Lossless representation used for numbers that must survive JSON round-trips. */
export interface LosslessNumber {
  readonly kind: 'lossless-number'
  readonly raw: string
}

export type JsonValue =
  | null
  | boolean
  | number
  | LosslessNumber
  | string
  | JsonValue[]
  | JsonObject

export type JsonObject = Map<string, JsonValue>

export type CanvasSide = 'top' | 'right' | 'bottom' | 'left'
export type CanvasEnd = 'none' | 'arrow'
export type GroupBackgroundStyle = 'cover' | 'ratio' | 'repeat'
export type CanvasNodeType = 'text' | 'file' | 'link' | 'group'

export interface Diagnostic {
  code: string
  severity: 'warning' | 'error'
  message: string
  path: string
  offset?: number
  line?: number
  column?: number
  id?: string
}

export interface CommonCanvasNode {
  id: string
  x: number
  y: number
  width: number
  height: number
  color?: string
  extras: JsonObject
  preservedInvalid: JsonObject
  optionalPresence: Set<string>
}

export interface TextCanvasNode extends CommonCanvasNode {
  type: 'text'
  text: string
}

export interface FileCanvasNode extends CommonCanvasNode {
  type: 'file'
  file: string
  subpath?: string
}

export interface LinkCanvasNode extends CommonCanvasNode {
  type: 'link'
  url: string
}

export interface GroupCanvasNode extends CommonCanvasNode {
  type: 'group'
  label?: string
  background?: string
  backgroundStyle?: GroupBackgroundStyle
}

export type KnownCanvasNode = TextCanvasNode | FileCanvasNode | LinkCanvasNode | GroupCanvasNode

export interface OpaqueNodeGeometry {
  id?: string
  type?: string
  x: number
  y: number
  width: number
  height: number
}

export interface OpaqueCanvasNode {
  kind: 'opaque-node'
  raw: JsonValue
  diagnostic: Diagnostic
  /** Safe common geometry is retained only for a non-editable diagnostic projection. */
  view?: OpaqueNodeGeometry
}

export type CanvasNodeEntry = KnownCanvasNode | OpaqueCanvasNode

export interface CanvasEdge {
  id: string
  fromNode: string
  fromSide?: CanvasSide
  fromEnd?: CanvasEnd
  toNode: string
  toSide?: CanvasSide
  toEnd?: CanvasEnd
  color?: string
  label?: string
  extras: JsonObject
  preservedInvalid: JsonObject
  optionalPresence: Set<string>
}

export interface OpaqueCanvasEdge {
  kind: 'opaque-edge'
  raw: JsonValue
  diagnostic: Diagnostic
}

export type CanvasEdgeEntry = CanvasEdge | OpaqueCanvasEdge

export interface CanvasDocument {
  nodes: CanvasNodeEntry[]
  edges: CanvasEdgeEntry[]
  extras: JsonObject
  presence: {
    nodes: boolean
    edges: boolean
  }
}

export function isLosslessNumber(value: JsonValue | undefined): value is LosslessNumber {
  return !!value && typeof value === 'object' && !Array.isArray(value) && !(value instanceof Map)
    && (value as LosslessNumber).kind === 'lossless-number'
}

export function isKnownCanvasNode(node: CanvasNodeEntry): node is KnownCanvasNode {
  return !('kind' in node)
}

export function isCanvasEdge(edge: CanvasEdgeEntry): edge is CanvasEdge {
  return !('kind' in edge)
}

export function cloneJsonValue<T extends JsonValue>(value: T): T {
  if (Array.isArray(value)) return value.map((item) => cloneJsonValue(item)) as T
  if (value instanceof Map) {
    const cloned: JsonObject = new Map()
    for (const [key, item] of value as JsonObject) cloned.set(key, cloneJsonValue(item))
    return cloned as T
  }
  if (isLosslessNumber(value)) return { kind: 'lossless-number', raw: value.raw } as T
  return value
}

export function cloneCanvasNode(node: CanvasNodeEntry): CanvasNodeEntry {
  if (!isKnownCanvasNode(node)) {
    return {
      kind: 'opaque-node',
      raw: cloneJsonValue(node.raw),
      diagnostic: { ...node.diagnostic },
      view: node.view ? { ...node.view } : undefined,
    }
  }
  return {
    ...node,
    extras: cloneJsonValue(node.extras),
    preservedInvalid: cloneJsonValue(node.preservedInvalid),
    optionalPresence: new Set(node.optionalPresence),
  }
}

export function cloneCanvasEdge(edge: CanvasEdgeEntry): CanvasEdgeEntry {
  if (!isCanvasEdge(edge)) {
    return {
      kind: 'opaque-edge',
      raw: cloneJsonValue(edge.raw),
      diagnostic: { ...edge.diagnostic },
    }
  }
  return {
    ...edge,
    extras: cloneJsonValue(edge.extras),
    preservedInvalid: cloneJsonValue(edge.preservedInvalid),
    optionalPresence: new Set(edge.optionalPresence),
  }
}

export function cloneCanvasDocument(document: CanvasDocument): CanvasDocument {
  return {
    nodes: document.nodes.map(cloneCanvasNode),
    edges: document.edges.map(cloneCanvasEdge),
    extras: cloneJsonValue(document.extras),
    presence: { ...document.presence },
  }
}

export function emptyCanvasDocument(): CanvasDocument {
  return {
    nodes: [],
    edges: [],
    extras: new Map(),
    presence: { nodes: true, edges: true },
  }
}
