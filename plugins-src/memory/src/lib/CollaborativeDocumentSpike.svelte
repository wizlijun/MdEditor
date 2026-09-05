<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { bridge, clipboardWrite } from './bridge'
  import {
    DOCUMENT_AGENT_POLL_MS,
    documentAgentReadiness,
    documentAgentStatus,
    interpretDocumentAgentStatus,
    startDocumentAgent,
  } from './document-agent'
  import type { AgentOption } from './agent-picker/types'
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
  import { loadKitV2, type EditorCommand, type EditorSurfaceState, type MountedDocumentEditor, type SurfaceUpdate } from './editor-kit-v2'

  class EditorSyncError extends Error {}

  let { agent }: { agent?: AgentOption } = $props()

  const ids = uuidIds()
  const INITIAL_REVISION_ID = 'memory-document/revision-1'
  const initialBlocks = [
    { blockId: 'b-a1b2c3', markdown: '# MEMORY Workspace' },
    { blockId: 'b-d4e5f6', markdown: '在这里记录需要长期保留的背景、术语、约束与共识。' },
    { blockId: 'b-0a1b2c', markdown: 'Agent 的建议需要由人审阅；版本变化后，旧检查结果会自动标记为过期。' },
  ]

  let session = $state<PersistentDocumentSession | null>(null)
  let humanCdr = $state<CdrApplicationService | null>(null)
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
  let surfaceState = $state<EditorSurfaceState>({ dirty: false, saving: false, readOnly: false, error: null, selectedBlockId: null })
  let saving = $derived(activeWrites > 0 || surfaceState.saving)
  let locked = $state(false)
  let editingDisabled = $derived(!editor || loading || locked || surfaceState.readOnly)
  let notice = $state('正在恢复共写文档…')
  let failed = $state('')
  let agentInstruction = $state('在不改变事实、范围和不确定性的前提下，让表述更清楚。')
  let activeAgentRun = $state<{
    action: 'suggest' | 'assess'
    runId: string
    harness: string
    documentId: string
    baseRevisionId: string
    blockId: string
    blockRevision: string
    content: string
    pollFailures: number
  } | null>(null)
  let agentTimer: ReturnType<typeof setTimeout> | undefined
  let agentStarting = $state(false)
  let agentBusy = $derived(agentStarting || activeAgentRun !== null)
  let agentReadiness = $derived(documentAgentReadiness(agent))
  let disposed = false
  let stopObserving: (() => void) | null = null
  let stopObservingState: (() => void) | null = null
  let addressForm = $state<'link' | 'image' | null>(null)
  let address = $state('')
  let addressLabel = $state('')
  let addressBlockId: string | null = null
  let addressBlockRevision: string | null = null
  let recoveryMarkdown = $state('')
  let lockedRecoveryMessage = $state('')
  let discardConfirm = $state(false)
  let selectedHistoryId = $state('')
  let history = $state<readonly DocumentRevision[]>([])
  let restoreConfirm = $state(false)
  let restoring = $state(false)
  let selectedHistory = $derived(history.find((revision) => revision.revisionId === selectedHistoryId))
  const toolbarCommands: { label: string; command: EditorCommand; title?: string }[] = [
    { label: '撤销', command: { kind: 'undo' }, title: '撤销 (⌘/Ctrl+Z)' },
    { label: '重做', command: { kind: 'redo' }, title: '重做 (⌘/Ctrl+Shift+Z)' },
    { label: '粗体', command: { kind: 'bold' }, title: '粗体 (⌘/Ctrl+B)' },
    { label: '斜体', command: { kind: 'italic' }, title: '斜体 (⌘/Ctrl+I)' },
    { label: '删除线', command: { kind: 'strikethrough' } },
    { label: '行内代码', command: { kind: 'code' } },
    { label: '高亮', command: { kind: 'highlight' } },
    { label: '引用', command: { kind: 'blockquote' } },
    { label: '无序列表', command: { kind: 'bullet-list' } },
    { label: '有序列表', command: { kind: 'ordered-list' } },
    { label: '任务列表', command: { kind: 'task-list' } },
    { label: '增加缩进', command: { kind: 'indent' } },
    { label: '减少缩进', command: { kind: 'outdent' } },
    { label: '代码块', command: { kind: 'code-block' } },
    { label: '分隔线', command: { kind: 'horizontal-rule' } },
    { label: '插入表格', command: { kind: 'table' } },
  ]
  const tableCommands: { label: string; command: EditorCommand }[] = [
    { label: '下方插入行', command: { kind: 'table.add-row' } },
    { label: '删除行', command: { kind: 'table.delete-row' } },
    { label: '右侧插入列', command: { kind: 'table.add-column' } },
    { label: '删除列', command: { kind: 'table.delete-column' } },
  ]

  onMount(() => {
    void initializeManagedDocument()
    return () => {
      disposed = true
      activeAgentRun = null
      stopObservingState?.()
      stopObservingState = null
      const unsubscribe = stopObserving
      stopObserving = null
      if (agentTimer) clearTimeout(agentTimer)
      if (editor) {
        // The surface flushes its debounced local edit during destroy. Keep the
        // observer and editor session alive for that final durable submission.
        void editor.surface.destroy().catch(() => undefined).finally(() => unsubscribe?.())
      } else {
        unsubscribe?.()
      }
    }
  })

  async function initializeManagedDocument() {
    try {
      const info = await bridge().request('host.vault.info', {}) as { root?: unknown; wiki_dir?: unknown; author?: unknown }
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
      await openManagedDocument(inspection.documentId, null, humanActor(info.author))
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
      const info = await bridge().request('host.vault.info', {}) as { author?: unknown }
      await openManagedDocument(documentId, canonicalMemoryFrontmatter(documentId), humanActor(info.author))
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

  function humanActor(value: unknown): { kind: 'human'; id: string } {
    if (typeof value !== 'string' || !value.startsWith('human:') || value.length <= 'human:'.length) {
      throw new Error('CDR_HOST_HUMAN_IDENTITY_REQUIRED')
    }
    return { kind: 'human', id: value.slice('human:'.length) }
  }

  async function openManagedDocument(
    documentId: string,
    frontmatter: string | null,
    actor: { kind: 'human'; id: string },
  ) {
    const store = new ManagedDocumentStore(vaultPath, documentId, frontmatter)
    const restoredSession = await PersistentDocumentSession.open(await initialRevision(documentId), ids, store)
    if (disposed) return
    managedStore = store
    session = restoredSession
    humanCdr = new CdrApplicationService(
      documentId,
      MEMORY_SELF_PROFILE_DESCRIPTOR,
      restoredSession,
      fixedActorSource(actor),
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
    const mountedEditor = await mount(editorHost, {
      snapshot: restoredSession.snapshot(),
      ids,
      readOnly: startsReadOnly,
      onBlockedStructuralEdit: () => {
        notice = '本次操作未能应用，请检查选区或等待输入完成后重试。'
      },
      onResyncRequired: (reason) => {
        void handleResyncRequired(reason)
      },
    })
    if (disposed) {
      await mountedEditor.surface.destroy()
      return
    }
    editor = mountedEditor
    stopObservingState = editor.surface.observeState((state) => {
      surfaceState = state
      if (state.error) recoveryMarkdown = mountedEditor.surface.getDraftMarkdown()
      else if (!state.dirty && !state.saving) { recoveryMarkdown = ''; discardConfirm = false }
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
          kind: 'proposal-stored',
          requestId: batch.requestId,
          changeSetId: preserved.changeSetId,
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
            ? '存储中的文档已由另一写入者更新；请比较当前草稿与最新提交。'
            : '本地修改未能持久化；草稿仍保留，可重试保存。',
        },
        authoritative: current,
        includedChangeIds: [],
      }, current)
      if (!synchronized) return
      notice = conflict
        ? '检测到并发保存冲突；本次修改未覆盖另一写入者。请保留草稿并比较最新版本。'
        : '保存失败；草稿已留在编辑器中。请重试保存，或先复制、下载草稿。'
    } finally {
      activeWrites -= 1
      syncViewModels()
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

  async function startAgentAction(action: 'suggest' | 'assess') {
    const instruction = agentInstruction.trim()
    if (!editor || !session || !instruction || agentBusy || locked || surfaceState.error) {
      notice = '请先结束输入、选择一个正文块并填写给 Agent 的要求。'
      return
    }
    const ready = agentReadiness
    if (!ready.ok) {
      notice = ready.message
      return
    }
    agentStarting = true
    notice = action === 'suggest' ? '正在启动 Agent 生成建议…' : '正在启动 Agent 检查当前版本…'
    try {
      // Read the exact committed block only after the editor's latest typing
      // has received a durable acknowledgement.
      await editor.surface.flush()
      if (disposed) return
      const blockId = editor.surface.selectedBlockId()
      const snapshot = session.snapshot()
      const block = snapshot.blocks.find((item) => item.blockId === blockId)
      if (!block) {
        notice = '请先选择一个正文块。'
        return
      }
      const selectedHarness = ready.providerId
      const result = await startDocumentAgent({
        action,
        documentId: snapshot.documentId,
        blockId: block.blockId,
        blockRevision: block.blockRevision,
        content: block.markdown,
        instruction,
      }, selectedHarness)
      if (disposed) return
      if (!result.ok) {
        notice = result.reason === 'agent-missing'
          ? '没有可用的 AI Agent；请先安装并启用一个 Agent 插件。'
          : `无法启动 Agent：${result.message}`
        return
      }
      activeAgentRun = {
        action,
        runId: result.runId,
        harness: selectedHarness,
        documentId: snapshot.documentId,
        baseRevisionId: snapshot.revisionId,
        blockId: block.blockId,
        blockRevision: block.blockRevision,
        content: block.markdown,
        pollFailures: 0,
      }
      setAgentActivity(action === 'suggest' ? 'Agent 正在为这一块准备建议…' : 'Agent 正在检查这一版本…')
      scheduleAgentPoll()
    } catch (cause) {
      notice = `无法启动 Agent：${cause instanceof Error ? cause.message : String(cause)}`
    } finally {
      if (!disposed) agentStarting = false
    }
  }

  function setAgentActivity(label: string) {
    const run = activeAgentRun
    if (!run) return
    editor?.decorations.setLayer('active-run', [{ blockId: run.blockId, kind: 'activity', label }])
    notice = label
  }

  function scheduleAgentPoll() {
    if (disposed || !activeAgentRun) return
    if (agentTimer) clearTimeout(agentTimer)
    agentTimer = setTimeout(() => { void pollAgentRun() }, DOCUMENT_AGENT_POLL_MS)
  }

  async function pollAgentRun() {
    const run = activeAgentRun
    if (!run || disposed) return
    try {
      const view = interpretDocumentAgentStatus(
        await documentAgentStatus(run.runId, run.harness),
        run.harness,
      )
      if (disposed || activeAgentRun?.runId !== run.runId) return
      if (view.kind === 'running') {
        activeAgentRun = { ...run, pollFailures: 0 }
        setAgentActivity(view.last || `Agent 已执行 ${view.steps} 步…`)
        scheduleAgentPoll()
        return
      }
      activeAgentRun = null
      editor?.decorations.removeLayer('active-run')
      if (view.kind === 'lost') {
        notice = '无法确认这次 Agent 运行的状态；没有修改文档。'
        return
      }
      if (!view.success) {
        notice = `Agent 运行失败：${view.message}`
        return
      }
      await persistAgentResult(run, view.providerId, view.result)
    } catch (cause) {
      if (disposed || activeAgentRun?.runId !== run.runId) return
      const failures = run.pollFailures + 1
      if (failures < 5) {
        activeAgentRun = { ...run, pollFailures: failures }
        scheduleAgentPoll()
      } else {
        activeAgentRun = null
        editor?.decorations.removeLayer('active-run')
        notice = `无法读取 Agent 结果：${cause instanceof Error ? cause.message : String(cause)}`
      }
    }
  }

  async function persistAgentResult(
    run: NonNullable<typeof activeAgentRun>,
    providerId: string,
    result: import('./document-agent').DocumentAgentResult,
  ) {
    if (!session || disposed) return
    const service = new CdrApplicationService(
      run.documentId,
      MEMORY_SELF_PROFILE_DESCRIPTOR,
      session,
      fixedActorSource({ kind: 'agent', id: `${providerId}/run/${run.runId}` }),
      localMemoryAuthorizer,
      memorySelfProfile,
    )
    await runAction(async () => {
      if (disposed) return
      if (run.action === 'suggest' && result.kind === 'suggestion') {
        const proposal = await service.propose({
          requestId: `document-agent/${run.runId}/suggestion`,
          documentId: run.documentId,
          baseRevisionId: run.baseRevisionId,
          operations: [{
            kind: 'block.replace',
            operationId: `operation/document-agent/${run.runId}`,
            target: { blockId: run.blockId, expectedBlockRevision: run.blockRevision },
            payload: { content: result.content },
          }],
        }, result.summary)
        notice = `Agent 建议已保存，接受前不会改变正文 · ${proposal.changeSetId}`
        return
      }
      if (run.action === 'assess' && result.kind === 'assessment') {
        const assessment = await service.assessRevision(run.blockId, run.blockRevision, result.conclusion, result.summary)
        notice = `Agent 检查已保存并绑定版本 ${assessment.blockRevision}`
        return
      }
      throw new Error('CDR_AGENT_RESULT_KIND_MISMATCH')
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

  function canRun(command: EditorCommand): boolean {
    // Selection-only transactions also publish a fresh surface state.
    surfaceState
    return !editingDisabled && !!editor?.surface.canExecuteCommand(command)
  }

  function runCommand(command: EditorCommand) {
    if (!editor?.surface.executeCommand(command)) notice = '当前选区无法执行这个编辑操作。'
  }

  function chooseBlockStyle(value: string) {
    if (value === 'paragraph') runCommand({ kind: 'paragraph' })
    else runCommand({ kind: 'heading', level: Number(value) as 1 | 2 | 3 | 4 | 5 | 6 })
  }

  async function openAddressForm(kind: 'link' | 'image') {
    if (!editor) return
    const selectedBlockId = editor.surface.selectedBlockId()
    try {
      await editor.surface.flush()
    } catch (cause) {
      notice = `请先保存或处理当前草稿：${cause instanceof Error ? cause.message : String(cause)}`
      return
    }
    if (disposed || selectedBlockId !== editor.surface.selectedBlockId()) {
      notice = '所选位置已改变，请重新选择插入位置。'
      return
    }
    addressForm = kind
    address = ''
    addressLabel = ''
    addressBlockId = selectedBlockId
    addressBlockRevision = session?.snapshot().blocks.find((block) => block.blockId === selectedBlockId)?.blockRevision ?? null
  }

  function applyAddress() {
    if (!addressForm || !editor) return
    if (editor.surface.selectedBlockId() !== addressBlockId
      || session?.snapshot().blocks.find((block) => block.blockId === addressBlockId)?.blockRevision !== addressBlockRevision) {
      notice = '所选位置或正文版本已改变，请重新选择插入位置。'
      addressForm = null
      return
    }
    const command: EditorCommand = addressForm === 'link'
      ? { kind: 'link', href: address, text: addressLabel }
      : { kind: 'image', src: address, alt: addressLabel }
    if (!editor.surface.executeCommand(command)) {
      notice = '无法插入，请检查地址与选区。'
      return
    }
    addressForm = null
  }

  function moveSelection(direction: 'up' | 'down') {
    const blockId = editor?.surface.selectedBlockId()
    if (!blockId || !editor?.surface.executeStructuralCommand({ kind: direction === 'up' ? 'block.move-up' : 'block.move-down', blockId })) {
      notice = '所选块已在文档边界，或暂时无法移动。'
    }
  }

  async function copyMarkdown(markdown: string, label: string) {
    try {
      await clipboardWrite(markdown)
      notice = `${label}已复制。`
    } catch (cause) {
      notice = `复制失败：${cause instanceof Error ? cause.message : String(cause)}；仍可从下方文本中手动复制。`
    }
  }

  function downloadDraft() {
    const url = URL.createObjectURL(new Blob([recoveryMarkdown], { type: 'text/markdown;charset=utf-8' }))
    const anchor = document.createElement('a')
    anchor.href = url
    anchor.download = 'MEMORY-unsaved-draft.md'
    anchor.click()
    setTimeout(() => URL.revokeObjectURL(url), 1000)
  }

  function retrySave() {
    if (!editor?.surface.retryPending()) notice = '当前修改需要先比较版本，不能直接重试覆盖。'
  }

  function revisionMarkdown(revision: DocumentRevision): string {
    return revision.blocks.map((block) => block.markdown).join('\n\n')
  }

  async function restoreHistoryRevision() {
    const revision = selectedHistory
    if (!editor || !revision || !restoreConfirm || restoring) return
    restoring = true
    try {
      await editor.surface.flush()
      if (!editor.surface.restoreRevision(revision)) {
        notice = '所选历史正文与当前一致，或暂时无法恢复。'
        return
      }
      await editor.surface.flush()
      notice = '历史正文已恢复为一个新版本；中间历史、提案与检查记录均保留。'
      restoreConfirm = false
    } catch (cause) {
      notice = `恢复未完成：${cause instanceof Error ? cause.message : String(cause)}`
    } finally {
      restoring = false
      syncViewModels()
    }
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
    if (editor && (surfaceState.dirty || surfaceState.saving)) {
      recoveryMarkdown = editor.surface.getDraftMarkdown()
      lockedRecoveryMessage = message
    }
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
    history = [currentSession.snapshot(), ...currentSession.revisionHistory().slice().reverse()]
    editor?.decorations.setLayer('proposals', proposals
      .filter((proposal) => proposal.status === 'pending' || proposal.status === 'conflicted')
      .flatMap((proposal) => [...new Set(proposal.batch.operations.map(operationDisplayBlockId))].map((blockId) => ({
        blockId,
        kind: 'proposal' as const,
        label: proposal.status === 'conflicted' ? 'Agent 提案已过期' : 'Agent 提案待审阅',
      }))))
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
    if (operation.kind === 'block.move') return `移动块 ${operation.target.blockId}`
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
      <p class="eyebrow">MEMORY-first · 本机共写预览</p>
      <h2 id="cdr-spike-title">共写文档</h2>
      <p>直接维护有稳定块身份的背景文档；所选 Agent 只能提交待审阅建议或绑定精确版本的检查结果。当前不改写 Claim、根 MEMORY.md 或 Agent context，也不包含跨设备协作。</p>
      {#if vaultPath}<small class="managed-path">{vaultPath}</small>{/if}
    </div>
    <span class="status" aria-live="polite" class:loading={loading || saving || surfaceState.dirty || locked || creationPending} class:save-error={surfaceState.error !== null}>{loading ? '加载中' : failed ? '不可用' : creationPending ? '未创建' : surfaceState.error ? '保存失败 · 草稿保留' : locked || surfaceState.readOnly ? '只读' : saving ? '保存中' : surfaceState.dirty ? '未保存' : '已保存'}</span>
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
        <div class="editor-toolbar" role="toolbar" aria-label="Markdown 编辑工具">
          <select aria-label="段落样式" disabled={editingDisabled} onchange={(event) => chooseBlockStyle(event.currentTarget.value)}>
            <option value="paragraph">正文</option>
            {#each [1, 2, 3, 4, 5, 6] as level}<option value={level}>标题 {level}</option>{/each}
          </select>
          {#each toolbarCommands as item}
            <button title={item.title ?? item.label} aria-label={item.label} disabled={!canRun(item.command)} onmousedown={(event) => event.preventDefault()} onclick={() => runCommand(item.command)}>{item.label}</button>
          {/each}
          <button disabled={editingDisabled} onmousedown={(event) => event.preventDefault()} onclick={() => openAddressForm('link')}>插入链接</button>
          <button disabled={!canRun({ kind: 'unlink' })} onmousedown={(event) => event.preventDefault()} onclick={() => runCommand({ kind: 'unlink' })}>移除链接</button>
          <button disabled={editingDisabled} onmousedown={(event) => event.preventDefault()} onclick={() => openAddressForm('image')}>插入图片</button>
          {#if canRun({ kind: 'table.add-row' })}
            <div class="table-tools" role="group" aria-label="表格行列">
              {#each tableCommands as item}<button disabled={!canRun(item.command)} onmousedown={(event) => event.preventDefault()} onclick={() => runCommand(item.command)}>{item.label}</button>{/each}
              <small>Tab 切换单元格</small>
            </div>
          {/if}
        </div>
        {#if addressForm}
          <form class="address-form" onsubmit={(event) => { event.preventDefault(); applyAddress() }}>
            <label>{addressForm === 'link' ? '链接地址' : '图片地址'}<input bind:value={address} placeholder="https://…" required /></label>
            <label>{addressForm === 'link' ? '显示文字（未选中文字时使用）' : '图片说明'}<input bind:value={addressLabel} /></label>
            <div><button type="button" onclick={() => { addressForm = null }}>取消</button><button type="submit" disabled={!address.trim()}>确认插入</button></div>
          </form>
        {/if}
        {#if surfaceState.error || lockedRecoveryMessage}
          <section class="draft-recovery" role="alert">
            <strong>{lockedRecoveryMessage ? '请保留当前草稿，重开后核对保存结果' : '尚未保存的草稿已保留'}</strong>
            <p>{surfaceState.error?.message ?? lockedRecoveryMessage}</p>
            <p>关闭窗口前请复制或下载草稿；本机恢复副本可能因存储受限而不可用。</p>
            <div>
              {#if !locked && surfaceState.error?.code === 'persistence-failed'}<button onclick={retrySave}>重试保存</button>{/if}
              <button onclick={() => copyMarkdown(recoveryMarkdown, '草稿')}>复制草稿</button>
              <button onclick={downloadDraft}>下载草稿</button>
              {#if !locked}<button onclick={() => { discardConfirm = true }}>放弃本地草稿</button>{/if}
            </div>
            {#if discardConfirm}<p>确定丢弃这份未保存草稿并恢复最后提交版本？此操作无法恢复草稿，请先复制或下载。</p><div><button onclick={() => { editor?.surface.discardDraft(); discardConfirm = false }}>确认放弃草稿</button><button onclick={() => { discardConfirm = false }}>保留草稿</button></div>{/if}
            <details><summary>查看草稿与最后提交版本</summary><label>未保存草稿<textarea readonly value={recoveryMarkdown} rows="6"></textarea></label><label>最后提交版本<textarea readonly value={session ? revisionMarkdown(session.snapshot()) : ''} rows="6"></textarea></label></details>
          </section>
        {/if}
        <div class="editor-host" bind:this={editorHost}></div>
      </div>
      <aside aria-label="协作活动与提案">
        <section class="actions">
          <h3>文档操作</h3>
          <button onclick={insertAfterSelection} disabled={!editor || !session || locked}>在选中块后新增段落</button>
          <button onclick={deleteSelection} disabled={!editor || !session || locked}>删除选中块</button>
          <div class="move-actions"><button onclick={() => moveSelection('up')} disabled={editingDisabled}>上移选中块</button><button onclick={() => moveSelection('down')} disabled={editingDisabled}>下移选中块</button></div>
          <label for="agent-instruction">给 Agent 的要求</label>
          <textarea id="agent-instruction" bind:value={agentInstruction} rows="3" disabled={agentBusy || locked}></textarea>
          <small class:agent-unavailable={!agentReadiness.ok}>{agentReadiness.ok ? `由 ${agentReadiness.label} 在 input-only 隔离中处理` : agentReadiness.message}</small>
          <button onclick={() => startAgentAction('suggest')} disabled={!editor || !session || agentBusy || locked || surfaceState.error !== null || !agentReadiness.ok}>让 Agent 建议改写选中块</button>
          <button onclick={() => startAgentAction('assess')} disabled={!editor || !session || agentBusy || locked || surfaceState.error !== null || !agentReadiness.ok}>让 Agent 检查选中版本</button>
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
              {#each proposal.batch.operations as operation (operation.operationId)}
                <div class="proposal-operation"><span>{operationSummary(operation)}</span><small>基于 {operationBasis(operation)}</small></div>
              {/each}
              {#if proposal.rationale}<small>{proposal.rationale}</small>{/if}
              <small>{proposal.status} · {proposal.batch.operations.length} 项修改</small>
              {#if proposal.status === 'pending'}
                <div><button onclick={() => decide(proposal, 'reject')} disabled={saving || locked}>拒绝</button><button class="primary" onclick={() => decide(proposal, 'accept')} disabled={saving || locked}>接受</button></div>
              {/if}
            </article>
          {:else}
            <p class="empty">暂无待处理提案。</p>
          {/each}
        </section>

        <section class="revision-history">
          <h3>版本历史</h3>
          <select aria-label="查看历史版本" value={selectedHistoryId} onchange={(event) => { selectedHistoryId = event.currentTarget.value; restoreConfirm = false }}>
            <option value="">选择一个版本</option>
            {#each history as revision, index (revision.revisionId)}<option value={revision.revisionId}>{index === 0 ? '当前 · ' : ''}{revision.revisionId}</option>{/each}
          </select>
          {#if selectedHistory}
            <textarea aria-label="历史版本正文" readonly value={revisionMarkdown(selectedHistory)} rows="8"></textarea>
            <button onclick={() => copyMarkdown(revisionMarkdown(selectedHistory!), '历史正文')}>复制历史正文</button>
            {#if selectedHistory.revisionId !== session?.snapshot().revisionId}
              {#if restoreConfirm}
                <p>将先保存当前编辑，再把所选正文恢复为新版本。提案和检查记录仍保留。</p>
                <button onclick={restoreHistoryRevision} disabled={editingDisabled || restoring || surfaceState.error !== null}>{restoring ? '恢复中…' : '确认恢复为新版本'}</button>
                <button onclick={() => { restoreConfirm = false }} disabled={restoring}>取消恢复</button>
              {:else}
                <button onclick={() => { restoreConfirm = true }} disabled={editingDisabled || surfaceState.error !== null}>恢复这个版本的正文</button>
              {/if}
            {/if}
          {/if}
        </section>

        <section class="assessment-list">
          <h3>版本检查</h3>
          {#each assessments as assessment (assessment.assessmentId)}
            <p class:outdated={session?.assessmentIsOutdated(assessment) ?? false}>
              <strong>{assessment.conclusion === 'verified' ? 'Agent 判断支持' : 'Agent 建议复核'}</strong>
              <span>{assessment.blockRevision}</span>
              <small>{assessment.actorId}</small>
              {#if assessment.rationale}<small>{assessment.rationale}</small>{/if}
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
  .editor-toolbar{display:flex;flex-wrap:wrap;gap:5px;align-items:center;position:sticky;top:-28px;z-index:2;margin:-12px -16px 18px;padding:10px 12px;background:Canvas;border-bottom:1px solid color-mix(in srgb,CanvasText 12%,transparent)}
  .editor-toolbar select{width:auto;min-width:90px}
  .workbench aside{min-width:0;grid-template-columns:minmax(0,1fr);overflow-wrap:anywhere}aside section,aside article{min-width:0}.move-actions button{min-width:0;flex:1}
  .editor-toolbar button,.editor-toolbar select{min-height:29px;padding:4px 7px;font-size:12px}.editor-toolbar button:disabled{opacity:.35}.table-tools{display:flex;flex-wrap:wrap;gap:5px;width:100%;align-items:center}.table-tools small{font-size:12px;color:var(--ui-secondary)}
  .address-form,.draft-recovery{display:grid;gap:9px;margin:0 0 16px;padding:12px;border:1px solid color-mix(in srgb,#0a84ff 25%,Canvas);border-radius:8px;background:color-mix(in srgb,#0a84ff 4%,Canvas)}.address-form label,.draft-recovery label{display:grid;gap:4px;font-size:12px}.address-form input,.draft-recovery textarea,.revision-history textarea,.revision-history select{width:100%;min-width:0;border:1px solid color-mix(in srgb,CanvasText 15%,transparent);border-radius:6px;padding:7px;background:Canvas;color:CanvasText;font:inherit}.address-form>div,.draft-recovery>div,.move-actions{display:flex;gap:6px}.draft-recovery{border-color:color-mix(in srgb,#ff9f0a 45%,Canvas);background:color-mix(in srgb,#ff9f0a 7%,Canvas)}.draft-recovery p,.revision-history p{margin:0;font-size:12px;line-height:1.5}.draft-recovery textarea,.revision-history textarea{font:12px/1.5 ui-monospace,SFMono-Regular,monospace;resize:vertical}.draft-recovery details{font-size:12px}.draft-recovery details label{margin-top:9px}.status.save-error{color:var(--ui-warning);background:color-mix(in srgb,#ff9f0a 15%,Canvas)}.proposal-operation{display:grid!important;gap:4px;justify-content:stretch!important;border-bottom:1px solid color-mix(in srgb,CanvasText 8%,transparent);padding-bottom:6px}.proposal-operation>span{font-size:12px;white-space:pre-wrap;overflow-wrap:anywhere}.revision-history{min-width:0}.revision-history select{font-size:12px}
  .cdr-spike{display:grid;gap:14px}.cdr-spike>header{display:flex;align-items:flex-start;justify-content:space-between;gap:20px}.cdr-spike h2{margin:2px 0 5px;font-size:20px}.cdr-spike header p:not(.eyebrow){max-width:720px;margin:0;color:var(--ui-secondary);font-size:12px}.managed-path{display:block;margin-top:6px;color:var(--ui-secondary);font:12px ui-monospace,SFMono-Regular,monospace}.eyebrow{margin:0;color:var(--ui-accent-text);font-size:12px;font-weight:750;letter-spacing:.04em;text-transform:uppercase}.status{flex:none;padding:4px 9px;border-radius:999px;background:color-mix(in srgb,#34c759 14%,Canvas);color:var(--ui-success);font-size:12px;font-weight:700}.status.loading{background:color-mix(in srgb,CanvasText 8%,Canvas);color:var(--ui-secondary)}.create-panel{display:grid;justify-items:start;gap:10px;padding:24px;border:1px solid color-mix(in srgb,CanvasText 12%,transparent);border-radius:12px;background:Canvas}.create-panel p{max-width:680px;margin:0;color:var(--ui-secondary);font-size:12px;line-height:1.55}.create-panel small{color:var(--ui-secondary);font-size:12px}.creation-error{max-width:100%;overflow-wrap:anywhere;color:var(--ui-danger);font-size:12px}.workbench{display:grid;grid-template-columns:minmax(0,1fr) 310px;min-height:540px;overflow:hidden;border:1px solid color-mix(in srgb,CanvasText 12%,transparent);border-radius:12px;background:Canvas}.document-shell{min-width:0;padding:28px 34px;overflow:auto;border-right:1px solid color-mix(in srgb,CanvasText 10%,transparent)}.editor-host{min-height:430px}.document-shell :global(.kit-host){height:100%;min-height:430px}.document-shell :global(.moraya-editor){min-height:410px;padding:0;outline:0}.workbench aside{display:grid;align-content:start;gap:14px;padding:16px;overflow-x:clip;overflow-y:auto;background:color-mix(in srgb,CanvasText 2.5%,Canvas)}aside section{display:grid;gap:7px}aside h3{margin:0;font-size:12px}.actions label{color:var(--ui-secondary);font-size:12px}.actions textarea{width:100%;resize:vertical;border:1px solid color-mix(in srgb,CanvasText 14%,transparent);border-radius:8px;padding:7px 8px;background:Canvas;color:CanvasText;font:inherit;line-height:1.4}.actions small{color:var(--ui-secondary);font-size:12px}.actions small.agent-unavailable{color:var(--ui-danger)}.actions button{width:100%;text-align:left}.activity{padding:11px;border-radius:9px;background:color-mix(in srgb,#0a84ff 8%,Canvas)}.activity p{margin:0;font-size:12px;line-height:1.5}.activity small,.proposal-list small{color:var(--ui-secondary);font-size:12px}.proposal-list article{display:grid;gap:6px;padding:10px;border:1px solid color-mix(in srgb,#ff9f0a 30%,transparent);border-radius:9px;background:Canvas}.proposal-list article.conflicted{border-color:color-mix(in srgb,#ff3b30 38%,transparent)}.proposal-list article>strong{font-size:12px}.proposal-list article>div{display:flex;justify-content:flex-end;gap:6px}.assessment-list p{display:grid;gap:2px;margin:0;padding:9px;border-radius:8px;background:Canvas;font-size:12px}.assessment-list p.outdated{background:color-mix(in srgb,#ff3b30 7%,Canvas)}.assessment-list span{color:var(--ui-secondary);font-size:12px}.assessment-list small{color:var(--ui-secondary);font-size:12px}.assessment-list p.outdated small:last-child{color:var(--ui-danger)}.empty{margin:0;color:var(--ui-secondary);font-size:12px}.failure{display:grid;gap:4px;padding:18px;border:1px solid color-mix(in srgb,#ff3b30 35%,transparent);border-radius:10px;background:color-mix(in srgb,#ff3b30 7%,Canvas)}.failure span{font:12px ui-monospace,SFMono-Regular,monospace}.primary{background:var(--ui-accent)!important;color:#fff!important}@media(max-width:800px){.workbench{grid-template-columns:1fr}.document-shell{border-right:0;border-bottom:1px solid color-mix(in srgb,CanvasText 10%,transparent)}.workbench aside{grid-template-columns:repeat(2,minmax(0,1fr))}.activity{grid-column:1/-1}}@media(max-width:580px){.cdr-spike>header{display:block}.status{display:inline-block;margin-top:10px}.document-shell{padding:20px}.workbench aside{grid-template-columns:1fr}.activity{grid-column:auto}}
</style>
