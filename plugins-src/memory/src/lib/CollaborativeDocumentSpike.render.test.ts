// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushSync, mount, tick, unmount } from 'svelte'

const mocks = vi.hoisted(() => ({
  reconcile: vi.fn(async (_update: any) => {}),
  observe: vi.fn(),
  destroy: vi.fn(async () => {}),
  setReadOnly: vi.fn(),
  setLayer: vi.fn(),
  removeLayer: vi.fn(),
  localListener: null as ((batch: unknown) => void) | null,
  resyncRequired: null as ((reason: { changeId?: string }) => void) | null,
  mountSnapshots: [] as any[],
}))

vi.mock('./editor-kit-v2', () => ({
  loadKitV2: async () => async (_container: HTMLElement, options: any) => {
    mocks.mountSnapshots.push(structuredClone(options.snapshot))
    mocks.resyncRequired = options.onResyncRequired
    return {
      surface: {
        reconcile: mocks.reconcile,
        observeLocalOperations(listener: (batch: unknown) => void) {
          mocks.localListener = listener
          return () => { mocks.localListener = null }
        },
        setReadOnly: mocks.setReadOnly,
        destroy: mocks.destroy,
      },
      decorations: { setLayer: mocks.setLayer, removeLayer: mocks.removeLayer },
    }
  },
  replaceOperation: (ids: { operationId(): string }, blockId: string, expectedBlockRevision: string, markdown: string) => ({
    kind: 'block.replace', operationId: ids.operationId(), blockId, expectedBlockRevision, markdown,
  }),
}))

import CollaborativeDocumentSpike from './CollaborativeDocumentSpike.svelte'

let component: ReturnType<typeof mount> | null = null
let repository: { generation: number; aggregate: any } | undefined
let repositories = new Map<string, { generation: number; aggregate: any }>()
let vaultRoot = '/vault/one'
let failNextCommit = false
let loseNextCommitReceipt = false
let failNextLoad = false

function copy<T>(value: T): T {
  return structuredClone(value)
}

async function hostRequest(method: string, params: any) {
  if (method === 'host.vault.info') return { root: vaultRoot }
  if (method === 'host.cdr.repository.v1.load') {
    if (failNextLoad) {
      failNextLoad = false
      throw new Error('read unavailable')
    }
    repository = repositories.get(params.document_id)
    return repository
      ? { kind: 'loaded', generation: repository.generation, aggregate: copy(repository.aggregate) }
      : { kind: 'missing' }
  }
  if (method === 'host.cdr.repository.v1.commit') {
    if (failNextCommit) {
      failNextCommit = false
      throw new Error('disk unavailable')
    }
    repository = repositories.get(params.document_id)
    const currentGeneration = repository?.generation ?? 0
    if (params.expected_generation !== currentGeneration) {
      return { kind: 'conflict', current: copy(repository ?? { generation: 0, aggregate: {} }) }
    }
    repository = { generation: currentGeneration + 1, aggregate: copy(params.aggregate) }
    repositories.set(params.document_id, repository)
    if (loseNextCommitReceipt) {
      loseNextCommitReceipt = false
      throw new Error('commit receipt lost')
    }
    return { kind: 'committed', generation: repository.generation }
  }
  throw new Error(`unexpected host method: ${method}`)
}

beforeEach(() => {
  repository = undefined
  repositories = new Map()
  vaultRoot = '/vault/one'
  failNextCommit = false
  loseNextCommitReceipt = false
  failNextLoad = false
  window.notemd = {
    pluginId: 'notemd.memory',
    locale: 'zh',
    theme: 'system',
    request: hostRequest,
    onMessage: () => {},
  }
})

afterEach(async () => {
  if (component) await unmount(component)
  component = null
  document.body.innerHTML = ''
  mocks.reconcile.mockReset()
  mocks.reconcile.mockResolvedValue(undefined)
  mocks.setLayer.mockClear()
  mocks.removeLayer.mockClear()
  mocks.destroy.mockClear()
  mocks.setReadOnly.mockClear()
  mocks.localListener = null
  mocks.resyncRequired = null
  mocks.mountSnapshots.length = 0
})

async function render() {
  component = mount(CollaborativeDocumentSpike, { target: document.body })
  flushSync()
  await Promise.resolve()
  await Promise.resolve()
  await tick()
  flushSync()
  await vi.waitFor(() => expect(mocks.localListener).not.toBeNull())
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
    await vi.waitFor(() => expect(document.body.textContent).toContain('pending'))
    expect(mocks.reconcile).not.toHaveBeenCalled()

    activate('接受')
    await vi.waitFor(() => expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({ kind: 'apply-remote' })))
    expect(document.body.textContent).toContain('applied')
  })

  it('shows a stale proposal conflict and never sends its old content to the editor', async () => {
    await render()
    activate('验证 stale-base')
    await vi.waitFor(() => expect(document.body.textContent).toContain('旧提案未覆盖人类新版本'))
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
    await vi.waitFor(() => expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({ kind: 'ack-local', requestId: 'local-request' })))
  })

  it('preserves a stale local draft as a proposal before restoring authoritative content', async () => {
    await render()
    activate('验证 stale-base')
    await vi.waitFor(() => expect(document.body.textContent).toContain('旧提案未覆盖人类新版本'))
    mocks.reconcile.mockClear()

    mocks.localListener?.({
      requestId: 'stale-local-request',
      baseRevisionId: 'memory-spike/revision-1',
      operations: [{
        kind: 'block.replace', operationId: 'stale-local-operation', blockId: 'b-d4e5f6',
        expectedBlockRevision: 'b-d4e5f6/1', markdown: '中文组合输入形成的本地草稿。',
      }],
    })
    await vi.waitFor(() => expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'reject-local', requestId: 'stale-local-request',
    })))

    expect(document.body.textContent).toContain('本地文字已保存为待比较提案')
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

  it('restores committed content, pending work, assessments, audit, and decorations after remount', async () => {
    await render()
    mocks.localListener?.({
      requestId: 'persisted-local-request',
      baseRevisionId: 'memory-spike/revision-1',
      operations: [{
        kind: 'block.replace', operationId: 'persisted-local-operation', blockId: 'b-d4e5f6',
        expectedBlockRevision: 'b-d4e5f6/1', markdown: '窗口重开后仍然存在的正文。',
      }],
    })
    await vi.waitFor(() => expect(document.body.textContent).toContain('人类局部修改已保存'))
    activate('Agent A 提出建议')
    await vi.waitFor(() => expect(document.body.textContent).toContain('pending'))
    activate('核验背景块')
    await vi.waitFor(() => expect(document.body.textContent).toContain('核验结论已保存'))
    const auditCount = repository?.aggregate.audit.length

    if (component) await unmount(component)
    component = null
    mocks.reconcile.mockClear()
    mocks.setLayer.mockClear()
    await render()

    expect(document.body.textContent).toContain('已恢复上次提交')
    expect(document.body.textContent).toContain(`${auditCount} 个审计事件`)
    expect(document.body.textContent).toContain('pending')
    expect(document.body.textContent).toContain('已核验')
    expect(mocks.mountSnapshots.at(-1).blocks.find((block: any) => block.blockId === 'b-d4e5f6').markdown)
      .toBe('窗口重开后仍然存在的正文。')
    expect(mocks.setLayer).toHaveBeenCalledWith('proposals', [expect.objectContaining({
      blockId: 'b-d4e5f6', kind: 'proposal',
    })])
  })

  it('uses a different persisted document namespace for each vault', async () => {
    await render()
    mocks.localListener?.({
      requestId: 'vault-one-edit',
      baseRevisionId: 'memory-spike/revision-1',
      operations: [{
        kind: 'block.replace', operationId: 'vault-one-edit/op', blockId: 'b-d4e5f6',
        expectedBlockRevision: 'b-d4e5f6/1', markdown: '只属于第一个 vault。',
      }],
    })
    await vi.waitFor(() => expect(document.body.textContent).toContain('人类局部修改已保存'))
    const firstDocumentId = mocks.mountSnapshots.at(-1).documentId

    if (component) await unmount(component)
    component = null
    vaultRoot = '/vault/two'
    await render()

    expect(mocks.mountSnapshots.at(-1).documentId).not.toBe(firstDocumentId)
    expect(mocks.mountSnapshots.at(-1).blocks.find((block: any) => block.blockId === 'b-d4e5f6').markdown)
      .not.toContain('第一个 vault')
    expect(repositories.size).toBe(2)
  })

  it('rejects a local edit when durable commit fails and never acknowledges it as saved', async () => {
    await render()
    failNextCommit = true
    mocks.localListener?.({
      requestId: 'failed-local-request',
      baseRevisionId: 'memory-spike/revision-1',
      operations: [{
        kind: 'block.replace', operationId: 'failed-local-operation', blockId: 'b-d4e5f6',
        expectedBlockRevision: 'b-d4e5f6/1', markdown: '这段内容不能被误报为已保存。',
      }],
    })

    await vi.waitFor(() => expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'reject-local',
      requestId: 'failed-local-request',
      reason: expect.objectContaining({ code: 'persistence-failed' }),
    })))
    expect(mocks.reconcile.mock.calls.some(([update]) => update.kind === 'ack-local')).toBe(false)
    expect(document.body.textContent).toContain('保存失败')
    expect(repository?.aggregate.head.blocks.find((block: any) => block.blockId === 'b-d4e5f6').markdown)
      .not.toContain('不能被误报')
  })

  it('locks the editor when a committed local edit cannot be reconciled or resynced', async () => {
    await render()
    mocks.reconcile.mockRejectedValue(new Error('surface unavailable'))
    mocks.localListener?.({
      requestId: 'saved-but-unsynced',
      baseRevisionId: 'memory-spike/revision-1',
      operations: [{
        kind: 'block.replace', operationId: 'saved-but-unsynced/op', blockId: 'b-d4e5f6',
        expectedBlockRevision: 'b-d4e5f6/1', markdown: '已经持久化但表面失联。',
      }],
    })

    await vi.waitFor(() => expect(mocks.setReadOnly).toHaveBeenCalledWith(true))
    expect(document.body.textContent).toContain('只读')
    expect(document.body.textContent).toContain('已提交状态无法同步到编辑器')
    expect(repository?.aggregate.head.blocks.find((block: any) => block.blockId === 'b-d4e5f6').markdown)
      .toBe('已经持久化但表面失联。')
  })

  it('locks the editor when an editor-requested resync cannot reach the authoritative snapshot', async () => {
    await render()
    mocks.reconcile.mockRejectedValue(new Error('surface unavailable'))
    mocks.resyncRequired?.({ changeId: 'missing-parent-change' })

    await vi.waitFor(() => expect(mocks.setReadOnly).toHaveBeenCalledWith(true))
    expect(document.body.textContent).toContain('只读')
    expect(document.body.textContent).toContain('已提交状态无法同步到编辑器')
    expect(document.body.textContent).not.toContain('已从当前权威快照重新同步')
    expect(button('Agent A 提出建议').disabled).toBe(true)
  })

  it('locks the editor when a lost commit receipt cannot be resolved by reloading', async () => {
    await render()
    loseNextCommitReceipt = true
    failNextLoad = true
    mocks.localListener?.({
      requestId: 'outcome-unknown',
      baseRevisionId: 'memory-spike/revision-1',
      operations: [{
        kind: 'block.replace', operationId: 'outcome-unknown/op', blockId: 'b-d4e5f6',
        expectedBlockRevision: 'b-d4e5f6/1', markdown: '提交结果需要重开后核对。',
      }],
    })

    await vi.waitFor(() => expect(mocks.setReadOnly).toHaveBeenCalledWith(true))
    expect(document.body.textContent).toContain('无法核定本次保存结果')
    expect(mocks.reconcile.mock.calls.some(([update]) => update.kind === 'ack-local')).toBe(false)
  })
})
