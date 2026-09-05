// Generic governed-document surface. ProseMirror owns the editable draft and
// history; Core owns versioned atomic operations. No Markdown whole-doc setter.
import { EditorState, Plugin, PluginKey, TextSelection } from 'prosemirror-state'
import { closeHistory } from 'prosemirror-history'
import { Decoration, DecorationSet } from 'prosemirror-view'
import { mountRich, setKitBaseDir } from '../editor-kit/rich'
import { loadVaultRoot } from '../editor-kit/media'
import { applyKitTheme, watchKitTheme } from '../editor-kit/theme'
import { applyDocumentChange } from '../lib/cdr/core'
import { operationContentWrites } from '../lib/cdr/operation'
import {
  assertBlockIdentity, deleteBlockSpan, insertBlockSpan, materializeBlocks,
  positionAtBlockStart, replaceBlockSpans, serializeSpan, spanContaining,
  type BlockLayout, type LayoutBlock,
} from './block-layout'
import { normalizeBlockIdentity, readBlockLayout, withBlockIdentitySchema, withoutBlockIdentity } from './identity'
import { mergeAcknowledgedDraft, operationsForDraft, sameDocument } from './document-diff'
import { canExecuteEditorCommand, executeEditorCommand } from './commands'
import { governedRedo, governedUndo, withGovernedHistory } from './governed-history'
import { isApplePlatformSync } from '../lib/platform-sync'
import type {
  AppliedChange, ChangeError, DecorationHost, DecorationItem, EditorCommand, EditorSnapshot,
  EditorSurface, EditorSurfaceState, LocalOperationBatch, MountDocumentEditorOptions,
  MountedDocumentEditor, StructuralCommand, SurfaceUpdate,
} from './contract'

export type * from './contract'
/** Paired with the Memory consumer; older v2 bundles must fail visibly. */
export const documentEditorApiVersion = 2

const REMOTE_META = 'notemd-cdr-remote'
const DECORATION_META = 'notemd-cdr-decorations'
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
      content: attr(data-cdr-label); position: absolute; right: 6px; top: 2px;
      max-width: min(46%, 340px); padding: 2px 7px; overflow: hidden;
      border-radius: 999px; color: #0878d1;
      background: color-mix(in srgb, #0a84ff 12%, Canvas);
      font: 600 10px/1.5 -apple-system, BlinkMacSystemFont, sans-serif;
      text-overflow: ellipsis; white-space: nowrap; pointer-events: none;
    }
    .kit-host [data-cdr-kind="proposal"] { background: color-mix(in srgb, #ff9f0a 7%, transparent); }
    .kit-host [data-cdr-kind="assessment-outdated"] { background: color-mix(in srgb, #ff3b30 6%, transparent); }
  `
  document.head.appendChild(style)
}

function cloneSnapshot(snapshot: EditorSnapshot): EditorSnapshot {
  return { ...snapshot, blocks: snapshot.blocks.map((block) => ({ ...block })) }
}

interface PendingRequest {
  batch: LocalOperationBatch
  base: EditorSnapshot
  draft: LayoutBlock[]
}

export async function mountDocumentEditor(
  container: HTMLElement,
  opts: MountDocumentEditorOptions,
): Promise<MountedDocumentEditor> {
  assertBlockIdentity(opts.snapshot.blocks)
  injectKitCss()
  watchKitTheme()
  await applyKitTheme()
  const root = await loadVaultRoot()
  setKitBaseDir(root ? `${root.replace(/\/$/, '')}/${(opts.baseDir ?? '').replace(/^\/+|\/+$/g, '')}` : '')
  const host = document.createElement('div')
  host.className = 'kit-host'
  container.appendChild(host)

  let snapshot = cloneSnapshot(opts.snapshot)
  const history = [cloneSnapshot(snapshot)]
  const listeners = new Set<(batch: LocalOperationBatch) => void>()
  const stateListeners = new Set<(state: EditorSurfaceState) => void>()
  const waiters = new Set<{ resolve(): void; reject(reason: Error): void }>()
  const appliedChanges = new Set<string>()
  const queuedChanges: AppliedChange[] = []
  const compositionReceipts: Array<Exclude<SurfaceUpdate, { kind: 'apply-remote' | 'resync' }>> = []
  const layers = new Map<string, readonly DecorationItem[]>()
  const recoveryKey = `notemd-cdr-draft:${snapshot.documentId}`
  let requestedReadOnly = opts.readOnly === true
  let destroyed = false
  let closing: Promise<void> | null = null
  let pending: PendingRequest | null = null
  let failedRequest: PendingRequest | null = null
  let error: ChangeError | null = null
  let retainedDraft: LayoutBlock[] | null = null
  let resyncRequired = false
  let deferredResync = false
  let composing = false
  let composingBlockId: string | null = null
  let compositionFrame: number | null = null
  let finishingComposition = false
  let applyingRemote = false
  let localTimer: ReturnType<typeof setTimeout> | null = null
  let queuedMicrotask = false
  let freshCounter = 0
  const canonicalContent = new Map<string, string>()

  const rich = await mountRich(host, '', root, () => {}, opts.placeholder, undefined, withBlockIdentitySchema)
  const freshId = () => opts.ids.blockId?.() ?? `block-${crypto.randomUUID?.() ?? `${Date.now()}-${++freshCounter}`}`
  const initial = materializeBlocks(snapshot.blocks, rich.view.state.schema)
  rich.view.dispatch(rich.view.state.tr.replaceWith(0, rich.view.state.doc.content.size, initial.doc.content)
    .setMeta(REMOTE_META, true).setMeta('addToHistory', false))

  const currentLayout = () => readBlockLayout(rich.view.state.doc)
  const draftBlocks = (): LayoutBlock[] => currentLayout().spans.map((span) => {
    const markdown = serializeSpan(rich.view.state.doc, span)
    const original = snapshot.blocks.find((block) => block.blockId === span.blockId)
    if (original) {
      const key = `${original.blockId}/${original.blockRevision}`
      if (!canonicalContent.has(key)) {
        const parsed = materializeBlocks([original], rich.view.state.schema)
        canonicalContent.set(key, serializeSpan(parsed.doc, parsed.layout.spans[0]))
      }
      if (markdown === canonicalContent.get(key)) return { blockId: span.blockId, markdown: original.markdown }
    }
    return { blockId: span.blockId, markdown }
  })
  const isDirty = () => !sameDocument(draftBlocks(), snapshot.blocks)
  const inComposition = () => composing || finishingComposition
  const isReadOnly = () => requestedReadOnly || resyncRequired || retainedDraft !== null
  const blockAtSelection = (): string | null => {
    const state = rich.view.state
    const layout = currentLayout()
    const left = spanContaining(layout, state.selection.$from.index(0))
    const to = state.selection.empty ? state.selection.to : Math.max(state.selection.from, state.selection.to - 1)
    const right = spanContaining(layout, state.doc.resolve(to).index(0))
    return left && right && left.blockId === right.blockId ? left.blockId : null
  }
  const currentState = (): EditorSurfaceState => ({
    dirty: isDirty(), saving: pending !== null, readOnly: isReadOnly(),
    error: error ? { ...error } : null, selectedBlockId: blockAtSelection(),
  })
  function notifyState(): void {
    if (destroyed) return
    const state = currentState()
    for (const listener of stateListeners) listener(state)
    if (error || state.readOnly || (!state.dirty && !state.saving && !inComposition())) {
      for (const waiter of waiters) {
        if (error) waiter.reject(new Error(error.message))
        else if (state.readOnly && (state.dirty || state.saving)) waiter.reject(new Error('The document is read-only; pending input is retained as a draft.'))
        else waiter.resolve()
      }
      waiters.clear()
    }
  }
  function refreshEditable(): void {
    // A disk round trip must never blur the editor or veto subsequent typing.
    rich.view.setProps({ editable: () => !isReadOnly() })
  }
  function remember(revision: EditorSnapshot): void {
    if (!history.some((item) => item.revisionId === revision.revisionId)) history.push(cloneSnapshot(revision))
  }
  function saveRecovery(): void {
    try {
      const draft = retainedDraft ?? draftBlocks()
      if (!sameDocument(draft, snapshot.blocks)) {
        // This is a recovery copy, never a committed read or a second write API.
        const missing = new Set(draft.filter((block) => !snapshot.blocks.some((item) => item.blockId === block.blockId)).map((block) => block.blockId))
        const restoreSources: EditorSnapshot[] = []
        for (const id of missing) {
          const source = [...history].reverse().find((revision) => revision.blocks.some((block) => block.blockId === id))
          if (source && !restoreSources.includes(source)) restoreSources.push(source)
        }
        localStorage.setItem(recoveryKey, JSON.stringify({ base: snapshot, draft, restoreSources,
          blocked: retainedDraft !== null || (error !== null && error.code !== 'persistence-failed'), error }))
      } else localStorage.removeItem(recoveryKey)
    } catch {
      // Editing and the durable repository still work when WebView storage is
      // unavailable. The visible draft/export remains the recovery source.
    }
  }
  function clearTimer(): void {
    if (localTimer !== null) clearTimeout(localTimer)
    localTimer = null
  }
  function setFailure(reason: ChangeError): void {
    error = reason
    clearTimer()
    refreshEditable()
    saveRecovery()
    notifyState()
  }
  function queueLocalWork(): void {
    if (queuedMicrotask || destroyed) return
    queuedMicrotask = true
    queueMicrotask(() => {
      queuedMicrotask = false
      if (destroyed) return
      saveRecovery()
      notifyState()
      scheduleFlush()
    })
  }
  const identityPlugin = new Plugin<BlockLayout>({
    key: layoutKey,
    state: { init: () => initial.layout, apply: (tr) => readBlockLayout(tr.doc) },
    props: {
      transformPasted: withoutBlockIdentity,
      handleKeyDown(view, event) {
        const modifier = isApplePlatformSync() ? event.metaKey : event.ctrlKey
        if (modifier && !event.altKey && !event.isComposing && !inComposition()
          && (event.key.toLowerCase() === 'z' || event.key.toLowerCase() === 'y')) {
          if (!isReadOnly()) {
            const command = event.key.toLowerCase() === 'y' || event.shiftKey ? governedRedo : governedUndo
            command(view.state, view.dispatch, view)
          }
          // Always consume: a rejected governed inverse must not fall through
          // to Moraya's unguarded native history shortcut.
          return true
        }
        if (event.key !== 'Tab' || event.isComposing || inComposition() || isReadOnly()) return false
        return executeEditorCommand(view, { kind: event.shiftKey ? 'table.previous-cell' : 'table.next-cell' })
      },
      decorations(state) {
        const byBlock = new Map<string, DecorationItem[]>()
        for (const items of layers.values()) {
          for (const item of items) byBlock.set(item.blockId, [...(byBlock.get(item.blockId) ?? []), item])
        }
        const decorations: Decoration[] = []
        const layout = readBlockLayout(state.doc)
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
    filterTransaction(tr, state) {
      if (!tr.docChanged) return true
      if (!tr.getMeta(REMOTE_META) && isReadOnly()) return false
      // Normalize before any plugin (especially history) records this edit.
      // Attribute steps stay in the very same input transaction and undo group.
      const after = EditorState.create({ schema: state.schema, doc: tr.doc, selection: tr.selection })
      const normalized = normalizeBlockIdentity(after, state, freshId, tr.mapping)
      for (const step of normalized?.steps ?? []) tr.step(step)
      return true
    },
    view() {
      return {
        update(view, previous) {
          if (!view.state.doc.eq(previous.doc) && !applyingRemote) queueLocalWork()
          notifyState()
        },
      }
    },
  })
  rich.view.updateState(rich.view.state.reconfigure({
    plugins: [identityPlugin, ...withGovernedHistory(rich.view.state.plugins, (tr) => !!tr.getMeta(REMOTE_META))],
  }))
  refreshEditable()

  function emit(request: PendingRequest): void {
    pending = request
    notifyState()
    for (const listener of listeners) listener(request.batch)
  }
  function flushLocal(): void {
    clearTimer()
    if (destroyed || pending || error || resyncRequired || inComposition()) return
    const draft = draftBlocks()
    const operations = operationsForDraft(snapshot, draft, history, opts.ids.operationId)
    if (!operations.length) {
      drainRemote()
      requestDeferredResync()
      saveRecovery()
      notifyState()
      return
    }
    emit({
      batch: { requestId: opts.ids.requestId(), documentId: snapshot.documentId,
        baseRevisionId: snapshot.revisionId, operations },
      base: cloneSnapshot(snapshot), draft,
    })
  }
  function scheduleFlush(): void {
    clearTimer()
    if (pending || error || inComposition() || resyncRequired || destroyed) return
    if (!isDirty()) {
      drainRemote()
      requestDeferredResync()
      notifyState()
      return
    }
    const delay = Math.max(0, opts.localChangeDebounceMs ?? 250)
    if (delay === 0) flushLocal()
    else localTimer = setTimeout(flushLocal, delay)
  }

  /** Reconcile by block-local deletes/inserts/replacements, including reorders. */
  function syncBlocks(blocks: readonly LayoutBlock[], local = false): void {
    assertBlockIdentity(blocks)
    if (sameDocument(draftBlocks(), blocks)) return
    let layout = currentLayout()
    let tr = rich.view.state.tr
    // Keep a temporary anchor when all old identities are being replaced.
    const wanted = new Set(blocks.map((block) => block.blockId))
    for (const span of [...layout.spans].reverse()) {
      if (wanted.has(span.blockId) || layout.spans.length === 1) continue
      const result = deleteBlockSpan(tr, layout, span.blockId)
      tr = result.transaction
      layout = result.layout
    }
    for (let i = 0; i < blocks.length; i += 1) {
      const block = blocks[i]
      const oldIndex = layout.spans.findIndex((span) => span.blockId === block.blockId)
      if (oldIndex === i) continue
      if (oldIndex >= 0) {
        const result = deleteBlockSpan(tr, layout, block.blockId)
        tr = result.transaction
        layout = result.layout
      }
      const result = insertBlockSpan(tr, layout, i, block)
      tr = result.transaction
      layout = result.layout
    }
    for (const span of [...layout.spans].reverse()) {
      if (wanted.has(span.blockId)) continue
      const result = deleteBlockSpan(tr, layout, span.blockId)
      tr = result.transaction
      layout = result.layout
    }
    tr = replaceBlockSpans(tr, layout, blocks).transaction
    if (!tr.docChanged) return
    if (!local) tr.setMeta(REMOTE_META, true).setMeta('addToHistory', false)
    else closeHistory(tr)
    applyingRemote = !local
    try { rich.view.dispatch(tr) } finally { applyingRemote = false }
  }
  function includeChanges(ids: readonly string[]): void {
    for (const id of ids) appliedChanges.add(id)
    for (let i = queuedChanges.length - 1; i >= 0; i -= 1) {
      if (appliedChanges.has(queuedChanges[i].changeId)) queuedChanges.splice(i, 1)
    }
  }
  function installAuthoritative(authoritative: EditorSnapshot, base: readonly LayoutBlock[]): void {
    const local = draftBlocks()
    let merged: LayoutBlock[]
    try {
      merged = mergeAcknowledgedDraft(base, local, authoritative.blocks)
    } catch (cause) {
      snapshot = cloneSnapshot(authoritative)
      remember(snapshot)
      retainedDraft = local
      setFailure({ code: 'stale-base', message: String(cause) })
      return
    }
    syncBlocks(merged)
    snapshot = cloneSnapshot(authoritative)
    remember(snapshot)
  }
  function requireResync(reason: ChangeError): void {
    if (pending || isDirty() || inComposition()) {
      deferredResync = true
      return
    }
    if (resyncRequired) return
    resyncRequired = true
    refreshEditable()
    notifyState()
    opts.onResyncRequired?.(reason)
  }
  function requestDeferredResync(): void {
    if (!deferredResync || pending || isDirty() || inComposition() || error) return
    deferredResync = false
    if (resyncRequired) {
      opts.onResyncRequired?.({ code: 'remote-base-mismatch', message: 'Fetch the committed head again after the local edit.' })
      return
    }
    requireResync({ code: 'remote-base-mismatch', message: 'Fetch the committed head again after the local edit.' })
  }
  function applyRemote(change: AppliedChange): boolean {
    if (appliedChanges.has(change.changeId)) return true
    if (!change.operations.length || change.baseRevisionId !== snapshot.revisionId) {
      requireResync({ code: 'remote-base-mismatch', message: 'Remote change is not the direct successor of the committed head.', changeId: change.changeId })
      return false
    }
    for (const write of change.operations.flatMap(operationContentWrites)) {
      const current = snapshot.blocks.find((block) => block.blockId === write.blockId)
      if (current?.blockRevision === change.blockRevisions[write.blockId] && current.markdown !== write.content) {
        throw new Error(`EDITOR_KIT_V2_REMOTE_REVISION_REUSED: ${write.blockId}`)
      }
    }
    let next: EditorSnapshot
    try {
      next = applyDocumentChange(snapshot, {
        requestId: change.originRequestId ?? change.changeId, documentId: snapshot.documentId,
        baseRevisionId: change.baseRevisionId, operations: change.operations,
      }, { revisionId: change.revisionId, blockRevisions: change.blockRevisions }, { historicalRevisions: history })
    } catch {
      requireResync({ code: 'remote-base-mismatch', message: 'Remote operations do not match the committed revision.', changeId: change.changeId })
      return false
    }
    installAuthoritative(next, snapshot.blocks)
    appliedChanges.add(change.changeId)
    saveRecovery()
    notifyState()
    return true
  }
  function drainRemote(): void {
    if (pending || inComposition() || resyncRequired || error || deferredResync) return
    while (queuedChanges.length) {
      if (!applyRemote(queuedChanges[0])) return
      queuedChanges.shift()
      if (error) return
    }
  }
  function queueRemote(change: AppliedChange): void {
    if (!appliedChanges.has(change.changeId) && !queuedChanges.some((item) => item.changeId === change.changeId)) {
      queuedChanges.push(change)
    }
  }

  async function reconcile(update: SurfaceUpdate): Promise<void> {
    if (destroyed) return
    if (update.kind === 'apply-remote') {
      const compositionConflict = inComposition() && (finishingComposition
        || update.change.operations.some((op) => op.kind !== 'block.replace' || op.target.blockId === composingBlockId))
      if (pending || compositionConflict || error || resyncRequired || deferredResync) queueRemote(update.change)
      else if (!applyRemote(update.change)) queueRemote(update.change)
      return
    }
    if (update.kind === 'resync') {
      if (pending || isDirty() || inComposition() || retainedDraft) {
        deferredResync = true
        return
      }
      syncBlocks(update.snapshot.blocks)
      snapshot = cloneSnapshot(update.snapshot)
      remember(snapshot)
      includeChanges(update.includedChangeIds)
      resyncRequired = false
      deferredResync = false
      refreshEditable()
      drainRemote()
      notifyState()
      return
    }
    if (!pending || pending.batch.requestId !== update.requestId) return
    if (inComposition()) {
      if (!compositionReceipts.some((item) => item.requestId === update.requestId)) compositionReceipts.push(update)
      return
    }
    const request = pending
    pending = null
    includeChanges(update.includedChangeIds)
    if (update.kind === 'ack-local') {
      failedRequest = null
      installAuthoritative(update.authoritative, request.draft)
    } else if (update.kind === 'proposal-stored') {
      const candidate = draftBlocks()
      const hasLaterEdits = !sameDocument(candidate, request.draft)
      retainedDraft = hasLaterEdits ? candidate : null
      syncBlocks(update.authoritative.blocks)
      snapshot = cloneSnapshot(update.authoritative)
      remember(snapshot)
      failedRequest = null
      if (hasLaterEdits) {
        setFailure({ code: 'stale-base', message: 'The submitted edit is a proposal. Later unsaved input is retained for comparison/export.' })
      }
    } else {
      failedRequest = request
      installAuthoritative(update.authoritative, request.base.blocks)
      // Rejection must not erase either the submitted candidate or later input.
      if (!error) setFailure(update.reason)
    }
    saveRecovery()
    drainRemote()
    requestDeferredResync()
    notifyState()
    scheduleFlush()
  }

  const onCompositionStart = () => {
    if (compositionFrame !== null) cancelAnimationFrame(compositionFrame)
    compositionFrame = null
    finishingComposition = false
    composing = true
    composingBlockId = blockAtSelection()
    clearTimer()
  }
  const onCompositionEnd = () => {
    composing = false
    finishingComposition = true
    compositionFrame = requestAnimationFrame(() => {
      compositionFrame = null
      if (destroyed) return
      finishingComposition = false
      composingBlockId = null
      for (const receipt of compositionReceipts.splice(0)) void reconcile(receipt)
      drainRemote()
      queueLocalWork()
    })
  }
  const onBlur = () => flushLocal()
  const onBeforeUnload = (event: BeforeUnloadEvent) => {
    saveRecovery()
    if (isDirty() || pending || retainedDraft) {
      event.preventDefault()
      event.returnValue = ''
    }
  }
  rich.view.dom.addEventListener('compositionstart', onCompositionStart)
  rich.view.dom.addEventListener('compositionend', onCompositionEnd)
  rich.view.dom.addEventListener('blur', onBlur, true)
  window.addEventListener('beforeunload', onBeforeUnload)

  function executeStructuralCommand(command: StructuralCommand): boolean {
    if (destroyed || isReadOnly() || inComposition()) return false
    const draft = draftBlocks()
    const index = draft.findIndex((block) => block.blockId === command.blockId)
    if (index < 0) return false
    if (command.kind === 'block.insert-after') {
      const id = freshId()
      const planned = insertBlockSpan(closeHistory(rich.view.state.tr), currentLayout(), index + 1,
        { blockId: id, markdown: command.content })
      const pos = positionAtBlockStart(planned.transaction.doc, planned.layout, id)
      planned.transaction.setSelection(TextSelection.near(planned.transaction.doc.resolve(pos + 1)))
      rich.view.dispatch(planned.transaction)
    } else if (command.kind === 'block.delete') {
      if (draft.length === 1) {
        syncBlocks([{ blockId: command.blockId, markdown: '' }], true)
      } else {
        rich.view.dispatch(deleteBlockSpan(closeHistory(rich.view.state.tr), currentLayout(), command.blockId).transaction)
      }
    } else {
      const nextIndex = command.kind === 'block.move-up' ? index - 1 : index + 1
      if (nextIndex < 0 || nextIndex >= draft.length) return false
      const [block] = draft.splice(index, 1)
      draft.splice(nextIndex, 0, block)
      syncBlocks(draft, true)
    }
    rich.view.focus()
    queueLocalWork()
    return true
  }
  const flush = (): Promise<void> => {
    if (destroyed) return Promise.reject(new Error('Editor has been destroyed.'))
    if (error) return Promise.reject(new Error(error.message))
    if (isReadOnly() && (isDirty() || pending)) return Promise.reject(new Error('The document is read-only; pending input is retained as a draft.'))
    const promise = new Promise<void>((resolve, reject) => waiters.add({ resolve, reject }))
    flushLocal()
    notifyState()
    return promise
  }

  // Restore only a non-authoritative recovery copy, and never auto-submit it.
  try {
    const raw = localStorage.getItem(recoveryKey)
    if (raw) {
      const cached = JSON.parse(raw) as { base: EditorSnapshot; draft: LayoutBlock[]; restoreSources?: EditorSnapshot[]; blocked?: boolean; error?: ChangeError }
      if (cached.base?.documentId === snapshot.documentId && Array.isArray(cached.draft)
        && cached.draft.every((block) => typeof block.blockId === 'string' && typeof block.markdown === 'string')) {
        assertBlockIdentity(cached.draft)
        if (sameDocument(cached.draft, snapshot.blocks)) localStorage.removeItem(recoveryKey)
        else {
          remember(cached.base)
          for (const source of cached.restoreSources ?? []) {
            if (source.documentId === snapshot.documentId && Array.isArray(source.blocks)) remember(source)
          }
          if (cached.base.revisionId === snapshot.revisionId && !requestedReadOnly && !cached.blocked) {
            syncBlocks(cached.draft)
            error = { code: 'persistence-failed', message: 'Recovered an unsaved local draft. Review it, then retry saving or discard it.' }
          } else {
            retainedDraft = cached.draft
            error = cached.error && cached.error.code !== 'persistence-failed' ? cached.error
              : { code: 'stale-base', message: 'The committed document changed. An older local draft is retained for comparison/export.' }
          }
        }
      }
    }
  } catch { /* Invalid recovery bytes never replace the authoritative document. */ }
  refreshEditable()

  const surface: EditorSurface = {
    reconcile,
    observeLocalOperations(listener) { listeners.add(listener); return () => listeners.delete(listener) },
    observeState(listener) { stateListeners.add(listener); listener(currentState()); return () => stateListeners.delete(listener) },
    selectedBlockId: blockAtSelection,
    executeStructuralCommand,
    executeCommand(command: EditorCommand) {
      if (destroyed || isReadOnly() || inComposition()) return false
      return executeEditorCommand(rich.view, command)
    },
    canExecuteCommand(command: EditorCommand) {
      return !destroyed && !isReadOnly() && !inComposition() && canExecuteEditorCommand(rich.view, command)
    },
    flush,
    getDraftMarkdown() { return (retainedDraft ?? draftBlocks()).map((block) => block.markdown).join('\n\n') },
    retryPending() {
      if (destroyed || isReadOnly() || pending || error?.code !== 'persistence-failed' || retainedDraft) return false
      error = null
      if (failedRequest) { const request = failedRequest; failedRequest = null; emit(request) }
      else flushLocal()
      notifyState()
      return true
    },
    discardDraft() {
      if (destroyed || pending || inComposition()) return
      retainedDraft = null
      failedRequest = null
      error = null
      syncBlocks(snapshot.blocks)
      refreshEditable()
      saveRecovery()
      drainRemote()
      requestDeferredResync()
      notifyState()
    },
    restoreRevision(revision) {
      if (destroyed || isReadOnly() || inComposition() || pending || error
        || revision.documentId !== snapshot.documentId) return false
      assertBlockIdentity(revision.blocks)
      remember(revision)
      syncBlocks(revision.blocks, true)
      queueLocalWork()
      return true
    },
    setReadOnly(value) {
      if (value && !requestedReadOnly) flushLocal()
      requestedReadOnly = value
      refreshEditable()
      notifyState()
    },
    destroy() {
      if (closing) return closing
      closing = (async () => {
        if (compositionFrame !== null) cancelAnimationFrame(compositionFrame)
        compositionFrame = null
        composing = false
        finishingComposition = false
        for (const receipt of compositionReceipts.splice(0)) await reconcile(receipt)
        saveRecovery()
        try { await flush() } catch { /* recovery copy survives failed close */ }
        destroyed = true
        clearTimer()
        listeners.clear()
        stateListeners.clear()
        queuedChanges.length = 0
        rich.view.dom.removeEventListener('compositionstart', onCompositionStart)
        rich.view.dom.removeEventListener('compositionend', onCompositionEnd)
        rich.view.dom.removeEventListener('blur', onBlur, true)
        window.removeEventListener('beforeunload', onBeforeUnload)
        rich.destroy()
        host.remove()
      })()
      return closing
    },
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
  return { surface, decorations }
}
