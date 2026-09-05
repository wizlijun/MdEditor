// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, tick, unmount } from 'svelte'
import type { SearchHit, SmartSearchHit, SmartSearchResponse } from './lib/search/api'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  writeText: vi.fn(async (_text: string) => {}),
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
  plannedSearchError: null as string | null,
  summaryStartFailures: 0,
  summaryStartGate: null as Promise<void> | null,
  plannedSearchGate: null as Promise<void> | null,
  customHits: null as SmartSearchHit[] | null,
  focusChanged: null as ((event: { payload: boolean }) => Promise<void>) | null,
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
    onFocusChanged: vi.fn(async (callback: (event: { payload: boolean }) => Promise<void>) => {
      mocks.focusChanged = callback
      return () => { mocks.focusChanged = null }
    }),
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
  const hits = mocks.customHits ?? (kind === 'empty'
    ? []
    : kind === 'long'
      ? [hit('long-section', 1, 'section')]
      : [hit('alpha', 4), hit('beta', 8), hit('gamma', 12)])
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

function plannedResponse(
  query: string,
  deep = false,
  plan: Record<string, any> = planJson(),
  referenceTime = '2026-09-03T00:00:00Z',
  timezone = 'UTC',
) {
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
      referenceTime, referenceDate: referenceTime.slice(0, 10), timezone,
      time: plan.time
        ? {
            appliesTo: plan.time.appliesTo,
            sourceText: plan.time.sourceText,
            after: plan.time.expression?.kind === 'absolute_range'
              ? plan.time.expression.after
              : '2026-08-24',
            before: plan.time.expression?.kind === 'absolute_range'
              ? plan.time.expression.before
              : '2026-08-30',
          }
        : null,
      constraints: {}, lockedFilters: {},
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
    if (command === 'notemd_search_plan_context') {
      return {
        lockedFilters: {},
        referenceDate: '2026-09-05',
        timeAnchors: {
          today: { after: '2026-09-05', before: '2026-09-05' },
          lastWeek: { after: '2026-08-24', before: '2026-08-30' },
        },
      }
    }
    if (command === 'smart_lookup_agent_default') return 'notemd.test-agent'
    if (command === 'notemd_planned_search') {
      const gate = mocks.plannedSearchGate
      mocks.plannedSearchGate = null
      if (gate) await gate
      if (mocks.plannedSearchError) throw new Error(mocks.plannedSearchError)
      return plannedResponse(
        String(args?.originalQuery ?? ''),
        args?.deep === true,
        args?.plan,
        args?.referenceTime,
        args?.timezone,
      )
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
  await tick()
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

function buttonNamed(label: string): HTMLButtonElement {
  const button = Array.from(document.querySelectorAll<HTMLButtonElement>('button'))
    .find((candidate) => candidate.textContent?.trim() === label)
  expect(button, `Button named "${label}"`).toBeDefined()
  return button!
}

async function openActionsMenu(): Promise<HTMLElement> {
  if (!document.querySelector('.actions-menu')) {
    document.querySelector<HTMLButtonElement>('.more-actions')!.click()
  }
  await vi.waitFor(() => expect(document.querySelector('.actions-menu[role="menu"]')).not.toBeNull())
  return document.querySelector<HTMLElement>('.actions-menu')!
}

async function chooseMenuAction(label: string): Promise<void> {
  await openActionsMenu()
  buttonNamed(label).click()
  await tick()
  expect(document.querySelector('.actions-menu')).toBeNull()
}

async function pressEnter(input: HTMLTextAreaElement): Promise<void> {
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
  await tick()
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
  mocks.plannedSearchError = null
  mocks.summaryStartFailures = 0
  mocks.summaryStartGate = null
  mocks.plannedSearchGate = null
  mocks.customHits = null
  mocks.focusChanged = null
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
    expect(document.querySelector('.pane-header')?.textContent).toContain('Search results')
    expect(document.querySelector('.plan-summary')?.textContent).toContain('Folder: projects')
    expect(mocks.invoke.mock.calls.some((call) => OLD_COMMANDS.has(String(call[0])))).toBe(false)
  })

  it('uses a host today anchor before typed search and reuses the frozen plan for deep search', async () => {
    const timedPlan = planJson()
    timedPlan.time = {
      appliesTo: 'document_date',
      sourceText: '今天',
      expression: { kind: 'absolute_range', after: '2026-09-05', before: '2026-09-05' },
    }
    mocks.planResults.push(JSON.stringify(timedPlan))
    mocks.plannedKind = 'empty'
    mocks.storedSmartLookup = { results: { autoDeepOnZero: false } }
    const input = await mountReady()
    await typeAndWait(input, '找今天的发布风险')
    await pressEnter(input)

    const start = taskStarts('search-plan')[0]?.[1]?.context
    expect(start?.prompt).toContain('QUESTION\n找今天的发布风险')
    expect(start?.prompt).toContain('TIME GATE')
    expect(start?.prompt).toContain('TRUSTED_TIME_ANCHORS_JSON')
    expect(start?.prompt).toContain('"today":{"after":"2026-09-05","before":"2026-09-05"}')
    const first = mocks.invoke.mock.calls.find((call) => call[0] === 'notemd_planned_search')?.[1]
    expect(first?.plan).toEqual(timedPlan)
    expect(first?.referenceTime).toMatch(/^\d{4}-\d{2}-\d{2}T/)
    expect(first?.timezone).toEqual(expect.any(String))
    const context = mocks.invoke.mock.calls.find((call) => call[0] === 'notemd_search_plan_context')?.[1]
    expect(context).toEqual(expect.objectContaining({
      originalQuery: '找今天的发布风险',
      referenceTime: first?.referenceTime,
      timezone: first?.timezone,
    }))
    expect(document.querySelector('.plan-summary')?.textContent).toContain('Sep 5, 2026')
    const contextIndex = mocks.invoke.mock.calls.findIndex((call) => call[0] === 'notemd_search_plan_context')
    const plannerIndex = mocks.invoke.mock.calls.findIndex((call) => (
      call[0] === 'plugin_v2_execute' && call[1]?.command === 'run-task'
    ))
    const statusIndex = mocks.invoke.mock.calls.findIndex((call) => (
      call[0] === 'plugin_v2_execute'
        && call[1]?.command === 'run-status'
        && call[1]?.context?.task === 'search-plan'
    ))
    const searchIndex = mocks.invoke.mock.calls.findIndex((call) => call[0] === 'notemd_planned_search')
    expect(contextIndex).toBeLessThan(plannerIndex)
    expect(plannerIndex).toBeLessThan(statusIndex)
    expect(statusIndex).toBeLessThan(searchIndex)
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_smart_search')).toHaveLength(1)

    buttonNamed('Expand search').click()
    await vi.waitFor(() => {
      expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_planned_search')).toHaveLength(2)
    })
    const calls = mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_planned_search')
    expect(calls[1][1]).toEqual(expect.objectContaining({
      plan: timedPlan,
      referenceTime: first?.referenceTime,
      timezone: first?.timezone,
      deep: true,
    }))
    expect(taskStarts('search-plan')).toHaveLength(1)
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_smart_search')).toHaveLength(1)
  })

  it('keeps the local preview when the host rejects a planned time range', async () => {
    mocks.plannedSearchError = 'invalid search plan: document_date sourceText is not trusted'
    const input = await mountReady()
    await typeAndWait(input, '找今天的发布风险')
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))

    await vi.waitFor(() => expect(document.querySelector('.preview-warning')).not.toBeNull())
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_planned_search')).toHaveLength(1)
    expect(document.querySelectorAll('.result-row')).toHaveLength(3)
    expect(document.querySelector('.plan-summary')).toBeNull()
    expect(document.querySelector('.answer-body')).toBeNull()
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

  it('keeps local results and hides provider internals when the Planner is cancelled', async () => {
    mocks.statusResults.push({
      state: 'done',
      record: {
        status: 'cancelled',
        stderr_tail: 'ERROR codex_models_manager::manager: failed to refresh available models',
      },
    })
    const input = await mountReady()
    await typeAndWait(input)

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await vi.waitFor(() => expect(document.querySelector('.preview-warning')).not.toBeNull())

    expect(document.querySelectorAll('.result-row')).toHaveLength(3)
    expect(document.querySelector('.preview-warning')?.textContent).toContain('Local matches are still available')
    document.querySelector<HTMLButtonElement>('.workflow-toggle')!.click()
    await vi.waitFor(() => expect(document.querySelector('[role="log"]')?.textContent).toContain('Stopped'))
    expect(document.body.textContent).not.toContain('codex_models_manager')
    expect(document.body.textContent).not.toContain('failed to refresh available models')
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
    await vi.waitFor(() => expect(document.body.textContent).toContain('Your selected model is unavailable'))
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

    buttonNamed('Expand search').click()
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(3))
    expect(mocks.invoke.mock.calls.some((call) => (
      call[0] === 'notemd_smart_search' && call[1]?.deep === true
    ))).toBe(true)

    await chooseMenuAction('Copy reference')
    await vi.waitFor(() => expect(mocks.writeText).toHaveBeenCalledWith('notes/alpha.md:4'))
    document.querySelector<HTMLButtonElement>('.preview-header .text-button')!.click()
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
    document.querySelector<HTMLElement>('.result-row')!.click()
    await vi.waitFor(() => expect(document.querySelector('.selection-bar')).not.toBeNull())
    await chooseMenuAction('Hide selected results')
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(2))
    await chooseMenuAction('Create answer')
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

    await chooseMenuAction('Create answer')
    await vi.waitFor(() => expect(document.querySelector('.stop-button')).not.toBeNull())

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

    await chooseMenuAction('Create answer')
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

    expect(document.querySelector('.block-note')?.textContent).toContain('Open the original to keep reading')
    await openActionsMenu()
    expect(document.querySelector<HTMLButtonElement>('.summary-menu-action')?.disabled).toBe(true)
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
    buttonNamed('Expand search').click()
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

    await openActionsMenu()
    document.querySelector<HTMLButtonElement>('.research-menu-action')!.click()
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

    await openActionsMenu()
    document.querySelector<HTMLButtonElement>('.research-menu-action')!.click()
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

    document.querySelector<HTMLButtonElement>('.workflow-toggle')!.click()
    await vi.waitFor(() => expect(document.querySelector('[role="log"]')).not.toBeNull())
    const log = document.querySelector('[role="log"]')
    expect(log?.textContent).toContain('Step 1 completed')
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
    await chooseMenuAction('Hide selected results')
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(1))
    buttonNamed('Undo').click()
    await vi.waitFor(() => expect(document.querySelectorAll('.result-row')).toHaveLength(3))
    expect(mocks.invoke.mock.calls.some((call) => /remove|delete/i.test(String(call[0])))).toBe(false)
  })

  it('can search again after Escape stops and hides a busy lookup', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    mocks.statusResults.push({ state: 'running', steps: 1 })
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await vi.waitFor(() => expect(document.querySelector('.stop-button')).not.toBeNull())
    await vi.waitFor(() => expect(mocks.invoke.mock.calls.some((call) => (
      call[0] === 'plugin_v2_execute' && call[1]?.command === 'run-status'
        && call[1]?.context?.task === 'search-plan'
    ))).toBe(true))
    await vi.waitFor(() => expect(mocks.focusChanged).not.toBeNull())

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('hide_smart_search_window'))
    await mocks.focusChanged?.({ payload: true })
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(input)
      expect(document.querySelector<HTMLButtonElement>('.lookup-button')?.disabled).toBe(false)
      expect(document.querySelector('.stop-button')).toBeNull()
    })
    await pressEnter(input)
    expect(taskStarts('search-plan')).toHaveLength(2)
  })

  it('stops a starting summary without leaving lookup disabled when the window is reopened', async () => {
    let releaseStart!: () => void
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)
    mocks.summaryStartGate = new Promise<void>((resolve) => { releaseStart = resolve })
    await chooseMenuAction('Create answer')
    await vi.waitFor(() => expect(document.querySelector('.stop-button')).not.toBeNull())

    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('hide_smart_search_window'))
    await mocks.focusChanged?.({ payload: true })
    releaseStart()
    await vi.waitFor(() => {
      expect(document.querySelector<HTMLButtonElement>('.lookup-button')?.disabled).toBe(false)
      expect(mocks.invoke.mock.calls.some((call) => (
        call[0] === 'plugin_v2_execute' && call[1]?.command === 'run-cancel'
          && call[1]?.context?.task === 'search-summary'
      ))).toBe(true)
    })
    expect(document.querySelector('.summary-card')).toBeNull()
    await pressEnter(input)
  })

  it('ignores a stopped search response that arrives after a newer query completed', async () => {
    let releaseSearch!: () => void
    const input = await mountReady()
    await typeAndWait(input)
    mocks.plannedSearchGate = new Promise<void>((resolve) => { releaseSearch = resolve })
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await vi.waitFor(() => expect(mocks.invoke.mock.calls.some((call) => call[0] === 'notemd_planned_search')).toBe(true))
    document.querySelector<HTMLButtonElement>('.stop-button')!.click()
    await vi.waitFor(() => expect(document.querySelector('.stop-button')).toBeNull())
    expect(mocks.invoke).not.toHaveBeenCalledWith('hide_smart_search_window')
    expect(document.querySelectorAll('.result-row')).toHaveLength(3)

    await typeAndWait(input, 'new question')
    await pressEnter(input)
    expect(document.querySelectorAll('.result-row')).toHaveLength(1)
    expect(document.querySelector('.result-row')?.textContent).toContain('new-result')
    releaseSearch()
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(document.querySelectorAll('.result-row')).toHaveLength(1)
    expect(document.querySelector('.result-row')?.textContent).toContain('new-result')
    expect(document.querySelector('.stop-button')).toBeNull()
  })

  it('opens settings without resetting the window size and restores focus when dismissed', async () => {
    const input = await mountReady()
    const settingsButton = document.querySelector<HTMLButtonElement>('.settings-button')!
    settingsButton.click()
    await vi.waitFor(() => {
      expect(mocks.setSize).not.toHaveBeenCalled()
      expect(document.activeElement).toBe(document.querySelector('.lookup-settings'))
    })
    const dialog = document.querySelector<HTMLElement>('.lookup-settings')!
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }))
    expect(document.activeElement).toBe(document.querySelector('.settings-header button'))
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await vi.waitFor(() => {
      expect(document.querySelector('.lookup-settings')).toBeNull()
      expect(document.activeElement).toBe(settingsButton)
    })
    expect(mocks.invoke).not.toHaveBeenCalledWith('hide_smart_search_window')

    settingsButton.click()
    await vi.waitFor(() => expect(document.querySelector('.settings-scrim')).not.toBeNull())
    document.querySelector<HTMLButtonElement>('.settings-scrim')!.click()
    await vi.waitFor(() => expect(document.activeElement).toBe(settingsButton))
    settingsButton.dispatchEvent(new KeyboardEvent('keydown', { key: 'f', metaKey: true, bubbles: true }))
    expect(document.activeElement).toBe(input)
  })

  it('navigates results from the query using arrows, Home, End, and Shift selection', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    const rows = Array.from(document.querySelectorAll<HTMLElement>('.result-row'))
    expect(rows.map((row) => row.tabIndex)).toEqual([0, -1, -1])
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    await vi.waitFor(() => expect(document.activeElement).toBe(rows[0]))
    rows[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    await vi.waitFor(() => expect(document.activeElement).toBe(rows[1]))
    rows[1].dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }))
    await vi.waitFor(() => expect(document.activeElement).toBe(rows[2]))
    rows[2].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }))
    await vi.waitFor(() => expect(document.activeElement).toBe(rows[1]))
    rows[1].dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true }))
    await vi.waitFor(() => expect(document.activeElement).toBe(rows[0]))
    rows[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', shiftKey: true, bubbles: true }))
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(rows[1])
      expect(rows.map((row) => row.getAttribute('aria-selected'))).toEqual(['true', 'true', 'false'])
      expect(rows.map((row) => row.tabIndex)).toEqual([-1, 0, -1])
    })
    rows[1].dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', metaKey: true, bubbles: true }))
    await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('editor_show_and_reveal_search_hit', expect.objectContaining({ path: '/vault/notes/beta.md' })))
    expect(mocks.invoke).not.toHaveBeenCalledWith('hide_smart_search_window')
  })

  it('uses the displayed date-group order for keyboard and pointer range selection', async () => {
    mocks.storedSmartLookup = { results: { groupBy: 'date' } }
    mocks.customHits = [
      { ...hit('alpha', 4), origin: 'source', docDate: '2026-09-01' },
      { ...hit('beta', 8), origin: 'human', docDate: '2026-08-01' },
      { ...hit('gamma', 12), origin: 'source', docDate: '2026-09-02' },
    ]
    const input = await mountReady()
    await typeAndWait(input)
    const rows = Array.from(document.querySelectorAll<HTMLElement>('.result-row'))
    expect(rows.map((row) => row.querySelector('strong')?.textContent)).toEqual(['alpha', 'gamma', 'beta'])
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    await vi.waitFor(() => expect(document.activeElement).toBe(rows[0]))
    rows[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'End', shiftKey: true, bubbles: true }))
    await vi.waitFor(() => {
      expect(document.activeElement).toBe(rows[2])
      expect(rows.every((row) => row.getAttribute('aria-selected') === 'true')).toBe(true)
    })
    rows[0].click()
    rows[2].dispatchEvent(new MouseEvent('click', { shiftKey: true, bubbles: true }))
    await vi.waitFor(() => expect(rows.every((row) => row.getAttribute('aria-selected') === 'true')).toBe(true))
  })

  it('shows the filtered result count and restores results from the empty-source state', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    const filter = document.querySelector<HTMLSelectElement>('.results-pane .pane-header select')!
    document.querySelector<HTMLElement>('.result-row')!.click()
    await vi.waitFor(() => expect(document.querySelector('.selection-bar')?.textContent).toContain('1 selected'))
    expect(document.querySelector('.results-pane .pane-header > span')?.textContent).toBe('3 results')
    filter.value = 'source'
    filter.dispatchEvent(new Event('change', { bubbles: true }))
    await vi.waitFor(() => {
      expect(document.querySelectorAll('.result-row')).toHaveLength(0)
      expect(document.querySelector('.selection-bar')).toBeNull()
      expect(document.querySelector('.results-pane .pane-header > span')?.textContent).toBe('0 results')
      expect(document.querySelector('.empty-results')?.textContent).toContain('No matching results from this source')
    })
    document.querySelector<HTMLButtonElement>('.empty-results button')!.click()
    await vi.waitFor(() => {
      expect(filter.value).toBe('all')
      expect(document.querySelectorAll('.result-row')).toHaveLength(3)
      expect(document.querySelector('.results-pane .pane-header > span')?.textContent).toBe('3 results')
    })
    expect(taskStarts('search-plan')).toHaveLength(0)
  })

  it('confirms copying and clears the query, results, and generated answer with focus restored', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)
    await chooseMenuAction('Copy reference')
    await vi.waitFor(() => {
      expect(mocks.writeText).toHaveBeenCalledWith('notes/alpha.md:4')
      expect(document.querySelector('.copy-feedback')?.textContent).toBe('Copied')
    })
    await chooseMenuAction('Create answer')
    await vi.waitFor(() => expect(document.querySelector('.summary-card')).not.toBeNull())
    document.querySelector<HTMLButtonElement>('.clear-button')!.click()
    await vi.waitFor(() => {
      expect(input.value).toBe('')
      expect(document.activeElement).toBe(input)
      expect(document.querySelectorAll('.result-row')).toHaveLength(0)
      expect(document.querySelector('.plan-summary')).toBeNull()
      expect(document.querySelector('.summary-card')).toBeNull()
      expect(document.querySelector<HTMLButtonElement>('.lookup-button')?.disabled).toBe(true)
    })
  })

  it('shows an actionable failure when both clipboard methods fail', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    mocks.writeText.mockRejectedValueOnce(new Error('native clipboard denied'))
    const browserCopy = vi.spyOn(navigator.clipboard, 'writeText')
      .mockRejectedValueOnce(new Error('browser clipboard denied'))
    try {
      await chooseMenuAction('Copy reference')
      await vi.waitFor(() => {
        expect(browserCopy).toHaveBeenCalledWith('notes/alpha.md:4')
        expect(document.querySelector('.copy-feedback')?.textContent).toBe('Could not copy. Please try again.')
      })
      expect(document.querySelectorAll('.result-row')).toHaveLength(3)
    } finally {
      browserCopy.mockRestore()
    }
  })

  it('keeps ArrowDown inside multiline input until an unselected caret is on the last line', async () => {
    const input = await mountReady()
    await typeAndWait(input, 'release plan\nnext steps')
    input.focus()
    input.setSelectionRange(2, 2)
    const moveWithinQuestion = new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true })
    input.dispatchEvent(moveWithinQuestion)
    await tick()
    expect(moveWithinQuestion.defaultPrevented).toBe(false)
    expect(document.activeElement).toBe(input)

    input.setSelectionRange(input.value.length - 3, input.value.length)
    const moveSelection = new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true })
    input.dispatchEvent(moveSelection)
    await tick()
    expect(moveSelection.defaultPrevented).toBe(false)
    expect(document.activeElement).toBe(input)

    input.setSelectionRange(input.value.length, input.value.length)
    const enterResults = new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true })
    input.dispatchEvent(enterResults)
    await vi.waitFor(() => expect(document.activeElement).toBe(document.querySelector('.result-row')))
    expect(enterResults.defaultPrevented).toBe(true)
  })

  it('cancels an in-flight summary when its selection changes and ignores the old answer', async () => {
    let finishSummary!: (result: unknown) => void
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)
    mocks.statusResults.push(new Promise((resolve) => { finishSummary = resolve }))
    await chooseMenuAction('Create answer')
    await vi.waitFor(() => expect(mocks.invoke.mock.calls.some((call) => (
      call[0] === 'plugin_v2_execute' && call[1]?.command === 'run-status'
        && call[1]?.context?.task === 'search-summary'
    ))).toBe(true))

    document.querySelectorAll<HTMLElement>('.result-row')[1].click()
    await vi.waitFor(() => {
      expect(document.querySelector('.selection-bar')?.textContent).toContain('1 selected')
      expect(document.querySelector('.stop-button')).toBeNull()
      expect(mocks.invoke.mock.calls.some((call) => (
        call[0] === 'plugin_v2_execute' && call[1]?.command === 'run-cancel'
          && call[1]?.context?.task === 'search-summary'
      ))).toBe(true)
    })
    finishSummary({
      state: 'done', record: { status: 'success' },
      terminal_result: { complete: true, content: '- OLD_SELECTION_ANSWER should not appear. [S1]' },
    })
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(document.querySelector('.summary-card')).toBeNull()
    expect(document.body.textContent).not.toContain('OLD_SELECTION_ANSWER')
    await openActionsMenu()
    expect(document.querySelector<HTMLButtonElement>('.summary-menu-action')?.disabled).toBe(false)
    expect(document.querySelector('.actions-menu .menu-hint')?.textContent).toContain('Using 1 selected results')
  })

  it.each([
    { label: 'failed', status: 'error', complete: true, message: 'The AI service is temporarily unavailable' },
    { label: 'timed out', status: 'timeout', complete: true, message: 'This is taking longer than expected' },
    { label: 'incomplete', status: 'success', complete: false, message: 'Your question could not be understood' },
  ])('shows a safe $label message without exposing provider payloads in activity', async ({ status, complete, message }) => {
    mocks.statusResults.push({
      state: 'done',
      record: { status, stderr_tail: 'PRIVATE_PROVIDER_STDERR' },
      terminal_result: { complete, content: 'PRIVATE_BODY_OR_PROMPT from a failed provider' },
    })
    const input = await mountReady()
    await typeAndWait(input)
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await vi.waitFor(() => expect(document.querySelector('.preview-warning')).not.toBeNull())
    expect(document.querySelectorAll('.result-row')).toHaveLength(3)
    expect(document.querySelector('.plan-summary')).toBeNull()
    document.querySelector<HTMLButtonElement>('.workflow-toggle')!.click()
    await vi.waitFor(() => expect(document.querySelector('[role="log"]')).not.toBeNull())
    expect(document.body.textContent).not.toContain('PRIVATE_BODY_OR_PROMPT')
    expect(document.body.textContent).not.toContain('PRIVATE_PROVIDER_STDERR')
    expect(document.querySelector('[role="log"]')?.textContent).toContain('Local matches are still available')
    expect(document.querySelector('[role="log"]')?.textContent).toContain(message)
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'notemd_planned_search')).toHaveLength(0)
  })

  it('shows the first result content directly with one open action and no action panels', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)

    expect(document.querySelector('.preview-text')?.textContent).toContain('PRIVATE_BODY_alpha')
    expect(document.querySelector('.result-row.active strong')?.textContent).toBe('alpha')
    expect(document.querySelectorAll('.preview-header > button')).toHaveLength(2)
    expect(Array.from(document.querySelectorAll<HTMLButtonElement>('button'))
      .filter((button) => button.textContent?.trim() === 'Open original ↗')).toHaveLength(1)
    expect(document.querySelectorAll('.preview-scroll button')).toHaveLength(0)
    expect(document.querySelector('.actions-menu')).toBeNull()
    expect(document.querySelector('.next-actions')).toBeNull()
    expect(document.querySelector('.card-actions')).toBeNull()
    expect(document.querySelector('.input-hint')).toBeNull()
    expect(document.querySelector('.results-footer')).toBeNull()
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'smart_lookup_start_summary')).toHaveLength(0)
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'smart_lookup_start_handoff')).toHaveLength(0)

    document.querySelectorAll<HTMLElement>('.result-row')[1].click()
    await vi.waitFor(() => expect(document.querySelector('.preview-text')?.textContent).toContain('PRIVATE_BODY_beta'))
    expect(document.querySelectorAll('.selection-bar button')).toHaveLength(0)
    await chooseMenuAction('Clear selection')
    await vi.waitFor(() => expect(document.querySelector('.selection-bar')).toBeNull())
    expect(document.querySelector('.preview-text')?.textContent).toContain('PRIVATE_BODY_beta')
    expect(taskStarts('search-plan')).toHaveLength(1)
  })

  it('opens and navigates the actions menu without model calls and returns focus on dismissal', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)
    const callsBefore = mocks.invoke.mock.calls.length
    const trigger = document.querySelector<HTMLButtonElement>('.more-actions')!
    const menu = await openActionsMenu()
    const items = Array.from(menu.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'))
    expect(trigger.getAttribute('aria-haspopup')).toBe('menu')
    expect(trigger.getAttribute('aria-expanded')).toBe('true')
    expect(document.activeElement).toBe(items[0])
    items[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    expect(document.activeElement).toBe(items[1])
    items[1].dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }))
    expect(document.activeElement).toBe(items.at(-1))
    items.at(-1)!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    expect(document.activeElement).toBe(items[0])
    items[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowUp', bubbles: true }))
    expect(document.activeElement).toBe(items.at(-1))
    items.at(-1)!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Home', bubbles: true }))
    expect(document.activeElement).toBe(items[0])
    items[0].dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await vi.waitFor(() => {
      expect(document.querySelector('.actions-menu')).toBeNull()
      expect(trigger.getAttribute('aria-expanded')).toBe('false')
      expect(document.activeElement).toBe(trigger)
    })
    await openActionsMenu()
    document.querySelector<HTMLButtonElement>('.menu-scrim')!.click()
    await vi.waitFor(() => {
      expect(document.querySelector('.actions-menu')).toBeNull()
      expect(document.activeElement).toBe(trigger)
    })
    expect(mocks.invoke.mock.calls).toHaveLength(callsBefore)
    expect(mocks.invoke).not.toHaveBeenCalledWith('hide_smart_search_window')
  })

  it('only copies research instructions even when a default research provider is configured', async () => {
    mocks.storedSmartLookup = { handoff: { defaultProvider: 'notemd.test-agent' } }
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)
    await chooseMenuAction('Copy research instructions')
    await vi.waitFor(() => expect(mocks.writeText).toHaveBeenCalledOnce())
    const copied = mocks.writeText.mock.calls[0]?.[0]
    expect(copied).toContain('release risk')
    expect(copied).toContain('notes/alpha.md')
    expect(copied).not.toContain('PRIVATE_BODY_')
    expect(copied).not.toContain('/vault/')
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'smart_lookup_start_handoff')).toHaveLength(0)
    expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'plugin_v2_open_window')).toHaveLength(0)
    expect(taskStarts('vault-research')).toHaveLength(0)
    expect(document.querySelector('.copy-feedback')?.textContent).toBe('Copied')
  })

  it('keeps research available in the menu when no results are found', async () => {
    mocks.preview = 'empty'
    mocks.plannedKind = 'empty'
    mocks.storedSmartLookup = { results: { autoDeepOnZero: false } }
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)
    expect(document.querySelectorAll('.result-row')).toHaveLength(0)
    await openActionsMenu()
    expect(document.querySelector('.summary-menu-action')).toBeNull()
    const research = document.querySelector<HTMLButtonElement>('.research-menu-action')!
    expect(research.disabled).toBe(false)
    research.click()
    await vi.waitFor(() => expect(mocks.invoke.mock.calls.filter((call) => call[0] === 'smart_lookup_start_handoff')).toHaveLength(1))
    expect(mocks.invoke.mock.calls.find((call) => call[0] === 'smart_lookup_start_handoff')?.[1]?.selectedRefs).toEqual([])
    expect(taskStarts('search-plan')).toHaveLength(1)
  })

  it('shows a summary failure in the content after its menu has closed', async () => {
    const input = await mountReady()
    await typeAndWait(input)
    await pressEnter(input)
    mocks.statusResults.push({
      state: 'done', record: { status: 'error' },
      terminal_result: { complete: true, content: 'PRIVATE_FAILED_SUMMARY_BODY' },
    })
    await chooseMenuAction('Create answer')
    await vi.waitFor(() => expect(document.querySelector('.summary-error[role="alert"]')?.textContent).toContain('The AI service is temporarily unavailable'))
    expect(document.querySelector('.actions-menu')).toBeNull()
    expect(document.querySelector('.summary-card')).toBeNull()
    expect(document.querySelector('.preview-text')?.textContent).toContain('PRIVATE_BODY_alpha')
    expect(document.body.textContent).not.toContain('PRIVATE_FAILED_SUMMARY_BODY')
    expect(document.querySelector('.stop-button')).toBeNull()
  })
})
