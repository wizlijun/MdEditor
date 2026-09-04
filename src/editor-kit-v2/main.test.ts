// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createSchema, parseMarkdown, serializeMarkdown } from '@moraya/core'
import { EditorState } from 'prosemirror-state'
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
      { blockId: 'block-b', blockRevision: 'block-b/1', markdown: 'Second paragraph.' },
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
    stylesheet.href = '/editor-kit-v1.css'
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
    })
    expect(rich.getMarkdown()).toBe(afterLocal)
    expect(batches).toHaveLength(1)

    const change = {
      changeId: 'change-agent',
      originRequestId: batches[0].requestId,
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
    ])
    rich.view.dispatch(rich.view.state.tr.replaceWith(0, rich.view.state.doc.content.size, swapped.content))
    await Promise.resolve()

    expect(rich.getMarkdown()).toBe(before)
    expect(blocked).toHaveBeenCalledOnce()
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
    })
    rich.view.dispatch(rich.view.state.tr.insertText(' second', 14))
    await Promise.resolve()
    expect(batches).toHaveLength(2)
    expect(batches[1].operations[0]).toMatchObject({ expectedBlockRevision: 'block-a/2' })
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
    })
    expect(rich.getMarkdown()).toContain('Heading local')
    expect(rich.getMarkdown()).toContain('Authoritative remote payload.')
    expect(batches).toHaveLength(1)

    rich.view.dispatch(rich.view.state.tr.insertText(' again', rich.view.state.doc.child(0).nodeSize - 1))
    await Promise.resolve()
    expect(batches).toHaveLength(2)
    expect(batches[1]).toMatchObject({ baseRevisionId: 'revision-3' })
  })

  it('queues a same-block remote update until composed input is submitted and acknowledged', async () => {
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
      kind: 'ack-local', requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-local', {
        'block-a': { blockRevision: 'block-a/local', markdown: batches[0].operations[0].markdown },
      }),
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
})
