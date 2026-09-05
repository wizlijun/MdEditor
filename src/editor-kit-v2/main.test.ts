// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createEditor, createEditorPlugins, createSchema, parseMarkdown, serializeMarkdown } from '@moraya/core'
import type { Schema } from 'prosemirror-model'
import { EditorState, NodeSelection, TextSelection } from 'prosemirror-state'
import { EditorView } from 'prosemirror-view'
import { applyDocumentChange } from '../lib/cdr/core'
import { operationContentWrites, type Operation } from '../lib/cdr/operation'
import type { EditorSurface, EditorSurfaceState } from './contract'

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

import {
  mountDocumentEditor as mountDocumentEditorBase,
  type EditorSnapshot,
  type LocalOperationBatch,
  type MountDocumentEditorOptions,
} from './main'

function mountDocumentEditor(container: HTMLElement, opts: MountDocumentEditorOptions) {
  return mountDocumentEditorBase(container, { localChangeDebounceMs: 0, ...opts })
}

const mediaResolver = {
  loadLocalImage: async (path: string) => path,
  loadLocalMedia: async (path: string) => path,
  loadRemoteMedia: async (url: string) => url,
}

function makeRich(host: HTMLElement, initialMarkdown: string, _root?: string, _change?: unknown, _placeholder?: unknown, _power?: unknown, customizeSchema?: (schema: Schema) => Schema) {
  const baseSchema = createSchema({ mediaResolver })
  const schema = customizeSchema ? customizeSchema(baseSchema) : baseSchema
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

async function makeMorayaRich(host: HTMLElement, initialMarkdown: string, _root?: string, _change?: unknown, _placeholder?: unknown, _power?: unknown, customizeSchema?: (schema: Schema) => Schema) {
  const options = {
    container: host,
    initialContent: initialMarkdown,
    mediaResolver,
    enableMath: false,
    enableMermaid: false,
    enableTableResize: true,
    enableImageSelection: false,
    enableHistory: true,
    enableInlineMarkInputRules: false,
    inlineSyntaxScope: 'line' as const,
  }
  const editor = await createEditor(options)
  if (customizeSchema) {
    const schema = customizeSchema(editor.view.state.schema)
    editor.view.updateState(EditorState.create({ schema, doc: parseMarkdown(initialMarkdown, schema), plugins: await createEditorPlugins(options, schema) }))
  }
  return editor
}

function pastePlainText(target: HTMLElement, text: string): void {
  const event = new Event('paste', { bubbles: true, cancelable: true }) as ClipboardEvent
  Object.defineProperty(event, 'clipboardData', {
    value: {
      getData(type: string) { return type === 'text/plain' ? text : '' },
      types: ['text/plain'],
    },
  })
  target.dispatchEvent(event)
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

function replaceOperation(
  operationId: string,
  blockId: string,
  expectedBlockRevision: string,
  content: string,
) {
  return {
    kind: 'block.replace' as const,
    operationId,
    target: { blockId, expectedBlockRevision },
    payload: { content },
  }
}

async function ackLocal(surface: EditorSurface, batch: LocalOperationBatch, base = snapshot(), revisionId = 'revision-2') {
  const authoritative = applyDocumentChange(base, batch, {
    revisionId,
    blockRevisions: Object.fromEntries(batch.operations.flatMap(operationContentWrites)
      .map((write) => [write.blockId, `${write.blockId}/${revisionId}`])),
  })
  await surface.reconcile({ kind: 'ack-local', requestId: batch.requestId, authoritative, includedChangeIds: [] })
  return authoritative
}

function contentOf(operation: Operation): string {
  if (operation.kind !== 'block.replace' && operation.kind !== 'block.insert') {
    throw new Error(`Operation ${operation.kind} has no text content`)
  }
  return operation.payload.content
}

describe('Editor Kit v2', () => {
  beforeEach(() => {
    localStorage.clear()
    document.head.innerHTML = ''
    document.body.innerHTML = ''
    const stylesheet = document.createElement('link')
    stylesheet.href = 'data:text/css,/*editor-kit-v1.css'
    document.head.appendChild(stylesheet)
    mocks.mountRich.mockReset()
    mocks.mountRich.mockImplementation(async (...args: Parameters<typeof makeRich>) => makeRich(...args))
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
      kind: 'block.replace', target: { blockId: 'block-a', expectedBlockRevision: 'block-a/1' },
    }])
    const afterLocal = rich.getMarkdown()

    await mounted.surface.reconcile({
      kind: 'ack-local',
      requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-2', {
        'block-a': { blockRevision: 'block-a/2', markdown: contentOf(batches[0].operations[0]) },
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
      operations: [replaceOperation('agent-operation', 'block-b', 'block-b/1', 'Agent changed only this paragraph.')],
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

  it('uses explicit insert/delete commands and preserves the failed deletion until explicitly discarded', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: {
        requestId: () => `request-${++sequence}`,
        operationId: () => `operation-${++sequence}`,
        blockId: () => 'block-c',
      },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))

    expect(mounted.surface.executeStructuralCommand({
      kind: 'block.insert-after', blockId: 'block-a', content: 'New context.',
    })).toBe(true)
    await Promise.resolve()
    expect(batches).toMatchObject([{
      documentId: 'document-1',
      baseRevisionId: 'revision-1',
      operations: [{
        kind: 'block.insert',
        target: { leftBlockId: 'block-a', rightBlockId: 'block-b' },
        payload: { candidateBlockId: 'block-c', content: 'New context.' },
      }],
    }])
    expect(rich.getMarkdown()).toContain('New context.')
    expect(rich.view.state.selection.$from.parent.textContent).toBe('New context.')

    const inserted: EditorSnapshot = {
      ...snapshot(),
      revisionId: 'revision-2',
      blocks: [
        snapshot().blocks[0],
        { blockId: 'block-c', blockRevision: 'block-c/1', markdown: 'New context.' },
        snapshot().blocks[1],
      ],
    }
    await mounted.surface.reconcile({
      kind: 'ack-local', requestId: batches[0].requestId, authoritative: inserted, includedChangeIds: [],
    })
    expect(batches).toHaveLength(1)

    expect(mounted.surface.executeStructuralCommand({ kind: 'block.delete', blockId: 'block-c' })).toBe(true)
    await Promise.resolve()
    expect(batches[1]).toMatchObject({
      documentId: 'document-1',
      baseRevisionId: 'revision-2',
      operations: [{
        kind: 'block.delete',
        target: { blockId: 'block-c', expectedBlockRevision: 'block-c/1' },
        payload: {},
      }],
    })
    expect(rich.getMarkdown()).not.toContain('New context.')
    expect(rich.view.state.selection.$from.parent.textContent).toContain('Second paragraph.')
    await mounted.surface.reconcile({
      kind: 'reject-local',
      requestId: batches[1].requestId,
      reason: { code: 'persistence-failed', message: 'not saved' },
      authoritative: inserted,
      includedChangeIds: [],
    })
    expect(rich.getMarkdown()).not.toContain('New context.')
    mounted.surface.discardDraft()
    expect(rich.getMarkdown()).toContain('New context.')
    expect(rich.view.dom.querySelectorAll('[data-cdr-block-id="block-c"]')).toHaveLength(1)
  })

  it('applies remote insert/delete as local ProseMirror transactions and preserves another block selection', async () => {
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request', operationId: () => 'operation' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const blockBStart = rich.view.state.doc.child(0).nodeSize
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(rich.view.state.doc, blockBStart + 2)))

    await mounted.surface.reconcile({
      kind: 'apply-remote',
      change: {
        changeId: 'insert-c', baseRevisionId: 'revision-1', revisionId: 'revision-2',
        blockRevisions: { 'block-c': 'block-c/1' },
        operations: [{
          kind: 'block.insert', operationId: 'insert-c/op',
          target: { leftBlockId: 'block-a', rightBlockId: 'block-b' },
          payload: { candidateBlockId: 'block-c', content: 'Remote context.' },
        }],
      },
    })
    expect(rich.getMarkdown()).toContain('Remote context.')
    expect(rich.view.state.selection.$from.parent.textContent).toContain('Second paragraph.')

    await mounted.surface.reconcile({
      kind: 'apply-remote',
      change: {
        changeId: 'delete-c', baseRevisionId: 'revision-2', revisionId: 'revision-3',
        blockRevisions: {},
        operations: [{
          kind: 'block.delete', operationId: 'delete-c/op',
          target: { blockId: 'block-c', expectedBlockRevision: 'block-c/1' }, payload: {},
        }],
      },
    })
    expect(rich.getMarkdown()).not.toContain('Remote context.')
    expect(rich.view.state.selection.$from.parent.textContent).toContain('Second paragraph.')
  })

  it('queues remote structure changes through composition and its finishing frame', async () => {
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request', operationId: () => 'operation' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const insertedChange = {
      changeId: 'ime-insert-c',
      baseRevisionId: 'revision-1',
      revisionId: 'revision-2',
      blockRevisions: { 'block-c': 'block-c/1' },
      operations: [{
        kind: 'block.insert' as const,
        operationId: 'ime-insert-c/op',
        target: { leftBlockId: 'block-a', rightBlockId: 'block-b' },
        payload: { candidateBlockId: 'block-c', content: 'Queued structure.' },
      }],
    }

    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    await mounted.surface.reconcile({ kind: 'apply-remote', change: insertedChange })
    expect(rich.getMarkdown()).not.toContain('Queued structure.')
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    await vi.waitFor(() => expect(rich.getMarkdown()).toContain('Queued structure.'))

    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    await mounted.surface.reconcile({
      kind: 'apply-remote',
      change: {
        changeId: 'ime-delete-c',
        baseRevisionId: 'revision-2',
        revisionId: 'revision-3',
        blockRevisions: {},
        operations: [{
          kind: 'block.delete',
          operationId: 'ime-delete-c/op',
          target: { blockId: 'block-c', expectedBlockRevision: 'block-c/1' },
          payload: {},
        }],
      },
    })
    expect(rich.getMarkdown()).toContain('Queued structure.')
    await vi.waitFor(() => expect(rich.getMarkdown()).not.toContain('Queued structure.'))
  })

  it('keeps composition intact until completion and clearing the final block retains its identity', async () => {
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request', operationId: () => 'operation', blockId: () => 'block-c' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    expect(mounted.surface.executeStructuralCommand({
      kind: 'block.insert-after', blockId: 'block-a', content: 'After IME.',
    })).toBe(false)
    expect(batches).toHaveLength(0)
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    await vi.waitFor(() => expect(mounted.surface.canExecuteCommand({ kind: 'bold' })).toBe(true))
    expect(mounted.surface.executeStructuralCommand({
      kind: 'block.insert-after', blockId: 'block-a', content: 'After IME.',
    })).toBe(true)
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(rich.getMarkdown()).toContain('After IME.')

    const single = await mountDocumentEditor(document.body, {
      snapshot: { ...snapshot(), documentId: 'single-document', blocks: [snapshot().blocks[0]] },
      ids: { requestId: () => 'single-request', operationId: () => 'single-operation' },
    })
    const singleBatches: LocalOperationBatch[] = []
    single.surface.observeLocalOperations((batch) => singleBatches.push(batch))
    expect(single.surface.executeStructuralCommand({ kind: 'block.delete', blockId: 'block-a' })).toBe(true)
    await vi.waitFor(() => expect(singleBatches).toHaveLength(1))
    expect(singleBatches[0].operations).toMatchObject([{ kind: 'block.replace',
      target: { blockId: 'block-a' }, payload: { content: '' } }])
  })

  it('does not expose one block identity for a cross-block selection', async () => {
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request', operationId: () => 'operation' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const secondBlockStart = rich.view.state.doc.child(0).nodeSize
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(
      rich.view.state.doc,
      2,
      secondBlockStart + 2,
    )))

    expect(mounted.surface.selectedBlockId()).toBeNull()
  })

  it('attributes top-level node and nested-list selections to their governed block', async () => {
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: {
        ...snapshot(),
        blocks: [
          { ...snapshot().blocks[0], markdown: '- one\n  - two' },
          snapshot().blocks[1],
        ],
      },
      ids: { requestId: () => 'request-selection', operationId: () => 'operation-selection' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    rich.view.dispatch(rich.view.state.tr.setSelection(NodeSelection.create(rich.view.state.doc, 0)))
    expect(mounted.surface.selectedBlockId()).toBe('block-a')

    let nestedTextStart = -1
    rich.view.state.doc.descendants((node: any, position: number) => {
      if (node.type.name === 'paragraph' && node.textContent === 'two') nestedTextStart = position + 1
    })
    expect(nestedTextStart).toBeGreaterThan(2)
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(
      rich.view.state.doc,
      3,
      nestedTextStart,
    )))
    expect(mounted.surface.selectedBlockId()).toBe('block-a')
  })

  it('treats Enter inside one governed block as one replacement and remaps following spans', async () => {
    const blocked = vi.fn()
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request-enter', operationId: () => 'operation-enter' },
      onBlockedStructuralEdit: blocked,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.split(4))
    await Promise.resolve()

    expect(blocked).not.toHaveBeenCalled()
    expect(batches).toHaveLength(1)
    expect(batches[0].operations).toMatchObject([{
      kind: 'block.replace',
      target: { blockId: 'block-a', expectedBlockRevision: 'block-a/1' },
    }])
    expect(rich.view.dom.querySelectorAll('[data-cdr-block-id="block-a"]')).toHaveLength(2)
    expect(rich.view.dom.querySelectorAll('[data-cdr-block-id="block-b"]')).toHaveLength(2)
  })

  it('accepts multi-paragraph paste-shaped replacement inside one block', async () => {
    const blocked = vi.fn()
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request-paste', operationId: () => `operation-paste-${++sequence}` },
      onBlockedStructuralEdit: blocked,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const pasted = parseMarkdown('# Pasted heading\n\n- one\n- two', rich.view.state.schema)

    rich.view.dispatch(rich.view.state.tr.replaceWith(
      0,
      rich.view.state.doc.child(0).nodeSize,
      pasted.content,
    ))
    await Promise.resolve()

    expect(blocked).not.toHaveBeenCalled()
    expect(batches).toHaveLength(1)
    expect(batches[0].operations).toMatchObject([{ kind: 'block.replace', target: { blockId: 'block-a' } }])
    const committed = await ackLocal(mounted.surface, batches[0])
    expect(committed.blocks.map((block) => block.markdown).join('\n\n')).toContain('# Pasted heading')
    expect(committed.blocks.map((block) => block.markdown).join('\n\n')).toContain('- one')
    expect(committed.blocks.find((block) => block.blockId === 'block-b')).toEqual(snapshot().blocks[1])
    expect(rich.view.dom.querySelectorAll('[data-cdr-block-id="block-b"]')).toHaveLength(2)
  })

  it('accepts Enter through the real Moraya keymap inside one governed block', async () => {
    mocks.mountRich.mockImplementationOnce(makeMorayaRich)
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request-real-enter', operationId: () => 'operation-real-enter' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const blockBStart = rich.view.state.doc.child(0).nodeSize
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(
      rich.view.state.doc,
      blockBStart + 7,
    )))
    const event = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })

    const handled = rich.view.someProp('handleKeyDown', (handler: any) => handler(rich.view, event))
    await Promise.resolve()

    expect(handled).toBe(true)
    expect(batches).toHaveLength(1)
    expect(batches[0].operations[0]).toMatchObject({ target: { blockId: 'block-b' } })
    expect(contentOf(batches[0].operations[0])).toContain('\n\n')
  })

  it('accepts Shift+Enter through the real Moraya keymap inside one governed block', async () => {
    mocks.mountRich.mockImplementationOnce(makeMorayaRich)
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request-real-hardbreak', operationId: () => 'operation-real-hardbreak' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const blockBStart = rich.view.state.doc.child(0).nodeSize
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(
      rich.view.state.doc,
      blockBStart + 7,
    )))
    const event = new KeyboardEvent('keydown', {
      key: 'Enter', shiftKey: true, bubbles: true, cancelable: true,
    })

    const handled = rich.view.someProp('handleKeyDown', (handler: any) => handler(rich.view, event))
    await Promise.resolve()

    expect(handled).toBe(true)
    expect(batches).toHaveLength(1)
    expect(batches[0].operations[0]).toMatchObject({ target: { blockId: 'block-b' } })
    expect(contentOf(batches[0].operations[0])).toContain('  \n')
  })

  it('accepts a real multi-paragraph clipboard paste inside one governed block', async () => {
    mocks.mountRich.mockImplementationOnce(makeMorayaRich)
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request-real-paste', operationId: () => 'operation-real-paste' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const blockBStart = rich.view.state.doc.child(0).nodeSize
    const firstParagraphEnd = blockBStart + rich.view.state.doc.child(1).nodeSize - 1
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(
      rich.view.state.doc,
      firstParagraphEnd,
    )))

    pastePlainText(rich.view.dom, '粘贴第一段\n\n粘贴第二段')
    await Promise.resolve()

    expect(batches).toHaveLength(1)
    expect(batches[0].operations[0]).toMatchObject({ target: { blockId: 'block-b' } })
    expect(contentOf(batches[0].operations[0])).toContain('粘贴第一段\n\n粘贴第二段')
  })

  it('keeps real Moraya undo and redo working after an Enter revision is acknowledged', async () => {
    mocks.mountRich.mockImplementationOnce(makeMorayaRich)
    let sequence = 0
    const blocked = vi.fn()
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
      onBlockedStructuralEdit: blocked,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const blockBStart = rich.view.state.doc.child(0).nodeSize
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(
      rich.view.state.doc,
      blockBStart + 7,
    )))
    rich.view.someProp('handleKeyDown', (handler: any) => handler(rich.view,
      new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true })))
    await Promise.resolve()
    expect(batches).toHaveLength(1)

    const entered = contentOf(batches[0].operations[0])
    const documentBeforeAck = rich.view.state.doc
    await mounted.surface.reconcile({
      kind: 'ack-local',
      requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-2', {
        'block-b': { blockRevision: 'block-b/2', markdown: entered },
      }),
      includedChangeIds: [],
    })
    expect(rich.view.state.doc.eq(documentBeforeAck)).toBe(true)

    const undoHandled = rich.view.someProp('handleKeyDown', (handler: any) => handler(rich.view,
      new KeyboardEvent('keydown', { key: 'z', ctrlKey: true, bubbles: true, cancelable: true })))
    await vi.waitFor(() => expect(batches).toHaveLength(2))
    expect(undoHandled).toBe(true)
    expect(contentOf(batches[1].operations[0])).toBe(snapshot().blocks[1].markdown)

    await mounted.surface.reconcile({
      kind: 'ack-local',
      requestId: batches[1].requestId,
      authoritative: authoritativeSnapshot('revision-3', {
        'block-b': { blockRevision: 'block-b/3', markdown: snapshot().blocks[1].markdown },
      }),
      includedChangeIds: [],
    })

    const redoHandled = rich.view.someProp('handleKeyDown', (handler: any) => handler(rich.view,
      new KeyboardEvent('keydown', {
        key: 'z', ctrlKey: true, shiftKey: true, bubbles: true, cancelable: true,
      })))
    await vi.waitFor(() => expect(batches).toHaveLength(3))
    expect(redoHandled).toBe(true)
    expect(contentOf(batches[2].operations[0])).toBe(entered)
    expect(blocked).not.toHaveBeenCalled()
  })

  it('accepts a Shift+Enter hard break and ordinary selection deletion inside one block', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const hardBreak = rich.view.state.schema.nodes.hardbreak
    expect(hardBreak).toBeTruthy()
    const blockBStart = rich.view.state.doc.child(0).nodeSize

    rich.view.dispatch(rich.view.state.tr
      .setSelection(TextSelection.create(rich.view.state.doc, blockBStart + 7))
      .replaceSelectionWith(hardBreak.create()))
    await Promise.resolve()

    expect(batches).toHaveLength(1)
    expect(batches[0].operations[0]).toMatchObject({ target: { blockId: 'block-b' } })
    expect(contentOf(batches[0].operations[0])).toContain('\n')

    await mounted.surface.reconcile({
      kind: 'ack-local',
      requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-2', {
        'block-b': { blockRevision: 'block-b/2', markdown: contentOf(batches[0].operations[0]) },
      }),
      includedChangeIds: [],
    })
    rich.view.dispatch(rich.view.state.tr
      .setSelection(TextSelection.create(rich.view.state.doc, blockBStart + 1, blockBStart + 7))
      .deleteSelection())
    await Promise.resolve()

    expect(batches).toHaveLength(2)
    expect(batches[1].operations[0]).toMatchObject({ target: { blockId: 'block-b' } })
  })

  it('normalizes joins across governed boundaries while preserving the surviving block identity', async () => {
    let sequence = 0
    const blocked = vi.fn()
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
      onBlockedStructuralEdit: blocked,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.join(rich.view.state.doc.child(0).nodeSize))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(blocked).not.toHaveBeenCalled()
    expect(rich.getMarkdown()).toContain('HeadingSecond paragraph.')
    expect(batches[0].operations.some((operation) => operation.kind === 'block.replace'
      && operation.target.blockId === 'block-a')).toBe(true)
    const committed = await ackLocal(mounted.surface, batches[0])
    rich.view.dispatch(rich.view.state.tr.join(rich.view.state.doc.child(0).nodeSize))
    await vi.waitFor(() => expect(batches).toHaveLength(2))
    const joined = await ackLocal(mounted.surface, batches[1], committed, 'revision-3')
    expect(joined.blocks[0].blockId).toBe('block-a')
    expect(joined.blocks).toHaveLength(1)
    expect(rich.getMarkdown()).toContain('Third paragraph.')
  })

  it('allows clearing a governed block without losing its stable identity', async () => {
    const blocked = vi.fn()
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request-clear', operationId: () => 'operation-clear' },
      onBlockedStructuralEdit: blocked,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const blockBStart = rich.view.state.doc.child(0).nodeSize
    rich.view.dispatch(rich.view.state.tr
      .setSelection(TextSelection.create(rich.view.state.doc, blockBStart + 1, rich.view.state.doc.content.size - 1))
      .deleteSelection())
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(blocked).not.toHaveBeenCalled()
    expect(batches[0].operations).toMatchObject([{ kind: 'block.replace',
      target: { blockId: 'block-b' }, payload: { content: '' } }])
    await ackLocal(mounted.surface, batches[0])
    expect(rich.getMarkdown()).not.toContain('Second paragraph.')
    expect(rich.view.dom.querySelector('[data-cdr-block-id="block-b"]')).not.toBeNull()
  })

  it('leaves native copy as a read-only browser action with no governed operation', async () => {
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request-copy', operationId: () => 'operation-copy' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const before = rich.getMarkdown()
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(rich.view.state.doc, 1, 5)))

    rich.view.dom.dispatchEvent(new Event('copy', { bubbles: true, cancelable: true }))
    await Promise.resolve()

    expect(rich.getMarkdown()).toBe(before)
    expect(batches).toHaveLength(0)
  })

  it('debounces continuous typing into one durable block replacement', async () => {
    vi.useFakeTimers()
    try {
      let sequence = 0
      const mounted = await mountDocumentEditorBase(document.body, {
        snapshot: snapshot(),
        ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
        localChangeDebounceMs: 250,
      })
      const rich = await mocks.mountRich.mock.results[0].value
      const batches: LocalOperationBatch[] = []
      mounted.surface.observeLocalOperations((batch) => batches.push(batch))

      rich.view.dispatch(rich.view.state.tr.insertText('A', 8))
      rich.view.dispatch(rich.view.state.tr.insertText('B', 9))
      rich.view.dispatch(rich.view.state.tr.insertText('C', 10))
      expect(rich.getMarkdown()).toContain('HeadingABC')
      expect(batches).toHaveLength(0)

      await vi.advanceTimersByTimeAsync(249)
      expect(batches).toHaveLength(0)
      await vi.advanceTimersByTimeAsync(1)
      expect(batches).toHaveLength(1)
      expect(contentOf(batches[0].operations[0])).toContain('HeadingABC')
      expect(rich.view.editable).toBe(true)

      await mounted.surface.reconcile({
        kind: 'ack-local',
        requestId: batches[0].requestId,
        authoritative: authoritativeSnapshot('revision-2', {
          'block-a': { blockRevision: 'block-a/2', markdown: contentOf(batches[0].operations[0]) },
        }),
        includedChangeIds: [],
      })
      expect(rich.view.editable).toBe(true)
    } finally {
      vi.useRealTimers()
    }
  })

  it('shows the first structural command immediately after blur and submits it after the current acknowledgement', async () => {
    let requestSequence = 0
    let operationSequence = 0
    const mounted = await mountDocumentEditorBase(document.body, {
      snapshot: snapshot(),
      ids: {
        requestId: () => `request-${++requestSequence}`,
        operationId: () => `operation-${++operationSequence}`,
        blockId: () => 'block-c',
      },
      localChangeDebounceMs: 250,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' edited', 8))

    rich.view.dom.dispatchEvent(new Event('blur', { bubbles: true }))
    expect(mounted.surface.executeStructuralCommand({
      kind: 'block.insert-after', blockId: 'block-a', content: 'Queued paragraph.',
    })).toBe(true)
    await Promise.resolve()

    expect(batches).toHaveLength(1)
    expect(batches[0].operations[0]).toMatchObject({ kind: 'block.replace', target: { blockId: 'block-a' } })
    expect(rich.getMarkdown()).toContain('Queued paragraph.')

    await mounted.surface.reconcile({
      kind: 'ack-local',
      requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-2', {
        'block-a': { blockRevision: 'block-a/2', markdown: contentOf(batches[0].operations[0]) },
      }),
      includedChangeIds: [],
    })
    await Promise.resolve()

    await vi.waitFor(() => expect(batches).toHaveLength(2))
    expect(batches[1]).toMatchObject({
      baseRevisionId: 'revision-2',
      operations: [{
        kind: 'block.insert',
        target: { leftBlockId: 'block-a', rightBlockId: 'block-b' },
        payload: { candidateBlockId: 'block-c', content: 'Queued paragraph.' },
      }],
    })
    expect(rich.getMarkdown()).toContain('Queued paragraph.')
  })

  it('defers resync until an in-flight edit and later structural draft are both acknowledged', async () => {
    let sequence = 0
    const requestFreshResync = vi.fn()
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}`, blockId: () => 'block-c' },
      onResyncRequired: requestFreshResync,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' edited', 8))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    await mounted.surface.reconcile({ kind: 'resync', snapshot: authoritativeSnapshot('stale-resync', {}), includedChangeIds: [] })
    expect(mounted.surface.executeStructuralCommand({ kind: 'block.insert-after', blockId: 'block-a', content: 'After resync.' })).toBe(true)
    const committed = await ackLocal(mounted.surface, batches[0])
    await vi.waitFor(() => expect(batches).toHaveLength(2))
    expect(requestFreshResync).not.toHaveBeenCalled()
    const next = await ackLocal(mounted.surface, batches[1], committed, 'revision-3')
    expect(requestFreshResync).toHaveBeenCalledOnce()
    await mounted.surface.reconcile({ kind: 'resync', snapshot: next, includedChangeIds: [] })
    expect(rich.getMarkdown()).toContain('After resync.')
    expect(batches).toHaveLength(2)
  })

  it('retains a later structural draft and rejects flush waiters when the surface becomes read-only', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}`, blockId: () => 'block-c' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' edited', 8))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(mounted.surface.executeStructuralCommand({ kind: 'block.insert-after', blockId: 'block-a', content: 'Preserved structure.' })).toBe(true)
    const flushed = mounted.surface.flush()
    const rejected = expect(flushed).rejects.toThrow()
    mounted.surface.setReadOnly(true)
    await rejected
    await ackLocal(mounted.surface, batches[0])
    expect(mounted.surface.getDraftMarkdown()).toContain('Preserved structure.')
    expect(rich.view.editable).toBe(false)
    expect(mounted.surface.executeStructuralCommand({ kind: 'block.delete', blockId: 'block-a' })).toBe(false)
  })

  it('keeps a later structural draft when the preceding save fails and retries serially', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}`, blockId: () => 'block-c' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' edited', 8))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    mounted.surface.executeStructuralCommand({ kind: 'block.insert-after', blockId: 'block-a', content: 'Preserved tail.' })
    await mounted.surface.reconcile({ kind: 'reject-local', requestId: batches[0].requestId,
      reason: { code: 'persistence-failed', message: 'not saved' }, authoritative: snapshot(), includedChangeIds: [] })
    expect(mounted.surface.getDraftMarkdown()).toContain('Preserved tail.')
    expect(rich.view.editable).toBe(true)
    expect(batches).toHaveLength(1)
    expect(mounted.surface.retryPending()).toBe(true)
    expect(batches).toHaveLength(2)
    expect(batches[1]).toEqual(batches[0])
    const committed = await ackLocal(mounted.surface, batches[1])
    await vi.waitFor(() => expect(batches).toHaveLength(3))
    expect(batches[2].operations).toMatchObject([{ kind: 'block.insert', payload: { content: 'Preserved tail.' } }])
    await ackLocal(mounted.surface, batches[2], committed, 'revision-3')
    await expect(mounted.surface.flush()).resolves.toBeUndefined()
  })

  it('releases a queued remote update when local edits cancel back to the authoritative text', async () => {
    vi.useFakeTimers()
    try {
      const mounted = await mountDocumentEditorBase(document.body, {
        snapshot: snapshot(),
        ids: { requestId: () => 'request-cancelled', operationId: () => 'operation-cancelled' },
        localChangeDebounceMs: 250,
      })
      const rich = await mocks.mountRich.mock.results[0].value
      const batches: LocalOperationBatch[] = []
      mounted.surface.observeLocalOperations((batch) => batches.push(batch))
      rich.view.dispatch(rich.view.state.tr.insertText('X', 8))
      rich.view.dispatch(rich.view.state.tr.delete(8, 9))

      await mounted.surface.reconcile({
        kind: 'apply-remote',
        change: {
          changeId: 'queued-after-cancel',
          baseRevisionId: 'revision-1',
          revisionId: 'revision-2',
          blockRevisions: { 'block-b': 'block-b/2' },
          operations: [replaceOperation(
            'queued-after-cancel/op',
            'block-b',
            'block-b/1',
            '远端更新在净零本地编辑后生效。',
          )],
        },
      })
      expect(rich.getMarkdown()).toContain('远端更新')

      await vi.advanceTimersByTimeAsync(250)

      expect(batches).toHaveLength(0)
      expect(rich.getMarkdown()).toContain('远端更新在净零本地编辑后生效。')
    } finally {
      vi.useRealTimers()
    }
  })

  it('requests a fresh resync when local edits cancel after a deferred snapshot', async () => {
    vi.useFakeTimers()
    try {
      const requestFreshResync = vi.fn()
      const mounted = await mountDocumentEditorBase(document.body, {
        snapshot: snapshot(),
        ids: { requestId: () => 'request-cancelled', operationId: () => 'operation-cancelled' },
        localChangeDebounceMs: 250,
        onResyncRequired: requestFreshResync,
      })
      const rich = await mocks.mountRich.mock.results[0].value
      rich.view.dispatch(rich.view.state.tr.insertText('X', 8))
      await mounted.surface.reconcile({
        kind: 'resync',
        snapshot: authoritativeSnapshot('revision-stale', {}),
        includedChangeIds: [],
      })
      expect(requestFreshResync).not.toHaveBeenCalled()
      rich.view.dispatch(rich.view.state.tr.delete(8, 9))

      await vi.advanceTimersByTimeAsync(250)

      expect(requestFreshResync).toHaveBeenCalledOnce()
    } finally {
      vi.useRealTimers()
    }
  })

  it('reschedules a dirty debounce after a composition starts and ends without another change', async () => {
    vi.useFakeTimers()
    try {
      const mounted = await mountDocumentEditorBase(document.body, {
        snapshot: snapshot(),
        ids: { requestId: () => 'request-composition', operationId: () => 'operation-composition' },
        localChangeDebounceMs: 250,
      })
      const rich = await mocks.mountRich.mock.results[0].value
      const batches: LocalOperationBatch[] = []
      mounted.surface.observeLocalOperations((batch) => batches.push(batch))
      rich.view.dispatch(rich.view.state.tr.insertText(' before IME', 8))
      rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))

      await vi.advanceTimersByTimeAsync(250)
      expect(batches).toHaveLength(0)
      rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
      await vi.advanceTimersByTimeAsync(16)
      await vi.advanceTimersByTimeAsync(250)

      expect(batches).toHaveLength(1)
      expect(contentOf(batches[0].operations[0])).toContain('before IME')
    } finally {
      vi.useRealTimers()
    }
  })

  it('flushes the current composition document when the surface is destroyed', async () => {
    const mounted = await mountDocumentEditorBase(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request-close-ime', operationId: () => 'operation-close-ime' },
      localChangeDebounceMs: 10_000,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('中文', 2))

    const destroyed = mounted.surface.destroy()
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(rich.destroy).not.toHaveBeenCalled()
    expect(contentOf(batches[0].operations[0])).toContain('中文')
    await ackLocal(mounted.surface, batches[0])
    await destroyed
    expect(rich.destroy).toHaveBeenCalledOnce()
  })

  it('flushes a pending debounced edit before destroying the surface', async () => {
    const mounted = await mountDocumentEditorBase(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => 'request-close', operationId: () => 'operation-close' },
      localChangeDebounceMs: 10_000,
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))

    rich.view.dispatch(rich.view.state.tr.insertText(' before close', 8))
    expect(batches).toHaveLength(0)
    const destroyed = mounted.surface.destroy()
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(rich.destroy).not.toHaveBeenCalled()
    expect(contentOf(batches[0].operations[0])).toContain('Heading before close')
    await ackLocal(mounted.surface, batches[0])
    await destroyed
    expect(rich.destroy).toHaveBeenCalledOnce()
  })

  it('preserves whole-block identity when a transaction reorders governed spans', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    const swapped = rich.view.state.schema.topNodeType.create(null, [
      rich.view.state.doc.child(1), rich.view.state.doc.child(2), rich.view.state.doc.child(0),
    ])
    rich.view.dispatch(rich.view.state.tr.replaceWith(0, rich.view.state.doc.content.size, swapped.content))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(batches[0].operations).toHaveLength(1)
    expect(batches[0].operations[0].kind).toBe('block.move')
    const committed = await ackLocal(mounted.surface, batches[0])
    expect(committed.blocks.map((block) => block.blockId)).toEqual(['block-b', 'block-a'])
    expect(committed.blocks[0].markdown).toBe(snapshot().blocks[1].markdown)
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
    expect(batches[0].operations[0]).toMatchObject({ target: { blockId: 'block-b' } })
    expect(contentOf(batches[0].operations[0])).toContain('Second paragraph.')
    expect(contentOf(batches[0].operations[0])).toContain('updated Third paragraph.')
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
    expect(batches[0].operations[0]).toMatchObject({ target: { blockId: 'block-b' } })
    expect(contentOf(batches[0].operations[0])).toContain('second: Second paragraph.')
    expect(contentOf(batches[0].operations[0])).toContain('third: Third paragraph.')
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
        operations: [replaceOperation('remote-expand', 'block-a', 'block-a/1', '# Heading\n\nExpanded context.')],
      },
    })
    expect(rich.view.dom.querySelectorAll('[data-cdr-block-id="block-a"]')).toHaveLength(2)
    expect(rich.view.state.selection.$from.index(0)).toBe(2)
    expect(rich.view.state.selection.$from.parent.textContent).toContain('Second paragraph.')

    const blockBStart = rich.view.state.doc.child(0).nodeSize + rich.view.state.doc.child(1).nodeSize
    rich.view.dispatch(rich.view.state.tr.insertText('Remote-safe ', blockBStart + 2))
    await Promise.resolve()
    expect(batches).toHaveLength(1)
    expect(batches[0].operations[0]).toMatchObject({ target: { blockId: 'block-b' } })
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

  it('keeps accepting typing while serializing durable submissions and rebases only after acknowledgement', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' first', 8))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    rich.view.dispatch(rich.view.state.tr.insertText(' second', 14))
    await Promise.resolve()
    expect(batches).toHaveLength(1)
    expect(rich.getMarkdown()).toContain('Heading first second')
    expect(rich.view.editable).toBe(true)
    const committed = await ackLocal(mounted.surface, batches[0])
    await vi.waitFor(() => expect(batches).toHaveLength(2))
    expect(rich.getMarkdown()).toContain('Heading first second')
    expect(batches[1].operations[0]).toMatchObject({ target: { expectedBlockRevision: committed.blocks[0].blockRevision } })
    await ackLocal(mounted.surface, batches[1], committed, 'revision-3')
    await expect(mounted.surface.flush()).resolves.toBeUndefined()
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
      'block-a': { blockRevision: 'block-a/2', markdown: contentOf(batches[0].operations[0]) },
    })
    await mounted.surface.reconcile({
      kind: 'ack-local', requestId: batches[0].requestId, authoritative: committed,
      includedChangeIds: [],
    })

    rich.view.dispatch(rich.view.state.tr.delete(8, 13))
    await Promise.resolve()
    expect(batches).toHaveLength(2)
    expect(batches[1].operations).toMatchObject([{
      target: { blockId: 'block-a', expectedBlockRevision: 'block-a/2' }, payload: { content: '# Heading' },
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
        operations: [replaceOperation('remote-operation', 'block-b', 'block-b/1', 'Authoritative remote payload.')],
      },
    })
    expect(rich.getMarkdown()).not.toContain('Authoritative remote payload.')

    await mounted.surface.reconcile({
      kind: 'ack-local', requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-3', {
        'block-a': { blockRevision: 'block-a/2', markdown: contentOf(batches[0].operations[0]) },
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
      operations: [replaceOperation('operation-b2', 'block-b', 'block-b/1', 'Second revision.')],
    }
    const third = {
      changeId: 'change-b3', baseRevisionId: 'revision-2', revisionId: 'revision-3',
      blockRevisions: { 'block-b': 'block-b/3' },
      operations: [replaceOperation('operation-b3', 'block-b', 'block-b/2', 'Third revision.')],
    }
    await mounted.surface.reconcile({ kind: 'apply-remote', change: second })
    await mounted.surface.reconcile({ kind: 'apply-remote', change: third })

    await mounted.surface.reconcile({
      kind: 'ack-local', requestId: batches[0].requestId,
      authoritative: authoritativeSnapshot('revision-4', {
        'block-a': { blockRevision: 'block-a/2', markdown: contentOf(batches[0].operations[0]) },
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
    expect(batches[1].operations[0]).toMatchObject({ target: { expectedBlockRevision: 'block-b/3' } })
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
      'block-a': { blockRevision: 'block-a/3', markdown: contentOf(batches[0].operations[0]) },
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
        operations: [replaceOperation('operation-3', 'block-b', 'block-b/1', 'Must not apply.')],
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

  it('retains a completed IME draft for comparison when a same-block remote commit arrived during composition', async () => {
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(), ids: { requestId: () => 'request-ime', operationId: () => 'operation-ime' },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    let state: EditorSurfaceState | undefined
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    mounted.surface.observeState((value) => { state = value })
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(rich.view.state.doc, 2)))
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('中文', 2))
    await mounted.surface.reconcile({ kind: 'apply-remote', change: {
      changeId: 'during-ime', baseRevisionId: 'revision-1', revisionId: 'revision-2',
      blockRevisions: { 'block-a': 'block-a/2' },
      operations: [replaceOperation('agent-operation', 'block-a', 'block-a/1', '# IME-safe remote heading')],
    } })
    expect(rich.getMarkdown()).not.toContain('IME-safe')
    expect(rich.getMarkdown()).toContain('中文')
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    await vi.waitFor(() => expect(state?.error?.code).toBe('stale-base'))
    expect(batches).toHaveLength(0)
    expect(mounted.surface.getDraftMarkdown()).toContain('中文')
    expect(mounted.surface.retryPending()).toBe(false)
    mounted.surface.discardDraft()
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
    expect(batches[0].operations[0]).toMatchObject({
      target: { blockId: 'block-a', expectedBlockRevision: 'block-a/1' },
    })
    expect(contentOf(batches[0].operations[0])).toContain('中文')
  })

  it('keeps both blocks when a second composition starts before the first finishing frame', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(),
      ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))

    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('甲', 2))
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))

    const blockBStart = rich.view.state.doc.child(0).nodeSize
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(rich.view.state.doc, blockBStart + 2)))
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('乙', blockBStart + 2))
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))

    await vi.waitFor(() => expect(batches).toHaveLength(1))
    expect(batches[0].operations).toHaveLength(2)
    expect(batches[0].operations).toEqual(expect.arrayContaining([
      expect.objectContaining({
        target: expect.objectContaining({ blockId: 'block-a' }),
        payload: expect.objectContaining({ content: expect.stringContaining('甲') }),
      }),
      expect.objectContaining({
        target: expect.objectContaining({ blockId: 'block-b' }),
        payload: expect.objectContaining({ content: expect.stringContaining('乙') }),
      }),
    ]))
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
    expect(contentOf(batches[0].operations[0])).toContain('中文')
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
    expect(mounted.surface.getDraftMarkdown()).toContain('中文')
    expect(requestFreshResync).not.toHaveBeenCalled()
    mounted.surface.discardDraft()
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
    expect(batches[0].operations[0]).toMatchObject({ target: { blockId: 'block-b' } })
  })

  it('retains typing after a submitted edit becomes a proposal and never treats it as committed text', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(), ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    let state: EditorSurfaceState | undefined
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    mounted.surface.observeState((value) => { state = value })
    rich.view.dispatch(rich.view.state.tr.insertText(' proposal', 8))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    rich.view.dispatch(rich.view.state.tr.insertText(' later typing', 17))
    await mounted.surface.reconcile({ kind: 'proposal-stored', requestId: batches[0].requestId,
      changeSetId: 'stored-proposal', authoritative: snapshot(), includedChangeIds: [] })
    expect(rich.getMarkdown()).toBe('# Heading\n\nSecond paragraph.\n\nThird paragraph.')
    expect(mounted.surface.getDraftMarkdown()).toContain('proposal later typing')
    expect(state?.error?.code).toBe('stale-base')
    expect(mounted.surface.retryPending()).toBe(false)
    expect(batches).toHaveLength(1)
    await expect(mounted.surface.flush()).rejects.toThrow()
    mounted.surface.discardDraft()
    expect(state?.error).toBeNull()
    expect(mounted.surface.getDraftMarkdown()).not.toContain('later typing')
  })

  it('restores an unsaved failed draft after closing and only submits it after explicit retry', async () => {
    let sequence = 0
    const ids = { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` }
    const mounted = await mountDocumentEditor(document.body, { snapshot: snapshot(), ids })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' recovered', 8))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    await mounted.surface.reconcile({ kind: 'reject-local', requestId: batches[0].requestId,
      reason: { code: 'persistence-failed', message: 'disk unavailable' }, authoritative: snapshot(), includedChangeIds: [] })
    await mounted.surface.destroy()
    const reopened = await mountDocumentEditor(document.body, { snapshot: snapshot(), ids })
    const reopenedBatches: LocalOperationBatch[] = []
    let state: EditorSurfaceState | undefined
    reopened.surface.observeState((value) => { state = value })
    reopened.surface.observeLocalOperations((batch) => reopenedBatches.push(batch))
    await Promise.resolve()
    expect(reopened.surface.getDraftMarkdown()).toContain('Heading recovered')
    expect(state?.error?.code).toBe('persistence-failed')
    expect(reopenedBatches).toHaveLength(0)
    expect(reopened.surface.retryPending()).toBe(true)
    await vi.waitFor(() => expect(reopenedBatches).toHaveLength(1))
    await ackLocal(reopened.surface, reopenedBatches[0])
    await reopened.surface.flush()
    expect(localStorage.getItem('notemd-cdr-draft:document-1')).toBeNull()
    await reopened.surface.destroy()
  })

  it('does not convert a retained stale draft into a retryable overwrite when reopened on the new committed head', async () => {
    let sequence = 0
    const ids = { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` }
    const mounted = await mountDocumentEditor(document.body, { snapshot: snapshot(), ids })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' local draft', 8))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    const remoteHead = authoritativeSnapshot('revision-remote', {
      'block-a': { markdown: '# Remote wins', blockRevision: 'block-a/remote' },
    })
    await mounted.surface.reconcile({ kind: 'reject-local', requestId: batches[0].requestId,
      reason: { code: 'stale-base', message: 'same block changed remotely' }, authoritative: remoteHead, includedChangeIds: [] })
    await mounted.surface.destroy()
    const reopened = await mountDocumentEditor(document.body, { snapshot: remoteHead, ids })
    let state: EditorSurfaceState | undefined
    reopened.surface.observeState((value) => { state = value })
    expect(state?.error?.code).toBe('stale-base')
    expect(reopened.surface.getDraftMarkdown()).toContain('local draft')
    expect(reopened.surface.retryPending()).toBe(false)
    const reopenedRich = await mocks.mountRich.mock.results[1].value
    expect(reopenedRich.getMarkdown()).toContain('Remote wins')
    reopened.surface.discardDraft()
    await reopened.surface.destroy()
  })

  it('defers an acknowledgement received during a later composition until its final input is available', async () => {
    let sequence = 0
    const mounted = await mountDocumentEditor(document.body, {
      snapshot: snapshot(), ids: { requestId: () => `request-${++sequence}`, operationId: () => `operation-${++sequence}` },
    })
    const rich = await mocks.mountRich.mock.results[0].value
    const batches: LocalOperationBatch[] = []
    mounted.surface.observeLocalOperations((batch) => batches.push(batch))
    rich.view.dispatch(rich.view.state.tr.insertText(' first', 8))
    await vi.waitFor(() => expect(batches).toHaveLength(1))
    rich.view.dispatch(rich.view.state.tr.setSelection(TextSelection.create(rich.view.state.doc, 14)))
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('中', 14))
    const committed = await ackLocal(mounted.surface, batches[0])
    expect(rich.getMarkdown()).toContain('first中')
    expect(batches).toHaveLength(1)
    rich.view.dom.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    rich.view.dispatch(rich.view.state.tr.insertText('文', 15))
    await vi.waitFor(() => expect(batches).toHaveLength(2))
    expect(rich.getMarkdown()).toContain('first中文')
    expect(batches[1].operations[0]).toMatchObject({ target: { expectedBlockRevision: committed.blocks[0].blockRevision } })
    await ackLocal(mounted.surface, batches[1], committed, 'revision-3')
    await expect(mounted.surface.flush()).resolves.toBeUndefined()
  })
})
