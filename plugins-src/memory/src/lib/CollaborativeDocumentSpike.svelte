<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { bridge } from './bridge'
  import {
    uuidIds,
    type AppliedChange,
    type Assessment,
    type DocumentRevision,
    type OperationBatch,
    type Proposal,
  } from './cdr/session'
  import {
    PersistentDocumentSession,
    RepositoryConflictError,
    RepositoryOutcomeUnknownError,
  } from './cdr/repository'
  import { loadKitV2, replaceOperation, type MountedDocumentEditor, type SurfaceUpdate } from './editor-kit-v2'

  class EditorSyncError extends Error {}

  const ids = uuidIds()
  const INITIAL_REVISION_ID = 'memory-spike/revision-1'
  const initialBlocks: DocumentRevision['blocks'] = [
    { blockId: 'b-a1b2c3', blockRevision: 'b-a1b2c3/1', markdown: '# MEMORY-first 共写文档' },
    { blockId: 'b-d4e5f6', blockRevision: 'b-d4e5f6/1', markdown: '这是一段可由人直接修改的背景叙事，保留前因后果和团队黑话。' },
    { blockId: 'b-0a1b2c', blockRevision: 'b-0a1b2c/1', markdown: '- Agent 修改必须绑定块版本\n- 冲突不得静默覆盖' },
  ]

  let session = $state<PersistentDocumentSession | null>(null)
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
    void mountEditor()
    return () => {
      disposed = true
      stopObserving?.()
      stopObserving = null
      if (editor) void editor.surface.destroy()
      editor = null
    }
  })

  async function mountEditor() {
    try {
      const initial = await initialForCurrentVault()
      const restoredSession = await PersistentDocumentSession.open(initial, ids)
      if (disposed) return
      session = restoredSession
      const mount = await loadKitV2()
      if (disposed) return
      if (!editorHost) throw new Error('CDR_EDITOR_HOST_MISSING')
      editor = await mount(editorHost, {
        snapshot: restoredSession.snapshot(),
        ids,
        onBlockedStructuralEdit: () => {
          notice = 'Stage 0 仅允许块内编辑；插入、删除和拆合块已 fail-closed。'
        },
        onResyncRequired: (reason) => {
          void handleResyncRequired(reason)
        },
      })
      stopObserving = editor.surface.observeLocalOperations(handleLocalOperations)
      loading = false
      notice = restoredSession.openKind === 'restored'
        ? '已恢复上次提交的正文、提案、核验与审计记录。'
        : '已创建本机文档：直接修改正文，每次只提交受影响的块。'
      syncViewModels()
    } catch (cause) {
      loading = false
      failed = cause instanceof Error ? cause.message : String(cause)
    }
  }

  async function initialForCurrentVault(): Promise<DocumentRevision> {
    const info = await bridge().request('host.vault.info', {}) as { root?: unknown }
    if (typeof info?.root !== 'string' || !info.root) throw new Error('CDR_VAULT_REQUIRED')
    if (typeof globalThis.crypto?.subtle?.digest !== 'function') throw new Error('CDR_SHA256_UNAVAILABLE')
    const digest = await globalThis.crypto.subtle.digest('SHA-256', new TextEncoder().encode(info.root))
    const namespace = [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, '0')).join('')
    return {
      documentId: `memory-spike/${namespace}`,
      revisionId: INITIAL_REVISION_ID,
      blocks: initialBlocks.map((block) => ({ ...block })),
    }
  }

  function handleLocalOperations(batch: OperationBatch) {
    void persistLocalOperations(batch)
  }

  async function persistLocalOperations(batch: OperationBatch) {
    if (!editor || !session) return
    activeWrites += 1
    notice = '正在原子保存本地修改…'
    let durable = false
    try {
      const result = await session.submit(batch, 'human:local')
      if (result.kind === 'conflicted') {
        const preserved = await session.propose(batch, 'human:local')
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
      if (cause instanceof RepositoryOutcomeUnknownError) {
        lockEditor('无法核定本次保存结果；已切换为只读，请重开窗口重新读取。')
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
    if (!session) return
    await runAction(async () => {
      const proposal = await session!.propose(
        batchFor('b-d4e5f6', '这段背景由人和 Agent 共同维护；Agent 先给出可审阅的局部建议。'),
        'agent:organizer',
      )
      notice = `整理 Agent 已保存提案 · ${proposal.changeSetId}`
    })
  }

  async function applyRemoteFixture() {
    if (!session) return
    const current = session.snapshot().blocks.find((block) => block.blockId === 'b-0a1b2c')?.markdown ?? ''
    const markdown = current.includes('保留两侧内容')
      ? '- Agent 修改必须绑定块版本\n- 冲突不得静默覆盖'
      : '- Agent 修改必须绑定块版本\n- 冲突必须显示并保留两侧内容'
    editor?.decorations.setLayer('active-run', [{ blockId: 'b-0a1b2c', kind: 'activity', label: '正在接收已授权的远端变更…' }])
    try {
      await waitForPaint()
      activeWrites += 1
      notice = '正在原子保存已授权的远端变更…'
      const result = await session.submit(batchFor('b-0a1b2c', markdown), 'fixture:authorized-remote')
      if (result.kind === 'applied') {
        await applyRemote(result.change)
        notice = `已保存并应用模拟服务端变更 · ${result.change.revisionId}`
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
    if (!session) return
    await runAction(async () => {
      const assessment = await session!.assess('b-d4e5f6', 'agent:verifier', 'verified')
      notice = `核验结论已保存并绑定 ${assessment.blockRevision}`
    })
  }

  async function decide(proposal: Proposal, decision: 'accept' | 'reject') {
    if (!session) return
    await runAction(async () => {
      const result = await session!.decideProposal(proposal.changeSetId, decision, 'human:local')
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
    if (!session) return
    await runAction(async () => {
      const stale = await session!.propose(
        batchFor('b-d4e5f6', '这是一份基于旧版本的 Agent 建议。'),
        'agent:organizer',
      )
      const direct = await session!.submit(
        batchFor('b-d4e5f6', '人类已先一步改写这段背景，旧提案不应覆盖它。'),
        'human:local',
      )
      if (direct.kind === 'applied') await applyRemote(direct.change)
      const result = await session!.decideProposal(stale.changeSetId, 'accept', 'human:local')
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
    if (cause instanceof RepositoryOutcomeUnknownError) {
      lockEditor('无法核定本次保存结果；已切换为只读，请重开窗口重新读取。')
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
    try {
      editor?.surface.setReadOnly(true)
    } catch {
      // The UI state still disables every command in this view.
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
        blockId: proposal.batch.operations[0].blockId,
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
</script>

<section class="cdr-spike" aria-labelledby="cdr-spike-title">
  <header>
    <div>
      <p class="eyebrow">Stage 0 · 本机技术验证</p>
      <h2 id="cdr-spike-title">共写文档实验</h2>
      <p>通用块操作以 MEMORY 作为第一个验证场景。提交状态按当前 vault 隔离并保存在插件本机仓库，窗口重开后恢复；当前不读写 Claim、根 MEMORY.md 或 Yjs。</p>
    </div>
    <span class="status" class:loading={loading || saving || locked}>{loading ? '加载中' : failed ? '不可用' : locked ? '只读' : saving ? '保存中' : '可编辑'}</span>
  </header>

  {#if failed}
    <div class="failure" role="alert">
      <strong>共写文档初始化失败</strong>
      <span>{failed}</span>
    </div>
  {:else}
    <div class="workbench">
      <div class="document-shell" aria-busy={loading}>
        <div class="editor-host" bind:this={editorHost}></div>
      </div>
      <aside aria-label="协作活动与提案">
        <section class="actions">
          <h3>验证动作</h3>
          <button onclick={proposeBackground} disabled={!editor || !session || saving || locked}>Agent A 提出建议</button>
          <button onclick={applyRemoteFixture} disabled={!editor || !session || saving || locked}>模拟已授权远端变更</button>
          <button onclick={assessBackground} disabled={!editor || !session || saving || locked}>核验背景块</button>
          <button onclick={createStaleConflict} disabled={!editor || !session || saving || locked}>验证 stale-base</button>
        </section>

        <section class="activity" aria-live="polite">
          <h3>当前状态</h3>
          <p>{notice}</p>
          <small>{session?.snapshot().revisionId ?? INITIAL_REVISION_ID} · {auditCount} 个审计事件</small>
        </section>

        <section class="proposal-list">
          <h3>提案</h3>
          {#each proposals as proposal (proposal.changeSetId)}
            <article class:conflicted={proposal.status === 'conflicted'}>
              <strong>{proposal.actorId}</strong>
              <span>{proposal.batch.operations[0].markdown}</span>
              <small>{proposal.status} · 基于 {proposal.batch.operations[0].expectedBlockRevision}</small>
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
  .cdr-spike{display:grid;gap:14px}.cdr-spike>header{display:flex;align-items:flex-start;justify-content:space-between;gap:20px}.cdr-spike h2{margin:2px 0 5px;font-size:20px}.cdr-spike header p:not(.eyebrow){max-width:720px;margin:0;color:color-mix(in srgb,CanvasText 62%,transparent);font-size:12px}.eyebrow{margin:0;color:#0a84ff;font-size:11px;font-weight:750;letter-spacing:.04em;text-transform:uppercase}.status{flex:none;padding:4px 9px;border-radius:999px;background:color-mix(in srgb,#34c759 14%,Canvas);color:#168333;font-size:11px;font-weight:700}.status.loading{background:color-mix(in srgb,CanvasText 8%,Canvas);color:color-mix(in srgb,CanvasText 55%,transparent)}.workbench{display:grid;grid-template-columns:minmax(0,1fr) 310px;min-height:540px;overflow:hidden;border:1px solid color-mix(in srgb,CanvasText 12%,transparent);border-radius:12px;background:Canvas}.document-shell{min-width:0;padding:28px 34px;overflow:auto;border-right:1px solid color-mix(in srgb,CanvasText 10%,transparent)}.editor-host{min-height:430px}.document-shell :global(.kit-host){height:100%;min-height:430px}.document-shell :global(.moraya-editor){min-height:410px;padding:0;outline:0}.workbench aside{display:grid;align-content:start;gap:14px;padding:16px;overflow:auto;background:color-mix(in srgb,CanvasText 2.5%,Canvas)}aside section{display:grid;gap:7px}aside h3{margin:0;font-size:12px}.actions button{width:100%;text-align:left}.activity{padding:11px;border-radius:9px;background:color-mix(in srgb,#0a84ff 8%,Canvas)}.activity p{margin:0;font-size:12px;line-height:1.5}.activity small,.proposal-list small{color:color-mix(in srgb,CanvasText 52%,transparent);font-size:10px}.proposal-list article{display:grid;gap:6px;padding:10px;border:1px solid color-mix(in srgb,#ff9f0a 30%,transparent);border-radius:9px;background:Canvas}.proposal-list article.conflicted{border-color:color-mix(in srgb,#ff3b30 38%,transparent)}.proposal-list article>strong{font-size:11px}.proposal-list article>span{font-size:12px;white-space:pre-wrap}.proposal-list article>div{display:flex;justify-content:flex-end;gap:6px}.assessment-list p{display:grid;gap:2px;margin:0;padding:9px;border-radius:8px;background:Canvas;font-size:11px}.assessment-list p.outdated{background:color-mix(in srgb,#ff3b30 7%,Canvas)}.assessment-list span{color:color-mix(in srgb,CanvasText 54%,transparent);font-size:10px}.assessment-list small{color:#d62d26}.empty{margin:0;color:color-mix(in srgb,CanvasText 45%,transparent);font-size:11px}.failure{display:grid;gap:4px;padding:18px;border:1px solid color-mix(in srgb,#ff3b30 35%,transparent);border-radius:10px;background:color-mix(in srgb,#ff3b30 7%,Canvas)}.failure span{font:11px ui-monospace,SFMono-Regular,monospace}.primary{background:#0a84ff!important;color:#fff!important}@media(max-width:800px){.workbench{grid-template-columns:1fr}.document-shell{border-right:0;border-bottom:1px solid color-mix(in srgb,CanvasText 10%,transparent)}.workbench aside{grid-template-columns:repeat(2,minmax(0,1fr))}.activity{grid-column:1/-1}}@media(max-width:580px){.cdr-spike>header{display:block}.status{display:inline-block;margin-top:10px}.document-shell{padding:20px}.workbench aside{grid-template-columns:1fr}.activity{grid-column:auto}}
</style>
