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

  // A finished result belongs to the note it was about; moving to another note
  // clears it rather than leaving a stale verdict under the wrong document.
  $effect(() => {
    void notePath
    if (!isAgentBusy() && agentRun.notePath && agentRun.notePath !== notePath) dismissRun()
  })

  const outcomeLabel = $derived(
    agentRun.phase === 'error'
      ? t('agent.doneError')
      : agentRun.outcome === 'skipped'
        ? t('agent.doneSkipped')
        : t('agent.doneSuccess'),
  )

  /** The single line shown; the whole story goes in its tooltip. */
  const line = $derived.by(() => {
    if (busy && mine) {
      return agentRun.last || t('agent.running')
    }
    if (agentRun.phase === 'done' || agentRun.phase === 'error') {
      return agentRun.message ? `${outcomeLabel} · ${agentRun.message}` : outcomeLabel
    }
    return notePath ? t('agent.hint') : t('agent.noNote')
  })

  const tip = $derived.by(() => {
    if (busy && mine) {
      return [t('agent.running'), t('agent.steps', { n: agentRun.steps }), agentRun.last, t('agent.locked')]
        .filter(Boolean)
        .join('\n')
    }
    if (agentRun.phase === 'done' || agentRun.phase === 'error') {
      return [outcomeLabel, agentRun.message, ...agentRun.artifacts].filter(Boolean).join('\n')
    }
    return [t('agent.hint'), notePath ?? t('agent.noNote')].join('\n')
  })
</script>

{#if available}
  <section class="agent" aria-label={t('agent.title')}>
    <span class="title">{t('agent.title')}</span>

    <span class="line" class:bad={agentRun.phase === 'error'} title={tip}>
      {#if busy && mine}<span class="spinner" aria-hidden="true"></span>{/if}
      {line}
    </span>

    {#if busy}
      <span class="elapsed">{t('agent.elapsed', { s: elapsed })}</span>
    {:else}
      <button
        class="run"
        disabled={!notePath}
        title={notePath ?? t('agent.noNote')}
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
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    font-size: 12px;
    border-top: 1px solid var(--border-color, #3333);
  }
  .title { flex: none; font-weight: 600; opacity: 0.7; }
  .line {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 5px;
    opacity: 0.6;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: default;
  }
  .line.bad { color: #d24b4b; opacity: 0.9; }
  .elapsed { flex: none; opacity: 0.55; font-variant-numeric: tabular-nums; }
  /* A button inherits neither font-size nor family — declare both. */
  .run {
    flex: none;
    font: inherit;
    font-size: 12px;
    padding: 3px 12px;
    border-radius: 6px;
    border: 1px solid color-mix(in srgb, currentColor 28%, transparent);
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  .run:hover:not(:disabled) { background: color-mix(in srgb, currentColor 10%, transparent); }
  .run:disabled { opacity: 0.4; cursor: default; }
  .spinner {
    flex: none;
    width: 8px;
    height: 8px;
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
