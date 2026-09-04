// Editor Kit v2 — experimental block-aware editing surface for governed
// documents. v1 remains frozen; this entry deliberately exposes no Markdown
// whole-document setter and no ProseMirror types to plugin consumers.

import { Plugin, PluginKey, TextSelection } from 'prosemirror-state'
import { Decoration, DecorationSet } from 'prosemirror-view'
import { mountRich, setKitBaseDir } from '../editor-kit/rich'
import { loadVaultRoot } from '../editor-kit/media'
import { applyKitTheme, watchKitTheme } from '../editor-kit/theme'
import {
  assertBlockIdentity,
  deleteBlockSpan,
  insertBlockSpan,
  materializeBlocks,
  planLocalBlockEdit,
  positionAtBlockStart,
  replaceBlockSpans,
  sameBlockOrder,
  serializeSpan,
  spanContaining,
  type BlockLayout,
} from './block-layout'
import { applyDocumentChange, type DocumentRevision as EditorSnapshot } from '../lib/cdr/core'
import {
  operationContentWrites,
  type AppliedChange,
  type DeleteBlockOperation,
  type InsertBlockOperation,
  type Operation,
  type OperationBatch as LocalOperationBatch,
  type ReplaceBlockOperation,
} from '../lib/cdr/operation'

export type { EditorSnapshot }
export type { AppliedChange, LocalOperationBatch, ReplaceBlockOperation }

export interface EditorIdProvider {
  requestId(): string
  operationId(): string
  blockId?(): string
}

export type StructuralCommand =
  | { kind: 'block.insert-after'; blockId: string; content: string }
  | { kind: 'block.delete'; blockId: string }

export interface ChangeError {
  code: 'stale-base' | 'invalid-operation' | 'unsupported-structure' | 'remote-base-mismatch' | 'persistence-failed'
  message: string
  changeId?: string
}

export type SurfaceUpdate =
  | {
      kind: 'ack-local'
      requestId: string
      authoritative: EditorSnapshot
      includedChangeIds: readonly string[]
    }
  | { kind: 'apply-remote'; change: AppliedChange }
  | {
      kind: 'proposal-stored'
      requestId: string
      changeSetId: string
      authoritative: EditorSnapshot
      includedChangeIds: readonly string[]
    }
  | {
      kind: 'reject-local'
      requestId: string
      reason: ChangeError
      authoritative: EditorSnapshot
      includedChangeIds: readonly string[]
    }
  | { kind: 'resync'; snapshot: EditorSnapshot; includedChangeIds: readonly string[] }

export interface DecorationItem {
  blockId: string
  kind: 'activity' | 'proposal' | 'assessment-outdated'
  label: string
}

export interface EditorSurface {
  reconcile(update: SurfaceUpdate): Promise<void>
  observeLocalOperations(listener: (batch: LocalOperationBatch) => void): () => void
  executeStructuralCommand(command: StructuralCommand): boolean
  selectedBlockId(): string | null
  setReadOnly(value: boolean): void
  destroy(): Promise<void>
}

export interface DecorationHost {
  setLayer(id: string, items: readonly DecorationItem[]): void
  removeLayer(id: string): void
}

export interface MountedDocumentEditor {
  surface: EditorSurface
  decorations: DecorationHost
}

export interface MountDocumentEditorOptions {
  snapshot: EditorSnapshot
  ids: EditorIdProvider
  readOnly?: boolean
  baseDir?: string
  placeholder?: string
  localChangeDebounceMs?: number
  onBlockedStructuralEdit?: () => void
  onResyncRequired?: (reason: ChangeError) => void
}

const REMOTE_META = 'notemd-cdr-remote'
const LOCAL_STRUCTURAL_META = 'notemd-cdr-local-structural'
const LOCAL_CONTENT_BLOCK_META = 'notemd-cdr-local-content-block'
const DECORATION_META = 'notemd-cdr-decorations'
const LAYOUT_META = 'notemd-cdr-layout'
const decorationKey = new PluginKey<number>('notemd-cdr-decoration')
const layoutKey = new PluginKey<BlockLayout>('notemd-cdr-layout')

function injectKitCss(): void {
  const href = new URL(/* @vite-ignore */ './editor-kit-v1.css', import.meta.url).href
  if (!document.querySelector('link[href$="editor-kit-v1.css"]')) {
    const link = document.createElement('link')
    link.rel = 'stylesheet'
    link.href = href
    document.head.appendChild(link)
  }
  if (document.querySelector('style[data-editor-kit-v2]')) return
  const style = document.createElement('style')
  style.dataset.editorKitV2 = ''
  style.textContent = `
    .kit-host [data-cdr-block-id] { position: relative; }
    .kit-host [data-cdr-decoration]::after {
      content: attr(data-cdr-label);
      position: absolute;
      right: 6px;
      top: 2px;
      max-width: min(46%, 340px);
      padding: 2px 7px;
      overflow: hidden;
      border-radius: 999px;
      color: #0878d1;
      background: color-mix(in srgb, #0a84ff 12%, Canvas);
      font: 600 10px/1.5 -apple-system, BlinkMacSystemFont, sans-serif;
      text-overflow: ellipsis;
      white-space: nowrap;
      pointer-events: none;
    }
    .kit-host [data-cdr-kind="proposal"] { background: color-mix(in srgb, #ff9f0a 7%, transparent); }
    .kit-host [data-cdr-kind="assessment-outdated"] { background: color-mix(in srgb, #ff3b30 6%, transparent); }
  `
  document.head.appendChild(style)
}

function joinAbsolute(root: string, relDir: string): string {
  const base = root.endsWith('/') ? root.slice(0, -1) : root
  const rel = relDir.replace(/^\/+|\/+$/g, '')
  return rel ? `${base}/${rel}` : base
}

function cloneSnapshot(snapshot: EditorSnapshot): EditorSnapshot {
  return {
    documentId: snapshot.documentId,
    revisionId: snapshot.revisionId,
    blocks: snapshot.blocks.map((block) => ({ ...block })),
  }
}

/**
 * Mount the Stage 0 block-aware surface.
 *
 * A block may cover multiple contiguous top-level editor nodes. Ordinary
 * editing inside one span emits block.replace, including Enter,
 * paragraph joins and multi-paragraph paste. Insert/delete of independently
 * governed blocks are admitted solely through explicit structural commands;
 * edits crossing block boundaries still fail closed instead of guessing
 * identity.
 */
export async function mountDocumentEditor(
  container: HTMLElement,
  opts: MountDocumentEditorOptions,
): Promise<MountedDocumentEditor> {
  assertBlockIdentity(opts.snapshot.blocks)
  injectKitCss()
  watchKitTheme()
  await applyKitTheme()

  const root = await loadVaultRoot()
  setKitBaseDir(root ? joinAbsolute(root, opts.baseDir ?? '') : '')

  const host = document.createElement('div')
  host.className = 'kit-host'
  container.appendChild(host)

  let snapshot = cloneSnapshot(opts.snapshot)
  let requestedReadOnly = opts.readOnly === true
  let destroyed = false
  const listeners = new Set<(batch: LocalOperationBatch) => void>()
  const pendingRequests = new Map<string, LocalOperationBatch>()
  const appliedChanges = new Set<string>()
  const layers = new Map<string, readonly DecorationItem[]>()
  const queuedUpdates: Array<Extract<SurfaceUpdate, { kind: 'apply-remote' }>> = []
  const dirtyLocalBlockIds = new Set<string>()
  const localChangeDebounceMs = Math.max(0, opts.localChangeDebounceMs ?? 250)
  let localChangeTimer: ReturnType<typeof setTimeout> | null = null
  let queuedStructuralCommand: StructuralCommand | null = null
  let composingBlockId: string | null = null
  let finishingComposition = false
  let finishingCompositionBlockId: string | null = null
  const compositionDirtyBlockIds = new Set<string>()
  let compositionFrame: number | null = null
  let resyncRequired = false
  let deferredResyncReason: ChangeError | null = null

  const rich = await mountRich(
    host,
    '',
    root,
    () => {},
    opts.placeholder,
  )

  let initial: ReturnType<typeof materializeBlocks>
  try {
    initial = materializeBlocks(snapshot.blocks, rich.view.state.schema)
  } catch (cause) {
    rich.destroy()
    host.remove()
    throw cause
  }
  const initialTransaction = rich.view.state.tr
    .replaceWith(0, rich.view.state.doc.content.size, initial.doc.content)
  initialTransaction
    .setSelection(TextSelection.near(initialTransaction.doc.resolve(1)))
    .setMeta(REMOTE_META, true)
    .setMeta('addToHistory', false)
  rich.view.dispatch(initialTransaction)

  const layoutPlugin = new Plugin<BlockLayout>({
    key: layoutKey,
    state: {
      init: () => initial.layout,
      apply: (transaction, value) => transaction.getMeta(LAYOUT_META) ?? value,
    },
  })

  function currentLayout(): BlockLayout {
    const layout = layoutKey.getState(rich.view.state)
    if (!layout) throw new Error('EDITOR_KIT_V2_LAYOUT_MISSING')
    return layout
  }

  function refreshEditable(): void {
    rich.view.setProps({ editable: () => !requestedReadOnly && pendingRequests.size === 0 && !resyncRequired })
  }

  function selectedBlockIdForState(state = rich.view.state): string | null {
    const layout = layoutKey.getState(state)
    if (!layout) return null
    const from = spanContaining(layout, state.selection.$from.index(0))
    const toIndex = state.selection.empty
      ? state.selection.$to.index(0)
      : state.doc.resolve(state.selection.to - 1).index(0)
    const to = spanContaining(layout, toIndex)
    return from && to && from.blockId === to.blockId ? from.blockId : null
  }

  const decorationPlugin = new Plugin<number>({
    key: decorationKey,
    state: {
      init: () => 0,
      apply: (transaction, value) => transaction.getMeta(DECORATION_META) ? value + 1 : value,
    },
    props: {
      decorations(state) {
        const layout = layoutKey.getState(state)
        if (!layout) return DecorationSet.empty
        const byBlock = new Map<string, DecorationItem[]>()
        for (const items of layers.values()) {
          for (const item of items) byBlock.set(item.blockId, [...(byBlock.get(item.blockId) ?? []), item])
        }
        const decorations: Decoration[] = []
        state.doc.forEach((node, position, index) => {
          const span = spanContaining(layout, index)
          if (!span) return
          const items = byBlock.get(span.blockId) ?? []
          const attrs: Record<string, string> = { 'data-cdr-block-id': span.blockId }
          if (items.length && index === span.startIndex) {
            attrs['data-cdr-decoration'] = 'true'
            attrs['data-cdr-kind'] = items.at(-1)!.kind
            attrs['data-cdr-label'] = items.map((item) => item.label).join(' · ')
          }
          decorations.push(Decoration.node(position, position + node.nodeSize, attrs))
        })
        return DecorationSet.create(state.doc, decorations)
      },
    },
  })

  function emitLocalOperations(operations: readonly Operation[]): void {
    if (!operations.length) return
    const batch: LocalOperationBatch = {
      requestId: opts.ids.requestId(),
      documentId: snapshot.documentId,
      baseRevisionId: snapshot.revisionId,
      operations,
    }
    pendingRequests.set(batch.requestId, batch)
    queueMicrotask(() => {
      if (destroyed) return
      refreshEditable()
      for (const listener of listeners) listener(batch)
    })
  }

  function flushLocalOperations(
    doc = rich.view.state.doc,
    layout = currentLayout(),
  ): void {
    if (localChangeTimer !== null) {
      clearTimeout(localChangeTimer)
      localChangeTimer = null
    }
    if (dirtyLocalBlockIds.size === 0
      || pendingRequests.size > 0
      || resyncRequired
      || composingBlockId !== null
      || finishingComposition) return
    const operations = [...dirtyLocalBlockIds]
      .map((blockId) => localOperationFor(blockId, doc, layout))
      .filter((operation): operation is ReplaceBlockOperation => operation !== null)
    dirtyLocalBlockIds.clear()
    if (operations.length === 0) {
      requestDeferredResync()
      void flushQueuedUpdates()
      runQueuedStructuralCommand()
      return
    }
    emitLocalOperations(operations)
  }

  function scheduleLocalOperations(
    blockIds: readonly string[],
    immediateDoc = rich.view.state.doc,
    immediateLayout = currentLayout(),
  ): void {
    for (const blockId of blockIds) dirtyLocalBlockIds.add(blockId)
    if (localChangeTimer !== null) clearTimeout(localChangeTimer)
    if (localChangeDebounceMs === 0) {
      localChangeTimer = null
      flushLocalOperations(immediateDoc, immediateLayout)
      return
    }
    localChangeTimer = setTimeout(() => {
      localChangeTimer = null
      flushLocalOperations()
    }, localChangeDebounceMs)
  }

  function localOperationFor(
    blockId: string,
    doc = rich.view.state.doc,
    layout = currentLayout(),
  ): ReplaceBlockOperation | null {
    const span = layout.spans.find((item) => item.blockId === blockId)
    const authoritative = snapshot.blocks.find((block) => block.blockId === blockId)
    if (!span || !authoritative) return null
    const markdown = serializeSpan(doc, span)
    if (markdown === authoritative.markdown.trimEnd()) return null
    return {
      kind: 'block.replace',
      operationId: opts.ids.operationId(),
      target: { blockId, expectedBlockRevision: authoritative.blockRevision },
      payload: { content: markdown },
    }
  }

  const operationPlugin = new Plugin({
    key: new PluginKey('notemd-cdr-operations'),
    filterTransaction(transaction, state) {
      if (transaction.getMeta(REMOTE_META) || transaction.getMeta(LOCAL_STRUCTURAL_META) || !transaction.docChanged) return true
      if (resyncRequired) return false
      const layout = layoutKey.getState(state)
      if (!layout) return false
      const preferredBlockId = selectedBlockIdForState(state)
      const plan = planLocalBlockEdit(state.doc, transaction.doc, layout, preferredBlockId)
      const activeCompositionBlockId = composingBlockId ?? finishingCompositionBlockId
      const span = plan?.layout.spans.find((item) => item.blockId === plan.blockId)
      const hasContent = span ? serializeSpan(transaction.doc, span).trim().length > 0 : false
      const isSingleBlockReplace = plan !== null
        && hasContent
        && (activeCompositionBlockId === null || plan.blockId === activeCompositionBlockId)
      if (pendingRequests.size === 0 && isSingleBlockReplace && plan) {
        transaction.setMeta(LAYOUT_META, plan.layout)
        transaction.setMeta(LOCAL_CONTENT_BLOCK_META, plan.blockId)
        return true
      }
      if (!isSingleBlockReplace) queueMicrotask(() => opts.onBlockedStructuralEdit?.())
      return false
    },
    appendTransaction(transactions, _oldState, newState) {
      const changed = transactions.some((transaction) => transaction.docChanged)
      const external = transactions.some((transaction) => transaction.getMeta(REMOTE_META))
      const structuralCommand = transactions.some((transaction) => transaction.getMeta(LOCAL_STRUCTURAL_META))
      if (!changed || external || structuralCommand) return null
      const changedBlockIds = [...new Set(transactions.flatMap((transaction) => {
        const blockId = transaction.getMeta(LOCAL_CONTENT_BLOCK_META)
        return typeof blockId === 'string' ? [blockId] : []
      }))]
      if (changedBlockIds.length === 0) return null
      if (composingBlockId !== null || finishingComposition) {
        for (const blockId of changedBlockIds) compositionDirtyBlockIds.add(blockId)
        return null
      }
      const layout = layoutKey.getState(newState)
      if (!layout) return null
      scheduleLocalOperations(changedBlockIds, newState.doc, layout)
      return null
    },
  })

  rich.view.updateState(rich.view.state.reconfigure({
    plugins: rich.view.state.plugins.concat(layoutPlugin, decorationPlugin, operationPlugin),
  }))
  refreshEditable()

  const blockAtSelection = () => selectedBlockIdForState()
  const promoteFinishedComposition = () => {
    if (compositionFrame !== null) cancelAnimationFrame(compositionFrame)
    compositionFrame = null
    const blockId = finishingCompositionBlockId
    if (blockId !== null && compositionDirtyBlockIds.delete(blockId) && localOperationFor(blockId)) {
      dirtyLocalBlockIds.add(blockId)
    }
    finishingComposition = false
    finishingCompositionBlockId = null
  }
  const onCompositionStart = () => {
    if (finishingComposition) promoteFinishedComposition()
    composingBlockId = blockAtSelection()
  }
  const onCompositionEnd = () => {
    finishingCompositionBlockId = composingBlockId
    composingBlockId = null
    finishingComposition = true
    compositionFrame = requestAnimationFrame(() => {
      compositionFrame = null
      if (destroyed) return
      promoteFinishedComposition()
      if (dirtyLocalBlockIds.size > 0) scheduleLocalOperations([])
      requestDeferredResync()
      void flushQueuedUpdates()
      runQueuedStructuralCommand()
    })
  }
  rich.view.dom.addEventListener('compositionstart', onCompositionStart)
  rich.view.dom.addEventListener('compositionend', onCompositionEnd)
  const onEditorBlur = () => flushLocalOperations()
  rich.view.dom.addEventListener('blur', onEditorBlur, true)

  function replaceFromSnapshot(authoritative: EditorSnapshot, allowFullResync: boolean): void {
    assertBlockIdentity(authoritative.blocks)
    let layout = currentLayout()
    let transaction = rich.view.state.tr
    if (!sameBlockOrder(layout, authoritative.blocks)) {
      const currentIds = layout.spans.map((span) => span.blockId)
      const nextIds = authoritative.blocks.map((block) => block.blockId)
      const removed = currentIds.filter((blockId) => !nextIds.includes(blockId))
      const added = nextIds.filter((blockId) => !currentIds.includes(blockId))
      if (removed.length === 1 && added.length === 0 && currentIds.length === nextIds.length + 1) {
        const planned = deleteBlockSpan(transaction, layout, removed[0])
        transaction = planned.transaction
        layout = planned.layout
      } else if (added.length === 1 && removed.length === 0 && nextIds.length === currentIds.length + 1) {
        const block = authoritative.blocks.find((item) => item.blockId === added[0])!
        const planned = insertBlockSpan(transaction, layout, nextIds.indexOf(added[0]), block)
        transaction = planned.transaction
        layout = planned.layout
      } else {
        if (!allowFullResync) throw new Error('EDITOR_KIT_V2_RESYNC_REQUIRED')
        const materialized = materializeBlocks(authoritative.blocks, rich.view.state.schema)
        transaction = transaction.replaceWith(0, transaction.doc.content.size, materialized.doc.content)
        layout = materialized.layout
      }
    }
    const replaced = replaceBlockSpans(transaction, layout, authoritative.blocks)
    transaction = replaced.transaction
    layout = replaced.layout
    if (transaction.docChanged) {
      transaction.setMeta(LAYOUT_META, layout)
      transaction.setMeta(REMOTE_META, true)
      transaction.setMeta('addToHistory', false)
      rich.view.dispatch(transaction)
    }
    snapshot = cloneSnapshot(authoritative)
  }

  function nextSnapshotForRemote(change: AppliedChange): EditorSnapshot | ChangeError {
    if (!change.operations.length || change.baseRevisionId !== snapshot.revisionId) {
      return {
        code: 'remote-base-mismatch',
        message: `Remote change ${change.changeId} does not extend document revision ${snapshot.revisionId}.`,
        changeId: change.changeId,
      }
    }
    for (const write of change.operations.flatMap(operationContentWrites)) {
      const current = snapshot.blocks.find((block) => block.blockId === write.blockId)
      if (current?.blockRevision === change.blockRevisions[write.blockId] && current.markdown !== write.content) {
        throw new Error(`EDITOR_KIT_V2_REMOTE_REVISION_REUSED: ${write.blockId}`)
      }
    }
    try {
      return applyDocumentChange(
        snapshot,
        {
          requestId: change.originRequestId ?? change.changeId,
          documentId: snapshot.documentId,
          baseRevisionId: change.baseRevisionId,
          operations: change.operations,
        },
        { revisionId: change.revisionId, blockRevisions: change.blockRevisions },
      )
    } catch {
      return {
        code: 'remote-base-mismatch',
        message: `Remote change ${change.changeId} is not valid for the current document state.`,
        changeId: change.changeId,
      }
    }
  }

  function applyRemote(change: AppliedChange): ChangeError | null {
    if (appliedChanges.has(change.changeId)) return null
    const next = nextSnapshotForRemote(change)
    if ('code' in next) return next
    replaceFromSnapshot(next, false)
    appliedChanges.add(change.changeId)
    return null
  }

  function includeChanges(changeIds: readonly string[]): void {
    for (const changeId of changeIds) appliedChanges.add(changeId)
    for (let index = queuedUpdates.length - 1; index >= 0; index -= 1) {
      const update = queuedUpdates[index]
      if (update.kind === 'apply-remote' && appliedChanges.has(update.change.changeId)) {
        queuedUpdates.splice(index, 1)
      }
    }
  }

  function queueUpdate(update: Extract<SurfaceUpdate, { kind: 'apply-remote' }>): void {
    if (!queuedUpdates.some((item) => item.kind === 'apply-remote'
      && item.change.changeId === update.change.changeId)) queuedUpdates.push(update)
  }

  function requireResync(reason: ChangeError): void {
    if (resyncRequired) return
    resyncRequired = true
    refreshEditable()
    opts.onResyncRequired?.(reason)
  }

  function deferResync(): void {
    deferredResyncReason = {
      code: 'remote-base-mismatch',
      message: 'A resync snapshot arrived during an uncommitted local edit; fetch the head again.',
    }
  }

  function requestDeferredResync(): void {
    if (!deferredResyncReason
      || pendingRequests.size > 0
      || dirtyLocalBlockIds.size > 0
      || composingBlockId !== null
      || finishingComposition) return
    resyncRequired = true
    refreshEditable()
    if (!opts.onResyncRequired) return
    const reason = deferredResyncReason
    deferredResyncReason = null
    opts.onResyncRequired(reason)
  }

  async function flushQueuedUpdates(): Promise<void> {
    if (composingBlockId !== null
      || finishingComposition
      || pendingRequests.size > 0
      || dirtyLocalBlockIds.size > 0) return
    if (resyncRequired) return
    while (queuedUpdates.length > 0) {
      const update = queuedUpdates[0]
      const mismatch = applyRemote(update.change)
      if (mismatch) {
        requireResync(mismatch)
        return
      }
      queuedUpdates.shift()
    }
  }

  async function reconcile(update: SurfaceUpdate): Promise<void> {
    if (destroyed) return
    if (update.kind === 'ack-local') {
      const batch = pendingRequests.get(update.requestId)
      if (!batch) return
      pendingRequests.delete(update.requestId)
      refreshEditable()
      replaceFromSnapshot(update.authoritative, false)
      includeChanges(update.includedChangeIds)
      requestDeferredResync()
      await flushQueuedUpdates()
      runQueuedStructuralCommand()
      return
    }
    if (update.kind === 'apply-remote') {
      const touchesComposingBlock = composingBlockId !== null
        && update.change.operations.some((operation) => (
          operation.kind !== 'block.insert' && operation.target.blockId === composingBlockId
        ))
      const hasStructuralOperation = update.change.operations.some((operation) => operation.kind !== 'block.replace')
      if (resyncRequired
        || deferredResyncReason !== null
        || pendingRequests.size > 0
        || dirtyLocalBlockIds.size > 0
        || finishingComposition
        || (composingBlockId !== null && hasStructuralOperation)
        || touchesComposingBlock) {
        queueUpdate(update)
        return
      }
      const mismatch = applyRemote(update.change)
      if (mismatch) {
        queueUpdate(update)
        requireResync(mismatch)
      }
      return
    }
    if (update.kind === 'resync') {
      if (pendingRequests.size > 0
        || dirtyLocalBlockIds.size > 0
        || composingBlockId !== null
        || finishingComposition) {
        deferResync()
        return
      }
      replaceFromSnapshot(update.snapshot, true)
      includeChanges(update.includedChangeIds)
      resyncRequired = false
      deferredResyncReason = null
      refreshEditable()
      await flushQueuedUpdates()
      runQueuedStructuralCommand()
      return
    }
    if (!pendingRequests.has(update.requestId)) return
    pendingRequests.delete(update.requestId)
    queuedStructuralCommand = null
    refreshEditable()
    replaceFromSnapshot(update.authoritative, false)
    includeChanges(update.includedChangeIds)
    requestDeferredResync()
    await flushQueuedUpdates()
  }

  function executeStructuralCommandNow(command: StructuralCommand): boolean {
    if (command.kind === 'block.insert-after') {
      if (!command.content.trim()) return false
      const anchorIndex = snapshot.blocks.findIndex((block) => block.blockId === command.blockId)
      if (anchorIndex < 0) return false
      const candidateBlockId = opts.ids.blockId?.()
      if (!candidateBlockId) return false
      const operation: InsertBlockOperation = {
        kind: 'block.insert',
        operationId: opts.ids.operationId(),
        target: {
          leftBlockId: snapshot.blocks[anchorIndex].blockId,
          rightBlockId: snapshot.blocks[anchorIndex + 1]?.blockId ?? null,
        },
        payload: { candidateBlockId, content: command.content },
      }
      const planned = insertBlockSpan(
        rich.view.state.tr,
        currentLayout(),
        anchorIndex + 1,
        { blockId: candidateBlockId, markdown: command.content },
      )
      const selectionPosition = positionAtBlockStart(planned.transaction.doc, planned.layout, candidateBlockId)
      planned.transaction
        .setSelection(TextSelection.near(planned.transaction.doc.resolve(selectionPosition + 1)))
        .setMeta(LAYOUT_META, planned.layout)
        .setMeta(LOCAL_STRUCTURAL_META, true)
        .setMeta('addToHistory', false)
      rich.view.dispatch(planned.transaction)
      emitLocalOperations([operation])
      return true
    }

    const block = snapshot.blocks.find((item) => item.blockId === command.blockId)
    if (!block || snapshot.blocks.length === 1) return false
    const operation: DeleteBlockOperation = {
      kind: 'block.delete',
      operationId: opts.ids.operationId(),
      target: { blockId: block.blockId, expectedBlockRevision: block.blockRevision },
      payload: {},
    }
    const planned = deleteBlockSpan(rich.view.state.tr, currentLayout(), block.blockId)
    planned.transaction
      .setMeta(LAYOUT_META, planned.layout)
      .setMeta(LOCAL_STRUCTURAL_META, true)
      .setMeta('addToHistory', false)
    rich.view.dispatch(planned.transaction)
    emitLocalOperations([operation])
    return true
  }

  function runQueuedStructuralCommand(): void {
    if (!queuedStructuralCommand) return
    if (destroyed || requestedReadOnly) {
      queuedStructuralCommand = null
      if (!destroyed) opts.onBlockedStructuralEdit?.()
      return
    }
    if (pendingRequests.size > 0
      || dirtyLocalBlockIds.size > 0
      || resyncRequired
      || composingBlockId !== null
      || finishingComposition) return
    const command = queuedStructuralCommand
    queuedStructuralCommand = null
    if (!executeStructuralCommandNow(command)) opts.onBlockedStructuralEdit?.()
  }

  function executeStructuralCommand(command: StructuralCommand): boolean {
    if (destroyed
      || requestedReadOnly
      || resyncRequired
      || queuedStructuralCommand !== null) return false
    if (command.kind === 'block.insert-after') {
      if (!command.content.trim() || !snapshot.blocks.some((block) => block.blockId === command.blockId)) return false
    } else if (!snapshot.blocks.some((block) => block.blockId === command.blockId)
      || snapshot.blocks.length === 1) return false

    if (dirtyLocalBlockIds.size > 0
      || pendingRequests.size > 0
      || composingBlockId !== null
      || finishingComposition) {
      queuedStructuralCommand = { ...command }
      if (dirtyLocalBlockIds.size > 0) flushLocalOperations()
      if (pendingRequests.size === 0) runQueuedStructuralCommand()
      return true
    }
    return executeStructuralCommandNow(command)
  }

  const decorations: DecorationHost = {
    setLayer(id, items) {
      layers.set(id, items.map((item) => ({ ...item })))
      rich.view.dispatch(rich.view.state.tr.setMeta(DECORATION_META, true))
    },
    removeLayer(id) {
      if (!layers.delete(id)) return
      rich.view.dispatch(rich.view.state.tr.setMeta(DECORATION_META, true))
    },
  }

  const surface: EditorSurface = {
    reconcile,
    observeLocalOperations(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    executeStructuralCommand,
    selectedBlockId: blockAtSelection,
    setReadOnly(value) {
      if (value && dirtyLocalBlockIds.size > 0) flushLocalOperations()
      requestedReadOnly = value
      if (value) queuedStructuralCommand = null
      refreshEditable()
    },
    async destroy() {
      if (destroyed) return
      if (compositionFrame !== null) cancelAnimationFrame(compositionFrame)
      compositionFrame = null
      for (const blockId of compositionDirtyBlockIds) {
        if (localOperationFor(blockId)) dirtyLocalBlockIds.add(blockId)
      }
      compositionDirtyBlockIds.clear()
      composingBlockId = null
      finishingComposition = false
      finishingCompositionBlockId = null
      flushLocalOperations()
      await Promise.resolve()
      destroyed = true
      queuedStructuralCommand = null
      listeners.clear()
      queuedUpdates.length = 0
      dirtyLocalBlockIds.clear()
      if (localChangeTimer !== null) clearTimeout(localChangeTimer)
      rich.view.dom.removeEventListener('compositionstart', onCompositionStart)
      rich.view.dom.removeEventListener('compositionend', onCompositionEnd)
      rich.view.dom.removeEventListener('blur', onEditorBlur, true)
      rich.destroy()
      host.remove()
    },
  }

  return { surface, decorations }
}
