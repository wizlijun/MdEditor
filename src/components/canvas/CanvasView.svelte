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
    type OnReconnect,
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
    cloneCanvasDocument,
    copyCanvasSelection,
    decodeJsonCanvas,
    deleteCanvasSelection,
    encodeJsonCanvas,
    flowConnectionToCanvasEdge,
    freezeGroupMove,
    insertCanvasEdge,
    insertCanvasNode,
    importCanvasResource,
    isCanvasEdge,
    isKnownCanvasNode,
    moveFrozenNodes,
    pasteCanvasSelection,
    projectCanvasToFlow,
    recallCanvasClipboard,
    rememberCanvasClipboard,
    markCanvasUiSessionContent,
    reorderCanvasNodes,
    resolveCanvasResource,
    updateCanvasNode,
    type CanvasClipboardPayload,
    type CanvasDocument,
    type CanvasEdge,
    type Diagnostic,
    type FrozenGroupMove,
    type KnownCanvasNode,
  } from '../../lib/canvas'
  import CanvasCardNode from './CanvasCardNode.svelte'
  import { loadCanvasViewport, saveCanvasViewport } from './canvas-view-state'

  let { tab }: { tab: Tab } = $props()

  type UiNode = Node<Record<string, unknown>>
  type UiEdge = Edge<Record<string, unknown>>

  const nodeTypes = {
    'canvas-text': CanvasCardNode,
    'canvas-file': CanvasCardNode,
    'canvas-link': CanvasCardNode,
    'canvas-group': CanvasCardNode,
    'canvas-diagnostic': CanvasCardNode,
  }

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
  let touchSelectionMode = $state(false)
  let historyVersion = $state(0)
  let surface: HTMLDivElement | undefined = $state()
  let viewport = $state.raw<Viewport>({ x: 0, y: 0, zoom: 1 })
  let viewportReady = $state(false)
  let hasStoredViewport = $state(false)
  let viewportTimer: ReturnType<typeof setTimeout> | null = null
  let groupDrag: { frozen: FrozenGroupMove; origin: { x: number; y: number } } | null = null
  let pasteCount = 0
  let resourceSession: CanvasResourceSession | null = null
  let resourceSessionRoot = ''
  const requestedImages = new Set<string>()
  const uiSession = acquireCanvasUiSession(initialTabId(), initialTabContent())
  const history = uiSession.history

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
  let touchLayout = $derived(formFactor.value !== 'desktop')

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
        data: {
          ...node.data,
          kind,
          diagnostic: node.data.diagnostic?.message,
          active: activeTextId === node.data.canonicalId,
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
          onResizeEnd: commitResize,
        },
      } as UiNode
    })
    flowEdges = projection.edges.map((edge) => {
      const color = displayColor(edge.data.colorToken)
      return {
        ...edge,
        selected: selectedEdgeIds.has(edge.id),
        markerStart: edge.markerStart ? { type: MarkerType.ArrowClosed } : undefined,
        markerEnd: edge.markerEnd ? { type: MarkerType.ArrowClosed } : undefined,
        style: color ? `stroke:${color};stroke-width:2` : 'stroke-width:2',
        labelStyle: 'fill:CanvasText;font-size:12px',
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

  function addNode(kind: KnownCanvasNode['type'], at = viewportCenter(), value?: string): void {
    if (!canvasDoc || !finishTextBeforeStructure()) return
    const id = newId()
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
    const node: KnownCanvasNode = kind === 'text'
      ? { ...common, type: 'text', text: value ?? '# 新卡片\n\n双击开始编辑' }
      : kind === 'file'
        ? { ...common, type: 'file', file: value ?? '' }
        : kind === 'link'
          ? { ...common, type: 'link', url: value ?? 'https://' }
          : { ...common, type: 'group', label: value || '分组' }
    const index = kind === 'group' ? 0 : canvasDoc.nodes.length
    const next = insertCanvasNode(canvasDoc, node, index)
    commitDocument(`创建${kind === 'text' ? '文本' : kind === 'file' ? '文件' : kind === 'link' ? '链接' : '分组'}节点`, next, new Set([id]), new Set())
    if (kind === 'text') queueMicrotask(() => activateTextNode(id))
  }

  async function chooseFileNode(): Promise<void> {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const picked = await open({ multiple: false })
      if (typeof picked !== 'string') return
      await addFilePath(picked, viewportCenter())
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
      const picked = await open({ multiple: false })
      if (typeof picked !== 'string' || !finishTextBeforeStructure()) return
      const imported = await importCanvasResource(resourceRoot(), tab.filePath, picked)
      const id = selected.id
      const next = updateCanvasNode(canvasDoc, id, (node) => {
        if (node.type === 'file') return { ...node, file: imported.relativePath }
        if (node.type === 'group') return { ...node, background: imported.relativePath }
        return node
      })
      commitDocument(selected.type === 'file' ? '重新链接文件' : '设置分组背景', next)
    } catch (error) {
      showError(`无法导入资源：${String(error)}`)
    }
  }

  function addLinkNode(): void {
    const value = window.prompt('输入 http 或 https 链接')?.trim()
    if (!value) return
    try {
      const url = new URL(value)
      if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error('unsupported protocol')
      addNode('link', viewportCenter(), url.href)
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

  function commitResize(id: string): void {
    if (!canvasDoc) return
    const flowNode = flowNodes.find((node) => node.id === id)
    const current = canvasDoc.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === id)
    if (!flowNode || !current || !isKnownCanvasNode(current)) return
    const width = flowNode.width ?? flowNode.measured?.width ?? current.width
    const height = flowNode.height ?? flowNode.measured?.height ?? current.height
    const next = updateCanvasNode(canvasDoc, id, (node) => ({
      ...node,
      x: Math.round(flowNode.position.x),
      y: Math.round(flowNode.position.y),
      width: Math.max(node.type === 'group' ? 180 : 160, Math.round(width)),
      height: Math.max(node.type === 'group' ? 120 : 100, Math.round(height)),
    }))
    commitDocument('调整节点大小', next)
  }

  function handleNodeDragStart({ targetNode }: { targetNode: UiNode | null }): void {
    if (!canvasDoc || !targetNode) return
    const node = canvasDoc.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === targetNode.id)
    if (!node || !isKnownCanvasNode(node) || node.type !== 'group') { groupDrag = null; return }
    groupDrag = { frozen: freezeGroupMove(canvasDoc, node.id), origin: { x: node.x, y: node.y } }
  }

  function handleNodeDrag({ targetNode }: { targetNode: UiNode | null }): void {
    if (!canvasDoc || !targetNode || groupDrag?.frozen.groupId !== targetNode.id) return
    const dx = targetNode.position.x - groupDrag.origin.x
    const dy = targetNode.position.y - groupDrag.origin.y
    const moving = new Set(groupDrag.frozen.nodeIds)
    flowNodes = flowNodes.map((flowNode) => {
      if (flowNode.id === targetNode.id || !moving.has(flowNode.id)) return flowNode
      const canonical = canvasDoc?.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === flowNode.id)
      if (!canonical || !isKnownCanvasNode(canonical)) return flowNode
      return { ...flowNode, position: { x: canonical.x + dx, y: canonical.y + dy } }
    })
  }

  function handleNodeDragStop({ targetNode, nodes }: { targetNode: UiNode | null; nodes: UiNode[] }): void {
    if (!canvasDoc) return
    if (!finishTextBeforeStructure()) { rebuildFlow(); return }
    if (targetNode && groupDrag?.frozen.groupId === targetNode.id) {
      const next = moveFrozenNodes(canvasDoc, groupDrag.frozen, {
        x: targetNode.position.x - groupDrag.origin.x,
        y: targetNode.position.y - groupDrag.origin.y,
      })
      groupDrag = null
      commitDocument('移动分组', next)
      return
    }
    groupDrag = null
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
    if (!canvasDoc || !connection.source || !connection.target || !finishTextBeforeStructure()) return
    const edge = flowConnectionToCanvasEdge(newId(), connection)
    commitDocument('创建连线', insertCanvasEdge(canvasDoc, edge), selectedNodeIds, new Set([edge.id]))
  }

  const handleReconnect: OnReconnect<UiEdge> = (oldEdge, connection) => {
    if (!canvasDoc || !finishTextBeforeStructure()) return
    const edgeId = (oldEdge.data?.canonicalId as string | undefined) ?? oldEdge.id
    commitDocument('重连连线', applyFlowEdgeConnection(canvasDoc, edgeId, connection), selectedNodeIds, new Set([edgeId]))
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
    if (!sameIds(selectedNodeIds, nextNodes)) selectedNodeIds = nextNodes
    if (!sameIds(selectedEdgeIds, nextEdges)) selectedEdgeIds = nextEdges
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
    try { await navigator.clipboard?.writeText(text) } catch { /* in-process clipboard remains valid */ }
    return true
  }

  async function cutSelection(): Promise<void> {
    if (await copySelection()) deleteSelection()
  }

  async function pasteFromClipboard(): Promise<void> {
    try {
      const text = await navigator.clipboard?.readText()
      if (text?.trim()) {
        const remembered = recallCanvasClipboard(text)
        if (remembered) pastePayload(remembered)
        else pasteText(text)
        return
      }
    } catch { /* fall back to the in-process Canvas clipboard */ }
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
    const center = viewportCenter()
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
        addNode('link', viewportCenter(), url.href)
        return
      }
    } catch { /* ordinary text */ }
    addNode('text', viewportCenter(), text)
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

  function handleKeydown(event: KeyboardEvent): void {
    if (event.isComposing || composing || isTextInput(event.target)) return
    const mod = event.metaKey || event.ctrlKey
    const key = event.key.toLowerCase()
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
    if (event.key === 'Enter' && selectedNodeIds.size === 1) {
      event.preventDefault(); event.stopPropagation()
      const id = Array.from(selectedNodeIds)[0]
      const node = canvasDoc?.nodes.find((entry) => isKnownCanvasNode(entry) && entry.id === id)
      if (node && isKnownCanvasNode(node) && node.type === 'text') activateTextNode(id)
      else openNode(id)
      return
    }
    if (event.key === 'Escape') finalizeTextSession()
  }

  function updateEdgeLabel(value: string): void {
    if (!canvasDoc || !selectedEdge || !finishTextBeforeStructure()) return
    const id = selectedEdge.id
    const next = cloneCanvasDocument(canvasDoc)
    const index = next.edges.findIndex((entry) => isCanvasEdge(entry) && entry.id === id)
    if (index < 0 || !isCanvasEdge(next.edges[index])) return
    const edge = next.edges[index] as CanvasEdge
    edge.preservedInvalid.delete('label')
    if (value.trim()) { edge.label = value; edge.optionalPresence.add('label') }
    else { delete edge.label; edge.optionalPresence.delete('label') }
    commitDocument('编辑连线标签', next)
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
    return () => {
      cancelled = true
      if (viewportTimer) clearTimeout(viewportTimer)
      resourceSession?.dispose()
      resourceSession = null
      resourceSessionRoot = ''
      requestedImages.clear()
      window.removeEventListener('notemd:canvas-native-drop', handleNativeDrop)
      window.removeEventListener('notemd:select-all', selectAll)
      window.removeEventListener('notemd:canvas-view-command', handleViewCommand)
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
      parseFailure = decoded.diagnostics
      diagnostics = decoded.diagnostics
      return
    }
    activeTextId = null
    textBefore = null
    composing = false
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
  bind:this={surface}
  role="application"
  aria-label={`无限画布：${tab.title}`}
  tabindex="0"
  onkeydown={handleKeydown}
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
      <button onclick={() => addNode('text')} title="新建文本卡片">＋ 文本</button>
      <button onclick={chooseFileNode} title="添加当前 Vault 中的文件或图片">＋ 文件</button>
      <button onclick={addLinkNode} title="新建链接卡片">＋ 链接</button>
      <button onclick={addGroupNode} title="新建分组">＋ 分组</button>
      {#if touchLayout}
        <button
          class:tool-active={touchSelectionMode}
          aria-pressed={touchSelectionMode}
          onclick={() => { touchSelectionMode = !touchSelectionMode }}
          title="切换平移或框选"
        >{touchSelectionMode ? '框选' : '平移'}</button>
        <button onclick={() => void copySelection()} disabled={selectedNodeIds.size === 0} title="复制选中内容">复制</button>
        <button onclick={() => void cutSelection()} disabled={selectedNodeIds.size === 0} title="剪切选中内容">剪切</button>
        <button onclick={() => void pasteFromClipboard()} title="粘贴">粘贴</button>
      {/if}
      <span class="toolbar-separator"></span>
      <button onclick={undo} disabled={!history.canUndo} title={history.undoLabel ? `撤销：${history.undoLabel}` : '撤销'}>↶</button>
      <button onclick={redo} disabled={!history.canRedo} title={history.redoLabel ? `重做：${history.redoLabel}` : '重做'}>↷</button>
      <button onclick={deleteSelection} disabled={selectedNodeIds.size + selectedEdgeIds.size === 0} title="删除选中内容">⌫</button>
      {#if selectedNodeIds.size > 0}
        <button onclick={() => changeLayer('front')} title="移至最上层">置顶</button>
        <button onclick={() => changeLayer('back')} title="移至最下层">置底</button>
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
      {:else if selectedKnownNode}
        <span class="toolbar-separator"></span>
        {#if selectedKnownNode.type === 'file'}
          <button onclick={() => void relinkSelectedResource()} title="重新选择并导入文件">重新链接</button>
        {:else if selectedKnownNode.type === 'group'}
          <button onclick={() => void relinkSelectedResource()} title="选择并导入分组背景图片">设置背景</button>
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
      fitView={!hasStoredViewport}
      fitViewOptions={{ padding: 0.2, maxZoom: 1.25 }}
      nodeOrigin={[0, 0]}
      zIndexMode="manual"
      elevateNodesOnSelect={false}
      connectionMode={ConnectionMode.Loose}
      selectionMode={SelectionMode.Partial}
      selectionOnDrag={!touchLayout || touchSelectionMode}
      panOnDrag={touchLayout ? !touchSelectionMode : [1, 2]}
      panOnScroll={true}
      zoomOnScroll={false}
      zoomOnPinch={true}
      zoomOnDoubleClick={false}
      minZoom={0.1}
      maxZoom={4}
      deleteKey={null}
      onlyRenderVisibleElements={flowNodes.length > 500 && !activeTextId}
      onconnect={handleConnect}
      onreconnect={handleReconnect}
      onnodedragstart={handleNodeDragStart}
      onnodedrag={handleNodeDrag}
      onnodedragstop={handleNodeDragStop}
      onselectionchange={handleSelection}
      ondelete={handleDelete}
      onpaneclick={() => finalizeTextSession()}
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
  .canvas-surface :global(.svelte-flow__node) {
    border: 0;
    background: transparent;
  }
  .canvas-surface :global(.svelte-flow__node.selected .canvas-card) {
    outline: 2px solid var(--accent, #4d88ff);
    outline-offset: 2px;
  }
  .canvas-surface :global(.svelte-flow__edge.selected path) {
    stroke: var(--accent, #4d88ff);
    stroke-width: 3;
  }
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
  .edge-label { display: flex; align-items: center; gap: 6px; padding: 0 5px; font-size: 11px; white-space: nowrap; }
  .edge-label input {
    width: 112px;
    padding: 5px 7px;
    border: 1px solid color-mix(in srgb, CanvasText 16%, transparent);
    border-radius: 6px;
    background: Canvas;
    color: CanvasText;
    font: inherit;
  }
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
    .canvas-surface :global(.svelte-flow__resize-control.handle) {
      width: 28px;
      height: 28px;
      border: 0;
      background: radial-gradient(circle, var(--accent, #4d88ff) 0 5px, transparent 6px);
    }
  }
</style>
