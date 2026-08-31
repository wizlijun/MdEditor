import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reduceEvents } from './domain'
import type { NextWorkspace, WorkspaceItem } from './repository'

const mocks = vi.hoisted(() => ({
  loadWorkspace: vi.fn(),
  appendEvent: vi.fn(),
  createIdeaSource: vi.fn(),
  openSource: vi.fn(),
}))

vi.mock('./repository', async (importOriginal) => {
  const original = await importOriginal<typeof import('./repository')>()
  return {
    ...original,
    loadWorkspace: mocks.loadWorkspace,
    appendEvent: mocks.appendEvent,
    createIdeaSource: mocks.createIdeaSource,
    openSource: mocks.openSource,
  }
})

import { createIdea, place, refresh, state } from './store.svelte'

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
    projection: reduceEvents([]),
    sources: [],
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
