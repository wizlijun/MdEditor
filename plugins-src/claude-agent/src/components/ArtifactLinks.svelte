<script lang="ts">
  import { openInEditor } from '../lib/bridge'

  let { paths, label, oncompact = false }:
    {
      paths: string[]
      label: (k: string, v?: Record<string, string | number>) => string
      /** History rows are cramped: show a count, not the whole list. */
      oncompact?: boolean
    } = $props()

  let failed = $state('')

  const name = (p: string) => p.split('/').pop() ?? p

  async function open(path: string) {
    failed = ''
    try {
      await openInEditor(path)
    } catch (e) {
      // Most likely the file was moved or deleted after the run.
      failed = e instanceof Error ? e.message : String(e)
    }
  }
</script>

{#if paths.length > 0}
  <div class="artifacts" class:compact={oncompact}>
    <span class="lead">{label('artifacts.label')}</span>
    {#each paths as path (path)}
      <button class="link" onclick={() => open(path)} title={path}>{name(path)}</button>
    {/each}
    {#if failed}<span class="err">{failed}</span>{/if}
  </div>
{/if}

<style>
  .artifacts {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px;
    padding: 6px 12px;
    font-size: 12px;
    border-top: 1px solid color-mix(in srgb, currentColor 12%, transparent);
  }
  .artifacts.compact { padding: 2px 0; border-top: 0; font-size: 11px; }
  .lead { opacity: 0.55; }
  /* A button inherits no font — say so, or these drift out of line. */
  .link {
    font: inherit;
    font-size: inherit;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
    text-decoration: underline;
    text-underline-offset: 2px;
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .link:hover { opacity: 0.7; }
  .err { color: #d9534f; }
</style>
