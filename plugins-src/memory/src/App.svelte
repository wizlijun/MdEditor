<script lang="ts">
  import { onMount, tick } from 'svelte'
  import AgentPicker from './lib/agent-picker/AgentPicker.svelte'
  import {
    rememberProvider,
    rememberedProvider,
    type AgentOption,
  } from './lib/agent-picker/types'
  import {
    agentProviders,
    memoryAdd,
    memoryApprove,
    memoryContext,
    memoryContextManifest,
    memoryDelete,
    memoryDeleteCandidate,
    memoryIgnore,
    memoryInitialize,
    memoryReject,
    memoryResolve,
    memorySetSalience,
    memorySnapshot,
    requestId,
    toast,
  } from './lib/bridge'
  import {
    actorLabel,
    approvalForPending,
    approvalKindFor,
    approvalLabels,
    categoryLabel,
    categoryOptions,
    claimKindLabels,
    currentClaims,
    expectedHeads,
    formatDate,
    hostError,
    pendingClaims,
    riskFor,
    subjectLabel,
    temporalLabel,
  } from './lib/domain'
  import {
    completedInference,
    detectInferenceMode,
    INFERENCE_POLL_MS,
    interpretInferenceStatus,
    memoryInferenceStatus,
    startMemoryInference,
    type InferenceMode,
  } from './lib/inference'
  import type {
    ClaimKind,
    ContextPreview,
    EffectiveClaim,
    MemoryConflict,
    MemorySnapshotV2,
    PendingClaim,
    ProjectionTarget,
    ProviderPolicy,
    Salience,
  } from './lib/types'

  type Tab = 'confirmed' | 'pending' | 'history'
  type DestructiveAction = { kind: 'reject' | 'delete-candidate' | 'approve-lifecycle'; pending: PendingClaim } | { kind: 'delete-claim'; current: EffectiveClaim }

  let snapshot = $state<MemorySnapshotV2 | null>(null)
  let loading = $state(true)
  let writing = $state(false)
  let error = $state('')
  let announcement = $state('')
  let tab = $state<Tab>('confirmed')
  let query = $state('')
  let target = $state<'all' | ProjectionTarget | 'structured'>('all')
  let selectedClaimId = $state<string | null>(null)
  let selectedPendingId = $state<string | null>(null)
  let showAdd = $state(false)
  let showContext = $state(false)
  let openMenuFor = $state<string | null>(null)
  let destructive = $state<DestructiveAction | null>(null)
  let mergeConflict = $state<MemoryConflict | null>(null)
  let mergedText = $state('')

  const AGENT_SURFACE = 'memory-inference'
  let agents: AgentOption[] = $state([])
  let agentId: string | undefined = $state(undefined)
  let inferenceMode = $state<InferenceMode>('full')
  let inferenceStarting = $state(false)
  let inferenceRun = $state<{ runId: string; invocationId: string; harness?: string } | null>(null)
  let inferenceProgress = $state('')
  let inferencePollFailures = 0
  let inferenceTimer: ReturnType<typeof setTimeout> | undefined

  let addTarget = $state<'user' | 'memory'>('user')
  let addCategory = $state('preferences')
  let addKind = $state<ClaimKind>('preference')
  let addText = $state('')
  let addSpace = $state('global')
  let addPurposes = $state<string[]>(['planning', 'writing', 'information-answer', 'projection', 'sync'])
  let addProviderPolicy = $state<ProviderPolicy>('prompt')
  let addSalience = $state<Salience>('normal')
  let addPolarity = $state<'positive' | 'negative' | 'neutral'>('neutral')
  let addSensitivity = $state<'normal' | 'private'>('normal')
  let addGuidance = $state('')
  let addAvoid = $state('')

  let contextSpace = $state('global')
  let contextPurpose = $state('planning')
  let contextProvider = $state('openai')
  let contextModel = $state('gpt-5')
  let contextTools = $state('notemd-search')
  let contextExternal = $state(true)
  let contextAsOf = $state(new Date().toISOString().slice(0, 16))
  let contextPreview = $state<ContextPreview | null>(null)
  let contextBusy = $state(false)
  let addTextarea = $state<HTMLTextAreaElement>()

  const writable = $derived(snapshot?.mode === 'v2' && snapshot.health.status !== 'damaged' && snapshot.health.status !== 'unsupported')
  const canInfer = $derived(writable || snapshot?.initialization_required === true)
  const visibleClaims = $derived(snapshot ? currentClaims(snapshot.claims, query, target) : [])
  const reviews = $derived(snapshot ? pendingClaims(snapshot.pending) : [])
  const selectedClaim = $derived(visibleClaims.find(({ claim }) => claim.claim_id === selectedClaimId) ?? visibleClaims[0])
  const selectedPending = $derived(reviews.find(({ revision }) => revision.revision_id === selectedPendingId) ?? reviews[0])
  const currentCategoryOptions = $derived(categoryOptions[addTarget])
  const addApproval = $derived(approvalKindFor(addKind))
  const addNeedsAvoid = $derived(addKind === 'boundary' || addPolarity === 'negative' || addKind === 'practice')

  onMount(() => {
    void refresh()
    void loadAgents()
    void loadInferenceMode()
    return () => { if (inferenceTimer) clearTimeout(inferenceTimer) }
  })

  async function loadAgents() {
    try {
      const result = await agentProviders()
      agents = result.providers ?? []
      agentId = rememberedProvider(AGENT_SURFACE, agents.map((agent) => agent.id), result.default)
    } catch {
      agents = []
      agentId = undefined
    }
  }

  function pickAgent(id: string) {
    agentId = id
    rememberProvider(AGENT_SURFACE, id)
  }

  function agentPickerLabel(key: string, vars: Record<string, string | number> = {}) {
    if (key === 'agentPicker.by') return `由 ${vars.name ?? ''}`
    if (key === 'agentPicker.model') return `模型 ${vars.model ?? ''}`
    if (key === 'agentPicker.broken') return '当前不可用'
    if (key === 'agentPicker.notInstalled') return '未安装运行环境'
    return '状态未知'
  }

  async function loadInferenceMode() {
    try { inferenceMode = await detectInferenceMode() } catch { inferenceMode = 'full' }
  }

  async function inferMemory() {
    if (inferenceStarting || inferenceRun || !canInfer) return
    inferenceStarting = true
    inferenceProgress = ''
    error = ''
    if (snapshot?.initialization_required) {
      try {
        snapshot = await memoryInitialize()
      } catch (cause) {
        inferenceStarting = false
        error = hostError(cause)
        return
      }
    }
    const harness = agentId
    const result = await startMemoryInference({ mode: inferenceMode, harness })
    inferenceStarting = false
    if (!result.ok) {
      error = result.reason === 'agent-missing'
        ? '没有可用的 AI Agent。请先安装并启用一个 Agent 插件。'
        : hostError(result.message)
      return
    }
    inferenceRun = { runId: result.runId, invocationId: result.invocationId, ...(harness ? { harness } : {}) }
    inferenceProgress = inferenceMode === 'full' ? '正在扫描 Vault…' : '正在读取 Vault 增量…'
    inferencePollFailures = 0
    scheduleInferencePoll()
  }

  function scheduleInferencePoll() {
    if (inferenceTimer) clearTimeout(inferenceTimer)
    inferenceTimer = setTimeout(() => { void pollInference() }, INFERENCE_POLL_MS)
  }

  async function pollInference() {
    const current = inferenceRun
    if (!current) return
    try {
      const view = interpretInferenceStatus(await memoryInferenceStatus(current.runId, current.harness))
      if (inferenceRun?.runId !== current.runId) return
      inferencePollFailures = 0
      if (view.kind === 'running') {
        inferenceProgress = view.last || `已执行 ${view.steps} 步…`
        scheduleInferencePoll()
        return
      }
      inferenceRun = null
      if (view.kind === 'lost') {
        error = '无法确认这次记忆推理的运行状态；未推进增量水位。'
        return
      }
      if (!view.success) {
        error = view.message ? `记忆推理失败：${view.message}` : '记忆推理失败；未推进增量水位。'
        return
      }
      const checkpointed = await completedInference(current.invocationId)
      inferenceProgress = ''
      tab = 'pending'
      await refresh()
      await loadInferenceMode()
      announcement = checkpointed
        ? '记忆推理完成；新建议已放入待确认。'
        : '记忆推理已结束；未确认成功水位，下次仍会安全执行全量扫描。'
      await toast(checkpointed ? 'success' : 'warn', announcement)
    } catch (cause) {
      if (inferenceRun?.runId !== current.runId) return
      inferencePollFailures += 1
      if (inferencePollFailures < 5) {
        scheduleInferencePoll()
      } else {
        inferenceRun = null
        error = `无法继续读取 Agent 状态：${hostError(cause)}`
      }
    }
  }

  async function refresh() {
    loading = true
    error = ''
    try {
      snapshot = await memorySnapshot()
      selectedClaimId ??= snapshot.claims[0]?.claim.claim_id ?? null
      selectedPendingId ??= snapshot.pending[0]?.revision.revision_id ?? null
      contextSpace = snapshot.context_options?.spaces[0]?.id ?? contextSpace
      contextPurpose = snapshot.context_options?.purposes[0]?.id ?? contextPurpose
      contextProvider = snapshot.context_options?.providers[0]?.id ?? contextProvider
      contextModel = snapshot.context_options?.models.find((option) => !option.provider_id || option.provider_id === contextProvider)?.id ?? contextModel
    } catch (cause) {
      error = hostError(cause)
    } finally {
      loading = false
    }
  }

  function protocol() {
    if (!snapshot?.protocol) throw new Error('Memory v2 protocol context is unavailable.')
    return snapshot.protocol
  }

  async function completed(message: string, projectionRebuilt = true) {
    announcement = projectionRebuilt ? message : `${message}；纯文本投影等待重建。`
    await toast(projectionRebuilt ? 'success' : 'warn', announcement)
    await refresh()
  }

  async function decidePending(item: PendingClaim, action: 'approve' | 'ignore', salience?: Salience) {
    if (writing) return
    writing = true
    error = ''
    try {
      const input = {
        request_id: requestId(`memory-ui/${action}`),
        expected_protocol: protocol(),
        expected_heads: item.expected_heads,
        revision_id: item.revision.revision_id,
        expected_sha256: item.expected_sha256,
        gesture_intent: action,
        ...(salience ? { salience_override: salience } : {}),
      } as const
      const receipt = action === 'approve' ? await memoryApprove(input) : await memoryIgnore(input)
      await completed(action === 'approve' ? (salience === 'pinned' ? '已确认并标为重要' : '已确认这条主张') : '已忽略这条建议', receipt.projection_rebuilt)
    } catch (cause) {
      error = hostError(cause)
    } finally {
      writing = false
    }
  }

  async function confirmDestructive() {
    if (!destructive || writing) return
    writing = true
    error = ''
    try {
      if (destructive.kind === 'delete-claim') {
        const claim = destructive.current.claim
        const receipt = await memoryDelete({
          request_id: requestId('memory-ui/delete'),
          expected_protocol: protocol(),
          claim_id: claim.claim_id,
          expected_heads: expectedHeads(claim),
        })
        destructive = null
        await completed('已将主张移出当前记忆；不可变历史仍保留', receipt.projection_rebuilt)
      } else {
        const item = destructive.pending
        if (destructive.kind === 'approve-lifecycle') {
          const receipt = await memoryApprove({
            request_id: requestId('memory-ui/approve-lifecycle'),
            expected_protocol: protocol(),
            expected_heads: item.expected_heads,
            revision_id: item.revision.revision_id,
            expected_sha256: item.expected_sha256,
            gesture_intent: 'approve',
          })
          destructive = null
          await completed(item.revision.lifecycle.state === 'deleted' ? '已确认删除这条主张' : '已确认将主张移出当前记忆', receipt.projection_rebuilt)
          return
        }
        const input = {
          request_id: requestId(`memory-ui/${destructive.kind}`),
          expected_protocol: protocol(),
          expected_heads: item.expected_heads,
          revision_id: item.revision.revision_id,
          expected_sha256: item.expected_sha256,
          gesture_intent: destructive.kind === 'delete-candidate' ? 'delete' : 'reject',
        } as const
        const receipt = destructive.kind === 'reject' ? await memoryReject(input) : await memoryDeleteCandidate(input)
        destructive = null
        await completed(receipt.effective_status === 'rejected' ? '已否认这条建议' : '已删除候选', receipt.projection_rebuilt)
      }
    } catch (cause) {
      error = hostError(cause)
    } finally {
      writing = false
    }
  }

  async function resetAdd() {
    addTarget = 'user'; addCategory = 'preferences'; addKind = 'preference'; addText = ''; addSpace = 'global'
    addPurposes = ['planning', 'writing', 'information-answer', 'projection', 'sync']; addProviderPolicy = 'prompt'
    addSalience = 'normal'; addPolarity = 'neutral'; addSensitivity = 'normal'; addGuidance = ''; addAvoid = ''
    showAdd = true
    await tick()
    addTextarea?.focus()
  }

  function targetChanged(event: Event) {
    addTarget = (event.currentTarget as HTMLSelectElement).value as 'user' | 'memory'
    const first = categoryOptions[addTarget][0]
    addCategory = first.id
    addKind = first.kind
  }

  function categoryChanged(event: Event) {
    addCategory = (event.currentTarget as HTMLSelectElement).value
    addKind = categoryOptions[addTarget].find((item) => item.id === addCategory)?.kind ?? 'belief'
  }

  function togglePurpose(purpose: string) {
    addPurposes = addPurposes.includes(purpose) ? addPurposes.filter((item) => item !== purpose) : [...addPurposes, purpose]
  }

  async function submitAdd() {
    if (!snapshot?.owner || !addText.trim() || writing) return
    writing = true
    error = ''
    try {
      const receipt = await memoryAdd({
        request_id: requestId('memory-ui/add'),
        expected_protocol: protocol(),
        target: addTarget,
        category: addCategory,
        text: addText.trim(),
        claim_kind: addKind,
        subject: { kind: 'vault-owner', id: snapshot.owner.subject.id, relation_to_owner: 'self' },
        approval_kind: addApproval,
        trust_tier: addKind === 'identity' ? 'identity' : addKind === 'preference' || addKind === 'boundary' ? 'stable-preference' : 'contextual',
        risk_class: riskFor(addKind),
        salience: addSalience,
        polarity: addPolarity,
        sensitivity: addSensitivity,
        context: { spaces: [addSpace.trim() || 'global'], applies_when: [], excludes_when: [] },
        consent: { scope: 'personal-assistant-only', allowed_purposes: addPurposes, external_provider_policy: addProviderPolicy },
        agent_use: { guidance: addGuidance.trim(), avoid_error: addAvoid.trim() },
      })
      showAdd = false
      await completed('主张已保存并批准', receipt.projection_rebuilt)
    } catch (cause) {
      error = hostError(cause)
    } finally {
      writing = false
    }
  }

  async function setClaimSalience(item: EffectiveClaim, salience: Salience) {
    if (writing) return
    writing = true
    error = ''
    try {
      const receipt = await memorySetSalience({
        request_id: requestId('memory-ui/salience'), expected_protocol: protocol(), claim_id: item.claim.claim_id,
        expected_heads: expectedHeads(item.claim), salience,
      })
      openMenuFor = null
      await completed(salience === 'pinned' ? '已标为重要' : '已恢复普通显著性', receipt.projection_rebuilt)
    } catch (cause) { error = hostError(cause) } finally { writing = false }
  }

  async function resolveConflict(conflict: MemoryConflict, strategy: 'keep-head' | 'merge' | 'revoke-all', selectedRevisionId?: string) {
    if (writing) return
    writing = true
    error = ''
    try {
      const receipt = await memoryResolve({
        request_id: requestId('memory-ui/resolve'), expected_protocol: protocol(), conflict_id: conflict.conflict_id,
        claim_id: conflict.claim_id, expected_heads: conflict.heads.map((head) => ({ revision_id: head.revision_id, payload_sha256: head.payload_sha256 })),
        strategy, selected_revision_id: selectedRevisionId, merged_text: strategy === 'merge' ? mergedText.trim() : undefined,
      })
      mergeConflict = null; mergedText = ''
      await completed('冲突已处理', receipt.projection_rebuilt)
    } catch (cause) { error = hostError(cause) } finally { writing = false }
  }

  async function previewContext() {
    contextBusy = true; error = ''; contextPreview = null
    try { contextPreview = await memoryContext(contextRequest()) } catch (cause) { error = hostError(cause) } finally { contextBusy = false }
  }

  function contextRequest() {
    return {
      space: contextSpace, purpose: contextPurpose, caller: 'plugin:notemd.memory', provider: contextProvider,
      model: contextModel.trim(), tools: contextTools.split(',').map((item) => item.trim()).filter(Boolean),
      external_transfer: contextExternal, as_of_valid_time: new Date(contextAsOf).toISOString(),
    }
  }

  async function createManifest() {
    contextBusy = true; error = ''
    try {
      if (!contextPreview) throw new Error('Context preview is required.')
      const receipt = await memoryContextManifest({ ...contextRequest(), preview_sha256: contextPreview.preview_sha256 })
      announcement = `Context Manifest 已记录：${receipt.selected_count} 条主张。`
      await toast('success', announcement, receipt.manifest_id)
    } catch (cause) { error = hostError(cause) } finally { contextBusy = false }
  }

  async function moveListFocus(event: KeyboardEvent, selector: string) {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
    event.preventDefault()
    const items = Array.from((event.currentTarget as HTMLElement).querySelectorAll<HTMLElement>(selector))
    const current = items.indexOf(document.activeElement as HTMLElement)
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : event.key === 'ArrowDown' ? Math.min(items.length - 1, current + 1) : Math.max(0, current - 1)
    items[next]?.focus()
    await tick()
  }

  function moveTab(event: KeyboardEvent) {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
    event.preventDefault()
    const tabs: Tab[] = ['confirmed', 'pending', 'history']
    const current = tabs.indexOf(tab)
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : event.key === 'ArrowRight' ? (current + 1) % tabs.length : (current + tabs.length - 1) % tabs.length
    tab = tabs[next]
    tick().then(() => document.querySelector<HTMLElement>(`[role=tab][data-tab="${tab}"]`)?.focus())
  }

  function closeTopLayer(event: KeyboardEvent) {
    if (event.key !== 'Escape' || writing) return
    if (destructive) destructive = null
    else if (mergeConflict) mergeConflict = null
    else if (showContext) showContext = false
    else if (showAdd) showAdd = false
    else openMenuFor = null
  }
</script>

<svelte:head><title>Memory</title></svelte:head>
<svelte:window onkeydown={closeTopLayer} />

<main>
  <header class="app-header">
    <div><h1>记忆</h1><p>结构化主张保存在 .notemd/memory；USER.md 与 MEMORY.md 只是纯文本视图。</p></div>
    <div class="header-actions">
      <div class="inference-action">
        <button class="primary" onclick={inferMemory} disabled={!canInfer || inferenceStarting || !!inferenceRun} title="首次运行会初始化 Memory v2；Agent 只提交待确认建议，不会自行批准">
          {inferenceStarting ? '正在启动…' : inferenceRun ? '正在推理…' : inferenceMode === 'full' ? '推理现有记忆' : '增量推理记忆'}
        </button>
        {#if agents.length}
          <AgentPicker options={agents} selected={agentId ?? null} disabled={inferenceStarting || !!inferenceRun} onselect={pickAgent} label={agentPickerLabel} />
        {/if}
      </div>
      <button class="secondary" onclick={() => showContext = true} disabled={!snapshot || snapshot.mode !== 'v2'}>Context Manifest…</button>
      <div class="health" class:conflict={snapshot?.health.status === 'conflict'} class:bad={snapshot?.health.status === 'damaged'}>
        <span aria-hidden="true"></span><span>{snapshot?.health.message ?? (loading ? '正在载入' : '不可用')}</span>
      </div>
      <button class="icon" onclick={refresh} disabled={loading} aria-label="刷新记忆">↻</button>
    </div>
  </header>

  <p class="sr-only" aria-live="polite">{announcement}</p>
  {#if error}<div class="banner error" role="alert"><strong>无法完成操作</strong><span>{error}</span></div>{/if}
  {#if inferenceRun}<div class="banner inference-progress" role="status"><strong>{inferenceMode === 'full' ? '全量推理' : '增量推理'}</strong><span>{inferenceProgress}</span></div>{/if}

  {#if loading && !snapshot}
    <div class="empty" aria-busy="true">正在读取 Memory Protocol v2…</div>
  {:else if snapshot}
    {#if snapshot.mode === 'recovery' || snapshot.mode === 'read-only'}
      <div class="banner warning" role="status"><strong>{snapshot.mode === 'recovery' ? '恢复模式' : '只读模式'}</strong><span>{snapshot.read_only_reason ?? '当前 Vault 暂停 Memory 写入；仍可查看现有主张与历史。'}</span></div>
    {/if}
    {#if snapshot.health.integrity_errors.length}
      <div class="banner warning" role="status"><strong>需要维护</strong><span>{snapshot.health.integrity_errors.join('；')}</span></div>
    {/if}

    <div class="segments" role="tablist" tabindex="-1" aria-label="Memory 区域" onkeydown={moveTab}>
      <button role="tab" data-tab="confirmed" aria-selected={tab === 'confirmed'} aria-controls="confirmed-panel" tabindex={tab === 'confirmed' ? 0 : -1} class:active={tab === 'confirmed'} onclick={() => tab = 'confirmed'}>已确认 <span class="count">{snapshot.claims.length}</span></button>
      <button role="tab" data-tab="pending" aria-selected={tab === 'pending'} aria-controls="pending-panel" tabindex={tab === 'pending' ? 0 : -1} class:active={tab === 'pending'} onclick={() => tab = 'pending'}>待确认 <span class="count">{snapshot.pending.length}</span></button>
      <button role="tab" data-tab="history" aria-selected={tab === 'history'} aria-controls="history-panel" tabindex={tab === 'history' ? 0 : -1} class:active={tab === 'history'} onclick={() => tab = 'history'}>冲突与历史 <span class="count">{snapshot.conflicts.length}</span></button>
    </div>

    {#if tab === 'confirmed'}
      <div id="confirmed-panel" role="tabpanel" aria-label="已确认主张">
        <div class="toolbar">
          <input class="search" type="search" bind:value={query} placeholder="搜索已确认主张" aria-label="搜索已确认主张" />
          <select bind:value={target} aria-label="投影位置"><option value="all">全部位置</option><option value="user">USER.md</option><option value="memory">MEMORY.md</option><option value="structured">仅结构化上下文</option></select>
          <button class="primary" onclick={resetAdd} disabled={!writable}>添加主张</button>
        </div>
        <section class="split">
          <div class="master" role="listbox" tabindex="0" aria-label="已确认主张列表" onkeydown={(event) => moveListFocus(event, '[role=option]')}>
            {#each visibleClaims as item (item.claim.claim_id)}
              <button role="option" aria-selected={selectedClaim?.claim.claim_id === item.claim.claim_id} class:selected={selectedClaim?.claim.claim_id === item.claim.claim_id} onclick={() => selectedClaimId = item.claim.claim_id}>
                <span class="polarity {item.claim.polarity}" aria-hidden="true"></span>
                <span><strong>{item.claim.text}</strong><small>{claimKindLabels[item.claim.claim_kind]} · {subjectLabel(item.claim)}</small></span>
                <span class="row-state">{item.claim.salience === 'pinned' ? '重要' : item.claim.epistemic.representation_certainty}</span>
              </button>
            {:else}<div class="empty">没有符合条件的主张。</div>{/each}
          </div>
          {#if selectedClaim}
            {@const claim = selectedClaim.claim}
            <article class="detail" aria-labelledby="claim-title">
              <div class="eyebrow">
                <span class="badge {claim.polarity}">{claim.polarity === 'negative' ? '负向 · 必须避免' : claim.polarity === 'positive' ? '正向 · 建议遵循' : '中性 · 上下文'}</span>
                <span class="badge">{claimKindLabels[claim.claim_kind]}</span>
                {#if claim.salience === 'pinned'}<span class="badge pinned">重要</span>{/if}
                {#if selectedClaim.do_not_rely}<span class="badge danger">不可依赖</span>{/if}
              </div>
              <h2 id="claim-title">{claim.text}</h2>
              {#if claim.agent_use.guidance || claim.agent_use.avoid_error}
                <section class="guidance"><small>Agent 使用方式</small>{#if claim.agent_use.guidance}<p>{claim.agent_use.guidance}</p>{/if}{#if claim.agent_use.avoid_error}<small>必须避免</small><p class="avoid">{claim.agent_use.avoid_error}</p>{/if}</section>
              {/if}
              <dl class="facts">
                <div><dt>关于谁</dt><dd>{subjectLabel(claim)}</dd></div>
                <div><dt>主张类型</dt><dd>{claimKindLabels[claim.claim_kind]}</dd></div>
                <div><dt>由谁表达</dt><dd>{claim.asserted_by.map(actorLabel).join('、')}</dd></div>
                <div><dt>由谁记录</dt><dd>{actorLabel(claim.recorded_by)}</dd></div>
                <div class="wide"><dt>批准含义</dt><dd>{claim.decision?.approval_kind ? approvalLabels[claim.decision.approval_kind].label : '未记录批准含义'}</dd></div>
                <div><dt>长期信任层</dt><dd>{claim.trust_tier}</dd></div>
                <div><dt>误用风险</dt><dd>{claim.risk_class}</dd></div>
                <div><dt>表达忠实度</dt><dd>{claim.epistemic.representation_certainty}</dd></div>
                <div><dt>外部真值</dt><dd>{claim.epistemic.truth_status} · {claim.epistemic.truth_confidence}</dd></div>
                <div class="wide"><dt>有效时间</dt><dd>{temporalLabel(claim)}</dd></div>
                <div class="wide"><dt>Space / 用途</dt><dd>{claim.context.spaces.join('、')} · {claim.consent.allowed_purposes.join('、')}</dd></div>
                <div><dt>Provider</dt><dd>{claim.consent.external_provider_policy}</dd></div>
                <div><dt>纯文本位置</dt><dd>{categoryLabel(claim.projection.target, claim.projection.category)}</dd></div>
                <div class="wide"><dt>Claim / Revision</dt><dd><code>{claim.claim_id}<br />{claim.revision_id}</code></dd></div>
              </dl>
              <div class="detail-actions">
                <div class="menu-anchor">
                  <button aria-haspopup="menu" aria-expanded={openMenuFor === claim.claim_id} onclick={() => openMenuFor = openMenuFor === claim.claim_id ? null : claim.claim_id}>更多</button>
                  {#if openMenuFor === claim.claim_id}
                    <div class="menu-panel" role="menu" aria-label="主张操作">
                      <button class="menu-row" role="menuitem" onclick={() => setClaimSalience(selectedClaim, claim.salience === 'pinned' ? 'normal' : 'pinned')} disabled={!writable || writing}>{claim.salience === 'pinned' ? '恢复普通显著性' : '标为重要'}</button>
                      <button class="menu-row danger-row" role="menuitem" onclick={() => { destructive = { kind: 'delete-claim', current: selectedClaim }; openMenuFor = null }} disabled={!writable || writing}>移出当前记忆…</button>
                    </div>
                  {/if}
                </div>
              </div>
            </article>
          {:else}<div class="empty detail-empty">选择一条主张查看语义。</div>{/if}
        </section>
      </div>
    {:else if tab === 'pending'}
      <div id="pending-panel" class="split" role="tabpanel" aria-label="待确认建议">
        <div class="master" role="listbox" tabindex="0" aria-label="待确认建议列表" onkeydown={(event) => moveListFocus(event, '[role=option]')}>
          {#each reviews as item (item.revision.revision_id)}
            <button role="option" aria-selected={selectedPending?.revision.revision_id === item.revision.revision_id} class:selected={selectedPending?.revision.revision_id === item.revision.revision_id} class:sensitive={item.revision.risk_class === 'action-sensitive'} onclick={() => selectedPendingId = item.revision.revision_id}>
              <span class="polarity {item.revision.polarity}" aria-hidden="true"></span>
              <span><strong>{item.revision.text}</strong><small>{claimKindLabels[item.revision.claim_kind]} · {actorLabel(item.revision.recorded_by)}</small></span>
              <span class="row-state">{item.revision.risk_class === 'action-sensitive' ? '需谨慎' : item.revision.salience}</span>
            </button>
          {:else}<div class="empty">没有待确认建议。</div>{/each}
        </div>
        {#if selectedPending}
          {@const revision = selectedPending.revision}
          {@const approval = approvalForPending(selectedPending)}
          {@const lifecycleChange = revision.lifecycle.state !== 'active'}
          <article class="detail" aria-labelledby="pending-title">
            <div class="eyebrow"><span class="badge">{claimKindLabels[revision.claim_kind]}</span><span class="badge {revision.risk_class === 'action-sensitive' ? 'danger' : ''}">{revision.risk_class}</span></div>
            <h2 id="pending-title">{revision.text}</h2>
            <section class="approval-meaning"><small>这次确认的含义</small><strong>{approvalLabels[approval].label}</strong><p>{approvalLabels[approval].explanation}</p></section>
            {#if lifecycleChange}<div class="banner error" role="status"><strong>这是生命周期变更</strong><span>确认后将{revision.lifecycle.state === 'deleted' ? '删除' : '撤销'}此主张；它会离开当前记忆、纯文本投影和 Agent context，不可变历史仍保留。</span></div>{/if}
            <div class="diff"><div><small>当前</small><p>{selectedPending.base_text ?? '—'}</p></div><div><small>确认后</small><p>{revision.text}</p></div></div>
            <dl class="facts">
              <div><dt>关于谁</dt><dd>{subjectLabel(revision)}</dd></div><div><dt>由谁提出</dt><dd>{actorLabel(revision.recorded_by)}</dd></div>
              <div><dt>Space</dt><dd>{revision.context.spaces.join('、')}</dd></div><div><dt>Provider</dt><dd>{revision.consent.external_provider_policy}</dd></div>
              <div><dt>生命周期</dt><dd>{revision.lifecycle.state}</dd></div>
              <div class="wide"><dt>来源</dt><dd>{selectedPending.source_summary ?? revision.evidence?.[0]?.resource ?? '未提供'}</dd></div>
            </dl>
            <div class="pending-actions">
              <button onclick={() => destructive = { kind: 'delete-candidate', pending: selectedPending }} disabled={!writable || writing}>删除候选…</button>
              <button onclick={() => destructive = { kind: 'reject', pending: selectedPending }} disabled={!writable || writing}>否认…</button>
              <button onclick={() => decidePending(selectedPending, 'ignore')} disabled={!writable || writing}>可以忽略</button>
              {#if !lifecycleChange}<button onclick={() => decidePending(selectedPending, 'approve', 'pinned')} disabled={!writable || writing}>认为重要</button>{/if}
              <button class="primary" onclick={() => lifecycleChange ? destructive = { kind: 'approve-lifecycle', pending: selectedPending } : decidePending(selectedPending, 'approve')} disabled={!writable || writing}>{lifecycleChange ? `确认${revision.lifecycle.state === 'deleted' ? '删除' : '撤销'}…` : approvalLabels[approval].button}</button>
            </div>
          </article>
        {:else}<div class="empty detail-empty">当前没有需要人工处理的建议。</div>{/if}
      </div>
    {:else}
      <div id="history-panel" class="history" role="tabpanel" aria-label="冲突与历史">
        <header><div><h2>并发冲突</h2><p>系统不会按时间替你选择。处理前，相关主张不会授权现实行动。</p></div><span class="count large">{snapshot.conflicts.length}</span></header>
        {#each snapshot.conflicts as conflict (conflict.conflict_id)}
          {@const mixedDecision = new Set(conflict.heads.map((head) => head.workflow.state)).size > 1}
          <article class="conflict-card">
            <div class="conflict-title"><div><span class="badge danger">{conflict.risk_class}</span><h3>{conflict.heads[0]?.text ?? conflict.claim_id}</h3></div><strong>{conflict.action_allowed ? '可用于信息回答' : '禁止行动'}</strong></div>
            {#if conflict.common_ancestor}<div class="ancestor"><small>最后共同版本</small><p>{conflict.common_ancestor.text}</p></div>{/if}
            <div class="heads">
              {#each conflict.heads as head, index (head.revision_id)}
                <section><small>版本 {String.fromCharCode(65 + index)} · {head.workflow.state}/{head.lifecycle.state} · {actorLabel(head.recorded_by)} · {formatDate(head.recorded_at)}</small><p>{head.text}</p><button onclick={() => resolveConflict(conflict, 'keep-head', head.revision_id)} disabled={!writable || writing}>{head.workflow.state === 'approved' ? '采用批准决定' : `采用${head.workflow.state === 'rejected' ? '否认' : '忽略'}决定`}</button></section>
              {/each}
            </div>
            <div class="conflict-actions"><button onclick={() => resolveConflict(conflict, 'revoke-all')} disabled={!writable || writing}>全部撤销</button>{#if !mixedDecision}<button class="primary" onclick={() => { mergeConflict = conflict; mergedText = conflict.heads.map((head) => head.text).join('\n') }} disabled={!writable || writing}>合并编辑…</button>{/if}</div>
          </article>
        {:else}<div class="empty compact">没有并发冲突。</div>{/each}

        <header class="history-heading"><div><h2>历史</h2><p>批准含义、撤销和删除均保留在不可变记录中。</p></div></header>
        <ol class="timeline">
          {#each snapshot.history as item (item.id)}
            <li><span class="timeline-dot" aria-hidden="true"></span><div><strong>{item.summary}</strong><small>{formatDate(item.recorded_at)} · {item.actor_id ?? 'system'} · {item.approval_kind ? approvalLabels[item.approval_kind].label : item.operation}</small></div><span>{item.lifecycle_state}</span></li>
          {:else}<li class="empty compact">尚无历史记录。</li>{/each}
        </ol>
      </div>
    {/if}
  {/if}
</main>

{#if showAdd && snapshot?.owner}
  <div class="scrim" role="presentation">
    <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="add-title">
      <header><div><h2 id="add-title">添加主张</h2><p>保存就是本次人工批准，不会再出现第二步。</p></div><button class="close" aria-label="关闭添加主张" onclick={() => showAdd = false} disabled={writing}>×</button></header>
      <div class="form-grid"><label>纯文本位置<select value={addTarget} onchange={targetChanged}><option value="user">USER.md</option><option value="memory">MEMORY.md</option></select></label><label>一级分类<select value={addCategory} onchange={categoryChanged}>{#each currentCategoryOptions as option}<option value={option.id}>{option.label}</option>{/each}</select></label></div>
      <label class="field">主张内容<textarea bind:this={addTextarea} bind:value={addText} rows="5" placeholder="一条可以独立确认或否认的多行主张"></textarea></label>
      <section class="approval-meaning"><small>保存的批准含义</small><strong>{approvalLabels[addApproval].label}</strong><p>{approvalLabels[addApproval].explanation}</p></section>
      <details><summary>使用范围与高级信息</summary>
        <div class="form-grid"><label>主张类型<select bind:value={addKind}>{#each Object.entries(claimKindLabels) as [id, label]}<option value={id}>{label}</option>{/each}</select></label><label>Space<input bind:value={addSpace} placeholder="global 或 project/name" /></label><label>Provider 策略<select bind:value={addProviderPolicy}><option value="deny">禁止外发</option><option value="prompt">每次询问</option><option value="allow">允许</option></select></label><label>敏感度<select bind:value={addSensitivity}><option value="normal">普通</option><option value="private">私人</option></select></label><label>显著性<select bind:value={addSalience}><option value="normal">普通</option><option value="pinned">重要</option></select></label><label>方向<select bind:value={addPolarity}><option value="neutral">中性</option><option value="positive">正向</option><option value="negative">负向</option></select></label></div>
        <fieldset><legend>允许用途</legend>{#each ['planning', 'writing', 'information-answer', 'external-action', 'projection', 'sync'] as purpose}<label class="check"><input type="checkbox" checked={addPurposes.includes(purpose)} onchange={() => togglePurpose(purpose)} />{purpose}</label>{/each}</fieldset>
        <label class="field">Agent 使用方式<input bind:value={addGuidance} placeholder="在什么情况下如何使用" /></label><label class="field">必须避免{addNeedsAvoid ? '（必填）' : ''}<input bind:value={addAvoid} placeholder="不能做出的推断或动作" /></label>
      </details>
      <footer><button onclick={() => showAdd = false} disabled={writing}>取消</button><button class="primary" onclick={submitAdd} disabled={writing || !addText.trim() || !addPurposes.length || (addNeedsAvoid && !addAvoid.trim())}>{writing ? '正在保存…' : '保存并确认'}</button></footer>
    </div>
  </div>
{/if}

{#if destructive}
  <div class="scrim" role="presentation"><div class="sheet compact-sheet" role="alertdialog" aria-modal="true" aria-labelledby="destructive-title"><header><div><h2 id="destructive-title">{destructive.kind === 'reject' ? '否认这条建议？' : destructive.kind === 'delete-candidate' ? '删除这个候选？' : destructive.kind === 'approve-lifecycle' ? `确认${destructive.pending.revision.lifecycle.state === 'deleted' ? '删除' : '撤销'}此主张？` : '移出当前记忆？'}</h2><p>{destructive.kind === 'delete-claim' ? '主张会从当前投影和 Agent context 移除；不可变历史仍会保留。这不是从 Git 历史永久擦除。' : destructive.kind === 'approve-lifecycle' ? '这是不可见性变更：主张会离开当前投影和 Agent context；不可变历史仍会保留。' : destructive.kind === 'reject' ? '系统会记录否认决定，建议不会进入当前记忆。' : '候选不会进入当前记忆；已有 Git 历史可能继续保留。'}</p></div></header><footer><button onclick={() => destructive = null} disabled={writing}>取消</button><button class="destructive" onclick={confirmDestructive} disabled={writing}>{writing ? '正在处理…' : '确认'}</button></footer></div></div>
{/if}

{#if mergeConflict}
  <div class="scrim" role="presentation"><div class="sheet" role="dialog" aria-modal="true" aria-labelledby="merge-title"><header><div><h2 id="merge-title">合并并发版本</h2><p>合并会创建同时引用所有当前 head 的新 revision。</p></div><button class="close" aria-label="关闭合并编辑" onclick={() => mergeConflict = null}>×</button></header><label class="field">合并后的主张<textarea bind:value={mergedText} rows="7"></textarea></label><footer><button onclick={() => mergeConflict = null}>取消</button><button class="primary" onclick={() => resolveConflict(mergeConflict!, 'merge')} disabled={writing || !mergedText.trim()}>保存合并</button></footer></div></div>
{/if}

{#if showContext && snapshot}
  <div class="scrim" role="presentation"><div class="sheet context-sheet" role="dialog" aria-modal="true" aria-labelledby="context-title"><header><div><h2 id="context-title">Context Manifest</h2><p>先明确 Space、用途和接收方，再查看将提供给 Agent 的最小主张集合。</p></div><button class="close" aria-label="关闭 Context Manifest" onclick={() => showContext = false}>×</button></header>
    <div class="form-grid"><label>Space<select bind:value={contextSpace}>{#each snapshot.context_options?.spaces ?? [{ id: 'global', label: '全局' }] as option}<option value={option.id}>{option.label}</option>{/each}</select></label><label>用途<select bind:value={contextPurpose}>{#each snapshot.context_options?.purposes ?? [{ id: 'planning', label: '规划' }] as option}<option value={option.id}>{option.label}</option>{/each}</select></label><label>Provider<select bind:value={contextProvider}>{#each snapshot.context_options?.providers ?? [{ id: 'openai', label: 'OpenAI' }] as option}<option value={option.id}>{option.label}</option>{/each}</select></label><label>Model<select bind:value={contextModel}>{#each (snapshot.context_options?.models ?? [{ id: 'gpt-5', label: 'GPT-5' }]).filter((option) => !option.provider_id || option.provider_id === contextProvider) as option}<option value={option.id}>{option.label}</option>{/each}</select></label><label>有效时间<input type="datetime-local" bind:value={contextAsOf} /></label><label>Tools（逗号分隔）<input bind:value={contextTools} /></label><label class="check wide"><input type="checkbox" bind:checked={contextExternal} />内容将发送给外部 Provider</label></div>
    <div class="context-actions"><button class="primary" onclick={previewContext} disabled={contextBusy || !contextSpace || !contextPurpose || !contextProvider || !contextModel || !contextAsOf}>{contextBusy ? '正在检查…' : '预览选择'}</button></div>
    {#if contextPreview}<section class="context-result" aria-live="polite"><div class="result-summary"><strong>{contextPreview.selected.length} 条将被选中</strong><span>{contextPreview.redactions} 条已脱敏 · {contextPreview.conflicts.length} 个冲突</span></div><dl class="facts"><div><dt>现实行动</dt><dd>{contextPreview.policy_result.external_action_allowed ? '允许' : '不允许'}</dd></div><div><dt>排除</dt><dd>{Object.entries(contextPreview.excluded_summary).map(([key, value]) => `${key} ${value}`).join('、') || '无'}</dd></div></dl><ul>{#each contextPreview.selected as item}<li>{item.text ?? item.claim_id}<small>{item.reasons.join(' · ')}</small></li>{/each}</ul><button onclick={createManifest} disabled={contextBusy || !writable || !contextPreview.preview_sha256}>记录本次使用清单</button>{#if !writable}<small class="readonly-note">当前模式只允许预览，不会写入 Context Manifest。</small>{/if}</section>{/if}
  </div></div>
{/if}

<style>
  :global(:root){color-scheme:light dark;font:13px/1.45 -apple-system,BlinkMacSystemFont,"SF Pro Text",sans-serif;accent-color:#0a84ff}:global(body){margin:0;background:Canvas;color:CanvasText}:global(*){box-sizing:border-box}:global(button),:global(input),:global(select),:global(textarea),:global(summary){font:inherit;pointer-events:auto;-webkit-app-region:no-drag}:global(button){min-height:32px;border:1px solid color-mix(in srgb,CanvasText 14%,transparent);border-radius:8px;padding:0 12px;background:color-mix(in srgb,CanvasText 5%,Canvas);color:CanvasText;cursor:pointer}:global(button:hover:not(:disabled)){background:color-mix(in srgb,CanvasText 10%,Canvas)}:global(button:active:not(:disabled)){transform:translateY(1px)}:global(button:focus-visible),:global(input:focus-visible),:global(select:focus-visible),:global(textarea:focus-visible),:global(summary:focus-visible){outline:3px solid color-mix(in srgb,#0a84ff 34%,transparent);outline-offset:2px}:global(button:disabled){opacity:.42;cursor:default}:global(button.primary){min-height:34px;background:#0a84ff;border-color:#0a84ff;color:white;font-weight:600}:global(button.primary:hover:not(:disabled)){background:#0077ed}:global(button.destructive){background:#ff3b30;border-color:#ff3b30;color:#fff;font-weight:600}:global(input),:global(select),:global(textarea){width:100%;min-height:32px;padding:6px 9px;border:1px solid color-mix(in srgb,CanvasText 16%,transparent);border-radius:7px;background:Canvas;color:CanvasText}:global(textarea){resize:vertical;line-height:1.5}.sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}
  main{max-width:1240px;margin:0 auto;padding:20px 24px 40px}.app-header{display:flex;justify-content:space-between;align-items:flex-start;gap:20px}.app-header h1{margin:0;font-size:20px;line-height:1.25;letter-spacing:-.02em}.app-header p{max-width:680px;margin:4px 0 0;font-size:12px;color:color-mix(in srgb,CanvasText 58%,transparent)}.header-actions{display:flex;align-items:center;gap:8px}.inference-action{display:flex;align-items:center;gap:4px}.health{display:flex;align-items:center;gap:7px;max-width:250px;color:color-mix(in srgb,CanvasText 62%,transparent);font-size:12px}.health>span:first-child{flex:0 0 8px;width:8px;height:8px;border-radius:50%;background:#34c759}.health.conflict>span:first-child{background:#ff9f0a}.health.bad>span:first-child{background:#ff3b30}.icon{width:32px;padding:0;font-size:17px}.secondary{background:transparent}.banner{display:flex;align-items:flex-start;gap:10px;margin-top:14px;padding:10px 12px;border:1px solid;border-radius:9px;font-size:12px}.banner strong{white-space:nowrap}.banner.error{border-color:color-mix(in srgb,#ff3b30 42%,transparent);background:color-mix(in srgb,#ff3b30 9%,Canvas)}.banner.warning{border-color:color-mix(in srgb,#ff9f0a 45%,transparent);background:color-mix(in srgb,#ff9f0a 9%,Canvas)}.banner.inference-progress{border-color:color-mix(in srgb,#0a84ff 36%,transparent);background:color-mix(in srgb,#0a84ff 7%,Canvas)}
  .segments{display:grid;grid-template-columns:repeat(3,1fr);gap:3px;max-width:560px;margin:18px auto 16px;padding:3px;border-radius:9px;background:color-mix(in srgb,CanvasText 8%,Canvas)}.segments button{min-height:34px;border:0;background:transparent;color:color-mix(in srgb,CanvasText 65%,transparent);font-weight:600}.segments button.active{background:Canvas;color:CanvasText;box-shadow:0 1px 4px rgba(0,0,0,.16)}.count{display:inline-grid;place-items:center;min-width:18px;height:18px;padding:0 5px;border-radius:9px;background:color-mix(in srgb,CanvasText 10%,Canvas);font-size:11px}.count.large{min-width:28px;height:24px;border-radius:12px}.toolbar{display:grid;grid-template-columns:minmax(220px,1fr) 170px auto;gap:8px;margin-bottom:12px}.split{display:grid;grid-template-columns:350px minmax(0,1fr);min-height:510px;border:1px solid color-mix(in srgb,CanvasText 13%,transparent);border-radius:11px;overflow:hidden;background:color-mix(in srgb,CanvasText 1.5%,Canvas)}
  .master{max-height:calc(100vh - 220px);min-height:510px;overflow:auto;border-right:1px solid color-mix(in srgb,CanvasText 12%,transparent);background:color-mix(in srgb,CanvasText 3%,Canvas)}.master>button{display:grid;grid-template-columns:8px minmax(0,1fr) auto;align-items:start;gap:9px;width:100%;min-height:70px;padding:11px 12px;border:0;border-bottom:1px solid color-mix(in srgb,CanvasText 8%,transparent);border-radius:0;background:transparent;text-align:left}.master>button.selected{background:#0a84ff;color:#fff}.master>button.selected small,.master>button.selected .row-state{color:rgba(255,255,255,.8)}.master>button.sensitive{box-shadow:inset 3px 0 #ff9f0a}.master strong{display:-webkit-box;overflow:hidden;line-clamp:2;-webkit-line-clamp:2;-webkit-box-orient:vertical;font-size:13px;line-height:1.4;white-space:pre-wrap}.master small{display:block;margin-top:4px;color:color-mix(in srgb,CanvasText 53%,transparent);font-size:12px}.row-state{color:color-mix(in srgb,CanvasText 55%,transparent);font-size:11px}.polarity{width:7px;height:7px;margin-top:5px;border-radius:50%;background:#8e8e93}.polarity.positive{background:#34c759}.polarity.negative{background:#ff3b30}
  .detail{min-width:0;padding:22px 24px}.detail h2{margin:12px 0 16px;font-size:17px;line-height:1.5;letter-spacing:-.01em;white-space:pre-wrap}.eyebrow{display:flex;flex-wrap:wrap;gap:6px}.badge{padding:3px 7px;border-radius:6px;background:color-mix(in srgb,CanvasText 7%,Canvas);font-size:11px;font-weight:650}.badge.positive{background:color-mix(in srgb,#34c759 18%,Canvas);color:#168333}.badge.negative,.badge.danger{background:color-mix(in srgb,#ff3b30 16%,Canvas);color:#d62d26}.badge.pinned{background:color-mix(in srgb,#ff9f0a 20%,Canvas);color:#9a5500}.guidance{margin-bottom:16px;padding:13px 14px;border-left:3px solid #0a84ff;border-radius:0 8px 8px 0;background:color-mix(in srgb,#0a84ff 7%,Canvas)}.guidance small,.approval-meaning small{font-size:11px;font-weight:700;color:#0a84ff}.guidance p{margin:4px 0 9px;font-size:13px}.guidance p:last-child{margin-bottom:0}.guidance .avoid{color:#d62d26;font-weight:600}.facts{display:grid;grid-template-columns:1fr 1fr;gap:1px;margin:0;background:color-mix(in srgb,CanvasText 8%,transparent);border:1px solid color-mix(in srgb,CanvasText 8%,transparent);border-radius:8px;overflow:hidden}.facts>div{display:flex;justify-content:space-between;gap:12px;padding:9px 10px;background:Canvas}.facts>div.wide{grid-column:1/-1}.facts dt{font-size:12px;color:color-mix(in srgb,CanvasText 54%,transparent)}.facts dd{margin:0;text-align:right;font-size:12px;overflow-wrap:anywhere}.detail-actions{display:flex;justify-content:flex-end;margin-top:16px}.menu-anchor{position:relative}.menu-panel{position:absolute;right:0;bottom:40px;z-index:20;min-width:190px}.menu-panel .menu-row{width:100%;min-height:30px;border:0;text-align:left}.menu-panel .danger-row{color:#ff3b30}.menu-panel .danger-row:hover{background:#ff3b30;color:#fff}.approval-meaning{margin:0 0 14px;padding:12px 13px;border-radius:9px;background:color-mix(in srgb,#0a84ff 7%,Canvas)}.approval-meaning strong{display:block;margin-top:3px;font-size:14px}.approval-meaning p{margin:4px 0 0;color:color-mix(in srgb,CanvasText 66%,transparent);font-size:12px}.diff{display:grid;grid-template-columns:1fr 1fr;gap:9px;margin:12px 0 16px}.diff>div{min-height:92px;padding:11px;border-radius:8px;background:color-mix(in srgb,CanvasText 5%,Canvas)}.diff small{font-size:11px;color:color-mix(in srgb,CanvasText 54%,transparent)}.diff p{margin:5px 0 0;white-space:pre-wrap}.pending-actions{display:flex;flex-wrap:wrap;justify-content:flex-end;gap:8px;margin-top:16px}
  .history{display:grid;gap:12px}.history>header{display:flex;justify-content:space-between;align-items:center}.history h2{margin:0;font-size:16px}.history header p{margin:3px 0 0;color:color-mix(in srgb,CanvasText 58%,transparent);font-size:12px}.conflict-card{padding:16px;border:1px solid color-mix(in srgb,#ff9f0a 42%,transparent);border-radius:11px;background:color-mix(in srgb,#ff9f0a 5%,Canvas)}.conflict-title{display:flex;justify-content:space-between;gap:18px}.conflict-title h3{margin:7px 0 0;font-size:15px}.conflict-title>strong{color:#b36300;font-size:12px}.ancestor{margin:12px 0;padding:10px 11px;border-radius:8px;background:color-mix(in srgb,CanvasText 5%,Canvas)}.ancestor small,.heads small{font-size:11px;color:color-mix(in srgb,CanvasText 55%,transparent)}.ancestor p,.heads p{margin:4px 0 0;white-space:pre-wrap}.heads{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:9px}.heads section{padding:11px;border:1px solid color-mix(in srgb,CanvasText 11%,transparent);border-radius:9px;background:Canvas}.heads button{margin-top:10px}.conflict-actions{display:flex;justify-content:flex-end;gap:8px;margin-top:12px}.history-heading{margin-top:12px}.timeline{margin:0;padding:0;list-style:none;border:1px solid color-mix(in srgb,CanvasText 10%,transparent);border-radius:10px;overflow:hidden}.timeline li{display:grid;grid-template-columns:10px minmax(0,1fr) auto;gap:10px;align-items:start;padding:11px 13px;border-bottom:1px solid color-mix(in srgb,CanvasText 8%,transparent)}.timeline li:last-child{border-bottom:0}.timeline-dot{width:8px;height:8px;margin-top:5px;border-radius:50%;background:#0a84ff}.timeline strong{display:block;font-size:13px}.timeline small{display:block;margin-top:2px;color:color-mix(in srgb,CanvasText 55%,transparent);font-size:12px}.timeline>li>span:last-child{font-size:11px}.empty{padding:52px 18px;text-align:center;color:color-mix(in srgb,CanvasText 48%,transparent)}.empty.compact{padding:22px}.detail-empty{display:grid;place-items:center}
  .scrim{position:fixed;inset:0;z-index:100;display:grid;place-items:center;padding:24px;background:rgba(0,0,0,.28);backdrop-filter:blur(8px)}.sheet{box-sizing:border-box;width:min(680px,100%);max-height:calc(100vh - 48px);overflow:auto;padding:22px;border:1px solid color-mix(in srgb,CanvasText 15%,transparent);border-radius:14px;background:Canvas;color:CanvasText;box-shadow:0 24px 70px rgba(0,0,0,.34)}.compact-sheet{width:min(470px,100%)}.context-sheet{width:min(760px,100%)}.sheet>header{display:flex;justify-content:space-between;gap:18px}.sheet h2{margin:0;font-size:17px}.sheet header p{margin:4px 0 0;color:color-mix(in srgb,CanvasText 62%,transparent);font-size:12px}.close{width:32px;padding:0;border:0;background:transparent;font-size:20px}.field,.form-grid label{display:grid;gap:4px;margin-top:12px;color:color-mix(in srgb,CanvasText 68%,transparent);font-size:12px}.form-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:0 10px}.form-grid .wide{grid-column:1/-1}details{margin-top:14px;padding:10px 12px;border:1px solid color-mix(in srgb,CanvasText 10%,transparent);border-radius:9px}summary{cursor:pointer;font-weight:600}fieldset{display:flex;flex-wrap:wrap;gap:10px;margin:14px 0 0;padding:10px;border:1px solid color-mix(in srgb,CanvasText 10%,transparent);border-radius:8px}legend{padding:0 4px;font-size:12px}.check{display:flex!important;align-items:center;gap:6px!important;margin:0!important}.check input{width:auto;min-height:auto}.sheet footer{display:flex;justify-content:flex-end;gap:8px;margin-top:18px}.context-actions{display:flex;justify-content:flex-end;margin-top:14px}.context-result{margin-top:14px;padding:13px;border-radius:10px;background:color-mix(in srgb,CanvasText 4%,Canvas)}.result-summary{display:flex;justify-content:space-between}.context-result ul{margin:10px 0;padding-left:20px}.context-result li{margin:5px 0}.context-result li small,.readonly-note{display:block;color:color-mix(in srgb,CanvasText 55%,transparent);font-size:12px}
  @media(max-width:820px){main{padding:16px}.split{grid-template-columns:290px minmax(0,1fr)}.header-actions{flex-wrap:wrap;justify-content:flex-end}.heads{grid-template-columns:1fr}}@media(max-width:660px){.app-header{display:block}.header-actions{justify-content:flex-start;margin-top:10px}.split{display:block}.master{min-height:180px;max-height:280px;border-right:0;border-bottom:1px solid color-mix(in srgb,CanvasText 12%,transparent)}.toolbar,.form-grid,.diff,.facts{grid-template-columns:1fr}.facts>div.wide,.form-grid .wide{grid-column:auto}.segments{max-width:none}.pending-actions{justify-content:stretch}.pending-actions button{flex:1 1 120px}}
  @media(prefers-reduced-motion:reduce){:global(*){scroll-behavior:auto!important;transition:none!important}}
</style>
