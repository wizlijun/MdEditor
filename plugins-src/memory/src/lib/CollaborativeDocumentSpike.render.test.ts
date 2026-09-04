// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'

const mocks = vi.hoisted(() => ({
  reconcile: vi.fn(async (_update: any) => {}),
  observe: vi.fn(),
  destroy: vi.fn(async () => {}),
  setLayer: vi.fn(),
  removeLayer: vi.fn(),
  localListener: null as ((batch: unknown) => void) | null,
}))

vi.mock('./editor-kit-v2', () => ({
  loadKitV2: async () => async () => ({
    surface: {
      reconcile: mocks.reconcile,
      observeLocalOperations(listener: (batch: unknown) => void) {
        mocks.localListener = listener
        return () => { mocks.localListener = null }
      },
      setReadOnly: vi.fn(),
      destroy: mocks.destroy,
    },
    decorations: { setLayer: mocks.setLayer, removeLayer: mocks.removeLayer },
  }),
  replaceOperation: (ids: { operationId(): string }, blockId: string, expectedBlockRevision: string, markdown: string) => ({
    kind: 'block.replace', operationId: ids.operationId(), blockId, expectedBlockRevision, markdown,
  }),
}))

import CollaborativeDocumentSpike from './CollaborativeDocumentSpike.svelte'

let component: ReturnType<typeof mount> | null = null

afterEach(async () => {
  if (component) await unmount(component)
  component = null
  document.body.innerHTML = ''
  mocks.reconcile.mockClear()
  mocks.setLayer.mockClear()
  mocks.removeLayer.mockClear()
  mocks.destroy.mockClear()
  mocks.localListener = null
})

async function render() {
  component = mount(CollaborativeDocumentSpike, { target: document.body })
  flushSync()
  await Promise.resolve()
  await Promise.resolve()
  await tick()
  flushSync()
}

function activate(label: string) {
  const target = button(label)
  expect(target.disabled).toBe(false)
  target.dispatchEvent(new MouseEvent('click', { bubbles: true }))
  flushSync()
}

async function settle() {
  await Promise.resolve()
  await Promise.resolve()
  await tick()
  flushSync()
}

function button(label: string) {
  return Array.from(document.querySelectorAll<HTMLButtonElement>('button'))
    .find((item) => item.textContent?.trim() === label)!
}

describe('CollaborativeDocumentSpike', () => {
  it('stores an Agent proposal without changing the surface, then applies it only after acceptance', async () => {
    await render()
    expect(document.body.textContent).toContain('Stage 0')
    activate('Agent A 提出建议')
    await settle()
    expect(document.body.textContent).toContain('pending')
    expect(mocks.reconcile).not.toHaveBeenCalled()

    activate('接受')
    await settle()
    expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({ kind: 'apply-remote' }))
    expect(document.body.textContent).toContain('applied')
  })

  it('shows a stale proposal conflict and never sends its old content to the editor', async () => {
    await render()
    activate('验证 stale-base')
    await settle()
    expect(document.body.textContent).toContain('旧提案未覆盖人类新版本')
    expect(document.body.textContent).toContain('conflicted')
    const remoteUpdates = mocks.reconcile.mock.calls
      .map(([update]) => update)
      .filter((update) => update.kind === 'apply-remote')
    expect(remoteUpdates).toHaveLength(1)
    expect(remoteUpdates[0].change.operations[0].markdown).toContain('人类已先一步')
  })

  it('acks a local editor operation without feeding it back as a remote change', async () => {
    await render()
    mocks.localListener?.({
      requestId: 'local-request',
      baseRevisionId: 'memory-spike/revision-1',
      operations: [{
        kind: 'block.replace', operationId: 'local-operation', blockId: 'b-d4e5f6',
        expectedBlockRevision: 'b-d4e5f6/1', markdown: '人类局部编辑。',
      }],
    })
    await settle()
    expect(mocks.reconcile).toHaveBeenCalledTimes(1)
    expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({ kind: 'ack-local', requestId: 'local-request' }))
  })

  it('preserves a stale local draft as a proposal before restoring authoritative content', async () => {
    await render()
    activate('验证 stale-base')
    await settle()
    mocks.reconcile.mockClear()

    mocks.localListener?.({
      requestId: 'stale-local-request',
      baseRevisionId: 'memory-spike/revision-1',
      operations: [{
        kind: 'block.replace', operationId: 'stale-local-operation', blockId: 'b-d4e5f6',
        expectedBlockRevision: 'b-d4e5f6/1', markdown: '中文组合输入形成的本地草稿。',
      }],
    })
    await settle()

    expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'reject-local', requestId: 'stale-local-request',
    }))
    expect(document.body.textContent).toContain('本地文字已保留为待比较提案')
    expect(document.body.textContent).toContain('中文组合输入形成的本地草稿')
  })

  it('keeps activity visible across a paint before applying the remote fixture', async () => {
    await render()
    activate('模拟已授权远端变更')
    expect(mocks.setLayer).toHaveBeenCalledWith('active-run', expect.any(Array))
    expect(mocks.removeLayer).not.toHaveBeenCalledWith('active-run')

    await vi.waitFor(() => expect(mocks.removeLayer).toHaveBeenCalledWith('active-run'))
    expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({ kind: 'apply-remote' }))
  })
})
