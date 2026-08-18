<script lang="ts">
  import { t } from '../../lib/i18n/store.svelte'
  import {
    activeProvider,
    agentProviders,
    agentRun,
    agentPluginAvailable,
    dismissRun,
    harnessStatuses,
    isAgentBusy,
    refreshHarnesses,
    setProvider,
    startNoteRun,
  } from '../../lib/agent-workspace/store.svelte'
  import { pluginRuntime } from '../../lib/plugins/runtime.svelte'
  import { pluginName } from '../../lib/plugins/plugin-i18n'

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

  const providers = $derived(agentProviders())
  const current = $derived(activeProvider())
  const status = $derived(harnessStatuses[current])

  /** "Claude Code 2.1.226 · model claude-opus-5" — what you are about to spend
      tokens on, before you spend them. */
  function harnessLabel(id: string): string {
    const s = harnessStatuses[id]
    const plugin = pluginRuntime.manifests.find((m) => m.id === id)
    const name = s?.harness ?? (plugin ? pluginName(plugin) : id)
    if (!s) return name
    if (!s.ok) return `${name} — ${t('agent.harnessUnknown')}`
    const bits = [name, s.version, s.default_model && t('agent.model', { model: s.default_model })]
    return bits.filter(Boolean).join(' · ')
  }

  // Ask each harness what it is. Once when the panel appears, and again after a
  // run ends — a run is exactly when an expired credential becomes visible.
  $effect(() => {
    void providers.length
    void refreshHarnesses()
  })
  $effect(() => {
    if (agentRun.phase === 'done' || agentRun.phase === 'error') void refreshHarnesses()
  })

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
  <section class="agents" aria-label={t('agent.title')}>
    <!-- A heading of its own, because this becomes a list: one row per task
         the enabled agent plugins offer for the current note. -->
    <h3>{t('agent.title')}</h3>

    <!-- WHICH agent, and is it in a state to run. Both plugins share one task
         directory, so without this the same task reads identically under either
         harness — which is how an expired Claude credential got read as a
         DeepSeek failure. -->
    <div class="harness">
      {#if providers.length > 1}
        <select
          class="pick"
          value={current}
          disabled={busy}
          title={status?.origin ?? ''}
          onchange={(e) => setProvider((e.currentTarget as HTMLSelectElement).value)}
        >
          {#each providers as id (id)}
            <option value={id}>{harnessLabel(id)}</option>
          {/each}
        </select>
      {:else}
        <span class="one" title={status?.origin ?? ''}>{harnessLabel(current)}</span>
      {/if}
    </div>

    {#if status && !status.ok}
      <p class="alert" title={status.hint ?? ''}>
        {t('agent.harnessMissing', { harness: status.harness })}
      </p>
    {:else if status?.warning}
      <!-- An environment failure repeats no matter what you ask it to do, so it
           is worth interrupting for: re-authenticating is the fix, not retrying. -->
      <p class="alert" title={status.warning}>
        {t('agent.harnessWarning', { detail: status.warning })}
      </p>
    {/if}

    <div class="row">
      <span class="line" class:bad={agentRun.phase === 'error'} title={tip}>
        {#if busy && mine}<span class="spinner" aria-hidden="true"></span>{/if}
        {line}
      </span>

      {#if busy}
        <span class="elapsed">{t('agent.elapsed', { s: elapsed })}</span>
      {:else}
        <button
          class="run"
          disabled={!notePath || status?.ok === false}
          title={notePath ?? t('agent.noNote')}
          onclick={() => notePath && void startNoteRun(notePath, onfinished)}
        >
          {t('agent.answerQuestions')}
        </button>
      {/if}
    </div>
  </section>
{/if}

<style>
  .agents {
    flex: none;
    padding: 5px 12px 7px;
    border-top: 1px solid var(--border-color, #3333);
  }
  h3 {
    margin: 0 0 3px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    opacity: 0.45;
  }
  .harness { display: flex; margin: 0 0 4px; }
  /* select inherits neither font-size nor family — declare both, or the row
     drifts at larger UI font sizes. */
  .pick {
    font: inherit;
    font-size: 11px;
    max-width: 100%;
    padding: 1px 4px;
    border-radius: 5px;
    border: 1px solid color-mix(in srgb, currentColor 20%, transparent);
    background: transparent;
    color: inherit;
    opacity: 0.75;
    cursor: pointer;
  }
  .pick:disabled { cursor: default; opacity: 0.45; }
  .one {
    font-size: 11px;
    opacity: 0.55;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: default;
  }
  .alert {
    margin: 0 0 4px;
    font-size: 11px;
    line-height: 1.4;
    color: #b8860b;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: default;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    min-height: 22px;
  }
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
