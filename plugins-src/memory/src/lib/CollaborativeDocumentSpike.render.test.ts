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
  mountReadOnly: [] as boolean[],
}))

vi.mock('./editor-kit-v2', () => ({
  loadKitV2: async () => async (_container: HTMLElement, options: any) => {
    mocks.mountSnapshots.push(structuredClone(options.snapshot))
    mocks.mountReadOnly.push(options.readOnly === true)
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
import { sha256Hex } from './cdr/session'

let component: ReturnType<typeof mount> | null = null
interface RepositoryRecord {
  generation: number
  aggregate: any
  markdown: string
  committedMarkdown: string
  documentId: string
}
let repository: RepositoryRecord | undefined
let repositories = new Map<string, RepositoryRecord>()
let vaultRoot = '/vault/one'
let wikiDirectory = 'wikipage'
let failNextCommit = false
let loseNextCommitReceipt = false
let failNextLoad = false

function copy<T>(value: T): T {
  return structuredClone(value)
}

async function hostRequest(method: string, params: any) {
  if (method === 'host.vault.info') return { root: vaultRoot, wiki_dir: wikiDirectory }
  const managedPath = method === 'host.cdr.repository.v2.commit'
    ? params?.representation?.vault_path
    : params?.vault_path
  const key = `${vaultRoot}:${managedPath ?? ''}`
  if (method === 'host.cdr.repository.v2.inspect') {
    repository = repositories.get(key)
    return repository ? { kind: 'located', document_id: repository.documentId } : { kind: 'missing' }
  }
  if (method === 'host.cdr.repository.v2.load') {
    if (failNextLoad) {
      failNextLoad = false
      throw new Error('read unavailable')
    }
    repository = repositories.get(key)
    if (!repository) return { kind: 'missing' }
    const committedSha = await sha256Hex(repository.committedMarkdown)
    const diskSha = await sha256Hex(repository.markdown)
    return {
      kind: 'loaded',
      generation: repository.generation,
      aggregate: copy(repository.aggregate),
      representation: {
        vault_path: params.vault_path,
        committed_sha256: committedSha,
        status: diskSha === committedSha ? 'in-sync' : 'external-drift',
        disk_sha256: diskSha,
        markdown: repository.markdown,
        profile_type: 'Memory',
      },
    }
  }
  if (method === 'host.cdr.repository.v2.commit') {
    if (failNextCommit) {
      failNextCommit = false
      throw new Error('disk unavailable')
    }
    repository = repositories.get(key)
    const currentGeneration = repository?.generation ?? 0
    if (params.expected_generation !== currentGeneration) {
      return {
        kind: 'aggregate-conflict',
        current: {
          generation: repository!.generation,
          aggregate: copy(repository!.aggregate),
          representation_sha256: await sha256Hex(repository!.committedMarkdown),
        },
      }
    }
    const expectedHash = params.representation.expected.kind === 'present'
      ? params.representation.expected.sha256
      : null
    const diskHash = repository ? await sha256Hex(repository.markdown) : null
    if (expectedHash !== diskHash) {
      return {
        kind: 'external-drift',
        current: repository ? {
          generation: repository.generation,
          aggregate: copy(repository.aggregate),
          representation_sha256: await sha256Hex(repository.committedMarkdown),
        } : null,
        representation: {
          vault_path: params.representation.vault_path,
          disk: repository
            ? { status: 'external-drift', disk_sha256: diskHash, markdown: repository.markdown }
            : { status: 'missing' },
        },
      }
    }
    repository = {
      generation: currentGeneration + 1,
      aggregate: copy(params.aggregate),
      markdown: params.representation.markdown,
      committedMarkdown: params.representation.markdown,
      documentId: params.document_id,
    }
    repositories.set(key, repository)
    if (loseNextCommitReceipt) {
      loseNextCommitReceipt = false
      throw new Error('commit receipt lost')
    }
    return {
      kind: 'committed',
      generation: repository.generation,
      representation_sha256: await sha256Hex(repository.markdown),
    }
  }
  throw new Error(`unexpected host method: ${method}`)
}

beforeEach(() => {
  repository = undefined
  repositories = new Map()
  vaultRoot = '/vault/one'
  wikiDirectory = 'wikipage'
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
  mocks.mountReadOnly.length = 0
})

async function render(createIfMissing = true) {
  const previousMounts = mocks.mountSnapshots.length
  component = mount(CollaborativeDocumentSpike, { target: document.body })
  flushSync()
  await vi.waitFor(() => expect(
    button('创建受控 MEMORY 文档') || mocks.mountSnapshots.length > previousMounts,
  ).toBeTruthy())
  if (!createIfMissing) return
  if (button('创建受控 MEMORY 文档')) {
    activate('创建受控 MEMORY 文档')
  }
  await vi.waitFor(() => expect(mocks.mountSnapshots.length).toBeGreaterThan(previousMounts))
  await vi.waitFor(() => expect(document.body.textContent).not.toContain('正在恢复共写文档'))
}

function activate(label: string) {
  const target = requireButton(label)
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
    .find((item) => item.textContent?.trim() === label)
}

function requireButton(label: string): HTMLButtonElement {
  const target = button(label)
  expect(target).toBeTruthy()
  return target!
}

function localBatch(requestId: string, blockId: string, markdown: string) {
  const snapshot = mocks.mountSnapshots.at(-1)
  const block = snapshot.blocks.find((item: any) => item.blockId === blockId)
  return {
    requestId,
    baseRevisionId: snapshot.revisionId,
    operations: [{
      kind: 'block.replace', operationId: `${requestId}/op`, blockId,
      expectedBlockRevision: block.blockRevision, markdown,
    }],
  }
}

describe('CollaborativeDocumentSpike', () => {
  it('does not create a vault file until the user explicitly asks', async () => {
    await render(false)
    expect(document.body.textContent).toContain('未创建')
    expect(document.body.textContent).toContain('只有点击创建后才会写入 vault')
    expect(repositories.size).toBe(0)
    expect(mocks.localListener).toBeNull()

    activate('创建受控 MEMORY 文档')
    await vi.waitFor(() => expect(mocks.localListener).not.toBeNull())
    expect(repositories.size).toBe(1)
    expect(repository?.markdown).toContain(`cdr:\n  document_id: ${repository!.documentId}`)
  })

  it('keeps the explicit creation panel retryable after a failed first commit', async () => {
    await render(false)
    failNextCommit = true
    activate('创建受控 MEMORY 文档')
    await vi.waitFor(() => expect(document.body.textContent).toContain('创建失败'))
    expect(button('创建受控 MEMORY 文档')).toBeTruthy()
    expect(repositories.size).toBe(0)

    activate('创建受控 MEMORY 文档')
    await vi.waitFor(() => expect(mocks.localListener).not.toBeNull())
    expect(repositories.size).toBe(1)
  })

  it('stores an Agent proposal without changing the surface, then applies it only after acceptance', async () => {
    await render()
    expect(document.body.textContent).toContain('Stage 1A')
    activate('模拟 Agent 请求直接修改')
    await vi.waitFor(() => expect(document.body.textContent).toContain('pending'))
    expect(repository?.aggregate.session.proposals[0].actorId).toBe('agent:organizer/simulated')
    expect(document.body.textContent).toContain('apply 请求已按策略降级')
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
    mocks.localListener?.(localBatch('local-request', 'b-d4e5f6', '人类局部编辑。'))
    await settle()
    await vi.waitFor(() => expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({ kind: 'ack-local', requestId: 'local-request' })))
  })

  it('preserves a stale local draft as a proposal before restoring authoritative content', async () => {
    await render()
    activate('验证 stale-base')
    await vi.waitFor(() => expect(document.body.textContent).toContain('旧提案未覆盖人类新版本'))
    mocks.reconcile.mockClear()

    mocks.localListener?.(localBatch('stale-local-request', 'b-d4e5f6', '中文组合输入形成的本地草稿。'))
    await vi.waitFor(() => expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'reject-local', requestId: 'stale-local-request',
    })))

    expect(document.body.textContent).toContain('本地文字已保存为待比较提案')
    expect(document.body.textContent).toContain('中文组合输入形成的本地草稿')
  })

  it('keeps activity visible across a paint before applying the remote fixture', async () => {
    await render()
    activate('模拟协作者修改')
    expect(mocks.setLayer).toHaveBeenCalledWith('active-run', expect.any(Array))
    expect(mocks.removeLayer).not.toHaveBeenCalledWith('active-run')

    await vi.waitFor(() => expect(mocks.removeLayer).toHaveBeenCalledWith('active-run'))
    expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({ kind: 'apply-remote' }))
  })

  it('restores committed content, pending work, assessments, audit, and decorations after remount', async () => {
    await render()
    mocks.localListener?.(localBatch('persisted-local-request', 'b-d4e5f6', '窗口重开后仍然存在的正文。'))
    await vi.waitFor(() => expect(document.body.textContent).toContain('人类局部修改已保存'))
    activate('模拟 Agent 请求直接修改')
    await vi.waitFor(() => expect(document.body.textContent).toContain('pending'))
    activate('模拟 Agent 核验背景块')
    await vi.waitFor(() => expect(document.body.textContent).toContain('结论已保存'))
    const auditCount = repository?.aggregate.session.audit.length

    if (component) await unmount(component)
    component = null
    mocks.reconcile.mockClear()
    mocks.setLayer.mockClear()
    await render()

    expect(document.body.textContent).toContain('已从受控 Markdown 与同代聚合恢复')
    expect(document.body.textContent).toContain(`${auditCount} 个审计事件`)
    expect(document.body.textContent).toContain('pending')
    expect(document.body.textContent).toContain('已核验')
    expect(mocks.mountSnapshots.at(-1).blocks.find((block: any) => block.blockId === 'b-d4e5f6').markdown)
      .toBe('窗口重开后仍然存在的正文。')
    expect(mocks.setLayer).toHaveBeenCalledWith('proposals', [expect.objectContaining({
      blockId: 'b-d4e5f6', kind: 'proposal',
    })])
  })

  it('keeps the fixed managed slot independent in each vault', async () => {
    await render()
    mocks.localListener?.(localBatch('vault-one-edit', 'b-d4e5f6', '只属于第一个 vault。'))
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

  it('opens external Markdown drift read-only without replacing either side', async () => {
    await render()
    const committedHead = copy(repository!.aggregate.session.head)
    repository!.markdown = `${repository!.markdown}外部编辑不得被静默覆盖。\n`

    if (component) await unmount(component)
    component = null
    await render()

    expect(mocks.mountReadOnly.at(-1)).toBe(true)
    expect(mocks.localListener).toBeNull()
    expect(document.body.textContent).toContain('当前阶段不支持导入')
    expect(repository?.markdown).toContain('外部编辑不得被静默覆盖')
    expect(repository?.aggregate.session.head).toEqual(committedHead)
    expect(requireButton('模拟 Agent 请求直接修改').disabled).toBe(true)
  })

  it('stops observing and locks when drift is detected during a local save', async () => {
    await render()
    const listener = mocks.localListener!
    const generation = repository!.generation
    repository!.markdown += '外部并发编辑。\n'
    listener(localBatch('runtime-drift', 'b-d4e5f6', '不得覆盖外部内容。'))

    await vi.waitFor(() => expect(mocks.setReadOnly).toHaveBeenCalledWith(true))
    expect(mocks.localListener).toBeNull()
    expect(repository!.generation).toBe(generation)
    expect(repository!.markdown).toContain('外部并发编辑')
    expect(document.body.textContent).toContain('本次修改未写入')
  })

  it('rejects a local edit when durable commit fails and never acknowledges it as saved', async () => {
    await render()
    failNextCommit = true
    mocks.localListener?.(localBatch('failed-local-request', 'b-d4e5f6', '这段内容不能被误报为已保存。'))

    await vi.waitFor(() => expect(mocks.reconcile).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'reject-local',
      requestId: 'failed-local-request',
      reason: expect.objectContaining({ code: 'persistence-failed' }),
    })))
    expect(mocks.reconcile.mock.calls.some(([update]) => update.kind === 'ack-local')).toBe(false)
    expect(document.body.textContent).toContain('保存失败')
    expect(repository?.aggregate.session.head.blocks.find((block: any) => block.blockId === 'b-d4e5f6').markdown)
      .not.toContain('不能被误报')
  })

  it('locks the editor when a committed local edit cannot be reconciled or resynced', async () => {
    await render()
    mocks.reconcile.mockRejectedValue(new Error('surface unavailable'))
    mocks.localListener?.(localBatch('saved-but-unsynced', 'b-d4e5f6', '已经持久化但表面失联。'))

    await vi.waitFor(() => expect(mocks.setReadOnly).toHaveBeenCalledWith(true))
    expect(document.body.textContent).toContain('只读')
    expect(document.body.textContent).toContain('已提交状态无法同步到编辑器')
    expect(repository?.aggregate.session.head.blocks.find((block: any) => block.blockId === 'b-d4e5f6').markdown)
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
    expect(requireButton('模拟 Agent 请求直接修改').disabled).toBe(true)
  })

  it('destroys the writable surface if switching it to read-only throws', async () => {
    await render()
    mocks.reconcile.mockRejectedValue(new Error('surface unavailable'))
    mocks.setReadOnly.mockImplementationOnce(() => { throw new Error('read-only unavailable') })
    mocks.resyncRequired?.({ changeId: 'missing-parent-change' })

    await vi.waitFor(() => expect(mocks.destroy).toHaveBeenCalled())
    expect(mocks.localListener).toBeNull()
    expect(document.querySelector('.editor-host')?.childElementCount).toBe(0)
    expect(requireButton('模拟 Agent 请求直接修改').disabled).toBe(true)
  })

  it('locks the editor when a lost commit receipt cannot be resolved by reloading', async () => {
    await render()
    loseNextCommitReceipt = true
    failNextLoad = true
    mocks.localListener?.(localBatch('outcome-unknown', 'b-d4e5f6', '提交结果需要重开后核对。'))

    await vi.waitFor(() => expect(mocks.setReadOnly).toHaveBeenCalledWith(true))
    expect(document.body.textContent).toContain('无法核定本次保存结果')
    expect(mocks.reconcile.mock.calls.some(([update]) => update.kind === 'ack-local')).toBe(false)
  })
})
