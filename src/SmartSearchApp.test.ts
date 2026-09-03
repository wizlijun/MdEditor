// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, unmount } from 'svelte'
import type { SearchHit, SmartSearchHit, SmartSearchResponse } from './lib/search/api'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  writeText: vi.fn(async () => {}),
  setTitle: vi.fn(async () => {}),
  setSize: vi.fn(async () => {}),
  center: vi.fn(async () => {}),
  planResults: [] as string[],
  statusResults: [] as any[],
  preview: 'normal' as 'normal' | 'empty',
  noAgents: false,
  plannedKind: 'normal' as 'normal' | 'long' | 'empty',
  storedSmartLookup: undefined as unknown,
  planStartFailures: 0,
  planStartError: 'temporary planner IPC response loss',
  summaryStartFailures: 0,
  summaryStartGate: null as Promise<void> | null,
  legacyAgent: false,
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => {}),
}))
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    setTitle: mocks.setTitle,
    setSize: mocks.setSize,
    center: mocks.center,
    onFocusChanged: vi.fn(async () => () => {}),
  }),
}))
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({ writeText: mocks.writeText }))
vi.mock('@tauri-apps/plugin-store', () => ({
  Store: {
    load: vi.fn(async () => ({
      get: vi.fn(async (key: string) => key === 'smartLookup' ? mocks.storedSmartLookup : undefined),
      set: vi.fn(async () => {}),
      delete: vi.fn(async () => {}),
      save: vi.fn(async () => {}),
    })),
  },
}))

import SmartSearchApp from './SmartSearchApp.svelte'

let component: ReturnType<typeof mount> | null = null

const OLD_COMMANDS = new Set([
  'smart_search_freeze_sources',
  'smart_search_memory_context',
  'smart_search_archive_answer',
  'smart_search_record_feedback',
  'smart_search_write_document',
])

function hit(name: string, line: number, level: SearchHit['level'] = 'line'): SmartSearchHit {
  return {
    path: `notes/${name}.md`,
    absPath: `/vault/notes/${name}.md`,
    line,
    lineEnd: level === 'section' ? 100_000 : line,
    text: `PRIVATE_BODY_${name}`,
    breadcrumb: name,
    level,
    score: 1,
    docDate: '2026-08-20',
    sourceRef: `notes/${name}.md#L${line}`,
    agentBy: null,
    humanVerified: false,
    origin: 'human' as const,
    conceptType: null,
    pinned: false,
    resultId: `result-${name}`,
    fusedScore: 0.8,
    relevanceReasons: ['strict_query'],
    matchedQueries: ['q1'],
  }
}

function searchResponse(kind: 'normal' | 'long' | 'empty' = 'normal'): SmartSearchResponse {
  const hits = kind === 'empty'
    ? []
    : kind === 'long'
      ? [hit('long-section', 1, 'section')]
      : [hit('alpha', 4), hit('beta', 8), hit('gamma', 12)]
  return {
    route: 'smart-fts',
    tookMs: 2,
    total: hits.length,
    hits,
    truncated: false,
    deepAvailable: hits.length === 0,
    extractedTerms: ['release', 'risk'],
    subqueries: [{
      id: 'q1', kind: 'recall', query: 'release risk', terms: ['release', 'risk'],
      executed: true, route: 't1-fts', hitCount: hits.length, deepUsed: false, truncated: false,
    }],
  }
}

function planJson(): Record<string, unknown> {
  return {
    schemaVersion: 1,
    intent: { kind: 'locate', focus: 'release risk' },
    time: null,
    constraints: {
      paths: { anyOf: [], allOf: [] }, tags: { anyOf: [], allOf: [] },
      types: { anyOf: [] }, extensions: { anyOf: [] }, origins: { anyOf: [] },
      linkedPages: { allOf: [] },
    },
    queries: [{
      id: 'q1', purpose: 'recall', terms: ['release', 'risk'], phrases: [], weight: 1,
      rationale: 'test',
    }],
    sort: 'relevance', unsupportedConstraints: [], ambiguities: [], confidence: 'high',
  }
}

function plannedResponse(query: string, deep = false) {
  const kind = mocks.plannedKind === 'empty' && deep ? 'normal' : mocks.plannedKind
  const search = searchResponse(kind)
  if (query === 'new question' && mocks.plannedKind === 'normal') {
    search.hits = [hit('new-result', 21)]
    search.total = 1
  }
  return {
    lookupRunId: 'lookup-1',
    resolvedPlan: {
      schemaVersion: 1,
      intent: { kind: 'locate', focus: query },
      referenceTime: '2026-09-03T00:00:00Z', referenceDate: '2026-09-03', timezone: 'UTC',
      time: null, constraints: {}, lockedFilters: {},
      queries: [{
        id: 'q1', logicalId: 'q1', purpose: 'recall', terms: ['release', 'risk'],
        phrases: [], weight: 1, rationale: 'test',
        filters: { paths: ['projects'], tags: ['release'] },
      }],
      sort: 'relevance', unsupportedConstraints: [], ambiguities: [], confidence: 'high',
    },
    search,
  }
}

function harnessStatus() {
  return {
    harness: 'Test Harness', ok: true, default_model: 'test-model',
    capabilities: {
      tasks: mocks.legacyAgent
        ? ['search-plan', 'search-answer']
        : ['search-plan', 'search-summary', 'vault-research'],
      search_plan_schemas: [1],
      terminal_result: true, input_only_isolation: true,
      model_routing: {
        invocation_override: true,
        profiles: {
          fast: { model: 'test-fast', available: true },
          default: { model: 'test-model', available: true },
        },
        selectable_models: ['test-fast', 'test-model'],
      },
    },
  }
}

function installInvokeMock(): void {
  mocks.invoke.mockImplementation(async (command: string, args?: Record<string, any>) => {
    if (OLD_COMMANDS.has(command)) throw new Error(`obsolete command called: ${command}`)
    if (command === 'get_plugin_manifests') {
      return mocks.noAgents ? [] : [{ id: 'notemd.test-agent', name: 'Test Agent', agent_provider: true }]
    }
    if (command === 'plugin_v2_execute') {
      if (args?.command === 'harness-status') return harnessStatus()
      if (args?.command === 'run-cancel') return { state: 'cancelling' }
      if (args?.command === 'run-task') {
        const task = args?.context?.task
        if (task === 'search-plan' && mocks.planStartFailures > 0) {
          mocks.planStartFailures -= 1
          throw new Error(mocks.planStartError)
        }
        return {
          run_id: task === 'search-plan' ? 'plan-1' : task === 'search-summary' ? 'summary-1' : 'research-1',
          resolved_model: task === 'vault-research' ? 'test-model' : 'test-fast',
        }
      }
      if (args?.command === 'run-status') {
        if (mocks.statusResults.length) return mocks.statusResults.shift()
        const task = args?.context?.task
        return {
          state: 'done',
          record: { status: 'success', usage: null },
          terminal_result: {
            complete: true,
            content: task === 'search-plan'
              ? (mocks.planResults.shift() ?? JSON.stringify(planJson()))
              : '- Fix the blocker before release. [S1]',
          },
        }
      }
    }
    if (command === 'notemd_search_plan_context') return { lockedFilters: {} }
    if (command === 'smart_lookup_agent_default') return 'notemd.test-agent'
    if (command === 'notemd_planned_search') {
      return plannedResponse(String(args?.originalQuery ?? ''), args?.deep === true)
    }
    if (command === 'notemd_smart_search') {
      if (args?.deep === true) return searchResponse('normal')
      return searchResponse(mocks.preview)
    }
    if (command === 'smart_lookup_start_summary') {
      if (mocks.summaryStartGate) await mocks.summaryStartGate
      if (mocks.summaryStartFailures > 0) {
        mocks.summaryStartFailures -= 1
        throw new Error('temporary IPC response loss')
      }
      return {
        runId: 'summary-1',
        resolvedModel: 'test-fast',
        sources: [{ id: 'S1', path: 'notes/alpha.md', line: 4, lineEnd: 4 }],
        staleCount: 0,
      }
    }
    if (command === 'smart_lookup_start_handoff') {
      return { runId: 'research-1', resolvedModel: 'test-model' }
    }
    if (command === 'plugin_v2_open_window') return undefined
    if (command === 'editor_show_and_reveal_search_hit') return undefined
    if (command === 'hide_smart_search_window') return undefined
    return undefined
  })
}

async function mountReady(): Promise<HTMLTextAreaElement> {
  component = mount(SmartSearchApp, { target: document.body })
  await vi.waitFor(() => {
    expect(mocks.invoke).toHaveBeenCalledWith('get_plugin_manifests')
    expect(document.querySelector<HTMLTextAreaElement>('.query-input')).not.toBeNull()
  })
  return document.querySelector<HTMLTextAreaElement>('.query-input')!
}

async function typeAndWait(input: HTMLTextAreaElement, value = 'release risk'): Promise<void> {
  input.value = value
  input.dispatchEvent(new InputEvent('input', { bubbles: true, data: value }))
  await vi.waitFor(() => {
    const expected = mocks.preview === 'empty' ? 0 : 3
    expect(document.querySelectorAll('.result-row')).toHaveLength(expected)
    expect(mocks.invoke.mock.calls.some((call) => call[0] === 'notemd_smart_search')).toBe(true)
  }, { timeout: 1_500 })
}

function taskStarts(task: string) {
  return mocks.invoke.mock.calls.filter((call) => (
    call[0] === 'plugin_v2_execute'
      && call[1]?.command === 'run-task'
      && call[1]?.context?.task === task
  ))
}

async function pressEnter(input: HTMLTextAreaElement): Promise<void> {
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
  await vi.waitFor(() => expect(document.querySelector('.plan-summary')).not.toBeNull(), { timeout: 2_000 })
}

beforeEach(() => {
  localStorage.clear()
  document.body.innerHTML = ''
  vi.clearAllMocks()
  mocks.planResults.length = 0
  mocks.statusResults.length = 0
  mocks.preview = 'normal'
  mocks.noAgents = false
  mocks.plannedKind = 'normal'
  mocks.storedSmartLookup = undefined
  mocks.planStartFailures = 0
  mocks.planStartError = 'temporary planner IPC response loss'
  mocks.summaryStartFailures = 0
  mocks.summaryStartGate = null
  mocks.legacyAgent = false
  installInvokeMock()
})

afterEach(async () => {
  if (component) await unmount(component)
  component = null
  document.body.innerHTML = ''
  localStorage.clear()
})

describe('SmartSearchApp Smart Lookup workflow', () => {
  it('keeps typing token-free and Enter performs exactly one Plan and one typed search', async () => {
    const input = await mountReady()
    await typeAndWait(input, `${'release '.repeat(12)}risk`)

    expect(taskStarts('search-plan')).toHaveLength(0)
    const previewCalls = mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_smart_search')
    expect(previewCalls).toHaveLength(1)
    expect(previewCalls[0][1]).toEqual(expect.objectContaining({ deep: false }))

    await pressEnter(input)

    expect(taskStarts('search-plan')).toHaveLength(1)
    expect(taskStarts('search-summary')).toHaveLength(0)
    expect(taskStarts('vault-research')).toHaveLength(0)
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_planned_search')).toHaveLength(1)
    expect(document.querySelector('.answer-body')).toBeNull()
    expect(document.querySelector('.pane-header')?.textContent).toContain('Smart results')
    expect(document.querySelector('.plan-summary')?.textContent).toContain('path:projects')
    expect(mocks.invoke.mock.calls.some((call) => OLD_COMMANDS.has(String(call[0])))).toBe(false)
  })

  it('does not retry or repair invalid Planner output and retains the local preview', async () => {
    mocks.planResults.push('not json')
    const input = await mountReady()
    await typeAndWait(input)

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await vi.waitFor(() => expect(document.querySelector('.preview-warning')).not.toBeNull())

    expect(taskStarts('search-plan')).toHaveLength(1)
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_planned_search')).toHaveLength(0)
    expect(document.querySelectorAll('.result-row')).toHaveLength(3)
    expect(document.body.textContent).not.toContain('Agent could not answer')
  })

  it('recovers a lost Planner start response without starting a different invocation', async () => {
    mocks.planStartFailures = 1
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)

    const starts = taskStarts('search-plan')
    expect(starts).toHaveLength(2)
    expect(starts[0][1]?.context?.invocation_id).toBe(starts[1][1]?.context?.invocation_id)
    expect(starts[0][1]?.context?.input_hash).toBe(starts[1][1]?.context?.input_hash)
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_planned_search')).toHaveLength(1)
  })

  it('does not retry an uncertain start against a legacy non-idempotent Planner', async () => {
    mocks.legacyAgent = true
    mocks.planStartFailures = 1
    mocks.planStartError = 'failed at /Users/private/Vault/.notemd/run.json token=secret-value'
    const input = await mountReady()
    await typeAndWait(input)
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await vi.waitFor(() => expect(document.querySelector('.preview-warning')).not.toBeNull())

    expect(taskStarts('search-plan')).toHaveLength(1)
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_planned_search')).toHaveLength(0)
    expect(document.body.textContent).not.toContain('/Users/private')
    expect(document.body.textContent).not.toContain('secret-value')
  })

  it('falls back from a removed exact model and explains the change', async () => {
    mocks.storedSmartLookup = {
      planner: { modelByProvider: { 'notemd.test-agent': 'model:removed-model' } },
    }
    await mountReady()
    await vi.waitFor(() => expect(document.body.textContent).toContain('previously selected model is unavailable'))
  })

  it('works without an Agent, never auto-deep-scans, and exposes manual expand/open/copy', async () => {
    mocks.noAgents = true
    mocks.preview = 'empty'
    const input = await mountReady()
    await typeAndWait(input)
    await new Promise((resolve) => setTimeout(resolve, 1_250))

    const shallowCalls = mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_smart_search')
    expect(shallowCalls).toHaveLength(1)
    expect(shallowCalls[0][1]).toEqual(expect.objectContaining({ deep: false }))

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await vi.waitFor(() => expect(document.querySelector('.preview-warning')).not.toBeNull())
    expect(taskStarts('search-plan')).toHaveLength(0)

    document.querySelector<HTMLButtonElement>('.deep-button')?.click()
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(3))
    expect(mocks.invoke.mock.calls.some((call) => (
      call[0] === 'notemd_smart_search' && call[1]?.deep === true
    ))).toBe(true)

    document.querySelectorAll<HTMLButtonElement>('.card-actions button')[1]?.click()
    await vi.waitFor(() => expect(mocks.writeText).toHaveBeenCalledWith('notes/alpha.md:4'))
    document.querySelector<HTMLButtonElement>('.primary-action')?.click()
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith(
      'editor_show_and_reveal_search_hit', expect.objectContaining({ path: '/vault/notes/alpha.md' }),
    ))
  })

  it('submits the exact new query even when Return interrupts its preview debounce', async () => {
    const input = await mountReady()
    await typeAndWait(input, 'old question')
    input.value = 'new question'
    input.dispatchEvent(new InputEvent('input', { bubbles: true, data: 'new question' }))
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))

    await vi.waitFor(() => expect(document.body.textContent).toContain('new-result'), { timeout: 2_000 })
    const planned = mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_planned_search')
    expect(planned.at(-1)?.[1]?.originalQuery).toBe('new question')
    expect(document.body.textContent).not.toContain('alpha')
  })

  it('generates a summary only after an explicit click and validates source citations', async () => {
    mocks.summaryStartFailures = 1
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)

    expect(taskStarts('search-summary')).toHaveLength(0)
    document.querySelector<HTMLButtonElement>('.remove-one')?.click()
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(2))
    document.querySelector<HTMLButtonElement>('.next-actions:not(.handoff-section) > button')?.click()
    await vi.waitFor(() => expect(document.querySelector('.summary-card')?.textContent).toContain('[S1]'))

    expect(taskStarts('search-summary')).toHaveLength(0)
    const summaryStarts = mocks.invoke.mock.calls.filter((call) => call[0] === 'smart_lookup_start_summary')
    expect(summaryStarts).toHaveLength(2)
    expect(summaryStarts[1]).toEqual(expect.arrayContaining(['smart_lookup_start_summary', expect.objectContaining({
      lookupRunId: 'lookup-1',
      selectedResultIds: ['result-beta', 'result-gamma'],
      sourceLimit: 4, charLimit: 4000,
      style: 'bullets', provider: 'notemd.test-agent', model_profile: 'fast',
    })]))
    expect(summaryStarts[0][1]?.invocationId).toBe(summaryStarts[1][1]?.invocationId)
    expect(mocks.invoke.mock.calls.some((call) => OLD_COMMANDS.has(String(call[0])))).toBe(false)
  })

  it('supersedes a running summary without leaving the next lookup disabled', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)
    mocks.statusResults.push({ state: 'running', steps: 1, last: 'do not render' })

    document.querySelector<HTMLButtonElement>('.next-actions:not(.handoff-section) > button')?.click()
    await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>(
      '.next-actions:not(.handoff-section) > button',
    )?.textContent).toContain('…'))

    input.value = 'new question'
    input.dispatchEvent(new InputEvent('input', { bubbles: true, data: 'new question' }))
    await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>('.lookup-button')?.disabled).toBe(false))
    expect(mocks.invoke.mock.calls.some((call) => (
      call[0] === 'plugin_v2_execute'
        && call[1]?.command === 'run-cancel'
        && call[1]?.context?.task === 'search-summary'
    ))).toBe(true)
  })

  it('cancels a summary that finishes starting after a new query superseded it', async () => {
    let releaseStart!: () => void
    mocks.summaryStartGate = new Promise<void>((resolve) => { releaseStart = resolve })
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)

    document.querySelector<HTMLButtonElement>('.next-actions:not(.handoff-section) > button')?.click()
    await vi.waitFor(() => expect(mocks.invoke.mock.calls.some((call) => (
      call[0] === 'smart_lookup_start_summary'
    ))).toBe(true))

    input.value = 'new question'
    input.dispatchEvent(new InputEvent('input', { bubbles: true, data: 'new question' }))
    releaseStart()

    await vi.waitFor(() => expect(mocks.invoke.mock.calls.some((call) => (
      call[0] === 'plugin_v2_execute'
        && call[1]?.command === 'run-cancel'
        && call[1]?.context?.task === 'search-summary'
        && call[1]?.context?.run_id === 'summary-1'
    ))).toBe(true))
    expect(document.querySelector('.summary-card')).toBeNull()
  })

  it('renders a very long section as a locator without freezing or reading its range', async () => {
    mocks.plannedKind = 'long'
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)

    expect(document.querySelector('.block-note')?.textContent).toContain('section')
    expect(document.querySelector<HTMLButtonElement>('.next-actions:not(.handoff-section) > button')?.disabled).toBe(true)
    expect(document.body.textContent).not.toContain('invalid source line range')
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'smart_lookup_start_summary')).toHaveLength(0)
    expect(mocks.invoke.mock.calls.some((call) => OLD_COMMANDS.has(String(call[0])))).toBe(false)
  })

  it('reuses the validated plan and its typed constraints when manually expanding zero results', async () => {
    mocks.plannedKind = 'empty'
    mocks.storedSmartLookup = { results: { autoDeepOnZero: false } }
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)

    expect(document.querySelectorAll('.result-row')).toHaveLength(0)
    document.querySelector<HTMLButtonElement>('.deep-button')?.click()
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(3))

    const planned = mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_planned_search')
    expect(planned).toHaveLength(2)
    expect(planned[1][1]).toEqual(expect.objectContaining({
      originalQuery: 'release risk',
      plan: planned[0][1]?.plan,
      referenceTime: planned[0][1]?.referenceTime,
      timezone: planned[0][1]?.timezone,
      deep: true,
    }))
    expect(taskStarts('search-plan')).toHaveLength(1)
    expect(mocks.invoke.mock.calls.some((call) => (
      call[0] === 'notemd_smart_search' && call[1]?.deep === true
    ))).toBe(false)
  })

  it('hands only relative references and constraints to a separate vault-research run', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)

    document.querySelector<HTMLButtonElement>('.handoff-button')?.click()
    await vi.waitFor(() => expect(document.querySelector('.handoff-menu')).not.toBeNull())
    document.querySelector<HTMLButtonElement>('.handoff-menu .menu-row')?.click()
    await vi.waitFor(() => expect(mocks.invoke.mock.calls.filter((call) => (
      call[0] === 'smart_lookup_start_handoff'
    ))).toHaveLength(1))

    const handoff = mocks.invoke.mock.calls.find((call) => call[0] === 'smart_lookup_start_handoff')?.[1]
    expect(handoff?.selectedRefs).toEqual([{ path: 'notes/alpha.md', line: 4, lineEnd: 4 }])
    expect(handoff?.resolvedFilters).toMatchObject({ paths: ['projects'], tags: ['release'] })
    expect(JSON.stringify(handoff)).not.toContain('/vault/')
    expect(JSON.stringify(handoff)).not.toContain('PRIVATE_BODY_')
    expect(taskStarts('vault-research')).toHaveLength(0)
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('plugin_v2_open_window', {
      pluginId: 'notemd.test-agent', windowId: 'main',
    }))
  })

  it('omits result references from handoff when that setting is disabled', async () => {
    mocks.storedSmartLookup = { handoff: { includeSelectedRefs: false } }
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)

    document.querySelector<HTMLButtonElement>('.handoff-button')?.click()
    await vi.waitFor(() => expect(document.querySelector('.handoff-menu')).not.toBeNull())
    document.querySelector<HTMLButtonElement>('.handoff-menu .menu-row')?.click()
    await vi.waitFor(() => expect(mocks.invoke.mock.calls.some((call) => (
      call[0] === 'smart_lookup_start_handoff'
    ))).toBe(true))

    const handoff = mocks.invoke.mock.calls.find((call) => call[0] === 'smart_lookup_start_handoff')?.[1]
    expect(handoff?.selectedRefs).toEqual([])
  })

  it('shows safe step counters but never provider progress text', async () => {
    mocks.statusResults.push(
      { state: 'running', steps: 1, last: 'secret prompt sk-do-not-render' },
      {
        state: 'done', record: { status: 'success' },
        terminal_result: { complete: true, content: JSON.stringify(planJson()) },
      },
    )
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)

    const log = document.querySelector('[role="log"]')
    expect(log?.textContent).toContain('background step 1')
    expect(log?.textContent).not.toContain('sk-do-not-render')
    expect(log?.getAttribute('aria-live')).toBe('polite')
  })

  it('removes multiple visible results only from this lookup and restores them with Undo', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    const rows = Array.from(document.querySelectorAll<HTMLElement>('.result-row'))
    rows[0].click()
    rows[1].dispatchEvent(new MouseEvent('click', { bubbles: true, metaKey: true }))

    await vi.waitFor(() => expect(document.querySelector('.selection-bar')?.textContent).toContain('2 selected'))
    document.querySelector<HTMLButtonElement>('.selection-bar button')?.click()
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(1))
    document.querySelector<HTMLButtonElement>('.selection-bar button')?.click()
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(3))
    expect(mocks.invoke.mock.calls.some((call) => /remove|delete/i.test(String(call[0])))).toBe(false)
  })
})
