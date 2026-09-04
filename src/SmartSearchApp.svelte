<script lang="ts">
  import './styles/app.css'
  import { onDestroy, onMount, tick } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { LogicalSize } from '@tauri-apps/api/dpi'
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
  let expandedWindow = false

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
  let handoffMenuOpen = $state(false)
  let handoffRun = $state<{ provider: string; runId: string } | null>(null)
  let handoffError = $state('')
  let handoffBusy = $state(false)

  let summaryBusy = $state(false)
  let summaryText = $state('')
  let summaryError = $state('')
  let summarySources = $state<SummarySource[]>([])
  let summaryModel = $state<string | null>(null)

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
  let flatVisibleHits = $derived(groups.flatMap((group) => group.files.flatMap((file) => file.hits)))
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
  let canLookup = $derived(inputValue.trim().length > 0 && !lookupBusy && !summaryBusy && !handoffBusy)
  let needsDeepAgent = $derived(/(?:所有|全部|完整|从未|不存在|有没有遗漏|精确数量|比较|分析原因|总结整)/u.test(inputValue))
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
        inputEl?.focus()
      })
    } catch { /* Browser preview has no Tauri window. */ }
  })

  onDestroy(() => {
    cancelTimer()
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

  async function expandWindow(): Promise<void> {
    if (expandedWindow) return
    try {
      await windowApi.setSize(new LogicalSize(1_020, 700))
      expandedWindow = true
      try { await windowApi.center() } catch { /* Keep current position. */ }
    } catch { /* Browser preview fills the viewport. */ }
  }

  function resetResultEdits(): void {
    selectedKeys = []
    removedKeys = []
    lastRemoved = []
    activeKey = null
    rangeAnchor = null
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
    summaryModel = null
    summaryBusy = false
    handoffRun = null
    handoffError = ''
  }

  function scheduleSearch(): void {
    cancelTimer()
    supersedeActive()
    resetResultEdits()
    clearGeneratedState()
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
    void expandWindow()
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
    if (settingsOpen || handoffMenuOpen) {
      settingsOpen = false
      handoffMenuOpen = false
      return
    }
    cancelTimer()
    supersedeActive()
    try { await invoke('hide_smart_search_window') } catch { /* Browser preview. */ }
  }

  function resetWorkflow(): void {
    workflowEntries = []
    workflowSequence = 0
    autoFollowActivity = true
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
    if (error instanceof AgentTaskError && error.status === 'timeout') message = '快速模型等待超时'
    if (error instanceof AgentTaskError && error.status === 'cancelled') message = '快速模型运行已停止'
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
        throw new Error(task === SEARCH_PLAN_TASK ? '智能理解等待超时' : '快速简答等待超时')
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
    void expandWindow()
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
      activeAgentRun = null
      if (mine !== requestSequence) return
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
      activeAgentRun = null
      if (mine !== requestSequence) return
      await ensurePreview(question, mine)
      if (mine !== requestSequence) return
      phase = 'preview_only'
      plannerWarning = `${t('smartSearch.plannerFallback')} ${readableError(error)}`
      appendActivity('plan', 'warning', plannerWarning)
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
    if (!question || store.loading) return
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
    const response = await store.run(question, {
      deep: true,
      limit: settings.smartLookup.results.limit,
      timeoutMs: settings.smartLookup.results.deepTimeoutMs,
    })
    if (!response) return
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
      const preference = summaryPreference(summaryAgent)
      const selector = selectorForPreference(preference)
      const invocationId = crypto.randomUUID()
      const startSummary = () => smartSearchApi.startSummary(
        lookupRunId,
        selectedIds,
        settings.smartLookup.summary.sourceLimit,
        settings.smartLookup.summary.charLimit,
        settings.smartLookup.summary.style,
        summaryAgent.id,
        selector,
        invocationId,
      )
      // The host/provider deduplicates the same invocation when an IPC response is lost.
      const startTimeout = Math.min(10_000, settings.smartLookup.summary.timeoutMs)
      const start = supportsIdempotentInvocation(summaryAgent)
        ? await retryIdempotentStart(startSummary, startTimeout)
        : await withTimeout(startSummary(), startTimeout, 'Agent 启动等待超时')
      if (mine !== requestSequence) {
        void cancelAgentTask(summaryAgent.id, SEARCH_SUMMARY_TASK, start.runId).catch(() => {})
        return
      }
      summarySources = start.sources
      if (start.staleCount > 0) {
        appendActivity('summary', 'warning', t('smartSearch.summaryStale', { n: start.staleCount }))
      }
      activeAgentRun = { provider: summaryAgent.id, task: SEARCH_SUMMARY_TASK, runId: start.runId }
      summaryModel = start.resolvedModel ?? resolvedModelHint(preference, summaryAgent.harness)
      const result = await pollWithTimeout(
        summaryAgent.id,
        SEARCH_SUMMARY_TASK,
        start.runId,
        settings.smartLookup.summary.timeoutMs,
        mine,
      )
      activeAgentRun = null
      if (mine !== requestSequence) return
      summaryText = validateSummaryOutput(
        result.content,
        start.sources,
        settings.smartLookup.summary.style,
      ).content
      appendActivity('summary', 'success', t('smartSearch.activitySummaryDone'), { runId: start.runId })
    } catch (error) {
      activeAgentRun = null
      if (mine === requestSequence) {
        summaryError = readableError(error)
        appendActivity('summary', 'warning', `${t('smartSearch.summaryUnavailable')} ${summaryError}`)
      }
    } finally {
      if (mine === requestSequence) summaryBusy = false
    }
  }

  function handoffHits(): SearchHit[] {
    if (!settings.smartLookup.handoff.includeSelectedRefs) return []
    if (selectedKeys.length) {
      return store.hits.filter((hit) => selectedSet.has(hitKey(hit)))
    }
    return activeHit ? [activeHit] : []
  }

  async function handoff(providerId?: string): Promise<void> {
    handoffMenuOpen = false
    const packet = buildHandoffPacket(inputValue, resolvedPlan, handoffHits())
    const prompt = buildHandoffPrompt(packet)
    const configured = providerId
      ?? (settings.smartLookup.handoff.defaultProvider === 'ask'
        ? ''
        : settings.smartLookup.handoff.defaultProvider)
    const provider = handoffAgents.find((agent) => agent.id === configured)
    if (!provider) {
      await copyText(prompt)
      handoffError = t('smartSearch.handoffCopied')
      appendActivity('handoff', 'success', handoffError)
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
      handoffError = readableError(error)
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

  async function copyText(value: string): Promise<void> {
    try {
      const { writeText } = await import('@tauri-apps/plugin-clipboard-manager')
      await writeText(value)
    } catch {
      await navigator.clipboard.writeText(value)
    }
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
    summaryText = ''
    summaryError = ''
  }

  function removeKeys(keys: string[]): void {
    const candidates = keys.filter((key) => store.hits.some((hit) => hitKey(hit) === key))
    if (!candidates.length) return
    removedKeys = addRemovedKeys(removedKeys, candidates)
    lastRemoved = candidates
    selectedKeys = []
    if (activeKey && candidates.includes(activeKey)) activeKey = null
  }

  function undoRemove(): void {
    removedKeys = restoreRemovedKeys(removedKeys, lastRemoved)
    selectedKeys = [...lastRemoved]
    activeKey = lastRemoved[0] ?? null
    lastRemoved = []
  }

  function onResultsKeydown(event: KeyboardEvent, hit: SearchHit): void {
    if (isImeKey(event) || event.target !== event.currentTarget) return
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      selectedKeys = flatVisibleHits.map(hitKey)
      activeKey = hitKey(hit)
      rangeAnchor = activeKey
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
          chips.push(`${label}:${value}`)
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
    summaryText = ''
    void saveSettings()
  }

  function changeSummaryProvider(event: Event): void {
    settings.smartLookup.summary.provider = (event.currentTarget as HTMLSelectElement).value
    summaryText = ''
    void saveSettings()
  }

  function changeSummaryModel(event: Event): void {
    if (!summaryAgent) return
    settings.smartLookup.summary.modelByProvider[summaryAgent.id]
      = (event.currentTarget as HTMLSelectElement).value as ModelPreference
    summaryText = ''
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
    summaryText = ''
    void saveSettings()
  }

  function setBoolean(path: 'planner' | 'deep' | 'summary' | 'refs', checked: boolean): void {
    if (path === 'planner') settings.smartLookup.planner.enabled = checked
    if (path === 'deep') settings.smartLookup.results.autoDeepOnZero = checked
    if (path === 'summary') settings.smartLookup.summary.enabled = checked
    if (path === 'refs') settings.smartLookup.handoff.includeSelectedRefs = checked
    void saveSettings()
  }
</script>

<svelte:window onkeydown={(event) => {
  if (event.key !== 'Escape' || isImeKey(event)) return
  const target = event.target as HTMLElement | null
  if (target === inputEl) return
  event.preventDefault()
  void hideWindow()
}} />

<main class:expanded={store.route !== null || phase !== 'idle'}>
  <section class="command-bar" aria-label={t('smartSearch.windowTitle')}>
    <span class="search-icon" aria-hidden="true">⌕</span>
    <textarea
      bind:this={inputEl}
      bind:value={inputValue}
      rows="1"
      class="query-input"
      placeholder={t('smartSearch.placeholder')}
      oninput={onInput}
      onkeydown={onInputKeydown}
      oncompositionstart={() => { composing = true; inputIme.start(); cancelTimer() }}
      oncompositionend={onCompositionEnd}
      onblur={() => inputIme.reset()}
      aria-label={t('smartSearch.placeholder')}
    ></textarea>
    <button
      class="settings-button"
      aria-label={t('smartSearch.settings')}
      aria-expanded={settingsOpen}
      onclick={() => { settingsOpen = !settingsOpen; handoffMenuOpen = false }}
    >⚙</button>
    <button class="lookup-button" disabled={!canLookup} onclick={() => void runSmartLookup()}>
      {lookupBusy ? '…' : t('smartSearch.lookup')}
      <kbd>↵</kbd>
    </button>
  </section>
  <div class="input-hint">
    <span>{t('smartSearch.inputHint')}</span>
    {#if plannerAgent}
      <span>{plannerAgent.harness?.harness} · {plannerModel ?? resolvedModelHint(plannerPreference(), plannerAgent.harness) ?? t('smartSearch.modelFast')}</span>
    {:else if ready}
      <span>{t('smartSearch.localAvailable')}</span>
    {/if}
  </div>

  {#if settingsOpen}
    <div class="menu-panel lookup-settings" role="dialog" aria-label={t('smartSearch.settings')}>
      <label class="menu-row setting-row">
        <span><strong>{t('smartSearch.smartUnderstanding')}</strong><small>{t('smartSearch.smartUnderstandingHint')}</small></span>
        <input type="checkbox" checked={settings.smartLookup.planner.enabled} onchange={(event) => setBoolean('planner', event.currentTarget.checked)} />
      </label>
      <label class="menu-row setting-row">
        <span>{t('smartSearch.plannerProvider')}</span>
        <select value={settings.smartLookup.planner.provider} onchange={changePlannerProvider}>
          <option value="auto">{t('smartSearch.auto')}</option>
          {#each plannerAgents as agent (agent.id)}<option value={agent.id}>{agent.harness?.harness || agent.name}</option>{/each}
        </select>
      </label>
      {#if plannerAgent}
        <label class="menu-row setting-row">
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
      <label class="menu-row setting-row">
        <span>{t('smartSearch.resultLimit')}</span>
        <select value={settings.smartLookup.results.limit} onchange={(event) => {
          settings.smartLookup.results.limit = Number(event.currentTarget.value) as 20 | 50 | 100; void saveSettings()
        }}><option value="20">20</option><option value="50">50</option><option value="100">100</option></select>
      </label>
      <label class="menu-row setting-row">
        <span>{t('smartSearch.groupBy')}</span>
        <select value={settings.smartLookup.results.groupBy} onchange={(event) => {
          settings.smartLookup.results.groupBy = event.currentTarget.value as 'auto' | 'source' | 'date'; void saveSettings()
        }}>
          <option value="auto">{t('smartSearch.auto')}</option>
          <option value="source">{t('smartSearch.groupSource')}</option>
          <option value="date">{t('smartSearch.groupDate')}</option>
        </select>
      </label>
      <label class="menu-row setting-row">
        <span>{t('smartSearch.autoDeep')}</span>
        <input type="checkbox" checked={settings.smartLookup.results.autoDeepOnZero} onchange={(event) => setBoolean('deep', event.currentTarget.checked)} />
      </label>
      <label class="menu-row setting-row">
        <span>{t('smartSearch.quickSummary')}</span>
        <input type="checkbox" checked={settings.smartLookup.summary.enabled} onchange={(event) => setBoolean('summary', event.currentTarget.checked)} />
      </label>
      <label class="menu-row setting-row">
        <span>{t('smartSearch.summaryStyle')}</span>
        <select value={settings.smartLookup.summary.style} onchange={changeSummaryStyle}>
          <option value="bullets">{t('smartSearch.summaryBullets')}</option>
          <option value="sentence">{t('smartSearch.summarySentence')}</option>
        </select>
      </label>
      <label class="menu-row setting-row">
        <span>{t('smartSearch.includeRefs')}</span>
        <input type="checkbox" checked={settings.smartLookup.handoff.includeSelectedRefs} onchange={(event) => setBoolean('refs', event.currentTarget.checked)} />
      </label>
      <details class="advanced-settings">
        <summary>{t('smartSearch.advancedSettings')}</summary>
        <label class="menu-row setting-row">
          <span>{t('smartSearch.plannerTimeout')}</span>
          <input type="number" min="10" max="60" step="1" value={settings.smartLookup.planner.timeoutMs / 1_000}
            onchange={(event) => saveBoundedSeconds(event, settings.smartLookup.planner.timeoutMs, 10, 60, (value) => { settings.smartLookup.planner.timeoutMs = value })} />
        </label>
        <label class="menu-row setting-row">
          <span>{t('smartSearch.deepTimeout')}</span>
          <input type="number" min="1" max="5" step="1" value={settings.smartLookup.results.deepTimeoutMs / 1_000}
            onchange={(event) => saveBoundedSeconds(event, settings.smartLookup.results.deepTimeoutMs, 1, 5, (value) => { settings.smartLookup.results.deepTimeoutMs = value })} />
        </label>
        <label class="menu-row setting-row">
          <span>{t('smartSearch.summaryProvider')}</span>
          <select value={settings.smartLookup.summary.provider} onchange={changeSummaryProvider}>
            <option value="same_as_planner">{t('smartSearch.sameAsPlanner')}</option>
            <option value="auto">{t('smartSearch.auto')}</option>
            {#each summaryAgents as agent (agent.id)}<option value={agent.id}>{agent.harness?.harness || agent.name}</option>{/each}
          </select>
        </label>
        {#if summaryAgent}
          <label class="menu-row setting-row">
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
        <label class="menu-row setting-row">
          <span>{t('smartSearch.summarySources')}</span>
          <input type="number" min="1" max="6" step="1" value={settings.smartLookup.summary.sourceLimit}
            onchange={(event) => saveBoundedInteger(event, settings.smartLookup.summary.sourceLimit, 1, 6, (value) => { settings.smartLookup.summary.sourceLimit = value })} />
        </label>
        <label class="menu-row setting-row">
          <span>{t('smartSearch.summaryChars')}</span>
          <input type="number" min="1000" max="6000" step="500" value={settings.smartLookup.summary.charLimit}
            onchange={(event) => saveBoundedInteger(event, settings.smartLookup.summary.charLimit, 1000, 6000, (value) => { settings.smartLookup.summary.charLimit = value })} />
        </label>
        <label class="menu-row setting-row">
          <span>{t('smartSearch.summaryTimeout')}</span>
          <input type="number" min="5" max="30" step="1" value={settings.smartLookup.summary.timeoutMs / 1_000}
            onchange={(event) => saveBoundedSeconds(event, settings.smartLookup.summary.timeoutMs, 5, 30, (value) => { settings.smartLookup.summary.timeoutMs = value })} />
        </label>
        <label class="menu-row setting-row">
          <span>{t('smartSearch.handoffProvider')}</span>
          <select value={settings.smartLookup.handoff.defaultProvider} onchange={(event) => {
            settings.smartLookup.handoff.defaultProvider = event.currentTarget.value; void saveSettings()
          }}>
            <option value="ask">{t('smartSearch.askEveryTime')}</option>
            {#each handoffAgents as agent (agent.id)}<option value={agent.id}>{agent.harness?.harness || agent.name}</option>{/each}
          </select>
        </label>
      </details>
    </div>
  {/if}

  {#if resolvedPlan}
    <div class="plan-summary" aria-label={t('smartSearch.intelligentResults')}>
      {#if resolvedPlan.time?.after || resolvedPlan.time?.before}
        <span>◷ {resolvedPlan.time.after ?? '…'} – {resolvedPlan.time.before ?? '…'}</span>
      {/if}
      {#each planFilterChips() as filter}<span>{filter}</span>{/each}
      {#each resolvedTerms as term}<span>{term}</span>{/each}
      {#if resolvedPlan.sort !== 'relevance'}<span>{resolvedPlan.sort}</span>{/if}
      {#each planWarnings() as warning}<span class="warning-chip">! {warning}</span>{/each}
    </div>
  {:else if plannerWarning}
    <div class="preview-warning">{plannerWarning}</div>
  {/if}

  {#if !ready}
    <div class="launch-state">…</div>
  {:else if agentsError}
    <div class="agent-warning">{t('smartSearch.localAvailable')} · {agentsError}</div>
  {/if}
  {#if modelWarning}<div class="agent-warning">{modelWarning}</div>{/if}

  {#if navigationError}
    <div class="navigation-error" role="alert">
      <span>{navigationError}</span>
      <button aria-label={t('common.close')} onclick={() => { navigationError = '' }}>×</button>
    </div>
  {/if}

  {#if store.route === null && phase === 'idle'}
    <div class="empty-launch">
      <div class="empty-mark">⌘</div>
      <p>{t('smartSearch.emptyPrompt')}</p>
    </div>
  {:else}
    <section class="workspace">
      <aside class="results-pane" aria-label={t('smartSearch.results')}>
        <header class="pane-header">
          <strong>{authoritativeResults ? t('smartSearch.intelligentResults') : t('smartSearch.quickPreview')}</strong>
          <span>{store.loading ? '…' : t('smartSearch.returnedCount', { n: store.hits.length })}</span>
          <select bind:value={sourceFilter} aria-label={t('smartSearch.sourceAll')}>
            <option value="all">{t('smartSearch.sourceAll')}</option>
            <option value="human">{t('search.group.human')}</option>
            <option value="source">{t('search.group.source')}</option>
            <option value="derived">{t('smartSearch.sourceDerived')}</option>
            <option value="unlabeled">{t('search.group.unlabeled')}</option>
          </select>
        </header>

        {#if store.truncated}
          <div class="partial-note">{t('smartSearch.partialResults', { n: store.hits.length })}</div>
        {/if}
        {#if selectedKeys.length > 0}
          <div class="selection-bar">
            <span>{t('smartSearch.selectedCount', { n: selectedKeys.length })}</span>
            <button onclick={() => removeKeys(selectedKeys)}>{t('smartSearch.removeSelected')}</button>
          </div>
        {:else if removedKeys.length > 0}
          <div class="selection-bar removed-note">
            <span>{t('smartSearch.removedCount', { n: removedKeys.length })}</span>
            {#if lastRemoved.length > 0}<button onclick={undoRemove}>{t('smartSearch.undo')}</button>{/if}
          </div>
        {/if}

        <div class="results-scroll" role="listbox" aria-multiselectable="true">
          {#if store.error}
            <p class="state error"><strong>{t('smartSearch.searchError')}:</strong> {readableError(store.error)}</p>
          {:else if store.loading && store.hits.length === 0}
            <div class="state"><span class="spinner"></span></div>
          {:else if store.route !== null && visibleHits.length === 0}
            <div class="state">
              <p>{removedKeys.length ? t('smartSearch.noContext') : t('search.noResults')}</p>
              {#if store.deepAvailable || phase === 'no_results'}
                <button class="deep-button" onclick={() => void runDeep()}>{t('smartSearch.expandSearch')}</button>
              {/if}
              {#if removedKeys.length}<small>{t('smartSearch.notDeleted')}</small>{/if}
            </div>
          {:else if useDateGrouping()}
            {#each dateGroups() as dateGroup (dateGroup.key)}
              <section class="result-group">
                <h2>{dateGroup.key} <span>{dateGroup.hits.length}</span></h2>
                {#each dateGroup.hits as hit (hitKey(hit))}
                  {@const key = hitKey(hit)}
                  {@const line = displayLine(hit)}
                  <div class="result-row" class:selected={selectedSet.has(key)} class:active={activeKey === key}
                    role="option" aria-selected={selectedSet.has(key)} tabindex="0"
                    onclick={(event) => selectHit(event, hit)} ondblclick={() => void openHit(hit)}
                    onkeydown={(event) => onResultsKeydown(event, hit)}>
                    <span class="check" aria-hidden="true">{selectedSet.has(key) ? '✓' : ''}</span>
                    <div class="result-copy">
                      <div class="result-title"><strong>{basename(hit.path)}</strong>{#if hit.breadcrumb}<span>{hit.breadcrumb}</span>{/if}<small>:{hit.line + line.line}</small></div>
                      <p>{#each highlightParts(line.text || hit.text.slice(0, 240), highlightTerms) as part}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</p>
                      <div class="result-meta">{#each relevanceReasons(hit).slice(0, 2) as reason}<span>{relevanceLabel(reason)}</span>{/each}{#if hit.humanVerified}<span>✓ {t('search.humanVerified')}</span>{/if}</div>
                    </div>
                    <button class="remove-one" aria-label={t('smartSearch.removeSelected')} onclick={(event) => { event.stopPropagation(); removeKeys([key]) }}>×</button>
                  </div>
                {/each}
              </section>
            {/each}
          {:else}
            {#each groups as group (groupKey(group))}
              <section class="result-group">
                <h2>{groupLabel(group)} <span>{group.hitCount}</span></h2>
                {#each group.files as file (`${groupKey(group)}:${file.path}`)}
                  {#each file.hits as hit (hitKey(hit))}
                    {@const key = hitKey(hit)}
                    {@const line = displayLine(hit)}
                    <div class="result-row" class:selected={selectedSet.has(key)} class:active={activeKey === key}
                      role="option" aria-selected={selectedSet.has(key)} tabindex="0"
                      onclick={(event) => selectHit(event, hit)} ondblclick={() => void openHit(hit)}
                      onkeydown={(event) => onResultsKeydown(event, hit)}>
                      <span class="check" aria-hidden="true">{selectedSet.has(key) ? '✓' : ''}</span>
                      <div class="result-copy">
                        <div class="result-title"><strong>{basename(file.path)}</strong>{#if hit.breadcrumb}<span>{hit.breadcrumb}</span>{/if}<small>:{hit.line + line.line}</small></div>
                        <p>{#each highlightParts(line.text || hit.text.slice(0, 240), highlightTerms) as part}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</p>
                        <div class="result-meta">{#each relevanceReasons(hit).slice(0, 2) as reason}<span>{relevanceLabel(reason)}</span>{/each}{#if hit.humanVerified}<span>✓ {t('search.humanVerified')}</span>{/if}</div>
                      </div>
                      <button class="remove-one" aria-label={t('smartSearch.removeSelected')} onclick={(event) => { event.stopPropagation(); removeKeys([key]) }}>×</button>
                    </div>
                  {/each}
                {/each}
              </section>
            {/each}
          {/if}
        </div>
      </aside>

      <article class="preview-pane" aria-label={t('smartSearch.currentResult')}>
        <header class="pane-header preview-header">
          <strong>{t('smartSearch.currentResult')}</strong>
          {#if activeHit}<span>{activeHit.level}</span>{/if}
        </header>

        {#if workflowEntries.length > 0}
          <section class="workflow-panel" aria-label={t('smartSearch.activityTitle')} aria-busy={lookupBusy || summaryBusy || handoffBusy}>
            <header><span class:spinner={lookupBusy || summaryBusy || handoffBusy}></span><strong>{t('smartSearch.activityTitle')}</strong></header>
            <div class="workflow-log" bind:this={activityLogEl} role="log" aria-live="polite" aria-relevant="additions text" onscroll={onActivityScroll}>
              {#each workflowEntries as entry (entry.id)}
                <div class="workflow-row" data-level={entry.level}>
                  <span aria-hidden="true">{entry.level === 'success' ? '✓' : entry.level === 'warning' ? '!' : entry.level === 'error' ? '×' : '›'}</span>
                  <span>{entry.message}</span>
                  {#if entry.steps !== undefined}<small>#{entry.steps}</small>{/if}
                </div>
              {/each}
            </div>
          </section>
        {/if}

        {#if activeHit}
          {@const line = displayLine(activeHit)}
          <div class="preview-card">
            <small>{simpleRef(activeHit)}</small>
            <h1>{activeHit.breadcrumb || basename(activeHit.path)}</h1>
            {#if activeHit.level !== 'line'}<div class="block-note">{t('smartSearch.longBlockPreview', { type: activeHit.level })}</div>{/if}
            <p>{line.text || activeHit.text}</p>
            <div class="card-actions">
              <button class="primary-action" onclick={() => void openHit(activeHit)}>{t('smartSearch.openEditor')} ↗</button>
              <button onclick={() => void copyText(simpleRef(activeHit))}>{t('smartSearch.copyRef')}</button>
              <button onclick={() => void copyText(markdownRef(activeHit))}>{t('smartSearch.copyMarkdown')}</button>
            </div>
          </div>
        {:else}
          <div class="state"><p>{t('smartSearch.emptyPrompt')}</p></div>
        {/if}

        <section class="next-actions">
          <div class="action-copy">
            <strong>{t('smartSearch.quickSummary')}</strong>
            <small>{t('smartSearch.summaryLimitation')}</small>
          </div>
          <button disabled={!lookupRunId || !summaryAgent || !hasSummaryCandidates || summaryBusy} onclick={() => void generateSummary()}>
            {summaryBusy ? '…' : t('smartSearch.generateSummary')}
          </button>
          {#if !lookupRunId || !summaryAgent || !hasSummaryCandidates}
            <p class="action-hint">{!lookupRunId
              ? t('smartSearch.summaryNeedsLookup')
              : !summaryAgent
                ? t('smartSearch.summaryAgentUnavailable')
                : t('smartSearch.summaryNeedsLine')}</p>
          {/if}
          {#if summaryError}<p class="summary-error" role="alert">{summaryError}</p>{/if}
          {#if summaryText && summarySources.length}
            <div class="summary-card">
              <header><strong>{t('smartSearch.basedOnMatches')}</strong>{#if summaryModel}<small>{summaryModel}</small>{/if}</header>
              <p>{summaryText}</p>
              <div class="summary-sources">
                {#each summarySources as source}
                  <button onclick={() => {
                    const hit = store.hits.find((candidate) => candidate.path === source.path && candidate.line === source.line)
                    if (hit) void openHit(hit, true)
                  }}>[{source.id}] {source.path}:{source.line}</button>
                {/each}
              </div>
              <button onclick={() => void copyText(summaryText)}>{t('smartSearch.copySummary')}</button>
            </div>
          {/if}
        </section>

        <section class="next-actions handoff-section">
          <div class="action-copy">
            <strong>{t('smartSearch.handoff')}</strong>
            <small>{needsDeepAgent ? t('smartSearch.deepAgentRecommended') : t('smartSearch.handoffHint')}</small>
          </div>
          <button class="handoff-button" disabled={handoffBusy} onclick={() => {
            const configured = settings.smartLookup.handoff.defaultProvider
            if (configured === 'ask') handoffMenuOpen = !handoffMenuOpen
            else void handoff(configured)
          }}>{handoffBusy ? '…' : t('smartSearch.handoff')}</button>
          {#if handoffMenuOpen}
            <div class="menu-panel handoff-menu">
              {#each handoffAgents as agent (agent.id)}
                <button class="menu-row" onclick={() => void handoff(agent.id)}>{agent.harness?.harness || agent.name}</button>
              {/each}
              <button class="menu-row" onclick={() => void handoff()}>{t('smartSearch.copyHandoff')}</button>
            </div>
          {/if}
          {#if handoffRun}
            <button class="run-link" onclick={openHandoffRun}>{t('smartSearch.openAgentRun')} · {handoffRun.runId}</button>
          {/if}
          {#if handoffError}<p class="action-hint">{handoffError}</p>{/if}
        </section>
      </article>
    </section>
  {/if}
</main>

<style>
  :global(:root) {
    color-scheme: light dark;
    --smart-border: color-mix(in srgb, CanvasText 14%, transparent);
    --smart-muted: color-mix(in srgb, CanvasText 58%, transparent);
    --smart-soft: color-mix(in srgb, CanvasText 5%, transparent);
  }
  :global(body) { background: Canvas; color: CanvasText; }
  button, select, textarea, input { font: inherit; }
  button { color: inherit; }
  main { position: relative; height: 100vh; min-height: 150px; display: flex; flex-direction: column; background: color-mix(in srgb, Canvas 96%, CanvasText 4%); overflow: hidden; }
  .command-bar { display: grid; grid-template-columns: 24px minmax(120px, 1fr) auto auto; align-items: center; gap: 8px; margin: 12px 12px 0; min-height: 48px; padding: 5px 7px 5px 12px; box-sizing: border-box; border: 1px solid color-mix(in srgb, AccentColor 45%, var(--smart-border)); border-radius: 13px; background: Canvas; box-shadow: 0 5px 20px rgba(0,0,0,.09), 0 0 0 3px color-mix(in srgb, AccentColor 10%, transparent); }
  .search-icon { font-size: 25px; color: var(--smart-muted); transform: rotate(-18deg); }
  .query-input { resize: none; border: 0; outline: 0; background: transparent; color: CanvasText; min-height: 24px; max-height: 68px; padding: 4px 0; line-height: 1.45; overflow-y: auto; }
  .query-input::placeholder { color: color-mix(in srgb, CanvasText 42%, transparent); }
  .lookup-button, .primary-action { min-height: 32px; border: 0; border-radius: 8px; background: #0a63ff; color: #fff; padding: 0 12px; font-weight: 650; }
  .lookup-button kbd { margin-left: 7px; opacity: .75; font: inherit; }
  button:disabled { opacity: .4; cursor: default; }
  .settings-button { width: 32px; height: 32px; border: 1px solid var(--smart-border); border-radius: 8px; background: var(--smart-soft); }
  .input-hint { display: flex; justify-content: space-between; gap: 16px; margin: 6px 18px 8px 48px; font-size: 11px; color: var(--smart-muted); }
  .lookup-settings { position: absolute; z-index: 20; top: 68px; right: 84px; width: 360px; max-height: min(540px, calc(100vh - 82px)); overflow: auto; padding: 6px; }
  .setting-row { display: flex; align-items: center; justify-content: space-between; gap: 18px; width: 100%; min-height: 42px; box-sizing: border-box; }
  .setting-row > span:first-child { display: grid; gap: 2px; }
  .setting-row small { color: var(--smart-muted); font-size: 11px; }
  .setting-row select { max-width: 172px; }
  .setting-row input[type='number'] { width: 76px; }
  .advanced-settings { border-top: 1px solid var(--smart-border); margin-top: 5px; padding-top: 5px; }
  .advanced-settings summary { padding: 7px 9px; color: var(--smart-muted); cursor: default; font-size: 11px; }
  .plan-summary { display: flex; gap: 6px; flex-wrap: wrap; padding: 0 14px 8px; }
  .plan-summary span { padding: 3px 7px; border: 1px solid var(--smart-border); border-radius: 999px; background: Canvas; color: var(--smart-muted); font-size: 11px; }
  .plan-summary .warning-chip { border-color: color-mix(in srgb, #d88700 50%, var(--smart-border)); color: #a76400; }
  .preview-warning, .agent-warning, .partial-note { margin: 0 14px 8px; padding: 7px 10px; border-radius: 7px; background: color-mix(in srgb, #d88700 12%, transparent); color: color-mix(in srgb, #9c5d00 82%, CanvasText); font-size: 12px; }
  .navigation-error { display: flex; justify-content: space-between; margin: 0 14px 8px; padding: 7px 10px; border-radius: 7px; background: color-mix(in srgb, #cc3030 12%, transparent); font-size: 12px; }
  .navigation-error button { border: 0; background: transparent; }
  .launch-state, .empty-launch { flex: 1; display: grid; place-content: center; justify-items: center; color: var(--smart-muted); }
  .empty-mark { font-size: 32px; opacity: .35; }
  .workspace { min-height: 0; flex: 1; display: grid; grid-template-columns: minmax(340px, 44%) minmax(400px, 56%); margin: 0 12px 12px; border: 1px solid var(--smart-border); border-radius: 12px; overflow: hidden; background: Canvas; }
  .results-pane, .preview-pane { min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .results-pane { border-right: 1px solid var(--smart-border); }
  .preview-pane { overflow-y: auto; }
  .pane-header { min-height: 42px; display: flex; align-items: center; gap: 9px; padding: 0 12px; border-bottom: 1px solid var(--smart-border); background: var(--smart-soft); }
  .pane-header strong { margin-right: auto; font-size: 12px; }
  .pane-header > span { font-size: 11px; color: var(--smart-muted); }
  .pane-header select { height: 27px; max-width: 130px; border: 1px solid var(--smart-border); border-radius: 6px; background: Canvas; color: CanvasText; }
  .selection-bar { display: flex; justify-content: space-between; align-items: center; min-height: 34px; padding: 0 10px; border-bottom: 1px solid var(--smart-border); background: color-mix(in srgb, AccentColor 9%, Canvas); font-size: 11px; }
  .selection-bar button, .deep-button { border: 0; background: transparent; color: AccentColor; font-weight: 600; }
  .results-scroll { flex: 1; overflow-y: auto; padding: 6px; }
  .result-group h2 { margin: 10px 8px 4px; color: var(--smart-muted); font-size: 10px; font-weight: 650; text-transform: uppercase; letter-spacing: .04em; }
  .result-group h2 span { float: right; }
  .result-row { display: grid; grid-template-columns: 18px minmax(0,1fr) 22px; gap: 6px; padding: 8px 6px; border-radius: 8px; outline: 0; cursor: default; }
  .result-row:hover, .result-row.active { background: var(--smart-soft); }
  .result-row.selected { background: color-mix(in srgb, AccentColor 15%, Canvas); }
  .check { color: AccentColor; font-weight: 700; }
  .result-copy { min-width: 0; }
  .result-title { display: flex; align-items: baseline; gap: 6px; min-width: 0; }
  .result-title strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
  .result-title span, .result-title small { color: var(--smart-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 10px; }
  .result-copy p { margin: 4px 0; overflow: hidden; display: -webkit-box; line-clamp: 2; -webkit-line-clamp: 2; -webkit-box-orient: vertical; color: color-mix(in srgb, CanvasText 82%, transparent); font-size: 12px; line-height: 1.45; }
  mark { border-radius: 2px; background: color-mix(in srgb, #ffd24b 62%, transparent); color: inherit; }
  .result-meta { display: flex; gap: 5px; flex-wrap: wrap; }
  .result-meta span { color: var(--smart-muted); font-size: 9px; }
  .remove-one { border: 0; background: transparent; opacity: 0; }
  .result-row:hover .remove-one, .remove-one:focus-visible { opacity: .55; }
  .state { display: grid; justify-items: center; gap: 8px; padding: 32px 16px; text-align: center; color: var(--smart-muted); font-size: 12px; }
  .state.error, .summary-error { color: #c93434; }
  .spinner { display: inline-block; width: 12px; height: 12px; border: 2px solid color-mix(in srgb, AccentColor 22%, transparent); border-top-color: AccentColor; border-radius: 50%; animation: spin .75s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .workflow-panel { margin: 12px 12px 0; border: 1px solid var(--smart-border); border-radius: 9px; overflow: hidden; background: var(--smart-soft); }
  .workflow-panel > header { display: flex; gap: 7px; align-items: center; padding: 7px 9px; font-size: 11px; }
  .workflow-log { max-height: 112px; overflow-y: auto; padding: 0 8px 7px; }
  .workflow-row { display: grid; grid-template-columns: 14px minmax(0,1fr) auto; gap: 5px; padding: 2px 0; color: var(--smart-muted); font-size: 10px; }
  .workflow-row[data-level='success'] > span:first-child { color: #198754; }
  .workflow-row[data-level='warning'] > span:first-child { color: #c47a00; }
  .preview-card { margin: 12px; padding: 15px; border: 1px solid var(--smart-border); border-radius: 10px; background: Canvas; }
  .preview-card > small { color: var(--smart-muted); font-size: 10px; }
  .preview-card h1 { margin: 7px 0 9px; font-size: 17px; }
  .preview-card > p { margin: 0; max-height: 180px; overflow: auto; white-space: pre-wrap; color: color-mix(in srgb, CanvasText 86%, transparent); font-size: 12px; line-height: 1.55; }
  .block-note { margin-bottom: 7px; color: #9c6500; font-size: 10px; }
  .card-actions { display: flex; gap: 7px; flex-wrap: wrap; margin-top: 13px; }
  .card-actions button, .next-actions > button, .summary-card > button { min-height: 29px; padding: 0 9px; border: 1px solid var(--smart-border); border-radius: 7px; background: var(--smart-soft); font-size: 11px; }
  .card-actions .primary-action { border: 0; background: #0a63ff; }
  .next-actions { position: relative; display: grid; grid-template-columns: minmax(0,1fr) auto; gap: 5px 12px; margin: 0 12px 12px; padding: 12px; border: 1px solid var(--smart-border); border-radius: 10px; }
  .action-copy { display: grid; gap: 3px; }
  .action-copy strong { font-size: 12px; }
  .action-copy small, .action-hint { color: var(--smart-muted); font-size: 10px; }
  .action-hint, .summary-error { grid-column: 1 / -1; margin: 2px 0 0; font-size: 10px; }
  .summary-card { grid-column: 1 / -1; margin-top: 6px; padding-top: 10px; border-top: 1px solid var(--smart-border); }
  .summary-card header { display: flex; justify-content: space-between; color: var(--smart-muted); font-size: 10px; }
  .summary-card p { white-space: pre-wrap; font-size: 12px; line-height: 1.55; }
  .summary-sources { display: flex; gap: 5px; flex-wrap: wrap; margin-bottom: 8px; }
  .summary-sources button, .run-link { border: 0; background: transparent; padding: 0; color: AccentColor; font-size: 10px; text-align: left; }
  .handoff-menu { position: absolute; z-index: 10; top: 48px; right: 12px; min-width: 200px; padding: 5px; }
  .handoff-menu .menu-row { width: 100%; border: 0; padding: 7px 9px; text-align: left; }
  .run-link { grid-column: 1 / -1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  @media (max-width: 760px) {
    .workspace { grid-template-columns: 1fr; overflow-y: auto; }
    .results-pane { min-height: 320px; border-right: 0; border-bottom: 1px solid var(--smart-border); }
    .lookup-settings { left: 16px; right: 16px; width: auto; }
  }
</style>
