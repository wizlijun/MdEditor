// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, unmount } from 'svelte'
import type { SearchHit, SmartSearchResponse } from './lib/search/api'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  setTitle: vi.fn(async () => {}),
  setSize: vi.fn(async () => {}),
  center: vi.fn(async () => {}),
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    setTitle: mocks.setTitle,
    setSize: mocks.setSize,
    center: mocks.center,
    onFocusChanged: vi.fn(async () => () => {}),
  }),
}))
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: {
    load: vi.fn(async () => ({
      get: vi.fn(async () => undefined),
      set: vi.fn(async () => {}),
      save: vi.fn(async () => {}),
    })),
  },
}))

import SmartSearchApp from './SmartSearchApp.svelte'

let component: ReturnType<typeof mount> | null = null

function hit(name: string, line: number): SearchHit & {
  fusedScore: number
  relevanceReasons: ['strict_query']
  matchedQueries: ['strict']
} {
  return {
    path: `notes/${name}.md`,
    absPath: `/vault/notes/${name}.md`,
    line,
    lineEnd: line,
    text: `${name} release risk`,
    breadcrumb: name,
    level: 'line',
    score: 1,
    docDate: null,
    sourceRef: `notes/${name}.md#L${line}`,
    agentBy: null,
    humanVerified: false,
    origin: 'human',
    conceptType: null,
    pinned: false,
    fusedScore: 0.8,
    relevanceReasons: ['strict_query'],
    matchedQueries: ['strict'],
  }
}

function searchResponse(): SmartSearchResponse {
  const hits = [hit('alpha', 4), hit('beta', 8), hit('gamma', 12)]
  return {
    route: 'smart-fts',
    tookMs: 2,
    total: hits.length,
    hits,
    truncated: false,
    deepAvailable: false,
    extractedTerms: ['release', 'risk'],
    subqueries: [{
      id: 'strict', kind: 'strict', query: 'release risk', terms: ['release', 'risk'],
      executed: true, route: 't1-fts', hitCount: hits.length, deepUsed: false, truncated: false,
    }],
  }
}

function installInvokeMock(): void {
  mocks.invoke.mockImplementation(async (command: string, args?: Record<string, any>) => {
    if (command === 'get_plugin_manifests') {
      return [{ id: 'notemd.test-agent', name: 'Test Agent', agent_provider: true }]
    }
    if (command === 'plugin_v2_execute') {
      if (args?.command === 'harness-status') {
        return { harness: 'Test Harness', ok: true, default_model: 'test-model' }
      }
      if (args?.command === 'run-task') return { run_id: 'run-1' }
      if (args?.command === 'run-status') {
        return {
          state: 'done',
          record: { status: 'success', usage: null },
          terminal_result: { complete: true, content: 'Ship after fixing the blocker. [S1]' },
        }
      }
    }
    if (command === 'notemd_smart_search') {
      if (args?.query === 'new question') {
        const response = searchResponse()
        response.hits = [hit('new-evidence', 21)]
        response.total = 1
        return response
      }
      return searchResponse()
    }
    if (command === 'smart_search_memory_context') {
      return { available: false, selected: [], excludedSummary: {}, manifestId: null, error: null }
    }
    if (command === 'smart_search_archive_answer') {
      return { path: '/vault/answers/2026-09-03-answer-release-risk.md', created: true }
    }
    if (command === 'smart_search_write_document') {
      return { path: '/vault/answers/2026-09-03-release-risk.md', created: true }
    }
    return undefined
  })
}

async function mountReady(): Promise<HTMLTextAreaElement> {
  component = mount(SmartSearchApp, { target: document.body })
  await vi.waitFor(() => {
    expect(document.querySelector<HTMLSelectElement>('.agent-select')?.value)
      .toBe('notemd.test-agent')
  })
  return document.querySelector<HTMLTextAreaElement>('.query-input')!
}

async function typeAndWait(input: HTMLTextAreaElement, value = 'release risk'): Promise<void> {
  input.value = value
  input.dispatchEvent(new InputEvent('input', { bubbles: true, data: value }))
  await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(3), {
    timeout: 1_000,
  })
}

beforeEach(() => {
  localStorage.clear()
  document.body.innerHTML = ''
  vi.clearAllMocks()
  installInvokeMock()
})

afterEach(async () => {
  if (component) await unmount(component)
  component = null
  document.body.innerHTML = ''
  localStorage.clear()
})

describe('SmartSearchApp interaction wiring', () => {
  it('searches while typing and Return invokes the Agent with the frozen sources', async () => {
    const input = await mountReady()
    await typeAndWait(input)

    expect(mocks.invoke).toHaveBeenCalledWith('notemd_smart_search', expect.objectContaining({
      query: 'release risk',
      deep: false,
    }))

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await vi.waitFor(() => {
      expect(document.querySelector('.answer-body')?.textContent)
        .toContain('Ship after fixing the blocker.')
    })
    expect(localStorage.length).toBe(0)

    const runCall = mocks.invoke.mock.calls.find((call) => (
      call[0] === 'plugin_v2_execute' && call[1]?.command === 'run-task'
    ))
    expect(runCall?.[1]?.context?.task).toBe('search-answer')
    expect(runCall?.[1]?.context?.prompt).toContain('[S1]')
    expect(runCall?.[1]?.context?.prompt).toContain('notes/alpha.md')

    document.querySelector<HTMLButtonElement>('[title="Helpful"]')?.click()
    await vi.waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith('smart_search_archive_answer', expect.objectContaining({
        payload: expect.objectContaining({ query: 'release risk', answer: expect.stringContaining('[S1]') }),
      }))
    })
  })

  it('IME tail Return and Shift+Return never start the Agent', async () => {
    const input = await mountReady()
    input.value = '发布风险'
    input.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }))
    input.dispatchEvent(new InputEvent('input', { bubbles: true, data: '发布风险' }))
    input.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }))
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await new Promise((resolve) => setTimeout(resolve, 70))
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, shiftKey: true }))
    await new Promise((resolve) => setTimeout(resolve, 0))

    expect(mocks.invoke.mock.calls.some((call) => (
      call[0] === 'plugin_v2_execute' && call[1]?.command === 'run-task'
    ))).toBe(false)
  })

  it('Return during the debounce window flushes the exact new query instead of answering from old hits', async () => {
    const input = await mountReady()
    await typeAndWait(input, 'old question')

    input.value = 'new question'
    input.dispatchEvent(new InputEvent('input', { bubbles: true, data: 'new question' }))
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(0))
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))

    await vi.waitFor(() => expect(document.querySelector('.answer-body')).not.toBeNull())
    const searchCalls = mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_smart_search')
    expect(searchCalls.at(-1)?.[1]?.query).toBe('new question')
    const runCall = mocks.invoke.mock.calls.find((call) => (
      call[0] === 'plugin_v2_execute' && call[1]?.command === 'run-task'
    ))
    expect(runCall?.[1]?.context?.prompt).toContain('new question')
    expect(runCall?.[1]?.context?.prompt).toContain('notes/new-evidence.md')
    expect(runCall?.[1]?.context?.prompt).not.toContain('notes/alpha.md')
  })

  it('Cmd/Ctrl multi-select removes only this result list and Undo restores the batch', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    const rows = Array.from(document.querySelectorAll<HTMLElement>('.result-row'))
    rows[0].click()
    rows[1].dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }))

    await vi.waitFor(() => expect(document.querySelector('.selection-bar')?.textContent).toContain('2 selected'))
    document.querySelector<HTMLButtonElement>('.selection-bar button')?.click()
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(1))

    expect(mocks.invoke.mock.calls.some((call) => /remove|delete/i.test(String(call[0])))).toBe(false)
    document.querySelector<HTMLButtonElement>('.selection-bar button')?.click()
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(3))
  })

  it('Cmd/Ctrl+A selects visible results and Cmd/Ctrl+Return keeps the search window open', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    const first = document.querySelector<HTMLElement>('.result-row')!

    first.dispatchEvent(new KeyboardEvent('keydown', { key: 'a', metaKey: true, bubbles: true }))
    await vi.waitFor(() => expect(document.querySelector('.selection-bar')?.textContent).toContain('3 selected'))

    first.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', metaKey: true, bubbles: true }))
    await vi.waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith('editor_show_and_reveal_search_hit', expect.any(Object))
    })
    expect(mocks.invoke.mock.calls.some((call) => call[0] === 'hide_smart_search_window')).toBe(false)

    first.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true }))
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(0))
    expect(document.querySelector<HTMLButtonElement>('.ask-button')?.disabled).toBe(true)
  })

  it('re-authorizes memory for a detailed document and opens it through the durable reveal path', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await vi.waitFor(() => expect(document.querySelector('.answer-body')).not.toBeNull())

    document.querySelector<HTMLButtonElement>('.document-button')?.click()
    await vi.waitFor(() => expect(document.querySelector('.document-dialog')).not.toBeNull())
    document.querySelector<HTMLButtonElement>('.document-dialog .primary')?.click()

    await vi.waitFor(() => {
      expect(mocks.invoke).toHaveBeenCalledWith('smart_search_write_document', expect.any(Object))
    })
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'smart_search_memory_context'))
      .toHaveLength(2)
    expect(mocks.invoke).toHaveBeenCalledWith('editor_show_and_reveal_search_hit', {
      path: '/vault/answers/2026-09-03-release-risk.md',
      line: 1,
      anchor: 'release risk',
    })
  })
})
