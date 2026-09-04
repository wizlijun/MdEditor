// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createSchema, parseMarkdown, serializeMarkdown } from '@moraya/core'
import { EditorState, TextSelection } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'

const mocks = vi.hoisted(() => ({
  mountRich: vi.fn(),
  setKitBaseDir: vi.fn(),
  loadVaultRoot: vi.fn(async () => '/vault'),
  applyKitTheme: vi.fn(async () => {}),
  watchKitTheme: vi.fn(),
}))

vi.mock('../editor-kit/rich', () => ({ mountRich: mocks.mountRich, setKitBaseDir: mocks.setKitBaseDir }))
vi.mock('../editor-kit/media', () => ({ loadVaultRoot: mocks.loadVaultRoot }))
vi.mock('../editor-kit/theme', () => ({ applyKitTheme: mocks.applyKitTheme, watchKitTheme: mocks.watchKitTheme }))

import { mountDocumentEditor, type EditorSnapshot, type LocalOperationBatch } from './main'

const mediaResolver = {
  loadLocalImage: async (path: string) => path,
  loadLocalMedia: async (path: string) => path,
  loadRemoteMedia: async (url: string) => url,
}

function makeRich(host: HTMLElement, initialMarkdown: string) {
  const schema = createSchema({ mediaResolver })
  const view = new EditorView(host, {
    state: EditorState.create({ schema, doc: parseMarkdown(initialMarkdown, schema) }),
  })
  const destroy = vi.fn(() => view.destroy())
  return {
    view,
    getMarkdown: () => serializeMarkdown(view.state.doc),
    setContent(markdown: string) {
      const doc = parseMarkdown(markdown, schema)
      view.dispatch(view.state.tr.replaceWith(0, view.state.doc.content.size, doc.content))
    },
    destroy,
  }
}

function snapshot(): EditorSnapshot {
  return {
    documentId: 'document-1',
    revisionId: 'revision-1',
    blocks: [
      { blockId: 'block-a', blockRevision: 'block-a/1', markdown: '# Heading' },
      { blockId: 'block-b', blockRevision: 'block-b/1', markdown: 'Second paragraph.\n\nThird paragraph.' },
    ],
  }
}

function authoritativeSnapshot(
  revisionId: string,
  changes: Readonly<Record<string, { blockRevision: string; markdown: string }>>,
): EditorSnapshot {
  const base = snapshot()
  return {
    ...base,
    revisionId,
    blocks: base.blocks.map((block) => changes[block.blockId] ? { ...block, ...changes[block.blockId] } : block),
  }
}

describe('Editor Kit v2', () => {
  beforeEach(() => {
    document.head.innerHTML = ''
    document.body.innerHTML = ''
    const stylesheet = document.createElement('link')
    stylesheet.href = 'data:text/css,/*editor-kit-v1.css'
    document.head.appendChild(stylesheet)
    mocks.mountRich.mockReset()
    mocks.mountRich.mockImplementation(async (host: HTMLElement, markdown: string) => makeRich(host, markdown))
  })

  it('emits one block replace, treats ack as metadata-only, and applies another block remotely', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))

    rich.view.dispatch(rich.view.state.tr.insertText(' updated', 8))
    await Promise.resolve()
    expect(batches).toHaveLength(1)
    expect(batches[0].operations).toMatchObject([{
      kind: 'block.replace', blockId: 'block-a', expectedBlockRevision: 'block-a/1',
    }])
    const afterLocal = rich.getMarkdown()

    await mounted.surface.reconcile({
      kind: 'ack-local',
      requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-2', {
        'block-a': { blockRevision: 'block-a/2', markdown: batches[0].operations[0].markdown },
      }),
      includedChangeIds: [],
    })
    expect(rich.getMarkdown()).toBe(afterLocal)
    expect(batches).toHaveLength(1)

    const change = {
      changeId: 'change-agent',
      originRequestId: batches[0].requestId,
      baseRevisionId: 'revision-2',
      revisionId: 'revision-3',
      blockRevisions: { 'block-b': 'block-b/2' },
      operations: [{
        kind: 'block.replace' as const,
        operationId: 'agent-operation',
        blockId: 'block-b',
        expectedBlockRevision: 'block-b/1',
        markdown: 'Agent changed only this paragraph.',
      }],
    }
    await mounted.surface.reconcile({ kind: 'apply-remote', change })
    const once = rich.getMarkdown()
    expect(once).toContain('Agent changed only this paragraph.')
    expect(once).toContain('Heading updated')
    expect(batches).toHaveLength(1)

    await mounted.surface.reconcile({ kind: 'apply-remote', change })
    expect(rich.getMarkdown()).toBe(once)
    expect(batches).toHaveLength(1)
  })

  it('fails closed when a local transaction changes the top-level block structure', async () => {
    const blocked = vi.fn()
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request', operationId: () => 'operation' },
      onBlockedStructuralEdit: blocked,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const before = rich.getMarkdown()
    rich.view.dispatch(rich.view.state.tr.split(4))
    await Promise.resolve()

    expect(rich.getMarkdown()).toBe(before)
    expect(blocked).toHaveBeenCalledOnce()
    await mounted.surface.destroy()
    expect(rich.destroy).toHaveBeenCalledOnce()
  })

  it('fails closed when a same-size transaction reorders top-level blocks', async () => {
    const blocked = vi.fn()
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request', operationId: () => 'operation' },
      onBlockedStructuralEdit: blocked,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const before = rich.getMarkdown()
    const swapped = rich.view.state.schema.topNodeType.create(null, [
      rich.view.state.doc.child(1),
      rich.view.state.doc.child(0),
      rich.view.state.doc.child(2),
    ])
    rich.view.dispatch(rich.view.state.tr.replaceWith(0, rich.view.state.doc.content.size, swapped.content))
    await Promise.resolve()

    expect(rich.getMarkdown()).toBe(before)
    expect(blocked).toHaveBeenCalledOnce()
  })

  it('maps edits and decorations across every top-level node in a multi-node block', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    mounted.decorations.setLayer('activity', [{ blockId: 'block-b', kind: 'activity', label: 'Working' }])

    expect(rich.view.dom.querySelectorAll('[data-cdr-block-id="block-b"]')).toHaveLength(2)
    expect(rich.view.dom.querySelectorAll('[data-cdr-label="Working"]')).toHaveLength(1)

    const thirdParagraphPosition = rich.view.state.doc.child(0).nodeSize
      + rich.view.state.doc.child(1).nodeSize + 1
    rich.view.dispatch(rich.view.state.tr.insertText('updated ', thirdParagraphPosition))
    await Promise.resolve()

    expect(batches).toHaveLength(1)
    expect(batches[0].operations).toHaveLength(1)
    expect(batches[0].operations[0]).toMatchObject({ blockId: 'block-b' })
    expect(batches[0].operations[0].markdown).toContain('Second paragraph.')
    expect(batches[0].operations[0].markdown).toContain('updated Third paragraph.')
  })

  it('emits one block replacement when one transaction changes two nodes in the same span', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const secondStart = rich.view.state.doc.child(0).nodeSize
    const thirdStart = secondStart + rich.view.state.doc.child(1).nodeSize

    rich.view.dispatch(rich.view.state.tr
      .insertText('third: ', thirdStart + 1)
      .insertText('second: ', secondStart + 1))
    await Promise.resolve()

    expect(batches).toHaveLength(1)
    expect(batches[0].operations).toHaveLength(1)
    expect(batches[0].operations[0]).toMatchObject({ blockId: 'block-b' })
    expect(batches[0].operations[0].markdown).toContain('second: Second paragraph.')
    expect(batches[0].operations[0].markdown).toContain('third: Third paragraph.')
  })

  it('keeps following block identity when a remote replacement changes its predecessor node count', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const initialBlockBStart = rich.view.state.doc.child(0).nodeSize
    rich.view.dispatch(rich.view.state.tr.setSelection(
      TextSelection.create(rich.view.state.doc, initialBlockBStart + 2),
    ))

    await mounted.surface.reconcile({
      kind: 'apply-remote',
      change: {
        changeId: 'expand-first-block', baseRevisionId: 'revision-1', revisionId: 'revision-2',
        blockRevisions: { 'block-a': 'block-a/2' },
        operations: [{
          kind: 'block.replace', operationId: 'remote-expand', blockId: 'block-a',
          expectedBlockRevision: 'block-a/1', markdown: '# Heading\n\nExpanded context.',
        }],
      },
    })
    expect(rich.view.dom.querySelectorAll('[data-cdr-block-id="block-a"]')).toHaveLength(2)
    expect(rich.view.state.selection.$from.index(0)).toBe(2)
    expect(rich.view.state.selection.$from.parent.textContent).toContain('Second paragraph.')

    const blockBStart = rich.view.state.doc.child(0).nodeSize + rich.view.state.doc.child(1).nodeSize
    rich.view.dispatch(rich.view.state.tr.insertText('Remote-safe ', blockBStart + 2))
    await Promise.resolve()
    expect(batches).toHaveLength(1)
    expect(batches[0].operations[0]).toMatchObject({ blockId: 'block-b' })
    expect(batches[0].baseRevisionId).toBe('revision-2')
  })

  it('rejects duplicate block identities before mounting the editor', async () => {
    const duplicated = snapshot()
    duplicated.blocks = [duplicated.blocks[0], { ...duplicated.blocks[1], blockId: 'block-a' }]
    await expect(mountDocumentEditor(document.body, {
      snapshot: duplicated,
      ids: { requestId: () => 'request', operationId: () => 'operation' },
    })).rejects.toThrow('EDITOR_KIT_V2_BLOCK_IDENTITY')
    expect(mocks.mountRich).not.toHaveBeenCalled()
  })

  it('serializes local edits until the previous block replacement is acknowledged', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))

    rich.view.dispatch(rich.view.state.tr.insertText(' first', 8))
    rich.view.dispatch(rich.view.state.tr.insertText(' second', 8))
    await Promise.resolve()
    expect(batches).toHaveLength(1)
    expect(rich.getMarkdown()).toContain('Heading first')
    expect(rich.getMarkdown()).not.toContain('second')

    await mounted.surface.reconcile({
      kind: 'ack-local', requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-2', {
        'block-a': { blockRevision: 'block-a/2', markdown: batches[0].operations[0].markdown },
      }),
      includedChangeIds: [],
    })
    rich.view.dispatch(rich.view.state.tr.insertText(' second', 14))
    await Promise.resolve()
    expect(batches).toHaveLength(2)
    expect(batches[1].operations[0]).toMatchObject({ expectedBlockRevision: 'block-a/2' })
  })

  it('emits a reverse local transaction against the acknowledged block revision', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))

    rich.view.dispatch(rich.view.state.tr.insertText(' undo', 8))
    await Promise.resolve()
    const committed = authoritativeSnapshot('revision-2', {
      'block-a': { blockRevision: 'block-a/2', markdown: batches[0].operations[0].markdown },
    })
    await mounted.surface.reconcile({
      kind: 'ack-local', requestId: batches[0].requestId, authoritative: committed,
      includedChangeIds: [],
    })

    rich.view.dispatch(rich.view.state.tr.delete(8, 13))
    await Promise.resolve()
    expect(batches).toHaveLength(2)
    expect(batches[1].operations).toMatchObject([{
      blockId: 'block-a', expectedBlockRevision: 'block-a/2', markdown: '# Heading',
    }])
  })

  it('reconstructs multi-node layout when remounted from a supplied committed snapshot', async () => {
    const committed = authoritativeSnapshot('revision-reopened', {
      'block-a': { blockRevision: 'block-a/reopened', markdown: '# Reopened\n\nPersisted context.' },
    })
    const first = await mountDocumentEditor(document.body, {
      snapshot: committed,
      ids: { requestId: () => 'request-first', operationId: () => 'operation-first' },
    })
    await first.surface.destroy()

    const reopened = await mountDocumentEditor(document.body, {
      snapshot: committed,
      ids: { requestId: () => 'request-second', operationId: () => 'operation-second' },
    })
    const rich = await mocks.mountRich.mock.results[1].value

    expect(rich.getMarkdown()).toContain('# Reopened')
    expect(rich.getMarkdown()).toContain('Persisted context.')
    expect(rich.view.dom.querySelectorAll('[data-cdr-block-id="block-a"]')).toHaveLength(2)
    expect(rich.view.dom.querySelectorAll('[data-cdr-block-id="block-b"]')).toHaveLength(2)
    await reopened.surface.destroy()
  })

  it('keeps the newer authoritative head when its ack already includes a queued remote change', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' local', 8))
    await Promise.resolve()

    await mounted.surface.reconcile({
      kind: 'apply-remote',
      change: {
        changeId: 'authoritative-change',
        originRequestId: batches[0].requestId,
        baseRevisionId: 'revision-1',
        revisionId: 'revision-2',
        blockRevisions: { 'block-b': 'block-b/2' },
        operations: [{
          kind: 'block.replace', operationId: 'remote-operation', blockId: 'block-b',
          expectedBlockRevision: 'block-b/1', markdown: 'Authoritative remote payload.',
        }],
      },
    })
    expect(rich.getMarkdown()).not.toContain('Authoritative remote payload.')

    await mounted.surface.reconcile({
      kind: 'ack-local', requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-3', {
        'block-a': { blockRevision: 'block-a/2', markdown: batches[0].operations[0].markdown },
        'block-b': { blockRevision: 'block-b/2', markdown: 'Authoritative remote payload.' },
      }),
      includedChangeIds: ['authoritative-change'],
    })
    expect(rich.getMarkdown()).toContain('Heading local')
    expect(rich.getMarkdown()).toContain('Authoritative remote payload.')
    expect(batches).toHaveLength(1)

    rich.view.dispatch(rich.view.state.tr.insertText(' again', rich.view.state.doc.child(0).nodeSize - 1))
    await Promise.resolve()
    expect(batches).toHaveLength(2)
    expect(batches[1]).toMatchObject({ baseRevisionId: 'revision-3' })
  })

  it('discards every queued change explicitly covered by a newer authoritative ack', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' local', 8))
    await Promise.resolve()

    const second = {
      changeId: 'change-b2', baseRevisionId: 'revision-1', revisionId: 'revision-2',
      blockRevisions: { 'block-b': 'block-b/2' },
      operations: [{
        kind: 'block.replace' as const, operationId: 'operation-b2', blockId: 'block-b',
        expectedBlockRevision: 'block-b/1', markdown: 'Second revision.',
      }],
    }
    const third = {
      changeId: 'change-b3', baseRevisionId: 'revision-2', revisionId: 'revision-3',
      blockRevisions: { 'block-b': 'block-b/3' },
      operations: [{
        kind: 'block.replace' as const, operationId: 'operation-b3', blockId: 'block-b',
        expectedBlockRevision: 'block-b/2', markdown: 'Third revision.',
      }],
    }
    await mounted.surface.reconcile({ kind: 'apply-remote', change: second })
    await mounted.surface.reconcile({ kind: 'apply-remote', change: third })

    await mounted.surface.reconcile({
      kind: 'ack-local', requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-4', {
        'block-a': { blockRevision: 'block-a/2', markdown: batches[0].operations[0].markdown },
        'block-b': { blockRevision: 'block-b/3', markdown: 'Third revision.' },
      }),
      includedChangeIds: ['change-b2', 'change-b3'],
    })
    expect(rich.getMarkdown()).toContain('Third revision.')
    expect(rich.getMarkdown()).not.toContain('Second revision.')

    const blockBStart = rich.view.state.doc.child(0).nodeSize
    rich.view.dispatch(rich.view.state.tr.insertText('Current ', blockBStart + 1))
    await Promise.resolve()
    expect(batches).toHaveLength(2)
    expect(batches[1]).toMatchObject({ baseRevisionId: 'revision-4' })
    expect(batches[1].operations[0]).toMatchObject({ expectedBlockRevision: 'block-b/3' })
  })

  it('never replays a resync snapshot received while a newer local commit is pending', async () => {
    const requestFreshResync = vi.fn()
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
      onResyncRequired: requestFreshResync,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' local', 8))
    await Promise.resolve()

    await mounted.surface.reconcile({
      kind: 'resync',
      snapshot: authoritativeSnapshot('revision-2', {
        'block-a': { blockRevision: 'block-a/old-resync', markdown: '# Old resync' },
      }),
      includedChangeIds: [],
    })
    expect(requestFreshResync).not.toHaveBeenCalled()
    expect(rich.getMarkdown()).toContain('Heading local')

    const committed = authoritativeSnapshot('revision-3', {
      'block-a': { blockRevision: 'block-a/3', markdown: batches[0].operations[0].markdown },
    })
    await mounted.surface.reconcile({
      kind: 'ack-local', requestId: batches[0].requestId, authoritative: committed,
      includedChangeIds: [],
    })
    expect(requestFreshResync).toHaveBeenCalledOnce()
    expect(rich.getMarkdown()).toContain('Heading local')
    expect(rich.getMarkdown()).not.toContain('Old resync')

    await mounted.surface.reconcile({ kind: 'resync', snapshot: committed, includedChangeIds: [] })
    expect(rich.getMarkdown()).toContain('Heading local')
  })

  it('fails closed and requests a resync when a different-block remote revision arrives out of order', async () => {
    const resyncRequired = vi.fn()
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request', operationId: () => 'operation' },
      onResyncRequired: resyncRequired,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const before = rich.getMarkdown()

    await mounted.surface.reconcile({
      kind: 'apply-remote',
      change: {
        changeId: 'missing-parent', baseRevisionId: 'revision-2', revisionId: 'revision-3',
        blockRevisions: { 'block-b': 'block-b/2' },
        operations: [{
          kind: 'block.replace', operationId: 'operation-3', blockId: 'block-b',
          expectedBlockRevision: 'block-b/1', markdown: 'Must not apply.',
        }],
      },
    })
    expect(rich.getMarkdown()).toBe(before)
    expect(resyncRequired).toHaveBeenCalledWith(expect.objectContaining({ code: 'remote-base-mismatch' }))
    rich.view.dispatch(rich.view.state.tr.insertText('blocked ', 2))
    expect(rich.getMarkdown()).toBe(before)

    await mounted.surface.reconcile({
      kind: 'resync',
      snapshot: authoritativeSnapshot('revision-3', {
        'block-b': { blockRevision: 'block-b/2', markdown: 'Resynced head.' },
      }),
      includedChangeIds: ['missing-parent'],
    })
    expect(rich.getMarkdown()).toContain('Resynced head')
    expect(rich.getMarkdown()).not.toContain('Must not apply')
  })

  it('queues a same-block remote update and consumes it through the stale local rejection', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('中文', 2))
    await mounted.surface.reconcile({
      kind: 'apply-remote',
      change: {
        changeId: 'during-ime',
        baseRevisionId: 'revision-1',
        revisionId: 'revision-2',
        blockRevisions: { 'block-a': 'block-a/2' },
        operations: [{
          kind: 'block.replace',
          operationId: 'agent-operation',
          blockId: 'block-a',
          expectedBlockRevision: 'block-a/1',
          markdown: '# IME-safe remote heading',
        }],
      },
    })
    expect(rich.getMarkdown()).not.toContain('IME-safe')
    expect(rich.getMarkdown()).toContain('中文')

    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(rich.getMarkdown()).toContain('中文')
    expect(rich.getMarkdown()).not.toContain('IME-safe')

    await mounted.surface.reconcile({
      kind: 'reject-local', requestId: batches[0].requestId,
      reason: { code: 'stale-base', message: 'The remote revision committed first.' },
      authoritative: authoritativeSnapshot('revision-2', {
        'block-a': { blockRevision: 'block-a/2', markdown: '# IME-safe remote heading' },
      }),
      includedChangeIds: ['during-ime'],
    })
    expect(rich.getMarkdown()).toContain('IME-safe remote heading')
  })

  it('coalesces composition transactions into an operation after composition ends', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))

    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('中', 2))
    await Promise.resolve()
    expect(batches).toHaveLength(0)

    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    // Chromium/WebKit may deliver the final input transaction after
    // compositionend. It must still join the same coalesced operation.
    rich.view.dispatch(rich.view.state.tr.insertText('文', 3))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(batches[0].operations[0]).toMatchObject({ blockId: 'block-a', expectedBlockRevision: 'block-a/1' })
    expect(batches[0].operations[0].markdown).toContain('中文')
  })

  it('accepts the final IME transaction before acting on a deferred resync', async () => {
    const requestFreshResync = vi.fn()
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
      onResyncRequired: requestFreshResync,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))

    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('中', 2))
    await mounted.surface.reconcile({
      kind: 'resync',
      snapshot: authoritativeSnapshot('revision-2', {
        'block-b': { blockRevision: 'block-b/2', markdown: 'Remote head.' },
      }),
      includedChangeIds: [],
    })
    expect(requestFreshResync).not.toHaveBeenCalled()

    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('文', 3))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(batches[0].operations[0].markdown).toContain('中文')
    expect(requestFreshResync).not.toHaveBeenCalled()

    await mounted.surface.reconcile({
      kind: 'reject-local',
      requestId: batches[0].requestId,
      reason: { code: 'stale-base', message: 'The remote revision committed first.' },
      authoritative: authoritativeSnapshot('revision-2', {
        'block-b': { blockRevision: 'block-b/2', markdown: 'Remote head.' },
      }),
      includedChangeIds: [],
    })
    expect(requestFreshResync).toHaveBeenCalledOnce()
  })

  it('submits only the composing block when another block normalizes on Markdown round-trip', async () => {
    let sequence = 0
    const base = snapshot()
    const normalized = {
      ...base,
      blocks: base.blocks.map((block, index) => index === 0 ? { ...block, markdown: '## Heading ##' } : block),
    }
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: normalized,
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const blockBStart = rich.view.state.doc.child(0).nodeSize
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(rich.view.state.doc, blockBStart + 2)))

    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('中文', blockBStart + 2))
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    await vi.waitFor(() => expect(batches).toHaveLength(1))

    expect(batches[0].operations).toHaveLength(1)
    expect(batches[0].operations[0].blockId).toBe('block-b')
  })
})
