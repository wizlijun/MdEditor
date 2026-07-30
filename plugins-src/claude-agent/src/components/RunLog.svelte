<script lang="ts">
  import type { RunRecord } from '../lib/events'
  import ArtifactLinks from './ArtifactLinks.svelte'

  let { run, log, label }:
    {
      run: RunRecord
      log: string
      label: (k: string, v?: Record<string, string | number>) => string
    } = $props()

  const lines = $derived(log.split('\n').filter((l) => l.length > 0))
</script>

<div class="pane">
  <header>
    <span class="status s-{run.status}">{label('status.' + run.status)}</span>
    <span class="task">{run.task}</span>
    <span class="when">{run.started_at.slice(0, 19).replace('T', ' ')}</span>
  </header>

  <div class="body">
    {#if run.result}
      <p class="result">{run.result}</p>
    {/if}
    {#if run.stderr_tail}
      <pre class="stderr">{run.stderr_tail}</pre>
    {/if}
    {#if lines.length > 0}
      <ol class="log">
        {#each lines as line, i (i)}
          <li>{line}</li>
        {/each}
      </ol>
    {:else}
      <p class="empty">{label('log.empty')}</p>
    {/if}
  </div>

  <ArtifactLinks paths={run.artifacts ?? []} {label} />
</div>

<style>
  .pane { flex: 1; display: flex; flex-direction: column; min-height: 0; }
  header {
    display: flex;
    gap: 8px;
    align-items: baseline;
    padding: 8px 12px;
    font-size: 11px;
    border-bottom: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }
  .status { font-weight: 600; }
  .s-error, .s-timeout { color: #d9534f; }
  .s-skipped { opacity: 0.65; }
  .task { opacity: 0.8; }
  .when { opacity: 0.5; margin-left: auto; font-variant-numeric: tabular-nums; }
  .body { flex: 1; overflow: auto; padding: 10px 12px; }
  .result { margin: 0 0 10px; font-size: 13px; line-height: 1.55; white-space: pre-wrap; }
  .stderr {
    margin: 0 0 10px;
    padding: 6px 8px;
    border-radius: 5px;
    background: color-mix(in srgb, #d9534f 12%, transparent);
    font-size: 11px;
    white-space: pre-wrap;
    overflow-x: auto;
  }
  .log {
    margin: 0;
    padding: 0;
    list-style: none;
    font-family: ui-monospace, SFMono-Regular, monospace;
    font-size: 11px;
    line-height: 1.6;
  }
  .log li {
    padding: 1px 0;
    white-space: pre-wrap;
    word-break: break-word;
    opacity: 0.85;
  }
  .empty { font-size: 12px; opacity: 0.5; }
</style>
