<script lang="ts">
  import { bridge, request, onMessage } from './lib/bridge'
  import { t, type MessageKey } from './lib/strings'
  import { errorKey } from './lib/errors'
  import {
    emptyView,
    reduce,
    type RunView,
    type HostMessage,
    type Task,
    type RunRecord,
    type HarnessStatus,
  } from './lib/events'
  import TaskList from './components/TaskList.svelte'
  import RunStream from './components/RunStream.svelte'
  import HarnessBanner from './components/HarnessBanner.svelte'
  import RunLog from './components/RunLog.svelte'
  import HistoryList from './components/HistoryList.svelte'
  import ArtifactLinks from './components/ArtifactLinks.svelte'
  import SettingsPage from './components/SettingsPage.svelte'
  import UsageSummary from './components/UsageSummary.svelte'
  import { loadUsageDisplay, type UsageDisplay } from './lib/settings'

  const locale = bridge().locale
  const tr = (k: MessageKey, v?: Record<string, string | number>) => t(locale, k, v)

  let tasks: Task[] = $state([])
  /** Which task the Run button will start. */
  let selectedTask: string | null = $state(null)
  let userPrompt = $state('')
  let ctx: { path: string; selection: string } | null = $state(null)
  let useCtx = $state(true)
  let view: RunView = $state(emptyView())
  let history: RunRecord[] = $state([])
  let allTasks = $state(true)
  let error = $state('')
  /** The harness behind this window.
   *  `undefined` = not asked yet, `null` = asked and the backend could not say.
   *  Collapsing the two made a failed probe render as a spinner that never
   *  stopped, which is what "正在检查运行环境…" forever actually was. */
  let harness: HarnessStatus | null | undefined = $state(undefined)
  /** A past run picked from the list: the centre pane shows ITS log instead of
   *  the live stream, until you go back or start a new run. */
  let selectedRun: RunRecord | null = $state(null)
  let selectedLog = $state('')
  let settingsOpen = $state(false)
  let runUsageDisplay: UsageDisplay = $state('tip')

  const running = $derived(view.status === 'running')
  const current = $derived(tasks.find((t) => t.id === selectedTask) ?? null)

  onMessage((m: HostMessage) => {
    view = reduce(view, m)
    // A finished run belongs in the list — and in the task's status — at once.
    if (m.kind === 'done' || m.kind === 'busy') {
      void refresh()
      // A run is when an expired credential stops being hypothetical.
      void loadHarness()
    }
  })

  // The backend resolves the vault root asynchronously (asking the host for it
  // from inside activate would deadlock its protocol loop), so the first
  // tasks.list can legitimately answer "not ready yet". Retry briefly instead
  // of showing an empty list.
  /**
   * Ask the backend what harness it has. Called at startup and after every run:
   * a run is exactly when an expired credential becomes visible, and the banner
   * would otherwise keep claiming everything is fine.
   */
  async function loadHarness() {
    try {
      harness = await request('harness-status')
    } catch {
      // A backend too old to answer, or one that failed to start. Say so —
      // `null` is "we asked and got nothing", which the banner renders as a
      // stated unknown rather than as a probe still in flight.
      harness = null
    }
  }

  async function load() {
    try {
      for (let i = 0; i < 20 && !(await loadTasks()); i++) {
        await new Promise((r) => setTimeout(r, 250))
      }
      if (!selectedTask && tasks.length) selectedTask = tasks[0].id
      const c = await request('context.get')
      ctx = c.tab?.path ? { path: c.tab.path, selection: c.tab.selection ?? '' } : null
      // Status probing starts local processes and is deliberately last so it
      // cannot hold up the task list or editor context on the protocol loop.
      void loadHarness()
    } catch (e) {
      error = message(e)
    }
  }

  /** Returns whether the vault was resolved (i.e. the list is trustworthy). */
  async function loadTasks(): Promise<boolean> {
    const r = await request('tasks.list')
    tasks = r.tasks
    return r.ready !== false
  }

  async function loadHistory() {
    try {
      const task = allTasks ? null : selectedTask
      if (!allTasks && !task) {
        history = []
        return
      }
      history = (await request('history.list', { task })).runs
    } catch {
      history = []
    }
  }

  async function refresh() {
    await Promise.all([loadTasks().catch(() => {}), loadHistory()])
  }

  // A detached CLI run lives in another process, so its status only reaches us
  // through the lock file and the run records — poll for it.
  $effect(() => {
    const id = setInterval(() => void refresh(), 5000)
    return () => clearInterval(id)
  })

  async function selectRun(run: RunRecord) {
    settingsOpen = false
    selectedRun = run
    selectedLog = ''
    try {
      selectedLog = (await request('history.log', { task: run.task, run_id: run.run_id })).log ?? ''
    } catch (e) {
      error = message(e)
    }
  }

  async function deleteRun(run: RunRecord) {
    if (selectedRun?.run_id === run.run_id) selectedRun = null
    try {
      await request('history.delete', { task: run.task, run_id: run.run_id })
    } catch (e) {
      error = message(e)
    }
    await loadHistory()
  }

  async function clearRuns() {
    selectedRun = null
    try {
      // Scope follows what's on screen: the all-tasks view clears everything.
      await request('history.clear', allTasks ? {} : { task: selectedTask })
    } catch (e) {
      error = message(e)
    }
    await refresh()
  }

  async function start() {
    if (!selectedTask) return
    error = ''
    selectedRun = null // a new run takes the pane back to live
    view = { ...emptyView(), status: 'running' }
    try {
      runUsageDisplay = await loadUsageDisplay().catch(() => 'tip' as const)
      const r = await request('run.start', {
        task: selectedTask,
        prompt: userPrompt,
        use_context: useCtx && !!ctx,
        usage_display: runUsageDisplay,
      })
      view = { ...view, runId: r.run_id }
    } catch (e) {
      error = message(e)
      view = emptyView()
    }
  }

  async function stop() {
    if (view.runId) {
      try {
        await request('run.cancel', { run_id: view.runId })
      } catch (e) {
        error = message(e)
      }
    }
  }

  // The backend speaks its own English strings (plugin.rs); classify the ones
  // that genuinely reach the user into a localized sentence. Anything the
  // classifier doesn't recognize falls through to the raw message — never a
  // wrong translation.
  const message = (e: unknown) => {
    const raw = e instanceof Error ? e.message : String(e)
    const key = errorKey(raw)
    return key ? tr(key) : raw
  }
  const fileName = (p: string) => p.split('/').pop() ?? p

  // Switching task, or switching scope, reloads the history list.
  $effect(() => {
    void selectedTask
    void allTasks
    void loadHistory()
  })

  void load()
</script>

<main>
  <aside>
    <!-- One scrollport owns all variable-height sidebar content. Tasks can no
         longer squeeze the history scrollport to zero height. -->
    <div class="sidebar-scroll">
      <HarnessBanner status={harness} label={tr} />

      <h2>{tr('tasks.title')}</h2>
      {#if tasks.length === 0}
        <p class="empty">{tr('tasks.empty')}</p>
      {/if}
      <TaskList
        {tasks}
        selected={selectedTask}
        onselect={(id) => {
          selectedTask = id
          selectedRun = null
          settingsOpen = false
        }}
        label={tr}
      />

      <h2>
        {tr('history.title')}
        <button
          class="scope"
          onclick={() => (allTasks = !allTasks)}
          title={allTasks ? tr('history.thisTask') : tr('history.all')}
        >
          {allTasks ? tr('history.all') : tr('history.thisTask')}
        </button>
      </h2>
      <div class="runs">
        <HistoryList
          runs={history}
          label={tr}
          showTask={allTasks}
          empty={allTasks ? tr('history.emptyAll') : tr('history.empty')}
          selectedId={selectedRun?.run_id ?? null}
          onselect={selectRun}
          ondelete={deleteRun}
          onclear={clearRuns}
        />
      </div>
    </div>
    <button
      type="button"
      class="settings-entry"
      class:active={settingsOpen}
      aria-pressed={settingsOpen}
      onclick={() => (settingsOpen = !settingsOpen)}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M9 3v2.2L6.8 6.5 5 5.4 3 9l2 1.1v3.8L3 15l2 3.6 1.8-1.1L9 18.8V21h6v-2.2l2.2-1.3 1.8 1.1 2-3.6-2-1.1v-3.8L21 9l-2-3.6-1.8 1.1L15 5.2V3z" />
        <circle cx="12" cy="12" r="3" />
      </svg>
      <span>{tr('settings.title')}</span>
    </button>
  </aside>

  <section>
    {#if settingsOpen}
      <SettingsPage label={tr} />
    {:else if selectedRun}
      <RunLog run={selectedRun} log={selectedLog} label={tr} />
    {:else}
      <header>
        {#if current}
          <p class="will-run">
            <span class="lead">{tr('run.willRun')}</span>
            <span class="name">{current.name}</span>
            {#if current.description}<span class="desc">{current.description}</span>{/if}
          </p>
        {/if}
        <label class="addendum" for="addendum">{tr('run.addendum')}</label>
        <textarea
          id="addendum"
          bind:value={userPrompt}
          placeholder={tr('run.addendum.placeholder')}
        ></textarea>
        {#if ctx}
          <label class="ctx">
            <input type="checkbox" bind:checked={useCtx} />
            {tr('ctx.label')}: {fileName(ctx.path)}
            {#if ctx.selection}
              ({tr('ctx.selection', { n: ctx.selection.length })})
            {/if}
          </label>
        {/if}
      </header>

      <RunStream items={view.items} />

      <ArtifactLinks paths={view.artifacts} label={tr} />

      {#if view.status !== 'idle' && view.status !== 'running' && runUsageDisplay === 'result'}
        <UsageSummary usage={view.usage} label={tr} />
      {/if}

      <footer>
        {#if error}
          <span class="err">{error}</span>
        {:else if view.status !== 'idle'}
          <span class="st">{tr(('status.' + view.status) as MessageKey)}</span>
        {/if}
        {#if view.turns != null}
          <span class="turns">{tr('turns', { n: view.turns })}</span>
        {/if}
        {#if running}
          <button onclick={stop}>{tr('run.stop')}</button>
        {:else}
          <button class="primary" onclick={start} disabled={!selectedTask || harness?.ok === false || !!current?.policy_error}>{tr('run.start')}</button>
          <!-- Display only, deliberately: this window IS one agent, so a picker
               here could only offer to run something somewhere else. The same
               "by X" phrasing as the pickers elsewhere, without the menu. -->
          {#if harness?.ok}
            <span class="by" title={harness.origin ?? ''}>
              {tr('agentPicker.by', { name: (harness.harness ?? '').replace(/\s*(Code|Harness)$/i, '') })}
              {#if harness.version}<span class="byver">{harness.version}</span>{/if}
            </span>
          {/if}
        {/if}
      </footer>
    {/if}
  </section>
</main>

<style>
  :global(:root) { color-scheme: light dark; }
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Helvetica Neue', sans-serif;
    background: Canvas;
    color: CanvasText;
  }
  main {
    --window-background: color-mix(in srgb, CanvasText 3%, Canvas);
    --window-surface: Canvas;
    --card-surface: color-mix(in srgb, CanvasText 2%, Canvas);
    --hover-surface: color-mix(in srgb, CanvasText 5%, Canvas);
    --window-border: color-mix(in srgb, CanvasText 11%, transparent);
    --strong-border: color-mix(in srgb, CanvasText 18%, transparent);
    --muted-text: color-mix(in srgb, CanvasText 58%, transparent);
    --standard-accent: #3479db;
    display: flex;
    height: 100vh;
    height: 100dvh;
    min-height: 0;
    overflow: hidden;
    background: var(--window-background);
  }
  @supports (color: AccentColor) { main { --standard-accent: AccentColor; } }
  aside {
    width: 264px;
    flex: none;
    padding: 12px 10px 10px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    box-sizing: border-box;
    border-right: 1px solid var(--window-border);
    background: var(--window-surface);
  }
  .sidebar-scroll {
    flex: 1;
    min-height: 0;
    overflow-x: hidden;
    overflow-y: auto;
    padding: 2px 4px 14px 2px;
    scrollbar-gutter: stable;
  }
  .runs { min-width: 0; }
  .settings-entry {
    display: flex;
    align-items: center;
    gap: 7px;
    flex: none;
    width: 100%;
    margin-top: 8px;
    padding: 8px 9px;
    border: 1px solid transparent;
    border-radius: 9px;
    background: transparent;
    color: var(--muted-text);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .settings-entry:hover { background: var(--hover-surface); color: CanvasText; }
  .settings-entry.active {
    border-color: var(--window-border);
    background: var(--card-surface);
    color: CanvasText;
  }
  .settings-entry svg { width: 15px; height: 15px; }
  h2 {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10px;
    letter-spacing: 0.075em;
    text-transform: uppercase;
    color: color-mix(in srgb, CanvasText 46%, transparent);
    margin: 16px 4px 7px;
    flex: none;
  }
  .scope {
    margin-left: auto;
    font: inherit;
    font-size: 9px;
    letter-spacing: 0;
    text-transform: none;
    padding: 3px 7px;
    border-radius: 999px;
    border: 1px solid var(--window-border);
    background: var(--card-surface);
    color: var(--muted-text);
    cursor: pointer;
  }
  .scope:hover { border-color: var(--strong-border); color: CanvasText; }
  .empty { margin: 8px 5px; font-size: 11px; color: var(--muted-text); }
  section {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--window-background);
  }
  header {
    flex: none;
    margin: 14px 16px 0;
    padding: 14px;
    border: 1px solid var(--window-border);
    border-radius: 14px;
    background: var(--window-surface);
    box-shadow: 0 2px 6px color-mix(in srgb, CanvasText 4%, transparent);
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    min-height: 58px;
    max-height: min(180px, 34vh);
    resize: vertical;
    font: inherit;
    font-size: 13px;
    line-height: 1.45;
    padding: 8px 10px;
    border-radius: 9px;
    border: 1px solid var(--window-border);
    background: var(--window-background);
    color: inherit;
  }
  textarea:hover { border-color: var(--strong-border); }
  textarea:focus { outline: 2px solid color-mix(in srgb, var(--standard-accent) 30%, transparent); border-color: var(--standard-accent); }
  .ctx { display: block; margin-top: 8px; font-size: 11px; color: var(--muted-text); }
  .will-run { margin: 0 0 8px; font-size: 12px; display: flex; gap: 6px; align-items: baseline; min-width: 0; }
  .will-run .lead { color: var(--muted-text); flex: none; }
  .will-run .name { font-weight: 600; flex: none; }
  .will-run .desc {
    color: var(--muted-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .addendum { display: block; margin-bottom: 5px; font-size: 11px; color: var(--muted-text); }
  footer {
    display: flex;
    align-items: center;
    gap: 10px;
    flex: none;
    min-height: 50px;
    padding: 8px 16px;
    border-top: 1px solid var(--window-border);
    background: color-mix(in srgb, var(--window-surface) 88%, transparent);
  }
  footer button {
    margin-left: auto;
    font: inherit;
    font-size: 13px;
    padding: 7px 17px;
    border-radius: 9px;
    border: 1px solid var(--strong-border);
    background: var(--window-surface);
    color: inherit;
    cursor: pointer;
  }
  footer button:hover { background: var(--hover-surface); }
  footer button.primary {
    border-color: var(--standard-accent);
    background: var(--standard-accent);
    color: white;
    font-weight: 600;
  }
  footer button.primary:hover { filter: brightness(1.06); }
  footer button:disabled { opacity: 0.45; cursor: default; }
  .err { color: #d9534f; font-size: 11px; }
  .st, .turns { font-size: 11px; color: var(--muted-text); }
  :is(button, textarea):focus-visible { outline: 2px solid var(--standard-accent); outline-offset: 2px; }
  @media (max-width: 720px) {
    aside { width: 224px; }
    header { margin-inline: 12px; }
  }
  @media (max-height: 520px) {
    header { margin-top: 10px; padding: 10px 12px; }
    textarea { min-height: 44px; }
    footer { min-height: 44px; }
  }
</style>
