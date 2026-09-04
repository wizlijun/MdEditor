// Editor Kit v2 — experimental block-aware editing surface for governed
// documents. v1 remains frozen; this entry deliberately exposes no Markdown
// whole-document setter and no ProseMirror types to plugin consumers.

import { parseMarkdown, serializeMarkdown } from '@moraya/core'
import type { Node as PmNode, Schema } from 'prosemirror-model'
import { Plugin, PluginKey } from 'prosemirror-state'
import { Decoration, DecorationSet } from 'prosemirror-view'
import { mountRich, setKitBaseDir } from '../editor-kit/rich'
import { loadVaultRoot } from '../editor-kit/media'
import { applyKitTheme, watchKitTheme } from '../editor-kit/theme'

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
  revisionId: string
  blockRevisions: Readonly<Record<string, string>>
  operations: readonly ReplaceBlockOperation[]
}

export interface ChangeError {
  code: 'stale-base' | 'invalid-operation' | 'unsupported-structure'
  message: string
}

export type SurfaceUpdate =
  | {
      kind: 'ack-local'
      requestId: string
      authoritative: EditorSnapshot
    }
  | { kind: 'apply-remote'; change: AppliedChange }
  | {
      kind: 'proposal-stored'
      requestId: string
      changeSetId: string
      authoritative: EditorSnapshot
    }
  | {
      kind: 'reject-local'
      requestId: string
      reason: ChangeError
      authoritative: EditorSnapshot
    }
  | { kind: 'resync'; snapshot: EditorSnapshot }

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
}

const REMOTE_META = 'notemd-cdr-remote'
const DECORATION_META = 'notemd-cdr-decorations'
const decorationKey = new PluginKey<number>('notemd-cdr-decoration')

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

function snapshotMarkdown(snapshot: EditorSnapshot): string {
  return snapshot.blocks.map((block) => block.markdown.trimEnd()).join('\n\n')
}

function blockMarkdown(node: PmNode): string {
  return serializeMarkdown(node.type.schema.topNodeType.create(null, [node])).trimEnd()
}

function singleBlock(markdown: string, schema: Schema): PmNode {
  const parsed = parseMarkdown(markdown, schema)
  if (parsed.childCount !== 1) {
    throw new Error('EDITOR_KIT_V2_BLOCK_SHAPE: every block must map to one top-level editor node')
  }
  return parsed.child(0)
}

function positionAt(doc: PmNode, index: number): number {
  let position = 0
  for (let i = 0; i < index; i += 1) position += doc.child(i).nodeSize
  return position
}

function sameBlockOrder(a: readonly EditorBlock[], b: readonly EditorBlock[]): boolean {
  return a.length === b.length && a.every((block, index) => block.blockId === b[index]?.blockId)
}

function assertBlockIdentity(blocks: readonly EditorBlock[]): void {
  const blockIds = blocks.map((block) => block.blockId)
  if (blockIds.some((blockId) => blockId.length === 0) || new Set(blockIds).size !== blockIds.length) {
    throw new Error('EDITOR_KIT_V2_BLOCK_IDENTITY: block IDs must be non-empty and unique')
  }
}

/**
 * Mount the Stage 0 block-aware surface.
 *
 * This first slice intentionally accepts only one in-place top-level block
 * replacement at a time. Insert/delete/move enter with the full Stage 1
 * operation set after the Spike proves block identity and IME behaviour;
 * unsupported structural edits fail closed instead of silently losing identity.
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
  let blockOrder = snapshot.blocks.map((block) => block.blockId)
  let blockRevisions = Object.fromEntries(snapshot.blocks.map((block) => [block.blockId, block.blockRevision]))
  let requestedReadOnly = opts.readOnly === true
  let destroyed = false
  const listeners = new Set<(batch: LocalOperationBatch) => void>()
  const pendingRequests = new Map<string, LocalOperationBatch>()
  const appliedChanges = new Set<string>()
  const layers = new Map<string, readonly DecorationItem[]>()
  const queuedUpdates: Array<Extract<SurfaceUpdate, { kind: 'apply-remote' | 'resync' }>> = []
  let composingBlockId: string | null = null
  let finishingComposition = false
  let compositionDirty = false
  let compositionFrame: number | null = null

  const rich = await mountRich(
    host,
    snapshotMarkdown(snapshot),
    root,
    () => {},
    opts.placeholder,
  )

  if (rich.view.state.doc.childCount !== blockOrder.length) {
    rich.destroy()
    host.remove()
    throw new Error('EDITOR_KIT_V2_BLOCK_SHAPE: snapshot blocks do not round-trip one-to-one')
  }

  function refreshEditable(): void {
    rich.view.setProps({ editable: () => !requestedReadOnly && pendingRequests.size === 0 })
  }

  const decorationPlugin = new Plugin<number>({
    key: decorationKey,
    state: {
      init: () => 0,
      apply: (transaction, value) => transaction.getMeta(DECORATION_META) ? value + 1 : value,
    },
    props: {
      decorations(state) {
        const byBlock = new Map<string, DecorationItem[]>()
        for (const items of layers.values()) {
          for (const item of items) byBlock.set(item.blockId, [...(byBlock.get(item.blockId) ?? []), item])
        }
        const decorations: Decoration[] = []
        state.doc.forEach((node, position, index) => {
          const blockId = blockOrder[index]
          if (!blockId) return
          const items = byBlock.get(blockId) ?? []
          const attrs: Record<string, string> = { 'data-cdr-block-id': blockId }
          if (items.length) {
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

  function localOperationsAgainstSnapshot(): ReplaceBlockOperation[] {
    const operations: ReplaceBlockOperation[] = []
    for (let index = 0; index < rich.view.state.doc.childCount; index += 1) {
      const blockId = blockOrder[index]
      const authoritative = snapshot.blocks[index]
      if (!blockId || !authoritative) continue
      const markdown = blockMarkdown(rich.view.state.doc.child(index))
      if (markdown === authoritative.markdown.trimEnd()) continue
      operations.push({
        kind: 'block.replace',
        operationId: opts.ids.operationId(),
        blockId,
        expectedBlockRevision: blockRevisions[blockId],
        markdown,
      })
    }
    return operations
  }

  const operationPlugin = new Plugin({
    key: new PluginKey('notemd-cdr-operations'),
    filterTransaction(transaction, state) {
      if (transaction.getMeta(REMOTE_META) || !transaction.docChanged) return true
      const changedIndices: number[] = []
      if (transaction.doc.childCount === state.doc.childCount) {
        for (let index = 0; index < state.doc.childCount; index += 1) {
          const before = state.doc.child(index)
          const after = transaction.doc.child(index)
          if (!before.eq(after)) changedIndices.push(index)
        }
      }
      const changedIndex = changedIndices[0]
      const isSingleBlockReplace = changedIndices.length === 1
        && changedIndex !== undefined
        && state.doc.child(changedIndex).sameMarkup(transaction.doc.child(changedIndex))
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

      const operations: ReplaceBlockOperation[] = []
      for (let index = 0; index < newState.doc.childCount; index += 1) {
        const before = oldState.doc.child(index)
        const after = newState.doc.child(index)
        if (before.eq(after)) continue
        const blockId = blockOrder[index]
        if (!blockId) continue
        operations.push({
          kind: 'block.replace',
          operationId: opts.ids.operationId(),
          blockId,
          expectedBlockRevision: blockRevisions[blockId],
          markdown: blockMarkdown(after),
        })
      }
      emitLocalOperations(operations)
      return null
    },
  })

  rich.view.updateState(rich.view.state.reconfigure({
    plugins: rich.view.state.plugins.concat(decorationPlugin, operationPlugin),
  }))
  refreshEditable()

  const blockAtSelection = () => blockOrder[rich.view.state.selection.$from.index(0)] ?? null
  const onCompositionStart = () => { composingBlockId = blockAtSelection() }
  const onCompositionEnd = () => {
    composingBlockId = null
    finishingComposition = true
    compositionFrame = requestAnimationFrame(() => {
      compositionFrame = null
      if (destroyed) return
      if (compositionDirty) emitLocalOperations(localOperationsAgainstSnapshot())
      compositionDirty = false
      finishingComposition = false
      void flushQueuedUpdates()
    })
  }
  rich.view.dom.addEventListener('compositionstart', onCompositionStart)
  rich.view.dom.addEventListener('compositionend', onCompositionEnd)

  function replaceFromSnapshot(authoritative: EditorSnapshot, allowFullResync: boolean): void {
    assertBlockIdentity(authoritative.blocks)
    if (!sameBlockOrder(snapshot.blocks, authoritative.blocks)) {
      if (!allowFullResync) throw new Error('EDITOR_KIT_V2_RESYNC_REQUIRED')
      const next = parseMarkdown(snapshotMarkdown(authoritative), rich.view.state.schema)
      const transaction = rich.view.state.tr
        .replaceWith(0, rich.view.state.doc.content.size, next.content)
        .setMeta(REMOTE_META, true)
        .setMeta('addToHistory', false)
      rich.view.dispatch(transaction)
    } else {
      let transaction = rich.view.state.tr
      for (let index = authoritative.blocks.length - 1; index >= 0; index -= 1) {
        const expected = authoritative.blocks[index]
        const current = transaction.doc.child(index)
        const next = singleBlock(expected.markdown, transaction.doc.type.schema)
        if (current.eq(next)) continue
        const from = positionAt(transaction.doc, index)
        transaction = transaction.replaceWith(from, from + current.nodeSize, next)
      }
      if (transaction.docChanged) {
        transaction.setMeta(REMOTE_META, true)
        transaction.setMeta('addToHistory', false)
        rich.view.dispatch(transaction)
      }
    }
    snapshot = cloneSnapshot(authoritative)
    blockOrder = snapshot.blocks.map((block) => block.blockId)
    blockRevisions = Object.fromEntries(snapshot.blocks.map((block) => [block.blockId, block.blockRevision]))
  }

  function applyRemote(change: AppliedChange): void {
    if (appliedChanges.has(change.changeId)) return
    const alreadyReflected = Object.entries(change.blockRevisions)
      .every(([blockId, revision]) => blockRevisions[blockId] === revision)
    if (alreadyReflected) {
      appliedChanges.add(change.changeId)
      return
    }
    let transaction = rich.view.state.tr
    for (const operation of change.operations) {
      const index = blockOrder.indexOf(operation.blockId)
      if (index < 0) throw new Error(`EDITOR_KIT_V2_UNKNOWN_BLOCK: ${operation.blockId}`)
      const current = transaction.doc.child(index)
      const next = singleBlock(operation.markdown, transaction.doc.type.schema)
      if (current.eq(next)) continue
      const from = positionAt(transaction.doc, index)
      transaction = transaction.replaceWith(from, from + current.nodeSize, next)
    }
    if (transaction.docChanged) {
      transaction.setMeta(REMOTE_META, true)
      transaction.setMeta('addToHistory', false)
      rich.view.dispatch(transaction)
    }
    snapshot = { ...snapshot, revisionId: change.revisionId }
    snapshot = {
      ...snapshot,
      blocks: snapshot.blocks.map((block) => {
        const operation = change.operations.find((item) => item.blockId === block.blockId)
        return operation ? { ...block, markdown: operation.markdown } : block
      }),
    }
    blockRevisions = { ...blockRevisions, ...change.blockRevisions }
    appliedChanges.add(change.changeId)
  }

  async function flushQueuedUpdates(): Promise<void> {
    if (composingBlockId !== null || finishingComposition || pendingRequests.size > 0) return
    const pending = queuedUpdates.splice(0)
    if (!pending.length) return
    await pending.reduce(
      (previous, update) => previous.then(() => reconcile(update)),
      Promise.resolve(),
    )
  }

  async function reconcile(update: SurfaceUpdate): Promise<void> {
    if (destroyed) return
    if (update.kind === 'ack-local') {
      const batch = pendingRequests.get(update.requestId)
      if (!batch) return
      pendingRequests.delete(update.requestId)
      refreshEditable()
      replaceFromSnapshot(update.authoritative, false)
      await flushQueuedUpdates()
      return
    }
    if (update.kind === 'apply-remote') {
      const touchesComposingBlock = composingBlockId !== null
        && update.change.operations.some((operation) => operation.blockId === composingBlockId)
      if (pendingRequests.size > 0 || finishingComposition || touchesComposingBlock) {
        if (!queuedUpdates.some((item) => item.kind === 'apply-remote'
          && item.change.changeId === update.change.changeId)) queuedUpdates.push(update)
        return
      }
      applyRemote(update.change)
      return
    }
    if (update.kind === 'resync') {
      if (pendingRequests.size > 0 || composingBlockId !== null || finishingComposition) {
        const previous = queuedUpdates.findIndex((item) => item.kind === 'resync')
        if (previous >= 0) queuedUpdates.splice(previous, 1)
        queuedUpdates.push(update)
        return
      }
      replaceFromSnapshot(update.snapshot, true)
      return
    }
    if (!pendingRequests.has(update.requestId)) return
    pendingRequests.delete(update.requestId)
    refreshEditable()
    replaceFromSnapshot(update.authoritative, false)
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
