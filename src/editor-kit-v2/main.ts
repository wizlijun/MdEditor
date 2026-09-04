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
  materializeBlocks,
  replaceBlockSpans,
  sameBlockOrder,
  serializeSpan,
  spanContaining,
  type BlockLayout,
} from './block-layout'

export interface EditorBlock {
  blockId: string
  blockRevision: string
  markdown: string
}

export interface EditorSnapshot {
  documentId: string
  revisionId: string
  blocks: readonly EditorBlock[]
}

export interface EditorIdProvider {
  requestId(): string
  operationId(): string
}

export interface ReplaceBlockOperation {
  kind: 'block.replace'
  operationId: string
  blockId: string
  expectedBlockRevision: string
  markdown: string
}

export interface LocalOperationBatch {
  requestId: string
  baseRevisionId: string
  operations: readonly ReplaceBlockOperation[]
}

export interface AppliedChange {
  changeId: string
  originRequestId?: string
  baseRevisionId: string
  revisionId: string
  blockRevisions: Readonly<Record<string, string>>
  operations: readonly ReplaceBlockOperation[]
}

export interface ChangeError {
  code: 'stale-base' | 'invalid-operation' | 'unsupported-structure' | 'remote-base-mismatch'
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
  onBlockedStructuralEdit?: () => void
  onResyncRequired?: (reason: ChangeError) => void
}

const REMOTE_META = 'notemd-cdr-remote'
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
 * A block may cover multiple contiguous top-level editor nodes. This slice
 * still accepts only one in-place block replacement at a time. Local
 * insert/delete/move enter with the full Stage 1 operation set after the Spike
 * proves block identity and IME behaviour; unsupported structural edits fail
 * closed instead of silently losing identity.
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
  let composingBlockId: string | null = null
  let finishingComposition = false
  let finishingCompositionBlockId: string | null = null
  let compositionDirty = false
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

  function emitLocalOperations(operations: readonly ReplaceBlockOperation[]): void {
    if (!operations.length) return
    const batch: LocalOperationBatch = {
      requestId: opts.ids.requestId(),
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
      blockId,
      expectedBlockRevision: authoritative.blockRevision,
      markdown,
    }
  }

  const operationPlugin = new Plugin({
    key: new PluginKey('notemd-cdr-operations'),
    filterTransaction(transaction, state) {
      if (transaction.getMeta(REMOTE_META) || !transaction.docChanged) return true
      if (resyncRequired) return false
      const layout = layoutKey.getState(state)
      if (!layout) return false
      const changedIndices: number[] = []
      if (transaction.doc.childCount === state.doc.childCount) {
        for (let index = 0; index < state.doc.childCount; index += 1) {
          const before = state.doc.child(index)
          const after = transaction.doc.child(index)
          if (!before.eq(after)) changedIndices.push(index)
        }
      }
      const changedSpans = new Set(changedIndices.map((index) => spanContaining(layout, index)?.blockId))
      const activeCompositionBlockId = composingBlockId ?? finishingCompositionBlockId
      const isSingleBlockReplace = changedIndices.length > 0
        && changedSpans.size === 1
        && !changedSpans.has(undefined)
        && changedIndices.every((index) => state.doc.child(index).sameMarkup(transaction.doc.child(index)))
        && (activeCompositionBlockId === null || changedSpans.has(activeCompositionBlockId))
      if (pendingRequests.size === 0 && isSingleBlockReplace) return true
      if (!isSingleBlockReplace) queueMicrotask(() => opts.onBlockedStructuralEdit?.())
      return false
    },
    appendTransaction(transactions, oldState, newState) {
      const changed = transactions.some((transaction) => transaction.docChanged)
      const external = transactions.some((transaction) => transaction.getMeta(REMOTE_META))
      if (!changed || external || oldState.doc.childCount !== newState.doc.childCount) return null
      if (composingBlockId !== null || finishingComposition) {
        compositionDirty = true
        return null
      }

      const layout = layoutKey.getState(newState)
      if (!layout) return null
      let changedSpan = null as ReturnType<typeof spanContaining>
      for (let index = 0; index < newState.doc.childCount; index += 1) {
        const before = oldState.doc.child(index)
        const after = newState.doc.child(index)
        if (before.eq(after)) continue
        changedSpan = spanContaining(layout, index)
        break
      }
      const operations: ReplaceBlockOperation[] = changedSpan
        ? [localOperationFor(changedSpan.blockId, newState.doc, layout)]
          .filter((operation): operation is ReplaceBlockOperation => operation !== null)
        : []
      emitLocalOperations(operations)
      return null
    },
  })

  rich.view.updateState(rich.view.state.reconfigure({
    plugins: rich.view.state.plugins.concat(layoutPlugin, decorationPlugin, operationPlugin),
  }))
  refreshEditable()

  const blockAtSelection = () => spanContaining(
    currentLayout(),
    rich.view.state.selection.$from.index(0),
  )?.blockId ?? null
  const onCompositionStart = () => { composingBlockId = blockAtSelection() }
  const onCompositionEnd = () => {
    finishingCompositionBlockId = composingBlockId
    composingBlockId = null
    finishingComposition = true
    compositionFrame = requestAnimationFrame(() => {
      compositionFrame = null
      if (destroyed) return
      const operation = finishingCompositionBlockId === null
        ? null
        : localOperationFor(finishingCompositionBlockId)
      if (compositionDirty && operation) emitLocalOperations([operation])
      compositionDirty = false
      finishingComposition = false
      finishingCompositionBlockId = null
      requestDeferredResync()
      void flushQueuedUpdates()
    })
  }
  rich.view.dom.addEventListener('compositionstart', onCompositionStart)
  rich.view.dom.addEventListener('compositionend', onCompositionEnd)

  function replaceFromSnapshot(authoritative: EditorSnapshot, allowFullResync: boolean): void {
    const layout = currentLayout()
    if (!sameBlockOrder(layout, authoritative.blocks)) {
      if (!allowFullResync) throw new Error('EDITOR_KIT_V2_RESYNC_REQUIRED')
      const materialized = materializeBlocks(authoritative.blocks, rich.view.state.schema)
      const transaction = rich.view.state.tr
        .replaceWith(0, rich.view.state.doc.content.size, materialized.doc.content)
        .setMeta(LAYOUT_META, materialized.layout)
        .setMeta(REMOTE_META, true)
        .setMeta('addToHistory', false)
      rich.view.dispatch(transaction)
    } else {
      assertBlockIdentity(authoritative.blocks)
      const planned = replaceBlockSpans(rich.view.state.tr, layout, authoritative.blocks)
      const transaction = planned.transaction
      if (transaction.docChanged) {
        transaction.setMeta(LAYOUT_META, planned.layout)
        transaction.setMeta(REMOTE_META, true)
        transaction.setMeta('addToHistory', false)
        rich.view.dispatch(transaction)
      }
    }
    snapshot = cloneSnapshot(authoritative)
  }

  function validateRemoteChange(change: AppliedChange): ReplaceBlockOperation[] | ChangeError {
    const operationIds = change.operations.map((operation) => operation.blockId)
    const revisionIds = Object.keys(change.blockRevisions)
    if (operationIds.length === 0
      || new Set(operationIds).size !== operationIds.length
      || operationIds.length !== revisionIds.length
      || operationIds.some((blockId) => !Object.hasOwn(change.blockRevisions, blockId))) {
      throw new Error('EDITOR_KIT_V2_REMOTE_CHANGE_SHAPE')
    }
    const pending: ReplaceBlockOperation[] = []
    for (const operation of change.operations) {
      const current = snapshot.blocks.find((block) => block.blockId === operation.blockId)
      if (!current) throw new Error(`EDITOR_KIT_V2_UNKNOWN_BLOCK: ${operation.blockId}`)
      const resultingRevision = change.blockRevisions[operation.blockId]
      if (current.blockRevision === resultingRevision) {
        if (current.markdown !== operation.markdown) {
          throw new Error(`EDITOR_KIT_V2_REMOTE_REVISION_REUSED: ${operation.blockId}`)
        }
        continue
      }
      pending.push(operation)
    }
    if (pending.length === 0) return pending
    if (change.baseRevisionId !== snapshot.revisionId) {
      return {
        code: 'remote-base-mismatch',
        message: `Remote change ${change.changeId} does not extend document revision ${snapshot.revisionId}.`,
        changeId: change.changeId,
      }
    }
    for (const operation of pending) {
      const current = snapshot.blocks.find((block) => block.blockId === operation.blockId)!
      if (current.blockRevision !== operation.expectedBlockRevision) {
        return {
          code: 'remote-base-mismatch',
          message: `Remote change ${change.changeId} does not extend block ${operation.blockId}.`,
          changeId: change.changeId,
        }
      }
    }
    return pending
  }

  function applyRemote(change: AppliedChange): ChangeError | null {
    if (appliedChanges.has(change.changeId)) return null
    const validated = validateRemoteChange(change)
    if (!Array.isArray(validated)) return validated
    if (validated.length === 0) {
      appliedChanges.add(change.changeId)
      return null
    }
    const planned = replaceBlockSpans(rich.view.state.tr, currentLayout(), validated)
    const transaction = planned.transaction
    if (transaction.docChanged) {
      transaction.setMeta(LAYOUT_META, planned.layout)
      transaction.setMeta(REMOTE_META, true)
      transaction.setMeta('addToHistory', false)
      rich.view.dispatch(transaction)
    }
    snapshot = { ...snapshot, revisionId: change.revisionId }
    snapshot = {
      ...snapshot,
      blocks: snapshot.blocks.map((block) => {
        const operation = validated.find((item) => item.blockId === block.blockId)
        return operation
          ? { ...block, markdown: operation.markdown, blockRevision: change.blockRevisions[block.blockId] }
          : block
      }),
    }
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
    if (composingBlockId !== null || finishingComposition || pendingRequests.size > 0) return
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
      return
    }
    if (update.kind === 'apply-remote') {
      const touchesComposingBlock = composingBlockId !== null
        && update.change.operations.some((operation) => operation.blockId === composingBlockId)
      if (resyncRequired
        || deferredResyncReason !== null
        || pendingRequests.size > 0
        || finishingComposition
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
      if (pendingRequests.size > 0 || composingBlockId !== null || finishingComposition) {
        deferResync()
        return
      }
      replaceFromSnapshot(update.snapshot, true)
      includeChanges(update.includedChangeIds)
      resyncRequired = false
      deferredResyncReason = null
      refreshEditable()
      await flushQueuedUpdates()
      return
    }
    if (!pendingRequests.has(update.requestId)) return
    pendingRequests.delete(update.requestId)
    refreshEditable()
    replaceFromSnapshot(update.authoritative, false)
    includeChanges(update.includedChangeIds)
    requestDeferredResync()
    await flushQueuedUpdates()
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
    setReadOnly(value) {
      requestedReadOnly = value
      refreshEditable()
    },
    async destroy() {
      if (destroyed) return
      destroyed = true
      listeners.clear()
      queuedUpdates.length = 0
      if (compositionFrame !== null) cancelAnimationFrame(compositionFrame)
      rich.view.dom.removeEventListener('compositionstart', onCompositionStart)
      rich.view.dom.removeEventListener('compositionend', onCompositionEnd)
      rich.destroy()
      host.remove()
    },
  }

  return { surface, decorations }
}
