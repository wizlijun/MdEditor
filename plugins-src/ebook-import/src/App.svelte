<script lang="ts">
  import { bridge } from './lib/bridge'
  import { describeLog } from './lib/logs'
  import { setLocale, t, type MessageKey } from './lib/strings'
  import { describeError } from './lib/errors'
  import {
    addPaths,
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
  type HostPush = JobPush | DragPush | AiPush | { type: string }

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
    if (running || !hasPending) return
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
        q = addPaths(q, d.paths ?? [])
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
    } else if (m.type === 'ai_read') {
      const a = m as AiPush
      q = onAiEvent(q, a.job_id, {
        event: a.event,
        started_at: a.started_at,
        summary_rel: a.summary_rel,
        error: a.error,
      })
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
        q = addPaths(q, paths)
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
    try {
      const dir = item.destRel.replace(/\/+$/, '')
      await bridge().request('host.editor.open', { path: `${dir}/book.md` })
    } catch (e) {
      globalError = message(e)
    }
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
      })
    } catch (e) {
      q = onAiEvent(q, jobId, { event: 'failed', error: message(e) })
      globalError = message(e)
    }
  }

  async function openSummary(item: QueueItem) {
    if (!item.aiSummaryRel) return
    try {
      await bridge().request('host.editor.open', { path: item.aiSummaryRel })
    } catch (e) {
      globalError = message(e)
    }
  }

  // 「AI 阅读中… 3m12s」的秒针。
  // effect 的依赖必须是这个 boolean,不能是整个 q:并发导入时日志/进度事件
  // 每 <1s 就刷新 q,effect 每次都会在 interval 触发前把它重建,秒数永远停在
  // 0s。$derived 的值不变就不通知下游,所以 anyAiRunning 只在真正切换时重挂。
  // interval 只写 nowMs,而 nowMs 不在依赖里 —— 不会自失效死循环($effect 纪律)。
  let nowMs = $state(Date.now())
  const anyAiRunning = $derived(q.items.some((i) => i.aiStatus === 'running'))
  $effect(() => {
    if (!anyAiRunning) return
    const t = setInterval(() => {
      nowMs = Date.now()
    }, 1000)
    return () => clearInterval(t)
  })
  function aiElapsed(item: QueueItem): string {
    if (!item.aiStartedAt) return ''
    const s = Math.max(0, Math.floor((nowMs - Date.parse(item.aiStartedAt)) / 1000))
    const m = Math.floor(s / 60)
    return m > 0 ? `${m}m${s % 60}s` : `${s}s`
  }

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

  void loadEnv()
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

      <div class="save-row">
        <button class="primary" onclick={saveSettings} disabled={saving}>{t('settings.save')}</button>
        {#if savedFlash}<span class="ok">✓</span>{/if}
      </div>
    </section>
  {/if}

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
      <button class="primary start" onclick={startRun} disabled={running || !hasPending}>
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
                <button class="link" onclick={() => aiRead(item)}>{t('action.aiRead')}</button>
              {:else if item.aiStatus === 'queued'}
                <span class="stage">{t('ai.queued')}</span>
              {:else if item.aiStatus === 'running'}
                <span class="stage">{t('ai.running', { elapsed: aiElapsed(item) })}</span>
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
  .queue {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
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
</style>
