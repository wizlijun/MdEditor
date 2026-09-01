import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reduceEvents } from './domain'
import type { NextWorkspace, WorkspaceItem } from './repository'

const mocks = vi.hoisted(() => ({
  loadWorkspace: vi.fn(),
  appendEvent: vi.fn(),
  createIdeaSource: vi.fn(),
  createTaskSource: vi.fn(),
  openSource: vi.fn(),
}))

vi.mock('./repository', async (importOriginal) => {
  const original = await importOriginal<typeof import('./repository')>()
  return {
    ...original,
    loadWorkspace: mocks.loadWorkspace,
    appendEvent: mocks.appendEvent,
    createIdeaSource: mocks.createIdeaSource,
    createTaskSource: mocks.createTaskSource,
    openSource: mocks.openSource,
  }
})

import { createIdea, createTask, place, refresh, state } from './store.svelte'

const capture: WorkspaceItem = {
  key: 'inbox/ideas/a-idea.md',
  state: 'capture',
  title: 'Idea',
  path: 'inbox/ideas/a-idea.md',
  created: '2026-08-29T01:00:00Z',
  proofed: false,
  orphan: false,
  relinkCandidates: [],
  relinkMatch: null,
}

function workspace(): NextWorkspace {
  return {
    ledger: { type: 'Next', version: 1, source_dirs: ['inbox/ideas'], events: [], extra: {} },
    ledgerRaw: 'loaded',
    sourceDirs: ['inbox/ideas'],
    ideaDir: 'inbox/ideas',
    taskDir: 'inbox/tasks',
    projection: reduceEvents([]),
    sources: [],
    taskSources: [],
    items: [capture],
    capture: [capture],
    wip: [],
    waiting: [],
    dormant: [],
    closed: [],
    unsupported: [],
    projectOptions: [],
    scanErrors: [],
    readOnlyError: null,
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  state.workspace = workspace()
  state.loading = false
  state.saving = false
  state.error = null
})

describe('Next store IO serialization', () => {
  it('creates a Task in Inbox without appending a lifecycle event', async () => {
    const refreshed = workspace()
    const taskItem: WorkspaceItem = {
      ...capture,
      key: 'task-id',
      kind: 'task',
      item_id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8',
      title: '提交构建',
      path: 'inbox/tasks/submit-task.md',
      task: { version: 1, id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8' },
    }
    refreshed.items = [taskItem]
    refreshed.capture = [taskItem]
    mocks.createTaskSource.mockResolvedValueOnce({
      path: taskItem.path,
      content: 'document',
      source: { path: taskItem.path, task: taskItem.task },
    })
    mocks.loadWorkspace.mockResolvedValueOnce(refreshed)

    await expect(createTask({ title: '提交构建' }, false)).resolves.toEqual({
      path: 'inbox/tasks/submit-task.md',
      placedCurrent: false,
    })
    expect(mocks.appendEvent).not.toHaveBeenCalled()
    expect(state.workspace).toBe(refreshed)
  })

  it('reports a refresh warning without misreporting a durably published Task as creation failure', async () => {
    mocks.createTaskSource.mockResolvedValueOnce({
      path: 'inbox/tasks/published-task.md',
      content: 'document',
      source: {
        path: 'inbox/tasks/published-task.md',
        task: { version: 1, id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8' },
      },
    })
    mocks.loadWorkspace.mockRejectedValueOnce(new Error('refresh unavailable'))

    await expect(createTask({ title: '已发布任务' }, false)).resolves.toEqual({
      path: 'inbox/tasks/published-task.md',
      placedCurrent: false,
      refreshError: 'Error: refresh unavailable',
    })
    expect(state.error).toBeNull()
  })

  it('marks a newly created Task current using source-first placement defaults', async () => {
    const inbox = workspace()
    const taskItem: WorkspaceItem = {
      ...capture,
      key: 'task-id',
      kind: 'task',
      item_id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8',
      title: '提交构建',
      path: 'inbox/tasks/submit-task.md',
      task: {
        version: 1,
        id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8',
        done_when: '构建可安装',
      },
    }
    inbox.items = [taskItem]
    inbox.capture = [taskItem]
    const current = workspace()
    current.wip = [taskItem]
    mocks.createTaskSource.mockResolvedValueOnce({
      path: taskItem.path,
      content: 'document',
      source: { path: taskItem.path, task: taskItem.task },
    })
    mocks.loadWorkspace.mockResolvedValueOnce(inbox)
    mocks.appendEvent.mockResolvedValueOnce(current)

    await expect(createTask({
      title: '提交构建',
      done_when: '构建可安装',
    }, true)).resolves.toEqual({ path: taskItem.path, placedCurrent: true })
    expect(mocks.appendEvent).toHaveBeenCalledWith(inbox, expect.objectContaining({
      item_kind: 'task',
      item_id: taskItem.item_id,
      action: 'commit',
      commitment: '提交构建',
      next_action: '提交构建',
      close_condition: '构建可安装',
    }), { hardWipLimit: true })
    expect(state.workspace).toBe(current)
  })

  it('never marks a same-id repair card current after refresh detects a collision', async () => {
    const inbox = workspace()
    const repair: WorkspaceItem = {
      ...capture,
      key: 'task-repair:inbox/tasks/collision-task.md',
      kind: 'task',
      item_id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8',
      item_kind: 'task',
      state: 'unsupported',
      title: '冲突任务',
      path: 'inbox/tasks/collision-task.md',
      task: { version: 1, id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8' },
      repairReason: 'duplicate task.id',
    }
    inbox.items = [repair]
    inbox.capture = []
    inbox.unsupported = [repair]
    mocks.createTaskSource.mockResolvedValueOnce({
      path: 'inbox/tasks/new-task.md',
      content: 'document',
      source: {
        path: 'inbox/tasks/new-task.md',
        task: repair.task!,
      },
    })
    mocks.loadWorkspace.mockResolvedValue(inbox)

    await expect(createTask({ title: '新任务', done_when: '可验收' }, true)).resolves.toMatchObject({
      path: 'inbox/tasks/new-task.md',
      placedCurrent: false,
      placementError: expect.stringContaining('not available for placement'),
    })
    expect(mocks.appendEvent).not.toHaveBeenCalled()
    expect(state.workspace).toBe(inbox)
  })

  it('keeps the Task in Inbox and returns a distinguishable result when current placement fails', async () => {
    const inbox = workspace()
    const taskItem: WorkspaceItem = {
      ...capture,
      key: 'task-id',
      kind: 'task',
      item_id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8',
      title: '提交构建',
      path: 'inbox/tasks/submit-task.md',
      task: { version: 1, id: '8afad9c5-07ac-4e4d-8d1e-4ed04c06f2d8' },
    }
    inbox.items = [taskItem]
    inbox.capture = [taskItem]
    mocks.createTaskSource.mockResolvedValueOnce({
      path: taskItem.path,
      content: 'document',
      source: { path: taskItem.path, task: taskItem.task },
    })
    mocks.loadWorkspace.mockResolvedValue(inbox)
    mocks.appendEvent.mockRejectedValueOnce(new Error('WIP is full'))

    await expect(createTask({ title: '提交构建', done_when: '构建可安装' }, true)).resolves.toEqual({
      path: taskItem.path,
      placedCurrent: false,
      placementError: 'Error: WIP is full',
    })
    expect(mocks.loadWorkspace).toHaveBeenCalledTimes(2)
    expect(state.workspace).toBe(inbox)
    expect(state.error).toBeNull()
  })
  it('creates one source idea, refreshes the projection, and returns its path', async () => {
    const refreshed = workspace()
    mocks.createIdeaSource.mockResolvedValueOnce({
      path: 'inbox/ideas/2026-08-30-0905-idea.md',
      content: 'document',
    })
    mocks.loadWorkspace.mockResolvedValueOnce(refreshed)

    await expect(createIdea('一个念头')).resolves.toBe('inbox/ideas/2026-08-30-0905-idea.md')
    expect(mocks.createIdeaSource).toHaveBeenCalledWith('一个念头')
    expect(state.workspace).toBe(refreshed)
    expect(state.saving).toBe(false)
  })

  it('preserves the current workspace and reports an error when creation fails', async () => {
    const before = state.workspace
    mocks.createIdeaSource.mockRejectedValueOnce(new Error('disk full'))
    await expect(createIdea('一个念头')).rejects.toThrow('disk full')
    expect(state.workspace).toBe(before)
    expect(state.error).toContain('disk full')
    expect(mocks.loadWorkspace).not.toHaveBeenCalled()
  })

  it('blocks placement while a focus refresh may be persisting source directories', async () => {
    let finishLoad!: (value: NextWorkspace) => void
    mocks.loadWorkspace.mockImplementationOnce(() => new Promise<NextWorkspace>((resolve) => {
      finishLoad = resolve
    }))

    const refreshing = refresh()
    expect(state.saving).toBe(true)
    await expect(place(capture, {
      route: 'commit',
      commitment: 'Validate',
      next_action: 'Run a test',
      close_condition: 'Evidence exists',
    })).rejects.toThrow('already saving')
    expect(mocks.appendEvent).not.toHaveBeenCalled()

    finishLoad(workspace())
    await refreshing
    expect(state.saving).toBe(false)
  })
})
