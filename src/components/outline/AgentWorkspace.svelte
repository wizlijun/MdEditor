<script lang="ts">
  import { t } from '../../lib/i18n/store.svelte'
  import {
    agentRun,
    agentPluginAvailable,
    dismissRun,
    isAgentBusy,
    startNoteRun,
  } from '../../lib/agent-workspace/store.svelte'

  let { notePath, onfinished }:
    {
      /** The sidecar note this workspace acts on; null when the tab has none. */
      notePath: string | null
      /** Called after a run reaches a terminal state, to refresh the views. */
      onfinished: () => void | Promise<void>
    } = $props()

  const available = $derived(agentPluginAvailable())
  const busy = $derived(isAgentBusy())
  const mine = $derived(agentRun.notePath === notePath)

  // Ticks once a second so the elapsed time moves while a run is in flight.
  let now = $state(Date.now())
  $effect(() => {
    if (!busy) return
    const id = setInterval(() => (now = Date.now()), 1000)
    return () => clearInterval(id)
  })
  const elapsed = $derived(
    agentRun.startedAt ? Math.max(0, Math.round((now - agentRun.startedAt) / 1000)) : 0,
  )

  const fileName = (p: string) => p.split('/').pop() ?? p
</script>

{#if available}
  <section class="agent" aria-label={t('agent.title')}>
    <header>
      <span class="title">{t('agent.title')}</span>
      {#if busy}
        <span class="spinner" aria-hidden="true"></span>
        <span class="phase">
          {agentRun.phase === 'starting' ? t('agent.starting') : t('agent.running')}
        </span>
        <span class="meta">{t('agent.elapsed', { s: elapsed })}</span>
      {/if}
    </header>

    {#if busy && mine}
      <p class="progress">
        <span class="steps">{t('agent.steps', { n: agentRun.steps })}</span>
        {#if agentRun.last}<span class="last" title={agentRun.last}>{agentRun.last}</span>{/if}
      </p>
      <p class="locked">{t('agent.locked')}</p>
    {:else if agentRun.phase === 'done' || agentRun.phase === 'error'}
      <p class="result" class:bad={agentRun.phase === 'error'}>
        <span class="outcome">
          {agentRun.phase === 'done' ? t('agent.doneSuccess') : t('agent.doneError')}
        </span>
        {#if agentRun.message}<span class="msg" title={agentRun.message}>{agentRun.message}</span>{/if}
      </p>
      <div class="actions">
        <button class="ghost" onclick={dismissRun}>{t('agent.dismiss')}</button>
      </div>
    {:else if notePath}
      <p class="hint">{t('agent.hint')}</p>
    {:else}
      <p class="hint">{t('agent.noNote')}</p>
    {/if}

    {#if !busy}
      <button
        class="run"
        disabled={!notePath}
        title={notePath ? fileName(notePath) : t('agent.noNote')}
        onclick={() => notePath && void startNoteRun(notePath, onfinished)}
      >
        {t('agent.answerQuestions')}
      </button>
    {/if}
  </section>
{/if}

<style>
  .agent {
    flex: none;
    border-top: 1px solid var(--border-color, #3333);
    padding: 8px 12px 10px;
    font-size: 12px;
  }
  header { display: flex; align-items: center; gap: 6px; }
  .title { font-weight: 600; opacity: 0.75; }
  .phase { opacity: 0.75; }
  .meta { margin-left: auto; opacity: 0.55; font-variant-numeric: tabular-nums; }
  .hint, .progress, .locked, .result { margin: 6px 0 0; }
  .hint { opacity: 0.55; line-height: 1.45; }
  .progress { display: flex; gap: 6px; align-items: baseline; min-width: 0; }
  .steps { opacity: 0.6; flex: none; font-variant-numeric: tabular-nums; }
  .last, .msg {
    opacity: 0.8;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .locked { opacity: 0.55; font-style: italic; }
  .result { display: flex; gap: 6px; align-items: baseline; min-width: 0; }
  .result .outcome { font-weight: 600; flex: none; }
  .result.bad .outcome { color: #d24b4b; }
  .actions { margin-top: 6px; }
  /* Buttons inherit neither font-size nor family — say both. */
  .run, .ghost {
    font: inherit;
    font-size: 12px;
    margin-top: 8px;
    padding: 5px 10px;
    border-radius: 6px;
    border: 1px solid color-mix(in srgb, currentColor 28%, transparent);
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  .run { width: 100%; font-weight: 600; }
  .run:hover:not(:disabled), .ghost:hover { background: color-mix(in srgb, currentColor 10%, transparent); }
  .run:disabled { opacity: 0.4; cursor: default; }
  .ghost { margin-top: 0; padding: 3px 8px; }
  .spinner {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    border: 1.5px solid color-mix(in srgb, currentColor 30%, transparent);
    border-top-color: currentColor;
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin { to { transform: rotate(360deg) } }
  @media (prefers-reduced-motion: reduce) {
    .spinner { animation: none; }
  }
</style>
