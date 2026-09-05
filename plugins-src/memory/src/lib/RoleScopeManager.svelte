<script lang="ts">
  import { onMount } from 'svelte'
  import {
    clipboardWrite,
    memoryContextRegistry,
    memoryContextRegistryReplace,
    memoryReassignApply,
    memoryReassignPreview,
    requestId,
    toast,
  } from './bridge'
  import { contextBootstrapPrompt } from './contextBootstrapPrompt'
  import { hostError } from './domain'
  import type {
    ContextRegistrySnapshot,
    ContextRole,
    ContextScope,
    EffectiveClaim,
    ReassignmentPreview,
    ReassignmentPreviewInput,
  } from './types'

  type View = 'roles' | 'scopes' | 'reassign'
  type StatusChange = { kind: 'role' | 'scope'; id: string; status: 'active' | 'archived' }

  let {
    claims,
    onchanged,
    onbusychange = () => {},
  }: {
    claims: EffectiveClaim[]
    onchanged: () => void | Promise<void>
    onbusychange?: (busy: boolean) => void
  } = $props()

  let view = $state<View>('roles')
  let registry = $state<ContextRegistrySnapshot | null>(null)
  let roles = $state<ContextRole[]>([])
  let scopes = $state<ContextScope[]>([])
  let loading = $state(true)
  let busy = $state(false)
  let error = $state('')
  let announcement = $state('')
  let selectedRoleId = $state<string | null>(null)
  let selectedScopeId = $state<string | null>(null)
  let roleDraft = $state<ContextRole | null>(null)
  let roleDraftOriginalId = $state<string | null>(null)
  let roleAliases = $state('')
  let scopeDraft = $state<ContextScope | null>(null)
  let scopeDraftOriginalId = $state<string | null>(null)
  let scopeAliases = $state('')
  let statusChange = $state<StatusChange | null>(null)

  let selectedClaimIds = $state<string[]>([])
  let replacementRoleIds = $state<string[]>([])
  let replacementScopeIds = $state<string[]>([])
  let reassignmentPreview = $state<ReassignmentPreview | null>(null)
  let previewRequest = $state<ReassignmentPreviewInput | null>(null)
  let applyRequestId = $state<string | null>(null)

  const selectedRole = $derived(roles.find((item) => item.id === selectedRoleId) ?? roles[0])
  const selectedScope = $derived(scopes.find((item) => item.id === selectedScopeId) ?? scopes[0])
  const currentClaims = $derived(claims.filter(({ claim, application_state }) =>
    application_state === 'current'
      && claim.workflow.state === 'approved'
      && claim.lifecycle.state === 'active'))
  const activeRoles = $derived(roles.filter((item) => item.status === 'active'))
  const activeScopes = $derived(scopes.filter((item) => item.status === 'active'))
  const canWrite = $derived(registry?.writable === true)
  const canPreview = $derived(selectedClaimIds.length > 0 && (replacementRoleIds.length > 0 || replacementScopeIds.length > 0))
  const previewCanApply = $derived(!!reassignmentPreview?.matched.length
    && reassignmentPreview.matched.every((item) => item.batch_eligible))

  onMount(() => { void loadRegistry() })

  function setBusy(next: boolean) {
    busy = next
    onbusychange(next)
  }

  function normalizeRole(item: ContextRole): ContextRole {
    return {
      ...item,
      label: item.label ?? item.id,
      description: item.description ?? '',
      aliases: [...(item.aliases ?? [])],
      status: item.status ?? 'active',
      guidance: item.guidance ?? '',
      avoid_error: item.avoid_error ?? '',
    }
  }

  function normalizeScope(item: ContextScope): ContextScope {
    return {
      ...item,
      label: item.label ?? item.id,
      description: item.description ?? '',
      aliases: [...(item.aliases ?? [])],
      status: item.status ?? 'active',
      kind: item.kind ?? 'space',
      security_domain: item.security_domain ?? '',
    }
  }

  async function loadRegistry() {
    loading = true
    error = ''
    try {
      const value = await memoryContextRegistry()
      registry = value
      roles = (value.roles ?? []).map(normalizeRole)
      scopes = (value.scopes ?? []).map(normalizeScope)
      selectedRoleId = roles.some((item) => item.id === selectedRoleId) ? selectedRoleId : roles[0]?.id ?? null
      selectedScopeId = scopes.some((item) => item.id === selectedScopeId) ? selectedScopeId : scopes[0]?.id ?? null
    } catch (cause) {
      error = hostError(cause)
    } finally {
      loading = false
    }
  }

  function aliases(value: string): string[] {
    return [...new Set(value.split(',').map((item) => item.trim()).filter(Boolean))]
  }

  async function replaceRegistry(nextRoles: ContextRole[], nextScopes: ContextScope[], message: string) {
    if (!registry || !canWrite || busy) return false
    setBusy(true)
    error = ''
    try {
      await memoryContextRegistryReplace({
        request_id: requestId('memory-ui/context-registry'),
        expected_protocol: registry.protocol,
        expected_registry_heads: registry.registry_heads,
        gesture_intent: 'replace-context-registry',
        roles: nextRoles,
        scopes: nextScopes,
      })
      announcement = message
      await loadRegistry()
      await onchanged()
      return true
    } catch (cause) {
      error = hostError(cause)
      return false
    } finally {
      setBusy(false)
    }
  }

  function editRole(role?: ContextRole) {
    const next = role ? normalizeRole(role) : {
      id: '', label: '', description: '', aliases: [], status: 'active' as const,
      guidance: '', avoid_error: '',
    }
    roleDraft = next
    roleDraftOriginalId = role?.id ?? null
    roleAliases = next.aliases.join(', ')
    statusChange = null
  }

  async function saveRole() {
    if (!roleDraft || !roleDraft.id.trim() || !roleDraft.label.trim()) return
    const next = normalizeRole({ ...roleDraft, id: roleDraft.id.trim(), label: roleDraft.label.trim(), aliases: aliases(roleAliases) })
    const duplicate = roles.some((item) => item.id === next.id && item.id !== roleDraftOriginalId)
    if (duplicate) { error = `Role ID 已存在：${next.id}`; return }
    const nextRoles = roleDraftOriginalId
      ? roles.map((item) => item.id === roleDraftOriginalId ? next : item)
      : [...roles, next]
    if (await replaceRegistry(nextRoles, scopes, roleDraftOriginalId ? 'Role 已更新。' : 'Role 已创建。')) {
      selectedRoleId = next.id
      roleDraft = null
      roleDraftOriginalId = null
    }
  }

  function editScope(scope?: ContextScope) {
    const next = scope ? normalizeScope(scope) : {
      id: '', label: '', description: '', aliases: [], status: 'active' as const,
      kind: 'realm' as const, security_domain: '',
    }
    scopeDraft = next
    scopeDraftOriginalId = scope?.id ?? null
    scopeAliases = next.aliases.join(', ')
    statusChange = null
  }

  async function saveScope() {
    if (!scopeDraft || !scopeDraft.id.trim() || !scopeDraft.label.trim() || !scopeDraft.security_domain.trim()) return
    const next = normalizeScope({
      ...scopeDraft,
      id: scopeDraft.id.trim(), label: scopeDraft.label.trim(), aliases: aliases(scopeAliases),
      security_domain: scopeDraft.security_domain.trim(),
      ...(scopeDraft.kind === 'space' && scopeDraft.parent_id?.trim()
        ? { parent_id: scopeDraft.parent_id.trim() }
        : { parent_id: undefined }),
    })
    const duplicate = scopes.some((item) => item.id === next.id && item.id !== scopeDraftOriginalId)
    if (duplicate) { error = `Scope ID 已存在：${next.id}`; return }
    const nextScopes = scopeDraftOriginalId
      ? scopes.map((item) => item.id === scopeDraftOriginalId ? next : item)
      : [...scopes, next]
    if (await replaceRegistry(roles, nextScopes, scopeDraftOriginalId ? 'Scope 已更新。' : 'Scope 已创建。')) {
      selectedScopeId = next.id
      scopeDraft = null
      scopeDraftOriginalId = null
    }
  }

  async function confirmStatusChange() {
    if (!statusChange) return
    const current = statusChange
    const message = current.status === 'archived' ? '已归档。' : '已恢复。'
    const succeeded = current.kind === 'role'
      ? await replaceRegistry(roles.map((item) => item.id === current.id
          ? { ...item, status: current.status, ...(current.status === 'active' ? { redirect_to: undefined } : {}) }
          : item), scopes, `Role ${message}`)
      : await replaceRegistry(roles, scopes.map((item) => item.id === current.id
          ? { ...item, status: current.status, ...(current.status === 'active' ? { redirect_to: undefined } : {}) }
          : item), `Scope ${message}`)
    if (succeeded) statusChange = null
  }

  function invalidatePreview() {
    reassignmentPreview = null
    previewRequest = null
    applyRequestId = null
  }

  function toggle(values: string[], id: string): string[] {
    return values.includes(id) ? values.filter((item) => item !== id) : [...values, id]
  }

  function toggleClaim(id: string) {
    selectedClaimIds = toggle(selectedClaimIds, id)
    invalidatePreview()
  }

  function toggleReplacementRole(id: string) {
    replacementRoleIds = toggle(replacementRoleIds, id)
    invalidatePreview()
  }

  function toggleReplacementScope(id: string) {
    replacementScopeIds = toggle(replacementScopeIds, id)
    invalidatePreview()
  }

  function previewInput(): ReassignmentPreviewInput {
    if (!registry) throw new Error('Context Registry 尚未载入。')
    return {
      expected_protocol: registry.protocol,
      expected_registry_heads: registry.registry_heads,
      selector: { claim_ids: [...selectedClaimIds].sort() },
      replacement: {
        ...(replacementRoleIds.length ? { role_ids: [...replacementRoleIds].sort() } : {}),
        ...(replacementScopeIds.length ? { scope_ids: [...replacementScopeIds].sort() } : {}),
      },
      as_of_valid_time: new Date().toISOString(),
    }
  }

  async function previewReassignment() {
    if (!canPreview || busy) return
    setBusy(true)
    error = ''
    try {
      const input = previewInput()
      const result = await memoryReassignPreview(input)
      previewRequest = input
      reassignmentPreview = result
      applyRequestId = requestId('memory-ui/reassignment')
    } catch (cause) {
      error = hostError(cause)
    } finally {
      setBusy(false)
    }
  }

  async function applyReassignment() {
    if (!previewRequest || !reassignmentPreview || !previewCanApply || !applyRequestId || busy) return
    setBusy(true)
    error = ''
    try {
      await memoryReassignApply({
        ...previewRequest,
        request_id: applyRequestId,
        preview_sha256: reassignmentPreview.preview_sha256,
        gesture_intent: 'apply-reassignment',
      })
      announcement = `已重新分配 ${reassignmentPreview.matched.length} 条记忆。`
      selectedClaimIds = []
      replacementRoleIds = []
      replacementScopeIds = []
      invalidatePreview()
      await loadRegistry()
      await onchanged()
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause)
      error = hostError(cause)
      if (message.includes('MEMORY_STALE_BASE')) invalidatePreview()
    } finally {
      setBusy(false)
    }
  }

  function summaryText(value: unknown): string {
    if (typeof value === 'string') return value
    if (value && typeof value === 'object') {
      return Object.entries(value as Record<string, unknown>).map(([key, count]) => `${key} ${String(count)}`).join(' · ')
    }
    return ''
  }

  function changeText(value: unknown): string {
    if (typeof value === 'string') return value
    try { return JSON.stringify(value) } catch { return String(value) }
  }

  async function copyBootstrapPrompt() {
    error = ''
    try {
      await clipboardWrite(contextBootstrapPrompt)
      announcement = '已复制身份与场景初始化 Prompt'
      await toast('success', announcement, '粘贴给能读取 Vault 并运行 notemd CLI 的外部 Agent；Registry 会由 Agent 更新，Claim 分配仍由你确认。')
    } catch (cause) {
      error = hostError(cause)
    }
  }
</script>

<section class="manager-panel" aria-labelledby="role-scope-title">
    <header>
      <div><h2 id="role-scope-title">身份与场景</h2><p>初始化 Prompt 可让外部 Agent 根据 Vault 证据更新 Registry，并提交仍需你确认的 Claim 分配提案；当前会话 Context 独立管理。</p></div>
      <button class="copy-prompt" onclick={copyBootstrapPrompt} disabled={busy} title="让外部 Agent 基于 Vault 证据生成、校验并更新 Role/Scope，再提交可审阅的分配提案">复制初始化 Prompt</button>
    </header>
    <p class="sr-only" aria-live="polite">{announcement}</p>
    {#if error}<div class="error" role="alert">{error}</div>{/if}

    <nav class="views" aria-label="身份与场景区域">
      <button class:active={view === 'roles'} aria-current={view === 'roles' ? 'page' : undefined} onclick={() => view = 'roles'}>Roles</button>
      <button class:active={view === 'scopes'} aria-current={view === 'scopes' ? 'page' : undefined} onclick={() => view = 'scopes'}>Scopes</button>
      <button class:active={view === 'reassign'} aria-current={view === 'reassign' ? 'page' : undefined} onclick={() => view = 'reassign'}>重新分配</button>
    </nav>

    {#if loading}
      <div class="empty" aria-busy="true">正在读取 Context Registry…</div>
    {:else if registry}
      {#if !registry.writable}<div class="notice">当前 Registry 只读；仍可查看 Role、Scope 与重分配范围。</div>{/if}

      {#if view === 'roles'}
        <div class="registry-layout">
          <aside>
            <div class="list-heading"><strong>Roles</strong><button onclick={() => editRole()} disabled={!canWrite || busy}>新增 Role</button></div>
            <div class="registry-list" role="listbox" aria-label="Role 列表">
              {#each roles as role (role.id)}
                <button role="option" aria-selected={selectedRole?.id === role.id} class:selected={selectedRole?.id === role.id} class:archived={role.status === 'archived'} onclick={() => { selectedRoleId = role.id; roleDraft = null; statusChange = null }}>
                  <strong>{role.label}</strong><small>{role.id} · {role.status === 'active' ? '使用中' : '已归档'}</small>
                </button>
              {:else}<div class="empty compact">尚未建立 Role。</div>{/each}
            </div>
          </aside>
          <section class="registry-detail">
            {#if roleDraft}
              <h3>{roleDraftOriginalId ? '编辑 Role' : '新增 Role'}</h3>
              <div class="form-grid">
                <label>稳定 ID<input bind:value={roleDraft.id} readonly={!!roleDraftOriginalId} placeholder="role:developer" /></label>
                <label>名称<input bind:value={roleDraft.label} placeholder="开发者" /></label>
                <label class="wide">说明<textarea bind:value={roleDraft.description} rows="3"></textarea></label>
                <label class="wide">别名（逗号分隔）<input bind:value={roleAliases} /></label>
                <label class="wide">行为指导<textarea bind:value={roleDraft.guidance} rows="3"></textarea></label>
                <label class="wide">必须避免<textarea bind:value={roleDraft.avoid_error} rows="3"></textarea></label>
                {#if roleDraft.status === 'archived'}<label>替代 Role ID<input bind:value={roleDraft.redirect_to} placeholder="可选" /></label>{/if}
              </div>
              <footer><button onclick={() => roleDraft = null} disabled={busy}>取消</button><button class="primary" onclick={saveRole} disabled={busy || !canWrite || !roleDraft.id.trim() || !roleDraft.label.trim()}>{busy ? '正在保存…' : '保存 Role'}</button></footer>
            {:else if selectedRole}
              <div class="detail-heading"><div><span class="badge">Role</span><h3>{selectedRole.label}</h3><code>{selectedRole.id}</code></div><span class="status {selectedRole.status}">{selectedRole.status === 'active' ? '使用中' : '已归档'}</span></div>
              <p>{selectedRole.description || '未填写说明。'}</p>
              <dl><div><dt>别名</dt><dd>{selectedRole.aliases.join('、') || '无'}</dd></div><div><dt>行为指导</dt><dd>{selectedRole.guidance || '无'}</dd></div><div><dt>必须避免</dt><dd>{selectedRole.avoid_error || '无'}</dd></div>{#if selectedRole.redirect_to}<div><dt>替代项</dt><dd>{selectedRole.redirect_to}</dd></div>{/if}</dl>
              <footer><button onclick={() => editRole(selectedRole)} disabled={!canWrite || busy}>编辑</button><button onclick={() => statusChange = { kind: 'role', id: selectedRole.id, status: selectedRole.status === 'active' ? 'archived' : 'active' }} disabled={!canWrite || busy || (selectedRole.status === 'active' && activeRoles.length <= 1)} title={selectedRole.status === 'active' && activeRoles.length <= 1 ? 'Registry 至少需要一个使用中的 Role' : ''}>{selectedRole.status === 'active' ? '归档…' : '恢复…'}</button></footer>
            {:else}<div class="empty">选择或新增一个 Role。</div>{/if}
          </section>
        </div>
      {:else if view === 'scopes'}
        <div class="registry-layout">
          <aside>
            <div class="list-heading"><strong>Scopes</strong><button onclick={() => editScope()} disabled={!canWrite || busy}>新增 Scope</button></div>
            <div class="registry-list" role="listbox" aria-label="Scope 列表">
              {#each scopes as scope (scope.id)}
                <button role="option" aria-selected={selectedScope?.id === scope.id} class:selected={selectedScope?.id === scope.id} class:archived={scope.status === 'archived'} onclick={() => { selectedScopeId = scope.id; scopeDraft = null; statusChange = null }}>
                  <strong>{scope.label}</strong><small>{scope.kind === 'realm' ? 'Realm' : 'Space'} · {scope.security_domain} · {scope.status === 'active' ? '使用中' : '已归档'}</small>
                </button>
              {:else}<div class="empty compact">尚未建立 Scope。</div>{/each}
            </div>
          </aside>
          <section class="registry-detail">
            {#if scopeDraft}
              <h3>{scopeDraftOriginalId ? '编辑 Scope' : '新增 Scope'}</h3>
              <div class="form-grid">
                <label>稳定 ID<input bind:value={scopeDraft.id} readonly={!!scopeDraftOriginalId} placeholder="realm:client/acme" /></label>
                <label>名称<input bind:value={scopeDraft.label} placeholder="客户 A" /></label>
                <label>类型<select bind:value={scopeDraft.kind} disabled={!!scopeDraftOriginalId}><option value="realm">Realm</option><option value="space">Space</option></select></label>
                <label>安全域<input bind:value={scopeDraft.security_domain} readonly={!!scopeDraftOriginalId} placeholder="client/acme" /></label>
                {#if scopeDraft.kind === 'space'}<label>父 Scope<select bind:value={scopeDraft.parent_id}><option value="">请选择父 Scope</option>{#each scopes.filter((item) => item.id !== scopeDraft?.id && item.security_domain === scopeDraft?.security_domain) as option}<option value={option.id}>{option.label}</option>{/each}</select></label>{/if}
                <label class="wide">说明<textarea bind:value={scopeDraft.description} rows="3"></textarea></label>
                <label class="wide">别名（逗号分隔）<input bind:value={scopeAliases} /></label>
                {#if scopeDraft.status === 'archived'}<label>替代 Scope ID<input bind:value={scopeDraft.redirect_to} placeholder="可选" /></label>{/if}
              </div>
              <footer><button onclick={() => scopeDraft = null} disabled={busy}>取消</button><button class="primary" onclick={saveScope} disabled={busy || !canWrite || !scopeDraft.id.trim() || !scopeDraft.label.trim() || !scopeDraft.security_domain.trim() || (scopeDraft.kind === 'space' && !scopeDraft.parent_id?.trim())}>{busy ? '正在保存…' : '保存 Scope'}</button></footer>
            {:else if selectedScope}
              <div class="detail-heading"><div><span class="badge">{selectedScope.kind === 'realm' ? 'Realm' : 'Space'}</span><h3>{selectedScope.label}</h3><code>{selectedScope.id}</code></div><span class="status {selectedScope.status}">{selectedScope.status === 'active' ? '使用中' : '已归档'}</span></div>
              <p>{selectedScope.description || '未填写说明。'}</p>
              <dl><div><dt>安全域</dt><dd>{selectedScope.security_domain}</dd></div><div><dt>父 Scope</dt><dd>{selectedScope.parent_id || '无'}</dd></div><div><dt>别名</dt><dd>{selectedScope.aliases.join('、') || '无'}</dd></div>{#if selectedScope.redirect_to}<div><dt>替代项</dt><dd>{selectedScope.redirect_to}</dd></div>{/if}</dl>
              <footer><button onclick={() => editScope(selectedScope)} disabled={!canWrite || busy}>编辑</button><button onclick={() => statusChange = { kind: 'scope', id: selectedScope.id, status: selectedScope.status === 'active' ? 'archived' : 'active' }} disabled={!canWrite || busy || (selectedScope.status === 'active' && activeScopes.length <= 1)} title={selectedScope.status === 'active' && activeScopes.length <= 1 ? 'Registry 至少需要一个使用中的 Scope' : ''}>{selectedScope.status === 'active' ? '归档…' : '恢复…'}</button></footer>
            {:else}<div class="empty">选择或新增一个 Scope。</div>{/if}
          </section>
        </div>
      {:else}
        <section class="reassign">
          <header><div><h3>批量重新分配当前记忆</h3><p>先选择主张与目标，再预览完整影响；应用时只发送一次原子请求。</p></div><div><button onclick={() => { selectedClaimIds = currentClaims.map(({ claim }) => claim.claim_id); invalidatePreview() }} disabled={!currentClaims.length || busy}>全选当前</button><button onclick={() => { selectedClaimIds = []; invalidatePreview() }} disabled={!selectedClaimIds.length || busy}>清除</button></div></header>
          <div class="reassign-grid">
            <fieldset><legend>当前主张（{selectedClaimIds.length}/{currentClaims.length}）</legend><div class="checks claims">{#each currentClaims as item (item.claim.claim_id)}<label><input type="checkbox" aria-label={`选择主张 ${item.claim.claim_id}`} checked={selectedClaimIds.includes(item.claim.claim_id)} onchange={() => toggleClaim(item.claim.claim_id)} /><span><strong>{item.claim.text}</strong><small>{item.claim.context.spaces.join('、') || '未分配'}</small></span></label>{:else}<div class="empty compact">没有可重新分配的当前主张。</div>{/each}</div></fieldset>
            <div class="targets">
              <fieldset><legend>目标 Roles</legend><div class="checks">{#each activeRoles as role (role.id)}<label><input type="checkbox" aria-label={`目标 Role ${role.id}`} checked={replacementRoleIds.includes(role.id)} onchange={() => toggleReplacementRole(role.id)} />{role.label}</label>{:else}<small>没有可用 Role。</small>{/each}</div></fieldset>
              <fieldset><legend>目标 Scopes</legend><div class="checks">{#each activeScopes as scope (scope.id)}<label><input type="checkbox" aria-label={`目标 Scope ${scope.id}`} checked={replacementScopeIds.includes(scope.id)} onchange={() => toggleReplacementScope(scope.id)} />{scope.label} <small>{scope.kind}</small></label>{:else}<small>没有可用 Scope。</small>{/each}</div></fieldset>
            </div>
          </div>
          <div class="preview-actions"><button class="primary" onclick={previewReassignment} disabled={!canWrite || !canPreview || busy}>{busy ? '正在检查…' : '预览重新分配'}</button></div>
          {#if reassignmentPreview}
            <section class="preview" aria-live="polite">
              <header><div><strong>{reassignmentPreview.matched.length} 条匹配</strong><small>{summaryText(reassignmentPreview.summary)}</small></div><span>{previewCanApply ? '可以批量应用' : '包含必须单独处理的风险项'}</span></header>
              <div class="matches">{#each reassignmentPreview.matched as item (item.claim_id)}<article class:blocked={!item.batch_eligible}><div><code>{item.claim_id}</code><span>{item.risk_bucket}</span></div><p>{changeText(item.before)} → {changeText(item.after)}</p><small>{item.reasons.join(' · ') || '无额外说明'}</small></article>{/each}</div>
              <footer><button class="primary" onclick={applyReassignment} disabled={!previewCanApply || busy}>{busy ? '正在应用…' : `应用 ${reassignmentPreview.matched.length} 条重新分配`}</button></footer>
            </section>
          {/if}
        </section>
      {/if}
    {/if}

    {#if statusChange}
      <div class="status-confirm" role="alertdialog" aria-labelledby="status-change-title">
        <div><strong id="status-change-title">确认{statusChange.status === 'archived' ? '归档' : '恢复'}？</strong><p>{statusChange.status === 'archived' ? '归档后不再用于新的 Context；历史引用仍然保留。' : '恢复后可再次用于新的 Context；仍以它为当前归属的历史 Claim 也会重新生效。'}</p></div>
        <footer><button onclick={() => statusChange = null} disabled={busy}>取消</button><button class="primary" onclick={confirmStatusChange} disabled={busy}>确认{statusChange.status === 'archived' ? '归档' : '恢复'}</button></footer>
      </div>
    {/if}
</section>

<style>
  .manager-panel{min-width:0}.manager-panel>header,.reassign>header,.preview>header,.detail-heading,.list-heading{display:flex;justify-content:space-between;align-items:flex-start;gap:16px}.manager-panel h2,.manager-panel h3{margin:0}.manager-panel>header{padding:2px 2px 0}.manager-panel>header p,.reassign header p{margin:4px 0 0;color:var(--ui-secondary);font-size:12px}.manager-panel h2{font-size:16px}.copy-prompt{flex:none;background:transparent}.sr-only{position:absolute;width:1px;height:1px;padding:0;margin:-1px;overflow:hidden;clip:rect(0,0,0,0);white-space:nowrap;border:0}.error,.notice{margin-top:12px;padding:9px 11px;border-radius:8px;font-size:12px}.error{color:var(--ui-danger);background:color-mix(in srgb,#ff3b30 10%,Canvas)}.notice{background:color-mix(in srgb,#ff9f0a 10%,Canvas)}
  .views{display:grid;grid-template-columns:repeat(3,1fr);gap:3px;max-width:520px;margin:16px auto;padding:3px;border-radius:9px;background:color-mix(in srgb,CanvasText 8%,Canvas)}.views button{border:0;background:transparent}.views button.active{background:Canvas;box-shadow:0 1px 4px rgba(0,0,0,.16);font-weight:650}
  .registry-layout{display:grid;grid-template-columns:310px minmax(0,1fr);min-height:470px;border:1px solid color-mix(in srgb,CanvasText 12%,transparent);border-radius:10px;overflow:hidden}.registry-layout aside{border-right:1px solid color-mix(in srgb,CanvasText 10%,transparent);background:color-mix(in srgb,CanvasText 3%,Canvas)}.list-heading{align-items:center;padding:10px 11px}.registry-list{max-height:430px;overflow:auto}.registry-list>button{display:block;width:100%;padding:10px 12px;border:0;border-top:1px solid color-mix(in srgb,CanvasText 8%,transparent);border-radius:0;background:transparent;text-align:left}.registry-list>button strong,.registry-list>button small{display:block}.registry-list>button small{margin-top:3px;color:var(--ui-secondary)}.registry-list>button.selected{background:var(--ui-selection);color:CanvasText}.registry-list>button.selected small{color:var(--ui-secondary)}.registry-list>button.archived:not(.selected){opacity:.6}.registry-detail{padding:20px}.registry-detail>p{white-space:pre-wrap}.detail-heading h3{margin:8px 0 3px}.badge,.status{display:inline-block;padding:3px 7px;border-radius:6px;background:color-mix(in srgb,CanvasText 8%,Canvas);font-size:12px}.status.archived{color:var(--ui-warning);background:color-mix(in srgb,#ff9f0a 17%,Canvas)}.registry-detail dl{display:grid;gap:1px;margin:16px 0;background:color-mix(in srgb,CanvasText 8%,transparent)}.registry-detail dl>div{display:grid;grid-template-columns:110px minmax(0,1fr);gap:12px;padding:9px;background:Canvas}.registry-detail dt{color:var(--ui-secondary)}.registry-detail dd{margin:0;white-space:pre-wrap;overflow-wrap:anywhere}.registry-detail footer,.preview footer,.status-confirm footer{display:flex;justify-content:flex-end;gap:8px;margin-top:16px}
  .form-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:0 10px}.form-grid label{display:grid;gap:4px;margin-top:12px;color:var(--ui-secondary);font-size:12px}.form-grid .wide{grid-column:1/-1}
  .reassign{display:grid;gap:14px}.reassign>header>div:last-child{display:flex;gap:7px}.reassign-grid{display:grid;grid-template-columns:minmax(0,1.5fr) minmax(240px,1fr);gap:12px}.reassign fieldset{min-width:0;margin:0;padding:10px;border:1px solid color-mix(in srgb,CanvasText 11%,transparent);border-radius:9px}.reassign legend{padding:0 5px;font-weight:650}.targets{display:grid;gap:12px}.checks{display:grid;gap:7px;max-height:160px;overflow:auto}.checks.claims{max-height:335px}.checks label{display:flex;align-items:flex-start;gap:7px}.checks input{flex:none;width:auto;min-height:auto;margin-top:3px}.checks span,.checks strong,.checks small{display:block}.checks small{color:var(--ui-secondary)}.preview-actions{display:flex;justify-content:flex-end}.preview{padding:13px;border-radius:10px;background:color-mix(in srgb,CanvasText 4%,Canvas)}.preview>header span{font-size:12px}.preview>header small{display:block;margin-top:3px;color:var(--ui-secondary)}.matches{display:grid;gap:7px;margin-top:10px;max-height:220px;overflow:auto}.matches article{padding:9px;border:1px solid color-mix(in srgb,CanvasText 10%,transparent);border-radius:7px;background:Canvas}.matches article.blocked{border-color:color-mix(in srgb,#ff3b30 38%,transparent)}.matches article>div{display:flex;justify-content:space-between;gap:12px}.matches p{margin:5px 0;overflow-wrap:anywhere}.matches small{color:var(--ui-secondary)}
  .status-confirm{position:sticky;bottom:0;display:flex;justify-content:space-between;align-items:center;gap:18px;margin:14px -10px -10px;padding:12px;border:1px solid color-mix(in srgb,#ff9f0a 40%,transparent);border-radius:9px;background:Canvas;box-shadow:0 -8px 24px rgba(0,0,0,.12)}.status-confirm p{margin:3px 0 0;color:var(--ui-secondary);font-size:12px}.status-confirm footer{margin:0}.empty{padding:44px 16px;text-align:center;color:var(--ui-secondary)}.empty.compact{padding:20px}
  @media(max-width:720px){.manager-panel>header{display:block}.copy-prompt{margin-top:10px}.registry-layout,.reassign-grid{grid-template-columns:1fr}.registry-layout aside{border-right:0;border-bottom:1px solid color-mix(in srgb,CanvasText 10%,transparent)}.registry-list{max-height:180px}.form-grid{grid-template-columns:1fr}.form-grid .wide{grid-column:auto}.status-confirm{display:block}}
</style>
