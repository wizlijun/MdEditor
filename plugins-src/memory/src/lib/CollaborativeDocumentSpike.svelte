<script lang="ts">
  import { onMount, tick } from 'svelte'
  import {
    InMemoryDocumentSession,
    sequentialIds,
    type AppliedChange,
    type Assessment,
    type DocumentRevision,
    type OperationBatch,
    type Proposal,
    type SubmitResult,
  } from './cdr/session'
  import { loadKitV2, replaceOperation, type MountedDocumentEditor } from './editor-kit-v2'

  const ids = sequentialIds('memory-spike')
  const initial: DocumentRevision = {
    documentId: 'memory-spike/document-1',
    revisionId: 'memory-spike/revision-1',
    blocks: [
      { blockId: 'b-a1b2c3', blockRevision: 'b-a1b2c3/1', markdown: '# MEMORY-first 共写文档' },
      { blockId: 'b-d4e5f6', blockRevision: 'b-d4e5f6/1', markdown: '这是一段可由人直接修改的背景叙事，保留前因后果和团队黑话。' },
      { blockId: 'b-0a1b2c', blockRevision: 'b-0a1b2c/1', markdown: '- Agent 修改必须绑定块版本\n- 冲突不得静默覆盖' },
    ],
  }

  const session = new InMemoryDocumentSession(initial, ids)
  let editorHost = $state<HTMLDivElement>()
  let editor = $state<MountedDocumentEditor | null>(null)
  let proposals = $state<readonly Proposal[]>([])
  let assessments = $state<readonly Assessment[]>([])
  let auditCount = $state(0)
  let loading = $state(true)
  let notice = $state('正在加载 Editor Kit v2…')
  let failed = $state('')
  let agentConstraintVariant = false
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
      const mount = await loadKitV2()
      if (disposed) return
      if (!editorHost) throw new Error('CDR_EDITOR_HOST_MISSING')
      editor = await mount(editorHost, {
        snapshot: session.snapshot(),
        ids,
        onBlockedStructuralEdit: () => {
          notice = 'Stage 0 仅允许块内编辑；插入、删除和拆合块已 fail-closed。'
        },
        onResyncRequired: (reason) => {
          notice = '检测到远端版本缺口，已从当前权威快照重新同步。'
          if (editor) void editor.surface.reconcile({
            kind: 'resync',
            snapshot: session.snapshot(),
            includedChangeIds: reason.changeId ? [reason.changeId] : [],
          })
        },
      })
      stopObserving = editor.surface.observeLocalOperations(handleLocalOperations)
      loading = false
      notice = '已就绪：直接修改正文，每次只提交受影响的块。'
      syncViewModels()
    } catch (cause) {
      loading = false
      failed = cause instanceof Error ? cause.message : String(cause)
    }
  }

  function handleLocalOperations(batch: OperationBatch) {
    const result = session.submit(batch, 'human:local')
    if (!editor) return
    if (result.kind === 'conflicted') {
      const preserved = session.propose(batch, 'human:local')
      notice = `${result.conflict.message} 本地文字已保留为待比较提案 · ${preserved.changeSetId}`
      void editor.surface.reconcile({
        kind: 'reject-local',
        requestId: batch.requestId,
        reason: result.conflict,
        authoritative: result.snapshot,
        includedChangeIds: [],
      })
    } else {
      notice = `人类局部修改已提交 · ${result.change.revisionId}`
      void editor.surface.reconcile({
        kind: 'ack-local',
        requestId: batch.requestId,
        authoritative: result.snapshot,
        includedChangeIds: [result.change.changeId],
      })
    }
    syncViewModels()
  }

  function batchFor(blockId: string, markdown: string): OperationBatch {
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
    if (!editor) return
    await editor.surface.reconcile({ kind: 'apply-remote', change })
  }

  async function waitForPaint() {
    await tick()
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()))
  }

  function proposeBackground() {
    const proposal = session.propose(
      batchFor('b-d4e5f6', '这段背景由人和 Agent 共同维护；Agent 先给出可审阅的局部建议。'),
      'agent:organizer',
    )
    notice = `整理 Agent 已保存提案 · ${proposal.changeSetId}`
    syncViewModels()
  }

  async function applyRemoteFixture() {
    const markdown = agentConstraintVariant
      ? '- Agent 修改必须绑定块版本\n- 冲突不得静默覆盖'
      : '- Agent 修改必须绑定块版本\n- 冲突必须显示并保留两侧内容'
    agentConstraintVariant = !agentConstraintVariant
    editor?.decorations.setLayer('active-run', [{ blockId: 'b-0a1b2c', kind: 'activity', label: '正在接收已授权的远端变更…' }])
    try {
      await waitForPaint()
      const result = session.submit(batchFor('b-0a1b2c', markdown), 'fixture:authorized-remote')
      if (result.kind === 'applied') {
        await applyRemote(result.change)
        notice = `已应用模拟服务端变更 · ${result.change.revisionId}`
      } else {
        notice = result.conflict.message
      }
    } finally {
      editor?.decorations.removeLayer('active-run')
    }
    syncViewModels()
  }

  function assessBackground() {
    const assessment = session.assess('b-d4e5f6', 'agent:verifier', 'verified')
    notice = `核验结论已绑定 ${assessment.blockRevision}`
    syncViewModels()
  }

  async function decide(proposal: Proposal, decision: 'accept' | 'reject') {
    const result = session.decideProposal(proposal.changeSetId, decision, 'human:local')
    if (result?.kind === 'applied') {
      await applyRemote(result.change)
      notice = '已接受 Agent 提案并以局部事务更新正文。'
    } else if (result?.kind === 'conflicted') {
      notice = '提案的目标块已变化；冲突已保留，没有覆盖正文。'
    } else {
      notice = '提案已拒绝，正文未改变。'
    }
    syncViewModels()
  }

  async function createStaleConflict() {
    const stale = session.propose(
      batchFor('b-d4e5f6', '这是一份基于旧版本的 Agent 建议。'),
      'agent:organizer',
    )
    const direct = session.submit(
      batchFor('b-d4e5f6', '人类已先一步改写这段背景，旧提案不应覆盖它。'),
      'human:local',
    )
    if (direct.kind === 'applied') await applyRemote(direct.change)
    const result = session.decideProposal(stale.changeSetId, 'accept', 'human:local')
    notice = result?.kind === 'conflicted'
      ? '已验证 stale-base：旧提案未覆盖人类新版本。'
      : '未能制造预期冲突。'
    syncViewModels()
  }

  function syncViewModels() {
    proposals = session.proposals()
    assessments = session.assessments()
    auditCount = session.audit().length
    editor?.decorations.setLayer('proposals', proposals
      .filter((proposal) => proposal.status === 'pending' || proposal.status === 'conflicted')
      .map((proposal) => ({
        blockId: proposal.batch.operations[0].blockId,
        kind: 'proposal' as const,
        label: proposal.status === 'conflicted' ? 'Agent 提案已过期' : 'Agent 提案待审阅',
      })))
    editor?.decorations.setLayer('assessments', assessments
      .filter((assessment) => session.assessmentIsOutdated(assessment))
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
      <p>通用块操作以 MEMORY 作为第一个验证场景。当前不读写 Claim、根 MEMORY.md 或 Yjs；切换离开本页会重置实验数据。</p>
    </div>
    <span class="status" class:loading>{loading ? '加载中' : failed ? '不可用' : '可编辑'}</span>
  </header>

  {#if failed}
    <div class="failure" role="alert">
      <strong>Editor Kit v2 加载失败</strong>
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
          <button onclick={proposeBackground} disabled={!editor}>Agent A 提出建议</button>
          <button onclick={applyRemoteFixture} disabled={!editor}>模拟已授权远端变更</button>
          <button onclick={assessBackground} disabled={!editor}>核验背景块</button>
          <button onclick={createStaleConflict} disabled={!editor}>验证 stale-base</button>
        </section>

        <section class="activity" aria-live="polite">
          <h3>当前状态</h3>
          <p>{notice}</p>
          <small>{session.snapshot().revisionId} · {auditCount} 个审计事件</small>
        </section>

        <section class="proposal-list">
          <h3>提案</h3>
          {#each proposals as proposal (proposal.changeSetId)}
            <article class:conflicted={proposal.status === 'conflicted'}>
              <strong>{proposal.actorId}</strong>
              <span>{proposal.batch.operations[0].markdown}</span>
              <small>{proposal.status} · 基于 {proposal.batch.operations[0].expectedBlockRevision}</small>
              {#if proposal.status === 'pending'}
                <div><button onclick={() => decide(proposal, 'reject')}>拒绝</button><button class="primary" onclick={() => decide(proposal, 'accept')}>接受</button></div>
              {/if}
            </article>
          {:else}
            <p class="empty">暂无待处理提案。</p>
          {/each}
        </section>

        <section class="assessment-list">
          <h3>核验</h3>
          {#each assessments as assessment (assessment.assessmentId)}
            <p class:outdated={session.assessmentIsOutdated(assessment)}>
              <strong>{assessment.conclusion === 'verified' ? '已核验' : '需复核'}</strong>
              <span>{assessment.blockRevision}</span>
              {#if session.assessmentIsOutdated(assessment)}<small>目标已改变</small>{/if}
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
