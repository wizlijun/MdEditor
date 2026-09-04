<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { bridge } from './bridge'
  import {
    documentId as newDocumentId,
    sha256Hex,
    uuidIds,
    type AppliedChange,
    type Assessment,
    type DocumentRevision,
    type Operation,
    type OperationBatch,
    type Proposal,
  } from './cdr/session'
  import { CdrApplicationService, fixedActorSource } from './cdr/application'
  import { localMemoryAuthorizer, MEMORY_SELF_PROFILE_DESCRIPTOR, memorySelfProfile } from './cdr/profile'
  import {
    PersistentDocumentSession,
    RepositoryConflictError,
    RepositoryOutcomeUnknownError,
    RepositoryWriteBlockedError,
  } from './cdr/repository'
  import {
    ManagedDocumentStore,
    canonicalMemoryFrontmatter,
    inspectManagedDocument,
    managedMemoryPath,
  } from './cdr/managed-document'
  import { loadKitV2, replaceOperation, type MountedDocumentEditor, type SurfaceUpdate } from './editor-kit-v2'

  class EditorSyncError extends Error {}

  const ids = uuidIds()
  const INITIAL_REVISION_ID = 'memory-spike/revision-1'
  const initialBlocks = [
    { blockId: 'b-a1b2c3', markdown: '# MEMORY-first 共写文档' },
    { blockId: 'b-d4e5f6', markdown: '这是一段可由人直接修改的背景叙事，保留前因后果和团队黑话。' },
    { blockId: 'b-0a1b2c', markdown: '- Agent 修改必须绑定块版本\n- 冲突不得静默覆盖' },
  ]

  let session = $state<PersistentDocumentSession | null>(null)
  let humanCdr = $state<CdrApplicationService | null>(null)
  let organizerCdr = $state<CdrApplicationService | null>(null)
  let verifierCdr = $state<CdrApplicationService | null>(null)
  let collaboratorCdr = $state<CdrApplicationService | null>(null)
  let managedStore = $state<ManagedDocumentStore | null>(null)
  let vaultPath = $state('')
  let creationPending = $state(false)
  let creationError = $state('')
  let editorHost = $state<HTMLDivElement>()
  let editor = $state<MountedDocumentEditor | null>(null)
  let proposals = $state<readonly Proposal[]>([])
  let assessments = $state<readonly Assessment[]>([])
  let auditCount = $state(0)
  let loading = $state(true)
  let activeWrites = $state(0)
  let saving = $derived(activeWrites > 0)
  let locked = $state(false)
  let notice = $state('正在恢复共写文档…')
  let failed = $state('')
  let disposed = false
  let stopObserving: (() => void) | null = null

  onMount(() => {
    void initializeManagedDocument()
    return () => {
      disposed = true
      stopObserving?.()
      stopObserving = null
      if (editor) void editor.surface.destroy()
      editor = null
    }
  })

  async function initializeManagedDocument() {
    try {
      const info = await bridge().request('host.vault.info', {}) as { root?: unknown; wiki_dir?: unknown }
      if (typeof info?.root !== 'string' || !info.root) throw new Error('CDR_VAULT_REQUIRED')
      if (typeof info.wiki_dir !== 'string' || !info.wiki_dir) throw new Error('CDR_WIKI_DIR_REQUIRED')
      vaultPath = managedMemoryPath(info.wiki_dir)
      const inspection = await inspectManagedDocument(vaultPath)
      if (inspection.kind === 'missing') {
        loading = false
        creationPending = true
        notice = '尚未创建受控 MEMORY 文档；只有点击创建后才会写入 vault。'
        return
      }
      await openManagedDocument(inspection.documentId)
    } catch (cause) {
      loading = false
      failed = cause instanceof Error ? cause.message : String(cause)
    }
  }

  async function initialRevision(documentId: string): Promise<DocumentRevision> {
    return {
      documentId,
      revisionId: INITIAL_REVISION_ID,
      blocks: await Promise.all(initialBlocks.map(async (block) => ({
        ...block,
        blockRevision: await sha256Hex(block.markdown),
      }))),
    }
  }

  async function createManagedDocument() {
    if (!creationPending || !vaultPath || saving) return
    activeWrites += 1
    creationError = ''
    notice = '正在创建受控 MEMORY 文档…'
    try {
      const documentId = newDocumentId()
      await openManagedDocument(documentId, canonicalMemoryFrontmatter(documentId))
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : String(cause)
      if (session) failed = message
      else {
        creationError = message
        notice = '创建失败，未写入受控文档；可在问题解决后重试。'
      }
    } finally {
      activeWrites -= 1
    }
  }

  async function openManagedDocument(documentId: string, frontmatter: string | null = null) {
    const store = new ManagedDocumentStore(vaultPath, documentId, frontmatter)
    const restoredSession = await PersistentDocumentSession.open(await initialRevision(documentId), ids, store)
    if (disposed) return
    managedStore = store
    session = restoredSession
    humanCdr = new CdrApplicationService(
      documentId,
      MEMORY_SELF_PROFILE_DESCRIPTOR,
      restoredSession,
      fixedActorSource({ kind: 'human', id: 'local' }),
      localMemoryAuthorizer,
      memorySelfProfile,
    )
    organizerCdr = new CdrApplicationService(
      documentId,
      MEMORY_SELF_PROFILE_DESCRIPTOR,
      restoredSession,
      fixedActorSource({ kind: 'agent', id: 'organizer/simulated' }),
      localMemoryAuthorizer,
      memorySelfProfile,
    )
    verifierCdr = new CdrApplicationService(
      documentId,
      MEMORY_SELF_PROFILE_DESCRIPTOR,
      restoredSession,
      fixedActorSource({ kind: 'agent', id: 'verifier/simulated' }),
      localMemoryAuthorizer,
      memorySelfProfile,
    )
    collaboratorCdr = new CdrApplicationService(
      documentId,
      MEMORY_SELF_PROFILE_DESCRIPTOR,
      restoredSession,
      fixedActorSource({ kind: 'human', id: 'collaborator/simulated' }),
      localMemoryAuthorizer,
      memorySelfProfile,
    )
    creationPending = false
    await tick()
    const mount = await loadKitV2()
    if (disposed) return
    if (!editorHost) throw new Error('CDR_EDITOR_HOST_MISSING')
    const status = store.managedStatus
    const startsReadOnly = status.readOnlyReason !== null
    editor = await mount(editorHost, {
      snapshot: restoredSession.snapshot(),
      ids,
      readOnly: startsReadOnly,
      onBlockedStructuralEdit: () => {
        notice = '请使用显式新增／删除命令；键盘拆分、合并和移动仍会 fail-closed。'
      },
      onResyncRequired: (reason) => {
        void handleResyncRequired(reason)
      },
    })
    if (!startsReadOnly) stopObserving = editor.surface.observeLocalOperations(handleLocalOperations)
    loading = false
    if (status.readOnlyReason) {
      locked = true
      notice = status.readOnlyReason
    } else {
      notice = restoredSession.openKind === 'restored'
        ? '已从受控 Markdown 与同代聚合恢复正文、提案、核验与审计记录。'
        : '受控 MEMORY 文档已创建；正文与结构化历史会作为一个逻辑提交保存。'
    }
    syncViewModels()
  }

  function handleLocalOperations(batch: OperationBatch) {
    if (locked) return
    void persistLocalOperations(batch)
  }

  async function persistLocalOperations(batch: OperationBatch) {
    if (!editor || !session) return
    activeWrites += 1
    notice = '正在原子保存本地修改…'
    let durable = false
    try {
      if (!humanCdr) throw new Error('CDR_APPLICATION_NOT_READY')
      const result = await humanCdr.submit(batch)
      if (result.kind === 'proposed') throw new Error('CDR_HUMAN_APPLY_DOWNGRADED')
      if (result.kind === 'conflicted') {
        const preserved = await humanCdr.propose(batch)
        durable = true
        const synchronized = await reconcileOrLock({
          kind: 'reject-local',
          requestId: batch.requestId,
          reason: result.conflict,
          authoritative: session.snapshot(),
          includedChangeIds: [],
        }, session.snapshot())
        if (!synchronized) return
        notice = `${result.conflict.message} 本地文字已保存为待比较提案 · ${preserved.changeSetId}`
      } else {
        durable = true
        const synchronized = await reconcileOrLock({
          kind: 'ack-local',
          requestId: batch.requestId,
          authoritative: result.snapshot,
          includedChangeIds: [result.change.changeId],
        }, result.snapshot)
        if (!synchronized) return
        notice = `人类局部修改已保存 · ${result.change.revisionId}`
      }
    } catch (cause) {
      if (durable) {
        lockEditor('修改已经保存，但编辑器未能确认；已切换为只读，窗口重开后恢复。')
        return
      }
      if (cause instanceof RepositoryOutcomeUnknownError || cause instanceof RepositoryWriteBlockedError) {
        lockEditor(managedStore?.managedStatus.readOnlyReason
          ?? '无法核定本次保存结果；已切换为只读，请重开窗口重新读取。')
        return
      }
      const current = session.snapshot()
      const conflict = cause instanceof RepositoryConflictError
      const synchronized = await reconcileOrLock({
        kind: 'reject-local',
        requestId: batch.requestId,
        reason: {
          code: conflict ? 'stale-base' : 'persistence-failed',
          message: conflict
            ? '存储中的文档已由另一写入者更新；已恢复最新提交版本。'
            : '本地修改未能持久化；已恢复上次提交版本。',
        },
        authoritative: current,
        includedChangeIds: [],
      }, current)
      if (!synchronized) return
      notice = conflict
        ? '检测到并发保存冲突；本次修改未覆盖另一写入者。'
        : '保存失败；本次修改未生效，也未显示为已保存。'
    } finally {
      activeWrites -= 1
      syncViewModels()
    }
  }

  function batchFor(blockId: string, markdown: string): OperationBatch {
    if (!session) throw new Error('CDR_SESSION_NOT_READY')
    const snapshot = session.snapshot()
    const block = snapshot.blocks.find((item) => item.blockId === blockId)
    if (!block) throw new Error('CDR_BLOCK_NOT_FOUND')
    return {
      requestId: ids.requestId(),
      documentId: snapshot.documentId,
      baseRevisionId: snapshot.revisionId,
      operations: [replaceOperation(ids, blockId, block.blockRevision, markdown)],
    }
  }

  async function applyRemote(change: AppliedChange) {
    if (!session) return
    if (!await reconcileOrLock({ kind: 'apply-remote', change }, session.snapshot())) {
      throw new EditorSyncError('CDR_EDITOR_SYNC_FAILED')
    }
  }

  async function handleResyncRequired(reason: { changeId?: string }) {
    if (!session) return
    const snapshot = session.snapshot()
    const synchronized = await reconcileOrLock({
      kind: 'resync',
      snapshot,
      includedChangeIds: reason.changeId ? [reason.changeId] : [],
    }, snapshot)
    if (synchronized) notice = '检测到远端版本缺口，已从当前权威快照重新同步。'
  }

  async function waitForPaint() {
    await tick()
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
  }

  async function proposeBackground() {
    if (!session || !organizerCdr) return
    await runAction(async () => {
      const result = await organizerCdr!.submit(
        batchFor('b-d4e5f6', '这段背景由人和 Agent 共同维护；Agent 先给出可审阅的局部建议。'),
        'apply',
      )
      if (result.kind !== 'proposed') throw new Error('CDR_AGENT_APPLY_NOT_DOWNGRADED')
      notice = `模拟整理 Agent 的 apply 请求已按策略降级并保存为提案 · ${result.proposal.changeSetId}`
    })
  }

  async function applyRemoteFixture() {
    if (!session || !collaboratorCdr) return
    const current = session.snapshot().blocks.find((block) => block.blockId === 'b-0a1b2c')?.markdown ?? ''
    const markdown = current.includes('保留两侧内容')
      ? '- Agent 修改必须绑定块版本\n- 冲突不得静默覆盖'
      : '- Agent 修改必须绑定块版本\n- 冲突必须显示并保留两侧内容'
    editor?.decorations.setLayer('active-run', [{ blockId: 'b-0a1b2c', kind: 'activity', label: '正在应用模拟协作者变更…' }])
    try {
      await waitForPaint()
      activeWrites += 1
      notice = '正在原子保存模拟协作者变更…'
      const result = await collaboratorCdr.submit(batchFor('b-0a1b2c', markdown))
      if (result.kind === 'proposed') throw new Error('CDR_COLLABORATOR_APPLY_DOWNGRADED')
      if (result.kind === 'applied') {
        await applyRemote(result.change)
        notice = `已保存并应用模拟协作者变更 · ${result.change.revisionId}`
      } else {
        notice = result.conflict.message
      }
    } catch (cause) {
      await recoverAfterActionFailure(cause)
    } finally {
      activeWrites -= 1
      editor?.decorations.removeLayer('active-run')
    }
    syncViewModels()
  }

  async function assessBackground() {
    if (!session || !verifierCdr) return
    await runAction(async () => {
      const assessment = await verifierCdr!.assess('b-d4e5f6', 'verified')
      notice = `模拟核验 Agent 的结论已保存并绑定 ${assessment.blockRevision}`
    })
  }

  function insertAfterSelection() {
    const blockId = editor?.surface.selectedBlockId?.()
    if (!editor || !blockId || !editor.surface.executeStructuralCommand?.({
      kind: 'block.insert-after',
      blockId,
      content: '新段落',
    })) {
      notice = '当前无法新增块；请先结束输入并选择一个正文块。'
      return
    }
    notice = '正在以稳定块 ID 新增段落…'
  }

  function deleteSelection() {
    const blockId = editor?.surface.selectedBlockId?.()
    if (!editor || !blockId || !editor.surface.executeStructuralCommand?.({ kind: 'block.delete', blockId })) {
      notice = '当前无法删除所选块；文档必须至少保留一个块。'
      return
    }
    notice = '正在删除所选块并保留历史记录…'
  }

  async function decide(proposal: Proposal, decision: 'accept' | 'reject') {
    if (!session || !humanCdr) return
    await runAction(async () => {
      const result = await humanCdr!.decideProposal(proposal.changeSetId, decision)
      if (result?.kind === 'applied') {
        await applyRemote(result.change)
        notice = '已保存采纳决定，并以局部事务更新正文。'
      } else if (result?.kind === 'conflicted') {
        notice = '提案冲突已保存；目标块没有被旧内容覆盖。'
      } else {
        notice = '拒绝决定已保存，正文未改变。'
      }
    })
  }

  async function createStaleConflict() {
    if (!session || !humanCdr || !organizerCdr) return
    await runAction(async () => {
      const stale = await organizerCdr!.propose(
        batchFor('b-d4e5f6', '这是一份基于旧版本的 Agent 建议。'),
      )
      const direct = await humanCdr!.submit(
        batchFor('b-d4e5f6', '人类已先一步改写这段背景，旧提案不应覆盖它。'),
      )
      if (direct.kind === 'proposed') throw new Error('CDR_HUMAN_APPLY_DOWNGRADED')
      if (direct.kind === 'applied') await applyRemote(direct.change)
      const result = await humanCdr!.decideProposal(stale.changeSetId, 'accept')
      notice = result?.kind === 'conflicted'
        ? '已保存 stale-base 冲突：旧提案未覆盖人类新版本。'
        : '未能制造预期冲突。'
    })
  }

  async function runAction(action: () => Promise<void>) {
    activeWrites += 1
    try {
      await action()
    } catch (cause) {
      await recoverAfterActionFailure(cause)
    } finally {
      activeWrites -= 1
      syncViewModels()
    }
  }

  async function recoverAfterActionFailure(cause: unknown) {
    if (cause instanceof EditorSyncError) return
    if (cause instanceof RepositoryOutcomeUnknownError || cause instanceof RepositoryWriteBlockedError) {
      lockEditor(managedStore?.managedStatus.readOnlyReason
        ?? '无法核定本次保存结果；已切换为只读，请重开窗口重新读取。')
      return
    }
    const conflict = cause instanceof RepositoryConflictError
    const message = conflict
      ? '检测到并发保存冲突；已恢复另一写入者的最新提交版本。'
      : '保存失败；当前界面仍以最后一次成功提交为准。'
    if (editor && session) {
      const snapshot = session.snapshot()
      if (!await reconcileOrLock({ kind: 'resync', snapshot, includedChangeIds: [] }, snapshot)) return
    }
    notice = message
  }

  async function reconcileOrLock(update: SurfaceUpdate, authoritative: DocumentRevision): Promise<boolean> {
    if (!editor) return false
    try {
      await editor.surface.reconcile(update)
      return true
    } catch {
      try {
        await editor.surface.reconcile({ kind: 'resync', snapshot: authoritative, includedChangeIds: [] })
        return true
      } catch {
        lockEditor('已提交状态无法同步到编辑器；已切换为只读，窗口重开后恢复。')
        return false
      }
    }
  }

  function lockEditor(message: string) {
    locked = true
    notice = message
    stopObserving?.()
    stopObserving = null
    try {
      editor?.surface.setReadOnly(true)
    } catch {
      const failedEditor = editor
      editor = null
      if (failedEditor) void failedEditor.surface.destroy().catch(() => undefined)
      editorHost?.replaceChildren()
    }
  }

  function syncViewModels() {
    if (!session) return
    const currentSession = session
    proposals = currentSession.proposals()
    assessments = currentSession.assessments()
    auditCount = currentSession.audit().length
    editor?.decorations.setLayer('proposals', proposals
      .filter((proposal) => proposal.status === 'pending' || proposal.status === 'conflicted')
      .map((proposal) => ({
        blockId: operationDisplayBlockId(proposal.batch.operations[0]),
        kind: 'proposal' as const,
        label: proposal.status === 'conflicted' ? 'Agent 提案已过期' : 'Agent 提案待审阅',
      })))
    editor?.decorations.setLayer('assessments', assessments
      .filter((assessment) => currentSession.assessmentIsOutdated(assessment))
      .map((assessment) => ({
        blockId: assessment.blockId,
        kind: 'assessment-outdated' as const,
        label: '核验结论已过期',
      })))
  }

  function operationDisplayBlockId(operation: Operation): string {
    if (operation.kind !== 'block.insert') return operation.target.blockId
    return operation.target.leftBlockId ?? operation.target.rightBlockId ?? ''
  }

  function operationSummary(operation: Operation): string {
    if (operation.kind === 'block.delete') return `删除块 ${operation.target.blockId}`
    return operation.payload.content
  }

  function operationBasis(operation: Operation): string {
    if (operation.kind !== 'block.insert') return operation.target.expectedBlockRevision
    return `${operation.target.leftBlockId ?? '文首'} → ${operation.target.rightBlockId ?? '文尾'}`
  }
</script>

<section class="cdr-spike" aria-labelledby="cdr-spike-title">
  <header>
    <div>
      <p class="eyebrow">Stage 1B-1 · MEMORY-first 结构验证</p>
      <h2 id="cdr-spike-title">共写文档实验</h2>
      <p>通用受控文档以 MEMORY 作为第一个验证场景。正文修改与显式新增／删除统一经过 Core 与本机模拟治理策略；模拟 Agent 只能提出建议，Host 可信身份尚未接入。当前不读写 Claim、根 MEMORY.md 或 Yjs，也尚未开放移动与结构撤销。</p>
      {#if vaultPath}<small class="managed-path">{vaultPath}</small>{/if}
    </div>
    <span class="status" class:loading={loading || saving || locked || creationPending}>{loading ? '加载中' : failed ? '不可用' : creationPending ? '未创建' : locked ? '只读' : saving ? '保存中' : '可编辑'}</span>
  </header>

  {#if failed}
    <div class="failure" role="alert">
      <strong>共写文档初始化失败</strong>
      <span>{failed}</span>
    </div>
  {:else if creationPending}
    <div class="create-panel">
      <strong>创建受控 MEMORY 文档</strong>
      <p>将在 <code>{vaultPath}</code> 创建一份带稳定文档身份的 Markdown。不会改写根 <code>MEMORY.md</code>，也不会自动纳管已有文件。</p>
      <button class="primary" onclick={createManagedDocument} disabled={saving}>创建受控 MEMORY 文档</button>
      <small aria-live="polite">{notice}</small>
      {#if creationError}<code class="creation-error" role="alert">{creationError}</code>{/if}
    </div>
  {:else}
    <div class="workbench">
      <div class="document-shell" aria-busy={loading}>
        <div class="editor-host" bind:this={editorHost}></div>
      </div>
      <aside aria-label="协作活动与提案">
        <section class="actions">
          <h3>验证动作</h3>
          <button onclick={insertAfterSelection} disabled={!editor || !session || saving || locked}>在选中块后新增段落</button>
          <button onclick={deleteSelection} disabled={!editor || !session || saving || locked}>删除选中块</button>
          <button onclick={proposeBackground} disabled={!editor || !session || saving || locked}>模拟 Agent 请求直接修改</button>
          <button onclick={applyRemoteFixture} disabled={!editor || !session || saving || locked}>模拟协作者修改</button>
          <button onclick={assessBackground} disabled={!editor || !session || saving || locked}>模拟 Agent 核验背景块</button>
          <button onclick={createStaleConflict} disabled={!editor || !session || saving || locked}>验证 stale-base</button>
        </section>

        <section class="activity" aria-live="polite">
          <h3>当前状态</h3>
          <p>{notice}</p>
          <small>{session?.snapshot().revisionId ?? INITIAL_REVISION_ID} · {session?.revisionHistory().length ?? 0} 个历史版本 · {auditCount} 个审计事件</small>
        </section>

        <section class="proposal-list">
          <h3>提案</h3>
          {#each proposals as proposal (proposal.changeSetId)}
            <article class:conflicted={proposal.status === 'conflicted'}>
              <strong>{proposal.actorId}</strong>
              <span>{operationSummary(proposal.batch.operations[0])}</span>
              <small>{proposal.status} · 基于 {operationBasis(proposal.batch.operations[0])}</small>
              {#if proposal.status === 'pending'}
                <div><button onclick={() => decide(proposal, 'reject')} disabled={saving || locked}>拒绝</button><button class="primary" onclick={() => decide(proposal, 'accept')} disabled={saving || locked}>接受</button></div>
              {/if}
            </article>
          {:else}
            <p class="empty">暂无待处理提案。</p>
          {/each}
        </section>

        <section class="assessment-list">
          <h3>核验</h3>
          {#each assessments as assessment (assessment.assessmentId)}
            <p class:outdated={session?.assessmentIsOutdated(assessment) ?? false}>
              <strong>{assessment.conclusion === 'verified' ? '已核验' : '需复核'}</strong>
              <span>{assessment.blockRevision}</span>
              {#if session?.assessmentIsOutdated(assessment)}<small>目标已改变</small>{/if}
            </p>
          {:else}
            <p class="empty">暂无核验记录。</p>
          {/each}
        </section>
      </aside>
    </div>
  {/if}
</section>

<style>
  .cdr-spike{display:grid;gap:14px}.cdr-spike>header{display:flex;align-items:flex-start;justify-content:space-between;gap:20px}.cdr-spike h2{margin:2px 0 5px;font-size:20px}.cdr-spike header p:not(.eyebrow){max-width:720px;margin:0;color:color-mix(in srgb,CanvasText 62%,transparent);font-size:12px}.managed-path{display:block;margin-top:6px;color:color-mix(in srgb,CanvasText 48%,transparent);font:10px ui-monospace,SFMono-Regular,monospace}.eyebrow{margin:0;color:#0a84ff;font-size:11px;font-weight:750;letter-spacing:.04em;text-transform:uppercase}.status{flex:none;padding:4px 9px;border-radius:999px;background:color-mix(in srgb,#34c759 14%,Canvas);color:#168333;font-size:11px;font-weight:700}.status.loading{background:color-mix(in srgb,CanvasText 8%,Canvas);color:color-mix(in srgb,CanvasText 55%,transparent)}.create-panel{display:grid;justify-items:start;gap:10px;padding:24px;border:1px solid color-mix(in srgb,CanvasText 12%,transparent);border-radius:12px;background:Canvas}.create-panel p{max-width:680px;margin:0;color:color-mix(in srgb,CanvasText 62%,transparent);font-size:12px;line-height:1.55}.create-panel small{color:color-mix(in srgb,CanvasText 52%,transparent);font-size:11px}.creation-error{max-width:100%;overflow-wrap:anywhere;color:#d62d26;font-size:10px}.workbench{display:grid;grid-template-columns:minmax(0,1fr) 310px;min-height:540px;overflow:hidden;border:1px solid color-mix(in srgb,CanvasText 12%,transparent);border-radius:12px;background:Canvas}.document-shell{min-width:0;padding:28px 34px;overflow:auto;border-right:1px solid color-mix(in srgb,CanvasText 10%,transparent)}.editor-host{min-height:430px}.document-shell :global(.kit-host){height:100%;min-height:430px}.document-shell :global(.moraya-editor){min-height:410px;padding:0;outline:0}.workbench aside{display:grid;align-content:start;gap:14px;padding:16px;overflow:auto;background:color-mix(in srgb,CanvasText 2.5%,Canvas)}aside section{display:grid;gap:7px}aside h3{margin:0;font-size:12px}.actions button{width:100%;text-align:left}.activity{padding:11px;border-radius:9px;background:color-mix(in srgb,#0a84ff 8%,Canvas)}.activity p{margin:0;font-size:12px;line-height:1.5}.activity small,.proposal-list small{color:color-mix(in srgb,CanvasText 52%,transparent);font-size:10px}.proposal-list article{display:grid;gap:6px;padding:10px;border:1px solid color-mix(in srgb,#ff9f0a 30%,transparent);border-radius:9px;background:Canvas}.proposal-list article.conflicted{border-color:color-mix(in srgb,#ff3b30 38%,transparent)}.proposal-list article>strong{font-size:11px}.proposal-list article>span{font-size:12px;white-space:pre-wrap}.proposal-list article>div{display:flex;justify-content:flex-end;gap:6px}.assessment-list p{display:grid;gap:2px;margin:0;padding:9px;border-radius:8px;background:Canvas;font-size:11px}.assessment-list p.outdated{background:color-mix(in srgb,#ff3b30 7%,Canvas)}.assessment-list span{color:color-mix(in srgb,CanvasText 54%,transparent);font-size:10px}.assessment-list small{color:#d62d26}.empty{margin:0;color:color-mix(in srgb,CanvasText 45%,transparent);font-size:11px}.failure{display:grid;gap:4px;padding:18px;border:1px solid color-mix(in srgb,#ff3b30 35%,transparent);border-radius:10px;background:color-mix(in srgb,#ff3b30 7%,Canvas)}.failure span{font:11px ui-monospace,SFMono-Regular,monospace}.primary{background:#0a84ff!important;color:#fff!important}@media(max-width:800px){.workbench{grid-template-columns:1fr}.document-shell{border-right:0;border-bottom:1px solid color-mix(in srgb,CanvasText 10%,transparent)}.workbench aside{grid-template-columns:repeat(2,minmax(0,1fr))}.activity{grid-column:1/-1}}@media(max-width:580px){.cdr-spike>header{display:block}.status{display:inline-block;margin-top:10px}.document-shell{padding:20px}.workbench aside{grid-template-columns:1fr}.activity{grid-column:auto}}
</style>
