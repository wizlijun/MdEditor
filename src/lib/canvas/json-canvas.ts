import {
  type CanvasDocument,
  type CanvasEdge,
  type CanvasEdgeEntry,
  type CanvasEnd,
  type CanvasNodeEntry,
  type CanvasSide,
  type Diagnostic,
  type GroupBackgroundStyle,
  type JsonObject,
  type JsonValue,
  type KnownCanvasNode,
  cloneJsonValue,
  isCanvasEdge,
  isKnownCanvasNode,
  isLosslessNumber,
} from './types'

export type DecodeCanvasResult =
  | { ok: true; document: CanvasDocument; diagnostics: Diagnostic[]; source: string }
  | { ok: false; diagnostics: Diagnostic[]; source: string }

export const MAX_CANVAS_NODES = 20_000
export const MAX_CANVAS_EDGES = 40_000

export function canvasEntryLimitDiagnostics(
  nodeCount: number,
  edgeCount: number,
): Diagnostic[] {
  const diagnostics: Diagnostic[] = []
  if (nodeCount > MAX_CANVAS_NODES) diagnostics.push({
    code: 'canvas-node-limit',
    severity: 'error',
    message: `节点数量 ${nodeCount} 超过安全上限 ${MAX_CANVAS_NODES}；画布以只读诊断模式打开`,
    path: '$.nodes',
  })
  if (edgeCount > MAX_CANVAS_EDGES) diagnostics.push({
    code: 'canvas-edge-limit',
    severity: 'error',
    message: `连线数量 ${edgeCount} 超过安全上限 ${MAX_CANVAS_EDGES}；画布以只读诊断模式打开`,
    path: '$.edges',
  })
  return diagnostics
}

class JsonCanvasParseError extends Error {
  constructor(
    readonly code: string,
    message: string,
    readonly path: string,
    readonly offset: number,
  ) {
    super(message)
  }
}

const JSON_NUMBER_TOKEN = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/

class LosslessJsonParser {
  private index = 0

  constructor(
    private readonly source: string,
    private readonly maxDepth: number,
  ) {}

  parse(): JsonValue {
    this.skipWhitespace()
    const value = this.parseValue('$', 0)
    this.skipWhitespace()
    if (this.index !== this.source.length) this.fail('json-syntax', 'JSON 值之后存在多余内容', '$')
    return value
  }

  private parseValue(path: string, depth: number): JsonValue {
    if (depth > this.maxDepth) this.fail('json-depth', `JSON 嵌套深度超过 ${this.maxDepth}`, path)
    this.skipWhitespace()
    const char = this.source[this.index]
    if (char === '{') return this.parseObject(path, depth + 1)
    if (char === '[') return this.parseArray(path, depth + 1)
    if (char === '"') return this.parseString(path)
    if (char === '-' || (char >= '0' && char <= '9')) return this.parseNumber(path)
    if (this.source.startsWith('true', this.index)) { this.index += 4; return true }
    if (this.source.startsWith('false', this.index)) { this.index += 5; return false }
    if (this.source.startsWith('null', this.index)) { this.index += 4; return null }
    this.fail('json-syntax', '无效的 JSON 值', path)
  }

  private parseObject(path: string, depth: number): JsonObject {
    this.index++
    const object: JsonObject = new Map()
    this.skipWhitespace()
    if (this.source[this.index] === '}') { this.index++; return object }
    while (this.index < this.source.length) {
      this.skipWhitespace()
      const keyOffset = this.index
      if (this.source[this.index] !== '"') this.fail('json-syntax', '对象属性名必须是字符串', path)
      const key = this.parseString(path)
      const keyPath = appendPath(path, key)
      if (object.has(key)) {
        throw new JsonCanvasParseError('duplicate-key', `JSON 对象包含重复属性“${key}”`, keyPath, keyOffset)
      }
      this.skipWhitespace()
      if (this.source[this.index] !== ':') this.fail('json-syntax', '对象属性名之后缺少冒号', keyPath)
      this.index++
      object.set(key, this.parseValue(keyPath, depth))
      this.skipWhitespace()
      if (this.source[this.index] === '}') { this.index++; return object }
      if (this.source[this.index] !== ',') this.fail('json-syntax', '对象属性之间缺少逗号', path)
      this.index++
    }
    this.fail('json-syntax', '对象没有结束', path)
  }

  private parseArray(path: string, depth: number): JsonValue[] {
    this.index++
    const array: JsonValue[] = []
    this.skipWhitespace()
    if (this.source[this.index] === ']') { this.index++; return array }
    while (this.index < this.source.length) {
      array.push(this.parseValue(`${path}[${array.length}]`, depth))
      this.skipWhitespace()
      if (this.source[this.index] === ']') { this.index++; return array }
      if (this.source[this.index] !== ',') this.fail('json-syntax', '数组元素之间缺少逗号', path)
      this.index++
    }
    this.fail('json-syntax', '数组没有结束', path)
  }

  private parseString(path: string): string {
    const start = this.index
    this.index++
    let escaped = false
    while (this.index < this.source.length) {
      const char = this.source[this.index++]
      if (!escaped && char === '"') {
        try {
          return JSON.parse(this.source.slice(start, this.index)) as string
        } catch {
          this.fail('json-syntax', '无效的 JSON 字符串', path, start)
        }
      }
      if (!escaped && char === '\\') escaped = true
      else escaped = false
    }
    this.fail('json-syntax', '字符串没有结束', path, start)
  }

  private parseNumber(path: string): JsonValue {
    const match = /^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/.exec(this.source.slice(this.index))
    if (!match) this.fail('json-syntax', '无效的 JSON 数字', path)
    const raw = match[0]
    this.index += raw.length
    return { kind: 'lossless-number', raw }
  }

  private skipWhitespace(): void {
    while (this.index < this.source.length && ' \t\r\n'.includes(this.source[this.index])) this.index++
  }

  private fail(code: string, message: string, path: string, offset = this.index): never {
    throw new JsonCanvasParseError(code, message, path, offset)
  }
}

function appendPath(path: string, key: string): string {
  return /^[A-Za-z_$][\w$]*$/.test(key) ? `${path}.${key}` : `${path}[${JSON.stringify(key)}]`
}

function lineAndColumn(source: string, offset: number): { line: number; column: number } {
  const before = source.slice(0, offset)
  const lines = before.split('\n')
  return { line: lines.length, column: (lines.at(-1)?.length ?? 0) + 1 }
}

function parseFailure(source: string, error: unknown): DecodeCanvasResult {
  if (error instanceof JsonCanvasParseError) {
    return {
      ok: false,
      source,
      diagnostics: [{
        code: error.code,
        severity: 'error',
        message: error.message,
        path: error.path,
        offset: error.offset,
        ...lineAndColumn(source, error.offset),
      }],
    }
  }
  return {
    ok: false,
    source,
    diagnostics: [{ code: 'json-syntax', severity: 'error', message: String(error), path: '$' }],
  }
}

function requiredString(object: JsonObject, key: string): string | undefined {
  const value = object.get(key)
  return typeof value === 'string' ? value : undefined
}

function safeInteger(value: JsonValue | undefined): number | undefined {
  const number = typeof value === 'number'
    ? value
    : isLosslessNumber(value) ? Number(value.raw) : Number.NaN
  return Number.isSafeInteger(number) ? number : undefined
}

function objectId(raw: JsonValue): string | undefined {
  return raw instanceof Map ? requiredString(raw, 'id') : undefined
}

function opaqueNodeView(raw: JsonValue): import('./types').OpaqueNodeGeometry | undefined {
  if (!(raw instanceof Map)) return undefined
  const x = safeInteger(raw.get('x'))
  const y = safeInteger(raw.get('y'))
  const width = safeInteger(raw.get('width'))
  const height = safeInteger(raw.get('height'))
  if (x === undefined || y === undefined || width === undefined || height === undefined || width <= 0 || height <= 0) return undefined
  return {
    id: requiredString(raw, 'id'),
    type: requiredString(raw, 'type'),
    x, y, width, height,
  }
}

const COMMON_NODE_KEYS = new Set(['id', 'type', 'x', 'y', 'width', 'height', 'color'])
const NODE_KEYS: Record<string, Set<string>> = {
  text: new Set([...COMMON_NODE_KEYS, 'text']),
  file: new Set([...COMMON_NODE_KEYS, 'file', 'subpath']),
  link: new Set([...COMMON_NODE_KEYS, 'url']),
  group: new Set([...COMMON_NODE_KEYS, 'label', 'background', 'backgroundStyle']),
}

function decodeNode(raw: JsonValue, index: number, diagnostics: Diagnostic[]): CanvasNodeEntry {
  const path = `$.nodes[${index}]`
  const invalid = (message: string): CanvasNodeEntry => {
    const diagnostic: Diagnostic = { code: 'invalid-node', severity: 'error', message, path, id: objectId(raw) }
    diagnostics.push(diagnostic)
    return { kind: 'opaque-node', raw: cloneJsonValue(raw), diagnostic, view: opaqueNodeView(raw) }
  }
  if (!(raw instanceof Map)) return invalid('节点必须是 JSON 对象')
  const id = requiredString(raw, 'id')
  const type = requiredString(raw, 'type')
  if (id === undefined) return invalid('节点 id 必须是字符串')
  if (type === undefined || !Object.hasOwn(NODE_KEYS, type)) return invalid(type !== undefined ? `未知节点类型“${type}”` : '节点 type 必须是字符串')
  const x = safeInteger(raw.get('x'))
  const y = safeInteger(raw.get('y'))
  const width = safeInteger(raw.get('width'))
  const height = safeInteger(raw.get('height'))
  if (x === undefined || y === undefined || width === undefined || height === undefined || width <= 0 || height <= 0) {
    return invalid('节点坐标和尺寸必须是安全整数，且 width/height 必须大于 0')
  }
  const payloadKey = type === 'text' ? 'text' : type === 'file' ? 'file' : type === 'link' ? 'url' : undefined
  if (payloadKey && typeof raw.get(payloadKey) !== 'string') return invalid(`节点 ${payloadKey} 必须是字符串`)

  const extras: JsonObject = new Map()
  for (const [key, value] of raw) if (!NODE_KEYS[type].has(key)) extras.set(key, cloneJsonValue(value))
  const preservedInvalid: JsonObject = new Map()
  const optionalPresence = new Set<string>()
  const common = { id, x, y, width, height, extras, preservedInvalid, optionalPresence }

  let color: string | undefined
  if (raw.has('color')) {
    optionalPresence.add('color')
    const value = raw.get('color')
    if (typeof value === 'string') color = value
    else preservedInvalid.set('color', cloneJsonValue(value!))
  }

  let node: KnownCanvasNode
  if (type === 'text') node = { ...common, type, text: raw.get('text') as string }
  else if (type === 'link') node = { ...common, type, url: raw.get('url') as string }
  else if (type === 'file') {
    let subpath: string | undefined
    if (raw.has('subpath')) {
      optionalPresence.add('subpath')
      const value = raw.get('subpath')
      if (typeof value === 'string' && value.startsWith('#')) subpath = value
      else preservedInvalid.set('subpath', cloneJsonValue(value!))
    }
    node = { ...common, type, file: raw.get('file') as string, subpath }
  } else {
    let label: string | undefined
    let background: string | undefined
    let backgroundStyle: GroupBackgroundStyle | undefined
    for (const key of ['label', 'background', 'backgroundStyle'] as const) {
      if (!raw.has(key)) continue
      optionalPresence.add(key)
      const value = raw.get(key)
      if (key === 'backgroundStyle') {
        if (value === 'cover' || value === 'ratio' || value === 'repeat') backgroundStyle = value
        else preservedInvalid.set(key, cloneJsonValue(value!))
      } else if (typeof value === 'string') {
        if (key === 'label') label = value
        else background = value
      } else preservedInvalid.set(key, cloneJsonValue(value!))
    }
    node = { ...common, type: 'group', label, background, backgroundStyle }
  }
  if (color !== undefined) node.color = color
  for (const key of preservedInvalid.keys()) {
    diagnostics.push({
      code: 'invalid-optional-field', severity: 'warning',
      message: `节点可选字段“${key}”无效，已原样保留`, path: appendPath(path, key), id,
    })
  }
  return node
}

const EDGE_KEYS = new Set([
  'id', 'fromNode', 'fromSide', 'fromEnd', 'toNode', 'toSide', 'toEnd', 'color', 'label',
])
const SIDES = new Set<CanvasSide>(['top', 'right', 'bottom', 'left'])
const ENDS = new Set<CanvasEnd>(['none', 'arrow'])

function decodeEdge(raw: JsonValue, index: number, diagnostics: Diagnostic[]): CanvasEdgeEntry {
  const path = `$.edges[${index}]`
  const invalid = (message: string): CanvasEdgeEntry => {
    const diagnostic: Diagnostic = { code: 'invalid-edge', severity: 'error', message, path, id: objectId(raw) }
    diagnostics.push(diagnostic)
    return { kind: 'opaque-edge', raw: cloneJsonValue(raw), diagnostic }
  }
  if (!(raw instanceof Map)) return invalid('连线必须是 JSON 对象')
  const id = requiredString(raw, 'id')
  const fromNode = requiredString(raw, 'fromNode')
  const toNode = requiredString(raw, 'toNode')
  if (id === undefined || fromNode === undefined || toNode === undefined) return invalid('连线 id/fromNode/toNode 必须是字符串')
  const extras: JsonObject = new Map()
  for (const [key, value] of raw) if (!EDGE_KEYS.has(key)) extras.set(key, cloneJsonValue(value))
  const preservedInvalid: JsonObject = new Map()
  const optionalPresence = new Set<string>()
  const edge: CanvasEdge = { id, fromNode, toNode, extras, preservedInvalid, optionalPresence }

  for (const key of ['fromSide', 'toSide', 'fromEnd', 'toEnd', 'color', 'label'] as const) {
    if (!raw.has(key)) continue
    optionalPresence.add(key)
    const value = raw.get(key)
    if (key === 'fromSide' || key === 'toSide') {
      if (typeof value === 'string' && SIDES.has(value as CanvasSide)) edge[key] = value as CanvasSide
      else preservedInvalid.set(key, cloneJsonValue(value!))
    } else if (key === 'fromEnd' || key === 'toEnd') {
      if (typeof value === 'string' && ENDS.has(value as CanvasEnd)) edge[key] = value as CanvasEnd
      else preservedInvalid.set(key, cloneJsonValue(value!))
    } else if (typeof value === 'string') edge[key] = value
    else preservedInvalid.set(key, cloneJsonValue(value!))
  }
  for (const key of preservedInvalid.keys()) {
    diagnostics.push({
      code: 'invalid-optional-field', severity: 'warning',
      message: `连线可选字段“${key}”无效，已原样保留`, path: appendPath(path, key), id,
    })
  }
  return edge
}

function entryId(entry: CanvasNodeEntry | CanvasEdgeEntry): string | undefined {
  if ('kind' in entry) return objectId(entry.raw)
  return entry.id
}

function countIds(entries: Array<CanvasNodeEntry | CanvasEdgeEntry>): Map<string, number> {
  const counts = new Map<string, number>()
  for (const entry of entries) {
    const id = entryId(entry)
    if (id !== undefined) counts.set(id, (counts.get(id) ?? 0) + 1)
  }
  return counts
}

/** Recomputes structural diagnostics after domain operations. */
export function analyzeCanvasDocument(document: CanvasDocument): Diagnostic[] {
  const diagnostics: Diagnostic[] = []
  document.nodes.forEach((entry, index) => {
    if (!isKnownCanvasNode(entry)) diagnostics.push({ ...entry.diagnostic, path: entry.diagnostic.path || `$.nodes[${index}]` })
    else for (const key of entry.preservedInvalid.keys()) diagnostics.push({
      code: 'invalid-optional-field', severity: 'warning', id: entry.id,
      path: `$.nodes[${index}].${key}`, message: `节点可选字段“${key}”无效，已原样保留`,
    })
  })
  document.edges.forEach((entry, index) => {
    if (!isCanvasEdge(entry)) diagnostics.push({ ...entry.diagnostic, path: entry.diagnostic.path || `$.edges[${index}]` })
    else for (const key of entry.preservedInvalid.keys()) diagnostics.push({
      code: 'invalid-optional-field', severity: 'warning', id: entry.id,
      path: `$.edges[${index}].${key}`, message: `连线可选字段“${key}”无效，已原样保留`,
    })
  })
  const nodeCounts = countIds(document.nodes)
  const edgeCounts = countIds(document.edges)
  for (const [id, count] of nodeCounts) if (count > 1) diagnostics.push({
    code: 'duplicate-node-id', severity: 'error', id, path: '$.nodes',
    message: `节点 id“${id}”重复 ${count} 次；相关节点与连线不会进入交互图`,
  })
  for (const [id, count] of edgeCounts) if (count > 1) diagnostics.push({
    code: 'duplicate-edge-id', severity: 'error', id, path: '$.edges',
    message: `连线 id“${id}”重复 ${count} 次；相关连线不会进入交互图`,
  })
  document.edges.forEach((entry, index) => {
    if (!isCanvasEdge(entry)) return
    for (const endpoint of ['fromNode', 'toNode'] as const) {
      const count = nodeCounts.get(entry[endpoint]) ?? 0
      if (count === 0) diagnostics.push({
        code: 'dangling-edge', severity: 'warning', id: entry.id, path: `$.edges[${index}].${endpoint}`,
        message: `连线端点“${entry[endpoint]}”不存在；连线已保留但不投影`,
      })
      else if (count > 1) diagnostics.push({
        code: 'ambiguous-edge', severity: 'error', id: entry.id, path: `$.edges[${index}].${endpoint}`,
        message: `连线端点“${entry[endpoint]}”对应多个节点；连线已保留但不投影`,
      })
    }
  })
  return diagnostics
}

function dedupeDiagnostics(diagnostics: Diagnostic[]): Diagnostic[] {
  const seen = new Set<string>()
  return diagnostics.filter((diagnostic) => {
    const key = `${diagnostic.code}\u0000${diagnostic.path}\u0000${diagnostic.id ?? ''}\u0000${diagnostic.message}`
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

export function decodeJsonCanvas(source: string, options: { maxDepth?: number } = {}): DecodeCanvasResult {
  let root: JsonValue
  try {
    root = new LosslessJsonParser(source, options.maxDepth ?? 64).parse()
  } catch (error) {
    return parseFailure(source, error)
  }
  if (!(root instanceof Map)) {
    return { ok: false, source, diagnostics: [{ code: 'invalid-root', severity: 'error', message: 'JSON Canvas 根必须是对象', path: '$' }] }
  }
  const nodesRaw = root.get('nodes')
  const edgesRaw = root.get('edges')
  if (nodesRaw !== undefined && !Array.isArray(nodesRaw)) {
    return { ok: false, source, diagnostics: [{ code: 'invalid-nodes', severity: 'error', message: 'nodes 必须是数组', path: '$.nodes' }] }
  }
  if (edgesRaw !== undefined && !Array.isArray(edgesRaw)) {
    return { ok: false, source, diagnostics: [{ code: 'invalid-edges', severity: 'error', message: 'edges 必须是数组', path: '$.edges' }] }
  }
  const limitDiagnostics = canvasEntryLimitDiagnostics(
    Array.isArray(nodesRaw) ? nodesRaw.length : 0,
    Array.isArray(edgesRaw) ? edgesRaw.length : 0,
  )
  if (limitDiagnostics.length > 0) return { ok: false, source, diagnostics: limitDiagnostics }
  const diagnostics: Diagnostic[] = []
  const extras: JsonObject = new Map()
  for (const [key, value] of root) {
    if (key !== 'nodes' && key !== 'edges') extras.set(key, cloneJsonValue(value))
  }
  const document: CanvasDocument = {
    nodes: (nodesRaw ?? []).map((raw, index) => decodeNode(raw, index, diagnostics)),
    edges: (edgesRaw ?? []).map((raw, index) => decodeEdge(raw, index, diagnostics)),
    extras,
    presence: { nodes: root.has('nodes'), edges: root.has('edges') },
  }
  diagnostics.push(...analyzeCanvasDocument(document))
  return { ok: true, document, diagnostics: dedupeDiagnostics(diagnostics), source }
}

function knownNodeToJson(node: KnownCanvasNode): JsonObject {
  const object: JsonObject = cloneJsonValue(node.extras)
  for (const [key, value] of node.preservedInvalid) object.set(key, cloneJsonValue(value))
  object.set('id', node.id)
  object.set('type', node.type)
  object.set('x', node.x)
  object.set('y', node.y)
  object.set('width', node.width)
  object.set('height', node.height)
  if (node.color !== undefined) object.set('color', node.color)
  if (node.type === 'text') object.set('text', node.text)
  else if (node.type === 'file') {
    object.set('file', node.file)
    if (node.subpath !== undefined) object.set('subpath', node.subpath)
  } else if (node.type === 'link') object.set('url', node.url)
  else {
    if (node.label !== undefined) object.set('label', node.label)
    if (node.background !== undefined) object.set('background', node.background)
    if (node.backgroundStyle !== undefined) object.set('backgroundStyle', node.backgroundStyle)
  }
  return object
}

function edgeToJson(edge: CanvasEdge): JsonObject {
  const object: JsonObject = cloneJsonValue(edge.extras)
  for (const [key, value] of edge.preservedInvalid) object.set(key, cloneJsonValue(value))
  object.set('id', edge.id)
  object.set('fromNode', edge.fromNode)
  object.set('toNode', edge.toNode)
  if (edge.fromSide !== undefined) object.set('fromSide', edge.fromSide)
  if (edge.fromEnd !== undefined) object.set('fromEnd', edge.fromEnd)
  if (edge.toSide !== undefined) object.set('toSide', edge.toSide)
  if (edge.toEnd !== undefined) object.set('toEnd', edge.toEnd)
  if (edge.color !== undefined) object.set('color', edge.color)
  if (edge.label !== undefined) object.set('label', edge.label)
  return object
}

function documentToJson(document: CanvasDocument): JsonObject {
  const root: JsonObject = cloneJsonValue(document.extras)
  if (document.presence.nodes || document.nodes.length > 0) {
    root.set('nodes', document.nodes.map((node) => isKnownCanvasNode(node) ? knownNodeToJson(node) : cloneJsonValue(node.raw)))
  }
  if (document.presence.edges || document.edges.length > 0) {
    root.set('edges', document.edges.map((edge) => isCanvasEdge(edge) ? edgeToJson(edge) : cloneJsonValue(edge.raw)))
  }
  return root
}

function stringifyJson(value: JsonValue, indent: number, level = 0): string {
  if (value === null || typeof value === 'boolean' || typeof value === 'string') return JSON.stringify(value)
  if (typeof value === 'number') {
    if (!Number.isFinite(value)) throw new TypeError('JSON Canvas 不能序列化非有限数字')
    return String(value)
  }
  if (isLosslessNumber(value)) {
    if (!JSON_NUMBER_TOKEN.test(value.raw)) throw new TypeError('无效的 lossless JSON number token')
    return value.raw
  }
  const pad = (n: number) => indent > 0 ? ' '.repeat(n * indent) : ''
  if (Array.isArray(value)) {
    if (value.length === 0) return '[]'
    if (indent === 0) return `[${value.map((item) => stringifyJson(item, indent, level + 1)).join(',')}]`
    return `[\n${pad(level + 1)}${value.map((item) => stringifyJson(item, indent, level + 1)).join(`,\n${pad(level + 1)}`)}\n${pad(level)}]`
  }
  const entries = Array.from(value)
  if (entries.length === 0) return '{}'
  if (indent === 0) return `{${entries.map(([key, item]) => `${JSON.stringify(key)}:${stringifyJson(item, indent, level + 1)}`).join(',')}}`
  return `{\n${pad(level + 1)}${entries.map(([key, item]) => `${JSON.stringify(key)}: ${stringifyJson(item, indent, level + 1)}`).join(`,\n${pad(level + 1)}`)}\n${pad(level)}}`
}

export function encodeJsonCanvas(document: CanvasDocument, options: { indent?: number } = {}): string {
  const indent = options.indent ?? 2
  if (!Number.isInteger(indent) || indent < 0 || indent > 10) throw new RangeError('indent 必须是 0 到 10 的整数')
  return `${stringifyJson(documentToJson(document), indent)}\n`
}

export function jsonValueToNative(value: JsonValue): unknown {
  if (value instanceof Map) return Object.fromEntries(Array.from(value, ([key, item]) => [key, jsonValueToNative(item)]))
  if (Array.isArray(value)) return value.map(jsonValueToNative)
  if (isLosslessNumber(value)) return Number(value.raw)
  return value
}
