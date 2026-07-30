<script lang="ts">
  interface Rec {
    run_id: string
    status: string
    trigger: string
    started_at: string
    result: string
  }
  let { runs, label, empty }:
    { runs: Rec[]; label: (k: string) => string; empty: string } = $props()

  // "2026-07-30T10:42:33+00:00" → "07-30 10:42"
  const when = (iso: string) => iso.slice(5, 16).replace('T', ' ')
</script>

{#if runs.length === 0}
  <p class="empty">{empty}</p>
{:else}
  <ul class="history">
    {#each runs as run (run.run_id)}
      <li title={run.result}>
        <span class="status s-{run.status}">{label('status.' + run.status)}</span>
        <span class="when">{when(run.started_at)}</span>
        {#if run.trigger === 'cli'}<span class="cli">CLI</span>{/if}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .history { list-style: none; margin: 0; padding: 0; font-size: 11px; }
  li { display: flex; gap: 6px; padding: 3px 0; align-items: baseline; }
  .status { font-weight: 600; }
  .s-error, .s-timeout { color: #d9534f; }
  .when { opacity: 0.55; font-variant-numeric: tabular-nums; }
  .cli {
    font-size: 9px;
    opacity: 0.5;
    border: 1px solid currentColor;
    border-radius: 3px;
    padding: 0 3px;
  }
  .empty { font-size: 11px; opacity: 0.55; margin: 4px 0; }
</style>
