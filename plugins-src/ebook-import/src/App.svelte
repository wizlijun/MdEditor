<script lang="ts">
  import { bridge, vaultExists } from './lib/bridge'
  import AgentPicker from './lib/agent-picker/AgentPicker.svelte'
  import LibraryPanel from './components/LibraryPanel.svelte'
  import TopicBar from './components/TopicBar.svelte'
  import TopicManager from './components/TopicManager.svelte'
  import TopicClassificationReview from './components/TopicClassificationReview.svelte'
  import { formatElapsed } from './lib/elapsed'
  import {
    bindAiJob,
    claimAiRead,
    failAiRead,
    latestSummary,
    mergeLibrary,
    replayPendingLibraryAi,
    stashOrApplyLibraryAi,
    type LibraryBook,
    type PendingLibraryAiEvent,
  } from './lib/library'
  import {
    rememberProvider,
    rememberedProvider,
    type AgentOption,
  } from './lib/agent-picker/types'
  import { describeLog } from './lib/logs'
  import { setLocale, t, type MessageKey } from './lib/strings'
  import { describeError } from './lib/errors'
  import {
    topicDesignAvailability,
    topicDesignProviders,
    topicDesignReadScope,
  } from './lib/topic-agent'
  import {
    addPaths,
    assignTopic,
    hasUnclassifiedPending,
    hasPending as queueHasPending,
    isRunComplete,
    nextToStart,
    onAiEvent,
    replayPending,
    reserve,
    stashOrApply,
    type PendingJobEvent,
    type Queue,
    type QueueItem,
  } from './lib/queue'
  import type { TopicCounts, TopicDefinition } from './lib/topics'
  import {
    cloneClassificationProposal,
    updateClassificationAssignment,
    type TopicClassificationProposal,
  } from './lib/topic-classification'

  setLocale(bridge().locale)

  // ── host push payload shapes (Task 1 drag-drop, Task 8 job events) ──────
  type JobPush = {
    type: 'job'
    job_id: number
    event: 'log' | 'progress' | 'done' | 'failed'
    line?: string
    stage?: string
    page?: number
    total?: number
    dest_rel?: string
    error?: string
  }
  type DragPush = { type: 'drag-drop'; phase: 'enter' | 'leave' | 'drop'; paths: string[] }
  // Backend only pushes started/done/failed — `queued` is applied locally
  // (see aiRead() below) right after plugin.ai_read_start succeeds.
  type AiPush = {
    type: 'ai_read'
    job_id: number
    event: 'started' | 'done' | 'failed'
    started_at?: string
    summary_rel?: string
    error?: string
  }
  type TopicProposal = {
    schema_version: number
    inventory_sha256: string
    topics: TopicDefinition[]
    assignments: { book: string; topic_id: string }[]
  }
  type TopicAgentPush = {
    type: 'topic_agent'
    job_id: number
    event: 'started' | 'done' | 'failed'
    proposal?: TopicProposal
    error?: string
  }
  type TopicClassificationPush = {
    type: 'topic_classification'
    job_id: number
    event: 'started' | 'done' | 'failed'
    proposal?: TopicClassificationProposal
    error?: string
  }
  type HostPush = JobPush | DragPush | AiPush | TopicAgentPush | TopicClassificationPush | { type: string }

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e))

  let q: Queue = $state({ items: [], activeId: null })
  // Job-push events that arrived for a jobId not yet folded into any item
  // (see queue.ts's stashOrApply doc) — replayed by schedule() once
  // import_start resolves and the jobId is known. Not `$state`: nothing in
  // the template reads it, and App.svelte only ever mutates it from plain
  // (non-reactive-context) functions below.
  let pending: PendingJobEvent[] = []
  let ocr = $state(false)
  let provider: 'wechat' | 'baidu' = $state('wechat')
  // A run is a batch: pressing Start locks in the OCR choice so every file in
  // the queue is processed the same way, even if the controls are touched
  // afterwards. `running` also gates the auto-advance — without it, queueing a
  // file would start it, which is exactly what Start exists to prevent.
  let running = $state(false)
  let runOcr = false
  let runProvider: 'wechat' | 'baidu' = 'wechat'

  let hasPending = $derived(queueHasPending(q))
  let dragActive = $state(false)
  let expanded: Record<number, boolean> = $state({})
  let globalError = $state('')
  let topics: TopicDefinition[] = $state([])
  let topicCatalogExtra: Record<string, unknown> = {}
  let topicRevision = 'absent'
  let topicCounts: TopicCounts = $state({})
  let selectedTopicId: string | null = $state(null)
  let topicManagerOpen = $state(false)
  let unclassifiedBooks = $state<string[]>([])
  let unknownTopicBooks = $state<string[]>([])
  let unsafeTopicBooks = $state<string[]>([])
  let topicAgentRunning = $state(false)
  let topicProposal: TopicProposal | null = $state(null)
  let topicClassificationRunning = $state(false)
  let classificationProposal: TopicClassificationProposal | null = $state(null)
  let classificationExpectedBooks: string[] = $state([])
  let classificationApplying = $state(false)
  let topicOperationRunning = $derived(topicAgentRunning || topicClassificationRunning)

  let settingsOpen = $state(false)
  let calibreFound: { path: string; version: string } | null = $state(null)
  let ebooksRoot = $state('')
  let wechatUrl = $state('')
  let calibrePathOverride = $state('')
  let baiduKeyInput = $state('')
  let baiduSecretInput = $state('')
  let baiduKeySet = $state(false)
  let baiduSecretSet = $state(false)
  let saving = $state(false)
  let savedFlash = $state(false)

  function applyEnv(env: any) {
    calibreFound = env.calibre ?? null
    ebooksRoot = env.settings?.ebooks_root ?? ''
    wechatUrl = env.settings?.wechat_url ?? ''
    if (env.settings?.provider === 'baidu' || env.settings?.provider === 'wechat') {
      provider = env.settings.provider
    }
    calibrePathOverride = env.device?.calibre_path ?? ''
    baiduKeySet = !!env.device?.baidu_api_key_set
    baiduSecretSet = !!env.device?.baidu_secret_key_set
  }

  // The vault root resolves asynchronously on the backend (host.vault.info is
  // a round trip made from inside $activate) — detect_env answers
  // `ready:false` until that settles, so retry briefly instead of showing
  // "no calibre / no vault" while it's still in flight.
  async function loadEnv() {
    try {
      for (let i = 0; i < 20; i++) {
        const env = await bridge().request('plugin.detect_env', {})
        applyEnv(env)
        if (env.ready !== false) return
        await new Promise((r) => setTimeout(r, 500))
      }
    } catch (e) {
      globalError = message(e)
    }
  }

  /**
   * Serial scheduler: starts the next pending item once nothing is active.
   * Called re-entrantly from several places (a drop, "Add files…", a job's
   * done/failed push, and this function's own failure-retry path) — `reserve`
   * MUST run synchronously right after `nextToStart`, before the
   * `import_start` await, or two overlapping calls could both see
   * `activeId: null` and both start the same item (see reserve()'s doc in
   * queue.ts).
   *
   * The backend spawns the job thread before writing `import_start`'s RPC
   * response, so a fast-failing job's push can arrive (and get stashed by
   * `onMessage`, see queue.ts's `stashOrApply`) before this `await` resolves.
   * Once the `job_id` is folded into the item below, replay whatever got
   * stashed for it — if that includes a done/failed event, `activeId` clears
   * right here and this function must re-invoke itself to pick up the next
   * pending item (no `onMessage` push is coming to do it).
   */
  async function schedule() {
    const n = nextToStart(q)
    if (!n) {
      // Nothing left to start: once the last item lands too, the run is over
      // and "Start" becomes available again.
      if (isRunComplete(q)) running = false
      return
    }
    q = reserve(q, n.id)
    try {
      const res = await bridge().request('plugin.import_start', {
        path: n.path,
        topic_id: n.topicId,
        ocr: runOcr,
        ...(runOcr ? { provider: runProvider } : {}),
      })
      q = { ...q, items: q.items.map((i) => (i.id === n.id ? { ...i, jobId: res.job_id } : i)) }
      const replay = replayPending(q, pending, res.job_id)
      q = replay.q
      pending = replay.pending
      if (q.activeId == null) void schedule()
    } catch (e) {
      q = {
        ...q,
        activeId: null,
        items: q.items.map((i) => (i.id === n.id ? { ...i, status: 'failed', error: message(e) } : i)),
      }
      void schedule() // this item never got a jobId — try the next one
    }
  }

  /** Begins a run over everything currently queued, freezing the OCR choice. */
  function startRun() {
    if (running || !hasPending || hasUnclassifiedPending(q)) return
    runOcr = ocr
    runProvider = provider
    running = true
    void schedule()
  }

  bridge().onMessage((raw: unknown) => {
    const m = raw as HostPush
    if (m.type === 'drag-drop') {
      const d = m as DragPush
      if (d.phase === 'enter') dragActive = true
      else if (d.phase === 'leave') dragActive = false
      else if (d.phase === 'drop') {
        dragActive = false
        // Dropping only queues: the OCR choice belongs to the user BEFORE the
        // work starts, so nothing runs until "Start" is pressed. A drop during
        // a run joins that run and inherits its locked-in OCR settings.
        q = addPaths(q, d.paths ?? [], selectedTopicId ?? undefined)
        if (running) void schedule()
      }
    } else if (m.type === 'job') {
      const j = m as JobPush
      const result = stashOrApply(q, pending, j.job_id, {
        event: j.event,
        line: j.line,
        stage: j.stage,
        page: j.page,
        total: j.total,
        dest_rel: j.dest_rel,
        error: j.error,
      })
      q = result.q
      pending = result.pending
      if (result.applied && (j.event === 'done' || j.event === 'failed')) void schedule()
      // A finished import is a new book in the vault — and thus a new library row.
      if (result.applied && j.event === 'done') void Promise.all([loadLibrary(), loadTopics()])
    } else if (m.type === 'ai_read') {
      const a = m as AiPush
      const ev = {
        event: a.event,
        started_at: a.started_at,
        summary_rel: a.summary_rel,
        error: a.error,
      }
      // Fed to BOTH reducers: a read can be started from the queue's row or
      // the library's, and each ignores a job it doesn't own. Keeping them
      // independent is why neither needs to know the other exists.
      q = onAiEvent(q, a.job_id, ev)
      const result = stashOrApplyLibraryAi(library, pendingLibraryAi, a.job_id, ev)
      library = result.list
      pendingLibraryAi = result.pending
      // A finished read wrote a summary file the last listing can't know about.
      if (a.event === 'done') void loadLibrary()
    } else if (m.type === 'topic_agent') {
      const event = m as TopicAgentPush
      if (event.event === 'done' && event.proposal) {
        topicAgentRunning = false
        topicProposal = event.proposal
      } else if (event.event === 'failed') {
        topicAgentRunning = false
        globalError = event.error ?? 'Topic design failed'
      }
    } else if (m.type === 'topic_classification') {
      const event = m as TopicClassificationPush
      if (event.event === 'done' && event.proposal) {
        topicClassificationRunning = false
        classificationProposal = cloneClassificationProposal(event.proposal)
        classificationExpectedBooks = event.proposal.assignments.map((assignment) => assignment.book)
      } else if (event.event === 'failed') {
        topicClassificationRunning = false
        globalError = event.error ?? 'Topic classification failed'
      }
    }
  })

  async function pickFiles() {
    try {
      const res = await bridge().request('host.dialog.open', {
        title: t('dialog.pickBooks'),
        multiple: true,
        filters: [{ name: t('dialog.ebooksFilter'), extensions: ['epub', 'pdf', 'docx'] }],
      })
      const paths: string[] = res?.paths ?? []
      if (paths.length) {
        q = addPaths(q, paths, selectedTopicId ?? undefined)
        if (running) void schedule()
      }
    } catch (e) {
      globalError = message(e)
    }
  }

  async function pickCalibre() {
    try {
      const res = await bridge().request('host.dialog.open', {
        title: t('dialog.pickCalibre'),
        multiple: false,
        filters: [],
      })
      const p = res?.paths?.[0]
      if (p) calibrePathOverride = p
    } catch (e) {
      globalError = message(e)
    }
  }

  async function saveSettings() {
    saving = true
    globalError = ''
    try {
      await bridge().request('plugin.save_settings', {
        vault: { ebooks_root: ebooksRoot, wechat_url: wechatUrl, provider },
        device: {
          calibre_path: calibrePathOverride,
          baidu_api_key: baiduKeyInput,
          baidu_secret_key: baiduSecretInput,
        },
      })
      // Secrets are never echoed back by detect_env — clear the plaintext
      // inputs so they don't linger on screen, then re-detect to refresh the
      // *_set flags and calibre status.
      baiduKeyInput = ''
      baiduSecretInput = ''
      await loadEnv()
      // Saving may have moved the ebooks root, which is where the library is
      // read from — the old root's books are not this vault's library anymore.
      await loadLibrary()
      savedFlash = true
      setTimeout(() => (savedFlash = false), 1500)
    } catch (e) {
      globalError = message(e)
    } finally {
      saving = false
    }
  }

  async function cancelItem(item: QueueItem) {
    if (item.jobId == null) return
    try {
      await bridge().request('plugin.import_cancel', { job_id: item.jobId })
    } catch (e) {
      globalError = message(e)
    }
  }

  async function openInEditor(item: QueueItem) {
    if (!item.destRel) return
    await openPath(`${item.destRel.replace(/\/+$/, '')}/book.md`)
  }

  // ── which agent reads the book ────────────────────────────────────────────
  // Remembered per surface: reading books with DeepSeek while a sidecar note is
  // answered by Claude is a reasonable thing to want, and one shared setting
  // cannot express it. The choice is carried INTO the queue, so a job that
  // waits behind others still runs on the agent it was queued for.
  const AGENT_SURFACE = 'ebook-import'
  const TOPIC_AGENT_SURFACE = 'ebook-topic-design'
  let agents: AgentOption[] = $state([])
  let agentId: string | undefined = $state(undefined)
  let topicAgentId: string | undefined = $state(undefined)
  const topicAgents = $derived(topicDesignProviders(agents))
  const topicDesign = $derived(topicDesignAvailability(topicAgents, topicAgentId))
  const topicDesignWideScope = $derived(
    topicDesign.available && topicDesignReadScope(topicDesign.provider.id) === 'vault',
  )
  const topicDesignStatus = $derived(
    topicDesign.available
      ? t(
          topicDesignReadScope(topicDesign.provider.id) === 'inventory'
            ? 'topic.agentScopeInventory'
            : 'topic.agentScopeVault',
        )
      : t(
          topicDesign.reason === 'missing'
            ? 'topic.agentProviderMissing'
            : 'topic.agentProviderUnavailable',
        ),
  )

  async function loadAgents() {
    try {
      const r = await bridge().request('host.agent.providers', {})
      agents = r?.providers ?? []
      agentId = rememberedProvider(AGENT_SURFACE, agents.map((a) => a.id), r?.default ?? '')
      const supportedTopicAgents = topicDesignProviders(agents)
      topicAgentId = rememberedProvider(
        TOPIC_AGENT_SURFACE,
        supportedTopicAgents.map((agent) => agent.id),
        r?.default ?? '',
      )
    } catch {
      // An older host without host.agent.providers, or no agent installed: hide
      // the picker and let the host decide, exactly as before.
      agents = []
      agentId = undefined
      topicAgentId = undefined
    }
  }

  function pickAgent(id: string) {
    agentId = id
    rememberProvider(AGENT_SURFACE, id)
  }

  function pickTopicAgent(id: string) {
    topicAgentId = id
    rememberProvider(TOPIC_AGENT_SURFACE, id)
  }

  async function aiRead(item: QueueItem) {
    if (!item.destRel || item.jobId == null) return
    const jobId = item.jobId
    // Same "synchronously claim, then await" shape as reserve()/schedule():
    // flipping aiStatus to 'queued' before the RPC — not after it resolves —
    // hides the button within this tick, closing the double-click window a
    // post-await write would leave open (see queue.ts reserve()'s doc for
    // why the ordering matters). If the RPC fails, roll the row back to
    // 'failed' so it isn't stuck showing "queued" forever with no retry.
    q = onAiEvent(q, jobId, { event: 'queued' })
    try {
      await bridge().request('plugin.ai_read_start', {
        job_id: jobId,
        dest_rel: item.destRel,
        name: item.name,
        ...(agentId ? { harness: agentId } : {}),
      })
    } catch (e) {
      q = onAiEvent(q, jobId, { event: 'failed', error: message(e) })
      globalError = message(e)
    }
  }

  async function openSummary(item: QueueItem) {
    if (item.aiSummaryRel) await openPath(item.aiSummaryRel)
  }

  // ── the library: every book in the vault, not just this session's ────────
  let library: LibraryBook[] = $state([])
  let pendingLibraryAi: PendingLibraryAiEvent[] = []

  async function loadLibrary() {
    try {
      const res = await bridge().request('plugin.library_list', {})
      library = mergeLibrary(library, res?.books ?? [])
    } catch (e) {
      globalError = message(e)
    }
  }

  async function loadTopics() {
    try {
      const state = await bridge().request('plugin.topic_state', {})
      topicRevision = typeof state?.revision === 'string' ? state.revision : 'absent'
      const catalog = state?.catalog ?? null
      topics = catalog?.topics ?? []
      if (catalog) {
        const { schema_version: _schemaVersion, topics: _topics, ...extra } = catalog
        topicCatalogExtra = extra
      } else {
        topicCatalogExtra = {}
      }
      const validIds = new Set(topics.map((topic) => topic.id))
      q = {
        ...q,
        items: q.items.map((item) =>
          item.status === 'pending' && item.topicId && !validIds.has(item.topicId)
            ? { ...item, topicId: undefined }
            : item,
        ),
      }
      topicCounts = state?.counts ?? {}
      unclassifiedBooks = state?.unclassified_books ?? []
      unknownTopicBooks = state?.unknown_topic_books ?? []
      unsafeTopicBooks = state?.unsafe_topic_books ?? []
      if (!topics.some((topic) => topic.id === selectedTopicId)) {
        selectedTopicId = topics[0]?.id ?? null
      }
    } catch (e) {
      globalError = message(e)
    }
  }

  function selectImportTopic(topicId: string) {
    selectedTopicId = topicId
  }

  async function saveTopics(nextTopics: TopicDefinition[], migrations: Record<string, string>) {
    await bridge().request('plugin.topic_save', {
      catalog: { ...topicCatalogExtra, schema_version: 1, topics: nextTopics },
      expected_revision: topicRevision,
      migrations,
    })
    topicManagerOpen = false
    await Promise.all([loadTopics(), loadLibrary()])
  }

  async function assignLibraryTopic(book: LibraryBook, topicId: string) {
    try {
      await bridge().request('plugin.topic_assign', { book: book.rel, topic_id: topicId })
      await Promise.all([loadTopics(), loadLibrary()])
    } catch (e) {
      globalError = message(e)
    }
  }

  async function startTopicDesign() {
    if (topicAgentRunning) return
    if (!topicDesign.available) {
      globalError = topicDesignStatus
      return
    }
    topicAgentRunning = true
    topicProposal = null
    globalError = ''
    try {
      await bridge().request('plugin.topic_agent_start', {
        harness: topicDesign.provider.id,
      })
    } catch (e) {
      topicAgentRunning = false
      globalError = message(e)
    }
  }

  async function applyTopicProposal() {
    if (!topicProposal) return
    try {
      await bridge().request('plugin.topic_agent_apply', { proposal: topicProposal })
      topicProposal = null
      await Promise.all([loadTopics(), loadLibrary()])
    } catch (e) {
      globalError = message(e)
    }
  }

  async function startTopicClassification() {
    if (topicOperationRunning || unclassifiedBooks.length === 0 || topics.length === 0) return
    if (!topicDesign.available) {
      globalError = topicDesignStatus
      return
    }
    topicClassificationRunning = true
    classificationProposal = null
    classificationExpectedBooks = []
    globalError = ''
    try {
      await bridge().request('plugin.topic_classification_start', {
        harness: topicDesign.provider.id,
      })
    } catch (e) {
      topicClassificationRunning = false
      globalError = message(e)
    }
  }

  function reviseClassification(book: string, topicId: string) {
    if (!classificationProposal || classificationApplying) return
    classificationProposal = updateClassificationAssignment(classificationProposal, book, topicId)
  }

  async function applyTopicClassification() {
    if (!classificationProposal || classificationApplying) return
    classificationApplying = true
    globalError = ''
    try {
      await bridge().request('plugin.topic_classification_apply', {
        proposal: classificationProposal,
      })
      classificationProposal = null
      classificationExpectedBooks = []
      await Promise.all([loadTopics(), loadLibrary()])
    } catch (e) {
      globalError = message(e)
    } finally {
      classificationApplying = false
    }
  }

  /**
   * "AI 先读" / "重读" from a library row. Same shape as `aiRead` above:
   * claim the row synchronously, then send. The difference is the job id —
   * a library book has no import job behind it, so the backend allocates one
   * and answers with it (or, if this book is already being read, with the id
   * of the run that exists, so this row follows that one instead of starting
   * a second read of the same book).
   */
  async function libraryRead(book: LibraryBook) {
    library = claimAiRead(library, book.rel)
    try {
      const res = await bridge().request('plugin.ai_read_start', {
        dest_rel: book.rel,
        name: book.name,
        ...(agentId ? { harness: agentId } : {}),
      })
      if (typeof res?.job_id === 'number') {
        library = bindAiJob(library, book.rel, res.job_id)
        const replay = replayPendingLibraryAi(library, pendingLibraryAi, res.job_id)
        library = replay.list
        pendingLibraryAi = replay.pending
      }
    } catch (e) {
      library = failAiRead(library, book.rel, message(e))
      globalError = message(e)
    }
  }

  async function openPath(path: string) {
    try {
      await bridge().request('host.editor.open', { path })
    } catch (e) {
      globalError = message(e)
    }
  }

  // ── the AI reading prompt ────────────────────────────────────────────────
  // The prompt is a plain file in the vault, not a setting in this window —
  // same as idea-spark's. Editing it is opening it in the main editor.
  // claude-agent seeds this template on first activation; this plugin does not
  // carry a copy, so that there is exactly one version of it to drift from.
  const PROMPT_PATH = '.notemd/agent-tasks/ai-read-ebook/CLAUDE.md'

  async function editPrompt() {
    globalError = ''
    try {
      // A failed existence check is NOT read as "missing" — that would refuse a
      // file that is probably there. Only a definite `false` gets the hint.
      if ((await vaultExists(PROMPT_PATH)) === false) {
        globalError = t('err.promptMissing')
        return
      }
    } catch {
      /* let the editor be the one to complain */
    }
    await openPath(PROMPT_PATH)
  }

  // 「AI 阅读中… 3m12s」的秒针。
  // effect 的依赖必须是这个 boolean,不能是整个 q:并发导入时日志/进度事件
  // 每 <1s 就刷新 q,effect 每次都会在 interval 触发前把它重建,秒数永远停在
  // 0s。$derived 的值不变就不通知下游,所以 anyAiRunning 只在真正切换时重挂。
  // interval 只写 nowMs,而 nowMs 不在依赖里 —— 不会自失效死循环($effect 纪律)。
  let nowMs = $state(Date.now())
  const anyAiRunning = $derived(
    q.items.some((i) => i.aiStatus === 'running') ||
      library.some((b) => b.aiStatus === 'running'),
  )
  $effect(() => {
    if (!anyAiRunning) return
    const t = setInterval(() => {
      nowMs = Date.now()
    }, 1000)
    return () => clearInterval(t)
  })

  function clearFinished() {
    // A 'done' import row with AI reading still queued/running must stay:
    // removing it would drop the item the backend's later ai_read push looks
    // up by jobId, silently losing that row's status forever.
    q = {
      ...q,
      items: q.items.filter(
        (i) =>
          i.status === 'pending' ||
          i.status === 'running' ||
          i.aiStatus === 'queued' ||
          i.aiStatus === 'running',
      ),
    }
  }

  function toggleLog(id: number) {
    expanded = { ...expanded, [id]: !expanded[id] }
  }

  function badgeKey(item: QueueItem): MessageKey {
    if (item.status === 'failed' && item.cancelled) return 'status.cancelled'
    return `status.${item.status}` as MessageKey
  }

  /** Localized label for the pipeline stage backing a running item, or '' for
   * an unset/unrecognized stage token (never shows the raw English token). */
  function stageLabel(item: QueueItem): string {
    if (!item.stage) return ''
    const key = `stage.${item.stage}` as MessageKey
    const known = ['stage.convert', 'stage.extract', 'stage.markdown', 'stage.ocr', 'stage.finalize']
    return known.includes(key) ? t(key) : ''
  }

  void loadAgents()
  // The vault root resolves asynchronously (see loadEnv), and library_list
  // fails without it — so the first listing waits for loadEnv to settle.
  void loadEnv().then(() => Promise.all([loadLibrary(), loadTopics()]))
</script>

<!-- Drag highlighting is driven entirely by the host's `type:"drag-drop"`
     push (Task 1): this isolated webview's own OS-level drag-drop handling
     swallows native HTML5 dragenter/dragleave/drop before they reach the
     DOM, so there are deliberately no ondragenter/ondragover handlers here. -->
<main class:drag={dragActive}>
  <header>
    <h1>{t('title')}</h1>
    <button class="link" onclick={() => (settingsOpen = !settingsOpen)}>
      {t('settings.toggle')} {settingsOpen ? '▲' : '▼'}
    </button>
  </header>

  {#if globalError}
    {@const desc = describeError(globalError)}
    <p class="error banner">
      {desc.text}
      {#if desc.detail}<span class="detail">{desc.detail}</span>{/if}
    </p>
  {/if}

  {#if settingsOpen}
    <section class="settings">
      <label>
        {t('settings.root')}
        <input type="text" bind:value={ebooksRoot} />
      </label>

      <!-- The two OCR services are alternatives, not a pair to fill in
           together: showing both services' fields at once invited filling in
           credentials for one while the other was selected. Pick the service,
           configure only that one. `provider` is the same state the OCR
           checkbox's selector binds to, so the two never disagree. -->
      <label>
        {t('settings.ocrProvider')}
        <select bind:value={provider}>
          <option value="wechat">{t('ocr.provider.wechat')}</option>
          <option value="baidu">{t('ocr.provider.baidu')}</option>
        </select>
      </label>

      {#if provider === 'wechat'}
        <label>
          {t('settings.wechatUrl')}
          <input type="text" bind:value={wechatUrl} />
        </label>
      {:else}
        <!-- Baidu has two credential pairs and only one of them works here;
             naming the console path is what stops the wrong one being pasted. -->
        <p class="field-hint">{t('settings.baiduHint')}</p>
        <label>
          {t('settings.baiduKey')}
          <input
            type="password"
            bind:value={baiduKeyInput}
            placeholder={baiduKeySet ? '••••••••' : ''}
          />
        </label>
        <label>
          {t('settings.baiduSecret')}
          <input
            type="password"
            bind:value={baiduSecretInput}
            placeholder={baiduSecretSet ? '••••••••' : ''}
          />
        </label>
      {/if}

      <div class="calibre-row">
        {#if calibreFound}
          <span class="ok">✓ {t('settings.calibre.found', { path: calibreFound.path, version: calibreFound.version })}</span>
        {:else}
          <span class="err">✗ {t('settings.calibre.missing')}</span>
          <a class="link" href="https://calibre-ebook.com" target="_blank" rel="noopener">
            {t('settings.calibre.install')}
          </a>
        {/if}
        <button class="secondary" onclick={pickCalibre}>{t('settings.calibre.pick')}</button>
      </div>

      <!-- The prompt is a file you own, not a box in this window: it lives in
           the vault, is plain markdown, and is git-versioned like everything
           else there. Clicking opens it in the main editor. -->
      <div class="prompt-row">
        <button class="prompt" onclick={editPrompt}>
          <span>{t('settings.prompt')}</span>
          <span class="path">{PROMPT_PATH}</span>
        </button>
        <p class="field-hint">{t('settings.promptHint')}</p>
      </div>

      <div class="save-row">
        <button class="primary" onclick={saveSettings} disabled={saving}>{t('settings.save')}</button>
        {#if savedFlash}<span class="ok">✓</span>{/if}
      </div>
    </section>
  {/if}

  <TopicBar
    {topics}
    counts={topicCounts}
    selectedId={selectedTopicId}
    disabled={running}
    onselect={selectImportTopic}
    onmanage={() => (topicManagerOpen = true)}
  />
  <div class="topic-actions">
    <button
      class="secondary"
      onclick={startTopicDesign}
      disabled={topicOperationRunning || library.length === 0 || !topicDesign.available}
      title={topicDesignStatus}
    >
      {topicAgentRunning ? t('topic.agentRunning') : t('topic.agentDesign')}
    </button>
    <button
      class="secondary"
      onclick={startTopicClassification}
      disabled={topicOperationRunning || topics.length === 0 || unclassifiedBooks.length === 0 || !topicDesign.available}
      title={topicDesignStatus}
    >
      {topicClassificationRunning
        ? t('topic.agentClassifying')
        : t('topic.agentClassify', { count: unclassifiedBooks.length })}
    </button>
    {#if topicAgents.length}
      <AgentPicker
        options={topicAgents}
        selected={topicAgentId ?? null}
        disabled={topicOperationRunning}
        onselect={pickTopicAgent}
        label={t as (k: string, v?: Record<string, string | number>) => string}
      />
    {/if}
    <span
      class:unavailable={!topicDesign.available}
      class:wide-scope={topicDesignWideScope}
      class="topic-agent-status"
    >
      {topicDesignStatus}
    </span>
    {#if unclassifiedBooks.length + unknownTopicBooks.length > 0}
      <span class="topic-warning">{t('topic.unclassifiedCount', { count: unclassifiedBooks.length + unknownTopicBooks.length })}</span>
    {/if}
    {#if unsafeTopicBooks.length > 0}
      <span class="topic-warning">{t('topic.unsafeBookCount', { count: unsafeTopicBooks.length })}</span>
    {/if}
  </div>

  <section class="dropzone">
    <p>{t('drop.hint')}</p>
    <button class="primary" onclick={pickFiles}>{t('drop.pick')}</button>
  </section>

  <!-- Disabled mid-run: the batch already locked these in, so leaving them
       live would suggest a change applies to files that are queued behind. -->
  <section class="ocr">
    <label class="ocr-toggle">
      <input type="checkbox" bind:checked={ocr} disabled={running} />
      {t('ocr.label')}
    </label>
    {#if ocr}
      <select bind:value={provider} disabled={running}>
        <option value="wechat">{t('ocr.provider.wechat')}</option>
        <option value="baidu">{t('ocr.provider.baidu')}</option>
      </select>
      <!-- Baidu bills per page, so a 300-page scan is real money. The settings
           pane says so too, but it's collapsed — this is the spot where the
           choice is actually made, right before Start. -->
      {#if provider === 'baidu'}
        <span class="cost">{t('ocr.baiduCost')}</span>
      {/if}
    {/if}
    <span class="hint">{t('ocr.onlyPdf')}</span>
  </section>

  <section class="queue">
    <div class="queue-head">
      <button
        class="primary start"
        onclick={startRun}
        disabled={running || !hasPending || hasUnclassifiedPending(q)}
      >
        {running ? t('action.running') : t('action.start')}
      </button>
      <span class="spacer"></span>
      <button class="link" onclick={clearFinished}>{t('action.clear')}</button>
    </div>
    {#if q.items.length === 0}
      <p class="empty">{t('queue.empty')}</p>
    {:else}
      {#each q.items as item (item.id)}
        <div class="row">
          <div class="row-main">
            <button class="chevron" onclick={() => toggleLog(item.id)} aria-label={t('log.toggle')}>
              {expanded[item.id] ? '▾' : '▸'}
            </button>
            <span class="name" title={item.path}>{item.name}</span>
            {#if item.status === 'pending'}
              <select
                class="topic-select"
                value={item.topicId ?? ''}
                aria-label={t('topic.chooseForBook', { name: item.name })}
                onchange={(event) => {
                  q = assignTopic(q, item.id, event.currentTarget.value)
                }}
              >
                <option value="">{t('topic.choose')}</option>
                {#each topics as topic (topic.id)}
                  <option value={topic.id}>{topic.label}</option>
                {/each}
              </select>
            {:else if item.topicId}
              <span class="topic-chip">{topics.find((topic) => topic.id === item.topicId)?.label ?? item.topicId}</span>
            {/if}
            <span class="badge {item.status}{item.cancelled ? ' cancelled' : ''}">
              {t(badgeKey(item))}
              {#if item.status === 'running' && item.total}
                {' '}{item.page ?? 0}/{item.total}
              {/if}
            </span>
            {#if item.status === 'running' && stageLabel(item)}
              <span class="stage">{stageLabel(item)}</span>
            {/if}
            {#if item.status === 'running'}
              <button class="secondary" onclick={() => cancelItem(item)}>{t('action.cancel')}</button>
            {/if}
            {#if item.status === 'done'}
              <button class="link" onclick={() => openInEditor(item)}>{t('action.openInEditor')}</button>
              {#if !item.aiStatus || item.aiStatus === 'failed'}
                <!-- `[ AI 先读 ] by Claude ▾` — the same pairing as every other
                     place a run can be started. -->
                <button class="link" onclick={() => aiRead(item)}>{t('action.aiRead')}</button>
                {#if agents.length}
                  <AgentPicker
                    options={agents}
                    selected={agentId ?? null}
                    onselect={pickAgent}
                    label={t as (k: string, v?: Record<string, string | number>) => string}
                  />
                {/if}
              {:else if item.aiStatus === 'queued'}
                <span class="stage">{t('ai.queued')}</span>
              {:else if item.aiStatus === 'running'}
                <span class="stage">{t('ai.running', { elapsed: formatElapsed(item.aiStartedAt, nowMs) })}</span>
              {:else if item.aiStatus === 'done'}
                <button class="link" onclick={() => openSummary(item)}>{t('action.viewSummary')}</button>
              {/if}
            {/if}
          </div>
          {#if item.status === 'done' && item.destRel}
            <p class="dest">{item.destRel}</p>
          {/if}
          {#if item.status === 'failed' && !item.cancelled && item.error}
            {@const desc = describeError(item.error)}
            <p class="error">
              {desc.text}
              {#if desc.detail}<span class="detail">{desc.detail}</span>{/if}
            </p>
          {/if}
          {#if item.aiStatus === 'failed' && item.aiError}
            <p class="error">{t('ai.failed')} <span class="detail">{item.aiError}</span></p>
          {/if}
          {#if expanded[item.id]}
            <pre class="log">{item.logs.map(describeLog).join('\n')}</pre>
          {/if}
        </div>
      {/each}
    {/if}
  </section>

  <LibraryPanel
    books={library}
    {topics}
    {agents}
    agentId={agentId ?? null}
    {nowMs}
    onread={libraryRead}
    onopenbook={(b) => openPath(`${b.rel}/book.md`)}
    onopensummary={(b) => {
      const s = latestSummary(b)
      if (s) void openPath(s)
    }}
    onpickagent={pickAgent}
    onrefresh={loadLibrary}
    onassigntopic={assignLibraryTopic}
  />

  <TopicManager
    open={topicManagerOpen}
    {topics}
    counts={topicCounts}
    onsave={saveTopics}
    onclose={() => (topicManagerOpen = false)}
  />

  {#if classificationProposal}
    <TopicClassificationReview
      proposal={classificationProposal}
      expectedBooks={classificationExpectedBooks}
      books={library}
      {topics}
      applying={classificationApplying}
      onchange={reviseClassification}
      onapply={applyTopicClassification}
      oncancel={() => {
        if (!classificationApplying) {
          classificationProposal = null
          classificationExpectedBooks = []
        }
      }}
    />
  {/if}

  {#if topicProposal}
    <div class="proposal-backdrop">
      <div class="proposal" role="dialog" aria-modal="true" aria-labelledby="proposal-title">
        <h2 id="proposal-title">{t('topic.proposalTitle')}</h2>
        <p>{t('topic.proposalHint')}</p>
        <div class="proposal-topics">
          {#each topicProposal.topics as topic (topic.id)}
            <article>
              <strong>{topic.label}</strong>
              <span>{topic.description}</span>
              <small>
                {t('topic.assignmentCount', {
                  count: topicProposal.assignments.filter((item) => item.topic_id === topic.id).length,
                })}
              </small>
            </article>
          {/each}
        </div>
        <div class="proposal-actions">
          <button class="secondary" onclick={() => (topicProposal = null)}>{t('action.cancel')}</button>
          <button class="primary" onclick={applyTopicProposal}>{t('topic.applyProposal')}</button>
        </div>
      </div>
    </div>
  {/if}
</main>

<style>
  :global(:root) {
    color-scheme: light dark;
  }
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
    font-size: 13px;
  }
  main {
    box-sizing: border-box;
    min-height: 100vh;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }
  h1 {
    font-size: 15px;
    margin: 0;
  }
  button {
    font: inherit;
    cursor: pointer;
  }
  button.link {
    background: none;
    border: none;
    color: inherit;
    opacity: 0.65;
    padding: 0;
  }
  button.link:hover {
    opacity: 1;
  }
  button.primary {
    background: color-mix(in srgb, currentColor 12%, transparent);
    border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
    border-radius: 6px;
    padding: 5px 14px;
    font-weight: 600;
    color: inherit;
  }
  button.secondary {
    background: transparent;
    border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
    border-radius: 6px;
    padding: 3px 10px;
    color: inherit;
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .settings {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border-radius: 8px;
    border: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }
  .settings label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 11px;
    opacity: 0.8;
  }
  .settings input,
  .settings select {
    font: inherit;
    font-size: 13px;
    padding: 5px 7px;
    border-radius: 5px;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    background: transparent;
    color: inherit;
  }
  .field-hint {
    margin: 0;
    font-size: 11px;
    line-height: 1.5;
    opacity: 0.55;
  }
  .calibre-row {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
  }
  .save-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
  }
  .prompt-row {
    margin-top: 2px;
    padding-top: 8px;
    border-top: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }
  .prompt {
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    padding: 4px 6px;
    border: 1px solid transparent;
    border-radius: 6px;
    background: none;
    color: inherit;
    text-align: left;
    font-size: 12px;
  }
  .prompt:hover,
  .prompt:focus-visible {
    background: color-mix(in srgb, currentColor 8%, transparent);
  }
  /* The file behind the row — the point of "the prompt is a file you own". */
  .prompt .path {
    margin-left: auto;
    font-size: 10px;
    opacity: 0.55;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dropzone {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 22px;
    border: 1.5px dashed color-mix(in srgb, currentColor 30%, transparent);
    border-radius: 10px;
    text-align: center;
    opacity: 0.85;
    transition: border-color 0.15s, background 0.15s;
  }
  main.drag .dropzone {
    border-color: color-mix(in srgb, currentColor 60%, transparent);
    background: color-mix(in srgb, currentColor 6%, transparent);
    opacity: 1;
  }
  .dropzone p {
    margin: 0;
    font-size: 12px;
    opacity: 0.7;
  }
  .ocr {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
  }
  .ocr-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .ocr select {
    font: inherit;
    font-size: 12px;
    background: transparent;
    color: inherit;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    border-radius: 5px;
    padding: 3px 6px;
  }
  .ocr .hint {
    opacity: 0.5;
    font-size: 11px;
  }
  /* Warmer than .hint: spending money should register, without alarming. */
  .ocr .cost {
    font-size: 11px;
    padding: 1px 7px;
    border-radius: 9px;
    color: #8a5a00;
    background: color-mix(in srgb, #e0a800 22%, transparent);
  }
  @media (prefers-color-scheme: dark) {
    .ocr .cost { color: #f0c04a; }
  }
  /* Shrinks to its contents rather than claiming half the window: opening this
     plugin to browse the library with an empty queue is the common case, and
     `flex: 1` would leave a large blank area above the books. Capped so a long
     queue can't push the library off-screen either — it scrolls instead. */
  .queue {
    flex: 0 1 auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
    max-height: 45vh;
    overflow-y: auto;
  }
  .queue-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .queue-head .spacer { flex: 1; }
  .start { padding: 4px 16px; }
  .empty {
    opacity: 0.5;
    font-size: 12px;
    text-align: center;
    padding: 16px 0;
  }
  .row {
    padding: 6px 4px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 10%, transparent);
  }
  .row-main {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .chevron {
    background: none;
    border: none;
    color: inherit;
    opacity: 0.5;
    width: 14px;
    padding: 0;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .badge {
    font-size: 10px;
    letter-spacing: 0.02em;
    padding: 2px 7px;
    border-radius: 10px;
    background: color-mix(in srgb, currentColor 10%, transparent);
    opacity: 0.85;
    flex: none;
  }
  .badge.done {
    color: #2e7d32;
  }
  .badge.failed {
    color: #c62828;
  }
  .badge.failed.cancelled {
    color: inherit;
    opacity: 0.6;
  }
  .badge.running {
    color: #1565c0;
  }
  .stage {
    font-size: 10px;
    opacity: 0.55;
    flex: none;
  }
  .dest {
    margin: 2px 0 0 22px;
    font-size: 11px;
    opacity: 0.6;
  }
  p.error {
    margin: 2px 0 0 22px;
    font-size: 11px;
    color: #c62828;
  }
  p.error.banner {
    margin: 0;
    padding: 6px 10px;
    border-radius: 6px;
    background: color-mix(in srgb, #c62828 12%, transparent);
  }
  .detail {
    display: block;
    margin-top: 2px;
    font-size: 10px;
    opacity: 0.65;
  }
  .log {
    margin: 4px 0 0 22px;
    padding: 6px 8px;
    max-height: 160px;
    overflow: auto;
    font-size: 11px;
    background: color-mix(in srgb, currentColor 6%, transparent);
    border-radius: 6px;
    white-space: pre-wrap;
  }
  .ok {
    color: #2e7d32;
  }
  .err {
    color: #c62828;
  }
  .topic-actions {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-top: -6px;
  }
  .topic-warning {
    font-size: 10px;
    color: #b26a00;
  }
  .topic-agent-status {
    font-size: 10px;
    color: #2e7d32;
  }
  .topic-agent-status.unavailable {
    color: #b26a00;
  }
  .topic-agent-status.wide-scope {
    color: #b26a00;
  }
  .topic-select {
    max-width: 150px;
    min-width: 100px;
    font: inherit;
    font-size: 11px;
  }
  .topic-chip {
    max-width: 130px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 2px 7px;
    border-radius: 10px;
    background: color-mix(in srgb, var(--accent-color, #0a84ff) 12%, transparent);
    font-size: 10px;
  }
  .proposal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: 20px;
    background: rgb(0 0 0 / 28%);
  }
  .proposal {
    width: min(560px, 100%);
    max-height: calc(100vh - 40px);
    overflow: auto;
    box-sizing: border-box;
    padding: 16px;
    border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
    border-radius: 10px;
    background: var(--background-color, Canvas);
    box-shadow: 0 18px 55px rgb(0 0 0 / 24%);
  }
  .proposal h2,
  .proposal p {
    margin: 0 0 8px;
  }
  .proposal-topics {
    display: grid;
    gap: 7px;
    margin: 12px 0;
  }
  .proposal-topics article {
    display: grid;
    gap: 3px;
    padding: 9px;
    border: 1px solid color-mix(in srgb, currentColor 13%, transparent);
    border-radius: 7px;
  }
  .proposal-topics span,
  .proposal-topics small {
    opacity: 0.65;
  }
  .proposal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
