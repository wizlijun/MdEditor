<script lang="ts">
  import './styles/app.css'
  import { onDestroy, onMount, tick } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { i18n, loadLocale, t, watchLocaleChanges } from './lib/i18n/store.svelte'
  import type { AgentOption } from './lib/agent-picker/types'
  import type { SearchHit, SmartRelevanceReason, SmartSearchHit, SmartSearchResponse } from './lib/search/api'
  import { groupHits, type HitGroup } from './lib/search/grouping'
  import { decideTrigger } from './lib/search/input-trigger'
  import { highlightParts, parseHighlightTerms, previewLine } from './lib/search/preview'
  import { createImeGuard, isImeKey } from './lib/ime'
  import { loadSettings, saveSettings, settings } from './lib/settings.svelte'
  import { SmartSearchStore } from './lib/smart-search/store.svelte'
  import {
    AgentTaskError,
    cancelAgentTask,
    createAgentInvocation,
    loadDefaultAgentProvider,
    loadSearchAgentOptions,
    pollAgentTask,
    SEARCH_PLAN_TASK,
    SEARCH_SUMMARY_TASK,
    startAgentTask,
    supportsSearchPlanner,
    supportsSearchTask,
    VAULT_RESEARCH_TASK,
  } from './lib/smart-search/agent'
  import { smartSearchApi } from './lib/smart-search/api'
  import { buildHandoffPacket, buildHandoffPrompt } from './lib/smart-search/handoff'
  import {
    availableModelPreference,
    rememberedModelPreference,
    resolvedModelHint,
    selectableModelPreferences,
    selectorForPreference,
    type ModelPreference,
  } from './lib/smart-search/model-routing'
  import { buildSearchPlanPrompt, type ResolvedSearchPlan } from './lib/smart-search/plan'
  import {
    addRemovedKeys,
    chooseResultKeys,
    hitKey,
    restoreRemovedKeys,
  } from './lib/smart-search/selection'
  import { validateSummaryOutput, type SummarySource } from './lib/smart-search/summary'
  import {
    appendWorkflowEntry,
    isNearLogBottom,
    type WorkflowEntry,
    type WorkflowLevel,
    type WorkflowStage,
  } from './lib/smart-search/workflow-log'

  type SourceFilter = 'all' | SearchHit['origin']
  type LookupPhase =
    | 'idle'
    | 'previewing'
    | 'understanding'
    | 'searching'
    | 'ready'
    | 'preview_only'
    | 'partial'
    | 'no_results'

  interface ActiveAgentRun {
    provider: string
    task: string
    runId: string
  }

  const store = new SmartSearchStore()
  const windowApi = getCurrentWindow()
  const inputIme = createImeGuard()

  let ready = $state(false)
  let inputValue = $state('')
  let composing = $state(false)
  let inputEl = $state<HTMLTextAreaElement>()
  let debounceTimer: ReturnType<typeof setTimeout> | undefined

  let phase = $state<LookupPhase>('idle')
  let authoritativeResults = $state(false)
  let resolvedPlan = $state<ResolvedSearchPlan | null>(null)
  let submittedPlan = $state<unknown | null>(null)
  let planReferenceTime = $state('')
  let planTimezone = $state('')
  let lookupRunId = $state('')
  let plannerModel = $state<string | null>(null)
  let plannerWarning = $state('')
  let modelWarning = $state('')
  let navigationError = $state('')

  let sourceFilter = $state<SourceFilter>('all')
  let selectedKeys = $state<string[]>([])
  let removedKeys = $state<string[]>([])
  let lastRemoved = $state<string[]>([])
  let activeKey = $state<string | null>(null)
  let rangeAnchor = $state<string | null>(null)

  let agents = $state<AgentOption[]>([])
  let appDefaultAgent = $state('')
  let agentsError = $state('')
  let settingsOpen = $state(false)
  let settingsEl = $state<HTMLDivElement>()
  let settingsButton = $state<HTMLButtonElement>()
  let resultsEl = $state<HTMLDivElement>()
  let mobilePane = $state<'results' | 'preview'>('results')
  let activityExpanded = $state<boolean | null>(null)
  let copyFeedback = $state('')
  let copyTimer: ReturnType<typeof setTimeout> | undefined
  let actionsMenuOpen = $state(false)
  let actionsMenuEl = $state<HTMLDivElement>()
  let actionsButton = $state<HTMLButtonElement>()
  let actionsMenuTop = $state(0)
  let handoffRun = $state<{ provider: string; runId: string } | null>(null)
  let handoffError = $state('')
  let handoffBusy = $state(false)

  let summaryBusy = $state(false)
  let summaryText = $state('')
  let summaryError = $state('')
  let summarySources = $state<SummarySource[]>([])
  let summaryCardEl = $state<HTMLDivElement>()

  let workflowEntries = $state<WorkflowEntry[]>([])
  let workflowSequence = 0
  let activityLogEl = $state<HTMLDivElement>()
  let autoFollowActivity = true
  let requestSequence = 0
  let activeAbort: AbortController | null = null
  let activeAgentRun: ActiveAgentRun | null = null

  let unlistenLocale: (() => void) | null = null
  let unlistenFocus: (() => void) | null = null
  let unlistenSettings: (() => void) | null = null

  let removedSet = $derived(new Set(removedKeys))
  let visibleHits = $derived(store.hits.filter((hit) => (
    !removedSet.has(hitKey(hit)) && (sourceFilter === 'all' || hit.origin === sourceFilter)
  )))
  let groups = $derived(groupHits(visibleHits))
  let flatVisibleHits = $derived(useDateGrouping()
    ? dateGroups().flatMap((group) => group.hits)
    : groups.flatMap((group) => group.files.flatMap((file) => file.hits)))
  let selectedSet = $derived(new Set(selectedKeys))
  let activeHit = $derived(
    flatVisibleHits.find((hit) => hitKey(hit) === activeKey) ?? flatVisibleHits[0] ?? null,
  )
  let highlightTerms = $derived(parseHighlightTerms(
    authoritativeResults && resolvedPlan
      ? resolvedPlan.queries.flatMap((query) => [...query.terms, ...query.phrases]).join(' ')
      : store.query || inputValue,
  ))
  let plannerAgents = $derived(agents.filter(supportsSearchPlanner))
  let summaryAgents = $derived(agents.filter((agent) => supportsSearchTask(agent, SEARCH_SUMMARY_TASK)))
  let handoffAgents = $derived(agents.filter((agent) => supportsSearchTask(agent, VAULT_RESEARCH_TASK)))
  let plannerAgent = $derived(resolvePlannerAgent())
  let summaryAgent = $derived(resolveSummaryAgent())
  let resolvedTerms = $derived(Array.from(new Set(
    resolvedPlan?.queries.flatMap((query) => [...query.terms, ...query.phrases]) ?? [],
  )).slice(0, 12))
  let lookupBusy = $derived(phase === 'understanding' || phase === 'searching')
  let working = $derived(lookupBusy || summaryBusy || handoffBusy)
  let showActivity = $derived(activityExpanded ?? working)
  let latestActivity = $derived(workflowEntries.at(-1))
  let resultSections = $derived(useDateGrouping()
    ? dateGroups().map((group) => ({ key: group.key, label: group.key, hits: group.hits }))
    : groups.map((group) => ({ key: groupKey(group), label: groupLabel(group), hits: group.files.flatMap((file) => file.hits) })))
  let canLookup = $derived(inputValue.trim().length > 0 && !lookupBusy && !summaryBusy && !handoffBusy)
  let hasSummaryCandidates = $derived(authoritativeResults && (
    selectedKeys.length ? store.hits.filter((hit) => selectedSet.has(hitKey(hit))) : visibleHits
  ).some((hit) => (
    hit.level === 'line'
      && hit.text.trim().length > 0
      && Array.from(hit.text).length <= 3_000
      && Boolean((hit as SmartSearchHit).resultId)
  )))

  onMount(async () => {
    try {
      await Promise.all([loadLocale(), loadSettings()])
      unlistenLocale = await watchLocaleChanges()
      await windowApi.setTitle(t('smartSearch.windowTitle'))
    } catch (error) {
      console.warn('[smart-lookup] settings init failed:', error)
    }
    try {
      const [loadedAgents, defaultAgent] = await Promise.all([
        loadSearchAgentOptions(),
        loadDefaultAgentProvider().catch(() => ''),
      ])
      agents = loadedAgents
      appDefaultAgent = defaultAgent
      repairUnavailableProviders()
      migrateLegacyPlannerPreferences()
    } catch (error) {
      agentsError = readableError(error)
    }
    try {
      const { listen } = await import('@tauri-apps/api/event')
      unlistenSettings = await listen('settings://changed', async () => {
        await loadSettings()
        repairUnavailableProviders()
      })
    } catch { /* Browser preview has no event bridge. */ }
    ready = true
    await tick()
    resizeInput()
    inputEl?.focus()
    try {
      unlistenFocus = await windowApi.onFocusChanged(async ({ payload }) => {
        if (!payload) return
        await tick()
        if (!settingsOpen) inputEl?.focus()
      })
    } catch { /* Browser preview has no Tauri window. */ }
  })

  onDestroy(() => {
    cancelTimer()
    clearTimeout(copyTimer)
    supersedeActive(false)
    unlistenLocale?.()
    unlistenFocus?.()
    unlistenSettings?.()
  })

  function resolvePlannerAgent(): AgentOption | null {
    if (!settings.smartLookup.planner.enabled) return null
    const configured = settings.smartLookup.planner.provider
    if (configured !== 'auto') {
      return plannerAgents.find((agent) => agent.id === configured) ?? null
    }
    return plannerAgents.find((agent) => agent.id === appDefaultAgent)
      ?? plannerAgents.find((agent) => agent.id === 'notemd.claude-agent')
      ?? plannerAgents[0]
      ?? null
  }

  function resolveSummaryAgent(): AgentOption | null {
    if (!settings.smartLookup.summary.enabled) return null
    const configured = settings.smartLookup.summary.provider
    if (configured === 'same_as_planner' && plannerAgent && supportsSearchTask(plannerAgent, SEARCH_SUMMARY_TASK)) {
      return plannerAgent
    }
    if (configured !== 'auto' && configured !== 'same_as_planner') {
      return summaryAgents.find((agent) => agent.id === configured) ?? null
    }
    return summaryAgents.find((agent) => agent.id === appDefaultAgent)
      ?? summaryAgents.find((agent) => agent.id === 'notemd.claude-agent')
      ?? summaryAgents[0]
      ?? null
  }

  function migrateLegacyPlannerPreferences(): void {
    if (settings.smartLookup.planner.provider !== 'auto') return
    try {
      const legacy = localStorage.getItem('notemd.agent.provider.global-search')
      if (!legacy || !plannerAgents.some((agent) => agent.id === legacy)) return
      settings.smartLookup.planner.provider = legacy
      const harness = plannerAgents.find((agent) => agent.id === legacy)?.harness
      settings.smartLookup.planner.modelByProvider[legacy] = rememberedModelPreference(
        'global-search', legacy, 'plan', harness,
      )
      void saveSettings()
    } catch { /* A blocked localStorage leaves the new defaults intact. */ }
  }

  function repairUnavailableProviders(): void {
    let changed = false
    if (settings.smartLookup.planner.provider !== 'auto'
      && !plannerAgents.some((agent) => agent.id === settings.smartLookup.planner.provider)) {
      settings.smartLookup.planner.provider = 'auto'
      changed = true
    }
    if (!['auto', 'same_as_planner'].includes(settings.smartLookup.summary.provider)
      && !summaryAgents.some((agent) => agent.id === settings.smartLookup.summary.provider)) {
      settings.smartLookup.summary.provider = 'auto'
      changed = true
    }
    if (settings.smartLookup.handoff.defaultProvider !== 'ask'
      && !handoffAgents.some((agent) => agent.id === settings.smartLookup.handoff.defaultProvider)) {
      settings.smartLookup.handoff.defaultProvider = 'ask'
      changed = true
    }
    for (const agent of agents) {
      const plannerSaved = settings.smartLookup.planner.modelByProvider[agent.id]
      if (plannerSaved && supportsSearchPlanner(agent)) {
        const available = availableModelPreference(plannerSaved, 'plan', agent.harness)
        if (available !== plannerSaved) {
          settings.smartLookup.planner.modelByProvider[agent.id] = available
          modelWarning = t('smartSearch.modelFallback')
          changed = true
        }
      }
      const summarySaved = settings.smartLookup.summary.modelByProvider[agent.id]
      if (summarySaved && supportsSearchTask(agent, SEARCH_SUMMARY_TASK)) {
        const available = availableModelPreference(summarySaved, 'summary', agent.harness)
        if (available !== summarySaved) {
          settings.smartLookup.summary.modelByProvider[agent.id] = available
          modelWarning = t('smartSearch.modelFallback')
          changed = true
        }
      }
    }
    if (changed) void saveSettings()
  }

  function plannerPreference(agent = plannerAgent): ModelPreference {
    if (!agent) return 'profile:fast'
    return availableModelPreference(
      settings.smartLookup.planner.modelByProvider[agent.id]
        ?? rememberedModelPreference('global-search', agent.id, 'plan', agent.harness),
      'plan',
      agent.harness,
    )
  }

  function summaryPreference(agent = summaryAgent): ModelPreference {
    if (!agent) return 'profile:fast'
    return availableModelPreference(
      settings.smartLookup.summary.modelByProvider[agent.id] ?? 'profile:fast',
      'summary',
      agent.harness,
    )
  }

  function supportsIdempotentInvocation(agent: AgentOption): boolean {
    const tasks = agent.harness?.capabilities?.tasks ?? []
    return tasks.includes(SEARCH_SUMMARY_TASK) && tasks.includes(VAULT_RESEARCH_TASK)
  }

  function cancelTimer(): void {
    if (debounceTimer) clearTimeout(debounceTimer)
    debounceTimer = undefined
  }

  function supersedeActive(increment = true): number {
    if (increment) requestSequence += 1
    activeAbort?.abort()
    activeAbort = null
    if (activeAgentRun) {
      const run = activeAgentRun
      activeAgentRun = null
      void cancelAgentTask(run.provider, run.task, run.runId).catch(() => {})
    }
    return requestSequence
  }

  function resetResultEdits(): void {
    selectedKeys = []
    removedKeys = []
    lastRemoved = []
    activeKey = null
    rangeAnchor = null
    mobilePane = 'results'
  }

  function clearGeneratedState(): void {
    resolvedPlan = null
    submittedPlan = null
    planReferenceTime = ''
    planTimezone = ''
    lookupRunId = ''
    authoritativeResults = false
    plannerModel = null
    plannerWarning = ''
    summaryText = ''
    summaryError = ''
    summarySources = []
    summaryBusy = false
    handoffRun = null
    handoffError = ''
  }

  function scheduleSearch(): void {
    closeActionsMenu(false)
    cancelTimer()
    supersedeActive()
    resetResultEdits()
    clearGeneratedState()
    workflowEntries = []
    store.clear()
    const decision = decideTrigger(inputValue, composing)
    if (decision.kind === 'clear') {
      phase = 'idle'
      workflowEntries = []
      return
    }
    if (decision.kind === 'hold') return
    phase = 'previewing'
    const query = inputValue
    debounceTimer = setTimeout(() => { void runPreview(query) }, decision.delayMs)
  }

  async function runPreview(query: string): Promise<SmartSearchResponse | null> {
    if (!query.trim()) return null
    return await store.run(query, {
      deep: false,
      limit: settings.smartLookup.results.limit,
    })
  }

  function onInput(event: Event): void {
    resizeInput(event.currentTarget as HTMLTextAreaElement)
    if ((event as InputEvent).isComposing) {
      cancelTimer()
      return
    }
    scheduleSearch()
  }

  function resizeInput(element = inputEl): void {
    if (!element) return
    element.style.height = 'auto'
    element.style.height = `${Math.min(element.scrollHeight, 68)}px`
  }

  function onCompositionEnd(): void {
    composing = false
    inputIme.end()
    scheduleSearch()
  }

  function onInputKeydown(event: KeyboardEvent): void {
    if (inputIme.blocks(event)) return
    const cursorOnLastLine = inputEl && inputEl.selectionStart === inputEl.selectionEnd
      && !inputValue.slice(inputEl.selectionEnd).includes('\n')
    if (event.key === 'ArrowDown' && !event.shiftKey && flatVisibleHits.length && cursorOnLastLine) {
      event.preventDefault()
      focusResult(0)
      return
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      void runSmartLookup()
      return
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      void hideWindow()
    }
  }

  async function hideWindow(): Promise<void> {
    if (settingsOpen || actionsMenuOpen) {
      closeSettings()
      closeActionsMenu()
      return
    }
    cancelTimer()
    if (lookupBusy || summaryBusy) stopWorking()
    try { await invoke('hide_smart_search_window') } catch { /* Browser preview. */ }
  }

  function stopWorking(): void {
    supersedeActive()
    store.cancel()
    summaryBusy = false
    phase = authoritativeResults ? (store.hits.length ? 'ready' : 'no_results') : 'preview_only'
    appendActivity('plan', 'warning', t('smartSearch.stopped'))
  }

  async function clearQuery(): Promise<void> {
    inputValue = ''
    scheduleSearch()
    await tick()
    resizeInput()
    inputEl?.focus()
  }

  async function useExample(value: string): Promise<void> {
    inputValue = value
    resizeInput()
    await runSmartLookup()
  }

  async function openSettings(): Promise<void> {
    closeActionsMenu(false)
    settingsOpen = true
    await tick()
    settingsEl?.focus()
  }

  function closeSettings(): void {
    if (!settingsOpen) return
    settingsOpen = false
    settingsButton?.focus()
  }

  async function toggleActionsMenu(): Promise<void> {
    if (actionsMenuOpen) { closeActionsMenu(); return }
    positionActionsMenu()
    actionsMenuOpen = true
    await tick()
    actionsMenuEl?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()
  }

  function positionActionsMenu(): void {
    if (actionsButton) actionsMenuTop = Math.min(actionsButton.getBoundingClientRect().bottom + 4, window.innerHeight - 100)
  }

  function closeActionsMenu(restoreFocus = true): void {
    if (!actionsMenuOpen) return
    actionsMenuOpen = false
    if (restoreFocus) actionsButton?.focus()
  }

  function menuAction(action: () => unknown): void {
    closeActionsMenu()
    void action()
  }

  function onWindowKeydown(event: KeyboardEvent): void {
    if (isImeKey(event)) return
    if (actionsMenuOpen) {
      if (event.key === 'Escape') {
        event.preventDefault()
        closeActionsMenu()
        return
      }
      if (event.key === 'Tab') closeActionsMenu()
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault()
        const items = Array.from(actionsMenuEl?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])
        const index = items.indexOf(document.activeElement as HTMLButtonElement)
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1
          : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length
        items[next]?.focus()
        return
      }
    }
    if (settingsOpen && event.key === 'Tab' && settingsEl) {
      const controls = Array.from(settingsEl.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), summary'))
        .filter((el) => !el.closest('details:not([open])') || el.tagName === 'SUMMARY')
      const first = controls[0]
      const last = controls.at(-1)
      if (event.shiftKey && (document.activeElement === first || document.activeElement === settingsEl)) {
        event.preventDefault(); last?.focus()
      } else if (!event.shiftKey && (document.activeElement === last || document.activeElement === settingsEl)) {
        event.preventDefault(); first?.focus()
      }
      return
    }
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'f') {
      event.preventDefault()
      if (!settingsOpen) { closeActionsMenu(false); inputEl?.focus(); inputEl?.select() }
      return
    }
    if (event.key === 'Escape' && event.target !== inputEl) {
      event.preventDefault()
      void hideWindow()
    }
  }

  function focusResult(index: number, extend = false): void {
    const hit = flatVisibleHits[Math.max(0, Math.min(index, flatVisibleHits.length - 1))]
    if (!hit) return
    const key = hitKey(hit)
    selectedKeys = chooseResultKeys(flatVisibleHits.map(hitKey), selectedKeys, key, rangeAnchor, { toggle: false, range: extend })
    if (!extend || !rangeAnchor) rangeAnchor = key
    activeKey = key
    invalidateSummary()
    void tick().then(() => {
      const row = Array.from(resultsEl?.querySelectorAll<HTMLElement>('.result-row') ?? [])
        .find((el) => el.dataset.key === key)
      row?.focus()
      row?.scrollIntoView?.({ block: 'nearest' })
    })
  }

  function resetWorkflow(): void {
    workflowEntries = []
    workflowSequence = 0
    autoFollowActivity = true
    activityExpanded = null
  }

  function appendActivity(
    stage: WorkflowStage,
    level: WorkflowLevel,
    message: string,
    detail: { runId?: string; steps?: number } = {},
  ): void {
    const shouldFollow = !activityLogEl || autoFollowActivity || isNearLogBottom(activityLogEl)
    workflowEntries = appendWorkflowEntry(workflowEntries, {
      id: ++workflowSequence,
      stage,
      level,
      message,
      ...detail,
    })
    if (shouldFollow) void scrollActivityToEnd()
  }

  async function scrollActivityToEnd(): Promise<void> {
    await tick()
    if (!activityLogEl) return
    activityLogEl.scrollTop = activityLogEl.scrollHeight
    autoFollowActivity = true
  }

  function onActivityScroll(): void {
    if (activityLogEl) autoFollowActivity = isNearLogBottom(activityLogEl)
  }

  function readableError(error: unknown): string {
    let message = error instanceof Error ? error.message : String(error)
    if (error instanceof AgentTaskError) return readableAgentError(error)
    message = message
      .replace(/\b(?:sk|key)-[A-Za-z0-9_-]{8,}\b/g, '[redacted]')
      .replace(/\bBearer\s+\S+/gi, 'Bearer [redacted]')
      .replace(/\b(api[_-]?key|token|password)\s*[:=]\s*\S+/gi, '$1=[redacted]')
      .replace(/(^|[\s("'`])\/(?:[^/\s"'`]+\/)+[^/\s"'`,;:)]+/g, '$1[local path]')
      .replace(/\b[A-Za-z]:\\[^\s"'`,;]+/g, '[local path]')
      .replace(/\s+/g, ' ')
      .trim()
    return Array.from(message).slice(0, 180).join('')
  }

  function readableAgentError(error: unknown): string {
    if (error instanceof AgentTaskError) {
      if (error.status === 'timeout') return t('smartSearch.agentTimeout')
      if (error.status === 'cancelled') return t('smartSearch.stopped')
      if (error.status === 'incomplete' || error.status === 'empty') return t('smartSearch.agentIncomplete')
    }
    return t('smartSearch.agentFailed')
  }

  function localeTimezone(): string {
    try { return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC' } catch { return 'UTC' }
  }

  async function pollWithTimeout(
    provider: string,
    task: string,
    runId: string,
    timeoutMs: number,
    mine: number,
  ) {
    const controller = new AbortController()
    activeAbort = controller
    const timer = setTimeout(() => controller.abort(), timeoutMs)
    try {
      return await pollAgentTask(provider, task, runId, (progress) => {
        if (mine !== requestSequence) return
        appendActivity(
          task === SEARCH_PLAN_TASK ? 'plan' : 'summary',
          'active',
          t('smartSearch.activityStep', { n: progress.steps }),
          { runId, steps: progress.steps },
        )
      }, { signal: controller.signal })
    } catch (error) {
      if (controller.signal.aborted && mine === requestSequence) {
        void cancelAgentTask(provider, task, runId).catch(() => {})
        throw new AgentTaskError('', task, runId, 'timeout')
      }
      throw error
    } finally {
      clearTimeout(timer)
      if (activeAbort === controller) activeAbort = null
    }
  }

  async function ensurePreview(query: string, mine: number): Promise<void> {
    if (mine !== requestSequence || (store.query === query && store.route !== null)) return
    await runPreview(query)
  }

  function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error(message)), timeoutMs)
      promise.then(
        (value) => { clearTimeout(timer); resolve(value) },
        (error) => { clearTimeout(timer); reject(error) },
      )
    })
  }

  async function retryIdempotentStart<T>(start: () => Promise<T>, timeoutMs = 10_000): Promise<T> {
    const startedAt = Date.now()
    const firstBudget = Math.max(500, Math.min(5_000, Math.floor(timeoutMs / 2)))
    try {
      return await withTimeout(start(), firstBudget, 'Agent 启动等待超时')
    } catch (firstError) {
      const remaining = timeoutMs - (Date.now() - startedAt)
      if (remaining <= 0) throw firstError
      return await withTimeout(start(), remaining, 'Agent 启动等待超时')
    }
  }

  function parsePlan(content: string): unknown {
    if (new TextEncoder().encode(content).length > 16 * 1024) throw new Error('智能理解结果超过安全上限')
    try {
      return JSON.parse(content)
    } catch {
      throw new Error('智能理解没有返回有效的检索计划')
    }
  }

  async function runSmartLookup(): Promise<void> {
    const question = inputValue.trim()
    if (!question || lookupBusy || summaryBusy || handoffBusy) return
    cancelTimer()
    const mine = supersedeActive()
    resetResultEdits()
    clearGeneratedState()
    resetWorkflow()
    appendActivity('preview', 'active', t('smartSearch.activityPreviewReady'))

    const questionBytes = new TextEncoder().encode(question).length
    const provider = resolvePlannerAgent()
    if (!settings.smartLookup.planner.enabled || !provider || Array.from(question).length > 2_000 || questionBytes > 8 * 1024) {
      await ensurePreview(question, mine)
      if (mine !== requestSequence) return
      phase = 'preview_only'
      plannerWarning = !settings.smartLookup.planner.enabled
        ? t('smartSearch.plannerDisabled')
        : !provider
          ? t('smartSearch.plannerUnavailable')
          : t('smartSearch.questionTooLong')
      appendActivity('plan', 'warning', plannerWarning)
      return
    }

    phase = 'understanding'
    appendActivity('plan', 'active', t('smartSearch.activityPlanStart'))
    try {
      const referenceTime = new Date().toISOString()
      const timezone = localeTimezone()
      const context = await smartSearchApi.planContext(question, referenceTime, timezone)
      if (mine !== requestSequence) return
      const prompt = buildSearchPlanPrompt({
        question,
        referenceTime,
        timezone,
        locale: i18n.locale,
        lockedFilters: context.lockedFilters,
        referenceDate: context.referenceDate,
        timeAnchors: context.timeAnchors,
      })
      const preference = plannerPreference(provider)
      const selector = selectorForPreference(preference)
      const invocation = await createAgentInvocation(SEARCH_PLAN_TASK, prompt, selector)
      const planStartedAt = Date.now()
      const startTask = () => startAgentTask(
        provider.id,
        SEARCH_PLAN_TASK,
        prompt,
        selector,
        'result',
        undefined,
        invocation,
      )
      const start = supportsIdempotentInvocation(provider)
        ? await retryIdempotentStart(startTask, settings.smartLookup.planner.timeoutMs)
        : await withTimeout(startTask(), settings.smartLookup.planner.timeoutMs, 'Agent 启动等待超时')
      if (mine !== requestSequence) {
        void cancelAgentTask(provider.id, SEARCH_PLAN_TASK, start.runId).catch(() => {})
        return
      }
      activeAgentRun = { provider: provider.id, task: SEARCH_PLAN_TASK, runId: start.runId }
      plannerModel = start.resolvedModel ?? resolvedModelHint(preference, provider.harness)
      appendActivity('plan', 'active', t('smartSearch.activityAgentStarted', {
        model: plannerModel ?? t('smartSearch.modelFast'),
      }), { runId: start.runId })
      const planResult = await pollWithTimeout(
        provider.id,
        SEARCH_PLAN_TASK,
        start.runId,
        Math.max(1, settings.smartLookup.planner.timeoutMs - (Date.now() - planStartedAt)),
        mine,
      )
      if (mine !== requestSequence) return
      activeAgentRun = null
      const rawPlan = parsePlan(planResult.content)

      phase = 'searching'
      appendActivity('search', 'active', t('smartSearch.activitySearchStart'))
      let planned = await smartSearchApi.plannedSearch(
        question,
        rawPlan,
        referenceTime,
        timezone,
        {
          limit: settings.smartLookup.results.limit,
          deep: false,
          timeoutMs: 2_000,
          retainRun: settings.smartLookup.summary.enabled,
        },
      )
      if (mine !== requestSequence) return
      if (planned.search.hits.length === 0 && settings.smartLookup.results.autoDeepOnZero) {
        appendActivity('search', 'active', t('smartSearch.activityDeepStart'))
        planned = await smartSearchApi.plannedSearch(
          question,
          rawPlan,
          referenceTime,
          timezone,
          {
            limit: settings.smartLookup.results.limit,
            deep: true,
            timeoutMs: settings.smartLookup.results.deepTimeoutMs,
            retainRun: settings.smartLookup.summary.enabled,
          },
        )
      }
      if (mine !== requestSequence) return
      submittedPlan = rawPlan
      planReferenceTime = referenceTime
      planTimezone = timezone
      resolvedPlan = planned.resolvedPlan
      lookupRunId = planned.lookupRunId ?? ''
      authoritativeResults = true
      store.apply(question, planned.search)
      activeKey = planned.search.hits[0] ? hitKey(planned.search.hits[0]) : null
      phase = planned.search.hits.length === 0
        ? 'no_results'
        : planned.search.truncated
          ? 'partial'
          : 'ready'
      appendActivity('plan', 'success', t('smartSearch.activityPlanReady'), { runId: start.runId })
      appendPlanActivities(planned.search)
    } catch (error) {
      if (mine !== requestSequence) return
      activeAgentRun = null
      const detail = phase === 'understanding' ? readableAgentError(error) : readableError(error)
      await ensurePreview(question, mine)
      if (mine !== requestSequence) return
      phase = 'preview_only'
      plannerWarning = t('smartSearch.plannerFallback')
      appendActivity('plan', 'warning', `${plannerWarning} ${detail}`)
    }
  }

  function appendPlanActivities(search: SmartSearchResponse): void {
    if (resolvedPlan?.time?.after || resolvedPlan?.time?.before) {
      appendActivity('plan', 'success', t('smartSearch.activityTime', {
        range: `${resolvedPlan.time.after ?? '…'} – ${resolvedPlan.time.before ?? '…'}`,
      }))
    }
    if (resolvedTerms.length) {
      appendActivity('plan', 'success', t('smartSearch.activityTerms', {
        terms: resolvedTerms.join('、'),
      }))
    }
    const documents = new Set(search.hits.map((hit) => hit.path)).size
    appendActivity(
      'search',
      search.truncated ? 'warning' : 'success',
      t(search.truncated ? 'smartSearch.activitySearchPartial' : 'smartSearch.activitySearchDone', {
        n: search.hits.length,
        docs: documents,
      }),
    )
  }

  async function runDeep(): Promise<void> {
    const question = inputValue.trim()
    if (!question || store.loading || working) return
    const mine = requestSequence
    appendActivity('search', 'active', t('smartSearch.activityDeepStart'))
    if (authoritativeResults && submittedPlan && planReferenceTime && planTimezone) {
      phase = 'searching'
      try {
        const planned = await smartSearchApi.plannedSearch(
          question,
          submittedPlan,
          planReferenceTime,
          planTimezone,
          {
            limit: settings.smartLookup.results.limit,
            deep: true,
            timeoutMs: settings.smartLookup.results.deepTimeoutMs,
            retainRun: settings.smartLookup.summary.enabled,
          },
        )
        if (mine !== requestSequence) return
        resolvedPlan = planned.resolvedPlan
        lookupRunId = planned.lookupRunId ?? ''
        store.apply(question, planned.search)
        resetResultEdits()
        activeKey = planned.search.hits[0] ? hitKey(planned.search.hits[0]) : null
        phase = planned.search.hits.length === 0
          ? 'no_results'
          : planned.search.truncated
            ? 'partial'
            : 'ready'
        appendPlanActivities(planned.search)
      } catch (error) {
        if (mine !== requestSequence) return
        phase = store.hits.length === 0 ? 'no_results' : store.truncated ? 'partial' : 'ready'
        appendActivity('search', 'warning', `${t('smartSearch.searchError')}: ${readableError(error)}`)
      }
      return
    }
    phase = 'searching'
    const response = await store.run(question, {
      deep: true,
      limit: settings.smartLookup.results.limit,
      timeoutMs: settings.smartLookup.results.deepTimeoutMs,
    })
    if (mine !== requestSequence) return
    if (!response) { phase = 'preview_only'; return }
    authoritativeResults = false
    resolvedPlan = null
    lookupRunId = ''
    phase = response.hits.length === 0 ? 'no_results' : response.truncated ? 'partial' : 'preview_only'
    appendActivity('search', response.truncated ? 'warning' : 'success', t('smartSearch.activitySearchDone', {
      n: response.hits.length,
      docs: new Set(response.hits.map((hit) => hit.path)).size,
    }))
  }

  async function generateSummary(): Promise<void> {
    if (!lookupRunId || !summaryAgent || !hasSummaryCandidates || summaryBusy) return
    const provider = summaryAgent
    const sourceRunId = lookupRunId
    const summarySettings = { ...settings.smartLookup.summary }
    const mine = requestSequence
    summaryBusy = true
    summaryText = ''
    summaryError = ''
    summarySources = []
    appendActivity('summary', 'active', t('smartSearch.activitySummaryStart'))
    try {
      const selectedIds = (selectedKeys.length
        ? store.hits.filter((hit) => selectedSet.has(hitKey(hit)))
        : visibleHits)
        .map((hit) => (hit as SmartSearchHit).resultId)
        .filter((id): id is string => Boolean(id))
      const preference = summaryPreference(provider)
      const selector = selectorForPreference(preference)
      const invocationId = crypto.randomUUID()
      const startSummary = () => smartSearchApi.startSummary(
        sourceRunId,
        selectedIds,
        summarySettings.sourceLimit,
        summarySettings.charLimit,
        summarySettings.style,
        provider.id,
        selector,
        invocationId,
      )
      // The host/provider deduplicates the same invocation when an IPC response is lost.
      const startTimeout = Math.min(10_000, summarySettings.timeoutMs)
      const start = supportsIdempotentInvocation(provider)
        ? await retryIdempotentStart(startSummary, startTimeout)
        : await withTimeout(startSummary(), startTimeout, 'Agent 启动等待超时')
      if (mine !== requestSequence) {
        void cancelAgentTask(provider.id, SEARCH_SUMMARY_TASK, start.runId).catch(() => {})
        return
      }
      summarySources = start.sources
      if (start.staleCount > 0) {
        appendActivity('summary', 'warning', t('smartSearch.summaryStale', { n: start.staleCount }))
      }
      activeAgentRun = { provider: provider.id, task: SEARCH_SUMMARY_TASK, runId: start.runId }
      const result = await pollWithTimeout(
        provider.id,
        SEARCH_SUMMARY_TASK,
        start.runId,
        summarySettings.timeoutMs,
        mine,
      )
      if (mine !== requestSequence) return
      activeAgentRun = null
      summaryText = validateSummaryOutput(
        result.content,
        start.sources,
        summarySettings.style,
      ).content
      appendActivity('summary', 'success', t('smartSearch.activitySummaryDone'), { runId: start.runId })
      await tick()
      summaryCardEl?.scrollIntoView?.({ block: 'nearest' })
    } catch (error) {
      if (mine === requestSequence) {
        activeAgentRun = null
        summaryError = readableAgentError(error)
        appendActivity('summary', 'warning', `${t('smartSearch.summaryUnavailable')} ${summaryError}`)
      }
    } finally {
      if (mine === requestSequence) summaryBusy = false
    }
  }

  function invalidateSummary(): void {
    if (summaryBusy) {
      supersedeActive()
      summaryBusy = false
      appendActivity('summary', 'warning', t('smartSearch.stopped'))
    }
    summaryText = ''
    summaryError = ''
    summarySources = []
  }

  function handoffHits(): SearchHit[] {
    if (!settings.smartLookup.handoff.includeSelectedRefs) return []
    if (selectedKeys.length) {
      return store.hits.filter((hit) => selectedSet.has(hitKey(hit)))
    }
    return activeHit ? [activeHit] : []
  }

  async function handoff(providerId?: string): Promise<void> {
    closeActionsMenu()
    const packet = buildHandoffPacket(inputValue, resolvedPlan, handoffHits())
    const prompt = buildHandoffPrompt(packet)
    const configured = providerId
      ?? (settings.smartLookup.handoff.defaultProvider === 'ask'
        ? ''
        : settings.smartLookup.handoff.defaultProvider)
    const provider = handoffAgents.find((agent) => agent.id === configured)
    if (!provider) {
      const copied = await copyText(prompt)
      handoffError = t(copied ? 'smartSearch.handoffCopied' : 'smartSearch.copyFailed')
      appendActivity('handoff', copied ? 'success' : 'warning', handoffError)
      return
    }
    handoffBusy = true
    handoffError = ''
    appendActivity('handoff', 'active', t('smartSearch.activityHandoffStart'))
    try {
      const invocationId = crypto.randomUUID()
      const startHandoff = () => smartSearchApi.startHandoff(packet, provider.id, invocationId)
      // The provider deduplicates this exact invocation if the first response was lost.
      const start = supportsIdempotentInvocation(provider)
        ? await retryIdempotentStart(startHandoff)
        : await withTimeout(startHandoff(), 10_000, 'Agent 启动等待超时')
      handoffRun = { provider: provider.id, runId: start.runId }
      await openAgentWindow(provider.id)
      appendActivity('handoff', 'success', t('smartSearch.activityHandoffDone'), { runId: start.runId })
    } catch (error) {
      handoffError = readableAgentError(error)
      appendActivity('handoff', 'warning', `${t('smartSearch.handoffFailed')} ${handoffError}`)
    } finally {
      handoffBusy = false
    }
  }

  async function openAgentWindow(provider: string): Promise<void> {
    await invoke('plugin_v2_open_window', { pluginId: provider, windowId: 'main' })
  }

  function openHandoffRun(): void {
    if (handoffRun) void openAgentWindow(handoffRun.provider)
  }

  async function copyText(value: string): Promise<boolean> {
    try {
      try {
        const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
        await writeText(value)
      } catch {
        await navigator.clipboard.writeText(value)
      }
      copyFeedback = t('smartSearch.copyDone')
    } catch {
      copyFeedback = t('smartSearch.copyFailed')
      return false
    } finally {
      clearTimeout(copyTimer)
      copyTimer = setTimeout(() => { copyFeedback = '' }, 2_500)
    }
    return true
  }

  function simpleRef(hit: SearchHit): string {
    return `${hit.path}:${hit.line}${hit.lineEnd > hit.line ? `-${hit.lineEnd}` : ''}`
  }

  function markdownRef(hit: SearchHit): string {
    const title = hit.breadcrumb || basename(hit.path)
    const anchor = hit.lineEnd > hit.line ? `L${hit.line}-L${hit.lineEnd}` : `L${hit.line}`
    return `[${title}](${hit.path}#${anchor})`
  }

  function selectHit(event: MouseEvent, hit: SearchHit): void {
    const key = hitKey(hit)
    const orderedKeys = flatVisibleHits.map(hitKey)
    selectedKeys = chooseResultKeys(orderedKeys, selectedKeys, key, rangeAnchor, {
      toggle: event.metaKey || event.ctrlKey,
      range: event.shiftKey,
    })
    if (!event.shiftKey || !rangeAnchor) rangeAnchor = key
    activeKey = key
    mobilePane = 'preview'
    invalidateSummary()
  }

  function removeKeys(keys: string[]): void {
    const candidates = keys.filter((key) => store.hits.some((hit) => hitKey(hit) === key))
    if (!candidates.length) return
    removedKeys = addRemovedKeys(removedKeys, candidates)
    lastRemoved = candidates
    selectedKeys = []
    invalidateSummary()
    if (activeKey && candidates.includes(activeKey)) activeKey = null
  }

  function undoRemove(): void {
    removedKeys = restoreRemovedKeys(removedKeys, lastRemoved)
    selectedKeys = [...lastRemoved]
    activeKey = lastRemoved[0] ?? null
    lastRemoved = []
    invalidateSummary()
  }

  function changeSourceFilter(event: Event): void {
    sourceFilter = (event.currentTarget as HTMLSelectElement).value as SourceFilter
    selectedKeys = []
    rangeAnchor = null
    activeKey = null
    invalidateSummary()
  }

  function onResultsKeydown(event: KeyboardEvent, hit: SearchHit): void {
    if (isImeKey(event) || event.target !== event.currentTarget) return
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault()
      const index = flatVisibleHits.findIndex((candidate) => hitKey(candidate) === hitKey(hit))
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? flatVisibleHits.length - 1 : index + (event.key === 'ArrowDown' ? 1 : -1)
      focusResult(next, event.shiftKey)
    } else if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      selectedKeys = flatVisibleHits.map(hitKey)
      activeKey = hitKey(hit)
      rangeAnchor = activeKey
      invalidateSummary()
    } else if (event.key === 'Enter') {
      event.preventDefault()
      void openHit(hit, event.metaKey || event.ctrlKey)
    } else if ((event.key === 'Delete' || event.key === 'Backspace') && selectedKeys.length) {
      event.preventDefault()
      removeKeys(selectedKeys)
    }
  }

  async function openHit(hit: SearchHit, keepSearchWindow = false): Promise<void> {
    const preview = previewLine(hit.text, highlightTerms)
    navigationError = ''
    try {
      await invoke('editor_show_and_reveal_search_hit', {
        path: hit.absPath,
        line: Math.max(1, hit.line + preview.line),
        anchor: preview.text || hit.breadcrumb || basename(hit.path),
      })
      if (!keepSearchWindow) await invoke('hide_smart_search_window')
    } catch {
      navigationError = t('smartSearch.staleNavigation')
    }
  }

  function displayLine(hit: SearchHit) {
    return previewLine(hit.text, highlightTerms)
  }

  function basename(path: string): string {
    return path.slice(path.lastIndexOf('/') + 1)
  }

  function displayTitle(hit: SearchHit): string {
    return basename(hit.path).replace(/\.(md|mdx|markdown)$/i, '')
  }

  function formattedDate(value: string): string {
    const date = new Date(`${value.slice(0, 10)}T12:00:00`)
    if (Number.isNaN(date.getTime())) return value
    return new Intl.DateTimeFormat(i18n.locale, { year: 'numeric', month: 'short', day: 'numeric' }).format(date)
  }

  function previewText(hit: SearchHit): string {
    return Array.from(hit.text.slice(0, 6_000)).slice(0, 3_000).join('')
  }

  function groupLabel(group: HitGroup): string {
    switch (group.kind) {
      case 'pinned': return t('search.group.pinned')
      case 'human': return t('search.group.human')
      case 'source': return t('search.group.source')
      case 'unlabeled': return t('search.group.unlabeled')
      case 'derivedOther': return t('search.group.other')
      case 'derivedType': return group.conceptType ?? ''
    }
  }

  function groupKey(group: HitGroup): string {
    return group.kind === 'derivedType' ? `derived:${group.conceptType}` : group.kind
  }

  function dateKey(hit: SearchHit): string {
    return hit.docDate?.slice(0, 7) || t('smartSearch.dateUnknown')
  }

  function dateGroups(): Array<{ key: string; hits: SearchHit[] }> {
    const result: Array<{ key: string; hits: SearchHit[] }> = []
    for (const hit of visibleHits) {
      const key = dateKey(hit)
      let group = result.find((entry) => entry.key === key)
      if (!group) {
        group = { key, hits: [] }
        result.push(group)
      }
      group.hits.push(hit)
    }
    return result
  }

  function useDateGrouping(): boolean {
    return settings.smartLookup.results.groupBy === 'date'
      || (settings.smartLookup.results.groupBy === 'auto'
        && resolvedPlan !== null
        && resolvedPlan.sort !== 'relevance')
  }

  function relevanceLabel(reason: SmartRelevanceReason): string {
    switch (reason) {
      case 'exact_page': return 'Wiki'
      case 'strict_query': return t('smartSearch.reasonExact')
      case 'exact_phrase': return t('smartSearch.reasonPhrase')
      case 'filename_match': return t('smartSearch.reasonFilename')
      case 'breadcrumb_match': return t('smartSearch.reasonHeading')
      case 'multiple_queries': return t('smartSearch.reasonMultiple')
      case 'relaxed_query': return t('smartSearch.reasonRelated')
    }
  }

  function relevanceReasons(hit: SearchHit): SmartRelevanceReason[] {
    return (hit as SmartSearchHit).relevanceReasons ?? []
  }

  function planWarnings(): string[] {
    return [
      ...(resolvedPlan?.unsupportedConstraints ?? []).map((value) => `${t('smartSearch.unsupported')}: ${value}`),
      ...(resolvedPlan?.ambiguities ?? []).map((value) => `${t('smartSearch.ambiguous')}: ${value}`),
    ].slice(0, 6)
  }

  function planFilterChips(): string[] {
    const chips: string[] = []
    const seen = new Set<string>()
    for (const query of resolvedPlan?.queries ?? []) {
      for (const [label, key] of [
        ['path', 'paths'], ['tag', 'tags'], ['type', 'types'], ['ext', 'extensions'],
        ['origin', 'origins'], ['page', 'linkedPages'],
      ] as const) {
        const values = query.filters[key]
        if (!Array.isArray(values)) continue
        for (const value of values) {
          if (typeof value !== 'string' || !value || seen.has(`${label}:${value}`)) continue
          seen.add(`${label}:${value}`)
          const title = { path: t('smartSearch.scopeFolder'), tag: t('smartSearch.scopeTag'), type: t('smartSearch.scopeType'), ext: t('smartSearch.scopeFormat'), origin: t('smartSearch.sourceLabel'), page: t('smartSearch.scopePage') }[label]
          const content = label === 'origin'
            ? { human: t('search.group.human'), source: t('search.group.source'), derived: t('smartSearch.sourceDerived'), unlabeled: t('search.group.unlabeled') }[value] ?? value
            : value
          chips.push(`${title}: ${content}`)
          if (chips.length >= 12) return chips
        }
      }
    }
    return chips
  }

  function changePlannerProvider(event: Event): void {
    settings.smartLookup.planner.provider = (event.currentTarget as HTMLSelectElement).value
    void saveSettings()
  }

  function changePlannerModel(event: Event): void {
    if (!plannerAgent) return
    settings.smartLookup.planner.modelByProvider[plannerAgent.id]
      = (event.currentTarget as HTMLSelectElement).value as ModelPreference
    void saveSettings()
  }

  function changeSummaryStyle(event: Event): void {
    settings.smartLookup.summary.style
      = (event.currentTarget as HTMLSelectElement).value as 'sentence' | 'bullets'
    invalidateSummary()
    void saveSettings()
  }

  function changeSummaryProvider(event: Event): void {
    settings.smartLookup.summary.provider = (event.currentTarget as HTMLSelectElement).value
    invalidateSummary()
    void saveSettings()
  }

  function changeSummaryModel(event: Event): void {
    if (!summaryAgent) return
    settings.smartLookup.summary.modelByProvider[summaryAgent.id]
      = (event.currentTarget as HTMLSelectElement).value as ModelPreference
    invalidateSummary()
    void saveSettings()
  }

  function saveBoundedSeconds(
    event: Event,
    currentMs: number,
    minSeconds: number,
    maxSeconds: number,
    apply: (valueMs: number) => void,
  ): void {
    const input = event.currentTarget as HTMLInputElement
    const seconds = Number(input.value)
    if (!Number.isInteger(seconds) || seconds < minSeconds || seconds > maxSeconds) {
      input.value = String(currentMs / 1_000)
      return
    }
    apply(seconds * 1_000)
    invalidateSummary()
    void saveSettings()
  }

  function saveBoundedInteger(
    event: Event,
    current: number,
    min: number,
    max: number,
    apply: (value: number) => void,
  ): void {
    const input = event.currentTarget as HTMLInputElement
    const value = Number(input.value)
    if (!Number.isInteger(value) || value < min || value > max) {
      input.value = String(current)
      return
    }
    apply(value)
    invalidateSummary()
    void saveSettings()
  }

  function setBoolean(path: 'planner' | 'deep' | 'summary' | 'refs', checked: boolean): void {
    if (path === 'planner') settings.smartLookup.planner.enabled = checked
    if (path === 'deep') settings.smartLookup.results.autoDeepOnZero = checked
    if (path === 'summary') { settings.smartLookup.summary.enabled = checked; invalidateSummary() }
    if (path === 'refs') settings.smartLookup.handoff.includeSelectedRefs = checked
    void saveSettings()
  }
</script>

<svelte:window onkeydown={onWindowKeydown} onresize={() => { if (actionsMenuOpen) positionActionsMenu() }} />

{#snippet magnifier(size = 22)}
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><circle cx="10.8" cy="10.8" r="7.3"/><path d="m16.2 16.2 4.6 4.6"/></svg>
{/snippet}

<main class:expanded={store.route !== null || phase !== 'idle'} class:show-preview={mobilePane === 'preview'}>
  <div class="search-chrome">
    <section class="command-bar" aria-label={t('smartSearch.windowTitle')}>
      <span class="search-icon">{@render magnifier()}</span>
      <textarea
        bind:this={inputEl} bind:value={inputValue} rows="1" class="query-input" disabled={handoffBusy}
        placeholder={t('smartSearch.placeholder')} aria-label={t('smartSearch.placeholder')}
        oninput={onInput} onkeydown={onInputKeydown}
        oncompositionstart={() => { composing = true; inputIme.start(); cancelTimer() }}
        oncompositionend={onCompositionEnd} onblur={() => inputIme.reset()}
      ></textarea>
      {#if inputValue && !handoffBusy}
        <button class="icon-button clear-button" aria-label={t('smartSearch.clearQuery')} title={t('smartSearch.clearQuery')} onclick={() => void clearQuery()}>
          <svg width="17" height="17" viewBox="0 0 20 20" fill="currentColor" aria-hidden="true"><path fill-rule="evenodd" d="M10 1a9 9 0 1 0 0 18 9 9 0 0 0 0-18ZM6.8 5.8 10 9l3.2-3.2 1 1L11 10l3.2 3.2-1 1L10 11l-3.2 3.2-1-1L9 10 5.8 6.8l1-1Z"/></svg>
        </button>
      {/if}
      {#if lookupBusy || summaryBusy}
        <button class="lookup-button stop-button" onclick={stopWorking}><span class="stop-square"></span>{t('smartSearch.stop')}</button>
      {:else}
        <button class="lookup-button" disabled={!canLookup || !ready} onclick={() => void runSmartLookup()}>
          {t('smartSearch.lookup')} <kbd>↵</kbd>
        </button>
      {/if}
      <button bind:this={settingsButton} class="icon-button settings-button" aria-label={t('smartSearch.settings')} title={t('smartSearch.settings')} aria-expanded={settingsOpen} onclick={() => void openSettings()}>
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><path d="M4 7h5m4 0h7M4 17h9m4 0h3"/><circle cx="11" cy="7" r="2"/><circle cx="15" cy="17" r="2"/></svg>
      </button>
    </section>
    {#if !authoritativeResults}<div class="input-hint">{t('smartSearch.searchShortcut')}</div>{/if}
  </div>

  {#if resolvedPlan}
    <div class="plan-summary" aria-label={t('smartSearch.intelligentResults')}>
      {#if resolvedPlan.time?.after || resolvedPlan.time?.before}
        <span class="date-chip" title={t('smartSearch.dateScope')}>
          <svg width="13" height="13" viewBox="0 0 20 20" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true"><rect x="3" y="4" width="14" height="13" rx="2"/><path d="M6 2v4m8-4v4M3 8h14"/></svg>
          {resolvedPlan.time.after ? formattedDate(resolvedPlan.time.after) : '…'}{#if resolvedPlan.time.before !== resolvedPlan.time.after} – {resolvedPlan.time.before ? formattedDate(resolvedPlan.time.before) : '…'}{/if}
        </span>
      {/if}
      {#each planFilterChips() as filter}<span title={t('smartSearch.filterScope')}>{filter}</span>{/each}
      {#each resolvedTerms as term}<span title={t('smartSearch.topicsScope')}>{term}</span>{/each}
      {#if resolvedPlan.sort !== 'relevance'}<span>{t(resolvedPlan.sort === 'doc_date_desc' ? 'smartSearch.sortNewest' : 'smartSearch.sortOldest')}</span>{/if}
      {#each planWarnings() as warning}<span class="warning-chip">{warning}</span>{/each}
    </div>
  {:else if plannerWarning}
    <div class="preview-warning" role="status">
      <span>{plannerWarning}</span>
      {#if plannerAgent}<button disabled={working} onclick={() => void runSmartLookup()}>{t('smartSearch.retry')}</button>{/if}
      <button onclick={() => void openSettings()}>{t('smartSearch.settings')}</button>
    </div>
  {/if}
  {#if agentsError || modelWarning}
    <div class="agent-warning"><span>{modelWarning || t('smartSearch.localAvailable')}</span><button onclick={() => void openSettings()}>{t('smartSearch.settings')}</button></div>
  {/if}
  {#if navigationError}
    <div class="navigation-error" role="alert"><span>{navigationError}</span><button onclick={() => { navigationError = ''; void runSmartLookup() }}>{t('smartSearch.retry')}</button><button aria-label={t('common.close')} onclick={() => { navigationError = '' }}>×</button></div>
  {/if}

  {#if workflowEntries.length > 0}
    <section class="workflow-panel" class:working aria-label={t('smartSearch.activityTitle')} aria-busy={working}>
      <button class="workflow-toggle" aria-expanded={showActivity} aria-label={t(showActivity ? 'smartSearch.hideActivity' : 'smartSearch.showActivity')} onclick={() => { activityExpanded = !showActivity; if (activityExpanded) void scrollActivityToEnd() }}>
        {#if working}<span class="spinner"></span>{:else}<span class="status-dot" data-level={latestActivity?.level}></span>{/if}
        <span class="current-activity" role="status">{lookupBusy ? t(phase === 'understanding' ? 'smartSearch.understanding' : 'smartSearch.searching') : summaryBusy ? t('smartSearch.summaryWorking') : handoffBusy ? t('smartSearch.handoffWorking') : plannerWarning || (summaryError ? t('smartSearch.summaryUnavailable') : latestActivity?.message)}</span>
        <span class="activity-caption">{t('smartSearch.activityTitle')}</span>
        <svg class:rotated={showActivity} width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" aria-hidden="true"><path d="m5 3 5 5-5 5"/></svg>
      </button>
      {#if showActivity}
        <div class="workflow-log" bind:this={activityLogEl} role="log" aria-live="polite" aria-relevant="additions text" onscroll={onActivityScroll}>
          {#each workflowEntries as entry (entry.id)}
            <div class="workflow-row" data-level={entry.level}><span aria-hidden="true">{entry.level === 'success' ? '✓' : entry.level === 'warning' ? '!' : entry.level === 'error' ? '×' : '·'}</span><span>{entry.message}</span></div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}

  {#if store.route === null && phase === 'idle'}
    <div class="empty-launch">
      <span class="empty-mark">{@render magnifier(32)}</span>
      <h1>{t('smartSearch.emptyTitle')}</h1>
      <p>{t('smartSearch.emptyPrompt')}</p>
      <div class="example-queries">
        <button onclick={() => void useExample(t('smartSearch.exampleRecent'))}>{t('smartSearch.exampleRecent')}<span aria-hidden="true">↗</span></button>
        <button onclick={() => void useExample(t('smartSearch.exampleDecision'))}>{t('smartSearch.exampleDecision')}<span aria-hidden="true">↗</span></button>
      </div>
    </div>
  {:else}
    <nav class="mobile-tabs" aria-label={t('smartSearch.results')}>
      <button class:chosen={mobilePane === 'results'} aria-pressed={mobilePane === 'results'} onclick={() => { mobilePane = 'results' }}>{t('smartSearch.resultsTab')}<span>{visibleHits.length}</span></button>
      <button class:chosen={mobilePane === 'preview'} aria-pressed={mobilePane === 'preview'} onclick={() => { mobilePane = 'preview' }}>{t('smartSearch.previewTab')}</button>
    </nav>
    <section class="workspace">
      <aside class="results-pane" aria-label={t('smartSearch.results')}>
        <header class="pane-header">
          <strong>{authoritativeResults ? t('smartSearch.intelligentResults') : t('smartSearch.quickPreview')}</strong>
          <span>{t('smartSearch.resultsCount', { n: visibleHits.length })}</span>
          <select value={sourceFilter} onchange={changeSourceFilter} aria-label={t('smartSearch.sourceAll')}>
            <option value="all">{t('smartSearch.sourceAll')}</option><option value="human">{t('search.group.human')}</option><option value="source">{t('search.group.source')}</option><option value="derived">{t('smartSearch.sourceDerived')}</option><option value="unlabeled">{t('search.group.unlabeled')}</option>
          </select>
        </header>
        {#if store.truncated}<div class="partial-note">{t('smartSearch.partialResults', { n: store.hits.length })}<button disabled={working} onclick={() => void runDeep()}>{t('smartSearch.expandSearch')}</button></div>{/if}
        {#if selectedKeys.length || removedKeys.length}
          <div class="selection-bar">
            {#if selectedKeys.length}<span>{t('smartSearch.selectedCount', { n: selectedKeys.length })}</span>{:else}<span>{t('smartSearch.removedCount', { n: removedKeys.length })}</span>{/if}
            {#if lastRemoved.length}<button onclick={undoRemove}>{t('smartSearch.undo')}</button>{/if}
          </div>
        {/if}
        <div class="results-scroll" bind:this={resultsEl} role="listbox" aria-label={t('smartSearch.results')} aria-multiselectable="true" aria-busy={store.loading || lookupBusy}>
          {#if store.error}
            <div class="state error"><strong>{t('smartSearch.searchError')}</strong><p>{readableError(store.error)}</p><button class="secondary-button" onclick={() => void runSmartLookup()}>{t('smartSearch.retry')}</button></div>
          {:else if (store.loading || lookupBusy) && !visibleHits.length}
            <div class="state"><span class="spinner"></span><p>{t(phase === 'understanding' ? 'smartSearch.understanding' : 'smartSearch.searching')}</p></div>
          {:else if !visibleHits.length}
            <div class="state empty-results">
              {@render magnifier(26)}
              {#if sourceFilter !== 'all' && store.hits.some((hit) => !removedSet.has(hitKey(hit)))}
                <strong>{t('smartSearch.noFilteredResults')}</strong><button class="secondary-button" onclick={() => { sourceFilter = 'all' }}>{t('smartSearch.showAllSources')}</button>
              {:else if removedKeys.length}
                <strong>{t('smartSearch.noContext')}</strong><button class="secondary-button" onclick={undoRemove}>{t('smartSearch.undo')}</button><small>{t('smartSearch.notDeleted')}</small>
              {:else}
                <strong>{t('search.noResults')}</strong><p>{t('smartSearch.noResultsHint')}</p>
                <div class="state-actions"><button class="secondary-button" onclick={() => { inputEl?.focus(); inputEl?.select() }}>{t('smartSearch.editQuery')}</button>{#if store.deepAvailable}<button class="secondary-button" disabled={working} onclick={() => void runDeep()}>{t('smartSearch.expandSearch')}</button>{/if}</div>
              {/if}
            </div>
          {:else}
            {#each resultSections as group (group.key)}
              <section class="result-group" role="group" aria-label={group.label}>
                <h2>{group.label}<span>{group.hits.length}</span></h2>
                {#each group.hits as hit (hitKey(hit))}
                  {@const key = hitKey(hit)}
                  {@const line = displayLine(hit)}
                  <div class="result-row" class:selected={selectedSet.has(key)} class:active={activeHit && hitKey(activeHit) === key} data-key={key}
                    role="option" aria-selected={selectedSet.has(key)} tabindex={activeHit && hitKey(activeHit) === key ? 0 : -1}
                    onclick={(event) => selectHit(event, hit)} ondblclick={() => void openHit(hit)} onkeydown={(event) => onResultsKeydown(event, hit)}>
                    <svg class="document-icon" width="19" height="22" viewBox="0 0 20 24" fill="none" stroke="currentColor" stroke-width="1.3" aria-hidden="true"><path d="M4 2h8l5 5v14H4V2Z"/><path d="M12 2v5h5M7 12h7m-7 4h5"/></svg>
                    <div class="result-copy">
                      <div class="result-title"><strong>{displayTitle(hit)}</strong>{#if selectedSet.has(key)}<span class="selection-check" aria-hidden="true">✓</span>{/if}</div>
                      {#if hit.breadcrumb && hit.breadcrumb !== displayTitle(hit)}<div class="result-heading">{hit.breadcrumb}</div>{/if}
                      <p>{#each highlightParts(line.text || hit.text.slice(0, 240), highlightTerms) as part}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</p>
                      <div class="result-meta">{#if hit.docDate}<time>{formattedDate(hit.docDate)}</time>{/if}{#each relevanceReasons(hit).slice(0, 1) as reason}<span>{relevanceLabel(reason)}</span>{/each}{#if hit.humanVerified}<span>✓ {t('search.humanVerified')}</span>{/if}</div>
                    </div>
                  </div>
                {/each}
              </section>
            {/each}
          {/if}
        </div>
      </aside>

      <article class="preview-pane" aria-label={t('smartSearch.currentResult')}>
        <header class="pane-header preview-header">
          <strong>{t('smartSearch.previewTab')}</strong>
          {#if activeHit}<button class="text-button" onclick={() => void openHit(activeHit, true)}>{t('smartSearch.openEditor')} ↗</button>{/if}
          <button bind:this={actionsButton} class="icon-button more-actions" aria-label={t('smartSearch.moreActions')} title={t('smartSearch.moreActions')} aria-haspopup="menu" aria-expanded={actionsMenuOpen} onclick={() => void toggleActionsMenu()}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.7"/><circle cx="12" cy="12" r="1.7"/><circle cx="19" cy="12" r="1.7"/></svg>
          </button>
          {#if actionsMenuOpen}
            <button class="menu-scrim" tabindex="-1" aria-label={t('common.close')} onclick={() => closeActionsMenu()}></button>
            <div class="menu-panel actions-menu" bind:this={actionsMenuEl} style={`--actions-menu-top: ${actionsMenuTop}px`} role="menu" aria-label={t('smartSearch.moreActions')}>
              {#if activeHit}
                <button class="menu-row" role="menuitem" tabindex="-1" onclick={() => menuAction(() => copyText(markdownRef(activeHit!)))}>{t('smartSearch.copyMarkdown')}</button>
                <button class="menu-row" role="menuitem" tabindex="-1" onclick={() => menuAction(() => copyText(simpleRef(activeHit!)))}>{t('smartSearch.copyRef')}</button>
              {/if}
              {#if selectedKeys.length}
                <button class="menu-row" role="menuitem" tabindex="-1" onclick={() => menuAction(() => { selectedKeys = []; invalidateSummary() })}>{t('smartSearch.clearSelection')}</button>
                <button class="menu-row" role="menuitem" tabindex="-1" onclick={() => menuAction(() => removeKeys(selectedKeys))}>{t('smartSearch.removeSelected')}</button>
              {/if}
              {#if lastRemoved.length}<button class="menu-row" role="menuitem" tabindex="-1" onclick={() => menuAction(undoRemove)}>{t('smartSearch.undo')}</button>{/if}
              {#if settings.smartLookup.summary.enabled && authoritativeResults && visibleHits.length}
                <div class="menu-separator" role="separator"></div>
                <button class="menu-row summary-menu-action" role="menuitem" tabindex="-1" disabled={!lookupRunId || !summaryAgent || !hasSummaryCandidates || working} onclick={() => menuAction(generateSummary)}>{t('smartSearch.generateSummary')}</button>
                <p class="menu-hint">{!summaryAgent ? t('smartSearch.summaryAgentUnavailable') : !hasSummaryCandidates ? t('smartSearch.summaryNeedsLine') : selectedKeys.length ? t('smartSearch.summarySelection', { n: selectedKeys.length }) : t('smartSearch.summaryVisible')}</p>
              {/if}
              {#if summaryText}<button class="menu-row" role="menuitem" tabindex="-1" onclick={() => menuAction(() => copyText(summaryText))}>{t('smartSearch.copySummary')}</button>{/if}
              {#if activeHit || lastRemoved.length || summaryText}<div class="menu-separator" role="separator"></div>{/if}
              {#each handoffAgents.filter((agent) => settings.smartLookup.handoff.defaultProvider === 'ask' || agent.id === settings.smartLookup.handoff.defaultProvider) as agent (agent.id)}
                <button class="menu-row research-menu-action" role="menuitem" tabindex="-1" disabled={working || !inputValue.trim()} onclick={() => menuAction(() => handoff(agent.id))}>{t('smartSearch.handoff')} · {agent.harness?.harness || agent.name}</button>
              {/each}
              <button class="menu-row" role="menuitem" tabindex="-1" onclick={() => menuAction(() => handoff(''))}>{t('smartSearch.copyHandoff')}</button>
              {#if handoffRun}<button class="menu-row" role="menuitem" tabindex="-1" onclick={() => menuAction(openHandoffRun)}>{t('smartSearch.openAgentRun')} ↗</button>{/if}
            </div>
          {/if}
        </header>
        <div class="preview-scroll">
          {#if activeHit}
            <div class="preview-card">
              <div class="preview-location" title={simpleRef(activeHit)}>{activeHit.path}</div>
              <h1>{activeHit.breadcrumb || displayTitle(activeHit)}</h1>
              {#if activeHit.docDate}<time class="preview-date">{formattedDate(activeHit.docDate)}</time>{/if}
              {#if activeHit.level !== 'line' || activeHit.text.length > 3_000}<p class="block-note">{t('smartSearch.previewLimited')}</p>{/if}
              <p class="preview-text">{#each highlightParts(previewText(activeHit), highlightTerms) as part}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</p>
            </div>
          {:else}<div class="state preview-empty"><span class="empty-mark">{@render magnifier(28)}</span><p>{t('smartSearch.previewHint')}</p></div>{/if}

          {#if summaryError}<p class="summary-error" role="alert">{summaryError}</p>{/if}
          {#if summaryText && summarySources.length}
            <div class="summary-card" bind:this={summaryCardEl}><header><strong>{t('smartSearch.quickSummary')}</strong><span>{t('smartSearch.basedOnMatches')}</span></header><p>{summaryText}</p><div class="summary-sources">{#each summarySources as source}<button onclick={() => { const hit = store.hits.find((candidate) => candidate.path === source.path && candidate.line === source.line); if (hit) void openHit(hit, true) }}>[{source.id}] {basename(source.path)}</button>{/each}</div></div>
          {/if}
          {#if handoffError}<p class="action-hint" role="status">{handoffError}</p>{/if}
        </div>
      </article>
    </section>
  {/if}
  <footer class="window-footer"><span>{t('smartSearch.keyboardHint', { modifier: /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘' : 'Ctrl+' })}</span><span class="copy-feedback" role="status">{copyFeedback}</span></footer>

  {#if settingsOpen}
    <button class="settings-scrim" aria-label={t('common.close')} onclick={closeSettings}></button>
    <div class="menu-panel lookup-settings" bind:this={settingsEl} role="dialog" aria-modal="true" aria-label={t('smartSearch.settings')} tabindex="-1">
      <header class="settings-header"><div><strong>{t('smartSearch.settings')}</strong><small>{t('smartSearch.settingsHint')}</small></div><button class="primary-action" onclick={closeSettings}>{t('smartSearch.settingsDone')}</button></header>
      <div class="settings-content"><h2>{t('smartSearch.settingsSearch')}</h2>
      <label class="setting-row">
        <span><strong>{t('smartSearch.smartUnderstanding')}</strong><small>{t('smartSearch.smartUnderstandingHint')}</small></span>
        <input type="checkbox" checked={settings.smartLookup.planner.enabled} onchange={(event) => setBoolean('planner', event.currentTarget.checked)} />
      </label>
      <label class="setting-row">
        <span>{t('smartSearch.plannerProvider')}</span>
        <select value={settings.smartLookup.planner.provider} onchange={changePlannerProvider}>
          <option value="auto">{t('smartSearch.auto')}</option>
          {#each plannerAgents as agent (agent.id)}<option value={agent.id}>{agent.harness?.harness || agent.name}</option>{/each}
        </select>
      </label>
      {#if plannerAgent}
        <label class="setting-row">
          <span>{t('smartSearch.plannerModel')}</span>
          <select value={plannerPreference()} onchange={changePlannerModel}>
            {#if plannerAgent.harness?.capabilities?.model_routing?.profiles?.fast?.available}
              <option value="profile:fast">{t('smartSearch.modelFast')}</option>
            {/if}
            {#if plannerAgent.harness?.capabilities?.model_routing?.profiles?.default?.available}
              <option value="profile:default">{t('smartSearch.modelDefault')}</option>
            {/if}
            {#each selectableModelPreferences(plannerAgent.harness) as preference}<option value={preference}>{preference.slice(6)}</option>{/each}
          </select>
        </label>
      {/if}
      <h2>{t('smartSearch.settingsResults')}</h2>
      <label class="setting-row">
        <span>{t('smartSearch.resultLimit')}</span>
        <select value={settings.smartLookup.results.limit} onchange={(event) => {
          settings.smartLookup.results.limit = Number(event.currentTarget.value) as 20 | 50 | 100; void saveSettings()
        }}><option value={20}>20</option><option value={50}>50</option><option value={100}>100</option></select>
      </label>
      <label class="setting-row">
        <span>{t('smartSearch.groupBy')}</span>
        <select value={settings.smartLookup.results.groupBy} onchange={(event) => {
          settings.smartLookup.results.groupBy = event.currentTarget.value as 'auto' | 'source' | 'date'; void saveSettings()
        }}>
          <option value="auto">{t('smartSearch.auto')}</option>
          <option value="source">{t('smartSearch.groupSource')}</option>
          <option value="date">{t('smartSearch.groupDate')}</option>
        </select>
      </label>
      <label class="setting-row">
        <span>{t('smartSearch.autoDeep')}</span>
        <input type="checkbox" checked={settings.smartLookup.results.autoDeepOnZero} onchange={(event) => setBoolean('deep', event.currentTarget.checked)} />
      </label>
      <h2>{t('smartSearch.settingsSummary')}</h2>
      <label class="setting-row">
        <span>{t('smartSearch.quickSummary')}</span>
        <input type="checkbox" checked={settings.smartLookup.summary.enabled} onchange={(event) => setBoolean('summary', event.currentTarget.checked)} />
      </label>
      <label class="setting-row">
        <span>{t('smartSearch.summaryStyle')}</span>
        <select value={settings.smartLookup.summary.style} onchange={changeSummaryStyle}>
          <option value="bullets">{t('smartSearch.summaryBullets')}</option>
          <option value="sentence">{t('smartSearch.summarySentence')}</option>
        </select>
      </label>
      <label class="setting-row">
        <span>{t('smartSearch.includeRefs')}</span>
        <input type="checkbox" checked={settings.smartLookup.handoff.includeSelectedRefs} onchange={(event) => setBoolean('refs', event.currentTarget.checked)} />
      </label>
      <details class="advanced-settings">
        <summary>{t('smartSearch.advancedSettings')}</summary>
        <label class="setting-row">
          <span>{t('smartSearch.plannerTimeout')}</span>
          <input type="number" min="10" max="60" step="1" value={settings.smartLookup.planner.timeoutMs / 1_000}
            onchange={(event) => saveBoundedSeconds(event, settings.smartLookup.planner.timeoutMs, 10, 60, (value) => { settings.smartLookup.planner.timeoutMs = value })} />
        </label>
        <label class="setting-row">
          <span>{t('smartSearch.deepTimeout')}</span>
          <input type="number" min="1" max="5" step="1" value={settings.smartLookup.results.deepTimeoutMs / 1_000}
            onchange={(event) => saveBoundedSeconds(event, settings.smartLookup.results.deepTimeoutMs, 1, 5, (value) => { settings.smartLookup.results.deepTimeoutMs = value })} />
        </label>
        <label class="setting-row">
          <span>{t('smartSearch.summaryProvider')}</span>
          <select value={settings.smartLookup.summary.provider} onchange={changeSummaryProvider}>
            <option value="same_as_planner">{t('smartSearch.sameAsPlanner')}</option>
            <option value="auto">{t('smartSearch.auto')}</option>
            {#each summaryAgents as agent (agent.id)}<option value={agent.id}>{agent.harness?.harness || agent.name}</option>{/each}
          </select>
        </label>
        {#if summaryAgent}
          <label class="setting-row">
            <span>{t('smartSearch.summaryModel')}</span>
            <select value={summaryPreference()} onchange={changeSummaryModel}>
              {#if summaryAgent.harness?.capabilities?.model_routing?.profiles?.fast?.available}
                <option value="profile:fast">{t('smartSearch.modelFast')}</option>
              {/if}
              {#if summaryAgent.harness?.capabilities?.model_routing?.profiles?.default?.available}
                <option value="profile:default">{t('smartSearch.modelDefault')}</option>
              {/if}
              {#each selectableModelPreferences(summaryAgent.harness) as preference}<option value={preference}>{preference.slice(6)}</option>{/each}
            </select>
          </label>
        {/if}
        <label class="setting-row">
          <span>{t('smartSearch.summarySources')}</span>
          <input type="number" min="1" max="6" step="1" value={settings.smartLookup.summary.sourceLimit}
            onchange={(event) => saveBoundedInteger(event, settings.smartLookup.summary.sourceLimit, 1, 6, (value) => { settings.smartLookup.summary.sourceLimit = value })} />
        </label>
        <label class="setting-row">
          <span>{t('smartSearch.summaryChars')}</span>
          <input type="number" min="1000" max="6000" step="500" value={settings.smartLookup.summary.charLimit}
            onchange={(event) => saveBoundedInteger(event, settings.smartLookup.summary.charLimit, 1000, 6000, (value) => { settings.smartLookup.summary.charLimit = value })} />
        </label>
        <label class="setting-row">
          <span>{t('smartSearch.summaryTimeout')}</span>
          <input type="number" min="5" max="30" step="1" value={settings.smartLookup.summary.timeoutMs / 1_000}
            onchange={(event) => saveBoundedSeconds(event, settings.smartLookup.summary.timeoutMs, 5, 30, (value) => { settings.smartLookup.summary.timeoutMs = value })} />
        </label>
        <label class="setting-row">
          <span>{t('smartSearch.handoffProvider')}</span>
          <select value={settings.smartLookup.handoff.defaultProvider} onchange={(event) => {
            settings.smartLookup.handoff.defaultProvider = event.currentTarget.value; void saveSettings()
          }}>
            <option value="ask">{t('smartSearch.askEveryTime')}</option>
            {#each handoffAgents as agent (agent.id)}<option value={agent.id}>{agent.harness?.harness || agent.name}</option>{/each}
          </select>
        </label>
      </details></div>
    </div>
  {/if}
</main>

<style>
  :global(:root) { color-scheme: light dark; --smart-accent: #0869ed; --smart-border: color-mix(in srgb, CanvasText 12%, transparent); --smart-muted: color-mix(in srgb, CanvasText 60%, transparent); --smart-soft: color-mix(in srgb, CanvasText 4%, transparent); }
  :global(body) { background: Canvas; color: CanvasText; }
  button, select, textarea, input { font: inherit; }
  button { color: inherit; cursor: default; }
  button:disabled { opacity: .4; cursor: default; }
  button:focus-visible, select:focus-visible, input:focus-visible, .result-row:focus-visible, summary:focus-visible { outline: 3px solid color-mix(in srgb, var(--smart-accent) 55%, transparent); outline-offset: 2px; }
  main { position: relative; height: 100dvh; height: 100vh; min-height: 150px; display: flex; flex-direction: column; background: Canvas; overflow: hidden; }
  .search-chrome { flex: 0 0 auto; padding: 18px 20px 10px; background: color-mix(in srgb, CanvasText 2.5%, Canvas); }
  .command-bar { display: flex; align-items: center; gap: 12px; min-height: 52px; padding: 5px 8px 5px 14px; box-sizing: border-box; border: 1px solid var(--smart-border); border-radius: 12px; background: Canvas; box-shadow: 0 2px 5px #00000005; }
  .command-bar:focus-within { border-color: color-mix(in srgb, var(--smart-accent) 58%, var(--smart-border)); box-shadow: 0 0 0 3px color-mix(in srgb, var(--smart-accent) 12%, transparent); }
  .search-icon { display: flex; color: var(--smart-muted); flex-shrink: 0; }
  .query-input { flex: 1; min-width: 0; resize: none; border: 0; outline: 0; background: transparent; color: CanvasText; min-height: 24px; max-height: 68px; padding: 4px 0; line-height: 1.5; overflow-y: auto; font-size: 17px; }
  .query-input::placeholder { color: var(--smart-muted); }
  .lookup-button, .primary-action { min-height: 32px; border: 0; border-radius: 7px; background: var(--smart-accent); color: #fff; padding: 0 13px; font-weight: 550; font-size: 13px; white-space: nowrap; }
  .lookup-button { display: flex; align-items: center; gap: 8px; flex-shrink: 0; }
  .lookup-button kbd { font: inherit; opacity: .8; }
  .stop-button { background: var(--smart-soft); color: CanvasText; border: 1px solid var(--smart-border); }
  .stop-square { width: 9px; height: 9px; border-radius: 2px; background: currentColor; }
  .icon-button { border: 0; background: transparent; color: var(--smart-muted); display: grid; place-items: center; width: 30px; height: 32px; padding: 0; border-radius: 6px; flex-shrink: 0; }
  .icon-button:hover { background: var(--smart-soft); }
  .input-hint { padding: 9px 2px 0; font-size: 11px; color: var(--smart-muted); }
  .plan-summary { display: flex; gap: 6px 12px; flex-wrap: wrap; padding: 2px 22px 12px; flex-shrink: 0; max-height: 90px; overflow: auto; background: color-mix(in srgb, CanvasText 2.5%, Canvas); }
  .plan-summary > span { display: inline-flex; align-items: center; gap: 5px; color: var(--smart-muted); font-size: 11px; line-height: 1.5; }
  .plan-summary .date-chip { color: var(--smart-accent); }
  .plan-summary .warning-chip { color: #ad7000; }
  .preview-warning, .agent-warning, .navigation-error, .partial-note { flex-shrink: 0; display: flex; align-items: center; gap: 10px; padding: 9px 20px; background: color-mix(in srgb, #dc9800 9%, Canvas); font-size: 12px; line-height: 1.45; }
  .preview-warning > span, .agent-warning > span, .navigation-error > span { flex: 1; min-width: 0; }
  .preview-warning button, .agent-warning button, .navigation-error button, .partial-note button, .selection-bar button { flex-shrink: 0; border: 0; padding: 4px 0; background: transparent; color: var(--smart-accent); font-size: 12px; }
  .navigation-error { background: color-mix(in srgb, #ce3030 8%, Canvas); }
  .workflow-panel { flex: 0 0 auto; border-top: 1px solid var(--smart-border); background: color-mix(in srgb, CanvasText 2%, Canvas); }
  .workflow-toggle { border: 0; background: transparent; width: 100%; padding: 10px 22px; display: flex; align-items: center; gap: 9px; text-align: left; font-size: 12px; }
  .current-activity { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .activity-caption { color: var(--smart-muted); font-size: 11px; }
  .workflow-toggle svg { color: var(--smart-muted); transition: transform .15s; }
  .workflow-toggle svg.rotated { transform: rotate(90deg); }
  .status-dot { width: 7px; height: 7px; border-radius: 50%; background: #319365; flex-shrink: 0; margin: 0 3px; }
  .status-dot[data-level='warning'], .status-dot[data-level='error'] { background: #bd800a; }
  .workflow-log { max-height: 96px; overflow-y: auto; padding: 0 22px 10px; }
  .workflow-row { display: grid; grid-template-columns: 13px minmax(0,1fr); gap: 8px; padding: 3px 0; color: var(--smart-muted); font-size: 12px; line-height: 1.45; }
  .workflow-row[data-level='success'] > span:first-child { color: #248156; }
  .workflow-row[data-level='warning'] > span:first-child { color: #bd800a; }
  .spinner { display: inline-block; flex-shrink: 0; width: 12px; height: 12px; border: 2px solid color-mix(in srgb, var(--smart-accent) 18%, transparent); border-top-color: var(--smart-accent); border-radius: 50%; animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .empty-launch { flex: 1; min-height: 0; display: flex; flex-direction: column; justify-content: center; align-items: center; padding: 24px; text-align: center; overflow-y: auto; }
  .empty-mark { display: grid; place-items: center; width: 58px; height: 58px; color: var(--smart-accent); background: color-mix(in srgb, var(--smart-accent) 7%, Canvas); border-radius: 16px; flex-shrink: 0; }
  .empty-launch h1 { font-size: 21px; font-weight: 600; letter-spacing: -.4px; margin: 20px 0 8px; }
  .empty-launch p { color: var(--smart-muted); margin: 0; font-size: 13px; line-height: 1.6; max-width: 380px; }
  .example-queries { display: grid; gap: 8px; margin-top: 24px; width: min(100%, 360px); }
  .example-queries button { display: flex; justify-content: space-between; gap: 14px; padding: 11px 13px; border: 1px solid var(--smart-border); border-radius: 8px; background: var(--smart-soft); text-align: left; font-size: 13px; }
  .example-queries button:hover { background: color-mix(in srgb, var(--smart-accent) 7%, Canvas); }
  .example-queries span { color: var(--smart-muted); }
  .workspace { min-height: 0; flex: 1; display: grid; grid-template-columns: minmax(280px, 42%) minmax(0, 58%); border-top: 1px solid var(--smart-border); }
  .results-pane, .preview-pane { min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .results-pane { border-right: 1px solid var(--smart-border); background: color-mix(in srgb, CanvasText 1.5%, Canvas); }
  .pane-header { flex: 0 0 auto; min-height: 43px; display: flex; align-items: center; gap: 8px; padding: 0 16px; border-bottom: 1px solid var(--smart-border); }
  .pane-header strong { margin-right: auto; font-size: 12px; font-weight: 600; }
  .pane-header > span { color: var(--smart-muted); font-size: 11px; white-space: nowrap; }
  .pane-header select { max-width: 122px; min-width: 0; height: 27px; border: 0; border-radius: 6px; background: transparent; color: var(--smart-muted); font-size: 11px; }
  .preview-header { position: relative; }
  .selection-bar { flex-shrink: 0; display: flex; align-items: center; gap: 12px; padding: 5px 16px; border-bottom: 1px solid var(--smart-border); font-size: 11px; }
  .selection-bar > span { margin-right: auto; color: var(--smart-muted); }
  .results-scroll { flex: 1; min-height: 0; overflow-y: auto; padding: 7px 9px; }
  .result-group h2 { display: flex; justify-content: space-between; margin: 13px 8px 7px; color: var(--smart-muted); font-size: 11px; font-weight: 600; }
  .result-group:first-child h2 { margin-top: 5px; }
  .result-row { display: grid; grid-template-columns: 22px minmax(0,1fr); gap: 9px; padding: 12px 10px; margin: 3px 0; border: 1px solid transparent; border-radius: 8px; cursor: default; scroll-margin: 8px; }
  .result-row:hover { background: var(--smart-soft); }
  .result-row.active { background: color-mix(in srgb, var(--smart-accent) 9%, Canvas); }
  .result-row.selected { border-color: color-mix(in srgb, var(--smart-accent) 19%, transparent); background: color-mix(in srgb, var(--smart-accent) 11%, Canvas); }
  .document-icon { color: var(--smart-muted); margin-top: 1px; }
  .result-copy { min-width: 0; }
  .result-title { display: flex; align-items: baseline; gap: 8px; }
  .result-title strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; font-weight: 600; }
  .selection-check { color: var(--smart-accent); font-size: 11px; margin-left: auto; }
  .result-heading { margin-top: 3px; font-size: 11px; color: var(--smart-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .result-copy p { margin: 5px 0 7px; overflow: hidden; display: -webkit-box; line-clamp: 2; -webkit-line-clamp: 2; -webkit-box-orient: vertical; color: color-mix(in srgb, CanvasText 82%, transparent); font-size: 12px; line-height: 1.55; overflow-wrap: anywhere; }
  mark { border-radius: 2px; background: color-mix(in srgb, #edbd42 30%, transparent); color: inherit; }
  .result-meta { display: flex; gap: 8px; color: var(--smart-muted); font-size: 11px; flex-wrap: wrap; }
  .preview-scroll { flex: 1; min-height: 0; overflow-y: auto; padding: 0 28px 20px; }
  .preview-card { padding: 26px 0; }
  .preview-location { color: var(--smart-muted); font-size: 11px; overflow-wrap: anywhere; line-height: 1.5; }
  .preview-card h1 { font-size: 23px; line-height: 1.35; font-weight: 650; letter-spacing: -.5px; margin: 9px 0 8px; overflow-wrap: anywhere; }
  .preview-date { color: var(--smart-muted); font-size: 12px; }
  .preview-card .preview-text { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 14px; line-height: 1.85; margin: 22px 0 0; }
  .block-note { margin: 15px 0 0; padding: 9px 11px; background: var(--smart-soft); border-radius: 6px; color: var(--smart-muted); font-size: 12px; line-height: 1.5; }
  .secondary-button { border: 1px solid var(--smart-border); border-radius: 7px; background: Canvas; padding: 6px 10px; font-size: 12px; min-height: 31px; }
  .secondary-button:hover { background: var(--smart-soft); }
  .text-button { padding: 3px 0; background: transparent; border: 0; color: var(--smart-accent); font-size: 12px; }
  .action-hint, .summary-error { margin: 0 0 16px; font-size: 12px; line-height: 1.5; color: var(--smart-muted); }
  .summary-error { color: #c93434; }
  .summary-card { border-top: 1px solid var(--smart-border); padding: 20px 0; }
  .summary-card header { display: grid; gap: 6px; color: var(--smart-muted); font-size: 11px; }
  .summary-card header strong { color: CanvasText; font-size: 13px; }
  .summary-card p { font-size: 14px; line-height: 1.75; white-space: pre-wrap; }
  .summary-sources { display: flex; gap: 7px; flex-wrap: wrap; margin-bottom: 12px; }
  .summary-sources button { border: 0; background: transparent; color: var(--smart-accent); padding: 0; font-size: 11px; text-align: left; overflow-wrap: anywhere; }
  .actions-menu { position: fixed; z-index: 12; top: var(--actions-menu-top); right: 12px; width: 240px; max-width: calc(100vw - 24px); max-height: min(440px, calc(100dvh - var(--actions-menu-top) - 12px)); overflow-y: auto; box-sizing: border-box; }
  .menu-hint { margin: 2px 9px 8px; color: var(--smart-muted); font-size: 11px; line-height: 1.5; }
  .menu-separator { border-top: 1px solid var(--smart-border); margin: 5px 3px; }
  .menu-row { width: 100%; border: 0; background: transparent; text-align: left; padding: 7px 9px; }
  .menu-scrim { position: fixed; inset: 0; z-index: 11; background: transparent; border: 0; }
  .state { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 13px; padding: 48px 20px; text-align: center; color: var(--smart-muted); font-size: 13px; line-height: 1.6; }
  .state strong { color: CanvasText; font-weight: 550; }
  .state p { margin: 0; }
  .state-actions { display: flex; gap: 9px; flex-wrap: wrap; }
  .preview-empty { min-height: 170px; }
  .window-footer { flex: 0 0 auto; display: flex; justify-content: space-between; padding: 8px 20px; border-top: 1px solid var(--smart-border); color: var(--smart-muted); font-size: 11px; min-height: 16px; }
  .copy-feedback { color: var(--smart-accent); font-weight: 550; }
  .settings-scrim { position: absolute; inset: 0; z-index: 20; background: #00000024; border: 0; }
  .lookup-settings { position: absolute; z-index: 21; inset: 20px 20px 20px auto; width: min(430px, calc(100% - 40px)); display: flex; flex-direction: column; padding: 0; outline: 0; box-sizing: border-box; background: Canvas; overflow: hidden; }
  .settings-header { display: flex; align-items: center; justify-content: space-between; padding: 18px 20px; border-bottom: 1px solid var(--smart-border); flex-shrink: 0; }
  .settings-header > div { display: grid; gap: 4px; }
  .settings-header strong { font-size: 15px; font-weight: 600; }
  .settings-header small { color: var(--smart-muted); font-size: 11px; }
  .settings-content { padding: 2px 20px 20px; min-height: 0; overflow-y: auto; }
  .settings-content h2 { margin: 21px 0 6px; color: var(--smart-muted); font-size: 11px; font-weight: 600; }
  .setting-row { display: flex; align-items: center; justify-content: space-between; gap: 16px; width: 100%; min-height: 44px; box-sizing: border-box; border-bottom: 1px solid color-mix(in srgb, CanvasText 6%, transparent); font-size: 13px; }
  .setting-row > span:first-child { display: grid; gap: 3px; padding: 9px 0; }
  .setting-row strong { font-weight: 550; }
  .setting-row small { color: var(--smart-muted); font-size: 11px; line-height: 1.45; }
  .setting-row select { max-width: 175px; min-width: 85px; color: CanvasText; background: var(--smart-soft); border: 1px solid var(--smart-border); border-radius: 5px; padding: 4px; font-size: 12px; }
  .setting-row input[type='number'] { width: 65px; padding: 4px; background: var(--smart-soft); color: CanvasText; border: 1px solid var(--smart-border); border-radius: 5px; }
  .setting-row input[type='checkbox'] { width: 16px; height: 16px; accent-color: var(--smart-accent); flex-shrink: 0; }
  .advanced-settings { margin-top: 18px; }
  .advanced-settings summary { padding: 8px 0; color: var(--smart-muted); font-size: 12px; }
  .mobile-tabs { display: none; }
  @media (max-width: 760px) {
    .search-chrome { padding: 12px 12px 8px; }
    .command-bar { gap: 7px; padding-left: 10px; }
    .query-input { font-size: 15px; }
    .input-hint { font-size: 10px; }
    .plan-summary { padding: 5px 12px 9px; max-height: 66px; }
    .workspace { grid-template-columns: minmax(0, 1fr); }
    .preview-pane { display: none; }
    .results-pane { border: 0; }
    .show-preview .results-pane { display: none; }
    .show-preview .preview-pane { display: flex; }
    .mobile-tabs { flex-shrink: 0; display: flex; padding: 6px 12px; gap: 4px; border-top: 1px solid var(--smart-border); }
    .mobile-tabs button { flex: 1; border: 0; border-radius: 6px; padding: 7px; background: transparent; color: var(--smart-muted); font-size: 12px; }
    .mobile-tabs .chosen { background: var(--smart-soft); color: CanvasText; font-weight: 550; }
    .mobile-tabs span { margin-left: 7px; opacity: .65; }
    .preview-scroll { padding-inline: 20px; }
    .workflow-toggle { padding-inline: 14px; }
    .workflow-log { padding-inline: 14px; max-height: 80px; }
    .activity-caption { display: none; }
    .window-footer { padding-inline: 12px; font-size: 10px; }
    .preview-warning, .agent-warning, .navigation-error { padding-inline: 12px; flex-wrap: wrap; }
    .preview-warning > span { flex-basis: 100%; }
  }
  @media (max-height: 300px) {
    .empty-launch { display: none; }
    .window-footer { margin-top: auto; }
  }
  @media (prefers-color-scheme: dark) { :global(:root) { --smart-accent: #70adff; } .lookup-button:not(.stop-button), .primary-action { background: #216eda; color: white; } }
  @media (prefers-reduced-motion: reduce) { .spinner { animation-duration: 2s; } .workflow-toggle svg { transition: none; } }
</style>
