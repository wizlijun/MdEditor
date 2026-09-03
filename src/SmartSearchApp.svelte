<script lang="ts">
  import './styles/app.css'
  import { onDestroy, onMount, tick } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { LogicalSize } from '@tauri-apps/api/dpi'
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { loadLocale, t, watchLocaleChanges } from './lib/i18n/store.svelte'
  import { rememberedProvider, rememberProvider, type AgentOption } from './lib/agent-picker/types'
  import type { SearchHit, SmartRelevanceReason, SmartSearchResponse } from './lib/search/api'
  import { groupHits, type HitGroup } from './lib/search/grouping'
  import {
    decideTrigger,
    DEEP_AFTER_MS,
    DEEP_TIMEOUT_MS,
  } from './lib/search/input-trigger'
  import { highlightParts, parseHighlightTerms, previewLine } from './lib/search/preview'
  import { createImeGuard, isImeKey } from './lib/ime'
  import { SmartSearchStore } from './lib/smart-search/store.svelte'
  import {
    buildSearchAnswerPrompt,
    hitKey,
    parseAnswerSegments,
    selectContextSources,
    sourceForCitation,
    type AnswerContext,
  } from './lib/smart-search/session'
  import {
    loadSearchAgentOptions,
    pollSearchAgentTask,
    startSearchAgentTask,
  } from './lib/smart-search/agent'
  import { smartSearchApi, type ArchiveReceipt } from './lib/smart-search/api'
  import { addRemovedKeys, chooseResultKeys, restoreRemovedKeys } from './lib/smart-search/selection'

  type SourceFilter = 'all' | SearchHit['origin']
  type AnswerPhase = 'idle' | 'preparing' | 'running' | 'done' | 'error'

  const store = new SmartSearchStore()
  const windowApi = getCurrentWindow()
  const inputIme = createImeGuard()

  let ready = $state(false)
  let inputValue = $state('')
  let composing = $state(false)
  let inputEl = $state<HTMLTextAreaElement>()
  let debounceTimer: ReturnType<typeof setTimeout> | undefined
  let deepTimer: ReturnType<typeof setTimeout> | undefined
  let expandedWindow = false

  let sourceFilter = $state<SourceFilter>('all')
  let selectedKeys = $state<string[]>([])
  let removedKeys = $state<string[]>([])
  let lastRemoved = $state<string[]>([])
  let activeKey = $state<string | null>(null)
  let rangeAnchor = $state<string | null>(null)

  let agents = $state<AgentOption[]>([])
  let selectedProvider = $state('')
  let agentsError = $state('')

  let answerPhase = $state<AnswerPhase>('idle')
  let answerError = $state('')
  let answer = $state('')
  let answerId = $state('')
  let answerProvider = $state('')
  let answerModel = $state<string | null>(null)
  let answerRunId = $state('')
  let answerContext = $state<AnswerContext | null>(null)
  let progressText = $state('')
  let answerAttempt = 0
  let navigationError = $state('')

  let feedback = $state<'helpful' | 'unhelpful' | null>(null)
  let feedbackStatus = $state('')
  let feedbackBusy = $state(false)
  let archiveReceipt = $state<ArchiveReceipt | null>(null)

  let documentDialog = $state(false)
  let documentTitle = $state('')
  let documentBusy = $state(false)
  let documentError = $state('')

  let unlistenLocale: (() => void) | null = null
  let unlistenFocus: (() => void) | null = null

  let removedSet = $derived(new Set(removedKeys))
  let visibleHits = $derived(store.hits.filter((hit) => (
    !removedSet.has(hitKey(hit)) && (sourceFilter === 'all' || hit.origin === sourceFilter)
  )))
  let contextHits = $derived(store.hits.filter((hit) => !removedSet.has(hitKey(hit))))
  let groups = $derived(groupHits(visibleHits))
  let flatVisibleHits = $derived(groups.flatMap((group) => group.files.flatMap((file) => file.hits)))
  let selectedSet = $derived(new Set(selectedKeys))
  let activeHit = $derived(
    flatVisibleHits.find((hit) => hitKey(hit) === activeKey) ?? flatVisibleHits[0] ?? null,
  )
  let highlightTerms = $derived(parseHighlightTerms(store.query || inputValue))
  let usableAgents = $derived(agents.filter((agent) => agent.harness?.ok === true))
  let selectedAgent = $derived(agents.find((agent) => agent.id === selectedProvider) ?? null)
  let answerBusy = $derived(answerPhase === 'preparing' || answerPhase === 'running')
  let currentResultsExhausted = $derived(
    store.route !== null && store.hits.length > 0 && contextHits.length === 0,
  )
  let canAsk = $derived(
    inputValue.trim().length > 0
      && selectedProvider.length > 0
      && !currentResultsExhausted
      && !answerBusy
      && !documentBusy,
  )

  onMount(async () => {
    try {
      await loadLocale()
      unlistenLocale = await watchLocaleChanges()
      await windowApi.setTitle(t('smartSearch.windowTitle'))
    } catch (error) {
      console.warn('[smart-search] locale init failed:', error)
    }
    try {
      agents = await loadSearchAgentOptions()
      const installed = agents.filter((agent) => agent.harness?.ok === true).map((agent) => agent.id)
      selectedProvider = rememberedProvider('global-search', installed, 'notemd.claude-agent')
    } catch (error) {
      agentsError = error instanceof Error ? error.message : String(error)
    }
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
    cancelTimers()
    unlistenLocale?.()
    unlistenFocus?.()
  })

  function cancelTimers(): void {
    if (debounceTimer) clearTimeout(debounceTimer)
    if (deepTimer) clearTimeout(deepTimer)
    debounceTimer = undefined
    deepTimer = undefined
  }

  async function expandWindow(): Promise<void> {
    if (expandedWindow) return
    try {
      await windowApi.setSize(new LogicalSize(980, 680))
      expandedWindow = true
      try { await windowApi.center() } catch { /* Keep the resized window in place. */ }
    } catch { /* Browser preview: CSS still fills the viewport. */ }
  }

  function resetResultEdits(): void {
    selectedKeys = []
    removedKeys = []
    lastRemoved = []
    activeKey = null
    rangeAnchor = null
  }

  function scheduleSearch(): void {
    cancelTimers()
    resetResultEdits()
    // Input changes invalidate in-flight work immediately. Old hits must not
    // remain actionable during the debounce window or an IME hold state.
    store.clear()
    const decision = decideTrigger(inputValue, composing)
    if (decision.kind === 'hold') return
    if (decision.kind === 'clear') return
    debounceTimer = setTimeout(() => { void runShallow(inputValue) }, decision.delayMs)
  }

  async function runShallow(asked: string): Promise<SmartSearchResponse | null> {
    if (!asked.trim()) return null
    void expandWindow()
    const response = await store.run(asked, { deep: false })
    if (response?.deepAvailable && inputValue === asked) {
      deepTimer = setTimeout(() => { void runDeep(asked) }, DEEP_AFTER_MS)
    }
    return response
  }

  async function runDeep(asked = inputValue): Promise<SmartSearchResponse | null> {
    cancelTimers()
    if (!asked.trim()) return null
    void expandWindow()
    return await store.run(asked, { deep: true, timeoutMs: DEEP_TIMEOUT_MS })
  }

  function onInput(event: Event): void {
    resizeInput(event.currentTarget as HTMLTextAreaElement)
    if ((event as InputEvent).isComposing) {
      cancelTimers()
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
      void askAgent()
      return
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      void hideWindow()
    }
  }

  async function hideWindow(): Promise<void> {
    if (documentDialog) {
      documentDialog = false
      return
    }
    cancelTimers()
    try { await invoke('hide_smart_search_window') } catch { /* Browser preview. */ }
  }

  function chooseProvider(event: Event): void {
    selectedProvider = (event.currentTarget as HTMLSelectElement).value
    rememberProvider('global-search', selectedProvider)
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

  function displayLine(hit: SearchHit, query = store.query || inputValue) {
    return previewLine(hit.text, parseHighlightTerms(query))
  }

  function basename(path: string): string {
    return path.slice(path.lastIndexOf('/') + 1)
  }

  function relevanceLabel(reason: SmartRelevanceReason): string {
    switch (reason) {
      case 'exact_page': return 'Wiki'
      case 'strict_query': return '精确'
      case 'exact_phrase': return '短语'
      case 'filename_match': return '文件名'
      case 'breadcrumb_match': return '标题'
      case 'multiple_queries': return '多路命中'
      case 'relaxed_query': return '相关'
    }
  }

  function relevanceReasons(hit: SearchHit): SmartRelevanceReason[] {
    return (hit as SearchHit & { relevanceReasons?: SmartRelevanceReason[] }).relevanceReasons ?? []
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
  }

  function removeKeys(keys: string[]): void {
    const candidates = keys.filter((key) => store.hits.some((hit) => hitKey(hit) === key))
    if (!candidates.length) return
    removedKeys = addRemovedKeys(removedKeys, candidates)
    lastRemoved = candidates
    selectedKeys = []
    if (activeKey && candidates.includes(activeKey)) activeKey = null
  }

  function removeSelected(): void {
    removeKeys(selectedKeys)
  }

  function undoRemove(): void {
    removedKeys = restoreRemovedKeys(removedKeys, lastRemoved)
    selectedKeys = [...lastRemoved]
    activeKey = lastRemoved[0] ?? null
    lastRemoved = []
  }

  function onResultsKeydown(event: KeyboardEvent, hit: SearchHit): void {
    if (isImeKey(event)) return
    if (event.target !== event.currentTarget) return
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      selectedKeys = flatVisibleHits.map(hitKey)
      activeKey = hitKey(hit)
      rangeAnchor = activeKey
    } else if (event.key === 'Enter') {
      event.preventDefault()
      void openHit(hit, store.query || inputValue, event.metaKey || event.ctrlKey)
    } else if ((event.key === 'Delete' || event.key === 'Backspace') && selectedKeys.length) {
      event.preventDefault()
      removeSelected()
    }
  }

  async function openHit(
    hit: SearchHit,
    query = store.query || inputValue,
    keepSearchWindow = false,
  ): Promise<void> {
    const line = displayLine(hit, query)
    navigationError = ''
    try {
      await invoke('editor_show_and_reveal_search_hit', {
        path: hit.absPath,
        line: hit.line + line.line,
        anchor: line.text || hit.text,
      })
      if (!keepSearchWindow) await invoke('hide_smart_search_window')
    } catch (error) {
      navigationError = error instanceof Error ? error.message : String(error)
    }
  }

  function currentModel(provider = selectedProvider): string | null {
    return agents.find((agent) => agent.id === provider)?.harness?.default_model ?? null
  }

  function newId(): string {
    return typeof crypto?.randomUUID === 'function'
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(16).slice(2)}`
  }

  async function exactSnapshot(query: string): Promise<SearchHit[]> {
    let response: SmartSearchResponse | null = null
    if (!store.loading && store.route !== null && store.query === query) {
      response = {
        route: store.route === 'smart-scan' ? 'smart-scan' : 'smart-fts',
        tookMs: store.tookMs,
        total: store.total,
        hits: store.hits as SmartSearchResponse['hits'],
        truncated: store.truncated,
        deepAvailable: store.deepAvailable,
        extractedTerms: store.extractedTerms,
        subqueries: store.subqueries,
      }
    } else {
      response = await store.run(query, { deep: false })
    }
    if (response?.hits.length === 0 && response.deepAvailable) {
      response = await store.run(query, { deep: true, timeoutMs: DEEP_TIMEOUT_MS })
    }
    return response?.hits ?? []
  }

  async function askAgent(): Promise<void> {
    const query = inputValue.trim()
    const provider = selectedProvider
    const model = currentModel(provider)
    const removedAtStart = new Set(removedKeys)
    if (!query || !provider || currentResultsExhausted || answerBusy || documentBusy) return
    cancelTimers()
    void expandWindow()
    const attempt = ++answerAttempt
    answerPhase = 'preparing'
    answerError = ''
    progressText = ''
    feedback = null
    feedbackStatus = ''
    archiveReceipt = null
    try {
      const snapshotHits = await exactSnapshot(query)
      if (attempt !== answerAttempt) return
      const availableHits = snapshotHits.filter((hit) => !removedAtStart.has(hitKey(hit)))
      const sources = selectContextSources(availableHits)
      if (!sources.length) throw new Error(t('smartSearch.noContext'))

      const memory = await smartSearchApi.memoryContext(provider, model).catch((error) => ({
        available: false,
        selected: [],
        excludedSummary: {},
        manifestId: null,
        error: error instanceof Error ? error.message : String(error),
      }))
      if (attempt !== answerAttempt) return
      const context: AnswerContext = {
        query,
        queryId: newId(),
        sources,
        memory: memory.selected,
        memoryManifestId: memory.manifestId,
      }
      const prompt = buildSearchAnswerPrompt('short', context)
      answerPhase = 'running'
      const runId = await startSearchAgentTask(provider, prompt)
      answerProvider = provider
      answerModel = model
      answerRunId = runId
      answerContext = context
      const result = await pollSearchAgentTask(provider, runId, (progress) => {
        progressText = progress.last || (progress.steps ? `${progress.steps}` : '')
      })
      if (attempt !== answerAttempt) return
      const content = result.content.trim()
      if (!content) throw new Error(t('smartSearch.answerError'))
      answer = content
      answerId = newId()
      answerPhase = 'done'
    } catch (error) {
      if (attempt !== answerAttempt) return
      answerPhase = 'error'
      answerError = error instanceof Error ? error.message : String(error)
    }
  }

  async function openCitation(citation: string): Promise<void> {
    if (!answerContext) return
    const hit = sourceForCitation(answerContext.sources, citation)
    if (hit) await openHit(hit, answerContext.query)
  }

  async function likeAnswer(): Promise<void> {
    if (!answerContext || !answer || feedbackBusy) return
    feedbackBusy = true
    feedbackStatus = ''
    try {
      archiveReceipt = await smartSearchApi.archiveAnswer({
        answerId,
        query: answerContext.query,
        answer,
        provider: answerProvider,
        model: answerModel,
        runId: answerRunId,
        memoryManifestId: answerContext.memoryManifestId,
        sources: answerContext.sources,
      })
      feedback = 'helpful'
      feedbackStatus = t('smartSearch.archived')
    } catch (error) {
      feedbackStatus = error instanceof Error ? error.message : String(error)
    } finally {
      feedbackBusy = false
    }
  }

  async function dislikeAnswer(reason: string | null = null): Promise<void> {
    if (!answerId || feedbackBusy) return
    feedbackBusy = true
    feedbackStatus = ''
    try {
      await smartSearchApi.recordFeedback(answerId, 'unhelpful', reason)
      feedback = 'unhelpful'
      feedbackStatus = t('smartSearch.feedbackSaved')
    } catch (error) {
      feedbackStatus = error instanceof Error ? error.message : String(error)
    } finally {
      feedbackBusy = false
    }
  }

  function showDocumentDialog(): void {
    if (!answerContext) return
    documentTitle = answerContext.query.replace(/\s+/g, ' ').slice(0, 72)
    documentError = ''
    documentDialog = true
  }

  async function generateDocument(): Promise<void> {
    if (!answerContext || !documentTitle.trim() || documentBusy) return
    documentBusy = true
    documentError = ''
    try {
      // A document is a new Agent run. Re-authorize long-term memory instead of
      // carrying an earlier policy decision across runs.
      const memory = await smartSearchApi.memoryContext(answerProvider, answerModel).catch((error) => ({
        available: false,
        selected: [],
        excludedSummary: {},
        manifestId: null,
        error: error instanceof Error ? error.message : String(error),
      }))
      const documentContext: AnswerContext = {
        ...answerContext,
        memory: memory.selected,
        memoryManifestId: memory.manifestId,
      }
      const prompt = buildSearchAnswerPrompt('document', documentContext, answer)
      const runId = await startSearchAgentTask(answerProvider, prompt)
      const result = await pollSearchAgentTask(answerProvider, runId, (progress) => {
        progressText = progress.last || (progress.steps ? `${progress.steps}` : '')
      })
      const receipt = await smartSearchApi.writeDocument({
        title: documentTitle.trim(),
        query: documentContext.query,
        content: result.content.trim(),
        provider: answerProvider,
        model: answerModel,
        runId,
        memoryManifestId: documentContext.memoryManifestId,
        sources: documentContext.sources,
      })
      documentDialog = false
      await invoke('editor_show_and_reveal_search_hit', {
        path: receipt.path,
        line: 1,
        anchor: documentTitle.trim(),
      })
      await invoke('hide_smart_search_window')
    } catch (error) {
      documentError = error instanceof Error ? error.message : String(error)
    } finally {
      documentBusy = false
    }
  }
</script>

<svelte:window onkeydown={(event) => {
  if (event.key !== 'Escape' || isImeKey(event)) return
  const target = event.target as HTMLElement | null
  if (target === inputEl) return
  event.preventDefault()
  void hideWindow()
}} />

<main class:expanded={store.route !== null || answerPhase !== 'idle'}>
  <section class="command-bar" aria-label={t('smartSearch.windowTitle')}>
    <span class="search-icon" aria-hidden="true">⌕</span>
    <textarea
      bind:this={inputEl}
      bind:value={inputValue}
      rows="1"
      class="query-input"
      placeholder={t('smartSearch.placeholder')}
      disabled={answerPhase === 'preparing'}
      oninput={onInput}
      onkeydown={onInputKeydown}
      oncompositionstart={() => { composing = true; inputIme.start(); cancelTimers() }}
      oncompositionend={onCompositionEnd}
      onblur={() => inputIme.reset()}
      aria-label={t('smartSearch.placeholder')}
    ></textarea>
    <select class="agent-select" value={selectedProvider} disabled={answerBusy} onchange={chooseProvider} aria-label="Agent">
      {#if usableAgents.length === 0}
        <option value="">Agent</option>
      {/if}
      {#each usableAgents as agent (agent.id)}
        <option value={agent.id}>{agent.harness?.harness || agent.name}</option>
      {/each}
    </select>
    <button class="ask-button" disabled={!canAsk} onclick={() => void askAgent()}>
      {answerBusy ? '…' : t('smartSearch.ask')}
      <kbd>↵</kbd>
    </button>
  </section>
  <div class="input-hint">
    <span>{t('smartSearch.inputHint')}</span>
    {#if selectedAgent?.harness?.default_model}
      <span>{selectedAgent.harness.default_model}</span>
    {/if}
  </div>

  {#if !ready}
    <div class="launch-state">…</div>
  {:else if agentsError || usableAgents.length === 0}
    <div class="agent-warning">{t('smartSearch.noAgents')}{agentsError ? ` ${agentsError}` : ''}</div>
  {/if}

  {#if navigationError}
    <div class="navigation-error" role="alert">
      <span>{navigationError}</span>
      <button aria-label={t('common.close')} onclick={() => { navigationError = '' }}>×</button>
    </div>
  {/if}

  {#if store.route === null && answerPhase === 'idle'}
    <div class="empty-launch">
      <div class="empty-mark">⌘</div>
      <p>{t('smartSearch.emptyPrompt')}</p>
    </div>
  {:else}
    <section class="workspace">
      <aside class="results-pane" aria-label={t('smartSearch.results')}>
        <header class="pane-header">
          <strong>{t('smartSearch.results')}</strong>
          <span>{store.loading ? '…' : store.hits.length}</span>
          <select bind:value={sourceFilter} aria-label={t('smartSearch.sourceAll')}>
            <option value="all">{t('smartSearch.sourceAll')}</option>
            <option value="human">{t('search.group.human')}</option>
            <option value="source">{t('search.group.source')}</option>
            <option value="derived">{t('smartSearch.sourceDerived')}</option>
            <option value="unlabeled">{t('search.group.unlabeled')}</option>
          </select>
        </header>

        {#if selectedKeys.length > 0}
          <div class="selection-bar">
            <span>{t('smartSearch.selectedCount', { n: selectedKeys.length })}</span>
            <button onclick={removeSelected}>{t('smartSearch.removeSelected')}</button>
          </div>
        {:else if removedKeys.length > 0}
          <div class="selection-bar removed-note">
            <span>{t('smartSearch.removedCount', { n: removedKeys.length })}</span>
            {#if lastRemoved.length > 0}<button onclick={undoRemove}>{t('smartSearch.undo')}</button>{/if}
          </div>
        {/if}

        <div class="results-scroll" role="listbox" aria-multiselectable="true">
          {#if store.error}
            <p class="state error"><strong>{t('smartSearch.searchError')}:</strong> {store.error}</p>
          {:else if store.loading && store.hits.length === 0}
            <div class="state"><span class="spinner"></span></div>
          {:else if store.route !== null && visibleHits.length === 0}
            <div class="state">
              <p>{removedKeys.length ? t('smartSearch.noContext') : t('search.noResults')}</p>
              {#if removedKeys.length}<small>{t('smartSearch.notDeleted')}</small>{/if}
            </div>
          {:else}
            {#each groups as group (groupKey(group))}
              <section class="result-group">
                <h2>{groupLabel(group)} <span>{group.hitCount}</span></h2>
                {#each group.files as file (`${groupKey(group)}:${file.path}`)}
                  {#each file.hits as hit (hitKey(hit))}
                    {@const key = hitKey(hit)}
                    {@const line = displayLine(hit)}
                    <div
                      class="result-row"
                      class:selected={selectedSet.has(key)}
                      class:active={activeKey === key}
                      role="option"
                      aria-selected={selectedSet.has(key)}
                      tabindex="0"
                      onclick={(event) => selectHit(event, hit)}
                      ondblclick={() => void openHit(hit)}
                      onkeydown={(event) => onResultsKeydown(event, hit)}
                    >
                      <span class="check" aria-hidden="true">{selectedSet.has(key) ? '✓' : ''}</span>
                      <div class="result-copy">
                        <div class="result-title">
                          <strong>{basename(file.path)}</strong>
                          {#if hit.breadcrumb}<span>{hit.breadcrumb}</span>{/if}
                          <small>:{hit.line + line.line}</small>
                        </div>
                        <p>
                          {#each highlightParts(line.text || hit.text.slice(0, 180), highlightTerms) as part}
                            {#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}
                          {/each}
                        </p>
                        <div class="result-meta">
                          {#each relevanceReasons(hit).slice(0, 2) as reason}
                            <span>{relevanceLabel(reason)}</span>
                          {/each}
                          {#if hit.humanVerified}<span>✓ {t('search.humanVerified')}</span>{/if}
                        </div>
                      </div>
                      <button
                        class="remove-one"
                        title={t('smartSearch.removeSelected')}
                        aria-label={t('smartSearch.removeSelected')}
                        onclick={(event) => { event.stopPropagation(); removeKeys([key]) }}
                      >×</button>
                    </div>
                  {/each}
                {/each}
              </section>
            {/each}
          {/if}
        </div>
      </aside>

      <article class="answer-pane" aria-label={t('smartSearch.answer')}>
        <header class="pane-header answer-header">
          <strong>{answerPhase === 'done' ? t('smartSearch.answer') : t('smartSearch.openEditor')}</strong>
          {#if answerContext?.memoryManifestId}<span class="memory-chip">◈ {t('smartSearch.memoryUsed')}</span>{/if}
        </header>

        {#if answerPhase === 'preparing' || answerPhase === 'running'}
          <div class="answer-state">
            <span class="spinner"></span>
            <strong>{answerPhase === 'preparing' ? t('smartSearch.preparing') : t('smartSearch.running')}</strong>
            {#if progressText}<small>{progressText}</small>{/if}
          </div>
        {:else if answerPhase === 'error'}
          <div class="answer-state error">
            <strong>{t('smartSearch.answerError')}</strong>
            <p>{answerError}</p>
          </div>
        {:else if answerPhase === 'done' && answerContext}
          <div class="answer-scroll">
            <p class="answered-query">{answerContext.query}</p>
            <div class="answer-body">
              {#each parseAnswerSegments(answer) as segment}
                {#if segment.kind === 'citation'}
                  <button class="citation" onclick={() => void openCitation(segment.value)}>[{segment.value}]</button>
                {:else}<span>{segment.value}</span>{/if}
              {/each}
            </div>
            <div class="answer-sources">
              {#each answerContext.sources as source}
                <button onclick={() => void openHit(source.hit, answerContext?.query || '')}>
                  <b>[{source.id}]</b> {basename(source.hit.path)}:{source.hit.line}
                </button>
              {/each}
            </div>
          </div>
          <footer class="answer-actions">
            <button class:chosen={feedback === 'helpful'} disabled={feedbackBusy || feedback !== null} onclick={() => void likeAnswer()} title={t('smartSearch.like')}>👍</button>
            <button class:chosen={feedback === 'unhelpful'} disabled={feedbackBusy || feedback !== null} onclick={() => void dislikeAnswer()} title={t('smartSearch.dislike')}>👎</button>
            <span>{feedbackStatus}</span>
            <button class="document-button" onclick={showDocumentDialog}>{t('smartSearch.detailed')}</button>
          </footer>
        {:else if activeHit}
          {@const line = displayLine(activeHit)}
          <div class="preview-card">
            <small>{activeHit.path}:{activeHit.line + line.line}</small>
            <h1>{activeHit.breadcrumb || basename(activeHit.path)}</h1>
            <p>{line.text || activeHit.text}</p>
            <button onclick={() => void openHit(activeHit)}>{t('smartSearch.openEditor')} ↗</button>
          </div>
        {:else}
          <div class="answer-state">
            <p>{t('smartSearch.emptyPrompt')}</p>
          </div>
        {/if}
      </article>
    </section>
  {/if}

  {#if documentDialog}
    <div class="dialog-backdrop" role="presentation" onclick={(event) => {
      if (event.currentTarget === event.target && !documentBusy) documentDialog = false
    }}>
      <div class="document-dialog" role="dialog" tabindex="-1" aria-modal="true" aria-label={t('smartSearch.detailed')}>
        <h2>{t('smartSearch.detailed')}</h2>
        <label>
          <span>{t('smartSearch.documentTitle')}</span>
          <input bind:value={documentTitle} disabled={documentBusy} />
        </label>
        {#if documentBusy}<p><span class="spinner"></span> {t('smartSearch.generating')}</p>{/if}
        {#if documentError}<p class="error">{documentError}</p>{/if}
        <footer>
          <button disabled={documentBusy} onclick={() => { documentDialog = false }}>{t('common.cancel')}</button>
          <button class="primary" disabled={documentBusy || !documentTitle.trim()} onclick={() => void generateDocument()}>{t('smartSearch.generateOpen')}</button>
        </footer>
      </div>
    </div>
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

  main {
    height: 100vh;
    min-height: 150px;
    display: flex;
    flex-direction: column;
    background: color-mix(in srgb, Canvas 96%, CanvasText 4%);
    overflow: hidden;
  }

  .command-bar {
    display: grid;
    grid-template-columns: 24px minmax(120px, 1fr) auto auto;
    align-items: center;
    gap: 8px;
    margin: 12px 12px 0;
    min-height: 48px;
    padding: 5px 7px 5px 12px;
    box-sizing: border-box;
    border: 1px solid color-mix(in srgb, AccentColor 45%, var(--smart-border));
    border-radius: 13px;
    background: Canvas;
    box-shadow: 0 5px 20px rgba(0, 0, 0, 0.09), 0 0 0 3px color-mix(in srgb, AccentColor 10%, transparent);
  }

  .search-icon { font-size: 25px; line-height: 1; color: var(--smart-muted); transform: rotate(-18deg); }
  .query-input {
    resize: none;
    border: 0;
    outline: 0;
    background: transparent;
    color: CanvasText;
    min-height: 24px;
    max-height: 68px;
    padding: 4px 0;
    line-height: 1.45;
    overflow-y: auto;
  }

  .query-input::placeholder { color: color-mix(in srgb, CanvasText 42%, transparent); }
  .agent-select, .pane-header select {
    max-width: 150px;
    height: 30px;
    border: 1px solid var(--smart-border);
    border-radius: 7px;
    background: var(--smart-soft);
    padding: 0 24px 0 8px;
    color: CanvasText;
  }

  .ask-button, .primary {
    height: 32px;
    border: 0;
    border-radius: 8px;
    background: #0a63ff;
    color: white;
    padding: 0 11px;
    font-weight: 600;
  }

  .ask-button:disabled, button:disabled { opacity: .45; }
  .ask-button kbd { margin-left: 6px; font: inherit; opacity: .7; }
  .input-hint { display: flex; justify-content: space-between; padding: 6px 19px 8px 48px; color: var(--smart-muted); font-size: 11px; }
  .launch-state, .agent-warning { padding: 6px 20px; color: var(--smart-muted); font-size: 12px; }
  .agent-warning { color: #b45309; }
  .navigation-error {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 7px 14px;
    background: color-mix(in srgb, #dc2626 9%, Canvas);
    color: #b42318;
    font-size: 11px;
  }
  .navigation-error button { border: 0; background: transparent; color: inherit; font-size: 17px; }

  .empty-launch {
    flex: 1;
    min-height: 62px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    color: var(--smart-muted);
  }

  .empty-launch p { margin: 0; font-size: 12px; }
  .empty-mark { width: 25px; height: 25px; display: grid; place-items: center; border: 1px solid var(--smart-border); border-radius: 6px; }

  .workspace {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(300px, 42%) minmax(360px, 58%);
    border-top: 1px solid var(--smart-border);
    background: Canvas;
  }

  .results-pane, .answer-pane { min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .results-pane { border-right: 1px solid var(--smart-border); background: color-mix(in srgb, Canvas 97%, CanvasText 3%); }
  .pane-header {
    min-height: 39px;
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 0 12px;
    border-bottom: 1px solid var(--smart-border);
    font-size: 12px;
  }

  .pane-header > span { color: var(--smart-muted); }
  .pane-header select { margin-left: auto; height: 25px; max-width: 130px; font-size: 11px; }
  .answer-header { justify-content: space-between; }
  .memory-chip { font-size: 10px; color: #0f766e !important; }

  .selection-bar {
    min-height: 34px;
    padding: 0 10px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    border-bottom: 1px solid var(--smart-border);
    background: color-mix(in srgb, AccentColor 9%, Canvas);
    font-size: 11px;
  }

  .selection-bar button { border: 0; background: transparent; color: #0a63ff; padding: 4px; }
  .removed-note { background: color-mix(in srgb, #d97706 8%, Canvas); }
  .results-scroll, .answer-scroll { flex: 1; min-height: 0; overflow: auto; }
  .result-group h2 {
    position: sticky;
    top: 0;
    z-index: 1;
    margin: 0;
    padding: 7px 11px 5px;
    display: flex;
    justify-content: space-between;
    background: color-mix(in srgb, Canvas 90%, transparent);
    backdrop-filter: blur(12px);
    color: var(--smart-muted);
    font-size: 10px;
    font-weight: 650;
    text-transform: uppercase;
    letter-spacing: .04em;
  }

  .result-row {
    display: grid;
    grid-template-columns: 17px minmax(0, 1fr) 22px;
    gap: 7px;
    padding: 8px 8px 8px 10px;
    border-bottom: 1px solid color-mix(in srgb, CanvasText 7%, transparent);
    outline: none;
    cursor: default;
  }

  .result-row:hover, .result-row.active { background: color-mix(in srgb, AccentColor 7%, Canvas); }
  .result-row.selected { background: color-mix(in srgb, AccentColor 15%, Canvas); }
  .result-row:focus-visible { box-shadow: inset 0 0 0 2px color-mix(in srgb, AccentColor 52%, transparent); }
  .check { width: 15px; height: 15px; margin-top: 1px; display: grid; place-items: center; border: 1px solid var(--smart-border); border-radius: 4px; color: white; font-size: 10px; }
  .selected .check { background: #0a63ff; border-color: #0a63ff; }
  .result-copy { min-width: 0; }
  .result-title { display: flex; align-items: baseline; gap: 5px; min-width: 0; }
  .result-title strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
  .result-title > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--smart-muted); font-size: 10px; }
  .result-title small { margin-left: auto; color: var(--smart-muted); font-size: 9px; }
  .result-copy p { margin: 4px 0; color: color-mix(in srgb, CanvasText 78%, transparent); font-size: 11px; line-height: 1.35; overflow: hidden; display: -webkit-box; line-clamp: 2; -webkit-line-clamp: 2; -webkit-box-orient: vertical; }
  mark { background: color-mix(in srgb, #facc15 48%, transparent); color: inherit; border-radius: 2px; }
  .result-meta { display: flex; flex-wrap: wrap; gap: 4px; }
  .result-meta span { padding: 1px 4px; border-radius: 4px; background: var(--smart-soft); color: var(--smart-muted); font-size: 9px; }
  .remove-one { opacity: 0; align-self: start; border: 0; background: transparent; border-radius: 4px; color: var(--smart-muted); font-size: 18px; line-height: 18px; padding: 0; }
  .result-row:hover .remove-one, .remove-one:focus-visible { opacity: 1; }
  .remove-one:hover { background: color-mix(in srgb, #dc2626 14%, transparent); color: #dc2626; }

  .state, .answer-state { margin: auto; padding: 28px; text-align: center; color: var(--smart-muted); }
  .state small, .answer-state small { display: block; margin-top: 7px; }
  .error { color: #b42318; }
  .spinner { display: inline-block; width: 12px; height: 12px; border: 2px solid var(--smart-border); border-top-color: #0a63ff; border-radius: 50%; animation: spin .8s linear infinite; vertical-align: -2px; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .preview-card { margin: auto; width: min(520px, calc(100% - 52px)); }
  .preview-card small, .answered-query { color: var(--smart-muted); }
  .preview-card h1 { margin: 7px 0 13px; font-size: 20px; }
  .preview-card p { white-space: pre-wrap; line-height: 1.6; }
  .preview-card button, .answer-sources button, .document-button {
    border: 1px solid var(--smart-border);
    border-radius: 7px;
    background: var(--smart-soft);
    padding: 6px 9px;
  }

  .answer-scroll { padding: 23px clamp(20px, 5vw, 54px); box-sizing: border-box; }
  .answered-query { margin: 0 0 16px; font-size: 11px; }
  .answer-body { white-space: pre-wrap; font-size: 14px; line-height: 1.72; }
  .citation { display: inline; margin: 0 1px; padding: 1px 4px; border: 0; border-radius: 4px; background: color-mix(in srgb, AccentColor 12%, Canvas); color: #0a63ff; font-size: 11px; font-weight: 700; vertical-align: 1px; }
  .answer-sources { margin-top: 26px; padding-top: 12px; border-top: 1px solid var(--smart-border); display: flex; flex-wrap: wrap; gap: 6px; }
  .answer-sources button { color: var(--smart-muted); font-size: 10px; }
  .answer-sources b { color: #0a63ff; }
  .answer-actions { min-height: 48px; padding: 0 12px; display: flex; align-items: center; gap: 7px; border-top: 1px solid var(--smart-border); }
  .answer-actions > button:not(.document-button) { width: 31px; height: 29px; border: 1px solid var(--smart-border); border-radius: 7px; background: transparent; }
  .answer-actions button.chosen { background: color-mix(in srgb, AccentColor 13%, Canvas); border-color: color-mix(in srgb, AccentColor 42%, transparent); }
  .answer-actions span { color: var(--smart-muted); font-size: 10px; }
  .document-button { margin-left: auto; }

  .dialog-backdrop { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; background: rgba(0, 0, 0, .25); }
  .document-dialog { width: min(420px, calc(100vw - 32px)); padding: 18px; box-sizing: border-box; border: 1px solid var(--smart-border); border-radius: 13px; background: Canvas; box-shadow: 0 18px 60px rgba(0,0,0,.25); }
  .document-dialog h2 { margin: 0 0 16px; font-size: 16px; }
  .document-dialog label span { display: block; margin-bottom: 6px; color: var(--smart-muted); font-size: 11px; }
  .document-dialog input { width: 100%; height: 34px; padding: 0 9px; box-sizing: border-box; border: 1px solid var(--smart-border); border-radius: 7px; background: Canvas; color: CanvasText; }
  .document-dialog footer { margin-top: 17px; display: flex; justify-content: flex-end; gap: 8px; }
  .document-dialog footer button { min-height: 31px; padding: 0 11px; border: 1px solid var(--smart-border); border-radius: 7px; background: var(--smart-soft); }
  .document-dialog footer .primary { border: 0; background: #0a63ff; color: white; }

  @media (max-width: 720px) {
    .command-bar { grid-template-columns: 20px minmax(90px, 1fr) minmax(84px, 104px) auto; }
    .agent-select { display: block; max-width: 104px; padding-left: 6px; }
    .ask-button { padding: 0 8px; }
    .ask-button kbd { display: none; }
    .input-hint span:last-child { display: none; }
    .workspace { grid-template-columns: 1fr; grid-template-rows: minmax(220px, 45%) minmax(250px, 55%); }
    .results-pane { border-right: 0; border-bottom: 1px solid var(--smart-border); }
    .answer-scroll { padding: 18px; }
  }

  @media (prefers-reduced-motion: reduce) { .spinner { animation: none; } }
</style>
