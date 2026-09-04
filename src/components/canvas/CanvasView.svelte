<script lang="ts">
  import { onMount } from 'svelte'
  import {
    Background,
    BackgroundVariant,
    ConnectionMode,
    Controls,
    MarkerType,
    SelectionMode,
    SvelteFlow,
    type Connection,
    type Edge,
    type Node,
    type OnConnectEnd,
    type OnReconnect,
    type ResizeDragEvent,
    type ResizeParams,
    type ResizeParamsWithDirection,
    type Viewport,
  } from '@xyflow/svelte'
  import '@xyflow/svelte/dist/style.css'
  import type { Tab } from '../../lib/tabs.svelte'
  import { openFile, setContent } from '../../lib/tabs.svelte'
  import { showError } from '../../lib/dialogs'
  import { sotvaultStore } from '../../lib/sotvault.svelte'
  import { folderView } from '../../lib/folder-view.svelte'
  import { formFactor } from '../../lib/platform.svelte'
  import { dirname, isAbsolute, joinPath, normalize, relative } from '../../lib/paths'
  import {
    CanvasResourceSession,
    acquireCanvasUiSession,
    applyFlowEdgeConnection,
    alignCanvasSelection,
    buildCanvasSnapIndex,
    canvasNodesIntersectPolygon,
    cloneCanvasDocument,
    commitNodePositions,
    computeCanvasAutoPanVelocity,
    computeCanvasResizeSnap,
    computeCanvasSnap,
    copyCanvasSelection,
    createCanvasResizeSnapshot,
    decodeJsonCanvas,
    deleteCanvasSelection,
    distributeCanvasSelection,
    encodeJsonCanvas,
    fitCanvasGroupToContents,
    flowConnectionToCanvasEdge,
    flowHandleForSide,
    freezeCanvasMove,
    freezeGroupMove,
    insertCanvasEdge,
    insertCanvasNode,
    importCanvasResource,
    isCanvasEdge,
    isKnownCanvasNode,
    getCanvasNodesBounds,
    getCanvasSelectionRoots,
    moveFrozenNodes,
    pasteCanvasSelection,
    projectCanvasToFlow,
    recallCanvasClipboard,
    rememberCanvasClipboard,
    markCanvasUiSessionContent,
    reorderCanvasNodes,
    resolveCanvasResource,
    resolveCanvasResizeScale,
    resolveCanvasEdgeSides,
    resizeCanvasSelection,
    spreadCanvasSelection,
    updateCanvasNode,
    updateCanvasEdge,
    type CanvasClipboardPayload,
    type CanvasAlignDirection,
    type CanvasDistributeAxis,
    type CanvasDocument,
    type CanvasEdge,
    type CanvasEnd,
    type Diagnostic,
    type FrozenCanvasMove,
    type GroupBackgroundStyle,
    type KnownCanvasNode,
    type CanvasPoint,
    type CanvasRect,
    type CanvasResizeSnapshot,
    type ResizeCorner,
    type SnapGuide,
    type SnapIndex,
  } from '../../lib/canvas'
  import CanvasCardNode from './CanvasCardNode.svelte'
  import CanvasEdgeView from './CanvasEdge.svelte'
  import CanvasInteractionOverlay from './CanvasInteractionOverlay.svelte'
  import CanvasSelectionResizer from './CanvasSelectionResizer.svelte'
  import { loadCanvasViewport, saveCanvasViewport } from './canvas-view-state'

  let { tab }: { tab: Tab } = $props()

  type UiNode = Node<Record<string, unknown>>
  type UiEdge = Edge<Record<string, unknown>>
  type CanvasTool = 'select' | 'pan' | 'lasso'

  const nodeTypes = {
    'canvas-text': CanvasCardNode,
    'canvas-file': CanvasCardNode,
    'canvas-link': CanvasCardNode,
    'canvas-group': CanvasCardNode,
    'canvas-diagnostic': CanvasCardNode,
  }
  const edgeTypes = { 'canvas-edge': CanvasEdgeView }

  function initialTabContent(): string { return tab.currentContent }
  function initialTabPath(): string { return tab.filePath }
  function initialTabId(): string { return tab.id }
  const initialDecode = decodeJsonCanvas(initialTabContent())
  let canvasDoc = $state.raw<CanvasDocument | null>(initialDecode.ok ? initialDecode.document : null)
  let diagnostics = $state.raw<Diagnostic[]>(initialDecode.diagnostics)
  let parseFailure = $state.raw<Diagnostic[] | null>(initialDecode.ok ? null : initialDecode.diagnostics)
  let observedTabContent = initialTabContent()
  let observedTabPath = initialTabPath()
  let flowNodes = $state.raw<UiNode[]>([])
  let flowEdges = $state.raw<UiEdge[]>([])
  let selectedNodeIds = $state.raw<Set<string>>(new Set())
  let selectedEdgeIds = $state.raw<Set<string>>(new Set())
  let activeTextId: string | null = $state(null)
  let textBefore = $state.raw<CanvasDocument | null>(null)
  let composing = $state(false)
  let activeTool = $state<CanvasTool>(formFactor.value === 'desktop' ? 'select' : 'pan')
  let spacePan = $state(false)
  let interactionLocked = $state(false)
  let pendingPlacement = $state<KnownCanvasNode['type'] | null>(null)
  let lastPointerFlow = $state.raw<CanvasPoint | null>(null)
  let snapGuides = $state.raw<SnapGuide[]>([])
  let lassoPoints = $state.raw<CanvasPoint[]>([])
  let lassoSession = $state.raw<{
    pointerId: number
    start: CanvasPoint
    additive: boolean
    initialNodes: Set<string>
    initialEdges: Set<string>
    active: boolean
  } | null>(null)
  let groupDrawSession = $state.raw<{
    pointerId: number
    start: CanvasPoint
    active: boolean
  } | null>(null)
  let groupDrawRect = $state.raw<CanvasRect | null>(null)
  let connectionDraft = $state.raw<{
    sourceId: string
    sourceHandle: string | null
    at: CanvasPoint
    screen: CanvasPoint
  } | null>(null)
  let connectionMenu: HTMLDivElement | undefined = $state()
  let reconnectActive = false
  let newConnectionActive = false
  let multiResize = $state.raw<{
    pointerId: number
    snapshot: CanvasResizeSnapshot
    latestScaleX: number
    latestScaleY: number
  } | null>(null)
  let singleResize = $state.raw<{
    id: string
    snapIndex: SnapIndex
    start: CanvasRect
    latest: CanvasRect
    minimumWidth: number
    minimumHeight: number
  } | null>(null)
  let historyVersion = $state(0)
  let surface: HTMLDivElement | undefined = $state()
  let viewport = $state.raw<Viewport>({ x: 0, y: 0, zoom: 1 })
  let viewportReady = $state(false)
  let hasStoredViewport = $state(false)
  let viewportTimer: ReturnType<typeof setTimeout> | null = null
  let autoPanFrame: number | null = null
  let autoPanLastTime: number | null = null
  let autoPanPointer: CanvasPoint | null = null
  let gestureAutoPanned = false
  let suppressNextPaneClick = false
  let nodeDrag: {
    frozen: FrozenCanvasMove
    origin: { x: number; y: number }
    bounds: CanvasRect
    snapIndex: SnapIndex
    delta: CanvasPoint
    hasGroup: boolean
  } | null = null
  let pasteCount = 0
  let resourceSession: CanvasResourceSession | null = null
  let resourceSessionRoot = ''
  const requestedImages = new Set<string>()
  const uiSession = acquireCanvasUiSession(initialTabId(), initialTabContent())
  const history = uiSession.history

  let canUndo = $derived.by(() => { historyVersion; return history.canUndo })
  let canRedo = $derived.by(() => { historyVersion; return history.canRedo })
  let undoTitle = $derived.by(() => { historyVersion; return history.undoLabel ? `撤销：${history.undoLabel}` : '撤销' })
  let redoTitle = $derived.by(() => { historyVersion; return history.redoLabel ? `重做：${history.redoLabel}` : '重做' })

  let selectedEdge = $derived.by(() => {
    historyVersion
    if (!canvasDoc || selectedEdgeIds.size !== 1) return null
    const id = Array.from(selectedEdgeIds)[0]
    const edge = canvasDoc.edges.find((entry) => isCanvasEdge(entry) && entry.id === id)
    return edge && isCanvasEdge(edge) ? edge : null
  })

  let selectedKnownNode = $derived.by(() => {
    historyVersion
    if (!canvasDoc || selectedNodeIds.size !== 1) return null
    const id = Array.from(selectedNodeIds)[0]
    const node = canvasDoc.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === id)
    return node && isKnownCanvasNode(node) ? node : null
  })
  let effectiveTool = $derived(interactionLocked || spacePan ? 'pan' : activeTool)
  let selectionRoots = $derived.by(() => canvasDoc
    ? getCanvasSelectionRoots(canvasDoc, selectedNodeIds)
    : [])
  let multiSelectionBounds = $derived.by(() => selectedNodeIds.size > 1 && selectionRoots.length > 0
    ? getCanvasNodesBounds(selectionRoots)
    : null)

  function resourceRoot(): string {
    const vaultRoot = sotvaultStore.vaultRoot
    if (vaultRoot && relative(vaultRoot, tab.filePath) !== null) return normalize(vaultRoot)
    const folderRoot = folderView.rootDir
    if (folderRoot && relative(folderRoot, tab.filePath) !== null) return normalize(folderRoot)
    return dirname(tab.filePath)
  }

  function currentResourceSession(): CanvasResourceSession {
    const root = resourceRoot()
    if (!resourceSession || resourceSessionRoot !== root) {
      resourceSession?.dispose()
      resourceSession = new CanvasResourceSession(root)
      resourceSessionRoot = root
      requestedImages.clear()
    }
    return resourceSession
  }

  function resolveCanvasFile(raw: string): string | null {
    if (!raw || raw.includes('\0') || isAbsolute(raw)) return null
    const normalizedRaw = normalize(raw)
    if (normalizedRaw.split('/').some((part) => part === '..')) return null
    const root = resourceRoot()
    const resolved = joinPath(root, normalizedRaw)
    return relative(root, resolved) !== null ? resolved : null
  }

  async function resolveMarkdownResource(raw: string): Promise<string | null> {
    if (!raw || raw.startsWith('#') || isAbsolute(raw)) return null
    try {
      const asUrl = new URL(raw)
      if (asUrl.protocol) return null
    } catch { /* relative path */ }
    const pathOnly = raw.split(/[?#]/, 1)[0]
    let decoded = pathOnly
    try { decoded = decodeURIComponent(pathOnly) } catch { /* keep literal value */ }
    if (!decoded || decoded.includes('\0')) return null
    const resolved = joinPath(dirname(tab.filePath), decoded)
    const url = await currentResourceSession().loadLocalImage(resolved)
    return url || null
  }

  function imageUrlFor(raw: string | undefined): string | null {
    if (!raw || !/\.(?:avif|bmp|gif|hei[cf]|ico|jpe?g|png|webp)$/i.test(raw)) return null
    const resolved = resolveCanvasFile(raw)
    if (!resolved) return null
    const session = currentResourceSession()
    const cached = session.peek(resolved)
    if (cached) return cached
    const requestKey = `${session.root}\0${resolved}`
    if (!requestedImages.has(requestKey)) {
      requestedImages.add(requestKey)
      void session.loadLocalImage(resolved).then((url) => {
        if (url && resourceSession === session) rebuildFlow()
      })
    }
    return null
  }

  function displayColor(token?: string): string | undefined {
    const palette: Record<string, string> = {
      '1': '#e05252', '2': '#e08a32', '3': '#d4ae35',
      '4': '#4b9d62', '5': '#3d91a6', '6': '#8066bd',
    }
    if (!token) return undefined
    if (palette[token]) return palette[token]
    return /^#[0-9a-f]{3,8}$/i.test(token) && [4, 5, 7, 9].includes(token.length) ? token : undefined
  }

  function sameIds(left: ReadonlySet<string>, right: ReadonlySet<string>): boolean {
    return left.size === right.size && Array.from(left).every((id) => right.has(id))
  }

  function setTool(tool: CanvasTool): void {
    if (interactionLocked) return
    cancelLasso(true)
    cancelGroupDraw()
    connectionDraft = null
    pendingPlacement = null
    activeTool = tool
    surface?.focus()
  }

  function setPlacement(kind: KnownCanvasNode['type']): void {
    if (interactionLocked) return
    cancelLasso(true)
    cancelGroupDraw()
    connectionDraft = null
    activeTool = 'select'
    pendingPlacement = pendingPlacement === kind ? null : kind
    surface?.focus()
  }

  function toggleInteractionLock(): void {
    interactionLocked = !interactionLocked
    if (interactionLocked) {
      cancelLasso(true)
      cancelGroupDraw()
      cancelSingleResize()
      cancelMultiResize()
      pendingPlacement = null
      connectionDraft = null
      spacePan = false
      finalizeTextSession()
    }
    rebuildFlow()
  }

  function arrangeSelection(direction: CanvasAlignDirection): void {
    if (!canvasDoc || selectionRoots.length < 2 || !finishTextBeforeStructure()) return
    const changes = alignCanvasSelection(canvasDoc, selectedNodeIds, direction)
    commitDocument(`对齐选中节点：${direction}`, commitNodePositions(canvasDoc, changes))
  }

  function distributeSelection(axis: CanvasDistributeAxis): void {
    if (!canvasDoc || selectionRoots.length < 3 || !finishTextBeforeStructure()) return
    const changes = distributeCanvasSelection(canvasDoc, selectedNodeIds, axis)
    commitDocument(`分布选中节点：${axis}`, commitNodePositions(canvasDoc, changes))
  }

  function spreadSelection(): void {
    if (!canvasDoc || selectionRoots.length < 2 || !finishTextBeforeStructure()) return
    const changes = spreadCanvasSelection(canvasDoc, selectedNodeIds)
    commitDocument('散开重叠节点', commitNodePositions(canvasDoc, changes))
  }

  function rebuildFlow(nextSelectedNodes = selectedNodeIds, nextSelectedEdges = selectedEdgeIds): void {
    if (!canvasDoc) {
      flowNodes = []
      flowEdges = []
      return
    }
    const projection = projectCanvasToFlow(canvasDoc)
    diagnostics = projection.diagnostics
    const validNodes = new Set(Array.from(nextSelectedNodes).filter((id) => projection.nodes.some((node) => node.id === id)))
    const validEdges = new Set(Array.from(nextSelectedEdges).filter((id) => projection.edges.some((edge) => edge.id === id)))
    if (!sameIds(selectedNodeIds, validNodes)) selectedNodeIds = validNodes
    if (!sameIds(selectedEdgeIds, validEdges)) selectedEdgeIds = validEdges
    flowNodes = projection.nodes.map((node) => {
      const kind = node.data.kind === 'diagnostic' ? 'opaque' : node.data.kind
      return {
        ...node,
        selected: selectedNodeIds.has(node.id),
        class: kind === 'group' ? 'canvas-group-shell' : undefined,
        dragHandle: kind === 'group' ? '.group-label' : undefined,
        data: {
          ...node.data,
          kind,
          diagnostic: node.data.diagnostic?.message,
          active: activeTextId === node.data.canonicalId,
          multipleSelected: selectedNodeIds.size > 1,
          interactionLocked,
          tabId: tab.id,
          canvasPath: tab.filePath,
          imageUrl: node.data.kind === 'file' ? imageUrlFor(node.data.file) : null,
          backgroundUrl: node.data.kind === 'group' ? imageUrlFor(node.data.background) : null,
          mediaResolver: currentResourceSession(),
          resolveLocalResource: resolveMarkdownResource,
          onActivate: activateTextNode,
          onOpen: openNode,
          onTextChange: updateTextDraft,
          onTextFlush: flushTextDraft,
          onTextBlur: finalizeTextSession,
          onCompositionChange: (value: boolean) => { composing = value },
          onResizeStart: startSingleResize,
          onResize: previewSingleResize,
          onResizeEnd: commitResize,
        },
      } as UiNode
    })
    const nodeRects = new Map(canvasDoc.nodes.flatMap((entry) =>
      isKnownCanvasNode(entry)
        ? [[entry.id, { id: entry.id, x: entry.x, y: entry.y, width: entry.width, height: entry.height }] as const]
        : [],
    ))
    const edgeObstacles = canvasDoc.nodes.flatMap((entry) =>
      isKnownCanvasNode(entry) && entry.type !== 'group'
        ? [{ id: entry.id, x: entry.x, y: entry.y, width: entry.width, height: entry.height }]
        : [],
    )
    const canonicalEdges = new Map(canvasDoc.edges.flatMap((entry) =>
      isCanvasEdge(entry) ? [[entry.id, entry] as const] : [],
    ))
    flowEdges = projection.edges.map((edge) => {
      const color = displayColor(edge.data.colorToken)
      const canonical = canonicalEdges.get(edge.id)
      const sourceRect = nodeRects.get(edge.source)
      const targetRect = nodeRects.get(edge.target)
      const smartSides = sourceRect && targetRect
        ? resolveCanvasEdgeSides(sourceRect, targetRect, canonical?.fromSide, canonical?.toSide, edgeObstacles)
        : null
      return {
        ...edge,
        sourceHandle: edge.sourceHandle ?? flowHandleForSide(smartSides?.fromSide),
        targetHandle: edge.targetHandle ?? flowHandleForSide(smartSides?.toSide),
        type: 'canvas-edge',
        selected: selectedEdgeIds.has(edge.id),
        markerStart: edge.markerStart ? { type: MarkerType.ArrowClosed } : undefined,
        markerEnd: edge.markerEnd ? { type: MarkerType.ArrowClosed } : undefined,
        style: color ? `stroke:${color};stroke-width:2` : 'stroke-width:2',
        labelStyle: 'fill:CanvasText;font-size:12px',
        data: {
          ...edge.data,
          interactionLocked,
          onLabelCommit: updateEdgeLabelById,
        },
      } as UiEdge
    })
  }

  function syncTab(): void {
    if (!canvasDoc) return
    const encoded = encodeJsonCanvas(canvasDoc)
    observedTabContent = encoded
    markCanvasUiSessionContent(tab.id, encoded)
    setContent(tab.id, encoded)
  }

  function commitDocument(label: string, next: CanvasDocument, nextNodes = selectedNodeIds, nextEdges = selectedEdgeIds): void {
    if (!canvasDoc || next === canvasDoc) return
    history.record(label, canvasDoc, next)
    canvasDoc = next
    historyVersion++
    syncTab()
    rebuildFlow(nextNodes, nextEdges)
  }

  function newId(): string {
    return crypto.randomUUID()
  }

  function viewportCenter(): { x: number; y: number } {
    const rect = surface?.getBoundingClientRect()
    return {
      x: Math.round(((rect?.width ?? 800) / 2 - viewport.x) / viewport.zoom),
      y: Math.round(((rect?.height ?? 600) / 2 - viewport.y) / viewport.zoom),
    }
  }

  function createNode(kind: KnownCanvasNode['type'], at: CanvasPoint, value?: string, id = newId()): KnownCanvasNode {
    const common = {
      id,
      x: Math.round(at.x - (kind === 'group' ? 190 : 140)),
      y: Math.round(at.y - (kind === 'group' ? 120 : 90)),
      width: kind === 'group' ? 380 : 280,
      height: kind === 'group' ? 240 : 180,
      extras: new Map(),
      preservedInvalid: new Map(),
      optionalPresence: new Set<string>(),
    }
    return kind === 'text'
      ? { ...common, type: 'text', text: value ?? '# 新卡片\n\n双击开始编辑' }
      : kind === 'file'
        ? { ...common, type: 'file', file: value ?? '' }
        : kind === 'link'
          ? { ...common, type: 'link', url: value ?? 'https://' }
          : { ...common, type: 'group', label: value || '分组' }
  }

  function addNode(kind: KnownCanvasNode['type'], at = lastPointerFlow ?? viewportCenter(), value?: string): void {
    if (!canvasDoc || !finishTextBeforeStructure()) return
    const node = createNode(kind, at, value)
    const index = kind === 'group' ? 0 : canvasDoc.nodes.length
    const next = insertCanvasNode(canvasDoc, node, index)
    commitDocument(`创建${kind === 'text' ? '文本' : kind === 'file' ? '文件' : kind === 'link' ? '链接' : '分组'}节点`, next, new Set([node.id]), new Set())
    if (kind === 'text') queueMicrotask(() => activateTextNode(node.id))
  }

  async function chooseFileNode(at = lastPointerFlow ?? viewportCenter()): Promise<void> {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const picked = await open({ multiple: false })
      if (typeof picked !== 'string') return
      await addFilePath(picked, at)
    } catch (error) {
      showError(`无法选择文件：${String(error)}`)
    }
  }

  async function addFilePath(path: string, at: { x: number; y: number }): Promise<void> {
    const root = resourceRoot()
    const imported = await importCanvasResource(root, tab.filePath, path)
    addNode('file', at, imported.relativePath)
  }

  async function relinkSelectedResource(): Promise<void> {
    if (!canvasDoc || !selectedKnownNode || (selectedKnownNode.type !== 'file' && selectedKnownNode.type !== 'group')) return
    const selected = selectedKnownNode
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const picked = await open({
        multiple: false,
        ...(selected.type === 'group' ? {
          filters: [{ name: '图片', extensions: ['avif', 'bmp', 'gif', 'heic', 'heif', 'ico', 'jpg', 'jpeg', 'png', 'webp'] }],
        } : {}),
      })
      if (typeof picked !== 'string' || !finishTextBeforeStructure()) return
      const imported = await importCanvasResource(resourceRoot(), tab.filePath, picked)
      const id = selected.id
      const next = updateCanvasNode(canvasDoc, id, (node) => {
        if (node.type === 'file') return { ...node, file: imported.relativePath }
        if (node.type === 'group') {
          const copy = { ...node, background: imported.relativePath }
          copy.preservedInvalid.delete('background')
          copy.optionalPresence.add('background')
          return copy
        }
        return node
      })
      commitDocument(selected.type === 'file' ? '重新链接文件' : '设置分组背景', next)
    } catch (error) {
      showError(`无法导入资源：${String(error)}`)
    }
  }

  function addLinkNode(at = lastPointerFlow ?? viewportCenter()): void {
    const value = window.prompt('输入 http 或 https 链接')?.trim()
    if (!value) return
    try {
      const url = new URL(value)
      if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error('unsupported protocol')
      addNode('link', at, url.href)
    } catch {
      showError('只支持 http:// 或 https:// 链接。')
    }
  }

  function addGroupNode(): void {
    const label = window.prompt('分组名称', '分组')
    if (label === null) return
    if (!canvasDoc || !finishTextBeforeStructure()) return
    const selected = canvasDoc.nodes.filter((entry) =>
      isKnownCanvasNode(entry) && selectedNodeIds.has(entry.id),
    ).filter(isKnownCanvasNode)
    if (selected.length === 0) {
      addNode('group', viewportCenter(), label.trim() || '分组')
      return
    }

    const id = newId()
    const minX = Math.min(...selected.map((node) => node.x))
    const minY = Math.min(...selected.map((node) => node.y))
    const maxX = Math.max(...selected.map((node) => node.x + node.width))
    const maxY = Math.max(...selected.map((node) => node.y + node.height))
    const group: KnownCanvasNode = {
      id,
      type: 'group',
      x: minX - 36,
      y: minY - 52,
      width: maxX - minX + 72,
      height: maxY - minY + 88,
      label: label.trim() || '分组',
      extras: new Map(),
      preservedInvalid: new Map(),
      optionalPresence: new Set(),
    }
    const selectedIndexes = canvasDoc.nodes.flatMap((entry, index) =>
      isKnownCanvasNode(entry) && selectedNodeIds.has(entry.id) ? [index] : [],
    )
    const insertAt = selectedIndexes.length > 0 ? Math.min(...selectedIndexes) : 0
    commitDocument('围绕选区创建分组', insertCanvasNode(canvasDoc, group, insertAt), new Set([id]), new Set())
  }

  function placePendingNode(at: CanvasPoint): void {
    const kind = pendingPlacement
    if (!kind || interactionLocked) return
    pendingPlacement = null
    if (kind === 'file') void chooseFileNode(at)
    else if (kind === 'link') addLinkNode(at)
    else addNode(kind, at)
  }

  function activateTextNode(id: string): void {
    if (!canvasDoc || composing || activeTextId === id) return
    finalizeTextSession()
    const node = canvasDoc.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === id)
    if (!node || !isKnownCanvasNode(node) || node.type !== 'text') return
    textBefore = cloneCanvasDocument(canvasDoc)
    activeTextId = id
    rebuildFlow(new Set([id]), new Set())
  }

  function updateTextDraft(id: string, markdown: string): void {
    if (!canvasDoc || activeTextId !== id) return
    const next = updateCanvasNode(canvasDoc, id, (node) => node.type === 'text' ? { ...node, text: markdown } : node)
    if (next === canvasDoc) return
    canvasDoc = next
    syncTab()
  }

  function flushTextDraft(id: string, markdown: string): void {
    updateTextDraft(id, markdown)
    if (!canvasDoc || composing || activeTextId !== id || !textBefore) return
    history.record('编辑文本节点', textBefore, canvasDoc)
    textBefore = cloneCanvasDocument(canvasDoc)
    historyVersion++
  }

  function finalizeTextSession(id = activeTextId ?? '', markdown?: string): void {
    if (!canvasDoc || !activeTextId || (id && id !== activeTextId) || composing) return
    if (markdown !== undefined) updateTextDraft(activeTextId, markdown)
    if (textBefore) history.record('编辑文本节点', textBefore, canvasDoc)
    textBefore = null
    activeTextId = null
    historyVersion++
    syncTab()
    rebuildFlow()
  }

  function finishTextBeforeStructure(): boolean {
    if (composing) return false
    finalizeTextSession()
    return true
  }

  async function openNode(id: string): Promise<void> {
    if (!canvasDoc) return
    const node = canvasDoc.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === id)
    if (!node || !isKnownCanvasNode(node)) return
    if (node.type === 'file') {
      const path = resolveCanvasFile(node.file)
      if (!path) { showError(`无法访问画布文件引用：${node.file}`); return }
      try {
        const canonicalPath = await resolveCanvasResource(resourceRoot(), path)
        await openFile(canonicalPath)
      } catch (error) {
        showError(`无法访问画布文件引用：${String(error)}`)
      }
    } else if (node.type === 'link') {
      try {
        const url = new URL(node.url)
        if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error('unsupported protocol')
        void import('@tauri-apps/plugin-opener').then(({ openUrl }) => openUrl(url.href)).catch((error) => showError(String(error)))
      } catch {
        showError('该链接协议不允许打开；地址仍会原样保留。')
      }
    }
  }

  function startSingleResize(id: string, rectangle: ResizeParams): void {
    if (!canvasDoc || interactionLocked) return
    const node = canvasDoc.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === id)
    if (!node || !isKnownCanvasNode(node)) return
    singleResize = {
      id,
      snapIndex: buildCanvasSnapIndex(canvasDoc, [id]),
      start: rectangle,
      latest: rectangle,
      minimumWidth: node.type === 'group' ? 180 : 160,
      minimumHeight: node.type === 'group' ? 120 : 100,
    }
    snapGuides = []
  }

  function previewSingleResize(
    id: string,
    event: ResizeDragEvent,
    rectangle: ResizeParamsWithDirection,
  ): void {
    const session = singleResize
    if (!session || session.id !== id) return
    const changesX = Math.abs(rectangle.x - session.start.x) > 0.01
    const changesY = Math.abs(rectangle.y - session.start.y) > 0.01
    const changesWidth = Math.abs(rectangle.width - session.start.width) > 0.01
    const changesHeight = Math.abs(rectangle.height - session.start.height) > 0.01
    const result = computeCanvasResizeSnap(rectangle, session.snapIndex, {
      thresholdFlow: 6 / Math.max(viewport.zoom, 0.05),
      bypass: Boolean((event.sourceEvent as { altKey?: boolean } | undefined)?.altKey),
      activeEdges: {
        x: changesX ? 'min' : changesWidth ? 'max' : 'none',
        y: changesY ? 'min' : changesHeight ? 'max' : 'none',
      },
      minimumWidth: session.minimumWidth,
      minimumHeight: session.minimumHeight,
    })
    session.latest = result.rectangle
    snapGuides = result.guides
    queueMicrotask(() => {
      if (singleResize !== session) return
      flowNodes = flowNodes.map((node) => node.id === id ? {
        ...node,
        position: { x: result.rectangle.x, y: result.rectangle.y },
        width: result.rectangle.width,
        height: result.rectangle.height,
        measured: { width: result.rectangle.width, height: result.rectangle.height },
      } : node)
    })
  }

  function commitResize(id: string, rectangle: ResizeParams): void {
    if (!canvasDoc) return
    const current = canvasDoc.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === id)
    if (!current || !isKnownCanvasNode(current)) return
    const finalRectangle = singleResize?.id === id ? singleResize.latest : rectangle
    singleResize = null
    snapGuides = []
    const next = updateCanvasNode(canvasDoc, id, (node) => ({
      ...node,
      x: Math.round(finalRectangle.x),
      y: Math.round(finalRectangle.y),
      width: Math.max(node.type === 'group' ? 180 : 160, Math.round(finalRectangle.width)),
      height: Math.max(node.type === 'group' ? 120 : 100, Math.round(finalRectangle.height)),
    }))
    commitDocument('调整节点大小', next)
  }

  function cancelSingleResize(): void {
    if (!singleResize) return
    singleResize = null
    snapGuides = []
    rebuildFlow()
  }

  function handleNodeDragStart({ targetNode, nodes }: { targetNode: UiNode | null; nodes: UiNode[] }): void {
    if (!canvasDoc || !targetNode) return
    const target = canvasDoc.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === targetNode.id)
    if (!target || !isKnownCanvasNode(target)) { nodeDrag = null; return }
    const draggedIds = nodes.map((node) => (node.data.canonicalId as string | undefined) ?? node.id)
    const dragged = canvasDoc.nodes.filter((entry) => isKnownCanvasNode(entry) && draggedIds.includes(entry.id))
    const frozen = freezeCanvasMove(canvasDoc, draggedIds)
    const movingNodes = canvasDoc.nodes.filter((entry) =>
      isKnownCanvasNode(entry) && frozen.nodeIds.includes(entry.id),
    )
    const bounds = getCanvasNodesBounds(movingNodes)
    if (!bounds) { nodeDrag = null; return }
    nodeDrag = {
      frozen,
      origin: { x: target.x, y: target.y },
      bounds,
      snapIndex: buildCanvasSnapIndex(canvasDoc, frozen.nodeIds),
      delta: { x: 0, y: 0 },
      hasGroup: dragged.some((node) => isKnownCanvasNode(node) && node.type === 'group'),
    }
    snapGuides = []
  }

  function dragDelta(targetNode: UiNode, event: MouseEvent | TouchEvent): CanvasPoint {
    if (!nodeDrag) return { x: 0, y: 0 }
    const raw = {
      x: targetNode.position.x - nodeDrag.origin.x,
      y: targetNode.position.y - nodeDrag.origin.y,
    }
    const snapped = computeCanvasSnap({
      x: nodeDrag.bounds.x + raw.x,
      y: nodeDrag.bounds.y + raw.y,
      width: nodeDrag.bounds.width,
      height: nodeDrag.bounds.height,
    }, nodeDrag.snapIndex, {
      thresholdFlow: 6 / Math.max(viewport.zoom, 0.05),
      bypass: event instanceof MouseEvent && event.altKey,
    })
    snapGuides = snapped.guides
    return { x: raw.x + snapped.deltaX, y: raw.y + snapped.deltaY }
  }

  function handleNodeDrag({ targetNode, event }: {
    targetNode: UiNode | null
    event: MouseEvent | TouchEvent
  }): void {
    if (!canvasDoc || !targetNode || !nodeDrag) return
    const delta = dragDelta(targetNode, event)
    nodeDrag.delta = delta
    const moving = new Set(nodeDrag.frozen.nodeIds)
    flowNodes = flowNodes.map((flowNode) => {
      if (!moving.has(flowNode.id)) return flowNode
      const canonical = canvasDoc?.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === flowNode.id)
      if (!canonical || !isKnownCanvasNode(canonical)) return flowNode
      return { ...flowNode, position: { x: canonical.x + delta.x, y: canonical.y + delta.y } }
    })
  }

  function handleNodeDragStop({ targetNode, nodes, event }: {
    targetNode: UiNode | null
    nodes: UiNode[]
    event: MouseEvent | TouchEvent
  }): void {
    if (!canvasDoc) return
    if (!finishTextBeforeStructure()) { nodeDrag = null; snapGuides = []; rebuildFlow(); return }
    if (targetNode && nodeDrag) {
      const drag = nodeDrag
      const delta = dragDelta(targetNode, event)
      const next = moveFrozenNodes(canvasDoc, drag.frozen, delta)
      nodeDrag = null
      snapGuides = []
      const label = drag.hasGroup
        ? (nodes.length > 1 ? '移动分组与选区' : '移动分组')
        : (nodes.length > 1 ? '移动多个节点' : '移动节点')
      commitDocument(label, next)
      return
    }
    nodeDrag = null
    snapGuides = []
    let next = canvasDoc
    for (const node of nodes) {
      next = updateCanvasNode(next, node.id, (current) => ({
        ...current,
        x: Math.round(node.position.x),
        y: Math.round(node.position.y),
      }))
    }
    commitDocument(nodes.length > 1 ? '移动多个节点' : '移动节点', next)
  }

  function handleConnect(connection: Connection): void {
    if (!canvasDoc || !connection.source || !connection.target || connection.source === connection.target || !finishTextBeforeStructure()) return
    const edge = flowConnectionToCanvasEdge(newId(), connection)
    commitDocument('创建连线', insertCanvasEdge(canvasDoc, edge), selectedNodeIds, new Set([edge.id]))
  }

  const handleReconnect: OnReconnect<UiEdge> = (oldEdge, connection) => {
    if (!canvasDoc || connection.source === connection.target || !finishTextBeforeStructure()) return
    const edgeId = (oldEdge.data?.canonicalId as string | undefined) ?? oldEdge.id
    commitDocument('重连连线', applyFlowEdgeConnection(canvasDoc, edgeId, connection), selectedNodeIds, new Set([edgeId]))
  }

  function addConnectedNode(
    sourceId: string,
    sourceHandle: string | null,
    kind: KnownCanvasNode['type'],
    at: CanvasPoint,
    value?: string,
  ): void {
    if (!canvasDoc || !finishTextBeforeStructure()) return
    const node = createNode(kind, at, value)
    const nodeIndex = kind === 'group' ? 0 : canvasDoc.nodes.length
    let next = insertCanvasNode(canvasDoc, node, nodeIndex)
    const connection: Connection = {
      source: sourceId,
      target: node.id,
      sourceHandle,
      targetHandle: null,
    }
    const edge = flowConnectionToCanvasEdge(newId(), connection)
    next = insertCanvasEdge(next, edge)
    commitDocument('创建节点并连接', next, new Set([node.id]), new Set())
    if (kind === 'text') queueMicrotask(() => activateTextNode(node.id))
  }

  async function chooseConnectedNode(kind: KnownCanvasNode['type']): Promise<void> {
    const draft = connectionDraft
    connectionDraft = null
    if (!draft) return
    if (kind === 'file') {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog')
        const picked = await open({ multiple: false })
        if (typeof picked !== 'string') return
        const imported = await importCanvasResource(resourceRoot(), tab.filePath, picked)
        addConnectedNode(draft.sourceId, draft.sourceHandle, kind, draft.at, imported.relativePath)
      } catch (error) {
        showError(`无法选择文件：${String(error)}`)
      }
      return
    }
    if (kind === 'link') {
      const value = window.prompt('输入 http 或 https 链接')?.trim()
      if (!value) return
      try {
        const url = new URL(value)
        if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error('unsupported protocol')
        addConnectedNode(draft.sourceId, draft.sourceHandle, kind, draft.at, url.href)
      } catch {
        showError('只支持 http:// 或 https:// 链接。')
      }
      return
    }
    addConnectedNode(draft.sourceId, draft.sourceHandle, kind, draft.at)
  }

  function eventClientPoint(event: MouseEvent | TouchEvent): CanvasPoint | null {
    if (event instanceof MouseEvent) return { x: event.clientX, y: event.clientY }
    const touch = event.changedTouches[0] ?? event.touches[0]
    return touch ? { x: touch.clientX, y: touch.clientY } : null
  }

  function handleConnectStart(): void {
    newConnectionActive = !reconnectActive
    connectionDraft = null
  }

  const handleConnectEnd: OnConnectEnd = (event, state) => {
    const isNewConnection = newConnectionActive && !reconnectActive
    newConnectionActive = false
    if (!isNewConnection || interactionLocked || state.isValid || !state.fromNode || !state.fromHandle) return
    const client = eventClientPoint(event)
    if (!client) return
    const targetElement = document.elementFromPoint(client.x, client.y)?.closest('.svelte-flow__node')
    const targetId = targetElement?.getAttribute('data-id') ?? null
    const sourceId = state.fromNode.id
    if (targetId === sourceId) return
    if (targetId) {
      handleConnect({
        source: sourceId,
        target: targetId,
        sourceHandle: state.fromHandle.id ?? null,
        targetHandle: null,
      })
      return
    }
    const screen = localPointer({ clientX: client.x, clientY: client.y })
    connectionDraft = {
      sourceId,
      sourceHandle: state.fromHandle.id ?? null,
      at: localToFlow(screen),
      screen,
    }
    queueMicrotask(() => connectionMenu?.querySelector<HTMLButtonElement>('button')?.focus())
  }

  function handleConnectionMenuKeydown(event: KeyboardEvent): void {
    if (!connectionDraft) return
    if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      connectionDraft = null
      queueMicrotask(() => surface?.focus())
      return
    }
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
    const buttons = Array.from(connectionMenu?.querySelectorAll<HTMLButtonElement>('button') ?? [])
    if (buttons.length === 0) return
    event.preventDefault()
    event.stopPropagation()
    const current = Math.max(0, buttons.indexOf(document.activeElement as HTMLButtonElement))
    const index = event.key === 'Home' ? 0
      : event.key === 'End' ? buttons.length - 1
        : event.key === 'ArrowLeft' ? (current - 1 + buttons.length) % buttons.length
          : (current + 1) % buttons.length
    buttons[index]?.focus()
  }

  function handleDelete({ nodes, edges }: { nodes: UiNode[]; edges: UiEdge[] }): void {
    if (!canvasDoc || !finishTextBeforeStructure()) return
    const nodeIds = new Set(nodes.map((node) => (node.data.canonicalId as string | undefined) ?? node.id))
    const edgeIds = new Set(edges.map((edge) => (edge.data?.canonicalId as string | undefined) ?? edge.id))
    commitDocument('删除画布元素', deleteCanvasSelection(canvasDoc, nodeIds, edgeIds), new Set(), new Set())
  }

  function deleteSelection(): void {
    handleDelete({
      nodes: flowNodes.filter((node) => selectedNodeIds.has(node.id)),
      edges: flowEdges.filter((edge) => selectedEdgeIds.has(edge.id)),
    })
  }

  function selectAll(): void {
    selectedNodeIds = new Set(flowNodes.filter((node) => node.selectable !== false).map((node) => node.id))
    selectedEdgeIds = new Set(flowEdges.filter((edge) => edge.selectable !== false).map((edge) => edge.id))
    rebuildFlow()
  }

  function handleSelection({ nodes, edges }: { nodes: UiNode[]; edges: UiEdge[] }): void {
    const nextNodes = new Set(nodes.map((node) => node.id))
    const nextEdges = new Set(edges.map((edge) => edge.id))
    const nodesChanged = !sameIds(selectedNodeIds, nextNodes)
    const edgesChanged = !sameIds(selectedEdgeIds, nextEdges)
    if (!nodesChanged && !edgesChanged) return
    if (nodesChanged) selectedNodeIds = nextNodes
    if (edgesChanged) selectedEdgeIds = nextEdges
    flowNodes = flowNodes.map((node) => ({
      ...node,
      selected: nextNodes.has(node.id),
      data: { ...node.data, multipleSelected: nextNodes.size > 1 },
    }))
    flowEdges = flowEdges.map((edge) => ({ ...edge, selected: nextEdges.has(edge.id) }))
  }

  async function copySelection(): Promise<boolean> {
    if (!canvasDoc || selectedNodeIds.size === 0) return false
    const payload = { ...copyCanvasSelection(canvasDoc, selectedNodeIds), sourceRoot: resourceRoot() }
    if (payload.nodes.length === 0) return false
    const text = encodeJsonCanvas({
      nodes: payload.nodes,
      edges: payload.edges,
      extras: new Map(),
      presence: { nodes: true, edges: true },
    })
    rememberCanvasClipboard(payload, text)
    let wroteSystemClipboard = false
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text)
        wroteSystemClipboard = true
      }
    } catch { /* try the Tauri clipboard below */ }
    if (!wroteSystemClipboard) {
      try {
        const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
        await writeText(text)
      } catch { /* in-process clipboard remains valid */ }
    }
    return true
  }

  async function cutSelection(): Promise<void> {
    if (await copySelection()) deleteSelection()
  }

  async function pasteFromClipboard(): Promise<void> {
    let text = ''
    try {
      text = await navigator.clipboard?.readText?.() ?? ''
    } catch { /* try the Tauri clipboard below */ }
    if (!text.trim()) {
      try {
        const { readText } = await import('@tauri-apps/plugin-clipboard-manager')
        text = await readText()
      } catch { /* fall back to the in-process Canvas clipboard */ }
    }
    if (text.trim()) {
      const remembered = recallCanvasClipboard(text)
      if (remembered) pastePayload(remembered)
      else pasteText(text)
      return
    }
    const remembered = recallCanvasClipboard()
    if (remembered) pastePayload(remembered)
  }

  function pastePayload(payload: CanvasClipboardPayload): void {
    if (!canvasDoc || payload.nodes.length === 0 || !finishTextBeforeStructure()) return
    if (payload.sourceRoot && payload.sourceRoot !== resourceRoot()) {
      showError('跨工作区粘贴会保留原始文件引用；无法在当前工作区解析的资源将显示为断链。')
    }
    const known = payload.nodes.filter(isKnownCanvasNode)
    const minX = known.length ? Math.min(...known.map((node) => node.x)) : 0
    const minY = known.length ? Math.min(...known.map((node) => node.y)) : 0
    const center = lastPointerFlow ?? viewportCenter()
    pasteCount = (pasteCount % 8) + 1
    const result = pasteCanvasSelection(canvasDoc, payload, {
      offset: { x: center.x - minX + pasteCount * 16, y: center.y - minY + pasteCount * 16 },
    })
    commitDocument('粘贴画布元素', result.document, new Set(result.insertedNodeIds), new Set(result.insertedEdgeIds))
  }

  function pasteText(text: string): void {
    const trimmed = text.trim()
    if (!trimmed) return
    const decoded = decodeJsonCanvas(trimmed)
    if (decoded.ok && decoded.document.nodes.length > 0) {
      pastePayload({ version: 1, nodes: decoded.document.nodes, edges: decoded.document.edges })
      return
    }
    try {
      const url = new URL(trimmed)
      if ((url.protocol === 'http:' || url.protocol === 'https:') && !trimmed.includes('\n')) {
        addNode('link', lastPointerFlow ?? viewportCenter(), url.href)
        return
      }
    } catch { /* ordinary text */ }
    addNode('text', lastPointerFlow ?? viewportCenter(), text)
  }

  function handlePaste(event: ClipboardEvent): void {
    if (isTextInput(event.target)) return
    event.preventDefault()
    event.stopPropagation()
    const text = event.clipboardData?.getData('text/plain') ?? ''
    // Prefer the current system clipboard. The in-process payload is only a
    // fallback for WebViews where navigator.clipboard.writeText was denied;
    // otherwise an old canvas copy would wrongly shadow text copied later in
    // another application.
    if (text.trim()) {
      const remembered = recallCanvasClipboard(text)
      if (remembered) pastePayload(remembered)
      else pasteText(text)
    } else {
      const remembered = recallCanvasClipboard()
      if (remembered) pastePayload(remembered)
    }
  }

  function undo(): void {
    if (composing) return
    finalizeTextSession()
    const previous = history.undo()
    if (!previous) return
    canvasDoc = previous
    historyVersion++
    syncTab()
    rebuildFlow()
  }

  function redo(): void {
    if (composing) return
    finalizeTextSession()
    const next = history.redo()
    if (!next) return
    canvasDoc = next
    historyVersion++
    syncTab()
    rebuildFlow()
  }

  function isTextInput(target: EventTarget | null): boolean {
    return target instanceof HTMLElement
      && !!target.closest('input,textarea,[contenteditable="true"],.embedded-markdown,.ProseMirror')
  }

  function localPointer(event: { clientX: number; clientY: number }): CanvasPoint {
    const rect = surface?.getBoundingClientRect()
    return {
      x: event.clientX - (rect?.left ?? 0),
      y: event.clientY - (rect?.top ?? 0),
    }
  }

  function localToFlow(point: CanvasPoint): CanvasPoint {
    return {
      x: (point.x - viewport.x) / viewport.zoom,
      y: (point.y - viewport.y) / viewport.zoom,
    }
  }

  function shouldRememberPointer(target: EventTarget | null): boolean {
    return !(target instanceof Element)
      || !target.closest('.canvas-toolbar,.svelte-flow__controls,.selection-resizer,.connection-create-menu,.zoom-indicator')
  }

  function edgeIdsInside(nodeIds: ReadonlySet<string>): Set<string> {
    if (!canvasDoc) return new Set()
    return new Set(canvasDoc.edges.flatMap((entry) =>
      isCanvasEdge(entry) && nodeIds.has(entry.fromNode) && nodeIds.has(entry.toNode)
        ? [entry.id]
        : [],
    ))
  }

  function previewLasso(points: CanvasPoint[], session = lassoSession): void {
    if (!canvasDoc || !session || points.length < 3) return
    const polygon = points.map(localToFlow)
    const hitIds = new Set(canvasNodesIntersectPolygon(canvasDoc.nodes, polygon).map((node) => node.id))
    const nextNodes = session.additive
      ? new Set([...session.initialNodes, ...hitIds])
      : hitIds
    const insideEdges = edgeIdsInside(nextNodes)
    const nextEdges = session.additive
      ? new Set([...session.initialEdges, ...insideEdges])
      : insideEdges
    selectedNodeIds = nextNodes
    selectedEdgeIds = nextEdges
    flowNodes = flowNodes.map((node) => ({
      ...node,
      selected: nextNodes.has(node.id),
      data: { ...node.data, multipleSelected: nextNodes.size > 1 },
    }))
    flowEdges = flowEdges.map((edge) => ({ ...edge, selected: nextEdges.has(edge.id) }))
  }

  function cancelLasso(restore: boolean): void {
    const session = lassoSession
    stopGestureAutoPan(true)
    if (session) {
      try { surface?.releasePointerCapture(session.pointerId) } catch { /* already released */ }
    }
    lassoSession = null
    lassoPoints = []
    if (!restore || !session) return
    selectedNodeIds = new Set(session.initialNodes)
    selectedEdgeIds = new Set(session.initialEdges)
    rebuildFlow()
  }

  function rectangleBetween(start: CanvasPoint, end: CanvasPoint): CanvasRect {
    return {
      x: Math.min(start.x, end.x),
      y: Math.min(start.y, end.y),
      width: Math.abs(end.x - start.x),
      height: Math.abs(end.y - start.y),
    }
  }

  function stopGestureAutoPan(persist: boolean): void {
    if (autoPanFrame !== null) cancelAnimationFrame(autoPanFrame)
    autoPanFrame = null
    autoPanLastTime = null
    autoPanPointer = null
    if (persist && gestureAutoPanned) handleMoveEnd(null, viewport)
    gestureAutoPanned = false
  }

  function autoPanTick(time: number): void {
    autoPanFrame = null
    const point = autoPanPointer
    const rect = surface?.getBoundingClientRect()
    if (!point || !rect || (!lassoSession && !groupDrawSession)) {
      stopGestureAutoPan(false)
      return
    }
    const velocity = computeCanvasAutoPanVelocity(point, { width: rect.width, height: rect.height })
    if (velocity.x === 0 && velocity.y === 0) {
      autoPanLastTime = null
      return
    }
    const elapsed = autoPanLastTime === null ? 1 / 60 : Math.min(0.05, Math.max(0, (time - autoPanLastTime) / 1000))
    autoPanLastTime = time
    const dx = velocity.x * elapsed
    const dy = velocity.y * elapsed
    viewport = { ...viewport, x: viewport.x + dx, y: viewport.y + dy }
    gestureAutoPanned = true
    lastPointerFlow = localToFlow(point)

    if (lassoSession) {
      lassoSession = {
        ...lassoSession,
        start: { x: lassoSession.start.x + dx, y: lassoSession.start.y + dy },
      }
      lassoPoints = appendLassoPoint(
        lassoPoints.map((entry) => ({ x: entry.x + dx, y: entry.y + dy })),
        point,
      )
      previewLasso(lassoPoints, lassoSession)
    } else if (groupDrawSession) {
      groupDrawSession = {
        ...groupDrawSession,
        start: { x: groupDrawSession.start.x + dx, y: groupDrawSession.start.y + dy },
      }
      groupDrawRect = rectangleBetween(groupDrawSession.start, point)
    }
    autoPanFrame = requestAnimationFrame(autoPanTick)
  }

  function updateGestureAutoPan(point: CanvasPoint): void {
    autoPanPointer = point
    if (autoPanFrame === null) autoPanFrame = requestAnimationFrame(autoPanTick)
  }

  function cancelGroupDraw(): void {
    const session = groupDrawSession
    stopGestureAutoPan(true)
    if (session) {
      try { surface?.releasePointerCapture(session.pointerId) } catch { /* already released */ }
    }
    groupDrawSession = null
    groupDrawRect = null
  }

  function createDrawnGroup(rect: CanvasRect): void {
    if (!canvasDoc || !finishTextBeforeStructure()) return
    const id = newId()
    const group: KnownCanvasNode = {
      id,
      type: 'group',
      x: Math.round(rect.x),
      y: Math.round(rect.y),
      width: Math.round(rect.width),
      height: Math.round(rect.height),
      label: '分组',
      extras: new Map(),
      preservedInvalid: new Map(),
      optionalPresence: new Set(),
    }
    commitDocument('拖拽创建分组', insertCanvasNode(canvasDoc, group, 0), new Set([id]), new Set())
  }

  function beginGroupDraw(event: PointerEvent): boolean {
    if (pendingPlacement !== 'group' || interactionLocked || event.button !== 0 || !event.isPrimary || event.pointerType === 'touch') return false
    const target = event.target
    if (!(target instanceof Element) || !target.closest('.svelte-flow__pane')) return false
    if (target.closest('.svelte-flow__node,.svelte-flow__edge,.svelte-flow__controls')) return false
    event.preventDefault()
    event.stopPropagation()
    try { surface?.setPointerCapture(event.pointerId) } catch { /* detached surface */ }
    const start = localPointer(event)
    groupDrawSession = { pointerId: event.pointerId, start, active: false }
    groupDrawRect = rectangleBetween(start, start)
    return true
  }

  function updateGroupDraw(event: PointerEvent): boolean {
    const session = groupDrawSession
    if (!session || session.pointerId !== event.pointerId) return false
    event.preventDefault()
    event.stopPropagation()
    const point = localPointer(event)
    const active = session.active || Math.hypot(point.x - session.start.x, point.y - session.start.y) >= 4
    if (active && !session.active) groupDrawSession = { ...session, active: true }
    groupDrawRect = rectangleBetween(groupDrawSession?.start ?? session.start, point)
    if (active) updateGestureAutoPan(point)
    return true
  }

  function finishGroupDraw(event: PointerEvent, commit: boolean): boolean {
    const session = groupDrawSession
    if (!session || session.pointerId !== event.pointerId) return false
    event.preventDefault()
    event.stopPropagation()
    const point = localPointer(event)
    const screenRect = rectangleBetween(session.start, point)
    stopGestureAutoPan(true)
    try { surface?.releasePointerCapture(event.pointerId) } catch { /* already released */ }
    groupDrawSession = null
    groupDrawRect = null
    if (!commit) return true
    if (!session.active) {
      pendingPlacement = null
      suppressNextPaneClick = true
      setTimeout(() => { suppressNextPaneClick = false }, 0)
      addNode('group', localToFlow(point))
      return true
    }
    const topLeft = localToFlow({ x: screenRect.x, y: screenRect.y })
    const bottomRight = localToFlow({
      x: screenRect.x + screenRect.width,
      y: screenRect.y + screenRect.height,
    })
    const flowRect = rectangleBetween(topLeft, bottomRight)
    if (flowRect.width < 20 || flowRect.height < 20) {
      suppressNextPaneClick = true
      setTimeout(() => { suppressNextPaneClick = false }, 0)
      return true
    }
    pendingPlacement = null
    suppressNextPaneClick = true
    setTimeout(() => { suppressNextPaneClick = false }, 0)
    createDrawnGroup(flowRect)
    return true
  }

  function handleSurfacePointerDown(event: PointerEvent): void {
    if (shouldRememberPointer(event.target)) lastPointerFlow = localToFlow(localPointer(event))
    if (connectionDraft && event.target instanceof Element && !event.target.closest('.connection-create-menu')) connectionDraft = null
    if (beginGroupDraw(event)) return
    if (effectiveTool !== 'lasso' || interactionLocked || event.button !== 0 || !event.isPrimary || event.pointerType === 'touch') return
    const target = event.target
    if (!(target instanceof Element) || !target.closest('.svelte-flow__pane')) return
    if (target.closest('.svelte-flow__node,.svelte-flow__edge,.svelte-flow__controls')) return
    event.preventDefault()
    event.stopPropagation()
    try { surface?.setPointerCapture(event.pointerId) } catch { /* detached surface */ }
    const start = localPointer(event)
    lassoSession = {
      pointerId: event.pointerId,
      start,
      additive: event.shiftKey,
      initialNodes: new Set(selectedNodeIds),
      initialEdges: new Set(selectedEdgeIds),
      active: false,
    }
    lassoPoints = [start]
  }

  function appendLassoPoint(points: CanvasPoint[], point: CanvasPoint): CanvasPoint[] {
    const previous = points.at(-1)
    if (!previous || Math.hypot(point.x - previous.x, point.y - previous.y) >= 4) return [...points, point]
    return points.length === 1 ? [points[0], point] : [...points.slice(0, -1), point]
  }

  function handleSurfacePointerMove(event: PointerEvent): void {
    const point = localPointer(event)
    if (shouldRememberPointer(event.target)) lastPointerFlow = localToFlow(point)
    if (updateGroupDraw(event)) return
    const session = lassoSession
    if (!session || session.pointerId !== event.pointerId) return
    event.preventDefault()
    event.stopPropagation()
    const active = session.active || Math.hypot(point.x - session.start.x, point.y - session.start.y) >= 4
    if (!active) return
    if (!session.active) lassoSession = { ...session, active: true }
    lassoPoints = appendLassoPoint(lassoPoints, point)
    previewLasso(lassoPoints, lassoSession)
    updateGestureAutoPan(point)
  }

  function finishLasso(event: PointerEvent, commit: boolean): void {
    const session = lassoSession
    if (!session || session.pointerId !== event.pointerId) return
    event.preventDefault()
    event.stopPropagation()
    stopGestureAutoPan(true)
    try { surface?.releasePointerCapture(event.pointerId) } catch { /* already released */ }
    if (!commit) { cancelLasso(true); return }
    const points = appendLassoPoint(lassoPoints, localPointer(event))
    const xs = points.map((point) => point.x)
    const ys = points.map((point) => point.y)
    const hasArea = session.active && points.length >= 3
      && Math.max(...xs) - Math.min(...xs) >= 10
      && Math.max(...ys) - Math.min(...ys) >= 10
    if (hasArea) previewLasso(points, session)
    else if (!session.additive) {
      selectedNodeIds = new Set()
      selectedEdgeIds = new Set()
    } else {
      selectedNodeIds = new Set(session.initialNodes)
      selectedEdgeIds = new Set(session.initialEdges)
    }
    lassoSession = null
    lassoPoints = []
    rebuildFlow()
  }

  function handleSurfacePointerUp(event: PointerEvent): void {
    if (finishGroupDraw(event, true)) return
    finishLasso(event, true)
  }
  function handleSurfacePointerCancel(event: PointerEvent): void {
    if (finishGroupDraw(event, false)) return
    finishLasso(event, false)
  }

  function handleKeyup(event: KeyboardEvent): void {
    if (event.key === ' ') spacePan = false
  }

  function startMultiResize(corner: ResizeCorner, event: PointerEvent): void {
    if (!canvasDoc || interactionLocked || selectedNodeIds.size < 2 || !finishTextBeforeStructure()) return
    const snapshot = createCanvasResizeSnapshot(canvasDoc, selectedNodeIds, corner)
    if (!snapshot) return
    multiResize = {
      pointerId: event.pointerId,
      snapshot,
      latestScaleX: 1,
      latestScaleY: 1,
    }
    snapGuides = []
  }

  function previewMultiResize(event: PointerEvent): void {
    if (!canvasDoc || !multiResize || multiResize.pointerId !== event.pointerId) return
    const cursor = screenToFlow({ x: event.clientX, y: event.clientY })
    const scale = resolveCanvasResizeScale(multiResize.snapshot, cursor, event.shiftKey)
    multiResize.latestScaleX = scale.scaleX
    multiResize.latestScaleY = scale.scaleY
    const preview = resizeCanvasSelection(
      canvasDoc,
      multiResize.snapshot,
      scale.scaleX,
      scale.scaleY,
    )
    const byId = new Map(preview.nodes.flatMap((entry) =>
      isKnownCanvasNode(entry) ? [[entry.id, entry] as const] : [],
    ))
    const resizedIds = new Set(multiResize.snapshot.nodes.map((node) => node.id))
    flowNodes = flowNodes.map((node) => {
      if (!resizedIds.has(node.id)) return node
      const geometry = byId.get(node.id)
      if (!geometry) return node
      return {
        ...node,
        position: { x: geometry.x, y: geometry.y },
        width: geometry.width,
        height: geometry.height,
        measured: { width: geometry.width, height: geometry.height },
      }
    })
  }

  function finishMultiResize(event: PointerEvent): void {
    if (!canvasDoc || !multiResize || multiResize.pointerId !== event.pointerId) return
    previewMultiResize(event)
    const session = multiResize
    multiResize = null
    const next = resizeCanvasSelection(
      canvasDoc,
      session.snapshot,
      session.latestScaleX,
      session.latestScaleY,
    )
    commitDocument('缩放多个节点', next)
  }

  function cancelMultiResize(): void {
    if (!multiResize) return
    multiResize = null
    rebuildFlow()
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.isComposing || composing) return
    if (event.target instanceof Element && event.target.closest('.connection-create-menu')) return
    if (isTextInput(event.target)) {
      if (event.key === 'Escape' && activeTextId) {
        event.preventDefault()
        event.stopPropagation()
        ;(event.target as HTMLElement).blur()
      }
      return
    }
    const mod = event.metaKey || event.ctrlKey
    const key = event.key.toLowerCase()
    if (!mod && !event.altKey && event.key === ' ') {
      event.preventDefault()
      spacePan = true
      return
    }
    if (!mod && !event.altKey && !event.shiftKey && ['s', 'p', 'l'].includes(key)) {
      event.preventDefault()
      event.stopPropagation()
      setTool(key === 's' ? 'select' : key === 'p' ? 'pan' : 'lasso')
      return
    }
    if (!mod && !event.altKey && !event.shiftKey && ['1', '2', '3', '4'].includes(key)) {
      event.preventDefault()
      event.stopPropagation()
      const kinds: Record<string, KnownCanvasNode['type']> = {
        '1': 'text', '2': 'group', '3': 'file', '4': 'link',
      }
      setPlacement(kinds[key])
      return
    }
    if (mod && (key === '+' || key === '=' || key === '-' || key === '_' || key === '0')) {
      event.preventDefault()
      event.stopPropagation()
      zoomViewport(key === '0' ? 'reset' : key === '+' || key === '=' ? 'in' : 'out')
      return
    }
    if (mod && key === 'z') { event.preventDefault(); event.stopPropagation(); event.shiftKey ? redo() : undo(); return }
    if (mod && key === 'y') { event.preventDefault(); event.stopPropagation(); redo(); return }
    if (mod && key === 'c') { event.preventDefault(); event.stopPropagation(); void copySelection(); return }
    if (mod && key === 'x') { event.preventDefault(); event.stopPropagation(); void cutSelection(); return }
    if (mod && key === 'a') {
      event.preventDefault(); event.stopPropagation()
      selectAll()
      return
    }
    if (event.key === 'Backspace' || event.key === 'Delete') {
      event.preventDefault(); event.stopPropagation(); deleteSelection(); return
    }
    if (!mod && !event.altKey && selectedNodeIds.size > 0 && ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) {
      event.preventDefault()
      event.stopPropagation()
      if (!canvasDoc || !finishTextBeforeStructure()) return
      const step = event.shiftKey ? 10 : 1
      const delta = {
        x: event.key === 'ArrowLeft' ? -step : event.key === 'ArrowRight' ? step : 0,
        y: event.key === 'ArrowUp' ? -step : event.key === 'ArrowDown' ? step : 0,
      }
      const frozen = freezeCanvasMove(canvasDoc, selectedNodeIds)
      commitDocument('移动选中节点', moveFrozenNodes(canvasDoc, frozen, delta))
      return
    }
    if (event.key === 'Enter' && selectedEdgeIds.size === 1 && selectedNodeIds.size === 0) {
      event.preventDefault(); event.stopPropagation()
      const edgeId = Array.from(selectedEdgeIds)[0]
      const target = Array.from(document.querySelectorAll<HTMLElement>('[data-canvas-edge-label]'))
        .find((entry) => entry.dataset.canvasEdgeLabel === edgeId)
      if (target instanceof HTMLButtonElement) target.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))
      return
    }
    if (event.key === 'Enter' && selectedNodeIds.size === 1) {
      event.preventDefault(); event.stopPropagation()
      const id = Array.from(selectedNodeIds)[0]
      const node = canvasDoc?.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === id)
      if (node && isKnownCanvasNode(node) && node.type === 'text') activateTextNode(id)
      else openNode(id)
      return
    }
    if (event.key === 'Escape') {
      if (singleResize) cancelSingleResize()
      else if (multiResize) cancelMultiResize()
      else if (groupDrawSession) { cancelGroupDraw(); pendingPlacement = null }
      else if (lassoSession) cancelLasso(true)
      else if (connectionDraft) connectionDraft = null
      else if (pendingPlacement) pendingPlacement = null
      else if (activeTool !== 'select') activeTool = 'select'
      else finalizeTextSession()
    }
  }

  function handleSurfaceDoubleClick(event: MouseEvent): void {
    if (interactionLocked || effectiveTool !== 'select' || pendingPlacement) return
    const target = event.target
    if (!(target instanceof Element) || !target.closest('.svelte-flow__pane')) return
    if (target.closest('.svelte-flow__node,.svelte-flow__edge,.svelte-flow__controls')) return
    event.preventDefault()
    addNode('text', screenToFlow({ x: event.clientX, y: event.clientY }))
  }

  function handleSurfaceClickCapture(event: MouseEvent): void {
    if (suppressNextPaneClick) {
      suppressNextPaneClick = false
      event.preventDefault()
      event.stopPropagation()
      return
    }
    if (!pendingPlacement || interactionLocked) return
    const target = event.target
    if (!(target instanceof Element) || !target.closest('.svelte-flow__pane')) return
    if (target.closest('.svelte-flow__node,.svelte-flow__edge,.svelte-flow__controls')) return
    // Svelte Flow owns pane clicks while drag-selection is enabled, so placement
    // must be resolved before the pane's selection handler consumes the event.
    event.preventDefault()
    event.stopPropagation()
    lastPointerFlow = screenToFlow({ x: event.clientX, y: event.clientY })
    placePendingNode(lastPointerFlow)
  }

  function handlePaneClick({ event }: { event: MouseEvent }): void {
    lastPointerFlow = screenToFlow({ x: event.clientX, y: event.clientY })
    if (pendingPlacement) placePendingNode(lastPointerFlow)
    else finalizeTextSession()
  }

  function updateEdgeLabelById(id: string, value: string): void {
    if (!canvasDoc || !finishTextBeforeStructure()) return
    const next = cloneCanvasDocument(canvasDoc)
    const index = next.edges.findIndex((entry) => isCanvasEdge(entry) && entry.id === id)
    if (index < 0 || !isCanvasEdge(next.edges[index])) return
    const edge = next.edges[index] as CanvasEdge
    const label = value.trim()
    if ((edge.label ?? '') === label) return
    edge.preservedInvalid.delete('label')
    if (label) { edge.label = label; edge.optionalPresence.add('label') }
    else { delete edge.label; edge.optionalPresence.delete('label') }
    commitDocument('编辑连线标签', next, selectedNodeIds, new Set([id]))
  }

  function updateEdgeLabel(value: string): void {
    if (selectedEdge) updateEdgeLabelById(selectedEdge.id, value)
  }

  function setEdgeEnd(field: 'fromEnd' | 'toEnd', value: CanvasEnd): void {
    if (!canvasDoc || !selectedEdge || !finishTextBeforeStructure()) return
    const next = updateCanvasEdge(canvasDoc, selectedEdge.id, (edge) => {
      const copy = { ...edge, [field]: value }
      copy.preservedInvalid.delete(field)
      copy.optionalPresence.add(field)
      return copy
    })
    commitDocument(field === 'fromEnd' ? '切换连线起点箭头' : '切换连线终点箭头', next)
  }

  function setEdgeColor(color: string | undefined): void {
    if (!canvasDoc || !selectedEdge || !finishTextBeforeStructure()) return
    const next = updateCanvasEdge(canvasDoc, selectedEdge.id, (edge) => {
      const copy = { ...edge }
      copy.preservedInvalid.delete('color')
      if (color) { copy.color = color; copy.optionalPresence.add('color') }
      else { delete copy.color; copy.optionalPresence.delete('color') }
      return copy
    })
    commitDocument('设置连线颜色', next)
  }

  function updateGroupLabel(value: string): void {
    if (!canvasDoc || selectedKnownNode?.type !== 'group' || !finishTextBeforeStructure()) return
    const next = updateCanvasNode(canvasDoc, selectedKnownNode.id, (node) => {
      if (node.type !== 'group') return node
      const copy = { ...node, label: value }
      copy.preservedInvalid.delete('label')
      copy.optionalPresence.add('label')
      return copy
    })
    commitDocument('重命名分组', next)
  }

  function setGroupBackgroundStyle(value: GroupBackgroundStyle): void {
    if (!canvasDoc || selectedKnownNode?.type !== 'group' || !finishTextBeforeStructure()) return
    const next = updateCanvasNode(canvasDoc, selectedKnownNode.id, (node) => {
      if (node.type !== 'group') return node
      const copy = { ...node, backgroundStyle: value }
      copy.preservedInvalid.delete('backgroundStyle')
      copy.optionalPresence.add('backgroundStyle')
      return copy
    })
    commitDocument('设置分组背景样式', next)
  }

  function clearGroupBackground(): void {
    if (!canvasDoc || selectedKnownNode?.type !== 'group' || !finishTextBeforeStructure()) return
    const next = updateCanvasNode(canvasDoc, selectedKnownNode.id, (node) => {
      if (node.type !== 'group') return node
      const copy = { ...node }
      delete copy.background
      delete copy.backgroundStyle
      copy.preservedInvalid.delete('background')
      copy.preservedInvalid.delete('backgroundStyle')
      copy.optionalPresence.delete('background')
      copy.optionalPresence.delete('backgroundStyle')
      return copy
    })
    commitDocument('移除分组背景', next)
  }

  function ungroupSelectedGroup(): void {
    if (!canvasDoc || selectedKnownNode?.type !== 'group' || !finishTextBeforeStructure()) return
    const groupId = selectedKnownNode.id
    const members = freezeGroupMove(canvasDoc, groupId).nodeIds.filter((id) => id !== groupId)
    const next = deleteCanvasSelection(canvasDoc, new Set([groupId]))
    commitDocument('解散分组', next, new Set(members), new Set())
  }

  function fitSelectedGroup(): void {
    if (!canvasDoc || selectedKnownNode?.type !== 'group' || !finishTextBeforeStructure()) return
    commitDocument('分组适配内容', fitCanvasGroupToContents(canvasDoc, selectedKnownNode.id))
  }

  function editSelectedLink(): void {
    if (!canvasDoc || selectedKnownNode?.type !== 'link') return
    const current = selectedKnownNode
    const value = window.prompt('输入 http 或 https 链接', current.url)?.trim()
    if (!value || !finishTextBeforeStructure()) return
    try {
      const url = new URL(value)
      if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error('unsupported protocol')
      const next = updateCanvasNode(canvasDoc, current.id, (node) => node.type === 'link' ? { ...node, url: url.href } : node)
      commitDocument('编辑链接', next)
    } catch {
      showError('只支持 http:// 或 https:// 链接。')
    }
  }

  function setNodeColor(color: string | undefined): void {
    if (!canvasDoc || !selectedKnownNode || !finishTextBeforeStructure()) return
    const next = updateCanvasNode(canvasDoc, selectedKnownNode.id, (node) => {
      const copy = { ...node }
      copy.preservedInvalid.delete('color')
      if (color) { copy.color = color; copy.optionalPresence.add('color') }
      else { delete copy.color; copy.optionalPresence.delete('color') }
      return copy
    })
    commitDocument('设置节点颜色', next)
  }

  function changeLayer(direction: 'front' | 'back'): void {
    if (!canvasDoc || selectedNodeIds.size === 0 || !finishTextBeforeStructure()) return
    commitDocument(
      direction === 'front' ? '移至最上层' : '移至最下层',
      reorderCanvasNodes(canvasDoc, selectedNodeIds, direction),
    )
  }

  function handleMoveEnd(_event: MouseEvent | TouchEvent | null, next: Viewport): void {
    viewport = next
    if (viewportTimer) clearTimeout(viewportTimer)
    viewportTimer = setTimeout(() => {
      void saveCanvasViewport(tab.filePath, { x: next.x, y: next.y, zoom: next.zoom })
    }, 300)
  }

  function screenToFlow(point: { x: number; y: number }): { x: number; y: number } {
    const rect = surface?.getBoundingClientRect()
    return {
      x: (point.x - (rect?.left ?? 0) - viewport.x) / viewport.zoom,
      y: (point.y - (rect?.top ?? 0) - viewport.y) / viewport.zoom,
    }
  }

  function zoomViewport(direction: 'in' | 'out' | 'reset'): void {
    const rect = surface?.getBoundingClientRect()
    const screenCenter = { x: (rect?.width ?? 800) / 2, y: (rect?.height ?? 600) / 2 }
    const flowCenter = {
      x: (screenCenter.x - viewport.x) / viewport.zoom,
      y: (screenCenter.y - viewport.y) / viewport.zoom,
    }
    const zoom = direction === 'reset' ? 1
      : Math.max(0.1, Math.min(4, viewport.zoom * (direction === 'in' ? 1.2 : 1 / 1.2)))
    viewport = {
      x: screenCenter.x - flowCenter.x * zoom,
      y: screenCenter.y - flowCenter.y * zoom,
      zoom,
    }
    handleMoveEnd(null, viewport)
  }

  function handleViewCommand(event: Event): void {
    const command = (event as CustomEvent<'in' | 'out' | 'reset'>).detail
    if (command === 'in' || command === 'out' || command === 'reset') zoomViewport(command)
  }

  function handleNativeDrop(event: Event): void {
    const detail = (event as CustomEvent<{ tabId: string; paths: string[]; position: { x: number; y: number } }>).detail
    if (!detail || detail.tabId !== tab.id) return
    const at = screenToFlow(detail.position)
    void (async () => {
      for (const [index, path] of detail.paths.entries()) {
        try {
          await addFilePath(path, { x: at.x + index * 28, y: at.y + index * 28 })
        } catch (error) {
          showError(`无法导入文件：${String(error)}`)
        }
      }
    })()
  }

  function handleWindowBlur(): void {
    spacePan = false
    cancelLasso(true)
    cancelGroupDraw()
    cancelSingleResize()
    cancelMultiResize()
    connectionDraft = null
    snapGuides = []
  }

  onMount(() => {
    rebuildFlow()
    let cancelled = false
    void loadCanvasViewport(tab.filePath).then((saved) => {
      if (cancelled) return
      if (saved) {
        viewport = { x: saved.x, y: saved.y, zoom: saved.zoom }
        hasStoredViewport = true
      }
      viewportReady = true
    })
    window.addEventListener('notemd:canvas-native-drop', handleNativeDrop)
    window.addEventListener('notemd:select-all', selectAll)
    window.addEventListener('notemd:canvas-view-command', handleViewCommand)
    window.addEventListener('keyup', handleKeyup)
    window.addEventListener('blur', handleWindowBlur)
    return () => {
      cancelled = true
      nodeDrag = null
      snapGuides = []
      cancelLasso(false)
      cancelGroupDraw()
      singleResize = null
      cancelMultiResize()
      if (viewportTimer) clearTimeout(viewportTimer)
      stopGestureAutoPan(false)
      resourceSession?.dispose()
      resourceSession = null
      resourceSessionRoot = ''
      requestedImages.clear()
      window.removeEventListener('notemd:canvas-native-drop', handleNativeDrop)
      window.removeEventListener('notemd:select-all', selectAll)
      window.removeEventListener('notemd:canvas-view-command', handleViewCommand)
      window.removeEventListener('keyup', handleKeyup)
      window.removeEventListener('blur', handleWindowBlur)
    }
  })

  $effect(() => {
    const nextRoot = resourceRoot()
    if (!resourceSession || resourceSessionRoot === nextRoot) return
    resourceSession.dispose()
    resourceSession = null
    resourceSessionRoot = ''
    requestedImages.clear()
    rebuildFlow()
  })

  $effect(() => {
    const incoming = tab.currentContent
    if (incoming === observedTabContent) return
    const preserveHistory = uiSession.content === incoming
    observedTabContent = incoming
    const decoded = decodeJsonCanvas(incoming)
    if (!decoded.ok) {
      nodeDrag = null
      snapGuides = []
      cancelLasso(false)
      cancelGroupDraw()
      singleResize = null
      cancelMultiResize()
      parseFailure = decoded.diagnostics
      diagnostics = decoded.diagnostics
      return
    }
    activeTextId = null
    textBefore = null
    composing = false
    nodeDrag = null
    if (!preserveHistory) history.clear()
    historyVersion++
    canvasDoc = decoded.document
    diagnostics = decoded.diagnostics
    parseFailure = null
    markCanvasUiSessionContent(tab.id, incoming)
    rebuildFlow()
  })

  $effect(() => {
    const nextPath = tab.filePath
    if (!nextPath || nextPath === observedTabPath) return
    observedTabPath = nextPath
    void saveCanvasViewport(nextPath, { x: viewport.x, y: viewport.y, zoom: viewport.zoom })
  })
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="canvas-surface"
  class:parse-error={!!parseFailure}
  class:tool-pan={effectiveTool === 'pan'}
  class:tool-lasso={effectiveTool === 'lasso'}
  class:placing={!!pendingPlacement}
  class:lod-compact={viewport.zoom < 0.48}
  style:--canvas-inverse-zoom={Math.min(1.8, 1 / Math.max(viewport.zoom, 0.55))}
  bind:this={surface}
  role="application"
  aria-label={`无限画布：${tab.title}`}
  tabindex="0"
  onkeydowncapture={handleKeydown}
  onpointerdowncapture={handleSurfacePointerDown}
  onpointermove={handleSurfacePointerMove}
  onpointerup={handleSurfacePointerUp}
  onpointercancel={handleSurfacePointerCancel}
  onclickcapture={handleSurfaceClickCapture}
  ondblclick={handleSurfaceDoubleClick}
  onpaste={handlePaste}
>
  {#if parseFailure}
    <div class="canvas-error" role="alert">
      <strong>无法编辑这个画布</strong>
      <p>源文件不是可安全读取的 JSON Canvas；内容已保持原样，不会被画布自动保存覆盖。</p>
      {#each parseFailure.slice(0, 5) as item}
        <code>{item.path}: {item.message}</code>
      {/each}
    </div>
  {:else if viewportReady}
    <div class="canvas-toolbar" aria-label="画布工具">
      <button class:tool-active={activeTool === 'select' && !interactionLocked} onclick={() => setTool('select')} title="选择工具 (S)" aria-label="选择工具">选择</button>
      <button class:tool-active={activeTool === 'pan' || interactionLocked} onclick={() => setTool('pan')} disabled={interactionLocked} title="平移工具 (P)，按住 Space 可临时平移" aria-label="平移工具">平移</button>
      <button class:tool-active={activeTool === 'lasso' && !interactionLocked} onclick={() => setTool('lasso')} disabled={interactionLocked} title="自由套索工具 (L)" aria-label="自由套索工具">套索</button>
      <button class:tool-active={interactionLocked} aria-pressed={interactionLocked} onclick={toggleInteractionLock} title="临时锁定或解锁当前画布交互">{interactionLocked ? '解锁' : '锁定'}</button>
      <span class="toolbar-separator"></span>
      <button onclick={() => addNode('text')} title="新建文本卡片">＋ 文本</button>
      <button onclick={chooseFileNode} title="添加当前 Vault 中的文件或图片">＋ 文件</button>
      <button onclick={addLinkNode} title="新建链接卡片">＋ 链接</button>
      <button onclick={addGroupNode} title="新建分组或围绕选中节点创建分组">＋ 分组</button>
      <button
        class:tool-active={pendingPlacement === 'group'}
        onclick={() => setPlacement('group')}
        title="拖拽绘制分组（快捷键 2）"
      >框组</button>
      <button onclick={() => void copySelection()} disabled={selectedNodeIds.size === 0} title="复制选中内容">复制</button>
      <button onclick={() => void cutSelection()} disabled={selectedNodeIds.size === 0} title="剪切选中内容">剪切</button>
      <button onclick={() => void pasteFromClipboard()} title="粘贴">粘贴</button>
      <span class="toolbar-separator"></span>
      <button onclick={undo} disabled={!canUndo} title={undoTitle} aria-label={undoTitle}>↶</button>
      <button onclick={redo} disabled={!canRedo} title={redoTitle} aria-label={redoTitle}>↷</button>
      <button onclick={deleteSelection} disabled={selectedNodeIds.size + selectedEdgeIds.size === 0} title="删除选中内容" aria-label="删除选中内容">⌫</button>
      {#if selectedNodeIds.size > 0}
        <button onclick={() => changeLayer('front')} title="移至最上层">置顶</button>
        <button onclick={() => changeLayer('back')} title="移至最下层">置底</button>
      {/if}
      {#if selectionRoots.length > 1}
        <span class="toolbar-separator"></span>
        <button onclick={() => arrangeSelection('left')} title="左对齐" aria-label="左对齐">⇤</button>
        <button onclick={() => arrangeSelection('center-h')} title="水平居中对齐" aria-label="水平居中对齐">↔</button>
        <button onclick={() => arrangeSelection('right')} title="右对齐" aria-label="右对齐">⇥</button>
        <button onclick={() => arrangeSelection('top')} title="顶部对齐" aria-label="顶部对齐">⤒</button>
        <button onclick={() => arrangeSelection('center-v')} title="垂直居中对齐" aria-label="垂直居中对齐">↕</button>
        <button onclick={() => arrangeSelection('bottom')} title="底部对齐" aria-label="底部对齐">⤓</button>
        <button onclick={() => distributeSelection('horizontal')} disabled={selectionRoots.length < 3} title="水平等距分布" aria-label="水平等距分布">H⋯</button>
        <button onclick={() => distributeSelection('vertical')} disabled={selectionRoots.length < 3} title="垂直等距分布" aria-label="垂直等距分布">V⋮</button>
        <button onclick={spreadSelection} title="散开重叠节点">散开</button>
      {/if}
      {#if selectedEdge}
        <span class="toolbar-separator"></span>
        <label class="edge-label">
          连线标签
          <input
            value={selectedEdge.label ?? ''}
            placeholder="可选"
            onchange={(event) => updateEdgeLabel(event.currentTarget.value)}
            onkeydown={(event) => event.stopPropagation()}
          />
        </label>
        <button
          class:tool-active={(selectedEdge.fromEnd ?? 'none') === 'arrow'}
          aria-pressed={(selectedEdge.fromEnd ?? 'none') === 'arrow'}
          onclick={() => setEdgeEnd('fromEnd', (selectedEdge?.fromEnd ?? 'none') === 'arrow' ? 'none' : 'arrow')}
          title="切换连线起点箭头"
        >起点箭头</button>
        <button
          class:tool-active={(selectedEdge.toEnd ?? 'arrow') === 'arrow'}
          aria-pressed={(selectedEdge.toEnd ?? 'arrow') === 'arrow'}
          onclick={() => setEdgeEnd('toEnd', (selectedEdge?.toEnd ?? 'arrow') === 'arrow' ? 'none' : 'arrow')}
          title="切换连线终点箭头"
        >终点箭头</button>
        <div class="color-row" aria-label="连线颜色">
          {#each [undefined, '1', '2', '3', '4', '5', '6'] as color}
            <button
              class="color-swatch"
              class:selected={selectedEdge.color === color}
              style:--swatch={displayColor(color) ?? 'CanvasText'}
              title={color ? `连线颜色 ${color}` : '默认连线颜色'}
              onclick={() => setEdgeColor(color)}
            ><span class="sr-only">{color ?? '默认'}</span></button>
          {/each}
        </div>
      {:else if selectedKnownNode}
        <span class="toolbar-separator"></span>
        {#if selectedKnownNode.type === 'file'}
          <button onclick={() => void relinkSelectedResource()} title="重新选择并导入文件">重新链接</button>
        {:else if selectedKnownNode.type === 'link'}
          <button onclick={editSelectedLink} title="编辑链接地址">编辑链接</button>
        {:else if selectedKnownNode.type === 'group'}
          <button onclick={fitSelectedGroup} title="缩放分组边界以适配其中节点">适配内容</button>
          <button onclick={() => void relinkSelectedResource()} title="选择并导入分组背景图片">设置背景</button>
          {#if selectedKnownNode.background || selectedKnownNode.preservedInvalid.has('background')}
            <button onclick={clearGroupBackground} title="移除分组背景图片">移除背景</button>
          {/if}
          <label class="group-name-label">
            分组名称
            <input
              value={selectedKnownNode.label ?? ''}
              placeholder="分组"
              onchange={(event) => updateGroupLabel(event.currentTarget.value)}
              onkeydown={(event) => event.stopPropagation()}
            />
          </label>
          {#if selectedKnownNode.background}
            <label class="group-style-label">
              背景
              <select
                value={selectedKnownNode.backgroundStyle ?? 'ratio'}
                onchange={(event) => setGroupBackgroundStyle(event.currentTarget.value as GroupBackgroundStyle)}
                onkeydown={(event) => event.stopPropagation()}
              >
                <option value="ratio">完整显示</option>
                <option value="cover">铺满</option>
                <option value="repeat">平铺</option>
              </select>
            </label>
          {/if}
          <button onclick={ungroupSelectedGroup} title="移除分组边框并保留其中节点">解组</button>
        {/if}
        <div class="color-row" aria-label="节点颜色">
          {#each [undefined, '1', '2', '3', '4', '5', '6'] as color}
            <button
              class="color-swatch"
              class:selected={selectedKnownNode.color === color}
              style:--swatch={displayColor(color) ?? 'Canvas'}
              title={color ? `颜色 ${color}` : '默认颜色'}
              onclick={() => setNodeColor(color)}
            ><span class="sr-only">{color ?? '默认'}</span></button>
          {/each}
        </div>
      {/if}
    </div>

    <SvelteFlow
      bind:nodes={flowNodes}
      bind:edges={flowEdges}
      bind:viewport
      {nodeTypes}
      {edgeTypes}
      fitView={!hasStoredViewport}
      fitViewOptions={{ padding: 0.2, maxZoom: 1.25 }}
      nodeOrigin={[0, 0]}
      zIndexMode="manual"
      elevateNodesOnSelect={false}
      connectionMode={ConnectionMode.Loose}
      selectionMode={SelectionMode.Partial}
      selectionOnDrag={effectiveTool === 'select'}
      panOnDrag={effectiveTool === 'pan' ? true : effectiveTool === 'select' ? [1, 2] : false}
      nodesDraggable={effectiveTool === 'select' && !interactionLocked && !multiResize}
      nodesConnectable={effectiveTool === 'select' && !interactionLocked}
      elementsSelectable={effectiveTool === 'select' && !interactionLocked}
      panOnScroll={true}
      zoomOnScroll={false}
      zoomOnPinch={true}
      zoomOnDoubleClick={false}
      minZoom={0.1}
      maxZoom={4}
      deleteKey={null}
      onlyRenderVisibleElements={flowNodes.length > 500 && !activeTextId}
      connectionDragThreshold={0}
      autoPanOnConnect={true}
      autoPanOnNodeDrag={true}
      autoPanSpeed={20}
      isValidConnection={(connection) => connection.source !== connection.target}
      onconnect={handleConnect}
      onconnectstart={handleConnectStart}
      onconnectend={handleConnectEnd}
      onreconnect={handleReconnect}
      onreconnectstart={() => { reconnectActive = true }}
      onreconnectend={() => { reconnectActive = false; newConnectionActive = false }}
      onnodedragstart={handleNodeDragStart}
      onnodedrag={handleNodeDrag}
      onnodedragstop={handleNodeDragStop}
      onselectionchange={handleSelection}
      ondelete={handleDelete}
      onpaneclick={handlePaneClick}
      onmoveend={handleMoveEnd}
    >
      <Background
        variant={BackgroundVariant.Dots}
        gap={22}
        size={1.2}
        patternColor="color-mix(in srgb, CanvasText 18%, transparent)"
      />
      <Controls
        position="bottom-right"
        showLock={false}
        fitViewOptions={{ padding: 0.2, maxZoom: 1.25 }}
      />
    </SvelteFlow>

    <CanvasInteractionOverlay guides={snapGuides} {lassoPoints} drawRect={groupDrawRect} {viewport} />
    {#if multiSelectionBounds && effectiveTool === 'select' && !interactionLocked && !activeTextId}
      <CanvasSelectionResizer
        bounds={multiSelectionBounds}
        {viewport}
        onStart={startMultiResize}
        onMove={previewMultiResize}
        onEnd={finishMultiResize}
        onCancel={() => cancelMultiResize()}
      />
    {/if}

    {#if connectionDraft}
      <div
        class="connection-create-menu nodrag nopan"
        bind:this={connectionMenu}
        style:left={`${Math.min(connectionDraft.screen.x + 12, Math.max(8, (surface?.clientWidth ?? 800) - 220))}px`}
        style:top={`${Math.min(connectionDraft.screen.y + 12, Math.max(8, (surface?.clientHeight ?? 600) - 70))}px`}
        role="toolbar"
        tabindex="-1"
        aria-label="创建并连接节点"
        onkeydown={handleConnectionMenuKeydown}
      >
        <span>创建并连接</span>
        <button type="button" onclick={() => void chooseConnectedNode('text')}>文本</button>
        <button type="button" onclick={() => void chooseConnectedNode('group')}>分组</button>
        <button type="button" onclick={() => void chooseConnectedNode('file')}>文件</button>
        <button type="button" onclick={() => void chooseConnectedNode('link')}>链接</button>
      </div>
    {/if}

    <button class="zoom-indicator" onclick={() => zoomViewport('reset')} title="重置缩放 (Cmd/Ctrl+0)">
      {Math.round(viewport.zoom * 100)}%
    </button>

    {#if diagnostics.length > 0}
      <div class="diagnostic-badge" title={diagnostics.map((item) => item.message).join('\n')}>
        ⚠ {diagnostics.length} 项兼容性提示
      </div>
    {/if}
  {/if}
</div>

<style>
  .canvas-surface {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: color-mix(in srgb, Canvas 97%, CanvasText 3%);
    color: CanvasText;
    outline: none;
  }
  .canvas-surface :global(.svelte-flow) { background: transparent; }
  .canvas-surface.tool-pan :global(.svelte-flow__pane) { cursor: grab; }
  .canvas-surface.tool-pan :global(.svelte-flow__pane:active) { cursor: grabbing; }
  .canvas-surface.tool-lasso :global(.svelte-flow__pane),
  .canvas-surface.placing :global(.svelte-flow__pane) { cursor: crosshair; }
  .canvas-surface.tool-pan :global(.svelte-flow__resize-control),
  .canvas-surface.tool-lasso :global(.svelte-flow__resize-control) { display: none; }
  .canvas-surface :global(.svelte-flow__node) {
    border: 0;
    background: transparent;
  }
  .canvas-surface :global(.svelte-flow__node.canvas-group-shell) { pointer-events: none; }
  .canvas-surface :global(.svelte-flow__node.selected .canvas-card) {
    outline: 2px solid var(--accent, #4d88ff);
    outline-offset: 2px;
  }
  .canvas-surface :global(.svelte-flow__edge.selected path) {
    stroke: var(--accent, #4d88ff);
    stroke-width: 3;
  }
  .canvas-surface :global(.canvas-edge-reconnect) {
    border: 2px solid Canvas;
    border-radius: 999px;
    background: var(--accent, #4d88ff);
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.24);
    cursor: crosshair;
  }
  .canvas-surface.lod-compact :global(.canvas-card:not(.group-node):not(.active-editor):not(.opaque-node) .node-detail) {
    display: none;
  }
  .canvas-surface.lod-compact :global(.canvas-card:not(.group-node):not(.active-editor):not(.opaque-node) .compact-label) {
    display: flex;
  }
  .canvas-surface.lod-compact :global(.canvas-handle) { display: none; }
  .canvas-toolbar {
    position: absolute;
    z-index: 20;
    top: 12px;
    left: 50%;
    display: flex;
    max-width: calc(100% - 32px);
    align-items: center;
    gap: 4px;
    padding: 5px;
    overflow-x: auto;
    border: 1px solid color-mix(in srgb, CanvasText 13%, transparent);
    border-radius: 11px;
    background: color-mix(in srgb, Canvas 86%, transparent);
    box-shadow: 0 8px 26px rgba(0, 0, 0, 0.13);
    backdrop-filter: blur(18px) saturate(1.25);
    transform: translateX(-50%);
  }
  .canvas-toolbar > button, .color-swatch {
    flex: 0 0 auto;
    min-height: 30px;
    padding: 4px 9px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 12px;
    white-space: nowrap;
    cursor: pointer;
  }
  .canvas-toolbar > button:hover:not(:disabled) { background: color-mix(in srgb, CanvasText 9%, transparent); }
  .canvas-toolbar > button.tool-active { background: var(--accent, #4d88ff); color: white; }
  .canvas-toolbar > button:disabled { opacity: 0.32; cursor: default; }
  .toolbar-separator { flex: 0 0 1px; align-self: stretch; margin: 3px; background: color-mix(in srgb, CanvasText 13%, transparent); }
  .edge-label, .group-name-label, .group-style-label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 5px;
    font-size: 11px;
    white-space: nowrap;
  }
  .edge-label input, .group-name-label input, .group-style-label select {
    width: 112px;
    padding: 5px 7px;
    border: 1px solid color-mix(in srgb, CanvasText 16%, transparent);
    border-radius: 6px;
    background: Canvas;
    color: CanvasText;
    font: inherit;
  }
  .group-name-label input { width: 90px; }
  .group-style-label select { width: auto; }
  .color-row { display: flex; align-items: center; gap: 3px; padding: 0 3px; }
  .color-swatch {
    width: 22px;
    min-height: 22px;
    padding: 0;
    border: 2px solid Canvas;
    border-radius: 50%;
    background: var(--swatch);
    box-shadow: 0 0 0 1px color-mix(in srgb, CanvasText 20%, transparent);
  }
  .color-swatch.selected { box-shadow: 0 0 0 2px var(--accent, #4d88ff); }
  .connection-create-menu {
    position: absolute;
    z-index: 24;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px;
    border: 1px solid color-mix(in srgb, CanvasText 16%, transparent);
    border-radius: 9px;
    background: color-mix(in srgb, Canvas 92%, transparent);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.16);
    backdrop-filter: blur(16px);
  }
  .connection-create-menu span {
    padding: 0 5px;
    color: color-mix(in srgb, CanvasText 62%, transparent);
    font-size: 11px;
    white-space: nowrap;
  }
  .connection-create-menu button,
  .zoom-indicator {
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: CanvasText;
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .connection-create-menu button { padding: 5px 7px; }
  .connection-create-menu button:hover,
  .zoom-indicator:hover { background: color-mix(in srgb, CanvasText 9%, transparent); }
  .zoom-indicator {
    position: absolute;
    z-index: 18;
    bottom: 14px;
    left: 50%;
    min-width: 48px;
    padding: 5px 8px;
    border: 1px solid color-mix(in srgb, CanvasText 13%, transparent);
    background: color-mix(in srgb, Canvas 86%, transparent);
    transform: translateX(-50%);
    backdrop-filter: blur(12px);
  }
  .diagnostic-badge {
    position: absolute;
    z-index: 18;
    right: 12px;
    bottom: 12px;
    padding: 6px 9px;
    border: 1px solid color-mix(in srgb, #c58b31 35%, transparent);
    border-radius: 7px;
    background: color-mix(in srgb, #c58b31 14%, Canvas);
    font-size: 11px;
    pointer-events: none;
  }
  .canvas-error {
    box-sizing: border-box;
    width: min(680px, calc(100% - 40px));
    margin: 40px auto;
    padding: 20px;
    border: 1px solid color-mix(in srgb, #c94a4a 35%, transparent);
    border-radius: 12px;
    background: color-mix(in srgb, #c94a4a 7%, Canvas);
  }
  .canvas-error p { color: color-mix(in srgb, CanvasText 68%, transparent); }
  .canvas-error code { display: block; margin-top: 6px; white-space: pre-wrap; }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
  @media (max-width: 700px) {
    .canvas-toolbar {
      top: 8px;
      max-width: calc(100% - 16px);
    }
    .canvas-toolbar > button { min-height: 44px; padding-inline: 12px; }
    .color-swatch { width: 44px; min-height: 44px; }
  }
  @media (pointer: coarse) {
    .canvas-toolbar > button { min-height: 44px; padding-inline: 12px; }
    .color-swatch { width: 44px; min-height: 44px; }
    .canvas-surface :global(.svelte-flow__resize-control.handle) {
      width: 28px;
      height: 28px;
      border: 0;
      background: radial-gradient(circle, var(--accent, #4d88ff) 0 5px, transparent 6px);
    }
    .canvas-surface :global(.canvas-edge-reconnect) {
      width: 32px !important;
      height: 32px !important;
      border: 0;
      background: radial-gradient(circle, var(--accent, #4d88ff) 0 6px, transparent 7px);
      box-shadow: none;
    }
    .canvas-surface :global(.svelte-flow__controls-button) {
      width: 44px;
      height: 44px;
    }
  }
</style>
