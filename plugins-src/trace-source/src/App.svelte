<!-- App.svelte — the Trace Source window: a delegation composer with a
     report inbox beside it.

     Same visual grammar as idea-spark (this window IS its trimmed sibling):
     the editor starts at the top edge and runs down to a single 38px action
     bar; the inbox *squeezes* the editor rather than covering it and is
     hidden by default.

       ┌───────────────────────┬─────────┐
       │ editor (flex:1)       │ inbox   │  ← past reports, newest first
       ├───────────────────────┴─────────┤
       │ status   New  Trace  by ▾  ▤  ⚙│  ← 38px bar
       └─────────────────────────────────┘

     The document model is deliberately asymmetric to idea-spark's: the
     composed text is NOT a file — it lives exactly as long as the delegation
     it is for. The durable artifacts are the agent's reports,
     `<traceDir>/<YYYY-MM-DD>-<HHmmss>-source-trace.md`, and the inbox lists
     exactly those. Rows open in the MAIN editor: a report is a ✦ product to
     read, not a draft to edit here.

     Settings (`.notemd/trace-source.json`): the report directory (default
     `inbox/traces`) and whether the inbox is open. Both persist best-effort —
     a vault whose `.notemd/` can't be written costs the memory of the
     choices, never the function.

     The passage arrives prefilled from the host's right-click 「溯源」 item —
     via the window URL's `?seed=` on a fresh window, via a `seed` host push on
     one that is already open. A new seed REPLACES the buffer: reaching for
     the tool again means "trace this now".

     After a delegation the window polls the run (same 2s cadence as
     idea-spark, a setTimeout chain, never an `$effect` — MEMORY
     feedback_svelte_effect_untrack) purely for its own inline progress and to
     refresh the inbox the moment the report lands. The run itself belongs to
     the agent plugin: closing this window loses the progress line and nothing
     else — the tray notification still arrives.

     Declares `color-scheme: light dark` so this standalone window follows the
     system appearance (MEMORY reference_webview_color_scheme). -->
<script lang="ts">
  import { onMount, tick } from 'svelte'
  import Icon from './components/Icon.svelte'
  import InboxPanel from './components/InboxPanel.svelte'
  import SettingsPopover from './components/SettingsPopover.svelte'
  import AgentPicker from './lib/agent-picker/AgentPicker.svelte'
  import {
    rememberProvider,
    rememberedProvider,
    type AgentOption,
  } from './lib/agent-picker/types'
  import {
    agentProviders,
    agentStatus,
    editorOpen,
    bridge,
    vaultInfo,
    vaultList,
    vaultRead,
    vaultRemove,
    vaultWrite,
  } from './lib/bridge'
  import {
    delegateTrace,
    interpretStatus,
    POLL_MS,
    seedTemplates,
    type RunView,
  } from './lib/delegate'
  import { deleteReport, listReports, previewDelete, type ReportEntry } from './lib/inbox'
  import { loadKit, type KitEditor } from './lib/editor-kit'
  import { parseState, serializeState, STATE_PATH, type TraceState } from './lib/state-io'
  import { TRACE_TASK_ID } from './lib/trace-template'
  import { setLocale, t, tv } from './lib/strings'

  setLocale(bridge().locale)

  let editorEl: HTMLDivElement | undefined = $state()
  let kit: KitEditor | null = null
  /** Editor content while the kit is unavailable (the degraded textarea). */
  let fallbackText = $state('')
  let kitFailed = $state(false)
  let booting = $state(true)
  let needVault = $state(false)
  let vaultRoot = $state<string | null>(null)
  let settingsOpen = $state(false)

  /** `.notemd/trace-source.json`, parsed. Written back best-effort on change. */
  let settings = $state<TraceState>(parseState(null))

  // ── delegation lifecycle ──────────────────────────────────────────────────
  /** A `host.agent.run` call is in flight (between the click and the run id). */
  let delegating = $state(false)
  /** The run being watched, with its newest progress line. */
  let run = $state<{ id: string; last: string } | null>(null)
  /** The last watched run's outcome; cleared by the next edit or New. */
  let finished = $state<'ok' | 'fail' | null>(null)
  /** A failure's message — shown in the bar, not in a toast that scrolls away. */
  let errorMsg = $state('')
  /** No agent plugin installed — the layer pointing at the market. */
  let agentMissing = $state(false)

  // ── the report inbox ──────────────────────────────────────────────────────
  let reports = $state<ReportEntry[]>([])
  let listFailed = $state(false)

  // ── which agent runs this trace ───────────────────────────────────────────
  const AGENT_SURFACE = 'trace-source'
  let agents: AgentOption[] = $state([])
  let agentId: string | undefined = $state(undefined)

  async function loadAgents() {
    try {
      const r = await agentProviders()
      agents = r.providers ?? []
      agentId = rememberedProvider(
        AGENT_SURFACE,
        agents.map((a) => a.id),
        r.default,
      )
    } catch {
      agents = []
      agentId = undefined
    }
  }

  function pickAgent(id: string) {
    agentId = id
    rememberProvider(AGENT_SURFACE, id)
  }

  /** Best-effort settings write: losing the memory of a toggle must never
   *  cost the user anything else. */
  async function persist(): Promise<void> {
    try {
      await vaultWrite(STATE_PATH, serializeState({ traceDir: settings.traceDir, inboxOpen: settings.inboxOpen }))
    } catch (e) {
      console.warn('[trace-source] persisting settings failed:', e)
    }
  }

  async function refreshReports(): Promise<void> {
    try {
      reports = await listReports({ list: vaultList, read: vaultRead }, settings.traceDir)
      listFailed = false
    } catch {
      // A directory that simply doesn't exist yet is not a failure — it means
      // no report has ever been written. Telling those apart matters: an
      // exists-check first, and only a listable-but-unlistable directory
      // reads as "couldn't read".
      reports = []
      try {
        const { exists } = await bridge().request('host.vault.exists', { path: settings.traceDir })
        listFailed = exists === true
      } catch {
        listFailed = true
      }
    }
  }

  async function toggleInbox(): Promise<void> {
    settings.inboxOpen = !settings.inboxOpen
    if (settings.inboxOpen) void refreshReports()
    await persist()
  }

  async function commitTraceDir(dir: string): Promise<void> {
    settings.traceDir = dir
    await persist()
    if (settings.inboxOpen) void refreshReports()
  }

  function openReport(name: string): void {
    void editorOpen(`${settings.traceDir}/${name}`).catch((e) => {
      errorMsg = e instanceof Error ? e.message : String(e)
    })
  }

  async function removeReport(name: string): Promise<void> {
    try {
      await deleteReport({ list: vaultList, read: vaultRead, remove: vaultRemove }, settings.traceDir, name)
    } catch (e) {
      errorMsg = e instanceof Error ? e.message : String(e)
    }
    await refreshReports()
  }

  function previewRemove(name: string): Promise<string[]> {
    return previewDelete({ list: vaultList }, settings.traceDir, name)
  }

  async function editPrompt(): Promise<void> {
    await seedTemplates()
    try {
      await editorOpen(`.notemd/agent-tasks/${TRACE_TASK_ID}/CLAUDE.md`)
    } catch (e) {
      errorMsg = e instanceof Error ? e.message : String(e)
    }
  }

  /** Whatever the live editor holds — kit or fallback. */
  function markdown(): string {
    return kit ? kit.getMarkdown() : fallbackText
  }

  function showMarkdown(md: string): void {
    if (kit) kit.setMarkdown(md)
    else fallbackText = md
  }

  /** A keystroke after a finished run starts the next composition: a stale
   *  "done" would otherwise claim the NEW text was already handled. */
  function onEdited(md: string): void {
    fallbackText = md
    if (finished) finished = null
    if (errorMsg) errorMsg = ''
  }

  function applySeed(text: string): void {
    showMarkdown(text)
    finished = null
    errorMsg = ''
    kit?.focus()
  }

  function startNew(): void {
    applySeed('')
  }

  // ── watching one run to its end ───────────────────────────────────────────
  /** Consecutive failed status calls before the watcher gives up. Giving up
   *  leaves the run alone on purpose: "I can't reach the agent" is not
   *  evidence the run failed — the notification still arrives. */
  const MAX_POLL_ERRORS = 5
  let pollTimer: ReturnType<typeof setTimeout> | null = null
  let unmounted = false

  function watch(runId: string): void {
    if (unmounted) return
    let errors = 0

    const schedule = () => {
      if (!unmounted) pollTimer = setTimeout(() => void poll(), POLL_MS)
    }

    const poll = async () => {
      pollTimer = null
      if (unmounted || run?.id !== runId) return

      let view: RunView
      try {
        view = interpretStatus(await agentStatus(TRACE_TASK_ID, runId))
        errors = 0
      } catch (e) {
        console.warn('[trace-source] a run status poll failed:', e)
        errors += 1
        if (errors < MAX_POLL_ERRORS) schedule()
        else run = null
        return
      }
      if (unmounted || run?.id !== runId) return

      if (view.kind === 'running') {
        run = { id: runId, last: view.last }
        schedule()
        return
      }

      run = null
      if (view.kind === 'done' && view.success) {
        finished = 'ok'
        // The report just landed: show it without waiting for a manual toggle.
        if (settings.inboxOpen) void refreshReports()
      } else {
        finished = 'fail'
        errorMsg = view.kind === 'done' && view.message ? view.message : ''
      }
    }

    schedule()
  }

  async function delegate(): Promise<void> {
    if (delegating || booting || needVault || !vaultRoot) return
    const text = markdown()
    if (!text.trim()) {
      errorMsg = t('delegateEmpty')
      return
    }
    delegating = true
    try {
      const r = await delegateTrace(text, vaultRoot, settings.traceDir, agentId)
      if (!r.ok) {
        if (r.reason === 'agent-missing') agentMissing = true
        else errorMsg = r.message
        return
      }
      finished = null
      errorMsg = ''
      run = { id: r.runId, last: '' }
      watch(r.runId)
    } finally {
      delegating = false
    }
  }

  /** Focuses the agent-missing layer so Esc reaches it (see its `onkeydown`). */
  function takeFocus(node: HTMLElement) {
    node.focus()
  }

  onMount(() => {
    let disposed = false

    bridge().onMessage((payload) => {
      const type = (payload as { type?: string } | null)?.type
      if (type === 'theme-changed') return
      if (type === 'seed') {
        const text = (payload as { payload?: { text?: string } } | null)?.payload?.text
        if (typeof text === 'string' && text) applySeed(text)
        return
      }
      console.debug('[trace-source] unhandled host push:', payload)
    })

    void (async () => {
      try {
        const info = await vaultInfo()
        vaultRoot = info.root
        needVault = !info.root
      } catch (e) {
        console.error('[trace-source] host.vault.info failed:', e)
        needVault = true
      }
      if (!needVault) {
        try {
          settings = parseState((await vaultRead(STATE_PATH)).content)
        } catch {
          settings = parseState(null) // first run: the file simply isn't there
        }
      }
      booting = false
      if (disposed || needVault) return
      void loadAgents()
      if (settings.inboxOpen) void refreshReports()

      // A fresh window gets its prefill through the URL query (an already-open
      // one gets a `seed` push instead).
      let initial = ''
      try {
        const raw = new URLSearchParams(location.search).get('seed')
        if (raw) {
          const seed = JSON.parse(raw) as { text?: string }
          if (typeof seed.text === 'string') initial = seed.text
        }
      } catch (e) {
        console.warn('[trace-source] bad seed query:', e)
      }
      fallbackText = initial

      await tick()
      if (disposed) return
      try {
        if (!editorEl) throw new Error('editor container missing')
        const mount = await loadKit()
        if (disposed) return
        kit = await mount(editorEl, {
          initialMarkdown: initial,
          mode: 'rich',
          placeholder: t('placeholder'),
          onChange: onEdited,
        })
        if (disposed) {
          kit.destroy()
          kit = null
          return
        }
        kit.focus()
      } catch (e) {
        console.error('[trace-source] the editor kit failed to load:', e)
        try { kit?.destroy() } catch { /* already broken */ }
        kit = null
        kitFailed = true
      }
    })()

    return () => {
      disposed = true
      unmounted = true
      if (pollTimer !== null) clearTimeout(pollTimer)
      kit?.destroy()
      kit = null
    }
  })
</script>

<main class="app">
  {#if needVault}
    <div class="notice">{t('needVault')}</div>
  {:else}
    <div class="content">
      <section class="editor-col">
        {#if kitFailed}
          <p class="warn">{t('editorUnavailable')}</p>
          <textarea
            class="fallback"
            bind:value={fallbackText}
            placeholder={t('placeholder')}
            spellcheck="false"
            oninput={() => onEdited(fallbackText)}
          ></textarea>
        {:else}
          <div class="editor" bind:this={editorEl}></div>
        {/if}
      </section>

      {#if settings.inboxOpen}
        <InboxPanel
          {reports}
          {listFailed}
          onopen={openReport}
          ondelete={removeReport}
          onpreviewdelete={previewRemove}
          ontoggle={() => void toggleInbox()}
        />
      {/if}
    </div>

    <div class="actionbar">
      <span class="status" class:error={!!errorMsg && !run}>
        {#if delegating}
          {t('delegating')}
        {:else if run}
          <span class="running">
            <Icon name="running" />
            <span class="runtext">{t('delegated')}{run.last ? ` · ${run.last}` : ''}</span>
          </span>
        {:else if finished === 'ok'}
          {t('traceDone')}
        {:else if finished === 'fail'}
          {t('traceFailed')}{errorMsg ? ` · ${errorMsg}` : ''}
        {:else if errorMsg}
          {errorMsg}
        {/if}
      </span>
      <div class="spacer"></div>
      <button type="button" class="ghost" onclick={startNew}>
        <Icon name="trace" />
        {t('newTrace')}
      </button>
      <button
        type="button"
        class="ghost"
        disabled={delegating}
        onclick={() => void delegate()}
      >
        <Icon name="delegate" />
        {delegating ? t('delegating') : t('delegate')}
      </button>
      {#if agents.length}
        <AgentPicker
          options={agents}
          selected={agentId ?? null}
          disabled={delegating}
          onselect={pickAgent}
          label={tv as (k: string, v?: Record<string, string | number>) => string}
        />
      {/if}
      <button
        type="button"
        class="icon"
        aria-pressed={settings.inboxOpen}
        aria-label={t('inbox')}
        title={t('inbox')}
        onclick={() => void toggleInbox()}
      >
        <Icon name="inbox" />
      </button>
      <button
        type="button"
        class="icon"
        aria-label={t('settings')}
        aria-expanded={settingsOpen}
        title={t('settings')}
        onclick={() => (settingsOpen = !settingsOpen)}
      >
        <Icon name="settings" />
      </button>
      {#if settingsOpen}
        <SettingsPopover
          traceDir={settings.traceDir}
          onclose={() => (settingsOpen = false)}
          oncommit={commitTraceDir}
          oneditprompt={editPrompt}
        />
      {/if}
    </div>
  {/if}

  <!-- No agent plugin: the one thing the user can do about it is install one,
       so say that instead of a raw `agent_unavailable:` message. -->
  {#if agentMissing}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="backdrop" onclick={() => (agentMissing = false)}></div>
    <div
      class="layer"
      role="dialog"
      aria-modal="true"
      aria-labelledby="agent-missing-title"
      tabindex="-1"
      use:takeFocus
      onkeydown={(e) => {
        if (e.key === 'Escape') {
          e.preventDefault()
          e.stopPropagation()
          agentMissing = false
        }
      }}
    >
      <h2 id="agent-missing-title">{t('agentMissing')}</h2>
      <p>{t('agentMissingHint')}</p>
      <div class="layer-actions">
        <button type="button" class="ghost" onclick={() => (agentMissing = false)}>{t('close')}</button>
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
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: Canvas;
    color: CanvasText;
  }
  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
  }
  .spacer { flex: 1; }
  .notice {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0.65;
    font-size: 0.9rem;
  }
  .content {
    flex: 1;
    display: flex;
    min-height: 0;
  }
  .editor-col {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  /* The kit sizes itself with height:100% + absolute positioning, so its
     container MUST have a determinate height (see idea-spark's App.svelte). */
  .editor {
    flex: 1;
    display: flex;
    position: relative;
    min-height: 0;
    overflow: hidden;
  }
  .warn {
    flex: 0 0 auto;
    margin: 0;
    padding: 0.4rem 0.75rem;
    font-size: 0.8rem;
    background: color-mix(in srgb, #dc2626 12%, transparent);
  }
  .fallback {
    flex: 1;
    min-height: 0;
    box-sizing: border-box;
    padding: 0.75rem;
    border: 0;
    resize: none;
    background: Canvas;
    color: CanvasText;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.9rem;
    line-height: 1.55;
  }
  .fallback:focus { outline: none; }
  .actionbar {
    position: relative;
    flex: 0 0 38px;
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0 0.75rem;
    border-top: 1px solid var(--line, #e5e7eb);
  }
  .status {
    font-size: 0.78rem;
    opacity: 0.6;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  .status.error {
    color: #dc2626;
    opacity: 1;
  }
  /* Icon + text on one line; `min-width: 0` lets the ellipsis engage instead
     of pushing the buttons off the bar (idea-spark's runtext arrangement). */
  .status .running {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    max-width: 100%;
    min-width: 0;
    vertical-align: middle;
  }
  .status .runtext {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actionbar > button {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.25rem 0.7rem;
    border-radius: 6px;
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .ghost {
    border: 1px solid var(--line, #d1d5db);
    background: none;
    color: inherit;
  }
  .actionbar > button:disabled { opacity: 0.5; cursor: default; }
  /* Scoped through `.actionbar >` so these beat the generic button rule above
     on specificity rather than on source order. */
  .actionbar > button.icon {
    padding: 0.3rem;
    border: 0;
    background: none;
    color: inherit;
  }
  .actionbar > button.icon:hover:not(:disabled) {
    background: color-mix(in srgb, currentColor 10%, transparent);
  }
  /* The agent-missing layer — same geometry as idea-spark's. */
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
    background: rgb(0 0 0 / 0.28);
  }
  .layer {
    position: fixed;
    z-index: 41;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(420px, calc(100vw - 2rem));
    box-sizing: border-box;
    padding: 1rem;
    border: 1px solid var(--line, #e5e7eb);
    border-radius: 10px;
    background: Canvas;
    color: CanvasText;
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.28);
  }
  .layer:focus { outline: none; }
  .layer h2 {
    margin: 0 0 0.4rem;
    font-size: 0.95rem;
  }
  .layer p {
    margin: 0;
    font-size: 0.82rem;
    line-height: 1.45;
    opacity: 0.8;
  }
  .layer-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 0.9rem;
  }
  .layer-actions button {
    padding: 0.3rem 0.7rem;
    border-radius: 6px;
    font: inherit;
    font-size: 0.82rem;
    cursor: pointer;
  }
</style>
