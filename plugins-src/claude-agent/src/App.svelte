<script lang="ts">
  import { bridge, request, onMessage } from './lib/bridge'
  import { t } from './lib/strings'
  import {
    emptyView,
    reduce,
    type RunView,
    type HostMessage,
    type Task,
    type RunRecord,
  } from './lib/events'
  import TaskList from './components/TaskList.svelte'
  import RunStream from './components/RunStream.svelte'
  import HistoryList from './components/HistoryList.svelte'
  import ArtifactLinks from './components/ArtifactLinks.svelte'

  const locale = bridge().locale
  const tr = (k: string, v?: Record<string, string | number>) => t(locale, k, v)

  let tasks: Task[] = $state([])
  let selected: string | null = $state(null)
  let userPrompt = $state('')
  let ctx: { path: string; selection: string } | null = $state(null)
  let useCtx = $state(true)
  let view: RunView = $state(emptyView())
  let history: RunRecord[] = $state([])
  let allTasks = $state(true)
  let error = $state('')

  const running = $derived(view.status === 'running')

  onMessage((m: HostMessage) => {
    view = reduce(view, m)
    // A finished run belongs in the list — and in the task's status — at once.
    if (m.kind === 'done' || m.kind === 'busy') void refresh()
  })

  // The backend resolves the vault root asynchronously (asking the host for it
  // from inside activate would deadlock its protocol loop), so the first
  // tasks.list can legitimately answer "not ready yet". Retry briefly instead
  // of showing an empty list.
  async function load() {
    try {
      for (let i = 0; i < 20 && !(await loadTasks()); i++) {
        await new Promise((r) => setTimeout(r, 250))
      }
      if (!selected && tasks.length) selected = tasks[0].id
      const c = await request('context.get')
      ctx = c.tab?.path ? { path: c.tab.path, selection: c.tab.selection ?? '' } : null
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
      const task = allTasks ? null : selected
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

  async function start() {
    if (!selected) return
    error = ''
    view = { ...emptyView(), status: 'running' }
    try {
      const r = await request('run.start', {
        task: selected,
        prompt: userPrompt,
        use_context: useCtx && !!ctx,
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

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e))
  const fileName = (p: string) => p.split('/').pop() ?? p

  // Switching task, or switching scope, reloads the history list.
  $effect(() => {
    void selected
    void allTasks
    void loadHistory()
  })

  void load()
</script>

<main>
  <aside>
    <h2>{tr('tasks.title')}</h2>
    {#if tasks.length === 0}
      <p class="empty">{tr('tasks.empty')}</p>
    {/if}
    <TaskList {tasks} {selected} onselect={(id) => (selected = id)} label={tr} />

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
    <HistoryList
      runs={history}
      label={tr}
      showTask={allTasks}
      empty={allTasks ? tr('history.emptyAll') : tr('history.empty')}
    />
  </aside>

  <section>
    <header>
      <textarea bind:value={userPrompt} placeholder={tr('run.prompt.placeholder')}></textarea>
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

    <footer>
      {#if error}
        <span class="err">{error}</span>
      {:else if view.status !== 'idle'}
        <span class="st">{tr('status.' + view.status)}</span>
      {/if}
      {#if view.turns != null}
        <span class="turns">{tr('turns', { n: view.turns })}</span>
      {/if}
      {#if running}
        <button onclick={stop}>{tr('run.stop')}</button>
      {:else}
        <button class="primary" onclick={start} disabled={!selected}>{tr('run.start')}</button>
      {/if}
    </footer>
  </section>
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, system-ui, sans-serif;
  }
  main { display: flex; height: 100vh; }
  aside {
    width: 236px;
    flex: none;
    padding: 12px;
    overflow: auto;
    box-sizing: border-box;
    border-right: 1px solid color-mix(in srgb, currentColor 15%, transparent);
  }
  h2 {
    display: flex;
    align-items: baseline;
    gap: 6px;
    font-size: 10px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    opacity: 0.5;
    margin: 14px 0 6px;
  }
  h2:first-child { margin-top: 0; }
  /* A button inherits no font — say so explicitly. */
  .scope {
    margin-left: auto;
    font: inherit;
    font-size: 9px;
    letter-spacing: 0;
    text-transform: none;
    padding: 1px 5px;
    border-radius: 4px;
    border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  .scope:hover { background: color-mix(in srgb, currentColor 10%, transparent); }
  .empty { font-size: 11px; opacity: 0.55; }
  section { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  header {
    padding: 10px 12px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    min-height: 52px;
    resize: vertical;
    font: inherit;
    font-size: 13px;
    padding: 6px 8px;
    border-radius: 6px;
    border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
    background: transparent;
    color: inherit;
  }
  .ctx { display: block; margin-top: 6px; font-size: 11px; opacity: 0.8; }
  footer {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-top: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }
  footer button {
    margin-left: auto;
    font: inherit;
    font-size: 13px;
    padding: 5px 16px;
    border-radius: 6px;
    border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  footer button.primary {
    background: color-mix(in srgb, currentColor 12%, transparent);
    font-weight: 600;
  }
  footer button:disabled { opacity: 0.45; cursor: default; }
  .err { color: #d9534f; font-size: 11px; }
  .st, .turns { font-size: 11px; opacity: 0.7; }
</style>
