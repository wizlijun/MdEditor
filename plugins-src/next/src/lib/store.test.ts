import { beforeEach, describe, expect, it, vi } from 'vitest'
import { reduceEvents } from './domain'
import type { NextWorkspace, WorkspaceItem } from './repository'

const mocks = vi.hoisted(() => ({
  loadWorkspace: vi.fn(),
  appendEvent: vi.fn(),
  openSource: vi.fn(),
}))

vi.mock('./repository', async (importOriginal) => {
  const original = await importOriginal<typeof import('./repository')>()
  return {
    ...original,
    loadWorkspace: mocks.loadWorkspace,
    appendEvent: mocks.appendEvent,
    openSource: mocks.openSource,
  }
})

import { place, refresh, state } from './store.svelte'

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
    projection: reduceEvents([]),
    sources: [],
    items: [capture],
    capture: [capture],
    wip: [],
    waiting: [],
    dormant: [],
    closed: [],
    unsupported: [],
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
