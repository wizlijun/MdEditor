<script lang="ts">
  import type { RunRecord } from '../lib/events'
  import ArtifactLinks from './ArtifactLinks.svelte'

  let { runs, label, empty, showTask = false }:
    {
      runs: RunRecord[]
      label: (k: string, v?: Record<string, string | number>) => string
      empty: string
      /** In the all-tasks view each row needs to say WHICH task it was. */
      showTask?: boolean
    } = $props()

  // "2026-07-30T10:42:33+00:00" → "07-30 10:42"
  const when = (iso: string) => iso.slice(5, 16).replace('T', ' ')
</script>

{#if runs.length === 0}
  <p class="empty">{empty}</p>
{:else}
  <ul class="history">
    {#each runs as run (run.run_id)}
      <li>
        <div class="row" title={run.result || run.stderr_tail}>
          <span class="status s-{run.status}">{label('status.' + run.status)}</span>
          {#if showTask}<span class="task">{run.task}</span>{/if}
          <span class="when">{when(run.started_at)}</span>
          {#if run.trigger === 'cli'}<span class="cli">CLI</span>{/if}
        </div>
        <!-- A past run's markdown stays one click away, including runs the CLI
             started while this window wasn't even open. -->
        <ArtifactLinks paths={run.artifacts ?? []} {label} oncompact />
      </li>
    {/each}
  </ul>
{/if}

<style>
  .history { list-style: none; margin: 0; padding: 0; font-size: 11px; }
  li { padding: 3px 0; }
  .row { display: flex; gap: 6px; align-items: baseline; }
  .status { font-weight: 600; flex: none; }
  .s-error, .s-timeout { color: #d9534f; }
  .task {
    opacity: 0.75;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .when { opacity: 0.55; margin-left: auto; flex: none; font-variant-numeric: tabular-nums; }
  .cli {
    font-size: 9px;
    opacity: 0.5;
    flex: none;
    border: 1px solid currentColor;
    border-radius: 3px;
    padding: 0 3px;
  }
  .empty { font-size: 11px; opacity: 0.55; margin: 4px 0; }
</style>
