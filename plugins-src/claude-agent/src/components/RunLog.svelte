<script lang="ts">
  import type { RunRecord } from '../lib/events'
  import ArtifactLinks from './ArtifactLinks.svelte'
  import UsageSummary from './UsageSummary.svelte'
  import { fmtFull } from '../lib/datetime'
  import type { MessageKey } from '../lib/strings'

  let { run, log, label }:
    {
      run: RunRecord
      log: string
      label: (k: MessageKey, v?: Record<string, string | number>) => string
    } = $props()

  const lines = $derived(log.split('\n').filter((l) => l.length > 0))
</script>

<div class="pane">
  <header>
    <span class="status s-{run.status}">{label(('status.' + run.status) as MessageKey)}</span>
    <span class="task">{run.task}</span>
    <span class="when">{fmtFull(run.started_at)}</span>
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
  <UsageSummary usage={run.usage} {label} />
</div>

<style>
  .pane { flex: 1; display: flex; flex-direction: column; min-height: 0; }
  header {
    display: flex;
    gap: 8px;
    align-items: baseline;
    margin: 14px 16px 0;
    padding: 11px 13px;
    font-size: 11px;
    border: 1px solid var(--window-border, color-mix(in srgb, currentColor 11%, transparent));
    border-radius: 12px;
    background: var(--window-surface, Canvas);
  }
  .status { font-weight: 600; }
  .s-error, .s-timeout { color: #d9534f; }
  .s-skipped { opacity: 0.65; }
  .task { color: var(--muted-text, currentColor); }
  .when { color: var(--muted-text, currentColor); margin-left: auto; font-variant-numeric: tabular-nums; }
  .body { flex: 1; min-height: 0; overflow: auto; padding: 18px 20px 24px; }
  .result { margin: 0 0 10px; font-size: 13px; line-height: 1.55; white-space: pre-wrap; }
  .stderr {
    margin: 0 0 10px;
    padding: 9px 10px;
    border: 1px solid color-mix(in srgb, #d9534f 24%, transparent);
    border-radius: 9px;
    background: color-mix(in srgb, #d9534f 8%, Canvas);
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
