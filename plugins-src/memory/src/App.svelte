<script lang="ts">
  import { onMount, tick } from 'svelte'
  import AgentPicker from './lib/agent-picker/AgentPicker.svelte'
  import RoleScopeManager from './lib/RoleScopeManager.svelte'
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
    memoryContextRegistry,
    memoryDelete,
    memoryDeleteCandidate,
    memoryIgnore,
    memoryInitialize,
    memoryReject,
    memoryReplace,
    memoryResetAll,
    memoryResolve,
    memorySetSalience,
    memorySnapshot,
    clipboardWrite,
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
    groupCurrentClaims,
    expectedHeads,
    formatDate,
    hostError,
    pendingClaims,
    riskFor,
    subjectLabel,
    temporalLabel,
    type ConfirmedSort,
  } from './lib/domain'
  import { importPrompt } from './lib/importPrompt'
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
    ContextRegistrySnapshot,
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
  type BulkPendingAction = 'ignore' | 'reject' | 'delete-candidate'
  type DestructiveAction =
    | { kind: 'reject' | 'delete-candidate' | 'approve-lifecycle'; pending: PendingClaim }
    | { kind: 'batch-pending'; action: BulkPendingAction; pending: PendingClaim[] }
    | { kind: 'delete-claim'; current: EffectiveClaim }
  type EditTarget = { kind: 'confirmed'; current: EffectiveClaim } | { kind: 'pending'; pending: PendingClaim }

  let snapshot = $state<MemorySnapshotV2 | null>(null)
  let loading = $state(true)
  let writing = $state(false)
  let error = $state('')
  let announcement = $state('')
  let tab = $state<Tab>('confirmed')
  let query = $state('')
  let target = $state<'all' | ProjectionTarget | 'structured'>('all')
  let confirmedSort = $state<ConfirmedSort>('priority')
  let selectedClaimId = $state<string | null>(null)
  let selectedPendingId = $state<string | null>(null)
  let selectedPendingIds = $state<string[]>([])
  let pendingAnchorId = $state<string | null>(null)
  let pendingContextMenu = $state<{ x: number; y: number } | null>(null)
  let showAdd = $state(false)
  let showContext = $state(false)
  let showRoleScopeManager = $state(false)
  let roleScopeBusy = $state(false)
  let showReset = $state(false)
  let openMenuFor = $state<string | null>(null)
  let destructive = $state<DestructiveAction | null>(null)
  let editTarget = $state<EditTarget | null>(null)
  let editText = $state('')
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
  let addRole = $state('role:unclassified')
  let contextRegistry = $state<ContextRegistrySnapshot | null>(null)
  let addPurposes = $state<string[]>(['planning', 'writing', 'information-answer', 'projection', 'sync'])
  let addProviderPolicy = $state<ProviderPolicy>('prompt')
  let addSalience = $state<Salience>('normal')
  let addPolarity = $state<'positive' | 'negative' | 'neutral'>('neutral')
  let addSensitivity = $state<'normal' | 'private'>('normal')
  let addGuidance = $state('')
  let addAvoid = $state('')

  let contextSpace = $state('global')
  let contextRole = $state('role:unclassified')
  let contextPurpose = $state('planning')
  let contextProvider = $state('openai')
  let contextModel = $state('gpt-5')
  let contextTools = $state('notemd-search')
  let contextExternal = $state(true)
  let contextAsOf = $state(new Date().toISOString().slice(0, 16))
  let contextPreview = $state<ContextPreview | null>(null)
  let contextBusy = $state(false)
  let addTextarea = $state<HTMLTextAreaElement>()
  let editTextarea = $state<HTMLTextAreaElement>()

  const writable = $derived(snapshot?.mode === 'v2' && snapshot.health.status !== 'damaged' && snapshot.health.status !== 'unsupported')
  const canInfer = $derived(writable || snapshot?.initialization_required === true)
  const visibleClaims = $derived(snapshot ? currentClaims(snapshot.claims, query, target, confirmedSort) : [])
  const confirmedGroups = $derived(groupCurrentClaims(visibleClaims))
  const reviews = $derived(snapshot ? pendingClaims(snapshot.pending) : [])
  const selectedClaim = $derived(visibleClaims.find(({ claim }) => claim.claim_id === selectedClaimId) ?? visibleClaims[0])
  const selectedPendingItems = $derived(reviews.filter(({ revision }) => selectedPendingIds.includes(revision.revision_id)))
  const selectedPending = $derived(reviews.find(({ revision }) => revision.revision_id === selectedPendingId) ?? selectedPendingItems[0])
  const availableRoles = $derived(contextRegistry?.roles?.filter((item) => item.status === 'active') ?? [])
  const availableScopes = $derived(contextRegistry?.scopes?.filter((item) => item.status === 'active')
    ?? snapshot?.context_options?.spaces.map((item) => ({ ...item, status: 'active' as const }))
    ?? [])
  const currentCategoryOptions = $derived(categoryOptions[addTarget])
  const addApproval = $derived(approvalKindFor(addKind))
  const addNeedsAvoid = $derived(addKind === 'boundary' || addPolarity === 'negative' || addKind === 'practice')
  const resetClaims = $derived(snapshot ? [
    ...snapshot.claims
      .filter(({ claim }) => claim.workflow.state === 'approved' && claim.lifecycle.state === 'active')
      .map(({ claim }) => ({ claim_id: claim.claim_id, expected_heads: expectedHeads(claim) })),
    ...snapshot.conflicts.map((conflict) => ({
      claim_id: conflict.claim_id,
      expected_heads: conflict.heads.map((head) => ({ revision_id: head.revision_id, payload_sha256: head.payload_sha256 })),
    })),
  ] : [])

  onMount(() => {
    void refresh()
    void refreshContextRegistry()
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
      inferenceProgress = ''
      tab = 'pending'
      await refresh()
      await loadInferenceMode()
      if (!view.success) {
        error = view.message ? `记忆推理失败：${view.message}` : '记忆推理失败；未推进增量水位。'
        await toast('error', '记忆推理失败', view.message || '未推进增量水位。')
        return
      }
      const checkpointed = await completedInference(current.invocationId)
      if (!checkpointed) {
        const detail = view.message || 'Agent 未写入与本次调用匹配的成功水位。'
        error = `记忆推理失败：${detail}`
        await toast('error', '记忆推理失败', detail)
        return
      }
      announcement = '记忆推理完成；新建议已放入待确认。'
      await toast('success', announcement)
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
      const orderedPending = pendingClaims(snapshot.pending)
      const availablePendingIds = new Set(orderedPending.map(({ revision }) => revision.revision_id))
      selectedPendingIds = selectedPendingIds.filter((id) => availablePendingIds.has(id))
      if (!selectedPendingIds.length && orderedPending[0]) selectedPendingIds = [orderedPending[0].revision.revision_id]
      if (!selectedPendingId || !selectedPendingIds.includes(selectedPendingId)) selectedPendingId = selectedPendingIds[0] ?? null
      if (pendingAnchorId && !availablePendingIds.has(pendingAnchorId)) pendingAnchorId = null
      pendingContextMenu = null
      contextSpace = snapshot.context_options?.spaces[0]?.id ?? contextSpace
      contextRole = snapshot.context_options?.roles?.[0]?.id ?? contextRole
      contextPurpose = snapshot.context_options?.purposes[0]?.id ?? contextPurpose
      contextProvider = snapshot.context_options?.providers[0]?.id ?? contextProvider
      contextModel = snapshot.context_options?.models.find((option) => !option.provider_id || option.provider_id === contextProvider)?.id ?? contextModel
    } catch (cause) {
      error = hostError(cause)
    } finally {
      loading = false
    }
  }

  async function refreshContextRegistry() {
    try {
      contextRegistry = await memoryContextRegistry()
    } catch (cause) {
      error = hostError(cause)
    }
  }

  async function roleScopeChanged() {
    await Promise.all([refresh(), refreshContextRegistry()])
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

  function selectPending(item: PendingClaim, event: MouseEvent) {
    const id = item.revision.revision_id
    const additive = event.metaKey || event.ctrlKey
    if (event.shiftKey && pendingAnchorId) {
      const anchorIndex = reviews.findIndex(({ revision }) => revision.revision_id === pendingAnchorId)
      const itemIndex = reviews.findIndex(({ revision }) => revision.revision_id === id)
      if (anchorIndex >= 0 && itemIndex >= 0) {
        const start = Math.min(anchorIndex, itemIndex)
        const end = Math.max(anchorIndex, itemIndex)
        const range = reviews.slice(start, end + 1).map(({ revision }) => revision.revision_id)
        selectedPendingIds = additive ? reviews.filter(({ revision }) => selectedPendingIds.includes(revision.revision_id) || range.includes(revision.revision_id)).map(({ revision }) => revision.revision_id) : range
        selectedPendingId = id
        pendingContextMenu = null
        return
      }
    }
    if (additive) {
      selectedPendingIds = selectedPendingIds.includes(id) ? selectedPendingIds.filter((candidate) => candidate !== id) : [...selectedPendingIds, id]
      selectedPendingId = selectedPendingIds.includes(id) ? id : selectedPendingIds[0] ?? null
    } else {
      selectedPendingIds = [id]
      selectedPendingId = id
    }
    pendingAnchorId = id
    pendingContextMenu = null
  }

  function pendingMenuPosition(x: number, y: number) {
    return {
      x: Math.max(8, Math.min(x, window.innerWidth - 238)),
      y: Math.max(8, Math.min(y, window.innerHeight - 170)),
    }
  }

  function openPendingContextMenu(item: PendingClaim, event: MouseEvent) {
    event.preventDefault()
    event.stopPropagation()
    const id = item.revision.revision_id
    if (!selectedPendingIds.includes(id)) {
      selectedPendingIds = [id]
      pendingAnchorId = id
    }
    selectedPendingId = id
    pendingContextMenu = pendingMenuPosition(event.clientX, event.clientY)
  }

  function openPendingContextMenuFromKeyboard(item: PendingClaim, event: KeyboardEvent) {
    if (event.key !== 'ContextMenu' && !(event.shiftKey && event.key === 'F10')) return
    event.preventDefault()
    event.stopPropagation()
    const target = event.currentTarget as HTMLElement
    const rect = target.getBoundingClientRect()
    openPendingContextMenu(item, new MouseEvent('contextmenu', { clientX: rect.left + 24, clientY: rect.top + 24 }))
  }

  function openSelectedPendingMenu(event: MouseEvent) {
    event.stopPropagation()
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
    pendingContextMenu = pendingMenuPosition(rect.right - 220, rect.bottom + 6)
  }

  function startPendingBatch(action: BulkPendingAction) {
    const pending = reviews.filter(({ revision }) => selectedPendingIds.includes(revision.revision_id))
    pendingContextMenu = null
    if (!pending.length || writing) return
    destructive = { kind: 'batch-pending', action, pending }
  }

  function batchActionText(action: BulkPendingAction) {
    return action === 'ignore' ? '忽略' : action === 'reject' ? '否认' : '删除候选'
  }

  async function confirmDestructive() {
    if (!destructive || writing) return
    writing = true
    error = ''
    try {
      if (destructive.kind === 'batch-pending') {
        const { action, pending } = destructive
        const batchId = requestId(`memory-ui/batch-${action}`)
        const expectedProtocol = protocol()
        let completedCount = 0
        let projectionRebuilt = true
        let failure: unknown
        for (const [index, item] of pending.entries()) {
          const input = {
            request_id: `${batchId}/${index}`,
            expected_protocol: expectedProtocol,
            expected_heads: item.expected_heads,
            revision_id: item.revision.revision_id,
            expected_sha256: item.expected_sha256,
            gesture_intent: action === 'delete-candidate' ? 'delete' : action,
          } as const
          try {
            const receipt = action === 'ignore'
              ? await memoryIgnore(input)
              : action === 'reject'
                ? await memoryReject(input)
                : await memoryDeleteCandidate(input)
            completedCount += 1
            projectionRebuilt &&= receipt.projection_rebuilt
          } catch (cause) {
            failure = cause
            break
          }
        }
        destructive = null
        await refresh()
        if (failure) {
          const remaining = pending.length - completedCount
          error = `批量${batchActionText(action)}已完成 ${completedCount} 条，剩余 ${remaining} 条未处理。${hostError(failure)}`
          announcement = error
          await toast('warn', `批量${batchActionText(action)}未全部完成`, `已完成 ${completedCount} 条，剩余 ${remaining} 条未处理。`)
        } else {
          announcement = `已批量${batchActionText(action)} ${completedCount} 条建议${projectionRebuilt ? '' : '；纯文本投影等待重建。'}`
          await toast(projectionRebuilt ? 'success' : 'warn', announcement)
        }
      } else if (destructive.kind === 'delete-claim') {
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

  async function resetAllMemory() {
    if (!snapshot || writing || inferenceRun) return
    writing = true
    error = ''
    try {
      const receipt = await memoryResetAll({
        request_id: requestId('memory-ui/reset-all'),
        expected_protocol: protocol(),
        gesture_intent: 'reset-all',
        expected_claims: resetClaims,
        expected_pending: snapshot.pending.map((item) => ({
          revision_id: item.revision.revision_id,
          expected_sha256: item.expected_sha256,
          expected_heads: item.expected_heads,
        })),
      })
      showReset = false
      selectedClaimId = null
      selectedPendingId = null
      selectedPendingIds = []
      pendingAnchorId = null
      pendingContextMenu = null
      inferenceMode = 'full'
      await completed(
        `已清理 ${receipt.deleted_claims} 条已确认记忆和 ${receipt.deleted_pending} 条待确认建议；下次将重新全量推理`,
        receipt.projection_rebuilt,
      )
      await loadInferenceMode()
    } catch (cause) {
      error = hostError(cause)
    } finally {
      writing = false
    }
  }

  async function copyImportPrompt() {
    try {
      await clipboardWrite(importPrompt)
      announcement = '已复制导入记忆 Prompt'
      await toast('success', '已复制导入记忆 Prompt', '粘贴给其他助手后，请先审阅它生成的 bash 命令，再在 vault 中运行。')
    } catch (failure) {
      error = hostError(failure)
    }
  }

  async function resetAdd() {
    addTarget = 'user'; addCategory = 'preferences'; addKind = 'preference'; addText = ''
    addRole = availableRoles[0]?.id ?? 'role:unclassified'
    addSpace = availableScopes[0]?.id ?? ''
    addPurposes = ['planning', 'writing', 'information-answer', 'projection', 'sync']; addProviderPolicy = 'prompt'
    addSalience = 'normal'; addPolarity = 'neutral'; addSensitivity = 'normal'; addGuidance = ''; addAvoid = ''
    showAdd = true
    if (!contextRegistry) void refreshContextRegistry()
    await tick()
    addTextarea?.focus()
  }

  function targetChanged(event: Event) {
    addTarget = (event.currentTarget as HTMLSelectElement).value as 'user' | 'memory'
    const first = categoryOptions[addTarget][0]
    addCategory = first.id
    addKind = first.kind
  }

  function confirmedSortChanged(event: Event) {
    confirmedSort = (event.currentTarget as HTMLSelectElement).value as ConfirmedSort
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
        context: { spaces: [addSpace], roles: [addRole], applies_when: [], excludes_when: [] },
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

  async function startEdit(target: EditTarget) {
    editTarget = target
    editText = target.kind === 'confirmed' ? target.current.claim.text : target.pending.revision.text
    openMenuFor = null
    await tick()
    editTextarea?.focus()
  }

  async function submitEdit() {
    if (!editTarget || !editText.trim() || writing) return
    writing = true
    error = ''
    try {
      const receipt = editTarget.kind === 'confirmed'
        ? await memoryReplace({
            request_id: requestId('memory-ui/replace'), expected_protocol: protocol(),
            claim_id: editTarget.current.claim.claim_id,
            expected_heads: expectedHeads(editTarget.current.claim),
            gesture_intent: 'replace', text: editText.trim(),
          })
        : await memoryApprove({
            request_id: requestId('memory-ui/approve-edited'), expected_protocol: protocol(),
            expected_heads: editTarget.pending.expected_heads,
            revision_id: editTarget.pending.revision.revision_id,
            expected_sha256: editTarget.pending.expected_sha256,
            gesture_intent: 'approve', text_override: editText.trim(),
          })
      const message = editTarget.kind === 'confirmed' ? '主张内容已修改' : '已修正并确认这条主张'
      editTarget = null
      editText = ''
      await completed(message, receipt.projection_rebuilt)
    } catch (cause) {
      error = hostError(cause)
    } finally {
      writing = false
    }
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
      space: contextSpace, role: contextRole, purpose: contextPurpose, caller: 'plugin:notemd.memory', provider: contextProvider,
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
    if (showReset) showReset = false
    else if (destructive) destructive = null
    else if (mergeConflict) mergeConflict = null
    else if (showRoleScopeManager && !roleScopeBusy) showRoleScopeManager = false
    else if (showContext) showContext = false
    else if (editTarget) { editTarget = null; editText = '' }
    else if (showAdd) showAdd = false
    else if (pendingContextMenu) pendingContextMenu = null
    else openMenuFor = null
  }
</script>

<svelte:head><title>Memory</title></svelte:head>
<svelte:window onkeydown={closeTopLayer} onclick={() => pendingContextMenu = null} onblur={() => pendingContextMenu = null} />

<main>
  <header class="app-header">
    <div><h1>记忆</h1><p>结构化主张保存在 .notemd/memory；MEMORY.md 只是按身份与场景分组的纯文本视图。</p></div>
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
      <button class="secondary" onclick={() => showRoleScopeManager = true} disabled={!snapshot || snapshot.mode !== 'v2'}>身份与场景…</button>
      <button class="secondary reset-trigger" onclick={() => showReset = true} disabled={!writable || writing || !!inferenceRun}>重构记忆…</button>
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
          <select bind:value={target} aria-label="记忆分组"><option value="all">全部分组</option><option value="user">身份与偏好</option><option value="memory">决定与背景</option><option value="structured">仅结构化上下文</option></select>
          <select value={confirmedSort} onchange={confirmedSortChanged} aria-label="已确认主张排序"><option value="priority">重点优先</option><option value="recent">最近更新</option><option value="oldest">最早更新</option><option value="text">正文 A–Z</option></select>
          <button class="secondary" onclick={copyImportPrompt} title="复制一段 Prompt，粘贴给其他 AI 助手，让它把记住你的条目导出成 notemd memory propose 命令。">复制导入记忆Prompt</button>
          <button class="primary" onclick={resetAdd} disabled={!writable}>添加主张</button>
        </div>
        <section class="split">
          <div class="master" role="listbox" tabindex="0" aria-label="已确认主张列表" onkeydown={(event) => moveListFocus(event, '[role=option]')}>
            {#each confirmedGroups as section, sectionIndex (section.key)}
              <section class="claim-section" role="group" aria-labelledby="claim-section-{sectionIndex}">
                <h3 id="claim-section-{sectionIndex}">{section.label}</h3>
                {#each section.categories as category, categoryIndex (`${section.key}:${category.key}`)}
                  <div class="claim-category" role="group" aria-labelledby="claim-category-{sectionIndex}-{categoryIndex}">
                    <h4 id="claim-category-{sectionIndex}-{categoryIndex}">{category.label}<span>{category.items.length}</span></h4>
                    {#each category.items as item (item.claim.claim_id)}
                      <button role="option" aria-selected={selectedClaim?.claim.claim_id === item.claim.claim_id} class:selected={selectedClaim?.claim.claim_id === item.claim.claim_id} onclick={() => selectedClaimId = item.claim.claim_id}>
                        <span class="polarity {item.claim.polarity}" aria-hidden="true"></span>
                        <span><strong>{item.claim.text}</strong><small>{claimKindLabels[item.claim.claim_kind]} · {subjectLabel(item.claim)}</small></span>
                        <span class="row-state">{item.claim.salience === 'pinned' ? '重要' : item.claim.epistemic.representation_certainty}</span>
                      </button>
                    {/each}
                  </div>
                {/each}
              </section>
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
                <div class="wide"><dt>Role / Scope / 用途</dt><dd>{claim.context.roles?.join('、') || 'role:unclassified'} · {claim.context.spaces.join('、')} · {claim.consent.allowed_purposes.join('、')}</dd></div>
                <div><dt>Provider</dt><dd>{claim.consent.external_provider_policy}</dd></div>
                <div><dt>纯文本位置</dt><dd>{categoryLabel(claim.projection.target, claim.projection.category)}</dd></div>
                <div class="wide"><dt>Claim / Revision</dt><dd><code>{claim.claim_id}<br />{claim.revision_id}</code></dd></div>
              </dl>
              <div class="detail-actions">
                <div class="menu-anchor">
                  <button aria-haspopup="menu" aria-expanded={openMenuFor === claim.claim_id} onclick={() => openMenuFor = openMenuFor === claim.claim_id ? null : claim.claim_id}>更多</button>
                  {#if openMenuFor === claim.claim_id}
                    <div class="menu-panel" role="menu" aria-label="主张操作">
                      <button class="menu-row" role="menuitem" onclick={() => startEdit({ kind: 'confirmed', current: selectedClaim })} disabled={!writable || writing}>编辑内容…</button>
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
        <div class="master" role="listbox" tabindex="0" aria-label="待确认建议列表" aria-multiselectable="true" onscroll={() => pendingContextMenu = null} onkeydown={(event) => moveListFocus(event, '[role=option]')}>
          {#each reviews as item (item.revision.revision_id)}
            <button role="option" aria-selected={selectedPendingIds.includes(item.revision.revision_id)} class:selected={selectedPendingIds.includes(item.revision.revision_id)} class:sensitive={item.revision.risk_class === 'action-sensitive'} onclick={(event) => selectPending(item, event)} oncontextmenu={(event) => openPendingContextMenu(item, event)} onkeydown={(event) => openPendingContextMenuFromKeyboard(item, event)}>
              <span class="polarity {item.revision.polarity}" aria-hidden="true"></span>
              <span><strong>{item.revision.text}</strong><small>{claimKindLabels[item.revision.claim_kind]} · {actorLabel(item.revision.recorded_by)}</small></span>
              <span class="row-state">{item.revision.risk_class === 'action-sensitive' ? '需谨慎' : item.revision.salience}</span>
            </button>
          {:else}<div class="empty">没有待确认建议。</div>{/each}
        </div>
        {#if selectedPendingItems.length > 1}
          <article class="detail bulk-pending-summary" aria-labelledby="pending-bulk-title">
            <div class="eyebrow"><span class="badge positive">已多选</span><span class="badge">{selectedPendingItems.length} 条</span></div>
            <h2 id="pending-bulk-title">批量处理待确认建议</h2>
            <p>右键任一选中项，或使用下方按钮，可批量忽略、否认或删除候选。</p>
            <div class="approval-meaning"><small>确认仍需逐条进行</small><strong>不会提供笼统的批量确认</strong><p>身份表达、事实确认、行为授权和生命周期变更的含义不同，需要逐条查看后确认；编辑和“认为重要”也保留为单项操作。</p></div>
            <ul class="pending-selection-preview">
              {#each selectedPendingItems as item (item.revision.revision_id)}<li>{item.revision.text}</li>{/each}
            </ul>
            <div class="pending-actions"><button aria-haspopup="menu" aria-expanded={!!pendingContextMenu} onclick={openSelectedPendingMenu}>批量操作…</button></div>
          </article>
        {:else if selectedPending}
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
              <div><dt>Role</dt><dd>{revision.context.roles?.join('、') || 'role:unclassified'}</dd></div><div><dt>Scope</dt><dd>{revision.context.spaces.join('、')}</dd></div><div><dt>Provider</dt><dd>{revision.consent.external_provider_policy}</dd></div>
              <div><dt>生命周期</dt><dd>{revision.lifecycle.state}</dd></div>
              <div class="wide"><dt>来源</dt><dd>{selectedPending.source_summary ?? revision.evidence?.[0]?.resource ?? '未提供'}</dd></div>
            </dl>
            <div class="pending-actions">
              <button onclick={() => destructive = { kind: 'delete-candidate', pending: selectedPending }} disabled={!writable || writing}>删除候选…</button>
              <button onclick={() => destructive = { kind: 'reject', pending: selectedPending }} disabled={!writable || writing}>否认…</button>
              <button onclick={() => decidePending(selectedPending, 'ignore')} disabled={!writable || writing}>可以忽略</button>
              {#if !lifecycleChange}<button onclick={() => startEdit({ kind: 'pending', pending: selectedPending })} disabled={!writable || writing}>编辑后确认…</button>{/if}
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

{#if pendingContextMenu && selectedPendingItems.length}
  <div class="menu-panel pending-context-menu" role="menu" aria-label="待确认建议批量操作" style:left="{pendingContextMenu.x}px" style:top="{pendingContextMenu.y}px">
    <div class="menu-heading" role="presentation">已选择 {selectedPendingItems.length} 条</div>
    <button class="menu-row" role="menuitem" onclick={() => startPendingBatch('ignore')} disabled={!writable || writing}>可以忽略…</button>
    <button class="menu-row" role="menuitem" onclick={() => startPendingBatch('reject')} disabled={!writable || writing}>否认所选…</button>
    <button class="menu-row danger-row" role="menuitem" onclick={() => startPendingBatch('delete-candidate')} disabled={!writable || writing}>删除所选候选…</button>
  </div>
{/if}

{#if showRoleScopeManager && snapshot}
  <RoleScopeManager
    claims={snapshot.claims}
    onclose={() => showRoleScopeManager = false}
    onchanged={roleScopeChanged}
    onbusychange={(value) => roleScopeBusy = value}
  />
{/if}

{#if showAdd && snapshot?.owner}
  <div class="scrim" role="presentation">
    <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="add-title">
      <header><div><h2 id="add-title">添加主张</h2><p>保存就是本次人工批准，不会再出现第二步。</p></div><button class="close" aria-label="关闭添加主张" onclick={() => showAdd = false} disabled={writing}>×</button></header>
      <div class="form-grid"><label>MEMORY.md 分组<select value={addTarget} onchange={targetChanged}><option value="user">身份与偏好</option><option value="memory">决定与背景</option></select></label><label>一级分类<select value={addCategory} onchange={categoryChanged}>{#each currentCategoryOptions as option}<option value={option.id}>{option.label}</option>{/each}</select></label></div>
      <label class="field">主张内容<textarea bind:this={addTextarea} bind:value={addText} rows="5" placeholder="一条可以独立确认或否认的多行主张"></textarea></label>
      <section class="approval-meaning"><small>保存的批准含义</small><strong>{approvalLabels[addApproval].label}</strong><p>{approvalLabels[addApproval].explanation}</p></section>
      <details><summary>使用范围与高级信息</summary>
        <div class="form-grid"><label>主张类型<select bind:value={addKind}>{#each Object.entries(claimKindLabels) as [id, label]}<option value={id}>{label}</option>{/each}</select></label><label>Role<select bind:value={addRole}>{#if availableRoles.length}{#each availableRoles as role}<option value={role.id}>{role.label}</option>{/each}{:else}<option value="role:unclassified">未分类 Role</option>{/if}</select></label><label>Scope<select bind:value={addSpace}><option value="">请选择明确 Scope</option>{#each availableScopes as scope}<option value={scope.id}>{scope.label}</option>{/each}</select></label><label>Provider 策略<select bind:value={addProviderPolicy}><option value="deny">禁止外发</option><option value="prompt">每次询问</option><option value="allow">允许</option></select></label><label>敏感度<select bind:value={addSensitivity}><option value="normal">普通</option><option value="private">私人</option></select></label><label>显著性<select bind:value={addSalience}><option value="normal">普通</option><option value="pinned">重要</option></select></label><label>方向<select bind:value={addPolarity}><option value="neutral">中性</option><option value="positive">正向</option><option value="negative">负向</option></select></label></div>
        <fieldset><legend>允许用途</legend>{#each ['planning', 'writing', 'information-answer', 'external-action', 'projection', 'sync'] as purpose}<label class="check"><input type="checkbox" checked={addPurposes.includes(purpose)} onchange={() => togglePurpose(purpose)} />{purpose}</label>{/each}</fieldset>
        <label class="field">Agent 使用方式<input bind:value={addGuidance} placeholder="在什么情况下如何使用" /></label><label class="field">必须避免{addNeedsAvoid ? '（必填）' : ''}<input bind:value={addAvoid} placeholder="不能做出的推断或动作" /></label>
      </details>
      <footer><button onclick={() => showAdd = false} disabled={writing}>取消</button><button class="primary" onclick={submitAdd} disabled={writing || !addText.trim() || !addSpace || !addRole || !addPurposes.length || (addNeedsAvoid && !addAvoid.trim())}>{writing ? '正在保存…' : '保存并确认'}</button></footer>
    </div>
  </div>
{/if}

{#if editTarget}
  <div class="scrim" role="presentation">
    <div class="sheet" role="dialog" aria-modal="true" aria-labelledby="edit-title">
      <header><div><h2 id="edit-title">{editTarget.kind === 'confirmed' ? '编辑已确认记忆' : '编辑待确认建议'}</h2><p>{editTarget.kind === 'confirmed' ? '保存会创建新的修订，不会改写不可变历史。' : '保存会用修正后的内容确认；原始提案仍保留在不可变历史中。'}</p></div><button class="close" aria-label="关闭编辑" onclick={() => { editTarget = null; editText = '' }} disabled={writing}>×</button></header>
      <label class="field">主张内容<textarea bind:this={editTextarea} bind:value={editText} rows="7" maxlength="32768"></textarea></label>
      <footer><button onclick={() => { editTarget = null; editText = '' }} disabled={writing}>取消</button><button class="primary" onclick={submitEdit} disabled={writing || !editText.trim() || (editTarget.kind === 'confirmed' && editText.trim() === editTarget.current.claim.text)}>{writing ? '正在保存…' : editTarget.kind === 'confirmed' ? '保存修改' : '保存并确认'}</button></footer>
    </div>
  </div>
{/if}

{#if destructive}
  <div class="scrim" role="presentation"><div class="sheet compact-sheet" role="alertdialog" aria-modal="true" aria-labelledby="destructive-title"><header><div><h2 id="destructive-title">{destructive.kind === 'batch-pending' ? `批量${batchActionText(destructive.action)} ${destructive.pending.length} 条建议？` : destructive.kind === 'reject' ? '否认这条建议？' : destructive.kind === 'delete-candidate' ? '删除这个候选？' : destructive.kind === 'approve-lifecycle' ? `确认${destructive.pending.revision.lifecycle.state === 'deleted' ? '删除' : '撤销'}此主张？` : '移出当前记忆？'}</h2><p>{destructive.kind === 'batch-pending' ? destructive.action === 'ignore' ? `这 ${destructive.pending.length} 条建议会离开待确认队列，并分别记录忽略决定，避免相同内容反复出现。` : destructive.action === 'reject' ? `系统会分别记录对这 ${destructive.pending.length} 条建议的否认决定；它们不会进入当前记忆。` : `这 ${destructive.pending.length} 个候选会从待确认队列移除；已有 Git 历史可能继续保留。` : destructive.kind === 'delete-claim' ? '主张会从当前投影和 Agent context 移除；不可变历史仍会保留。这不是从 Git 历史永久擦除。' : destructive.kind === 'approve-lifecycle' ? '这是不可见性变更：主张会离开当前投影和 Agent context；不可变历史仍会保留。' : destructive.kind === 'reject' ? '系统会记录否认决定，建议不会进入当前记忆。' : '候选不会进入当前记忆；已有 Git 历史可能继续保留。'}</p></div></header><footer><button onclick={() => destructive = null} disabled={writing}>取消</button><button class={destructive.kind === 'batch-pending' && destructive.action === 'ignore' ? 'primary' : 'destructive'} onclick={confirmDestructive} disabled={writing}>{writing ? '正在处理…' : destructive.kind === 'batch-pending' ? `确认${batchActionText(destructive.action)} ${destructive.pending.length} 条` : '确认'}</button></footer></div></div>
{/if}

{#if showReset && snapshot}
  <div class="scrim" role="presentation">
    <div class="sheet compact-sheet reset-sheet" role="alertdialog" aria-modal="true" aria-labelledby="reset-title">
      <header><div><h2 id="reset-title">清空并重构全部记忆？</h2><p>这是高影响操作，请先确认范围和后果。</p></div></header>
      <div class="reset-impact">
        <strong>将立即移除 {resetClaims.length} 条已确认记忆和 {snapshot.pending.length} 条待确认建议。</strong>
        <ul>
          <li>MEMORY.md 与 Agent context 会失去这些内容。</li>
          <li>推理进度会重置；下次推理将重新扫描整个 Vault。</li>
          <li>所有者身份、Memory 协议和不可变历史会保留，因此这不是从 Git 历史永久擦除。</li>
        </ul>
      </div>
      <footer><button onclick={() => showReset = false} disabled={writing}>取消</button><button class="destructive" onclick={resetAllMemory} disabled={writing}>{writing ? '正在清理…' : '确认清空并重构'}</button></footer>
    </div>
  </div>
{/if}

{#if mergeConflict}
  <div class="scrim" role="presentation"><div class="sheet" role="dialog" aria-modal="true" aria-labelledby="merge-title"><header><div><h2 id="merge-title">合并并发版本</h2><p>合并会创建同时引用所有当前 head 的新 revision。</p></div><button class="close" aria-label="关闭合并编辑" onclick={() => mergeConflict = null}>×</button></header><label class="field">合并后的主张<textarea bind:value={mergedText} rows="7"></textarea></label><footer><button onclick={() => mergeConflict = null}>取消</button><button class="primary" onclick={() => resolveConflict(mergeConflict!, 'merge')} disabled={writing || !mergedText.trim()}>保存合并</button></footer></div></div>
{/if}

{#if showContext && snapshot}
  <div class="scrim" role="presentation"><div class="sheet context-sheet" role="dialog" aria-modal="true" aria-labelledby="context-title"><header><div><h2 id="context-title">Context Manifest</h2><p>先明确 Role、Scope、用途和接收方，再查看将提供给 Agent 的最小主张集合。</p></div><button class="close" aria-label="关闭 Context Manifest" onclick={() => showContext = false}>×</button></header>
    <div class="form-grid"><label>Role<select bind:value={contextRole}>{#each snapshot.context_options?.roles ?? [{ id: 'role:unclassified', label: '未分类身份' }] as option}<option value={option.id}>{option.label}</option>{/each}</select></label><label>Scope<select bind:value={contextSpace}>{#each snapshot.context_options?.spaces ?? [{ id: 'global', label: '全局' }] as option}<option value={option.id}>{option.label}</option>{/each}</select></label><label>用途<select bind:value={contextPurpose}>{#each snapshot.context_options?.purposes ?? [{ id: 'planning', label: '规划' }] as option}<option value={option.id}>{option.label}</option>{/each}</select></label><label>Provider<select bind:value={contextProvider}>{#each snapshot.context_options?.providers ?? [{ id: 'openai', label: 'OpenAI' }] as option}<option value={option.id}>{option.label}</option>{/each}</select></label><label>Model<select bind:value={contextModel}>{#each (snapshot.context_options?.models ?? [{ id: 'gpt-5', label: 'GPT-5' }]).filter((option) => !option.provider_id || option.provider_id === contextProvider) as option}<option value={option.id}>{option.label}</option>{/each}</select></label><label>有效时间<input type="datetime-local" bind:value={contextAsOf} /></label><label>Tools（逗号分隔）<input bind:value={contextTools} /></label><label class="check wide"><input type="checkbox" bind:checked={contextExternal} />内容将发送给外部 Provider</label></div>
    <div class="context-actions"><button class="primary" onclick={previewContext} disabled={contextBusy || !contextSpace || !contextPurpose || !contextProvider || !contextModel || !contextAsOf}>{contextBusy ? '正在检查…' : '预览选择'}</button></div>
    {#if contextPreview}<section class="context-result" aria-live="polite"><div class="result-summary"><strong>{contextPreview.selected.length} 条将被选中</strong><span>{contextPreview.redactions} 条已脱敏 · {contextPreview.conflicts.length} 个冲突</span></div><dl class="facts"><div><dt>现实行动</dt><dd>{contextPreview.policy_result.external_action_allowed ? '允许' : '不允许'}</dd></div><div><dt>排除</dt><dd>{Object.entries(contextPreview.excluded_summary).map(([key, value]) => `${key} ${value}`).join('、') || '无'}</dd></div></dl><ul>{#each contextPreview.selected as item}<li>{item.text ?? item.claim_id}<small>{item.reasons.join(' · ')}</small></li>{/each}</ul><button onclick={createManifest} disabled={contextBusy || !writable || !contextPreview.preview_sha256}>记录本次使用清单</button>{#if !writable}<small class="readonly-note">当前模式只允许预览，不会写入 Context Manifest。</small>{/if}</section>{/if}
  </div></div>
{/if}

<style>
  :global(:root){color-scheme:light dark;font:13px/1.45 -apple-system,BlinkMacSystemFont,"SF Pro Text",sans-serif;accent-color:#0a84ff}:global(body){margin:0;background:Canvas;color:CanvasText}:global(*){box-sizing:border-box}:global(button),:global(input),:global(select),:global(textarea),:global(summary){font:inherit;pointer-events:auto;-webkit-app-region:no-drag}:global(button){min-height:32px;border:1px solid color-mix(in srgb,CanvasText 14%,transparent);border-radius:8px;padding:0 12px;background:color-mix(in srgb,CanvasText 5%,Canvas);color:CanvasText;cursor:pointer}:global(button:hover:not(:disabled)){background:color-mix(in srgb,CanvasText 10%,Canvas)}:global(button:active:not(:disabled)){transform:translateY(1px)}:global(button:focus-visible),:global(input:focus-visible),:global(select:focus-visible),:global(textarea:focus-visible),:global(summary:focus-visible){outline:3px solid color-mix(in srgb,#0a84ff 34%,transparent);outline-offset:2px}:global(button:disabled){opacity:.42;cursor:default}:global(button.primary){min-height:34px;background:#0a84ff;border-color:#0a84ff;color:white;font-weight:600}:global(button.primary:hover:not(:disabled)){background:#0077ed}:global(button.destructive){background:#ff3b30;border-color:#ff3b30;color:#fff;font-weight:600}:global(input),:global(select),:global(textarea){width:100%;min-height:32px;padding:6px 9px;border:1px solid color-mix(in srgb,CanvasText 16%,transparent);border-radius:7px;background:Canvas;color:CanvasText}:global(textarea){resize:vertical;line-height:1.5}.sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}
  main{max-width:1240px;margin:0 auto;padding:20px 24px 40px}.app-header{display:flex;justify-content:space-between;align-items:flex-start;gap:20px}.app-header h1{margin:0;font-size:20px;line-height:1.25;letter-spacing:-.02em}.app-header p{max-width:680px;margin:4px 0 0;font-size:12px;color:color-mix(in srgb,CanvasText 58%,transparent)}.header-actions{display:flex;align-items:center;gap:8px}.inference-action{display:flex;align-items:center;gap:4px}.health{display:flex;align-items:center;gap:7px;max-width:250px;color:color-mix(in srgb,CanvasText 62%,transparent);font-size:12px}.health>span:first-child{flex:0 0 8px;width:8px;height:8px;border-radius:50%;background:#34c759}.health.conflict>span:first-child{background:#ff9f0a}.health.bad>span:first-child{background:#ff3b30}.icon{width:32px;padding:0;font-size:17px}.secondary{background:transparent}.reset-trigger{color:#d62d26}.banner{display:flex;align-items:flex-start;gap:10px;margin-top:14px;padding:10px 12px;border:1px solid;border-radius:9px;font-size:12px}.banner strong{white-space:nowrap}.banner.error{border-color:color-mix(in srgb,#ff3b30 42%,transparent);background:color-mix(in srgb,#ff3b30 9%,Canvas)}.banner.warning{border-color:color-mix(in srgb,#ff9f0a 45%,transparent);background:color-mix(in srgb,#ff9f0a 9%,Canvas)}.banner.inference-progress{border-color:color-mix(in srgb,#0a84ff 36%,transparent);background:color-mix(in srgb,#0a84ff 7%,Canvas)}
  .segments{display:grid;grid-template-columns:repeat(3,1fr);gap:3px;max-width:560px;margin:18px auto 16px;padding:3px;border-radius:9px;background:color-mix(in srgb,CanvasText 8%,Canvas)}.segments button{min-height:34px;border:0;background:transparent;color:color-mix(in srgb,CanvasText 65%,transparent);font-weight:600}.segments button.active{background:Canvas;color:CanvasText;box-shadow:0 1px 4px rgba(0,0,0,.16)}.count{display:inline-grid;place-items:center;min-width:18px;height:18px;padding:0 5px;border-radius:9px;background:color-mix(in srgb,CanvasText 10%,Canvas);font-size:11px}.count.large{min-width:28px;height:24px;border-radius:12px}.toolbar{display:grid;grid-template-columns:minmax(180px,1fr) 160px 140px auto auto;gap:8px;margin-bottom:12px}.split{display:grid;grid-template-columns:350px minmax(0,1fr);min-height:510px;border:1px solid color-mix(in srgb,CanvasText 13%,transparent);border-radius:11px;overflow:hidden;background:color-mix(in srgb,CanvasText 1.5%,Canvas)}
  .master{max-height:calc(100vh - 220px);min-height:510px;overflow:auto;border-right:1px solid color-mix(in srgb,CanvasText 12%,transparent);background:color-mix(in srgb,CanvasText 3%,Canvas)}.claim-section>h3{position:sticky;top:0;z-index:2;margin:0;padding:8px 12px 6px;border-bottom:1px solid color-mix(in srgb,CanvasText 10%,transparent);background:color-mix(in srgb,CanvasText 9%,Canvas);font-size:11px;letter-spacing:.04em}.claim-category>h4{display:flex;justify-content:space-between;margin:0;padding:6px 12px;border-bottom:1px solid color-mix(in srgb,CanvasText 7%,transparent);background:color-mix(in srgb,CanvasText 5%,Canvas);color:color-mix(in srgb,CanvasText 62%,transparent);font-size:11px;font-weight:600}.claim-category>h4 span{font-variant-numeric:tabular-nums}.master [role=option]{display:grid;grid-template-columns:8px minmax(0,1fr) auto;align-items:start;gap:9px;width:100%;min-height:70px;padding:11px 12px;border:0;border-bottom:1px solid color-mix(in srgb,CanvasText 8%,transparent);border-radius:0;background:transparent;text-align:left}.master [role=option].selected{background:#0a84ff;color:#fff}.master [role=option].selected small,.master [role=option].selected .row-state{color:rgba(255,255,255,.8)}.master [role=option].sensitive{box-shadow:inset 3px 0 #ff9f0a}.master strong{display:-webkit-box;overflow:hidden;line-clamp:2;-webkit-line-clamp:2;-webkit-box-orient:vertical;font-size:13px;line-height:1.4;white-space:pre-wrap}.master small{display:block;margin-top:4px;color:color-mix(in srgb,CanvasText 53%,transparent);font-size:12px}.row-state{color:color-mix(in srgb,CanvasText 55%,transparent);font-size:11px}.polarity{width:7px;height:7px;margin-top:5px;border-radius:50%;background:#8e8e93}.polarity.positive{background:#34c759}.polarity.negative{background:#ff3b30}
  .detail{min-width:0;padding:22px 24px}.detail h2{margin:12px 0 16px;font-size:17px;line-height:1.5;letter-spacing:-.01em;white-space:pre-wrap}.eyebrow{display:flex;flex-wrap:wrap;gap:6px}.badge{padding:3px 7px;border-radius:6px;background:color-mix(in srgb,CanvasText 7%,Canvas);font-size:11px;font-weight:650}.badge.positive{background:color-mix(in srgb,#34c759 18%,Canvas);color:#168333}.badge.negative,.badge.danger{background:color-mix(in srgb,#ff3b30 16%,Canvas);color:#d62d26}.badge.pinned{background:color-mix(in srgb,#ff9f0a 20%,Canvas);color:#9a5500}.guidance{margin-bottom:16px;padding:13px 14px;border-left:3px solid #0a84ff;border-radius:0 8px 8px 0;background:color-mix(in srgb,#0a84ff 7%,Canvas)}.guidance small,.approval-meaning small{font-size:11px;font-weight:700;color:#0a84ff}.guidance p{margin:4px 0 9px;font-size:13px}.guidance p:last-child{margin-bottom:0}.guidance .avoid{color:#d62d26;font-weight:600}.facts{display:grid;grid-template-columns:1fr 1fr;gap:1px;margin:0;background:color-mix(in srgb,CanvasText 8%,transparent);border:1px solid color-mix(in srgb,CanvasText 8%,transparent);border-radius:8px;overflow:hidden}.facts>div{display:flex;justify-content:space-between;gap:12px;padding:9px 10px;background:Canvas}.facts>div.wide{grid-column:1/-1}.facts dt{font-size:12px;color:color-mix(in srgb,CanvasText 54%,transparent)}.facts dd{margin:0;text-align:right;font-size:12px;overflow-wrap:anywhere}.detail-actions{display:flex;justify-content:flex-end;margin-top:16px}.menu-anchor{position:relative}.menu-panel{position:absolute;right:0;bottom:40px;z-index:20;min-width:190px}.menu-panel.pending-context-menu{position:fixed;right:auto;bottom:auto;z-index:90;min-width:230px}.menu-heading{padding:6px 10px 5px;color:color-mix(in srgb,CanvasText 55%,transparent);font-size:11px;font-weight:650}.menu-panel .menu-row{width:100%;min-height:30px;border:0;text-align:left}.menu-panel .danger-row{color:#ff3b30}.menu-panel .danger-row:hover{background:#ff3b30;color:#fff}.approval-meaning{margin:0 0 14px;padding:12px 13px;border-radius:9px;background:color-mix(in srgb,#0a84ff 7%,Canvas)}.approval-meaning strong{display:block;margin-top:3px;font-size:14px}.approval-meaning p{margin:4px 0 0;color:color-mix(in srgb,CanvasText 66%,transparent);font-size:12px}.diff{display:grid;grid-template-columns:1fr 1fr;gap:9px;margin:12px 0 16px}.diff>div{min-height:92px;padding:11px;border-radius:8px;background:color-mix(in srgb,CanvasText 5%,Canvas)}.diff small{font-size:11px;color:color-mix(in srgb,CanvasText 54%,transparent)}.diff p{margin:5px 0 0;white-space:pre-wrap}.pending-actions{display:flex;flex-wrap:wrap;justify-content:flex-end;gap:8px;margin-top:16px}.bulk-pending-summary>p{color:color-mix(in srgb,CanvasText 66%,transparent)}.pending-selection-preview{max-height:210px;margin:14px 0 0;padding:0;overflow:auto;list-style:none;border:1px solid color-mix(in srgb,CanvasText 10%,transparent);border-radius:9px}.pending-selection-preview li{padding:8px 10px;border-bottom:1px solid color-mix(in srgb,CanvasText 8%,transparent);white-space:pre-wrap}.pending-selection-preview li:last-child{border-bottom:0}
  .history{display:grid;gap:12px}.history>header{display:flex;justify-content:space-between;align-items:center}.history h2{margin:0;font-size:16px}.history header p{margin:3px 0 0;color:color-mix(in srgb,CanvasText 58%,transparent);font-size:12px}.conflict-card{padding:16px;border:1px solid color-mix(in srgb,#ff9f0a 42%,transparent);border-radius:11px;background:color-mix(in srgb,#ff9f0a 5%,Canvas)}.conflict-title{display:flex;justify-content:space-between;gap:18px}.conflict-title h3{margin:7px 0 0;font-size:15px}.conflict-title>strong{color:#b36300;font-size:12px}.ancestor{margin:12px 0;padding:10px 11px;border-radius:8px;background:color-mix(in srgb,CanvasText 5%,Canvas)}.ancestor small,.heads small{font-size:11px;color:color-mix(in srgb,CanvasText 55%,transparent)}.ancestor p,.heads p{margin:4px 0 0;white-space:pre-wrap}.heads{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:9px}.heads section{padding:11px;border:1px solid color-mix(in srgb,CanvasText 11%,transparent);border-radius:9px;background:Canvas}.heads button{margin-top:10px}.conflict-actions{display:flex;justify-content:flex-end;gap:8px;margin-top:12px}.history-heading{margin-top:12px}.timeline{margin:0;padding:0;list-style:none;border:1px solid color-mix(in srgb,CanvasText 10%,transparent);border-radius:10px;overflow:hidden}.timeline li{display:grid;grid-template-columns:10px minmax(0,1fr) auto;gap:10px;align-items:start;padding:11px 13px;border-bottom:1px solid color-mix(in srgb,CanvasText 8%,transparent)}.timeline li:last-child{border-bottom:0}.timeline-dot{width:8px;height:8px;margin-top:5px;border-radius:50%;background:#0a84ff}.timeline strong{display:block;font-size:13px}.timeline small{display:block;margin-top:2px;color:color-mix(in srgb,CanvasText 55%,transparent);font-size:12px}.timeline>li>span:last-child{font-size:11px}.empty{padding:52px 18px;text-align:center;color:color-mix(in srgb,CanvasText 48%,transparent)}.empty.compact{padding:22px}.detail-empty{display:grid;place-items:center}
  .scrim{position:fixed;inset:0;z-index:100;display:grid;place-items:center;padding:24px;background:rgba(0,0,0,.28);backdrop-filter:blur(8px)}.sheet{box-sizing:border-box;width:min(680px,100%);max-height:calc(100vh - 48px);overflow:auto;padding:22px;border:1px solid color-mix(in srgb,CanvasText 15%,transparent);border-radius:14px;background:Canvas;color:CanvasText;box-shadow:0 24px 70px rgba(0,0,0,.34)}.compact-sheet{width:min(470px,100%)}.context-sheet{width:min(760px,100%)}.sheet>header{display:flex;justify-content:space-between;gap:18px}.sheet h2{margin:0;font-size:17px}.sheet header p{margin:4px 0 0;color:color-mix(in srgb,CanvasText 62%,transparent);font-size:12px}.reset-sheet{border-color:color-mix(in srgb,#ff3b30 35%,transparent)}.reset-impact{margin-top:16px;padding:12px 13px;border-radius:9px;background:color-mix(in srgb,#ff3b30 8%,Canvas)}.reset-impact>strong{color:#d62d26}.reset-impact ul{margin:9px 0 0;padding-left:20px}.reset-impact li{margin:5px 0}.close{width:32px;padding:0;border:0;background:transparent;font-size:20px}.field,.form-grid label{display:grid;gap:4px;margin-top:12px;color:color-mix(in srgb,CanvasText 68%,transparent);font-size:12px}.form-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:0 10px}.form-grid .wide{grid-column:1/-1}details{margin-top:14px;padding:10px 12px;border:1px solid color-mix(in srgb,CanvasText 10%,transparent);border-radius:9px}summary{cursor:pointer;font-weight:600}fieldset{display:flex;flex-wrap:wrap;gap:10px;margin:14px 0 0;padding:10px;border:1px solid color-mix(in srgb,CanvasText 10%,transparent);border-radius:8px}legend{padding:0 4px;font-size:12px}.check{display:flex!important;align-items:center;gap:6px!important;margin:0!important}.check input{width:auto;min-height:auto}.sheet footer{display:flex;justify-content:flex-end;gap:8px;margin-top:18px}.context-actions{display:flex;justify-content:flex-end;margin-top:14px}.context-result{margin-top:14px;padding:13px;border-radius:10px;background:color-mix(in srgb,CanvasText 4%,Canvas)}.result-summary{display:flex;justify-content:space-between}.context-result ul{margin:10px 0;padding-left:20px}.context-result li{margin:5px 0}.context-result li small,.readonly-note{display:block;color:color-mix(in srgb,CanvasText 55%,transparent);font-size:12px}
  @media(max-width:820px){main{padding:16px}.split{grid-template-columns:290px minmax(0,1fr)}.header-actions{flex-wrap:wrap;justify-content:flex-end}.heads{grid-template-columns:1fr}.toolbar{grid-template-columns:repeat(2,minmax(0,1fr))}.toolbar .search{grid-column:1/-1}}@media(max-width:660px){.app-header{display:block}.header-actions{justify-content:flex-start;margin-top:10px}.split{display:block}.master{min-height:180px;max-height:280px;border-right:0;border-bottom:1px solid color-mix(in srgb,CanvasText 12%,transparent)}.toolbar,.form-grid,.diff,.facts{grid-template-columns:1fr}.toolbar .search,.facts>div.wide,.form-grid .wide{grid-column:auto}.segments{max-width:none}.pending-actions{justify-content:stretch}.pending-actions button{flex:1 1 120px}}
  @media(prefers-reduced-motion:reduce){:global(*){scroll-behavior:auto!important;transition:none!important}}
</style>
